use fbfast_core::{
    config::{Config, EnvFile, Inputs},
    pool::{Output, Pending, Pool},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    env,
    path::Path,
    thread,
    time::{Duration, Instant},
};

fn config() -> Config {
    let values: Inputs = [
        ("host".into(), json!("127.0.0.1")),
        (
            "port".into(),
            json!(env::var("FBFAST_TEST_PORT")
                .unwrap()
                .parse::<u16>()
                .unwrap()),
        ),
        (
            "database".into(),
            json!("/var/lib/firebird/data/fbfast.fdb"),
        ),
        ("user".into(), json!("FBFAST_READER")),
        (
            "password".into(),
            json!(env::var("FBFAST_TEST_PASSWORD").unwrap()),
        ),
        ("role".into(), json!("FBFAST_READONLY")),
        ("pool_size".into(), json!(2)),
        ("min_idle".into(), json!(1)),
    ]
    .into();
    Config::resolve_sources(
        values,
        "FB_",
        EnvFile::Disabled,
        &BTreeMap::new(),
        Path::new("."),
    )
    .unwrap()
}
fn pool(config: Config) -> Pool {
    Pool::new(config, env::var("FBFAST_TEST_CLIENT").unwrap().into())
}
fn wait(request: &mut Pending) -> Result<Output, fbfast_core::error::Error> {
    let start = Instant::now();
    loop {
        if let Some(output) = request.wait_slice(Duration::from_millis(10))? {
            return Ok(output);
        }
        assert!(start.elapsed() < Duration::from_secs(10));
    }
}
fn warmup(pool: &Pool) {
    for mut pending in pool.warmup().unwrap() {
        wait(&mut pending).unwrap();
    }
}
fn until(mut predicate: impl FnMut() -> bool) {
    let start = Instant::now();
    while !predicate() {
        assert!(
            start.elapsed() < Duration::from_secs(4),
            "maintenance state did not converge"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn lifetime_recycling_restores_idle_floor_without_a_new_request() {
    let mut options = config();
    options.max_lifetime = 0.15;
    options.idle_timeout = 0.02;
    let pool = pool(options);
    warmup(&pool);
    until(|| {
        let stats = pool.stats().unwrap();
        assert!(stats.opening + stats.idle + stats.leased + stats.cleaning + stats.closing <= 2);
        stats.connections_created >= 3 && stats.idle >= 1
    });
    assert_eq!(pool.stats().unwrap().maintenance_failures, 0);
    pool.close(Duration::from_secs(3)).unwrap();
    assert_eq!(pool.stats().unwrap().control_threads, 0);
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn active_query_is_not_expired_and_spare_capacity_maintains_idle_floor() {
    let mut options = config();
    options.max_lifetime = 0.1;
    let pool = pool(options);
    warmup(&pool);
    let mut pending = pool.submit_rows(
        "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT SUM(CAST(A.I AS BIGINT)*B.I*C.I) FROM N A CROSS JOIN N B CROSS JOIN N C".into(),
        vec![], true, false, None).unwrap();
    until(|| {
        let stats = pool.stats().unwrap();
        stats.leased == 1 && stats.idle == 1
    });
    thread::sleep(Duration::from_millis(200));
    assert_eq!(pool.stats().unwrap().leased, 1);
    assert_eq!(pool.stats().unwrap().native_cancels, 0);
    pending.cancel(false);
    assert!(wait(&mut pending).is_err());
    until(|| pool.stats().unwrap().idle >= 1);
    pool.close(Duration::from_secs(3)).unwrap();
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn zero_idle_floor_does_not_reopen_retired_connections() {
    let mut options = config();
    options.min_idle = 0;
    options.idle_timeout = 0.02;
    let pool = pool(options);
    warmup(&pool); // still validates at least one attachment
    until(|| pool.stats().unwrap().closed == 2);
    thread::sleep(Duration::from_millis(300));
    assert_eq!(pool.stats().unwrap().connections_created, 1);
    assert_eq!(pool.stats().unwrap().idle, 0);
    pool.close(Duration::from_secs(3)).unwrap();
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn failed_background_open_uses_bounded_backoff_and_shutdown() {
    let mut options = config();
    options.user = "FBFAST_NONEXISTENT".into();
    let pool = pool(options);
    assert!(wait(&mut pool.warmup().unwrap().remove(0)).is_err());
    until(|| pool.stats().unwrap().maintenance_failures > 0);
    thread::sleep(Duration::from_millis(1000));
    let stats = pool.stats().unwrap();
    assert!(
        stats.maintenance_failures <= 5,
        "background retries must back off"
    );
    assert_eq!(stats.connections_created, 0);
    assert_eq!(stats.idle, 0);
    assert!(stats.opening <= 1);
    pool.close(Duration::from_secs(5)).unwrap();
}

#[test]
#[ignore = "requires bundled client; scripts/test_native.py runs this"]
fn background_retry_budget_starts_after_backoff() {
    use std::{
        net::TcpListener,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
    };
    fbfast_core::runtime::client_info(Path::new(&env::var("FBFAST_TEST_CLIENT").unwrap())).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut options = config();
    options.port = listener.local_addr().unwrap().port();
    options.acquire_timeout = 0.05;
    let stop = Arc::new(AtomicBool::new(false));
    let attempts = Arc::new(AtomicUsize::new(0));
    let (server_stop, server_attempts) = (stop.clone(), attempts.clone());
    let server = thread::spawn(move || {
        while !server_stop.load(Ordering::Acquire) {
            if let Ok((socket, _)) = listener.accept() {
                server_attempts.fetch_add(1, Ordering::Release);
                drop(socket);
            } else {
                thread::sleep(Duration::from_millis(1));
            }
        }
    });
    let pool = pool(options);
    assert!(wait(&mut pool.warmup().unwrap().remove(0)).is_err());
    until(|| pool.stats().unwrap().maintenance_failures > 0);
    let previous = attempts.load(Ordering::Acquire);
    // Backoff exceeds acquire_timeout; a fresh internal attempt must still reach TCP.
    until(|| attempts.load(Ordering::Acquire) > previous);
    pool.close(Duration::from_secs(5)).unwrap();
    stop.store(true, Ordering::Release);
    server.join().unwrap();
}

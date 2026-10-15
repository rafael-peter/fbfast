use fbfast_core::{
    config::{Config, EnvFile, Inputs},
    decode::Value,
    error::{Error, ErrorKind},
    pool::{Output, Pending, Pool},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    env,
    net::TcpListener,
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const LONG: &str = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT SUM(CAST(A.I AS BIGINT)*B.I*C.I) FROM N A CROSS JOIN N B CROSS JOIN N C";

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
        ("pool_size".into(), json!(1)),
        ("min_idle".into(), json!(1)),
        ("max_waiters".into(), json!(2)),
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
fn wait(pending: &mut Pending) -> Result<Output, Error> {
    let start = Instant::now();
    loop {
        if let Some(result) = pending.wait_slice(Duration::from_millis(10))? {
            return Ok(result);
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "test operation did not finish"
        );
    }
}
fn scalar(pool: &Pool, sql: &str) -> i128 {
    let result = wait(
        &mut pool
            .submit_rows(sql.into(), vec![], true, false, Some(3.0))
            .unwrap(),
    )
    .unwrap();
    let Output::Rows(rows) = result else {
        panic!("wrong output")
    };
    let Value::Int(value) = rows.row(0).unwrap()[0] else {
        panic!("wrong scalar")
    };
    value
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn real_cancel_preserves_capacity_fifo_and_discards_attachment() {
    let p = pool(config());
    let first_connection = scalar(&p, "SELECT CURRENT_CONNECTION FROM RDB$DATABASE");
    let mut active = p
        .submit_rows(LONG.into(), vec![], true, false, None)
        .unwrap();
    thread::sleep(Duration::from_millis(100));
    let mut first = p
        .submit_rows(
            "SELECT CURRENT_TRANSACTION FROM RDB$DATABASE".into(),
            vec![],
            true,
            false,
            None,
        )
        .unwrap();
    let mut second = p
        .submit_rows(
            "SELECT CURRENT_TRANSACTION FROM RDB$DATABASE".into(),
            vec![],
            true,
            false,
            None,
        )
        .unwrap();
    assert_eq!(p.stats().unwrap().waiting, 2);
    assert_eq!(p.stats().unwrap().connections_created, 1);
    assert!(matches!(
        p.submit_rows(
            "SELECT 1 FROM RDB$DATABASE".into(),
            vec![],
            true,
            false,
            None
        ),
        Err(Error {
            kind: ErrorKind::PoolBusy,
            ..
        })
    ));
    let start = Instant::now();
    active.cancel(false);
    assert_eq!(wait(&mut active).unwrap_err().kind, ErrorKind::Cancelled);
    assert!(start.elapsed() < Duration::from_secs(3));
    let Output::Rows(a) = wait(&mut first).unwrap() else {
        panic!("wrong result")
    };
    let Output::Rows(b) = wait(&mut second).unwrap() else {
        panic!("wrong result")
    };
    let (Value::Int(a), Value::Int(b)) = (&a.cells[0], &b.cells[0]) else {
        panic!("wrong type")
    };
    assert!(a < b, "queued operations must acquire FIFO");
    assert_ne!(
        scalar(&p, "SELECT CURRENT_CONNECTION FROM RDB$DATABASE"),
        first_connection
    );
    let stats = p.stats().unwrap();
    assert_eq!(stats.native_cancels, 1);
    assert_eq!(stats.discarded, 1);
    assert_eq!(stats.control_threads, 1);
    p.close(Duration::from_secs(5)).unwrap();
    assert_eq!(p.stats().unwrap().control_threads, 0);
    assert_eq!(p.stats().unwrap().closed, 1);
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn autonomous_deadline_works_without_polling_result() {
    let p = pool(config());
    scalar(&p, "SELECT 1 FROM RDB$DATABASE");
    let mut pending = p
        .submit_rows(LONG.into(), vec![], true, false, Some(0.1))
        .unwrap();
    // No waiter polling drives the timer: the dedicated controller must cancel it.
    thread::sleep(Duration::from_millis(400));
    assert_eq!(
        wait(&mut pending).unwrap_err().kind,
        ErrorKind::QueryTimeout
    );
    assert_eq!(p.stats().unwrap().timeouts, 1);
    assert!(p.stats().unwrap().native_cancels <= 1); // native timeout can win the race
    assert_eq!(scalar(&p, "SELECT 17 FROM RDB$DATABASE"), 17);
    p.close(Duration::from_secs(5)).unwrap();
}

#[test]
#[ignore = "requires bundled client; scripts/test_native.py runs this"]
fn unresponsive_attach_remains_counted_after_caller_timeout() {
    // This test targets a native attach already inside network I/O, not first
    // process-wide dylib loading consuming its intentionally short acquisition.
    fbfast_core::runtime::client_info(Path::new(&env::var("FBFAST_TEST_CLIENT").unwrap())).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut config = config();
    config.port = listener.local_addr().unwrap().port();
    config.acquire_timeout = 0.05;
    config.max_waiters = 0;
    let (entered, accepted) = mpsc::channel();
    let (release, finish) = mpsc::channel();
    let server = thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        entered.send(()).unwrap();
        finish.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(socket);
    });
    let p = pool(config);
    let mut pending = p
        .submit_rows(
            "SELECT 1 FROM RDB$DATABASE".into(),
            vec![],
            true,
            false,
            None,
        )
        .unwrap();
    accepted.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(wait(&mut pending).unwrap_err().kind, ErrorKind::PoolTimeout);
    assert_eq!(p.stats().unwrap().opening, 1);
    assert!(matches!(
        p.submit_rows(
            "SELECT 2 FROM RDB$DATABASE".into(),
            vec![],
            true,
            false,
            None
        ),
        Err(Error {
            kind: ErrorKind::PoolBusy,
            ..
        })
    ));
    assert_eq!(
        p.close(Duration::from_millis(10)).unwrap_err().kind,
        ErrorKind::PoolTimeout
    );
    assert_eq!(p.stats().unwrap().opening, 1);
    release.send(()).unwrap();
    server.join().unwrap();
    p.close(Duration::from_secs(3)).unwrap();
    assert_eq!(p.stats().unwrap().closed, 1);
}

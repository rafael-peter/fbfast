use super::{
    controller::Controller,
    state::{Progress, Work},
    Output, Pool,
};
use crate::{
    config::{Config, EnvFile, Inputs},
    error::ErrorKind,
    execution::{control::Cancellation, ExecutionConnection},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    env,
    path::Path,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

fn config() -> Config {
    let input: Inputs = [
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
    ]
    .into();
    Config::resolve_sources(
        input,
        "FB_",
        EnvFile::Disabled,
        &BTreeMap::new(),
        Path::new("."),
    )
    .unwrap()
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn controller_timer_cancels_without_native_statement_timeout_or_waiter() {
    let mut connection = ExecutionConnection::connect(
        config(),
        Path::new(&env::var("FBFAST_TEST_CLIENT").unwrap()),
    )
    .unwrap();
    connection.native.begin(false, 5, None).unwrap();
    let statement = connection.native.prepare("WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<1000) SELECT SUM(CAST(A.I AS BIGINT)*B.I*C.I) FROM N A CROSS JOIN N B CROSS JOIN N C").unwrap();
    let controller = Controller::new(Weak::new(), 0).unwrap();
    let progress = Arc::new(Progress {
        cancellation: Cancellation::default(),
        started: Mutex::new(Some(Instant::now())),
    });
    let start = Instant::now();
    let lease = controller.arm(
        connection.begin_control(1).unwrap(),
        1,
        Some(start + Duration::from_millis(100)),
        progress.clone(),
    );
    let result = connection
        .native
        .open_cursor(statement, &[], 0)
        .and_then(|_| connection.native.fetch(1, 1024).map(|_| ()));
    assert!(
        matches!(
            result,
            Err(fbfast_native::NativeError::Database {
                kind: fbfast_native::DatabaseErrorKind::Cancelled,
                ..
            })
        ),
        "{result:?}"
    );
    assert_eq!(
        progress.cancellation.check().unwrap_err().kind,
        ErrorKind::QueryTimeout
    );
    assert!(lease.finish().unwrap());
    eprintln!(
        "controller cancellation including 100ms deadline: {}ms",
        start.elapsed().as_millis()
    );
    assert!(start.elapsed() < Duration::from_secs(3));
    drop(lease);
    connection.close().unwrap();
    controller.stop_and_join().unwrap();
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn worker_panic_cleans_up_before_replacing_slot() {
    let pool = Pool::new(config(), env::var("FBFAST_TEST_CLIENT").unwrap().into());
    let mut request = pool.inner().unwrap().submit(Work::Panic).unwrap();
    loop {
        match request.wait_slice(Duration::from_millis(25)) {
            Ok(None) => continue,
            Err(error) => {
                assert_eq!(error.phase, "worker");
                break;
            }
            _ => panic!("fault injection returned a result"),
        }
    }
    let mut next = pool
        .submit_rows(
            "SELECT 1 FROM RDB$DATABASE".into(),
            vec![],
            true,
            false,
            Some(3.0),
        )
        .unwrap();
    loop {
        if let Some(output) = next.wait_slice(Duration::from_millis(25)).unwrap() {
            assert!(matches!(output, Output::Rows(_)));
            break;
        }
    }
    assert_eq!(pool.stats().unwrap().connections_created, 2);
    pool.close(Duration::from_secs(3)).unwrap();
    assert_eq!(pool.stats().unwrap().closed, 1);
    assert_eq!(pool.stats().unwrap().control_threads, 0);
}

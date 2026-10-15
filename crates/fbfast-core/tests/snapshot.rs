mod support;
use fbfast_core::{
    decode::Value,
    error::ErrorKind,
    execution::{control::Cancellation, ExecutionConnection},
    sinks::rows::Rows,
};
use std::{
    env,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
use support::{client, config, connection, options};

const DATA: &str = "SELECT VALUE_INTEGER FROM FBFAST_SNAPSHOT WHERE ID = 1";
const TRANSACTION: &str = "SELECT CURRENT_TRANSACTION, MON$READ_ONLY, MON$ISOLATION_MODE FROM MON$TRANSACTIONS WHERE MON$TRANSACTION_ID = CURRENT_TRANSACTION";

fn change_fixture(value: i32) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let result = Command::new(env::var("PYO3_PYTHON").unwrap())
        .arg(root.join("scripts/fixture_mutation.py"))
        .arg(value.to_string())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn snapshot_repeats_committed_data_across_queries_and_streams_then_releases() {
    change_fixture(10);
    let mut c = connection();
    c.begin_snapshot(Some(10.0)).unwrap();
    let identity = c.fetch_rows(TRANSACTION, &[], true, None).unwrap();
    assert_eq!(identity.cells[1..], [Value::Int(1), Value::Int(1)]);
    assert_eq!(
        c.fetch_rows(DATA, &[], true, None).unwrap().cells,
        [Value::Int(10)]
    );
    change_fixture(20);
    let hits = c.cache_counts().0;
    assert_eq!(
        c.fetch_rows(DATA, &[], true, None).unwrap().cells,
        [Value::Int(10)]
    );
    assert_eq!(c.cache_counts().0, hits + 1);
    let mut retained = None;
    c.stream_with(
        DATA,
        &[],
        1,
        &options(&Cancellation::default()),
        &mut |rows: Rows| {
            retained = Some(rows);
            Ok(true)
        },
    )
    .unwrap();
    assert_eq!(
        c.fetch_rows(TRANSACTION, &[], true, None).unwrap().cells,
        identity.cells
    );
    c.finish_snapshot(&options(&Cancellation::default()))
        .unwrap();
    assert!(!c.snapshot_active());
    assert!(c.is_healthy());
    assert_eq!(retained.unwrap().cells, [Value::Int(10)]);
    assert_eq!(
        c.fetch_rows(DATA, &[], true, None).unwrap().cells,
        [Value::Int(20)]
    );
    let after = c.fetch_rows(TRANSACTION, &[], true, None).unwrap();
    assert_ne!(after.cells[0], identity.cells[0]);
    assert_eq!(after.cells[1..], [Value::Int(1), Value::Int(4)]);
    c.close().unwrap();
    change_fixture(10);
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn snapshot_errors_end_transaction_and_prevent_accidental_implicit_restart() {
    for sql in [
        "SELECT NO_SUCH_COLUMN FROM RDB$DATABASE",
        "DELETE FROM FBFAST_TYPES",
    ] {
        let mut c = connection();
        c.begin_snapshot(None).unwrap();
        let error = c.fetch_rows(sql, &[], true, None).unwrap_err();
        assert!(error.cleanup.is_empty());
        assert!(!c.snapshot_active());
        assert!(!c.is_healthy());
        assert!(c
            .fetch_rows("SELECT 1 FROM RDB$DATABASE", &[], true, None)
            .is_err());
        c.close().unwrap();
    }
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn snapshot_deadline_is_shared_by_operations_and_explicit_close_is_idempotent() {
    let mut c = connection();
    c.begin_snapshot(Some(0.05)).unwrap();
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(
        c.fetch_rows("SELECT 1 FROM RDB$DATABASE", &[], true, None)
            .unwrap_err()
            .kind,
        ErrorKind::QueryTimeout
    );
    assert!(!c.snapshot_active());
    c.close().unwrap();
    let mut c = connection();
    c.begin_snapshot(None).unwrap();
    assert!(c.begin_snapshot(None).is_err());
    c.finish_snapshot(&options(&Cancellation::default()))
        .unwrap();
    c.finish_snapshot(&options(&Cancellation::default()))
        .unwrap();
    assert!(c.is_healthy());
    c.close().unwrap();
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn readiness_uses_acquisition_budget_and_does_not_inherit_query_timeout() {
    let error = match ExecutionConnection::connect_before(
        config(),
        Path::new("never-load-this"),
        Instant::now(),
    ) {
        Ok(_) => panic!("expired acquisition loaded a client"),
        Err(error) => error,
    };
    assert_eq!(error.kind, ErrorKind::PoolTimeout);
    assert_eq!(error.phase, "acquire");
    let mut config = config();
    config.query_timeout = Some(1e-9);
    let mut c = ExecutionConnection::connect(config, &client()).unwrap();
    assert_eq!(
        c.fetch_rows("SELECT 1 FROM RDB$DATABASE", &[], true, None)
            .unwrap()
            .cells,
        [Value::Int(1)]
    );
    c.close().unwrap();
}

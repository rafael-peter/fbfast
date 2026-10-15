mod support;
use fbfast_core::{
    decode::Value,
    error::{Error, ErrorKind},
    execution::{control::Cancellation, BatchConsumer, ExecutionConnection},
    sinks::rows::Rows,
};
use std::time::{Duration, Instant};
use support::{client, config, connection, options};

const MANY: &str = "WITH RECURSIVE N(I) AS (SELECT 1 FROM RDB$DATABASE UNION ALL SELECT I+1 FROM N WHERE I<103) SELECT I FROM N ORDER BY I";

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn owned_batches_match_materialized_rows_and_ignore_total_materialization_quota() {
    let mut c = connection();
    let all = c.fetch_rows(MANY, &[], false, None).unwrap();
    c.close().unwrap();
    let mut config = config();
    config.max_result_rows = Some(1);
    let mut c = ExecutionConnection::connect(config, &client()).unwrap();
    let mut batches = Vec::new();
    let summary = c
        .stream_with(
            MANY,
            &[],
            10,
            &options(&Cancellation::default()),
            &mut |rows: Rows| {
                batches.push(rows);
                Ok(true)
            },
        )
        .unwrap();
    assert_eq!(summary.rows, 103);
    assert_eq!(summary.batches, 11);
    assert!(batches.iter().all(|batch| batch.count <= 10));
    assert_eq!(batches.last().unwrap().count, 3);
    let hits = c.cache_counts().0;
    let summary = c
        .stream_with(
            MANY,
            &[],
            50,
            &options(&Cancellation::default()),
            &mut |_: Rows| Ok(false),
        )
        .unwrap();
    assert_eq!(summary.rows, 50);
    assert_eq!(c.cache_counts().0, hits + 1);
    c.close().unwrap();
    let cells: Vec<_> = batches.into_iter().flat_map(|batch| batch.cells).collect();
    assert_eq!(cells, all.cells); // owned buffers survive cursor reuse and detach
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn early_stop_consumer_error_and_cancel_always_close_cursor_and_transaction() {
    let mut c = connection();
    let first = c
        .stream_with(
            MANY,
            &[],
            7,
            &options(&Cancellation::default()),
            &mut |_: Rows| Ok(false),
        )
        .unwrap();
    assert_eq!(first.rows, 7);
    let error = c
        .stream_with(
            MANY,
            &[],
            7,
            &options(&Cancellation::default()),
            &mut |_: Rows| {
                Err(Error::new(
                    ErrorKind::DataConversion,
                    "sink",
                    "injected consumer failure",
                ))
            },
        )
        .unwrap_err();
    assert_eq!(error.phase, "sink");
    assert!(error.cleanup.is_empty());
    let token = Cancellation::default();
    let error = c
        .stream_with(MANY, &[], 7, &options(&token), &mut |_: Rows| {
            token.cancel();
            Ok(true)
        })
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled);
    assert!(error.cleanup.is_empty());
    assert!(c.is_healthy()); // no concurrent native cancel was raised in this direct-core test
    let identity = c.fetch_rows("SELECT MON$READ_ONLY, MON$ISOLATION_MODE FROM MON$TRANSACTIONS WHERE MON$TRANSACTION_ID = CURRENT_TRANSACTION", &[], true, None).unwrap();
    assert_eq!(identity.cells, [Value::Int(1), Value::Int(4)]);
    c.close().unwrap();
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn byte_limited_batches_empty_results_and_alias_validation() {
    let mut config = config();
    config.batch_bytes = 1;
    let mut c = ExecutionConnection::connect(config, &client()).unwrap();
    let summary = c
        .stream_with(
            MANY,
            &[],
            20,
            &options(&Cancellation::default()),
            &mut |rows: Rows| {
                assert_eq!(rows.count, 1);
                Ok(true)
            },
        )
        .unwrap();
    assert_eq!(summary.batches, 103);
    let summary = c
        .stream_with(
            "SELECT ID FROM FBFAST_TYPES WHERE 1=0",
            &[],
            10,
            &options(&Cancellation::default()),
            &mut |_: Rows| panic!("empty batch delivered"),
        )
        .unwrap();
    assert_eq!(summary.rows, 0);
    assert_eq!(summary.batches, 0);
    let token = Cancellation::default();
    let mut opts = options(&token);
    opts.unique_names = true;
    assert_eq!(
        c.stream_with(
            "SELECT ID AS X, ID AS X FROM FBFAST_TYPES",
            &[],
            10,
            &opts,
            &mut |_: Rows| panic!("ambiguous schema delivered")
        )
        .unwrap_err()
        .kind,
        ErrorKind::DataConversion
    );
    assert!(c.is_healthy());
    c.close().unwrap();
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn decoded_structures_count_toward_the_batch_byte_target() {
    let mut config = config();
    let target = 10 * std::mem::size_of::<Value>();
    config.batch_bytes = target as u64;
    let mut c = ExecutionConnection::connect(config, &client()).unwrap();
    let summary = c
        .stream_with(
            MANY,
            &[],
            100,
            &options(&Cancellation::default()),
            &mut |rows: Rows| {
                assert!(rows.bytes <= target);
                assert!(rows.count <= 10);
                Ok(true)
            },
        )
        .unwrap();
    assert_eq!(summary.batches, 11);
    c.close().unwrap();
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn deadline_includes_consumer_wait_and_demand_can_stop_before_first_fetch() {
    struct NoDemand;
    impl BatchConsumer for NoDemand {
        fn before_fetch(&mut self) -> Result<bool, Error> {
            Ok(false)
        }
        fn batch(&mut self, _: Rows) -> Result<bool, Error> {
            panic!("fetch without demand")
        }
    }
    let mut c = connection();
    assert_eq!(
        c.stream_with(
            MANY,
            &[],
            10,
            &options(&Cancellation::default()),
            &mut NoDemand
        )
        .unwrap()
        .rows,
        0
    );
    let token = Cancellation::default();
    let mut opts = options(&token);
    opts.deadline = Some(Instant::now() + Duration::from_millis(150));
    let error = c
        .stream_with(MANY, &[], 10, &opts, &mut |_: Rows| {
            std::thread::sleep(Duration::from_millis(200));
            Ok(true)
        })
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::QueryTimeout);
    assert!(error.cleanup.is_empty());
    assert_eq!(
        c.fetch_rows("SELECT 9 FROM RDB$DATABASE", &[], true, None)
            .unwrap()
            .cells,
        [Value::Int(9)]
    );
    c.close().unwrap();
}

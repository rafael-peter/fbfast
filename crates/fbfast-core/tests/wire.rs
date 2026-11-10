//! Round-trip regression counts, read from the Remote provider's own counters.
mod support;
use fbfast_core::{
    config::Config,
    execution::control::Cancellation,
    pool::{Output, Pool, Query},
};

const SQL: &str = "SELECT 1 FROM RDB$DATABASE";

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn direct_connection_round_trips_per_query_phase() {
    let mut connection = support::connection();
    let a = connection.wire_stats().unwrap().unwrap();
    let b = connection.wire_stats().unwrap().unwrap();
    assert_eq!(b.since(&a).roundtrips, 0, "reading wire counters is local");
    let cancel = Cancellation::default();
    let mut costs = Vec::new();
    for _ in 0..3 {
        let before = connection.wire_stats().unwrap().unwrap();
        connection
            .fetch_with(SQL, &[], &support::options(&cancel))
            .unwrap();
        costs.push(
            connection
                .wire_stats()
                .unwrap()
                .unwrap()
                .since(&before)
                .roundtrips,
        );
    }
    // begin + prepare + execute/fetch + rollback + reset; cached: no prepare.
    assert_eq!(costs, [5, 4, 4]);
    let before = connection.wire_stats().unwrap().unwrap();
    connection.ping().unwrap();
    assert_eq!(
        connection
            .wire_stats()
            .unwrap()
            .unwrap()
            .since(&before)
            .roundtrips,
        1
    );
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn pool_answers_cached_query_after_two_round_trips() {
    let mut config: Config = support::config();
    config.diagnostics = true;
    config.pool_size = 1;
    config.min_idle = 1;
    config.ready_transaction_timeout = None;
    let pool = Pool::new(config, support::client());
    for _ in 0..3 {
        let mut pending = pool
            .submit(Query::rows(SQL.into(), vec![], true, false, None))
            .unwrap();
        let output = loop {
            if let Some(output) = pending
                .wait_slice(std::time::Duration::from_millis(50))
                .unwrap()
            {
                break output;
            }
        };
        assert!(matches!(output, Output::Rows(_)));
    }
    let traces = pool.diagnostics().unwrap();
    assert_eq!(traces.len(), 3);
    let cached = &traces[2];
    assert!(cached.ok && cached.statement_cached);
    // begin + execute/first batch; cursor close travels with the next packet,
    // rollback and session reset run after the reply.
    assert_eq!(cached.roundtrips, 2, "{cached:?}");
    let phases: Vec<_> = cached.phases.iter().map(|p| p.phase).collect();
    assert_eq!(
        phases,
        ["queue", "begin", "prepare", "execute", "fetch", "cleanup"]
    );
    pool.close(std::time::Duration::from_secs(10)).unwrap();
    let stats = pool.stats().unwrap();
    assert_eq!(stats.deferred_cleanups, 3);
    assert_eq!(stats.cleanup_errors, 0);
    assert!(stats.wire_roundtrips >= 3 * 4, "{}", stats.wire_roundtrips);
}

#[test]
#[ignore = "requires isolated fixture; scripts/test_native.py runs this"]
fn pool_reuses_the_reset_transaction_for_one_round_trip() {
    let mut config: Config = support::config();
    config.diagnostics = true;
    config.pool_size = 1;
    config.min_idle = 1;
    let pool = Pool::new(config, support::client());
    for _ in 0..3 {
        let mut pending = pool
            .submit(Query::rows(SQL.into(), vec![], true, false, None))
            .unwrap();
        while pending
            .wait_slice(std::time::Duration::from_millis(50))
            .unwrap()
            .is_none()
        {}
        // The next query must find the slot after its deferred reset.
        while pool.stats().unwrap().cleaning != 0 {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    let traces = pool.diagnostics().unwrap();
    // reset+restart ran after the previous reply: begin costs nothing now.
    assert_eq!(traces[2].roundtrips, 1, "{:?}", traces[2]);
    pool.close(std::time::Duration::from_secs(10)).unwrap();
    assert!(pool.stats().unwrap().transaction_restarts >= 2);
}

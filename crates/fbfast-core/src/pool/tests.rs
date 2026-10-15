use super::{
    state::{Inner, Phase, Work},
    *,
};
use crate::config::{EnvFile, Inputs};
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

fn occupied() -> Arc<Inner> {
    let input: Inputs = [
        ("database".into(), json!("unused")),
        ("user".into(), json!("unused")),
        ("password".into(), json!("unused")),
        ("pool_size".into(), json!(1)),
        ("min_idle".into(), json!(0)),
        ("max_waiters".into(), json!(2)),
        ("acquire_timeout".into(), json!(0.001)),
    ]
    .into();
    let config = Config::resolve_sources(
        input,
        "FB_",
        EnvFile::Disabled,
        &BTreeMap::new(),
        Path::new("."),
    )
    .unwrap();
    let inner = Arc::new(Inner::new(config, "not-loaded".into()));
    // Model a slot held by native work; no worker or database is started here.
    inner.state.lock().unwrap().slots[0].phase = Phase::Leased;
    inner
}

#[test]
fn bounded_fifo_and_cancel_do_not_release_active_slot() {
    let inner = occupied();
    let first = inner.submit(Work::Warmup).unwrap();
    let second = inner.submit(Work::Warmup).unwrap();
    assert!(matches!(
        inner.submit(Work::Warmup),
        Err(Error {
            kind: ErrorKind::PoolBusy,
            ..
        })
    ));
    assert_eq!(
        inner
            .state
            .lock()
            .unwrap()
            .queue
            .iter()
            .map(|j| j.id)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    first.cancel(false);
    drop(first); // idempotent; no double-counted cancellation
    assert_eq!(inner.stats().cancellations, 1);
    assert_eq!(inner.stats().leased, 1);
    assert_eq!(inner.state.lock().unwrap().queue.front().unwrap().id, 2);
    drop(second);
    assert_eq!(inner.stats().waiting, 0);
    assert_eq!(inner.stats().leased, 1);
}

#[test]
fn acquire_timeout_removes_queue_entry_and_shutdown_is_bounded() {
    let inner = occupied();
    let mut pending = inner.submit(Work::Warmup).unwrap();
    assert!(matches!(
        pending.wait_slice(Duration::from_millis(10)),
        Err(Error {
            kind: ErrorKind::PoolTimeout,
            ..
        })
    ));
    assert_eq!(inner.stats().waiting, 0);
    assert_eq!(inner.stats().leased, 1);
    assert_eq!(inner.stats().timeouts, 1);
    inner.begin_close();
    assert!(matches!(
        inner.wait_closed(Duration::from_millis(1)),
        Err(Error {
            kind: ErrorKind::PoolTimeout,
            phase: "close",
            ..
        })
    ));
    assert_eq!(inner.stats().leased, 1);
}

#[test]
fn closing_wakes_waiters_and_rejects_new_work() {
    let inner = occupied();
    let mut pending = inner.submit(Work::Warmup).unwrap();
    inner.begin_close();
    assert!(matches!(
        pending.wait_slice(Duration::ZERO),
        Err(Error {
            kind: ErrorKind::PoolClosed,
            ..
        })
    ));
    assert!(matches!(
        inner.submit(Work::Warmup),
        Err(Error {
            kind: ErrorKind::PoolClosed,
            ..
        })
    ));
}

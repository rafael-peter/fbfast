use super::{
    cache_invalidation::{available, closed, empty, service},
    state::{Command, Phase},
    tests::occupied,
    Output,
};
use crate::error::{Error, ErrorKind};
use std::{sync::mpsc::sync_channel, sync::Arc, time::Duration};

#[test]
fn invalidation_between_acknowledgement_and_idle_publication_is_not_lost() {
    let inner = occupied();
    let weak = Arc::downgrade(&inner);
    let (sender, receiver) = sync_channel(1);
    inner.state.lock().unwrap().slots[0].sender = Some(sender);
    let mut first = inner.request_cache_clear(None).unwrap();
    // Pause a synthetic owner just after its cleanup, before publishing Idle.
    service(&weak, 0, &mut None).unwrap();
    assert!(matches!(
        first.wait_slice(Duration::ZERO),
        Ok(Some(Output::Ready))
    ));
    let mut second = inner.request_cache_clear(None).unwrap();
    {
        let mut state = inner.state.lock().unwrap();
        available(&mut state, 0, true);
        assert!(matches!(state.slots[0].phase, Phase::Cleaning));
    }
    assert!(matches!(receiver.try_recv(), Ok(Command::ClearCache)));
    assert!(matches!(second.wait_slice(Duration::ZERO), Ok(None)));
    service(&weak, 0, &mut None).unwrap();
    assert!(matches!(
        second.wait_slice(Duration::ZERO),
        Ok(Some(Output::Ready))
    ));
    assert_eq!(inner.stats().native_cancels, 0);
}

#[test]
fn invalidation_between_detach_and_closed_publication_needs_no_new_worker() {
    let inner = occupied();
    let weak = Arc::downgrade(&inner);
    inner.state.lock().unwrap().slots[0].phase = Phase::Closing;
    closed(&weak, 0, None);
    let mut pending = inner.request_cache_clear(None).unwrap();
    assert!(matches!(pending.wait_slice(Duration::ZERO), Ok(None)));
    {
        let mut state = inner.state.lock().unwrap();
        state.slots[0].phase = Phase::Closed;
        empty(&mut state, 0);
    }
    assert!(matches!(
        pending.wait_slice(Duration::ZERO),
        Ok(Some(Output::Ready))
    ));
    assert_eq!(inner.stats().connections_created, 0);
}

#[test]
fn coalesced_waiters_receive_cleanup_errors_and_cancel_does_not_release_owner() {
    let inner = occupied();
    let first = inner.request_cache_clear(None).unwrap();
    let mut second = inner.request_cache_clear(None).unwrap();
    first.cancel(false);
    let mut third = inner.request_cache_clear(None).unwrap();
    assert_eq!(inner.stats().leased, 1);
    assert_eq!(inner.stats().waiting, 2);
    let error = Error::new(
        ErrorKind::Cleanup,
        "statement_free",
        "injected cleanup failure",
    );
    closed(&Arc::downgrade(&inner), 0, Some(&error));
    for pending in [&mut second, &mut third] {
        let error = pending.wait_slice(Duration::ZERO).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Cleanup);
        assert_eq!(error.cleanup.len(), 1);
        assert_eq!(error.cleanup[0].phase, "statement_free");
    }
    assert_eq!(inner.stats().waiting, 0);
    assert_eq!(inner.stats().cancellations, 1);
    assert_eq!(inner.stats().native_cancels, 0);
}

#[test]
fn shutdown_rejects_pending_cache_clear_without_waiting_for_native_owner() {
    let inner = occupied();
    let mut pending = inner.request_cache_clear(None).unwrap();
    inner.begin_close();
    assert_eq!(
        pending.wait_slice(Duration::ZERO).unwrap_err().kind,
        ErrorKind::PoolClosed
    );
    assert_eq!(inner.stats().waiting, 0);
    assert_eq!(inner.stats().leased, 1);
}

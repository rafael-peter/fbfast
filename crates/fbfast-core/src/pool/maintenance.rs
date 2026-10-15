use super::state::{Inner, Phase, State, Work};
use std::sync::Arc;

impl Inner {
    /// Reserve only free slots after foreground FIFO dispatch. Opening slots
    /// count toward the desired floor so concurrent workers cannot overfill it.
    /// Reuse the existing workers/backoff; no maintenance thread or job queue.
    pub(super) fn keep_min_idle(self: &Arc<Self>, state: &mut State) {
        if !state.initialized || state.closing || !state.queue.is_empty() {
            return;
        }
        let ready = state
            .slots
            .iter()
            .filter(|s| matches!(s.phase, Phase::Idle | Phase::Opening))
            .count();
        let missing = (self.config.min_idle as usize).saturating_sub(ready);
        for _ in 0..missing {
            let Some(index) = state
                .slots
                .iter()
                .position(|s| matches!(s.phase, Phase::Closed | Phase::Dormant))
            else {
                break;
            };
            if state.next_id == u64::MAX {
                break;
            }
            let (job, pending) = self.job(state, Work::KeepWarm);
            pending.detach();
            if !self.start_slot(state, index, job) {
                state.stats.maintenance_failures += 1;
                break;
            }
        }
    }
}

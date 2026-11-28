//! Lifecycle controls that remain reachable while a chat holds the front end's runtime lock.

use std::sync::Arc;
use std::time::Duration;

use crate::stream::TurnRegistry;

/// A cheap handle to stop every streamed turn or shut down admission and await cleanup.
/// No microphone is owned by the runtime yet; voice cancellation must join this handle in P4.
#[derive(Debug, Clone)]
pub struct LifecycleControl {
    pub(crate) turns: Arc<TurnRegistry>,
}

impl LifecycleControl {
    /// Stops admitted turns (including approval waits) without preventing a later user turn.
    pub fn emergency_stop(&self) {
        self.turns.cancel_all(false);
    }

    /// Refuses new turns and cancels admitted work atomically. Safe to call repeatedly.
    pub fn begin_shutdown(&self) {
        self.turns.cancel_all(true);
    }

    /// Waits at most `budget` for cancelled turns to finish storing partial replies/audit.
    /// False means cleanup timed out; startup recovery will mark unfinished tasks interrupted.
    pub async fn wait_for_idle(&self, budget: Duration) -> bool {
        tokio::time::timeout(budget, async {
            while !self.turns.is_idle() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok()
    }
}

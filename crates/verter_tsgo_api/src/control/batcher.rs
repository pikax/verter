//! Coalesces concurrent carrier overlay writes into `verter/carrierSyncBatch`
//! requests.
//!
//! Every overlay write must be ordered behind a sync barrier before the `--api`
//! side may read it, and a barrier costs the editor's engine a program update. A
//! caller that injects a project's worth of overlays one request at a time pays
//! that once per overlay. Writes submitted while a batch is in flight ride the
//! NEXT batch together, so the cost follows the number of round-trips, not the
//! number of overlays — with no timer and no added latency for a lone write.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::oneshot;

use super::client::ControlClient;
use super::messages::{CarrierBatchFailureKind, CarrierBatchOp};
use crate::error::{TsgoApiError, TsgoApiResult};

type Pending = (CarrierBatchOp, oneshot::Sender<TsgoApiResult<()>>);

#[derive(Default)]
struct Shared {
    queue: Mutex<Vec<Pending>>,
    flushing: AtomicBool,
}

/// Lowers the flag even if the flusher task is dropped mid-flight, so a later
/// write can start a new one.
struct FlushingGuard(Option<Arc<Shared>>);

impl Drop for FlushingGuard {
    fn drop(&mut self) {
        if let Some(shared) = &self.0 {
            shared.flushing.store(false, Ordering::Release);
        }
    }
}

/// See the module documentation.
pub struct OverlayBatcher {
    control: Arc<ControlClient>,
    shared: Arc<Shared>,
}

impl OverlayBatcher {
    /// A batcher writing through `control`.
    #[must_use]
    pub fn new(control: Arc<ControlClient>) -> Self {
        Self {
            control,
            shared: Arc::default(),
        }
    }

    /// Queue one overlay write and resolve once a barrier has ordered it (or its
    /// send / the barrier failed). A write that was sent but whose barrier failed is
    /// [`TsgoApiError::Timeout`] — the same error the single-op control calls report —
    /// and every other failure is [`TsgoApiError::Transport`].
    ///
    /// The flush runs on its OWN task: a caller that is cancelled while waiting —
    /// a request deadline — must not abandon a batch other callers are part of.
    pub async fn submit(&self, op: CarrierBatchOp) -> TsgoApiResult<()> {
        let (tx, rx) = oneshot::channel();
        self.shared.queue.lock().push((op, tx));
        self.ensure_flusher();
        rx.await.unwrap_or(Err(TsgoApiError::Closed))
    }

    fn ensure_flusher(&self) {
        if self.shared.flushing.swap(true, Ordering::AcqRel) {
            return;
        }
        let control = Arc::clone(&self.control);
        let shared = Arc::clone(&self.shared);
        tokio::spawn(async move {
            let mut guard = FlushingGuard(Some(Arc::clone(&shared)));
            loop {
                let batch: Vec<Pending> = {
                    let mut queue = shared.queue.lock();
                    if queue.is_empty() {
                        // Lowered UNDER the queue lock: a write pushed after this
                        // point finds the flag down and starts the next flusher.
                        shared.flushing.store(false, Ordering::Release);
                        // A newer flusher may already own the flag.
                        guard.0 = None;
                        break;
                    }
                    std::mem::take(&mut *queue)
                };
                flush(&control, batch).await;
            }
        });
    }
}

async fn flush(control: &ControlClient, batch: Vec<Pending>) {
    let (ops, waiters): (Vec<_>, Vec<_>) = batch.into_iter().unzip();
    match control.carrier_sync_batch(ops).await {
        Ok(result) => {
            let outcomes = outcomes(waiters.len(), &result.failures);
            for (waiter, outcome) in waiters.into_iter().zip(outcomes) {
                let _ = waiter.send(outcome);
            }
        }
        Err(error) => {
            let message = error.to_string();
            for waiter in waiters {
                let _ = waiter.send(Err(TsgoApiError::Transport(message.clone())));
            }
        }
    }
}

/// Each write's outcome, in batch order. A failure names its write by POSITION:
/// one batch may carry several writes for a URI, and each caller must hear about
/// its own.
fn outcomes(
    writes: usize,
    failures: &[super::messages::CarrierBatchFailure],
) -> Vec<TsgoApiResult<()>> {
    let mut outcomes: Vec<TsgoApiResult<()>> = (0..writes).map(|_| Ok(())).collect();
    for failure in failures {
        let Some(outcome) = outcomes.get_mut(failure.index) else {
            continue;
        };
        *outcome = Err(match failure.kind {
            CarrierBatchFailureKind::SentUnconfirmed => {
                TsgoApiError::Timeout(failure.message.clone())
            }
            CarrierBatchFailureKind::SendFailed => TsgoApiError::Transport(failure.message.clone()),
        });
    }
    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::messages::CarrierBatchFailure;

    #[test]
    fn two_writes_for_one_uri_each_hear_their_own_outcome() {
        let failure = CarrierBatchFailure {
            index: 2,
            uri: "file:///w/A.ts".to_string(),
            message: "m".to_string(),
            kind: CarrierBatchFailureKind::SendFailed,
        };
        let outcomes = outcomes(3, &[failure]);
        assert!(
            outcomes[0].is_ok(),
            "the earlier write for the same URI succeeded"
        );
        assert!(outcomes[1].is_ok());
        assert!(matches!(outcomes[2], Err(TsgoApiError::Transport(_))));
    }
}

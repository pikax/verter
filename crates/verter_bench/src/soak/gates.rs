//! The publication-gate workload.
//!
//! The LSP publish coordinator serialises membership transitions per source
//! behind a gate that exists only while a participant (the holder or a
//! waiter) is registered with it. The workload drives the public reconciler
//! surface: one holder per source parks inside the membership commit while
//! holding its gate, a second transition per source waits behind it, half of
//! the waiters are cancelled mid-wait, and the rest complete once the
//! holders are released. The registry's live gate count, per-source
//! participant count and backing capacity are read throughout and must
//! return to nothing once every participant has left, the backing capacity
//! shrinking from its peak.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::Poll;

use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use verter_lsp::external_ts::membership_reconciler::SourceGates;
use verter_lsp::external_ts::{
    AbsentReason, CanonicalSource, CarrierCompanion, CarrierMembershipCommitter,
    CarrierPublishError, CommitFuture, MembershipLedger, MembershipReconciler,
};
use verter_session::external_ts::ProjectBinding;

/// What one gate action observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GateEvidence {
    /// Sources driven.
    pub sources: usize,
    /// Holders that reached the commit while holding their gate.
    pub holders_entered: usize,
    /// Live gates once every holder held one.
    pub live_with_holders: usize,
    /// Sources whose gate showed holder and waiter together.
    pub waited_sources: usize,
    /// Cancelled waiters whose source went back to the holder alone.
    pub cancelled_left: usize,
    /// Cancelled waiters.
    pub cancelled: usize,
    /// Transitions that completed after the release.
    pub completed: usize,
    /// Live gates once every participant had left.
    pub live_after: usize,
    /// Participants left across the driven sources.
    pub participants_after: usize,
    /// Backing capacity with every gate live.
    pub capacity_peak: usize,
    /// Backing capacity after every participant left.
    pub capacity_after: usize,
}

/// A committer whose retraction parks until released, holding the
/// transition's gate.
struct ParkedCommitter {
    entered: AtomicUsize,
    release: Semaphore,
}

impl CarrierMembershipCommitter for ParkedCommitter {
    fn commit_owned<'a>(
        &'a self,
        _binding: &'a ProjectBinding,
        _source_canonical: &'a str,
        _companions: &'a [CarrierCompanion],
    ) -> CommitFuture<'a> {
        Box::pin(async { Ok(()) })
    }

    fn retract<'a>(&'a self, _source_canonical: &'a str) -> CommitFuture<'a> {
        Box::pin(async move {
            self.entered.fetch_add(1, Ordering::SeqCst);
            match self.release.acquire().await {
                Ok(permit) => {
                    permit.forget();
                    Ok(())
                }
                Err(_) => Err(CarrierPublishError::Retract("gate drive closed".into())),
            }
        })
    }
}

/// A membership transition staged by one poll.
struct Staged<F> {
    future: Pin<Box<F>>,
    /// The answer, when the first poll already completed it.
    early: Option<bool>,
}

impl<F: Future<Output = bool>> Staged<F> {
    /// Poll once. `Pending` is the registration event: the transition ran
    /// synchronously up to its first suspension point, so it already holds
    /// (or has queued behind) its source's gate.
    async fn start(future: F) -> Self {
        let mut future = Box::pin(future);
        let early = std::future::poll_fn(|cx| match future.as_mut().poll(cx) {
            Poll::Ready(answer) => Poll::Ready(Some(answer)),
            Poll::Pending => Poll::Ready(None),
        })
        .await;
        Self { future, early }
    }

    async fn finish(self) -> bool {
        match self.early {
            Some(answer) => answer,
            None => self.future.await,
        }
    }
}

/// Run one gate action over `sources` sources of `tranche`.
#[must_use]
pub fn run(tranche: u32, sources: usize) -> GateEvidence {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a gate drive runtime");
    runtime.block_on(drive(tranche, sources))
}

async fn drive(tranche: u32, sources: usize) -> GateEvidence {
    let gates = Arc::new(SourceGates::new());
    let committer = Arc::new(ParkedCommitter {
        entered: AtomicUsize::new(0),
        release: Semaphore::new(0),
    });
    let reconciler = Arc::new(MembershipReconciler::new_store_only(
        Arc::new(MembershipLedger::with_initial_session()),
        Arc::clone(&committer) as Arc<dyn CarrierMembershipCommitter>,
        Arc::clone(&gates),
    ));
    let names: Vec<CanonicalSource> = (0..sources)
        .map(|i| CanonicalSource::new(format!("/gate/t{tranche:03}/Comp{i}.vue")))
        .collect();
    let transition = |source: &CanonicalSource| {
        let reconciler = Arc::clone(&reconciler);
        let source = source.clone();
        async move {
            reconciler
                .remove_source_membership(&source, AbsentReason::Deleted)
                .await
                .is_ok()
        }
    };

    let mut holders = Vec::new();
    for name in &names {
        // The holder parks inside the commit while holding its gate.
        holders.push(Staged::start(transition(name)).await);
    }
    let holders_entered = committer.entered.load(Ordering::SeqCst);
    let live_with_holders = gates.live_gates();
    let capacity_peak = gates.backing_capacity();

    let mut waiters = Vec::new();
    for name in &names {
        waiters.push(Staged::start(transition(name)).await);
    }
    let waited_sources = names
        .iter()
        .filter(|n| gates.participants(n.as_str()) == 2)
        .count();

    let mut cancelled = 0;
    let mut kept = Vec::new();
    for (i, waiter) in waiters.into_iter().enumerate() {
        if i % 2 == 0 {
            drop(waiter);
            cancelled += 1;
        } else {
            kept.push(waiter);
        }
    }
    let cancelled_left = names
        .iter()
        .step_by(2)
        .filter(|n| gates.participants(n.as_str()) == 1)
        .count();

    committer.release.add_permits(sources + kept.len());
    let mut completed = 0;
    for task in holders.into_iter().chain(kept) {
        if task.finish().await {
            completed += 1;
        }
    }
    GateEvidence {
        sources,
        holders_entered,
        live_with_holders,
        waited_sources,
        cancelled_left,
        cancelled,
        completed,
        live_after: gates.live_gates(),
        participants_after: names.iter().map(|n| gates.participants(n.as_str())).sum(),
        capacity_peak,
        capacity_after: gates.backing_capacity(),
    }
}

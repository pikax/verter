//! Project-level singleflight for full-open-file provider re-syncs.
//!
//! `resync_open_files` closes and re-opens EVERY open document in the type
//! provider — an O(open-files) burst of provider traffic. Background init fires
//! it up to twice per pass, and a workspace-folder / tsconfig / watcher event can
//! fire it again concurrently. With no coalescing, N overlapping triggers become
//! N full close+reopen sweeps stacked on the interactive lane — a resync storm
//! that starves interactive requests.
//!
//! [`ResyncCoordinator`] enforces two properties: the sweep is NEVER concurrent,
//! and overlapping triggers fold into at most ONE queued re-run behind the
//! in-flight sweep (every trigger arriving mid-sweep sets the same single pending
//! bit). So a burst of 10 concurrent triggers runs the in-flight sweep plus one
//! re-run reflecting the latest state, not 10 sweeps. A steady drip of triggers
//! that keeps re-arming `pending` mid-sweep keeps the runner looping — the bound
//! is on CONCURRENCY and on queue depth, not on the lifetime sweep count.
//!
//! A follower does NOT wait for the in-flight sweep: it arms the bit and returns.
//! The per-document IDE-sync repair lease is per-DOCUMENT; this is the
//! project-level counterpart the resync path lacked.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use std::future::Future;

/// Coalescing singleflight gate for `resync_open_files`.
#[derive(Default)]
pub struct ResyncCoordinator {
    /// A sweep is currently running.
    running: AtomicBool,
    /// At least one trigger arrived while a sweep was running (or is queued).
    /// A single bit — any number of concurrent triggers coalesce into ONE re-run.
    pending: AtomicBool,
}

impl ResyncCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run `sweep` under the coalescing singleflight.
    ///
    /// - If no sweep is running, this caller becomes the runner and drains the
    ///   pending bit in a loop: it runs `sweep`, and if any trigger arrived while
    ///   it ran, it runs `sweep` again (folding all of them together).
    /// - If a sweep is already running, this caller only arms the pending bit and
    ///   returns IMMEDIATELY — it does not wait for the sweep to finish. A caller
    ///   that must observe the swept state has to be the runner, or re-trigger.
    ///
    /// Net effect: the sweep is never concurrent, and a burst of N concurrent
    /// triggers is folded into at most one queued re-run behind the in-flight one.
    pub async fn resync<F, Fut>(&self, sweep: F)
    where
        F: Fn() -> Fut,
        Fut: Future<Output = ()>,
    {
        // Register this trigger. Idempotent: many concurrent triggers set the
        // same single bit.
        self.pending.store(true, Ordering::SeqCst);

        // Try to become the runner. If another task already is, our pending bit
        // is enough — it will drain it.
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }

        // We own the run. The guard hands `running` back on EVERY exit path,
        // including a panicking sweep and a dropped runner future: a run that
        // ends without releasing the flag makes every later trigger return early,
        // which silently kills provider resync for the rest of the session.
        let mut run = RunGuard {
            running: &self.running,
            pending: &self.pending,
            owns_run: true,
        };

        // Drain the pending bit until it stays clear, re-checking once after
        // releasing `running` to close the race with a trigger that arms
        // `pending` between our last drain and the release.
        loop {
            while self.pending.swap(false, Ordering::SeqCst) {
                sweep().await;
            }
            run.release();
            // A trigger may have armed `pending` after our last `swap(false)` but
            // before we cleared `running`; reclaim the run to honor it, else stop.
            if self.pending.load(Ordering::SeqCst) && !self.running.swap(true, Ordering::SeqCst) {
                run.reclaim();
                continue;
            }
            break;
        }
    }
}

/// Hands the runner slot back on every exit path.
///
/// A run that ends abnormally — the sweep panicked, or the runner future was
/// dropped when its owning task was cancelled — must not keep `running` set: the
/// gate would reject every later trigger forever. An abnormal end also RE-ARMS
/// `pending`, so the triggers that coalesced onto the interrupted run are honored
/// by the next one instead of being swallowed with it.
struct RunGuard<'a> {
    running: &'a AtomicBool,
    pending: &'a AtomicBool,
    owns_run: bool,
}

impl RunGuard<'_> {
    /// Hand the slot back at a clean loop boundary (no work was interrupted).
    fn release(&mut self) {
        if self.owns_run {
            self.owns_run = false;
            self.running.store(false, Ordering::SeqCst);
        }
    }

    /// Take the slot back after re-winning the `running` swap.
    fn reclaim(&mut self) {
        self.owns_run = true;
    }
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        if self.owns_run {
            // Abnormal end: whatever the interrupted sweep was draining is
            // unfinished, so leave the work armed for the next trigger.
            self.pending.store(true, Ordering::SeqCst);
            self.release();
        }
    }
}

/// Convenience: run a provider's `resync_open_files` under the coordinator.
pub async fn resync_open_files_coalesced(
    coordinator: &Arc<ResyncCoordinator>,
    provider: Arc<dyn crate::type_provider::traits::TypeProvider>,
) {
    coordinator
        .resync(|| {
            let provider = Arc::clone(&provider);
            async move {
                let _ = provider.resync_open_files().await;
            }
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// A burst of concurrent resync triggers coalesces to the in-flight sweep plus
    /// at most one queued re-run, never N. Without this gate every trigger runs a
    /// full sweep.
    #[tokio::test]
    async fn concurrent_triggers_coalesce_to_at_most_two_sweeps() {
        let coordinator = Arc::new(ResyncCoordinator::new());
        let runs = Arc::new(AtomicUsize::new(0));
        // Gate the FIRST sweep open until every trigger has been submitted, so the
        // coalescing window is deterministic (all followers arrive mid-flight).
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());

        let leader = {
            let coordinator = Arc::clone(&coordinator);
            let runs = Arc::clone(&runs);
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            tokio::spawn(async move {
                coordinator
                    .resync(|| {
                        let runs = Arc::clone(&runs);
                        let started = Arc::clone(&started);
                        let release = Arc::clone(&release);
                        async move {
                            let n = runs.fetch_add(1, Ordering::SeqCst) + 1;
                            if n == 1 {
                                // Signal that the first sweep is running, then block
                                // until the test releases it — the whole storm lands
                                // during this window.
                                started.notify_one();
                                release.notified().await;
                            }
                        }
                    })
                    .await;
            })
        };

        // Wait until the leader's first sweep is in-flight.
        started.notified().await;

        // Fire 10 concurrent triggers while the first sweep is blocked. Each uses
        // the SAME counting sweep, so a non-coalescing implementation would run it
        // 10 more times (11 total); the coalescing gate folds them into the single
        // re-arm the leader drains after release (2 total).
        let mut followers = Vec::new();
        for _ in 0..10 {
            let coordinator = Arc::clone(&coordinator);
            let runs = Arc::clone(&runs);
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            followers.push(tokio::spawn(async move {
                coordinator
                    .resync(|| {
                        let runs = Arc::clone(&runs);
                        let started = Arc::clone(&started);
                        let release = Arc::clone(&release);
                        async move {
                            let n = runs.fetch_add(1, Ordering::SeqCst) + 1;
                            if n == 1 {
                                started.notify_one();
                                release.notified().await;
                            }
                        }
                    })
                    .await;
            }));
        }
        for f in followers {
            f.await.unwrap();
        }

        // Release the first sweep; the coordinator now drains the single coalesced
        // re-arm.
        release.notify_one();
        leader.await.unwrap();

        let total = runs.load(Ordering::SeqCst);
        assert!(
            (1..=2).contains(&total),
            "10 concurrent triggers must coalesce to at most 2 sweeps, ran {total}"
        );
        assert!(total >= 1, "the sweep must run at least once");
    }

    /// A follower does NOT wait for the in-flight sweep — it arms the pending bit
    /// and returns immediately.
    ///
    /// This is the ordering the gate changed: a direct `resync_open_files().await`
    /// did not return until the sweep finished, so a caller that awaited it (such
    /// as background init before committing its snapshot) observed the swept state.
    /// A follower no longer does. Callers that need the swept state must be the
    /// runner or re-trigger afterwards.
    #[tokio::test]
    async fn a_follower_returns_without_waiting_for_the_in_flight_sweep() {
        let coordinator = Arc::new(ResyncCoordinator::new());
        let runs = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let leader_sweep_finished = Arc::new(AtomicBool::new(false));

        let leader = {
            let coordinator = Arc::clone(&coordinator);
            let runs = Arc::clone(&runs);
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            let leader_sweep_finished = Arc::clone(&leader_sweep_finished);
            tokio::spawn(async move {
                coordinator
                    .resync(|| {
                        let runs = Arc::clone(&runs);
                        let started = Arc::clone(&started);
                        let release = Arc::clone(&release);
                        let leader_sweep_finished = Arc::clone(&leader_sweep_finished);
                        async move {
                            // Only the FIRST sweep parks; the coalesced re-run the
                            // follower arms must not deadlock on an unheld release.
                            if runs.fetch_add(1, Ordering::SeqCst) == 0 {
                                started.notify_one();
                                release.notified().await;
                                leader_sweep_finished.store(true, Ordering::SeqCst);
                            }
                        }
                    })
                    .await;
            })
        };
        started.notified().await;

        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            coordinator.resync(|| async {}),
        )
        .await
        .expect("a follower must not block on the in-flight sweep");
        assert!(
            !leader_sweep_finished.load(Ordering::SeqCst),
            "the follower must return BEFORE the in-flight sweep completes — that is the \
             ordering this gate changed"
        );

        release.notify_one();
        leader.await.unwrap();
        assert!(
            leader_sweep_finished.load(Ordering::SeqCst),
            "the leader's parked sweep must still complete after release"
        );
    }

    /// A sweep that PANICS must not leave the gate permanently armed. Without an
    /// RAII release the `running` flag stays set forever, every later trigger
    /// returns early, and provider resync is silently dead for the whole session.
    #[tokio::test]
    async fn a_panicking_sweep_does_not_wedge_the_gate_forever() {
        let coordinator = Arc::new(ResyncCoordinator::new());
        let runs = Arc::new(AtomicUsize::new(0));

        let outcome = {
            let coordinator = Arc::clone(&coordinator);
            tokio::spawn(async move {
                coordinator
                    .resync(|| async { panic!("sweep failed mid-flight") })
                    .await;
            })
            .await
        };
        assert!(
            outcome.is_err(),
            "the sweep must actually panic, else this test proves nothing"
        );

        let sweep = || {
            let runs = Arc::clone(&runs);
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
            }
        };
        coordinator.resync(sweep).await;
        assert_eq!(
            runs.load(Ordering::SeqCst),
            1,
            "a trigger after a panicking sweep must still run: the gate must not stay armed"
        );
    }

    /// A runner future DROPPED mid-sweep (a cancelled background-init generation)
    /// must not leave the gate armed either.
    #[tokio::test]
    async fn a_cancelled_runner_does_not_wedge_the_gate_forever() {
        let coordinator = Arc::new(ResyncCoordinator::new());
        let runs = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(tokio::sync::Notify::new());

        let runner = {
            let coordinator = Arc::clone(&coordinator);
            let started = Arc::clone(&started);
            tokio::spawn(async move {
                coordinator
                    .resync(|| {
                        let started = Arc::clone(&started);
                        async move {
                            started.notify_one();
                            // Park forever: only cancellation ends this sweep.
                            std::future::pending::<()>().await;
                        }
                    })
                    .await;
            })
        };
        started.notified().await;
        runner.abort();
        assert!(
            runner.await.is_err(),
            "the runner must actually be cancelled, else this test proves nothing"
        );

        let sweep = || {
            let runs = Arc::clone(&runs);
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), coordinator.resync(sweep))
            .await
            .expect("a trigger after a cancelled runner must not block on the dead gate");
        assert_eq!(
            runs.load(Ordering::SeqCst),
            1,
            "a trigger after a cancelled runner must still run: the gate must not stay armed"
        );
    }

    /// A trigger that arrives AFTER a sweep finished starts a fresh sweep (the
    /// coordinator does not permanently suppress later work).
    #[tokio::test]
    async fn a_later_trigger_runs_again() {
        let coordinator = Arc::new(ResyncCoordinator::new());
        let runs = Arc::new(AtomicUsize::new(0));

        let sweep = || {
            let runs = Arc::clone(&runs);
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
            }
        };
        coordinator.resync(sweep).await;
        assert_eq!(runs.load(Ordering::SeqCst), 1);

        let sweep2 = || {
            let runs = Arc::clone(&runs);
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
            }
        };
        coordinator.resync(sweep2).await;
        assert_eq!(
            runs.load(Ordering::SeqCst),
            2,
            "a fresh trigger after the gate settled must run again"
        );
    }
}

//! Project-level singleflight for full-open-file provider re-syncs.
//!
//! `resync_open_files` closes and re-opens EVERY open document in the type
//! provider — an O(open-files) burst of provider traffic. Background init fires
//! it up to twice per pass, and a workspace-folder / tsconfig / watcher event can
//! fire it again concurrently. With no coalescing, N overlapping triggers become
//! N full close+reopen sweeps stacked on the interactive lane — the resync storm
//! that starves interactive requests (probe finding #1's traffic burst).
//!
//! [`ResyncCoordinator`] collapses a storm to AT MOST one in-flight sweep plus
//! one coalesced re-arm: every trigger that arrives while a sweep runs is folded
//! into a single pending bit, so 10 concurrent triggers run the sweep at most
//! twice (the in-flight one, then one re-run that reflects the latest state).
//! The per-document IDE-sync repair lease is per-DOCUMENT; this is the project-level counterpart the
//! resync path lacked.

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
    ///   it ran, it runs `sweep` ONE more time (folding all of them together).
    /// - If a sweep is already running, this caller only arms the pending bit and
    ///   returns immediately; the active runner will honor it.
    ///
    /// Net effect: a burst of N concurrent triggers performs the sweep at most
    /// twice, and never concurrently.
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

        // We own the run. Drain the pending bit until it stays clear, re-checking
        // once after releasing `running` to close the race with a trigger that
        // arms `pending` between our last drain and the `running` release.
        loop {
            while self.pending.swap(false, Ordering::SeqCst) {
                sweep().await;
            }
            self.running.store(false, Ordering::SeqCst);
            // A trigger may have armed `pending` after our last `swap(false)` but
            // before we cleared `running`; reclaim the run to honor it, else stop.
            if self.pending.load(Ordering::SeqCst) && !self.running.swap(true, Ordering::SeqCst) {
                continue;
            }
            break;
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

    /// T4-core — a burst of concurrent resync triggers coalesces to AT MOST two
    /// sweeps (one in-flight + one re-arm), never N. Without this gate every trigger runs
    /// a full sweep.
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

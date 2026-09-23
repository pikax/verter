//! Activity gate for the semantic substrate's close-time payload release.
//!
//! Semantic node ids are plain ordinals, read by id through
//! [`crate::semantic_query_memo::SemanticGraphStore::node_data`] from many
//! places, and a computation commonly reads the same id more than once —
//! classify it on one read, then take a shape-specific accessor on a later
//! one. When a document closes, the store releases the nodes it interned for
//! that document: each released slot drops its payload and reads as the
//! `Opaque(Miss)` placeholder from then on. If that switch lands BETWEEN two
//! reads of one computation, the second read sees a different node than the
//! first, and a shape-specific accessor that the first read justified fails
//! (the WSP6 churn lane hit exactly this: a carrier normaliser panicked on a
//! worker thread, the worker pool lost a thread, and the scheduler stalled).
//!
//! So the payload switch never runs while a computation is in flight. A close
//! does what is safe immediately — the edit-path memo drain, which only
//! removes cache entries readers hold by `Arc` — and QUEUES the payload
//! release with the arena's current size as a watermark, so nodes interned
//! after the close (the reloaded content) are never taken by it. The queue is
//! applied the moment no [`SemanticActivityGuard`] is alive: inline when the
//! close itself runs outside any guard (a batch host, a test), or when the
//! last guard of an interactive session drops.
//!
//! The check-and-apply is atomic against guard creation (a Dekker pair on two
//! `SeqCst` atomics): either the reclaimer sees a guard and backs off, or the
//! guard sees the reclaimer and waits for it to finish before its computation
//! starts. Guards are plain counters — `Send`, re-entrant, and free to be held
//! across an `.await`, which only delays the release.
//!
//! **Starvation is bounded.** The gate is reader-preferential: a release
//! waits for a zero-reader instant, and a busy editor whose computations
//! always overlap never offers one on its own. So a release that has waited
//! [`DRAIN_AFTER`] flips the gate into a drain: each computation arriving
//! after that waits for the readers in flight to end (at most
//! [`DRAIN_ADMISSION_BOUND`], see [`SemanticActivityGate::admit`]) and applies
//! the release itself at the zero-reader instant this opens. A reader that
//! outlives the bound is work, not starvation: the entrant proceeds and the
//! release lands when that reader's guard drops. Below the threshold the gate
//! stays lazy, so a close costs an interactive request nothing; the retention
//! snapshot reports the longest wait and every drain (`SemanticReclaimStats`).
//!
//! Every path into the host that can read semantic nodes must hold a guard for
//! the whole call. The language server takes one per host access
//! ([`crate::GuardedHost`], its `SharedHost`); work a guarded caller fans out
//! to the CPU pool is covered because the caller blocks on it; and the
//! scheduler jobs that DO outlive a host call (the close-time background
//! reload, dependency auto-ingest loads, a superseded load finishing on its
//! worker) never read semantic nodes: they run the Source/Analysis/Artifact
//! stages, which build parse snapshots and envelopes only. Which consumers
//! must hold the handle, and that those stages stay clear of the graph, is
//! pinned by the crate's `semantic_activity_boundary` test.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use crate::semantic_query_memo::SemanticReleaseReport;

/// A queued release that has waited this long behind readers that never left
/// a zero-reader instant is starving: the gate then drains (module docs).
pub const DRAIN_AFTER: Duration = Duration::from_millis(100);
/// The most one arriving computation waits for the readers in flight to end
/// while the gate drains. The bound is what keeps a drain from ever
/// deadlocking: an entrant that a reader in flight is itself waiting on (a
/// nested host call from a pool worker) proceeds once it expires.
pub const DRAIN_ADMISSION_BOUND: Duration = Duration::from_millis(50);

/// One queued close: the canonical to release and the arena watermark taken
/// at the close (only nodes below it are released).
#[derive(Debug)]
struct PendingRelease {
    canonical: Arc<str>,
    below: u64,
    /// When the close queued it; the wait until application is the
    /// starvation figure the retention snapshot reports.
    queued_at: Instant,
}

/// What the gate has applied so far and how long queued releases waited
/// (retention observability; read through
/// [`crate::project_type_store::ProjectTypeStore::deferred_release_stats`]).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SemanticReclaimStats {
    /// Queued releases applied over the store's lifetime.
    pub releases_applied: u64,
    /// The longest a queued release waited for a zero-reader instant, in
    /// microseconds. A reader-preferential gate can in principle starve a
    /// release; this is the figure that says whether it did.
    pub wait_max_micros: u64,
    /// The slowest single release, and every release added up, in
    /// microseconds: the cost the O(live nodes) close-time scan actually
    /// charged.
    pub elapsed_max_micros: u64,
    pub elapsed_total_micros: u64,
    /// Times the gate drained (a release had waited [`DRAIN_AFTER`]), and
    /// the longest an arriving computation waited for a drain. Zero on a
    /// workload whose computations leave zero-reader instants of their own.
    pub drains: u64,
    pub drain_wait_max_micros: u64,
    /// The last release applied, or `None` before the first.
    pub last: Option<SemanticReleaseReceipt>,
}

/// One applied close-time release: its wait, its cost and what it found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticReleaseReceipt {
    pub wait_micros: u64,
    pub elapsed_micros: u64,
    pub nodes_scanned: usize,
    pub nodes_released: usize,
    pub storage_slots_before: usize,
    pub storage_slots_after: usize,
    pub memo_entries_evicted: usize,
}

fn micros(elapsed: std::time::Duration) -> u64 {
    u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX)
}

/// The gate: live-guard count, the in-progress reclaim flag, and the queue.
#[derive(Debug, Default)]
pub(crate) struct SemanticActivityGate {
    active: AtomicUsize,
    reclaiming: AtomicBool,
    /// Serialises reclaimers. A second reclaimer waits for the first and
    /// re-checks rather than skipping (see [`Self::try_reclaim`]).
    reclaim: parking_lot::Mutex<()>,
    pending: parking_lot::Mutex<Vec<PendingRelease>>,
    has_pending: AtomicBool,
    /// Set while an arriving computation waits for the readers in flight
    /// (observability, and the handoff the drain tests key on).
    draining: AtomicBool,
    stats: parking_lot::Mutex<SemanticReclaimStats>,
}

/// A live computation on the semantic substrate. While any guard is alive,
/// queued close-time payload releases wait; the last guard to drop applies
/// them.
#[must_use = "the guard covers the computation only while it is alive"]
pub struct SemanticActivityGuard {
    store: Arc<crate::project_type_store::ProjectTypeStore>,
}

impl std::fmt::Debug for SemanticActivityGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticActivityGuard")
            .finish_non_exhaustive()
    }
}

impl SemanticActivityGate {
    /// Register one computation, waiting out a reclaim that is already
    /// applying queued releases.
    fn enter(&self) {
        loop {
            self.active.fetch_add(1, Ordering::SeqCst);
            if !self.reclaiming.load(Ordering::SeqCst) {
                return;
            }
            // A reclaim saw `active == 0` before this increment and is
            // switching payloads now: step back out and wait for it.
            self.active.fetch_sub(1, Ordering::SeqCst);
            while self.reclaiming.load(Ordering::SeqCst) {
                std::thread::yield_now();
            }
        }
    }

    /// Unregister one computation; `true` when it was the last one and a
    /// release is queued (the caller then attempts the reclaim).
    fn exit(&self) -> bool {
        self.active.fetch_sub(1, Ordering::SeqCst) == 1 && self.has_pending.load(Ordering::SeqCst)
    }

    fn enqueue(&self, canonical: &str, below: u64) {
        self.pending.lock().push(PendingRelease {
            canonical: Arc::from(canonical),
            below,
            queued_at: Instant::now(),
        });
        self.has_pending.store(true, Ordering::SeqCst);
    }

    /// Number of queued releases not yet applied (observability, tests).
    pub(crate) fn pending_count(&self) -> usize {
        self.pending.lock().len()
    }

    pub(crate) fn stats(&self) -> SemanticReclaimStats {
        *self.stats.lock()
    }

    /// Whether the oldest queued release has waited past [`DRAIN_AFTER`].
    fn starving(&self) -> bool {
        self.has_pending.load(Ordering::SeqCst)
            && self
                .pending
                .lock()
                .first()
                .is_some_and(|release| release.queued_at.elapsed() >= DRAIN_AFTER)
    }

    /// Wait for the readers in flight to end, at most `bound`; `true` when
    /// a zero-reader instant was seen. Never waits on a single-threaded
    /// target: a reader in flight there is the caller's own frame.
    fn wait_for_drain(&self, bound: Duration) -> bool {
        if cfg!(target_arch = "wasm32") {
            return self.active.load(Ordering::SeqCst) == 0;
        }
        struct Draining<'a>(&'a AtomicBool);
        impl Drop for Draining<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::SeqCst);
            }
        }
        self.draining.store(true, Ordering::SeqCst);
        let _draining = Draining(&self.draining);
        let started = Instant::now();
        loop {
            if self.active.load(Ordering::SeqCst) == 0 {
                return true;
            }
            if started.elapsed() >= bound {
                return false;
            }
            std::thread::yield_now();
        }
    }

    /// Register a computation. A starving release (module docs) is drained
    /// first: the entrant waits for the readers in flight, applies the queue
    /// at the zero-reader instant, then enters. `apply` is the store's
    /// release, as for [`Self::try_reclaim`].
    fn admit(&self, apply: &dyn Fn(&str, u64) -> Option<SemanticReleaseReport>) {
        if self.starving() {
            let started = Instant::now();
            if self.wait_for_drain(DRAIN_ADMISSION_BOUND) {
                let _ = self.try_reclaim(apply);
            }
            let waited = micros(started.elapsed());
            let mut stats = self.stats.lock();
            stats.drains += 1;
            stats.drain_wait_max_micros = stats.drain_wait_max_micros.max(waited);
        }
        self.enter();
    }

    fn record(&self, wait_micros: u64, report: &SemanticReleaseReport) {
        let mut stats = self.stats.lock();
        stats.releases_applied += 1;
        stats.wait_max_micros = stats.wait_max_micros.max(wait_micros);
        stats.elapsed_max_micros = stats.elapsed_max_micros.max(report.elapsed_micros);
        stats.elapsed_total_micros = stats
            .elapsed_total_micros
            .saturating_add(report.elapsed_micros);
        stats.last = Some(SemanticReleaseReceipt {
            wait_micros,
            elapsed_micros: report.elapsed_micros,
            nodes_scanned: report.nodes_scanned,
            nodes_released: report.nodes_released,
            storage_slots_before: report.storage_slots_before,
            storage_slots_after: report.storage_slots_after,
            memo_entries_evicted: report.memo_entries_evicted,
        });
    }

    /// Apply every queued release iff no computation is in flight. Returns
    /// the queued releases applied (0 when a guard is alive — the last guard
    /// to drop retries — or when nothing is queued).
    ///
    /// A reclaimer that finds another one running WAITS for it and re-checks
    /// instead of skipping. Skipping could strand the queue: a closer's
    /// reclaim reads `active` just before the last guard exits, and the last
    /// guard's own attempt — the one that would have seen `active == 0` —
    /// finds the lock held and leaves. Waiting cannot deadlock: the running
    /// reclaimer never waits on a guard, and a waiter holds none it could be
    /// waiting on (it is either the last guard, already exited, or a closer
    /// whose own guard makes the running reclaimer back off at once).
    fn try_reclaim(&self, apply: &dyn Fn(&str, u64) -> Option<SemanticReleaseReport>) -> usize {
        if !self.has_pending.load(Ordering::SeqCst) {
            return 0;
        }
        let _serial = self.reclaim.lock();
        if !self.has_pending.load(Ordering::SeqCst) {
            return 0;
        }
        self.reclaiming.store(true, Ordering::SeqCst);
        // Reset the flag on every exit, including an unwinding release, so a
        // failed reclaim can never leave new computations spinning.
        struct Reset<'a>(&'a AtomicBool);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::SeqCst);
            }
        }
        let _reset = Reset(&self.reclaiming);
        if self.active.load(Ordering::SeqCst) != 0 {
            return 0;
        }
        let batch = {
            let mut pending = self.pending.lock();
            self.has_pending.store(false, Ordering::SeqCst);
            std::mem::take(&mut *pending)
        };
        for release in &batch {
            let wait_micros = micros(release.queued_at.elapsed());
            if let Some(report) = apply(&release.canonical, release.below) {
                self.record(wait_micros, &report);
            }
        }
        batch.len()
    }
}

impl crate::project_type_store::ProjectTypeStore {
    /// Register a computation that may read semantic nodes. Hold the guard
    /// for the whole host call; see the module docs.
    pub fn semantic_activity(self: &Arc<Self>) -> SemanticActivityGuard {
        self.activity_gate().admit(&self.release_applier());
        SemanticActivityGuard {
            store: Arc::clone(self),
        }
    }

    /// The gate's view of a release: apply one queued close to this store,
    /// if the store is still alive.
    fn release_applier(self: &Arc<Self>) -> impl Fn(&str, u64) -> Option<SemanticReleaseReport> {
        let weak: Weak<Self> = Arc::downgrade(self);
        move |canonical, below| {
            weak.upgrade()
                .map(|store| store.release_canonical_below(canonical, below))
        }
    }

    /// A document closed: drain what is safe now and queue the payload
    /// release, applied as soon as no computation is in flight (inline when
    /// none is). Returns whether the release was applied before returning.
    pub fn defer_release_canonical(self: &Arc<Self>, canonical_id: &str) -> bool {
        // Safe immediately: the edit-path drain removes memo entries (held by
        // `Arc` by any reader) and the arena's dedup entries (a later intern
        // mints a fresh id); no payload changes.
        let _ = self.semantic_graph().invalidate_canonical(canonical_id);
        let below = self.semantic_graph().node_slot_count() as u64;
        self.activity_gate().enqueue(canonical_id, below);
        self.try_apply_deferred_releases() > 0
    }

    /// Apply queued close-time releases iff no computation is in flight.
    pub fn try_apply_deferred_releases(self: &Arc<Self>) -> usize {
        self.activity_gate().try_reclaim(&self.release_applier())
    }

    /// Queued close-time releases not yet applied.
    #[must_use]
    pub fn deferred_release_count(&self) -> usize {
        self.activity_gate().pending_count()
    }

    /// What the gate has applied so far, how long releases waited, and the
    /// last release's own cost.
    #[must_use]
    pub fn deferred_release_stats(&self) -> SemanticReclaimStats {
        self.activity_gate().stats()
    }
}

impl Drop for SemanticActivityGuard {
    fn drop(&mut self) {
        if self.store.activity_gate().exit() {
            let _ = self.store.try_apply_deferred_releases();
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// A gate whose one queued release has already waited past [`DRAIN_AFTER`].
    fn gate_with_starving_release() -> SemanticActivityGate {
        let gate = SemanticActivityGate::default();
        gate.pending.lock().push(PendingRelease {
            canonical: Arc::from("/a.ts"),
            below: u64::MAX,
            queued_at: Instant::now() - DRAIN_AFTER - Duration::from_millis(1),
        });
        gate.has_pending.store(true, Ordering::SeqCst);
        gate
    }

    fn counting_apply(
        applied: &Arc<AtomicUsize>,
    ) -> impl Fn(&str, u64) -> Option<SemanticReleaseReport> {
        let applied = Arc::clone(applied);
        move |_, _| {
            applied.fetch_add(1, Ordering::SeqCst);
            Some(SemanticReleaseReport::default())
        }
    }

    /// A reader in flight ends once a computation arriving at a gate whose
    /// release is starving has begun to wait for it: the entrant applies the
    /// release at the zero-reader instant this opens, and enters (an
    /// application proves the instant came within the bound, since an expired
    /// bound skips the reclaim). Discriminating: the lazy path alone (the
    /// last guard's exit) belongs to the reader thread, which never attempts
    /// a reclaim here, so without the drain the release would stay queued.
    #[test]
    fn a_starving_release_lands_when_the_readers_in_flight_end() {
        let gate = Arc::new(gate_with_starving_release());
        let applied = Arc::new(AtomicUsize::new(0));
        gate.enter();
        let reader = {
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || {
                while !gate.draining.load(Ordering::SeqCst) {
                    std::thread::yield_now();
                }
                let _ = gate.exit();
            })
        };
        gate.admit(&counting_apply(&applied));
        reader.join().expect("reader thread");
        assert_eq!(
            applied.load(Ordering::SeqCst),
            1,
            "the drain applied the release at the zero-reader instant the reader's exit opened"
        );
        assert_eq!(gate.pending_count(), 0);
        let stats = gate.stats();
        assert_eq!(stats.drains, 1);
        assert_eq!(stats.releases_applied, 1);
        assert!(stats.wait_max_micros >= u64::try_from(DRAIN_AFTER.as_micros()).expect("fits"));
        assert_eq!(
            gate.active.load(Ordering::SeqCst),
            1,
            "the entrant is in flight"
        );
        let _ = gate.exit();
    }

    /// A reader that outlives the bound is work the entrant must not wait
    /// for: admission proceeds after the bound with the release still queued
    /// (the reader's own exit lands it), so a nested host call from a pool
    /// worker the reader is blocked on can never deadlock the gate.
    #[test]
    fn a_reader_that_outlives_the_bound_is_not_waited_for() {
        let gate = gate_with_starving_release();
        let applied = Arc::new(AtomicUsize::new(0));
        gate.enter();
        let started = Instant::now();
        gate.admit(&counting_apply(&applied));
        let waited = started.elapsed();
        assert!(
            waited >= DRAIN_ADMISSION_BOUND,
            "the entrant waited the bound out"
        );
        assert!(waited < DRAIN_ADMISSION_BOUND * 4, "and no longer");
        assert_eq!(
            applied.load(Ordering::SeqCst),
            0,
            "nothing was applied under a live reader"
        );
        assert_eq!(
            gate.pending_count(),
            1,
            "the release stays queued for the last guard"
        );
        assert_eq!(gate.active.load(Ordering::SeqCst), 2);
        assert_eq!(gate.stats().drains, 1);
        let _ = gate.exit();
        let _ = gate.exit();
    }

    /// Below the threshold the gate stays lazy: a computation arriving over
    /// a fresh release enters at once, whatever is in flight.
    #[test]
    fn a_fresh_release_costs_an_arriving_computation_nothing() {
        let gate = SemanticActivityGate::default();
        gate.enqueue("/a.ts", u64::MAX);
        gate.enter();
        let applied = Arc::new(AtomicUsize::new(0));
        let started = Instant::now();
        gate.admit(&counting_apply(&applied));
        assert!(
            started.elapsed() < DRAIN_AFTER,
            "no drain below the threshold"
        );
        assert_eq!(gate.stats().drains, 0);
        assert_eq!(applied.load(Ordering::SeqCst), 0);
        assert_eq!(gate.pending_count(), 1);
        let _ = gate.exit();
        let _ = gate.exit();
    }
}

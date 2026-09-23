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
//! Every path into the host that can read semantic nodes must hold a guard for
//! the whole call: the language server takes one per host access
//! (`verter_lsp`'s guarded host handle), and work a guarded caller fans out to
//! the CPU pool is covered because the caller blocks on it.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};

/// One queued close: the canonical to release and the arena watermark taken
/// at the close (only nodes below it are released).
#[derive(Debug)]
struct PendingRelease {
    canonical: Arc<str>,
    below: u64,
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
        });
        self.has_pending.store(true, Ordering::SeqCst);
    }

    /// Number of queued releases not yet applied (observability, tests).
    pub(crate) fn pending_count(&self) -> usize {
        self.pending.lock().len()
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
    fn try_reclaim(&self, apply: &dyn Fn(&str, u64)) -> usize {
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
            apply(&release.canonical, release.below);
        }
        batch.len()
    }
}

impl crate::project_type_store::ProjectTypeStore {
    /// Register a computation that may read semantic nodes. Hold the guard
    /// for the whole host call; see the module docs.
    pub fn semantic_activity(self: &Arc<Self>) -> SemanticActivityGuard {
        self.activity_gate().enter();
        SemanticActivityGuard {
            store: Arc::clone(self),
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
        let weak: Weak<Self> = Arc::downgrade(self);
        self.activity_gate().try_reclaim(&move |canonical, below| {
            if let Some(store) = weak.upgrade() {
                let _ = store.release_canonical_below(canonical, below);
            }
        })
    }

    /// Queued close-time releases not yet applied.
    #[must_use]
    pub fn deferred_release_count(&self) -> usize {
        self.activity_gate().pending_count()
    }
}

impl Drop for SemanticActivityGuard {
    fn drop(&mut self) {
        if self.store.activity_gate().exit() {
            let _ = self.store.try_apply_deferred_releases();
        }
    }
}

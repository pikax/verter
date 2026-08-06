//! A generation counter whose advance BRACKETS the mutation it
//! describes.
//!
//! ## The gap this closes
//!
//! A compaction domain's terminal aggregate claims "every precise fact
//! this scope observed in the domain held as of generation `N`". The
//! claim is only true if no reader can ever observe a MUTATED store
//! beside an UNMOVED generation.
//!
//! A naive post-mutation `fetch_add` cannot promise that. Between the
//! store write landing and the counter moving there is a window in
//! which the store already holds the new membership while the counter
//! still reads `N`. A scope that installs its basis, reads the store
//! and finalises entirely inside that window snapshots `N`, re-reads
//! `N`, detects no movement — and admits an aggregate asserting the
//! domain held at `N` over facts it read from the `N + 1` world. That
//! is a stale serve over a whole domain at once, and no amount of
//! re-reading afterwards finds it.
//!
//! ## The protocol
//!
//! The counter is ODD for exactly as long as a mutation is in flight,
//! and EVEN otherwise. [`BracketedGeneration::stable`] hands out a stamp
//! only from the even state, so "a mutation is running" is a state a
//! reader can observe rather than a race it can lose. A mutation that
//! reports a membership change leaves the counter two higher; one that
//! reports no change restores the value it entered with.
//!
//! An installer that snapshots `Some(g)` and a finaliser that re-reads
//! `Some(g)` therefore prove no membership-changing mutation ran
//! between them: any such mutation would have had to pass through the
//! odd window and leave a different even value behind.
//!
//! ## Writers are serialised, and that is load-bearing
//!
//! Two concurrent `fetch_add`s would make the counter EVEN in the
//! middle of both mutations — recreating the stable-looking window this
//! type exists to eliminate. The writer lock is what makes the odd/even
//! discipline well-defined; it is not incidental mutual exclusion. It
//! is held only across the mutation the counter describes, never across
//! resolution or I/O.

use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

/// A monotonic domain generation whose advance brackets its mutation.
///
/// See the module documentation for the gap this closes and why writers
/// are serialised.
#[derive(Debug, Default)]
pub(crate) struct BracketedGeneration {
    /// ODD while a mutation is in flight, EVEN and readable otherwise.
    /// A membership-changing mutation leaves it two higher; a no-change
    /// mutation restores it.
    seq: AtomicU64,
    /// Serialises writers so the odd/even discipline holds. Held only
    /// across the mutation body.
    writer: Mutex<()>,
}

impl BracketedGeneration {
    /// The current stable generation, or `None` while a mutation is in
    /// flight.
    ///
    /// `None` is not a failure — it is the honest answer that no stamp
    /// can be vouched for right now. A basis installer that receives it
    /// simply leaves the domain absent, so the domain stays precise and
    /// nothing compacts. That is the same fail-safe direction as a
    /// domain with no producer at all.
    #[must_use]
    pub(crate) fn stable(&self) -> Option<u64> {
        let seq = self.seq.load(Ordering::Acquire);
        seq.is_multiple_of(2).then_some(seq)
    }

    /// Run `mutation` inside the in-flight window.
    ///
    /// `mutation` returns `(value, changed)`. `changed` must be `true`
    /// exactly when the mutation altered membership that a recorded
    /// fact could depend on — a refused admission and a genuine
    /// identical-candidate skip both report `false`, because advancing
    /// for them would refuse every concurrent reader's compaction while
    /// describing nothing.
    ///
    /// On unwind the generation ADVANCES. The store's membership is
    /// unknown at that point, so claiming a new generation (every
    /// spanning reader refuses) is the conservative direction; restoring
    /// would vouch for a state nobody verified, and leaving the counter
    /// odd would disarm the domain for the process's lifetime.
    pub(crate) fn mutate<R>(&self, mutation: impl FnOnce() -> (R, bool)) -> R {
        let _writer = self.writer.lock();
        self.seq.fetch_add(1, Ordering::AcqRel);
        // Defaults to ADVANCE so an unwind through `mutation` leaves the
        // counter stable-and-moved rather than wedged odd.
        let mut guard = ExitGuard {
            seq: &self.seq,
            changed: true,
        };
        let (value, changed) = mutation();
        guard.changed = changed;
        drop(guard);
        value
    }
}

/// Leaves the counter EVEN however the mutation body exits.
struct ExitGuard<'a> {
    seq: &'a AtomicU64,
    changed: bool,
}

impl Drop for ExitGuard<'_> {
    fn drop(&mut self) {
        if self.changed {
            self.seq.fetch_add(1, Ordering::AcqRel);
        } else {
            self.seq.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

#[cfg(test)]
#[path = "bracketed_generation_tests.rs"]
mod bracketed_generation_tests;

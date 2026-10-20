//! Shared cache-occupancy counter handle.
//!
//! Several cache stores report their live-entry and sweep counts into
//! counters one owner aggregates (the project store's counter snapshot).
//! No production decision reads those counts: they exist so tests and
//! measurement harnesses can see memory and coherence behaviour. The handle
//! therefore keeps a shared atomic only under the observation gate — this
//! crate's tests, its `test-support` feature (dependents' tests), or
//! `semantic-observe`. Everywhere else it is zero-size and every write
//! compiles to nothing.

/// A cloneable handle onto one shared occupancy count.
///
/// Clones share the count. The reader exists only where the count is kept,
/// so a default build cannot substitute zero for a measurement it never
/// took.
#[derive(Clone, Default)]
pub struct OccupancyCounter {
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    count: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl std::fmt::Debug for OccupancyCounter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut f = f.debug_struct("OccupancyCounter");
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        f.field("count", &self.load(std::sync::atomic::Ordering::Relaxed));
        f.finish()
    }
}

impl OccupancyCounter {
    /// A fresh count at zero, shared by nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `amount`. A no-op outside the observation gate.
    #[inline(always)]
    pub fn fetch_add(&self, amount: u64, order: std::sync::atomic::Ordering) {
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        self.count.fetch_add(amount, order);
        #[cfg(not(any(test, feature = "test-support", feature = "semantic-observe")))]
        let _ = (amount, order);
    }

    /// Subtract `amount`, wrapping like [`std::sync::atomic::AtomicU64`]. A
    /// no-op outside the observation gate.
    #[inline(always)]
    pub fn fetch_sub(&self, amount: u64, order: std::sync::atomic::Ordering) {
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        self.count.fetch_sub(amount, order);
        #[cfg(not(any(test, feature = "test-support", feature = "semantic-observe")))]
        let _ = (amount, order);
    }

    /// Subtract `amount`, clamped so the count never wraps below zero. A
    /// shared count several stores decrement can otherwise underflow when a
    /// store drops more entries than it ever reported. Relaxed, like every
    /// other occupancy write. A no-op outside the observation gate.
    #[inline(always)]
    pub fn saturating_sub(&self, amount: u64) {
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        {
            use std::sync::atomic::Ordering::Relaxed;
            self.count
                .fetch_sub(amount.min(self.count.load(Relaxed)), Relaxed);
        }
        #[cfg(not(any(test, feature = "test-support", feature = "semantic-observe")))]
        let _ = amount;
    }

    /// Read the count. Exists only where the count is kept.
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    #[must_use]
    pub fn load(&self, order: std::sync::atomic::Ordering) -> u64 {
        self.count.load(order)
    }
}

//! Counter table and reader. Compiled only under `attribution` — without
//! it there is no table, no [`snapshot`]/[`reset`]/`record_*`, so a
//! production build cannot branch on a counter. See [`super`].

use std::sync::atomic::{AtomicU64, Ordering};

use super::schema::{WorkDomain, WorkSite, WorkUnit};

/// One site's accumulators. `Relaxed` atomics: counters never order
/// program state, so a hit is one uncontended `lock xadd`.
pub(super) struct SiteCell {
    /// Number of times the site was hit.
    pub(super) calls: AtomicU64,
    /// Unit-dependent quantity — a sum, or a maximum for a gauge site.
    pub(super) amount: AtomicU64,
    /// Inclusive wall-clock nanoseconds, populated by scope guards.
    pub(super) nanos: AtomicU64,
    /// Order-independent fold of the values reported to this site.
    pub(super) digest: AtomicU64,
    /// Heap allocations made while this site was the innermost open
    /// scope.
    pub(super) alloc_count: AtomicU64,
    /// Heap bytes requested while this site was the innermost open
    /// scope.
    pub(super) alloc_bytes: AtomicU64,
    /// Heap bytes released while this site was the innermost open
    /// scope. `alloc_bytes - dealloc_bytes` is the site's net retention
    /// contribution.
    pub(super) dealloc_bytes: AtomicU64,
}

impl SiteCell {
    const fn new() -> Self {
        Self {
            calls: AtomicU64::new(0),
            amount: AtomicU64::new(0),
            nanos: AtomicU64::new(0),
            digest: AtomicU64::new(0),
            alloc_count: AtomicU64::new(0),
            alloc_bytes: AtomicU64::new(0),
            dealloc_bytes: AtomicU64::new(0),
        }
    }
}

pub(super) static CELLS: [SiteCell; WorkSite::COUNT] = [const { SiteCell::new() }; WorkSite::COUNT];

#[inline]
pub(super) fn cell(site: WorkSite) -> &'static SiteCell {
    // `WorkSite::index()` is the dense declaration ordinal and `CELLS`
    // is sized by `WorkSite::COUNT`, so this is in bounds by
    // construction.
    &CELLS[site.index()]
}

/// Record one hit with no associated quantity.
#[inline]
pub fn record_call(site: WorkSite) {
    cell(site).calls.fetch_add(1, Ordering::Relaxed);
}

/// Record one hit carrying `amount` in the site's declared unit.
///
/// For a [`WorkUnit::Gauge`] site the column keeps the running maximum
/// instead of a sum.
#[inline]
pub fn record_amount(site: WorkSite, amount: u64) {
    let cell = cell(site);
    cell.calls.fetch_add(1, Ordering::Relaxed);
    if site.unit().is_gauge() {
        cell.amount.fetch_max(amount, Ordering::Relaxed);
    } else {
        cell.amount.fetch_add(amount, Ordering::Relaxed);
    }
}

/// Record `nanos` of inclusive wall-clock against `site`, plus one hit.
#[inline]
pub fn record_scope(site: WorkSite, nanos: u64) {
    let cell = cell(site);
    cell.calls.fetch_add(1, Ordering::Relaxed);
    cell.nanos.fetch_add(nanos, Ordering::Relaxed);
}

/// Fold `value` into the site's determinism digest.
///
/// The fold is `wrapping_add` over a bit-mixed value, which is
/// commutative and associative: two runs that produce the same MULTISET
/// of observations agree on the digest regardless of the order the
/// threads reported them. That is exactly the property a determinism
/// comparison needs, and it is why the fold is not a hash chain.
#[inline]
pub fn record_digest(site: WorkSite, value: u64) {
    let cell = cell(site);
    cell.calls.fetch_add(1, Ordering::Relaxed);
    cell.digest.fetch_add(mix64(value), Ordering::Relaxed);
}

/// SplitMix64 finaliser — spreads low-entropy inputs (small integers,
/// lengths) across the whole word before they are summed, so the fold
/// does not collapse structurally different observation multisets.
#[inline]
pub(super) const fn mix64(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// One site's accumulated values at a point in time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SiteSample {
    /// Which site produced the row.
    pub site: WorkSite,
    /// Hits.
    pub calls: u64,
    /// Sum (or maximum, for a gauge) in the site's declared unit.
    pub amount: u64,
    /// Summed INCLUSIVE wall-clock nanoseconds from scope guards.
    ///
    /// Not additive across sites, and not a share of wall clock: a site
    /// that re-enters itself records the full inclusive interval once
    /// per open frame, so the column double-counts by recursion depth
    /// and can exceed the run's total wall clock. See
    /// [`WorkUnit::Nanoseconds`](super::WorkUnit::Nanoseconds).
    pub nanos: u64,
    /// Order-independent determinism fold.
    pub digest: u64,
    /// Allocations attributed to this site as innermost open scope.
    pub alloc_count: u64,
    /// Allocated bytes attributed to this site.
    pub alloc_bytes: u64,
    /// Released bytes attributed to this site.
    pub dealloc_bytes: u64,
}

impl SiteSample {
    /// The site's stable identifier.
    pub const fn id(&self) -> &'static str {
        self.site.id()
    }

    /// The site's measurement category.
    pub const fn domain(&self) -> WorkDomain {
        self.site.domain()
    }

    /// What [`SiteSample::amount`] means.
    pub const fn unit(&self) -> WorkUnit {
        self.site.unit()
    }

    /// Net bytes still held by allocations made inside this site.
    ///
    /// Saturating, because a scope legitimately frees data it did not
    /// allocate (a drop inside the scope of something built outside it).
    pub const fn net_bytes(&self) -> u64 {
        self.alloc_bytes.saturating_sub(self.dealloc_bytes)
    }

    /// Whether the row carries any observation at all.
    pub const fn is_empty(&self) -> bool {
        self.calls == 0
            && self.amount == 0
            && self.nanos == 0
            && self.digest == 0
            && self.alloc_count == 0
            && self.alloc_bytes == 0
            && self.dealloc_bytes == 0
    }
}

fn sample_of(site: WorkSite) -> SiteSample {
    let cell = cell(site);
    SiteSample {
        site,
        calls: cell.calls.load(Ordering::Relaxed),
        amount: cell.amount.load(Ordering::Relaxed),
        nanos: cell.nanos.load(Ordering::Relaxed),
        digest: cell.digest.load(Ordering::Relaxed),
        alloc_count: cell.alloc_count.load(Ordering::Relaxed),
        alloc_bytes: cell.alloc_bytes.load(Ordering::Relaxed),
        dealloc_bytes: cell.dealloc_bytes.load(Ordering::Relaxed),
    }
}

/// Read every declared site, including sites with no observations.
///
/// Declaration order, so two snapshots line up row for row.
pub fn snapshot_all() -> Vec<SiteSample> {
    WorkSite::ALL.iter().copied().map(sample_of).collect()
}

/// Read only the sites that recorded something.
pub fn snapshot() -> Vec<SiteSample> {
    WorkSite::ALL
        .iter()
        .copied()
        .map(sample_of)
        .filter(|row| !row.is_empty())
        .collect()
}

/// Read one site.
pub fn read(site: WorkSite) -> SiteSample {
    sample_of(site)
}

/// Zero every counter.
///
/// Not synchronised against concurrent recording — a reset racing live
/// work loses an unbounded number of increments. Harnesses call it
/// between phases, on the driving thread, with the workload quiesced.
pub fn reset() {
    for site in WorkSite::ALL {
        let cell = cell(*site);
        cell.calls.store(0, Ordering::Relaxed);
        cell.amount.store(0, Ordering::Relaxed);
        cell.nanos.store(0, Ordering::Relaxed);
        cell.digest.store(0, Ordering::Relaxed);
        cell.alloc_count.store(0, Ordering::Relaxed);
        cell.alloc_bytes.store(0, Ordering::Relaxed);
        cell.dealloc_bytes.store(0, Ordering::Relaxed);
    }
}

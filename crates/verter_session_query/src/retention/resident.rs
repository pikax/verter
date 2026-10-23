//! Resident occupancy of shared semantic backing storage, tied to the
//! storage's own lifetime.
//!
//! A backing allocation several owners and readers share — a file's frozen
//! closure-capture summary, a flow skeleton's name index — is counted here
//! from the moment it is built until the last handle on it drops. The count
//! rides the allocation itself (a [`ResidentCharge`] field released by its
//! `Drop`), so it follows the storage wherever it is held: a cache entry, a
//! retired artifact version, or only a reader that outlived both. Nothing
//! here keeps storage alive; nothing walks a cache to find it.
//!
//! Like the aggregate retention account, the figures are per PROCESS: every
//! host of the process contributes to the same totals.
//! Each kind's counts and allocation total are read and updated together,
//! so a snapshot cannot mix occupancy before a release with its total after it.

use parking_lot::Mutex;

use verter_no_typeexpr::NoTypeExpr;

/// Counted quantities per kind; a kind uses a prefix of them.
pub(crate) const RESIDENT_SLOTS: usize = 6;

/// The kinds of shared storage counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, NoTypeExpr, verter_retention::heap_size::HeapSize)]
pub(crate) enum ResidentKind {
    /// A file's frozen closure-capture summary.
    CaptureSummary,
    /// The storage of a flow skeleton's name index.
    SkeletonNameIndex,
}

impl ResidentKind {
    const COUNT: usize = 2;

    fn ledger(self) -> &'static Ledger {
        &LEDGERS[self as usize]
    }
}

struct Ledger {
    totals: Mutex<Totals>,
}

struct Totals {
    counts: [usize; RESIDENT_SLOTS],
    allocations: usize,
}

impl Totals {
    fn add(&mut self, counts: &[usize; RESIDENT_SLOTS]) {
        for (slot, count) in self.counts.iter_mut().zip(counts) {
            *slot += *count;
        }
    }

    fn sub(&mut self, counts: &[usize; RESIDENT_SLOTS]) {
        for (slot, count) in self.counts.iter_mut().zip(counts) {
            *slot -= *count;
        }
    }
}

impl Ledger {
    const fn new() -> Self {
        Self {
            totals: Mutex::new(Totals {
                counts: [0; RESIDENT_SLOTS],
                allocations: 0,
            }),
        }
    }

    fn admit(&self, counts: &[usize; RESIDENT_SLOTS]) {
        let mut totals = self.totals.lock();
        totals.add(counts);
        totals.allocations += 1;
    }

    fn release(&self, counts: &[usize; RESIDENT_SLOTS]) {
        let mut totals = self.totals.lock();
        totals.sub(counts);
        totals.allocations -= 1;
    }

    fn recharge(&self, previous: &[usize; RESIDENT_SLOTS], counts: &[usize; RESIDENT_SLOTS]) {
        let mut totals = self.totals.lock();
        totals.sub(previous);
        totals.add(counts);
    }

    fn snapshot(&self) -> ([usize; RESIDENT_SLOTS], usize) {
        let totals = self.totals.lock();
        (totals.counts, totals.allocations)
    }
}

static LEDGERS: [Ledger; ResidentKind::COUNT] = [const { Ledger::new() }; ResidentKind::COUNT];

/// One allocation's contribution to the resident totals, released exactly
/// once when the allocation drops. The default charges nothing.
#[derive(Debug, Default, NoTypeExpr, verter_retention::heap_size::HeapSize)]
pub(crate) struct ResidentCharge {
    charged: Option<(ResidentKind, [usize; RESIDENT_SLOTS])>,
}

impl ResidentCharge {
    /// Count one allocation of `kind` holding `counts`.
    pub(crate) fn admit(kind: ResidentKind, counts: [usize; RESIDENT_SLOTS]) -> Self {
        let ledger = kind.ledger();
        ledger.admit(&counts);
        Self {
            charged: Some((kind, counts)),
        }
    }

    /// The allocation now holds `counts`: replace what it contributes.
    pub(crate) fn recharge(&mut self, kind: ResidentKind, counts: [usize; RESIDENT_SLOTS]) {
        match &mut self.charged {
            Some((charged_kind, charged)) if *charged_kind == kind => {
                let ledger = kind.ledger();
                ledger.recharge(charged, &counts);
                *charged = counts;
            }
            _ => *self = Self::admit(kind, counts),
        }
    }
}

impl Drop for ResidentCharge {
    fn drop(&mut self) {
        if let Some((kind, counts)) = self.charged.take() {
            let ledger = kind.ledger();
            ledger.release(&counts);
        }
    }
}

/// What every live allocation of `kind` holds, summed, and how many there
/// are.
pub(crate) fn resident(kind: ResidentKind) -> ([usize; RESIDENT_SLOTS], usize) {
    kind.ledger().snapshot()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::sync::Barrier;

    #[test]
    fn snapshots_are_coherent_during_admission_recharge_and_release() {
        let ledger = Ledger::new();
        let start = Barrier::new(4);
        std::thread::scope(|scope| {
            for _ in 0..3 {
                scope.spawn(|| {
                    start.wait();
                    for _ in 0..20_000 {
                        let initial = [1, 2, 3, 4, 5, 6];
                        let grown = [2, 4, 6, 8, 10, 12];
                        ledger.admit(&initial);
                        std::thread::yield_now();
                        ledger.recharge(&initial, &grown);
                        std::thread::yield_now();
                        ledger.release(&grown);
                    }
                });
            }
            start.wait();
            for _ in 0..60_000 {
                let (counts, allocations) = ledger.snapshot();
                assert_eq!(
                    counts,
                    std::array::from_fn(|slot| counts[0] * (slot + 1)),
                    "every slot must describe the same set of allocations"
                );
                assert!(
                    (allocations..=allocations * 2).contains(&counts[0]),
                    "allocation count must agree with occupancy: {counts:?}, {allocations}"
                );
                std::thread::yield_now();
            }
        });
        assert_eq!(ledger.snapshot(), ([0; RESIDENT_SLOTS], 0));
    }
}

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

use std::sync::atomic::{AtomicUsize, Ordering};

use verter_no_typeexpr::NoTypeExpr;

/// Counted quantities per kind; a kind uses a prefix of them.
pub(crate) const RESIDENT_SLOTS: usize = 6;

/// The kinds of shared storage counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, NoTypeExpr)]
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
    counts: [AtomicUsize; RESIDENT_SLOTS],
    allocations: AtomicUsize,
}

impl Ledger {
    const fn new() -> Self {
        Self {
            counts: [const { AtomicUsize::new(0) }; RESIDENT_SLOTS],
            allocations: AtomicUsize::new(0),
        }
    }

    fn add(&self, counts: &[usize; RESIDENT_SLOTS]) {
        for (slot, count) in self.counts.iter().zip(counts) {
            slot.fetch_add(*count, Ordering::Relaxed);
        }
    }

    fn sub(&self, counts: &[usize; RESIDENT_SLOTS]) {
        for (slot, count) in self.counts.iter().zip(counts) {
            slot.fetch_sub(*count, Ordering::Relaxed);
        }
    }
}

static LEDGERS: [Ledger; ResidentKind::COUNT] = [const { Ledger::new() }; ResidentKind::COUNT];

/// One allocation's contribution to the resident totals, released exactly
/// once when the allocation drops. The default charges nothing.
#[derive(Debug, Default, NoTypeExpr)]
pub(crate) struct ResidentCharge {
    charged: Option<(ResidentKind, [usize; RESIDENT_SLOTS])>,
}

impl ResidentCharge {
    /// Count one allocation of `kind` holding `counts`.
    pub(crate) fn admit(kind: ResidentKind, counts: [usize; RESIDENT_SLOTS]) -> Self {
        let ledger = kind.ledger();
        ledger.add(&counts);
        ledger.allocations.fetch_add(1, Ordering::Relaxed);
        Self {
            charged: Some((kind, counts)),
        }
    }

    /// The allocation now holds `counts`: replace what it contributes.
    pub(crate) fn recharge(&mut self, kind: ResidentKind, counts: [usize; RESIDENT_SLOTS]) {
        match &mut self.charged {
            Some((charged_kind, charged)) if *charged_kind == kind => {
                let ledger = kind.ledger();
                ledger.sub(charged);
                ledger.add(&counts);
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
            ledger.sub(&counts);
            ledger.allocations.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

/// What every live allocation of `kind` holds, summed, and how many there
/// are.
pub(crate) fn resident(kind: ResidentKind) -> ([usize; RESIDENT_SLOTS], usize) {
    let ledger = kind.ledger();
    (
        std::array::from_fn(|slot| ledger.counts[slot].load(Ordering::Relaxed)),
        ledger.allocations.load(Ordering::Relaxed),
    )
}

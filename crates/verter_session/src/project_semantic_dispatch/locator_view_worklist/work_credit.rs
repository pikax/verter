//! Panic-safe accounting for one explicit locator projection worklist.

use super::super::connected_demand::ConnectedDemandLedger;

/// Structural traversal consumes this local credit and synchronizes it before
/// any nested semantic query can observe or join the connected demand. Drop
/// commits on unwind as well.
///
/// Typed on the [`ConnectedDemandLedger`] alone, not on the dispatcher: this
/// is pure work accounting, and the ledger carries no semantic capability, so
/// a credit window cannot reach back into dispatch.
pub(super) struct ConnectedWorkCredit<'ledger, 'ctx> {
    ledger: &'ledger ConnectedDemandLedger<'ctx>,
    window_start: usize,
    remaining: usize,
}

impl<'ledger, 'ctx> ConnectedWorkCredit<'ledger, 'ctx> {
    pub(super) fn new(
        ledger: &'ledger ConnectedDemandLedger<'ctx>,
    ) -> Result<Self, crate::semantic_query::PartialReasonSet> {
        let available = ledger.work_available()?;
        Ok(Self {
            ledger,
            window_start: available,
            remaining: available,
        })
    }

    #[inline(always)]
    pub(super) fn consume(&mut self) -> Result<(), crate::semantic_query::PartialReasonSet> {
        if self.remaining == 0 {
            self.settle();
            return self.ledger.charge();
        }
        self.remaining -= 1;
        Ok(())
    }

    #[inline(always)]
    pub(super) fn settle(&mut self) {
        self.ledger.commit(self.window_start - self.remaining);
        self.window_start = 0;
        self.remaining = 0;
    }

    pub(super) fn refresh(&mut self) -> Result<(), crate::semantic_query::PartialReasonSet> {
        verter_debug_assert_eq!(self.window_start, 0);
        verter_debug_assert_eq!(self.remaining, 0);
        let available = self.ledger.work_available()?;
        self.window_start = available;
        self.remaining = available;
        Ok(())
    }
}

impl Drop for ConnectedWorkCredit<'_, '_> {
    fn drop(&mut self) {
        self.settle();
    }
}

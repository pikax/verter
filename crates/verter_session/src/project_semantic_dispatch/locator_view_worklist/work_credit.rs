//! Panic-safe accounting for one explicit locator projection worklist.

use super::super::ProjectSemanticDispatch;

/// Structural traversal consumes this local credit and synchronizes it before
/// any nested semantic query can observe or join the connected demand. Drop
/// commits on unwind as well.
pub(super) struct ConnectedWorkCredit<'dispatch, 'ctx> {
    dispatch: &'dispatch ProjectSemanticDispatch<'ctx>,
    window_start: usize,
    remaining: usize,
}

impl<'dispatch, 'ctx> ConnectedWorkCredit<'dispatch, 'ctx> {
    pub(super) fn new(
        dispatch: &'dispatch ProjectSemanticDispatch<'ctx>,
    ) -> Result<Self, crate::semantic_query::PartialReasonSet> {
        let available = dispatch.connected_work_available()?;
        Ok(Self {
            dispatch,
            window_start: available,
            remaining: available,
        })
    }

    #[inline(always)]
    pub(super) fn consume(&mut self) -> Result<(), crate::semantic_query::PartialReasonSet> {
        if self.remaining == 0 {
            self.settle();
            return self.dispatch.charge_connected_work();
        }
        self.remaining -= 1;
        Ok(())
    }

    #[inline(always)]
    pub(super) fn settle(&mut self) {
        self.dispatch
            .commit_connected_work(self.window_start - self.remaining);
        self.window_start = 0;
        self.remaining = 0;
    }

    pub(super) fn refresh(&mut self) -> Result<(), crate::semantic_query::PartialReasonSet> {
        verter_debug_assert_eq!(self.window_start, 0);
        verter_debug_assert_eq!(self.remaining, 0);
        let available = self.dispatch.connected_work_available()?;
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

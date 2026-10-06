//! The selected binding-disable authority retains the one transaction owner.
use super::super::dispatch_txn::CheckerDispatchTransaction;
use std::cell::RefCell;
pub(super) struct BindingControl<'a> {
    txn: &'a RefCell<CheckerDispatchTransaction>,
}
impl<'a> BindingControl<'a> {
    pub(super) fn new(txn: &'a RefCell<CheckerDispatchTransaction>) -> Self {
        Self { txn }
    }
    pub(super) fn disable(&self) -> BindingGuard<'a> {
        self.txn.borrow_mut().begin_binding_disabled();
        BindingGuard { txn: self.txn }
    }
}
pub(super) struct BindingGuard<'a> {
    txn: &'a RefCell<CheckerDispatchTransaction>,
}
impl Drop for BindingGuard<'_> {
    fn drop(&mut self) {
        self.txn.borrow_mut().end_binding_disabled();
    }
}

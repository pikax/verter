//! The push buffer: consecutive static and interpolated content shares one
//! `` _push(`…`) `` template literal; structural constructs (`v-if`,
//! `v-for`, components) close it, and the next content opens a new one.

use super::state::{SsrEmitter, SsrOut};

impl<'c, M: verter_audit::observe::ObserveExec> SsrEmitter<'c, M> {
    /// Close the open `` _push(`…` `` literal, if any.
    pub(super) fn close_push(&mut self, pos: u32, out: &mut SsrOut<'c>) {
        if let Some(close) = self.take_push_close() {
            out.prepend_static(pos, close);
        }
    }

    /// Take the closing text so a relocated slot can carry it in its suffix.
    pub(super) fn take_push_close(&mut self) -> Option<&'static str> {
        std::mem::take(&mut self.in_push).then_some("`)\n")
    }

    /// Open a `` _push(`…` `` literal unless one is open.
    pub(super) fn ensure_push(&mut self, pos: u32, out: &mut SsrOut<'c>) {
        if !self.in_push {
            out.prepend_alloc(pos, "_push(`");
            self.in_push = true;
        }
    }

    /// Drop every pending comment removal inside `[start, end)`: the
    /// header text claiming that range already elides those bytes.
    pub(super) fn absorb_pending_comment_removals(&mut self, start: u32, end: u32) {
        self.pending_disabled_comment_removals
            .retain(|&(cs, ce)| !(cs >= start && ce <= end));
    }
}

//! `MacroHotMirror` trait impls (Debug snapshot + the clone-resets-mirror
//! contract) — split from the producer body for the module line budget.

use super::MacroHotMirror;
use std::sync::OnceLock;

impl std::fmt::Debug for MacroHotMirror {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacroHotMirror")
            .field(
                "demanded",
                &self
                    .cells
                    .get()
                    .map(|c| c.iter().filter(|x| x.committed.get().is_some()).count())
                    .unwrap_or(0),
            )
            .finish()
    }
}

impl Clone for MacroHotMirror {
    /// A cloned artifact starts with an EMPTY mirror: the `HotTypeRef`
    /// handles are interned ids valid for the project graph, but the mirror
    /// is a per-artifact demand cache and a clone is a distinct artifact
    /// instance. Re-demand repopulates it (the underlying interned nodes are
    /// content-addressed, so a re-lower hits the same node ids).
    fn clone(&self) -> Self {
        Self {
            cells: OnceLock::new(),
        }
    }
}

//! Slot-root metadata: the flags a construct carries when it is a root of a
//! slot block that has no stable root.
//!
//! Vue 3.6.0-rc.15 decides this in `markSlotRootOperations` only once the
//! whole slot block is known, after its constructs are built. Each construct
//! therefore records the flag text it writes as a run of its own, beside the
//! marked text, and the blocks whose roots are marked in turn; marking a slot
//! block redirects those runs before any output is written.
use super::ops::{BlockKind, ClosedBlock, Part, RunId};
use super::scope::MergedConstructKind;
use super::state::VaporState;

/// One returned root of a block, as `markSlotRootOperations` reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SlotRoot {
    /// The root's node name: a construct's own id, or a template's node.
    pub(super) node_ref: u32,
    /// `hasStableSlotRoot`: a component with a static type, or a template
    /// with content other than comments.
    pub(super) stable: bool,
}

/// What marking one construct as a slot root changes.
#[derive(Debug, Default)]
pub(super) struct SlotRootMark {
    /// Each flag run this construct writes, with the run it writes marked.
    pub(super) flags: Vec<(RunId, RunId)>,
    /// The blocks whose roots are marked with it: a conditional's branches,
    /// a loop's row.
    pub(super) blocks: Vec<Vec<SlotRoot>>,
}

impl<'c> VaporState<'c> {
    pub(super) fn slot_root(&self, block: &ClosedBlock) -> SlotRoot {
        SlotRoot {
            node_ref: block.node_ref,
            stable: match block.kind {
                BlockKind::Component => true,
                BlockKind::Node => self.is_stable_template_root(block.template_idx),
                BlockKind::Fragment | BlockKind::Roots => false,
            },
        }
    }

    /// The roots a closed block returns.
    pub(super) fn block_roots(&self, block: &ClosedBlock) -> Vec<SlotRoot> {
        match block.kind {
            BlockKind::Roots => self
                .block_roots
                .get(&block.run.0)
                .cloned()
                .unwrap_or_default(),
            _ => vec![self.slot_root(block)],
        }
    }

    /// Record the roots of a block returning several, read back through
    /// [`Self::block_roots`].
    pub(super) fn record_block_roots(&mut self, run: RunId, roots: Vec<SlotRoot>) {
        self.block_roots.insert(run.0, roots);
    }

    /// Register a construct's slot-root metadata: `flags` is written as is
    /// unless the construct is marked, when `marked` is written instead.
    pub(super) fn slot_root_flags(
        &mut self,
        node_ref: u32,
        flags: &mut Vec<Part<'c>>,
        marked: &mut Vec<Part<'c>>,
    ) -> RunId {
        let run = self.runs.commit(flags);
        let marked = self.runs.commit(marked);
        self.slot_root_marks
            .entry(node_ref)
            .or_default()
            .flags
            .push((run, marked));
        run
    }

    /// A block whose roots are marked with the construct `node_ref`.
    pub(super) fn slot_root_block(&mut self, node_ref: u32, roots: Vec<SlotRoot>) {
        self.slot_root_marks
            .entry(node_ref)
            .or_default()
            .blocks
            .push(roots);
    }

    /// Vue's `markSlotRootOperations`: with no stable root, every returned
    /// construct is marked and the blocks it renders are marked in turn.
    /// Returns whether the block has a stable root (nothing is marked then).
    pub(super) fn mark_slot_roots(&mut self, roots: &[SlotRoot]) -> bool {
        if roots.iter().any(|root| root.stable) {
            return true;
        }
        for root in roots {
            let Some(mark) = self.slot_root_marks.remove(&root.node_ref) else {
                continue;
            };
            for (run, marked) in mark.flags {
                self.runs.redirect(run, marked);
            }
            for block in mark.blocks {
                self.mark_slot_roots(&block);
            }
        }
        false
    }
}

/// The root a construct merged into a container-less slot body is.
pub(super) fn merged_root(kind: MergedConstructKind, node_ref: u32) -> SlotRoot {
    SlotRoot {
        node_ref,
        stable: kind == MergedConstructKind::Component,
    }
}

/// A flags argument, with the development comment naming each set flag.
pub(super) fn flags_text(flags: u32, names: &[&str], is_production: bool) -> String {
    if is_production {
        flags.to_string()
    } else {
        format!("{flags} /* {} */", names.join(", "))
    }
}

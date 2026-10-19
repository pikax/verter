//! A visited set for the short graph walks the dispatcher runs per query.

use super::SemanticNodeId;

/// Node ids [`SmallNodeSet`] keeps inline before it spills to a hash set.
const INLINE: usize = 16;

/// The nodes one walk has visited: the first few in an inline array scanned
/// with integer compares, every one in a hash set once the array fills.
///
/// Most walks visit a handful of nodes. A hash set allocates on its first
/// insert and hashes and probes on every one, which an unoptimized build
/// pays through several generic calls per visit; the inline scan does not.
pub(crate) struct SmallNodeSet {
    inline: [SemanticNodeId; INLINE],
    len: usize,
    spilled: Option<rustc_hash::FxHashSet<SemanticNodeId>>,
}

impl Default for SmallNodeSet {
    fn default() -> Self {
        Self {
            inline: [SemanticNodeId(0); INLINE],
            len: 0,
            spilled: None,
        }
    }
}

impl SmallNodeSet {
    /// Record `node`; `false` when it was already recorded.
    pub(crate) fn insert(&mut self, node: SemanticNodeId) -> bool {
        if let Some(spilled) = self.spilled.as_mut() {
            return spilled.insert(node);
        }
        if self.inline_contains(node) {
            return false;
        }
        if self.len < INLINE {
            self.inline[self.len] = node;
            self.len += 1;
            return true;
        }
        let mut spilled: rustc_hash::FxHashSet<SemanticNodeId> =
            self.inline.iter().copied().collect();
        spilled.insert(node);
        self.spilled = Some(spilled);
        true
    }

    fn inline_contains(&self, node: SemanticNodeId) -> bool {
        let mut index = 0;
        while index < self.len {
            if self.inline[index].0 == node.0 {
                return true;
            }
            index += 1;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership_holds_across_the_spill() {
        let mut set = SmallNodeSet::default();
        for id in 0..40u64 {
            assert!(set.insert(SemanticNodeId(id)), "first insert of {id}");
            for seen in 0..=id {
                assert!(!set.insert(SemanticNodeId(seen)), "{seen} after {id}");
            }
        }
    }
}

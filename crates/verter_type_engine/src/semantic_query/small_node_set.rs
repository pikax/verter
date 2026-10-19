//! A visited set for the short graph walks the dispatcher runs per query.

use super::SemanticNodeId;
use verter_retention::word_set::WordSet;

/// Node ids [`SmallNodeSet`] keeps inline before it spills to a word set.
const INLINE: usize = 16;

/// The nodes one walk has visited: the first few in an inline array scanned
/// with integer compares, every one in an open-addressed [`WordSet`] once
/// the array fills.
///
/// Most walks visit a handful of nodes. A hash set allocates on its first
/// insert and hashes and probes on every one, which an unoptimized build
/// pays through several generic calls per visit; the inline scan does not,
/// and the word set past it probes with a multiply and plain compares.
pub(crate) struct SmallNodeSet {
    inline: [SemanticNodeId; INLINE],
    len: usize,
    spilled: Option<WordSet>,
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
            return spilled.insert(node.0);
        }
        if self.inline_contains(node) {
            return false;
        }
        if self.len < INLINE {
            self.inline[self.len] = node;
            self.len += 1;
            return true;
        }
        let mut spilled: WordSet = self.inline.iter().map(|held| held.0).collect();
        spilled.insert(node.0);
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

    /// Operands above `u32::MAX` — the high-bit binder tokens a walk may
    /// meet beside arena ordinals — are distinct full-width keys after the
    /// spill, on 32-bit targets too.
    #[test]
    fn full_width_operands_stay_distinct_after_the_spill() {
        let mut set = SmallNodeSet::default();
        let low = (0..INLINE as u64).map(SemanticNodeId);
        let high = [1u64 << 32, (1 << 32) | 1, 1 << 63, (1 << 63) | 7, u64::MAX];
        for node in low.clone().chain(high.map(SemanticNodeId)) {
            assert!(set.insert(node), "first insert of {}", node.0);
        }
        // Each high operand truncates to a low ordinal already held; it
        // must still read as its own key.
        assert!(set.insert(SemanticNodeId((1 << 32) | 2)));
        for node in low.chain(high.map(SemanticNodeId)) {
            assert!(!set.insert(node), "repeat insert of {}", node.0);
        }
    }
}

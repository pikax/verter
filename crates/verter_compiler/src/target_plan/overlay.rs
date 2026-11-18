use verter_debug_assert::verter_debug_assert;

use crate::framework_common::structure::{Arena, Id, SparseFacts, StructureError, TargetScratch};

/// Target state for the few nodes that have any. Iterates in node order.
pub struct SparseOverlay<K, T> {
    facts: SparseFacts<K, T, TargetScratch>,
}

impl<K, T> SparseOverlay<K, T> {
    pub fn new() -> Self {
        Self {
            facts: SparseFacts::new(),
        }
    }
    pub fn insert(&mut self, id: Id<K>, value: T) -> Option<T> {
        self.facts.insert(id, value)
    }
    pub fn get(&self, id: Id<K>) -> Option<&T> {
        self.facts.get(id)
    }
    pub fn get_mut(&mut self, id: Id<K>) -> Option<&mut T> {
        self.facts.get_mut(id)
    }
    pub fn iter(&self) -> impl Iterator<Item = (Id<K>, &T)> {
        self.facts.iter()
    }
    pub fn len(&self) -> usize {
        self.facts.len()
    }
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }
}

impl<K, T> Default for SparseOverlay<K, T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Target state every node has: one `Copy` record per node, same index.
pub struct DenseOverlay<K, T: Copy> {
    values: Arena<K, T, TargetScratch>,
}

impl<K, T: Copy> DenseOverlay<K, T> {
    /// `len` records, each `fill`.
    pub fn filled(len: usize, fill: T) -> Result<Self, StructureError> {
        let mut values = Arena::new();
        for _ in 0..len {
            values.push(fill)?;
        }
        Ok(Self { values })
    }
    pub fn get(&self, id: Id<K>) -> Option<T> {
        self.values.get(id).copied()
    }
    /// Overwrite the record of `id`. The overlay has one record per node of
    /// the structure it was filled for, so an identity from another
    /// structure is a caller bug and changes nothing.
    pub fn set(&mut self, id: Id<K>, value: T) {
        let slot = self.values.get_mut(id);
        verter_debug_assert!(slot.is_some(), "node {id:?} is outside the overlay");
        if let Some(slot) = slot {
            *slot = value;
        }
    }
    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    enum Node {}

    fn ids(count: usize) -> Vec<Id<Node>> {
        let mut arena = Arena::<Node, (), TargetScratch>::new();
        (0..count).map(|_| arena.push(()).unwrap()).collect()
    }

    #[test]
    fn sparse_overlay_iterates_in_node_order() {
        let ids = ids(5);
        let mut overlay = SparseOverlay::<Node, &str>::new();
        assert!(overlay.is_empty());
        assert_eq!(overlay.insert(ids[3], "d"), None);
        assert_eq!(overlay.insert(ids[0], "a"), None);
        assert_eq!(overlay.insert(ids[2], "c"), None);
        assert_eq!(overlay.insert(ids[3], "D"), Some("d"));
        *overlay.get_mut(ids[0]).unwrap() = "A";

        assert_eq!(
            overlay.iter().collect::<Vec<_>>(),
            [(ids[0], &"A"), (ids[2], &"c"), (ids[3], &"D")]
        );
        assert_eq!(overlay.len(), 3);
        assert_eq!(overlay.get(ids[1]), None);
        assert!(overlay.get_mut(ids[4]).is_none());
    }

    #[test]
    fn dense_overlay_reads_what_it_set() {
        let ids = ids(4);
        let mut overlay = DenseOverlay::<Node, u8>::filled(3, 7).unwrap();
        assert_eq!(overlay.len(), 3);
        assert!(!overlay.is_empty());
        overlay.set(ids[1], 9);
        assert_eq!(
            ids.iter().map(|id| overlay.get(*id)).collect::<Vec<_>>(),
            [Some(7), Some(9), Some(7), None]
        );
        assert!(DenseOverlay::<Node, u8>::filled(0, 0).unwrap().is_empty());
    }
}

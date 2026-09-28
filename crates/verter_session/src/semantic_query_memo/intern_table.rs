//! Sparse value interner with ordinal ids that are never reused.
//!
//! The relation-proof and relate-key tables hand out opaque `u32` ordinals
//! that are retained outside the store (a `CoinductiveCycle` proof names
//! relate keys by id, memo payloads name proofs), so an ordinal is never
//! reused. A dense `Vec<Option<T>>` would then grow with every id ever
//! handed out: a document close releases entries, but their slots stayed
//! allocated forever. Storage here is keyed by ordinal in a hash map, so it
//! follows the LIVE set: a released ordinal simply has no entry and reads
//! as unknown, exactly like an ordinal never handed out.

use std::hash::Hash;

use rustc_hash::FxHashMap;

use crate::semantic_query::{RelateKeyId, RelateMemoKey, RelationProof, RelationProofId};

/// Interned relation proofs: sparse ordinal-keyed storage (an id is never
/// reused; a released proof's ordinal simply has no entry) plus the dedup
/// map over the live ones, so storage follows the live set.
pub(super) type RelationProofTable = InternTable<RelationProof, RelationProofId>;

/// Interned co-discharged relate keys, same discipline as
/// [`RelationProofTable`].
pub(super) type RelateKeyTable = InternTable<RelateMemoKey, RelateKeyId>;

/// Interned values addressed by a never-reused `u32` ordinal, deduplicated
/// by value. `Id` is the newtype the ordinal is handed out as.
pub(super) struct InternTable<T, Id> {
    by_ordinal: FxHashMap<u32, T>,
    by_value: FxHashMap<T, Id>,
    /// The next ordinal to hand out; every ordinal below it was handed out
    /// exactly once.
    next: u32,
}

impl<T, Id> Default for InternTable<T, Id> {
    fn default() -> Self {
        Self {
            by_ordinal: FxHashMap::default(),
            by_value: FxHashMap::default(),
            next: 0,
        }
    }
}

impl<T: Clone + Eq + Hash, Id: Copy + From<u32>> InternTable<T, Id> {
    /// Intern `value`, returning the existing id when it is already held.
    pub(super) fn intern(&mut self, value: T) -> Id {
        if let Some(id) = self.by_value.get(&value) {
            return *id;
        }
        let ordinal = self.next;
        self.next = ordinal
            .checked_add(1)
            .expect("intern table ordinal space exhausted");
        let id = Id::from(ordinal);
        self.by_ordinal.insert(ordinal, value.clone());
        self.by_value.insert(value, id);
        id
    }

    /// The value at `ordinal`; `None` for an ordinal never handed out and
    /// for a released one.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn get(&self, ordinal: u32) -> Option<&T> {
        self.by_ordinal.get(&ordinal)
    }

    /// Live entries — exactly what the table stores.
    pub(super) fn len(&self) -> usize {
        self.by_ordinal.len()
    }

    /// Remove every entry `stale` selects and return their ordinals.
    pub(super) fn release_where(&mut self, mut stale: impl FnMut(&T) -> bool) -> Vec<u32> {
        let released: Vec<u32> = self
            .by_ordinal
            .iter()
            .filter(|(_, value)| stale(value))
            .map(|(ordinal, _)| *ordinal)
            .collect();
        for ordinal in &released {
            if let Some(value) = self.by_ordinal.remove(ordinal) {
                self.by_value.remove(&value);
            }
        }
        released
    }
}

impl From<u32> for RelationProofId {
    fn from(ordinal: u32) -> Self {
        Self(ordinal)
    }
}

impl From<u32> for RelateKeyId {
    fn from(ordinal: u32) -> Self {
        Self(ordinal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct Id(u32);
    impl From<u32> for Id {
        fn from(ordinal: u32) -> Self {
            Self(ordinal)
        }
    }

    /// Releasing drops storage; ids keep counting up and a released ordinal
    /// reads as unknown. Discriminating: the dense slot vector this replaces
    /// kept one slot per id ever handed out, so `len` (then storage) grew
    /// with every intern/release cycle.
    #[test]
    fn released_entries_leave_no_storage_and_ids_are_never_reused() {
        let mut table: InternTable<String, Id> = InternTable::default();
        let mut last = None;
        for cycle in 0..50u32 {
            let ids: Vec<Id> = (0..10)
                .map(|n| table.intern(format!("{cycle}:{n}")))
                .collect();
            assert_eq!(
                table.len(),
                10,
                "cycle {cycle}: storage holds the live entries only"
            );
            assert_eq!(
                table.intern(format!("{cycle}:0")),
                ids[0],
                "cycle {cycle}: interning a held value returns its id"
            );
            if let Some(previous) = last {
                assert!(ids[0].0 > previous, "cycle {cycle}: ids are never reused");
            }
            last = Some(ids[9].0);
            let released = table.release_where(|value| value.starts_with(&format!("{cycle}:")));
            assert_eq!(released.len(), 10);
            assert_eq!(
                table.len(),
                0,
                "cycle {cycle}: a released entry holds no storage"
            );
            assert!(
                table.get(ids[0].0).is_none(),
                "cycle {cycle}: a released ordinal is unknown"
            );
        }
    }
}

//! Retention observability of the semantic graph store: the live and
//! physical sizes of every structure counted ownership and document closes
//! are expected to keep flat, as `$/verter/getStatistics` reports them and
//! the churn endurance lane bounds them.

use super::*;

/// Current graph ownership, independent of optional observation histories.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeOwnershipCounts {
    /// Slots currently holding payloads.
    pub live_nodes: usize,
    /// Retained child occurrences, including repeated operands.
    pub internal_edges: usize,
    /// Independently acquired roots (shared clones count once).
    pub roots: usize,
    /// Global primitive vocabulary slots.
    pub permanent_nodes: usize,
}

impl SemanticGraphStore {
    /// Snapshot required current counts under the arena lock.
    pub fn node_ownership_counts(&self) -> NodeOwnershipCounts {
        self.arena.ownership_counts()
    }
    /// Number of LIVE interned semantic nodes — the slots that still hold
    /// a payload. A destroyed slot is not counted;
    /// [`Self::node_slot_count`] is the append-only id space.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.arena.live_len()
    }

    /// Number of node ids ever allocated — live PLUS destroyed. Grows
    /// monotonically (ids are never reused); the retained payload set is
    /// [`Self::node_count`] and the storage held is
    /// [`Self::node_storage_slots`].
    #[must_use]
    pub fn node_slot_count(&self) -> usize {
        self.arena.len()
    }

    /// Node slots the arena physically holds right now (live chunks times
    /// the chunk size). Unlike [`Self::node_slot_count`] this is bounded by
    /// the live set: a chunk whose nodes were all destroyed is dropped.
    #[must_use]
    pub fn node_storage_slots(&self) -> usize {
        self.arena.storage_slots()
    }

    /// Whether `id` still resolves to its interned payload. `false` for an
    /// id this store never handed out and for a destroyed slot (such an id
    /// reads the shared `Opaque(Miss)` placeholder through
    /// [`Self::node_data`]). Reports liveness only; it authorizes no read.
    #[must_use]
    pub fn node_is_live(&self, id: SemanticNodeId) -> bool {
        self.arena.is_live(id)
    }

    /// Number of resident union member views (V8's `union_views`); a close
    /// evicts the views over its closed content.
    #[must_use]
    pub fn union_view_count(&self) -> usize {
        self.union_views.lock().len()
    }

    /// Number of stable-key classes (distinct subtrees) the store's key
    /// table holds; see [`crate::semantic_query::stable_key::KeyClasses`].
    #[must_use]
    pub fn stable_key_class_count(&self) -> usize {
        self.key_classes.lock().class_count()
    }

    /// Number of nodes whose stable-key class the store's key table holds.
    #[must_use]
    pub fn stable_key_classified_count(&self) -> usize {
        self.key_classes.lock().classified_count()
    }

    /// Number of memoized class orders and fingerprint hash-map offsets the
    /// store's key table holds.
    #[must_use]
    pub fn stable_key_memo_count(&self) -> usize {
        self.key_classes.lock().memo_count()
    }

    /// Destroyed nodes whose per-node sidecar entries wait for cleanup:
    /// the bounded pending log (`release.rs`).
    #[must_use]
    pub fn pending_destroyed_nodes(&self) -> usize {
        self.arena.pending_destroyed()
    }

    /// Number of `unresolved_reach` entries (retention observability).
    #[must_use]
    pub fn unresolved_reach_count(&self) -> usize {
        self.unresolved_reach.lock().len()
    }

    /// Retention breakdown of [`Self::memo_entry_count`] by family name
    /// (the `FamilyKey` variant label), sorted by name. Lets a churn
    /// measurement name the family whose entries survive a release.
    #[must_use]
    pub fn memo_entry_counts_by_family(&self) -> Vec<(&'static str, usize)> {
        let entries = self.entries.lock();
        let mut counts: FxHashMap<&'static str, usize> = FxHashMap::default();
        for (family, slots) in entries.iter() {
            *counts.entry(family.variant_label()).or_default() += slots.populated_count();
        }
        drop(entries);
        let mut out: Vec<(&'static str, usize)> = counts.into_iter().collect();
        out.sort_unstable();
        out
    }

    /// Number of retained memo candidates across every family and slot.
    /// A slot holding variants for several store versions counts each, so
    /// unlike [`Self::memo_entry_count`] this sees superseded candidates a
    /// slot keeps beside its current one.
    #[must_use]
    pub fn memo_candidate_count(&self) -> usize {
        self.entries
            .lock()
            .values()
            .map(FamilySlots::candidate_count)
            .sum()
    }

    /// Number of warm memo entries — sums populated slots across every
    /// family. Useful for tests and counters. Two distinct mode slots in
    /// the same family count as two entries.
    #[must_use]
    pub fn memo_entry_count(&self) -> usize {
        self.entries
            .lock()
            .values()
            .map(FamilySlots::populated_count)
            .sum()
    }
}

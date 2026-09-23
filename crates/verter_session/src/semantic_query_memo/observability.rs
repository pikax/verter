//! Retention observability of the semantic graph store: the live and
//! physical sizes of every structure a document close is expected to keep
//! flat, as `$/verter/getStatistics` reports them and the churn endurance
//! lane bounds them.

use super::*;

impl SemanticGraphStore {
    /// Number of LIVE interned semantic nodes — the slots that still hold
    /// a payload. A slot [`Self::release_canonical`] tombstoned is not
    /// counted; [`Self::node_slot_count`] is the append-only id space.
    /// Useful for tests and counters.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.arena.live_len()
    }

    /// Number of node ids ever allocated — live PLUS released. Grows
    /// monotonically (ids are never reused); the retained payload set is
    /// [`Self::node_count`] and the storage held is
    /// [`Self::node_storage_slots`].
    #[must_use]
    pub fn node_slot_count(&self) -> usize {
        self.arena.len()
    }

    /// Node slots the arena physically holds right now (live chunks times
    /// the chunk size). Unlike [`Self::node_slot_count`] this is bounded by
    /// the live set: a chunk whose nodes were all released is dropped.
    #[must_use]
    pub fn node_storage_slots(&self) -> usize {
        self.arena.storage_slots()
    }

    /// Whether `id` still resolves to its interned payload. `false` for an
    /// id this store never handed out and for a slot
    /// [`Self::release_canonical`] tombstoned (such an id reads the shared
    /// `Opaque(Miss)` placeholder through [`Self::node_data`]).
    #[must_use]
    pub fn node_is_live(&self, id: SemanticNodeId) -> bool {
        self.arena.is_live(id)
    }

    /// Number of `unresolved_reach` entries (retention observability).
    #[must_use]
    pub fn unresolved_reach_count(&self) -> usize {
        self.unresolved_reach.lock().len()
    }

    /// Number of LIVE interned relation proofs (retention observability),
    /// which is exactly what the table stores: a proof
    /// [`Self::release_canonical`] dropped leaves no entry.
    #[must_use]
    pub fn relation_proof_count(&self) -> usize {
        self.relation_proof_table.lock().len()
    }

    /// Number of LIVE interned co-discharged relate keys (retention
    /// observability), exactly what the table stores.
    #[must_use]
    pub fn relate_key_count(&self) -> usize {
        self.relate_key_table.lock().len()
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

    /// Diagnostics dump of every candidate in the families whose variant
    /// label is `family_label` (see [`Self::memo_entry_counts_by_family`]):
    /// one line per candidate with the family key, the slot, the result,
    /// the carrier's canonicals and self-roots, and — for a family keyed by
    /// a node — the origin scope of that node. Not on any hot path; a churn
    /// measurement reads it to see what a growing family's keys carry.
    #[doc(hidden)]
    #[must_use]
    pub fn memo_family_dump_for_diagnostics(&self, family_label: &str) -> Vec<String> {
        let entries = self.entries.lock();
        let mut out: Vec<String> = Vec::new();
        for (family, slots) in entries.iter() {
            if family.variant_label() != family_label {
                continue;
            }
            let mut key_nodes: Vec<String> = Vec::new();
            family.for_each_node_id(|id| {
                key_nodes.push(format!(
                    "{id:?}@{:?}",
                    self.arena.scope(id).map(|scope| scope.canonical_file())
                ));
            });
            for (slot, entry) in slots.iter_populated_slots_all() {
                out.push(format!(
                    "{family:?} slot={slot:?} key_nodes={key_nodes:?} result={:?} \
                     carrier_canonicals={:?} self_roots={:?} aggregated={:?}",
                    entry.result,
                    entry.read_set_signature.canonical_ids(),
                    entry.self_root_canonicals,
                    entry.read_set_signature.aggregated_domains(),
                ));
            }
        }
        drop(entries);
        out.sort();
        out
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

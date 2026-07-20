//! `SemanticGraphStore` test-support surface — the `#[doc(hidden)]`
//! `*_for_tests` publish / probe helpers the integration suite drives.
//!
//! Extracted from `mod.rs` as a continuation `impl SemanticGraphStore` block
//! (same module tree, sibling file). These are public test-only entry points
//! (compiled in every build so integration-test crates can call them); they
//! reach the store's private admission internals through the parent module
//! (`use super::*`).

use super::*;

impl SemanticGraphStore {
    /// Test-only accessor: read the entry's [`ReadSetSignature`]
    /// carrier for `key`. Returns `None` when no entry is present.
    /// Surfaces the carrier's path-precise `facts` rail so integration
    /// tests can assert what facts the entry actually holds.
    #[doc(hidden)]
    #[must_use]
    pub fn entry_read_set_signature_for_tests(
        &self,
        key: &SemanticQueryKey,
    ) -> Option<crate::fact_signature_helpers::ReadSetSignature> {
        let (family, slot) = family_and_slot(key);
        let entries = self.entries_lock_diagnosed();
        entries
            .get(&family)
            .and_then(|slots| slots.slot_peek_any(slot).cloned())
            .map(|entry| entry.read_set_signature)
    }

    /// Test-only probe: the §3.4 `satisfied_projection` (materialised
    /// record set) of the FIRST candidate in `key`'s `(family, slot)`.
    /// Lets a guard assert backfill writes the RECORDED points verbatim
    /// (never a synthesised target-slot / meet point). `None` when the
    /// slot is empty.
    #[doc(hidden)]
    #[must_use]
    pub fn entry_satisfied_projection_for_tests(
        &self,
        key: &SemanticQueryKey,
    ) -> Option<MaterializedSet> {
        let (family, slot) = family_and_slot(key);
        let entries = self.entries_lock_diagnosed();
        entries
            .get(&family)
            .and_then(|slots| slots.slot_peek_any(slot).cloned())
            .map(|entry| entry.satisfied_projection)
    }

    /// Test-only direct publish. Delegates to
    /// [`Self::publish_with_carrier_dispatch_and_generation_for_tests`]
    /// with an empty dispatch-dep signature and generation `0`.
    #[doc(hidden)]
    #[cfg(any(test, feature = "test-support"))]
    pub fn publish_with_carrier_for_tests(
        &self,
        key: SemanticQueryKey,
        result: QueryResult<SemanticNodeId>,
        read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
    ) -> usize {
        self.publish_with_carrier_dispatch_and_generation_for_tests(
            key,
            result,
            read_set_signature,
            self_root_canonicals,
            empty_signature(),
            0,
        )
    }

    /// Variant of [`Self::publish_with_carrier_for_tests`] taking an
    /// explicit `dispatch_dep_signature` (FIFO reverse-index symmetry
    /// discriminator). Generation `0`.
    #[doc(hidden)]
    #[cfg(any(test, feature = "test-support"))]
    pub fn publish_with_carrier_and_dispatch_for_tests(
        &self,
        key: SemanticQueryKey,
        result: QueryResult<SemanticNodeId>,
        read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        dispatch_dep_signature: DepSignature,
    ) -> usize {
        self.publish_with_carrier_dispatch_and_generation_for_tests(
            key,
            result,
            read_set_signature,
            self_root_canonicals,
            dispatch_dep_signature,
            0,
        )
    }

    /// Variant taking an explicit `validated_at_generation` — used
    /// by multi-candidate overlay/base tests. The `satisfied_projection`
    /// defaults to the single requested point for `key` (so the published
    /// entry self-satisfies its own slot's warm-hit gate). Use
    /// [`Self::publish_with_materialized_set_for_tests`] to craft a record
    /// set that DIFFERS from the nominal slot (the §3.4 discriminating
    /// guards).
    #[doc(hidden)]
    #[cfg(any(test, feature = "test-support"))]
    pub fn publish_with_carrier_dispatch_and_generation_for_tests(
        &self,
        key: SemanticQueryKey,
        result: QueryResult<SemanticNodeId>,
        read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        dispatch_dep_signature: DepSignature,
        validated_at_generation: u64,
    ) -> usize {
        let satisfied_projection = MaterializedSet::single(requested_point_for_key(&key));
        self.publish_with_materialized_set_for_tests(
            key,
            result,
            read_set_signature,
            self_root_canonicals,
            dispatch_dep_signature,
            validated_at_generation,
            satisfied_projection,
        )
    }

    /// Test-only direct publish taking an EXPLICIT
    /// `satisfied_projection` — the §3.4 materialised-record set the
    /// published entry carries. Lets a guard publish an entry whose
    /// recorded points DIFFER from its nominal slot (e.g. an `Expanded`
    /// slot whose compute only materialised a `Navigate` point), exercising
    /// the warm-hit `cached_satisfies` gate and the recorded-point
    /// backfill directly.
    #[doc(hidden)]
    #[cfg(any(test, feature = "test-support"))]
    pub fn publish_with_materialized_set_for_tests(
        &self,
        key: SemanticQueryKey,
        result: QueryResult<SemanticNodeId>,
        read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        dispatch_dep_signature: DepSignature,
        validated_at_generation: u64,
        satisfied_projection: MaterializedSet,
    ) -> usize {
        if !matches!(result, QueryResult::Value(_)) {
            return 0;
        }
        let (family, slot) = family_and_slot(&key);
        let requested_path = requested_path_for_key(&key);
        let admission_seq = self.alloc_candidate_admission_seq();
        let dispatch_dep_signature = self.dep_signature_interner.intern(&dispatch_dep_signature);
        let entry = MemoEntry {
            result: match result {
                QueryResult::Value(node) => QueryResult::Value(SemanticQueryValue::TypeNode(node)),
                QueryResult::Recursive(node) => QueryResult::Recursive(node),
                QueryResult::Error(error) => QueryResult::Error(error),
            },
            read_set_signature: read_set_signature.clone(),
            dispatch_dep_signature: Arc::clone(&dispatch_dep_signature),
            self_root_canonicals,
            walker_diagnostics: Arc::from([]),
            satisfied_projection,
            validated_at_generation,
            admission_seq,
        };
        let mut entries = self.entries_lock_diagnosed();
        let family_was_new = !entries.contains_key(&family);
        let outcome =
            entries
                .entry(family.clone())
                .or_default()
                .publish(slot, entry, &requested_path);
        let populated_slots = outcome.populated;
        for (displaced_slot, displaced_entry) in &outcome.displaced {
            reverse_index::drain_candidate_reverse_index_registrations(
                &self.canonical_to_entries,
                &family,
                *displaced_slot,
                displaced_entry,
            );
        }
        if family_was_new && !populated_slots.is_empty() {
            self.record_family_admission_locked(&mut entries, &family);
        }
        reverse_index::register_reverse_index(
            &self.canonical_to_entries,
            &family,
            &populated_slots,
            &read_set_signature,
            &dispatch_dep_signature,
            admission_seq,
        );
        drop(entries);
        populated_slots.len()
    }

    /// Test-only candidate-count probe for `(family, slot)`.
    #[doc(hidden)]
    pub fn slot_candidate_count_for_tests(&self, key: &SemanticQueryKey) -> usize {
        let (family, slot) = family_and_slot(key);
        let entries = self.entries_lock_diagnosed();
        entries
            .get(&family)
            .map(|slots| slots.slot_candidate_count_for_test(slot))
            .unwrap_or(0)
    }
}

//! `SemanticGraphStore` test-support surface — the `#[doc(hidden)]`
//! `*_for_tests` publish / probe helpers the integration suite drives.
//!
//! Extracted from `mod.rs` as a continuation `impl SemanticGraphStore` block
//! (same module tree, sibling file). These are public test-only entry points
//! (compiled in every build so integration-test crates can call them); they
//! reach the store's private admission internals through the parent module
//! (`use super::*`).

use super::*;

/// Per-key error returned by `SemanticGraphStore::execute_cooperative_batch`.
///
/// The enum is re-exported through the test-support surface so integration
/// tests can project per-key failures without losing their typed reason.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchExpandError {
    /// Canonical content changed between the surface stamp and this read.
    StaleContentChanged,
    /// The canonical was deleted between stamp and read.
    FileDeleted,
    /// The declaration no longer exists under the current view.
    DeclarationRemoved,
    /// The semantic node was evicted and would require an unauthorized cold
    /// rebuild.
    EvictedNode,
}

impl SemanticGraphStore {
    /// Resolve a test batch through validated warm reads without admitting
    /// cold work. Missing or stale entries retain the typed per-key error.
    #[cfg(test)]
    pub(crate) fn execute_cooperative_batch(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        keys: &[crate::semantic_query::SemanticQueryKey],
    ) -> Vec<Result<SemanticNodeId, BatchExpandError>> {
        keys.iter()
            .map(|key| {
                if let Some(hit) = self.get_validated(key, ctx) {
                    match hit.value {
                        QueryResult::Value(node) | QueryResult::Recursive(node) => Ok(node),
                        QueryResult::Error(_) => Err(BatchExpandError::EvictedNode),
                    }
                } else {
                    Err(BatchExpandError::EvictedNode)
                }
            })
            .collect()
    }

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

    /// Test-only accessor: read the entry's `self_root_canonicals` for
    /// `key` (the content-version rail of the files the entry's value was
    /// built from). Returns `None` when no entry is present.
    #[doc(hidden)]
    #[must_use]
    pub fn entry_self_root_canonicals_for_tests(
        &self,
        key: &SemanticQueryKey,
    ) -> Option<Arc<[Arc<str>]>> {
        let (family, slot) = family_and_slot(key);
        let entries = self.entries_lock_diagnosed();
        entries
            .get(&family)
            .and_then(|slots| slots.slot_peek_any(slot).cloned())
            .map(|entry| entry.self_root_canonicals)
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
        self.publish_for_tests_impl(
            None,
            key,
            result,
            read_set_signature,
            self_root_canonicals,
            dispatch_dep_signature,
            validated_at_generation,
            satisfied_projection,
        )
    }

    /// Host-view-aware variant of
    /// [`Self::publish_with_carrier_dispatch_and_generation_for_tests`]:
    /// the publish plans its per-family bounded-retention eviction
    /// against the publishing caller's stable store view (invalid-first
    /// victim selection). The `satisfied_projection` defaults to the
    /// single requested point for `key`. Backs the per-family
    /// bounded-retention guards.
    #[doc(hidden)]
    #[cfg(any(test, feature = "test-support"))]
    pub fn publish_with_view_for_tests(
        &self,
        host: &crate::VerterHost,
        key: SemanticQueryKey,
        result: QueryResult<SemanticNodeId>,
        read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        dispatch_dep_signature: DepSignature,
        validated_at_generation: u64,
    ) -> usize {
        let satisfied_projection = MaterializedSet::single(requested_point_for_key(&key));
        self.publish_for_tests_impl(
            Some(host),
            key,
            result,
            read_set_signature,
            self_root_canonicals,
            dispatch_dep_signature,
            validated_at_generation,
            satisfied_projection,
        )
    }

    /// Shared implementation of the test-only direct publishes. `view`
    /// is the publishing caller's stable store view used for
    /// invalid-first eviction planning; `None` (the legacy helpers)
    /// falls back to the front-of-LRU-order victim.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(clippy::too_many_arguments)]
    fn publish_for_tests_impl(
        &self,
        view: Option<&dyn crate::resolver_core::ResolverContext>,
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
        let cap = family.candidate_cap();
        let eviction = match view {
            Some(ctx) => {
                family::plan_family_slot_eviction(&self.entries, &family, slot, &entry, cap, ctx)
            }
            None => family::EvictionVictim::LruFront,
        };
        let mut entries = self.entries_lock_diagnosed();
        let family_was_new = !entries.contains_key(&family);
        let outcome = entries.entry(family.clone()).or_default().publish(
            slot,
            entry,
            &requested_path,
            cap,
            eviction,
        );
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
    /// Test-only probe: the keys of every in-flight entry still CLAIMED
    /// and UNCOMPLETED — a flight whose owner has finished but which was
    /// never published, drained, or aborted.
    ///
    /// A retained entry in this state is lifecycle poison: a later
    /// demand of the same key joins it, `register_wait` reports a cycle
    /// against an owner that is no longer active, and the caller gets a
    /// permanent false `QueryResult::Recursive`.
    #[doc(hidden)]
    #[must_use]
    pub fn retained_claimed_flight_keys_for_tests(&self) -> Vec<SemanticQueryKey> {
        // Collect the handles under the table lock, then inspect each
        // entry's `state` with the table lock RELEASED (the store's
        // lock-order rule: `state` is never taken while `inflight` is
        // held).
        let handles: Vec<(PreparedKeyHandle, Arc<InflightEntry>)> = {
            let table = self.inflight.lock();
            table
                .iter()
                .map(|(handle, entry)| (handle.clone(), Arc::clone(entry)))
                .collect()
        };
        handles
            .into_iter()
            .filter(|(_, entry)| {
                let state = entry.state.lock();
                state.claimed && state.completed.is_none() && !state.aborted
            })
            .map(|(handle, _)| handle.key().clone())
            .collect()
    }

    pub fn slot_candidate_count_for_tests(&self, key: &SemanticQueryKey) -> usize {
        let (family, slot) = family_and_slot(key);
        let entries = self.entries_lock_diagnosed();
        entries
            .get(&family)
            .map(|slots| slots.slot_candidate_count_for_test(slot))
            .unwrap_or(0)
    }

    /// Test-only probe: the per-family bounded-retention candidate cap
    /// (`FamilyKey::candidate_cap`) for `key`'s family. Backs the
    /// per-family cap guard.
    #[doc(hidden)]
    #[must_use]
    pub fn family_candidate_cap_for_tests(&self, key: &SemanticQueryKey) -> usize {
        let (family, _) = family_and_slot(key);
        family.candidate_cap()
    }

    /// Test-only probe: `key`'s `(family, slot)` candidates'
    /// `validated_at_generation` stamps in slot order (front =
    /// least-recently admitted / validated-hit). Backs survivor/victim
    /// identity and LRU-order assertions in the bounded-retention
    /// guards.
    #[doc(hidden)]
    #[must_use]
    pub fn slot_candidate_generations_for_tests(&self, key: &SemanticQueryKey) -> Vec<u64> {
        let (family, slot) = family_and_slot(key);
        let entries = self.entries_lock_diagnosed();
        entries
            .get(&family)
            .map(|slots| slots.slot_candidate_generations_for_test(slot))
            .unwrap_or_default()
    }

    /// Test-only probe: exact admission tokens in one candidate slot, in LRU
    /// order. Concurrency tests use these tokens to distinguish an admitted
    /// candidate from a same-discriminant ABA replacement.
    #[doc(hidden)]
    #[must_use]
    pub fn slot_candidate_admission_seqs_for_tests(&self, key: &SemanticQueryKey) -> Vec<u64> {
        let (family, slot) = family_and_slot(key);
        let entries = self.entries_lock_diagnosed();
        entries
            .get(&family)
            .map(|slots| {
                slots
                    .snapshot_slot(slot)
                    .iter()
                    .map(|candidate| candidate.admission_seq)
                    .collect()
            })
            .unwrap_or_default()
    }
}

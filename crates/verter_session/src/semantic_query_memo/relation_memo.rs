//! Relation memo — the relation engine's read/write path over the family memo's
//! `Relate` family. Extracted from `mod.rs` as a continuation
//! `impl SemanticGraphStore` block (same module tree, sibling file),
//! mirroring `store_test_support.rs`: the store's private admission
//! internals are reached through the parent module (`use super::*`).
//!
//! Storage is the family memo's [`FamilyKey::Relate`] family in the
//! [`ModeSlot::Single`] slot (the rehomed relation memo — the retired
//! dedicated `BudgetedRelationMemo` folded into the family substrate).
//! The stored value is the compute-side tri-state verdict
//! ([`SemanticQueryValue::RelationVerdict`] wrapping
//! [`crate::semantic_query::RelationComputeResult`] — the compute/public
//! split); warm reads validate the self-version-rooted carrier strictly
//! AND hard-miss on a `validated_at_generation` mismatch. Retention rides
//! the family rails (per-family cap, invalid-first / LRU eviction, the
//! family `memo_budget` global bound, reverse-index drains).

use super::*;

/// Value-side relation admission decision. The relation engine supplies only
/// the observed self-roots and judgement; [`SemanticGraphStore`] owns the
/// tracer, finalization, carrier construction, and storage write.
pub(crate) enum RelationPublishDecision {
    Publish {
        observed_self_roots: Vec<ObservedGraphSelfRoot>,
        result: crate::semantic_query::RelationResult,
        validated_at_generation: u64,
    },
    ReturnOnly(crate::cache_runtime::NonAdmissionReason),
}

impl RelationPublishDecision {
    #[inline]
    pub(crate) fn publish(
        observed_self_roots: Vec<ObservedGraphSelfRoot>,
        result: crate::semantic_query::RelationResult,
        validated_at_generation: u64,
    ) -> Self {
        Self::Publish {
            observed_self_roots,
            result,
            validated_at_generation,
        }
    }

    #[inline]
    pub(crate) fn return_only(reason: crate::cache_runtime::NonAdmissionReason) -> Self {
        Self::ReturnOnly(reason)
    }
}

/// The `satisfied_projection` every relation-memo entry carries: the
/// modeless [`ModeSlot::Single`] identity point at the empty path, so the
/// family materialisation gates treat relation entries exactly like any
/// other modeless family's (the gate never blocks a modeless hit).
fn relation_satisfied_projection() -> MaterializedSet {
    MaterializedSet::single(MaterializedPoint::new(family::point_for_slot(
        ModeSlot::Single,
        &ProjectionPath::empty(),
    )))
}

impl SemanticGraphStore {
    /// Strict warm-hit read of a cached relation judgement for the full
    /// relation identity `key`.
    ///
    /// Returns the tri-state
    /// [`RelationResult`](crate::semantic_query::RelationResult) plus
    /// a dispatch-plumbing [`empty_signature`] (the relation carrier
    /// is the sole fact rail; the tuple shape is preserved for
    /// `relate_nodes` call-site compatibility) **only when** the
    /// stored entry's self-version-rooted carrier validates against
    /// the live store view — every self-root canonical's
    /// `FileWholeHash` is validated strictly AND the entry's
    /// `validated_at_generation` still equals the live project
    /// generation. A stale entry (same-canonical content edit,
    /// untracked self-root, or `ProjectGeneration` bump) returns
    /// `None`. Validation failure does NOT bubble the carrier.
    ///
    /// A `key` differing only in relation kind / policy / source freshness /
    /// inference context / env from a cached one is a DISTINCT slot and misses
    /// — the warm hit is on the FULL identity, not the bare `(source, target)`
    /// pair.
    ///
    /// Storage is the family memo's [`FamilyKey::Relate`] family in the
    /// [`ModeSlot::Single`] slot. The generation hard-miss is re-expressed
    /// on the read side: the family substrate treats
    /// `validated_at_generation` as recency / admission-discriminant
    /// metadata only (`family.rs`), so the gate lives HERE — a candidate
    /// serves warm only when its generation stamp still equals the live
    /// project generation AND its carrier validates, exactly the
    /// observable behavior the dedicated memo had.
    #[must_use]
    pub(crate) fn get_relation(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        key: &crate::semantic_query::RelateMemoKey,
    ) -> Option<(DepSignature, crate::semantic_query::RelationResult)> {
        let family = FamilyKey::Relate {
            key: Box::new(key.clone()),
        };
        // Snapshot the candidate list under the `entries` lock, then
        // validate OUTSIDE the lock — the same discipline as the family
        // warm read (`get_validated_value_impl`): `validate` / `bubble`
        // consult the resolver store view and fan into TLS tracers, which
        // may re-enter the memo; holding `entries` across that re-entry
        // would serialise — or, on a re-entrant read, deadlock — against
        // the single global memo mutex.
        let snapshot: CandidateList = {
            let entries = self.entries_lock_diagnosed();
            entries
                .get(&family)
                .map(|slots| slots.snapshot_slot(ModeSlot::Single))?
        };
        // Project-generation gate — the carrier alone misses a reset.
        let live_generation = ctx.project_type_store().current_project_generation();
        let hit = snapshot.into_iter().find(|entry| {
            entry.validated_at_generation == live_generation && entry.validate(ctx)
        })?;
        // Brief LRU bookkeeping — reacquire only to promote the hit
        // candidate in the slot's recency order (the family bounded-
        // retention treatment); a concurrent invalidation that drained it
        // in between makes this a no-op.
        {
            let mut entries = self.entries_lock_diagnosed();
            if let Some(slots) = entries.get_mut(&family) {
                slots.mark_validated_freshest(ModeSlot::Single, &hit);
            }
        }
        hit.read_set_signature.bubble(ctx);
        // Dispatch-plumbing payload; every `relate_nodes` caller
        // discards it. The carrier is the cache-validity oracle
        // (validated above); there is no second rail to return. The
        // stored compute verdict converts back to the engine's transient
        // tri-state — the inverse of the publish-path conversion (the
        // compute/public split); both directions are total and lossless.
        let verdict = match hit.result {
            QueryResult::Value(SemanticQueryValue::RelationVerdict(verdict)) => verdict,
            // Structural invariant: `insert_relation_owned` is the sole
            // writer of `Relate` family entries and only ever stores
            // `RelationVerdict` values.
            other => unreachable!(
                "Relate family entries store RelationVerdict values only; found {other:?}"
            ),
        };
        Some((empty_signature(), verdict.into()))
    }

    /// Run the complete cold relation computation inside a store-owned fact
    /// tracer, finalize the dependency evidence, build the self-rooted carrier,
    /// and perform the sole production relation-memo write.
    pub(crate) fn compute_relation_and_admit<R, Compute, Decide>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        key: crate::semantic_query::RelateMemoKey,
        compute: Compute,
        decide: Decide,
    ) -> R
    where
        Compute: FnOnce() -> R,
        Decide: FnOnce(&R) -> RelationPublishDecision,
    {
        let host = ctx.host_for_fact_tracer_install();
        let (value, read_set) = host.with_fact_tracer(compute);
        match read_set.finalise() {
            crate::resolver_core::FactReadSetFinalise::Ok(facts) => match decide(&value) {
                RelationPublishDecision::Publish {
                    observed_self_roots,
                    result,
                    validated_at_generation,
                } => {
                    let mut self_root_canonicals: Vec<Arc<str>> =
                        Vec::with_capacity(observed_self_roots.len());
                    for (canonical, _) in &observed_self_roots {
                        if !self_root_canonicals.iter().any(|root| root == canonical) {
                            self_root_canonicals.push(Arc::clone(canonical));
                        }
                    }
                    match semantic_graph_read_set_signature(&observed_self_roots, &facts) {
                        Some(carrier) => self.insert_relation_owned(
                            Some(ctx),
                            key,
                            carrier,
                            Arc::from(self_root_canonicals),
                            result,
                            validated_at_generation,
                        ),
                        None => crate::cache_runtime::admission::propagate_non_admission(
                            crate::cache_runtime::NonAdmissionReason::UnresolvedProvenance,
                        ),
                    }
                }
                RelationPublishDecision::ReturnOnly(reason) => {
                    crate::cache_runtime::admission::propagate_non_admission(reason);
                }
            },
            crate::resolver_core::FactReadSetFinalise::NonCacheable(_) => {
                crate::cache_runtime::admission::propagate_non_admission(
                    crate::cache_runtime::NonAdmissionReason::UnresolvedProvenance,
                );
            }
            crate::resolver_core::FactReadSetFinalise::Overflow => {
                crate::cache_runtime::admission::propagate_non_admission(
                    crate::cache_runtime::NonAdmissionReason::SignatureOverflow,
                );
            }
        }
        value
    }

    /// Publish a relation judgement for the full relation identity `key`
    /// into the family memo's [`FamilyKey::Relate`] family
    /// ([`ModeSlot::Single`] slot) — the sole relation-memo write shape,
    /// shared by the production path ([`Self::compute_relation_and_admit`])
    /// and the test seed seams.
    ///
    /// **The compute/public conversion happens HERE, at the publish
    /// path.** The engine's transient tri-state
    /// [`RelationResult`](crate::semantic_query::RelationResult) converts
    /// into the stored compute-side verdict
    /// ([`SemanticQueryValue::RelationVerdict`] wrapping
    /// [`RelationComputeResult`](crate::semantic_query::RelationComputeResult)):
    /// decided judgements ride the `Decided` arm (the publicly-representable
    /// subset), a clean `Unknown` rides `Undecided(UndecidedReason::Unknown)`
    /// and stays admitted EXACTLY as the dedicated memo admitted it
    /// (future admission work deletes that admission — the current contract
    /// preserves it byte-equivalent). The public `RelationPayload` is never
    /// fabricated:
    /// no proof table exists, so a `relation_proof` id would be a lie.
    ///
    /// The entry is self-version-rooted: `carrier` is built by
    /// [`semantic_graph_read_set_signature`] from the relation build's
    /// observed self-roots; `self_root_canonicals` is checked strictly,
    /// and `validated_at_generation` joins the candidate discriminant
    /// (same generation + same facts ⇒ in-place replace) and gates the
    /// warm read on a project-shape bump.
    ///
    /// Retention rides the family rails: the per-family
    /// [`FamilyKey::candidate_cap`] with invalid-first / LRU eviction
    /// (`ctx = Some` plans the victim against the publisher's stable view;
    /// `None` — the legacy test seam — falls back to the LRU front, the
    /// same convention as the family `*_for_tests` publishes), the family
    /// `memo_budget` provides the global bound, and the reverse-index
    /// registration makes per-canonical drains see relation entries. The
    /// `(entries slot, memo_budget record, reverse-index register)`
    /// triple lands under the ONE `entries` lock, atomic against a
    /// concurrent `invalidate_all` — the family consistency-cluster
    /// discipline.
    fn insert_relation_owned(
        &self,
        ctx: Option<&dyn crate::resolver_core::ResolverContext>,
        key: crate::semantic_query::RelateMemoKey,
        carrier: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        result: crate::semantic_query::RelationResult,
        validated_at_generation: u64,
    ) {
        let family = FamilyKey::Relate { key: Box::new(key) };
        let slot = ModeSlot::Single;
        let admission_seq = self.alloc_candidate_admission_seq();
        // The relation carrier is the sole fact rail; the dispatch-fence
        // slot carries the interned empty signature so the entry's shape
        // matches every other family's (the warm read returns
        // `empty_signature()` for the same reason).
        let dispatch_dep_signature = self.dep_signature_interner.intern(&empty_signature());
        let entry = MemoEntry {
            result: QueryResult::Value(SemanticQueryValue::RelationVerdict(result.into())),
            read_set_signature: carrier.clone(),
            dispatch_dep_signature: Arc::clone(&dispatch_dep_signature),
            self_root_canonicals,
            walker_diagnostics: Arc::from([]),
            satisfied_projection: relation_satisfied_projection(),
            validated_at_generation,
            admission_seq,
        };
        // Per-family bounded retention: plan the cap eviction BEFORE the
        // publish lock (the plan releases `entries` before validating fact
        // rails — never validate under the global memo mutex).
        let cap = family.candidate_cap();
        let eviction = match ctx {
            Some(ctx) => {
                family::plan_family_slot_eviction(&self.entries, &family, slot, &entry, cap, ctx)
            }
            None => family::EvictionVictim::LruFront,
        };
        let mut entries = self.entries_lock_diagnosed();
        // Record whether this relation identity is newly entering the memo
        // so the retention budget tracks one ledger record per family.
        let family_was_new = !entries.contains_key(&family);
        let outcome = entries.entry(family.clone()).or_default().publish(
            slot,
            entry,
            &ProjectionPath::empty(),
            cap,
            eviction,
        );
        let populated_slots = outcome.populated;
        // Drain the displaced candidates' reverse-index registrations by
        // per-candidate `admission_seq` (same-discriminant replacements +
        // cap-eviction victims), under the held `entries` lock.
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
            &carrier,
            &dispatch_dep_signature,
            admission_seq,
        );
        drop(entries);
    }

    /// Test-support seed seam for relation-memo fixtures. Production writes
    /// route through [`Self::compute_relation_and_admit`]. Publishes with
    /// the legacy no-view eviction policy (LRU front), mirroring the family
    /// `publish_with_carrier_for_tests` convention.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_relation(
        &self,
        key: crate::semantic_query::RelateMemoKey,
        carrier: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        result: crate::semantic_query::RelationResult,
        validated_at_generation: u64,
    ) {
        self.insert_relation_owned(
            None,
            key,
            carrier,
            self_root_canonicals,
            result,
            validated_at_generation,
        );
    }

    /// Host-view-aware variant of [`Self::insert_relation`]: the publish
    /// plans its per-family bounded-retention eviction against the
    /// publishing caller's stable store view (invalid-first victim
    /// selection), mirroring the family `publish_with_view_for_tests`
    /// convention. Backs the per-family bounded-retention relation guards.
    #[doc(hidden)]
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_relation_with_view_for_tests(
        &self,
        host: &crate::VerterHost,
        key: crate::semantic_query::RelateMemoKey,
        carrier: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        result: crate::semantic_query::RelationResult,
        validated_at_generation: u64,
    ) {
        self.insert_relation_owned(
            Some(host),
            key,
            carrier,
            self_root_canonicals,
            result,
            validated_at_generation,
        );
    }

    /// Count of relation memo entries (the summed candidate count of every
    /// [`FamilyKey::Relate`] family in the family memo). Useful for tests
    /// and counters.
    #[must_use]
    pub fn relation_memo_count(&self) -> usize {
        let entries = self.entries_lock_diagnosed();
        entries
            .iter()
            .filter(|(family, _)| matches!(family, FamilyKey::Relate { .. }))
            .map(|(_, slots)| slots.slot_candidate_count_for_test(ModeSlot::Single))
            .sum()
    }

    /// Drop every relation memo entry. Invoked on project-generation bumps
    /// so warm relation judgements cannot leak across a version boundary.
    /// Removes every [`FamilyKey::Relate`] family from the family memo and
    /// drains each candidate's reverse-index registrations and the family's
    /// `memo_budget` ledger record — the three-member consistency cluster
    /// mutated under the ONE `entries` lock, exclusive against concurrent
    /// publishers (which mutate the same cluster under the same lock).
    pub fn clear_relation_memo(&self) {
        let mut entries = self.entries_lock_diagnosed();
        let relate_families: Vec<FamilyKey> = entries
            .keys()
            .filter(|family| matches!(family, FamilyKey::Relate { .. }))
            .cloned()
            .collect();
        for family in relate_families {
            let Some(slots) = entries.remove(&family) else {
                continue;
            };
            for (slot, entry) in slots.iter_populated_slots_all() {
                reverse_index::drain_candidate_reverse_index_registrations(
                    &self.canonical_to_entries,
                    &family,
                    slot,
                    entry,
                );
            }
            self.memo_budget.forget_key_under_exclusive_lock(&family);
        }
    }
}

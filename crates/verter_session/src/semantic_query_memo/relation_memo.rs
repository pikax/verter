//! Relation storage — the payload read/write path over the family memo's
//! `Relate` family, plus the payload-side relation-proof interners
//! (design `docs/arch/u2-relation-infer-design.md` Decision 4).
//!
//! Storage is the family memo's [`FamilyKey::Relate`] family in the
//! [`ModeSlot::Single`] slot. The stored value is the PUBLIC
//! [`SemanticQueryValue::Relation`] payload — decided binary
//! `Assignable`/`NotAssignable` outcomes ONLY: `Unknown` has no
//! value-domain form and is never admitted anywhere (memo / fact /
//! reverse index), and a `BudgetExceeded` payload is public but
//! ReturnOnly (never written here). Warm reads validate the
//! self-version-rooted carrier strictly AND hard-miss on a
//! `validated_at_generation` mismatch. Retention rides the family rails
//! (per-family cap, invalid-first / LRU eviction, the family
//! `memo_budget` global bound, reverse-index drains).
//!
//! The proofs a payload references by [`crate::semantic_query::RelationProofId`]
//! / [`crate::semantic_query::RelateKeyId`] live in the two store-owned
//! append-only interners here — the payload-side `relation_proofs` table
//! backing, OFF the type-values surface.

use super::*;

/// The `satisfied_projection` every relation entry carries: the modeless
/// [`ModeSlot::Single`] identity point at the empty path, so the family
/// materialisation gates treat relation entries exactly like any other
/// modeless family's (the gate never blocks a modeless hit).
/// The SCC-union publish carrier of the relation ROOT's just-published
/// family entry (see [`SemanticGraphStore::relation_published_carrier`]).
#[derive(Debug, Clone)]
pub(crate) struct RelationPublishedCarrier {
    pub(crate) read_set_signature: crate::fact_signature_helpers::ReadSetSignature,
    pub(crate) self_root_canonicals: Arc<[Arc<str>]>,
    pub(crate) validated_at_generation: u64,
}

fn relation_satisfied_projection() -> MaterializedSet {
    MaterializedSet::single(MaterializedPoint::new(family::point_for_slot(
        ModeSlot::Single,
        &ProjectionPath::empty(),
    )))
}

impl SemanticGraphStore {
    /// Intern a relation proof, returning its opaque
    /// [`crate::semantic_query::RelationProofId`] (deduplicated by value).
    /// The proof rides the payload-side table — it is NOT a
    /// validity oracle (warm-hit validity is decided SOLELY by the
    /// `ReadSetSignature` rail).
    pub(crate) fn intern_relation_proof(
        &self,
        proof: crate::semantic_query::RelationProof,
    ) -> crate::semantic_query::RelationProofId {
        let mut table = self.relation_proof_table.lock();
        if let Some(id) = table.1.get(&proof) {
            return *id;
        }
        let id = crate::semantic_query::RelationProofId(table.0.len() as u32);
        table.0.push(proof.clone());
        table.1.insert(proof, id);
        id
    }

    /// Intern a COMPLETED full `Relate` key for a `CoinductiveCycle`
    /// proof, returning its opaque [`crate::semantic_query::RelateKeyId`].
    pub(crate) fn intern_relate_key(
        &self,
        key: crate::semantic_query::RelateMemoKey,
    ) -> crate::semantic_query::RelateKeyId {
        let mut table = self.relate_key_table.lock();
        if let Some(id) = table.1.get(&key) {
            return *id;
        }
        let id = crate::semantic_query::RelateKeyId(table.0.len() as u32);
        table.0.push(key.clone());
        table.1.insert(key, id);
        id
    }

    /// Read back a proof by id (test + future display surface).
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn relation_proof_for(
        &self,
        id: crate::semantic_query::RelationProofId,
    ) -> Option<crate::semantic_query::RelationProof> {
        self.relation_proof_table
            .lock()
            .0
            .get(id.0 as usize)
            .cloned()
    }

    /// Read back a co-discharged key by id (test surface).
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn relate_key_for_id(
        &self,
        id: crate::semantic_query::RelateKeyId,
    ) -> Option<crate::semantic_query::RelateMemoKey> {
        self.relate_key_table.lock().0.get(id.0 as usize).cloned()
    }

    /// Strict warm-hit read of a published relation payload for the full
    /// relation identity `key`.
    ///
    /// Returns the PUBLIC [`crate::semantic_query::RelationPayload`] **only
    /// when** the stored entry's self-version-rooted carrier validates
    /// against the live store view AND the entry's
    /// `validated_at_generation` still equals the live project generation
    /// — the same gate the retired `get_relation` enforced. A stale entry
    /// (same-canonical content edit, untracked self-root, or
    /// `ProjectGeneration` bump) returns `None`. Only decided binary
    /// payloads are ever stored, so a hit is always a determinate
    /// judgement.
    #[must_use]
    pub(crate) fn get_relation_payload(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        key: &crate::semantic_query::RelateMemoKey,
    ) -> Option<crate::semantic_query::RelationPayload> {
        let family = FamilyKey::Relate {
            key: Box::new(key.clone()),
        };
        // Snapshot the candidate list under the `entries` lock, then
        // validate OUTSIDE the lock (the family warm-read discipline:
        // validation may re-enter the memo through the resolver view).
        let snapshot: CandidateList = {
            let entries = self.entries_lock_diagnosed();
            entries
                .get(&family)
                .map(|slots| slots.snapshot_slot(ModeSlot::Single))?
        };
        let live_generation = ctx.project_type_store().current_project_generation();
        let hit = snapshot.into_iter().find(|entry| {
            entry.validated_at_generation == live_generation && entry.validate(ctx)
        })?;
        // Brief LRU bookkeeping — promote the hit candidate in the slot's
        // recency order (a concurrent invalidation makes this a no-op).
        {
            let mut entries = self.entries_lock_diagnosed();
            if let Some(slots) = entries.get_mut(&family) {
                slots.mark_validated_freshest(ModeSlot::Single, &hit);
            }
        }
        hit.read_set_signature.bubble(ctx);
        match hit.result {
            QueryResult::Value(SemanticQueryValue::Relation(payload)) => Some(payload),
            // Structural invariant: the relation authority only ever
            // stores `Relation` payloads in `Relate` family entries.
            other => {
                unreachable!("Relate family entries store Relation payloads only; found {other:?}")
            }
        }
    }

    /// Read back the just-published carrier (read-set signature,
    /// self-root canonicals, generation stamp) of the relation ROOT's
    /// family entry — the SCC-union carrier the batched member publish
    /// rides (design §2.3: the published fact set is the UNION of all SCC
    /// members' observed facts, never the bare per-member set).
    #[must_use]
    pub(crate) fn relation_published_carrier(
        &self,
        key: &crate::semantic_query::RelateMemoKey,
    ) -> Option<RelationPublishedCarrier> {
        let family = FamilyKey::Relate {
            key: Box::new(key.clone()),
        };
        let entries = self.entries_lock_diagnosed();
        let slots = entries.get(&family)?;
        let snapshot = slots.snapshot_slot(ModeSlot::Single);
        let entry = snapshot.into_iter().next_back()?;
        Some(RelationPublishedCarrier {
            read_set_signature: entry.read_set_signature.clone(),
            self_root_canonicals: Arc::clone(&entry.self_root_canonicals),
            validated_at_generation: entry.validated_at_generation,
        })
    }

    /// Publish a DECIDED SCC member's payload under the SCC-union carrier
    /// (design §2.3 step 4 batched admission). Only ever called with a
    /// binary `Assignable`/`NotAssignable` payload by the relation
    /// authority's drain — `Unknown` / `BudgetExceeded` never reach here.
    ///
    /// Retention rides the family rails (per-family cap with invalid-first
    /// / LRU eviction, `memo_budget`, reverse-index registration), and the
    /// `(entries slot, memo_budget record, reverse-index register)` triple
    /// lands under the ONE `entries` lock — the family consistency-cluster
    /// discipline.
    pub(crate) fn publish_relation_member(
        &self,
        ctx: Option<&dyn crate::resolver_core::ResolverContext>,
        key: crate::semantic_query::RelateMemoKey,
        payload: crate::semantic_query::RelationPayload,
        carrier: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        validated_at_generation: u64,
    ) {
        debug_assert!(
            matches!(
                payload.outcome,
                crate::semantic_query::RelationOutcome::Assignable
                    | crate::semantic_query::RelationOutcome::NotAssignable
            ),
            "only decided binary relation payloads publish; {payload:?} must route ReturnOnly"
        );
        let family = FamilyKey::Relate { key: Box::new(key) };
        let slot = ModeSlot::Single;
        let admission_seq = self.alloc_candidate_admission_seq();
        let dispatch_dep_signature = self.dep_signature_interner.intern(&empty_signature());
        let entry = MemoEntry {
            result: QueryResult::Value(SemanticQueryValue::Relation(payload)),
            read_set_signature: carrier.clone(),
            dispatch_dep_signature: Arc::clone(&dispatch_dep_signature),
            self_root_canonicals,
            walker_diagnostics: Arc::from([]),
            satisfied_projection: relation_satisfied_projection(),
            validated_at_generation,
            admission_seq,
        };
        let cap = family.candidate_cap();
        let eviction = match ctx {
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
            &ProjectionPath::empty(),
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
            &carrier,
            &dispatch_dep_signature,
            admission_seq,
        );
        drop(entries);
    }

    /// Test-support enumeration of every published `Relate` family entry as
    /// `(key, outcome)` (freshest candidate per slot). Lets relation tests
    /// assert over the ACTUAL published set instead of probing guessed keys.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn relation_entries_for_tests(
        &self,
    ) -> Vec<(
        crate::semantic_query::RelateMemoKey,
        crate::semantic_query::RelationOutcome,
    )> {
        let entries = self.entries_lock_diagnosed();
        entries
            .iter()
            .filter_map(|(family, slots)| {
                let FamilyKey::Relate { key } = family else {
                    return None;
                };
                let snapshot = slots.snapshot_slot(ModeSlot::Single);
                snapshot.into_iter().next_back().map(|entry| {
                    let outcome = match &entry.result {
                        QueryResult::Value(SemanticQueryValue::Relation(payload)) => {
                            payload.outcome.clone()
                        }
                        other => unreachable!(
                            "Relate family entries store Relation payloads only; found {other:?}"
                        ),
                    };
                    ((**key).clone(), outcome)
                })
            })
            .collect()
    }

    /// Test-support: intern the CONSTRUCT twin of a call `Signature` node
    /// (same params/return/type-params/spans, `kind: Construct`).
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn intern_construct_twin_for_tests(
        &self,
        call: crate::semantic_query::SemanticNodeId,
    ) -> crate::semantic_query::SemanticNodeId {
        let data = self
            .node_data(call)
            .expect("construct twin source must be interned");
        let crate::semantic_query::SemanticNodeData::Signature {
            kind: _,
            params,
            return_type,
            type_parameters,
            signature_span,
            return_type_span,
        } = data.as_ref()
        else {
            panic!("construct twin source must be a Signature node");
        };
        self.intern_node(crate::semantic_query::SemanticNodeData::Signature {
            kind: crate::semantic_query::SignatureKind::Construct,
            params: Arc::clone(params),
            return_type: *return_type,
            type_parameters: Arc::clone(type_parameters),
            signature_span: *signature_span,
            return_type_span: *return_type_span,
        })
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

    /// Test-support seed seam for relation fixtures (mirrors the retired
    /// `insert_relation` shape): publishes a DECIDED payload with the
    /// legacy no-view eviction policy (LRU front). Production writes route
    /// through the relation authority's batched publish
    /// ([`Self::publish_relation_member`]) or the family singleflight.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_relation_payload_for_tests(
        &self,
        key: crate::semantic_query::RelateMemoKey,
        carrier: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        payload: crate::semantic_query::RelationPayload,
        validated_at_generation: u64,
    ) {
        self.publish_relation_member(
            None,
            key,
            payload,
            carrier,
            self_root_canonicals,
            validated_at_generation,
        );
    }

    /// Host-view-aware variant of [`Self::insert_relation_payload_for_tests`]:
    /// the publish plans its per-family bounded-retention eviction against
    /// the publishing caller's stable store view (invalid-first victim
    /// selection), backing the per-family bounded-retention relation guards.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn insert_relation_payload_with_view_for_tests(
        &self,
        host: &crate::VerterHost,
        key: crate::semantic_query::RelateMemoKey,
        carrier: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        payload: crate::semantic_query::RelationPayload,
        validated_at_generation: u64,
    ) {
        self.publish_relation_member(
            Some(host),
            key,
            payload,
            carrier,
            self_root_canonicals,
            validated_at_generation,
        );
    }

    /// Test-support payload constructor: a decided outcome with a default
    /// root-witness proof interned (fixtures that need a publishable
    /// payload without driving the reducer).
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn relation_payload_for_tests(
        &self,
        outcome: crate::semantic_query::RelationOutcome,
    ) -> crate::semantic_query::RelationPayload {
        let proof = match &outcome {
            crate::semantic_query::RelationOutcome::Assignable => {
                crate::semantic_query::RelationProof::Assignable {
                    witness: crate::semantic_query::DerivationTree {
                        sub_derivations: Arc::from(Vec::new().into_boxed_slice()),
                    },
                }
            }
            crate::semantic_query::RelationOutcome::NotAssignable => {
                crate::semantic_query::RelationProof::NotAssignable {
                    reason: crate::semantic_query::RelationFailureCode::Structural,
                    failing_sub: crate::semantic_query::SubRelationRef {
                        source: crate::semantic_query::SemanticNodeId(u64::MAX),
                        target: crate::semantic_query::SemanticNodeId(u64::MAX),
                        position: crate::semantic_query::SubRelationPosition::Root,
                    },
                }
            }
            crate::semantic_query::RelationOutcome::BudgetExceeded(kind) => {
                crate::semantic_query::RelationProof::BudgetExceeded {
                    cap: crate::semantic_query::RecursionOrBudgetCap {
                        kind: *kind,
                        limit: 0,
                    },
                }
            }
        };
        let relation_proof = self.intern_relation_proof(proof);
        crate::semantic_query::RelationPayload {
            outcome,
            bindings: Arc::from(Vec::new().into_boxed_slice()),
            relation_proof,
        }
    }
}

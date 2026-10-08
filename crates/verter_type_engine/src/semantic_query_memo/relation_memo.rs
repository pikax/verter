//! Relation storage — the payload read/write path over the family memo's
//! `Relate` family.
//!
//! Writes land through the batched SCC member publish in
//! [`super::scc_publish`] — the one store-owned admission path every
//! deferred domain rides.
//!
//! Storage is the family memo's [`FamilyKey::Relate`] family in the
//! [`ModeSlot::Single`] slot. The stored value is the PUBLIC
//! [`SemanticQueryValue::Relation`] payload — decided binary
//! `Assignable`/`NotAssignable` outcomes ONLY: `Unknown` has no
//! value-domain form and is never admitted anywhere (memo / fact /
//! reverse index), and a `BudgetExceeded` payload is public but
//! ReturnOnly (never written here). Warm reads validate the
//! self-version-rooted carrier strictly. Project-shape invalidation
//! rides `FactVersionRef::ProjectGeneration` on that carrier. Retention rides the family rails
//! (per-family cap, invalid-first / LRU eviction, the family
//! `memo_budget` global bound, reverse-index drains).
//!
//! The store retains no explanation of a relation: a payload names no proof
//! and no co-discharged key, so a relation entry's residency is its memo
//! candidate alone, and eviction or a document close leaves nothing behind.
//! Explanations are optional request-owned capture
//! (`project_semantic_dispatch::relation_explanation`).

use super::*;

/// Test-observer name for a relation family's published candidate.
#[cfg(any(test, feature = "test-support"))]
pub(crate) type RelationPublishedCarrier = PublishedMemoCandidate;

/// The `satisfied_projection` every relation entry carries: the modeless
/// [`ModeSlot::Single`] identity point at the empty path, so the family
/// materialisation gates treat relation entries exactly like any other
/// modeless family's (the gate never blocks a modeless hit).
pub(super) fn relation_satisfied_projection() -> MaterializedSet {
    MaterializedSet::single(MaterializedPoint::new(family::point_for_slot(
        ModeSlot::Single,
        &ProjectionPath::empty(),
    )))
}

impl SemanticGraphStore {
    /// Claim the ordinary family flight for a non-binding relation member
    /// that will be computed inline. `None` means another cold owner already
    /// owns this exact full relation key.
    ///
    /// Binding relation roots are refused here rather than in the shared
    /// claim: they run on independently-owned cooperative flights, so
    /// claiming the ordinary family flight for one would collide with the
    /// root's own admission.
    pub fn begin_inline_relation_flight(
        &self,
        key: &crate::semantic_query::RelateMemoKey,
    ) -> Option<InlineMemberFlight> {
        verter_debug_assert!(
            key.inference_context.is_none(),
            "binding relation roots use independently-owned cooperative flights"
        );
        self.begin_inline_member_flight(key.to_query_key())
    }

    /// Strict warm-hit read of a published relation payload for the full
    /// relation identity `key`.
    ///
    /// Returns the PUBLIC [`crate::semantic_query::RelationPayload`] **only
    /// when** the stored entry's self-version-rooted carrier validates
    /// against the live store view — the carrier's
    /// `FactVersionRef::ProjectGeneration` fact is the validity oracle
    /// (`validated_at_generation` is recency metadata, not a gate). A stale
    /// entry (same-canonical content edit, untracked self-root, or
    /// `ProjectGeneration` bump) returns `None`. Only decided binary
    /// payloads are ever stored, so a hit is always a determinate
    /// judgement.
    #[must_use]
    pub fn get_relation_payload<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        key: &crate::semantic_query::RelateMemoKey,
    ) -> Option<Served<crate::semantic_query::RelationPayload>> {
        let family = FamilyKey::Relate {
            key: super::family_intern::InternedRelateKey::intern(key.clone()),
        };
        // A relation entry always materialises the modeless identity
        // point, so the §3.4 gate is the modeless identity point every
        // entry records — only carrier validation can block.
        let requested = MaterializedPoint::new(family::point_for_slot(
            ModeSlot::Single,
            &ProjectionPath::empty(),
        ));
        // Miss-neutral probe: a miss falls through to the owning
        // cooperative dispatch, which records the single miss (see
        // `get_validated_value_impl`'s `record_miss` contract).
        let mut receipt = None;
        let hit = self
            .get_validated_value_impl(
                &family,
                ModeSlot::Single,
                &requested,
                ctx,
                None,
                None,
                &mut receipt,
                false,
            )?
            .value;
        let receipt = receipt.expect("a served candidate carries its receipt");
        match hit {
            QueryResult::Value(SemanticQueryValue::Relation(payload)) => Some(Served {
                read: payload,
                receipt,
            }),
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
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn relation_published_carrier(
        &self,
        key: &crate::semantic_query::RelateMemoKey,
    ) -> Option<RelationPublishedCarrier> {
        let family = FamilyKey::Relate {
            key: super::family_intern::InternedRelateKey::intern(key.clone()),
        };
        let entries = self.entries_lock_diagnosed();
        let slots = entries.get(&family)?;
        let snapshot = slots.snapshot_slot(ModeSlot::Single);
        let entry = snapshot.into_iter().next_back()?;
        Some(PublishedMemoCandidate {
            read_set_signature: entry.read_set_signature.clone(),
            self_root_canonicals: Arc::clone(&entry.self_root_canonicals),
            validated_at_generation: entry.validated_at_generation,
            admission_seq: entry.admission_seq,
            cost_receipt: Arc::clone(&entry.cost_receipt),
        })
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
            occurrence,
            return_carrier,
            signature_span,
            return_type_span,
            predicate,
            is_abstract,
        } = data.as_ref()
        else {
            panic!("construct twin source must be a Signature node");
        };
        self.intern_node(crate::semantic_query::SemanticNodeData::Signature {
            kind: crate::semantic_query::SignatureKind::Construct,
            params: Arc::clone(params),
            return_type: *return_type,
            type_parameters: Arc::clone(type_parameters),
            occurrence: occurrence.clone(),
            return_carrier: return_carrier.clone(),
            signature_span: *signature_span,
            return_type_span: *return_type_span,
            predicate: *predicate,
            is_abstract: *is_abstract,
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

    /// Test-support count of admitted relation candidates on one relation
    /// axis. This observes the actual family key; it does not issue queries.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn relation_memo_count_of_kind(
        &self,
        relation: crate::semantic_query::RelationKind,
    ) -> usize {
        let entries = self.entries_lock_diagnosed();
        entries
            .iter()
            .filter(|(family, _)| {
                matches!(family, FamilyKey::Relate { key } if key.relation == relation)
            })
            .map(|(_, slots)| slots.slot_candidate_count_for_test(ModeSlot::Single))
            .sum()
    }

    /// Test-support seed seam for relation fixtures (mirrors the retired
    /// `insert_relation` shape): publishes a DECIDED payload with the
    /// legacy no-view eviction policy (LRU front). This seeds a ROOT, so
    /// it takes no root witness and no flight — production member writes
    /// route through the store-owned batched SCC publish.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_relation_payload_for_tests(
        &self,
        key: crate::semantic_query::RelateMemoKey,
        carrier: verter_session_query::facts::fact_cache::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        payload: crate::semantic_query::RelationPayload,
        validated_at_generation: u64,
    ) {
        self.publish_unfenced_candidate_for_tests(
            None,
            FamilyKey::Relate {
                key: super::family_intern::InternedRelateKey::intern(key),
            },
            SemanticQueryValue::Relation(payload),
            relation_satisfied_projection(),
            carrier,
            self_root_canonicals,
            validated_at_generation,
        );
    }

    /// Store-view-aware variant of [`Self::insert_relation_payload_for_tests`]:
    /// the publish plans its per-family bounded-retention eviction against
    /// the publishing caller's stable store view (invalid-first victim
    /// selection), backing the per-family bounded-retention relation guards.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn insert_relation_payload_with_view_for_tests(
        &self,
        view: &dyn crate::resolver_core::fact_validation_port::FactValidation,
        key: crate::semantic_query::RelateMemoKey,
        carrier: verter_session_query::facts::fact_cache::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        payload: crate::semantic_query::RelationPayload,
        validated_at_generation: u64,
    ) {
        self.publish_unfenced_candidate_for_tests(
            Some(view),
            FamilyKey::Relate {
                key: super::family_intern::InternedRelateKey::intern(key),
            },
            SemanticQueryValue::Relation(payload),
            relation_satisfied_projection(),
            carrier,
            self_root_canonicals,
            validated_at_generation,
        );
    }

    /// Test-support payload constructor: a decided outcome with no
    /// bindings (fixtures that need a publishable payload without driving
    /// the reducer).
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn relation_payload_for_tests(
        &self,
        outcome: crate::semantic_query::RelationOutcome,
    ) -> crate::semantic_query::RelationPayload {
        crate::semantic_query::RelationPayload {
            outcome,
            bindings: Arc::from(Vec::new().into_boxed_slice()),
            recursion: crate::semantic_query::RelationRecursionFootprint::default(),
        }
    }
}

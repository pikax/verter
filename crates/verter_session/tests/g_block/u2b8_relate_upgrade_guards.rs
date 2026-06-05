//! U2B.8 guards — the upgraded full-identity `Relate` key surface.
//!
//! These tests pin the IDENTITY contract of the upgraded
//! [`SemanticQueryKey::Relate`] variant — no longer the bare
//! `(source, target)` pair but the full relation identity (relation kind,
//! comparison policy, source freshness, optional inference context, and the
//! `R T L J` + substitution + projection-reduction [`RelationContext`]) — the
//! RE-KEYING of the relation memo onto the full [`RelateMemoKey`] (so two
//! judgements over the same nodes that differ in any identity axis occupy
//! DISTINCT memo slots), the DEDICATED non-aliasing family mapping (`Relate` →
//! `FamilyKey::Relate`, never `FamilyKey::IndexedAccess`), the value-domain
//! mapping (`Relate` → `Relation`, never `TypeNode`), and the shape of the
//! forward-declared [`RelationPayload`] carrier (public three-valued outcome +
//! inference bindings + an opaque [`RelationProofId`] into the payload-side
//! [`RelationProofTable`]; budget folded into the outcome; no public `Unknown`).
//!
//! Each warm-hit guard is DISCRIMINATING against the retired bare-pair key: a
//! relation memo keyed on `(source, target)` would collapse every variant in a
//! group onto ONE slot, so the slot-count assertions would read `1` and FAIL;
//! the full-identity key splits them and the assertions PASS.

use std::sync::Arc;

use verter_session::for_tests::{
    family_key_size_for_tests, family_variant_label_for_tests, ReadSetSignature,
};
use verter_session::semantic_query::query_key_spec::semantic_query_key_specs;
use verter_session::semantic_query::{
    BudgetExceededKind, ConstParamPolicy, ContextualInferenceMode, DerivationTree, FreshnessKey,
    IndexKey, InferableParamSetId, InferenceCandidatePriority, InferenceContextKey, NoInferMask,
    OverloadSelectionPolicy, PrimitiveKind, ProjectionMode, ProjectionReductionContext,
    RecursionOrBudgetCap, RelateKeyId, RelateMemoKey, RelationContext, RelationFailureReason,
    RelationKind, RelationOutcome, RelationPayload, RelationPolicy, RelationProof, RelationProofId,
    RelationProofTable, RelationResult, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    SemanticQueryKeyTag, SemanticQueryValue, SemanticQueryValueTag, SubRelationPosition,
    SubRelationRef, SubstitutionCanonicalHash, VariancePhase, VariancePolicy,
};
use verter_session::{HostConfig, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn hash16(byte: u8) -> [u8; 16] {
    [byte; 16]
}

fn relation_context(r: u8, t: u8, l: u8, j: u8) -> RelationContext {
    RelationContext {
        resolve_env_hash: hash16(r),
        type_env_hash: hash16(t),
        lib_env_hash: hash16(l),
        project_identity: hash16(j),
        ..RelationContext::default()
    }
}

/// A full-identity `Relate` key with every discriminator explicit.
#[allow(clippy::too_many_arguments)]
fn relate_key(
    source: SemanticNodeId,
    target: SemanticNodeId,
    relation: RelationKind,
    policy: RelationPolicy,
    source_freshness: FreshnessKey,
    inference_context: Option<InferenceContextKey>,
    context: RelationContext,
) -> SemanticQueryKey {
    SemanticQueryKey::Relate {
        source,
        target,
        relation,
        policy,
        source_freshness,
        inference_context,
        context,
    }
}

// ---------------------------------------------------------------------------
// (1) The `Relate` KEY identity covers relation kind / policy / freshness /
//     inference context / env — every discriminator is part of `Hash`/`Eq`
//     identity. Two keys differing in ONE discriminator are non-equal.
// ---------------------------------------------------------------------------

#[test]
fn relate_key_covers_relation_kind_policy_freshness_and_context() {
    let s = SemanticNodeId(1);
    let t = SemanticNodeId(2);
    let base = relate_key(
        s,
        t,
        RelationKind::Assignable,
        RelationPolicy::default(),
        FreshnessKey::Regular,
        None,
        relation_context(0, 0, 0, 0),
    );

    // Same key built twice is equal — the identity is a total function of its
    // fields (sanity, so the `assert_ne`s below are not vacuous).
    assert_eq!(
        base,
        relate_key(
            s,
            t,
            RelationKind::Assignable,
            RelationPolicy::default(),
            FreshnessKey::Regular,
            None,
            relation_context(0, 0, 0, 0),
        ),
        "two Relate keys with identical fields must be equal"
    );

    // Relation kind is identity.
    assert_ne!(
        base,
        relate_key(
            s,
            t,
            RelationKind::Identity,
            RelationPolicy::default(),
            FreshnessKey::Regular,
            None,
            relation_context(0, 0, 0, 0),
        ),
        "a different relation kind must be a DISTINCT key"
    );
    // Policy: overload-selection axis is identity.
    assert_ne!(
        base,
        relate_key(
            s,
            t,
            RelationKind::Assignable,
            RelationPolicy {
                overload_selection: OverloadSelectionPolicy::FirstApplicable,
                ..RelationPolicy::default()
            },
            FreshnessKey::Regular,
            None,
            relation_context(0, 0, 0, 0),
        ),
        "a different overload-selection policy must be a DISTINCT key"
    );
    // Policy: excess-property axis is identity.
    assert_ne!(
        base,
        relate_key(
            s,
            t,
            RelationKind::Assignable,
            RelationPolicy {
                excess_property_check: true,
                ..RelationPolicy::default()
            },
            FreshnessKey::Regular,
            None,
            relation_context(0, 0, 0, 0),
        ),
        "a different excess-property policy must be a DISTINCT key"
    );
    // Policy: variance axis (incl. method-parameter bivariance) is identity.
    assert_ne!(
        base,
        relate_key(
            s,
            t,
            RelationKind::Assignable,
            RelationPolicy {
                variance: VariancePolicy::StrictContravariance,
                ..RelationPolicy::default()
            },
            FreshnessKey::Regular,
            None,
            relation_context(0, 0, 0, 0),
        ),
        "a different variance policy must be a DISTINCT key"
    );
    // Source freshness is identity.
    assert_ne!(
        base,
        relate_key(
            s,
            t,
            RelationKind::Assignable,
            RelationPolicy::default(),
            FreshnessKey::Fresh,
            None,
            relation_context(0, 0, 0, 0),
        ),
        "a different source freshness must be a DISTINCT key"
    );
    // Each env dimension (R, T, L, J) is identity.
    for ctx in [
        relation_context(9, 0, 0, 0),
        relation_context(0, 9, 0, 0),
        relation_context(0, 0, 9, 0),
        relation_context(0, 0, 0, 9),
    ] {
        assert_ne!(
            base,
            relate_key(
                s,
                t,
                RelationKind::Assignable,
                RelationPolicy::default(),
                FreshnessKey::Regular,
                None,
                ctx,
            ),
            "a different env dimension must be a DISTINCT key"
        );
    }
    // The substitution axis on the context is identity.
    assert_ne!(
        base,
        relate_key(
            s,
            t,
            RelationKind::Assignable,
            RelationPolicy::default(),
            FreshnessKey::Regular,
            None,
            RelationContext {
                substitution: SubstitutionCanonicalHash(hash16(9)),
                ..relation_context(0, 0, 0, 0)
            },
        ),
        "a different canonical substitution must be a DISTINCT key"
    );
    // The projection-reduction axis on the context is identity.
    assert_ne!(
        base,
        relate_key(
            s,
            t,
            RelationKind::Assignable,
            RelationPolicy::default(),
            FreshnessKey::Regular,
            None,
            RelationContext {
                projection_reduction:
                    verter_session::semantic_query::ProjectionReductionContext::published(
                        ProjectionMode::Expanded,
                    ),
                ..relation_context(0, 0, 0, 0)
            },
        ),
        "a different projection-reduction context must be a DISTINCT key"
    );
    // Source / target nodes are identity.
    assert_ne!(
        base,
        relate_key(
            SemanticNodeId(7),
            t,
            RelationKind::Assignable,
            RelationPolicy::default(),
            FreshnessKey::Regular,
            None,
            relation_context(0, 0, 0, 0),
        ),
        "a different source node must be a DISTINCT key"
    );
}

// ---------------------------------------------------------------------------
// (2) RELATION MEMO RE-KEY: same nodes, different relation kind / policy / env
//     occupy DISTINCT memo slots — the warm read is on the FULL identity, not
//     the bare pair. DISCRIMINATES against the retired bare-pair memo (every
//     variant would collapse onto one slot → count 1).
// ---------------------------------------------------------------------------

#[test]
fn relate_same_nodes_different_relation_kind_policy_or_env_do_not_warm_hit() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let s = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let t = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    let publish = |key: RelateMemoKey| {
        graph.insert_relation(
            key,
            ReadSetSignature::empty(),
            Arc::from(Vec::<Arc<str>>::new()),
            RelationResult::Unknown,
            0,
        );
    };

    // Base: assignability, default policy, regular freshness, no inference
    // context, all-zero env.
    let base = RelateMemoKey::assignable(s, t, relation_context(0, 0, 0, 0));
    publish(base.clone());
    assert_eq!(
        graph.relation_memo_count(),
        1,
        "first publish lands one slot"
    );

    // Different RELATION KIND over the SAME nodes → distinct slot.
    publish(RelateMemoKey {
        relation: RelationKind::Identity,
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        2,
        "RE-KEY: a different relation KIND over the same nodes must NOT warm-hit \
         — distinct slot (bare-pair key would collapse to one)"
    );

    // Different OVERLOAD-SELECTION policy over the SAME nodes → distinct slot.
    publish(RelateMemoKey {
        policy: RelationPolicy {
            overload_selection: OverloadSelectionPolicy::FirstApplicable,
            ..RelationPolicy::default()
        },
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        3,
        "RE-KEY: a different overload-selection POLICY must NOT warm-hit — distinct slot"
    );

    // Different EXCESS-PROPERTY policy over the SAME nodes → distinct slot.
    publish(RelateMemoKey {
        policy: RelationPolicy {
            excess_property_check: true,
            ..RelationPolicy::default()
        },
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        4,
        "RE-KEY: a different excess-property POLICY must NOT warm-hit — distinct slot"
    );

    // Different VARIANCE policy (method-parameter bivariance vs strict
    // contravariance) over the SAME nodes → distinct slot.
    publish(RelateMemoKey {
        policy: RelationPolicy {
            variance: VariancePolicy::StrictContravariance,
            ..RelationPolicy::default()
        },
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        5,
        "RE-KEY: a different variance POLICY must NOT warm-hit — distinct slot"
    );

    // Different SOURCE FRESHNESS over the SAME nodes → distinct slot.
    publish(RelateMemoKey {
        source_freshness: FreshnessKey::Fresh,
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        6,
        "RE-KEY: a different source FRESHNESS must NOT warm-hit — distinct slot"
    );

    // Each env axis of `RelationContext` is identity — varied ONE AT A TIME so
    // dropping ANY of `resolve_env_hash` (R) / `type_env_hash` (T) /
    // `lib_env_hash` (L) / `project_identity` (J) from `RelateMemoKey`'s
    // `Hash`/`Eq` makes the corresponding slot collapse and this guard FAIL.
    // Different ENV (resolve dim R) over the SAME nodes → distinct slot.
    publish(RelateMemoKey {
        context: relation_context(9, 0, 0, 0),
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        7,
        "RE-KEY: a different resolve-env (R) must NOT warm-hit — distinct slot"
    );

    // Different ENV (type dim T) over the SAME nodes → distinct slot.
    publish(RelateMemoKey {
        context: relation_context(0, 9, 0, 0),
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        8,
        "RE-KEY: a different type-env (T) must NOT warm-hit — distinct slot \
         (dropping T from RelateMemoKey collapses this onto the base)"
    );

    // Different ENV (lib dim L) over the SAME nodes → distinct slot. The
    // relation outcome reads lib intrinsics, so `L` is in identity.
    publish(RelateMemoKey {
        context: relation_context(0, 0, 9, 0),
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        9,
        "RE-KEY: a different lib-env (L) must NOT warm-hit — distinct slot \
         (dropping L from RelateMemoKey collapses this onto the base)"
    );

    // Different ENV (project identity J) over the SAME nodes → distinct slot.
    publish(RelateMemoKey {
        context: relation_context(0, 0, 0, 9),
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        10,
        "RE-KEY: a different project-identity (J) must NOT warm-hit — distinct \
         slot (dropping J from RelateMemoKey collapses this onto the base)"
    );

    // Different PROJECTION-REDUCTION (context axis) over the SAME nodes →
    // distinct slot. The reduction regime a relation runs under is identity.
    publish(RelateMemoKey {
        context: RelationContext {
            projection_reduction: ProjectionReductionContext::published(ProjectionMode::Expanded),
            ..relation_context(0, 0, 0, 0)
        },
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        11,
        "RE-KEY: a different projection-reduction context must NOT warm-hit — \
         distinct slot (dropping projection_reduction from RelateMemoKey \
         collapses this onto the base)"
    );

    // Different SUBSTITUTION (context axis) over the SAME nodes → distinct slot.
    publish(RelateMemoKey {
        context: RelationContext {
            substitution: SubstitutionCanonicalHash(hash16(9)),
            ..relation_context(0, 0, 0, 0)
        },
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        12,
        "RE-KEY: a different canonical SUBSTITUTION must NOT warm-hit — distinct slot"
    );

    // Re-publishing the FIRST key replaces in place — NO new slot.
    publish(base);
    assert_eq!(
        graph.relation_memo_count(),
        12,
        "re-publishing an identical key replaces in place — no new slot"
    );
}

// ---------------------------------------------------------------------------
// (3) RELATION MEMO RE-KEY: same nodes, different inference context occupy
//     DISTINCT memo slots — the inference session a relation runs within is
//     part of identity. Each of the six content-free session axes
//     (inferable_params / variance_phase / candidate_priority / no_infer_mask /
//     const_param_policy / contextual_inference_mode) is a distinct
//     discriminator.
// ---------------------------------------------------------------------------

#[test]
fn relate_same_nodes_different_inference_context_do_not_warm_hit() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let s = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let t = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    let publish = |key: RelateMemoKey| {
        graph.insert_relation(
            key,
            ReadSetSignature::empty(),
            Arc::from(Vec::<Arc<str>>::new()),
            RelationResult::Unknown,
            0,
        );
    };

    let base = RelateMemoKey::assignable(s, t, relation_context(0, 0, 0, 0));
    publish(base.clone());
    assert_eq!(graph.relation_memo_count(), 1);

    // A baseline session over one inferable param.
    let session = InferenceContextKey {
        inferable_params: InferableParamSetId::new(Arc::from(
            vec![SemanticNodeId(100)].into_boxed_slice(),
        )),
        ..InferenceContextKey::default()
    };

    // Same nodes, but inside an inference session → distinct slot.
    publish(RelateMemoKey {
        inference_context: Some(session.clone()),
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        2,
        "RE-KEY: a relation inside an inference context must NOT warm-hit the \
         context-free judgement — distinct slot"
    );

    // A DIFFERENT inferable-param set → another distinct slot.
    publish(RelateMemoKey {
        inference_context: Some(InferenceContextKey {
            inferable_params: InferableParamSetId::new(Arc::from(
                vec![SemanticNodeId(200)].into_boxed_slice(),
            )),
            ..session.clone()
        }),
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        3,
        "RE-KEY: a different inferable-param set must NOT warm-hit — distinct slot"
    );

    // Each remaining session axis, mutated one at a time off `session`, is a
    // distinct discriminator — same nodes, distinct memo slot.
    let axis_mutations: [(InferenceContextKey, &str); 5] = [
        (
            InferenceContextKey {
                variance_phase: VariancePhase::Contravariant,
                ..session.clone()
            },
            "variance_phase",
        ),
        (
            InferenceContextKey {
                candidate_priority: InferenceCandidatePriority::ReturnType,
                ..session.clone()
            },
            "candidate_priority",
        ),
        (
            InferenceContextKey {
                no_infer_mask: NoInferMask(1),
                ..session.clone()
            },
            "no_infer_mask",
        ),
        (
            InferenceContextKey {
                const_param_policy: ConstParamPolicy::Const,
                ..session.clone()
            },
            "const_param_policy",
        ),
        (
            InferenceContextKey {
                contextual_inference_mode: ContextualInferenceMode::Contextual,
                ..session.clone()
            },
            "contextual_inference_mode",
        ),
    ];

    let mut expected = graph.relation_memo_count();
    for (mutated, axis) in axis_mutations {
        publish(RelateMemoKey {
            inference_context: Some(mutated),
            ..base.clone()
        });
        expected += 1;
        assert_eq!(
            graph.relation_memo_count(),
            expected,
            "RE-KEY: a different inference-session `{axis}` must NOT warm-hit — distinct slot"
        );
    }
}

// ---------------------------------------------------------------------------
// (4) The `Relation` value-domain payload carries the public relation outcome,
//     the inference bindings, and an opaque `RelationProofId` into the
//     payload-side proof table. The budget state is FOLDED into the outcome
//     (`BudgetExceeded`), NOT a separate field. The public outcome is EXACTLY
//     three-valued — no `Unknown` / `Holds` / `DoesNotHold`. DISCRIMINATES
//     against the retired tri-state enum + the retired embedded `proof:
//     CoinductiveProof` / `budget_state: RelationBudgetState` fields (this test
//     would not compile against the old shape).
// ---------------------------------------------------------------------------

#[test]
fn relate_query_value_carries_relation_proof_and_budget_state() {
    let binding = verter_session::semantic_query::InferBinding {
        name: Arc::from("T"),
        bound: SemanticNodeId(3),
    };
    let payload = RelationPayload {
        // Budget state is FOLDED into the outcome — there is no separate
        // `budget_state` field (the old field would fail to compile here).
        outcome: RelationOutcome::BudgetExceeded(BudgetExceededKind::RelationBudget),
        bindings: Arc::from(vec![binding].into_boxed_slice()),
        // Proof rides the payload-side table BY OPAQUE ID — there is no embedded
        // `proof: CoinductiveProof` field (the old field would fail to compile).
        relation_proof: RelationProofId(7),
    };

    assert!(
        matches!(
            payload.outcome,
            RelationOutcome::BudgetExceeded(BudgetExceededKind::RelationBudget)
        ),
        "the payload must carry the budget state FOLDED into the outcome"
    );
    assert_eq!(
        payload.relation_proof,
        RelationProofId(7),
        "the payload must carry the opaque payload-side proof id"
    );
    assert_eq!(
        payload.bindings.len(),
        1,
        "the payload carries the bindings"
    );
    assert_eq!(payload.bindings[0].name.as_ref(), "T");

    // Compile-time proof: the public outcome is EXACTLY `Assignable |
    // NotAssignable { .. } | BudgetExceeded(_)`. This exhaustive match (no
    // wildcard arm) is impossible to write if `Unknown` / `Holds` /
    // `DoesNotHold` still exist, or if `NotAssignable` is fieldless.
    let token = match &payload.outcome {
        RelationOutcome::Assignable => "assignable",
        RelationOutcome::NotAssignable { .. } => "not-assignable",
        RelationOutcome::BudgetExceeded(_) => "budget-exceeded",
    };
    assert_eq!(token, "budget-exceeded");

    // Wrapped in the value domain, it tags as `Relation`, never `TypeNode`.
    let value = SemanticQueryValue::Relation(payload);
    assert_eq!(
        value.tag(),
        SemanticQueryValueTag::Relation,
        "the Relation payload must tag as the Relation value domain"
    );
    assert_ne!(
        value.tag(),
        SemanticQueryValueTag::TypeNode,
        "the Relation payload must NOT tag as TypeNode"
    );
}

// ---------------------------------------------------------------------------
// (4b) The proof witness lives in a payload-side `RelationProofTable` holding
//      the four `RelationProof` shapes; the payload references one entry by
//      opaque `RelationProofId`. The coinductive cycle stores opaque
//      `RelateKeyId`s, NOT `RelateMemoKey` directly. DISCRIMINATES against an
//      embedded proof enum on the payload and against a `RelateMemoKey`-typed
//      cycle witness.
// ---------------------------------------------------------------------------

#[test]
fn relation_payload_uses_payload_side_relation_proofs_table() {
    let sub = SubRelationRef {
        source: SemanticNodeId(1),
        target: SemanticNodeId(2),
        position: SubRelationPosition::Member(Arc::from("next")),
    };
    let table = RelationProofTable {
        proofs: Arc::from(
            vec![
                RelationProof::Assignable {
                    witness: DerivationTree {
                        sub_derivations: Arc::from(vec![sub.clone()].into_boxed_slice()),
                    },
                },
                RelationProof::NotAssignable {
                    reason: RelationFailureReason::PrimitiveKindMismatch,
                    failing_sub: sub.clone(),
                },
                RelationProof::BudgetExceeded {
                    cap: RecursionOrBudgetCap {
                        kind: BudgetExceededKind::RelationBudget,
                        limit: 64,
                    },
                },
                // The cycle stores OPAQUE `RelateKeyId`s — a `RelateMemoKey`
                // here would fail to compile, pinning the opaque-id contract.
                RelationProof::CoinductiveCycle {
                    keys: Arc::from(vec![RelateKeyId(0), RelateKeyId(1)].into_boxed_slice()),
                },
            ]
            .into_boxed_slice(),
        ),
    };
    assert_eq!(
        table.proofs.len(),
        4,
        "all four RelationProof shapes coexist in the payload-side table"
    );

    // The payload references a proof BY OPAQUE ID into the table — the proof
    // enum is NOT embedded on the payload.
    let payload = RelationPayload {
        outcome: RelationOutcome::NotAssignable {
            primary_reason: RelationFailureReason::PrimitiveKindMismatch,
            secondary_reasons: Arc::from(Vec::<RelationFailureReason>::new().into_boxed_slice()),
        },
        bindings: Arc::from(
            Vec::<verter_session::semantic_query::InferBinding>::new().into_boxed_slice(),
        ),
        relation_proof: RelationProofId(1),
    };
    let RelationProofId(idx) = payload.relation_proof;
    assert!(
        matches!(
            &table.proofs[idx as usize],
            RelationProof::NotAssignable {
                reason: RelationFailureReason::PrimitiveKindMismatch,
                ..
            }
        ),
        "the payload's RelationProofId must index the payload-side proof table"
    );

    // The coinductive cycle entry stores opaque `RelateKeyId`s.
    match &table.proofs[3] {
        RelationProof::CoinductiveCycle { keys } => {
            assert_eq!(keys.len(), 2);
            assert_eq!(keys[0], RelateKeyId(0));
            assert_eq!(keys[1], RelateKeyId(1));
        }
        other => panic!("expected CoinductiveCycle at index 3, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// (4c) The public relation outcome has NO `Unknown` form on the display
//      surface. `display_relation` covers exactly Assignable / NotAssignable /
//      BudgetExceeded. DISCRIMINATES against a `RelationOutcome::Unknown` arm.
// ---------------------------------------------------------------------------

#[test]
fn relation_public_outcome_has_no_unknown_display() {
    // Compile-time proof: an exhaustive match over the public outcome covers
    // exactly the three public arms (no `Unknown` arm exists to match).
    fn token(outcome: &RelationOutcome) -> &'static str {
        match outcome {
            RelationOutcome::Assignable => "true",
            RelationOutcome::NotAssignable { .. } => "false",
            RelationOutcome::BudgetExceeded(_) => "budget-exceeded",
        }
    }
    assert_eq!(token(&RelationOutcome::Assignable), "true");
    assert_eq!(
        token(&RelationOutcome::BudgetExceeded(
            BudgetExceededKind::FlowSliceBudget
        )),
        "budget-exceeded"
    );

    // Static scan of the display renderer: it must NOT reference a public
    // `RelationOutcome::Unknown` arm, and MUST cover the three public arms.
    let display_src = include_str!("../../src/semantic_query/display.rs");
    assert!(
        !display_src.contains("RelationOutcome::Unknown"),
        "display_relation must not match a public `RelationOutcome::Unknown` arm"
    );
    assert!(
        display_src.contains("RelationOutcome::Assignable"),
        "display_relation must cover the Assignable arm"
    );
    assert!(
        display_src.contains("RelationOutcome::NotAssignable"),
        "display_relation must cover the NotAssignable arm"
    );
    assert!(
        display_src.contains("RelationOutcome::BudgetExceeded"),
        "display_relation must cover the BudgetExceeded arm"
    );
}

// ---------------------------------------------------------------------------
// (5) VALUE-DOMAIN MAP: `Relate` maps to the `Relation` value domain, NOT
//     `TypeNode`; and the spec table remains a total function (exactly one row
//     per variant). Asserts the U2B.8 DELTA on the spec row: env == `R T L J`
//     and the key-fields string carries the full relation identity (pre-change
//     the row was `(source,target)` only).
// ---------------------------------------------------------------------------

#[test]
fn relate_key_returns_relation_value() {
    let specs = semantic_query_key_specs();
    let row = specs
        .iter()
        .find(|s| s.variant == SemanticQueryKeyTag::Relate)
        .expect("missing spec row for Relate");
    assert_eq!(
        row.value_domain,
        SemanticQueryValueTag::Relation,
        "Relate must map to the Relation value domain, not {:?}",
        row.value_domain
    );
    assert_ne!(
        row.value_domain,
        SemanticQueryValueTag::TypeNode,
        "Relate must NOT carry the TypeNode value domain"
    );

    // U2B.8 DELTA: the upgraded `RelationContext` carries the `R` the bare
    // `{source,target}` key lacked, so the env is `R T L J`. Pre-change the
    // row's key-fields string was `(source,target)` only — these assertions
    // FAIL against the bare-pair tree.
    assert_eq!(
        row.env_dims.render(),
        "R T L J",
        "Relate spec row env must be `R T L J` (the upgraded RelationContext \
         carries the R the bare pair lacked)"
    );
    for field in [
        "relation",
        "policy",
        "source_freshness",
        "inference_context",
        "context",
    ] {
        assert!(
            row.context_shape.contains(field),
            "Relate spec-row key-fields must carry the full relation identity — \
             missing `{field}` in `{}`",
            row.context_shape
        );
    }
}

#[test]
fn every_semantic_query_key_maps_to_exactly_one_value_domain_with_relate_relation() {
    let specs = semantic_query_key_specs();
    for tag in SemanticQueryKeyTag::ALL {
        let rows: Vec<_> = specs.iter().filter(|s| s.variant == *tag).collect();
        assert_eq!(
            rows.len(),
            1,
            "variant {tag:?} must have EXACTLY ONE spec row (→ exactly one value domain), found {}",
            rows.len()
        );
    }
    let relate_row = specs
        .iter()
        .find(|s| s.variant == SemanticQueryKeyTag::Relate)
        .unwrap();
    assert_eq!(relate_row.value_domain, SemanticQueryValueTag::Relation);
    // U2B.8 DELTA — pin the upgraded env on the total-function guard too.
    assert_eq!(relate_row.env_dims.render(), "R T L J");
}

// ---------------------------------------------------------------------------
// (6) FAMILY-MEMO MAP: `Relate` maps to the DEDICATED non-aliasing
//     `FamilyKey::Relate`, NEVER `FamilyKey::IndexedAccess`. A `Relate` key and
//     an `IndexedAccess` key over the SAME `(source, target)` nodes get DISTINCT
//     family identities — no wrong-domain warm-hit collision. DISCRIMINATES
//     against the retired arm that aliased `IndexedAccess` (where the two would
//     share one family identity).
// ---------------------------------------------------------------------------

#[test]
fn relate_maps_to_dedicated_relate_family_not_indexed_access() {
    let s = SemanticNodeId(1);
    let t = SemanticNodeId(2);

    let relate = relate_key(
        s,
        t,
        RelationKind::Assignable,
        RelationPolicy::default(),
        FreshnessKey::Regular,
        None,
        relation_context(0, 0, 0, 0),
    );
    let indexed = SemanticQueryKey::IndexedAccess {
        base: s,
        index: IndexKey::TypeNode(t),
        mode: ProjectionMode::Shallow,
    };

    assert_eq!(
        family_variant_label_for_tests(&relate),
        "Relate",
        "Relate must map to the DEDICATED FamilyKey::Relate"
    );
    assert_ne!(
        family_variant_label_for_tests(&relate),
        "IndexedAccess",
        "Relate must NOT alias FamilyKey::IndexedAccess (the retired wrong-domain hazard)"
    );
    assert_ne!(
        family_variant_label_for_tests(&relate),
        family_variant_label_for_tests(&indexed),
        "a Relate key and an IndexedAccess key over the same (source,target) must \
         occupy DISTINCT family identities"
    );
    // Sanity: the IndexedAccess key still maps to its own family (so the
    // assert_ne above is not vacuous).
    assert_eq!(family_variant_label_for_tests(&indexed), "IndexedAccess");
}

// ---------------------------------------------------------------------------
// (7) KEYSPACE SIZE DISCIPLINE: the `Relate` payload is BOXED, never embedded
//     by value. A Rust enum is sized to its largest variant, so embedding the
//     ~144B `RelateMemoKey` BY VALUE would inflate EVERY entry key of the hot
//     single-node `FamilyKey → FamilySlots` keyspace — for a variant that is
//     NEVER admitted in production. `Box<RelateMemoKey>` is 8 bytes, keeping the
//     enum driven by the legitimately-present env-heavy variants (`FamilyKey` is
//     112B with the boxed payload). DISCRIMINATES against the inline shape: with
//     `RelateMemoKey` embedded by value the enum is >= 144B + discriminant
//     (>= 152B) and this bound FAILS; boxed it is 112B and the bound PASSES.
// ---------------------------------------------------------------------------

#[test]
fn family_key_does_not_embed_relate_memo_key_by_value() {
    let family_key = family_key_size_for_tests();
    let relate_memo = std::mem::size_of::<RelateMemoKey>();

    // The bound (128B) is derived from the env-heavy variants that legitimately
    // drive the enum size (boxed `FamilyKey` measures 112B), with a small margin
    // for incidental growth — but well BELOW the inline `RelateMemoKey` size
    // (144B) + discriminant. An inline `Relate` payload pushes the enum past this
    // bound; the boxed payload stays under it.
    const FAMILY_KEY_KEYSPACE_BOUND: usize = 128;

    assert!(
        family_key <= FAMILY_KEY_KEYSPACE_BOUND,
        "FamilyKey is {family_key}B (> {FAMILY_KEY_KEYSPACE_BOUND}B bound): the \
         Relate payload (RelateMemoKey is {relate_memo}B) must be BOXED, not \
         embedded by value — an inline payload re-imports the node-memo keyspace \
         inflation the relation memo was separated out to AVOID"
    );

    // Cross-check the discipline is real (not vacuous): the inline shape really
    // is larger than the bound, so this guard genuinely discriminates.
    assert!(
        relate_memo > FAMILY_KEY_KEYSPACE_BOUND,
        "RelateMemoKey ({relate_memo}B) must exceed the keyspace bound \
         ({FAMILY_KEY_KEYSPACE_BOUND}B) — otherwise embedding it by value would \
         not inflate the enum and this guard would not discriminate"
    );
}

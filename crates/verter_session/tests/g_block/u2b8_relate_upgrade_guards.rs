//! U2B.8 guards — the upgraded full-identity `Relate` key surface.
//!
//! These tests pin the IDENTITY contract of the upgraded
//! [`SemanticQueryKey::Relate`] variant — no longer the bare
//! `(source, target)` pair but the full relation identity (relation kind,
//! comparison policy, source freshness, optional inference context, and the
//! `R T L J` env [`RelationContext`]) — the RE-KEYING of the relation memo onto
//! the full [`RelateMemoKey`] (so two judgements over the same nodes that differ
//! in any identity axis occupy DISTINCT memo slots), the value-domain mapping
//! (`Relate` → `Relation`, never `TypeNode`), and the shape of the forward-
//! declared [`RelationPayload`] carrier (outcome + off-surface coinductive proof
//! + budget state).
//!
//! Each warm-hit guard is DISCRIMINATING against the retired bare-pair key: a
//! relation memo keyed on `(source, target)` would collapse every variant in a
//! group onto ONE slot, so the slot-count assertions would read `1` and FAIL;
//! the full-identity key splits them and the assertions PASS.

use std::sync::Arc;

use verter_session::for_tests::ReadSetSignature;
use verter_session::semantic_query::query_key_spec::semantic_query_key_specs;
use verter_session::semantic_query::{
    CoinductiveProof, FreshnessKey, InferenceContextKey, PrimitiveKind, RelateMemoKey,
    RelationBudgetState, RelationContext, RelationKind, RelationOutcome, RelationPayload,
    RelationPolicy, RelationResult, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    SemanticQueryKeyTag, SemanticQueryValue, SemanticQueryValueTag,
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
    // Policy is identity.
    assert_ne!(
        base,
        relate_key(
            s,
            t,
            RelationKind::Assignable,
            RelationPolicy {
                excess_property_check: true,
                report_errors: false,
            },
            FreshnessKey::Regular,
            None,
            relation_context(0, 0, 0, 0),
        ),
        "a different relation policy must be a DISTINCT key"
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

    // Different POLICY over the SAME nodes → distinct slot.
    publish(RelateMemoKey {
        policy: RelationPolicy {
            excess_property_check: true,
            report_errors: false,
        },
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        3,
        "RE-KEY: a different relation POLICY must NOT warm-hit — distinct slot"
    );

    // Different SOURCE FRESHNESS over the SAME nodes → distinct slot.
    publish(RelateMemoKey {
        source_freshness: FreshnessKey::Fresh,
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        4,
        "RE-KEY: a different source FRESHNESS must NOT warm-hit — distinct slot"
    );

    // Different ENV (resolve dim) over the SAME nodes → distinct slot.
    publish(RelateMemoKey {
        context: relation_context(9, 0, 0, 0),
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        5,
        "RE-KEY: a different ENV must NOT warm-hit — distinct slot"
    );

    // Re-publishing the FIRST key replaces in place — NO new slot.
    publish(base);
    assert_eq!(
        graph.relation_memo_count(),
        5,
        "re-publishing an identical key replaces in place — no new slot"
    );
}

// ---------------------------------------------------------------------------
// (3) RELATION MEMO RE-KEY: same nodes, different inference context occupy
//     DISTINCT memo slots — the inference session a relation runs within is
//     part of identity.
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

    // Same nodes, but inside an inference session → distinct slot.
    let session_a = InferenceContextKey {
        inference_targets: Arc::from(vec![SemanticNodeId(100)].into_boxed_slice()),
        pass: 0,
    };
    publish(RelateMemoKey {
        inference_context: Some(session_a.clone()),
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        2,
        "RE-KEY: a relation inside an inference context must NOT warm-hit the \
         context-free judgement — distinct slot"
    );

    // A DIFFERENT inference session (different targets) → another distinct slot.
    let session_b = InferenceContextKey {
        inference_targets: Arc::from(vec![SemanticNodeId(200)].into_boxed_slice()),
        pass: 0,
    };
    publish(RelateMemoKey {
        inference_context: Some(session_b),
        ..base.clone()
    });
    assert_eq!(
        graph.relation_memo_count(),
        3,
        "RE-KEY: a different inference session must NOT warm-hit — distinct slot"
    );

    // A different fixing PASS within the same target set → another distinct slot.
    publish(RelateMemoKey {
        inference_context: Some(InferenceContextKey {
            inference_targets: session_a.inference_targets.clone(),
            pass: 1,
        }),
        ..base
    });
    assert_eq!(
        graph.relation_memo_count(),
        4,
        "RE-KEY: a different inference fixing pass must NOT warm-hit — distinct slot"
    );
}

// ---------------------------------------------------------------------------
// (4) The `Relation` value-domain payload carries the relation outcome, the
//     off-surface coinductive proof, and the budget state. DISCRIMINATES
//     against the retired fieldless tri-state enum (which had no proof / budget
//     fields and would not compile against this test).
// ---------------------------------------------------------------------------

#[test]
fn relate_query_value_carries_relation_proof_and_budget_state() {
    let payload = RelationPayload {
        outcome: RelationOutcome::Holds,
        proof: CoinductiveProof::Coinductive {
            assumptions: Arc::from(vec![(SemanticNodeId(1), SemanticNodeId(2))].into_boxed_slice()),
        },
        budget_state: RelationBudgetState::Exhausted,
    };

    // The outcome, proof, and budget state are all reachable on the payload.
    assert_eq!(payload.outcome, RelationOutcome::Holds);
    assert!(
        matches!(
            &payload.proof,
            CoinductiveProof::Coinductive { assumptions } if assumptions.len() == 1
        ),
        "the payload must carry the off-surface coinductive proof witness"
    );
    assert_eq!(
        payload.budget_state,
        RelationBudgetState::Exhausted,
        "the payload must carry the budget state"
    );

    // Wrapped in the value domain, it tags as `Relation`.
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

    // The honest forward-declared default is Unknown / no proof / within budget.
    let unknown = RelationPayload::unknown();
    assert_eq!(unknown.outcome, RelationOutcome::Unknown);
    assert_eq!(unknown.proof, CoinductiveProof::None);
    assert_eq!(unknown.budget_state, RelationBudgetState::WithinBudget);
}

// ---------------------------------------------------------------------------
// (5) VALUE-DOMAIN MAP: `Relate` maps to the `Relation` value domain, NOT
//     `TypeNode`; and the spec table remains a total function (exactly one row
//     per variant).
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
    let relate_domain = specs
        .iter()
        .find(|s| s.variant == SemanticQueryKeyTag::Relate)
        .map(|s| s.value_domain)
        .unwrap();
    assert_eq!(relate_domain, SemanticQueryValueTag::Relation);
}

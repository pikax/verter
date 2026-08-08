//! Guards for the `ResolveCall` key surface and the U6.CALL_RESOLVE contract
//! surfaces.
//!
//! Two families:
//!
//! - **Behavioral key/value/spec guards** — pin the IDENTITY contract of
//!   [`SemanticQueryKey::ResolveCall`]: the full `P R T L J` env + the
//!   `CanonicalTypeSubstitution` axis ride INSIDE the [`ResolveCallContext`]
//!   (no slot), the call-site point / callee / kind / receiver / args /
//!   explicit type args / sealed flow axis are all identity, and the
//!   non-producing execute arm is an HONEST `Miss` that admits NOTHING.
//!   Identity is probed behaviorally through the family memo exactly as the
//!   sibling key-surface guards (`u2b7_flow_contextual_guards`) do.
//! - **Structural contract guards** — pin the public contract surfaces by
//!   EXHAUSTIVE destructuring: per-parameter `const` metadata on
//!   `TypeParamDecl` and across the `TypeParam` / `NarrowTypeParam` IR
//!   chain, and the occurrence-aware `SignatureRef`. A renamed, dropped, or
//!   added field fails to COMPILE. The crate-private surfaces (the
//!   candidate-session lifecycle, the mixed return equation), the Call-IR
//!   convergence, and the typed `FlowReturnFailure::CallResolution` arm are
//!   pinned the same way beside their own types (see the note at the end of
//!   this file).

use std::sync::Arc;

use verter_session::for_tests::ReadSetSignature;
use verter_session::semantic_query::query_key_spec::semantic_query_key_specs;
use verter_session::semantic_query::{
    ArgumentLiteralMode, CallArgKey, CallKind, CanonicalTypeSubstitution, FlowNarrowingKey,
    PrimitiveKind, ProgramPointId, QueryError, QueryResult, ResolveCallContext, ResolveCallKey,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey, SemanticQueryKeyTag, SemanticQueryValueTag,
    SignatureCandidateOrigin, SignatureRef, SignatureReturnCarrier, TypeParamDecl,
};
use verter_session::{HostConfig, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn hash16(byte: u8) -> [u8; 16] {
    [byte; 16]
}

/// Publish a synthetic candidate under `a`, then return the candidate
/// count `b` projects to. `> 0` ⟺ `a` and `b` share a `(FamilyKey, slot)`.
/// A FRESH host per call keeps every pair independent. `ResolveCall` carries
/// no projection mode (`ModeSlot::Single`), so backfill never muddies the
/// probe.
fn count_for_b_after_publishing_a(a: &SemanticQueryKey, b: &SemanticQueryKey) -> usize {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    graph.publish_with_carrier_dispatch_and_generation_for_tests(
        a.clone(),
        QueryResult::Value(node),
        ReadSetSignature::empty(),
        Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
        Arc::from(Vec::new().into_boxed_slice()),
        100,
    );
    graph.slot_candidate_count_for_tests(b)
}

/// `a` and `b` are NON-equal keys AND project to DISTINCT `(FamilyKey,
/// slot)`. Also asserts the positive sanity (`a` reaches its own slot) so
/// the probe is not vacuously passing on a broken publish path.
fn assert_distinct_identity(a: &SemanticQueryKey, b: &SemanticQueryKey) {
    assert_ne!(a, b, "keys must be non-equal");
    assert_eq!(
        count_for_b_after_publishing_a(a, a),
        1,
        "sanity: publishing `a` must reach `a`'s own slot (count 1)"
    );
    assert_eq!(
        count_for_b_after_publishing_a(a, b),
        0,
        "a warm candidate published under `a` must NOT be reachable from \
         `b` — they must project to DISTINCT (FamilyKey, slot)"
    );
}

// ---------------------------------------------------------------------------
// Key constructors.
// ---------------------------------------------------------------------------

fn point(canonical: &str, offset: u32) -> ProgramPointId {
    ProgramPointId {
        canonical_id: Arc::from(canonical),
        offset,
    }
}

fn call_context(p: u8, r: u8, t: u8, l: u8, j: u8) -> ResolveCallContext {
    ResolveCallContext {
        parse_env_hash: hash16(p),
        resolve_env_hash: hash16(r),
        type_env_hash: hash16(t),
        lib_env_hash: hash16(l),
        project_identity: hash16(j),
        substitution: CanonicalTypeSubstitution::empty(),
    }
}

/// The baseline key: one eager argument, no receiver, no explicit type
/// args, sealed-empty flow, zeroed env. Tests mutate individual axes of the
/// payload and re-wrap.
fn base_key() -> ResolveCallKey {
    ResolveCallKey {
        point: point("a.ts", 0),
        callee: SemanticNodeId(1),
        kind: CallKind::Call,
        receiver: None,
        args: Arc::from(
            vec![CallArgKey::Eager {
                ty: SemanticNodeId(2),
                spread: false,
                literal_mode: ArgumentLiteralMode::Widened,
                context_sensitive: false,
            }]
            .into_boxed_slice(),
        ),
        explicit_type_args: Arc::from(Vec::new().into_boxed_slice()),
        flow: FlowNarrowingKey::empty(),
        context: call_context(0, 0, 0, 0, 0),
    }
}

fn wrap(key: ResolveCallKey) -> SemanticQueryKey {
    SemanticQueryKey::ResolveCall(Box::new(key))
}

fn node_set(id: u64) -> Arc<[SemanticNodeId]> {
    Arc::from(vec![SemanticNodeId(id)].into_boxed_slice())
}

// ---------------------------------------------------------------------------
// (1) ResolveCall identity covers the FULL P/R/T/L/J env (carried IN the
//     context, NOT a slot) plus the ProgramPointId (canonical_id + offset).
// ---------------------------------------------------------------------------

#[test]
fn resolve_call_key_covers_full_env_and_point() {
    let base = base_key();

    // Every one of the FULL P R T L J env dims is part of identity — call
    // resolution is the widest-env operation in the key surface.
    for mutated in [
        {
            let mut k = base_key();
            k.context.parse_env_hash = hash16(9);
            k
        },
        {
            let mut k = base_key();
            k.context.resolve_env_hash = hash16(9);
            k
        },
        {
            let mut k = base_key();
            k.context.type_env_hash = hash16(9);
            k
        },
        {
            let mut k = base_key();
            k.context.lib_env_hash = hash16(9);
            k
        },
        {
            let mut k = base_key();
            k.context.project_identity = hash16(9);
            k
        },
    ] {
        assert_distinct_identity(&wrap(base.clone()), &wrap(mutated));
    }

    // The ProgramPointId is part of identity — both the canonical file and
    // the offset within it.
    assert_distinct_identity(
        &wrap(base.clone()),
        &wrap({
            let mut k = base_key();
            k.point = point("b.ts", 0);
            k
        }),
    );
    assert_distinct_identity(
        &wrap(base.clone()),
        &wrap({
            let mut k = base_key();
            k.point = point("a.ts", 7);
            k
        }),
    );
}

// ---------------------------------------------------------------------------
// (2) ResolveCall identity covers every call axis: callee, kind, receiver,
//     args (ty / spread / literal mode / carrier form / arg point), and
//     explicit type args.
// ---------------------------------------------------------------------------

#[test]
fn resolve_call_key_covers_callee_kind_receiver_args_and_type_args() {
    let base = base_key();

    // Callee node.
    assert_distinct_identity(
        &wrap(base.clone()),
        &wrap({
            let mut k = base_key();
            k.callee = SemanticNodeId(9);
            k
        }),
    );
    // Call vs construct.
    assert_distinct_identity(
        &wrap(base.clone()),
        &wrap({
            let mut k = base_key();
            k.kind = CallKind::Construct;
            k
        }),
    );
    // Receiver presence and receiver node.
    assert_distinct_identity(
        &wrap(base.clone()),
        &wrap({
            let mut k = base_key();
            k.receiver = Some(SemanticNodeId(3));
            k
        }),
    );
    assert_distinct_identity(
        &wrap({
            let mut k = base_key();
            k.receiver = Some(SemanticNodeId(3));
            k
        }),
        &wrap({
            let mut k = base_key();
            k.receiver = Some(SemanticNodeId(4));
            k
        }),
    );
    // Argument type node.
    assert_distinct_identity(
        &wrap(base.clone()),
        &wrap({
            let mut k = base_key();
            k.args = Arc::from(
                vec![CallArgKey::Eager {
                    ty: SemanticNodeId(8),
                    spread: false,
                    literal_mode: ArgumentLiteralMode::Widened,
                    context_sensitive: false,
                }]
                .into_boxed_slice(),
            );
            k
        }),
    );
    // Argument spread bit.
    assert_distinct_identity(
        &wrap(base.clone()),
        &wrap({
            let mut k = base_key();
            k.args = Arc::from(
                vec![CallArgKey::Eager {
                    ty: SemanticNodeId(2),
                    spread: true,
                    literal_mode: ArgumentLiteralMode::Widened,
                    context_sensitive: false,
                }]
                .into_boxed_slice(),
            );
            k
        }),
    );
    // Argument literal mode.
    assert_distinct_identity(
        &wrap(base.clone()),
        &wrap({
            let mut k = base_key();
            k.args = Arc::from(
                vec![CallArgKey::Eager {
                    ty: SemanticNodeId(2),
                    spread: false,
                    literal_mode: ArgumentLiteralMode::Literal,
                    context_sensitive: false,
                }]
                .into_boxed_slice(),
            );
            k
        }),
    );
    // Argument carrier form (Eager vs ProgramExpression).
    assert_distinct_identity(
        &wrap(base.clone()),
        &wrap({
            let mut k = base_key();
            k.args = Arc::from(
                vec![CallArgKey::ProgramExpression {
                    point: point("a.ts", 4),
                    spread: false,
                    literal_mode: ArgumentLiteralMode::Widened,
                    context_sensitive: false,
                }]
                .into_boxed_slice(),
            );
            k
        }),
    );
    // Explicit type args (presence and node).
    assert_distinct_identity(
        &wrap(base.clone()),
        &wrap({
            let mut k = base_key();
            k.explicit_type_args = node_set(5);
            k
        }),
    );
    assert_distinct_identity(
        &wrap({
            let mut k = base_key();
            k.explicit_type_args = node_set(5);
            k
        }),
        &wrap({
            let mut k = base_key();
            k.explicit_type_args = node_set(6);
            k
        }),
    );
}

// ---------------------------------------------------------------------------
// (3) same-expr-different-flow-or-substitution — the manifest-named guard.
//     Two keys at the SAME program point differing ONLY in the `flow` axis
//     (or ONLY in the substitution axis) occupy DISTINCT (FamilyKey, slot):
//     no warm hit across either boundary. FAILS if the flow or
//     substitution axis is dropped from the family identity.
// ---------------------------------------------------------------------------

#[test]
fn resolve_call_same_expr_different_flow_or_substitution_does_not_warm_hit() {
    // Differ ONLY in the sealed flow axis.
    let flow_empty = wrap(base_key());
    let flow_non_empty = wrap({
        let mut k = base_key();
        k.flow = FlowNarrowingKey::new(node_set(7));
        k
    });
    assert_distinct_identity(&flow_empty, &flow_non_empty);
    assert_eq!(
        count_for_b_after_publishing_a(&flow_empty, &flow_non_empty),
        0,
        "ResolveCall must not warm-hit across the flow axis"
    );

    // Differ ONLY in the substitution axis.
    let subst_empty = wrap(base_key());
    let subst_non_empty = wrap({
        let mut k = base_key();
        k.context.substitution =
            CanonicalTypeSubstitution::new(vec![(SemanticNodeId(1), SemanticNodeId(2))]);
        k
    });
    assert_distinct_identity(&subst_empty, &subst_non_empty);
    assert_eq!(
        count_for_b_after_publishing_a(&subst_empty, &subst_non_empty),
        0,
        "ResolveCall must not warm-hit across the substitution axis"
    );
}

// ---------------------------------------------------------------------------
// (4) CROSS-KEY NEGATIVE: a ResolveCall and a FlowNarrowingAt at the SAME
//     program point + SAME env are DISTINCT queries and MUST NOT collide.
// ---------------------------------------------------------------------------

#[test]
fn resolve_call_key_distinct_from_flow_narrowing_at_same_point() {
    let call = wrap(base_key());
    let narrowing = SemanticQueryKey::FlowNarrowingAt {
        point: point("a.ts", 0),
        flow: FlowNarrowingKey::empty(),
        context: verter_session::semantic_query::ProgramAnalysisContext {
            parse_env_hash: hash16(0),
            resolve_env_hash: hash16(0),
            type_env_hash: hash16(0),
            lib_env_hash: hash16(0),
            project_identity: 0,
            substitution: verter_session::semantic_query::SubstitutionCanonicalHash::empty(),
        },
    };
    assert_distinct_identity(&call, &narrowing);
}

// ---------------------------------------------------------------------------
// (5) NON-CALLABLE discriminator — the live executor returns Error(Miss) for
//     a callee with no requested signature and admits NOTHING.
// ---------------------------------------------------------------------------

#[test]
fn resolve_call_non_callable_is_miss_and_never_admits() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let key = wrap(base_key());

    let result =
        verter_session::for_tests::dispatch_execute_type_node_for_tests(&host, key.clone());
    assert!(
        matches!(result, QueryResult::Error(QueryError::Miss)),
        "a non-callable ResolveCall must return Error(Miss), got {result:?}"
    );
    assert_eq!(
        graph.slot_candidate_count_for_tests(&key),
        0,
        "a non-callable ResolveCall must admit NOTHING into the shared memo"
    );
}

// ---------------------------------------------------------------------------
// (6) VALUE-DOMAIN mapping — ResolveCall maps to the `ResolveCall` value
//     domain, NOT `TypeNode`, with EXACTLY ONE spec row.
// ---------------------------------------------------------------------------

#[test]
fn resolve_call_value_domain_is_resolve_call() {
    let specs = semantic_query_key_specs();
    let rows: Vec<_> = specs
        .iter()
        .filter(|s| s.variant == SemanticQueryKeyTag::ResolveCall)
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "ResolveCall must have EXACTLY ONE spec row, found {}",
        rows.len()
    );
    assert_eq!(
        rows[0].value_domain,
        SemanticQueryValueTag::ResolveCall,
        "ResolveCall must map to the ResolveCall value domain, not {:?}",
        rows[0].value_domain
    );
    // NEGATIVE: explicitly NOT the easy `TypeNode` default.
    assert_ne!(
        rows[0].value_domain,
        SemanticQueryValueTag::TypeNode,
        "ResolveCall must NOT carry the TypeNode value domain"
    );
}

// ---------------------------------------------------------------------------
// Structural contract guards. Each pins a contract surface by EXHAUSTIVE
// destructuring / matching: a renamed, dropped, or added field or variant
// fails to COMPILE, so the invariant is held by the type system rather than
// by reading source text.
// ---------------------------------------------------------------------------

/// CONST-METADATA guard — `TypeParamDecl` carries the EXACT interned
/// parameter node (the identity inference binds) and the PER-PARAMETER
/// `is_const` modifier (`<const T, U>` is valid — never a session-wide
/// flag). Exhaustive destructuring: dropping or renaming `param` /
/// `is_const` (const policy silently reverting to session-wide) fails to
/// compile.
#[test]
fn type_param_decl_carries_exact_param_node_and_per_parameter_const() {
    let decl = TypeParamDecl {
        name: Arc::from("T"),
        param: SemanticNodeId(41),
        constraint: Some(SemanticNodeId(42)),
        default: Some(SemanticNodeId(43)),
        is_const: true,
    };
    let TypeParamDecl {
        name,
        param,
        constraint,
        default,
        is_const,
    } = &decl;
    assert_eq!(name.as_ref(), "T");
    assert_eq!(*param, SemanticNodeId(41));
    assert_eq!(*constraint, Some(SemanticNodeId(42)));
    assert_eq!(*default, Some(SemanticNodeId(43)));
    assert!(*is_const);

    // The const modifier is PER-PARAMETER: a second declaration in the same
    // list carries its own value, and the two decls stay distinct.
    let non_const = TypeParamDecl {
        is_const: false,
        ..decl.clone()
    };
    assert!(!non_const.is_const);
    assert_ne!(decl, non_const);
}

/// CONST-CHAIN guard — the `<const T>` modifier survives the whole IR
/// chain: the parser-level `TypeParam` AND the fact-level `NarrowTypeParam`
/// both record it, so the executor's per-parameter const policy reads
/// authored metadata, never a text scan. Exhaustive destructuring at both
/// layers: dropping the field at either one fails to compile.
#[test]
fn const_modifier_survives_type_param_and_narrow_type_param() {
    let param = verter_type_expr::TypeParam {
        name: "T".to_string(),
        constraint: None,
        default: None,
        is_const: true,
    };
    let verter_type_expr::TypeParam {
        name,
        constraint,
        default,
        is_const,
    } = &param;
    assert_eq!(name, "T");
    assert!(constraint.is_none());
    assert!(default.is_none());
    assert!(*is_const, "the parser-level TypeParam records `const`");

    let narrow = verter_type_expr::facts::NarrowTypeParam {
        name: "T".to_string(),
        ordinal: 0,
        constraint: None,
        default: None,
        is_const: true,
    };
    let verter_type_expr::facts::NarrowTypeParam {
        name,
        ordinal,
        constraint,
        default,
        is_const,
    } = &narrow;
    assert_eq!(name, "T");
    assert_eq!(*ordinal, 0);
    assert!(constraint.is_none());
    assert!(default.is_none());
    assert!(*is_const, "the fact-level NarrowTypeParam records `const`");
}

/// OCCURRENCE guard — `SignatureRef` carries the occurrence identity and
/// the return carrier, so the overload set's ordered candidates stay the
/// sole candidate source and instantiation PRESERVES the occurrence.
/// Exhaustive destructuring: dropping or renaming `occurrence` /
/// `return_carrier` fails to compile.
#[test]
fn signature_ref_is_occurrence_aware() {
    let origin = SignatureCandidateOrigin::Rootless;
    let candidate = SignatureRef {
        node: SemanticNodeId(71),
        occurrence: origin.clone(),
        return_carrier: SignatureReturnCarrier::Declared(SemanticNodeId(72)),
        arm_ordinal: 0,
    };
    let SignatureRef {
        node,
        occurrence,
        return_carrier,
        arm_ordinal,
    } = &candidate;
    assert_eq!(*node, SemanticNodeId(71));
    assert_eq!(occurrence, &origin);
    assert!(matches!(
        return_carrier,
        SignatureReturnCarrier::Declared(id) if *id == SemanticNodeId(72)
    ));
    assert_eq!(
        *arm_ordinal, 0,
        "a non-union callee's candidates all carry arm ordinal 0"
    );

    // Instantiating the signature node preserves the occurrence: the same
    // origin rides a different signature node.
    let instantiated = SignatureRef {
        node: SemanticNodeId(73),
        ..candidate.clone()
    };
    assert_eq!(instantiated.occurrence, candidate.occurrence);
    assert_ne!(instantiated.node, candidate.node);
}

// VERDICT guards (failure typing + IR convergence) are STRUCTURAL, not
// source scans: they live beside the types they pin, where the compiler
// enforces them.
//
// - `FlowReturnFailure`'s typed `CallResolution` arm and the real failing
//   call that surfaces it:
//   `crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs`
//   (`flow_return_failure_taxonomy_is_exhaustive_and_carries_call_resolution`
//   plus the `verdict_*` rows).
// - The single `FlowIrExpr::Call` convergence: every call shape lowered
//   from real source, plus the exhaustive `FlowIrExpr` match that fails to
//   COMPILE if a legacy call variant is re-introduced —
//   `crates/verter_session/src/flow_ir_tests.rs`
//   (`every_call_shape_lowers_to_the_one_call_carrier`,
//   `flow_ir_expr_taxonomy_is_exhaustive`).
// - The candidate-session lifecycle and the mixed `FlowReturn | ResolveCall`
//   return equation (both crate-private):
//   `crates/verter_session/src/project_semantic_dispatch/dispatch_txn_tests.rs`
//   (`candidate_session_lifecycle_states_are_collecting_staged_committed_abandoned`,
//   `return_equation_identity_spans_flow_return_and_resolve_call`).

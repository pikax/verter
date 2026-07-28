//! Parity tests for the node-domain raised-shape classifiers + equality
//! primitive (owner-local in [`super::raise`]).
//!
//! The ORACLE is the test-only shell-raise mirror of the Kind-B bridge,
//! [`ProjectSemanticDispatch::materialize_output_type_expr_for_test`] — the
//! exact `raise_node_to_type_expr` the bridge runs. For every fixture node the
//! tests assert each node-domain classifier EQUALS the legacy `TypeExpr`
//! predicate applied to the raised oracle output:
//!
//! PARITY IS SPLIT: degradation classification lives ONLY in the
//! typed node domain (`QueryError` / the materialize sidecar); the raised
//! compat tree is INERT (no raw sentinel classification reads it). So:
//!
//! - `node_contains_semantic_miss_or_unraisable(node)` is TYPED — asserted
//!   per-fixture and cross-checked against BOTH node-domain algebras; the
//!   legacy tree oracle (`type_expr_contains_semantic_miss`) is pinned INERT
//!   (always false) on every raised tree.
//! - `node_is_expanded_surface_legacy_equivalent(node) == match raise(node)
//!   { Some(e) => type_expr_is_expanded_surface(&e), None => false }`
//!   (the expanded oracle is STRUCTURAL, not sentinel-based — still exact).
//! - `node_can_shell_raise(node) == raise(node).is_some()`
//! - `raised_shape_eq_node_type_expr(node, &e) == Some(raise(node) == Some(e))`
//!   (raw-only interner identity — a compatibility projection keys EQUAL to a
//!   genuine `UnknownValue` with the same spelling).
//! - `raised_shape_eq_nodes(a, b) == Some(raise(a) == raise(b))` (both `Some`).
//!
//! The corpus is built with DIRECT `graph.intern_node(SemanticNodeData::…)`
//! fixtures (NOT only `TypeExpr -> lower -> raise`), so it covers the
//! production-only graph shapes lowering a hand-written `TypeExpr` cannot
//! reach. The six required coverage classes — aliases, intersections, objects,
//! opaque errors, raw fallback, deferred operator shells — are each exercised
//! below (and exceeded: carriers, merged decls, equality-divergence pairs).
//!
//! Each test is DISCRIMINATING: the parity-vs-oracle assertions FAIL if a
//! classifier returns a constant or the wrong fact, and the dedicated
//! `*_discriminates` self-tests at the end prove the oracle itself separates
//! the fact values (so an always-true / always-false classifier could not pass
//! the suite).

use std::sync::Arc;

use verter_type_expr::{PrimitiveName, TypeExpr};

use super::raise::{
    node_can_shell_raise, node_contains_semantic_miss_or_unraisable,
    node_is_expanded_surface_legacy_equivalent, node_raised_shape_facts, node_raised_shape_for_eq,
    project_node_publication_score_with_dispatch, raised_shape_eq_node_type_expr,
    raised_shape_eq_nodes, type_expr_publication_score, PublicationScore,
};
use super::ProjectSemanticDispatch;
use crate::resolver_core::component_meta_query_engine::{
    type_expr_contains_semantic_miss, type_expr_is_expanded_surface,
};
use crate::resolver_core::shallow_file_state::{BudgetDomain, BudgetExceededFailure};
use crate::semantic_query::{
    DeclIdentity, FunctionParam, IndexKey, IndexSignature, MapperKey, MapperKind,
    MemberSurfaceCompleteness, NodeScopeId, OpenSpreadOperands, OptionalityMod, PrimitiveKind,
    QueryError, ReadonlyMod, ScopeId, SemanticNodeData, SemanticNodeId, SemanticQueryValueTag,
    SurfaceMember, SurfaceView, TypeParamDecl, ValueRootKey,
};
use crate::{CompileErrorPolicy, HostConfig, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn graph_of(host: &VerterHost) -> Arc<crate::semantic_query_memo::SemanticGraphStore> {
    Arc::clone(host.project_type_store().semantic_graph())
}

/// The oracle: the raised `TypeExpr` the Kind-B bridge would produce for
/// `node` (the test-only shell-raise mirror), `None` on a whole-raise miss.
fn raise_oracle(host: &VerterHost, node: SemanticNodeId) -> Option<TypeExpr> {
    let dispatch = ProjectSemanticDispatch::new(host);
    dispatch.materialize_output_type_expr_for_test(node)
}

/// Assert ALL three node-domain classifiers equal the legacy `TypeExpr`
/// predicate applied to the raised oracle output for `node`. This is the core
/// parity contract; every fixture routes through here.
#[track_caller]
fn assert_classifier_parity(host: &VerterHost, node: SemanticNodeId, label: &str) {
    let oracle = raise_oracle(host, node);

    // SPLIT PARITY: the tree oracle is INERT (no raw sentinel
    // classification reads the compat tree); the typed miss fact is
    // cross-checked against the node-domain facts below. Pin the inertness
    // here so the split is asserted on EVERY fixture.
    if let Some(e) = &oracle {
        assert!(
            !type_expr_contains_semantic_miss(e),
            "[{label}] the compat tree must be INERT — no raw sentinel classification (oracle = {oracle:?})"
        );
    }
    // Self-consistency of the TYPED fact across the two node-domain
    // projections: `node_contains_semantic_miss_or_unraisable` must equal
    // `!facts.materialized` (or `true` when the whole raise is `None`).
    let facts = node_raised_shape_facts(host, node);
    assert_eq!(
        node_contains_semantic_miss_or_unraisable(host, node),
        facts.map(|f| !f.materialized()).unwrap_or(true),
        "[{label}] the typed miss fact must agree across node-domain projections (oracle = {oracle:?})"
    );

    let expect_expanded = match &oracle {
        Some(e) => type_expr_is_expanded_surface(e),
        None => false,
    };
    assert_eq!(
        node_is_expanded_surface_legacy_equivalent(host, node),
        expect_expanded,
        "[{label}] node_is_expanded_surface must equal \
         type_expr_is_expanded_surface(raise(node)) (oracle = {oracle:?})"
    );

    assert_eq!(
        node_can_shell_raise(host, node),
        oracle.is_some(),
        "[{label}] node_can_shell_raise must equal raise(node).is_some() (oracle = {oracle:?})"
    );

    // FACTS-ONLY vs FULL-FOLD parity: the facts the FACTS-ONLY algebra
    // (`node_raised_shape_facts` -> RaisedFactsAlg, no key interning) produces
    // MUST byte-equal the facts the KEY-bearing algebra
    // (`node_raised_shape_for_eq` -> RaisedShapeAlg) produces for the SAME node.
    // Both build their per-arm facts through the SHARED `summary` layer, so any
    // drift between the two algebras (a divergent fact/tag formula in one) fails
    // here. `node_raised_shape_for_eq` always returns `Some(true)` for
    // `eq_to_expr` against the node's own raise, so the facts are what we assert.
    let facts_only = node_raised_shape_facts(host, node);
    match (&oracle, facts_only) {
        (Some(e), Some(facts_only)) => {
            let full = node_raised_shape_for_eq(host, node, e)
                .expect("a raisable node yields a combined facts+eq projection");
            assert_eq!(
                (
                    facts_only.materialized(),
                    facts_only.expanded_surface(),
                    facts_only.can_shell_raise()
                ),
                (
                    full.facts().materialized(),
                    full.facts().expanded_surface(),
                    full.facts().can_shell_raise()
                ),
                "[{label}] FACTS-ONLY algebra facts must equal KEY-bearing algebra facts \
                 (shared summary layer) (oracle = {oracle:?})"
            );
            assert!(
                full.eq_to_expr(),
                "[{label}] node_raised_shape_for_eq(node, raise(node)).eq_to_expr must be true"
            );
        }
        (None, None) => {}
        _ => panic!(
            "[{label}] facts-only raisability must agree with the oracle (oracle = {oracle:?}, \
             facts_only.is_some() = {})",
            facts_only.is_some()
        ),
    }

    // Equality of the node against its OWN raised `TypeExpr`: must be
    // `Some(true)` whenever the raise is `Some` (a node always equals its own
    // raised shape), `None` when the raise is `None`.
    match &oracle {
        Some(e) => assert_eq!(
            raised_shape_eq_node_type_expr(host, node, e),
            Some(true),
            "[{label}] raised_shape_eq_node_type_expr(node, raise(node)) must be Some(true)"
        ),
        None => assert_eq!(
            raised_shape_eq_node_type_expr(host, node, &TypeExpr::Primitive(PrimitiveName::Never)),
            None,
            "[{label}] raised_shape_eq_node_type_expr on an unraisable node must be None"
        ),
    }
}

/// Assert `raised_shape_eq_nodes(a, b)` matches the oracle comparison of the
/// two raised `TypeExpr`s (when both raise `Some`).
#[track_caller]
fn assert_eq_nodes_parity(host: &VerterHost, a: SemanticNodeId, b: SemanticNodeId, label: &str) {
    let oa = raise_oracle(host, a);
    let ob = raise_oracle(host, b);
    let observed = raised_shape_eq_nodes(host, a, b);
    match (oa, ob) {
        (Some(ea), Some(eb)) => assert_eq!(
            observed,
            Some(ea == eb),
            "[{label}] raised_shape_eq_nodes must equal (raise(a) == raise(b))"
        ),
        _ => assert_eq!(
            observed, None,
            "[{label}] raised_shape_eq_nodes must be None when either raise is None"
        ),
    }
}

// ---------------------------------------------------------------------------
// Class 1 — opaque errors (incl. sentinel-like `Other`), raw fallback.
// ---------------------------------------------------------------------------

#[test]
fn parity_opaque_errors_and_raw_fallback() {
    let host = host();
    let graph = graph_of(&host);

    // `Opaque(Miss)` raises to the `SEMANTIC_MISS` sentinel ⇒ NOT materialized.
    let miss = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    assert_classifier_parity(&host, miss, "opaque-miss");
    assert!(
        node_contains_semantic_miss_or_unraisable(&host, miss),
        "Opaque(Miss) raises to a sentinel ⇒ contains-semantic-miss must be true"
    );

    // `Opaque(Other("custom"))` raises to `Unknown { raw: "custom" }` which is
    // NOT in the sentinel set ⇒ MATERIALIZED (proves the sentinel set is exact,
    // not "any Unknown is a miss").
    let other = graph.intern_node(SemanticNodeData::Opaque(QueryError::Other(Arc::from(
        "custom",
    ))));
    assert_classifier_parity(&host, other, "opaque-other-nonsentinel");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, other),
        "Opaque(Other(\"custom\")) is a non-sentinel Unknown ⇒ NOT a semantic miss"
    );

    // `Opaque(BudgetExceeded)` raises to the `budgetExceeded(` prefix sentinel
    // ⇒ NOT materialized.
    let budget = graph.intern_node(SemanticNodeData::Opaque(QueryError::BudgetExceeded(
        BudgetExceededFailure {
            domain: BudgetDomain::ProjectionOperation,
            limit: 2000,
            actual: 2001,
            context: "parity fixture".to_string(),
        },
    )));
    assert_classifier_parity(&host, budget, "opaque-budget-exceeded");
    assert!(
        node_contains_semantic_miss_or_unraisable(&host, budget),
        "Opaque(BudgetExceeded) raises to the budget prefix sentinel ⇒ semantic miss"
    );

    // `Opaque(RecursiveRef)` raises to `TypeExpr::RecursiveRef` ⇒ MATERIALIZED
    // (a materialized leaf, not a sentinel).
    let recursive = graph.intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
        name: Arc::from("Tree"),
    }));
    assert_classifier_parity(&host, recursive, "opaque-recursive-ref");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, recursive),
        "Opaque(RecursiveRef) raises to RecursiveRef ⇒ materialized, NOT a miss"
    );

    // `Opaque(DeclPlaceholder)` raises to a `Ref` shell ⇒ MATERIALIZED.
    let placeholder = graph.intern_node(SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
        name: Arc::from("Pending"),
        canonical_id: Arc::from("/w/p.ts"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: [0u8; 16],
    }));
    assert_classifier_parity(&host, placeholder, "opaque-decl-placeholder");

    // Raw-fallback carrier — a non-sentinel raw ⇒ MATERIALIZED.
    let raw = graph.intern_node(SemanticNodeData::RawFallback {
        value: verter_type_expr::UnknownValue::unsupported_syntax("Weird<& Type>"),
    });
    assert_classifier_parity(&host, raw, "raw-fallback-nonsentinel");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, raw),
        "RawFallback(non-sentinel text) ⇒ materialized"
    );

    // FLIPPED: a Raw-fallback carrier whose text IS a
    // legacy sentinel spelling is a GENUINE `UnknownValue` — ALWAYS
    // materialized, NEVER a sentinel. Degradation is typed-only.
    let raw_sentinel = graph.intern_node(SemanticNodeData::RawFallback {
        value: verter_type_expr::UnknownValue::unsupported_syntax("semanticObjectSurface"),
    });
    assert_classifier_parity(&host, raw_sentinel, "raw-fallback-sentinel-text");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, raw_sentinel),
        "a genuine UnknownValue spelled like a sentinel is MATERIALIZED (typed-only degradation)"
    );
}

/// BEHAVIOUR-PRESERVATION across the typed-sentinel swap: the converted
/// `fold_node` arms (here, the surface-member
/// fallback) must raise to the BYTE-IDENTICAL legacy compat spelling AND drive
/// the SAME downstream materialised/miss decision the old hardcoded literal did —
/// end-to-end through the real graph fixtures, not just at the algebra entry
/// point. The spelling is now the terminal compatibility projection
/// (inert text); the miss decision is read off the TYPED node-domain fact.
/// Each assertion here is exact (a producer emitting a different/empty
/// spelling, or losing the typed miss verdict, fails).
#[test]
fn typed_control_sentinel_producers_raise_byte_identical_and_keep_miss_decision() {
    use crate::resolver_core::component_meta_query_engine::SEMANTIC_SURFACE_MEMBER;

    let host = host();
    let graph = graph_of(&host);

    // Surface-member fallback (the CONVERTED `fold_member` miss arm): an object
    // whose member value node is unraisable hits the `SEMANTIC_SURFACE_MEMBER`
    // fallback. Pin the member's raised `raw` and the object-level miss verdict.
    let unraisable_member = SemanticNodeId(u64::MAX);
    let obj_with_unraisable_member = graph.intern_node(SemanticNodeData::Object(object_surface(
        &[("broken", unraisable_member)],
    )));
    let obj_raised = raise_oracle(&host, obj_with_unraisable_member)
        .expect("an object with one (unraisable-valued) member still raises to Some");
    let member_raw = match &obj_raised {
        TypeExpr::Object(object) => {
            let property = object
                .properties
                .iter()
                .find_map(|m| match m {
                    verter_type_expr::ObjectMember::Property(p) if p.name == "broken" => {
                        Some(&p.ty)
                    }
                    _ => None,
                })
                .expect("the `broken` member must survive into the projected surface");
            match property {
                TypeExpr::Unknown(value) => value.raw().to_string(),
                other => panic!("the unraisable member must fall back to Unknown, got {other:?}"),
            }
        }
        other => panic!("expected an object surface, got {other:?}"),
    };
    assert_eq!(
        member_raw, SEMANTIC_SURFACE_MEMBER,
        "the converted surface-member fallback must project the byte-identical SEMANTIC_SURFACE_MEMBER spelling"
    );
    assert!(
        node_contains_semantic_miss_or_unraisable(&host, obj_with_unraisable_member),
        "an object whose member degraded to UnrepresentableSurfaceMember is a TYPED semantic miss"
    );
}

/// The `fold_node` `Opaque(err)` arm routes the typed `QueryError` through the
/// typed `opaque_sentinel` algebra entry (not a string round-trip). The
/// MATERIALIZE path must still emit the byte-identical legacy spelling (the
/// terminal compatibility projection), and the NODE-DOMAIN path classifies on
/// the TYPED variant ONLY — for the full `_ => opaque_sentinel`-reachable set
/// the arm routes (every `Opaque` variant EXCEPT `RecursiveRef` /
/// `DeclPlaceholder`, which hit the earlier `recursive_ref` / `reference`
/// sub-arms).
///
/// SPLIT PARITY + the FLIPPED text-bearing equations:
/// - `materialized`: an `Opaque(Other("semanticMiss"))` projects the
///   byte-identical `semanticMiss` spelling, but the typed verdict is
///   MATERIALIZED — `Other` payloads are INERT text, never sentinels (the
///   legacy delegation equation is deleted).
/// - `tag` (end-to-end): an `Opaque(Other("semanticObjectSurface"))` arm is
///   RETAINED in an intersection (NEVER dropped — only a typed root
///   `UnrepresentableSurface` degradation drops); the typed
///   `UnrepresentableSurface` arm IS dropped (covered by
///   `parity_intersection_arm_drop_and_collapse` and the dedicated
///   discriminating test below).
#[test]
fn opaque_arm_routes_through_typed_sentinel_byte_identical_and_keeps_node_domain_verdict() {
    use crate::resolver_core::component_meta_query_engine::semantic_query_error_raw;

    let host = host();
    let graph = graph_of(&host);

    // The `_ => opaque_sentinel`-reachable set the converted `Opaque(err)` arm
    // routes (the two specialised sub-arms — `RecursiveRef` → `recursive_ref` and
    // the earlier `Opaque(DeclPlaceholder)` → `reference` shell — raise to
    // `RecursiveRef` / `Ref` and are NOT this arm's responsibility; the typed
    // authority's classification of those carriers is covered by the agreement
    // test in `raise_sentinel.rs`). Across the materialisation classes: a
    // recognised exact sentinel (`Miss`), the recognised prefix sentinels
    // (`UnsupportedIntrinsic` → `unsupportedIntrinsic(<name>)`, `BudgetExceeded`),
    // the unmaterialised producer-control carriers
    // (`UnstableState`, `AliasCycle`, `RaiseAliasCycle`, `UnrepresentableSurface`,
    // `UnrepresentableSurfaceMember`), the
    // MATERIALISED producer placeholders (`TypeParamCycle`, `RaiseMiss`,
    // `ValueDomainMismatch`), the adversarial text-bearing sentinel
    // (`Other("semanticMiss")`) + object-surface-spelling (`Other("semanticObjectSurface")`)
    // + prefix-text (`Other("budgetExceeded(x)")`), and a benign non-sentinel
    // (`Other("free text")`).
    let variants = [
        QueryError::Miss,
        QueryError::UnsupportedIntrinsic {
            name: Arc::from("FixtureIntrinsic"),
        },
        QueryError::BudgetExceeded(BudgetExceededFailure {
            domain: BudgetDomain::ProjectionOperation,
            limit: 2000,
            actual: 2001,
            context: "opaque-arm fixture".to_string(),
        }),
        QueryError::UnstableState { attempts: 3 },
        QueryError::AliasCycle {
            chain: Arc::from(vec![Arc::from("A"), Arc::from("B")].into_boxed_slice()),
        },
        QueryError::ValueDomainMismatch {
            expected: SemanticQueryValueTag::TypeNode,
            actual: SemanticQueryValueTag::Relation,
        },
        QueryError::RaiseAliasCycle,
        QueryError::TypeParamCycle,
        QueryError::RaiseMiss,
        QueryError::UnrepresentableSurface,
        QueryError::UnrepresentableSurfaceMember,
        QueryError::Other(Arc::from("semanticMiss")),
        QueryError::Other(Arc::from("semanticObjectSurface")),
        QueryError::Other(Arc::from("budgetExceeded(x)")),
        QueryError::Other(Arc::from("free text")),
    ];

    for variant in variants {
        let node = graph.intern_node(SemanticNodeData::Opaque(variant.clone()));

        // MATERIALIZE path: byte-identical terminal compatibility projection —
        // the tree EQUALS a genuine `UnknownValue` with the legacy spelling
        // (raw-only identity), and the interned key is EQUAL too
        // (`assert_classifier_parity`'s eq_to_expr block).
        let raised = raise_oracle(&host, node).expect("an Opaque(err) node raises to Some");
        assert_eq!(
            raised,
            TypeExpr::Unknown(verter_type_expr::UnknownValue::compatibility_projection(
                semantic_query_error_raw(&variant)
            )),
            "the Opaque({variant:?}) arm must project byte-identical to the legacy \
             Unknown {{ raw: semantic_query_error_raw(err) }} spelling"
        );

        // NODE-DOMAIN path: typed classification only (split parity).
        assert_classifier_parity(&host, node, &format!("opaque-arm-{variant:?}"));
    }

    // The FLIPPED `materialized` equation: an `Other("semanticMiss")` payload
    // is INERT — MATERIALIZED, never a miss (only the typed `Miss` variant is
    // the miss sentinel).
    let adversarial = graph.intern_node(SemanticNodeData::Opaque(QueryError::Other(Arc::from(
        "semanticMiss",
    ))));
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, adversarial),
        "Opaque(Other(\"semanticMiss\")) is INERT — MATERIALIZED (typed-only classification)"
    );
    let typed_miss = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    assert!(
        node_contains_semantic_miss_or_unraisable(&host, typed_miss),
        "the typed Miss variant IS the miss sentinel"
    );

    // The FLIPPED end-to-end `tag` equation: an
    // `Opaque(Other("semanticObjectSurface"))` arm is RETAINED in an
    // intersection — `Other` never acts as the surface sentinel.
    let real_member = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let real_obj = graph.intern_node(SemanticNodeData::Object(object_surface(&[(
        "kept",
        real_member,
    )])));
    let object_surface_other_arm = graph.intern_node(SemanticNodeData::Opaque(QueryError::Other(
        Arc::from("semanticObjectSurface"),
    )));
    // Sanity: the lone arm is MATERIALIZED (inert Other payload).
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, object_surface_other_arm),
        "Opaque(Other(\"semanticObjectSurface\")) is INERT — MATERIALIZED"
    );
    let inter_other_real = graph.intern_node(SemanticNodeData::Intersection(Arc::from(
        vec![object_surface_other_arm, real_obj].into_boxed_slice(),
    )));
    assert_classifier_parity(
        &host,
        inter_other_real,
        "intersection-object-surface-other-text-and-real",
    );
    assert_eq!(
        raised_shape_eq_nodes(&host, inter_other_real, real_obj),
        Some(false),
        "(Other(\"semanticObjectSurface\") & RealObject) RETAINS the arm — a 2-arm Intersection \
         that is NOT equal to the lone real object"
    );
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, inter_other_real),
        "the retained-Other intersection is materialized (Other payloads are inert)"
    );
}

// ---------------------------------------------------------------------------
// Class 2 — aliases (incl. cycles), TypeParam cycles, MergedDecl.
// ---------------------------------------------------------------------------

#[test]
fn parity_alias_chain_transparent() {
    let host = host();
    let graph = graph_of(&host);

    let leaf = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let alias1 = graph.intern_node(SemanticNodeData::Alias(leaf));
    let alias2 = graph.intern_node(SemanticNodeData::Alias(alias1));

    // A transparent alias chain raises to the leaf's shape.
    assert_classifier_parity(&host, alias1, "alias-1");
    assert_classifier_parity(&host, alias2, "alias-2-nested");

    // The whole chain raises EQUAL to the leaf ⇒ raised-shape equality holds
    // even though the node ids differ (carrier/alias transparency).
    assert_eq_nodes_parity(&host, leaf, alias2, "alias-vs-leaf");
    assert_eq!(
        raised_shape_eq_nodes(&host, leaf, alias2),
        Some(true),
        "alias chain raises EQUAL to its leaf — raised-shape equality must be Some(true)"
    );
}

#[test]
fn parity_alias_cycle_sentinel_string_classification() {
    let host = host();
    let graph = graph_of(&host);

    // A TRUE `Alias` self-cycle (or 2-node mutual `Alias` cycle) — which fires
    // the raiser's `active.insert` cycle arm emitting `Unknown { raw:
    // "semanticAliasCycle" }` — is NOT constructible from a directly-interned
    // graph fixture: the node arena is append-only with sequential ids
    // (`SemanticGraphStore::intern_node` → `arena.push`), so a node cannot
    // reference its own (not-yet-assigned) id and there is no
    // placeholder-then-patch entry point to wire a mutual A→B/B→A cycle. The
    // raiser's cycle arm is exercised instead by the resolver-driven cycle tests
    // (`project_semantic_dispatch::tests::alias_cycle_returns_opaque_cyclic_not_stack_overflow`
    // and `mutual_alias_cycle_x_y_x_returns_opaque...`), where the cycle is
    // produced by the resolver re-visiting a resolved declaration identity.
    //
    // FLIPPED: a `RawFallback` mirror carrying the
    // alias-cycle / type-param-cycle SPELLING is a genuine `UnknownValue` —
    // ALWAYS materialized, never a sentinel. The typed classifications live
    // on the `Opaque(QueryError::RaiseAliasCycle)` /
    // `Opaque(QueryError::TypeParamCycle)` carriers, pinned here directly.
    let alias_cycle_spelling = graph.intern_node(SemanticNodeData::RawFallback {
        value: verter_type_expr::UnknownValue::unsupported_syntax("semanticAliasCycle"),
    });
    assert_classifier_parity(&host, alias_cycle_spelling, "alias-cycle-spelling-genuine");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, alias_cycle_spelling),
        "a genuine UnknownValue spelled `semanticAliasCycle` is MATERIALIZED"
    );
    let tp_cycle_spelling = graph.intern_node(SemanticNodeData::RawFallback {
        value: verter_type_expr::UnknownValue::unsupported_syntax("semanticTypeParamCycle"),
    });
    assert_classifier_parity(&host, tp_cycle_spelling, "typeparam-cycle-spelling-genuine");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, tp_cycle_spelling),
        "a genuine UnknownValue spelled `semanticTypeParamCycle` is MATERIALIZED"
    );

    // The TYPED carriers keep their classifications: `RaiseAliasCycle` is an
    // unmaterialised sentinel; `TypeParamCycle` is deliberately materialized.
    let alias_cycle_typed =
        graph.intern_node(SemanticNodeData::Opaque(QueryError::RaiseAliasCycle));
    assert_classifier_parity(&host, alias_cycle_typed, "alias-cycle-typed-carrier");
    assert!(
        node_contains_semantic_miss_or_unraisable(&host, alias_cycle_typed),
        "the TYPED RaiseAliasCycle carrier ⇒ semantic miss"
    );
    let tp_cycle_typed = graph.intern_node(SemanticNodeData::Opaque(QueryError::TypeParamCycle));
    assert_classifier_parity(&host, tp_cycle_typed, "typeparam-cycle-typed-carrier");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, tp_cycle_typed),
        "the TYPED TypeParamCycle carrier is deliberately materialized"
    );
}

#[test]
fn parity_merged_decl_peer_merges_to_object() {
    let host = host();
    let graph = graph_of(&host);

    // Two same-name interface contributors (bare objects) peer-merge into one
    // Object surface. The raiser reduces the MergedDecl then raises the merged
    // object — the classifier must read THAT shape, not the raw MergedDecl kind.
    let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let obj_a = graph.intern_node(SemanticNodeData::Object(object_surface(&[(
        "a", string_id,
    )])));
    let obj_b = graph.intern_node(SemanticNodeData::Object(object_surface(&[(
        "b", number_id,
    )])));
    let merged = graph.intern_node(SemanticNodeData::MergedDecl {
        contributors: Arc::from(vec![obj_a, obj_b].into_boxed_slice()),
    });
    assert_classifier_parity(&host, merged, "merged-decl-two-objects");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, merged),
        "a merged decl of two concrete objects raises to a materialized Object"
    );
    assert!(
        node_is_expanded_surface_legacy_equivalent(&host, merged),
        "a merged Object surface is an expanded (non-open) surface"
    );
}

// ---------------------------------------------------------------------------
// Class 3 — lazy carriers: DeclRef, InstantiationRef, BareRef, ImportType,
// TypeOf, SyntheticBinding.
// ---------------------------------------------------------------------------

#[test]
fn parity_lazy_carriers() {
    let host = host();
    let graph = graph_of(&host);

    let declref = graph.intern_node(SemanticNodeData::DeclRef {
        identity: DeclIdentity {
            canonical_id: Arc::from("/w/d.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: [0u8; 16],
            decl_name: Arc::from("Foo"),
        },
    });
    assert_classifier_parity(&host, declref, "declref");

    // A carrier WITH a non-empty type-arg that MATERIALIZES: the `Number` arg
    // raises to a real `TypeExpr::Primitive`, so the InstantiationRef raises to
    // `Ref { name: "Box", type_arguments: [Primitive(Number)] }` (no miss
    // placeholder).
    let arg = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let instref = graph.intern_node(SemanticNodeData::InstantiationRef {
        base: DeclIdentity {
            canonical_id: Arc::from("/w/d.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: [0u8; 16],
            decl_name: Arc::from("Box"),
        },
        args: Arc::from(vec![arg].into_boxed_slice()),
    });
    assert_classifier_parity(&host, instref, "instantiation-ref");

    // A carrier whose type-arg points at an ABSENT node id ⇒ the mandatory
    // arg position degrades to the TYPED `UnrepresentableSurfaceMember`
    // (terminal projection `semanticSurfaceMember`) so the OUTER `Ref` still
    // constructs (the carrier does NOT fail the whole raise). The node-domain
    // `reference_leaf` fact stays MATERIALIZED (a `Ref` is materialized
    // regardless of arg shapes); the payload-side degradation is carried by
    // the materialize sidecar (partial), pinned separately.
    let instref_miss_arg = graph.intern_node(SemanticNodeData::InstantiationRef {
        base: DeclIdentity {
            canonical_id: Arc::from("/w/d.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: [0u8; 16],
            decl_name: Arc::from("Box"),
        },
        args: Arc::from(vec![SemanticNodeId(u64::MAX)].into_boxed_slice()),
    });
    assert_classifier_parity(&host, instref_miss_arg, "instantiation-ref-raise-miss-arg");
    assert!(
        node_can_shell_raise(&host, instref_miss_arg),
        "a carrier with an absent type-arg still raises (the arg degrades to the typed \
         surface-member fallback; the outer Ref constructs)"
    );
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, instref_miss_arg),
        "the node-domain Ref-leaf fact is materialized regardless of the degraded arg"
    );

    let bare = graph.intern_node(SemanticNodeData::new_bare_ref(
        Arc::from("Bar"),
        NodeScopeId::Global,
        Arc::from(Vec::new().into_boxed_slice()),
    ));
    assert_classifier_parity(&host, bare, "bare-ref");

    let import = graph.intern_node(SemanticNodeData::new_import_type(
        Arc::from("./m"),
        Arc::from(vec![Arc::<str>::from("A")].into_boxed_slice()),
        Arc::from(Vec::new().into_boxed_slice()),
        false,
    ));
    assert_classifier_parity(&host, import, "import-type");

    let typeof_node = graph.intern_node(SemanticNodeData::new_typeof(
        ValueRootKey {
            scope: ScopeId {
                canonical_id: Arc::from("/m.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                local_scope: None,
                binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
            },
            name: Arc::from("factory"),
        },
        Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
        Arc::from(Vec::new().into_boxed_slice()),
    ));
    // `TypeOf` raises to `TypeExpr::TypeOf` ⇒ a materialized leaf, but it IS an
    // open deferred shell ⇒ expanded_surface == false. The parity helper proves
    // both facts against the oracle.
    assert_classifier_parity(&host, typeof_node, "typeof-carrier");
    assert!(
        !node_is_expanded_surface_legacy_equivalent(&host, typeof_node),
        "a raised-root TypeOf is an open deferred shell ⇒ NOT an expanded surface"
    );
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, typeof_node),
        "a raised TypeOf is a materialized leaf, not a sentinel"
    );

    let synthetic = graph.intern_node(SemanticNodeData::SyntheticBinding {
        id: crate::semantic_query::SyntheticBindingId {
            scope_canonical_id: Arc::from("/Comp.vue"),
            surface_kind: verter_type_expr::SyntheticCarrierSurfaceKind::SlotBinding,
            slot_name: Some(Arc::from("default")),
            binding_name: Arc::from("row"),
        },
        value_node: 7,
    });
    assert_classifier_parity(&host, synthetic, "synthetic-binding");
}

// ---------------------------------------------------------------------------
// Class 4 — object edge cases: empty surface, single call signature, open
// synthetic index signature, missing member raises.
// ---------------------------------------------------------------------------

#[test]
fn parity_object_edge_cases() {
    let host = host();
    let graph = graph_of(&host);

    // Empty surface ⇒ representable empty `Object{}` ⇒ materialized + expanded.
    let empty = graph.intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from(Vec::<SurfaceMember>::new().into_boxed_slice()),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
            completeness: crate::semantic_query::MemberSurfaceCompleteness::Closed,
        },
    ));
    assert_classifier_parity(&host, empty, "object-empty");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, empty),
        "an empty object is the representable {{}} ⇒ materialized"
    );

    // A surface with one concrete property ⇒ materialized.
    let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let obj = graph.intern_node(SemanticNodeData::Object(object_surface(&[(
        "foo", string_id,
    )])));
    assert_classifier_parity(&host, obj, "object-one-prop");

    // A surface whose member value raises to a sentinel-bearing shape ⇒ the
    // member is the `SEMANTIC_SURFACE_MEMBER` fallback only when the member
    // value MISSES; a member pointing at an opaque-miss node raises that member
    // to the miss sentinel ⇒ the object contains a semantic miss.
    let member_miss = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let obj_with_miss = graph.intern_node(SemanticNodeData::Object(object_surface(&[(
        "bad",
        member_miss,
    )])));
    assert_classifier_parity(&host, obj_with_miss, "object-member-miss");
    assert!(
        node_contains_semantic_miss_or_unraisable(&host, obj_with_miss),
        "an object whose member raises to the Miss sentinel contains a semantic miss"
    );

    // Open synthetic index signature ⇒ the synthetic `projectedOpenSurface`
    // value is a sentinel ⇒ the object contains a semantic miss.
    let open = graph.intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from(Vec::<SurfaceMember>::new().into_boxed_slice()),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: true,
            completeness: crate::semantic_query::MemberSurfaceCompleteness::Closed,
        },
    ));
    assert_classifier_parity(&host, open, "object-open-index");
}

// ---------------------------------------------------------------------------
// Class 5 — intersections with empty-object AND SEMANTIC_OBJECT_SURFACE arms.
// Both arm-DROP paths are exercised with DIRECT fixtures: the empty-`Object{}`
// arm and the `SEMANTIC_OBJECT_SURFACE` sentinel arm (an Object whose only
// signature raises non-Function), each followed by the 0/1/many collapse.
// ---------------------------------------------------------------------------

#[test]
fn parity_intersection_arm_drop_and_collapse() {
    let host = host();
    let graph = graph_of(&host);

    let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let real_obj = graph.intern_node(SemanticNodeData::Object(object_surface(&[(
        "a", string_id,
    )])));
    let empty_obj = graph.intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from(Vec::<SurfaceMember>::new().into_boxed_slice()),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
            completeness: crate::semantic_query::MemberSurfaceCompleteness::Closed,
        },
    ));

    // `{} & RealObject` ⇒ drops the empty arm, collapses to RealObject ⇒
    // MATERIALIZED (proves the collapse: the raw graph has 2 arms, the raised
    // shape is a single materialized Object).
    let inter_empty_real = graph.intern_node(SemanticNodeData::Intersection(Arc::from(
        vec![empty_obj, real_obj].into_boxed_slice(),
    )));
    assert_classifier_parity(&host, inter_empty_real, "intersection-empty-and-real");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, inter_empty_real),
        "({{}} & RealObject) collapses to RealObject ⇒ materialized"
    );

    // The collapsed intersection raises EQUAL to the remaining real arm — the
    // raised-shape equality must see them equal even though the node ids differ.
    assert_eq_nodes_parity(
        &host,
        inter_empty_real,
        real_obj,
        "collapse-vs-remaining-arm",
    );
    assert_eq!(
        raised_shape_eq_nodes(&host, inter_empty_real, real_obj),
        Some(true),
        "the 1-arm-collapsed intersection raises EQUAL to its remaining arm"
    );

    // `{} & {}` ⇒ every arm vacuous ⇒ falls back to empty `Object{}` ⇒
    // materialized.
    let inter_both_empty = graph.intern_node(SemanticNodeData::Intersection(Arc::from(
        vec![empty_obj, empty_obj].into_boxed_slice(),
    )));
    assert_classifier_parity(&host, inter_both_empty, "intersection-both-empty");

    // A `SEMANTIC_OBJECT_SURFACE`-raising Object arm: an Object surface that is
    // NOT structurally-empty (it carries a construct-signature) but whose only
    // signature raises to a NON-Function shape, so the Object reconstruction
    // yields zero representable members and the raiser emits
    // `Unknown { raw: SEMANTIC_OBJECT_SURFACE }`. This exercises the
    // SENTINEL-arm DROP path (distinct from the empty-`Object{}`-arm drop above):
    // `Intersection` filters out `Unknown { raw == SEMANTIC_OBJECT_SURFACE }`
    // arms, so `SurfaceSentinel & RealObject` collapses to RealObject.
    let non_fn_ctor = graph.intern_node(SemanticNodeData::Opaque(QueryError::Other(Arc::from(
        "not-a-fn",
    ))));
    let surface_sentinel = graph.intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from(Vec::<SurfaceMember>::new().into_boxed_slice()),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(vec![non_fn_ctor].into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
            completeness: crate::semantic_query::MemberSurfaceCompleteness::Closed,
        },
    ));
    // Sanity: the surface-sentinel node alone raises to the SEMANTIC_OBJECT_SURFACE
    // sentinel ⇒ semantic miss (proves the arm we are about to drop is the real
    // sentinel, not an incidental materialized shape).
    assert_classifier_parity(&host, surface_sentinel, "object-surface-sentinel");
    assert!(
        node_contains_semantic_miss_or_unraisable(&host, surface_sentinel),
        "an Object whose only construct-signature raises to a non-Function shape raises to the \
         SEMANTIC_OBJECT_SURFACE sentinel ⇒ semantic miss"
    );

    // `SurfaceSentinel & RealObject` ⇒ drops the SEMANTIC_OBJECT_SURFACE arm,
    // collapses to RealObject ⇒ MATERIALIZED (the sentinel-arm drop + 1-arm
    // collapse, mirroring how `Id<T> = {} & { … }` helper patterns reduce).
    let inter_sentinel_real = graph.intern_node(SemanticNodeData::Intersection(Arc::from(
        vec![surface_sentinel, real_obj].into_boxed_slice(),
    )));
    assert_classifier_parity(&host, inter_sentinel_real, "intersection-sentinel-and-real");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, inter_sentinel_real),
        "(SurfaceSentinel & RealObject) drops the sentinel arm and collapses to RealObject ⇒ \
         materialized"
    );
    assert_eq!(
        raised_shape_eq_nodes(&host, inter_sentinel_real, real_obj),
        Some(true),
        "the sentinel-arm-dropped intersection raises EQUAL to its remaining real arm"
    );

    // `RealObject & RealObject2` (two real arms) ⇒ stays an Intersection of
    // materialized arms ⇒ materialized + expanded.
    let number_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let real_obj2 = graph.intern_node(SemanticNodeData::Object(object_surface(&[(
        "b", number_id,
    )])));
    let inter_two_real = graph.intern_node(SemanticNodeData::Intersection(Arc::from(
        vec![real_obj, real_obj2].into_boxed_slice(),
    )));
    assert_classifier_parity(&host, inter_two_real, "intersection-two-real");
    assert!(
        node_is_expanded_surface_legacy_equivalent(&host, inter_two_real),
        "an intersection of two object arms is an expanded surface"
    );
}

/// The intersection arm-drop is typed-root-only: a root-level
/// `QueryError::UnrepresentableSurface` degradation is removed (vacuous arm),
/// while an IDENTICALLY-SPELLED genuine `UnknownValue` arm AND a
/// `QueryError::Other("semanticObjectSurface")` arm are both RETAINED (the
/// flipped legacy equations — raw spelling never drops an arm).
#[test]
fn intersection_drops_only_typed_root_unrepresentable_surface() {
    let host = host();
    let graph = graph_of(&host);

    let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let real_obj = graph.intern_node(SemanticNodeData::Object(object_surface(&[(
        "a", string_id,
    )])));

    // (1) The TYPED root degradation drops: `UnrepresentableSurface & Real`
    // collapses to `Real`.
    let typed_surface =
        graph.intern_node(SemanticNodeData::Opaque(QueryError::UnrepresentableSurface));
    let inter_typed_real = graph.intern_node(SemanticNodeData::Intersection(Arc::from(
        vec![typed_surface, real_obj].into_boxed_slice(),
    )));
    assert_eq!(
        raised_shape_eq_nodes(&host, inter_typed_real, real_obj),
        Some(true),
        "a typed ROOT UnrepresentableSurface arm is REMOVED (collapses to the real arm)"
    );
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, inter_typed_real),
        "the collapsed intersection is materialized"
    );

    // (2) An IDENTICALLY-SPELLED genuine `UnknownValue` arm is RETAINED.
    let genuine_surface_spelling = graph.intern_node(SemanticNodeData::RawFallback {
        value: verter_type_expr::UnknownValue::unsupported_syntax("semanticObjectSurface"),
    });
    let inter_genuine_real = graph.intern_node(SemanticNodeData::Intersection(Arc::from(
        vec![genuine_surface_spelling, real_obj].into_boxed_slice(),
    )));
    assert_eq!(
        raised_shape_eq_nodes(&host, inter_genuine_real, real_obj),
        Some(false),
        "an identically-spelled GENUINE UnknownValue arm is NEVER removed"
    );
    // … and its terminal tree keeps the legacy two-arm shape with the exact
    // spelling (the projection is inert text).
    let raised = raise_oracle(&host, inter_genuine_real).expect("the intersection raises");
    match &raised {
        TypeExpr::Intersection(arms) => {
            assert_eq!(arms.len(), 2, "both arms survive");
            assert!(
                matches!(&arms[0], TypeExpr::Unknown(value) if value.raw() == "semanticObjectSurface"),
                "the genuine arm keeps its exact spelling, got {raised:?}"
            );
        }
        other => panic!("expected a retained 2-arm Intersection, got {other:?}"),
    }

    // (3) A `QueryError::Other("semanticObjectSurface")` arm is RETAINED —
    // `Other` never acts as the surface sentinel.
    let other_surface = graph.intern_node(SemanticNodeData::Opaque(QueryError::Other(Arc::from(
        "semanticObjectSurface",
    ))));
    let inter_other_real = graph.intern_node(SemanticNodeData::Intersection(Arc::from(
        vec![other_surface, real_obj].into_boxed_slice(),
    )));
    assert_eq!(
        raised_shape_eq_nodes(&host, inter_other_real, real_obj),
        Some(false),
        "a QueryError::Other(\"semanticObjectSurface\") arm is NEVER removed"
    );
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, inter_other_real),
        "the retained-Other intersection is materialized (Other payloads are inert)"
    );
}

// ---------------------------------------------------------------------------
// Class 6 — deferred operator shells (KeyOf / IndexedAccess / Mapped /
// Conditional / TypeOf) — expanded_surface == false, plus `?`-None propagation.
// ---------------------------------------------------------------------------

#[test]
fn parity_deferred_operator_shells() {
    let host = host();
    let graph = graph_of(&host);

    let tp = graph.intern_node(SemanticNodeData::TypeParam {
        decl: decl_identity_unscoped("/w/op.ts", "T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });

    // `keyof T` ⇒ raised-root KeyOf ⇒ expanded_surface == false, materialized
    // (the operand T raises to a materialized TypeParameter).
    let keyof = graph.intern_node(SemanticNodeData::KeyOf { base: tp });
    assert_classifier_parity(&host, keyof, "keyof-T");
    assert!(
        !node_is_expanded_surface_legacy_equivalent(&host, keyof),
        "raised-root KeyOf ⇒ NOT an expanded surface"
    );

    // `T[K]` (indexed access with a TypeNode index) ⇒ raised-root IndexedAccess
    // ⇒ expanded_surface == false.
    let k = graph.intern_node(SemanticNodeData::TypeParam {
        decl: decl_identity_unscoped("/w/op.ts", "K"),
        param_index: 1,
        constraint: None,
        default: None,
        display_name: Arc::from("K"),
    });
    let indexed = graph.intern_node(SemanticNodeData::IndexedAccess {
        object: tp,
        index: IndexKey::TypeNode(k),
    });
    assert_classifier_parity(&host, indexed, "indexed-access-T-K");
    assert!(
        !node_is_expanded_surface_legacy_equivalent(&host, indexed),
        "raised-root IndexedAccess ⇒ NOT an expanded surface"
    );

    // A conditional shell ⇒ raised-root Conditional ⇒ expanded_surface == false.
    let bool_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let str_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let num_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let conditional = graph.intern_node(SemanticNodeData::Conditional {
        check: tp,
        extends: bool_id,
        true_branch_ref: str_id,
        false_branch_ref: num_id,
        distributive: false,
    });
    assert_classifier_parity(&host, conditional, "conditional-shell");
    assert!(
        !node_is_expanded_surface_legacy_equivalent(&host, conditional),
        "raised-root Conditional ⇒ NOT an expanded surface"
    );

    // `?`-None propagation: an Array whose element node is ABSENT from the graph
    // makes the WHOLE raise None ⇒ can_shell_raise == false, contains-miss ==
    // true, expanded == false.
    let array_missing_elem = graph.intern_node(SemanticNodeData::Array {
        element: SemanticNodeId(u64::MAX),
        readonly: false,
    });
    assert_classifier_parity(&host, array_missing_elem, "array-missing-element-none");
    assert!(
        !node_can_shell_raise(&host, array_missing_elem),
        "an Array over an absent element node raises to None (?-propagation)"
    );

    // A Mapped shell over an open generic key-space ⇒ raised-root Mapped ⇒
    // expanded_surface == false.
    let value_tp = graph.intern_node(SemanticNodeData::TypeParam {
        decl: decl_identity_unscoped("/w/op.ts", "V"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("V"),
    });
    let keyspace = graph.intern_node(SemanticNodeData::KeyOf { base: tp });
    let mapped = graph.intern_node(SemanticNodeData::Mapped {
        source: tp,
        mapper: MapperKey {
            parameter_node: tp,
            key_space: keyspace,
            value_expr: value_tp,
            optionality: OptionalityMod::Keep,
            readonly: ReadonlyMod::Keep,
            name_remap: None,
            kind: MapperKind::Computed,
        },
    });
    assert_classifier_parity(&host, mapped, "mapped-shell");
    assert!(
        !node_is_expanded_surface_legacy_equivalent(&host, mapped),
        "raised-root Mapped ⇒ NOT an expanded surface"
    );
}

// ---------------------------------------------------------------------------
// Union / terminal coverage + the whole-raise-None classifier contract.
// ---------------------------------------------------------------------------

#[test]
fn parity_union_and_terminals_and_none() {
    let host = host();
    let graph = graph_of(&host);

    let str_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let num_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    assert_classifier_parity(&host, str_id, "primitive-string");

    // `string | number` ⇒ materialized union, expanded.
    let union = graph.intern_node(SemanticNodeData::Union(Arc::from(
        vec![str_id, num_id].into_boxed_slice(),
    )));
    assert_classifier_parity(&host, union, "union-string-number");
    assert!(
        node_is_expanded_surface_legacy_equivalent(&host, union),
        "a union of materialized primitives is an expanded surface"
    );

    // `string | Opaque(Miss)` ⇒ the miss arm raises to a sentinel ⇒ the union
    // contains a semantic miss (Union recurses).
    let miss = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let union_with_miss = graph.intern_node(SemanticNodeData::Union(Arc::from(
        vec![str_id, miss].into_boxed_slice(),
    )));
    assert_classifier_parity(&host, union_with_miss, "union-with-miss");
    assert!(
        node_contains_semantic_miss_or_unraisable(&host, union_with_miss),
        "a union with a Miss arm contains a semantic miss"
    );

    // Whole-raise-None classifier contract: an absent node id ⇒ can_shell_raise
    // false, contains-miss true, expanded false, eq_node_type_expr None.
    let absent = SemanticNodeId(u64::MAX);
    assert!(
        raise_oracle(&host, absent).is_none(),
        "absent node misses the oracle"
    );
    assert!(
        !node_can_shell_raise(&host, absent),
        "absent ⇒ cannot shell-raise"
    );
    assert!(
        node_contains_semantic_miss_or_unraisable(&host, absent),
        "absent ⇒ contains-miss is true (None treated as miss)"
    );
    assert!(
        !node_is_expanded_surface_legacy_equivalent(&host, absent),
        "absent ⇒ expanded surface is false"
    );
    assert_eq!(
        raised_shape_eq_node_type_expr(&host, absent, &TypeExpr::Primitive(PrimitiveName::Never)),
        None,
        "absent node ⇒ raised_shape_eq_node_type_expr is None"
    );
    assert_eq!(
        raised_shape_eq_nodes(&host, absent, str_id),
        None,
        "absent node ⇒ raised_shape_eq_nodes is None"
    );
}

// ---------------------------------------------------------------------------
// Equality-divergence pairs — DIFFERENT nodes raise EQUAL (so raw node-id
// equality would be WRONG and the raised-shape primitive is RIGHT).
// ---------------------------------------------------------------------------

#[test]
fn parity_equality_pairs_different_nodes_raise_equal() {
    let host = host();
    let graph = graph_of(&host);

    // Different-scope `BareRef("T")`: two distinct nodes (distinct scope) that
    // raise to the SAME `Ref { name: "T" }` (BareRef drops scope on raise).
    let scope_a = NodeScopeId::File {
        canonical_id: Arc::from("/w/a.ts"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: [1u8; 16],
        local_scope: Some(1),
    };
    let scope_b = NodeScopeId::File {
        canonical_id: Arc::from("/w/b.ts"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: [2u8; 16],
        local_scope: Some(2),
    };
    let bare_a = graph.intern_node_with_scope(
        SemanticNodeData::new_bare_ref(
            Arc::from("T"),
            scope_a,
            Arc::from(Vec::new().into_boxed_slice()),
        ),
        NodeScopeId::Global,
    );
    let bare_b = graph.intern_node_with_scope(
        SemanticNodeData::new_bare_ref(
            Arc::from("T"),
            scope_b,
            Arc::from(Vec::new().into_boxed_slice()),
        ),
        NodeScopeId::Global,
    );
    assert_ne!(bare_a, bare_b, "the two BareRef nodes must be DISTINCT ids");
    assert_eq_nodes_parity(&host, bare_a, bare_b, "bareref-different-scope");
    assert_eq!(
        raised_shape_eq_nodes(&host, bare_a, bare_b),
        Some(true),
        "two different-scope BareRef(\"T\") raise EQUAL ⇒ raised-shape equality is Some(true) \
         (raw node-id equality would be WRONG here)"
    );

    // Different-canonical `DeclRef` with the SAME display name: both raise to
    // `Ref { name: "Same" }` (DeclRef drops canonical identity on raise).
    let declref_a = graph.intern_node(SemanticNodeData::DeclRef {
        identity: DeclIdentity {
            canonical_id: Arc::from("/w/a.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: [1u8; 16],
            decl_name: Arc::from("Same"),
        },
    });
    let declref_b = graph.intern_node(SemanticNodeData::DeclRef {
        identity: DeclIdentity {
            canonical_id: Arc::from("/w/b.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: [2u8; 16],
            decl_name: Arc::from("Same"),
        },
    });
    assert_ne!(
        declref_a, declref_b,
        "the two DeclRef nodes must be DISTINCT ids"
    );
    assert_eq_nodes_parity(&host, declref_a, declref_b, "declref-different-canonical");
    assert_eq!(
        raised_shape_eq_nodes(&host, declref_a, declref_b),
        Some(true),
        "two DeclRef with same name but different canonical raise EQUAL"
    );

    // `DeclPlaceholder` vs `DeclRef` with the same name: both raise to `Ref {
    // name }` ⇒ EQUAL even though the node KINDS differ.
    let placeholder = graph.intern_node(SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
        name: Arc::from("Same"),
        canonical_id: Arc::from("/w/c.ts"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: [3u8; 16],
    }));
    assert_eq_nodes_parity(&host, placeholder, declref_a, "placeholder-vs-declref");
    assert_eq!(
        raised_shape_eq_nodes(&host, placeholder, declref_a),
        Some(true),
        "DeclPlaceholder and DeclRef with the same name raise to the same Ref shape"
    );

    // Negative: two DIFFERENT names must raise UNEQUAL (the key discriminates).
    let declref_other = graph.intern_node(SemanticNodeData::DeclRef {
        identity: DeclIdentity {
            canonical_id: Arc::from("/w/a.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: [1u8; 16],
            decl_name: Arc::from("Different"),
        },
    });
    assert_eq!(
        raised_shape_eq_nodes(&host, declref_a, declref_other),
        Some(false),
        "DeclRefs with DIFFERENT names raise UNEQUAL ⇒ Some(false)"
    );
    assert_eq_nodes_parity(&host, declref_a, declref_other, "declref-different-name");
}

// ---------------------------------------------------------------------------
// Discrimination self-tests — prove the suite is NOT vacuous: the oracle
// separates the fact values, so an always-true / always-false classifier (or a
// node-id-equality stub) could not pass the parity assertions above.
// ---------------------------------------------------------------------------

#[test]
fn discrimination_oracle_separates_the_facts() {
    let host = host();
    let graph = graph_of(&host);

    // contains_semantic_miss is BOTH true (Miss) and false (String) across the
    // corpus — a constant classifier cannot match both.
    let miss = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    assert!(node_contains_semantic_miss_or_unraisable(&host, miss));
    assert!(!node_contains_semantic_miss_or_unraisable(&host, string_id));

    // expanded_surface is BOTH true (Object) and false (KeyOf) — a constant
    // classifier cannot match both.
    let obj = graph.intern_node(SemanticNodeData::Object(object_surface(&[(
        "a", string_id,
    )])));
    let tp = graph.intern_node(SemanticNodeData::TypeParam {
        decl: decl_identity_unscoped("/w/d.ts", "T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let keyof = graph.intern_node(SemanticNodeData::KeyOf { base: tp });
    assert!(node_is_expanded_surface_legacy_equivalent(&host, obj));
    assert!(!node_is_expanded_surface_legacy_equivalent(&host, keyof));

    // can_shell_raise is BOTH true (String) and false (Array over absent elem) —
    // a constant classifier cannot match both.
    let array_missing = graph.intern_node(SemanticNodeData::Array {
        element: SemanticNodeId(u64::MAX),
        readonly: false,
    });
    assert!(node_can_shell_raise(&host, string_id));
    assert!(!node_can_shell_raise(&host, array_missing));
}

#[test]
fn discrimination_equality_is_not_node_id_equality() {
    // PROOF the equality primitive is raised-shape, NOT node-id: two DISTINCT
    // node ids that raise EQUAL must compare `Some(true)` (a node-id-equality
    // stub `a == b` would return `Some(false)` here and FAIL).
    let host = host();
    let graph = graph_of(&host);

    let declref_a = graph.intern_node(SemanticNodeData::DeclRef {
        identity: DeclIdentity {
            canonical_id: Arc::from("/w/a.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: [1u8; 16],
            decl_name: Arc::from("Same"),
        },
    });
    let declref_b = graph.intern_node(SemanticNodeData::DeclRef {
        identity: DeclIdentity {
            canonical_id: Arc::from("/w/b.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: [9u8; 16],
            decl_name: Arc::from("Same"),
        },
    });
    assert_ne!(declref_a, declref_b);
    assert_eq!(
        raised_shape_eq_nodes(&host, declref_a, declref_b),
        Some(true),
        "raised-shape equality of two distinct nodes raising the SAME Ref must be Some(true) — a \
         node-id-equality stub would FAIL this"
    );

    // And the SAME node id is the fast path ⇒ Some(true).
    assert_eq!(
        raised_shape_eq_nodes(&host, declref_a, declref_a),
        Some(true),
        "same node id ⇒ Some(true) fast path"
    );
}

// Sanity: the raiser is byte-identical via the shared core — a primitive
// round-trips through the oracle to the exact `TypeExpr` (a coarse byte-identity
// canary alongside the existing raise suite that proves the delegation).
#[test]
fn shared_core_raiser_output_matches_oracle_for_primitive() {
    let host = host();
    let graph = graph_of(&host);
    let s = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    assert_eq!(
        raise_oracle(&host, s),
        Some(TypeExpr::Primitive(PrimitiveName::String)),
        "the shared-core raiser still produces the exact primitive TypeExpr"
    );
}

// ---------------------------------------------------------------------------
// Class 7 — recursion/carrier boundaries the bottom-up node-domain
// reimplementation must preserve: shapes where the RAISER recurses children but
// `type_expr_contains_semantic_miss` treats the raised node as a LEAF (the
// leaf-vs-recurse divergence surface). DIRECT graph fixtures; each routes
// through `assert_classifier_parity`, so each DISCRIMINATES against the oracle.
// ---------------------------------------------------------------------------

#[test]
fn parity_function_and_constructor_type() {
    let host = host();
    let graph = graph_of(&host);

    let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    // `(a: string) => number` — Function with one materialized param + return.
    // `contains_semantic_miss` recurses the return + params; all materialized ⇒
    // NOT a miss, AND a Function is an expanded surface.
    let func = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from(
            vec![FunctionParam::synthetic(
                Some(Arc::from("a")),
                string_id,
                false,
                false,
            )]
            .into_boxed_slice(),
        ),
        return_type: number_id,
        type_parameters: Arc::from(Vec::<TypeParamDecl>::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    });
    assert_classifier_parity(&host, func, "function-materialized");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, func),
        "a Function with all-materialized params + return ⇒ NOT a semantic miss"
    );
    assert!(
        node_is_expanded_surface_legacy_equivalent(&host, func),
        "a raised Function is an expanded surface"
    );

    // A Function whose PARAM raises to a sentinel (`Opaque(Miss)` param ty) ⇒
    // `contains_semantic_miss` recurses INTO the param (Function is NOT a leaf for
    // the miss predicate) ⇒ semantic miss. The parity helper proves the raiser
    // and predicate agree on the recursion.
    let miss = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let func_miss_param = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from(
            vec![FunctionParam::synthetic(
                Some(Arc::from("bad")),
                miss,
                false,
                false,
            )]
            .into_boxed_slice(),
        ),
        return_type: number_id,
        type_parameters: Arc::from(Vec::<TypeParamDecl>::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    });
    assert_classifier_parity(&host, func_miss_param, "function-miss-param");
    assert!(
        node_contains_semantic_miss_or_unraisable(&host, func_miss_param),
        "a Function whose param raises to the Miss sentinel contains a semantic miss (the miss \
         predicate recurses the param — NOT a leaf)"
    );

    // `new (a: string) => Foo` — ConstructorType over a Function signature node.
    // The raiser raises the signature Function then rewraps as ConstructorType;
    // the miss predicate treats ConstructorType identically to Function (recurses
    // the same FunctionExpr payload).
    let ctor = graph.intern_construct_twin_for_tests(func);
    assert_classifier_parity(&host, ctor, "constructor-type-materialized");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, ctor),
        "a ConstructorType over a materialized signature ⇒ NOT a semantic miss"
    );

    // A ConstructorType whose signature param misses ⇒ semantic miss (recursion
    // through the construct signature, mirroring Function).
    let ctor_miss = graph.intern_construct_twin_for_tests(func_miss_param);
    assert_classifier_parity(&host, ctor_miss, "constructor-type-miss-param");
    assert!(
        node_contains_semantic_miss_or_unraisable(&host, ctor_miss),
        "a ConstructorType whose signature param misses contains a semantic miss"
    );
}

#[test]
fn parity_template_literal_leaf_vs_recurse_boundary() {
    let host = host();
    let graph = graph_of(&host);

    let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // `` `a${string}` `` — TemplateLiteral with a materialized expression child.
    // The raiser RECURSES the expression, but `type_expr_contains_semantic_miss`
    // treats a raised `TypeExpr::TemplateLiteral` as a LEAF (always materialized)
    // — the exact leaf-vs-recurse boundary. ⇒ NOT a miss.
    let template = graph.intern_node(SemanticNodeData::TemplateLiteral {
        quasis: Arc::from(vec![Arc::<str>::from("a"), Arc::<str>::from("")].into_boxed_slice()),
        expressions: Arc::from(vec![string_id].into_boxed_slice()),
    });
    assert_classifier_parity(&host, template, "template-literal-materialized-expr");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, template),
        "a raised TemplateLiteral is a LEAF for the miss predicate ⇒ NOT a miss"
    );

    // A TemplateLiteral whose expression child raises to a SENTINEL-bearing shape
    // (an `Opaque(Miss)` expr) STILL reads as MATERIALIZED — because the miss
    // predicate does NOT recurse the raised TemplateLiteral's expressions (leaf),
    // even though the raiser recursed the child. This is the leaf-vs-recurse
    // divergence the bottom-up node-domain reimplementation MUST preserve:
    // assert it matches LEGACY (the oracle), not intuition.
    let miss = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let template_miss_expr = graph.intern_node(SemanticNodeData::TemplateLiteral {
        quasis: Arc::from(vec![Arc::<str>::from("x"), Arc::<str>::from("")].into_boxed_slice()),
        expressions: Arc::from(vec![miss].into_boxed_slice()),
    });
    assert_classifier_parity(&host, template_miss_expr, "template-literal-miss-expr-leaf");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, template_miss_expr),
        "a TemplateLiteral whose expr raises to a sentinel STILL reads materialized — the miss \
         predicate treats the raised TemplateLiteral as a LEAF (leaf-vs-recurse divergence, \
         matched to LEGACY)"
    );
    assert!(
        node_is_expanded_surface_legacy_equivalent(&host, template_miss_expr),
        "a raised TemplateLiteral is an expanded surface"
    );
}

#[test]
fn parity_type_param_with_constraint_and_default() {
    let host = host();
    let graph = graph_of(&host);

    let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    // `T extends string = number` — TypeParam carrying BOTH constraint AND
    // default. The raiser recurses constraint + default into the projected
    // `TypeExpr::TypeParameter`, but `type_expr_contains_semantic_miss` treats a
    // raised `TypeParameter` as a LEAF (does NOT recurse constraint/default). ⇒
    // NOT a miss regardless of the constraint/default shapes.
    let tp = graph.intern_node(SemanticNodeData::TypeParam {
        decl: decl_identity_unscoped("/w/tp.ts", "T"),
        param_index: 0,
        constraint: Some(string_id),
        default: Some(number_id),
        display_name: Arc::from("T"),
    });
    assert_classifier_parity(&host, tp, "typeparam-constraint-and-default");
    assert!(
        node_can_shell_raise(&host, tp),
        "a TypeParam with constraint + default raises"
    );
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, tp),
        "a raised TypeParameter is a LEAF for the miss predicate ⇒ NOT a miss"
    );

    // A TypeParam whose CONSTRAINT raises to a sentinel (`Opaque(Miss)`
    // constraint) STILL reads MATERIALIZED — the miss predicate does NOT recurse
    // the raised TypeParameter's constraint/default (leaf), though the raiser
    // recursed them. Assert it matches LEGACY (the oracle), per the brief's
    // "facts must match LEGACY" requirement for the constraint/default case.
    let miss = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let tp_miss_constraint = graph.intern_node(SemanticNodeData::TypeParam {
        decl: decl_identity_unscoped("/w/tp.ts", "U"),
        param_index: 1,
        constraint: Some(miss),
        default: Some(miss),
        display_name: Arc::from("U"),
    });
    assert_classifier_parity(&host, tp_miss_constraint, "typeparam-miss-constraint-leaf");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, tp_miss_constraint),
        "a TypeParam whose constraint/default raise to a sentinel STILL reads materialized — the \
         miss predicate treats the raised TypeParameter as a LEAF (matched to LEGACY)"
    );
}

#[test]
fn parity_real_index_signature_member() {
    let host = host();
    let graph = graph_of(&host);

    let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    // A REAL `{ [k: string]: number }` index signature (a declared
    // `index_signatures` entry, NOT the synthetic open `has_index_signature`
    // placeholder). The raiser re-emits its declared key/value shape; the miss
    // predicate recurses `key_type` + `value_type`. Both materialized ⇒ NOT a
    // miss, and an Object with a real index signature is an expanded surface.
    let string_keyed = graph.intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from(Vec::<SurfaceMember>::new().into_boxed_slice()),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(
                vec![IndexSignature {
                    key_type: string_id,
                    value_type: number_id,
                    readonly: false,
                    spans: Default::default(),
                    declaration_origin: Some(Arc::from("/w/idx.ts")),
                }]
                .into_boxed_slice(),
            ),
            keyspace: None,
            // A REAL declared index signature is present, so `has_index_signature`
            // is true but `index_signatures` is non-empty (NOT the synthetic open
            // case that injects the `projectedOpenSurface` sentinel).
            has_index_signature: true,
            completeness: crate::semantic_query::MemberSurfaceCompleteness::Closed,
        },
    ));
    assert_classifier_parity(
        &host,
        string_keyed,
        "object-real-index-signature-string-number",
    );
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, string_keyed),
        "an Object with a real `[k: string]: number` index signature ⇒ materialized"
    );
    assert!(
        node_is_expanded_surface_legacy_equivalent(&host, string_keyed),
        "an Object with a real index signature is an expanded surface"
    );

    // A real index signature whose VALUE type misses ⇒ the miss predicate
    // recurses `value_type` ⇒ semantic miss (proves the recursion into the
    // declared index signature, not a leaf).
    let miss = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let idx_value_miss = graph.intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from(Vec::<SurfaceMember>::new().into_boxed_slice()),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(
                vec![IndexSignature {
                    key_type: string_id,
                    value_type: miss,
                    readonly: false,
                    spans: Default::default(),
                    declaration_origin: Some(Arc::from("/w/idx.ts")),
                }]
                .into_boxed_slice(),
            ),
            keyspace: None,
            has_index_signature: true,
            completeness: crate::semantic_query::MemberSurfaceCompleteness::Closed,
        },
    ));
    assert_classifier_parity(&host, idx_value_miss, "object-index-signature-value-miss");
    assert!(
        node_contains_semantic_miss_or_unraisable(&host, idx_value_miss),
        "an index signature whose value type raises to the Miss sentinel contains a semantic miss"
    );
}

#[test]
fn parity_carriers_with_type_args_and_raise_miss() {
    let host = host();
    let graph = graph_of(&host);

    let number_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let absent = SemanticNodeId(u64::MAX);

    // --- BareRef<Arg> ---------------------------------------------------------
    // `Foo<number>` — BareRef with a materialized type-arg ⇒ raises to `Ref {
    // name: "Foo", type_arguments: [Primitive(Number)] }`. A `Ref` is a LEAF for
    // the miss predicate (it does NOT recurse `type_arguments`) ⇒ NOT a miss.
    let bare_arg = graph.intern_node(SemanticNodeData::new_bare_ref(
        Arc::from("Foo"),
        NodeScopeId::Global,
        Arc::from(vec![number_id].into_boxed_slice()),
    ));
    assert_classifier_parity(&host, bare_arg, "bare-ref-with-arg");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, bare_arg),
        "a BareRef<number> raises to a Ref leaf ⇒ NOT a miss"
    );

    // `Foo<absent>` — the absent type-arg degrades to the TYPED
    // `UnrepresentableSurfaceMember` (projection `semanticSurfaceMember`) so the
    // OUTER Ref still constructs. The node-domain `reference_leaf` fact stays
    // MATERIALIZED (does NOT taint the parent); the payload-side degradation is
    // carried by the materialize sidecar.
    let bare_miss_arg = graph.intern_node(SemanticNodeData::new_bare_ref(
        Arc::from("Foo"),
        NodeScopeId::Global,
        Arc::from(vec![absent].into_boxed_slice()),
    ));
    assert_classifier_parity(&host, bare_miss_arg, "bare-ref-raise-miss-arg");
    assert!(
        node_can_shell_raise(&host, bare_miss_arg),
        "a BareRef with an absent type-arg still raises (the arg degrades to the typed \
         surface-member fallback; the outer Ref constructs)"
    );
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, bare_miss_arg),
        "the `<raise miss>` carrier-arg placeholder is NOT a sentinel ⇒ the BareRef reads \
         materialized (the carrier-arg miss does not taint the parent)"
    );

    // --- ImportType<Arg> ------------------------------------------------------
    // `import("./m").A<number>` — ImportType with a materialized type-arg ⇒
    // raises to `ImportType { type_arguments: [Primitive(Number)] }`, a LEAF ⇒
    // NOT a miss.
    let import_arg = graph.intern_node(SemanticNodeData::new_import_type(
        Arc::from("./m"),
        Arc::from(vec![Arc::<str>::from("A")].into_boxed_slice()),
        Arc::from(vec![number_id].into_boxed_slice()),
        false,
    ));
    assert_classifier_parity(&host, import_arg, "import-type-with-arg");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, import_arg),
        "an ImportType<number> raises to an ImportType leaf ⇒ NOT a miss"
    );

    // `import("./m").A<absent>` — the absent type-arg becomes `<raise miss>`; the
    // outer ImportType still constructs and reads MATERIALIZED.
    let import_miss_arg = graph.intern_node(SemanticNodeData::new_import_type(
        Arc::from("./m"),
        Arc::from(vec![Arc::<str>::from("A")].into_boxed_slice()),
        Arc::from(vec![absent].into_boxed_slice()),
        false,
    ));
    assert_classifier_parity(&host, import_miss_arg, "import-type-raise-miss-arg");
    assert!(
        node_can_shell_raise(&host, import_miss_arg)
            && !node_contains_semantic_miss_or_unraisable(&host, import_miss_arg),
        "an ImportType with an absent type-arg materialises the arg as <raise miss>; the carrier \
         reads materialized"
    );

    // --- TypeOf<Arg> ----------------------------------------------------------
    // `typeof factory<number>` — TypeOf carrying an instantiation type-arg ⇒ the
    // arg raises onto `ValueRef.type_args`. A raised `TypeOf` is a LEAF for the
    // miss predicate ⇒ NOT a miss (but it IS an open deferred shell ⇒
    // expanded_surface == false).
    let typeof_arg = graph.intern_node(SemanticNodeData::new_typeof(
        ValueRootKey {
            scope: ScopeId {
                canonical_id: Arc::from("/m.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                local_scope: None,
                binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
            },
            name: Arc::from("factory"),
        },
        Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
        Arc::from(vec![number_id].into_boxed_slice()),
    ));
    assert_classifier_parity(&host, typeof_arg, "typeof-with-arg");
    assert!(
        !node_contains_semantic_miss_or_unraisable(&host, typeof_arg),
        "a raised TypeOf is a LEAF for the miss predicate ⇒ NOT a miss"
    );
    assert!(
        !node_is_expanded_surface_legacy_equivalent(&host, typeof_arg),
        "a raised-root TypeOf is an open deferred shell ⇒ NOT an expanded surface"
    );

    // `typeof factory<absent>` — the absent instantiation arg becomes `<raise
    // miss>` on `ValueRef.type_args`; the outer TypeOf still constructs and reads
    // MATERIALIZED (the carrier-arg miss does not taint the parent).
    let typeof_miss_arg = graph.intern_node(SemanticNodeData::new_typeof(
        ValueRootKey {
            scope: ScopeId {
                canonical_id: Arc::from("/m.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                local_scope: None,
                binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
            },
            name: Arc::from("factory"),
        },
        Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
        Arc::from(vec![absent].into_boxed_slice()),
    ));
    assert_classifier_parity(&host, typeof_miss_arg, "typeof-raise-miss-arg");
    assert!(
        node_can_shell_raise(&host, typeof_miss_arg)
            && !node_contains_semantic_miss_or_unraisable(&host, typeof_miss_arg),
        "a TypeOf with an absent instantiation arg degrades it to the typed surface-member \
         fallback; the node-domain carrier fact reads materialized"
    );
}

#[test]
fn raised_shape_eq_node_type_expr_negative_some_false_discriminates() {
    // `raised_shape_eq_node_type_expr` must return `Some(false)` when a RAISABLE
    // node's raised shape does NOT equal the supplied `TypeExpr`. Without a
    // `Some(false)` assertion, an always-`Some(true)` implementation would pass
    // the parity suite (which only checks node-vs-its-own-raise = `Some(true)` and
    // unraisable = `None`).
    let host = host();
    let graph = graph_of(&host);

    // A `Literal("idle")` node raises to `TypeExpr::Literal("idle")`, which does
    // NOT equal `TypeExpr::Primitive(Number)` ⇒ `Some(false)`.
    let literal = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::String("idle".to_string()),
    ));
    assert_eq!(
        raised_shape_eq_node_type_expr(&host, literal, &TypeExpr::Primitive(PrimitiveName::Number)),
        Some(false),
        "a Literal(\"idle\") node's raised shape must NOT equal Primitive(Number) ⇒ Some(false) \
         (an always-Some(true) impl would FAIL here)"
    );
    // Same node vs its OWN raised shape is `Some(true)` (sanity — proves the node
    // raises and the comparand discriminates, not a blanket false).
    let literal_oracle = raise_oracle(&host, literal).expect("literal raises");
    assert_eq!(
        raised_shape_eq_node_type_expr(&host, literal, &literal_oracle),
        Some(true),
        "a Literal node vs its own raised shape must be Some(true)"
    );

    // A `Primitive(String)` node raises to `TypeExpr::Primitive(String)` which
    // does NOT equal `TypeExpr::Primitive(Number)` ⇒ `Some(false)` (broad-kind
    // mismatch).
    let string_prim = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    assert_eq!(
        raised_shape_eq_node_type_expr(
            &host,
            string_prim,
            &TypeExpr::Primitive(PrimitiveName::Number)
        ),
        Some(false),
        "a Primitive(String) node's raised shape must NOT equal Primitive(Number) ⇒ Some(false)"
    );

    // STRUCTURED mismatch: an `Object { a: string }` node raises to a one-property
    // object that does NOT equal a bare `TypeExpr::Primitive(Object)` ⇒
    // `Some(false)` (the structured shape is compared, not collapsed to a kind).
    let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let object_node = graph.intern_node(SemanticNodeData::Object(object_surface(&[(
        "a", string_id,
    )])));
    assert_eq!(
        raised_shape_eq_node_type_expr(
            &host,
            object_node,
            &TypeExpr::Primitive(PrimitiveName::Object)
        ),
        Some(false),
        "an Object {{ a: string }} node's raised shape must NOT equal the bare Primitive(Object) \
         keyword ⇒ Some(false) (structured comparison)"
    );

    // CARRIER mismatch: a `BareRef Foo<number>` raises to `Ref { name: \"Foo\",
    // type_arguments: [Number] }` which does NOT equal `Ref { name: \"Foo\",
    // type_arguments: [] }` (the bare `Foo`) ⇒ `Some(false)` (type-arg payload is
    // part of the raised-shape identity).
    let number_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let bare_with_arg = graph.intern_node(SemanticNodeData::new_bare_ref(
        Arc::from("Foo"),
        NodeScopeId::Global,
        Arc::from(vec![number_id].into_boxed_slice()),
    ));
    assert_eq!(
        raised_shape_eq_node_type_expr(
            &host,
            bare_with_arg,
            &TypeExpr::Ref {
                name: Arc::from("Foo"),
                type_arguments: verter_type_expr::empty_type_args(),
            }
        ),
        Some(false),
        "a BareRef Foo<number> must NOT raise-equal the bare `Foo` (Ref with empty type_arguments) \
         ⇒ Some(false) (type-arg payload is part of the raised-shape identity)"
    );
    // ...but it DOES raise-equal `Ref { name: \"Foo\", type_arguments: [Number] }`
    // ⇒ Some(true) (proves the Some(false) above is discriminating on the arg, not
    // a blanket false).
    assert_eq!(
        raised_shape_eq_node_type_expr(
            &host,
            bare_with_arg,
            &TypeExpr::Ref {
                name: Arc::from("Foo"),
                type_arguments: Arc::from(
                    vec![TypeExpr::Primitive(PrimitiveName::Number)].into_boxed_slice()
                ),
            }
        ),
        Some(true),
        "a BareRef Foo<number> DOES raise-equal `Ref {{ name: Foo, type_arguments: [Number] }}` ⇒ \
         Some(true)"
    );
}

#[test]
fn raised_shape_eq_node_type_expr_ignores_has_ts_annotation_like_typeexpr_partialeq() {
    // REGRESSION (FINDING 1): the interned `RaisedShapeKey` must be EXACTLY
    // `TypeExpr::PartialEq`-equivalent. `verter_type_expr::FunctionParam`'s
    // hand-written `PartialEq`/`Eq`/`Hash` DELIBERATELY EXCLUDES
    // `has_ts_annotation` (a transient lowering-time gate, not semantic
    // identity). The raised mirror (`RaisedFunctionParam`) must exclude it too,
    // or the key would distinguish a field `TypeExpr::PartialEq` ignores —
    // falsely reading a no-op as "changed" at the `surface.rs` route gates
    // (`lower_and_project_to_expanded_node` / `instantiate_local_
    // generic_ref_published` :261).
    //
    // The node side ALWAYS raises `has_ts_annotation: false` (the materializer
    // hardcodes it; `build_raised_function` hardcodes it). So the divergence
    // triggers when the INPUT `TypeExpr` carries `has_ts_annotation: true` (any
    // annotated function param). This test builds exactly that pair and asserts
    // the key compare AGREES with `TypeExpr::PartialEq`.
    //
    // Before the hand-written-eq fix this asserts `Some(false)` (RED); after, it
    // asserts `Some(true)` (GREEN). The existing parity fixtures all use
    // `FunctionParam::synthetic` → `has_ts_annotation: false`, so they never
    // exercised the mismatch — that gap is why the bug shipped.
    //
    // NOTE: `SemanticNodeData::Signature` uses the GRAPH param
    // (`semantic_query::FunctionParam`, already imported at the top); the input
    // `TypeExpr::Function` uses the IR param (`verter_type_expr::FunctionParam`).
    // They are distinct types — alias the IR ones to keep both in scope.
    use verter_type_expr::{
        FunctionExpr as IrFunctionExpr, FunctionParam as IrFunctionParam, TypeExpr,
    };

    let host = host();
    let graph = graph_of(&host);

    let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    // `(a: string) => number` — the node side. Its param raises with
    // `has_ts_annotation: false` (synthetic param → false; materializer/algebra
    // both hardcode false).
    let func_node = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from(
            vec![FunctionParam::synthetic(
                Some(Arc::from("a")),
                string_id,
                false,
                false,
            )]
            .into_boxed_slice(),
        ),
        return_type: number_id,
        type_parameters: Arc::from(Vec::<TypeParamDecl>::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    });

    // The oracle shape the node raises to (param `has_ts_annotation: false`).
    let oracle = raise_oracle(&host, func_node).expect("function node raises");

    // The EXTERNAL input `TypeExpr`: the SAME `(a: string) => number` shape but
    // its param carries `has_ts_annotation: true` (an annotated param). Built
    // with `with_span(.., true)`; span `None` and default function spans so it
    // is otherwise byte-for-byte the oracle's shape.
    let expr_annotation_true = TypeExpr::Function(Arc::new(IrFunctionExpr::synthetic(
        vec![IrFunctionParam::with_span(
            Some("a".to_string()),
            TypeExpr::Primitive(PrimitiveName::String),
            false,
            false,
            None,
            true, // <-- the ONLY difference from the node-raised shape
        )],
        Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Number))),
        Vec::new(),
    )));

    // PREMISE: the `has_ts_annotation: true` input IS `TypeExpr::PartialEq`-equal
    // to the node's raised shape (the field is excluded from `FunctionParam`'s
    // hand-written eq). If this premise broke, the test would not be testing the
    // bug.
    assert_eq!(
        oracle, expr_annotation_true,
        "PREMISE: TypeExpr::PartialEq must IGNORE has_ts_annotation — the annotated \
         input equals the node's raised shape (oracle = {oracle:?})"
    );

    // The FIX: the key compare must AGREE with `TypeExpr::PartialEq` ⇒ Some(true).
    // (Pre-fix, the derived `RaisedFunctionParam` distinguishes the field ⇒
    // Some(false), and the route gate falsely reads "changed".)
    assert_eq!(
        raised_shape_eq_node_type_expr(&host, func_node, &expr_annotation_true),
        Some(true),
        "raised_shape_eq_node_type_expr must IGNORE has_ts_annotation exactly like \
         TypeExpr::PartialEq: a node raising a param (has_ts_annotation=false) vs a \
         PartialEq-equal input whose param has has_ts_annotation=true must be Some(true)"
    );

    // DISCRIMINATION GUARD: prove the key compare still SEPARATES a genuine
    // change (a different param TYPE), so the Some(true) above is not a blanket
    // "always equal" — `(a: number) => number` must NOT raise-equal the node ⇒
    // Some(false).
    let expr_changed_param_ty = TypeExpr::Function(Arc::new(IrFunctionExpr::synthetic(
        vec![IrFunctionParam::with_span(
            Some("a".to_string()),
            TypeExpr::Primitive(PrimitiveName::Number), // <-- real shape change
            false,
            false,
            None,
            true,
        )],
        Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Number))),
        Vec::new(),
    )));
    assert_eq!(
        raised_shape_eq_node_type_expr(&host, func_node, &expr_changed_param_ty),
        Some(false),
        "a real param-TYPE change (string -> number) must still be Some(false) — proves \
         the has_ts_annotation Some(true) is field-exclusion, not a blanket true"
    );
}

#[test]
fn raise_oracle_payload_shape_preserves_recursed_children() {
    // The leaf-vs-recursion fixtures assert PREDICATE parity (the miss predicate
    // treats a raised TemplateLiteral / TypeParameter / Ref-carrier as a LEAF),
    // which MASKS the recursed child PAYLOAD: if `TemplateLiteral` / `TypeParam` /
    // carrier-type-arg raising stopped preserving the recursed child shape, the
    // leaf predicate would still read "materialized" and the parity tests would
    // not notice. These DIRECT `raise_oracle(...)` shape assertions pin the
    // recursed child payloads so a regression in child-raising is caught.
    let host = host();
    let graph = graph_of(&host);

    let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let miss = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));

    // (1) TemplateLiteral with a materialized expression child: the raised
    // `TypeExpr::TemplateLiteral` must PRESERVE the recursed expression
    // (`Primitive(String)`) in its `expressions`, not drop it.
    let template = graph.intern_node(SemanticNodeData::TemplateLiteral {
        quasis: Arc::from(vec![Arc::<str>::from("a"), Arc::<str>::from("")].into_boxed_slice()),
        expressions: Arc::from(vec![string_id].into_boxed_slice()),
    });
    let template_oracle = raise_oracle(&host, template).expect("template raises");
    match &template_oracle {
        TypeExpr::TemplateLiteral { expressions, .. } => {
            assert_eq!(
                expressions.as_ref(),
                &[TypeExpr::Primitive(PrimitiveName::String)],
                "the raised TemplateLiteral must preserve the recursed expression child \
                 Primitive(String) (the leaf miss-predicate would mask a dropped child)"
            );
        }
        other => panic!("expected TypeExpr::TemplateLiteral, got {other:?}"),
    }

    // The miss-expr variant: the recursed child is the `Miss` sentinel — the
    // raised TemplateLiteral must still CARRY it (as the raised sentinel
    // `Unknown`), proving the child is recursed-and-preserved even though the leaf
    // predicate reads materialized.
    let template_miss = graph.intern_node(SemanticNodeData::TemplateLiteral {
        quasis: Arc::from(vec![Arc::<str>::from("x"), Arc::<str>::from("")].into_boxed_slice()),
        expressions: Arc::from(vec![miss].into_boxed_slice()),
    });
    let template_miss_oracle = raise_oracle(&host, template_miss).expect("template raises");
    match &template_miss_oracle {
        TypeExpr::TemplateLiteral { expressions, .. } => {
            assert_eq!(
                expressions.len(),
                1,
                "the raised TemplateLiteral must carry the (sentinel-bearing) recursed expression \
                 child, not drop it"
            );
            assert!(
                matches!(&expressions[0], TypeExpr::Unknown(_)),
                "the recursed miss child must raise to an Unknown sentinel inside the template's \
                 expressions; got {:?}",
                expressions[0]
            );
        }
        other => panic!("expected TypeExpr::TemplateLiteral, got {other:?}"),
    }

    // (2) TypeParam `T extends string = number`: the raised
    // `TypeExpr::TypeParameter` must PRESERVE both the recursed constraint
    // (`Primitive(String)`) and default (`Primitive(Number)`) payloads.
    let tp = graph.intern_node(SemanticNodeData::TypeParam {
        decl: decl_identity_unscoped("/w/tp.ts", "T"),
        param_index: 0,
        constraint: Some(string_id),
        default: Some(number_id),
        display_name: Arc::from("T"),
    });
    let tp_oracle = raise_oracle(&host, tp).expect("typeparam raises");
    match &tp_oracle {
        TypeExpr::TypeParameter(param) => {
            assert_eq!(
                param.constraint.as_deref(),
                Some(&TypeExpr::Primitive(PrimitiveName::String)),
                "the raised TypeParameter must preserve the recursed constraint Primitive(String)"
            );
            assert_eq!(
                param.default.as_deref(),
                Some(&TypeExpr::Primitive(PrimitiveName::Number)),
                "the raised TypeParameter must preserve the recursed default Primitive(Number)"
            );
        }
        other => panic!("expected TypeExpr::TypeParameter, got {other:?}"),
    }

    // (3) Carrier type-arg `Foo<number>` (BareRef): the raised `Ref` must PRESERVE
    // the recursed type-arg payload (`Primitive(Number)`) on `type_arguments`.
    let bare_arg = graph.intern_node(SemanticNodeData::new_bare_ref(
        Arc::from("Foo"),
        NodeScopeId::Global,
        Arc::from(vec![number_id].into_boxed_slice()),
    ));
    let bare_arg_oracle = raise_oracle(&host, bare_arg).expect("bare ref raises");
    match &bare_arg_oracle {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(
                name.as_ref(),
                "Foo",
                "the raised Ref keeps the carrier name"
            );
            assert_eq!(
                type_arguments.as_ref(),
                &[TypeExpr::Primitive(PrimitiveName::Number)],
                "the raised Ref must preserve the recursed type-arg payload Primitive(Number) (the \
                 leaf miss-predicate over a Ref would mask a dropped/altered arg)"
            );
        }
        other => panic!("expected TypeExpr::Ref, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Local fixture helpers.
// ---------------------------------------------------------------------------

/// Build an `Object` `SurfaceView` with the given `(name, value-node)`
/// properties (all public, non-method, non-optional).
fn object_surface(props: &[(&str, SemanticNodeId)]) -> SurfaceView {
    let members = props
        .iter()
        .map(|(name, value)| SurfaceMember {
            excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
            visibility: verter_type_expr::MemberVisibility::Public,
            name: Arc::from(*name),
            value: *value,
            optional: false,
            readonly: false,
            is_method: false,
            declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
            merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            spans: Default::default(),
            declaration_origin: None,
        })
        .collect::<Vec<_>>();
    crate::semantic_query::surface_view! {
        members: Arc::from(members.into_boxed_slice()),
        call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
        completeness: crate::semantic_query::MemberSurfaceCompleteness::Closed,
    }
}

/// A content-free `DeclIdentity` (whole_hash zeroed) for `TypeParam.decl` /
/// fixture decl refs — tests carry no real content hash.
fn decl_identity_unscoped(canonical_id: &str, name: &str) -> DeclIdentity {
    DeclIdentity {
        canonical_id: Arc::from(canonical_id),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: [0u8; 16],
        decl_name: Arc::from(name),
    }
}

// ---------------------------------------------------------------------------
// Union NO-collapse edges — the easy-to-get-wrong subtlety BOTH codex legs
// flagged: the raiser's `Union` arm NEVER collapses (a single-member
// `Union([A])` stays a union and an empty `Union([])` stays an empty union),
// and a PRESENT-but-unraisable member fails the WHOLE composite
// (presence-aware — never silently erased). The bottom-up fold must preserve
// this EXACTLY (a collapse would diverge from the materialize-then-predicate
// oracle). Each routes through `assert_classifier_parity` so it discriminates
// against the oracle, plus a DIRECT oracle shape assertion pinning the
// no-collapse.
// ---------------------------------------------------------------------------

#[test]
fn parity_union_no_collapse_single_and_empty() {
    let host = host();
    let graph = graph_of(&host);

    // `Union([A])` — a single-member union — stays a `Union`, NOT collapsed to A.
    let str_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let single = graph.intern_node(SemanticNodeData::Union(Arc::from(
        vec![str_id].into_boxed_slice(),
    )));
    assert_classifier_parity(&host, single, "union-single-member");
    match raise_oracle(&host, single) {
        Some(TypeExpr::Union(ref members)) => assert_eq!(
            members.len(),
            1,
            "Union([A]) must stay a single-member Union (NO collapse to A)"
        ),
        other => panic!("Union([String]) must raise to a Union, got {other:?}"),
    }
    // The node-domain equality agrees the single-member union equals itself
    // (its raised shape), proving the bottom-up key built the un-collapsed union.
    assert_eq!(
        raised_shape_eq_node_type_expr(
            &host,
            single,
            &TypeExpr::Union(Arc::from(
                vec![TypeExpr::Primitive(PrimitiveName::String)].into_boxed_slice()
            ))
        ),
        Some(true),
        "the bottom-up key of Union([String]) must equal Union([String]) (un-collapsed)"
    );
    // ...and is NOT equal to the bare member `String` (a collapse bug would make
    // these equal).
    assert_eq!(
        raised_shape_eq_node_type_expr(&host, single, &TypeExpr::Primitive(PrimitiveName::String)),
        Some(false),
        "Union([String]) must NOT equal the bare String (a collapse-to-single bug would make \
         these equal)"
    );

    // An empty `Union([])` stays an empty union (NO empty→sentinel).
    let empty = graph.intern_node(SemanticNodeData::Union(Arc::from(
        Vec::<SemanticNodeId>::new().into_boxed_slice(),
    )));
    assert_classifier_parity(&host, empty, "union-empty");
    match raise_oracle(&host, empty) {
        Some(TypeExpr::Union(ref members)) => assert!(
            members.is_empty(),
            "empty Union([]) must stay an empty Union (NO empty→sentinel)"
        ),
        other => panic!("empty Union([]) must raise to an empty Union, got {other:?}"),
    }

    // FAIL-CLOSED (R2-F1): a `Union([A, <absent>])` FAILS THE WHOLE raise —
    // a present-but-unraisable member is never silently erased.
    let absent = SemanticNodeId(u64::MAX);
    let with_absent = graph.intern_node(SemanticNodeData::Union(Arc::from(
        vec![str_id, absent].into_boxed_slice(),
    )));
    assert!(
        raise_oracle(&host, with_absent).is_none(),
        "Union([String, <absent>]) must FAIL the whole raise (presence-aware)"
    );
    assert!(
        !node_can_shell_raise(&host, with_absent),
        "the node-domain fold fails identically (shared fold)"
    );
}

// ---------------------------------------------------------------------------
// Type-level fence: the interned `RaisedShapeKey` does NOT own a `TypeExpr`.
// This FAILS on the former materialize-then-predicate tree (where
// `RaisedShapeKey(TypeExpr)` wrapped a full `TypeExpr`, so `size_of` was the
// large `TypeExpr` enum size) and PASSES on the interned-structural-key tree
// (a `u32` intern id). The interned key replaces `TypeExpr`'s `PartialEq` in
// the node-domain decision surface, so it must NOT own a materialised
// `TypeExpr` — this is the architecture-fence half of that property.
// ---------------------------------------------------------------------------

#[test]
fn raised_shape_key_is_an_interned_id_not_a_typeexpr() {
    use super::raise::RaisedShapeKey;

    // The interned key is a tiny id (a `u32` newtype), NOT a `TypeExpr`-owning
    // wrapper. `TypeExpr` is a large multi-variant enum (Arc-heavy); a key that
    // OWNED one would be at least pointer-sized and far larger than 4 bytes.
    assert_eq!(
        std::mem::size_of::<RaisedShapeKey>(),
        std::mem::size_of::<u32>(),
        "RaisedShapeKey must be an interned id (u32-sized), NOT own a TypeExpr — a \
         `RaisedShapeKey(TypeExpr)` would be `size_of::<TypeExpr>()` bytes ({} here), far larger \
         than {} (u32)",
        std::mem::size_of::<TypeExpr>(),
        std::mem::size_of::<u32>(),
    );
    // Belt-and-braces: the key is strictly smaller than a `TypeExpr` (the
    // former wrapper would be `>=` it).
    assert!(
        std::mem::size_of::<RaisedShapeKey>() < std::mem::size_of::<TypeExpr>(),
        "RaisedShapeKey ({} bytes) must be strictly smaller than TypeExpr ({} bytes) — it interns \
         an id, it does not own the materialised shape",
        std::mem::size_of::<RaisedShapeKey>(),
        std::mem::size_of::<TypeExpr>(),
    );
}

// ---------------------------------------------------------------------------
// Publication-scoring differential: the node-front publication score
// (`project_node_publication_score`) and the `&TypeExpr`-front score
// (`type_expr_publication_score`) must agree per-fact on `raise(node)`, and the
// node-domain improvement verdict (`compare_node_improvement`) must equal the
// `TypeExpr` verdict (`compare_type_expr_improvement`) over the raised shapes.
//
// These fixtures hit the classes where the FORMER hand-rolled node scorers
// diverged from the `TypeExpr` predicates (the divergence this fix closes):
// a `Mapped` whose `source == key_space` (the double-count), a `Mapped` over a
// free `TypeParam` (the missing-`TypeParam` arm), and an `Opaque(RecursiveRef)`
// root (the unknown-root + structural-misclassification). Because both fronts now
// feed the SAME shared per-arm rules and the node front rides the SAME
// `fold_node`, they agree by construction.
// ---------------------------------------------------------------------------

/// A `Mapped { source, mapper.key_space, value }` direct fixture. The fold reads
/// `mapper.key_space` (KeyOf-aware) as the source child and `value_expr` as the
/// value — NEVER the `source` field — so a fixture with `source == key_space`
/// (both `key_space`) exercises the former double-count.
fn mapped_fixture(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    source: SemanticNodeId,
    key_space: SemanticNodeId,
    value_expr: SemanticNodeId,
) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Mapped {
        source,
        mapper: MapperKey {
            parameter_node: source,
            key_space,
            value_expr,
            optionality: OptionalityMod::Keep,
            readonly: ReadonlyMod::Keep,
            name_remap: None,
            kind: MapperKind::Computed,
        },
    })
}

#[test]
fn node_improvement_verdict_matches_type_expr_improvement_over_raise() {
    let host = host();
    let graph = graph_of(&host);

    let foo = graph.intern_node(SemanticNodeData::DeclRef {
        identity: decl_identity_unscoped("/w/m.ts", "Foo"),
    });
    let bar = graph.intern_node(SemanticNodeData::DeclRef {
        identity: decl_identity_unscoped("/w/m.ts", "Bar"),
    });
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // `Foo<Bar>` — a generic instantiation, symbolic-carrier penalty 2, no divergence.
    let foo_of_bar = graph.intern_node(SemanticNodeData::InstantiationRef {
        base: decl_identity_unscoped("/w/m.ts", "Foo"),
        args: Arc::from(vec![bar].into_boxed_slice()),
    });

    // Mapped whose `source == key_space == Foo` — the former double-count fixture
    // (correct carriers = 2; the former node scorer counted 3).
    let mapped_double = mapped_fixture(&graph, foo, foo, string);

    // Mapped over a free `TypeParam` — the former missing-`TypeParam` fixture
    // (correct carriers = 3; the former node scorer counted 1).
    let tp = graph.intern_node(SemanticNodeData::TypeParam {
        decl: decl_identity_unscoped("/w/m.ts", "T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let v_tp = graph.intern_node(SemanticNodeData::TypeParam {
        decl: decl_identity_unscoped("/w/m.ts", "V"),
        param_index: 1,
        constraint: None,
        default: None,
        display_name: Arc::from("V"),
    });
    let keyof_tp = graph.intern_node(SemanticNodeData::KeyOf { base: tp });
    let mapped_tp = mapped_fixture(&graph, tp, keyof_tp, v_tp);

    // A Conditional with carrier penalty 3 (matches `mapped_tp`'s correct penalty).
    let cond = graph.intern_node(SemanticNodeData::Conditional {
        check: foo,
        extends: bar,
        true_branch_ref: string,
        false_branch_ref: string,
        distributive: false,
    });

    // `Opaque(RecursiveRef)` — raises to `TypeExpr::RecursiveRef`: a STRUCTURAL,
    // NON-unknown carrier (the former node scorer mis-read it as unknown-root and
    // non-structural).
    let rr = graph.intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
        name: Arc::from("Loop"),
    }));

    // (candidate, current) pairs. Each verdict is asserted equal to the verdict
    // the `TypeExpr` comparator returns on the RAISED shapes.
    let pairs: &[(SemanticNodeId, SemanticNodeId, &str)] = &[
        // Divergent classes (the former node scorers got these WRONG):
        (mapped_double, cond, "mapped-double-count-vs-conditional"),
        (foo_of_bar, mapped_tp, "generic-vs-mapped-typeparam"),
        (foo, rr, "ref-vs-recursive-ref-root"),
        (rr, foo, "recursive-ref-vs-ref-root"),
        // Controls (no divergence; pin a TRUE and a FALSE verdict):
        (string, foo, "primitive-beats-ref"),
        (foo, string, "ref-does-not-beat-primitive"),
    ];

    let mut saw_true = false;
    let mut saw_false = false;
    for (candidate, current, label) in pairs {
        let node_verdict =
            crate::meta_resolve::compare_node_improvement(&host, *candidate, *current);
        let cand_raise = raise_oracle(&host, *candidate).expect("candidate raises");
        let cur_raise = raise_oracle(&host, *current).expect("current raises");
        let expr_verdict =
            crate::meta_resolve::compare_type_expr_improvement(&cand_raise, &cur_raise);
        assert_eq!(
            node_verdict, expr_verdict,
            "[{label}] compare_node_improvement must equal \
             compare_type_expr_improvement(raise(candidate), raise(current)) \
             (cand_raise = {cand_raise:?}, cur_raise = {cur_raise:?})"
        );
        if expr_verdict {
            saw_true = true;
        } else {
            saw_false = true;
        }
    }
    assert!(
        saw_true && saw_false,
        "the differential must exercise BOTH a better and a not-better verdict"
    );
}

/// A corpus of DIRECT node fixtures covering EVERY `SemanticNodeData` variant that
/// raises to a `TypeExpr` shape, plus the classes where the FORMER hand-rolled
/// node scorers diverged (`Mapped` with `source == key_space`; `Mapped` over a
/// free `TypeParam`; a `TypeParam` with constraint+default; `Opaque(RecursiveRef)`
/// / `Opaque(Miss)`; `RawFallback` raw + sentinel; `BareRef` / `ImportType` with
/// type args). Each entry is `(label, node)`; the per-fact differential and the
/// coverage guard both route every fixture.
fn publication_score_corpus(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
) -> Vec<(&'static str, SemanticNodeId)> {
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let foo = graph.intern_node(SemanticNodeData::DeclRef {
        identity: decl_identity_unscoped("/w/m.ts", "Foo"),
    });
    let bar = graph.intern_node(SemanticNodeData::DeclRef {
        identity: decl_identity_unscoped("/w/m.ts", "Bar"),
    });
    let obj_a = graph.intern_node(SemanticNodeData::Object(object_surface(&[("a", string)])));
    let obj_b = graph.intern_node(SemanticNodeData::Object(object_surface(&[("b", number)])));

    let tp = graph.intern_node(SemanticNodeData::TypeParam {
        decl: decl_identity_unscoped("/w/m.ts", "T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let v_tp = graph.intern_node(SemanticNodeData::TypeParam {
        decl: decl_identity_unscoped("/w/m.ts", "V"),
        param_index: 1,
        constraint: None,
        default: None,
        display_name: Arc::from("V"),
    });
    let tp_bounded = graph.intern_node(SemanticNodeData::TypeParam {
        decl: decl_identity_unscoped("/w/m.ts", "B"),
        param_index: 2,
        constraint: Some(foo),
        default: Some(bar),
        display_name: Arc::from("B"),
    });
    let keyof_tp = graph.intern_node(SemanticNodeData::KeyOf { base: tp });

    let function = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from(
            vec![FunctionParam::synthetic(
                Some(Arc::from("a")),
                string,
                false,
                false,
            )]
            .into_boxed_slice(),
        ),
        return_type: number,
        type_parameters: Arc::from(Vec::<TypeParamDecl>::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    });

    vec![
        ("primitive", string),
        (
            "literal",
            graph.intern_node(SemanticNodeData::Literal(
                crate::semantic_query::LiteralValue::String("idle".to_string()),
            )),
        ),
        ("alias", graph.intern_node(SemanticNodeData::Alias(foo))),
        (
            "union",
            graph.intern_node(SemanticNodeData::Union(Arc::from(
                vec![string, number].into_boxed_slice(),
            ))),
        ),
        (
            "intersection",
            graph.intern_node(SemanticNodeData::Intersection(Arc::from(
                vec![foo, bar].into_boxed_slice(),
            ))),
        ),
        (
            "array",
            graph.intern_node(SemanticNodeData::Array {
                element: string,
                readonly: false,
            }),
        ),
        (
            "tuple",
            graph.intern_node(SemanticNodeData::Tuple {
                elements: Arc::from(
                    vec![crate::semantic_query::TupleElement {
                        label: None,
                        value: foo,
                        optional: false,
                        rest: false,
                    }]
                    .into_boxed_slice(),
                ),
                readonly: false,
            }),
        ),
        ("object", obj_a),
        (
            "merged_decl",
            graph.intern_node(SemanticNodeData::MergedDecl {
                contributors: Arc::from(vec![obj_a, obj_b].into_boxed_slice()),
            }),
        ),
        (
            "conditional",
            graph.intern_node(SemanticNodeData::Conditional {
                check: foo,
                extends: bar,
                true_branch_ref: string,
                false_branch_ref: number,
                distributive: false,
            }),
        ),
        (
            "template_literal",
            graph.intern_node(SemanticNodeData::TemplateLiteral {
                quasis: Arc::from(
                    vec![Arc::<str>::from("a"), Arc::<str>::from("")].into_boxed_slice(),
                ),
                expressions: Arc::from(vec![foo].into_boxed_slice()),
            }),
        ),
        ("key_of", keyof_tp),
        (
            "indexed_access",
            graph.intern_node(SemanticNodeData::IndexedAccess {
                object: foo,
                index: IndexKey::String(Arc::from("a")),
            }),
        ),
        // Mapped, `source == key_space == Foo` — the former double-count fixture.
        ("mapped_double", mapped_fixture(graph, foo, foo, string)),
        // Mapped over a free `TypeParam` — the former missing-`TypeParam` fixture.
        (
            "mapped_typeparam",
            mapped_fixture(graph, tp, keyof_tp, v_tp),
        ),
        (
            "typeof",
            graph.intern_node(SemanticNodeData::new_typeof(
                ValueRootKey {
                    scope: ScopeId {
                        canonical_id: Arc::from("/m.ts"),
                        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        local_scope: None,
                        binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
                            verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        ),
                    },
                    name: Arc::from("factory"),
                },
                Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
                Arc::from(Vec::new().into_boxed_slice()),
            )),
        ),
        ("type_param", tp),
        ("type_param_bounded", tp_bounded),
        (
            "infer",
            graph.intern_node(SemanticNodeData::Infer {
                name: Arc::from("U"),
                binder: graph.alloc_infer_binder_id(),
            }),
        ),
        (
            "opaque_recursive_ref",
            graph.intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
                name: Arc::from("Loop"),
            })),
        ),
        (
            "opaque_miss",
            graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        ),
        (
            "opaque_surface_sentinel",
            graph.intern_node(SemanticNodeData::Opaque(QueryError::UnrepresentableSurface)),
        ),
        ("function", function),
        ("constructor_type", {
            graph.intern_construct_twin_for_tests(function)
        }),
        ("decl_ref", foo),
        (
            "instantiation_ref",
            graph.intern_node(SemanticNodeData::InstantiationRef {
                base: decl_identity_unscoped("/w/m.ts", "Foo"),
                args: Arc::from(vec![bar].into_boxed_slice()),
            }),
        ),
        (
            "bare_ref",
            graph.intern_node(SemanticNodeData::new_bare_ref(
                Arc::from("Bare"),
                NodeScopeId::Global,
                Arc::from(Vec::new().into_boxed_slice()),
            )),
        ),
        (
            "bare_ref_with_args",
            graph.intern_node(SemanticNodeData::new_bare_ref(
                Arc::from("Bare"),
                NodeScopeId::Global,
                Arc::from(vec![foo, string].into_boxed_slice()),
            )),
        ),
        (
            "import_type",
            graph.intern_node(SemanticNodeData::new_import_type(
                Arc::from("./m"),
                Arc::from(vec![Arc::<str>::from("A")].into_boxed_slice()),
                Arc::from(Vec::new().into_boxed_slice()),
                false,
            )),
        ),
        (
            "import_type_with_args",
            graph.intern_node(SemanticNodeData::new_import_type(
                Arc::from("./m"),
                Arc::from(vec![Arc::<str>::from("A")].into_boxed_slice()),
                Arc::from(vec![foo].into_boxed_slice()),
                false,
            )),
        ),
        (
            "raw_fallback",
            graph.intern_node(SemanticNodeData::RawFallback {
                value: verter_type_expr::UnknownValue::unsupported_syntax("SomeRawText"),
            }),
        ),
        (
            "raw_fallback_sentinel_spelling",
            graph.intern_node(SemanticNodeData::RawFallback {
                value: verter_type_expr::UnknownValue::unsupported_syntax("semanticAliasCycle"),
            }),
        ),
        (
            "synthetic_binding",
            graph.intern_node(SemanticNodeData::SyntheticBinding {
                id: crate::semantic_query::SyntheticBindingId {
                    scope_canonical_id: Arc::from("/Comp.vue"),
                    surface_kind: verter_type_expr::SyntheticCarrierSurfaceKind::SlotBinding,
                    slot_name: Some(Arc::from("default")),
                    binding_name: Arc::from("row"),
                },
                value_node: foo.0,
            }),
        ),
    ]
}

/// NON-VACUOUS PER-FACT DIFFERENTIAL: for EVERY corpus fixture, the node-front
/// publication score (`project_node_publication_score`) must equal the
/// `&TypeExpr`-front score (`type_expr_publication_score`) applied to `raise(node)`
/// — field-for-field (`symbolic_carriers`, `generic_detail`, `structural_top_level`,
/// `exact_unknown_root`). This is what locks the two fronts: the former
/// hand-rolled node scorers diverged here (the `Mapped` double-count, the missing
/// `TypeParam` / `RawFallback` / `SyntheticBinding` arms, the
/// `Opaque(RecursiveRef)` misclassification); riding the SAME `fold_node` makes
/// them agree by construction.
#[test]
fn node_publication_score_matches_type_expr_score_per_fact() {
    let host = host();
    let graph = graph_of(&host);
    let dispatch = ProjectSemanticDispatch::new(&host);

    for (label, node) in publication_score_corpus(&graph) {
        let node_score = project_node_publication_score_with_dispatch(&dispatch, node);
        let expr_score: Option<PublicationScore> =
            raise_oracle(&host, node).map(|e| type_expr_publication_score(&e));
        assert_eq!(
            node_score,
            expr_score,
            "[{label}] node publication score must equal \
             type_expr_publication_score(raise(node)) per-fact (raise = {:?})",
            raise_oracle(&host, node)
        );
    }
}

/// COVERAGE GUARD: every `SemanticNodeData` variant must be exercised by the
/// publication-score corpus. The exhaustive `variant_label` match (NO wildcard)
/// fails to COMPILE if a new variant is added without classification; the runtime
/// assertion below fails if a variant is added to `variant_label` but NOT given a
/// corpus fixture — so a new variant cannot silently bypass the per-fact
/// differential.
#[test]
fn publication_score_corpus_covers_every_semantic_node_data_variant() {
    use rustc_hash::FxHashSet;

    /// Exhaustive label for a `SemanticNodeData` discriminant — NO wildcard, so a
    /// new variant forces a new arm here (the compile-time coverage tripwire).
    fn variant_label(data: &SemanticNodeData) -> &'static str {
        match data {
            SemanticNodeData::Primitive(_) => "primitive",
            SemanticNodeData::Literal(_) => "literal",
            SemanticNodeData::Alias(_) => "alias",
            SemanticNodeData::Union(_) => "union",
            SemanticNodeData::Intersection(_) => "intersection",
            SemanticNodeData::Array { .. } => "array",
            SemanticNodeData::Tuple { .. } => "tuple",
            SemanticNodeData::Object(_) => "object",
            SemanticNodeData::MergedDecl { .. } => "merged_decl",
            SemanticNodeData::Conditional { .. } => "conditional",
            SemanticNodeData::TemplateLiteral { .. } => "template_literal",
            SemanticNodeData::KeyOf { .. } => "key_of",
            SemanticNodeData::IndexedAccess { .. } => "indexed_access",
            SemanticNodeData::Mapped { .. } => "mapped",
            SemanticNodeData::TypeOf(_) => "typeof",
            SemanticNodeData::TypeParam { .. } => "type_param",
            SemanticNodeData::Infer { .. } => "infer",
            SemanticNodeData::InferRef { .. } => "infer-ref",
            SemanticNodeData::Opaque(_) => "opaque",
            SemanticNodeData::Signature {
                kind: crate::semantic_query::SignatureKind::Construct,
                ..
            } => "constructor_type",
            SemanticNodeData::Signature { .. } => "function",
            SemanticNodeData::DeclRef { .. } => "decl_ref",
            SemanticNodeData::InstantiationRef { .. } => "instantiation_ref",
            SemanticNodeData::BareRef(_) => "bare_ref",
            SemanticNodeData::ImportType(_) => "import_type",
            SemanticNodeData::RawFallback { .. } => "raw_fallback",
            SemanticNodeData::SyntheticBinding { .. } => "synthetic_binding",
        }
    }

    // The complete discriminant set the corpus must cover (the distinct values
    // `variant_label` can return).
    const EXPECTED: &[&str] = &[
        "primitive",
        "literal",
        "alias",
        "union",
        "intersection",
        "array",
        "tuple",
        "object",
        "merged_decl",
        "conditional",
        "template_literal",
        "key_of",
        "indexed_access",
        "mapped",
        "typeof",
        "type_param",
        "infer",
        "opaque",
        "function",
        "decl_ref",
        "instantiation_ref",
        "bare_ref",
        "import_type",
        "raw_fallback",
        "constructor_type",
        "synthetic_binding",
    ];

    let host = host();
    let graph = graph_of(&host);

    let mut covered: FxHashSet<&'static str> = FxHashSet::default();
    for (_label, node) in publication_score_corpus(&graph) {
        let data = graph
            .node_data(node)
            .expect("a corpus fixture node must exist in the graph");
        covered.insert(variant_label(&data));
        // Every corpus fixture must score (it raises to a real shape).
        let dispatch = ProjectSemanticDispatch::new(&host);
        assert!(
            project_node_publication_score_with_dispatch(&dispatch, node).is_some(),
            "every corpus fixture must yield a publication score"
        );
    }

    let expected: FxHashSet<&'static str> = EXPECTED.iter().copied().collect();
    assert_eq!(
        covered,
        expected,
        "the publication-score corpus must cover EXACTLY every SemanticNodeData discriminant — a \
         new variant added to `variant_label` without a corpus fixture (or an extra label) fails \
         here. Missing: {:?}; Extra: {:?}",
        expected.difference(&covered).collect::<Vec<_>>(),
        covered.difference(&expected).collect::<Vec<_>>(),
    );
}

/// F3-1 regression: the intersection normalization drops a typed
/// `UnrepresentableSurface` arm — but the arm's TYPED degradation must
/// survive into the folded payload (fail-closed: a normalized-away
/// degradation must never read as a complete result).
#[test]
fn intersection_arm_drop_keeps_typed_degradation_in_sidecar() {
    let host = host();
    let graph = graph_of(&host);

    let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let real_obj = graph.intern_node(SemanticNodeData::Object(object_surface(&[(
        "a", string_id,
    )])));
    let typed_surface =
        graph.intern_node(SemanticNodeData::Opaque(QueryError::UnrepresentableSurface));
    let inter = graph.intern_node(SemanticNodeData::Intersection(Arc::from(
        vec![typed_surface, real_obj].into_boxed_slice(),
    )));

    let dispatch = ProjectSemanticDispatch::new(&host);
    let mut active = rustc_hash::FxHashSet::default();
    let folded = super::raise::fold_to_type_expr(&dispatch, inter, &mut active)
        .expect("the intersection raises");
    assert!(
        folded.has_degradation(),
        "the dropped sentinel arm's typed degradation must survive the normalization"
    );
    // The compat tree still collapses to the real object (bytes preserved).
    assert!(
        matches!(folded.expr(), TypeExpr::Object(_)),
        "the collapsed tree is the real object, got {:?}",
        folded.expr()
    );
    // … and the surviving degradation is NOT root-anchored (a nested path —
    // the collapsed result must never read as a fresh root sentinel, so an
    // OUTER intersection does not re-drop it).
    assert!(
        folded.root_degradation().is_none(),
        "the absorbed degradation must not re-anchor at the root"
    );
}

/// F3-2 regression: an invalid call signature (raises to a degraded
/// non-function, so it cannot become a `CallSignature` member) is dropped
/// from the object — its TYPED degradation must survive into the folded
/// payload (fail-closed, never silently complete).
#[test]
fn invalid_call_signature_drop_keeps_typed_degradation_in_sidecar() {
    let host = host();
    let graph = graph_of(&host);

    let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let miss = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let mut surface = object_surface(&[("a", string_id)]);
    surface.call_signatures = Arc::from(vec![miss].into_boxed_slice());
    let obj = graph.intern_node(SemanticNodeData::Object(surface));

    let dispatch = ProjectSemanticDispatch::new(&host);
    let mut active = rustc_hash::FxHashSet::default();
    let folded =
        super::raise::fold_to_type_expr(&dispatch, obj, &mut active).expect("the object raises");
    assert!(
        folded.has_degradation(),
        "the dropped call signature's typed degradation must survive"
    );
    let TypeExpr::Object(object) = folded.expr() else {
        panic!("expected an object, got {:?}", folded.expr());
    };
    assert_eq!(
        object.properties.len(),
        1,
        "only the real property survives (the invalid signature is dropped)"
    );
}

#[test]
fn open_spread_surface_raises_positive_members_and_typed_operands() {
    let host = host();
    let graph = graph_of(&host);
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let array = graph.intern_node(SemanticNodeData::Array {
        element: number,
        readonly: false,
    });
    let mut surface = object_surface(&[("a", number), ("x", number)]);
    let completeness =
        MemberSurfaceCompleteness::OpenSpread(OpenSpreadOperands::new(Arc::from([array])));
    surface.replace_completeness(completeness);
    let node = graph.intern_node(SemanticNodeData::Object(surface));
    let dispatch = ProjectSemanticDispatch::new(&host);
    let mut active = rustc_hash::FxHashSet::default();
    let folded =
        super::raise::fold_to_type_expr(&dispatch, node, &mut active).expect("object raises");
    let TypeExpr::Object(object) = folded.expr() else {
        panic!("expected an object, got {:?}", folded.expr());
    };
    assert!(matches!(
        object.properties.as_slice(),
        [
            verter_type_expr::ObjectMember::Spread(spread),
            verter_type_expr::ObjectMember::Property(a),
            verter_type_expr::ObjectMember::Property(x),
        ] if a.name == "a"
            && x.name == "x"
            && matches!(spread.ty, TypeExpr::Array { .. })
    ));
    assert_classifier_parity(&host, node, "open-spread");
}

#[test]
fn open_spread_surface_raises_sole_positive_member_state_once() {
    let host = host();
    let graph = graph_of(&host);
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let array = graph.intern_node(SemanticNodeData::Array {
        element: number,
        readonly: false,
    });
    let mut surface = object_surface(&[("a", number)]);
    let mut optional = surface.positive_members()[0].clone();
    optional.optional = true;
    surface = surface.with_positive_members(Arc::from([optional.clone()]));
    surface.replace_completeness(MemberSurfaceCompleteness::OpenSpread(
        OpenSpreadOperands::new(Arc::from([array])),
    ));
    let node = graph.intern_node(SemanticNodeData::Object(surface));
    let dispatch = ProjectSemanticDispatch::new(&host);
    let mut active = rustc_hash::FxHashSet::default();
    let folded =
        super::raise::fold_to_type_expr(&dispatch, node, &mut active).expect("object raises");
    let TypeExpr::Object(object) = folded.expr() else {
        panic!("expected an object, got {:?}", folded.expr());
    };
    assert!(matches!(
        object.properties.as_slice(),
        [
            verter_type_expr::ObjectMember::Spread(array),
            verter_type_expr::ObjectMember::Property(property),
        ] if matches!(array.ty, TypeExpr::Array { .. })
            && property.name == "a"
            && property.optional
    ));
}

// ---------------------------------------------------------------------------
// R2-F1 — presence-aware fold: a PRESENT-but-unraisable child fails the
// WHOLE composite (never silently erased); mandatory-output fallbacks are the
// TYPED `UnrepresentableSurfaceMember` (never a complete placeholder).
// ---------------------------------------------------------------------------

fn fold_raises(host: &VerterHost, node: SemanticNodeId) -> bool {
    let dispatch = ProjectSemanticDispatch::new(host);
    let mut active = rustc_hash::FxHashSet::default();
    super::raise::fold_to_type_expr(&dispatch, node, &mut active).is_some()
}

#[test]
fn union_with_unraisable_member_fails_whole() {
    let host = host();
    let graph = graph_of(&host);
    let str_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let node = graph.intern_node(SemanticNodeData::Union(Arc::from(
        vec![str_id, SemanticNodeId(u64::MAX)].into_boxed_slice(),
    )));
    assert!(
        !fold_raises(&host, node),
        "Union([String, <absent>]) must fail whole"
    );
}

#[test]
fn intersection_with_unraisable_arm_fails_whole() {
    let host = host();
    let graph = graph_of(&host);
    let str_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let node = graph.intern_node(SemanticNodeData::Intersection(Arc::from(
        vec![str_id, SemanticNodeId(u64::MAX)].into_boxed_slice(),
    )));
    assert!(
        !fold_raises(&host, node),
        "Intersection([String, <absent>]) must fail whole"
    );
}

#[test]
fn tuple_with_unraisable_element_fails_whole() {
    let host = host();
    let graph = graph_of(&host);
    let str_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let node = graph.intern_node(SemanticNodeData::Tuple {
        elements: Arc::from(
            vec![
                crate::semantic_query::TupleElement {
                    label: None,
                    value: str_id,
                    optional: false,
                    rest: false,
                },
                crate::semantic_query::TupleElement {
                    label: None,
                    value: SemanticNodeId(u64::MAX),
                    optional: false,
                    rest: false,
                },
            ]
            .into_boxed_slice(),
        ),
        readonly: false,
    });
    assert!(
        !fold_raises(&host, node),
        "Tuple([String, <absent>]) must fail whole"
    );
}

#[test]
fn template_literal_with_unraisable_expression_fails_whole() {
    let host = host();
    let graph = graph_of(&host);
    let node = graph.intern_node(SemanticNodeData::TemplateLiteral {
        quasis: Arc::from(vec![Arc::from("x")].into_boxed_slice()),
        expressions: Arc::from(vec![SemanticNodeId(u64::MAX)].into_boxed_slice()),
    });
    assert!(
        !fold_raises(&host, node),
        "TemplateLiteral with an unraisable expression must fail whole"
    );
}

#[test]
fn function_with_unraisable_return_fails_whole() {
    let host = host();
    let graph = graph_of(&host);
    let node = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from(Vec::<FunctionParam>::new().into_boxed_slice()),
        return_type: SemanticNodeId(u64::MAX),
        type_parameters: Arc::from(Vec::<TypeParamDecl>::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    });
    assert!(
        !fold_raises(&host, node),
        "a function whose REQUIRED return is unraisable must fail whole"
    );
}

#[test]
fn function_with_unraisable_parameter_fails_whole() {
    let host = host();
    let graph = graph_of(&host);
    let str_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let node = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from(
            vec![FunctionParam::synthetic(
                Some(Arc::from("a")),
                SemanticNodeId(u64::MAX),
                false,
                false,
            )]
            .into_boxed_slice(),
        ),
        return_type: str_id,
        type_parameters: Arc::from(Vec::<TypeParamDecl>::new().into_boxed_slice()),
        signature_span: None,
        return_type_span: None,
    });
    assert!(
        !fold_raises(&host, node),
        "a function with an unraisable parameter must fail whole"
    );
}

#[test]
fn type_parameter_with_unraisable_constraint_or_default_fails_whole() {
    let host = host();
    let graph = graph_of(&host);
    let decl = decl_identity_unscoped("/w/tp.ts", "T");
    let with_constraint = graph.intern_node(SemanticNodeData::TypeParam {
        decl: decl.clone(),
        param_index: 0,
        constraint: Some(SemanticNodeId(u64::MAX)),
        default: None,
        display_name: Arc::from("T"),
    });
    assert!(
        !fold_raises(&host, with_constraint),
        "a PRESENT-but-unraisable constraint must fail whole (None ≠ absent)"
    );
    let with_default = graph.intern_node(SemanticNodeData::TypeParam {
        decl,
        param_index: 0,
        constraint: None,
        default: Some(SemanticNodeId(u64::MAX)),
        display_name: Arc::from("T"),
    });
    assert!(
        !fold_raises(&host, with_default),
        "a PRESENT-but-unraisable default must fail whole (None ≠ absent)"
    );
    // … and a genuinely-absent slot still raises (None is preserved).
    let plain = graph.intern_node(SemanticNodeData::TypeParam {
        decl: decl_identity_unscoped("/w/tp.ts", "T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    assert!(fold_raises(&host, plain), "absent slots stay absent");
}

#[test]
fn carrier_arg_fallback_is_typed_surface_member_and_marks_partial() {
    use crate::project_semantic_dispatch::raise::{MaterializedOutputTypeExpr, OutputTypeExpr};
    use crate::semantic_query::DepSignature;

    let host = host();
    let graph = graph_of(&host);
    let node = graph.intern_node(SemanticNodeData::InstantiationRef {
        base: decl_identity_unscoped("/w/r.ts", "Box"),
        args: Arc::from(vec![SemanticNodeId(u64::MAX)].into_boxed_slice()),
    });
    let dispatch = ProjectSemanticDispatch::new(&host);
    let mut active = rustc_hash::FxHashSet::default();
    let folded = super::raise::fold_to_type_expr(&dispatch, node, &mut active)
        .expect("the carrier still raises (degraded arg)");
    assert!(
        folded.has_degradation(),
        "an unraisable carrier arg must degrade (typed UnrepresentableSurfaceMember)"
    );
    let TypeExpr::Ref { type_arguments, .. } = folded.expr() else {
        panic!("expected Ref, got {:?}", folded.expr());
    };
    assert!(
        matches!(
            type_arguments.first(),
            Some(TypeExpr::Unknown(v)) if v.raw() == "semanticSurfaceMember"
        ),
        "the fallback arg is the typed surface-member projection, got {:?}",
        folded.expr()
    );
    // … and the payload goes PARTIAL at the choke point (no warm admission).
    let carrier = MaterializedOutputTypeExpr::from_parts(
        Some(node),
        OutputTypeExpr::from_raise(folded),
        DepSignature::default(),
        false,
    );
    assert!(
        carrier.result_is_partial(),
        "a degraded carrier arg must mark the payload partial"
    );
}

/// R3-F1 — terminal laundering: a PRESENT-BUT-UNRAISABLE composite reaches
/// the reduce-then-raise terminal as a TYPED unmaterialized failure (partial,
/// never admitted complete); a GENUINE absence stays exact + non-partial.
#[test]
fn terminal_marks_unraisable_composite_partial_and_genuine_absence_exact() {
    use crate::project_semantic_dispatch::output_materialization::{
        wrap_output_type_expr, TestOutputCap,
    };
    use crate::project_semantic_dispatch::raise::{MaterializedTypeExpr, OutputTypeExpr};
    use crate::semantic_query::{DepSignature, ProjectionMode, ProjectionReductionContext};

    let host = host();
    let graph = graph_of(&host);
    let str_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // (1) A present-but-unraisable composite: reduce succeeds (the union node
    // exists), the raise FAILS on the absent child — the terminal payload must
    // be PARTIAL (typed unmaterialized failure), never admitted complete.
    let union_absent = graph.intern_node(SemanticNodeData::Union(Arc::from(
        vec![str_id, SemanticNodeId(u64::MAX)].into_boxed_slice(),
    )));
    let dispatch = ProjectSemanticDispatch::new(&host);
    let reduced = dispatch.raise_and_reduce_with_context(
        union_absent,
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    );
    assert!(
        reduced.result_is_partial(),
        "a present-but-unraisable composite must reach the terminal as PARTIAL"
    );
    // (2) A GENUINE absence (a lane source with no typed payload) stays the
    // exact `missing_output` — NON-partial.
    let cap2 = TestOutputCap::new(&dispatch);
    let sealed = wrap_output_type_expr(
        &cap2,
        TypeExpr::Unknown(verter_type_expr::UnknownValue::missing_output()),
    );
    let genuine = crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr::from_parts(
        None,
        sealed,
        DepSignature::default(),
        false,
    );
    assert!(
        !genuine.result_is_partial(),
        "a genuine absence stays exact + non-partial"
    );

    // (3) The sealed-carrier None fallback degrades TYPED (partial), not an
    // exact empty Unknown.
    let degraded = MaterializedTypeExpr::degraded(QueryError::Miss);
    assert!(degraded.has_degradation());
    let carrier = crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr::from_parts(
        None,
        OutputTypeExpr::from_raise(degraded),
        DepSignature::default(),
        false,
    );
    assert!(
        carrier.result_is_partial(),
        "the terminal None fallback is a typed unmaterialized failure"
    );
}

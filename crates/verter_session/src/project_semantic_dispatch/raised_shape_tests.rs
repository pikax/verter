//! Parity tests for the node-domain raised-shape classifiers + equality
//! primitive (owner-local in [`super::raise`]).
//!
//! The ORACLE is the test-only shell-raise mirror of the Kind-B bridge,
//! [`ProjectSemanticDispatch::materialize_output_type_expr_for_test`] — the
//! exact `raise_node_to_type_expr` the bridge runs. For every fixture node the
//! tests assert each node-domain classifier EQUALS the legacy `TypeExpr`
//! predicate applied to the raised oracle output:
//!
//! - `node_contains_semantic_miss_legacy_equivalent(node) == match raise(node)
//!   { Some(e) => type_expr_contains_semantic_miss(&e), None => true }`
//! - `node_is_expanded_surface_legacy_equivalent(node) == match raise(node)
//!   { Some(e) => type_expr_is_expanded_surface(&e), None => false }`
//! - `node_can_shell_raise(node) == raise(node).is_some()`
//! - `raised_shape_eq_node_type_expr(node, &e) == Some(raise(node) == Some(e))`
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
    node_can_shell_raise, node_contains_semantic_miss_legacy_equivalent,
    node_is_expanded_surface_legacy_equivalent, raised_shape_eq_node_type_expr,
    raised_shape_eq_nodes,
};
use super::ProjectSemanticDispatch;
use crate::resolver_core::component_meta_query_engine::{
    type_expr_contains_semantic_miss, type_expr_is_expanded_surface,
};
use crate::resolver_core::shallow_file_state::{BudgetDomain, BudgetExceededFailure};
use crate::semantic_query::{
    DeclIdentity, FunctionParam, IndexKey, IndexSignature, MapperKey, MapperKind, NodeScopeId,
    OptionalityMod, PrimitiveKind, QueryError, ReadonlyMod, ScopeId, SemanticNodeData,
    SemanticNodeId, SurfaceMember, SurfaceView, TypeParamDecl, ValueRootKey,
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

    let expect_miss = match &oracle {
        Some(e) => type_expr_contains_semantic_miss(e),
        None => true,
    };
    assert_eq!(
        node_contains_semantic_miss_legacy_equivalent(host, node),
        expect_miss,
        "[{label}] node_contains_semantic_miss must equal \
         type_expr_contains_semantic_miss(raise(node)) (oracle = {oracle:?})"
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
// Class 1 — opaque errors (incl. sentinel-like `Other`), raw fallback,
// VueMacroElements.
// ---------------------------------------------------------------------------

#[test]
fn parity_opaque_errors_and_raw_fallback_and_vue_macro_elements() {
    let host = host();
    let graph = graph_of(&host);

    // `Opaque(Miss)` raises to the `SEMANTIC_MISS` sentinel ⇒ NOT materialized.
    let miss = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    assert_classifier_parity(&host, miss, "opaque-miss");
    assert!(
        node_contains_semantic_miss_legacy_equivalent(&host, miss),
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
        !node_contains_semantic_miss_legacy_equivalent(&host, other),
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
        node_contains_semantic_miss_legacy_equivalent(&host, budget),
        "Opaque(BudgetExceeded) raises to the budget prefix sentinel ⇒ semantic miss"
    );

    // `Opaque(RecursiveRef)` raises to `TypeExpr::RecursiveRef` ⇒ MATERIALIZED
    // (a materialized leaf, not a sentinel).
    let recursive = graph.intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
        name: Arc::from("Tree"),
    }));
    assert_classifier_parity(&host, recursive, "opaque-recursive-ref");
    assert!(
        !node_contains_semantic_miss_legacy_equivalent(&host, recursive),
        "Opaque(RecursiveRef) raises to RecursiveRef ⇒ materialized, NOT a miss"
    );

    // `Opaque(DeclPlaceholder)` raises to a `Ref` shell ⇒ MATERIALIZED.
    let placeholder = graph.intern_node(SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
        name: Arc::from("Pending"),
        canonical_id: Arc::from("/w/p.ts"),
        whole_hash: [0u8; 16],
    }));
    assert_classifier_parity(&host, placeholder, "opaque-decl-placeholder");

    // Raw-fallback carrier — a non-sentinel raw ⇒ MATERIALIZED.
    let raw = graph.intern_node(SemanticNodeData::RawFallback {
        raw: Arc::from("Weird<& Type>"),
    });
    assert_classifier_parity(&host, raw, "raw-fallback-nonsentinel");
    assert!(
        !node_contains_semantic_miss_legacy_equivalent(&host, raw),
        "RawFallback(non-sentinel text) ⇒ materialized"
    );

    // Raw-fallback carrier whose text IS a sentinel string ⇒ NOT materialized
    // (proves the classifier reads the raised raw, not the carrier kind).
    let raw_sentinel = graph.intern_node(SemanticNodeData::RawFallback {
        raw: Arc::from("semanticObjectSurface"),
    });
    assert_classifier_parity(&host, raw_sentinel, "raw-fallback-sentinel-text");
    assert!(
        node_contains_semantic_miss_legacy_equivalent(&host, raw_sentinel),
        "RawFallback whose raw IS a sentinel string ⇒ semantic miss"
    );

    // VueMacroElements raises to the `VueMacroElements` sentinel ⇒ NOT
    // materialized. DIRECT fixture (the real `SemanticNodeData::VueMacroElements`
    // match arm, not a RawFallback mirror): the arm ignores the payload, so an
    // empty `ResolvedElements::default()` suffices to exercise it.
    let vue = graph.intern_node(SemanticNodeData::VueMacroElements(Arc::new(
        verter_compiler::utils::oxc::script::type_surface::ResolvedElements::default(),
    )));
    assert_classifier_parity(&host, vue, "vue-macro-elements");
    assert!(
        node_contains_semantic_miss_legacy_equivalent(&host, vue),
        "VueMacroElements raises to the `VueMacroElements` sentinel ⇒ semantic miss"
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
    // Here we pin the CLASSIFICATION of the alias-cycle sentinel STRING via a
    // `RawFallback` mirror (the identical `type_expr_contains_semantic_miss`
    // branch the raised cycle sentinel would take): the spelling
    // `semanticAliasCycle` IS in the sentinel set ⇒ semantic miss.
    let alias_cycle_sentinel = graph.intern_node(SemanticNodeData::RawFallback {
        raw: Arc::from("semanticAliasCycle"),
    });
    assert_classifier_parity(&host, alias_cycle_sentinel, "alias-cycle-sentinel-mirror");
    assert!(
        node_contains_semantic_miss_legacy_equivalent(&host, alias_cycle_sentinel),
        "the alias-cycle sentinel string ⇒ semantic miss"
    );

    // `semanticTypeParamCycle` is NOT in the sentinel set ⇒ MATERIALIZED
    // (the raiser emits it for a TypeParam constraint/default cycle; it must read
    // as materialized, unlike the alias-cycle sentinel). Same constructibility
    // limitation as the alias self-cycle, so mirror the spelling via
    // `RawFallback` to pin its NON-sentinel classification.
    let tp_cycle = graph.intern_node(SemanticNodeData::RawFallback {
        raw: Arc::from("semanticTypeParamCycle"),
    });
    assert_classifier_parity(&host, tp_cycle, "typeparam-cycle-materialized-mirror");
    assert!(
        !node_contains_semantic_miss_legacy_equivalent(&host, tp_cycle),
        "the type-param-cycle string is NOT a sentinel ⇒ materialized"
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
        !node_contains_semantic_miss_legacy_equivalent(&host, merged),
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
            whole_hash: [0u8; 16],
            decl_name: Arc::from("Box"),
        },
        args: Arc::from(vec![arg].into_boxed_slice()),
    });
    assert_classifier_parity(&host, instref, "instantiation-ref");

    // A carrier whose type-arg points at an ABSENT node id ⇒ that arg raises to
    // `None` and the carrier arm materialises it as `Unknown { raw: "<raise
    // miss>" }` so the OUTER `Ref` still constructs (the carrier does NOT fail
    // the whole raise). `<raise miss>` is NOT in the sentinel set ⇒ the carrier
    // reads as MATERIALIZED — pinning the `.unwrap_or(Unknown { "<raise miss>" })`
    // arm parity.
    let instref_miss_arg = graph.intern_node(SemanticNodeData::InstantiationRef {
        base: DeclIdentity {
            canonical_id: Arc::from("/w/d.ts"),
            whole_hash: [0u8; 16],
            decl_name: Arc::from("Box"),
        },
        args: Arc::from(vec![SemanticNodeId(u64::MAX)].into_boxed_slice()),
    });
    assert_classifier_parity(&host, instref_miss_arg, "instantiation-ref-raise-miss-arg");
    assert!(
        node_can_shell_raise(&host, instref_miss_arg),
        "a carrier with an absent type-arg still raises (the arg becomes the <raise miss> \
         placeholder; the outer Ref constructs)"
    );
    assert!(
        !node_contains_semantic_miss_legacy_equivalent(&host, instref_miss_arg),
        "the `<raise miss>` placeholder is NOT in the sentinel set ⇒ the carrier reads materialized"
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
                local_scope: None,
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
        !node_contains_semantic_miss_legacy_equivalent(&host, typeof_node),
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
    let empty = graph.intern_node(SemanticNodeData::Object(SurfaceView {
        members: Arc::from(Vec::<SurfaceMember>::new().into_boxed_slice()),
        call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    }));
    assert_classifier_parity(&host, empty, "object-empty");
    assert!(
        !node_contains_semantic_miss_legacy_equivalent(&host, empty),
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
        node_contains_semantic_miss_legacy_equivalent(&host, obj_with_miss),
        "an object whose member raises to the Miss sentinel contains a semantic miss"
    );

    // Open synthetic index signature ⇒ the synthetic `projectedOpenSurface`
    // value is a sentinel ⇒ the object contains a semantic miss.
    let open = graph.intern_node(SemanticNodeData::Object(SurfaceView {
        members: Arc::from(Vec::<SurfaceMember>::new().into_boxed_slice()),
        call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: true,
    }));
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
    let empty_obj = graph.intern_node(SemanticNodeData::Object(SurfaceView {
        members: Arc::from(Vec::<SurfaceMember>::new().into_boxed_slice()),
        call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    }));

    // `{} & RealObject` ⇒ drops the empty arm, collapses to RealObject ⇒
    // MATERIALIZED (proves the collapse: the raw graph has 2 arms, the raised
    // shape is a single materialized Object).
    let inter_empty_real = graph.intern_node(SemanticNodeData::Intersection(Arc::from(
        vec![empty_obj, real_obj].into_boxed_slice(),
    )));
    assert_classifier_parity(&host, inter_empty_real, "intersection-empty-and-real");
    assert!(
        !node_contains_semantic_miss_legacy_equivalent(&host, inter_empty_real),
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
    let surface_sentinel = graph.intern_node(SemanticNodeData::Object(SurfaceView {
        members: Arc::from(Vec::<SurfaceMember>::new().into_boxed_slice()),
        call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        construct_signatures: Arc::from(vec![non_fn_ctor].into_boxed_slice()),
        index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    }));
    // Sanity: the surface-sentinel node alone raises to the SEMANTIC_OBJECT_SURFACE
    // sentinel ⇒ semantic miss (proves the arm we are about to drop is the real
    // sentinel, not an incidental materialized shape).
    assert_classifier_parity(&host, surface_sentinel, "object-surface-sentinel");
    assert!(
        node_contains_semantic_miss_legacy_equivalent(&host, surface_sentinel),
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
        !node_contains_semantic_miss_legacy_equivalent(&host, inter_sentinel_real),
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
        node_contains_semantic_miss_legacy_equivalent(&host, union_with_miss),
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
        node_contains_semantic_miss_legacy_equivalent(&host, absent),
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
        whole_hash: [1u8; 16],
        local_scope: Some(1),
    };
    let scope_b = NodeScopeId::File {
        canonical_id: Arc::from("/w/b.ts"),
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
            whole_hash: [1u8; 16],
            decl_name: Arc::from("Same"),
        },
    });
    let declref_b = graph.intern_node(SemanticNodeData::DeclRef {
        identity: DeclIdentity {
            canonical_id: Arc::from("/w/b.ts"),
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
    assert!(node_contains_semantic_miss_legacy_equivalent(&host, miss));
    assert!(!node_contains_semantic_miss_legacy_equivalent(
        &host, string_id
    ));

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
            whole_hash: [1u8; 16],
            decl_name: Arc::from("Same"),
        },
    });
    let declref_b = graph.intern_node(SemanticNodeData::DeclRef {
        identity: DeclIdentity {
            canonical_id: Arc::from("/w/b.ts"),
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
    let func = graph.intern_node(SemanticNodeData::Function {
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
        !node_contains_semantic_miss_legacy_equivalent(&host, func),
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
    let func_miss_param = graph.intern_node(SemanticNodeData::Function {
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
        node_contains_semantic_miss_legacy_equivalent(&host, func_miss_param),
        "a Function whose param raises to the Miss sentinel contains a semantic miss (the miss \
         predicate recurses the param — NOT a leaf)"
    );

    // `new (a: string) => Foo` — ConstructorType over a Function signature node.
    // The raiser raises the signature Function then rewraps as ConstructorType;
    // the miss predicate treats ConstructorType identically to Function (recurses
    // the same FunctionExpr payload).
    let ctor = graph.intern_node(SemanticNodeData::ConstructorType { signature: func });
    assert_classifier_parity(&host, ctor, "constructor-type-materialized");
    assert!(
        !node_contains_semantic_miss_legacy_equivalent(&host, ctor),
        "a ConstructorType over a materialized signature ⇒ NOT a semantic miss"
    );

    // A ConstructorType whose signature param misses ⇒ semantic miss (recursion
    // through the construct signature, mirroring Function).
    let ctor_miss = graph.intern_node(SemanticNodeData::ConstructorType {
        signature: func_miss_param,
    });
    assert_classifier_parity(&host, ctor_miss, "constructor-type-miss-param");
    assert!(
        node_contains_semantic_miss_legacy_equivalent(&host, ctor_miss),
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
        !node_contains_semantic_miss_legacy_equivalent(&host, template),
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
        !node_contains_semantic_miss_legacy_equivalent(&host, template_miss_expr),
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
        !node_contains_semantic_miss_legacy_equivalent(&host, tp),
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
        !node_contains_semantic_miss_legacy_equivalent(&host, tp_miss_constraint),
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
    let string_keyed = graph.intern_node(SemanticNodeData::Object(SurfaceView {
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
    }));
    assert_classifier_parity(
        &host,
        string_keyed,
        "object-real-index-signature-string-number",
    );
    assert!(
        !node_contains_semantic_miss_legacy_equivalent(&host, string_keyed),
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
    let idx_value_miss = graph.intern_node(SemanticNodeData::Object(SurfaceView {
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
    }));
    assert_classifier_parity(&host, idx_value_miss, "object-index-signature-value-miss");
    assert!(
        node_contains_semantic_miss_legacy_equivalent(&host, idx_value_miss),
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
        !node_contains_semantic_miss_legacy_equivalent(&host, bare_arg),
        "a BareRef<number> raises to a Ref leaf ⇒ NOT a miss"
    );

    // `Foo<absent>` — the absent type-arg raises to `None`, the BareRef arm
    // materialises it as `Unknown { raw: "<raise miss>" }` so the OUTER Ref still
    // constructs. `<raise miss>` is NOT in the sentinel set ⇒ the carrier reads
    // MATERIALIZED. This proves the carrier-arg miss is MATERIALIZED (does NOT
    // taint the parent) per LEGACY — the `.unwrap_or(Unknown { "<raise miss>" })`
    // arm.
    let bare_miss_arg = graph.intern_node(SemanticNodeData::new_bare_ref(
        Arc::from("Foo"),
        NodeScopeId::Global,
        Arc::from(vec![absent].into_boxed_slice()),
    ));
    assert_classifier_parity(&host, bare_miss_arg, "bare-ref-raise-miss-arg");
    assert!(
        node_can_shell_raise(&host, bare_miss_arg),
        "a BareRef with an absent type-arg still raises (the arg becomes <raise miss>; the outer \
         Ref constructs)"
    );
    assert!(
        !node_contains_semantic_miss_legacy_equivalent(&host, bare_miss_arg),
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
        !node_contains_semantic_miss_legacy_equivalent(&host, import_arg),
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
            && !node_contains_semantic_miss_legacy_equivalent(&host, import_miss_arg),
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
                local_scope: None,
            },
            name: Arc::from("factory"),
        },
        Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
        Arc::from(vec![number_id].into_boxed_slice()),
    ));
    assert_classifier_parity(&host, typeof_arg, "typeof-with-arg");
    assert!(
        !node_contains_semantic_miss_legacy_equivalent(&host, typeof_arg),
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
                local_scope: None,
            },
            name: Arc::from("factory"),
        },
        Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
        Arc::from(vec![absent].into_boxed_slice()),
    ));
    assert_classifier_parity(&host, typeof_miss_arg, "typeof-raise-miss-arg");
    assert!(
        node_can_shell_raise(&host, typeof_miss_arg)
            && !node_contains_semantic_miss_legacy_equivalent(&host, typeof_miss_arg),
        "a TypeOf with an absent instantiation arg materialises it as <raise miss>; the carrier \
         reads materialized"
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
                matches!(&expressions[0], TypeExpr::Unknown { .. }),
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
            visibility: verter_type_expr::MemberVisibility::Public,
            name: Arc::from(*name),
            value: *value,
            optional: false,
            readonly: false,
            is_method: false,
            declared_in_macro_type_arg: false,
            merge_role: crate::semantic_query::MemberMergeRole::Authored,
            spans: Default::default(),
            declaration_origin: None,
        })
        .collect::<Vec<_>>();
    SurfaceView {
        members: Arc::from(members.into_boxed_slice()),
        call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    }
}

/// A content-free `DeclIdentity` (whole_hash zeroed) for `TypeParam.decl` /
/// fixture decl refs — tests carry no real content hash.
fn decl_identity_unscoped(canonical_id: &str, name: &str) -> DeclIdentity {
    DeclIdentity {
        canonical_id: Arc::from(canonical_id),
        whole_hash: [0u8; 16],
        decl_name: Arc::from(name),
    }
}

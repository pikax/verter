//! Node-domain registry predicate / reduction-context parity + carrier-contract
//! tests for `node_materialize` — the `#[path]`-attached `node_predicate_parity_tests`
//! submodule. It pins the node-domain object-surface / published-operator predicates
//! against the `TypeExpr` predicates, the Mapped-sentinel boundary, the
//! registry-publication carrier contract, and the member-path None-branch raw-route
//! facts.

use super::{
    component_meta_registry_node_has_explicit_object_surface,
    component_meta_registry_node_has_non_object_top_level_surface,
    materialize_member_node_to_type_expr, node_raises_to_object_surface,
};
use crate::meta::MetaProject;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::component_meta_registry::{
    component_meta_registry_has_explicit_object_surface,
    component_meta_registry_has_non_object_top_level_surface,
};
use crate::semantic_query::ProjectionMode;
use crate::types::{AnalysisLevel, HostConfig};
use crate::VerterHost;
use std::sync::Arc as StdArc;
use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, TypeExpr};

fn one_prop_object(name: &str) -> TypeExpr {
    TypeExpr::Object(StdArc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
            name.to_string(),
            TypeExpr::string_literal("x"),
            false,
            false,
        ))],
    }))
}

fn open_host() -> StdArc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let project = MetaProject::new(host);
    project
        .upsert_base("/p.ts", "export type Anchor = number\n")
        .unwrap();
    project
}

/// PARITY: the node-domain registry object-surface predicates answer IDENTICALLY
/// to the `TypeExpr` predicates applied to the node's raised value (the exact
/// value the host-side registry loop publishes). A node-fact MUTATION breaks this:
/// dropping the `Union`/`Intersection` arm from
/// `component_meta_registry_node_has_explicit_object_surface`, or adding `KeyOf` to
/// `component_meta_registry_node_has_non_object_top_level_surface` (which the
/// `TypeExpr` predicate does NOT have), flips a `Union`/operator case below.
#[test]
fn registry_object_surface_node_predicates_mirror_type_expr_predicates_on_raised_value() {
    let project = open_host();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/p.ts").unwrap();
    let host = session.host();
    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    let dispatch = ProjectSemanticDispatch::new(ctx);

    let cases: Vec<TypeExpr> = vec![
        // Object surface.
        one_prop_object("a"),
        // Union carrying an object arm AND a non-object arm.
        TypeExpr::Union(StdArc::from(vec![
            one_prop_object("a"),
            TypeExpr::string_literal("lit"),
        ])),
        // Intersection of two object arms (still an object surface).
        TypeExpr::Intersection(StdArc::from(vec![
            one_prop_object("a"),
            one_prop_object("b"),
        ])),
        // A bare literal (non-object, non-ref).
        TypeExpr::string_literal("solo"),
        // A bare reference carrier to a missing name (raises to `Ref`).
        TypeExpr::Ref {
            name: StdArc::from("DefinitelyMissingType"),
            type_arguments: StdArc::from(Vec::new().into_boxed_slice()),
        },
    ];

    for expr in &cases {
        let Some(node) =
            dispatch.lower_type_expr_in_scope_with_mode("/p.ts", expr, ProjectionMode::Navigate)
        else {
            continue;
        };
        let Some(raised) = materialize_member_node_to_type_expr(ctx, node) else {
            continue;
        };
        assert_eq!(
            component_meta_registry_node_has_explicit_object_surface(ctx, node),
            component_meta_registry_has_explicit_object_surface(&raised),
            "explicit-object-surface NODE predicate must mirror the TypeExpr predicate on the \
             raised value for {expr:?} (raised={raised:?})",
        );
        assert_eq!(
            component_meta_registry_node_has_non_object_top_level_surface(ctx, node),
            component_meta_registry_has_non_object_top_level_surface(&raised),
            "non-object-top-level NODE predicate must mirror the TypeExpr predicate on the \
             raised value for {expr:?} (raised={raised:?})",
        );
    }
}

/// DISCRIMINATION: a `Union[Object, literal]` node exercises BOTH arms of the
/// node predicates — the object arm (`explicit_object_surface == true`) and the
/// non-object arm (`non_object_top_level == true`) — while
/// `node_raises_to_object_surface` is FALSE on the `Union` root (a `Union` does
/// not raise to a plain `Object`, the property that gates the owner-local arm). An
/// inline `Object` root inverts the non-object arm and IS an object-raising root.
/// Removing any arm flips one of these assertions.
#[test]
fn registry_node_predicates_discriminate_union_and_object_roots() {
    let project = open_host();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/p.ts").unwrap();
    let host = session.host();
    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    let dispatch = ProjectSemanticDispatch::new(ctx);

    let union = TypeExpr::Union(StdArc::from(vec![
        one_prop_object("a"),
        TypeExpr::string_literal("lit"),
    ]));
    let union_node = dispatch
        .lower_type_expr_in_scope_with_mode("/p.ts", &union, ProjectionMode::Navigate)
        .expect("union lowers");
    assert!(
        component_meta_registry_node_has_explicit_object_surface(ctx, union_node),
        "a Union with an Object arm IS an explicit object surface",
    );
    assert!(
        component_meta_registry_node_has_non_object_top_level_surface(ctx, union_node),
        "a Union with a non-object arm HAS a non-object top-level surface",
    );
    assert!(
        !node_raises_to_object_surface(ctx, union_node),
        "a Union root does NOT raise to a plain Object",
    );

    let object_node = dispatch
        .lower_type_expr_in_scope_with_mode(
            "/p.ts",
            &one_prop_object("a"),
            ProjectionMode::Navigate,
        )
        .expect("object lowers");
    assert!(component_meta_registry_node_has_explicit_object_surface(
        ctx,
        object_node
    ));
    assert!(!component_meta_registry_node_has_non_object_top_level_surface(ctx, object_node));
    assert!(
        node_raises_to_object_surface(ctx, object_node),
        "an inline Object root DOES raise to a plain Object",
    );
}

/// PARITY (§6 published-operator-root trap): the node-domain second-pass
/// reduction context must EQUAL the `TypeExpr`-start context the former
/// `materialize_component_meta_type_expr_until_stable` computed on the SAME
/// surface (the node's raised value). A `node_root_is_published_operator`
/// mis-classification (e.g. treating a `Union`/`Object` root as a published
/// operator, or missing the `Ref`/`Mapped`/`IndexedAccess` carriers) silently
/// flips `StructuralTransit(Navigate)` ↔ `Published(Navigate)` and is caught here.
#[test]
fn node_reduction_context_mirrors_type_expr_reduction_context_on_raised_value() {
    use crate::meta_resolve::materialize::{
        node_materialize_reduction_context, type_expr_materialize_reduction_context,
    };

    let project = open_host();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/p.ts").unwrap();
    let host = session.host();
    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    let dispatch = ProjectSemanticDispatch::new(ctx);

    let cases: Vec<TypeExpr> = vec![
        one_prop_object("a"),
        TypeExpr::Union(StdArc::from(vec![
            one_prop_object("a"),
            TypeExpr::string_literal("lit"),
        ])),
        TypeExpr::string_literal("solo"),
        TypeExpr::Ref {
            name: StdArc::from("DefinitelyMissingType"),
            type_arguments: StdArc::from(Vec::new().into_boxed_slice()),
        },
    ];

    for expr in &cases {
        let Some(node) =
            dispatch.lower_type_expr_in_scope_with_mode("/p.ts", expr, ProjectionMode::Navigate)
        else {
            continue;
        };
        let Some(raised) = materialize_member_node_to_type_expr(ctx, node) else {
            continue;
        };
        assert_eq!(
            node_materialize_reduction_context(ctx, node, ProjectionMode::Navigate),
            type_expr_materialize_reduction_context(
                ctx,
                "/p.ts",
                &raised,
                ProjectionMode::Navigate
            ),
            "node reduction context must mirror the TypeExpr reduction context on the raised \
             value for {expr:?} (raised={raised:?})",
        );
    }
}

/// F1 / mutations (a) + (b): the node-domain published-operator + non-object-top
/// predicates pin EVERY operator root exactly as the `TypeExpr` predicates do.
/// Operator nodes are DIRECT-interned (`KeyOf` / `IndexedAccess` / `Conditional` /
/// `TypeOf`) so the arms are pinned structurally, independent of whether a lowered
/// operator survives reduction. DISCRIMINATING:
/// - mutation (a) — adding a `KeyOf` arm to
///   `component_meta_registry_node_has_non_object_top_level_surface` (which the
///   `TypeExpr` predicate does NOT have) flips the `!non_object_top(KeyOf)` assertion;
/// - mutation (b) — dropping the `KeyOf|IndexedAccess|Conditional|TypeOf` arm of
///   `node_root_is_published_operator` flips every operator from Published(Navigate)
///   to StructuralTransit(Navigate).
#[test]
fn node_published_operator_and_non_object_top_pin_every_operator_root() {
    use crate::meta_resolve::materialize::node_materialize_reduction_context;
    use crate::semantic_query::{
        IndexKey, ProjectionReductionContext, QueryError, ScopeId, SemanticNodeData, ValueRootKey,
    };

    let project = open_host();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/p.ts").unwrap();
    let host = session.host();
    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    let graph = ctx.project_type_store().semantic_graph();

    let dummy = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let keyof = graph.intern_node(SemanticNodeData::KeyOf { base: dummy });
    let indexed = graph.intern_node(SemanticNodeData::IndexedAccess {
        object: dummy,
        index: IndexKey::String(StdArc::from("x")),
    });
    let conditional = graph.intern_node(SemanticNodeData::Conditional {
        check: dummy,
        extends: dummy,
        true_branch_ref: dummy,
        false_branch_ref: dummy,
        distributive: false,
    });
    let typeof_node = graph.intern_node(SemanticNodeData::new_typeof(
        ValueRootKey {
            scope: ScopeId {
                canonical_id: StdArc::from("/p.ts"),
                local_scope: None,
            },
            name: StdArc::from("missingValue"),
        },
        StdArc::from(Vec::new().into_boxed_slice()),
        StdArc::from(Vec::new().into_boxed_slice()),
    ));

    let published = ProjectionReductionContext::published(ProjectionMode::Navigate);
    for (label, node) in [
        ("KeyOf", keyof),
        ("IndexedAccess", indexed),
        ("Conditional", conditional),
        ("TypeOf", typeof_node),
    ] {
        assert_eq!(
            node_materialize_reduction_context(ctx, node, ProjectionMode::Navigate),
            published,
            "{label} root is a published operator ⇒ Published(Navigate); dropping the operator \
             arm of node_root_is_published_operator flips this to StructuralTransit",
        );
    }

    // `KeyOf` and `TypeOf` are NOT non-object-top roots (the `_` arm of BOTH
    // predicates); `IndexedAccess` and `Conditional` ARE. Adding a `KeyOf` arm to
    // the node predicate flips the KeyOf assertion.
    assert!(
        !component_meta_registry_node_has_non_object_top_level_surface(ctx, keyof),
        "a KeyOf root is NOT a non-object-top surface (mirrors the TypeExpr `_` arm)",
    );
    assert!(
        !component_meta_registry_node_has_non_object_top_level_surface(ctx, typeof_node),
        "a TypeOf root is NOT a non-object-top surface",
    );
    assert!(
        component_meta_registry_node_has_non_object_top_level_surface(ctx, indexed),
        "an IndexedAccess root IS a non-object-top surface (anti-vacuity: the arm is reachable)",
    );
    assert!(
        component_meta_registry_node_has_non_object_top_level_surface(ctx, conditional),
        "a Conditional root IS a non-object-top surface",
    );
}

/// F1 Mapped-arm fix: `node_root_is_published_operator` suppresses a `Mapped` root
/// (StructuralTransit) ONLY when its value raises to EXACTLY `semanticMiss`, and
/// PUBLISHES (Published) when the value raises to ANY OTHER unmaterialised sentinel
/// (object-surface) — mirroring `type_expr_root_is_published_operator`'s
/// `value == Unknown { raw == "semanticMiss" }` carrier check. DISCRIMINATING:
/// before the fix the node path used the BROAD sentinel set
/// (`node_root_is_unmaterialized_sentinel_with_dispatch`), so the object-surface
/// value wrongly suppressed to StructuralTransit. The two `Mapped` values are
/// DIRECT-interned `Opaque(QueryError)` carriers (`Miss` vs `UnrepresentableSurface`)
/// so the test pins the exact sentinel boundary.
#[test]
fn node_published_operator_mapped_suppresses_only_semantic_miss_value() {
    use crate::meta_resolve::materialize::node_materialize_reduction_context;
    use crate::semantic_query::{
        MapperKey, MapperKind, OptionalityMod, ProjectionReductionContext, QueryError, ReadonlyMod,
        SemanticNodeData,
    };

    let project = open_host();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/p.ts").unwrap();
    let host = session.host();
    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    let graph = ctx.project_type_store().semantic_graph();

    let dummy = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let mapped_with = |value_expr| {
        graph.intern_node(SemanticNodeData::Mapped {
            source: dummy,
            mapper: MapperKey {
                parameter_node: dummy,
                key_space: dummy,
                value_expr,
                optionality: OptionalityMod::Keep,
                readonly: ReadonlyMod::Keep,
                name_remap: None,
                kind: MapperKind::Identity,
            },
        })
    };
    let miss_val = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let objsurface_val =
        graph.intern_node(SemanticNodeData::Opaque(QueryError::UnrepresentableSurface));

    let published = ProjectionReductionContext::published(ProjectionMode::Navigate);
    let transit =
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);

    assert_eq!(
        node_materialize_reduction_context(ctx, mapped_with(miss_val), ProjectionMode::Navigate),
        transit,
        "a Mapped whose value raises to semanticMiss is a carrier ⇒ StructuralTransit",
    );
    assert_eq!(
        node_materialize_reduction_context(
            ctx,
            mapped_with(objsurface_val),
            ProjectionMode::Navigate
        ),
        published,
        "a Mapped whose value raises to the object-surface sentinel (NOT semanticMiss) \
         PUBLISHES ⇒ Published; the pre-fix BROAD sentinel check wrongly suppressed this to \
         StructuralTransit",
    );
}

/// §1a (R2 normalization-fix DISCRIMINATION): the node-domain raised-ROOT mirrors
/// (`node_raises_to_object_surface`, `node_is_indexed_access_shell`, the
/// published-operator + typeof root facts) read the POST-NORMALIZED raised root
/// through the shared shape-engine fold, so each answers IDENTICALLY to the
/// `TypeExpr` predicate on `raise(node)` — even for DIRECTLY-INTERNED shapes that
/// real lowering pre-collapses (the divergence is unreachable today; this pins it
/// closed defensively). The fold drops the Intersection empty-object / object-
/// surface-sentinel arms BEFORE classifying the root, so an `Intersection([{}, X])`
/// is classified by its surviving arm `X`.
///
/// DISCRIMINATING: each asserted value is the one a RAW-NODE walk gets WRONG —
/// reverting any mirror to `match node_data { … }` over the bare `Intersection`
/// root flips it. E.g. reverting `node_is_indexed_access_shell` to its former
/// `matches!(node_data, IndexedAccess)` immediate-data check returns `false` on
/// `Intersection([{}, IndexedAccess])` (top-level is `Intersection`), while the
/// fold-backed mirror — and `matches!(raise(node), IndexedAccess)` — return `true`,
/// so the `assert_eq!` against the raised value FAILS. Likewise reverting
/// `node_raises_to_object_surface` / `node_root_is_typeof` / the published-operator
/// walk to the raw `match node_data` flips the `Intersection([{}, Object])` /
/// `Intersection([{}, TypeOf])` anchors.
#[test]
fn raised_root_mirrors_match_type_expr_predicate_on_collapsed_intersection_roots() {
    use crate::meta_resolve::materialize::{
        node_materialize_reduction_context, type_expr_materialize_reduction_context,
    };
    use crate::project_semantic_dispatch::raise::node_root_is_typeof_with_dispatch;
    use crate::semantic_query::{
        IndexKey, QueryError, ScopeId, SemanticNodeData, SurfaceView, ValueRootKey,
    };

    let project = open_host();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/p.ts").unwrap();
    let host = session.host();
    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let graph = ctx.project_type_store().semantic_graph();

    let empty_surface = || SurfaceView {
        members: StdArc::from(Vec::new().into_boxed_slice()),
        call_signatures: StdArc::from(Vec::new().into_boxed_slice()),
        construct_signatures: StdArc::from(Vec::new().into_boxed_slice()),
        index_signatures: StdArc::from(Vec::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    };
    // The representable empty object `{}` — an Intersection arm the fold DROPS
    // before classifying the surviving root.
    let empty_obj = graph.intern_node(SemanticNodeData::Object(empty_surface()));
    let dummy = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let indexed = graph.intern_node(SemanticNodeData::IndexedAccess {
        object: dummy,
        index: IndexKey::String(StdArc::from("x")),
    });
    let typeof_node = graph.intern_node(SemanticNodeData::new_typeof(
        ValueRootKey {
            scope: ScopeId {
                canonical_id: StdArc::from("/p.ts"),
                local_scope: None,
            },
            name: StdArc::from("missingValue"),
        },
        StdArc::from(Vec::new().into_boxed_slice()),
        StdArc::from(Vec::new().into_boxed_slice()),
    ));
    let one_prop = dispatch
        .lower_type_expr_in_scope_with_mode(
            "/p.ts",
            &one_prop_object("a"),
            ProjectionMode::Navigate,
        )
        .expect("object lowers");

    // Divergent shapes (DIRECTLY interned so lowering does NOT pre-collapse them).
    let int_obj = graph.intern_node(SemanticNodeData::Intersection(StdArc::from(
        vec![empty_obj, one_prop].into_boxed_slice(),
    )));
    let int_indexed = graph.intern_node(SemanticNodeData::Intersection(StdArc::from(
        vec![empty_obj, indexed].into_boxed_slice(),
    )));
    let int_typeof = graph.intern_node(SemanticNodeData::Intersection(StdArc::from(
        vec![empty_obj, typeof_node].into_boxed_slice(),
    )));
    // The brief's `Union([Intersection([{}, Object])])`: a single-arm union is NOT
    // collapsed by the materializer, so it raises to `Union([Object])` (a Union
    // root) — a clean AGREEMENT case (both mirror + TypeExpr predicate say "not an
    // object/operator root"), included to pin that the outer union is preserved.
    let union_int_obj = graph.intern_node(SemanticNodeData::Union(StdArc::from(
        vec![int_obj].into_boxed_slice(),
    )));

    // Every node: the mirror MUST equal the `TypeExpr` predicate on `raise(node)`.
    for node in [
        int_obj,
        int_indexed,
        int_typeof,
        union_int_obj,
        indexed,
        typeof_node,
        one_prop,
        empty_obj,
    ] {
        let raised =
            materialize_member_node_to_type_expr(ctx, node).expect("interned shape raises");
        assert_eq!(
            node_raises_to_object_surface(ctx, node),
            matches!(raised, TypeExpr::Object(_)),
            "node_raises_to_object_surface must mirror matches!(raise, Object) for {raised:?}",
        );
        assert_eq!(
            super::node_is_indexed_access_shell(ctx, node),
            matches!(raised, TypeExpr::IndexedAccess { .. }),
            "node_is_indexed_access_shell must mirror matches!(raise, IndexedAccess) for {raised:?}",
        );
        assert_eq!(
            node_root_is_typeof_with_dispatch(&dispatch, node),
            matches!(raised, TypeExpr::TypeOf(_)),
            "node_root_is_typeof must mirror matches!(raise, TypeOf) for {raised:?}",
        );
        assert_eq!(
            node_materialize_reduction_context(ctx, node, ProjectionMode::Navigate),
            type_expr_materialize_reduction_context(
                ctx,
                "/p.ts",
                &raised,
                ProjectionMode::Navigate
            ),
            "node reduction context (published-operator mirror) must mirror the TypeExpr context \
             for {raised:?}",
        );
    }

    // DISCRIMINATING anchors — the exact values a RAW-NODE walk gets WRONG (it sees
    // the bare `Intersection` root ⇒ false; the fold collapses the `{}` arm and
    // classifies the surviving operator/object/typeof arm ⇒ true).
    assert!(
        super::node_is_indexed_access_shell(ctx, int_indexed),
        "Intersection([{{}}, IndexedAccess]) raises to IndexedAccess (raw walk ⇒ false)",
    );
    assert!(
        node_raises_to_object_surface(ctx, int_obj),
        "Intersection([{{}}, Object]) raises to Object (raw walk ⇒ false)",
    );
    assert!(
        node_root_is_typeof_with_dispatch(&dispatch, int_typeof),
        "Intersection([{{}}, TypeOf]) raises to TypeOf (raw walk ⇒ false)",
    );
}

/// Count the route-admitted-carrier MINT literal spellings in `src` (ignoring
/// `//`-comment lines): the carrier constructor `AdmittedRouteProjectionNode::new`
/// AND the gated mint API `route_admission::admit` (the only routes to a minted
/// carrier). A doc / comment NAMING either is NOT a mint. Shared by the residual
/// literal tripwire ([`registry_candidate_module_never_forges_route_admitted_carrier`])
/// and its self-test
/// ([`admitted_carrier_mint_detector_discriminates_forge_from_no_admission_path`]).
fn admitted_carrier_mint_count(src: &str) -> usize {
    src.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .map(|line| {
            line.matches("AdmittedRouteProjectionNode::new").count()
                + line.matches("route_admission::admit").count()
        })
        .sum()
}

/// Carrier-contract RESIDUAL literal tripwire (whole-module): the node-domain
/// registry-candidate materialisation module (`node_materialize.rs`) holds
/// first-pass / stabilised member-surface nodes that are NOT route/surface-adapter
/// admitted nodes (each can be a `Miss`/`Recursive`/`Tainted`/degenerate outcome),
/// so it must publish them ONLY through the no-admission `RegistryPublicationNode`
/// carrier + the `materialize_registry_publication_node` sink — never as a
/// route-admitted `AdmittedRouteProjectionNode`.
///
/// SCOPE (narrowed — NOT the primary defense, NOT every-site/structural): this
/// scans for the LITERAL mint spellings in `node_materialize.rs` only. The
/// STRUCTURAL CONFINEMENT is primary and lives in `route_admission`, now a
/// COMPLETE three-layer seal: (1) `AdmittedRouteProjectionNode::new` is PRIVATE to
/// that module, so a `node_materialize.rs` `AdmittedRouteProjectionNode::new(node)`
/// — directly OR alias-laundered (`use … as Forge; Forge::new(node)`) — is a
/// COMPILE error (`E0624: associated function `new` is private`); (2) the gate
/// inputs are the node-bound `RaisedNodeShapeFacts` / `NodeShapeEq` witness with
/// PRIVATE fields, so a sibling cannot fabricate a passing witness struct literal
/// to drive a gated helper (`E0451: field … is private`); (3) every `admit_*`
/// takes ONLY the witness (no free `SemanticNodeId` param) and mints from
/// `witness.node()`, so a "node A's facts, node B's carrier" mispair is
/// UNREPRESENTABLE. The only mints are the gated `route_admission::admit_*`
/// helpers; this tripwire is a cheap literal backstop for THAT spelling, which the
/// compiler permits (the helpers are subtree-visible) but which `node_materialize`
/// must never use.
///
/// GUARD-LOCAL SC-FIRST RECORD:
/// - scanner_invariant: `node_materialize.rs` contains zero route-admitted-carrier
///   mint literal spellings (`AdmittedRouteProjectionNode::new` /
///   `route_admission::admit`).
/// - scanner_justification: residual literal backstop for the gated-helper spelling
///   the compiler cannot reject (the helpers are subtree-visible); the private
///   constructor PLUS the sealed node-bound `RaisedNodeShapeFacts` / `NodeShapeEq`
///   witness (admit_* take only the witness, no free node param) are the primary,
///   compiler-enforced seal.
/// - mechanism_ruling: r3-scfirst-ruling (structural-confinement-first), extended
///   by the r5-disposition-ruling node-bound-witness hardening.
/// - hardening_rounds: SUPERSEDED — the prior broadened whole-module name-scanner
///   was REPLACED by structural confinement (a private constructor, since hardened
///   to a node-bound witness input); this is the narrowed residual remnant, not a
///   further scanner-hardening round.
/// - escape_stop: the alias-laundering forge `use super::AdmittedRouteProjectionNode
///   as Forge; Forge::new(node)` — which the OLD name-scanner missed (it scanned for
///   the literal `AdmittedRouteProjectionNode::new`, not the alias) — is now STOPPED
///   structurally by the private constructor (`E0624`), not by this scanner.
/// - hardening_history: was a whole-module name-scanner asserting "every-site"
///   enforcement; that claim was launderable, so enforcement moved to the private
///   constructor and this scanner was narrowed to a literal-spelling residual.
#[test]
fn registry_candidate_module_never_forges_route_admitted_carrier() {
    const SRC: &str = include_str!("node_materialize.rs");
    let forged = admitted_carrier_mint_count(SRC);
    assert_eq!(
        forged, 0,
        "the node-domain registry-candidate module (node_materialize.rs) must NOT mint a \
         route-admitted AdmittedRouteProjectionNode for a held member-surface node (directly or \
         via a route_admission::admit_* helper) — publish every such node through the \
         no-admission RegistryPublicationNode carrier + the materialize_registry_publication_node \
         sink. Found {forged} mint spelling(s). (The carrier constructor is also privately sealed \
         in route_admission, so a direct/alias-forged `::new` is already an E0624 compile error; \
         this residual catches the gated-helper spelling.)",
    );
    // Anti-vacuity: the module DOES publish member-surface nodes through the
    // no-admission carrier, so the absence above is a real property (never vacuous on
    // an empty / renamed module).
    assert!(
        SRC.contains("RegistryPublicationNode"),
        "anti-vacuity: the registry-candidate module must route member-surface nodes through \
         the no-admission RegistryPublicationNode carrier",
    );
}

/// Self-test for the [`admitted_carrier_mint_count`] residual detector: BOTH mint
/// spellings — the direct constructor `AdmittedRouteProjectionNode::new(...)` (now
/// also an `E0624` compile error in production) and the gated helper
/// `route_admission::admit_*(...)` (the compiler-permitted residual) — are COUNTED
/// (so the tripwire fails on either), the no-admission
/// `materialize_member_node_to_type_expr` / `RegistryPublicationNode` path is NOT
/// counted, and a comment NAMING a mint is NOT counted (no false positive).
#[test]
fn admitted_carrier_mint_detector_discriminates_forge_from_no_admission_path() {
    // Direct-constructor forge (also an E0624 compile error in production).
    let forged_ctor = "        if !node_raises_to_object_surface(ctx, node) { return None; }\n\
                  \x20       let admitted = AdmittedRouteProjectionNode::new(node);\n\
                  \x20       let ty = super::surface::materialize_route_projection_node(ctx, &admitted)?;\n";
    assert_eq!(
        admitted_carrier_mint_count(forged_ctor),
        1,
        "the detector MUST flag a forged AdmittedRouteProjectionNode::new mint",
    );
    // Gated-helper forge — the residual the compiler permits (helpers are subtree-visible).
    let forged_helper =
        "        let witness = node_raised_shape_facts_with_dispatch(&dispatch, node)?;\n\
                  \x20       let admitted = route_admission::admit_materialized(&witness)?;\n";
    assert_eq!(
        admitted_carrier_mint_count(forged_helper),
        1,
        "the detector MUST flag a route_admission::admit_* gated-helper mint",
    );
    // The sanctioned no-admission path — routes through the RegistryPublicationNode
    // helper, no forged admitted carrier.
    let sanctioned = "        if !node_raises_to_object_surface(ctx, node) { return None; }\n\
                      \x20       let ty = materialize_member_node_to_type_expr(ctx, node)?;\n";
    assert_eq!(
        admitted_carrier_mint_count(sanctioned),
        0,
        "the sanctioned no-admission RegistryPublicationNode path is NOT a forge",
    );
    // A comment NAMING a mint is not a mint (no false positive).
    let comment = "        // NOT AdmittedRouteProjectionNode::new / route_admission::admit — \
                   see the no-admission carrier.\n";
    assert_eq!(
        admitted_carrier_mint_count(comment),
        0,
        "a comment naming a mint must NOT be counted as a forge",
    );
}

/// Parse `src` (a `route_admission.rs` source) with `syn` and return, for each
/// `fn admit_*` it defines, the fn name PAIRED with its EXACT parameter-TYPE list —
/// each type rendered to tokens with all whitespace stripped, so
/// `&RaisedNodeShapeFacts` normalises to `"&RaisedNodeShapeFacts"`. The list is
/// keyed on parameter TYPES (NOT a textual `SemanticNodeId` substring scan), so
/// ANY extra parameter — any name, any type, INCLUDING an alias of `SemanticNodeId`
/// (`extra: NodeIdAlias`) — appears as an extra list entry the exact-param-list
/// guard rejects, closing the alias evasion the substring scan missed.
fn admit_fn_param_types(src: &str) -> Vec<(String, Vec<String>)> {
    use quote::ToTokens;
    let file = syn::parse_file(src).expect("route_admission.rs must parse as a syn::File");
    let mut out = Vec::new();
    for item in &file.items {
        let syn::Item::Fn(item_fn) = item else {
            continue;
        };
        let name = item_fn.sig.ident.to_string();
        if !name.starts_with("admit_") {
            continue;
        }
        let params: Vec<String> = item_fn
            .sig
            .inputs
            .iter()
            .map(|arg| match arg {
                syn::FnArg::Typed(pat_type) => pat_type
                    .ty
                    .to_token_stream()
                    .to_string()
                    .split_whitespace()
                    .collect::<String>(),
                // A free `admit_*` mint helper has no receiver; a `self` param
                // would itself be an unexpected signature, surfaced verbatim so
                // the exact-list comparison rejects it.
                syn::FnArg::Receiver(_) => "self".to_string(),
            })
            .collect();
        out.push((name, params));
    }
    out
}

/// EXACT-PARAM-LIST admission signature guard: every `route_admission::admit_*`
/// gated mint helper takes EXACTLY its expected node-bound-witness parameter list
/// and NOTHING else — so ANY extra parameter (any name, any TYPE, including an
/// alias of `SemanticNodeId`) fails. The carrier a helper mints is bound to
/// `witness.node()` / `shape.node()`, so a "node A's facts, node B's carrier"
/// mispair is unrepresentable through the safe API.
///
/// GUARD-LOCAL SC-FIRST RECORD:
/// - scanner_invariant: each `route_admission::admit_*` has EXACTLY its expected
///   parameter-TYPE list (the node-bound witness) and no extra parameter.
/// - scanner_justification: a `syn`-parsed regression tripwire over the exact
///   parameter-TYPE list (`admit_fn_param_types` via `syn::parse_file`). The
///   STRUCTURAL PRIMARY is the witness TYPE itself + `E0451` (its private fields):
///   `admit_*` take only the sealed node-bound witness, so a raw-node mispair is
///   unrepresentable and the mispair vector is COMPLETELY compiler-sealed. This scan
///   covers only the residual shape the COMPILER CANNOT EXPRESS — "this signature
///   carries no additional parameter": no Rust type rule can forbid an extra param,
///   so a scanner is the sole available check for the no-extra-param shape. Unlike a
///   textual `SemanticNodeId` substring scan, the exact-list form also catches an
///   ALIAS-typed extra param (`extra: NodeIdAlias`).
/// - mechanism_ruling: r5-disposition-ruling (node-bound witness as the sole
///   cross-module admission input) — the structural witness seal is the COMPLETE
///   primary; this signature-shape supplement covers only the compiler-inexpressible
///   no-extra-param remnant.
/// - hardening_rounds: SUPERSEDES the prior literal textual `SemanticNodeId`
///   substring scan (which an alias-typed param evaded) with the `syn`
///   exact-param-list form.
/// - hardening_history: textual `SemanticNodeId` substring scan → `syn`
///   exact-param-list. The rebuild closed an alias-typed-param evasion the textual
///   form missed, surfaced in adversarial re-review and settled by the
///   r5-disposition-ruling that fixed node-binding as the terminal admission input;
///   the witness TYPE + `E0451` remain the primary structural seal, this exact-list
///   scan is the backstop.
#[test]
fn route_admission_admit_helpers_take_no_node_param() {
    const SRC: &str = include_str!("route_admission.rs");
    let sigs = admit_fn_param_types(SRC);
    assert!(
        sigs.len() >= 3,
        "anti-vacuity: expected the three gated admit_* mint helpers, found {}",
        sigs.len()
    );
    // Each helper's EXACT expected parameter-TYPE list (names elided — the gate is
    // the type list, normalised whitespace-free).
    let expected: &[(&str, &[&str])] = &[
        ("admit_expanded_surface", &["&RaisedNodeShapeFacts"]),
        ("admit_expanded_surface_changed", &["&NodeShapeEq"]),
        ("admit_materialized", &["&RaisedNodeShapeFacts"]),
    ];
    // Every found admit_* must be a known helper with EXACTLY its expected param
    // list — an extra param of ANY type/name (incl. an alias of SemanticNodeId)
    // makes the actual list differ and FAILS here.
    for (name, params) in &sigs {
        let Some((_, exp)) = expected.iter().find(|(e, _)| e == name) else {
            panic!(
                "unexpected route_admission::{name} mint helper — not covered by the \
                 exact-param-list guard; add it to `expected` with its sealed param list"
            );
        };
        assert_eq!(
            params.as_slice(),
            *exp,
            "route_admission::{name} must take EXACTLY {exp:?} and NOTHING else — any extra \
             parameter (any name, any type, INCLUDING an alias of SemanticNodeId) re-opens the \
             node-mispair forge vector. Found: {params:?}"
        );
    }
    // Every expected helper is actually present (the corpus is not silently short).
    for (name, _) in expected {
        assert!(
            sigs.iter().any(|(n, _)| n == name),
            "expected gated mint helper route_admission::{name} is missing"
        );
    }
}

/// Self-test for [`admit_fn_param_types`] + the exact-param-list invariant. Proves
/// the detector discriminates the sealed witness-only shape from BOTH a re-added
/// `node: SemanticNodeId` parameter AND — the closure the retired textual
/// `SemanticNodeId` substring scan MISSED — an ALIAS-typed extra parameter.
#[test]
fn admit_signature_node_param_detector_discriminates() {
    // The sealed, witness-only shape: EXACTLY `[&RaisedNodeShapeFacts]` ⇒ the
    // exact-list guard PASSES.
    let witness_only = "pub(in x) fn admit_materialized(witness: &RaisedNodeShapeFacts) \
                        -> Option<AdmittedRouteProjectionNode> { body }";
    let clean = admit_fn_param_types(witness_only);
    assert_eq!(clean.len(), 1, "exactly one admit_* in the fixture");
    assert_eq!(
        clean[0].1.as_slice(),
        ["&RaisedNodeShapeFacts"],
        "the witness-only signature parses to the EXACT expected param list (guard PASSES)",
    );

    // (a) A re-added `node: SemanticNodeId` parameter — the exact list now has TWO
    // entries, differing from the expected `[&RaisedNodeShapeFacts]` ⇒ guard FAILS.
    let with_node = "pub(in x) fn admit_materialized(witness: &RaisedNodeShapeFacts, \
                     node: SemanticNodeId) -> Option<AdmittedRouteProjectionNode> { body }";
    let wn = admit_fn_param_types(with_node);
    assert_eq!(
        wn[0].1.as_slice(),
        ["&RaisedNodeShapeFacts", "SemanticNodeId"],
        "the detector SEES the re-added node param as a second list entry (guard FAILS)",
    );
    assert_ne!(
        wn[0].1.as_slice(),
        ["&RaisedNodeShapeFacts"],
        "the node-param list differs from the expected sealed list",
    );

    // (b) An ALIAS-typed extra parameter (`extra: NodeIdAlias`, where
    // `type NodeIdAlias = SemanticNodeId`). The exact-list detector SEES the second
    // entry and the guard FAILS — even though the parameter's text contains NO
    // literal `SemanticNodeId`, so the retired textual substring scan would have
    // MISSED it. This is the new closure the exact-param-list hardening adds.
    let with_alias = "pub(in x) fn admit_materialized(witness: &RaisedNodeShapeFacts, \
                      extra: NodeIdAlias) -> Option<AdmittedRouteProjectionNode> { body }";
    let wa = admit_fn_param_types(with_alias);
    assert_eq!(
        wa[0].1.as_slice(),
        ["&RaisedNodeShapeFacts", "NodeIdAlias"],
        "the exact-param-list detector SEES an ALIAS-typed extra param (guard FAILS)",
    );
    assert_ne!(
        wa[0].1.as_slice(),
        ["&RaisedNodeShapeFacts"],
        "the alias-param list differs from the expected sealed list (guard FAILS)",
    );
    assert!(
        !with_alias.contains("SemanticNodeId"),
        "the retired textual `SemanticNodeId` substring scan would MISS the alias-typed param \
         (it spells `NodeIdAlias`) — the exact-param-list approach is what closes this evasion",
    );
}

/// F5 member-path None-branch: when class-A projection FAILS for a nested
/// `IndexedAccess` member route (`Bar['a']['b']`, leaf `Missing` unresolved), the
/// reject/accept facts are computed on the RAW `route_expr` via the `TypeExpr`
/// predicates (the former `.unwrap_or(route_expr)` leaf) — so a non-object
/// `IndexedAccess` leaf is the REJECT shape (matching OLD), never read off
/// `lower(route_expr, Navigate)`.
///
/// The third assertion PINS the invariant that makes the lower-vs-raw divergence
/// UNREACHABLE for `IndexedAccess` routes: `lower(route_expr, Navigate)` keeps the
/// `IndexedAccess` shell, so its node-facts already equal the raw-`route_expr`
/// facts. The fix removes the fragile dependence on that invariant (and matches OLD);
/// if `Navigate` lowering ever STARTED reducing an `IndexedAccess` shell — making the
/// divergence reachable — this assertion fails and surfaces that the fix's defensive
/// value became load-bearing.
#[test]
fn member_path_leaf_facts_none_branch_reads_raw_route_expr() {
    use crate::resolver_core::ComponentMetaQueryEngine;

    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let project = MetaProject::new(host);
    project
        .upsert_base("/p.ts", "export type Bar = { a: { b: Missing } }\n")
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/p.ts").unwrap();
    let host = session.host();
    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    let dispatch = ProjectSemanticDispatch::new(ctx);

    let route_expr =
        super::registry_indexed_access_expr("Bar", &["a".to_string(), "b".to_string()]);
    assert!(
        crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
            ctx,
            None,
            "/p.ts",
            &route_expr
        )
        .is_none(),
        "class-A projection must FAIL for the unresolved nested member route (None branch)",
    );

    let mut engine = ComponentMetaQueryEngine::new(ctx);
    let (leaf, is_object, non_object_top, is_indexed) =
        engine.project_member_path_leaf_facts("/p.ts", &route_expr);

    // (1) The facts equal the RAW `route_expr` `TypeExpr` predicates — OLD semantics.
    assert_eq!(
        &leaf, &route_expr,
        "the None-branch leaf IS the raw route_expr"
    );
    assert_eq!(
        (is_object, non_object_top, is_indexed),
        (
            component_meta_registry_has_explicit_object_surface(&route_expr),
            component_meta_registry_has_non_object_top_level_surface(&route_expr),
            matches!(route_expr, TypeExpr::IndexedAccess { .. }),
        ),
        "None-branch facts must equal the raw-route_expr TypeExpr facts",
    );
    // (2) The raw nested-IndexedAccess leaf is the REJECT shape the host's
    // `path.len() > 1` member-path arm rejects (non-object, indexed).
    assert!(
        !is_object && non_object_top && is_indexed,
        "the raw nested-IndexedAccess leaf is the REJECT shape (non-object, indexed)",
    );
    // (3) Unreachability invariant: lowering the route keeps the IndexedAccess
    // shell, so the lowered node-facts already equal the raw facts (no divergence to
    // exploit). If this breaks, the fix's defensive value became load-bearing.
    let lowered = dispatch
        .lower_type_expr_in_scope_with_mode("/p.ts", &route_expr, ProjectionMode::Navigate)
        .expect("route_expr lowers");
    assert_eq!(
        (
            component_meta_registry_node_has_explicit_object_surface(ctx, lowered),
            component_meta_registry_node_has_non_object_top_level_surface(ctx, lowered),
            super::node_is_indexed_access_shell(ctx, lowered),
        ),
        (is_object, non_object_top, is_indexed),
        "Navigate lowering keeps the IndexedAccess shell, so lowered facts equal raw facts \
         (the divergence is unreachable for IndexedAccess routes)",
    );
}

/// Build an `Alias`-chain `depth` hops deep terminating in `terminal`, interned in
/// `graph` (each hop is a distinct `Alias(child)` node). Exercises the node-predicate
/// walkers on a chain DEEPER than the former fixed depth cap so the visited-set
/// termination is proven to walk an acyclic chain of ANY depth.
fn deep_alias_chain(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    terminal: crate::semantic_query::SemanticNodeId,
    depth: usize,
) -> crate::semantic_query::SemanticNodeId {
    use crate::semantic_query::SemanticNodeData;
    let mut node = terminal;
    for _ in 0..depth {
        node = graph.intern_node(SemanticNodeData::Alias(node));
    }
    node
}

/// Build the TypeExpr-domain equivalent of [`deep_alias_chain`]: a `Parenthesized`
/// chain `depth` deep terminating in `terminal`. `Parenthesized` is the identity hop
/// the uncapped registry `TypeExpr` predicates peel, mirroring the node domain's
/// `Alias` hop, so the two chains are parity inputs.
fn deep_paren_chain(terminal: TypeExpr, depth: usize) -> TypeExpr {
    let mut expr = terminal;
    for _ in 0..depth {
        expr = TypeExpr::Parenthesized(StdArc::new(expr));
    }
    expr
}

/// Build a `TypeExpr::IndexedAccess` leaf (`Foo["x"]`) — a non-object top-level /
/// published-operator root for the parity comparisons.
fn indexed_access_type_expr() -> TypeExpr {
    TypeExpr::IndexedAccess {
        object: StdArc::new(TypeExpr::named("Foo")),
        index: StdArc::new(TypeExpr::string_literal("x")),
    }
}

/// DEPTH regression: the object-surface / non-object-top node predicates walk an
/// `Alias` chain DEEPER than the former fixed depth cap (32) and answer IDENTICALLY
/// to the uncapped `TypeExpr` predicates on the equivalent `Parenthesized` chain.
///
/// MUTATION-PROOF: reinstating a `MAX_DEPTH = 32` cap cuts the node walk short of the
/// 40-deep terminal, so the node predicate flips (object-surface false instead of
/// true; non-object-top false instead of true) while the uncapped `TypeExpr`
/// predicate stays true — the `assert_eq!`s then FAIL. The visited-set termination
/// walks all 40 hops, so both sides agree.
#[test]
fn registry_object_surface_node_predicates_walk_deep_alias_chain_without_depth_cutoff() {
    use crate::semantic_query::{IndexKey, QueryError, SemanticNodeData};

    let project = open_host();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/p.ts").unwrap();
    let host = session.host();
    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let graph = ctx.project_type_store().semantic_graph();

    // 40 > the former 32 cap: the visited-set walk must follow ALL hops.
    const DEPTH: usize = 40;

    // Chain A: Alias^DEPTH -> Object (discriminates explicit-object-surface).
    let object_terminal = dispatch
        .lower_type_expr_in_scope_with_mode(
            "/p.ts",
            &one_prop_object("a"),
            ProjectionMode::Navigate,
        )
        .expect("object terminal lowers");
    let deep_object_node = deep_alias_chain(graph, object_terminal, DEPTH);
    let deep_object_expr = deep_paren_chain(one_prop_object("a"), DEPTH);

    assert_eq!(
        component_meta_registry_node_has_explicit_object_surface(ctx, deep_object_node),
        component_meta_registry_has_explicit_object_surface(&deep_object_expr),
        "explicit-object-surface NODE predicate must EQUAL the uncapped TypeExpr predicate on a \
         >32-deep alias chain (a reinstated MAX_DEPTH=32 cuts the node walk short and flips it)",
    );
    // Discriminating direction: the uncapped answer IS true (terminal at depth DEPTH
    // is an Object), so a depth-capped node walk (false) would diverge.
    assert!(
        component_meta_registry_node_has_explicit_object_surface(ctx, deep_object_node),
        "the deep alias chain DOES reach the object surface (visited-set walks all {DEPTH} hops)",
    );

    // Chain B: Alias^DEPTH -> IndexedAccess (discriminates non-object-top).
    let dummy = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let indexed_terminal = graph.intern_node(SemanticNodeData::IndexedAccess {
        object: dummy,
        index: IndexKey::String(StdArc::from("x")),
    });
    let deep_indexed_node = deep_alias_chain(graph, indexed_terminal, DEPTH);
    let deep_indexed_expr = deep_paren_chain(indexed_access_type_expr(), DEPTH);

    assert_eq!(
        component_meta_registry_node_has_non_object_top_level_surface(ctx, deep_indexed_node),
        component_meta_registry_has_non_object_top_level_surface(&deep_indexed_expr),
        "non-object-top-level NODE predicate must EQUAL the uncapped TypeExpr predicate on a \
         >32-deep alias chain (a reinstated MAX_DEPTH=32 cuts the node walk short and flips it)",
    );
    assert!(
        component_meta_registry_node_has_non_object_top_level_surface(ctx, deep_indexed_node),
        "the deep alias chain DOES reach the non-object IndexedAccess root (all {DEPTH} hops)",
    );
}

/// DEPTH regression: the second-pass reduction-context predicate (which drives the
/// `node_root_is_published_operator` field walk) walks an `Alias` chain DEEPER than
/// the former fixed depth cap (32) and answers IDENTICALLY to the uncapped `TypeExpr`
/// reduction context on the equivalent `Parenthesized` chain.
///
/// MUTATION-PROOF: reinstating a `MAX_DEPTH = 32` cap stops the node walk before the
/// 40-deep published-operator (`IndexedAccess`) root, so `node_root_is_published_operator`
/// returns false and the Navigate reduction context flips from `Published(Navigate)`
/// to `StructuralTransit(Navigate)` while the uncapped `TypeExpr` context stays
/// `Published(Navigate)` — the `assert_eq!`s then FAIL.
#[test]
fn node_reduction_context_walks_deep_alias_chain_without_depth_cutoff() {
    use crate::meta_resolve::materialize::{
        node_materialize_reduction_context, type_expr_materialize_reduction_context,
    };
    use crate::semantic_query::{
        IndexKey, ProjectionReductionContext, QueryError, SemanticNodeData,
    };

    let project = open_host();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/p.ts").unwrap();
    let host = session.host();
    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    let graph = ctx.project_type_store().semantic_graph();

    const DEPTH: usize = 40;

    // Alias^DEPTH -> IndexedAccess: a published-operator root reached only after all
    // DEPTH alias hops.
    let dummy = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let indexed_terminal = graph.intern_node(SemanticNodeData::IndexedAccess {
        object: dummy,
        index: IndexKey::String(StdArc::from("x")),
    });
    let deep_indexed_node = deep_alias_chain(graph, indexed_terminal, DEPTH);
    let deep_indexed_expr = deep_paren_chain(indexed_access_type_expr(), DEPTH);

    assert_eq!(
        node_materialize_reduction_context(ctx, deep_indexed_node, ProjectionMode::Navigate),
        type_expr_materialize_reduction_context(
            ctx,
            "/p.ts",
            &deep_indexed_expr,
            ProjectionMode::Navigate
        ),
        "node reduction context must EQUAL the uncapped TypeExpr reduction context on a >32-deep \
         alias chain (a reinstated MAX_DEPTH=32 flips Published(Navigate) to StructuralTransit)",
    );
    // Discriminating direction: the uncapped answer IS Published(Navigate) (the deep
    // root is a published IndexedAccess operator).
    assert_eq!(
        node_materialize_reduction_context(ctx, deep_indexed_node, ProjectionMode::Navigate),
        ProjectionReductionContext::published(ProjectionMode::Navigate),
        "the deep alias chain root IS a published operator ⇒ Published(Navigate)",
    );
}

/// The Navigate reduction-context MAPPED carrier decision branches on TYPED
/// node-domain state — the shape-engine fold's
/// `RaisedRootKind::Mapped { value_is_semantic_miss }` root class, derived
/// from `QueryError::Miss` through the shared sentinel authority — never on
/// the raised sentinel STRING:
///
/// - a builtin broad-modifier carrier (a mapped whose VALUE position is the
///   miss carrier) stays a carrier-stop ⇒ `StructuralTransit(Navigate)`, both
///   for the `TypeExpr`-start entry (whose value position re-enters as the
///   raised `Unknown { raw: "semanticMiss" }` leaf, classified once at the
///   node-domain boundary) AND for a directly-interned mapped node whose
///   mapper value is the TYPED `Opaque(QueryError::Miss)` — no sentinel
///   string exists anywhere on that second decision path;
/// - an author-visible mapped value (`string`) publishes ⇒
///   `Published(Navigate)` at the `TypeExpr`-start entry.
#[test]
fn mapped_miss_carrier_reduction_context_decides_on_typed_node_state() {
    use crate::meta_resolve::materialize::{
        node_materialize_reduction_context, type_expr_materialize_reduction_context,
    };
    use crate::semantic_query::{
        DeclIdentity, HashValue, MapperKey, MapperKind, OptionalityMod, ProjectionReductionContext,
        QueryError, ReadonlyMod, SemanticNodeData,
    };
    use verter_type_expr::MappedModifier;

    let project = open_host();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/p.ts").unwrap();
    let host = session.host();
    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let graph = ctx.project_type_store().semantic_graph();

    let mapped_with_value = |value: TypeExpr| TypeExpr::Mapped {
        parameter: "K".to_string(),
        source: StdArc::new(one_prop_object("a")),
        value: StdArc::new(value),
        optional: MappedModifier::None,
        readonly: MappedModifier::None,
        name_type: None,
    };

    // TypeExpr-start entry: the value position of a broad-modifier carrier
    // re-enters as the raised miss leaf ⇒ carrier-stop at Navigate depth.
    assert_eq!(
        type_expr_materialize_reduction_context(
            ctx,
            "/p.ts",
            &mapped_with_value(TypeExpr::Unknown {
                raw: "semanticMiss".to_string(),
            }),
            ProjectionMode::Navigate,
        ),
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
        "a mapped carrier whose value is the miss carrier must carrier-stop at Navigate depth",
    );
    // An author-visible mapped value publishes.
    assert_eq!(
        type_expr_materialize_reduction_context(
            ctx,
            "/p.ts",
            &mapped_with_value(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)),
            ProjectionMode::Navigate,
        ),
        ProjectionReductionContext::published(ProjectionMode::Navigate),
        "an author-visible mapped value must publish at Navigate depth",
    );

    // Node entry, TYPED origin: the mapper value is `Opaque(QueryError::Miss)` —
    // the decision derives from the typed error variant, and no sentinel string
    // exists anywhere on this path.
    let source = dispatch
        .lower_type_expr_in_scope_with_mode(
            "/p.ts",
            &one_prop_object("a"),
            ProjectionMode::Navigate,
        )
        .expect("object lowers");
    let param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: DeclIdentity {
            canonical_id: StdArc::from("/p.ts"),
            whole_hash: HashValue::default(),
            decl_name: StdArc::from("<mapper-param>"),
        },
        param_index: 0,
        constraint: None,
        default: None,
        display_name: StdArc::from("K"),
    });
    let miss_value = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let mapped_miss_node = graph.intern_node(SemanticNodeData::Mapped {
        source,
        mapper: MapperKey {
            parameter_node: param,
            key_space: source,
            value_expr: miss_value,
            optionality: OptionalityMod::Keep,
            readonly: ReadonlyMod::Keep,
            name_remap: None,
            kind: MapperKind::classify_value_expr(graph.as_ref(), miss_value, source, param),
        },
    });
    assert_eq!(
        node_materialize_reduction_context(ctx, mapped_miss_node, ProjectionMode::Navigate),
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
        "a mapped node whose mapper value is the TYPED Opaque(QueryError::Miss) carrier must \
         carrier-stop — the decision derives from the typed error, not a raised string",
    );
}

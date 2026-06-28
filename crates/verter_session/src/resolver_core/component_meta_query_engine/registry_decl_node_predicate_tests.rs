//! Node-domain registry predicate / reduction-context parity + carrier-contract
//! tests for `registry_decl` — the `#[path]`-attached `node_predicate_parity_tests`
//! submodule (extracted to keep `registry_decl.rs` under the file-size budget). It
//! pins the node-domain object-surface / published-operator predicates against the
//! `TypeExpr` predicates, the Mapped-sentinel boundary, the registry-publication
//! carrier contract, and the member-path None-branch raw-route facts.

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
            type_expr_materialize_reduction_context(&raised, ProjectionMode::Navigate),
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

/// F4 carrier-contract guard: `materialize_member_node_to_type_expr` publishes a
/// first-pass / stabilised member-surface node that is NOT a route/surface adapter's
/// admitted node (it can be a `Miss`/`Recursive`/`Tainted` outcome), so it MUST mint
/// the no-admission-claim `RegistryPublicationNode` carrier and route through
/// `materialize_registry_publication_node` — NEVER forge `AdmittedRouteProjectionNode`
/// (whose contract asserts a passed `materialized && expanded_surface` gate).
/// DISCRIMINATING: the pre-fix body minted `AdmittedRouteProjectionNode::new(node)`,
/// which the absence assertion rejects. Scoped to the one function via a
/// balanced-brace body extractor so the genuine route-admission mints elsewhere in
/// the file are not in scope.
#[test]
fn registry_publication_path_does_not_forge_route_admitted_carrier() {
    const SRC: &str = include_str!("registry_decl.rs");

    fn extract_fn_body(src: &str, fn_sig: &str) -> Option<String> {
        let start = src.find(fn_sig)?;
        let after = &src[start..];
        let open = after.find('{')?;
        let mut depth = 0usize;
        for (i, b) in after.bytes().enumerate().skip(open) {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(after[..=i].to_string());
                    }
                }
                _ => {}
            }
        }
        None
    }

    let body = extract_fn_body(SRC, "fn materialize_member_node_to_type_expr")
        .expect("materialize_member_node_to_type_expr is present in source");
    assert!(
        body.contains("RegistryPublicationNode"),
        "the registry publication path must mint the no-admission-claim \
         RegistryPublicationNode carrier; body=\n{body}",
    );
    assert!(
        !body.contains("AdmittedRouteProjectionNode"),
        "the registry publication path must NOT forge the route-admitted \
         AdmittedRouteProjectionNode carrier for an un-admitted member-surface node; \
         body=\n{body}",
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

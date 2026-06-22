//! Handle-capable consumer equivalence fixtures.
//!
//! Several component-meta consumers accept a start point as a
//! parser-produced `TypeExpr` today. Each such consumer is being made
//! HANDLE-CAPABLE: it grows an additive sibling arm that accepts an
//! ALREADY-LOWERED graph node (a `SemanticNodeId` / `HotTypeRef`) and
//! routes it through the SAME query-time dispatch the `TypeExpr` arm
//! reaches — read-compat, ONE resolver, never a reverse
//! materialise-then-re-lower bridge and never a second engine.
//!
//! These fixtures prove the equivalence directly: they lower a fixture
//! `TypeExpr` to a node through the single dispatch
//! (`lower_type_expr_in_scope_with_mode`, the same lowering the
//! `TypeExpr` arm performs internally), then drive BOTH arms and assert
//! the two public results are identical. The handle input is minted in
//! the test only — no production producer emits these carriers yet (the
//! structural lowerer stays dormant). Each fixture carries a negative
//! assertion so an implementer who routed the handle arm through a
//! different / wrong path (a materialise bridge, a dropped projection,
//! a skipped dispatch) fails it.
//!
//! Coverage:
//!  - Member-surface seam (`materialize_member_surface_{expr,node}`):
//!    the `TypeExpr` arm lowers `expr` then delegates to the node-core;
//!    the handle arm calls the node-core directly. Same surface.
//!  - Owner-collection seam (`owner_collection_surface_from_node`):
//!    reduces a body handle through the same node-core, producing the
//!    same surface the `TypeExpr` body yields once materialised.
//!  - Registry "stay symbolic" root classifier
//!    (`node_root_should_stay_symbolic`): the graph-native sibling of
//!    the `TypeExpr`-shape predicate classifies the equivalent node
//!    root identically.

use std::sync::Arc;

use verter_type_expr::TypeExpr;

use crate::meta::MetaProject;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ComponentMetaQueryEngine;
use crate::semantic_query::{ProjectionMode, SemanticNodeId};
use crate::types::HostConfig;
use crate::VerterHost;

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    MetaProject::new(host)
}

/// `TypeExpr::Ref { name, [] }` builder.
fn bare_ref(name: &str) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    }
}

/// Lower `expr` in `scope` (Navigate) through the single dispatch — the
/// SAME lowering the `TypeExpr` consumer arm performs internally — so
/// the handle arm receives a node that is path-identical to the
/// `TypeExpr` arm's lowered input.
fn lower_handle(host: &VerterHost, scope: &str, expr: &TypeExpr) -> SemanticNodeId {
    let dispatch = ProjectSemanticDispatch::new(host);
    dispatch
        .lower_type_expr_in_scope_with_mode(scope, expr, ProjectionMode::Navigate)
        .expect("fixture body must lower to a node in the prepared scope")
}

// ---------------------------------------------------------------------------
// Member-surface seam: TypeExpr arm == handle arm.
//
// `materialize_member_surface_expr(expr)` lowers `expr` then delegates
// to `materialize_member_surface_node(node)`. A consumer holding a
// settled body handle calls the node-core directly. Both reduce the
// SAME node through the SAME dispatch. The fixture body is an aliased
// generic object so the materialised surface has real members.
// ---------------------------------------------------------------------------
#[test]
fn member_surface_expr_and_node_arms_produce_equivalent_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            "export type Box<T> = { value: T; tag: string }\n\
             export type StrBox = Box<string>\n\
             // projected member pins the instantiated value type\n\
             export type StrBoxValue = StrBox['value']",
        )
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/types.ts").unwrap();
    let host = session.host();

    let expr = bare_ref("StrBox");

    let ((expr_arm, node_arm), _facts) = host.with_fact_tracer(|| {
        let mut engine = ComponentMetaQueryEngine::new(host);
        let expr_arm = engine.materialize_member_surface_expr("/types.ts", &expr, false);

        let node = lower_handle(host, "/types.ts", &expr);
        let node_arm = engine
            .materialize_member_surface_node("/types.ts", node, false)
            .expect("handle arm must produce a surface for the same lowered node");
        (expr_arm, node_arm)
    });

    assert_eq!(
        expr_arm, node_arm,
        "the member-surface handle arm (materialize_member_surface_node) MUST produce the \
         identical public surface as the TypeExpr arm (materialize_member_surface_expr) for the \
         node it lowers `expr` to — both route the SAME node through the SAME dispatch"
    );

    // NEGATIVE: the surface must be a real materialised Object with the
    // expected members — not the bare input `Ref` that a dropped /
    // skipped-dispatch handle arm would leave, and not an `Unknown`.
    let member_names: Vec<String> = match &node_arm {
        TypeExpr::Object(obj) => obj
            .properties
            .iter()
            .filter_map(|m| match m {
                verter_type_expr::ObjectMember::Property(p) => Some(p.name.clone()),
                _ => None,
            })
            .collect(),
        other => panic!(
            "handle arm must materialise the aliased generic body to an Object surface; got \
             {other:?} — a skipped-dispatch handle arm would leave the bare Ref"
        ),
    };
    assert!(
        member_names.iter().any(|n| n == "value") && member_names.iter().any(|n| n == "tag"),
        "the materialised surface must carry the instantiated members `value` + `tag`; got \
         {member_names:?}"
    );
    assert!(
        !matches!(node_arm, TypeExpr::Ref { .. }),
        "the handle arm must NOT leave the bare carrier Ref (that is the un-materialised input, \
         proving the dispatch never ran)"
    );

    // Pin the INSTANTIATED member VALUE through a projection: the
    // top-level member surface keeps member values shallow-by-default, so
    // the concrete `value: string` instantiation is pinned by reducing
    // `StrBox['value']` through the ShapeSubject path (both arms). A
    // handle arm that preserved keys but dropped the `Box<string>`
    // instantiation would leave `value` as the bare `T` and this
    // projection would NOT reduce to `string`.
    assert_shape_subject_reduces_to_primitive(
        host,
        "/types.ts",
        "StrBoxValue",
        verter_type_expr::PrimitiveName::String,
    );
}

// ---------------------------------------------------------------------------
// Owner-collection seam: the handle arm reduces a body handle through
// the same node-core and yields the same surface the materialised
// TypeExpr body would.
// ---------------------------------------------------------------------------
#[test]
fn owner_collection_node_arm_matches_materialized_body_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/owner.ts",
            "export type Pair<T> = { first: T; second: T }\n\
             export type NumPair = Pair<number>\n\
             // projected member pins the instantiated value type\n\
             export type NumPairFirst = NumPair['first']",
        )
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/owner.ts").unwrap();
    let host = session.host();

    let body_expr = bare_ref("NumPair");

    let ((expr_surface, node_surface), _facts) = host.with_fact_tracer(|| {
        let mut engine = ComponentMetaQueryEngine::new(host);
        // The TypeExpr baseline: the owner-collection body materialised
        // through the member-surface seam (what the registry walker does
        // with the body the `TypeExpr` arm returns).
        let expr_surface = engine.materialize_member_surface_expr("/owner.ts", &body_expr, false);

        // The handle arm: reduce the SAME body, lowered to a node,
        // through `owner_collection_surface_from_node`.
        let body_node = lower_handle(host, "/owner.ts", &body_expr);
        let node_surface = engine
            .owner_collection_surface_from_node("/owner.ts", body_node)
            .expect("owner-collection handle arm must produce a surface");
        (expr_surface, node_surface)
    });

    assert_eq!(
        expr_surface, node_surface,
        "owner_collection_surface_from_node (handle arm) MUST yield the identical surface the \
         materialised TypeExpr body yields — same node, same dispatch"
    );
    // The top-level surface keeps member values shallow-by-default; pin
    // the INSTANTIATED member value by reducing `NumPair['first']` through
    // the ShapeSubject path (both arms). A handle arm that preserved keys
    // but dropped the `Pair<number>` instantiation would NOT reduce
    // `first` to `number`.
    assert_shape_subject_reduces_to_primitive(
        host,
        "/owner.ts",
        "NumPairFirst",
        verter_type_expr::PrimitiveName::Number,
    );
    assert!(
        matches!(node_surface, TypeExpr::Object(_)),
        "owner-collection handle arm must materialise the body to an Object; got {node_surface:?}"
    );
}

// ---------------------------------------------------------------------------
// ShapeSubject seam: the TypeExpr-subject materialiser
// (`materialize_component_meta_type_expr_until_stable_full`, the
// `ShapeSubject::TypeExpr` path) and the graph-native member reducer
// (`reduce_member_value_graph_native_with_context`, the
// `ShapeSubject::MemberValueNode` / handle path) produce the same public
// shape for the same body. This locks the existing handle-native
// consumer's equivalence — the handle arm drives the reducer DIRECTLY
// on the lowered node, never re-lowering a materialised TypeExpr.
// ---------------------------------------------------------------------------
#[test]
fn shape_subject_type_expr_and_member_value_node_arms_agree() {
    use crate::meta_resolve::materialize::{
        materialize_component_meta_type_expr_until_stable,
        reduce_member_value_graph_native_with_context,
    };
    use crate::semantic_query::ProjectionReductionContext;

    let project = make_project();
    project
        .upsert_base(
            "/shape.ts",
            "export type Wrap<T> = { inner: T }\n\
             export type Members = Wrap<number>['inner']",
        )
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/shape.ts").unwrap();
    let host = session.host();

    // `Members` resolves to `number` via the indexed-access hop; both
    // the TypeExpr-subject materialiser and the node reducer must agree.
    let expr = bare_ref("Members");
    let mode = ProjectionMode::Expanded;

    let ((expr_shape, node_shape), _facts) = host.with_fact_tracer(|| {
        let mut engine = ComponentMetaQueryEngine::new(host);
        // ShapeSubject::TypeExpr arm.
        let expr_shape = materialize_component_meta_type_expr_until_stable(
            &expr,
            "/shape.ts",
            mode,
            &mut engine,
        );

        // ShapeSubject::MemberValueNode / handle arm: lower then reduce the
        // node DIRECTLY (no TypeExpr round-trip).
        let node = lower_handle(host, "/shape.ts", &expr);
        let ctx: &dyn crate::resolver_core::ResolverContext = host;
        let node_shape = reduce_member_value_graph_native_with_context(
            ctx,
            "/shape.ts",
            node,
            ProjectionReductionContext::published(mode),
        )
        .type_expr;
        (expr_shape, node_shape)
    });

    assert_eq!(
        expr_shape, node_shape,
        "the ShapeSubject::MemberValueNode handle arm (reduce_member_value_graph_native_with_context) \
         MUST produce the identical public shape as the ShapeSubject::TypeExpr arm \
         (materialize_component_meta_type_expr_until_stable) for the same body"
    );
    // NEGATIVE: the indexed-access hop must have reduced to the concrete
    // `number`, proving the dispatch ran on the handle (not a passthrough).
    assert!(
        matches!(
            node_shape,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        ),
        "the handle arm must reduce `Wrap<number>['inner']` to `number`; got {node_shape:?} — a \
         passthrough / dropped-dispatch handle arm would leave the indexed-access shell"
    );
}

/// Shared equivalence assertion for the ShapeSubject reduction path: the
/// `ShapeSubject::TypeExpr` materialiser
/// (`materialize_component_meta_type_expr_until_stable`) and the
/// `ShapeSubject::MemberValueNode` / handle reducer
/// (`reduce_member_value_graph_native_with_context`, fed the lowered
/// node) must yield the identical public shape. Returns the agreed
/// shape for fixture-specific negative assertions. This is the
/// whole-body reduction seam; a member-projecting body
/// (`Holder<number>['value']`) forces the construct under test
/// (type-param substitution, heritage args, value `typeof`) to reduce
/// to a concrete terminal so the negative assertion discriminates.
fn assert_shape_subject_arms_agree(host: &VerterHost, scope: &str, expr: &TypeExpr) -> TypeExpr {
    use crate::meta_resolve::materialize::{
        materialize_component_meta_type_expr_until_stable,
        reduce_member_value_graph_native_with_context,
    };
    use crate::semantic_query::ProjectionReductionContext;

    let mode = ProjectionMode::Expanded;
    let ((expr_arm, node_arm), _facts) = host.with_fact_tracer(|| {
        let mut engine = ComponentMetaQueryEngine::new(host);
        let expr_arm =
            materialize_component_meta_type_expr_until_stable(expr, scope, mode, &mut engine);
        let node = lower_handle(host, scope, expr);
        let ctx: &dyn crate::resolver_core::ResolverContext = host;
        let node_arm = reduce_member_value_graph_native_with_context(
            ctx,
            scope,
            node,
            ProjectionReductionContext::published(mode),
        )
        .type_expr;
        (expr_arm, node_arm)
    });
    assert_eq!(
        expr_arm, node_arm,
        "the ShapeSubject::MemberValueNode handle arm MUST produce the identical shape as the \
         ShapeSubject::TypeExpr arm for scope {scope}"
    );
    node_arm
}

/// Assert a member-projecting body reduces to the named primitive
/// through BOTH ShapeSubject arms.
fn assert_shape_subject_reduces_to_primitive(
    host: &VerterHost,
    scope: &str,
    alias: &str,
    expected: verter_type_expr::PrimitiveName,
) {
    let shape = assert_shape_subject_arms_agree(host, scope, &bare_ref(alias));
    assert!(
        matches!(&shape, TypeExpr::Primitive(p) if *p == expected),
        "`{alias}` must reduce to `{expected:?}` through both arms; got {shape:?}"
    );
}

// ---------------------------------------------------------------------------
// Type-param seam: a body whose surface depends on a generic type
// parameter's EXPLICIT ARG, its DEFAULT, and a CONSTRAINT must reduce
// identically through both arms. Each projects to a terminal so a handle
// arm that dropped/mishandled the type-param `constraint`/`default` node
// or the substitution would leave the bare `T` / a wrong type / a shell.
// ---------------------------------------------------------------------------
#[test]
fn type_param_constraint_default_and_substitution_reduce_equivalently_through_handle_arm() {
    use verter_type_expr::PrimitiveName;

    let project = make_project();
    project
        .upsert_base(
            "/tp.ts",
            "export type Holder<T = string> = { value: T }\n\
             // explicit arg: T substituted to number\n\
             export type AppliedValue = Holder<number>['value']\n\
             // DEFAULT: `Holder` with no arg uses T = string\n\
             export type DefaultValue = Holder['value']\n\
             // CONSTRAINT: a generic FUNCTION whose lowered `Function` node\n\
             // retains `type_parameters: [{ constraint: number }]`. `T` is\n\
             // UNUSED in params/return, so the ONLY reachable constraint is\n\
             // the `TypeParamDecl.constraint` on `Function.type_parameters`\n\
             // (no alternate binder TypeParam node to fall back to)\n\
             export function bounded<T extends number>(x: string): string { return x }",
        )
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/tp.ts").unwrap();
    let host = session.host();

    // Explicit arg: T -> number.
    assert_shape_subject_reduces_to_primitive(
        host,
        "/tp.ts",
        "AppliedValue",
        PrimitiveName::Number,
    );
    // DEFAULT: the unused-arg `Holder` resolves T to its default `string`.
    let default_shape = assert_shape_subject_arms_agree(host, "/tp.ts", &bare_ref("DefaultValue"));
    assert!(
        matches!(
            &default_shape,
            TypeExpr::Primitive(PrimitiveName::String)
                | TypeExpr::Literal(verter_type_expr::LiteralValue::String(_))
        ),
        "`Holder['value']` must reduce to the type-param DEFAULT `string` through both arms; got \
         {default_shape:?} — a handle arm that dropped the type-param default node would leave the \
         bare `T`"
    );
    // CONSTRAINT: structural — the lowered generic function `bounded<T
    // extends number>` MUST carry its type-param's `constraint` node on
    // its `Function.type_parameters`. An explicit-arg test does NOT
    // discriminate constraint handling (the concrete arg drives the
    // reduction either way), so we assert the constraint node is PRESENT
    // on the lowered function: a handle representation that dropped the
    // `TypeParamDecl.constraint` field would lose it here. We resolve
    // `typeof bounded` through the SAME dispatch the handle arm uses (no
    // materialize bridge) and assert the constraint resolves to `number`.
    let typeof_bounded = TypeExpr::TypeOf(verter_type_expr::ValueRef {
        path: vec!["bounded".to_string()],
        type_args: Vec::new(),
    });
    let bounded_node = lower_handle(host, "/tp.ts", &typeof_bounded);
    let constraint_node = find_type_param_constraint(host, bounded_node).unwrap_or_else(|| {
        panic!(
            "the lowered `typeof bounded` (a generic function `bounded<T extends number>`) MUST \
             carry its type-param `constraint` node — a handle representation that dropped the \
             `TypeParamDecl.constraint` field would lose it"
        )
    });
    assert!(
        matches!(
            crate::project_semantic_dispatch::node_data_for(host, constraint_node).as_deref(),
            Some(crate::semantic_query::SemanticNodeData::Primitive(
                crate::semantic_query::PrimitiveKind::Number
            ))
        ),
        "the `Bounded` type-param constraint must resolve to the bound `number`; got {:?}",
        crate::project_semantic_dispatch::node_data_for(host, constraint_node).as_deref()
    );
}

/// Find the `constraint` node on the lowered function value `root`'s
/// `Function.type_parameters[*]` (`TypeParamDecl.constraint`),
/// navigating `Object.call_signatures -> Function`. Returns the
/// constraint node id. Used to prove the `TypeParamDecl.constraint`
/// survives lowering (it would be `None` everywhere if constraint
/// handling were dropped).
fn find_type_param_constraint(host: &VerterHost, root: SemanticNodeId) -> Option<SemanticNodeId> {
    use crate::semantic_query::SemanticNodeData;
    // A function value lowers to an Object whose `call_signatures` hold
    // the generic `Function` node. We assert SPECIFICALLY on
    // `Function.type_parameters[*].constraint` (the `TypeParamDecl`
    // field) — NOT on a binder `TypeParam` node, which is populated
    // INDEPENDENTLY: if the `TypeParamDecl.constraint` population were
    // deleted, a binder-node fallback would still pass and the test
    // would not discriminate the field this seam carries.
    let object = crate::project_semantic_dispatch::node_data_for(host, root)?;
    let signatures: Vec<SemanticNodeId> = match object.as_ref() {
        SemanticNodeData::Object(view) => view.call_signatures.iter().copied().collect(),
        SemanticNodeData::Function { .. } => vec![root],
        SemanticNodeData::Alias(target) => return find_type_param_constraint(host, *target),
        _ => return None,
    };
    for sig in signatures {
        let data = crate::project_semantic_dispatch::node_data_for(host, sig)?;
        if let SemanticNodeData::Function {
            type_parameters, ..
        } = data.as_ref()
        {
            for tp in type_parameters.iter() {
                if let Some(c) = tp.constraint {
                    return Some(c);
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Class heritage seam: a class extending a generic base with a concrete
// type arg must surface the INHERITED member with the substituted type
// through both arms. A handle arm ignoring heritage args would leave the
// inherited member as `T` / unknown or miss it entirely.
// ---------------------------------------------------------------------------
#[test]
fn class_heritage_inherited_member_reduces_equivalently_through_handle_arm() {
    let project = make_project();
    project
        .upsert_base(
            "/heritage.ts",
            "export interface Base<T> { based: T }\n\
             export interface Derived extends Base<string> { own: number }\n\
             // project the inherited member so the heritage arg reduces to a terminal\n\
             export type InheritedBased = Derived['based']",
        )
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/heritage.ts").unwrap();
    let host = session.host();

    let shape = assert_shape_subject_arms_agree(host, "/heritage.ts", &bare_ref("InheritedBased"));

    // NEGATIVE: the inherited `based` member projected from `Derived`
    // must reduce to the heritage arg `string` through both arms — a
    // handle arm ignoring heritage args would leave the bare `T` / miss.
    assert!(
        matches!(
            shape,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "`Derived['based']` must reduce to the heritage arg `string`; got {shape:?} — a handle \
         arm ignoring heritage args would leave the bare `T` / a miss"
    );
}

// ---------------------------------------------------------------------------
// Value-decl seam: a `typeof`-over-a-value body must reduce to the
// value's inferred surface identically through both arms. `Shape` is
// `typeof config`, whose object literal infers `{ name: string;
// count: number }` — a handle arm that mishandled the value decl would
// not surface those members.
// ---------------------------------------------------------------------------
#[test]
fn value_decl_typeof_reduces_equivalently_through_handle_arm() {
    let project = make_project();
    project
        .upsert_base(
            "/value.ts",
            "export const config = { name: 'x', count: 1 }\n\
             // project a value-decl member so the typeof reduces to a terminal\n\
             export type CountType = (typeof config)['count']",
        )
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/value.ts").unwrap();
    let host = session.host();

    let shape = assert_shape_subject_arms_agree(host, "/value.ts", &bare_ref("CountType"));

    // NEGATIVE: the `(typeof config)['count']` projection must reduce to
    // `number` (the value's inferred member type) through both arms — a
    // handle arm that mishandled the value decl would not reach it.
    assert!(
        matches!(
            shape,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        ),
        "`(typeof config)['count']` must reduce to `number`; got {shape:?} — a handle arm that \
         mishandled the value decl / typeof would leave a shell or miss"
    );
}

// ---------------------------------------------------------------------------
// Registry "stay symbolic" root classifier: the graph-native predicate
// classifies the equivalent node root identically to the TypeExpr-shape
// predicate (Mapped / Conditional / IndexedAccess / TypeOf => symbolic;
// a plain object alias => not symbolic).
// ---------------------------------------------------------------------------
#[test]
fn node_root_should_stay_symbolic_matches_type_expr_predicate_for_every_root() {
    use crate::meta_resolve::exactness::{
        expr_root_should_stay_symbolic, node_root_should_stay_symbolic,
    };
    use crate::semantic_query::{
        IndexKey, MapperKey, MapperKind, OptionalityMod, PrimitiveKind, ReadonlyMod, ScopeId,
        SemanticNodeData, ValueRootKey,
    };
    use verter_type_expr::{LiteralValue, MappedModifier, PrimitiveName, ValueRef};

    let project = make_project();
    project
        .upsert_base("/sym.ts", "export type X = number")
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/sym.ts").unwrap();
    let host = session.host();
    let graph = host.project_type_store().semantic_graph();

    // Build the four DEFERRED-SHELL `SemanticNodeData` roots directly
    // (the dormant structural producer emits exactly these even where the
    // eager path would reduce/resolve) plus a non-symbolic Primitive, and
    // the matching `TypeExpr` shapes. Constructing the nodes directly is
    // what makes this discriminate every match arm of the graph-native
    // classifier: deleting `Mapped`/`Conditional`/`IndexedAccess`/`TypeOf`
    // from EITHER predicate flips the corresponding case and fails.
    let prim = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let tparam_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    let node_indexed = graph.intern_node(SemanticNodeData::IndexedAccess {
        object: tparam_node,
        index: IndexKey::String(Arc::from("a")),
    });
    let node_conditional = graph.intern_node(SemanticNodeData::Conditional {
        check: tparam_node,
        extends: prim,
        true_branch_ref: prim,
        false_branch_ref: prim,
        distributive: false,
    });
    let node_mapped = graph.intern_node(SemanticNodeData::Mapped {
        source: tparam_node,
        mapper: MapperKey {
            parameter_node: tparam_node,
            key_space: tparam_node,
            value_expr: prim,
            optionality: OptionalityMod::Keep,
            readonly: ReadonlyMod::Keep,
            name_remap: None,
            kind: MapperKind::Computed,
        },
    });
    let node_typeof = graph.intern_node(SemanticNodeData::new_typeof(
        ValueRootKey {
            scope: ScopeId {
                canonical_id: Arc::from("/sym.ts"),
                local_scope: None,
            },
            name: Arc::from("cfg"),
        },
        Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
        Arc::from(Vec::new().into_boxed_slice()),
    ));

    let tparam_expr = TypeExpr::Primitive(PrimitiveName::Number);
    let expr_indexed = TypeExpr::IndexedAccess {
        object: Arc::new(tparam_expr.clone()),
        index: Arc::new(TypeExpr::Literal(LiteralValue::String("a".to_string()))),
    };
    let expr_conditional = TypeExpr::Conditional {
        check: Arc::new(tparam_expr.clone()),
        extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
        true_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
        false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Boolean)),
    };
    let expr_mapped = TypeExpr::Mapped {
        parameter: "K".to_string(),
        source: Arc::new(TypeExpr::KeyOf(Arc::new(tparam_expr.clone()))),
        value: Arc::new(TypeExpr::Primitive(PrimitiveName::Number)),
        optional: MappedModifier::None,
        readonly: MappedModifier::None,
        name_type: None,
    };
    let expr_typeof = TypeExpr::TypeOf(ValueRef {
        path: vec!["cfg".to_string()],
        type_args: Vec::new(),
    });
    let expr_plain = TypeExpr::Primitive(PrimitiveName::Number);

    // (label, node, expr, expected) — every symbolic root kind + a
    // non-symbolic negative. The graph-native verdict on the node, the
    // TypeExpr verdict on the expr, and the expected boolean must all
    // agree per row.
    let cases: &[(&str, crate::semantic_query::SemanticNodeId, &TypeExpr, bool)] = &[
        ("IndexedAccess", node_indexed, &expr_indexed, true),
        ("Conditional", node_conditional, &expr_conditional, true),
        ("Mapped", node_mapped, &expr_mapped, true),
        ("TypeOf", node_typeof, &expr_typeof, true),
        ("non-symbolic", prim, &expr_plain, false),
    ];

    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    for (label, node, expr, expected) in cases {
        let expr_verdict = expr_root_should_stay_symbolic(expr);
        let node_verdict = node_root_should_stay_symbolic(ctx, *node);
        assert_eq!(
            expr_verdict, *expected,
            "the TypeExpr predicate for a {label} root must be {expected}"
        );
        assert_eq!(
            node_verdict, *expected,
            "the graph-native predicate for a {label} root must be {expected}"
        );
        // EQUIVALENCE: the two arms agree on every root kind.
        assert_eq!(
            node_verdict, expr_verdict,
            "the graph-native and TypeExpr stay-symbolic predicates MUST agree for a {label} root"
        );
    }
}

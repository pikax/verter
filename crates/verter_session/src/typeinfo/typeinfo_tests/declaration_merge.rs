//! Discriminating oracles for SAME-FILE TypeScript declaration merging.
//!
//! TypeScript merges multiple same-name declarations in one file:
//!   * Two `interface Foo` blocks UNION their members.
//!   * Same-name interface METHODS accumulate into an ordered overload group
//!     (NOT a single shadowed signature — the Contest-2 killer case).
//!   * Function overload sets surface every bodiless overload signature with
//!     the trailing implementation hidden.
//!
//! Each oracle is written to FAIL on the pre-merge tree (last-wins drops
//! earlier contributors / overloads collapse) and PASS once the `MergedDecl`
//! carrier + peer-merge reducer + overload-projection land. Negative controls
//! assert unrelated symbols do NOT accidentally merge.

use super::support::*;
use crate::VerterHost;
use verter_type_expr::{FunctionExpr, TypeExpr};

const PATH: &str = "/fixtures/declaration_merge.ts";

/// First-parameter primitive of an overload call signature.
fn first_param_primitive(f: &FunctionExpr) -> PrimitiveName {
    match &f
        .parameters
        .first()
        .expect("overload signature must have >=1 param")
        .ty
    {
        TypeExpr::Primitive(p) => *p,
        other => panic!("expected primitive first param, got {other:?}"),
    }
}

/// The ordered list of first-parameter primitives carried by a callable
/// member's type. A bare `Function` is a one-element overload group; an
/// intersection of functions is an ordered overload group of N — the canonical
/// structural encoding of an overloaded method. Anything else is a projection
/// bug (a shadowed single signature, or a non-callable).
fn overload_param_primitives(ty: &TypeExpr) -> Vec<PrimitiveName> {
    match ty {
        TypeExpr::Function(f) => vec![first_param_primitive(f)],
        TypeExpr::Intersection(arms) => arms
            .iter()
            .map(|arm| match arm {
                TypeExpr::Function(f) => first_param_primitive(f),
                other => panic!("expected function arm in overload group, got {other:?}"),
            })
            .collect(),
        other => panic!("expected callable member type, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 1. Two interface blocks UNION their members.
// ---------------------------------------------------------------------------

#[test]
fn same_file_interface_merge_unions_members() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export interface Foo { a: string }\nexport interface Foo { b: number }\n",
    );

    let (expr, record) = resolve_expr(&host, PATH, "Foo", &[], ProjectionMode::Expanded);
    let props = object_props(&expr);
    let names = prop_names(&props);

    // Positive: BOTH members survive the merge.
    assert!(
        names.contains(&"a"),
        "merged Foo must expose `a`; got {names:?}"
    );
    assert!(
        names.contains(&"b"),
        "merged Foo must expose `b`; got {names:?}"
    );
    // Negative: neither contributor is dropped (last-wins drops `a`).
    assert!(
        !names.is_empty() && names.contains(&"a") && names.contains(&"b"),
        "neither `a` nor `b` may be absent from the merged surface; got {names:?}"
    );
    assert_primitive(&props["a"].ty, PrimitiveName::String);
    assert_primitive(&props["b"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// 2. Adding a contributor invalidates the merged warm entry.
// ---------------------------------------------------------------------------

#[test]
fn same_file_interface_merge_invalidates_on_contributor_add() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export interface Foo { a: string }\nexport interface Foo { b: number }\n",
    );

    let (expr, _) = resolve_expr(&host, PATH, "Foo", &[], ProjectionMode::Expanded);
    let before = prop_names(&object_props(&expr))
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert!(
        before.contains(&"a".to_string()) && before.contains(&"b".to_string()),
        "warm merged surface must start with a,b; got {before:?}"
    );

    // Add a third contributor to the same file.
    upsert_ts(
        &host,
        PATH,
        "export interface Foo { a: string }\nexport interface Foo { b: number }\nexport interface Foo { c: boolean }\n",
    );

    let (expr, _) = resolve_expr(&host, PATH, "Foo", &[], ProjectionMode::Expanded);
    let props = object_props(&expr);
    let names = prop_names(&props);
    assert!(
        names.contains(&"a"),
        "after add: `a` must remain; got {names:?}"
    );
    assert!(
        names.contains(&"b"),
        "after add: `b` must remain; got {names:?}"
    );
    assert!(
        names.contains(&"c"),
        "after add: `c` must appear; got {names:?}"
    );
    assert_primitive(&props["c"].ty, PrimitiveName::Boolean);
}

// ---------------------------------------------------------------------------
// 3. Function overload group: bodiless overloads visible, impl hidden.
// ---------------------------------------------------------------------------

#[test]
fn same_file_function_overloads_surface_bodiless_signatures() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export function f(x: number): void;\nexport function f(x: string): void;\nexport function f(x: any): void {}\n",
    );

    let (expr, record) = evaluate_expr(&host, PATH, "typeof f", ProjectionMode::Expanded);
    let sigs = object_call_signatures(&expr);

    // The two bodiless overloads are visible; the implementation is hidden.
    assert_eq!(
        sigs.len(),
        2,
        "typeof f must expose exactly the TWO bodiless overloads (impl hidden); got {} signatures",
        sigs.len()
    );
    let params: Vec<PrimitiveName> = sigs.iter().map(first_param_primitive).collect();
    assert!(
        params.contains(&PrimitiveName::Number) && params.contains(&PrimitiveName::String),
        "overload params must be number and string; got {params:?}"
    );
    // Negative: not collapsed to one, not the impl `any` signature.
    assert!(
        !params.contains(&PrimitiveName::Any),
        "implementation `any` overload must be hidden; got {params:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// 4. Interface METHOD overload-merge (the Contest-2 killer).
// ---------------------------------------------------------------------------

#[test]
fn same_file_interface_method_merge_accumulates_overload_group() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export interface I { m(x: number): void }\nexport interface I { m(x: string): void }\n",
    );

    let (expr, record) = resolve_expr(&host, PATH, "I", &[], ProjectionMode::Expanded);
    let props = object_props(&expr);
    let member = props.get("m").unwrap_or_else(|| {
        panic!(
            "merged I must expose member `m`; got {:?}",
            prop_names(&props)
        )
    });

    let overloads = overload_param_primitives(&member.ty);
    // Peer-merge accumulates BOTH method signatures; shadow would keep one.
    assert_eq!(
        overloads.len(),
        2,
        "I.m must be an ordered overload group of 2 signatures, not a shadowed single; got {overloads:?}"
    );
    assert!(
        overloads.contains(&PrimitiveName::Number) && overloads.contains(&PrimitiveName::String),
        "the two I.m overloads must take number and string; got {overloads:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// 5. Negative controls — no accidental merge.
// ---------------------------------------------------------------------------

#[test]
fn unrelated_consts_do_not_merge() {
    let host = make_host_with_footprint();
    upsert_ts(&host, PATH, "export const p = 1;\nexport const q = 2;\n");

    let (p_expr, _) = evaluate_expr(&host, PATH, "typeof p", ProjectionMode::Expanded);
    let (q_expr, _) = evaluate_expr(&host, PATH, "typeof q", ProjectionMode::Expanded);
    assert_number_literal(&p_expr, 1.0);
    assert_number_literal(&q_expr, 2.0);
}

// ---------------------------------------------------------------------------
// 6. interface + class merge: the interface members augment the
//    class INSTANCE type. The merged type-side surface unions both — the class
//    value / static / ctor side is a separate value declaration and is
//    unaffected.
// ---------------------------------------------------------------------------

#[test]
fn same_file_interface_class_merge_unions_instance_members() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export interface Foo { a: string }\nexport class Foo { b: number = 1 }\n",
    );

    let (expr, record) = resolve_expr(&host, PATH, "Foo", &[], ProjectionMode::Expanded);
    let props = object_props(&expr);
    let names = prop_names(&props);

    // The interface member `a` augments the class instance type alongside `b`.
    // Last-wins keeps only the class's `b` (drops the interface `a`).
    assert!(
        names.contains(&"a"),
        "interface+class merge must expose the interface member `a`; got {names:?}"
    );
    assert!(
        names.contains(&"b"),
        "interface+class merge must expose the class instance member `b`; got {names:?}"
    );
    assert_primitive(&props["a"].ty, PrimitiveName::String);
    assert_primitive(&props["b"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// 7. A merged interface whose contributor carries `extends` heritage MUST
//    surface the inherited members alongside every contributor's own members.
//    `interface X extends Base { a }` + `interface X { b }` ⇒ {base, a, b}.
//    The pre-fix reducer dropped the heritage `Ref(Base)` arm (it kept only
//    direct `Object` arms), so `base` went missing.
// ---------------------------------------------------------------------------

#[test]
fn merged_interface_preserves_extends_heritage_members() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export interface Base { base: string }\n\
         export interface X extends Base { a: number }\n\
         export interface X { b: boolean }\n",
    );

    // The merged surface (heritage ∪ own) is delivered by the empty-path
    // Shallow terminal synthesiser (the shallow-surface walker), which is the
    // path that resolves `extends` heritage references into a flat member
    // surface. A bare `resolve_named_symbol` + raise keeps `extends Base` as an
    // un-inlined `Ref(Base)` arm (true for a single interface too).
    let expr = shallow_surface_expr(&host, PATH, "X");
    let props = object_props(&expr);
    let names = prop_names(&props);

    // Inherited member from `extends Base` survives the merge (pre-fix the
    // merged-decl reducer dropped the heritage `Ref(Base)` arm → `base` gone).
    assert!(
        names.contains(&"base"),
        "merged X must inherit `base` from `extends Base`; got {names:?}"
    );
    // Own members of both contributors survive.
    assert!(
        names.contains(&"a"),
        "merged X must expose own member `a`; got {names:?}"
    );
    assert!(
        names.contains(&"b"),
        "merged X must expose own member `b`; got {names:?}"
    );
    assert_primitive(&props["base"].ty, PrimitiveName::String);
    assert_primitive(&props["a"].ty, PrimitiveName::Number);
    assert_primitive(&props["b"].ty, PrimitiveName::Boolean);
}

// ---------------------------------------------------------------------------
// 7b. Own members SHADOW inherited members of the same name (TS heritage
//     precedence), and overload accumulation across merged contributors is
//     unaffected by the heritage-preservation fix.
// ---------------------------------------------------------------------------

#[test]
fn merged_interface_own_member_shadows_heritage() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export interface Base { shared: string }\n\
         export interface X extends Base { shared: number }\n\
         export interface X { extra: boolean }\n",
    );

    let expr = shallow_surface_expr(&host, PATH, "X");
    let props = object_props(&expr);
    let names = prop_names(&props);

    assert!(
        names.contains(&"extra"),
        "own `extra` must survive; got {names:?}"
    );
    assert!(
        names.contains(&"shared"),
        "merged `shared` must be present; got {names:?}"
    );
    // Own `shared: number` shadows the inherited `shared: string`.
    assert_primitive(&props["shared"].ty, PrimitiveName::Number);
}

/// Resolve `name` to its declaration CARRIER and run the empty-path
/// `ProjectPath` terminal-surface synthesiser in the EXPANDED demand — the
/// role-consuming Expanded surface route (heritage arms materialise and the
/// own-body-shadows-heritage merge fires), projecting the merged one-level
/// surface to a [`TypeExpr`]. The Expanded sibling of
/// [`shallow_surface_expr`].
fn expanded_surface_expr(host: &VerterHost, canonical_id: &str, name: &str) -> TypeExpr {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        ProjectionReductionContext, ResolveDeclKey, ScopeId, SemanticQueryApi, SemanticQueryKey,
    };

    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);

    let base = match dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from(canonical_id),
            local_scope: None,
        },
        name: Arc::from(name),
    })) {
        crate::semantic_query::QueryResult::Value(crate::semantic_query::SemanticQueryOutput {
            value: node,
            ..
        }) => node,
        other => panic!("{name} must resolve to a declaration carrier: {other:?}"),
    };

    let terminal = match dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(Vec::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Expanded),
    }) {
        crate::semantic_query::QueryResult::Value(crate::semantic_query::SemanticQueryOutput {
            value: node,
            ..
        }) => node,
        other => panic!("empty-path Expanded projection of {name} failed: {other:?}"),
    };
    dispatch
        .materialize_output_type_expr_for_test(terminal)
        .unwrap_or_else(|| panic!("{name} empty-path Expanded surface must project to TypeExpr"))
}

/// Own members shadow inherited members through the EXPANDED route when the
/// heritage base is GENERIC (`extends Base<string>` would materialise
/// through `Instantiate` instead of staying a lazy declaration anchor):
///
/// 1. the Expanded-projected merged contributor keeps its heritage arm a
///    REFERENCE carrier (per-arm heritage discrimination — never a blanket
///    own-body stamp that eagerly materialises the heritage into an
///    `Object` the peer-merge reducer then mis-buckets as OWN surface);
/// 2. projecting the conflicting member through the shared walk applies
///    own-body-shadows-heritage — `X['shared']` is the own `number`, not
///    the inherited `string`.
#[test]
fn merged_interface_generic_heritage_own_member_shadows_expanded() {
    use crate::semantic_query::SemanticNodeData;

    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export interface Base<T> { shared: T; inherited: boolean }\n\
         export interface X extends Base<string> { shared: number }\n\
         export interface X { extra: boolean }\n",
    );

    let node = host
        .resolve_named_symbol(PATH, "X", &[], Some(ProjectionMode::Expanded))
        .expect("merged X must resolve Expanded");
    let graph = host.project_type_store().semantic_graph();

    // 1. Carrier topology: the merged carrier survives, and the extending
    //    contributor's heritage arm is still a REFERENCE carrier (the
    //    peer-merge reducer classifies heritage arms by topology — an
    //    eagerly-materialised heritage `Object` is indistinguishable from
    //    the own body and silently steals member precedence).
    let contributors = match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::MergedDecl { contributors }) => contributors.clone(),
        other => panic!("merged X must stay a MergedDecl carrier, got {other:?}"),
    };
    assert_eq!(contributors.len(), 2, "two same-name contributors");
    let heritage_arm_is_reference =
        contributors.iter().any(
            |contributor| match graph.node_data(*contributor).as_deref() {
                Some(SemanticNodeData::Intersection(arms)) => arms.iter().any(|arm| {
                    matches!(
                        graph.node_data(*arm).as_deref(),
                        Some(
                            SemanticNodeData::InstantiationRef { .. }
                                | SemanticNodeData::DeclRef { .. }
                        )
                    )
                }),
                _ => false,
            },
        );
    assert!(
        heritage_arm_is_reference,
        "the `extends Base<string>` heritage arm must stay a reference \
         carrier in the Expanded-projected contributor — an eagerly \
         materialised heritage Object loses own-body-shadows-heritage"
    );

    // 2. Role-consuming surface merge: the complete surface reader applies
    //    own-body-shadows-heritage over the preserved heritage carrier —
    //    the own `shared: number` wins, the non-conflicting inherited
    //    member surfaces, and the second contributor's member survives.
    let surface = shallow_surface_expr(&host, PATH, "X");
    let props = object_props(&surface);
    let names = prop_names(&props);
    assert!(
        names.contains(&"inherited"),
        "the non-conflicting inherited member must surface; got {names:?}"
    );
    assert!(
        names.contains(&"extra"),
        "the second contributor's own member must survive the merge; got {names:?}"
    );
    assert_primitive(&props["shared"].ty, PrimitiveName::Number);
}

/// The same own-body-shadows-heritage precedence when the heritage clause
/// is a MAPPER BUILTIN over a CLOSED base (`extends Partial<Base>` — not an
/// L1 object-filter utility, no open argument): the merged contributor's
/// heritage arm must stay a REFERENCE carrier through the deferred-arm
/// projection — an eagerly materialised `Partial<Base>` becomes an `Object`
/// the peer-merge reducer mis-buckets as OWN surface, and first-contributor
/// precedence then INVERTS own-body-shadows-heritage (the Partial-ized
/// inherited `shared` steals the own `shared: number`).
#[test]
fn merged_interface_mapper_builtin_heritage_own_member_shadows_expanded() {
    use crate::semantic_query::SemanticNodeData;

    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export interface Base { shared: string; fromBase: boolean }\n\
         export interface X extends Partial<Base> { shared: number }\n\
         export interface X { other: string }\n",
    );

    let node = host
        .resolve_named_symbol(PATH, "X", &[], Some(ProjectionMode::Expanded))
        .expect("merged X must resolve Expanded");
    let graph = host.project_type_store().semantic_graph();

    // 1. Carrier topology: the merged carrier survives and the extending
    //    contributor's `extends Partial<Base>` arm is still a REFERENCE
    //    carrier (the peer-merge reducer classifies heritage arms by
    //    topology — a materialised heritage Object is indistinguishable
    //    from the own body and silently steals member precedence).
    let contributors = match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::MergedDecl { contributors }) => contributors.clone(),
        other => panic!("merged X must stay a MergedDecl carrier, got {other:?}"),
    };
    assert_eq!(contributors.len(), 2, "two same-name contributors");
    let heritage_arm_is_reference =
        contributors.iter().any(
            |contributor| match graph.node_data(*contributor).as_deref() {
                Some(SemanticNodeData::Intersection(arms)) => arms.iter().any(|arm| {
                    matches!(
                        graph.node_data(*arm).as_deref(),
                        Some(
                            SemanticNodeData::InstantiationRef { .. }
                                | SemanticNodeData::DeclRef { .. }
                        )
                    )
                }),
                _ => false,
            },
        );
    assert!(
        heritage_arm_is_reference,
        "the `extends Partial<Base>` heritage arm must stay a reference \
         carrier in the projected contributor — an eagerly materialised \
         mapper-builtin heritage Object inverts own-body-shadows-heritage"
    );

    // 2. Surface precedence: the own `shared: number` wins over the
    //    Partial-ized inherited `shared`; the non-conflicting heritage
    //    member still surfaces (optional-ized by Partial — heritage is
    //    NOT dropped); the second contributor's own member survives.
    let surface = shallow_surface_expr(&host, PATH, "X");
    let props = object_props(&surface);
    let names = prop_names(&props);
    assert!(
        names.contains(&"fromBase"),
        "the non-conflicting Partial-ized heritage member must still \
         surface; got {names:?}"
    );
    assert!(
        props["fromBase"].optional,
        "the heritage member reached through Partial must surface \
         optional-ized"
    );
    assert!(
        names.contains(&"other"),
        "the second contributor's own member must survive the merge; got {names:?}"
    );
    // Negative: `shared` must NOT be the heritage type (the Partial-ized
    // `string`) — that is the precedence inversion this test pins.
    assert!(
        !matches!(
            props["shared"].ty,
            TypeExpr::Primitive(PrimitiveName::String)
        ),
        "own-body-shadows-heritage inverted: `shared` resolved to the \
         heritage `string` instead of the own `number`"
    );
    assert_primitive(&props["shared"].ty, PrimitiveName::Number);
}

/// The SKELETON twin of
/// [`merged_interface_mapper_builtin_heritage_own_member_shadows_expanded`]:
/// Published(Skeleton) (projection mode 4) is wire-reachable, and the
/// merged-decl deferred heritage arm (`extends Partial<Base>`) projects under
/// `StructuralTransit(Skeleton)` carrying the `Heritage` merge role. The
/// mapper-builtin `Partial<Base>` MUST stay a reference carrier there — an
/// eagerly materialised `Partial<Base>` Object is mis-bucketed by the
/// topology-driven peer-merge reducer as OWN surface, and first-contributor
/// precedence then INVERTS own-body-shadows-heritage (the Partial-ized
/// heritage `shared?: string` steals the own `shared: number`).
///
/// Discriminating: pre-fix the builtin gate exempts EVERY `Skeleton` demand
/// from the transit carrier-stop (an exemption sized for the two neutral-role
/// Skeleton probe executors), so the `Heritage`-role deferred arm materialises
/// and `shared` resolves to the heritage `string`; post-fix the exemption is
/// keyed off the `Heritage` merge role so the deferred heritage arm
/// carrier-stops and `shared` resolves to the own `number`.
#[test]
fn merged_interface_mapper_builtin_heritage_own_member_shadows_skeleton() {
    use crate::semantic_query::SemanticNodeData;

    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export interface Base { shared: string; fromBase: boolean }\n\
         export interface X extends Partial<Base> { shared: number }\n\
         export interface X { other: string }\n",
    );

    // Resolve X under Published(Skeleton): the deferred merged-decl heritage
    // arm projects under StructuralTransit(Skeleton), where the builtin gate
    // decides whether `Partial<Base>` carrier-stops or materialises.
    let node = host
        .resolve_named_symbol(PATH, "X", &[], Some(ProjectionMode::Skeleton))
        .expect("merged X must resolve under Skeleton");
    let graph = host.project_type_store().semantic_graph();

    // Carrier topology (the direct structural discriminator, mirroring the
    // Expanded twin): the merged carrier survives and the extending
    // contributor's `extends Partial<Base>` arm is still a REFERENCE carrier.
    // A materialised heritage `Object` / `Mapped` there is mis-bucketed as OWN
    // surface by the topology-driven peer-merge reducer and silently steals
    // member precedence (own-body-shadows-heritage inverts).
    let contributors = match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::MergedDecl { contributors }) => contributors.clone(),
        other => panic!("merged X must stay a MergedDecl carrier under Skeleton, got {other:?}"),
    };
    assert_eq!(contributors.len(), 2, "two same-name contributors");
    let heritage_arm_is_reference =
        contributors.iter().any(
            |contributor| match graph.node_data(*contributor).as_deref() {
                Some(SemanticNodeData::Intersection(arms)) => arms.iter().any(|arm| {
                    matches!(
                        graph.node_data(*arm).as_deref(),
                        Some(
                            SemanticNodeData::InstantiationRef { .. }
                                | SemanticNodeData::DeclRef { .. }
                        )
                    )
                }),
                _ => false,
            },
        );
    assert!(
        heritage_arm_is_reference,
        "the `extends Partial<Base>` heritage arm must stay a reference carrier in the \
         Published(Skeleton)-projected contributor — the builtin gate's blanket Skeleton \
         exemption materialised the mapper-builtin heritage `Partial<Base>` mid-transit, \
         which the topology-driven peer-merge reducer mis-buckets as OWN surface and \
         inverts own-body-shadows-heritage"
    );
}

/// The same own-body-shadows-heritage precedence through the EXPANDED
/// surface route for a SINGLE (non-merged) interface with a GENERIC heritage
/// base: the `extends Base<string>` reference arm is HERITAGE relative to
/// `X`, so the own `shared: number` shadows the inherited `shared: string`.
#[test]
fn single_interface_generic_heritage_own_member_shadows_expanded() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export interface Base<T> { shared: T; inherited: boolean }\n\
         export interface X extends Base<string> { shared: number }\n",
    );

    let expr = expanded_surface_expr(&host, PATH, "X");
    let props = object_props(&expr);
    let names = prop_names(&props);

    assert!(
        names.contains(&"inherited"),
        "the non-conflicting inherited member must surface; got {names:?}"
    );
    assert_primitive(&props["shared"].ty, PrimitiveName::Number);
}

#[test]
fn distinct_interface_names_stay_distinct() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        PATH,
        "export interface A { a: string }\nexport interface B { b: number }\n",
    );

    let (a_expr, _) = resolve_expr(&host, PATH, "A", &[], ProjectionMode::Expanded);
    let (b_expr, _) = resolve_expr(&host, PATH, "B", &[], ProjectionMode::Expanded);
    assert_eq!(prop_names(&object_props(&a_expr)), vec!["a"]);
    assert_eq!(prop_names(&object_props(&b_expr)), vec!["b"]);
}

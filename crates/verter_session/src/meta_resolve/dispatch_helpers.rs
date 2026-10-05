//! Dispatch-direct surface helpers + surface bridge helpers.
//!
//! These helpers resolve a root or surface expression through the shared
//! `ProjectSemanticDispatch` path. They sit next to their `meta_resolve`
//! callers so each callsite stays a one-liner over the shared resolver.
//!
//! They fall into two structural categories:
//!
//! Class A helpers resolve an arbitrary expression through
//! `dispatch.execute_read(ProjectPath { lowered, [], mode })` after
//! caller-side lowering. The accepted result NODE is gated NODE-DOMAIN
//! (raised-shape facts: drop results still carrying deferred shells or
//! semantic-miss markers) and materialised to its single publication
//! `TypeExpr` ONCE at the registered surface sink (the demand-bound
//! publication adapters in `component_meta_query_engine::surface`).
//!
//! The surface bridge helpers thread the caller's `ResolverContext` through
//! dispatch and compose the surviving `pub(crate)` cycle-protected dispatch
//! helpers (`dispatch_projected_surface_with_node`,
//! `dispatch_routed_expr_surface_node`, etc.) plus the surface→expr /
//! surface→shape raises. Dispatch is the sole resolution authority on these
//! paths.

use crate::resolver_core::ResolverContext;
use std::sync::Arc;

/// Extract the OUTER utility/identifier name from a route-bearing
/// `TypeExpr` and return `true` iff the owner scope shadows that name
/// with a userland declaration.
///
/// Background: the route extractors recognise
/// `Pick<…>` / `Omit<…>` / `Foo['a']['b']` syntactically. They do NOT
/// consult the owner scope. The "user shadowing wins" rule for the
/// route fast-path is: if the owner's same-file scope declares a
/// userland type whose name collides with the OUTER utility (e.g.
/// `type Pick<T, _K> = T`), the registry route MUST be suppressed so
/// the dispatch's standard `ResolveDecl` path resolves the userland
/// declaration instead.
///
/// The route's `root_symbol` is the route's INNER root identity (for
/// `Pick<Foo, K>` that is `Foo`, for `Foo['a']` that is `Foo`). The
/// shadow check MUST run on the OUTER identifier the userland alias
/// would shadow:
///
/// - `Pick<Foo, K>` → outer = `"Pick"` (the wrapping utility).
/// - `Omit<Foo, K>` → outer = `"Omit"`.
/// - `Foo['a']['b']` → outer = `"Foo"` (the indexed-access chain has
///   no wrapping utility; the chain root IS the outer identifier).
///
/// Returns `true` iff the extracted outer name appears in the
/// owner-scope shadow set per
/// [`ScopeShadowing::is_shadowing_lib`](crate::resolver_core::scope_shadowing::ScopeShadowing::is_shadowing_lib).
///
/// `Parenthesized` wrappers are stripped at every layer. Refs whose
/// shape does not match a known route pattern return `false`
/// (no shadowing — the registry route extractor would also have
/// returned `None`, so the filter would still drop the entry; the
/// helper's return value is therefore irrelevant outside the
/// registry-recognised shape set).
fn route_outer_utility_is_shadowed(
    expr: &verter_type_expr::TypeExpr,
    shadowing: &crate::resolver_core::scope_shadowing::ScopeShadowing,
) -> bool {
    use verter_type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => route_outer_utility_is_shadowed(inner, shadowing),
        // Utility-route shapes — the outer utility name (`Pick`,
        // `Omit`, …) is the wrapping `Ref`'s name. The route
        // extractor only matches when the type-argument arity is
        // exactly 2, but the shadow check is independent of that —
        // any Ref whose name shadows the lib utility suppresses the
        // route regardless of args.
        TypeExpr::Ref { name, .. } => shadowing.is_shadowing_lib(name.as_ref()),
        // Indexed-access chain — descend to the chain root. The
        // shadow check applies to the root identifier (the chain has
        // no wrapping utility name).
        TypeExpr::IndexedAccess { object, .. } => {
            route_outer_utility_is_shadowed(object, shadowing)
        }
        // Any other shape — the route extractors return `None` for
        // these; the shadow check is moot.
        _ => false,
    }
}

/// Class A surface projection through the shared dispatch.
///
/// The body has TWO paths:
///   1. Registry-route fast path for indexed-access / utility shapes
///      (`Button['ui']`, `Pick<Foo, K>`). This routes through the
///      Class D route helpers (`project_route_surface_expr` /
///      `lower_and_project_to_expanded`) via an engine instance so route
///      projection stays correct.
///   2. Generic ProjectPath dispatch for arbitrary expressions —
///      direct Expanded-mode `ProjectPath` query (the `Instantiate`
///      equivalent being `Instantiate { base, args: [], context:
///      InstantiateContext { projection_reduction, resolve_env_hash } }`
///      with `context.projection_reduction.mode = Expanded`), raised to a
///      `TypeExpr` and filtered for a fully-expanded surface.
///
/// Returns `Some(projected)` only when the projection produced a
/// fully-expanded surface (no deferred `KeyOf` / `IndexedAccess` /
/// `Mapped` / `TypeOf` / `Conditional` shells).
pub(crate) fn project_expr_class_a_via_dispatch(
    ctx: &dyn ResolverContext<crate::resolver_core::HostCapabilities>,
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<
        '_,
        crate::resolver_core::HostCapabilities,
    >,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> Option<verter_type_expr::TypeExpr> {
    // Resolve via the node-domain Class-A sibling (registry route fast-path +
    // terminal), materialising ONCE at the surface sink — the engine-less
    // counterpart of `project_expr_class_a_node_via_dispatch_threaded`.
    crate::resolver_core::project_class_a_published(ctx, dispatch, scope_canonical_id, expr)
}

/// Node-domain Class-A projection: returns the admitted route/surface NODE
/// instead of a materialised `TypeExpr`.
///
/// Applies the SAME scope-shadowing gate, the SAME registry route fast-path, and
/// the SAME primary/fallback order as the materialising
/// [`project_expr_class_a_via_dispatch`], but composes the node-domain bridge
/// counterparts ([`project_route_surface_node_via_host_threaded`] /
/// [`lower_and_project_to_expanded_node_via_host_threaded`]) and the node tail
/// [`crate::resolver_core::project_class_a_terminal_node`]. None of these
/// materialises: the admitted node is published ONCE downstream at the registry /
/// surface sink (e.g. via [`crate::resolver_core::project_class_a_published`]), so
/// the registry member-path / refine consumers compute their reject/accept facts
/// off the projected node WITHOUT re-lowering a materialised leaf.
pub(crate) fn project_expr_class_a_node_via_dispatch_threaded<'ctx>(
    ctx: &'ctx dyn ResolverContext<crate::resolver_core::HostCapabilities>,
    dispatch: &'ctx crate::project_semantic_dispatch::ProjectSemanticDispatch<
        'ctx,
        crate::resolver_core::HostCapabilities,
    >,
    mut engine: Option<&mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>>,
    scope_canonical_id: &str,
    scope_owner: verter_type_expr::TopLevelOwnerId,
    expr: &verter_type_expr::TypeExpr,
) -> Option<crate::resolver_core::AdmittedRouteProjectionNode> {
    use crate::resolver_core::{
        component_meta_registry::{
            component_meta_registry_public_indexed_access_route,
            component_meta_registry_public_utility_route,
        },
        ComponentMetaQueryEngine,
    };

    // r15/F11 scope-shadowing gate — IDENTICAL to the `TypeExpr` form: a userland
    // `type Pick`/`Omit`/chain-root shadow suppresses the registry fast-path so the
    // bare-name walk resolves the userland declaration.
    let shadowing = match engine.as_deref_mut() {
        Some(e) => e.scope_shadowing_for_scope(scope_canonical_id, scope_owner),
        None => std::sync::Arc::new(
            crate::resolver_core::scope_shadowing::ScopeShadowing::from_host_scope(
                ctx,
                scope_canonical_id,
                scope_owner,
            ),
        ),
    };
    let route = component_meta_registry_public_indexed_access_route(expr)
        .or_else(|| component_meta_registry_public_utility_route(expr))
        .filter(|_| !route_outer_utility_is_shadowed(expr, &shadowing));
    if let Some((root_symbol, route)) = route {
        let mut transient_engine: Option<ComponentMetaQueryEngine<'_>> = None;
        let engine_ref: &mut ComponentMetaQueryEngine<'_> = match engine {
            Some(e) => e,
            None => transient_engine.insert(ComponentMetaQueryEngine::new(ctx, dispatch)),
        };
        if let Some(projected) = project_route_surface_node_via_host_threaded(
            engine_ref,
            scope_canonical_id,
            scope_owner,
            &root_symbol,
            &route,
        ) {
            return Some(projected);
        }
        if let Some(solved) = lower_and_project_to_expanded_node_via_host_threaded(
            engine_ref,
            scope_canonical_id,
            scope_owner,
            expr,
        ) {
            return Some(solved);
        }
    }
    crate::resolver_core::project_class_a_terminal_node(
        dispatch,
        scope_canonical_id,
        scope_owner,
        expr,
    )
}

/// decompose an IndexedAccess chain over literal-string
/// indices into `(base_expr, path_segments)` so the dispatch helper can
/// route through `ProjectPath { base, path, Expanded }` per CLAUDE.md
/// "Macro Type Traversal Rule".
///
/// Returns `(expr, &[])` when the input is not a string-indexed
/// IndexedAccess chain — caller falls back to lowering `expr` whole.
/// Walks Parenthesized wrappers transparently. Stops decomposition at
/// the first non-string-literal index (returns the partial chain as
/// path with the partial-chain root as base).
/// Decompose a string-literal-indexed access chain
/// (`Root['a']['b']['c']`) into its `(base, [a, b, c])` path-precise
/// form. The base is the innermost non-`IndexedAccess` carrier (a `Ref`,
/// a generic instantiation, etc.) and the path is the ordered list of
/// string-literal index hops. A non-string-literal index stops the
/// descent (the whole expression becomes the base, empty path).
///
/// Shared by the transit-shallow Class-A projector and the Vue macro
/// surface adapter so the deep-indexed-access macro type argument
/// (`defineProps<DeepConfig['ui']['header']>()`) walks the SAME
/// path-precise `ProjectPath` both paths use — intermediate hops in
/// `Navigate`, terminal hop in the caller's mode (one engine).
pub(crate) fn decompose_indexed_access_chain(
    expr: &verter_type_expr::TypeExpr,
) -> (
    &verter_type_expr::TypeExpr,
    Arc<[crate::semantic_query::PathSegment]>,
) {
    use crate::semantic_query::{IndexKey, PathSegment};
    use verter_type_expr::{LiteralValue, TypeExpr};

    fn descend<'a>(expr: &'a TypeExpr, path: &mut Vec<PathSegment>) -> &'a TypeExpr {
        match expr {
            TypeExpr::Parenthesized(inner) => descend(inner, path),
            TypeExpr::IndexedAccess { object, index } => match index.as_ref() {
                TypeExpr::Literal(LiteralValue::String(member)) => {
                    let inner_base = descend(object, path);
                    path.push(PathSegment::Index(IndexKey::String(Arc::from(
                        member.as_str(),
                    ))));
                    inner_base
                }
                _ => expr,
            },
            other => other,
        }
    }

    let mut path: Vec<PathSegment> = Vec::new();
    let base = descend(expr, &mut path);
    (base, Arc::from(path.into_boxed_slice()))
}

// Class B helpers (dispatch-only surface projection) resolve a root
// symbol's surface through the shared dispatch surface projector. They
// have regressed in the past on transitive heritage chains and
// barrel-routed declarations, so the shared walker (the merge / heritage
// / Omit composition) is the place to fix any compound-root gap.

// Generic-`Ref` instantiation for component-meta type resolution goes through
// the shared dispatch lowering (`lower_type_expr_in_scope*` → `Instantiate`).
// The route-key leaf stabiliser lowers a generic `Ref` through that same shared
// dispatch path; there is no engine-side single-scope generic-`Ref` materialise
// helper.

// =============================================================================
// Class B surface bridge helpers — these thread `query_engine.ctx` through dispatch.
//
// The threaded `_threaded(engine, …)` variants are the production
// callsite shape (engine threaded through caller). The `_via_host_threaded`
// variants below are the canonical entrypoints.
// =============================================================================

// ===========================================================================
// Node-returning route projection adapters.
//
// These wrappers return the admitted `AdmittedRouteProjectionNode` (never a
// `TypeExpr`): the node-domain Class-A dispatch
// (`project_expr_class_a_node_via_dispatch_threaded`) stabilises on interned
// `RaisedShapeKey` identity with NO per-iteration materialisation, and the
// sole publication materialisation happens ONCE, downstream, at the surface
// sink. Each wrapper carries the same budget guard as the resolver entry it
// adapts.
// ===========================================================================

/// Node-domain empty-terminal `Expanded` projection: returns the admitted route
/// node (no materialisation).
pub(crate) fn lower_and_project_to_expanded_node_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    scope_owner: verter_type_expr::TopLevelOwnerId,
    expr: &verter_type_expr::TypeExpr,
) -> Option<crate::resolver_core::AdmittedRouteProjectionNode> {
    if engine.projection_op_budget_exhausted() {
        return None;
    }
    crate::resolver_core::lower_and_project_to_expanded_node(
        engine.dispatch,
        scope_canonical_id,
        scope_owner,
        expr,
    )
}

/// Node-domain registry-route projection: returns the admitted registry-route
/// node (no materialisation).
pub(crate) fn project_route_surface_node_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    scope_owner: verter_type_expr::TopLevelOwnerId,
    root_symbol: &str,
    route: &crate::resolver_core::RouteDemand,
) -> Option<crate::resolver_core::AdmittedRouteProjectionNode> {
    if engine.projection_op_budget_exhausted() {
        return None;
    }
    engine.dispatch_routed_expr_surface_node(scope_canonical_id, scope_owner, root_symbol, route)
}

// ===========================================================================
// Arg-preserving authored use-site recovery (Q10).
// ===========================================================================

/// Recover the authored USE-SITE body slot of a surface member's VALUE for
/// arg-preserving shallow publication: the declaring declaration's prepared
/// member-value [`TypeBodySlot`](verter_type_expr::locators::TypeBodySlot),
/// whose deref through the one shared dispatch replays the authored generic
/// instantiation (`message: MessageBase<string>`) WITH its type arguments —
/// the existing `Instantiate` query re-derives the substitution on demand.
/// The returned slot is a content-free CARRIER: publication never executes
/// the instantiation and never serialises a graph node.
///
/// Applies ONLY to an ARGUMENT-BEARING named-reference value head (a lossy
/// argument-less `Ref` publication would destroy the substitution there);
/// every other shape returns `None` so callers keep their existing
/// publication source. Recovery fails CLOSED (`None`) unless ALL hold:
///
/// - the member records a declaring file (`declaration_origin`) — synthetic
///   / multi-origin members (union common-members, mapped-produced) don't;
/// - EXACTLY ONE file-scope type declaration in that file declares the
///   member name in its OWN syntactic member headers (header-level shallow
///   inventory — no body lowering; ambiguity fails closed);
/// - the declaring declaration is NON-GENERIC (a generic declaring surface
///   replays UNSUBSTITUTED — the slot would be dishonest);
/// - the honesty verification passes: the slot raises (memoized
///   `Navigate` structural transit — one member annotation, never a body
///   expansion) to the SAME resolved instantiation head — equal base
///   [`DeclIdentity`](crate::semantic_query::DeclIdentity) AND equal
///   interned argument nodes — as the observed `value_node`. This closes
///   the inline-shadow class (an inline-authored member colliding with a
///   same-named declared member in the same file can never adopt the wrong
///   slot).
pub(crate) fn arg_preserving_member_use_site_slot<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_, C>,
    member_key: &crate::semantic_query::PropertyKey,
    declaration_origin: Option<&str>,
    value_node: crate::semantic_query::SemanticNodeId,
) -> Option<verter_type_expr::locators::TypeBodySlot> {
    // Gate: the observed value head must be an argument-bearing named
    // reference (the class the argument-less `Ref` publication is lossy
    // for). One bounded node-domain peek — no dispatch.
    let observed = resolved_instantiation_head(dispatch, value_node)?;

    let origin = declaration_origin?;
    let declaring_owner = observed.0.owner;
    let bundle = dispatch.ctx.prepared_decl_bundle(origin)?;
    let owner_scope = bundle.owner_scope(declaring_owner)?;
    // The UNIQUE file-scope type declaration whose OWN syntactic member
    // headers declare this member name (heritage contributes nothing to
    // `type_member_headers`, so a heritage-reached member resolves against
    // its true declaring contributor's file).
    let mut declaring: Option<&str> = None;
    for name in owner_scope.scope_type_names.iter().map(String::as_str) {
        let declares_member = dispatch
            .ctx
            .prepared_type_decl_return_only(origin, declaring_owner, name)
            .is_some_and(|prepared| prepared.member_index.contains_key(member_key));
        if !declares_member {
            continue;
        }
        if declaring.is_some() {
            // Two same-file declarers — ambiguous, fail closed.
            return None;
        }
        declaring = Some(name);
    }
    let declaring = declaring?;
    // Substitution-honesty gate: a generic declaring declaration's member
    // slot replays UNSUBSTITUTED (`MessageBase<T>`, not the instantiated
    // value) — fail closed.
    let prepared =
        dispatch
            .ctx
            .prepared_type_decl_return_only(origin, declaring_owner, declaring)?;
    if !prepared.type_parameters.is_empty() {
        return None;
    }
    let slot = prepared.member_index.get(member_key)?.ty.clone();

    // Honesty verification: the candidate slot must raise to the SAME
    // resolved instantiation as the observed value — equal base identity,
    // equal interned argument nodes. One memoized Navigate-transit raise of
    // one member annotation (never a body expansion, never an Instantiate
    // execution).
    let raised = dispatch
        .raise_authored_locator_to_hot(
            &verter_type_expr::locators::AuthoredBodyLocator::DeclBody(slot.clone()),
            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                crate::semantic_query::ProjectionMode::Navigate,
            ),
        )
        .at_optional_boundary()?;
    let authored = resolved_instantiation_head(dispatch, raised.node())?;
    (authored == observed).then_some(slot)
}

/// The alias-peeled, carrier-head-resolved INSTANTIATION identity of a node:
/// `Some((base, args))` when the head is an argument-bearing named reference
/// (`InstantiationRef`, or a `BareRef` / `ImportType` carrier still holding
/// authored type arguments that head-resolves to one through the shared
/// carrier-preserving normalization — name-to-declaration routing only,
/// never body expansion). `None` for every other shape.
fn resolved_instantiation_head<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_, C>,
    node: crate::semantic_query::SemanticNodeId,
) -> Option<(
    crate::semantic_query::DeclIdentity,
    std::sync::Arc<[crate::semantic_query::SemanticNodeId]>,
)> {
    use crate::semantic_query::SemanticNodeData;

    let mut current = node;
    // Bounded: alias chains are short; the cap only guards pathological
    // graph shapes.
    for _ in 0..16 {
        let data = crate::project_semantic_dispatch::node_data_for(dispatch.graph(), current)?;
        match &*data {
            SemanticNodeData::Alias(inner) => current = *inner,
            SemanticNodeData::InstantiationRef { base, args } => {
                return (!args.is_empty()).then(|| (base.clone(), std::sync::Arc::clone(args)));
            }
            SemanticNodeData::BareRef(_) | SemanticNodeData::ImportType(_)
                if !data.carrier_type_args().is_empty() =>
            {
                drop(data);
                let resolved = dispatch.resolve_carrier_subject_node(
                    current,
                    crate::semantic_query::ProjectionReductionContext::published(
                        crate::semantic_query::ProjectionMode::Navigate,
                    ),
                );
                if resolved == current {
                    return None;
                }
                current = resolved;
            }
            _ => return None,
        }
    }
    None
}

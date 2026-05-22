//! dispatch-direct surface helpers + bridge helpers.
//!
//! domains 1+2 of the meta_resolve.rs split.
//!
//! dispatch-direct surface helpers.
//!
//! The trampolines on `ComponentMetaQueryEngine` are slated for
//! retirement in 5g. migrates Class A and Class B callers off
//! the engine helpers. The two helpers below are the dispatch-direct
//! equivalents of the trampoline bodies, placed next to the meta_resolve
//! callers so each migrated callsite stays a one-liner.
//!
//! Class A migrates to `dispatch.execute_to_type_expr(ProjectPath{
//! lowered, [], mode })` after caller-side lowering, with the same
//! expanded-surface filter the trampoline applied (drops results that
//! still carry deferred shells or semantic-miss markers).
//!
//! Class B migrates to `dispatch.execute_to_type_expr(Instantiate{
//! base: bare_name_decl_identity, args: [], body_mode: Expanded })` —
//! the trampoline went through `project_type_surface` which itself
//! lowered to `Instantiate { args: [], body_mode: Expanded }` per
//! `build.rs`'s utility router; the Class B helper inlines that path.
//!
//! bridge helpers (post engine-method deletion).
//!
//! The 18 external callsites that 5m migrated onto these wrappers continue
//! to call them, but their bodies no longer dispatch through the deleted
//! `ComponentMetaQueryEngine` resolver methods. Each helper inlines the
//! same trampoline body the deprecated method had — a thin composition of
//! the engine's surviving `pub(crate)` cycle-protected dispatch helpers
//! (`dispatch_projected_surface`, `cached_prepared_root_surface`,
//! `project_routed_expr_surface_expr`, `project_direct_utility_surface_shape`,
//! etc.) plus the surface→expr / surface→shape raises.

use crate::resolver_core::ResolverContext;
use crate::types::ProjectionMode;
use std::sync::Arc;

/// Class A surface projection — dispatch-equivalent
/// of `ComponentMetaQueryEngine::project_expr_surface_expr`.
///
/// The trampoline's body has TWO paths:
///   1. Registry-route fast path for indexed-access / utility shapes
///      (`Button['ui']`, `Pick<Foo, K>`). This routes through the
///      Class D route helpers (`project_route_surface_expr` /
///      `lower_and_project_to_expanded`); we call them via a
///      transient engine instance so route projection stays correct
///      after callsite migration.
///   2. Generic ProjectPath dispatch for arbitrary expressions —
///      direct `Instantiate { args: [], body_mode: Expanded }`-shaped
///      `ProjectPath` query, raised to a `TypeExpr` and filtered for
///      a fully-expanded surface.
///
/// Returns `Some(projected)` only when the projection produced a
/// fully-expanded surface (no deferred `KeyOf` / `IndexedAccess` /
/// `Mapped` / `TypeOf` / `Conditional` shells). This matches the
/// trampoline's post-filter so Class A parity is preserved.
pub(crate) fn project_expr_class_a_via_dispatch(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> Option<verter_type_expr::TypeExpr> {
    project_expr_class_a_via_dispatch_threaded(ctx, None, scope_canonical_id, expr)
}

/// Engine-threaded variant of [`project_expr_class_a_via_dispatch`].
///
/// When `engine` is `Some(...)`, the route fast-path uses the
/// caller's engine instance so engine-local fuse / scope-payload /
/// request-local cache state persists across callsites that share an
/// engine. This matters for utility shapes like `Partial<T>` whose
/// optionality propagation is observed via the engine's prepared-decl
/// fixed-point. When `engine` is `None`, a transient engine is
/// created (suitable for top-level entry points without a caller
/// engine).
///
/// The engine route-fast-path is retained because
/// `engine.project_route_surface_expr` exercises engine-local
/// resolution paths (re-export chains, prepared-decl fallbacks) that
/// the dispatch's `lower_type_expr_in_scope` does not subsume —
/// removing it would cause stack overflows on realistic
/// indexed-access / utility shapes (e.g., `*_keeps_imported_*`
/// member-path test family). The engine method itself remains a
/// trampoline (already routes through dispatch), so the
/// fast-path remains semantically aligned with dispatch.
pub(crate) fn project_expr_class_a_via_dispatch_threaded<'ctx>(
    ctx: &'ctx dyn ResolverContext,
    engine: Option<&mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>>,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> Option<verter_type_expr::TypeExpr> {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::resolver_core::{
        component_meta_registry::{
            component_meta_registry_public_indexed_access_route,
            component_meta_registry_public_utility_route,
        },
        type_expr_contains_semantic_miss, type_expr_is_expanded_surface, ComponentMetaQueryEngine,
    };
    use crate::semantic_query::{PathSegment, QueryResult, SemanticQueryKey};

    // registry-route fast path via caller's engine (or a
    // transient engine when caller doesn't pass one). The Class D
    // route helpers (`project_route_surface_expr`,
    // `lower_and_project_to_expanded`) exercise engine-local
    // re-export and prepared-decl resolution paths that dispatch's
    // generic `lower_type_expr_in_scope` does not inherit verbatim.
    //
    // r15/F11 — scope-shadowing gate. The TypeExpr
    // route extractors recognise `Pick<…>` / `Omit<…>` syntactically;
    // they do not consult the owner scope. When the SFC's same-file
    // scope already declares a userland `type Pick<T,_K> = T`
    // (ScopeShadowing::is_shadowing_lib returns true), the registry
    // fast-path MUST be suppressed so the bare-name walk below
    // resolves `Pick` to the userland declaration via dispatch's
    // standard `ResolveDecl` path — preserving the "user shadowing
    // wins" rule across BOTH lowering entry points. Constructed once
    // per call from the same prepared-decl bundle the dispatch path
    // consumes, so the two paths observe identical shadow sets.
    let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::from_host_scope(
        ctx,
        scope_canonical_id,
    );
    let route = component_meta_registry_public_indexed_access_route(expr)
        .or_else(|| component_meta_registry_public_utility_route(expr))
        .filter(|(root_symbol, _)| !shadowing.is_shadowing_lib(root_symbol));
    if let Some((root_symbol, route)) = route {
        let mut transient_engine: Option<ComponentMetaQueryEngine<'_>> = None;
        let engine_ref: &mut ComponentMetaQueryEngine<'_> = match engine {
            Some(e) => e,
            None => transient_engine.insert(ComponentMetaQueryEngine::new(ctx)),
        };
        // route engine.project_route_surface_expr
        // and engine.lower_and_project_to_expanded through the bridge
        // helpers so the §5.14.1 pre-flight gate sees zero external
        // engine-method callers. The bridges' bodies remain
        // engine-method consumers for the migration window per
        // §5.13a.2's "the engine continues to serve the route fast-
        // path until 5l atomically deletes engine + bridges".
        if let Some(projected) = project_route_surface_expr_via_host_threaded(
            engine_ref,
            scope_canonical_id,
            &root_symbol,
            &route,
        ) {
            return Some(projected);
        }
        if let Some(solved) =
            lower_and_project_to_expanded_via_host_threaded(engine_ref, scope_canonical_id, expr)
        {
            return Some(solved);
        }
    }

    // Block 6.i Commit AX — every lowering site explicitly states
    // its mode (the implicit-Expanded wrapper has been retired).
    //
    // Empty path: lowering the whole expr is the carrier hop that
    // feeds the empty-terminal `ProjectPath { ..., Expanded }`. The
    // walker's `expand_empty_path_terminal` does NOT have a generic
    // `InstantiationRef` arm (the catch-all returns the node
    // unchanged), so a `Navigate` carrier would prevent the
    // expanded-surface filter downstream from observing an
    // Object/Intersection. The lowering mode therefore stays
    // `Expanded` here; the audit-leak fix lands at the macro-shape
    // publication boundary in a follow-up commit once the Shallow
    // walker grows InstantiationRef / deferred-Mapped enumeration
    // support (see Commit AX STOP report).
    //
    // Non-empty path: the base is an intermediate hop, and PathWalker
    // handles `InstantiationRef` per-hop. `Navigate` is correct here
    // — operator reductions inside the base body do NOT fire at the
    // lowering site, only at the terminal hop (CLAUDE.md "Macro Type
    // Traversal Rule" — walking `A['c']['full']['bar']` navigates
    // intermediate hops and expands only the terminal requested
    // projection).
    let (base_expr, path_segments) = decompose_indexed_access_chain(expr);
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let (base, project_path) = if path_segments.is_empty() {
        let base = dispatch.lower_type_expr_in_scope_with_mode(
            scope_canonical_id,
            expr,
            ProjectionMode::Expanded,
        )?;
        (
            base,
            Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        )
    } else {
        let base = dispatch.lower_type_expr_in_scope_with_mode(
            scope_canonical_id,
            base_expr,
            ProjectionMode::Navigate,
        )?;
        (base, path_segments)
    };
    let read = dispatch.execute_to_type_expr(&SemanticQueryKey::ProjectPath {
        base,
        path: project_path,
        mode: ProjectionMode::Expanded,
    });
    let projected = match read.value {
        QueryResult::Value(expr) => expr,
        QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
    };
    (!type_expr_contains_semantic_miss(&projected) && type_expr_is_expanded_surface(&projected))
        .then_some(projected)
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
fn decompose_indexed_access_chain(
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

/// Class A shape variant — dispatch-direct equivalent
/// of `ComponentMetaQueryEngine::project_expr_surface_shape`.
///
/// Returns the projection's `ExpandedObjectShape` when it has at least
/// one property or call signature (matching the trampoline's
/// shape-has-surface filter).
pub(crate) fn project_expr_class_a_shape_via_dispatch(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
    project_expr_class_a_shape_via_dispatch_threaded(ctx, None, scope_canonical_id, expr)
}

/// Engine-threaded variant of
/// [`project_expr_class_a_shape_via_dispatch`].
pub(crate) fn project_expr_class_a_shape_via_dispatch_threaded<'ctx>(
    ctx: &'ctx dyn ResolverContext,
    engine: Option<&mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>>,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
    let projected =
        project_expr_class_a_via_dispatch_threaded(ctx, engine, scope_canonical_id, expr)?;
    let shape = verter_semantic::analysis::type_expand::type_expr_to_object_shape(&projected);
    (!shape.properties.is_empty() || !shape.call_signatures.is_empty()).then_some(shape)
}

// Class B helpers were prototyped during 5d but caused regressions
// in transitive heritage chains and barrel-routed declarations
// because their dispatch-only path bypasses the engine's
// prepared-decl fallback (`cached_prepared_root_surface`). The
// trampoline's `project_type_surface` body is dispatch-first then
// prepared-decl-second; threading the prepared-decl path through
// dispatch atomically is a change. Class B callsite
// migration deferred to 5g per CLAUDE.md fix-quality discipline.

/// Class D — Pick route-target via dispatch's `execute_pick`
///.
///
/// Resolves `symbol_name` to a base `SemanticNodeId` (via Class A
/// lowering on a bare `Ref` of the symbol), then dispatches
/// `Pick<base, key_set>` through the dispatch's `execute_pick` helper
/// (which routes through `build_builtin_utility` Pick arm at
/// `build.rs:870`).
///
/// Returns `Some(reduced)` when the dispatch produces a non-Opaque
/// projection. Caller pattern (per the engine's
/// `project_route_surface_expr` Pick fallback): use the result as a
/// route surface; fall back to the raw registry-candidate path on miss.
pub(crate) fn pick_via_dispatch_pick_helper(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    symbol_name: &str,
    members: &[String],
) -> Option<verter_type_expr::TypeExpr> {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{ProjectionMode, QueryResult};

    let dispatch = ProjectSemanticDispatch::new(query_engine.ctx());
    let symbol_ref = verter_type_expr::TypeExpr::Ref {
        name: Arc::from(symbol_name),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    };
    // Block 6.i Commit AX — bare-Ref base for the Pick builtin is an
    // intermediate hop; the Pick result is the terminal demand.
    let base = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        &symbol_ref,
        ProjectionMode::Navigate,
    )?;

    let members_arc: Vec<Arc<str>> = members.iter().map(|s| Arc::from(s.as_str())).collect();
    let result = dispatch.execute_pick(base, &members_arc, ProjectionMode::Expanded);
    let node = match result {
        QueryResult::Value(id) => id,
        QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
    };
    dispatch.raise_node_to_type_expr(node)
}

/// Class D — generic-Ref instantiation via dispatch.
///
/// Dispatch-equivalent of
/// `ComponentMetaQueryEngine::instantiate_local_generic_ref`. The
/// engine method matched a `TypeExpr::Ref { name, type_arguments }`
/// with non-empty type_arguments, resolved the declaration, gated
/// against package-backed targets, and applied the prepared-decl's
/// type-parameter substitutions to produce the instantiated body.
///
/// The dispatch path goes through `lower_type_expr_in_scope` which
/// routes a generic `Ref` through
/// `SemanticQueryKey::Instantiate { base, args, body_mode: Expanded }`
/// internally — the dispatcher's `build_instantiate` performs the
/// same substitution logic (`build_default_type_param_substitutions`
/// + `apply_type_param_substitutions`) the engine method called.
///
/// Returns `Some(reduced)` only when:
/// - `expr` is a generic `Ref` (else returns `None`, matching the
///   engine method's bail-on-non-Ref / bail-on-empty-args),
/// - the dispatch lowering produced a node distinct from the carrier
///   `Opaque(Miss)` shell (the dispatcher's miss sentinel for
///   unresolved decl / package-backed / substitution-failure cases),
/// - the raised body differs from the input expression (matches the
///   engine method's "no change ⇒ caller falls back" semantics).
pub(crate) fn instantiate_local_generic_ref_via_dispatch(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> Option<verter_type_expr::TypeExpr> {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use verter_type_expr::TypeExpr;

    // Engine-method parity: bail when `expr` is not a generic `Ref`.
    let TypeExpr::Ref { type_arguments, .. } = expr else {
        return None;
    };
    if type_arguments.is_empty() {
        return None;
    }

    let dispatch = ProjectSemanticDispatch::new(ctx);
    // Block 6.i Commit AX — generic-Ref instantiation publishes its
    // raised body as the result; the caller reads the raised TypeExpr
    // directly without a path-walking follow-up. Lower at Expanded so
    // the body materialises in one step.
    let lowered = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        expr,
        ProjectionMode::Expanded,
    )?;
    let raised = dispatch.raise_node_to_type_expr(lowered)?;
    // Engine-method parity: callers use `.unwrap_or_else(|| original.clone())`,
    // so a no-op (raised == expr) must surface as `None` to preserve the
    // fallback path. A miss-shaped raise (Unknown/Opaque) likewise surfaces
    // as `None` — mirrors the engine method's `prepared_type_decl?` /
    // `build_default_type_param_substitutions(...)?` early-return rules.
    if raised == *expr {
        return None;
    }
    Some(raised)
}

// =============================================================================
// Class B bridge helpers — Class B engine methods are deleted; these bridges thread `query_engine.ctx` through dispatch.
//
// The threaded `_threaded(engine, …)` variants are the production
// callsite shape (engine threaded through caller). The non-threaded sync
// variants (`project_type_surface_expr_via_host`,
// `project_type_surface_shape_via_host`,
// `project_prepared_type_surface_shape_via_host`) had no caller — neither
// in production nor in tests — and were removed in the
// clippy cleanup. The threaded `_via_host_threaded` variants below are
// the canonical entrypoints; one (`_prepared_type_surface_expr_via_host_threaded`)
// is gated `#[cfg(test)]` because tests are its only consumer.
// =============================================================================

pub(crate) fn project_type_surface_expr_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    symbol_name: &str,
) -> Option<verter_type_expr::TypeExpr> {
    use crate::resolver_core::projected_surface_to_type_expr;
    if engine.projection_op_budget_exhausted() {
        return None;
    }
    let surface = engine
        .dispatch_projected_surface(scope_canonical_id, symbol_name)
        .or_else(|| engine.cached_prepared_root_surface(scope_canonical_id, symbol_name))?;
    projected_surface_to_type_expr(&surface)
}

pub(crate) fn project_type_surface_shape_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    symbol_name: &str,
) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
    use crate::resolver_core::projected_surface_to_expanded_shape;
    if engine.projection_op_budget_exhausted() {
        return None;
    }
    let surface = engine
        .dispatch_projected_surface(scope_canonical_id, symbol_name)
        .or_else(|| engine.cached_prepared_root_surface(scope_canonical_id, symbol_name))?;
    Some(projected_surface_to_expanded_shape(&surface))
}

#[cfg(test)]
pub(crate) fn project_prepared_type_surface_expr_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    symbol_name: &str,
) -> Option<verter_type_expr::TypeExpr> {
    use crate::resolver_core::projected_surface_to_type_expr;
    let surface = engine.cached_prepared_root_surface(scope_canonical_id, symbol_name)?;
    projected_surface_to_type_expr(&surface)
}

pub(crate) fn project_prepared_type_surface_shape_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    symbol_name: &str,
) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
    use crate::resolver_core::projected_surface_to_expanded_shape;
    let surface = engine.cached_prepared_root_surface(scope_canonical_id, symbol_name)?;
    Some(projected_surface_to_expanded_shape(&surface))
}

pub(crate) fn project_expr_surface_shape_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::resolver_core::component_meta_registry::{
        component_meta_registry_public_indexed_access_route,
        component_meta_registry_public_utility_route,
    };
    use crate::resolver_core::{
        projected_surface_from_semantic_node, projected_surface_to_expanded_shape,
    };
    use crate::semantic_query::{
        PathSegment, ProjectionMode, QueryResult, SemanticQueryApi, SemanticQueryKey,
    };

    if engine.projection_op_budget_exhausted() {
        return None;
    }
    if let Some((root_symbol, route)) = component_meta_registry_public_indexed_access_route(expr)
        .or_else(|| component_meta_registry_public_utility_route(expr))
    {
        if let Some(projected) =
            engine.project_routed_expr_surface_expr(scope_canonical_id, &root_symbol, &route)
        {
            return Some(
                verter_semantic::analysis::type_expand::type_expr_to_object_shape(&projected),
            );
        }
    }
    if let Some(shape) = engine.project_direct_utility_surface_shape(scope_canonical_id, expr) {
        return Some(shape);
    }
    let ctx = engine.ctx();
    let dispatch = ProjectSemanticDispatch::new(ctx);
    // Block 6.i Commit AX — intermediate-base lowering is Navigate;
    // the terminal `ProjectPath { .., Shallow }` carries the demand.
    let base = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        expr,
        ProjectionMode::Navigate,
    )?;
    let QueryResult::Value(node) = dispatch.execute(SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        mode: ProjectionMode::Shallow,
    }) else {
        return None;
    };
    let surface = projected_surface_from_semantic_node(ctx, node)?;
    let shape = projected_surface_to_expanded_shape(&surface);
    (!shape.properties.is_empty() || !shape.call_signatures.is_empty()).then_some(shape)
}

pub(crate) fn project_route_surface_expr_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    root_symbol: &str,
    route: &crate::resolver_core::RouteDemand,
) -> Option<verter_type_expr::TypeExpr> {
    if engine.projection_op_budget_exhausted() {
        return None;
    }
    engine.project_routed_expr_surface_expr(scope_canonical_id, root_symbol, route)
}

pub(crate) fn lower_and_project_to_expanded_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> Option<verter_type_expr::TypeExpr> {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::resolver_core::{type_expr_contains_semantic_miss, type_expr_is_expanded_surface};
    use crate::semantic_query::{PathSegment, ProjectionMode, QueryResult, SemanticQueryKey};

    if engine.projection_op_budget_exhausted() {
        return None;
    }
    let ctx = engine.ctx();
    let dispatch = ProjectSemanticDispatch::new(ctx);
    // Block 6.i Commit AX — empty-terminal `ProjectPath { ..,
    // Expanded }` requires the base to be a structural surface that
    // `expand_empty_path_terminal` can walk. `Navigate` would freeze
    // a generic carrier at `InstantiationRef` (catch-all), so the
    // expanded-surface filter downstream would reject. Keep
    // `Expanded` lowering until the walker grows InstantiationRef
    // support (Commit AX STOP report).
    let base = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        expr,
        ProjectionMode::Expanded,
    )?;
    let read = dispatch.execute_to_type_expr(&SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        mode: ProjectionMode::Expanded,
    });
    let reduced = match read.value {
        QueryResult::Value(expr) => expr,
        QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
    };
    (!type_expr_contains_semantic_miss(&reduced)
        && type_expr_is_expanded_surface(&reduced)
        && reduced != *expr)
        .then_some(reduced)
}

/// direct equivalent of the deleted
/// `ComponentMetaQueryEngine::project_expr_surface_expr` engine method:
/// the registry-route fast-path falls through to
/// `lower_and_project_to_expanded_via_host_threaded`, then to a
/// pure-dispatch `ProjectPath { empty, Expanded }` against the lowered
/// expression. Distinct from `project_expr_class_a_via_dispatch_threaded`
/// in that this bridge does NOT decompose IndexedAccess chains —
/// matching the engine method's "lower the whole expr, dispatch with
/// empty path" semantics for callers that depend on it (e.g.
/// `solve_or_project_leaf_expr_until_stable`).
pub(crate) fn project_expr_surface_expr_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> Option<verter_type_expr::TypeExpr> {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::resolver_core::component_meta_registry::{
        component_meta_registry_public_indexed_access_route,
        component_meta_registry_public_utility_route,
    };
    use crate::resolver_core::{type_expr_contains_semantic_miss, type_expr_is_expanded_surface};
    use crate::semantic_query::{PathSegment, ProjectionMode, QueryResult, SemanticQueryKey};

    if engine.projection_op_budget_exhausted() {
        return None;
    }
    if let Some((root_symbol, route)) = component_meta_registry_public_indexed_access_route(expr)
        .or_else(|| component_meta_registry_public_utility_route(expr))
    {
        if let Some(projected) = project_route_surface_expr_via_host_threaded(
            engine,
            scope_canonical_id,
            &root_symbol,
            &route,
        ) {
            return Some(projected);
        }
        if let Some(solved) =
            lower_and_project_to_expanded_via_host_threaded(engine, scope_canonical_id, expr)
        {
            return Some(solved);
        }
    }
    let ctx = engine.ctx();
    let dispatch = ProjectSemanticDispatch::new(ctx);
    // Block 6.i Commit AX — empty-terminal Expanded requires the
    // base to be a structural surface (see comment on
    // `lower_and_project_to_expanded_via_host_threaded`).
    let base = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        expr,
        ProjectionMode::Expanded,
    )?;
    let read = dispatch.execute_to_type_expr(&SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        mode: ProjectionMode::Expanded,
    });
    let projected = match read.value {
        QueryResult::Value(expr) => expr,
        QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
    };
    (!type_expr_contains_semantic_miss(&projected) && type_expr_is_expanded_surface(&projected))
        .then_some(projected)
}

pub(crate) fn project_expr_surface_expr_with_compound_objects_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> Option<verter_type_expr::TypeExpr> {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::resolver_core::type_expr_has_any_object_arm;
    use crate::semantic_query::{PathSegment, ProjectionMode, QueryResult, SemanticQueryKey};

    if engine.projection_op_budget_exhausted() {
        return None;
    }
    let ctx = engine.ctx();
    let dispatch = ProjectSemanticDispatch::new(ctx);
    // Block 6.i Commit AX — empty-terminal Expanded requires the
    // base to be a structural surface (see comment on
    // `lower_and_project_to_expanded_via_host_threaded`).
    let base = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        expr,
        ProjectionMode::Expanded,
    )?;
    let read = dispatch.execute_to_type_expr(&SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        mode: ProjectionMode::Expanded,
    });
    let projected = match read.value {
        QueryResult::Value(expr) => expr,
        QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
    };
    type_expr_has_any_object_arm(&projected).then_some(projected)
}

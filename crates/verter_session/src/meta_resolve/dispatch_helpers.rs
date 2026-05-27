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

// ─────────────────────────────────────────────────────────────────────
// Realize-callable-member primitive — Transit-Shallow Publication.
// ─────────────────────────────────────────────────────────────────────

/// Realize a slot/macro member value to its underlying callable
/// [`crate::semantic_query::SemanticNodeData::Function`] node, if one
/// exists, by normalizing through the carrier shells the
/// `StructuralTransit(Navigate)` macro-publication path produces.
///
/// Under the Transit-Shallow Publication contract, a macro
/// publication helper lowers its payload at
/// `structural_transit_with_mode(Navigate)`. The published slot
/// member's value is therefore NOT a fully-reduced `Function` —
/// it may be:
///   - a `Function` (the simple case);
///   - an `Alias { inner }` (one-level alias wrap);
///   - a `Conditional` that's decidable but didn't reduce because the
///     publication context carrier-stopped operator reduction;
///   - an `InstantiationRef { base, args }` carrier waiting for
///     instantiation;
///   - a `DeclRef { identity }` carrier waiting for declaration
///     resolution.
///
/// Consumers (the graph-native slot binding extractor's `Function`
/// match arm, `surface_member_to_expanded_field`'s classification,
/// the slot projector) MUST normalize their input through this
/// realization primitive BEFORE deciding "not a callable".
///
/// ## Realization steps (in order)
///
/// 1. **`Function`** → return verbatim.
/// 2. **`Alias { inner }`** → recurse on `inner` (one-hop alias unwrap).
/// 3. **`Conditional`** → dispatch the conditional through the relation
///    engine so a decidable conditional reduces to a single branch.
///    The reduction is independent of the parent's
///    `may_reduce_operator` gate (the relation engine has its own
///    decidability check). Recurse on the reduced result.
/// 4. **`InstantiationRef { base, args }`** → dispatch `Instantiate`
///    under `structural_transit_with_mode(Navigate)` so the body's
///    nested operators carrier-stop while `Conditional` reduction
///    (which is what produces the Function for `T extends (props: P)
///    => any ? F : ...`) still fires. Recurse on the body.
/// 5. **`DeclRef { identity }`** → dispatch `ResolveDecl` to unwrap the
///    identity, then recurse on the resolved body.
///
/// Any other shape (Object, Union, Intersection, Primitive, Mapped,
/// KeyOf, ...) returns `None` — the consumer's "not a function" arm
/// fires.
///
/// **Cycle / depth safety**: bounded at depth 32 — generous enough for
/// real-world carrier nesting (Alias → InstantiationRef → Conditional →
/// Function is depth 4), tight enough to fail loudly on pathological
/// graphs without consuming the test budget.
///
/// **Diagnostic propagation**: every sub-dispatch uses `execute_read`
/// and fans the `dep_signature` into the active fact tracer so the
/// caller's cache validity signature observes the same facts the
/// realization depended on.
#[allow(dead_code)]
pub(crate) fn realize_callable_member(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_>,
    node: crate::semantic_query::SemanticNodeId,
    context: crate::semantic_query::ProjectionReductionContext,
) -> Option<crate::semantic_query::SemanticNodeId> {
    realize_callable_member_inner(dispatch, node, context, 0)
}

#[allow(dead_code, clippy::only_used_in_recursion)]
fn realize_callable_member_inner(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_>,
    node: crate::semantic_query::SemanticNodeId,
    context: crate::semantic_query::ProjectionReductionContext,
    depth: u32,
) -> Option<crate::semantic_query::SemanticNodeId> {
    use crate::meta_resolve::dep_signature::emit_dispatch_dep_signature_facts;
    use crate::semantic_query::{
        ProjectionMode, ProjectionReductionContext, QueryResult, ResolveDeclKey, SemanticNodeData,
        SemanticQueryKey,
    };

    if depth > 32 {
        return None;
    }
    let data = crate::project_semantic_dispatch::node_data_for(dispatch.ctx, node)?;
    match data.as_ref() {
        // (1) Function — the realized callable. Return verbatim.
        SemanticNodeData::Function { .. } => Some(node),

        // (2) Alias → recurse on inner.
        SemanticNodeData::Alias(inner) => {
            realize_callable_member_inner(dispatch, *inner, context, depth + 1)
        }

        // (3) Conditional — re-dispatch through the relation engine so
        // a decidable conditional reduces to a single branch (the
        // SemanticQueryKey::Conditional dispatch carries its own
        // decidability gate independent of the parent's
        // may_reduce_operator demand). The Conditional node stores
        // structurally-normalised `true_branch` / `false_branch` ids;
        // the `*_ref` companion fields hold the pre-normalisation
        // carrier identities used for cache identity but are not
        // needed here (the query key uses the normalised branches).
        SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            distributive,
        } => {
            let check = *check;
            let extends = *extends;
            let true_branch = *true_branch_ref;
            let false_branch = *false_branch_ref;
            let distributive = *distributive;
            drop(data);
            let read = dispatch.execute_read(SemanticQueryKey::Conditional {
                check,
                extends,
                true_branch,
                false_branch,
                distributive,
            });
            emit_dispatch_dep_signature_facts(dispatch.ctx, &read.dep_signature);
            let reduced = match read.value {
                QueryResult::Value(id) if id != node => id,
                // A `Value(id) where id == node` means the dispatch
                // returned the deferred Conditional shell unchanged
                // (not decidable) — nothing further to realize.
                _ => return None,
            };
            realize_callable_member_inner(dispatch, reduced, context, depth + 1)
        }

        // (4) InstantiationRef — instantiate under transit demand so
        // the body's Mapped / KeyOf carriers stay shallow while
        // Conditional reduction (which is what turns
        // `ExtendSlotWithPlan<TPlan, K>` into a Function) fires.
        SemanticNodeData::InstantiationRef { base, args } => {
            let base = base.clone();
            let args = Arc::clone(args);
            drop(data);
            let body_context =
                ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
            let read = dispatch.execute_read(SemanticQueryKey::Instantiate {
                base,
                args,
                context: body_context,
            });
            emit_dispatch_dep_signature_facts(dispatch.ctx, &read.dep_signature);
            let body = match read.value {
                QueryResult::Value(id) => id,
                QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
            };
            realize_callable_member_inner(dispatch, body, context, depth + 1)
        }

        // (5) DeclRef — resolve the declaration, then recurse. The
        // `whole_hash` on the DeclIdentity participates in DeclRef
        // interning but the resolver picks the current artifact via
        // `ScopeId { canonical_id, local_scope }` — the canonical
        // ResolveDecl dispatch pattern from
        // `project_semantic_dispatch::mod::resolve_decl_key`.
        SemanticNodeData::DeclRef { identity } => {
            let identity = identity.clone();
            drop(data);
            let read = dispatch.execute_read(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                scope: crate::semantic_query::ScopeId {
                    canonical_id: Arc::clone(&identity.canonical_id),
                    local_scope: None,
                },
                name: Arc::clone(&identity.decl_name),
            }));
            emit_dispatch_dep_signature_facts(dispatch.ctx, &read.dep_signature);
            let resolved = match read.value {
                QueryResult::Value(id) => id,
                QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
            };
            realize_callable_member_inner(dispatch, resolved, context, depth + 1)
        }

        // Any other shape (Object, Union, Intersection, Primitive,
        // Mapped, KeyOf, IndexedAccess, TypeOf, TypeParam, Literal,
        // Tuple, Array, TemplateLiteral, Opaque) — not callable.
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────
// Macro payload root-shape classifier — path-precise non-slot
// Class A transit-shallow gate.
// ─────────────────────────────────────────────────────────────────────

/// Classify the macro payload's lowered root as a top-level Conditional
/// carrier vs an Object/Intersection/Mapped/Ref/InstantiationRef-rooted
/// shape.
///
/// Used by [`super::materialize::macro_shapes::produce_one_macro_object_shape`]
/// to choose between the existing `Published(Expanded)` dispatch
/// (Conditional root → projector pipeline branch-merge protocol fires
/// on the open Conditional via
/// [`super::projectors::resolve_payload_surface_with_scope`]) and the
/// transit-shallow dispatch
/// ([`project_expr_class_a_via_dispatch_transit_shallow_threaded`])
/// for non-Conditional roots.
///
/// ## Algorithm — structural TypeExpr walk
///
/// The classifier inspects the macro payload's [`TypeExpr`] directly
/// without going through dispatch lowering. Dispatch-lowering under
/// `Expanded` mode would reduce nested Mapped/KeyOf operators and
/// emit `ProjectMember` audit edges (the exact leak we are closing).
/// Dispatch-lowering under `Navigate` mode interns a `DeclRef`
/// carrier whose `ResolveDecl` returns an `Opaque(DeclPlaceholder)`
/// sentinel rather than the alias body, so the carrier walker
/// can't reach the body's structural shape that way either.
///
/// The structural walker:
/// 1. Strips `Parenthesized` wrappers.
/// 2. If the expr is `TypeExpr::Conditional { .. }`, returns `true`.
/// 3. If the expr is `TypeExpr::Ref { name, type_arguments: [] }`
///    AND the engine can resolve a `named_decl_body` for that name
///    in the current scope, recurses on the body TypeExpr.
/// 4. Otherwise (Object, Intersection, Mapped, KeyOf, Union, generic
///    Ref with args, Function, Primitive, etc.) — returns `false`.
///
/// ## Discrimination
///
/// - `defineEmits<ConditionalEmits>()` where
///   `type ConditionalEmits = Mode extends X ? Y : Z` →
///   `Ref("ConditionalEmits", [])` → engine's `named_decl_body` →
///   `Conditional { … }` → returns `true`.
/// - `defineProps<Partial<EditorOptions>>()` →
///   `Ref("Partial", [EditorOptions])` (type_arguments non-empty) →
///   returns `false`.
/// - `defineProps<EditorProps>()` where
///   `interface EditorProps extends Omit<Partial<EditorOptions>, …>` →
///   `Ref("EditorProps", [])` → `named_decl_body` for an interface →
///   `Object` body (with heritage merged) → returns `false`.
///
/// ## Cycle / depth safety
///
/// Bounded at depth 32 — generous enough for real-world alias
/// chains (Conditional aliases that re-route through 2–3 named
/// helpers), tight enough to fail loudly on pathological graphs
/// (alias self-cycles, mutual aliases without TypeAlias resolution
/// fences).
pub(crate) fn macro_payload_root_is_conditional_carrier(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    expr: &verter_type_expr::TypeExpr,
) -> bool {
    walk_type_expr_to_root_conditional(query_engine, owner_canonical, expr, 0)
}

fn walk_type_expr_to_root_conditional(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    expr: &verter_type_expr::TypeExpr,
    depth: u32,
) -> bool {
    use verter_type_expr::TypeExpr;

    if depth > 32 {
        return false;
    }

    match expr {
        // Top-level Conditional carrier. The inherited-emits branch-
        // merge protocol downstream needs the Conditional to remain
        // visible at the macro publication surface; the existing
        // `Published(Expanded)` dispatch in
        // `produce_one_macro_object_shape` produces that surface.
        TypeExpr::Conditional { .. } => true,

        // Transparent wrapper — recurse on inner.
        TypeExpr::Parenthesized(inner) => {
            walk_type_expr_to_root_conditional(query_engine, owner_canonical, inner, depth + 1)
        }

        // Zero-arg named reference — resolve the alias body in the
        // owner scope (or the cross-file declaration's scope) and
        // recurse on the body. Generic-arg refs (`Foo<X>`) stop here:
        // the InstantiationRef's body may Conditional-reduce under
        // substitution (decidable → the macro publication root is
        // the chosen branch, not a Conditional shell) OR stay open
        // (the projector pipeline's branch-merge fires on the
        // deferred Conditional at the payload boundary, not at the
        // macro publication site).
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => {
            let declaration = query_engine.resolve_type_declaration(owner_canonical, name.as_ref());
            let body_canonical = if declaration.canonical_source.is_empty() {
                owner_canonical.to_string()
            } else {
                declaration.canonical_source.clone()
            };
            let body_name = if declaration.resolved_name.is_empty() {
                name.as_ref().to_string()
            } else {
                declaration.resolved_name.clone()
            };
            let Some(body) =
                query_engine.named_decl_body(body_canonical.as_str(), body_name.as_str())
            else {
                return false;
            };
            walk_type_expr_to_root_conditional(
                query_engine,
                body_canonical.as_str(),
                &body,
                depth + 1,
            )
        }

        // Every concrete root shape (Object, Intersection, Mapped,
        // KeyOf, IndexedAccess, Union, generic Ref with args,
        // Function, Primitive, Literal, etc.) — NOT a top-level
        // Conditional carrier.
        _ => false,
    }
}

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
    // r15/F11 shadow gate — check the OUTER utility / chain-root
    // identifier the userland alias would shadow (e.g. `Pick` /
    // `Omit` / the chain root for indexed-access). The route's
    // `root_symbol` is the route's INNER root identity (for
    // `Pick<Source, K>` that is `Source`, NOT the outer `Pick`).
    // Checking shadowing on `root_symbol` mis-suppressed the route
    // whenever the source was a locally-declared interface — the
    // common case — because `ScopeShadowing.shadowed_type_names`
    // contains ALL locally-declared type names. The helper extracts
    // the outer identifier via a structural walk of `expr`.
    let route = component_meta_registry_public_indexed_access_route(expr)
        .or_else(|| component_meta_registry_public_utility_route(expr))
        .filter(|_| !route_outer_utility_is_shadowed(expr, &shadowing));
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

    // Every lowering site explicitly states its mode; there is no
    // implicit-Expanded wrapper.
    //
    // Empty path: lowering the whole expr is the carrier hop that
    // feeds the empty-terminal `ProjectPath { ..., Expanded }`. The
    // walker's `expand_empty_path_terminal` does NOT have a generic
    // `InstantiationRef` arm (the catch-all returns the node
    // unchanged), so a `Navigate` carrier would prevent the
    // expanded-surface filter downstream from observing an
    // Object/Intersection. The lowering mode therefore stays
    // `Expanded` here — the Shallow walker would need an
    // `InstantiationRef` / deferred-`Mapped` enumeration arm before
    // a carrier base could feed an Expanded terminal, and the
    // transit-shallow leak-fix lives at the macro-shape publication
    // boundary instead (see `project_expr_class_a_via_dispatch_transit_shallow`).
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
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
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

/// **Transit-shallow Class A surface projection** — dispatch-direct
/// variant of [`project_expr_class_a_via_dispatch`] for the macro
/// publication boundary's transit-shallow path.
///
/// Used by macro publication paths (e.g.
/// `produce_one_macro_object_shape_for_slots`) that publish the
/// macro's outer Object surface but keep inner member values (slot
/// Function param types, mapped helper carriers, conditional shells)
/// as carrier nodes for the consumer's graph-native re-resolution
/// (CLAUDE.md §"Component-Meta Shallow-By-Default Rule").
///
/// Difference vs [`project_expr_class_a_via_dispatch`]:
/// - Empty-path lowering: `Navigate` (keeps the lowering chain's
///   nested `Instantiate` / `KeyOf` / `MappedType` operators lazy so
///   the inner operator-reduction work that would breadth-enumerate
///   carrier member bodies does not fire at the publication boundary).
///   The silent-miss diagnostic contract is restored at the macro-
///   payload boundary by [`super::projectors::resolve_macro_payload`]'s
///   empty-surface probe: when the transit-shallow payload resolves to
///   an empty Object surface (instead of the eager-resolution `Opaque`
///   sentinel a `Published(Expanded)` lowering would have produced),
///   the payload boundary re-runs `Published(Expanded)` lowering on
///   the macro's `parsed_type_argument` and emits a
///   `macro-payload-decl-unresolved` diagnostic on an
///   `Opaque(DeclPlaceholder)` result.
/// - Terminal `ProjectPath` context: `Published(Shallow)` (one-level
///   surface; inner carrier shells preserved at the publication
///   boundary) instead of `Published(Expanded)` (fully reduce all
///   nested operators across the surface).
/// - Surface filter: accepts any non-`semantic_miss` projection. The
///   `type_expr_is_expanded_surface` check applied by the Expanded
///   sibling is intentionally NOT applied — under transit-shallow the
///   surface intentionally retains carrier shells (`KeyOf`, `Mapped`,
///   `Conditional`, `IndexedAccess`) that the consumer re-resolves on
///   demand. The downstream convert-to-object path
///   (`solver_result_to_object_expansion`) handles Intersection
///   merging where one arm is an Object and another is a deferred
///   operator carrier.
pub(crate) fn project_expr_class_a_via_dispatch_transit_shallow(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> Option<verter_type_expr::TypeExpr> {
    project_expr_class_a_via_dispatch_transit_shallow_threaded(ctx, None, scope_canonical_id, expr)
}

/// Engine-threaded variant of
/// [`project_expr_class_a_via_dispatch_transit_shallow`]. The route
/// fast-path is preserved (registry-routed `Pick<>` / `Omit<>` /
/// indexed-access shapes route through `project_route_surface_expr`
/// independently of the lowering demand — the routes produce already-
/// projected TypeExprs); only the generic `ProjectPath` fallback
/// uses the `Navigate`-base + `Published(Shallow)`-terminal pair so
/// the transit-shallow surface contract is enforced where the registry
/// routes don't already publish a fully-projected shape.
pub(crate) fn project_expr_class_a_via_dispatch_transit_shallow_threaded<'ctx>(
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
        type_expr_contains_semantic_miss, ComponentMetaQueryEngine,
    };
    use crate::semantic_query::{PathSegment, QueryResult, SemanticQueryKey};

    let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::from_host_scope(
        ctx,
        scope_canonical_id,
    );
    // r15/F11 shadow gate — check the OUTER utility / chain-root
    // identifier the userland alias would shadow (e.g. `Pick` /
    // `Omit` / the chain root for indexed-access). The route's
    // `root_symbol` is the route's INNER root identity (for
    // `Pick<Source, K>` that is `Source`, NOT the outer `Pick`).
    // Checking shadowing on `root_symbol` mis-suppressed the route
    // whenever the source was a locally-declared interface — the
    // common case — because `ScopeShadowing.shadowed_type_names`
    // contains ALL locally-declared type names. The helper extracts
    // the outer identifier via a structural walk of `expr`.
    let route = component_meta_registry_public_indexed_access_route(expr)
        .or_else(|| component_meta_registry_public_utility_route(expr))
        .filter(|_| !route_outer_utility_is_shadowed(expr, &shadowing));
    if let Some((root_symbol, route)) = route {
        let mut transient_engine: Option<ComponentMetaQueryEngine<'_>> = None;
        let engine_ref: &mut ComponentMetaQueryEngine<'_> = match engine {
            Some(e) => e,
            None => transient_engine.insert(ComponentMetaQueryEngine::new(ctx)),
        };
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

    let (base_expr, path_segments) = decompose_indexed_access_chain(expr);
    let dispatch = ProjectSemanticDispatch::new(ctx);
    // Empty-path lowering: `Navigate` mode keeps the lowering chain's
    // nested operators (`Instantiate` / `KeyOf` / `MappedType`) lazy
    // so the inner operator-reduction work that would breadth-enumerate
    // carrier member bodies (the Rule-5 leak shape) does not fire.
    // Combined with the `Published(Shallow)` terminal below, the macro
    // publication boundary observes a one-level Object surface with
    // carrier-shaped member values per the shallow-by-default rule.
    // The silent-miss diagnostic contract is restored at the slot-
    // binding-graph payload boundary
    // (`resolve_slot_bindings_graph_native`'s explicit `DeclRef` /
    // `Opaque` payload check) since the eager-resolution side effect
    // of `Published(Expanded)` lowering is no longer available.
    let (base, project_path) = if path_segments.is_empty() {
        let base = dispatch.lower_type_expr_in_scope_with_mode(
            scope_canonical_id,
            expr,
            ProjectionMode::Navigate,
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
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Shallow,
        ),
    });
    let projected = match read.value {
        QueryResult::Value(expr) => expr,
        QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
    };
    // Transit-shallow surface admission: accept any non-semantic-miss
    // projection. The `type_expr_is_expanded_surface` filter applied by
    // the Expanded sibling deliberately rejects residual `KeyOf` /
    // `Mapped` / `Conditional` / `IndexedAccess` shells — but under
    // transit-shallow those shells are the canonical published value
    // for carrier-shaped member positions (the shallow-by-default
    // rule). The downstream convert-to-object path
    // (`solver_result_to_object_expansion`) handles Intersection
    // merging where one arm is an Object and another is a deferred
    // operator carrier.
    (!type_expr_contains_semantic_miss(&projected)).then_some(projected)
}

/// **Transit-shallow Class A shape variant** — consumes the
/// transit-shallow Class A projection and lowers it to an
/// [`verter_semantic::analysis::type_expand::ExpandedObjectShape`].
/// Mirrors the Expanded variant's post-filter (at least one property
/// or call signature) so a publication-empty shape returns `None`.
pub(crate) fn project_expr_class_a_shape_via_dispatch_transit_shallow(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
    let projected =
        project_expr_class_a_via_dispatch_transit_shallow(ctx, scope_canonical_id, expr)?;
    let shape = verter_semantic::analysis::type_expand::type_expr_to_object_shape(&projected);
    (!shape.properties.is_empty() || !shape.call_signatures.is_empty()).then_some(shape)
}

// Class B helpers (dispatch-only surface projection) bypass the
// engine's prepared-decl fallback (`cached_prepared_root_surface`)
// and have regressed in the past on transitive heritage chains and
// barrel-routed declarations. The trampoline's `project_type_surface`
// body remains dispatch-first then prepared-decl-second; threading
// the prepared-decl path through dispatch is not done here.

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
    // Bare-Ref base for the Pick builtin is an intermediate hop;
    // the Pick result is the terminal demand.
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
    // Generic-Ref instantiation publishes its raised body as the
    // result; the caller reads the raised TypeExpr directly without
    // a path-walking follow-up. Lower at Expanded so the body
    // materialises in one step.
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

/// **Transit-shallow** sibling of
/// [`project_type_surface_shape_via_host_threaded`].
///
/// Route projection is demand-explicit: this helper builds a
/// synthetic `Ref { name, [] }` carrier for `(scope, symbol)` and
/// dispatches it through
/// [`project_expr_class_a_shape_via_dispatch_transit_shallow`] under
/// transit-shallow demand:
///
/// - **Empty-path lowering**: `Navigate` (keeps nested operators
///   lazy at the publication boundary).
/// - **Terminal `ProjectPath` context**: `Published(Shallow)`
///   (one-level surface; inner carrier shells preserved).
///
/// This keeps macro publication path-precise: the Expanded sibling
/// instantiates the root's full structural body and re-enters
/// `build_key_of` / `build_mapped_type` for `extends Omit<…>` /
/// generic-substituted carrier members, emitting one per-key
/// `ProjectMember` edge per enumerated key. The shallow helper
/// keeps those operators lazy so a macro payload that does not need
/// a full surface walk never enters the per-key emission loop.
///
/// The macro fast-path in `produce_one_macro_object_shape` uses
/// this sibling when the payload root is NOT a Conditional carrier
/// — Conditional macro payloads still route through the Expanded
/// path for the inherited-emits branch-merge protocol.
pub(crate) fn project_type_surface_shape_transit_shallow_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    symbol_name: &str,
) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
    if engine.projection_op_budget_exhausted() {
        return None;
    }
    let synthetic_ref = verter_type_expr::TypeExpr::Ref {
        name: Arc::from(symbol_name),
        type_arguments: Arc::from(Vec::<verter_type_expr::TypeExpr>::new().into_boxed_slice()),
    };
    project_expr_class_a_shape_via_dispatch_transit_shallow(
        engine.ctx(),
        scope_canonical_id,
        &synthetic_ref,
    )
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
    // Intermediate-base lowering is `Navigate`; the terminal
    // `ProjectPath { .., Shallow }` carries the publication demand.
    let base = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        expr,
        ProjectionMode::Navigate,
    )?;
    let QueryResult::Value(node) = dispatch.execute(SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Shallow,
        ),
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
    // Empty-terminal `ProjectPath { .., Expanded }` requires the
    // base to be a structural surface that `expand_empty_path_terminal`
    // can walk. `Navigate` would freeze a generic carrier at
    // `InstantiationRef` (catch-all), so the expanded-surface filter
    // downstream would reject. The lowering therefore stays `Expanded`
    // here — the Shallow walker would need an `InstantiationRef`
    // enumeration arm before a carrier base could feed an Expanded
    // terminal.
    let base = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        expr,
        ProjectionMode::Expanded,
    )?;
    let read = dispatch.execute_to_type_expr(&SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
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

/// Mode-explicit dispatch-direct surface projection. Caller states
/// `(base_mode, terminal_mode, demand)` so each callsite expresses
/// its publication intent rather than inheriting the legacy
/// `Expanded`/`Expanded` default.
///
/// Behaviour:
/// 1. **Registry-route fast-path** — `Pick<…>` / `Omit<…>` /
///    `Button['ui']` shapes route through the engine's
///    `project_routed_expr_surface_expr` / `lower_and_project_to_expanded`
///    helpers. The fast-path returns the registry's pre-computed
///    Expanded shape regardless of caller mode; downstream caches store
///    one canonical entry per route, so reusing it on a Shallow request
///    does not introduce a new leak.
/// 2. **Pure-dispatch path** — lower the whole expression at
///    `base_mode`, dispatch
///    `ProjectPath { base, path: [], context: { mode: terminal_mode, demand } }`
///    against the lowered base. The empty-path form preserves the
///    engine method's "no IndexedAccess decomposition" semantics for
///    callers that depend on it (e.g.
///    `solve_or_project_leaf_expr_until_stable`).
///
/// Mode-aware result filter:
///   - `terminal_mode == Expanded`: gate on `type_expr_is_expanded_surface`
///     so only fully-materialised surfaces pass (no deferred
///     `KeyOf` / `IndexedAccess` / `Mapped` / `TypeOf` / `Conditional`
///     shells).
///   - `terminal_mode == Shallow`: admit Ref carriers and one-level
///     Object surfaces — do NOT call `type_expr_is_expanded_surface`,
///     which would reject the carrier form the caller explicitly asked
///     for.
///   - either way: refuse `semanticMiss`-bearing results.
///
/// Diagnostic propagation: `QueryResult::Error` and
/// `QueryResult::Recursive` map to `None` so the macro-shape
/// diagnostic flow at `produce_one_macro_object_shape*` keeps emitting
/// the `MacroExpansionDiagnostics` envelope without observing a
/// synthesised `TypeExpr::Error`.
///
/// Caller note on `base_mode` vs `terminal_mode`: per the empty-
/// terminal Expanded constraint documented on
/// [`lower_and_project_to_expanded_via_host_threaded`], the base must
/// be a structural surface that `expand_empty_path_terminal` can walk.
/// A `Navigate` carrier would freeze a generic `InstantiationRef` and
/// the expanded-surface filter would reject. Callers that need
/// Expanded terminal output on arbitrary inputs MUST pass
/// `base_mode = Expanded`; callers on the empty-terminal Shallow
/// path may pass `base_mode = Navigate` (carrier-preserving) per the
/// sister [`project_expr_surface_shape_via_host_threaded`] pattern.
pub(crate) fn project_expr_surface_expr_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
    base_mode: ProjectionMode,
    terminal_mode: ProjectionMode,
    demand: crate::semantic_query::ReductionDemand,
) -> Option<verter_type_expr::TypeExpr> {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::resolver_core::component_meta_registry::{
        component_meta_registry_public_indexed_access_route,
        component_meta_registry_public_utility_route,
    };
    use crate::resolver_core::{type_expr_contains_semantic_miss, type_expr_is_expanded_surface};
    use crate::semantic_query::{
        PathSegment, ProjectionReductionContext, QueryResult, SemanticQueryKey,
    };

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
    let base = dispatch.lower_type_expr_in_scope_with_mode(scope_canonical_id, expr, base_mode)?;
    let read = dispatch.execute_to_type_expr(&SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext {
            mode: terminal_mode,
            demand,
        },
    });
    let projected = match read.value {
        QueryResult::Value(expr) => expr,
        QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
    };
    if type_expr_contains_semantic_miss(&projected) {
        return None;
    }
    match terminal_mode {
        // Expanded terminal — only fully materialised surfaces qualify.
        // A residual `KeyOf` / `IndexedAccess` / `Mapped` / `TypeOf` /
        // `Conditional` shell means the projection did not reach a
        // structural answer and the caller MUST fall back.
        ProjectionMode::Expanded => type_expr_is_expanded_surface(&projected).then_some(projected),
        // Shallow terminal — accept Ref carriers and one-level
        // surfaces. The whole point of the Shallow contract is that
        // the published value may stay as a carrier the consumer
        // re-resolves on demand; running it through
        // `type_expr_is_expanded_surface` would reject the carrier
        // form callers explicitly asked for.
        ProjectionMode::Shallow => Some(projected),
        // `Identity` / `Navigate` / `Skeleton` are not produced by
        // current callsites; admit the result as-is so the helper
        // remains a single drop-in dispatch primitive if a future
        // caller picks them up.
        ProjectionMode::Identity | ProjectionMode::Navigate | ProjectionMode::Skeleton => {
            Some(projected)
        }
    }
}

/// Per codex Q1-Z (BINDING):
///
/// > the slot fallback must not call the compound-object helper in
/// > Expanded mode. Either replace
/// > `project_expr_surface_expr_with_compound_objects_via_host_threaded`
/// > with a demand-aware helper or add a transit-shallow
/// > compound-object sibling and use that from
/// > `produce_one_macro_object_shape_for_slots`.
///
/// The Expanded sibling has been retired (it had exactly one
/// caller, now migrated). This transit-shallow helper mirrors the
/// transit-shallow Class A helper's demand profile:
/// - **Empty-path lowering**: `Navigate` (keeps nested
///   `Instantiate` / `KeyOf` / `MappedType` operators lazy).
/// - **Terminal `ProjectPath` context**: `Published(Shallow)`
///   (one-level surface; inner carrier shells preserved at the
///   publication boundary).
/// - **Surface filter**: `type_expr_has_any_object_arm` — the slot
///   fallback's acceptance contract requires `lower_type_expr` to
///   produce a structural shape with at least one Object arm;
///   otherwise the fallback returns `None` and the slot publication
///   leaves the carrier as-is. The transit-shallow projection still
///   admits Intersection-of-Object shapes through
///   `solver_result_to_object_expansion` downstream; the filter
///   here only refuses results that have NO object arm anywhere
///   (purely scalar / Function / deferred shells), which the slot
///   fallback also refuses.
///
/// Used exclusively by `produce_one_macro_object_shape_for_slots`'s
/// `or_else` fallback when the primary
/// `project_expr_class_a_via_dispatch_transit_shallow` returns
/// `None`.
pub(crate) fn project_expr_surface_expr_with_compound_objects_transit_shallow_via_host_threaded<
    'ctx,
>(
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
    // Navigate-mode lowering keeps nested operators lazy. The
    // Published(Shallow) terminal walks the one-level Object
    // publication surface; member values stay as carrier shells per
    // the shallow-by-default rule.
    let base = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        expr,
        ProjectionMode::Navigate,
    )?;
    let read = dispatch.execute_to_type_expr(&SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Shallow,
        ),
    });
    let projected = match read.value {
        QueryResult::Value(expr) => expr,
        QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
    };
    type_expr_has_any_object_arm(&projected).then_some(projected)
}

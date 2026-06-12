//! Dispatch-direct surface helpers + surface bridge helpers.
//!
//! These helpers resolve a root or surface expression through the shared
//! `ProjectSemanticDispatch` path. They sit next to their `meta_resolve`
//! callers so each callsite stays a one-liner over the shared resolver.
//!
//! They fall into two structural categories:
//!
//! Class A helpers resolve an arbitrary expression through
//! `dispatch.execute_to_type_expr(ProjectPath { lowered, [], mode })`
//! after caller-side lowering, with an expanded-surface filter that drops
//! results still carrying deferred shells or semantic-miss markers.
//!
//! Class B helpers resolve a root symbol's surface through
//! `dispatch.execute_to_type_expr(Instantiate { base, args: [], context:
//! InstantiateContext { projection_reduction, resolve_env_hash } })` with
//! `context.projection_reduction.mode = Expanded`, where `base` is the
//! env-bearing content-free `ResolvedDeclSlotIdentity` slot. The slot
//! carries the project-identity / type-env / lib-env dims and the
//! resolve-env dim rides on `InstantiateContext`; the live whole-hash is
//! re-sourced at value-compute via `ensure_indexed_ready_serve`, never in the key.
//!
//! The surface bridge helpers thread the caller's `ResolverContext` through
//! dispatch and compose the surviving `pub(crate)` cycle-protected dispatch
//! helpers (`dispatch_projected_surface`, `dispatch_routed_expr_surface_expr`,
//! `project_direct_utility_surface_shape`, etc.) plus the surface→expr /
//! surface→shape raises. Dispatch is the sole resolution authority on these
//! paths.

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
            crate::request_context::observe_component_meta_read_suppress(&read);
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
            let slot =
                dispatch.type_slot_for(Arc::clone(&base.canonical_id), Arc::clone(&base.decl_name));
            let owner_canonical = Arc::clone(&base.canonical_id);
            let args = Arc::clone(args);
            drop(data);
            let body_context =
                ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
            let read = dispatch.execute_read(SemanticQueryKey::Instantiate {
                base: slot,
                args,
                context: dispatch.instantiate_context_for(&owner_canonical, body_context),
            });
            crate::request_context::observe_component_meta_read_suppress(&read);
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
            crate::request_context::observe_component_meta_read_suppress(&read);
            emit_dispatch_dep_signature_facts(dispatch.ctx, &read.dep_signature);
            let resolved = match read.value {
                QueryResult::Value(id) => id,
                QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
            };
            realize_callable_member_inner(dispatch, resolved, context, depth + 1)
        }

        // (5b) DeclPlaceholder — the shallow ResolveDecl of an alias / interface
        // declaration returns this carrier rather than the eagerly-resolved
        // body (a `type SlotA = (props) => any` resolves to
        // `Opaque(DeclPlaceholder { name: "SlotA" })` under Navigate). Instantiate
        // the placeholder to obtain the declaration body (the Function), then
        // recurse. Without this arm a slot member typed as an ALIAS to a function
        // (`default: SlotFn` / a `Union` / `Intersection` of such aliases) never
        // realizes to a callable. Mirrors the `expand_empty_path_terminal`
        // DeclPlaceholder expansion in the dispatch walker.
        SemanticNodeData::Opaque(crate::semantic_query::QueryError::DeclPlaceholder {
            canonical_id,
            name,
            whole_hash: _,
        }) => {
            let slot = dispatch.type_slot_for(Arc::clone(canonical_id), Arc::clone(name));
            let owner_canonical = Arc::clone(canonical_id);
            drop(data);
            let read = dispatch.execute_read(SemanticQueryKey::Instantiate {
                base: slot,
                args: Arc::from(
                    Vec::<crate::semantic_query::SemanticNodeId>::new().into_boxed_slice(),
                ),
                context: dispatch.instantiate_context_for(
                    &owner_canonical,
                    ProjectionReductionContext::structural_transit_with_mode(
                        ProjectionMode::Navigate,
                    ),
                ),
            });
            crate::request_context::observe_component_meta_read_suppress(&read);
            emit_dispatch_dep_signature_facts(dispatch.ctx, &read.dep_signature);
            let body = match read.value {
                QueryResult::Value(id) if id != node => id,
                // `Value(id) where id == node` means the instantiate returned the
                // placeholder unchanged (unresolved declaration) — nothing to
                // realize.
                _ => return None,
            };
            realize_callable_member_inner(dispatch, body, context, depth + 1)
        }

        // (6) Union / Intersection — a composite of slot-callable arms
        // (`default: SlotA | SlotB` raises to `Union(Ref(SlotA), Ref(SlotB))`;
        // `(SlotA & SlotB)['default']` to an `Intersection`). Realize EACH arm
        // to its callable Function and rebuild the composite of realized arms,
        // so the slot normalizer's `slot_callable_param_and_return` sees
        // `Union(Function, Function)` / `Intersection(Function, Function)`
        // rather than a composite of unresolved alias `Ref`s. If ANY arm does
        // not realize to a callable the whole composite is not slot-callable
        // (`None`) — the slot normalizer then classifies the member non-slot.
        SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
            let is_union = matches!(data.as_ref(), SemanticNodeData::Union(_));
            let arms = Arc::clone(arms);
            drop(data);
            let mut realized_arms: Vec<crate::semantic_query::SemanticNodeId> =
                Vec::with_capacity(arms.len());
            for arm in arms.iter() {
                let realized = realize_callable_member_inner(dispatch, *arm, context, depth + 1)?;
                realized_arms.push(realized);
            }
            if realized_arms.is_empty() {
                return None;
            }
            // If realization left every arm unchanged, return the original node
            // (avoid interning an identical composite).
            if realized_arms.iter().zip(arms.iter()).all(|(a, b)| a == b) {
                return Some(node);
            }
            let boxed = Arc::from(realized_arms.into_boxed_slice());
            let rebuilt = if is_union {
                SemanticNodeData::Union(boxed)
            } else {
                SemanticNodeData::Intersection(boxed)
            };
            Some(
                dispatch
                    .ctx
                    .project_type_store()
                    .semantic_graph()
                    .intern_node(rebuilt),
            )
        }

        // Any other shape (Object, Primitive, Mapped, KeyOf, IndexedAccess,
        // TypeOf, TypeParam, Literal, Tuple, Array, TemplateLiteral, Opaque) —
        // not callable.
        _ => None,
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
/// member-path test family). The engine route helper itself routes
/// through dispatch, so the fast-path stays semantically aligned with
/// dispatch.
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
        // Route through the surface bridge helpers
        // (`project_route_surface_expr_via_host_threaded` /
        // `lower_and_project_to_expanded_via_host_threaded`), which thread
        // the caller's engine through dispatch. The bridges compose the
        // engine's surviving cycle-protected route helpers — dispatch is
        // the resolution authority on this path.
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
/// Matches a `TypeExpr::Ref { name, type_arguments }` with non-empty
/// type_arguments, resolves the declaration through dispatch, gates
/// against package-backed targets, and applies the prepared-decl's
/// type-parameter substitutions to produce the instantiated body.
///
/// The dispatch path goes through `lower_type_expr_in_scope` which
/// routes a generic `Ref` through
/// `SemanticQueryKey::Instantiate { base, args, context: InstantiateContext {
/// projection_reduction, resolve_env_hash } }` (with
/// `context.projection_reduction.mode = Expanded`)
/// internally — the dispatcher's `build_instantiate` binds the explicit
/// / default type arguments into the lowering env and substitutes them
/// while lowering the prepared-decl body. This is the sole generic-Ref
/// instantiation path for component-meta type resolution.
///
/// Returns `Some(reduced)` only when:
/// - `expr` is a generic `Ref` (else returns `None`: bail on non-Ref /
///   empty type-arguments),
/// - the dispatch lowering produced a node distinct from the carrier
///   `Opaque(Miss)` shell (the dispatcher's miss sentinel for
///   unresolved decl / package-backed / substitution-failure cases),
/// - the raised body differs from the input expression (so a no-op lets
///   the caller fall back).
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
    // Callers use `.unwrap_or_else(|| original.clone())`, so a no-op
    // (raised == expr) must surface as `None` to preserve the caller's
    // own fallback path. A miss-shaped raise (Unknown/Opaque) likewise
    // surfaces as `None` (unresolved decl / package-backed /
    // substitution-failure all collapse to the dispatcher's miss
    // sentinel).
    if raised == *expr {
        return None;
    }
    Some(raised)
}

// =============================================================================
// Class B surface bridge helpers — these thread `query_engine.ctx` through dispatch.
//
// The threaded `_threaded(engine, …)` variants are the production
// callsite shape (engine threaded through caller). The `_via_host_threaded`
// variants below are the canonical entrypoints.
// =============================================================================

// The root-surface bridge resolves a root symbol's surface through the
// shared dispatch surface projector ALONE — `dispatch_projected_surface`
// composes Object / Alias roots directly and compound (Union /
// Intersection / InstantiationRef) roots from the decl anchor through the
// shared empty-path Shallow walker. Dispatch is the sole root-surface
// authority here; there is no prepared-decl root-surface rescue behind
// dispatch. The `root_surface_bridges_carry_no_prepared_decl_fallback`
// architecture guard enforces this absence.
pub(crate) fn project_type_surface_expr_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    symbol_name: &str,
) -> Option<verter_type_expr::TypeExpr> {
    use crate::resolver_core::projected_surface_to_type_expr;
    if engine.projection_op_budget_exhausted() {
        return None;
    }
    let surface = engine.dispatch_projected_surface(scope_canonical_id, symbol_name)?;
    projected_surface_to_type_expr(&surface)
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
        SemanticQueryOutput,
    };

    if engine.projection_op_budget_exhausted() {
        return None;
    }
    if let Some((root_symbol, route)) = component_meta_registry_public_indexed_access_route(expr)
        .or_else(|| component_meta_registry_public_utility_route(expr))
    {
        if let Some(projected) =
            engine.dispatch_routed_expr_surface_expr(scope_canonical_id, &root_symbol, &route)
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
    let QueryResult::Value(SemanticQueryOutput { value: node, .. }) =
        dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
            base,
            path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
            context: crate::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Shallow,
            ),
        })
    else {
        return None;
    };
    let surface = projected_surface_from_semantic_node(ctx, node)?;
    let shape = projected_surface_to_expanded_shape(&surface);
    // An index-signature-only surface (`{ [k: string]: string }`) is a
    // genuine props surface — `defineProps<{ [k: string]: string }>()` admits
    // every string key. Admitting it here lets the owner-local root gate (which
    // already counts index signatures) see a non-empty shape; gating on
    // properties / call-signatures alone would drop an index-sig-only root.
    (!shape.properties.is_empty()
        || !shape.call_signatures.is_empty()
        || !shape.index_signatures.is_empty())
    .then_some(shape)
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
    engine.dispatch_routed_expr_surface_expr(scope_canonical_id, root_symbol, route)
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
/// its publication intent explicitly rather than defaulting to
/// `Expanded`/`Expanded`.
///
/// Behaviour:
/// 1. **Registry-route fast-path** — `Pick<…>` / `Omit<…>` /
///    `Button['ui']` shapes route through the engine's
///    `dispatch_routed_expr_surface_expr` / `lower_and_project_to_expanded`
///    helpers. The fast-path returns the registry's pre-computed
///    Expanded shape regardless of caller mode; downstream caches store
///    one canonical entry per route, so reusing it on a Shallow request
///    does not introduce a new leak.
/// 2. **Pure-dispatch path** — lower the whole expression at
///    `base_mode`, dispatch
///    `ProjectPath { base, path: [], context: { mode: terminal_mode, demand } }`
///    against the lowered base. The empty-path form performs no
///    IndexedAccess decomposition, which callers depend on (e.g.
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
    let base = dispatch.lower_type_expr_in_scope_with_context(
        scope_canonical_id,
        expr,
        ProjectionReductionContext {
            mode: base_mode,
            demand,
            provenance: crate::semantic_query::SurfaceProvenanceContext::Structural,
            merge_role: crate::semantic_query::MemberMergeRole::Authored,
        },
    )?;
    let read = dispatch.execute_to_type_expr(&SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext {
            mode: terminal_mode,
            demand,
            provenance: crate::semantic_query::SurfaceProvenanceContext::Structural,
            merge_role: crate::semantic_query::MemberMergeRole::Authored,
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

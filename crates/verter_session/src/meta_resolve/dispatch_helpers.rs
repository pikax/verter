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
//! Class B helpers resolve a root symbol's surface through
//! `dispatch.execute_read(Instantiate { base, args: [], context:
//! InstantiateContext { projection_reduction, resolve_env_hash } })` with
//! `context.projection_reduction.mode = Expanded`, where `base` is the
//! env-bearing content-free `ResolvedDeclSlotIdentity` slot, the result
//! node likewise materialised once at the surface sink. The slot
//! carries the project-identity / type-env / lib-env dims and the
//! resolve-env dim rides on `InstantiateContext`; the live whole-hash is
//! re-sourced at value-compute via `ensure_indexed_ready_serve`, never in the key.
//!
//! The surface bridge helpers thread the caller's `ResolverContext` through
//! dispatch and compose the surviving `pub(crate)` cycle-protected dispatch
//! helpers (`dispatch_projected_surface_with_node`,
//! `dispatch_routed_expr_surface_node`, etc.) plus the surface→expr /
//! surface→shape raises. Dispatch is the sole resolution authority on these
//! paths.

use crate::resolver_core::ResolverContext;
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
            let read = dispatch.execute_read(SemanticQueryKey::Instantiate(
                crate::semantic_query::InstantiateKey::new(
                    slot,
                    args,
                    dispatch.instantiate_context_for(&owner_canonical, body_context),
                ),
            ));
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
            let read = dispatch.execute_read(SemanticQueryKey::Instantiate(
                crate::semantic_query::InstantiateKey::new(
                    slot,
                    Arc::from(
                        Vec::<crate::semantic_query::SemanticNodeId>::new().into_boxed_slice(),
                    ),
                    dispatch.instantiate_context_for(
                        &owner_canonical,
                        ProjectionReductionContext::structural_transit_with_mode(
                            ProjectionMode::Navigate,
                        ),
                    ),
                ),
            ));
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
        // so the node-domain slot reader (`CallableNodeView::
        // slot_param_and_return_by_arm`) sees `Union(Function, Function)` /
        // `Intersection(Function, Function)` rather than a composite of
        // unresolved alias carriers. If ANY arm does not realize to a callable
        // the whole composite is not slot-callable (`None`) — the slot
        // normalizer then classifies the member non-slot.
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
    // Resolve via the node-domain Class-A sibling (registry route fast-path +
    // terminal), materialising ONCE at the surface sink — the engine-less
    // counterpart of `project_expr_class_a_node_via_dispatch_threaded`.
    crate::resolver_core::project_class_a_published(ctx, scope_canonical_id, expr)
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
    ctx: &'ctx dyn ResolverContext,
    mut engine: Option<&mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>>,
    scope_canonical_id: &str,
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
        Some(e) => e.scope_shadowing_for_scope(scope_canonical_id),
        None => std::sync::Arc::new(
            crate::resolver_core::scope_shadowing::ScopeShadowing::from_host_scope(
                ctx,
                scope_canonical_id,
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
            None => transient_engine.insert(ComponentMetaQueryEngine::new(ctx)),
        };
        if let Some(projected) = project_route_surface_node_via_host_threaded(
            engine_ref,
            scope_canonical_id,
            &root_symbol,
            &route,
        ) {
            return Some(projected);
        }
        if let Some(solved) = lower_and_project_to_expanded_node_via_host_threaded(
            engine_ref,
            scope_canonical_id,
            expr,
        ) {
            return Some(solved);
        }
    }
    crate::resolver_core::project_class_a_terminal_node(ctx, scope_canonical_id, expr)
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

/// Graph-native sibling of [`decompose_indexed_access_chain`]: decompose a
/// lowered `IndexedAccess` carrier GRAPH node into `(base_node, path)`.
///
/// The macro hot mirror produces a mode-neutral structural carrier graph for
/// the macro type argument; an indexed-access type argument
/// (`DeepConfig['ui']['header']`) lowers to nested
/// [`SemanticNodeData::IndexedAccess`] shells. This walks those shells —
/// collecting each string-literal / canonical-number index hop into a
/// `ProjectPath` selector — until it reaches the base node, so a deep
/// indexed-access decomposes to `(base, [Index("ui"), Index("header")])`
/// WITHOUT lowering the base a second time (it IS the same handle). A
/// non-indexed carrier decomposes to `(node, [])`.
pub(crate) fn decompose_indexed_access_chain_node(
    ctx: &dyn ResolverContext,
    node: crate::semantic_query::SemanticNodeId,
) -> (
    crate::semantic_query::SemanticNodeId,
    Arc<[crate::semantic_query::PathSegment]>,
) {
    use crate::semantic_query::{IndexKey, PathSegment, SemanticNodeData};

    // Collect outer→inner, then reverse so the path reads base→terminal.
    let mut rev_path: Vec<PathSegment> = Vec::new();
    let mut current = node;
    while let Some(data) = crate::project_semantic_dispatch::node_data_for(ctx, current) {
        match data.as_ref() {
            SemanticNodeData::IndexedAccess { object, index } => match index {
                IndexKey::String(s) => {
                    rev_path.push(PathSegment::Index(IndexKey::String(Arc::clone(s))));
                    current = *object;
                }
                IndexKey::Number(n) => {
                    rev_path.push(PathSegment::Index(IndexKey::Number(*n)));
                    current = *object;
                }
                // A type-node index is not a path-precise string/number hop —
                // stop and let the dispatch resolve the whole indexed-access.
                IndexKey::TypeNode(_) => break,
            },
            _ => break,
        }
    }
    rev_path.reverse();
    (current, Arc::from(rev_path.into_boxed_slice()))
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
    expr: &verter_type_expr::TypeExpr,
) -> Option<crate::resolver_core::AdmittedRouteProjectionNode> {
    if engine.projection_op_budget_exhausted() {
        return None;
    }
    crate::resolver_core::lower_and_project_to_expanded_node(engine.ctx(), scope_canonical_id, expr)
}

/// Node-domain registry-route projection: returns the admitted registry-route
/// node (no materialisation).
pub(crate) fn project_route_surface_node_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    root_symbol: &str,
    route: &crate::resolver_core::RouteDemand,
) -> Option<crate::resolver_core::AdmittedRouteProjectionNode> {
    if engine.projection_op_budget_exhausted() {
        return None;
    }
    engine.dispatch_routed_expr_surface_node(scope_canonical_id, root_symbol, route)
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
pub(crate) fn arg_preserving_member_use_site_slot(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_>,
    member_name: &str,
    declaration_origin: Option<&str>,
    value_node: crate::semantic_query::SemanticNodeId,
) -> Option<verter_type_expr::locators::TypeBodySlot> {
    // Gate: the observed value head must be an argument-bearing named
    // reference (the class the argument-less `Ref` publication is lossy
    // for). One bounded node-domain peek — no dispatch.
    let observed = resolved_instantiation_head(dispatch, value_node)?;

    let origin = declaration_origin?;
    let state = dispatch.ctx.shallow_file_state(origin)?;
    // The UNIQUE file-scope type declaration whose OWN syntactic member
    // headers declare this member name (heritage contributes nothing to
    // `type_member_headers`, so a heritage-reached member resolves against
    // its true declaring contributor's file).
    let mut declaring: Option<&str> = None;
    for name in state.type_symbol_names() {
        let declares_member = state
            .type_member_headers(name)
            .is_some_and(|headers| headers.iter().any(|h| h.name == member_name));
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
    if !state.symbol(declaring)?.type_param_names.is_empty() {
        return None;
    }
    let prepared = dispatch.ctx.prepared_type_decl(origin, declaring)?;
    let slot = prepared.member_index.get(member_name)?.ty.clone();

    // Honesty verification: the candidate slot must raise to the SAME
    // resolved instantiation as the observed value — equal base identity,
    // equal interned argument nodes. One memoized Navigate-transit raise of
    // one member annotation (never a body expansion, never an Instantiate
    // execution).
    let raised = dispatch.raise_authored_locator_to_hot(
        &verter_type_expr::locators::AuthoredBodyLocator::DeclBody(slot.clone()),
        crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
            crate::semantic_query::ProjectionMode::Navigate,
        ),
    )?;
    let authored = resolved_instantiation_head(dispatch, raised.node())?;
    (authored == observed).then_some(slot)
}

/// The alias-peeled, carrier-head-resolved INSTANTIATION identity of a node:
/// `Some((base, args))` when the head is an argument-bearing named reference
/// (`InstantiationRef`, or a `BareRef` / `ImportType` carrier still holding
/// authored type arguments that head-resolves to one through the shared
/// carrier-preserving normalization — name-to-declaration routing only,
/// never body expansion). `None` for every other shape.
fn resolved_instantiation_head(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_>,
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
        let data = crate::project_semantic_dispatch::node_data_for(dispatch.ctx, current)?;
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

#[cfg(test)]
mod pick_demand_api_signature_tests {
    //! Boundary-closure proof (compile-level ONLY): the routed-Pick member
    //! surface is reached through the query-engine DEMAND API
    //! `ComponentMetaQueryEngine::materialize_pick_member_surface`, which takes
    //! a scope + root symbol + member keys and returns the materialised
    //! `Option<TypeExpr>` — it accepts NO `SemanticNodeId` and exposes none.
    //!
    //! The Pick dispatch + node-core materialisation happen INTERNALLY inside
    //! the demand API, so a forgeable `SemanticNodeId` never crosses the
    //! query-engine boundary. The prior shape — a `pick_via_dispatch_pick_node`
    //! helper that returned a bare `SemanticNodeId` for an out-of-subtree caller
    //! to feed into `materialize_member_surface_node` — is GONE.
    //!
    //! Asserted at COMPILE LEVEL by coercing the demand API to a fn pointer
    //! whose parameters are exactly `(scope, symbol, members, nested)` and whose
    //! return is `Option<TypeExpr>`. The coercion type-checks only if the real
    //! signature matches; a return of `Option<SemanticNodeId>` (the leaking
    //! node-returning shape) would fail to unify here. This is the successor of
    //! the §1a fence: no node crosses the boundary because the boundary API
    //! resolves it internally.

    #[test]
    fn pick_demand_api_takes_no_node_and_returns_a_type_expr() {
        // A monomorphic shim whose signature is the boundary contract:
        // `(scope, symbol, members, nested) -> Option<TypeExpr>`, NO
        // `SemanticNodeId` in or out. Binding the method to this shim
        // type-checks only if the real demand-API signature matches; a
        // node-returning `Option<SemanticNodeId>` shape (the leaking form)
        // would fail to unify here. This is the successor of the §1a fence.
        fn _proof(
            engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
            scope: &str,
            symbol: &str,
            members: &[String],
            nested: bool,
        ) -> Option<verter_type_expr::TypeExpr> {
            engine.materialize_pick_member_surface(scope, symbol, members, nested)
        }
        let _ = _proof
            as fn(
                &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
                &str,
                &str,
                &[String],
                bool,
            ) -> Option<verter_type_expr::TypeExpr>;
    }
}

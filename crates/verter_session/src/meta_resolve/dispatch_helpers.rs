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

/// Extract the single callable [`verter_type_expr::FunctionExpr`] arm from a
/// raised [`TypeExpr`], stripping the nullish (`undefined` / `null`) arms an
/// EXPLICIT nullish UNION/INTERSECTION VALUE carries.
///
/// This handles a callback prop whose WRITTEN VALUE is an explicit composite
/// containing a callable arm — `onselect: ((r: Row) => void) | undefined`
/// realises (after `realize_callable_member` + `raise_node_to_type_expr`) to
/// `Union([Function, Primitive(Undefined)])`; a bare `TypeExpr::Function` match
/// would drop it.
///
/// This is NOT about member-`?` optionality: a member-OPTIONAL prop
/// (`onselect?: (r: Row) => void`) factors the `?` into the surface `optional`
/// flag, so its VALUE raises to a BARE `TypeExpr::Function`, not a union — that
/// case lands on the `Function(f)` arm below, not the composite arm.
///
/// This helper accepts:
///
/// - `Function(f)` → `f` verbatim (a bare callable value — required props and
///   member-`?`-optional props alike).
/// - `Parenthesized(inner)` → recurse (the parser keeps `(…) => void`'s wrap).
/// - `Union(arms)` / `Intersection(arms)` → drop every `undefined` / `null`
///   arm of an explicit nullish composite, then if EXACTLY ONE callable arm
///   remains (recursively), return it.
///
/// It is deliberately NARROW: a composite with NO callable arm returns `None`
/// (a non-callable union like `onmode: "a" | "b"` is not an event), and a
/// composite with MULTIPLE distinct callable arms also returns `None`
/// (ambiguous — the caller must not fabricate a single payload from divergent
/// signatures). It NEVER broadens to a non-callable arm and NEVER introduces
/// `any`.
#[must_use]
pub(crate) fn callable_arm_from_raised(
    raised: &verter_type_expr::TypeExpr,
) -> Option<Arc<verter_type_expr::FunctionExpr>> {
    use verter_type_expr::{PrimitiveName, TypeExpr};

    match raised {
        TypeExpr::Function(func) => Some(Arc::clone(func)),
        TypeExpr::Parenthesized(inner) => callable_arm_from_raised(inner),
        TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => {
            let mut callable: Option<Arc<verter_type_expr::FunctionExpr>> = None;
            for arm in arms.iter() {
                // Nullish arms (`undefined` / `null`) from an explicit nullish
                // union are stripped — they are not the callable.
                if matches!(
                    arm,
                    TypeExpr::Primitive(PrimitiveName::Undefined | PrimitiveName::Null)
                ) {
                    continue;
                }
                match callable_arm_from_raised(arm) {
                    // A second, distinct callable arm makes the payload
                    // ambiguous — refuse rather than pick one.
                    Some(found) => {
                        if let Some(existing) = &callable {
                            if !Arc::ptr_eq(existing, &found) && **existing != *found {
                                return None;
                            }
                        } else {
                            callable = Some(found);
                        }
                    }
                    // A non-nullish, non-callable arm means the member is not a
                    // pure callable — do not classify it as an event.
                    None => return None,
                }
            }
            callable
        }
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
    mut engine: Option<&mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>>,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> Option<verter_type_expr::TypeExpr> {
    use crate::resolver_core::{
        component_meta_registry::{
            component_meta_registry_public_indexed_access_route,
            component_meta_registry_public_utility_route,
        },
        ComponentMetaQueryEngine,
    };

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
    // wins" rule across BOTH lowering entry points. With a
    // `ComponentMetaQueryEngine` threaded in, the `Some` arm takes the
    // shadow set from its per-scope memo (`from_scope_payload` of the
    // loaded bundle payload, built once per scope and reused per field);
    // the engine-less `None`-arm fallback builds it directly via
    // `from_host_scope`. When the scope's prepared-decl bundle is loaded —
    // the case an engine-present caller hits, since the SFC's own bundle is
    // already loaded — the two observe a membership-equivalent shadow set.
    // In the rare unloaded-bundle case the memo's lazy load yields the
    // properly-shadowed set where the bare `from_host_scope` fallback would
    // see an empty one: strictly more correct, not a regression.
    // `as_deref_mut` reborrows so `engine` stays usable at the later
    // `match engine` below.
    let shadowing = match engine.as_deref_mut() {
        Some(e) => e.scope_shadowing_for_scope(scope_canonical_id),
        None => std::sync::Arc::new(
            crate::resolver_core::scope_shadowing::ScopeShadowing::from_host_scope(
                ctx,
                scope_canonical_id,
            ),
        ),
    };
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
    // Pure-dispatch tail: the decompose + lower + ProjectPath + node-domain
    // gate + publication materialisation are confined to the registered surface
    // sink (M4 demand-bound adapter), so no `SemanticNodeId -> TypeExpr`
    // mid-flight raise happens here.
    crate::resolver_core::project_class_a_terminal_published(ctx, scope_canonical_id, expr)
}

/// Node-domain sibling of [`project_expr_class_a_via_dispatch_threaded`]: returns
/// the admitted route/surface NODE instead of its materialised `TypeExpr`.
///
/// Mirrors the `TypeExpr` form ARM FOR ARM — the SAME scope-shadowing gate, the
/// SAME registry route fast-path, the SAME primary/fallback order — but swaps the
/// two host-threaded `*_expr_via_host_threaded` bridges for their node
/// counterparts ([`project_route_surface_node_via_host_threaded`] /
/// [`lower_and_project_to_expanded_node_via_host_threaded`]) and the pure-dispatch
/// tail for [`crate::resolver_core::project_class_a_terminal_node`]. None of these
/// materialises: the admitted node is published ONCE downstream at the registry
/// sink, so the registry member-path / refine consumers compute their reject/accept
/// facts off the projected node WITHOUT re-lowering a materialised leaf.
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
    loop {
        let Some(data) = crate::project_semantic_dispatch::node_data_for(ctx, current) else {
            break;
        };
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
    if engine.projection_op_budget_exhausted() {
        return None;
    }
    // The raw `SurfaceView` / `SemanticNodeId` projection is confined to the
    // query-engine sink; this host-threaded wrapper reaches it ONLY through the
    // engine's sink-local composition method.
    engine.dispatch_projected_surface_to_type_expr(scope_canonical_id, symbol_name)
}

pub(crate) fn project_expr_surface_shape_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
    if engine.projection_op_budget_exhausted() {
        return None;
    }
    // The `expr -> lower -> ProjectPath -> node -> shape` resolution lives behind
    // the query-engine demand API: the caller passes a scope + `&TypeExpr`, never
    // a resolved node, so the raw `SemanticNodeId`-to-surface projection stays
    // confined to the query-engine sink (the forgeable node never crosses here).
    engine.project_expr_to_surface_shape(scope_canonical_id, expr)
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
    if engine.projection_op_budget_exhausted() {
        return None;
    }
    // Empty-terminal `ProjectPath { .., Expanded }` requires the base to be a
    // structural surface `expand_empty_path_terminal` can walk; the lowering
    // therefore stays `Expanded` (see the demand-bound adapter). The lower +
    // ProjectPath + node-domain gate (`materialized && expanded_surface &&
    // node-domain-changed`) + publication materialisation are confined to the
    // registered surface sink (M4), so no mid-flight raise happens here.
    crate::resolver_core::lower_and_project_to_expanded_published(
        engine.ctx(),
        scope_canonical_id,
        expr,
    )
}

// ===========================================================================
// Node-returning route fixpoint adapters.
//
// The route fixpoint (`solve_or_project_leaf_node_until_stable`) stabilises on
// node-domain `RaisedShapeKey` identity with NO per-iteration materialisation:
// each iteration projects through these node wrappers (which return the admitted
// `AdmittedRouteProjectionNode`, never a `TypeExpr`), the fixpoint compares
// successive nodes by interned raised-shape key, and the sole publication
// materialisation happens ONCE after convergence at the surface sink. The
// node wrappers mirror the `*_via_host_threaded` TypeExpr wrappers above arm for
// arm (same budget guard, same registry fast-path, same primary/fallback order)
// minus the terminal raise.
// ===========================================================================

/// Node-domain counterpart of [`lower_and_project_to_expanded_via_host_threaded`]:
/// returns the admitted route node (no materialisation).
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

/// Node-domain counterpart of [`project_route_surface_expr_via_host_threaded`]:
/// returns the admitted registry-route node (no materialisation).
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

/// Node-domain mode-explicit dispatch-direct surface projection: the caller
/// states `(base_mode, terminal_mode, demand)` and the registry fast-path
/// returns the admitted route node while the pure-dispatch path returns the
/// admitted surface node — neither materialises. The sole publication
/// materialisation happens once, downstream, at the surface sink.
pub(crate) fn project_expr_surface_expr_node_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
    base_mode: ProjectionMode,
    terminal_mode: ProjectionMode,
    demand: crate::semantic_query::ReductionDemand,
) -> Option<crate::resolver_core::AdmittedRouteProjectionNode> {
    use crate::resolver_core::component_meta_registry::{
        component_meta_registry_public_indexed_access_route,
        component_meta_registry_public_utility_route,
    };

    if engine.projection_op_budget_exhausted() {
        return None;
    }
    if let Some((root_symbol, route)) = component_meta_registry_public_indexed_access_route(expr)
        .or_else(|| component_meta_registry_public_utility_route(expr))
    {
        if let Some(projected) = project_route_surface_node_via_host_threaded(
            engine,
            scope_canonical_id,
            &root_symbol,
            &route,
        ) {
            return Some(projected);
        }
        if let Some(solved) =
            lower_and_project_to_expanded_node_via_host_threaded(engine, scope_canonical_id, expr)
        {
            return Some(solved);
        }
    }
    crate::resolver_core::project_expr_surface_expr_node(
        engine.ctx(),
        scope_canonical_id,
        expr,
        base_mode,
        terminal_mode,
        demand,
    )
}

/// Re-project an already-admitted route node one fixpoint step (node-base
/// re-projection, no re-lowering, no materialisation). Used by the fixpoint for
/// iterations after the first, where the cursor is already an admitted node.
pub(crate) fn project_admitted_node_to_expanded_node_via_host_threaded<'ctx>(
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'ctx>,
    prior: &crate::resolver_core::AdmittedRouteProjectionNode,
) -> Option<crate::resolver_core::AdmittedRouteProjectionNode> {
    if engine.projection_op_budget_exhausted() {
        return None;
    }
    crate::resolver_core::project_admitted_node_to_expanded_node(engine.ctx(), prior)
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

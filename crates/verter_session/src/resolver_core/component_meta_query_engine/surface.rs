//! Surface-projection helpers, prepared-substitution machinery, and
//! arc cache-key constructors used by `ComponentMetaQueryEngine`.
//!
//! Free functions (not engine methods) that operate on
//! `TypeExpr` / [`SurfaceView`] values produced by the engine and
//! dispatch layers; no engine-state dependencies beyond a borrowed
//! `VerterHost` reference.
//!
//! Cross-callers reach the public-API symbols here via the parent
//! module's `pub(crate) use surface::{...};` re-export at the bottom of
//! `component_meta_query_engine/mod.rs`. Internal helpers stay
//! parent-private (no visibility relaxation).

use rustc_hash::FxHashSet;
use verter_type_expr::TypeExpr;

use super::route_admission::{
    admit_expanded_surface, admit_expanded_surface_changed, AdmittedRouteProjectionNode,
};
use super::{
    BUDGET_EXCEEDED_SENTINEL_PREFIX, SEMANTIC_MISS, SEMANTIC_OBJECT_SURFACE,
    SEMANTIC_SURFACE_MEMBER,
};
use crate::project_semantic_dispatch::output_materialization::OutputProjector;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{QueryError, SemanticNodeData, SemanticNodeId, SurfaceView};

crate::project_semantic_dispatch::output_materialization::define_output_capability! {
    /// The component-meta query-engine SURFACE projector's output-sink
    /// capability. The surface projector here holds this to materialize a
    /// graph node into a sealed output carrier and unwrap it. Its constructor
    /// is visible ONLY within
    /// `crate::resolver_core::component_meta_query_engine::surface` — NOT the
    /// whole query-engine subtree — so no query-engine sibling can mint it
    /// (planted `MetaQuerySurfaceOutputCap::new` outside this leaf is
    /// `E0624`).
    pub(crate) struct MetaQuerySurfaceOutputCap;
    mint: pub(in crate::resolver_core::component_meta_query_engine::surface)
}

// ===========================================================================
// Demand-bound publication adapters (M4 — codex-settled).
//
// The Kind-B route helpers / route fixpoint make their convergence / gating /
// equality decisions NODE-DOMAIN (raised-shape facts + interned key, no
// `TypeExpr` materialisation). The single PUBLICATION `TypeExpr` they return is
// materialised ONCE here, at this registered surface sink, through the sealed
// [`MetaQuerySurfaceOutputCap`]. The adapters take a HIGH-LEVEL demand (scope +
// `&TypeExpr` + modes); they lower internally so no raw forgeable
// `SemanticNodeId` ever crosses in from a non-sink caller, and the bare
// `TypeExpr` leaves only as the accepted publication value.
// ===========================================================================

/// Materialise an accepted graph `node` into a published `TypeExpr` at this
/// surface sink. MODULE-PRIVATE: the bare `TypeExpr` is produced here and
/// handed back to the demand-bound adapters below as the accepted publication
/// value — a raw node is never accepted from outside the adapters.
fn materialize_published_node(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<TypeExpr> {
    let cap = MetaQuerySurfaceOutputCap::new(dispatch);
    cap.materialize_output_type_expr(node)
        .map(|raised| raised.into_type_expr(&cap))
}

/// Terminal sink: materialise an [`AdmittedRouteProjectionNode`] into a
/// published `TypeExpr` ONCE, at the existing `materialize_published_node`
/// surface sink (the sealed [`MetaQuerySurfaceOutputCap`]). The route fixpoint
/// and the surface publication wrappers call this exactly once after their
/// node-domain decisions converge — there is no mid-flight materialisation. The
/// carrier's node was admitted by a route/surface adapter's node-domain gate,
/// so this is a pure one-shot publication with no decision on the result.
///
/// Subtree-scoped (`pub(in …::component_meta_query_engine)`): every caller is a
/// route/surface adapter or the route fixpoint inside this subtree, so the
/// confinement is COMPILER-enforced — no out-of-subtree site can reach the
/// node→`TypeExpr` materialisation except through the engine's sink-local
/// publication methods.
pub(in crate::resolver_core::component_meta_query_engine) fn materialize_route_projection_node(
    ctx: &dyn ResolverContext,
    node: &AdmittedRouteProjectionNode,
) -> Option<TypeExpr> {
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
    materialize_published_node(&dispatch, node.node())
}

/// Demand-bound adapter for the empty-terminal Expanded publication path.
/// Lower `expr` at `Expanded`, dispatch `ProjectPath { base, [],
/// Published(Expanded) }`, gate on NODE-DOMAIN facts
/// (`materialized && expanded_surface`) plus the node-domain "changed" check
/// (`!raised_shape_eq_node_type_expr(result, expr)`), and materialise the
/// accepted result node ONCE at this sink. `None` on lower-miss, dispatch
/// error/recursive, gate-reject, or raise-miss.
pub(crate) fn lower_and_project_to_expanded_node(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &TypeExpr,
) -> Option<AdmittedRouteProjectionNode> {
    use crate::project_semantic_dispatch::raise::node_raised_shape_for_eq_with_dispatch;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{PathSegment, ProjectionMode, QueryResult, SemanticQueryKey};

    let dispatch = ProjectSemanticDispatch::new(ctx);
    let base = dispatch.lower_type_expr_in_scope_with_mode(
        scope_canonical_id,
        expr,
        ProjectionMode::Expanded,
    )?;
    let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base,
        path: std::sync::Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    });
    let result_node = match read.value {
        QueryResult::Value(node) => node,
        QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
    };
    // Facts + the no-op/changed decision come from ONE node fold (reusing the
    // dispatch above); the gate (`materialized && expanded_surface && changed`,
    // where `changed = !shape.eq_to_expr` is the node-domain shape inequality
    // against `expr`) is encoded in `admit_expanded_surface_changed`.
    let shape = node_raised_shape_for_eq_with_dispatch(&dispatch, result_node, expr)?;
    admit_expanded_surface_changed(&shape)
}

/// Demand-bound NODE adapter for the Class-A path-precise projection (the
/// pure-dispatch tail of the node-domain Class-A projection). Decompose
/// the IndexedAccess chain INTERNALLY, lower the base (empty path → lower the
/// whole `expr` at `Expanded`; non-empty path → lower the chain root at
/// `Navigate`), dispatch `ProjectPath { base, path, Published(Expanded) }`,
/// gate on NODE-DOMAIN facts (`materialized && expanded_surface`), and return
/// the admitted node — NO materialisation. The lowering happens here so no raw
/// node crosses in; the `*_published` wrapper materialises the accepted node
/// ONCE at the surface sink.
pub(crate) fn project_class_a_terminal_node(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &TypeExpr,
) -> Option<AdmittedRouteProjectionNode> {
    use crate::project_semantic_dispatch::raise::node_raised_shape_facts_with_dispatch;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{PathSegment, ProjectionMode, QueryResult, SemanticQueryKey};

    let (base_expr, path_segments) =
        crate::meta_resolve::dispatch_helpers::decompose_indexed_access_chain(expr);
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let (base, project_path) = if path_segments.is_empty() {
        let base = dispatch.lower_type_expr_in_scope_with_mode(
            scope_canonical_id,
            expr,
            ProjectionMode::Expanded,
        )?;
        (
            base,
            std::sync::Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        )
    } else {
        let base = dispatch.lower_type_expr_in_scope_with_mode(
            scope_canonical_id,
            base_expr,
            ProjectionMode::Navigate,
        )?;
        (base, path_segments)
    };
    let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base,
        path: project_path,
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    });
    let result_node = match read.value {
        QueryResult::Value(node) => node,
        QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
    };
    // A `BareRef` carrier SURVIVING the `Published(Expanded)` demand is a
    // genuine unresolved route/declaration — this sink IS the resolving
    // demand point, so the class-A projection FAILS here exactly as the
    // pre-carrier `Opaque(Miss)` terminal did. (`DeclRef` /
    // `InstantiationRef` identity carriers are NOT in this class — they
    // name a resolved declaration.)
    if crate::project_semantic_dispatch::node_data_for(ctx, result_node)
        .as_deref()
        .is_some_and(|data| data.bare_ref_head().is_some())
    {
        return None;
    }
    // Facts-only gate — reuses the dispatch above; no structural key interned.
    let witness = node_raised_shape_facts_with_dispatch(&dispatch, result_node)?;
    admit_expanded_surface(&witness)
}

/// Publication wrapper over the FULL node-domain Class-A projection
/// ([`crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded`]):
/// resolve the registry route fast-path THEN the terminal node adapter in node
/// domain, then materialise the accepted route node ONCE at the surface sink.
///
/// This is the materialising counterpart of the node sibling: the node-domain
/// decision (route fast-path + terminal
/// `materialized && expanded_surface` gate) lives in the node fn; this wrapper
/// adds only the one terminal raise. It lives in the surface sink module so the
/// node→`TypeExpr` materialisation stays owner-confined. The engine is NOT
/// threaded (a transient engine is created internally), matching the engine-less
/// `project_expr_class_a_via_dispatch` callers.
pub(crate) fn project_class_a_published(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    expr: &TypeExpr,
) -> Option<TypeExpr> {
    let node = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
        ctx,
        None,
        scope_canonical_id,
        expr,
    )?;
    materialize_route_projection_node(ctx, &node)
}

/// Resolve a root node to its one-level `Object` [`SurfaceView`], following
/// `Alias` identity hops (cycle-guarded).
///
/// Compound roots (`A | B`, `A & B` / heritage overlay, `Foo<Bar>`) carry no
/// single `Object` surface on the post-`Published(Expanded)` instantiated
/// node, and that node can collapse a generic heritage / `Omit` carrier arm
/// to `Opaque(Miss)`. So this projector returns `None` for them; the seam
/// (`dispatch_projected_surface_with_node`) composes the compound root via
/// [`compound_root_surface_view_via_dispatch`] driven from the decl anchor
/// (carrier intact).
pub(super) fn surface_view_from_semantic_node(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<SurfaceView> {
    let mut active = FxHashSet::default();
    surface_view_from_semantic_node_inner(ctx, node, &mut active)
}

fn surface_view_from_semantic_node_inner(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<SurfaceView> {
    let data = ctx.dispatch_node_data(node)?;
    match data.as_ref() {
        SemanticNodeData::Alias(target) => {
            if !active.insert(node) {
                return None;
            }
            let result = surface_view_from_semantic_node_inner(ctx, *target, active);
            active.remove(&node);
            result
        }
        SemanticNodeData::Object(surface) => Some(surface.clone()),
        _ => None,
    }
}

/// Compose the shallow surface of a compound root node (`Union` /
/// `Intersection` / `InstantiationRef`) through the shared empty-path
/// Shallow surface walker: drives `ProjectPath { base: node, path: [],
/// macro_object_surface(Shallow, Structural) }` via
/// `resolve_typeinfo_surface_view_with_node` and returns the terminal
/// [`SurfaceView`] directly — no materialisation.
///
/// `node` is the decl-anchor base the seam supplies — NOT the
/// post-`Published(Expanded)` instantiated root, which can collapse a
/// generic heritage / `Omit` carrier arm to `Opaque(Miss)` (the shared
/// walker cannot re-resolve an already-collapsed node, whereas the decl
/// anchor still carries the carrier intact). Returns `None` when the walker
/// resolves no `Object` terminal OR the composed surface is empty (an empty
/// surface is never a COMPLETE compound-root projection).
///
/// Returns the composed [`SurfaceView`] PAIRED with the terminal `Object`
/// NODE the walker read it from. That node IS the composed surface, so the
/// Whole-route publication gate folds its node-domain materializedness over
/// THAT node — never over the carrier-intact `node` decl anchor, whose own
/// raise keeps heritage / import carriers unresolved (materialized) and would
/// admit a partial composed surface the surface-materialization filter rejects.
pub(super) fn compound_root_surface_view_via_dispatch(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<(SurfaceView, SemanticNodeId)> {
    use crate::semantic_query::{
        ProjectionMode, ProjectionReductionContext, SurfaceProvenanceContext,
    };

    let (surface, surface_node) = ctx.dispatch().resolve_typeinfo_surface_view_with_node(
        node,
        ProjectionReductionContext::macro_object_surface(
            ProjectionMode::Shallow,
            SurfaceProvenanceContext::Structural,
        ),
    )?;
    if surface_view_is_empty(&surface) {
        return None;
    }
    Some((surface, surface_node))
}

/// A surface with no members, no call/construct signatures, and no index
/// signature carries nothing to publish (never a COMPLETE compound-root
/// projection). Node-domain — no materialisation feeds this decision.
pub(super) fn surface_view_is_empty(surface: &SurfaceView) -> bool {
    surface.members.is_empty()
        && surface.call_signatures.is_empty()
        && surface.construct_signatures.is_empty()
        && !surface.has_index_signature
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "TypeExpr parity oracle for the node-domain materialized fact; \
                  production gates read the shape-engine node facts"
    )
)]
pub(super) fn dispatch_route_expr_is_materialized(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Unknown { raw } => {
            // Every sentinel emitted by the `shape_engine::fold_node`
            // materialisation algebra (exact matches) or by
            // `semantic_query_error_raw` (prefix matches for parameterised
            // errors) must round-trip to
            // "not materialised". The sentinel set
            // is owned by the shared `raise_sentinel` classifier so the
            // node-domain raised-shape projection and this `TypeExpr`
            // recogniser can never disagree on the spelling.
            !crate::project_semantic_dispatch::raise_sentinel::raw_is_unmaterialized_sentinel(raw)
        }
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().all(dispatch_route_expr_is_materialized)
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => dispatch_route_expr_is_materialized(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .all(|element| dispatch_route_expr_is_materialized(&element.ty)),
        TypeExpr::Object(object) => object.properties.iter().all(|member| match member {
            verter_type_expr::ObjectMember::Property(property) => {
                dispatch_route_expr_is_materialized(&property.ty)
            }
            verter_type_expr::ObjectMember::Method(method) => {
                method
                    .function
                    .return_type
                    .as_deref()
                    .is_none_or(dispatch_route_expr_is_materialized)
                    && method
                        .function
                        .parameters
                        .iter()
                        .all(|parameter| dispatch_route_expr_is_materialized(&parameter.ty))
            }
            verter_type_expr::ObjectMember::CallSignature(signature)
            | verter_type_expr::ObjectMember::ConstructSignature(signature) => {
                signature
                    .return_type
                    .as_deref()
                    .is_none_or(dispatch_route_expr_is_materialized)
                    && signature
                        .parameters
                        .iter()
                        .all(|parameter| dispatch_route_expr_is_materialized(&parameter.ty))
            }
            verter_type_expr::ObjectMember::IndexSignature(signature) => {
                dispatch_route_expr_is_materialized(&signature.key_type)
                    && dispatch_route_expr_is_materialized(&signature.value_type)
            }
        }),
        // A constructor type's signature is checked identically to a function
        // type's (same `FunctionExpr` payload).
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
            function
                .return_type
                .as_deref()
                .is_none_or(dispatch_route_expr_is_materialized)
                && function
                    .parameters
                    .iter()
                    .all(|parameter| dispatch_route_expr_is_materialized(&parameter.ty))
        }
        TypeExpr::IndexedAccess { object, index } => {
            dispatch_route_expr_is_materialized(object)
                && dispatch_route_expr_is_materialized(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            dispatch_route_expr_is_materialized(check)
                && dispatch_route_expr_is_materialized(extends)
                && dispatch_route_expr_is_materialized(true_type)
                && dispatch_route_expr_is_materialized(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            dispatch_route_expr_is_materialized(source)
                && dispatch_route_expr_is_materialized(value)
                && name_type
                    .as_deref()
                    .is_none_or(dispatch_route_expr_is_materialized)
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Ref { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::Infer { .. }
        // Synthetic carriers are fully materialised at the projector
        // surface — they ARE the published leaf, not a deferred token.
        | TypeExpr::SyntheticSlotBinding(_)
        // An import-type is a published shallow carrier (like a bare `Ref`),
        // not an unmaterialised dispatch sentinel — count it as materialised.
        | TypeExpr::ImportType { .. }
        | TypeExpr::RecursiveRef { .. } => true,
    }
}

/// Detects sentinel tokens emitted by the `shape_engine::fold_node`
/// materialisation algebra when dispatch cannot materialise a node — the
/// whole-tree `TypeExpr`-domain miss walk.
///
/// DISPLAY/PARITY oracle only, NOT a production semantic gate: production
/// reads the node-domain whole-tree miss fact
/// (`node_contains_semantic_miss_with_dispatch`, the typed
/// `!RaisedShapeFacts.materialized` projection) off the shape-engine fold.
/// This `TypeExpr` predicate survives as the oracle the raised-shape parity
/// suite compares that node fact against.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "TypeExpr parity oracle for the node-domain whole-tree miss fact; \
                  production gates read node_contains_semantic_miss_with_dispatch"
    )
)]
pub(crate) fn type_expr_contains_semantic_miss(expr: &TypeExpr) -> bool {
    !dispatch_route_expr_is_materialized(expr)
}

/// Root-level (carrier-position) unmaterialised-sentinel recogniser.
///
/// Returns `true` when the expression IS a raise sentinel at its root
/// (unwrapping only `Parenthesized`) — the shape produced when a
/// published carrier is re-lowered by NAME in a scope where the name
/// does not resolve, so the demanded reduction itself failed. Distinct
/// from [`type_expr_contains_semantic_miss`], which also fires on
/// genuine NESTED partial values: an unresolvable member-value
/// reference (`element?: HTMLElement` without the DOM lib) inside an
/// otherwise-materialised surface is a contract-conformant partial
/// result (Macro Type Traversal — the field that transitively depends
/// on the unresolved name publishes partially; sibling members resolve
/// normally), not a failed reduction.
///
/// Production reads the node-domain root-sentinel fact
/// (`node_root_is_unmaterialized_sentinel_with_dispatch`); this `TypeExpr`
/// predicate survives ONLY as the `#[cfg(test)]` parity oracle the raised-shape
/// suite compares that node fact against.
#[cfg(test)]
pub(crate) fn type_expr_root_is_unmaterialized_sentinel(expr: &TypeExpr) -> bool {
    let mut current = expr;
    while let TypeExpr::Parenthesized(inner) = current {
        current = inner;
    }
    match current {
        TypeExpr::Unknown { .. } => !dispatch_route_expr_is_materialized(current),
        _ => false,
    }
}

/// Returns `true` when `expr` is the budget-exceeded sentinel
/// (`TypeExpr::Unknown { raw }` whose `raw` starts with
/// [`BUDGET_EXCEEDED_SENTINEL_PREFIX`]). This is the single shared
/// recognizer for the spelling `semantic_query_error_raw` emits for
/// `QueryError::BudgetExceeded` — production routing and every test that
/// scans a published surface for a leaked budget sentinel call this so
/// the spelling can never drift between producer and detector.
pub(crate) fn type_expr_is_budget_exceeded_sentinel(expr: &TypeExpr) -> bool {
    matches!(expr, TypeExpr::Unknown { raw } if raw.starts_with(BUDGET_EXCEEDED_SENTINEL_PREFIX))
}

/// Returns `true` when `expr` still carries open deferred shell shapes
/// (`KeyOf`, `IndexedAccess`, `Mapped`, `TypeOf`, `Conditional`) that
/// indicate dispatch could not structurally expand the surface further.
//
// The node-domain `expanded_surface` fact (computed bottom-up in `shape_engine`)
// now drives every production gate; this `TypeExpr` predicate survives ONLY as
// the `#[cfg(test)]` parity ORACLE the raised-shape suite compares the bottom-up
// fact against, so the non-test build sees it as dead.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "TypeExpr parity oracle for the bottom-up expanded_surface fact; \
                  production gates read the node-domain fact via shape_engine"
    )
)]
pub(crate) fn type_expr_is_expanded_surface(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::KeyOf(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::Conditional { .. } => false,
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().all(type_expr_is_expanded_surface)
        }
        _ => true,
    }
}

pub(crate) fn semantic_query_error_raw(err: &QueryError) -> String {
    match err {
        QueryError::Miss => SEMANTIC_MISS.to_string(),
        QueryError::Other(text) => text.as_ref().to_string(),
        QueryError::UnsupportedIntrinsic { name } => format!("unsupportedIntrinsic({name})"),
        QueryError::BudgetExceeded(failure) => format!("budgetExceeded({:?})", failure.domain),
        QueryError::UnstableState { attempts } => format!("unstableState({attempts})"),
        QueryError::AliasCycle { chain } => format!("aliasCycle({})", chain.len()),
        QueryError::RecursiveRef { name } => format!("recursiveRef({name})"),
        QueryError::DeclPlaceholder { name, .. } => format!("declPlaceholder({name})"),
        QueryError::ValueDomainMismatch { expected, actual } => {
            format!("valueDomainMismatch(expected={expected:?},actual={actual:?})")
        }
        // Typed semantic-sentinel carriers → BYTE-IDENTICAL legacy raw
        // strings. A future stage that swaps a raw `Unknown { raw: "X" }`
        // construction for `Opaque(QueryError::…)` must raise to the same
        // text, so these mappings are pinned by
        // `typed_query_error_sentinels_round_trip_to_legacy_raw`.
        QueryError::RaiseAliasCycle => "semanticAliasCycle".to_string(),
        QueryError::TypeParamCycle => "semanticTypeParamCycle".to_string(),
        QueryError::RaiseMiss => "<raise miss>".to_string(),
        QueryError::UnrepresentableSurface => SEMANTIC_OBJECT_SURFACE.to_string(),
        QueryError::UnrepresentableSurfaceMember => SEMANTIC_SURFACE_MEMBER.to_string(),
    }
}

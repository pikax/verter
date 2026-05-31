//! Resolved-state types + small scope-selection / cycle-reachability
//! helpers.
//!
//! Domain 5 — `ResolvedComponentMetaState`, `SurfaceNodeIdentities`,
//! type aliases, and a handful of standalone scope-selection and
//! transitive-cycle-reachability helpers.

use super::dep_signature::emit_dispatch_dep_signature_facts;
use crate::types::{FileAnalysisSnapshot, Hash16, ProjectionMode};
use std::sync::Arc;

// `ResolvedDeclarationKind`, `ResolvedTypeDeclaration`,
// `ResolvedTypeRegistryMeta`, `ResolvedMacroMeta`, `ResolvedNativeProp`,
// `ResolvedJsdocBlock`, `ResolvedJsdocTag`, and
// `ResolvedComponentMetaComputeAudit` live in the request-ctx sibling
// (`super::request_host`); this module imports them via `super::*`
// re-exports through the shell.
use super::{ResolvedComponentMetaComputeAudit, ResolvedMacroMeta, ResolvedTypeRegistryMeta};

/// Vector-aligned sidecar carrying the producing `SemanticNodeId`
/// for each output entry in `ExpandedComponentTypes` /
/// `ResolvedTypeRegistry`.
///
/// Populated when audit is on so `build_origin_graph` can scope the
/// reachable-subgraph walk to the actual surface nodes the request
/// touched, rather than exporting every edge ever recorded by the
/// shared graph store. `None` entries indicate synthetic /
/// inline-annotation results that bypassed dispatch (no
/// `SemanticNodeId` available).
///
/// Index alignment is invariant: `prop_node_ids[i]` corresponds to
/// `evaluated_types.props[i]`, etc. Length-equality checked at
/// construction time inside `compute_component_meta_state_inner`.
///
/// Stored on `ResolvedComponentMetaState.surface_identities` —
/// session-layer only (per crate-layering §1.3 + D19, NOT pushed
/// upstream into `verter_semantic` types).
#[derive(Debug, Clone, Default)]
pub struct SurfaceNodeIdentities {
    /// Index-aligned with `ExpandedComponentTypes.props`.
    pub prop_node_ids: Vec<Option<crate::semantic_query::SemanticNodeId>>,
    /// Index-aligned with `ExpandedComponentTypes.emits`.
    pub emit_node_ids: Vec<Option<crate::semantic_query::SemanticNodeId>>,
    /// Index-aligned with `ExpandedComponentTypes.slot_bindings`.
    pub slot_binding_node_ids: Vec<Option<crate::semantic_query::SemanticNodeId>>,
    /// Index-aligned with `ExpandedComponentTypes.bindings`.
    pub binding_node_ids: Vec<Option<crate::semantic_query::SemanticNodeId>>,
    /// Index-aligned with `ResolvedComponentMetaState.resolved_type_registry`.
    pub registry_node_ids: Vec<Option<crate::semantic_query::SemanticNodeId>>,
}

#[derive(Debug, Clone)]
pub struct ResolvedComponentMetaState {
    /// The raw analysis snapshot (never mutated for enrichment).
    pub snapshot: FileAnalysisSnapshot,
    /// Which mode was used to produce this state.
    pub mode: ProjectionMode,
    /// Content hash of the owner file at resolution time.
    pub whole_hash: Hash16,
    /// Resolved macro metadata from cross-file traversal.
    pub resolved_macros: Vec<ResolvedMacroMeta>,
    /// Resolved type registry entries (populated in `Expanded` mode).
    pub resolved_type_registry:
        Vec<verter_semantic::analysis::component_meta::ResolvedTypeAnalysis>,
    /// Native declaration metadata for each resolved type-registry entry.
    pub resolved_type_registry_meta: Vec<ResolvedTypeRegistryMeta>,
    /// Expanded types (populated in `Expanded` mode only).
    pub evaluated_types: Option<verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
    /// Semantic fact versions consumed while producing this resolved state.
    pub fact_versions: Vec<crate::resolver_core::FactVersionRef>,
    /// Non-semantic compute audit captured only when native audit is enabled.
    pub compute_audit: Option<ResolvedComponentMetaComputeAudit>,
    /// Surface-id sidecar. Populated only
    /// when audit is on; the scoped origin export reads `prop_node_ids`
    /// etc. as starting points for the reachable-subgraph walk.
    pub surface_identities: Option<SurfaceNodeIdentities>,
    /// Origin subgraph for semantic results. Populated in `Expanded` mode
    /// by walking the `SemanticGraphStore` after dispatch resolution.
    pub origin_graph: Option<verter_protocol::types::OriginGraphDto>,
    /// Request identifier stamped by the ctx at the entry of
    /// `get_component_meta_with_resolution`. Non-zero. Consumers (the
    /// `AuditedRequest` harness and NAPI/WASM/LSP wrappers) use this
    /// to retrieve the matching `RequestAuditRecord` via
    /// `VerterHost::take_audit_record(resolution.request_id)`.
    ///
    /// Zero is reserved for "not populated" — emitted by internal
    /// tests / FFI fixtures that do not stamp a real request id.
    pub request_id: u64,
    /// Macro-expansion diagnostics produced by graph-native slot-binding
    /// synthesis. Merged into
    /// [`ComponentMetaAnalysis::macro_expansion_diagnostics`] by
    /// [`crate::host_manage::component_meta_extract::extract_component_meta_from_resolved`]
    /// and projected onto the audit substrate via
    /// [`crate::host_audit_bridge::macro_expansion_to_audit_entries`].
    pub synthesis_diagnostics:
        Vec<verter_semantic::analysis::component_meta::MacroExpansionDiagnostics>,
    /// `true` when graph-native slot-binding synthesis observed a fatal
    /// `QueryError` (`BudgetExceeded`, `UnstableState`, walker
    /// `cache_suppress`) during the cold compute. Gates
    /// `ComponentMetaResultDb` publication so partially-populated
    /// results never warm the shared final-result cache.
    pub synthesis_should_suppress: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegistryMaterialization {
    Full,
    SkipAppend,
}

pub(crate) fn select_imported_materialization_scope(
    expr: &verter_type_expr::TypeExpr,
    owner_canonical: &str,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> Option<String> {
    use crate::resolver_core::component_meta_registry::{
        component_meta_registry_public_indexed_access_route,
        component_meta_registry_public_utility_route,
    };
    let route_root_name = component_meta_registry_public_utility_route(expr)
        .or_else(|| component_meta_registry_public_indexed_access_route(expr))
        .map(|(root_name, _)| root_name);
    let root_name = match expr {
        verter_type_expr::TypeExpr::Ref { name, .. } => name.as_ref(),
        _ => route_root_name.as_deref()?,
    };

    let declaration = query_engine.resolve_type_declaration(owner_canonical, root_name);
    let declaration_scope = if declaration.canonical_source.is_empty() {
        owner_canonical.to_string()
    } else {
        declaration.canonical_source.clone()
    };
    let declaration_name = if declaration.resolved_name.is_empty() {
        root_name.to_string()
    } else {
        declaration.resolved_name.clone()
    };
    let (final_scope, _) = query_engine
        .resolve_final_prepared_type_target(declaration_scope.as_str(), declaration_name.as_str());

    (!final_scope.is_empty() && final_scope != owner_canonical).then_some(final_scope)
}

pub(crate) fn lowered_root_reaches_transitive_cycle(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> bool {
    lowered_root_reaches_transitive_cycle_with_fence(query_engine, scope_canonical_id, expr).0
}

/// Variant of [`lowered_root_reaches_transitive_cycle`] that also returns
/// the BFS-observed dependency fence (per-canonical
/// `(Arc<str>, DepVersion)` pairs).
///
/// Used by the projector's gate-short-circuit admit
/// paths to thread the cycle gate's cross-file deps into the cache
/// entry's `fact_dep_signature`. The bare bool variant remains for
/// callers that only need the predicate verdict.
///
/// The fence is ALSO emitted via `emit_dispatch_dep_signature_facts`
/// here (matching the legacy bool-only variant's behaviour) so the
/// outer `state.fact_versions` accumulator + active `with_fact_tracer`
/// scope still observe the cycle dep graph regardless of whether the
/// caller threads the returned fence into a cache admit.
pub(crate) fn lowered_root_reaches_transitive_cycle_with_fence(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    expr: &verter_type_expr::TypeExpr,
) -> (bool, crate::semantic_query::DepSignature) {
    use verter_type_expr::TypeExpr;

    // Extract the root identity carried by the TypeExpr structure
    // WITHOUT lowering. Lowering is recursive over the entire subtree
    // (including generic args' constraints/defaults that may load
    // third-party `.d.ts` files); calling it on a deeply-generic
    // `IndexedAccess { Ref<X<TMetadata, TDataParts, TTools>>, "k" }`
    // expression deeply lowers all children only to discard the result
    // (the post-lowering identity match accepts only `DeclRef` and
    // `InstantiationRef`, never `IndexedAccess`). For ChatMessage's
    // `leading.avatar` slot binding this lowering ate 213 seconds per
    // call on the cold path. Walk the TypeExpr surface here and use
    // the cached `resolve_type_declaration` to produce a
    // `DeclIdentity` directly — no eager lowering, no third-party
    // file loads triggered by constraint resolution.
    fn root_decl_identity(
        expr: &TypeExpr,
        owner_canonical: &str,
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) -> Option<crate::semantic_query::DeclIdentity> {
        match expr {
            TypeExpr::Parenthesized(inner) => {
                root_decl_identity(inner, owner_canonical, query_engine)
            }
            TypeExpr::IndexedAccess { object, .. } => {
                root_decl_identity(object, owner_canonical, query_engine)
            }
            TypeExpr::Ref { name, .. } | TypeExpr::RecursiveRef { name, .. } => {
                let declaration = query_engine.resolve_type_declaration(owner_canonical, name);
                let resolved_canonical = if declaration.canonical_source.is_empty() {
                    Arc::<str>::from(owner_canonical)
                } else {
                    Arc::<str>::from(declaration.canonical_source.as_str())
                };
                let resolved_name = if declaration.resolved_name.is_empty() {
                    Arc::<str>::from(name.as_ref())
                } else {
                    Arc::<str>::from(declaration.resolved_name.as_str())
                };
                let whole_hash = query_engine
                    .ctx
                    .shallow_file_state(resolved_canonical.as_ref())
                    .map(|state| state.whole_hash)
                    .unwrap_or_default();
                Some(crate::semantic_query::DeclIdentity {
                    canonical_id: resolved_canonical,
                    whole_hash,
                    decl_name: resolved_name,
                })
            }
            _ => None,
        }
    }

    let Some(identity) = root_decl_identity(expr, scope_canonical_id, query_engine) else {
        return (false, Arc::from(Vec::new()));
    };
    crate::loop5_instrumentation::LOWERED_ROOT_CYCLE_FAST_PATH_HITS
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
    // F1: record the root declaration identity itself in the fence —
    // the BFS only records `roots` it visits inside `bfs_compute_inner`
    // (and merges any cache-hit dep_signature); the root identity is
    // not appended on the fast path, so we add it explicitly so the
    // admit's `fact_dep_signature` invalidates on root-declaration-file
    // edits.
    if !identity.canonical_id.as_ref().is_empty()
        && identity.canonical_id.as_ref() != "__builtin__"
        && identity.canonical_id.as_ref() != scope_canonical_id
    {
        fence.push((
            Arc::clone(&identity.canonical_id),
            crate::semantic_query::DepVersion::WholeHash(identity.whole_hash),
        ));
    }
    let result =
        super::ref_root_reaches_transitive_cycle_node(&identity, query_engine.ctx, &mut fence);
    // Dual-emit the BFS fence into both downstream channels so the
    // legacy `state.fact_versions` curated signature and the outer
    // `with_fact_tracer` scope both observe the cycle dep graph.
    let fence_signature: crate::semantic_query::DepSignature = Arc::from(fence.into_boxed_slice());
    emit_dispatch_dep_signature_facts(query_engine.ctx, &fence_signature);
    (result, fence_signature)
}

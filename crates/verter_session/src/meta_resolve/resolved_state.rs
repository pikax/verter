//! Resolved-state types + small scope-selection / cycle-reachability
//! helpers.
//!
//! Domain 5 — `ResolvedComponentMetaState`, `SurfaceNodeIdentities`,
//! type aliases, and a handful of standalone scope-selection and
//! transitive-cycle-reachability helpers.

use crate::types::{FileAnalysisSnapshot, Hash16, ProjectionMode};

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
    /// Typed per-result completeness — `Complete` when this resolved state is
    /// the full surface, `Partial` (with its reason set) when a budget
    /// exhaustion / fatal `QueryError` / partial macro surface produced a
    /// structurally-incomplete result during the cold compute. This is the
    /// AUTHORITATIVE partial-result signal; [`Self::synthesis_should_suppress`]
    /// is a compatibility projection derived from
    /// `completeness.is_partial()` (do not duplicate the truth — set
    /// `completeness`, read `synthesis_should_suppress`). A `Partial` result
    /// is RETURNED to the caller but is refused warm admission to the
    /// `ComponentMetaResultDb` / resolved-meta caches (the no-poison
    /// invariant).
    pub completeness: crate::semantic_query::ResultCompleteness,
    /// `true` when graph-native slot-binding synthesis observed a fatal
    /// `QueryError` (`BudgetExceeded`, `UnstableState`, walker
    /// `cache_suppress`) during the cold compute. Gates
    /// `ComponentMetaResultDb` publication so partially-populated
    /// results never warm the shared final-result cache.
    ///
    /// COMPATIBILITY PROJECTION of [`Self::completeness`]: equals
    /// `completeness.is_partial()`. Kept as a bool field so the many existing
    /// consumers read it directly; the typed `completeness` is the single
    /// source of truth.
    pub synthesis_should_suppress: bool,
}

impl ResolvedComponentMetaState {
    /// Merge facts observed by the extraction/fallthrough phase into this
    /// call-owned state. Existing resolve facts retain their order; new facts
    /// append in producer order with deterministic equality-based dedup.
    pub(crate) fn merge_extraction_fact_versions(
        &mut self,
        extraction_facts: Option<&[crate::resolver_core::FactVersionRef]>,
    ) -> bool {
        let Some(extraction_facts) = extraction_facts else {
            return false;
        };
        let previous_len = self.fact_versions.len();
        crate::resolver_core::extend_unique_fact_versions(
            &mut self.fact_versions,
            extraction_facts.iter().cloned(),
        );
        self.fact_versions.len() != previous_len
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegistryMaterialization {
    Full,
    SkipAppend,
}

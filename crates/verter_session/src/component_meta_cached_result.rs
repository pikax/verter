//! The session payload of the final component-meta cache: the native
//! component-meta result and its sanitized resolution sidecar, stored in the
//! generic [`crate::component_meta_result_db::ComponentMetaResultDb`].

use std::sync::Arc;

use verter_session_query::analysis::types::Hash16;

use verter_type_engine::semantic_query::ProjectionMode;

impl verter_session_query::retention::RetainedFootprint for CachedComponentMetaResult {
    fn retained_footprint_bytes(&self) -> usize {
        let analysis = &self.analysis;
        let surfaces = analysis.props.len()
            + analysis.events.len()
            + analysis.slots.len()
            + analysis.models.len()
            + analysis.exposed.len()
            + analysis.accepted_props.len()
            + analysis.accepted_events.len();
        let structural = analysis.components.len()
            + analysis.template_refs.len()
            + analysis.imports.len()
            + analysis.bindings.len()
            + analysis.vue_api_calls.len()
            + analysis.styles.len();
        let types = analysis.type_registry.len()
            + self.resolution_template.resolved_type_registry.len()
            + self.resolution_template.resolved_type_registry_meta.len()
            + self.resolution_template.resolved_macros.len();
        surfaces * crate::component_meta_result_db::footprint::SURFACE_RECORD_BYTES
            + structural * crate::component_meta_result_db::footprint::STRUCTURAL_RECORD_BYTES
            + types * crate::component_meta_result_db::footprint::TYPE_RECORD_BYTES
            + self.resolution_template.fact_versions.len()
                * crate::component_meta_result_db::footprint::FACT_BYTES
            + self.canonical_id.len()
            + std::mem::size_of::<Self>()
    }
}

/// Sanitized snapshot of a
/// [`crate::meta_resolve::ResolvedComponentMetaState`] suitable for
/// cross-request reuse. Excludes per-request fields (`request_id`,
/// `compute_audit`) and the [`FileAnalysisSnapshot`] (reloaded from
/// `ProjectTypeStore::indexed()` at rehydrate time).
///
/// Field-by-field partition (per D4.1):
///
/// - **EXCLUDED — per-request, never cached:**
///   - `request_id: u64` (allocated per request).
///   - `compute_audit: Option<...>` (request-specific timings/counters).
///
/// - **EXCLUDED — snapshot-derived, reloaded from host:**
///   - `snapshot: FileAnalysisSnapshot` (reload via
///     `ProjectTypeStore::indexed().get(canonical, whole_hash)`).
///
/// - **INCLUDED — content-addressed via `dep_signature`:**
///   - `mode`, `whole_hash`.
///   - `resolved_macros`, `resolved_type_registry`,
///     `resolved_type_registry_meta`.
///   - `evaluated_types`.
///   - `fact_versions`.
///   - `surface_identities` (audit sidecar; cache, do not rehydrate as
///     None).
///   - `origin_graph` (audit sidecar; cache).
#[derive(Debug, Clone)]
pub struct ResolutionTemplate {
    pub mode: ProjectionMode,
    pub whole_hash: Hash16,
    pub resolved_macros: Vec<crate::meta_resolve::ResolvedMacroMeta>,
    pub resolved_type_registry:
        Vec<verter_session_query::analysis::component_meta::ResolvedTypeAnalysis>,
    pub resolved_type_registry_meta: Vec<crate::meta_resolve::ResolvedTypeRegistryMeta>,
    pub evaluated_types:
        Option<verter_session_query::analysis::type_expand::ExpandedComponentTypes>,
    pub fact_versions: Vec<verter_session_query::facts::fact_cache::FactVersionRef>,
    pub surface_identities: Option<crate::meta_resolve::SurfaceNodeIdentities>,
    pub origin_graph: Option<verter_protocol::types::OriginGraphDto>,
    /// Per-result completeness preserved across the template round-trip.
    /// Only `Complete` results are admitted to `ComponentMetaResultDb` (the
    /// publication gate refuses partials), so a cached template is `Complete`
    /// in production; preserving the typed value keeps rehydrate honest
    /// rather than independently resetting the suppression bool to `false`.
    pub completeness: verter_type_engine::semantic_query::ResultCompleteness,
}

/// Cached component-meta payload AND its sanitized
/// resolution sidecar. The DB generic migrates from
/// `ComponentMetaResultDb<ComponentMetaAnalysis>` to
/// `ComponentMetaResultDb<CachedComponentMetaResult>` so warm-cache
/// hits on the audit-enabled path
/// (`VerterHost::get_component_meta_with_resolution`) can rehydrate
/// both halves without rerunning the cold resolver.
#[derive(Debug, Clone)]
pub struct CachedComponentMetaResult {
    pub analysis: verter_session_query::analysis::component_meta::ComponentMetaAnalysis,
    pub resolution_template: ResolutionTemplate,
    /// Owner canonical id used to reload `snapshot` via
    /// [`ProjectTypeStore::indexed()`] on rehydrate.
    pub canonical_id: Arc<str>,
    /// Owner whole-hash this template was produced against.
    pub whole_hash: Hash16,
}

impl ResolutionTemplate {
    /// Build a template by sanitizing a freshly-resolved
    /// [`crate::meta_resolve::ResolvedComponentMetaState`]. Strips
    /// `request_id`, `snapshot`, and `compute_audit`; keeps the
    /// content-addressed sidecars.
    #[must_use]
    pub fn from_resolved_state(resolved: &crate::meta_resolve::ResolvedComponentMetaState) -> Self {
        Self {
            mode: resolved.mode,
            whole_hash: resolved.whole_hash,
            resolved_macros: resolved.resolved_macros.clone(),
            resolved_type_registry: resolved.resolved_type_registry.clone(),
            resolved_type_registry_meta: resolved.resolved_type_registry_meta.clone(),
            evaluated_types: resolved.evaluated_types.clone(),
            fact_versions: resolved.fact_versions.clone(),
            surface_identities: resolved.surface_identities.clone(),
            origin_graph: resolved.origin_graph.clone(),
            completeness: resolved.completeness,
        }
    }

    /// Rehydrate the template into a per-request
    /// [`crate::meta_resolve::ResolvedComponentMetaState`]:
    ///
    /// - **`snapshot`** supplied by the private request root from exact indexed storage
    ///   at `(canonical_id, whole_hash)`. A bounded eviction race is handled
    ///   by that root before this pure reconstruction begins.
    /// - **`request_id`** is the caller-allocated fresh id.
    /// - **`compute_audit`** stays `None` on warm-cache hits — the
    ///   audit-record consumer observes `from_cache = true` and
    ///   `total_ms = 0` instead.
    /// - All other fields are restored from the cached template.
    pub fn rehydrate(
        &self,
        snapshot: verter_session_query::analysis::file_analysis::FileAnalysisSnapshot,
        request_id: u64,
    ) -> crate::meta_resolve::ResolvedComponentMetaState {
        crate::meta_resolve::ResolvedComponentMetaState {
            snapshot,
            mode: self.mode,
            whole_hash: self.whole_hash,
            resolved_macros: self.resolved_macros.clone(),
            resolved_type_registry: self.resolved_type_registry.clone(),
            resolved_type_registry_meta: self.resolved_type_registry_meta.clone(),
            evaluated_types: self.evaluated_types.clone(),
            fact_versions: self.fact_versions.clone(),
            compute_audit: None,
            surface_identities: self.surface_identities.clone(),
            origin_graph: self.origin_graph.clone(),
            request_id,
            // Rehydrated state was synthesised cold; suppression decisions
            // already applied at publish time. Synthesis diagnostics live
            // on the cached `ComponentMetaAnalysis.macro_expansion_diagnostics`.
            synthesis_diagnostics: Vec::new(),
            // Preserve the cached completeness; do NOT independently reset
            // suppression to `false`. `synthesis_should_suppress` is the bool
            // projection of `completeness` (a cached template is `Complete` in
            // production — only complete results admit — but rehydrate stays
            // honest to the stored value rather than fabricating one).
            completeness: self.completeness,
            synthesis_should_suppress: self.completeness.is_partial(),
        }
    }
}

/// Test-only accessor returning the merged carrier dep_signature
/// canonicals for an owner. Used by the dep-signature regression
/// tests — the slot-binding merge test
/// `slot_bindings_dep_signature_merges_carrier_deps`, the
/// component-meta surface-equivalence cross-file tests, and the
/// warm-invalidation oracle test — to inspect the dep-signature
/// carriers stored alongside the cached entry. Returns an empty vec
/// when the owner has no cached entry.
///
/// Constructs the lookup key with the same options fingerprint
/// the production `publish_component_meta_cache_entry` writes
/// (the default `ComponentMetaOptions` fingerprint), so the
/// lookup matches the published entry. A bare
/// `ComponentMetaOptionsFingerprint::default()` (= zeros) would
/// silently miss every published entry, masking real cache-key
/// drift behind a permanently empty result.
#[cfg(test)]
pub fn dep_signature_for_owner_in_test(
    host: &crate::VerterHost,
    owner_canonical: &str,
) -> Vec<std::sync::Arc<str>> {
    let store = host.project_type_store();
    let whole_hash = host
        .ensure_indexed_ready(owner_canonical)
        .map(|ir| ir.whole_hash)
        .unwrap_or_default();
    // Build the lookup key through the SAME production builder
    // `publish_component_meta_cache_entry` writes, so these
    // host-backed accessors address the exact published slot
    // (env axes included). A hand-rolled 2-field key would silently
    // miss every published entry after the R21 env-axis migration.
    let key = host.component_meta_result_key(
        owner_canonical,
        &crate::host_manage::ComponentMetaOptions::default(),
    );
    let backing = store.component_meta_results();
    match backing.get(&key, whole_hash) {
        Some(entry) => entry.read_set_signature.canonical_ids(),
        None => Vec::new(),
    }
}

/// Test-only accessor returning whether the owner has a cached
/// entry. Used by the slot-binding regression
/// `slot_bindings_skip_cache_on_budget_exceeded` to assert that
/// fatal-suppression synthesis runs do not warm the cache.
///
/// Constructs the lookup key with the same options fingerprint
/// the production `publish_component_meta_cache_entry` writes
/// (the default `ComponentMetaOptions` fingerprint).
#[cfg(test)]
pub fn has_owner_entry_in_test(host: &crate::VerterHost, owner_canonical: &str) -> bool {
    let store = host.project_type_store();
    let whole_hash = host
        .ensure_indexed_ready(owner_canonical)
        .map(|ir| ir.whole_hash)
        .unwrap_or_default();
    // Build the lookup key through the SAME production builder
    // `publish_component_meta_cache_entry` writes, so these
    // host-backed accessors address the exact published slot
    // (env axes included). A hand-rolled 2-field key would silently
    // miss every published entry after the R21 env-axis migration.
    let key = host.component_meta_result_key(
        owner_canonical,
        &crate::host_manage::ComponentMetaOptions::default(),
    );
    store
        .component_meta_results()
        .get(&key, whole_hash)
        .is_some()
}

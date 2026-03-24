//! Shared materialization and resolved-meta owner for component-meta.
//!
//! This module owns:
//! - mode selection (`ResolverMode::Type` vs `ResolverMode::Expanded`)
//! - materialized resolved outputs (`ResolvedComponentMetaState`)
//! - mode-aware caching
//! - JSDoc attachment and typed-tag resolution
//!
//! It calls into `host_resolve.rs` for declaration traversal — it does NOT
//! replace or duplicate the shared traversal substrate.
//!
//! # Architecture
//!
//! ```text
//! caller → resolve_component_meta(canonical, mode)
//!            ↓
//!        meta_resolve.rs  (orchestration, materialization, caching)
//!            ↓
//!        host_resolve.rs  (declaration graph traversal, shared cache)
//! ```

use crate::host_manage::{component_meta_debug, component_meta_debug_enabled};
use crate::types::{FileAnalysisSnapshot, Hash16, ResolverMode};
use crate::VerterHost;
use std::sync::Arc;
use std::time::Instant;
use verter_resolver::{
    run_component_meta_request, ComponentMetaEvalOutputs, ComponentMetaRequestHost,
    RequestSource, SingleflightRole, StoreView,
};

const STORE_VIEW_STABILITY_MAX_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
pub struct CapturedComponentMetaInputs {
    whole_hash: Hash16,
    snapshot: FileAnalysisSnapshot,
    owner_eval_source: Option<String>,
    owner_env: Option<verter_analysis::type_eval::EvalEnv>,
    dep_resolutions: rustc_hash::FxHashMap<String, crate::types::DependencyResolution>,
}

impl ComponentMetaRequestHost for VerterHost {
    type View = crate::resolver_store::HostStoreView;
    type Mode = ResolverMode;
    type Resolution = ResolvedComponentMetaState;
    type CapturedInputs = CapturedComponentMetaInputs;

    fn cache_key(&self, canonical: &str, mode: Self::Mode) -> verter_resolver::ResolutionNodeKey {
        resolved_meta_cache_key(canonical, mode)
    }

    fn snapshot_store_view(&self) -> Self::View {
        self.resolver_store_view()
    }

    fn view_mutation_epoch(&self, store_view: &Self::View) -> u64 {
        store_view.mutation_epoch()
    }

    fn current_store_view_epoch(&self) -> u64 {
        VerterHost::current_store_view_epoch(self)
    }

    fn capture_component_meta_inputs(
        &self,
        canonical: &str,
        view: &Self::View,
    ) -> Option<Self::CapturedInputs> {
        let snapshot = self.get_raw_analysis_snapshot_in_view(canonical, Some(view))?;
        let (source, cached_parse, whole_hash) = self.current_eval_state_in_view(canonical, Some(view))?;
        let owner_eval_source = VerterHost::build_eval_script_source(&source, cached_parse.as_deref());
        Some(CapturedComponentMetaInputs {
            whole_hash,
            snapshot,
            owner_eval_source: Some(owner_eval_source),
            owner_env: self.base_eval_env_in_view(canonical, Some(view)),
            dep_resolutions: self.dependency_resolutions_for_eval_in_view(canonical, Some(view))?,
        })
    }

    fn try_get_cached_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        store_view: &Self::View,
    ) -> Option<Self::Resolution> {
        self.try_get_cached_resolved_meta(canonical, mode, store_view)
    }

    fn compute_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        captured: Option<&Self::CapturedInputs>,
        store_view: Option<&Self::View>,
    ) -> Option<Self::Resolution> {
        if let Some(captured) = captured {
            return self.compute_component_meta_state_from_captured(
                canonical,
                mode,
                captured,
                store_view,
            );
        }

        let whole_hash = store_view
            .and_then(|view| view.whole_hash(canonical))
            .or_else(|| self.get_whole_hash(canonical))
            .unwrap_or_default();
        self.compute_component_meta_state(canonical, mode, whole_hash, store_view)
    }

    fn store_component_meta_result(
        &self,
        canonical: &str,
        mode: Self::Mode,
        result: &Self::Resolution,
    ) {
        self.store_cached_resolved_meta(canonical, mode, result, &result.fact_versions);
    }
}

/// Native declaration kind for the resolved pre-expansion type.
pub type ResolvedDeclarationKind = verter_resolver::ResolvedDeclarationKind;

/// Native pre-expansion declaration metadata retained by the shared resolver.
pub type ResolvedTypeDeclaration = verter_resolver::ResolvedTypeDeclaration;
pub type ResolvedTypeRegistryMeta = verter_resolver::ResolvedTypeRegistryMeta;
pub type ResolvedMacroMeta = verter_resolver::ResolvedMacroMeta;
pub type ResolvedNativeProp = verter_resolver::ResolvedNativeProp;
pub type ResolvedJsdocBlock = verter_resolver::ResolvedJsdocBlock;
pub type ResolvedJsdocTag = verter_resolver::ResolvedJsdocTag;

/// Host-owned sidecar result for component-meta / analysis enrichment.
///
/// Raw snapshot remains raw — resolved imported metadata lives in this sidecar.
/// `Expanded` mode carries materialized surfaces; `Type` mode carries
/// identity/location only.
#[derive(Debug, Clone)]
pub struct ResolvedComponentMetaState {
    /// The raw analysis snapshot (never mutated for enrichment).
    pub snapshot: FileAnalysisSnapshot,
    /// Which mode was used to produce this state.
    pub mode: ResolverMode,
    /// Content hash of the owner file at resolution time.
    pub whole_hash: Hash16,
    /// Resolved macro metadata from cross-file traversal.
    pub resolved_macros: Vec<ResolvedMacroMeta>,
    /// Resolved type registry entries (populated in `Expanded` mode).
    pub resolved_type_registry: Vec<verter_analysis::component_meta::ResolvedTypeAnalysis>,
    /// Native declaration metadata for each resolved type-registry entry.
    pub resolved_type_registry_meta: Vec<ResolvedTypeRegistryMeta>,
    /// Expanded types (populated in `Expanded` mode only).
    pub evaluated_types: Option<verter_analysis::type_expand::ExpandedComponentTypes>,
    /// Cached imported eval inputs from `resolve_component_meta(Expanded)`.
    /// Threaded through to `build_fallthrough_eval_env_with_inputs` to avoid
    /// a redundant second `imported_eval_inputs()` call in the fallthrough path.
    pub cached_eval_inputs: Option<Arc<crate::host_manage::ImportedEvalInputs>>,
    /// Semantic fact versions consumed while producing this resolved state.
    pub fact_versions: Vec<verter_resolver::FactVersionRef>,
}

impl VerterHost {
    /// Single host-backed resolver API for cross-file component-meta enrichment.
    ///
    /// This is the ONLY entry point for cross-file component-meta resolution.
    /// Mode is chosen explicitly by callers — never inferred.
    ///
    /// - `Type`: resolves symbol identity, canonical location, and attached JSDoc
    ///   without materializing expanded shapes.
    /// - `Expanded`: resolves the same way, then materializes props/emits/slots,
    ///   populates the type registry, and computes evaluated types.
    pub fn resolve_component_meta(
        &self,
        canonical_or_alias: &str,
        mode: ResolverMode,
    ) -> Option<ResolvedComponentMetaState> {
        let started = component_meta_debug_enabled().then(Instant::now);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let result = run_component_meta_request(
            self,
            &self.resolved_meta_singleflight,
            &canonical,
            mode,
            STORE_VIEW_STABILITY_MAX_ATTEMPTS,
        );

        if matches!(result.source, RequestSource::Cache) {
            self.provenance
                .resolver_node_cache_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if !(matches!(result.source, RequestSource::Cache) && result.attempts == 1) {
            self.provenance
                .resolver_node_cache_misses
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if let RequestSource::Flight { role, forked_lane } = result.source {
            if role == SingleflightRole::Follower {
                self.provenance
                    .resolver_singleflight_coalesced
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if forked_lane {
                self.provenance
                    .resolver_cross_view_lane_forks
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }

        if let Some(started) = started {
            match result.source {
                RequestSource::Cache => component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} cached attempt={} took {:?}",
                    canonical,
                    mode,
                    result.attempts.saturating_sub(1),
                    started.elapsed(),
                )),
                RequestSource::Flight { role, .. } => component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} role={:?} stable attempt={} total took {:?}",
                    canonical,
                    mode,
                    role,
                    result.attempts.saturating_sub(1),
                    started.elapsed(),
                )),
                RequestSource::Fallback => component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} retries_exhausted total took {:?}",
                    canonical,
                    mode,
                    started.elapsed(),
                )),
            }
        }

        result.value
    }

    pub(crate) fn compute_component_meta_state(
        &self,
        canonical: &str,
        mode: ResolverMode,
        whole_hash: Hash16,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<ResolvedComponentMetaState> {
        self.compute_component_meta_state_inner(canonical, mode, whole_hash, None, store_view)
    }

    fn compute_component_meta_state_from_captured(
        &self,
        canonical: &str,
        mode: ResolverMode,
        captured: &CapturedComponentMetaInputs,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<ResolvedComponentMetaState> {
        self.compute_component_meta_state_inner(
            canonical,
            mode,
            captured.whole_hash,
            Some(captured),
            store_view,
        )
    }

    fn compute_component_meta_state_inner(
        &self,
        canonical: &str,
        mode: ResolverMode,
        whole_hash: Hash16,
        captured: Option<&CapturedComponentMetaInputs>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<ResolvedComponentMetaState> {
        self.provenance
            .component_meta_resolved_state_recomputes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let snapshot = captured
            .map(|captured| captured.snapshot.clone())
            .or_else(|| self.get_raw_analysis_snapshot_in_view(canonical, store_view))?;
        let parts = verter_resolver::resolve_component_meta_parts(
            &HostComponentMetaResolver {
                host: self,
                store_view,
            },
            canonical,
            &snapshot,
            mode == ResolverMode::Expanded,
            captured,
        );
        let state = ResolvedComponentMetaState {
            snapshot,
            mode,
            whole_hash,
            resolved_macros: parts.resolved_macros,
            resolved_type_registry: parts.resolved_type_registry,
            resolved_type_registry_meta: parts.resolved_type_registry_meta,
            evaluated_types: parts.evaluated_types,
            cached_eval_inputs: parts.cached_eval_inputs,
            fact_versions: parts.fact_versions,
        };
        Some(state)
    }

    /// Get a raw analysis snapshot without any enrichment.
    ///
    /// This bypasses any legacy `get_analysis()` enrichment path, returning only the base snapshot
    /// with resolved imports and destructured bindings.
    pub(crate) fn get_raw_analysis_snapshot_in_view(
        &self,
        canonical: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<FileAnalysisSnapshot> {
        // Eviction gate (scheduler path)
        #[cfg(feature = "scheduler")]
        {
            if let Some(cc) = self.compile_cache.get(canonical) {
                if cc.evicted {
                    return None;
                }
            }
        }

        #[cfg(feature = "scheduler")]
        {
            let mut snapshot = if let Some(snapshot) = self.build_snapshot_from_scheduler(canonical)
            {
                let whole_hash = store_view
                    .and_then(|view| view.whole_hash(canonical))
                    .or_else(|| self.get_whole_hash(canonical))
                    .unwrap_or_default();
                if !self.store_view_allows_current_whole_hash(canonical, whole_hash, store_view) {
                    return None;
                }
                snapshot
            } else {
                let source = self.read_analysis_source_in_view(canonical, store_view)?;
                self.build_snapshot_from_source(canonical, &source)
            };
            self.resolve_snapshot_imports_in_view(canonical, &mut snapshot, store_view);
            self.enrich_destructured_bindings(&mut snapshot);
            if self.config.effective_scope().needs_template_analysis() {
                self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            }
            Some(snapshot)
        }

        #[cfg(not(feature = "scheduler"))]
        {
            use crate::shared::read_lock;

            let files = read_lock(&self.files);
            let entry = files.get(canonical)?;
            if !self.store_view_allows_current_whole_hash(canonical, entry.whole_hash, store_view) {
                return None;
            }
            // Use build_snapshot_from_entry for Arc::clone pointer bumps
            // instead of allocating new Arcs.
            let mut snapshot = Self::build_snapshot_from_entry(entry);
            drop(files);
            self.resolve_snapshot_imports_in_view(canonical, &mut snapshot, store_view);
            self.enrich_destructured_bindings(&mut snapshot);
            if self.config.effective_scope().needs_template_analysis() {
                self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            }
            Some(snapshot)
        }
    }

    fn try_get_cached_resolved_meta(
        &self,
        canonical: &str,
        mode: ResolverMode,
        store_view: &crate::resolver_store::HostStoreView,
    ) -> Option<ResolvedComponentMetaState> {
        let cache_key = resolved_meta_cache_key(canonical, mode);
        if let Some(cached) = self
            .resolved_meta_cache
            .get_if_valid(&cache_key, store_view)
        {
            self.mirror_cached_resolved_meta_arc(canonical, mode, cached.clone());
            return Some(cached.as_ref().clone());
        }

        #[cfg(feature = "scheduler")]
        {
            let entry = self.compile_cache.get(canonical)?;
            let cached = entry.cached_resolved_meta.get(&mode)?;
            if !cached
                .fact_versions
                .iter()
                .all(|fact| store_view.validates(fact))
            {
                return None;
            }
            self.resolved_meta_cache.insert_arc(
                cache_key,
                cached.state.clone(),
                cached.fact_versions.clone(),
            );
            Some(cached.state.as_ref().clone())
        }

        #[cfg(not(feature = "scheduler"))]
        {
            use crate::shared::read_lock;

            let files = read_lock(&self.files);
            let entry = files.get(canonical)?;
            let cached = entry.cached_resolved_meta.get(&mode)?;
            if !cached
                .fact_versions
                .iter()
                .all(|fact| store_view.validates(fact))
            {
                return None;
            }
            self.resolved_meta_cache.insert_arc(
                cache_key,
                cached.state.clone(),
                cached.fact_versions.clone(),
            );
            Some(cached.state.as_ref().clone())
        }
    }

    fn store_cached_resolved_meta(
        &self,
        canonical: &str,
        mode: ResolverMode,
        state: &ResolvedComponentMetaState,
        fact_versions: &[verter_resolver::FactVersionRef],
    ) {
        let state = Arc::new(state.clone());
        self.resolved_meta_cache.insert_arc(
            resolved_meta_cache_key(canonical, mode),
            state.clone(),
            fact_versions.to_vec(),
        );
        self.mirror_cached_resolved_meta_arc(canonical, mode, state);
    }

    fn mirror_cached_resolved_meta_arc(
        &self,
        canonical: &str,
        mode: ResolverMode,
        state: Arc<ResolvedComponentMetaState>,
    ) {
        let cached = crate::types::ResolvedComponentMetaCacheEntry {
            fact_versions: state.fact_versions.clone(),
            state,
        };

        #[cfg(feature = "scheduler")]
        {
            if let Some(mut entry) = self.compile_cache.get_mut(canonical) {
                entry.cached_resolved_meta.insert(mode, cached);
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            use crate::shared::write_lock;

            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(canonical) {
                entry.cached_resolved_meta.insert(mode, cached);
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) -> Vec<verter_resolver::FactVersionRef> {
        self.current_dependency_fact_versions_in_view(canonical, tracked_deps, None)
    }

    pub(crate) fn current_dependency_fact_versions_in_view(
        &self,
        canonical: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Vec<verter_resolver::FactVersionRef> {
        let mut facts = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();

        self.append_dependency_fact_versions_in_view(canonical, &mut facts, &mut seen, store_view);
        for dep in tracked_deps {
            self.append_dependency_fact_versions_in_view(
                dep.as_str(),
                &mut facts,
                &mut seen,
                store_view,
            );
        }

        facts
    }

    #[cfg(test)]
    pub(crate) fn fact_versions_match(
        &self,
        fact_versions: &[verter_resolver::FactVersionRef],
    ) -> bool {
        let view = self.resolver_store_view();
        fact_versions.iter().all(|fact| view.validates(fact))
    }

    fn append_dependency_fact_versions_in_view(
        &self,
        canonical: &str,
        facts: &mut Vec<verter_resolver::FactVersionRef>,
        seen: &mut rustc_hash::FxHashSet<verter_resolver::FactVersionRef>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) {
        let file_fact = verter_resolver::FactVersionRef::FileWholeHash {
            canonical_id: canonical.to_string(),
            hash: store_view
                .and_then(|view| view.whole_hash(canonical))
                .or_else(|| self.get_whole_hash(canonical))
                .unwrap_or_default(),
        };
        if seen.insert(file_fact.clone()) {
            facts.push(file_fact);
        }

        for kind in [
            verter_resolver::DerivedFactKind::ExportRegistry,
            verter_resolver::DerivedFactKind::BarrelSurface,
        ] {
            if let Some(hash) = store_view
                .and_then(|view| view.derived_hash(canonical, kind))
                .or_else(|| self.current_derived_fact_hash(canonical, kind))
            {
                let fact = verter_resolver::FactVersionRef::DerivedFactHash {
                    canonical_id: canonical.to_string(),
                    kind,
                    hash,
                };
                if seen.insert(fact.clone()) {
                    facts.push(fact);
                }
            }
        }

        if let Some(generation) = store_view
            .and_then(|view| view.barrel_generation(canonical))
            .or_else(|| self.current_barrel_generation(canonical))
        {
            let fact = verter_resolver::FactVersionRef::BarrelGeneration {
                canonical_id: canonical.to_string(),
                generation,
            };
            if seen.insert(fact.clone()) {
                facts.push(fact);
            }
        }
    }

    fn current_derived_fact_hash(
        &self,
        canonical_id: &str,
        kind: verter_resolver::DerivedFactKind,
    ) -> Option<Hash16> {
        match kind {
            verter_resolver::DerivedFactKind::DirectSource => self.get_whole_hash(canonical_id),
            verter_resolver::DerivedFactKind::ExportRegistry => {
                #[cfg(feature = "scheduler")]
                {
                    self.compile_cache.get(canonical_id).and_then(|cc| {
                        cc.export_registry
                            .as_ref()
                            .map(|registry| registry.source_hash)
                    })
                }
                #[cfg(not(feature = "scheduler"))]
                {
                    None
                }
            }
            verter_resolver::DerivedFactKind::BarrelSurface => {
                #[cfg(feature = "scheduler")]
                {
                    self.compile_cache.get(canonical_id).and_then(|cc| {
                        cc.barrel_export_surface
                            .as_ref()
                            .map(|surface| surface.source_hash)
                    })
                }
                #[cfg(not(feature = "scheduler"))]
                {
                    None
                }
            }
            verter_resolver::DerivedFactKind::Route
            | verter_resolver::DerivedFactKind::ExactResolution => None,
        }
    }

    fn current_barrel_generation(&self, canonical_id: &str) -> Option<u64> {
        #[cfg(feature = "scheduler")]
        {
            self.compile_cache.get(canonical_id).and_then(|cc| {
                cc.barrel_export_surface
                    .as_ref()
                    .map(|surface| surface.generation)
            })
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let _ = canonical_id;
            None
        }
    }
}

fn resolved_meta_cache_key(
    canonical: &str,
    mode: ResolverMode,
) -> verter_resolver::ResolutionNodeKey {
    verter_resolver::ResolutionNodeKey {
        symbol_id: canonical.to_string(),
        node_kind: verter_resolver::ResolutionNodeKind::Assemble,
        traversal_lens: verter_resolver::TraversalLens::StructuralObject,
        member_path_hash: 0,
        type_args_hash: 0,
        behavior_flags: match mode {
            ResolverMode::Type => 1,
            ResolverMode::Expanded => 2,
        },
    }
}

struct HostComponentMetaResolver<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a crate::resolver_store::HostStoreView>,
}

impl verter_resolver::DeclarationMetadataResolver for HostComponentMetaResolver<'_> {
    fn resolve_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<verter_resolver::ResolvedExportTarget> {
        self.host
            .resolve_exports_in_view(dep_canonical, self.store_view)
            .into_iter()
            .find(|export| export.name == requested_name)
            .map(|export| verter_resolver::ResolvedExportTarget {
                source_canonical_id: export.source_canonical_id,
                source_name: export.source_name,
            })
    }

    fn get_export_span_follow_reexports(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<verter_span::Span> {
        self.host
            .get_export_span_follow_reexports_in_view(
                dep_canonical,
                requested_name,
                self.store_view,
            )
            .map(|(_, start, end)| verter_span::Span::new(start, end))
    }

    fn read_source(&self, canonical_source: &str) -> Option<String> {
        read_full_source(self.host, canonical_source, self.store_view)
    }

    fn type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<verter_analysis::type_eval::DeclarationId> {
        self.host
            .base_eval_env_in_view(canonical_source, self.store_view)
            .and_then(|env| env.type_declaration_id(resolved_name))
    }

    fn resolve_type_dependency_canonical(
        &self,
        from_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        self.host.resolve_type_dependency_canonical_in_view(
            from_canonical,
            import_source,
            self.store_view,
        )
    }
}

impl verter_resolver::ComponentMetaResolverHost for HostComponentMetaResolver<'_> {
    type Snapshot = FileAnalysisSnapshot;
    type EvalContext = CapturedComponentMetaInputs;
    type ImportedInputs = crate::host_manage::ImportedEvalInputs;

    fn snapshot_imports<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_analysis::types::AnalyzedImport] {
        snapshot.imports.as_slice()
    }

    fn snapshot_macros<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_analysis::types::AnalyzedMacro] {
        snapshot.macros.as_slice()
    }

    fn snapshot_macro_type_deps<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_analysis::types::MacroTypeDep] {
        snapshot.macro_type_deps.as_slice()
    }

    fn build_eval_outputs(
        &self,
        owner_canonical: &str,
        snapshot: &Self::Snapshot,
        eval_context: Option<&Self::EvalContext>,
    ) -> ComponentMetaEvalOutputs<Self::ImportedInputs> {
        let eval_started = component_meta_debug_enabled().then(Instant::now);
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "resolve_component_meta owner={} mode={:?} step=evaluated_types:start imports={} macro_type_deps={}",
                owner_canonical,
                ResolverMode::Expanded,
                snapshot.imports.len(),
                snapshot.macro_type_deps.len(),
            ));
        }
        let dep_resolutions = eval_context
            .map(|captured| captured.dep_resolutions.clone())
            .unwrap_or_else(|| {
                self.host
                    .dependency_resolutions_for_eval_in_view(owner_canonical, self.store_view)
                    .unwrap_or_default()
            });
        let imported_inputs = Arc::new(self.host.imported_eval_inputs_with_owner_context_in_view(
            owner_canonical,
            snapshot,
            &dep_resolutions,
            eval_context.and_then(|captured| captured.owner_eval_source.as_deref()),
            eval_context.and_then(|captured| captured.owner_env.as_ref()),
            self.store_view,
        ));
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "resolve_component_meta owner={} mode={:?} step=evaluated_types:imported_inputs_done sources={} type_aliases={} tracked_deps={}",
                owner_canonical,
                ResolverMode::Expanded,
                imported_inputs.sources.len(),
                imported_inputs.type_aliases.len(),
                imported_inputs.canonical_dependencies.len(),
            ));
        }
        let mut tracked_dependencies = imported_inputs.canonical_dependencies.clone();
        tracked_dependencies.extend(self.host.cache_dependency_candidates_from_snapshot(
            owner_canonical,
            snapshot,
            &dep_resolutions,
        ));
        let computed_eval_types = if imported_inputs.overflow.is_some() {
            None
        } else {
            self.host
                .compute_evaluated_types_with_tracking_from_owner_context_in_view(
                    owner_canonical,
                    snapshot,
                    &imported_inputs,
                    eval_context.and_then(|captured| captured.owner_eval_source.as_deref()),
                    eval_context.and_then(|captured| captured.owner_env.clone()),
                    self.store_view,
                )
        };
        if let Some(computed) = computed_eval_types.as_ref() {
            tracked_dependencies.extend(computed.discovered_dependencies.iter().cloned());
        }
        let evaluated_types = computed_eval_types.and_then(|computed| computed.evaluated_types);
        if let Some(eval_started) = eval_started {
            component_meta_debug(format!(
                "resolve_component_meta owner={} mode={:?} evaluated_types took {:?} has_output={}",
                owner_canonical,
                ResolverMode::Expanded,
                eval_started.elapsed(),
                evaluated_types
                    .as_ref()
                    .is_some_and(|types| !types.is_empty()),
            ));
        }
        ComponentMetaEvalOutputs {
            evaluated_types,
            cached_eval_inputs: Some(imported_inputs),
            tracked_dependencies,
        }
    }

    fn resolve_macro_elements(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut rustc_hash::FxHashMap<
            (String, String),
            Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>,
        >,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements> {
        self.host
            .resolve_external_type_from_loaded_files_in_view(
                owner_canonical,
                import_source,
                exported_name,
                tracked_deps,
                resolution_deps,
                cache,
                visiting,
                false,
                verter_vfs::ResolveRequestKind::TypeImport,
                true,
                None,
                0,
                self.store_view,
            )
            .ok()
            .flatten()
    }

    fn resolve_jsdoc_block(
        &self,
        canonical_source: &str,
        span: verter_span::Span,
        expanded: bool,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut rustc_hash::FxHashMap<
            (String, String),
            Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>,
        >,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<ResolvedJsdocBlock> {
        resolve_jsdoc_block(
            self.host,
            canonical_source,
            span,
            if expanded {
                ResolverMode::Expanded
            } else {
                ResolverMode::Type
            },
            tracked_deps,
            cache,
            visiting,
            verter_vfs::ResolveRequestKind::TypeImport,
            self.store_view,
        )
    }

    fn sync_transitive_macro_type_dependencies(
        &self,
        canonical_id: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) {
        self.host
            .sync_transitive_macro_type_dependencies(canonical_id, tracked_deps);
    }

    fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) -> Vec<verter_resolver::FactVersionRef> {
        self.host
            .current_dependency_fact_versions_in_view(canonical, tracked_deps, self.store_view)
    }
}

pub(crate) fn resolve_type_declaration_in_view(
    host: &VerterHost,
    dep_canonical: &str,
    requested_name: &str,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> ResolvedTypeDeclaration {
    verter_resolver::resolve_type_declaration(
        &HostComponentMetaResolver { host, store_view },
        dep_canonical,
        requested_name,
    )
}

fn read_full_source(
    host: &VerterHost,
    canonical_source: &str,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> Option<String> {
    host.read_analysis_source_in_view(canonical_source, store_view)
        .map(|source| source.to_string())
        .or_else(|| {
            host.workspace
                .read()
                .read_file(canonical_source)
                .and_then(|source| {
                    let whole_hash = crate::hash::hash_16(source.as_bytes());
                    if !host.store_view_allows_current_whole_hash(
                        canonical_source,
                        whole_hash,
                        store_view,
                    ) {
                        return None;
                    }
                    Some(source.to_string())
                })
        })
}

#[allow(clippy::too_many_arguments)]
fn resolve_jsdoc_block(
    host: &VerterHost,
    canonical_source: &str,
    span: verter_span::Span,
    mode: ResolverMode,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    cache: &mut rustc_hash::FxHashMap<
        (String, String),
        Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>,
    >,
    visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    kind: verter_vfs::ResolveRequestKind,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> Option<ResolvedJsdocBlock> {
    if span.start == 0 && span.end == 0 {
        return None;
    }

    let source = read_full_source(host, canonical_source, store_view)?;
    let (description, tags) =
        verter_analysis::jsdoc::extract_jsdoc_near_offset(&source, span.start);
    if description.is_none() && tags.is_empty() {
        return None;
    }

    Some(ResolvedJsdocBlock {
        description,
        tags: tags
            .into_iter()
            .map(|tag| {
                map_jsdoc_tag(
                    host,
                    canonical_source,
                    mode,
                    tracked_deps,
                    cache,
                    visiting,
                    kind,
                    store_view,
                    tag,
                )
            })
            .collect(),
    })
}

#[allow(clippy::too_many_arguments)]
fn map_jsdoc_tag(
    host: &VerterHost,
    canonical_source: &str,
    mode: ResolverMode,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    cache: &mut rustc_hash::FxHashMap<
        (String, String),
        Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>,
    >,
    visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    kind: verter_vfs::ResolveRequestKind,
    store_view: Option<&crate::resolver_store::HostStoreView>,
    tag: verter_analysis::types::JsdocTag,
) -> ResolvedJsdocTag {
    let (text, raw_type, subject_name) = parse_jsdoc_tag_payload(tag.name.as_str(), tag.text);
    let resolved_type = if mode == ResolverMode::Expanded {
        raw_type.as_deref().and_then(|raw_type| {
            resolve_jsdoc_tag_type(
                host,
                canonical_source,
                raw_type,
                tracked_deps,
                cache,
                visiting,
                kind,
                store_view,
            )
        })
    } else {
        None
    };
    ResolvedJsdocTag {
        name: tag.name,
        text,
        raw_type,
        subject_name,
        resolved_type,
    }
}

fn parse_jsdoc_tag_payload(
    tag_name: &str,
    text: Option<String>,
) -> (Option<String>, Option<String>, Option<String>) {
    let Some(text) = text else {
        return (None, None, None);
    };
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix('{') else {
        return (Some(text), None, None);
    };
    // Depth-aware brace matching: find the closing `}` that matches the
    // opening `{`, handling nested braces like `{Record<string, {nested: true}>}`.
    let end = {
        let mut depth = 0u32;
        let mut found = None;
        for (i, ch) in rest.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    if depth == 0 {
                        found = Some(i);
                        break;
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
        found
    };
    let Some(end) = end else {
        return (Some(text), None, None);
    };

    let raw_type = Some(rest[..end].trim().to_string());
    let trailing = rest[end + 1..].trim();
    if trailing.is_empty() {
        return (None, raw_type, None);
    }

    if matches!(tag_name, "param" | "arg" | "argument") {
        let mut parts = trailing.splitn(2, char::is_whitespace);
        let subject_name = parts.next().map(str::to_string);
        let text = parts
            .next()
            .map(str::trim)
            .filter(|rest| !rest.is_empty())
            .map(str::to_string);
        (text, raw_type, subject_name)
    } else {
        (Some(trailing.to_string()), raw_type, None)
    }
}

fn resolve_jsdoc_tag_type(
    host: &VerterHost,
    canonical_source: &str,
    raw_type: &str,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    cache: &mut rustc_hash::FxHashMap<
        (String, String),
        Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>,
    >,
    visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    kind: verter_vfs::ResolveRequestKind,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> Option<verter_analysis::type_expr::TypeExpr> {
    let source = read_full_source(host, canonical_source, store_view)?;
    let synthetic_source = format!("{source}\nexport type __VerterJsdocTag = {raw_type};");

    let import_alloc = oxc_allocator::Allocator::new();
    let extracted = verter_core::utils::oxc::vue::resolve_type::extract_imported_type_bindings(
        &synthetic_source,
        &import_alloc,
    );
    let required_import_names =
        verter_core::utils::oxc::vue::resolve_type::collect_required_import_names_for_external_type(
            "__VerterJsdocTag",
            &synthetic_source,
            &import_alloc,
        );

    let mut companion_types = rustc_hash::FxHashMap::default();
    for binding in &extracted.bindings {
        let required_aliases =
            verter_core::utils::oxc::vue::resolve_type::required_import_alias_names_for_binding(
                binding,
                &required_import_names,
            );
        for required_alias in required_aliases {
            let Some(imported_name) =
                verter_core::utils::oxc::vue::resolve_type::imported_member_name_for_required_alias(
                    binding,
                    &required_alias,
                )
            else {
                continue;
            };
            let mut resolution_deps = std::collections::BTreeSet::new();
            if let Ok(Some(resolved)) = host.resolve_external_type_from_loaded_files_in_view(
                canonical_source,
                &binding.source,
                &imported_name,
                tracked_deps,
                &mut resolution_deps,
                cache,
                visiting,
                false,
                kind,
                true,
                None,
                0,
                store_view,
            ) {
                companion_types.entry(required_alias).or_insert(resolved);
            }
        }
    }

    let resolve_alloc = oxc_allocator::Allocator::new();
    let resolved =
        verter_core::utils::oxc::vue::resolve_type::resolve_external_type_with_companion(
            "__VerterJsdocTag",
            &synthetic_source,
            &companion_types,
            &resolve_alloc,
        )?;
    Some(verter_resolver::resolved_elements_to_type_expr_via_type_text(&resolved))
}

#[cfg(test)]
#[path = "meta_resolve_tests.rs"]
mod meta_resolve_tests;

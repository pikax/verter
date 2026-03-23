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
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Instant;
use verter_resolver::{
    run_stable_request, RequestSource, SingleflightRole, StableRequestExecutor, StoreView,
};

const STORE_VIEW_STABILITY_MAX_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
struct CapturedComponentMetaInputs {
    whole_hash: Hash16,
    snapshot: FileAnalysisSnapshot,
    owner_eval_source: Option<String>,
    owner_env: Option<verter_analysis::type_eval::EvalEnv>,
    dep_resolutions: rustc_hash::FxHashMap<String, crate::types::DependencyResolution>,
}

struct ComponentMetaRequestExecutor<'a> {
    host: &'a VerterHost,
    canonical: String,
    mode: ResolverMode,
    last_snapshot_epoch: Option<u64>,
    captured_inputs: Option<CapturedComponentMetaInputs>,
}

impl<'a> ComponentMetaRequestExecutor<'a> {
    fn new(host: &'a VerterHost, canonical: String, mode: ResolverMode) -> Self {
        Self {
            host,
            canonical,
            mode,
            last_snapshot_epoch: None,
            captured_inputs: None,
        }
    }

    fn capture_owner_inputs(&self) -> Option<CapturedComponentMetaInputs> {
        let snapshot = self.host.get_raw_analysis_snapshot(&self.canonical)?;
        let (source, cached_parse, whole_hash) = self.host.current_eval_state(&self.canonical)?;
        let owner_eval_source =
            VerterHost::build_eval_script_source(&source, cached_parse.as_deref());
        Some(CapturedComponentMetaInputs {
            whole_hash,
            snapshot,
            owner_eval_source: Some(owner_eval_source),
            owner_env: self.host.base_eval_env(&self.canonical),
            dep_resolutions: self.host.dependency_resolutions_for_eval(&self.canonical),
        })
    }
}

impl<'a>
    StableRequestExecutor<verter_resolver::ResolutionNodeKey, Option<ResolvedComponentMetaState>>
    for ComponentMetaRequestExecutor<'a>
{
    type View = crate::resolver_store::HostStoreView;
    type Error = ();

    fn cache_key(&self) -> verter_resolver::ResolutionNodeKey {
        resolved_meta_cache_key(&self.canonical, self.mode)
    }

    fn snapshot_view(&mut self) -> Self::View {
        for _ in 0..STORE_VIEW_STABILITY_MAX_ATTEMPTS {
            let view = self.host.resolver_store_view();
            let captured_inputs = self.capture_owner_inputs();
            if self.host.current_store_view_epoch() == view.mutation_epoch() {
                self.last_snapshot_epoch = Some(view.mutation_epoch());
                self.captured_inputs = captured_inputs;
                return view;
            }
        }

        let view = self.host.resolver_store_view();
        self.last_snapshot_epoch = Some(view.mutation_epoch());
        self.captured_inputs = self.capture_owner_inputs();
        view
    }

    fn try_get_cached(&mut self, view: &Self::View) -> Option<Option<ResolvedComponentMetaState>> {
        self.host
            .try_get_cached_resolved_meta(&self.canonical, self.mode, view)
            .map(Some)
    }

    fn compute(
        &mut self,
        _view: &Self::View,
    ) -> Result<Option<ResolvedComponentMetaState>, Self::Error> {
        if let Some(captured) = self.captured_inputs.as_ref() {
            return Ok(self.host.compute_component_meta_state_from_captured(
                &self.canonical,
                self.mode,
                captured,
            ));
        }

        let whole_hash = self
            .host
            .get_whole_hash(&self.canonical)
            .unwrap_or_default();
        Ok(self
            .host
            .compute_component_meta_state(&self.canonical, self.mode, whole_hash))
    }

    fn is_stable(&mut self, _view: &Self::View) -> bool {
        self.last_snapshot_epoch
            .is_some_and(|epoch| self.host.current_store_view_epoch() == epoch)
    }

    fn store_stable(&mut self, value: &Option<ResolvedComponentMetaState>) {
        if let Some(state) = value.as_ref() {
            self.host.store_cached_resolved_meta(
                &self.canonical,
                self.mode,
                state,
                &state.fact_versions,
            );
        }
    }

    fn max_attempts(&self) -> usize {
        STORE_VIEW_STABILITY_MAX_ATTEMPTS
    }
}

/// Native declaration kind for the resolved pre-expansion type.
pub type ResolvedDeclarationKind = verter_resolver::ResolvedDeclarationKind;

/// Native pre-expansion declaration metadata retained by the shared resolver.
pub type ResolvedTypeDeclaration = verter_resolver::ResolvedTypeDeclaration;

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

/// Native provenance retained for an expanded type-registry entry.
#[derive(Debug, Clone)]
pub struct ResolvedTypeRegistryMeta {
    /// Registry key used by component-meta / compat.
    pub name: String,
    /// Pre-expansion declaration metadata for the source declaration.
    pub declaration: ResolvedTypeDeclaration,
}

/// Resolved metadata for a single macro's cross-file type.
#[derive(Debug, Clone)]
pub struct ResolvedMacroMeta {
    /// Index of the target macro in the raw snapshot.
    pub macro_index: usize,
    /// Which macro kind this resolved metadata belongs to.
    pub macro_kind: verter_analysis::AnalyzedMacroKind,
    /// The type name that was resolved (e.g., "ButtonProps").
    pub type_name: String,
    /// The import specifier (e.g., "./types").
    pub import_source: String,
    /// Pre-expansion declaration metadata for the resolved symbol.
    pub declaration: ResolvedTypeDeclaration,
    /// Native resolved props prior to compat/public filtering.
    pub native_props: Vec<ResolvedNativeProp>,
    /// Resolved prop fields (populated in `Expanded` mode).
    pub props: Vec<verter_analysis::AnalyzedPropField>,
    /// Resolved emit fields (populated in `Expanded` mode).
    pub emits: Vec<verter_analysis::AnalyzedEmitField>,
    /// Resolved slot fields (populated in `Expanded` mode).
    pub slots: Vec<verter_analysis::AnalyzedSlotField>,
    /// Resolved JSDoc block attached to the declaration.
    pub jsdoc: Option<ResolvedJsdocBlock>,
}

/// Native resolved prop metadata retained before compat/public projection.
pub type ResolvedNativeProp = verter_resolver::ResolvedNativeProp;

/// Resolved JSDoc block with parsed tags.
#[derive(Debug, Clone)]
pub struct ResolvedJsdocBlock {
    /// The raw description text.
    pub description: Option<String>,
    /// Parsed and optionally type-resolved tags.
    pub tags: Vec<ResolvedJsdocTag>,
}

/// A JSDoc tag with optional type resolution.
#[derive(Debug, Clone)]
pub struct ResolvedJsdocTag {
    /// Tag name (e.g., "param", "type", "returns").
    pub name: String,
    /// Raw text after the tag name.
    pub text: Option<String>,
    /// Raw type expression from braces (e.g., "Foo | Bar" from `{Foo | Bar}`).
    pub raw_type: Option<String>,
    /// Subject name for param-like tags (e.g., "id" from `@param id`).
    pub subject_name: Option<String>,
    /// Expanded type information for typed JSDoc tags in `Expanded` mode.
    pub resolved_type: Option<verter_analysis::type_expr::TypeExpr>,
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
        let mut executor = ComponentMetaRequestExecutor::new(self, canonical.clone(), mode);
        let result = run_stable_request(&self.resolved_meta_singleflight, &mut executor)
            .expect("resolved-meta request execution is infallible");

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

    fn compute_component_meta_state(
        &self,
        canonical: &str,
        mode: ResolverMode,
        whole_hash: Hash16,
    ) -> Option<ResolvedComponentMetaState> {
        self.compute_component_meta_state_inner(canonical, mode, whole_hash, None)
    }

    fn compute_component_meta_state_from_captured(
        &self,
        canonical: &str,
        mode: ResolverMode,
        captured: &CapturedComponentMetaInputs,
    ) -> Option<ResolvedComponentMetaState> {
        self.compute_component_meta_state_inner(
            canonical,
            mode,
            captured.whole_hash,
            Some(captured),
        )
    }

    fn compute_component_meta_state_inner(
        &self,
        canonical: &str,
        mode: ResolverMode,
        whole_hash: Hash16,
        captured: Option<&CapturedComponentMetaInputs>,
    ) -> Option<ResolvedComponentMetaState> {
        self.provenance
            .component_meta_resolved_state_recomputes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Step 1: Get the raw analysis snapshot (without enrichment).
        let snapshot = captured
            .map(|captured| captured.snapshot.clone())
            .or_else(|| self.get_raw_analysis_snapshot(canonical))?;

        let mut resolved_macros = Vec::new();
        let mut resolved_type_registry = Vec::new();
        let mut resolved_type_registry_meta = Vec::new();
        let mut seen_registry_names = rustc_hash::FxHashSet::default();
        let mut cache = rustc_hash::FxHashMap::default();
        let mut visiting = rustc_hash::FxHashSet::default();
        let mut tracked_deps = std::collections::BTreeSet::new();
        let kind = verter_vfs::ResolveRequestKind::TypeImport;

        let (evaluated_types, cached_eval_inputs) = if mode == ResolverMode::Expanded {
            let eval_started = component_meta_debug_enabled().then(Instant::now);
            if component_meta_debug_enabled() {
                component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} step=evaluated_types:start imports={} macro_type_deps={}",
                    canonical,
                    mode,
                    snapshot.imports.len(),
                    snapshot.macro_type_deps.len(),
                ));
            }
            let dep_resolutions = captured
                .map(|captured| captured.dep_resolutions.clone())
                .unwrap_or_else(|| self.dependency_resolutions_for_eval(canonical));
            let imported_inputs = Arc::new(self.imported_eval_inputs_with_owner_context(
                canonical,
                &snapshot,
                &dep_resolutions,
                captured.and_then(|captured| captured.owner_eval_source.as_deref()),
                captured.and_then(|captured| captured.owner_env.as_ref()),
            ));
            if component_meta_debug_enabled() {
                component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} step=evaluated_types:imported_inputs_done sources={} type_aliases={} tracked_deps={}",
                    canonical,
                    mode,
                    imported_inputs.sources.len(),
                    imported_inputs.type_aliases.len(),
                    imported_inputs.canonical_dependencies.len(),
                ));
            }
            tracked_deps.extend(imported_inputs.canonical_dependencies.iter().cloned());
            tracked_deps.extend(self.cache_dependency_candidates_from_snapshot(
                canonical,
                &snapshot,
                &dep_resolutions,
            ));
            let computed_eval_types = if imported_inputs.overflow.is_some() {
                None
            } else {
                self.compute_evaluated_types_with_tracking_from_owner_context(
                    canonical,
                    &snapshot,
                    &imported_inputs,
                    captured.and_then(|captured| captured.owner_eval_source.as_deref()),
                    captured.and_then(|captured| captured.owner_env.clone()),
                )
            };
            if let Some(computed) = computed_eval_types.as_ref() {
                tracked_deps.extend(computed.discovered_dependencies.iter().cloned());
            }
            let eval_types = computed_eval_types.and_then(|computed| computed.evaluated_types);
            if let Some(eval_started) = eval_started {
                component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} evaluated_types took {:?} has_output={}",
                    canonical,
                    mode,
                    eval_started.elapsed(),
                    eval_types.as_ref().is_some_and(|types| !types.is_empty()),
                ));
            }
            (eval_types, Some(imported_inputs))
        } else {
            (None, None)
        };

        let macro_resolution_started = component_meta_debug_enabled().then(Instant::now);
        let macro_type_deps: Vec<verter_analysis::MacroTypeDep> =
            snapshot.macro_type_deps.iter().cloned().collect();
        for dep in &macro_type_deps {
            let macro_index = dep.macro_index;
            let dep_exported_name = macro_dep_exported_type_name(&snapshot, dep);
            let dep_canonical = self
                .resolve_type_dependency_canonical(canonical, &dep.import_source)
                .unwrap_or_default();
            let declaration =
                resolve_type_declaration(self, &dep_canonical, dep_exported_name.as_ref());
            let jsdoc = resolve_jsdoc_block(
                self,
                declaration.canonical_source.as_str(),
                declaration.span,
                mode,
                &mut tracked_deps,
                &mut cache,
                &mut visiting,
                kind,
            );

            if !dep_canonical.is_empty() {
                tracked_deps.insert(dep_canonical.clone());
            }
            if !declaration.canonical_source.is_empty()
                && declaration.canonical_source != dep_canonical
            {
                tracked_deps.insert(declaration.canonical_source.clone());
            }

            match mode {
                ResolverMode::Type => {
                    resolved_macros.push(ResolvedMacroMeta {
                        macro_index,
                        macro_kind: dep.macro_kind,
                        type_name: dep.type_name.clone(),
                        import_source: dep.import_source.clone(),
                        declaration: declaration.clone(),
                        native_props: Vec::new(),
                        props: Vec::new(),
                        emits: Vec::new(),
                        slots: Vec::new(),
                        jsdoc: jsdoc.clone(),
                    });
                }
                ResolverMode::Expanded => {
                    let skip_external = should_ignore_external_macro_type(dep);
                    if skip_external {
                        resolved_macros.push(ResolvedMacroMeta {
                            macro_index,
                            macro_kind: dep.macro_kind,
                            type_name: dep.type_name.clone(),
                            import_source: dep.import_source.clone(),
                            declaration: declaration.clone(),
                            native_props: Vec::new(),
                            props: Vec::new(),
                            emits: Vec::new(),
                            slots: Vec::new(),
                            jsdoc: jsdoc.clone(),
                        });
                        continue;
                    }

                    let mut resolution_deps = std::collections::BTreeSet::new();
                    let resolved = self.resolve_external_type_from_loaded_files(
                        canonical,
                        &dep.import_source,
                        dep_exported_name.as_ref(),
                        &mut tracked_deps,
                        &mut resolution_deps,
                        &mut cache,
                        &mut visiting,
                        false,
                        kind,
                        true,
                        None,
                        0,
                    );

                    match resolved {
                        Ok(Some(elements)) => {
                            let projected = verter_resolver::project_macro_surfaces(
                                read_full_source(self, declaration.canonical_source.as_str())
                                    .as_deref(),
                                dep.macro_kind,
                                &elements,
                            );
                            if seen_registry_names.insert(dep.type_name.clone()) {
                                resolved_type_registry.push(
                                    verter_analysis::component_meta::ResolvedTypeAnalysis {
                                        name: dep.type_name.clone(),
                                        type_expr: crate::host_manage::resolved_elements_to_type_expr_via_type_text(&elements),
                                        type_expansion: None,
                                    },
                                );
                                resolved_type_registry_meta.push(ResolvedTypeRegistryMeta {
                                    name: dep.type_name.clone(),
                                    declaration: declaration.clone(),
                                });
                            }

                            resolved_macros.push(ResolvedMacroMeta {
                                macro_index,
                                macro_kind: dep.macro_kind,
                                type_name: dep.type_name.clone(),
                                import_source: dep.import_source.clone(),
                                declaration: declaration.clone(),
                                native_props: projected.native_props,
                                props: projected.props,
                                emits: projected.emits,
                                slots: projected.slots,
                                jsdoc: jsdoc.clone(),
                            });
                        }
                        Ok(None) | Err(_) => {
                            resolved_macros.push(ResolvedMacroMeta {
                                macro_index,
                                macro_kind: dep.macro_kind,
                                type_name: dep.type_name.clone(),
                                import_source: dep.import_source.clone(),
                                declaration: declaration.clone(),
                                native_props: Vec::new(),
                                props: Vec::new(),
                                emits: Vec::new(),
                                slots: Vec::new(),
                                jsdoc,
                            });
                        }
                    }
                }
            }
        }
        if let Some(macro_resolution_started) = macro_resolution_started {
            component_meta_debug(format!(
                "resolve_component_meta owner={} mode={:?} macro_resolution deps={} took {:?}",
                canonical,
                mode,
                macro_type_deps.len(),
                macro_resolution_started.elapsed(),
            ));
        }

        if mode == ResolverMode::Expanded {
            for mac in snapshot.macros.iter() {
                for resolved in &mac.resolved_local_types {
                    if seen_registry_names.insert(resolved.name.clone()) {
                        resolved_type_registry.push(
                            verter_analysis::component_meta::ResolvedTypeAnalysis {
                                name: resolved.name.clone(),
                                type_expr: resolved.type_expr.clone().unwrap_or_else(|| {
                                    verter_analysis::type_expr_lower::parse_type_annotation(
                                        &resolved.expanded,
                                    )
                                }),
                                type_expansion: None,
                            },
                        );
                        resolved_type_registry_meta.push(ResolvedTypeRegistryMeta {
                            name: resolved.name.clone(),
                            declaration: resolve_local_type_declaration(self, canonical, resolved),
                        });
                    }
                }
            }
        }

        self.sync_transitive_macro_type_dependencies(canonical, &tracked_deps);

        let fact_versions = self.current_dependency_fact_versions(canonical, &tracked_deps);
        let state = ResolvedComponentMetaState {
            snapshot,
            mode,
            whole_hash,
            resolved_macros,
            resolved_type_registry,
            resolved_type_registry_meta,
            evaluated_types,
            cached_eval_inputs,
            fact_versions: fact_versions.clone(),
        };
        Some(state)
    }

    /// Get a raw analysis snapshot without any enrichment.
    ///
    /// This bypasses any legacy `get_analysis()` enrichment path, returning only the base snapshot
    /// with resolved imports and destructured bindings.
    pub(crate) fn get_raw_analysis_snapshot(
        &self,
        canonical: &str,
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
            let mut snapshot = self.build_snapshot_from_scheduler(canonical)?;
            self.resolve_snapshot_imports(canonical, &mut snapshot);
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
            // Use build_snapshot_from_entry for Arc::clone pointer bumps
            // instead of allocating new Arcs.
            let mut snapshot = Self::build_snapshot_from_entry(entry);
            drop(files);
            self.resolve_snapshot_imports(canonical, &mut snapshot);
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

    pub(crate) fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) -> Vec<verter_resolver::FactVersionRef> {
        let mut facts = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();

        self.append_dependency_fact_versions(canonical, &mut facts, &mut seen);
        for dep in tracked_deps {
            self.append_dependency_fact_versions(dep.as_str(), &mut facts, &mut seen);
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

    fn append_dependency_fact_versions(
        &self,
        canonical: &str,
        facts: &mut Vec<verter_resolver::FactVersionRef>,
        seen: &mut rustc_hash::FxHashSet<verter_resolver::FactVersionRef>,
    ) {
        let file_fact = verter_resolver::FactVersionRef::FileWholeHash {
            canonical_id: canonical.to_string(),
            hash: self.get_whole_hash(canonical).unwrap_or_default(),
        };
        if seen.insert(file_fact.clone()) {
            facts.push(file_fact);
        }

        for kind in [
            verter_resolver::DerivedFactKind::ExportRegistry,
            verter_resolver::DerivedFactKind::BarrelSurface,
        ] {
            if let Some(hash) = self.current_derived_fact_hash(canonical, kind) {
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

        if let Some(generation) = self.current_barrel_generation(canonical) {
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

fn should_ignore_external_macro_type(dep: &verter_analysis::MacroTypeDep) -> bool {
    dep.macro_kind == verter_analysis::AnalyzedMacroKind::DefineSlots
        && dep.import_source == "vue"
        && dep.type_name == "Slot"
}

fn macro_dep_exported_type_name<'a>(
    snapshot: &'a FileAnalysisSnapshot,
    dep: &'a verter_analysis::MacroTypeDep,
) -> Cow<'a, str> {
    for import in snapshot
        .imports
        .iter()
        .filter(|import| import.source == dep.import_source)
    {
        for binding in &import.bindings {
            if dep.type_name == binding.name {
                return Cow::Owned(
                    binding
                        .imported_name
                        .clone()
                        .unwrap_or_else(|| binding.name.clone()),
                );
            }

            if matches!(
                binding.kind,
                verter_analysis::types::ImportBindingKind::Namespace
            ) {
                let prefix = format!("{}.", binding.name);
                if let Some(member_name) = dep.type_name.strip_prefix(&prefix) {
                    return Cow::Owned(member_name.to_string());
                }
            }
        }
    }

    Cow::Borrowed(dep.type_name.as_str())
}

struct HostDeclarationMetadataResolver<'a> {
    host: &'a VerterHost,
}

impl verter_resolver::DeclarationMetadataResolver for HostDeclarationMetadataResolver<'_> {
    fn resolve_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<verter_resolver::ResolvedExportTarget> {
        self.host
            .resolve_exports(dep_canonical)
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
            .get_export_span_follow_reexports(dep_canonical, requested_name)
            .map(|(_, start, end)| verter_span::Span::new(start, end))
    }

    fn read_source(&self, canonical_source: &str) -> Option<String> {
        read_full_source(self.host, canonical_source)
    }

    fn type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<verter_analysis::type_eval::DeclarationId> {
        self.host
            .base_eval_env(canonical_source)
            .and_then(|env| env.type_declaration_id(resolved_name))
    }

    fn resolve_type_dependency_canonical(
        &self,
        from_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        self.host
            .resolve_type_dependency_canonical(from_canonical, import_source)
    }
}

pub(crate) fn resolve_type_declaration(
    host: &VerterHost,
    dep_canonical: &str,
    requested_name: &str,
) -> ResolvedTypeDeclaration {
    verter_resolver::resolve_type_declaration(
        &HostDeclarationMetadataResolver { host },
        dep_canonical,
        requested_name,
    )
}

fn resolve_local_type_declaration(
    host: &VerterHost,
    canonical_source: &str,
    resolved: &verter_analysis::ResolvedLocalType,
) -> ResolvedTypeDeclaration {
    verter_resolver::resolve_local_type_declaration(
        &HostDeclarationMetadataResolver { host },
        canonical_source,
        resolved.name.as_str(),
        resolved.span,
    )
}

fn read_full_source(host: &VerterHost, canonical_source: &str) -> Option<String> {
    host.get_source(canonical_source)
        .map(|source| source.to_string())
        .or_else(|| {
            host.workspace
                .read()
                .read_file(canonical_source)
                .map(|source| source.to_string())
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
) -> Option<ResolvedJsdocBlock> {
    if span.start == 0 && span.end == 0 {
        return None;
    }

    let source = read_full_source(host, canonical_source)?;
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
) -> Option<verter_analysis::type_expr::TypeExpr> {
    let source = read_full_source(host, canonical_source)?;
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
            if let Ok(Some(resolved)) = host.resolve_external_type_from_loaded_files(
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
    Some(crate::host_manage::resolved_elements_to_type_expr_via_type_text(&resolved))
}

#[cfg(test)]
#[path = "meta_resolve_tests.rs"]
mod meta_resolve_tests;

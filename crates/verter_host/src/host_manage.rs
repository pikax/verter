//! `impl VerterHost` — file management, analysis, and diagnostics methods.
//!
//! Contains [`VerterHost::remove`], [`VerterHost::get_analysis`],
//! [`VerterHost::get_diagnostics`], and [`VerterHost::set_import_dependencies`].

use std::sync::{Arc, OnceLock};
use std::time::Instant;

use crate::hash::compile_profile_hash;
use crate::id::canonicalize_id;
use crate::resolver_store::HostStoreView;
use crate::shared::{read_lock, write_lock};
use crate::types::*;
use crate::VerterHost;
use verter_resolver::{
    build_imported_eval_inputs, build_owner_eval_env_with_inputs,
    collect_dynamic_root_candidates_from_type,
    evaluate_imported_decl_with_owner_env as resolver_evaluate_imported_decl_with_owner_env,
    fallthrough_cache_key, known_spread_keys_from_type_expr,
    get_export_span_follow_reexports_from_graph as resolver_get_export_span_follow_reexports_from_graph,
    inject_prop_type_overrides, materialize_imported_runtime_values_into_env,
    push_partial_reason,
    resolve_exports_from_graph as resolver_resolve_exports_from_graph,
    resolve_exports_from_graph_best_effort as resolver_resolve_exports_from_graph_best_effort,
    resolve_usage_prop_type,
    resolve_fallthrough_surface as resolver_resolve_fallthrough_surface, run_stable_request,
    DeclarationMetadataResolver, DynamicRootCandidate, ExportGraphFileKind,
    ExportGraphResolver, ExportSurface, FallthroughComputeHost,
    FallthroughResolutionView, FallthroughResolverHost, ImportedDeclEvalResolver,
    ImportedEvalBinding, ImportedEvalCollectorResolver, ImportedEvalLookup,
    ImportedEvalLookupResolver, ImportedEvalOwnerResolver, ImportedEvalOwnerSnapshot,
    ImportedEvalSourceMergeResolver, ImportedEvalTraversalBudget, ImportedRuntimeValueResolver,
    ImportedTypeAliasPrepareError, ImportedTypeAliasResolveRequest, ImportedTypeAliasResolver,
    OwnerEvalEnvAssembler, PreparedImportedDeclContext, RequestSource,
    ResolvedConsumedBindings, ResolvedExportTarget, SingleflightRole,
    StableRequestExecutor, StoreView,
};

pub(crate) fn component_meta_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("VERTER_COMPONENT_META_DEBUG").is_some()
            || std::env::var_os("VERTER_META_DEBUG").is_some()
    })
}

pub(crate) fn component_meta_debug(message: impl AsRef<str>) {
    if component_meta_debug_enabled() {
        use std::io::Write;

        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(stderr, "[verter-meta] {}", message.as_ref());
        let _ = stderr.flush();
    }
}

const COMPONENT_META_MAX_SYMBOLIC_STEPS: usize = 2_000;
const COMPONENT_META_MAX_IMPORTED_TYPE_ROOTS: usize = 2_000;
const STORE_VIEW_STABILITY_MAX_ATTEMPTS: usize = 3;

struct FallthroughRequestExecutor<'a, 'b> {
    host: &'a VerterHost,
    canonical_id: String,
    prop_type_overrides:
        Option<&'a rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>>,
    visiting: &'b mut rustc_hash::FxHashSet<String>,
    fixed_store_view: Option<HostStoreView>,
    last_snapshot_epoch: Option<u64>,
}

impl<'a, 'b> FallthroughRequestExecutor<'a, 'b> {
    fn new(
        host: &'a VerterHost,
        canonical_id: String,
        prop_type_overrides: Option<
            &'a rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        visiting: &'b mut rustc_hash::FxHashSet<String>,
    ) -> Self {
        Self {
            host,
            canonical_id,
            prop_type_overrides,
            visiting,
            fixed_store_view: None,
            last_snapshot_epoch: None,
        }
    }

    fn with_fixed_view(mut self, store_view: Option<&HostStoreView>) -> Self {
        self.fixed_store_view = store_view.cloned();
        self
    }
}

impl<'a, 'b>
    StableRequestExecutor<
        verter_resolver::FallthroughNodeKey,
        Option<crate::types::FallthroughResolution>,
    > for FallthroughRequestExecutor<'a, 'b>
{
    type View = crate::resolver_store::HostStoreView;
    type Error = ();

    fn cache_key(&self) -> verter_resolver::FallthroughNodeKey {
        fallthrough_cache_key(
            &self.canonical_id,
            self.host.config.generic_root_propagation,
            self.prop_type_overrides,
        )
    }

    fn snapshot_view(&mut self) -> Self::View {
        if let Some(view) = self.fixed_store_view.as_ref() {
            self.last_snapshot_epoch = Some(view.mutation_epoch());
            return view.clone();
        }
        let view = self.host.resolver_store_view();
        self.last_snapshot_epoch = Some(view.mutation_epoch());
        view
    }

    fn try_get_cached(
        &mut self,
        store_view: &Self::View,
    ) -> Option<Option<crate::types::FallthroughResolution>> {
        let cache_key = self.cache_key();
        if let Some(cached) = self
            .host
            .fallthrough_cache
            .get_if_valid(&cache_key, store_view)
        {
            if self.prop_type_overrides.is_none() {
                self.host
                    .mirror_cached_fallthrough_arc(&self.canonical_id, cached.clone());
            }
            return Some(Some(cached.as_ref().clone()));
        }

        if self.prop_type_overrides.is_none() {
            #[cfg(feature = "scheduler")]
            {
                if let Some(cc) = self.host.compile_cache.get(&self.canonical_id) {
                    if let Some(ref cached) = cc.cached_fallthrough {
                        if cached.generic_root_propagation
                            == self.host.config.generic_root_propagation
                            && cached
                                .fact_versions
                                .iter()
                                .all(|fact| store_view.validates(fact))
                        {
                            self.host.fallthrough_cache.insert_arc(
                                cache_key,
                                cached.resolution.clone(),
                                cached.fact_versions.clone(),
                            );
                            self.host.mirror_cached_fallthrough_arc(
                                &self.canonical_id,
                                cached.resolution.clone(),
                            );
                            return Some(Some((*cached.resolution).clone()));
                        }
                    }
                }
            }
        }

        None
    }

    fn compute(
        &mut self,
        view: &Self::View,
    ) -> Result<Option<crate::types::FallthroughResolution>, Self::Error> {
        Ok(self.host.compute_fallthrough_surface_uncached(
            &self.canonical_id,
            self.prop_type_overrides,
            self.visiting,
            Some(view),
        ))
    }

    fn is_stable(&mut self, _view: &Self::View) -> bool {
        if self.fixed_store_view.is_some() {
            return true;
        }
        self.last_snapshot_epoch
            .is_some_and(|epoch| self.host.current_store_view_epoch() == epoch)
    }

    fn store_stable(&mut self, value: &Option<crate::types::FallthroughResolution>) {
        if let Some(result) = value.as_ref() {
            self.host.cache_fallthrough_result(
                &self.canonical_id,
                self.prop_type_overrides,
                result,
            );
        }
    }

    fn max_attempts(&self) -> usize {
        STORE_VIEW_STABILITY_MAX_ATTEMPTS
    }
}

impl FallthroughResolutionView for crate::types::FallthroughResolution {
    fn accepted_props(&self) -> &[verter_analysis::component_meta::AcceptedPropAnalysis] {
        &self.accepted_props
    }

    fn accepted_events(&self) -> &[verter_analysis::component_meta::AcceptedEventAnalysis] {
        &self.accepted_events
    }

    fn fallthrough_surface(&self) -> &verter_analysis::component_meta::FallthroughSurface {
        &self.fallthrough_surface
    }

    fn fact_versions(&self) -> &[verter_resolver::FactVersionRef] {
        &self.fact_versions
    }
}

struct HostFallthroughResolver<'a> {
    host: &'a VerterHost,
    parent_canonical_id: &'a str,
    store_view: Option<&'a HostStoreView>,
}

impl FallthroughResolverHost for HostFallthroughResolver<'_> {
    type ChildResolution = crate::types::FallthroughResolution;

    fn intrinsic_members_for_tag(
        &self,
        tag: &str,
    ) -> Vec<verter_analysis::html_intrinsics::OwnedIntrinsicMember> {
        self.host.intrinsic_members_for_tag(tag)
    }

    fn resolve_child_component_canonical(
        &self,
        parent_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        debug_assert_eq!(self.parent_canonical_id, parent_canonical);
        if let Some(view) = self.store_view {
            if let Some(resolution) = view.dependency_resolution(parent_canonical, import_source) {
                return resolution.resolved_canonical_id.clone();
            }
        }
        self.host.resolve_loaded_dependency_canonical(
            parent_canonical,
            import_source,
            verter_vfs::ResolveRequestKind::EsmImport,
        )
    }

    fn current_dependency_fact_versions(
        &self,
        canonical_id: &str,
    ) -> Vec<verter_resolver::FactVersionRef> {
        self.host
            .current_dependency_fact_versions_in_view(
                canonical_id,
                &std::collections::BTreeSet::new(),
                self.store_view,
            )
    }

    fn resolve_child_fallthrough(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Option<Self::ChildResolution> {
        self.host.resolve_fallthrough_surface_internal_with_overrides_in_view(
            canonical_id,
            prop_type_overrides,
            visiting,
            self.store_view,
        )
    }
}

impl FallthroughComputeHost for HostFallthroughResolver<'_> {
    type Snapshot = FileAnalysisSnapshot;
    type EvalEnv = verter_analysis::type_eval::EvalEnv;

    fn resolve_root_consumption(
        &self,
        snapshot: &Self::Snapshot,
        element_index: u32,
        base: &verter_analysis::component_meta::ConsumedRootBindings,
        has_unknown_spread: bool,
        eval_env: &mut Option<Self::EvalEnv>,
    ) -> ResolvedConsumedBindings {
        let resolved = self.host.resolve_root_consumption(
            snapshot,
            element_index,
            base,
            has_unknown_spread,
            eval_env,
        );
        ResolvedConsumedBindings {
            bindings: resolved.bindings,
            partial_reasons: resolved.partial_reasons,
        }
    }

    fn build_generic_child_prop_overrides(
        &self,
        snapshot: &Self::Snapshot,
        usage_index: u32,
        eval_env: &mut Option<Self::EvalEnv>,
    ) -> Option<rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>> {
        self.host
            .build_generic_child_prop_overrides(snapshot, usage_index, eval_env)
    }

    fn resolve_dynamic_root_candidates(
        &self,
        snapshot: &Self::Snapshot,
        usage_index: u32,
        eval_env: &mut Option<Self::EvalEnv>,
    ) -> Vec<DynamicRootCandidate> {
        self.host
            .resolve_dynamic_root_candidates(snapshot, usage_index, eval_env)
    }
}

pub(crate) fn component_meta_symbolic_step_budget() -> usize {
    COMPONENT_META_MAX_SYMBOLIC_STEPS
}

fn component_meta_expansion_budget() -> verter_analysis::type_expand::ExpansionBudget {
    let mut budget = verter_analysis::type_expand::ExpansionBudget::default();
    budget.max_symbolic_work = COMPONENT_META_MAX_SYMBOLIC_STEPS;
    budget
}

fn macro_debug_summary(snapshot: &FileAnalysisSnapshot) -> String {
    snapshot
        .macros
        .iter()
        .map(|mac| {
            format!(
                "{:?}(refs=[{}], props={}, emits={}, slots={})",
                mac.kind,
                mac.type_references.join(","),
                mac.prop_fields.len(),
                mac.emit_fields.len(),
                mac.slot_fields.len(),
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn log_snapshot_debug(
    stage: &str,
    canonical: &str,
    started: Instant,
    snapshot: &FileAnalysisSnapshot,
) {
    component_meta_debug(format!(
        "{stage} {canonical} took {:?} imports={} macro_type_deps={} macros=[{}]",
        started.elapsed(),
        snapshot.imports.len(),
        snapshot.macro_type_deps.len(),
        macro_debug_summary(snapshot),
    ));
}

pub type ImportedEvalInputs = verter_resolver::ImportedEvalInputs;
pub(crate) type ImportedEvalSource = verter_resolver::ImportedEvalSource;
pub(crate) type ImportedTypeAlias = verter_resolver::ImportedTypeAlias;
pub(crate) type ComputedEvaluatedTypes = verter_resolver::ComputedEvaluatedTypes;

struct HostImportedEvalResolver<'a> {
    host: &'a VerterHost,
    dep_resolutions: rustc_hash::FxHashMap<String, DependencyResolution>,
    alias_env_stack: rustc_hash::FxHashSet<String>,
    budget: ImportedEvalTraversalBudget,
    snapshot_cache: rustc_hash::FxHashMap<String, Option<FileAnalysisSnapshot>>,
    eval_source_cache: rustc_hash::FxHashMap<String, Option<String>>,
    store_view: Option<&'a crate::resolver_store::HostStoreView>,
}

struct HostRuntimeValueResolver<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a crate::resolver_store::HostStoreView>,
}

struct HostExportGraphResolver<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a crate::resolver_store::HostStoreView>,
}

impl<'a> HostImportedEvalResolver<'a> {
    fn new(
        host: &'a VerterHost,
        owner_canonical_id: &'a str,
        store_view: Option<&'a crate::resolver_store::HostStoreView>,
    ) -> Self {
        let mut alias_env_stack = rustc_hash::FxHashSet::default();
        alias_env_stack.insert(owner_canonical_id.to_string());
        Self {
            host,
            dep_resolutions: host
                .dependency_resolutions_for_eval_in_view(owner_canonical_id, store_view)
                .unwrap_or_default(),
            alias_env_stack,
            budget: ImportedEvalTraversalBudget::new(
                owner_canonical_id,
                COMPONENT_META_MAX_IMPORTED_TYPE_ROOTS,
            ),
            snapshot_cache: rustc_hash::FxHashMap::default(),
            eval_source_cache: rustc_hash::FxHashMap::default(),
            store_view,
        }
    }

    fn with_dep_resolutions(
        host: &'a VerterHost,
        owner_canonical_id: &'a str,
        dep_resolutions: rustc_hash::FxHashMap<String, DependencyResolution>,
        store_view: Option<&'a crate::resolver_store::HostStoreView>,
    ) -> Self {
        let mut alias_env_stack = rustc_hash::FxHashSet::default();
        alias_env_stack.insert(owner_canonical_id.to_string());
        Self {
            host,
            dep_resolutions,
            alias_env_stack,
            budget: ImportedEvalTraversalBudget::new(
                owner_canonical_id,
                COMPONENT_META_MAX_IMPORTED_TYPE_ROOTS,
            ),
            snapshot_cache: rustc_hash::FxHashMap::default(),
            eval_source_cache: rustc_hash::FxHashMap::default(),
            store_view,
        }
    }
}

impl HostExportGraphResolver<'_> {
    fn file_kind_in_view(&self, canonical_id: &str) -> Option<ExportGraphFileKind> {
        let (_, cached_parse, _) = self
            .host
            .current_eval_state_in_view(canonical_id, self.store_view)?;
        Some(if cached_parse.is_some() {
            ExportGraphFileKind::VueSfc
        } else {
            ExportGraphFileKind::NonSfc
        })
    }
}

impl ExportGraphResolver for HostExportGraphResolver<'_> {
    fn export_surface(&self, canonical_id: &str) -> Option<ExportSurface> {
        let snapshot = self
            .host
            .get_raw_analysis_snapshot_in_view(canonical_id, self.store_view)?;
        Some(ExportSurface {
            file_kind: self.file_kind_in_view(canonical_id)?,
            export_signatures: snapshot.export_signatures.as_ref().clone(),
        })
    }

    fn local_export_span(&self, canonical_id: &str, binding_name: &str) -> Option<verter_span::Span> {
        let snapshot = self
            .host
            .get_raw_analysis_snapshot_in_view(canonical_id, self.store_view)?;
        let file_kind = self.file_kind_in_view(canonical_id)?;

        match file_kind {
            ExportGraphFileKind::VueSfc => {
                if let Some(binding) = snapshot.bindings.iter().find(|binding| binding.name == binding_name) {
                    if binding.span.start > 0 || binding.span.end > 0 {
                        return Some(binding.span);
                    }
                }

                for mac in snapshot.macros.iter() {
                    if mac.binding_name.as_deref() == Some(binding_name)
                        && (mac.span.start > 0 || mac.span.end > 0)
                    {
                        return Some(mac.span);
                    }
                }

                if binding_name == "default" {
                    if let Some(first_binding) = snapshot.bindings.first() {
                        if first_binding.span.start > 0 || first_binding.span.end > 0 {
                            return Some(first_binding.span);
                        }
                    }
                    if let Some(first_macro) = snapshot.macros.first() {
                        if first_macro.span.start > 0 || first_macro.span.end > 0 {
                            return Some(first_macro.span);
                        }
                    }
                    return Some(verter_span::Span::default());
                }

                None
            }
            ExportGraphFileKind::NonSfc => snapshot
                .export_signatures
                .iter()
                .find(|sig| sig.name == binding_name)
                .map(|sig| sig.span)
                .filter(|span| span.start > 0 || span.end > 0),
        }
    }

    fn resolve_reexport_target(
        &self,
        canonical_id: &str,
        source: &str,
        sig: &verter_analysis::ExportSignature,
    ) -> Option<String> {
        if sig.is_type {
            self.host
                .resolve_type_dependency_canonical_in_view(canonical_id, source, self.store_view)
        } else {
            self.host.resolve_loaded_dependency_canonical_in_view(
                canonical_id,
                source,
                verter_vfs::ResolveRequestKind::EsmImport,
                self.store_view,
            )
        }
    }
}

impl ImportedRuntimeValueResolver for HostRuntimeValueResolver<'_> {
    fn dependency_eval_env(
        &self,
        canonical_id: &str,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        self.host
            .base_eval_env_in_view(canonical_id, self.store_view)
            .or_else(|| {
                self.host
                    .load_eval_dependency_source_text_with_fallback_in_view(
                        canonical_id,
                        self.store_view,
                    )
                    .map(|source| {
                        verter_analysis::type_eval_build::parse_and_build_env(source.as_ref())
                    })
            })
    }
}

impl DeclarationMetadataResolver for HostImportedEvalResolver<'_> {
    fn resolve_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<ResolvedExportTarget> {
        self.host
            .resolve_exports_in_view(dep_canonical, self.store_view)
            .into_iter()
            .find(|export| export.name == requested_name)
            .map(|export| ResolvedExportTarget {
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
        self.host
            .read_analysis_source_in_view(canonical_source, self.store_view)
            .map(|source| source.to_string())
            .or_else(|| {
                self.host
                    .load_eval_dependency_source_text_with_fallback_in_view(
                        canonical_source,
                        self.store_view,
                    )
                    .map(|source| source.to_string())
            })
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

impl ImportedEvalLookupResolver for HostImportedEvalResolver<'_> {
    fn resolve_import_canonical_id(
        &self,
        owner_canonical_id: &str,
        import: &verter_analysis::AnalyzedImport,
    ) -> Option<String> {
        import
            .resolved_canonical_id
            .clone()
            .or_else(|| {
                self.dep_resolutions
                    .get(&import.source)
                    .and_then(|resolution| resolution.resolved_canonical_id.clone())
            })
            .or_else(|| {
                self.dep_resolutions
                    .get(&import.source)
                    .and_then(DependencyResolution::effective_target)
                    .map(str::to_string)
            })
            .or_else(|| {
                self.host.resolve_type_dependency_canonical_in_view(
                    owner_canonical_id,
                    &import.source,
                    self.store_view,
                )
            })
            .or_else(|| {
                (self.store_view.is_none() && import.source.starts_with('.'))
                    .then(|| crate::id::resolve_external(owner_canonical_id, &import.source))
            })
    }

    fn prepare_imported_type_alias(
        &mut self,
        request: ImportedTypeAliasResolveRequest,
        discovered_dependencies: &mut std::collections::BTreeSet<String>,
    ) -> Option<verter_analysis::type_eval::TypeDeclInfo> {
        verter_resolver::prepare_imported_type_alias(self, request, discovered_dependencies)
            .map(|alias| alias.decl)
    }

    fn resolve_value_export_target(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> Option<ResolvedExportTarget> {
        self.host
            .resolve_exports_in_view(dep_canonical_id, self.store_view)
            .into_iter()
            .find(|export| !export.is_type && export.name == imported_name)
            .map(|export| ResolvedExportTarget {
                source_canonical_id: export.source_canonical_id,
                source_name: export.source_name,
            })
    }

    fn dependency_eval_env(
        &self,
        canonical_id: &str,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        self.host
            .base_eval_env_in_view(canonical_id, self.store_view)
            .or_else(|| {
                self.host
                    .load_eval_dependency_source_text_with_fallback_in_view(
                        canonical_id,
                        self.store_view,
                    )
                    .map(|source| {
                        verter_analysis::type_eval_build::parse_and_build_env(source.as_ref())
                    })
            })
    }
}

impl ImportedTypeAliasResolver for HostImportedEvalResolver<'_> {
    fn canonicalize_imported_source(&self, source_canonical_id: &str) -> String {
        self.host
            .load_eval_dependency_canonical_with_fallback_in_view(
                source_canonical_id,
                self.store_view,
            )
            .unwrap_or_else(|| source_canonical_id.to_string())
    }

    fn dependency_eval_env(
        &self,
        canonical_id: &str,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        self.host
            .base_eval_env_in_view(canonical_id, self.store_view)
    }

    fn budget_is_exhausted(&self) -> bool {
        self.budget.is_exhausted()
    }

    fn set_budget_overflow(&mut self, message: String) {
        self.budget.set_overflow(message);
    }

    fn resolve_external_type_body(
        &mut self,
        request: &ImportedTypeAliasResolveRequest,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
    ) -> Result<Option<verter_analysis::type_expr::TypeExpr>, ImportedTypeAliasPrepareError> {
        let mut cache = rustc_hash::FxHashMap::default();
        let mut visiting = rustc_hash::FxHashSet::default();
        match self.host.resolve_external_type_from_loaded_files_in_view(
            request.owner_canonical_id.as_str(),
            request.import_source.as_str(),
            request.imported_name.as_str(),
            tracked_deps,
            resolution_deps,
            &mut cache,
            &mut visiting,
            true,
            verter_vfs::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
            self.store_view,
        ) {
            Ok(resolved) => Ok(resolved.map(|resolved| {
                verter_resolver::resolved_elements_to_type_expr_via_type_text(&resolved)
            })),
            Err(crate::types::ExternalTypeResolveError::StepLimitExceeded {
                limit,
                type_name,
                last_dep,
            }) => Err(ImportedTypeAliasPrepareError::StepLimitExceeded {
                limit,
                type_name,
                last_dep,
            }),
            Err(_) => Err(ImportedTypeAliasPrepareError::Other),
        }
    }

    fn evaluate_imported_decl_with_owner_env(
        &mut self,
        source_canonical_id: &str,
        exported_name: &str,
        canonical_dependencies: &mut std::collections::BTreeSet<String>,
    ) -> Option<verter_analysis::type_expr::TypeExpr> {
        resolver_evaluate_imported_decl_with_owner_env(
            self,
            source_canonical_id,
            exported_name,
            canonical_dependencies,
        )
    }
}

impl ImportedEvalCollectorResolver for HostImportedEvalResolver<'_> {
    fn resolve_imported_type_dependency(
        &self,
        owner_canonical_id: &str,
        import: &verter_analysis::AnalyzedImport,
    ) -> Option<String> {
        import
            .resolved_canonical_id
            .clone()
            .or_else(|| {
                self.dep_resolutions
                    .get(&import.source)
                    .and_then(|resolution| resolution.resolved_canonical_id.clone())
            })
            .or_else(|| {
                self.dep_resolutions
                    .get(&import.source)
                    .and_then(DependencyResolution::effective_target)
                    .map(str::to_string)
            })
            .or_else(|| {
                self.host.resolve_type_dependency_canonical_in_view(
                    owner_canonical_id,
                    &import.source,
                    self.store_view,
                )
            })
            .or_else(|| {
                (self.store_view.is_none() && import.source.starts_with('.'))
                    .then(|| crate::id::resolve_external(owner_canonical_id, &import.source))
            })
    }

    fn collect_imported_type_alias(
        &mut self,
        request: ImportedTypeAliasResolveRequest,
        canonical_dependencies: &mut std::collections::BTreeSet<String>,
        budget: &mut ImportedEvalTraversalBudget,
    ) -> Option<ImportedTypeAlias> {
        std::mem::swap(&mut self.budget, budget);
        let result =
            verter_resolver::prepare_imported_type_alias(self, request, canonical_dependencies);
        std::mem::swap(&mut self.budget, budget);
        result
    }
}

impl ImportedEvalOwnerResolver for HostImportedEvalResolver<'_> {
    fn collect_required_owner_import_names(
        &self,
        owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
        owner_eval_source: &str,
        owner_env: &verter_analysis::type_eval::EvalEnv,
    ) -> rustc_hash::FxHashSet<String> {
        collect_required_owner_import_names_from_parts(owner_snapshot, owner_eval_source, owner_env)
    }

    fn track_direct_eval_dependencies(
        &self,
        owner_canonical_id: &str,
        owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
        canonical_dependencies: &mut std::collections::BTreeSet<String>,
    ) {
        self.host.track_direct_eval_dependencies(
            owner_canonical_id,
            owner_snapshot,
            self.store_view.is_none(),
            &self.dep_resolutions,
            canonical_dependencies,
        );
    }
}

impl ImportedDeclEvalResolver for HostImportedEvalResolver<'_> {
    fn budget_is_exhausted(&self) -> bool {
        self.budget.is_exhausted()
    }

    fn canonicalize_imported_source(&self, source_canonical_id: &str) -> String {
        self.host
            .load_eval_dependency_canonical_with_fallback_in_view(
                source_canonical_id,
                self.store_view,
            )
            .unwrap_or_else(|| source_canonical_id.to_string())
    }

    fn enter_alias_env(&mut self, canonical_id: &str) -> bool {
        self.alias_env_stack.insert(canonical_id.to_string())
    }

    fn leave_alias_env(&mut self, canonical_id: &str) {
        self.alias_env_stack.remove(canonical_id);
    }

    fn load_imported_decl_context(
        &self,
        source_canonical_id: &str,
        exported_name: &str,
    ) -> Option<PreparedImportedDeclContext> {
        let snapshot = self
            .host
            .get_raw_analysis_snapshot_in_view(source_canonical_id, self.store_view)?;
        let dep_eval_source = self
            .host
            .load_eval_dependency_source_text_with_fallback_in_view(
                source_canonical_id,
                self.store_view,
            )?;
        let dep_env = self
            .host
            .base_eval_env_in_view(source_canonical_id, self.store_view)?;
        let decl = dep_env.type_symbols.get(exported_name)?.clone();

        Some(PreparedImportedDeclContext {
            imports: snapshot.imports.to_vec(),
            macros: snapshot.macros.as_ref().to_vec(),
            bindings: snapshot.bindings.to_vec(),
            macro_type_deps: snapshot.macro_type_deps.as_ref().to_vec(),
            eval_source: dep_eval_source.as_ref().to_string(),
            env: dep_env,
            decl,
        })
    }

    fn required_import_names_for_decl(
        &self,
        decl: &verter_analysis::type_eval::TypeDeclInfo,
        owner_env: &verter_analysis::type_eval::EvalEnv,
    ) -> rustc_hash::FxHashSet<String> {
        collect_required_import_names_for_type_decl(decl, owner_env)
    }

    fn build_imported_inputs_for_decl(
        &mut self,
        owner_canonical_id: &str,
        context: &PreparedImportedDeclContext,
        additional_required_import_names: &rustc_hash::FxHashSet<String>,
    ) -> ImportedEvalInputs {
        let mut budget = std::mem::replace(
            &mut self.budget,
            ImportedEvalTraversalBudget::new(
                owner_canonical_id,
                COMPONENT_META_MAX_IMPORTED_TYPE_ROOTS,
            ),
        );
        let inputs = build_imported_eval_inputs(
            self,
            owner_canonical_id,
            &context.owner_snapshot(),
            context.eval_source.as_str(),
            &context.env,
            Some(additional_required_import_names),
            &mut budget,
        );
        self.budget = budget;
        inputs
    }

    fn build_owner_eval_env_for_decl(
        &self,
        canonical_id: &str,
        context: &PreparedImportedDeclContext,
        imported_inputs: &ImportedEvalInputs,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        let snapshot = FileAnalysisSnapshot {
            imports: context.imports.clone(),
            bindings: context.bindings.clone(),
            module_references: std::sync::Arc::new(Vec::new()),
            macros: std::sync::Arc::new(context.macros.clone()),
            macro_type_deps: std::sync::Arc::new(context.macro_type_deps.clone()),
            script_flags: 0,
            styles: std::sync::Arc::new(Vec::new()),
            template: None,
            vue_api_calls: std::sync::Arc::new(Vec::new()),
            dom_query_calls: std::sync::Arc::new(Vec::new()),
            css_var_manipulations: std::sync::Arc::new(Vec::new()),
            script_binding_occurrences: std::sync::Arc::new(Vec::new()),
            export_signatures: std::sync::Arc::new(Vec::new()),
            options_api: None,
            store_usages: std::sync::Arc::new(Vec::new()),
            store_definitions: std::sync::Arc::new(Vec::new()),
            is_typescript: true,
        };
        self.host
            .build_owner_eval_env_with_inputs_from_owner_env_in_view(
                canonical_id,
                &snapshot,
                imported_inputs,
                None,
                Some(context.env.clone()),
                self.store_view,
            )
            .map(|built| built.env)
    }
}

impl ImportedEvalSourceMergeResolver for HostImportedEvalResolver<'_> {
    fn record_eval_input_source(
        &mut self,
        canonical_id: &str,
        seen_sources: &mut rustc_hash::FxHashSet<String>,
        inputs: &mut Vec<ImportedEvalSource>,
        canonical_dependencies: &mut std::collections::BTreeSet<String>,
    ) {
        let Some((resolved_canonical_id, source)) = self
            .host
            .load_eval_dependency_source_with_fallback_in_view(canonical_id, self.store_view)
        else {
            canonical_dependencies.insert(canonical_id.to_string());
            return;
        };

        canonical_dependencies.insert(resolved_canonical_id.clone());
        if !seen_sources.insert(resolved_canonical_id.clone()) {
            return;
        }

        inputs.push(ImportedEvalSource {
            canonical_id: resolved_canonical_id,
            source,
        });
    }

    fn load_eval_source_for_merge(&mut self, canonical_id: &str) -> Option<String> {
        self.eval_source_cache
            .entry(canonical_id.to_string())
            .or_insert_with(|| {
                self.host
                    .current_eval_state_in_view(canonical_id, self.store_view)
                    .map(|(source, cached_parse, _)| {
                        VerterHost::build_eval_script_source(&source, cached_parse.as_deref())
                    })
            })
            .clone()
    }

    fn import_bindings_for_merge(
        &mut self,
        canonical_id: &str,
        eval_source: &str,
    ) -> Vec<ImportedEvalBinding> {
        let snapshot = self
            .snapshot_cache
            .entry(canonical_id.to_string())
            .or_insert_with(|| {
                let snapshot = self
                    .host
                    .get_analysis_snapshot_internal(canonical_id, None)?;
                let whole_hash = self.host.get_whole_hash(canonical_id).unwrap_or_default();
                if !self.host.store_view_allows_current_whole_hash(
                    canonical_id,
                    whole_hash,
                    self.store_view,
                ) {
                    return None;
                }
                Some(snapshot)
            })
            .clone();

        if let Some(snapshot) = snapshot {
            return snapshot
                .imports
                .iter()
                .flat_map(|import| {
                    import
                        .bindings
                        .iter()
                        .map(move |binding| ImportedEvalBinding {
                            local_name: binding.name.clone(),
                            imported_name: binding.imported_name.clone(),
                            source: import.source.clone(),
                            resolved_canonical_id: import.resolved_canonical_id.clone(),
                            is_namespace: matches!(
                                binding.kind,
                                verter_analysis::types::ImportBindingKind::Namespace
                            ),
                        })
                })
                .collect();
        }

        let alloc = oxc_allocator::Allocator::new();
        verter_core::utils::oxc::vue::resolve_type::extract_imported_type_bindings(
            eval_source,
            &alloc,
        )
        .bindings
        .into_iter()
        .map(|binding| ImportedEvalBinding {
            local_name: binding.local_name.clone(),
            imported_name: if binding.is_namespace {
                None
            } else if binding.imported_name != binding.local_name {
                Some(binding.imported_name)
            } else {
                None
            },
            source: binding.source,
            resolved_canonical_id: None,
            is_namespace: binding.is_namespace,
        })
        .collect()
    }

    fn resolve_import_binding_dependency(
        &self,
        owner_canonical_id: &str,
        binding: &ImportedEvalBinding,
    ) -> Option<String> {
        binding
            .resolved_canonical_id
            .clone()
            .or_else(|| {
                self.dep_resolutions
                    .get(&binding.source)
                    .and_then(|resolution| resolution.resolved_canonical_id.clone())
            })
            .or_else(|| {
                self.dep_resolutions
                    .get(&binding.source)
                    .and_then(DependencyResolution::effective_target)
                    .map(str::to_string)
            })
            .or_else(|| {
                self.host.resolve_type_dependency_canonical_in_view(
                    owner_canonical_id,
                    &binding.source,
                    self.store_view,
                )
            })
            .or_else(|| {
                (self.store_view.is_none() && binding.source.starts_with('.'))
                    .then(|| crate::id::resolve_external(owner_canonical_id, &binding.source))
            })
    }

    fn resolve_imported_type_declaration(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> verter_resolver::ResolvedTypeDeclaration {
        crate::meta_resolve::resolve_type_declaration_in_view(
            self.host,
            dep_canonical,
            imported_name,
            self.store_view,
        )
    }
}

type OwnerEvalEnvBuild = verter_resolver::OwnerEvalEnvBuild;

struct HostOwnerEvalEnvAssembler<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a HostStoreView>,
}

impl OwnerEvalEnvAssembler for HostOwnerEvalEnvAssembler<'_> {
    type Snapshot = FileAnalysisSnapshot;

    fn base_eval_env(&self, canonical_id: &str) -> Option<verter_analysis::type_eval::EvalEnv> {
        self.host.base_eval_env_in_view(canonical_id, self.store_view)
    }

    fn materialize_imported_runtime_values(
        &self,
        snapshot: &Self::Snapshot,
        owner_local_value_names: &rustc_hash::FxHashSet<String>,
        env: &mut verter_analysis::type_eval::EvalEnv,
    ) {
        self.host.materialize_imported_runtime_values_into_env_in_view(
            snapshot,
            owner_local_value_names,
            env,
            self.store_view,
        );
    }
}

impl VerterHost {
    pub(crate) fn store_view_allows_current_whole_hash(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> bool {
        let Some(view) = store_view else {
            return true;
        };

        view.accepts_whole_hash(canonical_id, whole_hash)
            || (!view.tracks_whole_hash(canonical_id)
                && self.current_store_view_epoch() == view.mutation_epoch())
    }

    pub(crate) fn build_eval_script_source(
        source: &str,
        cached_parse: Option<&verter_core::parser::types::ParsedSfc>,
    ) -> String {
        crate::host_resolve::extract_vue_script_content(source, cached_parse)
            .unwrap_or_else(|| source.to_string())
    }

    pub(crate) fn read_analysis_source(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.get_source(canonical_id)
            .or_else(|| self.ws().read_file(canonical_id))
    }

    pub(crate) fn read_analysis_source_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<str>> {
        let source = self.read_analysis_source(canonical_id)?;
        let whole_hash = crate::hash::hash_16(source.as_bytes());
        if !self.store_view_allows_current_whole_hash(canonical_id, whole_hash, store_view) {
            return None;
        }
        Some(source)
    }

    fn clone_cached_eval_env(
        &self,
        cache_key: &str,
        whole_hash: Hash16,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        self.eval_env_cache
            .lock()
            .get(cache_key)
            .and_then(|(cached_hash, cached_env)| {
                (*cached_hash == whole_hash).then(|| (**cached_env).clone())
            })
    }

    fn cache_eval_env(
        &self,
        cache_keys: &[String],
        whole_hash: Hash16,
        env: verter_analysis::type_eval::EvalEnv,
    ) -> verter_analysis::type_eval::EvalEnv {
        let mut cache = self.eval_env_cache.lock();
        for cache_key in cache_keys {
            if let Some((cached_hash, cached_env)) = cache.get(cache_key) {
                if *cached_hash == whole_hash {
                    return (**cached_env).clone();
                }
            }
        }

        let cached_env = Arc::new(env.clone());
        for cache_key in cache_keys {
            cache.insert(cache_key.clone(), (whole_hash, Arc::clone(&cached_env)));
        }
        env
    }

    pub(crate) fn base_eval_env(
        &self,
        canonical_id: &str,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        self.base_eval_env_in_view(canonical_id, None)
    }

    pub(crate) fn base_eval_env_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        if let Some((source, cached_parse, whole_hash)) =
            self.current_eval_state_in_view(canonical_id, store_view)
        {
            if let Some(cached_env) = self.clone_cached_eval_env(canonical_id, whole_hash) {
                return Some(cached_env);
            }

            let eval_source = Self::build_eval_script_source(&source, cached_parse.as_deref());
            let env = verter_analysis::type_eval_build::parse_and_build_env(&eval_source);
            return Some(self.cache_eval_env(&[canonical_id.to_string()], whole_hash, env));
        }

        let (resolved_canonical_id, eval_source) =
            self.load_eval_dependency_source_with_fallback_in_view(canonical_id, store_view)?;
        let whole_hash = crate::hash::hash_16(eval_source.as_bytes());

        if let Some(cached_env) = self.clone_cached_eval_env(&resolved_canonical_id, whole_hash) {
            return Some(cached_env);
        }

        let env = verter_analysis::type_eval_build::parse_and_build_env(eval_source.as_ref());
        Some(self.cache_eval_env(
            &[resolved_canonical_id, canonical_id.to_string()],
            whole_hash,
            env,
        ))
    }

    fn build_snapshot_from_parse(parse: crate::ParseSnapshot) -> FileAnalysisSnapshot {
        let script_analysis = parse.script_analysis;
        FileAnalysisSnapshot {
            imports: script_analysis.imports,
            bindings: script_analysis.bindings,
            module_references: Arc::new(script_analysis.module_references),
            macros: Arc::new(script_analysis.macros),
            macro_type_deps: Arc::new(script_analysis.macro_type_deps),
            script_flags: script_analysis.flags.bits(),
            styles: Arc::new(parse.style_analyses),
            template: None,
            vue_api_calls: Arc::new(script_analysis.vue_api_calls),
            dom_query_calls: Arc::new(script_analysis.dom_query_calls),
            css_var_manipulations: Arc::new(script_analysis.css_var_manipulations),
            script_binding_occurrences: Arc::new(script_analysis.script_binding_occurrences),
            export_signatures: Arc::new(parse.export_signatures),
            options_api: script_analysis.options_api,
            store_usages: Arc::new(script_analysis.store_usages),
            store_definitions: Arc::new(script_analysis.store_definitions),
            is_typescript: script_analysis.is_typescript,
        }
    }

    fn build_snapshot_from_source(
        &self,
        canonical: &str,
        source: &Arc<str>,
    ) -> FileAnalysisSnapshot {
        if canonical.ends_with(".vue") {
            let (parse, _) =
                crate::parse::parse_vue_snapshot(canonical, source, self.config.effective_scope());
            Self::build_snapshot_from_parse(parse)
        } else {
            let parse = crate::parse::parse_non_sfc_snapshot(canonical, source);
            Self::build_snapshot_from_parse(parse)
        }
    }

    fn finalize_analysis_snapshot(
        &self,
        canonical: &str,
        mut snapshot: FileAnalysisSnapshot,
        needs_template_analysis: bool,
        analysis_started: Option<Instant>,
    ) -> FileAnalysisSnapshot {
        self.resolve_snapshot_imports(canonical, &mut snapshot);
        self.enrich_destructured_bindings(&mut snapshot);
        if needs_template_analysis {
            self.compute_template_analysis_if_missing(canonical, &mut snapshot);
        }
        if let Some(started) = analysis_started {
            log_snapshot_debug("get_analysis", canonical, started, &snapshot);
        }
        snapshot
    }

    fn is_expanded_types_empty(
        result: &verter_analysis::type_expand::ExpandedComponentTypes,
    ) -> bool {
        result.is_empty()
    }

    pub(crate) fn current_eval_state(
        &self,
        canonical_id: &str,
    ) -> Option<(
        Arc<str>,
        Option<Arc<verter_core::parser::types::ParsedSfc>>,
        Hash16,
    )> {
        self.current_eval_state_in_view(canonical_id, None)
    }

    pub(crate) fn current_eval_state_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<(
        Arc<str>,
        Option<Arc<verter_core::parser::types::ParsedSfc>>,
        Hash16,
    )> {
        #[cfg(feature = "scheduler")]
        {
            if let Some(state) = self.effective_file_state(canonical_id, None) {
                if !self.store_view_allows_current_whole_hash(
                    canonical_id,
                    state.whole_hash,
                    store_view,
                ) {
                    return None;
                }
                Some((state.source, state.cached_parse, state.whole_hash))
            } else {
                let source = self.read_analysis_source_in_view(canonical_id, store_view)?;
                let cached_parse = canonical_id
                    .ends_with(".vue")
                    .then(|| Arc::new(verter_core::compile::parse_sfc(&source, None, None)));
                let whole_hash = crate::hash::hash_16(source.as_bytes());
                Some((source.clone(), cached_parse, whole_hash))
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            if let Some(entry) = files.get(canonical_id) {
                if !self.store_view_allows_current_whole_hash(
                    canonical_id,
                    entry.whole_hash,
                    store_view,
                ) {
                    return None;
                }
                Some((
                    Arc::clone(&entry.source),
                    entry.cached_parse.clone(),
                    entry.whole_hash,
                ))
            } else {
                drop(files);
                let source = self.read_analysis_source_in_view(canonical_id, store_view)?;
                let cached_parse = canonical_id
                    .ends_with(".vue")
                    .then(|| Arc::new(verter_core::compile::parse_sfc(&source, None, None)));
                let whole_hash = crate::hash::hash_16(source.as_bytes());
                Some((source.clone(), cached_parse, whole_hash))
            }
        }
    }

    pub(crate) fn dependency_resolutions_for_eval(
        &self,
        canonical_id: &str,
    ) -> rustc_hash::FxHashMap<String, DependencyResolution> {
        self.dependency_resolutions_for_eval_in_view(canonical_id, None)
            .unwrap_or_default()
    }

    pub(crate) fn dependency_resolutions_for_eval_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<rustc_hash::FxHashMap<String, DependencyResolution>> {
        if let Some(view) = store_view {
            self.current_eval_state_in_view(canonical_id, Some(view))?;
            return Some(
                view.dependency_resolutions(canonical_id)
                    .cloned()
                    .unwrap_or_default(),
            );
        }

        if self
            .current_eval_state_in_view(canonical_id, store_view)
            .is_none()
        {
            return None;
        }

        #[cfg(feature = "scheduler")]
        {
            Some(
                self.compile_cache
                    .get(canonical_id)
                    .map(|entry| entry.dependency_resolutions.clone())
                    .unwrap_or_default(),
            )
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            Some(
                files
                    .get(canonical_id)
                    .map(|entry| entry.dependency_resolutions.clone())
                    .unwrap_or_default(),
            )
        }
    }

    /// Load an evaluation dependency source, hydrating workspace-owned files into
    /// host state when necessary before reading them.
    fn load_eval_dependency_source_with_fallback(
        &self,
        dep_canonical: &str,
    ) -> Option<(String, Arc<str>)> {
        self.load_eval_dependency_source_with_fallback_in_view(dep_canonical, None)
    }

    fn load_eval_dependency_source_with_fallback_in_view(
        &self,
        dep_canonical: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<(String, Arc<str>)> {
        let read_candidate = |candidate: &str| -> Option<Arc<str>> {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let _ = self.ensure_loaded(candidate);
            }

            if let Some((source, cached_parse, whole_hash)) =
                self.current_eval_state_in_view(candidate, store_view)
            {
                if !self.store_view_allows_current_whole_hash(candidate, whole_hash, store_view) {
                    return None;
                }
                return Some(Arc::<str>::from(Self::build_eval_script_source(
                    &source,
                    cached_parse.as_deref(),
                )));
            }

            self.read_dep_source_for_type_resolution(candidate, None)
                .and_then(|source| {
                    let whole_hash = crate::hash::hash_16(source.as_bytes());
                    if !self.store_view_allows_current_whole_hash(candidate, whole_hash, store_view)
                    {
                        return None;
                    }
                    Some(Arc::<str>::from(Self::build_eval_script_source(
                        &source, None,
                    )))
                })
        };

        if let Some(source) = read_candidate(dep_canonical) {
            return Some((dep_canonical.to_string(), source));
        }

        let mut candidates = Vec::new();
        if let Some(stem) = dep_canonical.strip_suffix(".js") {
            candidates.push(format!("{stem}.d.ts"));
        }
        if let Some(stem) = dep_canonical.strip_suffix(".jsx") {
            candidates.push(format!("{stem}.d.ts"));
        }
        if let Some(stem) = dep_canonical.strip_suffix(".mjs") {
            candidates.push(format!("{stem}.d.mts"));
        }
        if let Some(stem) = dep_canonical.strip_suffix(".cjs") {
            candidates.push(format!("{stem}.d.cts"));
        }
        candidates.extend([
            format!("{dep_canonical}.d.ts"),
            format!("{dep_canonical}.ts"),
            format!("{dep_canonical}.tsx"),
            format!("{dep_canonical}/index.d.ts"),
            format!("{dep_canonical}/index.ts"),
            format!("{dep_canonical}/index.tsx"),
        ]);

        for candidate in candidates {
            if let Some(source) = read_candidate(&candidate) {
                return Some((candidate, source));
            }
        }

        None
    }

    fn load_eval_dependency_canonical_with_fallback(&self, dep_canonical: &str) -> Option<String> {
        self.load_eval_dependency_source_with_fallback(dep_canonical)
            .map(|(canonical, _)| canonical)
    }

    fn load_eval_dependency_canonical_with_fallback_in_view(
        &self,
        dep_canonical: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<String> {
        self.load_eval_dependency_source_with_fallback_in_view(dep_canonical, store_view)
            .map(|(canonical, _)| canonical)
    }

    fn load_eval_dependency_source_text_with_fallback(
        &self,
        dep_canonical: &str,
    ) -> Option<Arc<str>> {
        self.load_eval_dependency_source_with_fallback(dep_canonical)
            .map(|(_, source)| source)
    }

    fn load_eval_dependency_source_text_with_fallback_in_view(
        &self,
        dep_canonical: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<str>> {
        self.load_eval_dependency_source_with_fallback_in_view(dep_canonical, store_view)
            .map(|(_, source)| source)
    }

    pub(crate) fn imported_eval_inputs(
        &self,
        owner_canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
    ) -> ImportedEvalInputs {
        self.imported_eval_inputs_with_owner_context(
            owner_canonical_id,
            snapshot,
            dep_resolutions,
            None,
            None,
        )
    }

    pub(crate) fn imported_eval_inputs_with_owner_context(
        &self,
        owner_canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
        owner_eval_source: Option<&str>,
        owner_env: Option<&verter_analysis::type_eval::EvalEnv>,
    ) -> ImportedEvalInputs {
        self.imported_eval_inputs_with_owner_context_in_view(
            owner_canonical_id,
            snapshot,
            dep_resolutions,
            owner_eval_source,
            owner_env,
            None,
        )
    }

    pub(crate) fn imported_eval_inputs_with_owner_context_in_view(
        &self,
        owner_canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
        owner_eval_source: Option<&str>,
        owner_env: Option<&verter_analysis::type_eval::EvalEnv>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> ImportedEvalInputs {
        self.provenance
            .imported_eval_inputs_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut alias_env_stack = rustc_hash::FxHashSet::default();
        alias_env_stack.insert(owner_canonical_id.to_string());
        let mut budget = ImportedEvalTraversalBudget::new(
            owner_canonical_id,
            COMPONENT_META_MAX_IMPORTED_TYPE_ROOTS,
        );
        self.imported_eval_inputs_inner(
            owner_canonical_id,
            snapshot,
            dep_resolutions,
            owner_eval_source,
            owner_env,
            None,
            &mut alias_env_stack,
            &mut budget,
            store_view,
        )
    }

    fn imported_eval_inputs_inner(
        &self,
        owner_canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
        owner_eval_source: Option<&str>,
        owner_env_override: Option<&verter_analysis::type_eval::EvalEnv>,
        additional_required_import_names: Option<&rustc_hash::FxHashSet<String>>,
        alias_env_stack: &mut rustc_hash::FxHashSet<String>,
        budget: &mut ImportedEvalTraversalBudget,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> ImportedEvalInputs {
        let started = component_meta_debug_enabled().then(Instant::now);
        let owner_eval_source = owner_eval_source
            .map(str::to_string)
            .or_else(|| {
                self.current_eval_state_in_view(owner_canonical_id, store_view)
                    .map(|(source, cached_parse, _)| {
                        Self::build_eval_script_source(&source, cached_parse.as_deref())
                    })
            })
            .unwrap_or_default();
        let owner_env = owner_env_override
            .cloned()
            .or_else(|| self.base_eval_env_in_view(owner_canonical_id, store_view))
            .unwrap_or_else(|| {
                verter_analysis::type_eval_build::parse_and_build_env(&owner_eval_source)
            });
        let owner_snapshot = ImportedEvalOwnerSnapshot {
            imports: snapshot.imports.as_slice(),
            macros: snapshot.macros.as_ref(),
            bindings: snapshot.bindings.as_ref(),
            macro_type_deps: snapshot.macro_type_deps.as_ref(),
        };
        if let Some(started) = started {
            component_meta_debug(format!(
                "imported_eval_inputs:start owner={} imports={} prework_took {:?}",
                owner_canonical_id,
                snapshot.imports.len(),
                started.elapsed(),
            ));
        }
        let mut collector = HostImportedEvalResolver::with_dep_resolutions(
            self,
            owner_canonical_id,
            dep_resolutions.clone(),
            store_view,
        );
        collector.alias_env_stack = alias_env_stack.clone();
        let imported_inputs = build_imported_eval_inputs(
            &mut collector,
            owner_canonical_id,
            &owner_snapshot,
            owner_eval_source.as_str(),
            &owner_env,
            additional_required_import_names,
            budget,
        );
        *alias_env_stack = collector.alias_env_stack;

        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "imported_eval_inputs:end owner={} type_aliases=[{}] sources={} total_took={:?}",
                owner_canonical_id,
                imported_inputs
                    .type_aliases
                    .iter()
                    .map(|alias| format!(
                        "{}<-{}#{}",
                        alias.local_name, alias.source_canonical_id, alias.exported_name
                    ))
                    .collect::<Vec<_>>()
                    .join(", "),
                imported_inputs.sources.len(),
                started.map(|start| start.elapsed()).unwrap_or_default(),
            ));
        }

        imported_inputs
    }

    fn track_direct_eval_dependencies(
        &self,
        owner_canonical_id: &str,
        owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
        allow_relative_fallback: bool,
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
        canonical_dependencies: &mut std::collections::BTreeSet<String>,
    ) {
        for dep in owner_snapshot.macro_type_deps.iter() {
            if let Some(dep_canonical) = self
                .resolve_type_dependency_canonical(owner_canonical_id, &dep.import_source)
                .or_else(|| {
                    dep_resolutions
                        .get(&dep.import_source)
                        .and_then(|resolution| resolution.resolved_canonical_id.clone())
                })
                .or_else(|| {
                    (allow_relative_fallback && dep.import_source.starts_with('.')).then(|| {
                        crate::id::resolve_external(owner_canonical_id, &dep.import_source)
                    })
                })
            {
                canonical_dependencies.insert(dep_canonical);
            }
        }

        for import in owner_snapshot
            .imports
            .iter()
            .filter(|import| !import.is_type_only)
        {
            if let Some(dep_canonical) = import
                .resolved_canonical_id
                .clone()
                .or_else(|| {
                    dep_resolutions
                        .get(&import.source)
                        .and_then(DependencyResolution::effective_target)
                        .map(str::to_string)
                })
                .or_else(|| {
                    (allow_relative_fallback && import.source.starts_with('.'))
                        .then(|| crate::id::resolve_external(owner_canonical_id, &import.source))
                })
            {
                canonical_dependencies.insert(dep_canonical);
            }
        }
    }

    pub(crate) fn cache_dependency_candidates_from_snapshot(
        &self,
        owner_canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
    ) -> std::collections::BTreeSet<String> {
        let mut candidates = std::collections::BTreeSet::new();

        for import in &snapshot.imports {
            if let Some(resolved) = import.resolved_canonical_id.as_deref() {
                candidates.insert(resolved.to_string());
                continue;
            }

            if let Some(target) = dep_resolutions
                .get(&import.source)
                .and_then(DependencyResolution::effective_target)
            {
                candidates.insert(target.to_string());
                continue;
            }

            if import.source.starts_with('.') {
                candidates
                    .extend(self.expand_relative_candidates(owner_canonical_id, &import.source));
            }
        }

        candidates
    }

    /// Compute evaluated types using pre-computed imported eval inputs.
    /// Avoids redundant `imported_eval_inputs()` calls when the caller
    /// already has them (e.g., `resolve_component_meta`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn compute_evaluated_types_with_inputs(
        &self,
        canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        imported_inputs: &ImportedEvalInputs,
    ) -> Option<verter_analysis::type_expand::ExpandedComponentTypes> {
        self.compute_evaluated_types_with_tracking(canonical, snapshot, imported_inputs)
            .and_then(|computed| computed.evaluated_types)
    }

    pub(crate) fn compute_evaluated_types_with_tracking(
        &self,
        canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        imported_inputs: &ImportedEvalInputs,
    ) -> Option<ComputedEvaluatedTypes> {
        self.compute_evaluated_types_with_tracking_from_owner_context(
            canonical,
            snapshot,
            imported_inputs,
            None,
            None,
        )
    }

    pub(crate) fn compute_evaluated_types_with_tracking_from_owner_context(
        &self,
        canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        imported_inputs: &ImportedEvalInputs,
        owner_eval_source: Option<&str>,
        owner_env: Option<verter_analysis::type_eval::EvalEnv>,
    ) -> Option<ComputedEvaluatedTypes> {
        self.compute_evaluated_types_with_tracking_from_owner_context_in_view(
            canonical,
            snapshot,
            imported_inputs,
            owner_eval_source,
            owner_env,
            None,
        )
    }

    pub(crate) fn compute_evaluated_types_with_tracking_from_owner_context_in_view(
        &self,
        canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        imported_inputs: &ImportedEvalInputs,
        owner_eval_source: Option<&str>,
        owner_env: Option<verter_analysis::type_eval::EvalEnv>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<ComputedEvaluatedTypes> {
        let eval_source = owner_eval_source.map(str::to_string).or_else(|| {
            self.current_eval_state_in_view(canonical, store_view).map(
                |(source, cached_parse, _)| {
                    Self::build_eval_script_source(&source, cached_parse.as_deref())
                },
            )
        })?;
        let built = self.build_owner_eval_env_with_inputs_from_owner_env_in_view(
            canonical,
            snapshot,
            imported_inputs,
            None,
            owner_env,
            store_view,
        )?;
        let mut env = built.env;
        let mut resolver = HostImportedEvalResolver::new(self, canonical, store_view);
        let mut lookup =
            ImportedEvalLookup::new(&mut resolver, canonical, snapshot.imports.as_slice());

        let budget = component_meta_expansion_budget();
        let result = verter_analysis::type_eval_build::expand_macro_types_with_lookup(
            snapshot.macros.as_ref(),
            Some(&eval_source),
            &mut env,
            Some(&built.requested_binding_names),
            &budget,
            &mut lookup,
        );
        let discovered_dependencies = lookup.into_discovered_dependencies();
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "compute_evaluated_types owner={} props={} define_props={} emits={} slot_bindings={} bindings={} discovered_deps={}",
                canonical,
                result.props.len(),
                result.define_props.len(),
                result.emits.len(),
                result.slot_bindings.len(),
                result.bindings.len(),
                discovered_dependencies.len(),
            ));
        }
        Some(ComputedEvaluatedTypes {
            evaluated_types: (!Self::is_expanded_types_empty(&result)).then_some(result),
            discovered_dependencies,
        })
    }

    pub fn evaluate_types(
        &self,
        canonical_or_alias: &str,
    ) -> Option<verter_analysis::type_expand::ExpandedComponentTypes> {
        self.provenance
            .evaluate_types_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let resolved =
            self.resolve_component_meta(canonical_or_alias, crate::types::ResolverMode::Expanded)?;
        resolved.evaluated_types
    }

    /// Single native component-meta query.
    ///
    /// Uses `resolve_component_meta(Expanded)` as the single enrichment owner,
    /// then projects the result through the analysis-owned `extract_component_meta`.
    ///
    /// Returns `None` if the file doesn't exist.
    pub fn get_component_meta(
        &self,
        canonical_or_alias: &str,
    ) -> Option<verter_analysis::component_meta::ComponentMetaAnalysis> {
        self.provenance
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let started = component_meta_debug_enabled().then(Instant::now);

        let resolved =
            self.resolve_component_meta(canonical_or_alias, crate::types::ResolverMode::Expanded)?;
        let include_fallthrough = !resolved
            .cached_eval_inputs
            .as_ref()
            .is_some_and(|inputs| inputs.overflow.is_some());
        let meta = extract_component_meta_from_resolved(
            self,
            canonical_or_alias,
            &resolved,
            include_fallthrough,
        );
        if let Some(started) = started {
            component_meta_debug(format!(
                "get_component_meta owner={} took {:?}",
                self.resolve_alias_or_canonical(canonical_or_alias),
                started.elapsed(),
            ));
        }
        Some(meta)
    }

    /// Combined query: resolves component-meta once and returns both the
    /// analysis projection and the resolved-meta sidecar. Avoids the
    /// double `resolve_component_meta(Expanded)` that happens if callers
    /// invoke `get_component_meta()` + `resolve_component_meta()` separately.
    pub fn get_component_meta_with_resolution(
        &self,
        canonical_or_alias: &str,
    ) -> Option<(
        verter_analysis::component_meta::ComponentMetaAnalysis,
        crate::meta_resolve::ResolvedComponentMetaState,
    )> {
        self.provenance
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let resolved =
            self.resolve_component_meta(canonical_or_alias, crate::types::ResolverMode::Expanded)?;
        let include_fallthrough = !resolved
            .cached_eval_inputs
            .as_ref()
            .is_some_and(|inputs| inputs.overflow.is_some());
        let analysis = extract_component_meta_from_resolved(
            self,
            canonical_or_alias,
            &resolved,
            include_fallthrough,
        );
        Some((analysis, resolved))
    }

    /// Resolve the accepted surface for a component's fallthrough inheritance.
    ///
    /// This is an internal method — the host owns all inheritance semantics.
    /// Returns `None` if the file doesn't exist or has no analysis.
    pub fn resolve_fallthrough_surface(
        &self,
        canonical_id: &str,
    ) -> Option<crate::types::FallthroughResolution> {
        let mut visiting = rustc_hash::FxHashSet::default();
        self.resolve_fallthrough_surface_internal(canonical_id, &mut visiting)
    }

    /// Internal recursive method with cycle detection.
    fn resolve_fallthrough_surface_internal(
        &self,
        canonical_id: &str,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Option<crate::types::FallthroughResolution> {
        self.resolve_fallthrough_surface_internal_with_overrides_in_view(
            canonical_id,
            None,
            visiting,
            None,
        )
    }

    fn resolve_fallthrough_surface_internal_with_overrides_in_view(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        visiting: &mut rustc_hash::FxHashSet<String>,
        store_view: Option<&HostStoreView>,
    ) -> Option<crate::types::FallthroughResolution> {
        use verter_analysis::component_meta::*;
        let started = component_meta_debug_enabled().then(Instant::now);

        // Cycle detection
        if !visiting.insert(canonical_id.to_string()) {
            return Some(crate::types::FallthroughResolution {
                accepted_props: Vec::new(),
                accepted_events: Vec::new(),
                accepted_surface_completeness: AcceptedSurfaceCompleteness::LowerBound,
                fallthrough_surface: FallthroughSurface::Branches {
                    branches: vec![FallthroughBranch {
                        branch_key: "0".to_string(),
                        condition_text: None,
                        props: Vec::new(),
                        events: Vec::new(),
                        root_chain: vec![ResolvedRootStep::Unresolved {
                            tag: "component".to_string(),
                            reason: UnresolvedBranchReason::Cycle {
                                canonical_id: canonical_id.to_string(),
                            },
                        }],
                        status: BranchStatus::Unresolved {
                            reason: UnresolvedBranchReason::Cycle {
                                canonical_id: canonical_id.to_string(),
                            },
                        },
                    }],
                },
                fact_versions: self.current_dependency_fact_versions_in_view(
                    canonical_id,
                    &std::collections::BTreeSet::new(),
                    store_view,
                ),
            });
        }

        let mut executor = FallthroughRequestExecutor::new(
            self,
            canonical_id.to_string(),
            prop_type_overrides,
            visiting,
        )
        .with_fixed_view(store_view);
        let result = run_stable_request(&self.fallthrough_singleflight, &mut executor)
            .expect("fallthrough request execution is infallible");

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

        visiting.remove(canonical_id);
        if let Some(started) = started {
            match result.source {
                RequestSource::Cache => component_meta_debug(format!(
                    "resolve_fallthrough owner={} cached attempt={} took {:?}",
                    canonical_id,
                    result.attempts.saturating_sub(1),
                    started.elapsed(),
                )),
                RequestSource::Flight { role, .. } => component_meta_debug(format!(
                    "resolve_fallthrough owner={} role={:?} stable attempt={} took {:?}",
                    canonical_id,
                    role,
                    result.attempts.saturating_sub(1),
                    started.elapsed(),
                )),
                RequestSource::Fallback => component_meta_debug(format!(
                    "resolve_fallthrough owner={} retries_exhausted took {:?}",
                    canonical_id,
                    started.elapsed(),
                )),
            }
        }
        result.value
    }

    fn compute_fallthrough_surface_uncached(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        visiting: &mut rustc_hash::FxHashSet<String>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<crate::types::FallthroughResolution> {
        let whole_hash = store_view
            .and_then(|view| view.whole_hash(canonical_id))
            .or_else(|| self.get_whole_hash(canonical_id))
            .unwrap_or_default();
        let resolved = self.compute_component_meta_state(
            canonical_id,
            crate::types::ResolverMode::Expanded,
            whole_hash,
            store_view,
        )?;
        let mut fallthrough_fact_versions = resolved.fact_versions.clone();

        let resolved_macros =
            component_meta_resolved_macros(&resolved.snapshot, &resolved.resolved_macros);
        let resolved_type_registry = component_meta_type_registry(&resolved.resolved_type_registry);
        let input = verter_analysis::component_meta::ComponentMetaInput {
            macros: &resolved.snapshot.macros,
            bindings: &resolved.snapshot.bindings,
            imports: &resolved.snapshot.imports,
            template: resolved.snapshot.template.as_deref(),
            options_api: resolved.snapshot.options_api.as_ref(),
            analysis_flags: verter_analysis::types::AnalysisFlags::from_bits_truncate(
                resolved.snapshot.script_flags,
            ),
            styles: &resolved.snapshot.styles,
            vue_api_calls: &resolved.snapshot.vue_api_calls,
            store_usages: &resolved.snapshot.store_usages,
            resolved_macros: &resolved_macros,
            resolved_type_registry: &resolved_type_registry,
            evaluated_types: resolved.evaluated_types.as_ref(),
            file_path: canonical_id,
        };
        let base_meta = verter_analysis::component_meta::extract_component_meta(input);
        let fallthrough_resolver = HostFallthroughResolver {
            host: self,
            parent_canonical_id: canonical_id,
            store_view,
        };
        let eval_env = if let Some(ref cached_inputs) = resolved.cached_eval_inputs {
            self.build_fallthrough_eval_env_with_inputs_in_view(
                canonical_id,
                &resolved.snapshot,
                prop_type_overrides,
                cached_inputs,
                store_view,
            )
        } else {
            self.build_fallthrough_eval_env_in_view(
                canonical_id,
                &resolved.snapshot,
                prop_type_overrides,
                store_view,
            )
        };

        let resolved_surface = resolver_resolve_fallthrough_surface(
            &fallthrough_resolver,
            canonical_id,
            &resolved.snapshot,
            &base_meta,
            prop_type_overrides,
            eval_env,
            fallthrough_fact_versions,
            visiting,
        );

        Some(crate::types::FallthroughResolution {
            accepted_props: resolved_surface.accepted_props,
            accepted_events: resolved_surface.accepted_events,
            accepted_surface_completeness: resolved_surface.accepted_surface_completeness,
            fallthrough_surface: resolved_surface.fallthrough_surface,
            fact_versions: resolved_surface.fact_versions,
        })
    }

    fn build_fallthrough_eval_env_in_view(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        let dep_resolutions = self
            .dependency_resolutions_for_eval_in_view(canonical_id, store_view)
            .unwrap_or_default();
        let imported_inputs = self.imported_eval_inputs_with_owner_context_in_view(
            canonical_id,
            snapshot,
            &dep_resolutions,
            None,
            None,
            store_view,
        );
        self.build_fallthrough_eval_env_with_inputs_in_view(
            canonical_id,
            snapshot,
            prop_type_overrides,
            &imported_inputs,
            store_view,
        )
    }

    fn build_fallthrough_eval_env_with_inputs_in_view(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        imported_inputs: &ImportedEvalInputs,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        Some(
            self.build_owner_eval_env_with_inputs_from_owner_env_in_view(
                canonical_id,
                snapshot,
                imported_inputs,
                prop_type_overrides,
                None,
                store_view,
            )?
            .env,
        )
    }

    fn build_owner_eval_env_with_inputs(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        imported_inputs: &ImportedEvalInputs,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
    ) -> Option<OwnerEvalEnvBuild> {
        self.build_owner_eval_env_with_inputs_from_owner_env(
            canonical_id,
            snapshot,
            imported_inputs,
            prop_type_overrides,
            None,
        )
    }

    fn build_owner_eval_env_with_inputs_in_view(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        imported_inputs: &ImportedEvalInputs,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<OwnerEvalEnvBuild> {
        self.build_owner_eval_env_with_inputs_from_owner_env_in_view(
            canonical_id,
            snapshot,
            imported_inputs,
            prop_type_overrides,
            None,
            store_view,
        )
    }

    fn build_owner_eval_env_with_inputs_from_owner_env(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        imported_inputs: &ImportedEvalInputs,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        owner_env: Option<verter_analysis::type_eval::EvalEnv>,
    ) -> Option<OwnerEvalEnvBuild> {
        self.build_owner_eval_env_with_inputs_from_owner_env_in_view(
            canonical_id,
            snapshot,
            imported_inputs,
            prop_type_overrides,
            owner_env,
            None,
        )
    }

    fn build_owner_eval_env_with_inputs_from_owner_env_in_view(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        imported_inputs: &ImportedEvalInputs,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        owner_env: Option<verter_analysis::type_eval::EvalEnv>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<OwnerEvalEnvBuild> {
        let started = component_meta_debug_enabled().then(Instant::now);
        let assembler = HostOwnerEvalEnvAssembler {
            host: self,
            store_view,
        };
        let built = build_owner_eval_env_with_inputs(
            &assembler,
            canonical_id,
            snapshot,
            snapshot.macros.as_ref(),
            imported_inputs,
            prop_type_overrides,
            owner_env,
        )?;
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "build_owner_eval_env owner={} dep_sources={} type_symbols={} value_symbols={} took {:?}",
                canonical_id,
                imported_inputs.sources.len(),
                built.env.type_symbols.len(),
                built.env.value_symbols.len(),
                started.map(|start| start.elapsed()).unwrap_or_default(),
            ));
        }
        Some(built)
    }

    fn materialize_imported_runtime_values_into_env(
        &self,
        snapshot: &FileAnalysisSnapshot,
        owner_local_value_names: &rustc_hash::FxHashSet<String>,
        env: &mut verter_analysis::type_eval::EvalEnv,
    ) {
        self.materialize_imported_runtime_values_into_env_in_view(
            snapshot,
            owner_local_value_names,
            env,
            None,
        )
    }

    fn materialize_imported_runtime_values_into_env_in_view(
        &self,
        snapshot: &FileAnalysisSnapshot,
        owner_local_value_names: &rustc_hash::FxHashSet<String>,
        env: &mut verter_analysis::type_eval::EvalEnv,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) {
        let started = component_meta_debug_enabled().then(Instant::now);
        let resolver = HostRuntimeValueResolver {
            host: self,
            store_view,
        };
        materialize_imported_runtime_values_into_env(
            snapshot.imports.as_slice(),
            owner_local_value_names,
            env,
            &resolver,
        );
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "materialize_runtime_values imports={} value_symbols={} took {:?}",
                snapshot.imports.len(),
                env.value_symbols.len(),
                started.map(|start| start.elapsed()).unwrap_or_default(),
            ));
        }
    }

    fn build_generic_child_prop_overrides(
        &self,
        snapshot: &FileAnalysisSnapshot,
        usage_index: u32,
        eval_env: &mut Option<verter_analysis::type_eval::EvalEnv>,
    ) -> Option<rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>> {
        if !self.config.generic_root_propagation {
            return None;
        }

        let template = snapshot.template.as_deref()?;
        let usage = template.components.get(usage_index as usize)?;
        let mut overrides = rustc_hash::FxHashMap::default();

        for prop in &usage.props {
            if prop.from_spread {
                continue;
            }
            if usage.is_dynamic && prop.name == "is" {
                continue;
            }

            let Some(prop_type) = resolve_usage_prop_type(prop, eval_env) else {
                continue;
            };
            overrides.insert(prop.name.clone(), prop_type);
        }

        if overrides.is_empty() {
            None
        } else {
            Some(overrides)
        }
    }

    fn resolve_root_consumption(
        &self,
        snapshot: &FileAnalysisSnapshot,
        element_index: u32,
        base: &verter_analysis::component_meta::ConsumedRootBindings,
        has_unknown_spread: bool,
        eval_env: &mut Option<verter_analysis::type_eval::EvalEnv>,
    ) -> ResolvedConsumedBindings {
        use verter_analysis::component_meta::PartialBranchReason;

        let mut resolved = ResolvedConsumedBindings {
            bindings: verter_analysis::component_meta::ConsumedRootBindings {
                attrs: base.attrs.clone(),
                listeners: base.listeners.clone(),
                has_dynamic_attr_name: base.has_dynamic_attr_name,
                has_dynamic_listener_name: base.has_dynamic_listener_name,
            },
            partial_reasons: Vec::new(),
        };

        if base.has_dynamic_attr_name {
            push_partial_reason(
                &mut resolved.partial_reasons,
                PartialBranchReason::DynamicAttrName,
            );
        }
        if base.has_dynamic_listener_name {
            push_partial_reason(
                &mut resolved.partial_reasons,
                PartialBranchReason::DynamicListenerName,
            );
        }

        if has_unknown_spread {
            let Some(template) = snapshot.template.as_deref() else {
                push_partial_reason(
                    &mut resolved.partial_reasons,
                    PartialBranchReason::UnknownSpread,
                );
                return resolved;
            };

            let Some(element) = template.elements.get(element_index as usize) else {
                push_partial_reason(
                    &mut resolved.partial_reasons,
                    PartialBranchReason::UnknownSpread,
                );
                return resolved;
            };

            let spread_directives: Vec<_> = element
                .directives
                .iter()
                .filter(|directive| directive.name == "bind" && directive.argument.is_none())
                .collect();

            if spread_directives.is_empty() {
                push_partial_reason(
                    &mut resolved.partial_reasons,
                    PartialBranchReason::UnknownSpread,
                );
            }

            for directive in spread_directives {
                let Some(expression) = directive.expression.as_deref() else {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                    continue;
                };

                let Some(env) = eval_env.as_mut() else {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                    continue;
                };

                let Some(ty) =
                    verter_analysis::type_eval_build::evaluate_value_expression(expression, env)
                else {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                    continue;
                };

                let Some(summary) = known_spread_keys_from_type_expr(&ty) else {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                    continue;
                };

                resolved.bindings.attrs.extend(summary.attrs.into_iter());
                resolved
                    .bindings
                    .listeners
                    .extend(summary.listeners.into_iter());
                if !summary.exact {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                }
            }
        }

        resolved.bindings.attrs.sort();
        resolved.bindings.attrs.dedup();
        resolved.bindings.listeners.sort();
        resolved.bindings.listeners.dedup();
        resolved.partial_reasons.sort();
        resolved.partial_reasons.dedup();
        resolved
    }

    fn resolve_dynamic_root_candidates(
        &self,
        snapshot: &FileAnalysisSnapshot,
        usage_index: u32,
        eval_env: &mut Option<verter_analysis::type_eval::EvalEnv>,
    ) -> Vec<DynamicRootCandidate> {
        let Some(template) = snapshot.template.as_deref() else {
            return Vec::new();
        };
        let Some(usage) = template.components.get(usage_index as usize) else {
            return Vec::new();
        };
        let Some(is_prop) = usage.props.iter().find(|prop| prop.name == "is") else {
            return Vec::new();
        };

        let expression = is_prop
            .expression
            .clone()
            .or_else(|| is_prop.is_shorthand.then(|| is_prop.name.clone()));
        let Some(expression) = expression else {
            return Vec::new();
        };

        let mut candidates = Vec::new();
        if let Some(lowered) =
            verter_analysis::type_eval_build::parse_value_expression_type(&expression)
        {
            candidates.extend(collect_dynamic_root_candidates_from_type(
                &lowered,
                snapshot.imports.as_slice(),
            ));
        }
        if let Some(env) = eval_env.as_mut() {
            if let Some(evaluated) =
                verter_analysis::type_eval_build::evaluate_value_expression(&expression, env)
            {
                candidates.extend(collect_dynamic_root_candidates_from_type(
                    &evaluated,
                    snapshot.imports.as_slice(),
                ));
            }
        }

        candidates.sort_by(|left, right| match (left, right) {
            (
                DynamicRootCandidate::NativeTag { tag: left_tag },
                DynamicRootCandidate::NativeTag { tag: right_tag },
            ) => left_tag.cmp(right_tag),
            (
                DynamicRootCandidate::NativeTag { .. },
                DynamicRootCandidate::ComponentImport { .. },
            ) => std::cmp::Ordering::Less,
            (
                DynamicRootCandidate::ComponentImport { .. },
                DynamicRootCandidate::NativeTag { .. },
            ) => std::cmp::Ordering::Greater,
            (
                DynamicRootCandidate::ComponentImport {
                    component_name: left_name,
                    import_source: left_source,
                },
                DynamicRootCandidate::ComponentImport {
                    component_name: right_name,
                    import_source: right_source,
                },
            ) => (left_name, left_source).cmp(&(right_name, right_source)),
        });
        candidates.dedup();
        candidates
    }

    /// Store fallthrough resolution in the compile cache.
    fn cache_fallthrough_result(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        result: &crate::types::FallthroughResolution,
    ) {
        let resolution = Arc::new(result.clone());
        self.fallthrough_cache.insert_arc(
            fallthrough_cache_key(
                canonical_id,
                self.config.generic_root_propagation,
                prop_type_overrides,
            ),
            resolution.clone(),
            result.fact_versions.clone(),
        );
        if prop_type_overrides.is_none() {
            self.mirror_cached_fallthrough_arc(canonical_id, resolution);
        }
    }

    fn mirror_cached_fallthrough_arc(
        &self,
        canonical_id: &str,
        resolution: Arc<crate::types::FallthroughResolution>,
    ) {
        #[cfg(feature = "scheduler")]
        {
            if self.effective_file_state(canonical_id, None).is_some() {
                let mut cc = self
                    .compile_cache
                    .entry(canonical_id.to_string())
                    .or_default();
                cc.cached_fallthrough = Some(crate::types::CachedFallthroughEntry {
                    fact_versions: resolution.fact_versions.clone(),
                    generic_root_propagation: self.config.generic_root_propagation,
                    resolution,
                });
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let _ = (canonical_id, resolution);
        }
    }

    fn parse_dependency_set_for_file(
        &self,
        canonical_id: &str,
    ) -> std::collections::BTreeSet<String> {
        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;

            let Some(source) = self.scheduler.try_get_source(canonical_id) else {
                return std::collections::BTreeSet::new();
            };
            let Some(hd) = source.downcast_data::<HostSourceData>() else {
                return std::collections::BTreeSet::new();
            };

            hd.parse
                .external_requests
                .iter()
                .map(|r| r.resolved_canonical_id.clone())
                .chain(
                    hd.parse
                        .script_analysis
                        .imports
                        .iter()
                        .filter(|imp| imp.source.starts_with('.'))
                        .map(|imp| crate::id::resolve_external(canonical_id, &imp.source)),
                )
                .collect()
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let Some(entry) = files.get(canonical_id) else {
                return std::collections::BTreeSet::new();
            };

            entry
                .external_requests
                .iter()
                .map(|r| r.resolved_canonical_id.clone())
                .chain(
                    entry
                        .script_analysis
                        .imports
                        .iter()
                        .filter(|imp| imp.source.starts_with('.'))
                        .map(|imp| crate::id::resolve_external(canonical_id, &imp.source)),
                )
                .collect()
        }
    }

    fn resolved_dependency_targets(
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
    ) -> std::collections::BTreeSet<String> {
        dep_resolutions
            .values()
            .filter_map(|res| res.effective_target().map(|s| s.to_string()))
            .collect()
    }

    pub(crate) fn sync_transitive_macro_type_dependencies(
        &self,
        canonical_id: &str,
        transitive_deps: &std::collections::BTreeSet<String>,
    ) {
        let mut new_deps = self.parse_dependency_set_for_file(canonical_id);

        #[cfg(feature = "scheduler")]
        let old_deps = {
            let mut cc_ref = self
                .compile_cache
                .entry(canonical_id.to_string())
                .or_default();
            let cc = cc_ref.value_mut();
            new_deps.extend(Self::resolved_dependency_targets(
                &cc.dependency_resolutions,
            ));
            new_deps.extend(transitive_deps.iter().cloned());
            let old_deps = cc.dependencies.clone();
            cc.dependencies = new_deps.clone();
            old_deps
        };

        #[cfg(not(feature = "scheduler"))]
        let old_deps = {
            let mut files = write_lock(&self.files);
            let Some(entry) = files.get_mut(canonical_id) else {
                return;
            };
            new_deps.extend(Self::resolved_dependency_targets(
                &entry.dependency_resolutions,
            ));
            new_deps.extend(transitive_deps.iter().cloned());
            let old_deps = entry.dependencies.clone();
            entry.dependencies = new_deps.clone();
            old_deps
        };

        if old_deps != new_deps {
            self.update_reverse_deps(canonical_id, &old_deps, &new_deps);
        }
    }

    /// Returns the original source for a file by canonical ID or alias.
    /// Returns `None` when the file does not exist in the host.
    pub fn get_source(&self, canonical_or_alias: &str) -> Option<std::sync::Arc<str>> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return None;
                }
            }
            self.scheduler
                .try_get_source(&canonical)
                .map(|s| s.source.clone())
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files.get(&canonical).map(|entry| entry.source.clone())
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_template_analysis(
        &self,
        canonical: &str,
        source: &Arc<str>,
        cached_parse: Option<Arc<verter_core::parser::types::ParsedSfc>>,
        src_blocks: &[crate::SrcBlockInfo],
        external_requests: &[crate::ExternalSourceRequest],
        imports: &[verter_analysis::AnalyzedImport],
        macros: &[verter_analysis::AnalyzedMacro],
        bindings: &[verter_analysis::AnalyzedBinding],
    ) -> Option<Arc<verter_analysis::template::TemplateAnalysisSnapshot>> {
        let ext_map = if !src_blocks.is_empty() {
            let mut map = rustc_hash::FxHashMap::default();
            for req in external_requests {
                let dep_source =
                    self.resolve_dep_source(canonical, &req.resolved_canonical_id, &req.specifier);
                if let Some(source) = dep_source {
                    map.insert(req.resolved_canonical_id.clone(), source);
                }
            }
            map
        } else {
            rustc_hash::FxHashMap::default()
        };

        for req in external_requests {
            if !ext_map.contains_key(&req.resolved_canonical_id) {
                return None;
            }
        }

        let merged_source = if !src_blocks.is_empty() {
            std::borrow::Cow::Owned(crate::compile::merge_external_sources(
                source, src_blocks, &ext_map,
            ))
        } else {
            std::borrow::Cow::Borrowed(source.as_ref())
        };

        let parsed = if src_blocks.is_empty() {
            cached_parse
                .as_deref()
                .map(std::borrow::Cow::Borrowed)
                .unwrap_or_else(|| {
                    std::borrow::Cow::Owned(verter_core::compile::parse_sfc(
                        &merged_source,
                        None,
                        None,
                    ))
                })
        } else {
            std::borrow::Cow::Owned(verter_core::compile::parse_sfc(&merged_source, None, None))
        };

        let alloc = oxc_allocator::Allocator::new();
        let options = verter_core::compile::CodegenOptions {
            target: verter_core::compile::CompileTarget::META,
            filename: Some(canonical.to_string()),
            ..verter_core::compile::CodegenOptions::default()
        };
        let verter_opts = verter_core::compile::VerterCompileOptions {
            extract_template_data: true,
            ..verter_core::compile::VerterCompileOptions::default()
        };
        let compiled = verter_core::compile::compile_from_parsed(
            &merged_source,
            &parsed,
            &options,
            &verter_opts,
            &alloc,
        );

        let has_structural_errors = compiled.errors.iter().any(|d| {
            matches!(
                d.severity,
                verter_core::compile::CompileDiagnosticSeverity::Error
            ) && !d.code.starts_with("XInvalidMacroType")
                && !d.code.starts_with("XMissingMacroType")
        });
        if has_structural_errors {
            return None;
        }

        let raw = compiled.template_data?;
        let (imports, unions, props_name) =
            crate::host_resolve::template_converter_inputs(imports, macros, bindings);
        Some(Arc::new(crate::template_convert::convert_raw_to_analysis(
            &raw,
            &imports,
            &unions,
            props_name.as_deref(),
        )))
    }

    /// Lazily compute template analysis for a VueSfc file that hasn't been compiled.
    ///
    /// Uses `CompileTarget::META` (= SCRIPT + TEMPLATE_DATA) via the core
    /// `compile_from_parsed()` — bypassing the host `compile_entry()` which fails
    /// on unresolved macro type deps. External-src blocks are merged using the
    /// same `merge_external_sources()` helper. Results are persisted on the entry
    /// for inline-template files to avoid recomputation.
    pub(crate) fn compute_template_analysis_if_missing(
        &self,
        canonical: &str,
        snapshot: &mut FileAnalysisSnapshot,
    ) {
        if snapshot.template.is_some() {
            return;
        }

        #[cfg(feature = "scheduler")]
        let (source, cached_parse, src_blocks, external_requests) = {
            use crate::host_executor::HostSourceData;
            if let Some(snap) = self.scheduler.try_get_source(canonical) {
                let Some(hd) = snap.downcast_data::<HostSourceData>() else {
                    return;
                };
                if hd.file_kind != FileKind::VueSfc {
                    return;
                }
                (
                    snap.source.clone(),
                    hd.cached_parse.clone(),
                    hd.parse.src_blocks.clone(),
                    hd.parse.external_requests.clone(),
                )
            } else {
                let Some(source) = self.read_analysis_source(canonical) else {
                    return;
                };
                if !canonical.ends_with(".vue") {
                    return;
                }
                let (parse, parsed) = crate::parse::parse_vue_snapshot(
                    canonical,
                    &source,
                    self.config.effective_scope(),
                );
                (
                    source,
                    Some(Arc::new(parsed)),
                    parse.src_blocks,
                    parse.external_requests,
                )
            }
        };

        #[cfg(not(feature = "scheduler"))]
        let (source, cached_parse, src_blocks, external_requests) = {
            let files = read_lock(&self.files);
            if let Some(entry) = files.get(canonical) {
                if entry.file_kind != FileKind::VueSfc {
                    return;
                }
                (
                    entry.source.clone(),
                    entry.cached_parse.clone(),
                    entry.src_blocks.clone(),
                    entry.external_requests.clone(),
                )
            } else {
                drop(files);
                let Some(source) = self.read_analysis_source(canonical) else {
                    return;
                };
                if !canonical.ends_with(".vue") {
                    return;
                }
                let (parse, parsed) = crate::parse::parse_vue_snapshot(
                    canonical,
                    &source,
                    self.config.effective_scope(),
                );
                (
                    source,
                    Some(Arc::new(parsed)),
                    parse.src_blocks,
                    parse.external_requests,
                )
            }
        };

        // Resolve external src blocks (e.g., <template src="./tpl.html">)
        let ext_map = if !src_blocks.is_empty() {
            let mut map = rustc_hash::FxHashMap::default();
            for req in &external_requests {
                if let Some(dep_source) =
                    self.resolve_dep_source(canonical, &req.resolved_canonical_id, &req.specifier)
                {
                    map.insert(req.resolved_canonical_id.clone(), dep_source);
                }
            }
            map
        } else {
            rustc_hash::FxHashMap::default()
        };

        // Abort if any external src blocks are unresolved (same guard as compile_entry)
        for req in &external_requests {
            if !ext_map.contains_key(&req.resolved_canonical_id) {
                return;
            }
        }

        let merged_source = if !src_blocks.is_empty() {
            std::borrow::Cow::Owned(crate::compile::merge_external_sources(
                &source,
                &src_blocks,
                &ext_map,
            ))
        } else {
            std::borrow::Cow::Borrowed(source.as_ref())
        };

        // Parse SFC (reuse cached parse when no external src)
        let parsed = if src_blocks.is_empty() {
            cached_parse
                .as_deref()
                .map(std::borrow::Cow::Borrowed)
                .unwrap_or_else(|| {
                    std::borrow::Cow::Owned(verter_core::compile::parse_sfc(
                        &merged_source,
                        None,
                        None,
                    ))
                })
        } else {
            std::borrow::Cow::Owned(verter_core::compile::parse_sfc(&merged_source, None, None))
        };

        // Compile with META target — script codegen + template data, no JS/TSX output
        let alloc = oxc_allocator::Allocator::new();
        let options = verter_core::compile::CodegenOptions {
            target: verter_core::compile::CompileTarget::META,
            filename: Some(canonical.to_string()),
            ..verter_core::compile::CodegenOptions::default()
        };
        let verter_opts = verter_core::compile::VerterCompileOptions {
            extract_template_data: true,
            ..verter_core::compile::VerterCompileOptions::default()
        };
        let compiled = verter_core::compile::compile_from_parsed(
            &merged_source,
            &parsed,
            &options,
            &verter_opts,
            &alloc,
        );

        // Bail on structural compile errors that would invalidate template data.
        // Skip type-resolution errors (XInvalidMacroType, XMissingMacroType) since
        // template slot extraction doesn't depend on type resolution.
        let has_structural_errors = compiled.errors.iter().any(|d| {
            matches!(
                d.severity,
                verter_core::compile::CompileDiagnosticSeverity::Error
            ) && !d.code.starts_with("XInvalidMacroType")
                && !d.code.starts_with("XMissingMacroType")
        });
        if has_structural_errors {
            return;
        }

        // Convert RawTemplateData → TemplateAnalysisSnapshot using existing converter
        if let Some(raw) = compiled.template_data {
            // Build converter inputs from snapshot (already computed, not stale entry)
            let imports: Vec<(String, String)> = snapshot
                .imports
                .iter()
                .flat_map(|imp| {
                    imp.bindings
                        .iter()
                        .map(|b| (b.name.clone(), imp.source.clone()))
                })
                .collect();

            // Build binding_class_unions + props_binding_name from snapshot
            let mut unions: Vec<(String, Vec<String>)> = Vec::new();
            let define_props = snapshot
                .macros
                .iter()
                .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps);
            if let Some(dp) = define_props {
                for field in &dp.prop_fields {
                    if let Some(type_ann) = &field.type_annotation {
                        let classes = verter_analysis::parse_string_literal_union(type_ann);
                        if !classes.is_empty() {
                            unions.push((field.name.clone(), classes));
                        }
                    }
                }
            }
            for binding in &snapshot.bindings {
                if let Some(type_ann) = &binding.type_annotation {
                    let effective =
                        verter_analysis::unwrap_reactive_type(type_ann).unwrap_or(type_ann);
                    let classes = verter_analysis::parse_string_literal_union(effective);
                    if !classes.is_empty() {
                        unions.push((binding.name.clone(), classes));
                    }
                }
            }
            let props_name = define_props.and_then(|dp| dp.binding_name.clone());

            let tpl = crate::template_convert::convert_raw_to_analysis(
                &raw,
                &imports,
                &unions,
                props_name.as_deref(),
            );
            let tpl_arc = Arc::new(tpl);
            snapshot.template = Some(Arc::clone(&tpl_arc));

            // Persist for inline templates only. Files with external src
            // blocks are NOT persisted to avoid stale cache when the external
            // dep changes (reverse-dep invalidation only clears compile_slots).
            if src_blocks.is_empty() {
                #[cfg(feature = "scheduler")]
                if let Some(mut cc) = self.compile_cache.get_mut(canonical) {
                    cc.raw_template_analysis = Some(tpl_arc);
                }

                #[cfg(not(feature = "scheduler"))]
                {
                    let mut files = write_lock(&self.files);
                    if let Some(entry) = files.get_mut(canonical) {
                        entry.template_analysis = Some(tpl_arc);
                    }
                }
            }
        }
    }

    /// Returns a serializable snapshot of the file's static analysis data.
    /// Returns `None` if the file doesn't exist.
    /// When `eager_analysis` is false, computes analysis on demand from stored source.
    ///
    /// Template analysis is lazily computed via `CompileTarget::META` when the scope
    /// includes template analysis and no prior compilation has populated it.
    ///
    /// Import `resolved_canonical_id` fields are populated lazily using the host's
    /// file map, alias map, and parent dependency set.
    pub fn get_analysis(&self, canonical_or_alias: &str) -> Option<FileAnalysisSnapshot> {
        self.provenance
            .get_analysis_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let analysis_started = component_meta_debug_enabled().then(Instant::now);
        self.get_analysis_snapshot_internal(&canonical, analysis_started)
    }

    fn get_analysis_snapshot_internal(
        &self,
        canonical: &str,
        analysis_started: Option<Instant>,
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
            use crate::host_executor::HostSourceData;

            let Some(source_snap) = self.scheduler.try_get_source(canonical) else {
                let source = self.read_analysis_source(canonical)?;
                let snapshot = self.build_snapshot_from_source(canonical, &source);
                return Some(self.finalize_analysis_snapshot(
                    canonical,
                    snapshot,
                    self.config.effective_scope().needs_template_analysis(),
                    analysis_started,
                ));
            };
            let hd = source_snap.downcast_data::<HostSourceData>()?;
            let file_kind = hd.file_kind;
            let source = source_snap.source.clone();
            let cached_parse = hd.cached_parse.clone();

            let scope = self.config.effective_scope();
            if file_kind == FileKind::VueSfc
                && (!scope.needs_script_analysis() || !scope.needs_style_analysis())
            {
                let stored_script = hd.parse.script_analysis.clone();
                let stored_styles = self
                    .scheduler
                    .try_get_analysis(canonical)
                    .and_then(|a| {
                        a.downcast_data::<crate::host_executor::HostAnalysisData>()
                            .map(|ad| Arc::clone(&ad.style_analyses))
                    })
                    .unwrap_or_else(|| Arc::new(Vec::new()));
                let template = self
                    .compile_cache
                    .get(canonical)
                    .and_then(|cc| cc.raw_template_analysis.clone());
                let export_sigs = self
                    .scheduler
                    .try_get_analysis(canonical)
                    .and_then(|a| {
                        a.downcast_data::<crate::host_executor::HostAnalysisData>()
                            .map(|ad| ad.export_signatures.clone())
                    })
                    .unwrap_or_default();
                drop(source_snap);

                let mut script_analysis = if !scope.needs_script_analysis() {
                    if let Some(parsed) = cached_parse.as_deref() {
                        crate::parse::build_script_analysis_from_parsed(parsed, &source)
                    } else {
                        crate::parse::build_script_analysis_from_source(&source)
                    }
                } else {
                    stored_script
                };
                let style_analyses = if !scope.needs_style_analysis() {
                    if let Some(parsed) = cached_parse.as_deref() {
                        Arc::new(crate::parse::build_style_analyses_from_parsed(
                            parsed, &source, canonical,
                        ))
                    } else {
                        Arc::new(crate::parse::build_style_analyses_from_source(
                            &source, canonical,
                        ))
                    }
                } else {
                    stored_styles
                };
                if !style_analyses.is_empty() && !script_analysis.bindings.is_empty() {
                    script_analysis.mark_bindings_used_in_style(&style_analyses);
                }
                let snapshot = FileAnalysisSnapshot {
                    imports: script_analysis.imports,
                    module_references: Arc::new(script_analysis.module_references),
                    bindings: script_analysis.bindings,
                    macros: Arc::new(script_analysis.macros),
                    macro_type_deps: Arc::new(script_analysis.macro_type_deps),
                    script_flags: script_analysis.flags.bits(),
                    styles: style_analyses,
                    template,
                    vue_api_calls: Arc::new(script_analysis.vue_api_calls),
                    dom_query_calls: Arc::new(script_analysis.dom_query_calls),
                    css_var_manipulations: Arc::new(script_analysis.css_var_manipulations),
                    script_binding_occurrences: Arc::new(
                        script_analysis.script_binding_occurrences,
                    ),
                    export_signatures: Arc::new(export_sigs),
                    options_api: script_analysis.options_api,
                    store_usages: Arc::new(script_analysis.store_usages),
                    store_definitions: Arc::new(script_analysis.store_definitions),
                    is_typescript: script_analysis.is_typescript,
                };
                return Some(self.finalize_analysis_snapshot(
                    canonical,
                    snapshot,
                    scope.needs_template_analysis(),
                    analysis_started,
                ));
            }
            drop(source_snap);

            let snapshot = self.build_snapshot_from_scheduler(canonical)?;
            Some(self.finalize_analysis_snapshot(
                canonical,
                snapshot,
                self.config.effective_scope().needs_template_analysis(),
                analysis_started,
            ))
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let Some(entry) = files.get(canonical) else {
                drop(files);
                let source = self.read_analysis_source(canonical)?;
                let snapshot = self.build_snapshot_from_source(canonical, &source);
                return Some(self.finalize_analysis_snapshot(
                    canonical,
                    snapshot,
                    self.config.effective_scope().needs_template_analysis(),
                    analysis_started,
                ));
            };

            let scope = self.config.effective_scope();
            if entry.file_kind == FileKind::VueSfc
                && (!scope.needs_script_analysis() || !scope.needs_style_analysis())
            {
                let source = entry.source.clone();
                let stored_script = entry.script_analysis.clone();
                let stored_styles = Arc::clone(&entry.style_analyses);
                let template = entry.template_analysis.clone();
                let cached_parse = entry.cached_parse.clone();
                let export_sigs = entry.export_signatures.clone();
                drop(files);

                let mut script_analysis = if !scope.needs_script_analysis() {
                    if let Some(parsed) = cached_parse.as_deref() {
                        crate::parse::build_script_analysis_from_parsed(parsed, &source)
                    } else {
                        crate::parse::build_script_analysis_from_source(&source)
                    }
                } else {
                    stored_script
                };
                let style_analyses = if !scope.needs_style_analysis() {
                    if let Some(parsed) = cached_parse.as_deref() {
                        Arc::new(crate::parse::build_style_analyses_from_parsed(
                            parsed, &source, &canonical,
                        ))
                    } else {
                        Arc::new(crate::parse::build_style_analyses_from_source(
                            &source, &canonical,
                        ))
                    }
                } else {
                    stored_styles
                };
                if !style_analyses.is_empty() && !script_analysis.bindings.is_empty() {
                    script_analysis.mark_bindings_used_in_style(&style_analyses);
                }
                let snapshot = FileAnalysisSnapshot {
                    imports: script_analysis.imports,
                    module_references: Arc::new(script_analysis.module_references),
                    bindings: script_analysis.bindings,
                    macros: Arc::new(script_analysis.macros),
                    macro_type_deps: Arc::new(script_analysis.macro_type_deps),
                    script_flags: script_analysis.flags.bits(),
                    styles: style_analyses,
                    template,
                    vue_api_calls: Arc::new(script_analysis.vue_api_calls),
                    dom_query_calls: Arc::new(script_analysis.dom_query_calls),
                    css_var_manipulations: Arc::new(script_analysis.css_var_manipulations),
                    script_binding_occurrences: Arc::new(
                        script_analysis.script_binding_occurrences,
                    ),
                    export_signatures: Arc::new(export_sigs),
                    options_api: script_analysis.options_api,
                    store_usages: Arc::new(script_analysis.store_usages),
                    store_definitions: Arc::new(script_analysis.store_definitions),
                    is_typescript: script_analysis.is_typescript,
                };
                return Some(self.finalize_analysis_snapshot(
                    canonical,
                    snapshot,
                    scope.needs_template_analysis(),
                    analysis_started,
                ));
            }

            let snapshot = Self::build_snapshot_from_entry(entry);
            drop(files);
            Some(self.finalize_analysis_snapshot(
                canonical,
                snapshot,
                self.config.effective_scope().needs_template_analysis(),
                analysis_started,
            ))
        }
    }

    /// Get the current whole_hash for a file.
    pub(crate) fn get_whole_hash(&self, canonical: &str) -> Option<Hash16> {
        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;
            let snap = self.scheduler.try_get_source(canonical)?;
            let hd = snap.downcast_data::<HostSourceData>()?;
            Some(hd.parse.whole_hash)
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files.get(canonical).map(|entry| entry.whole_hash)
        }
    }

    /// Returns the semantic hash for a file by canonical ID or alias.
    ///
    /// The semantic hash changes when the file's semantically significant content
    /// changes (script, template, scoped styles). Returns `None` for missing files.
    pub fn get_semantic_hash(&self, canonical_or_alias: &str) -> Option<Hash16> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;
            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return None;
                }
            }
            let snap = self.scheduler.try_get_source(&canonical)?;
            let hd = snap.downcast_data::<HostSourceData>()?;
            Some(hd.parse.semantic_hash)
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files.get(&canonical).map(|entry| entry.semantic_hash)
        }
    }

    /// Returns the compile-blocking dependencies for a Vue SFC.
    ///
    /// This exposes the SFC's external `src` blocks and macro type dependencies
    /// so embedding environments can resolve/load them before triggering codegen.
    pub fn get_compile_blockers(
        &self,
        canonical_or_alias: &str,
    ) -> Option<CompileBlockersSnapshot> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::{HostAnalysisData, HostSourceData};
            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return None;
                }
            }
            let snap = self.scheduler.try_get_source(&canonical)?;
            let hd = snap.downcast_data::<HostSourceData>()?;
            if hd.file_kind != FileKind::VueSfc {
                return None;
            }
            // Use pre-built AnalysisArcs for cheap pointer clone instead of Vec clone
            let macro_type_deps = self
                .scheduler
                .try_get_analysis(&canonical)
                .and_then(|a| {
                    a.downcast_data::<HostAnalysisData>()
                        .map(|ad| Arc::clone(&ad.arcs.macro_type_deps))
                })
                .unwrap_or_else(|| Arc::new(hd.parse.script_analysis.macro_type_deps.clone()));
            Some(CompileBlockersSnapshot {
                external_source_requests: hd.parse.external_requests.clone(),
                macro_type_deps,
            })
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let entry = files.get(&canonical)?;
            if entry.file_kind != FileKind::VueSfc {
                return None;
            }
            Some(CompileBlockersSnapshot {
                external_source_requests: entry.external_requests.clone(),
                macro_type_deps: Arc::clone(&entry.arc_script_cache.macro_type_deps),
            })
        }
    }

    /// Returns analysis snapshots for multiple files in a single lock acquisition.
    ///
    /// More efficient than calling `get_analysis()` in a loop: acquires the
    /// files read-lock once for all files instead of N separate acquisitions.
    ///
    /// Accepts canonical IDs, aliases, or `None` to return all files.
    /// When `canonical_ids` is `None`, returns snapshots for every file in the host.
    pub fn get_analysis_batch(
        &self,
        canonical_ids: &[&str],
    ) -> Vec<(String, FileAnalysisSnapshot)> {
        let mut results = Vec::with_capacity(canonical_ids.len());

        #[cfg(feature = "scheduler")]
        {
            for &id in canonical_ids {
                let canonical = self.resolve_alias_or_canonical(id);
                if let Some(cc) = self.compile_cache.get(&canonical) {
                    if cc.evicted {
                        continue;
                    }
                }
                if let Some(snapshot) = self.build_snapshot_from_scheduler(&canonical) {
                    results.push((canonical, snapshot));
                }
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            for &id in canonical_ids {
                let canonical = self.resolve_alias_or_canonical(id);
                if let Some(entry) = files.get(&canonical) {
                    let snapshot = Self::build_snapshot_from_entry(entry);
                    results.push((canonical, snapshot));
                }
            }
        }

        // Post-process: resolve imports and enrich bindings for all
        for (canonical, snapshot) in &mut results {
            self.resolve_snapshot_imports(canonical, snapshot);
            self.enrich_destructured_bindings(snapshot);
        }
        results
    }

    /// Returns analysis snapshots for all files in the host.
    ///
    /// Single lock acquisition for the entire file map. Use instead of
    /// `list_files()` + loop when you need analysis for every file.
    pub fn get_analysis_all(&self) -> Vec<(String, FileAnalysisSnapshot)> {
        #[cfg(feature = "scheduler")]
        let mut results = {
            let ids = self.scheduler.node_ids();
            let mut results = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(cc) = self.compile_cache.get(&id) {
                    if cc.evicted {
                        continue;
                    }
                }
                if let Some(snapshot) = self.build_snapshot_from_scheduler(&id) {
                    results.push((id, snapshot));
                }
            }
            results
        };

        #[cfg(not(feature = "scheduler"))]
        let mut results = {
            let files = read_lock(&self.files);
            let mut results = Vec::with_capacity(files.len());
            for (canonical, entry) in files.iter() {
                let snapshot = Self::build_snapshot_from_entry(entry);
                results.push((canonical.clone(), snapshot));
            }
            results
        };

        for (canonical, snapshot) in &mut results {
            self.resolve_snapshot_imports(canonical, snapshot);
            self.enrich_destructured_bindings(snapshot);
        }
        results
    }

    /// Build a `FileAnalysisSnapshot` from a `FileEntry` using Arc::clone
    /// for immutable fields and deep clone for mutable fields (imports, bindings).
    #[cfg(not(feature = "scheduler"))]
    pub(crate) fn build_snapshot_from_entry(entry: &crate::FileEntry) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            imports: entry.script_analysis.imports.clone(),
            bindings: entry.script_analysis.bindings.clone(),
            // Arc::clone — cheap pointer bump, no deep copy
            module_references: Arc::clone(&entry.arc_script_cache.module_references),
            macros: Arc::clone(&entry.arc_script_cache.macros),
            macro_type_deps: Arc::clone(&entry.arc_script_cache.macro_type_deps),
            script_flags: entry.script_analysis.flags.bits(),
            styles: Arc::clone(&entry.style_analyses),
            template: entry.template_analysis.clone(),
            vue_api_calls: Arc::clone(&entry.arc_script_cache.vue_api_calls),
            dom_query_calls: Arc::clone(&entry.arc_script_cache.dom_query_calls),
            css_var_manipulations: Arc::clone(&entry.arc_script_cache.css_var_manipulations),
            script_binding_occurrences: Arc::clone(
                &entry.arc_script_cache.script_binding_occurrences,
            ),
            export_signatures: Arc::new(entry.export_signatures.clone()),
            options_api: entry.script_analysis.options_api.clone(),
            store_usages: Arc::clone(&entry.arc_script_cache.store_usages),
            store_definitions: Arc::clone(&entry.arc_script_cache.store_definitions),
            is_typescript: entry.script_analysis.is_typescript,
        }
    }

    /// Build a `FileAnalysisSnapshot` from scheduler snapshots and compile_cache.
    ///
    /// Reads `HostAnalysisData` for script analysis, export signatures, styles,
    /// and pre-computed `AnalysisArcs`. Template analysis comes from compile_cache
    /// (raw_template_analysis). Uses Arc::clone for all immutable fields.
    #[cfg(feature = "scheduler")]
    pub(crate) fn build_snapshot_from_scheduler(
        &self,
        canonical: &str,
    ) -> Option<FileAnalysisSnapshot> {
        use crate::host_executor::HostAnalysisData;

        let analysis_snap = self.scheduler.try_get_analysis(canonical)?;
        let ad = analysis_snap.downcast_data::<HostAnalysisData>()?;

        let template = self
            .compile_cache
            .get(canonical)
            .and_then(|cc| cc.raw_template_analysis.clone());

        Some(FileAnalysisSnapshot {
            imports: ad.script_analysis.imports.clone(),
            bindings: ad.script_analysis.bindings.clone(),
            module_references: Arc::clone(&ad.arcs.module_references),
            macros: Arc::clone(&ad.arcs.macros),
            macro_type_deps: Arc::clone(&ad.arcs.macro_type_deps),
            script_flags: ad.script_analysis.flags.bits(),
            styles: Arc::clone(&ad.style_analyses),
            template,
            vue_api_calls: Arc::clone(&ad.arcs.vue_api_calls),
            dom_query_calls: Arc::clone(&ad.arcs.dom_query_calls),
            css_var_manipulations: Arc::clone(&ad.arcs.css_var_manipulations),
            script_binding_occurrences: Arc::clone(&ad.arcs.script_binding_occurrences),
            export_signatures: Arc::new(ad.export_signatures.clone()),
            options_api: ad.script_analysis.options_api.clone(),
            store_usages: Arc::clone(&ad.arcs.store_usages),
            store_definitions: Arc::clone(&ad.arcs.store_definitions),
            is_typescript: ad.script_analysis.is_typescript,
        })
    }

    /// Resolve the source code of a dependency file.
    ///
    /// Tries scheduler (native) or files map (WASM) first, falling back to
    /// VFS resolution + disk read. Used by template analysis and external src
    /// block resolution.
    pub(crate) fn resolve_dep_source(
        &self,
        owner_canonical: &str,
        resolved_canonical_id: &str,
        specifier: &str,
    ) -> Option<Arc<str>> {
        #[cfg(feature = "scheduler")]
        {
            // Try scheduler first (dep may be loaded)
            if let Some(snap) = self.scheduler.try_get_source(resolved_canonical_id) {
                return Some(snap.source.clone());
            }
            // Try VFS resolution fallback (handles aliases like @/... and bare modules)
            let dep_id = self.resolve_loaded_dependency_canonical(
                owner_canonical,
                specifier,
                verter_vfs::ResolveRequestKind::EsmImport,
            );
            if let Some(ref id) = dep_id {
                // File resolved but not yet in scheduler — try loading from disk
                if self.scheduler.try_get_source(id).is_none() {
                    self.ensure_loaded(id);
                }
            }
            dep_id.and_then(|id| self.scheduler.try_get_source(&id).map(|s| s.source.clone()))
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let dep_id = files
                .contains_key(resolved_canonical_id)
                .then(|| resolved_canonical_id.to_string())
                .or_else(|| {
                    self.resolve_loaded_dependency_canonical(
                        owner_canonical,
                        specifier,
                        verter_vfs::ResolveRequestKind::EsmImport,
                    )
                });
            dep_id.and_then(|id| files.get(&id).map(|e| e.source.clone()))
        }
    }

    /// Populate `resolved_canonical_id` on each import in the snapshot
    /// using the host's file map, alias map, and parent's dependency set.
    pub(crate) fn resolve_snapshot_imports(
        &self,
        parent_canonical_id: &str,
        snapshot: &mut FileAnalysisSnapshot,
    ) {
        for import in &mut snapshot.imports {
            if import.resolved_canonical_id.is_none() {
                let ctx = verter_vfs::ResolutionContext {
                    phase: verter_vfs::ResolvePhase::CodegenBlocker,
                    kind: if import.is_type_only {
                        verter_vfs::ResolveRequestKind::TypeImport
                    } else {
                        verter_vfs::ResolveRequestKind::EsmImport
                    },
                };
                import.resolved_canonical_id =
                    self.resolve_via_vfs(parent_canonical_id, &import.source, ctx);
            }
        }
    }

    /// Enrich destructured composable bindings with per-field reactivity info.
    ///
    /// When a binding has `reactivity_kind: MaybeRef` and its initializer is a
    /// `FunctionCall` to a composable, look up the composable's `return_shape`
    /// from the resolved file's `exported_functions`. If it's `Object(fields)`,
    /// match binding names to field names and replace `MaybeRef` with the
    /// field's actual `ReactivityKind`.
    pub(crate) fn enrich_destructured_bindings(&self, snapshot: &mut FileAnalysisSnapshot) {
        use verter_analysis::types::{BindingInitializer, ComposableReturn, ReactivityKind};

        // Build a map of import source → resolved canonical ID from the snapshot
        let import_resolved: rustc_hash::FxHashMap<&str, &str> = snapshot
            .imports
            .iter()
            .filter_map(|imp| {
                imp.resolved_canonical_id
                    .as_deref()
                    .map(|resolved| (imp.source.as_str(), resolved))
            })
            .collect();

        for binding in &mut snapshot.bindings {
            if binding.reactivity_kind != ReactivityKind::MaybeRef {
                continue;
            }

            let Some(BindingInitializer::FunctionCall {
                callee,
                callee_import_source,
                ..
            }) = &binding.initializer
            else {
                continue;
            };

            let import_source = match callee_import_source {
                Some(src) => src.as_str(),
                None => continue,
            };

            let canonical_id = match import_resolved.get(import_source) {
                Some(id) => *id,
                None => continue,
            };

            // Look up exported_functions from the dep's analysis
            #[cfg(feature = "scheduler")]
            let composable_info = self.scheduler.try_get_analysis(canonical_id).and_then(|a| {
                a.downcast_data::<crate::host_executor::HostAnalysisData>()
                    .and_then(|ad| {
                        ad.script_analysis
                            .exported_functions
                            .iter()
                            .find(|f| f.name == *callee)
                            .and_then(|f| f.composable.clone())
                    })
            });

            #[cfg(not(feature = "scheduler"))]
            let composable_info = {
                let files = read_lock(&self.files);
                files.get(canonical_id).and_then(|entry| {
                    entry
                        .script_analysis
                        .exported_functions
                        .iter()
                        .find(|f| f.name == *callee)
                        .and_then(|f| f.composable.clone())
                })
            };

            let Some(info) = composable_info else {
                continue;
            };

            match &info.return_shape {
                ComposableReturn::Object(fields) => {
                    if let Some(field) = fields.iter().find(|f| f.name == binding.name) {
                        binding.reactivity_kind = field.reactivity;
                        binding.is_reactive = !matches!(field.reactivity, ReactivityKind::None);
                    }
                }
                ComposableReturn::Single(kind) => {
                    binding.reactivity_kind = *kind;
                    binding.is_reactive = !matches!(kind, ReactivityKind::None);
                }
                _ => {}
            }
        }
    }

    /// Returns stored diagnostics for a file+profile without triggering compilation.
    /// Returns `None` if the file doesn't exist or has no diagnostics for this profile.
    pub fn get_diagnostics(
        &self,
        canonical_or_alias: &str,
        profile: &CompileProfile,
    ) -> Option<DiagnosticsSnapshot> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let profile_hash = compile_profile_hash(profile);

        #[cfg(feature = "scheduler")]
        {
            let cc = self.compile_cache.get(&canonical)?;
            if cc.evicted {
                return None;
            }
            cc.latest_diagnostics.get(&profile_hash).cloned()
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let entry = files.get(&canonical)?;
            entry.latest_diagnostics.get(&profile_hash).cloned()
        }
    }

    /// Returns the monotonic diagnostics generation counter for a file.
    /// Incremented on every write to `latest_diagnostics`. Used by the LSP
    /// cache to detect host-driven recompiles without a document version change.
    pub fn get_diagnostics_generation(&self, canonical_or_alias: &str) -> Option<u64> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            let cc = self.compile_cache.get(&canonical)?;
            if cc.evicted {
                return None;
            }
            Some(cc.diagnostics_generation)
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files.get(&canonical).map(|e| e.diagnostics_generation)
        }
    }

    /// Bump the diagnostics generation counter for a file without changing
    /// its diagnostics.
    pub fn bump_diagnostics_generation(&self, canonical_or_alias: &str) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        if let Some(mut cc) = self.compile_cache.get_mut(&canonical) {
            cc.diagnostics_generation += 1;
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical) {
                entry.diagnostics_generation += 1;
            }
        }
    }

    /// Clear all compile slots for a specific file.
    pub fn invalidate_compile_slots(&self, canonical_or_alias: &str) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        if let Some(mut cc) = self.compile_cache.get_mut(&canonical) {
            cc.compile_slots.clear();
            cc.cached_resolved_meta.clear();
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical) {
                entry.compile_slots.clear();
                entry.cached_resolved_meta.clear();
            }
        }
    }

    /// Invalidate compile outputs of files that depend on the given path.
    ///
    /// Unlike `remove()`, this works even when the dependency file was never
    /// loaded into the host but reverse-dependency edges were registered.
    pub fn invalidate_dependents_of(&self, canonical_or_alias: &str) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        self.smart_invalidate_dependents(&canonical, &[], &[]);
    }

    /// Remove a file from the host, cleaning up aliases, dependencies,
    /// and invalidating compile slots of any dependents.
    pub fn remove(&self, canonical_or_alias: &str) -> Option<HostRemoveResult> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            // Read aliases and dependencies from compile_cache before removing.
            let (aliases, deps) = {
                let cc = self.compile_cache.get(&canonical)?;
                (cc.aliases.clone(), cc.dependencies.clone())
            };

            {
                let mut alias_map = write_lock(&self.alias_to_canonical);
                for alias in &aliases {
                    alias_map.remove(alias);
                }
            }

            let dependents = {
                let rev = read_lock(&self.reverse_dependencies);
                rev.get(&canonical).cloned().unwrap_or_default()
            };

            {
                let mut rev = write_lock(&self.reverse_dependencies);
                for dep in &deps {
                    if let Some(owners) = rev.get_mut(dep) {
                        owners.remove(&canonical);
                        if owners.is_empty() {
                            rev.remove(dep);
                        }
                    }
                }
                rev.remove(&canonical);
            }

            // Invalidate compile_cache slots for dependents.
            for owner in &dependents {
                if let Some(mut cc) = self.compile_cache.get_mut(owner) {
                    cc.compile_slots.clear();
                    cc.cached_resolved_meta.clear();
                }
            }

            self.ws().notify_delete(&canonical);
            self.compile_cache.remove(&canonical);
            self.scheduler.remove(&canonical);

            self.bump_store_view_epoch();
            Some(HostRemoveResult {
                canonical_id: canonical,
            })
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let removed = {
                let mut files = write_lock(&self.files);
                files.remove(&canonical)
            }?;

            {
                let mut alias_map = write_lock(&self.alias_to_canonical);
                for alias in &removed.aliases {
                    alias_map.remove(alias);
                }
            }

            let dependents = {
                let rev = read_lock(&self.reverse_dependencies);
                rev.get(&canonical).cloned().unwrap_or_default()
            };

            {
                let mut rev = write_lock(&self.reverse_dependencies);
                for dep in &removed.dependencies {
                    if let Some(owners) = rev.get_mut(dep) {
                        owners.remove(&canonical);
                        if owners.is_empty() {
                            rev.remove(dep);
                        }
                    }
                }
                rev.remove(&canonical);
            }

            if !dependents.is_empty() {
                let mut files = write_lock(&self.files);
                for owner in &dependents {
                    if let Some(file) = files.get_mut(owner) {
                        file.compile_slots.clear();
                        file.cached_resolved_meta.clear();
                    }
                }
            }

            self.ws().notify_delete(&canonical);

            self.bump_store_view_epoch();
            Some(HostRemoveResult {
                canonical_id: canonical,
            })
        }
    }

    /// Returns the list of virtual node kinds for a file.
    /// Returns an empty vec if the file doesn't exist.
    pub fn list_virtual_nodes(&self, canonical_or_alias: &str) -> Vec<VirtualNodeKind> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;
            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return Vec::new();
                }
            }
            if let Some(snap) = self.scheduler.try_get_source(&canonical) {
                if let Some(hd) = snap.downcast_data::<HostSourceData>() {
                    return hd.parse.meta.virtual_nodes();
                }
            }
            Vec::new()
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files
                .get(&canonical)
                .map(|e| e.all_virtual_nodes())
                .unwrap_or_default()
        }
    }

    /// Provide caller-resolved import dependency resolution records.
    ///
    /// Called after `upsert()` when the caller resolves import specifiers
    /// (tsconfig paths, vite aliases, etc.) using bundler/LSP resolution.
    /// Each record maps a raw import specifier to its resolved canonical ID
    /// (or a list of candidate canonical IDs).
    ///
    /// Records are merged into the file's `dependency_resolutions` map (keyed by
    /// specifier). The flat `dependencies` set is updated in parallel for
    /// reverse-dependency tracking.
    pub fn set_import_dependencies(
        &self,
        canonical_or_alias: &str,
        resolutions: Vec<DependencyResolution>,
    ) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let parse_deps = self.parse_dependency_set_for_file(&canonical);

        // Build VFS exact resolutions for ALL relevant (phase, kind) contexts.
        let vfs_resolutions: Vec<verter_vfs::ExactResolution> = resolutions
            .iter()
            .flat_map(|r| {
                let resolved = r.resolved_canonical_id.as_ref().map(|id| {
                    let norm = canonicalize_id(id);
                    norm.into_owned()
                });
                let possible: Vec<String> = r
                    .possible_canonical_ids
                    .iter()
                    .map(|c| {
                        let norm = canonicalize_id(c);
                        norm.into_owned()
                    })
                    .collect();
                use verter_vfs::{ResolvePhase as P, ResolveRequestKind as K};
                [
                    (P::CodegenBlocker, K::EsmImport),
                    (P::CodegenBlocker, K::TypeImport),
                    (P::ProviderGraph, K::EsmImport),
                    (P::ProviderGraph, K::TypeImport),
                ]
                .into_iter()
                .map(move |(phase, kind)| verter_vfs::ExactResolution {
                    specifier: r.specifier.clone(),
                    phase,
                    kind,
                    resolved_canonical_id: resolved.clone(),
                    possible_canonical_ids: possible.clone(),
                })
            })
            .collect();

        // Normalize resolutions and persist direct import resolutions.
        let mut dep_resolutions = rustc_hash::FxHashMap::default();
        for mut res in resolutions {
            if let Some(ref mut id) = res.resolved_canonical_id {
                let norm = canonicalize_id(id);
                if norm != id.as_str() {
                    *id = norm.into_owned();
                }
            }
            for candidate in &mut res.possible_canonical_ids {
                let norm = canonicalize_id(candidate);
                if norm != candidate.as_str() {
                    *candidate = norm.into_owned();
                }
            }
            dep_resolutions.insert(res.specifier.clone(), res);
        }

        // Preserve already-discovered transitive macro-type deps; compilation
        // refreshes them, but direct import updates should not discard them.
        #[cfg(feature = "scheduler")]
        let old_transitive_deps = {
            let mut cc_ref = self.compile_cache.entry(canonical.clone()).or_default();
            let cc = cc_ref.value_mut();
            let old_deps = cc.dependencies.clone();
            let old_direct_deps = {
                let mut deps = parse_deps.clone();
                deps.extend(Self::resolved_dependency_targets(
                    &cc.dependency_resolutions,
                ));
                deps
            };
            cc.dependency_resolutions = dep_resolutions.clone();
            old_deps
                .difference(&old_direct_deps)
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
        };
        #[cfg(not(feature = "scheduler"))]
        let old_transitive_deps = {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical) {
                let old_deps = entry.dependencies.clone();
                let old_direct_deps = {
                    let mut deps = parse_deps.clone();
                    deps.extend(Self::resolved_dependency_targets(
                        &entry.dependency_resolutions,
                    ));
                    deps
                };
                entry.dependency_resolutions = dep_resolutions;
                old_deps
                    .difference(&old_direct_deps)
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>()
            } else {
                std::collections::BTreeSet::new()
            }
        };

        self.sync_transitive_macro_type_dependencies(&canonical, &old_transitive_deps);

        // Sync exact resolutions to workspace.
        self.ws().set_exact_resolutions(&canonical, vfs_resolutions);
    }

    /// Returns all known canonical file IDs and their file kinds.
    pub fn list_files(&self) -> Vec<(String, FileKind)> {
        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;
            self.scheduler
                .node_ids()
                .into_iter()
                .filter_map(|id| {
                    if let Some(cc) = self.compile_cache.get(&id) {
                        if cc.evicted {
                            return None;
                        }
                    }
                    let snap = self.scheduler.try_get_source(&id)?;
                    let hd = snap.downcast_data::<HostSourceData>()?;
                    Some((id, hd.file_kind))
                })
                .collect()
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files
                .iter()
                .map(|(id, entry)| (id.clone(), entry.file_kind))
                .collect()
        }
    }

    pub(crate) fn raw_template_analysis_for_file(
        &self,
        canonical: &str,
    ) -> Option<Arc<verter_analysis::template::TemplateAnalysisSnapshot>> {
        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;
            if let Some(cc) = self.compile_cache.get(canonical) {
                if cc.evicted {
                    return None;
                }
            }
            let source_snap = self.scheduler.try_get_source(canonical)?;
            let hd = source_snap.downcast_data::<HostSourceData>()?;
            if hd.file_kind != FileKind::VueSfc {
                return None;
            }
            drop(source_snap);
            let mut snapshot = self.build_snapshot_from_scheduler(canonical)?;
            self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            snapshot.template
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let mut snapshot = {
                let files = read_lock(&self.files);
                let entry = files.get(canonical)?;
                if entry.file_kind != FileKind::VueSfc {
                    return None;
                }
                Self::build_snapshot_from_entry(entry)
            };
            self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            snapshot.template
        }
    }

    #[cfg(feature = "scheduler")]
    fn compute_override_template_analysis(
        &self,
        canonical: &str,
        profile_hash: u64,
    ) -> Option<Arc<verter_analysis::template::TemplateAnalysisSnapshot>> {
        let override_with_parse = {
            let cc = self.compile_cache.get(canonical)?;
            cc.content_overrides.get(&profile_hash)?.clone()
        };

        self.build_template_analysis(
            canonical,
            &override_with_parse.source,
            override_with_parse.cached_parse.clone(),
            &override_with_parse.parse.src_blocks,
            &override_with_parse.parse.external_requests,
            &override_with_parse.parse.script_analysis.imports,
            &override_with_parse.parse.script_analysis.macros,
            &override_with_parse.parse.script_analysis.bindings,
        )
    }

    /// Returns cross-component CSS variable flow for a given variable name.
    ///
    /// Scans all files in the host to find where the variable is defined (in `<style>`),
    /// referenced via `var()` (in `<style>`), set via `:style` bindings (in `<template>`),
    /// and manipulated via DOM APIs (in `<script>`).
    ///
    /// When `profile` is provided, override-aware style/template/script state is
    /// used for that compile profile. `None` keeps the read profileless/raw.
    pub fn css_var_flow(
        &self,
        var_name: &str,
        profile: Option<&CompileProfile>,
    ) -> verter_analysis::CssVarFlow {
        #[cfg(feature = "scheduler")]
        let profile_hash = profile.map(compile_profile_hash);
        #[cfg(not(feature = "scheduler"))]
        let _ = profile;

        #[cfg(feature = "scheduler")]
        let canonical_ids: Vec<String> = self
            .scheduler
            .node_ids()
            .into_iter()
            .filter(|id| self.compile_cache.get(id).is_none_or(|cc| !cc.evicted))
            .collect();

        #[cfg(not(feature = "scheduler"))]
        let canonical_ids: Vec<String> = {
            let files = read_lock(&self.files);
            files.keys().cloned().collect()
        };

        let mut flow = verter_analysis::CssVarFlow {
            name: var_name.to_string(),
            ..Default::default()
        };

        for canonical_id in canonical_ids {
            let path: std::sync::Arc<std::path::Path> =
                std::sync::Arc::from(std::path::Path::new(canonical_id.as_str()));

            #[cfg(feature = "scheduler")]
            let style_analyses = self
                .effective_style_analyses(&canonical_id, profile_hash)
                .unwrap_or_default();
            #[cfg(not(feature = "scheduler"))]
            let style_analyses = {
                let files = read_lock(&self.files);
                files
                    .get(&canonical_id)
                    .map(|entry| entry.style_analyses.as_ref().clone())
                    .unwrap_or_default()
            };

            // Check style blocks for definitions and var() references
            for style in &style_analyses {
                if let Some(ref css) = style.css {
                    let has_def = css.custom_properties.iter().any(|p| p.name == var_name);
                    if has_def {
                        flow.style_definitions.push(std::sync::Arc::clone(&path));
                    }

                    let has_ref = css.var_usages.iter().any(|u| u.reference.name == var_name);
                    if has_ref {
                        flow.style_var_usages.push(std::sync::Arc::clone(&path));
                    }
                }
            }

            // Check template for :style CSS variable bindings
            #[cfg(feature = "scheduler")]
            let template_analysis = if let Some(profile_hash) = profile_hash {
                self.compile_cache
                    .get(&canonical_id)
                    .and_then(|cc| {
                        if cc.content_overrides.contains_key(&profile_hash) {
                            cc.compile_slots
                                .get(&profile_hash)
                                .and_then(|slot| slot.template_analysis.clone())
                                .map(Arc::new)
                        } else {
                            None
                        }
                    })
                    .or_else(|| {
                        self.compute_override_template_analysis(&canonical_id, profile_hash)
                    })
                    .or_else(|| self.raw_template_analysis_for_file(&canonical_id))
            } else {
                self.raw_template_analysis_for_file(&canonical_id)
            };
            #[cfg(not(feature = "scheduler"))]
            let template_analysis = self.raw_template_analysis_for_file(&canonical_id);

            if let Some(ref tmpl) = template_analysis {
                if tmpl.css_var_names.iter().any(|n| n == var_name) {
                    flow.template_definitions.push(std::sync::Arc::clone(&path));
                }
            }

            // Check script for DOM API CSS variable manipulations
            #[cfg(feature = "scheduler")]
            let script_has_manipulation = self
                .effective_file_state(&canonical_id, profile_hash)
                .map(|efs| {
                    efs.script_analysis
                        .css_var_manipulations
                        .iter()
                        .any(|m| m.var_name == var_name)
                })
                .unwrap_or(false);
            #[cfg(not(feature = "scheduler"))]
            let script_has_manipulation = {
                let files = read_lock(&self.files);
                files
                    .get(&canonical_id)
                    .map(|entry| {
                        entry
                            .script_analysis
                            .css_var_manipulations
                            .iter()
                            .any(|m| m.var_name == var_name)
                    })
                    .unwrap_or(false)
            };

            if script_has_manipulation {
                flow.script_manipulations.push(std::sync::Arc::clone(&path));
            }
        }

        flow
    }

    /// Look up the byte span of an exported name in a target file.
    ///
    /// For `.vue` files: searches `ScriptAnalysisSnapshot.bindings` (script-setup
    /// auto-exports) — spans are SFC-absolute.
    /// For `.ts`/`.js` files: searches `FileEntry.export_signatures` — spans are
    /// file-absolute.
    ///
    /// Returns `None` if the file doesn't exist or the name isn't exported.
    pub fn get_export_span(
        &self,
        canonical_or_alias: &str,
        binding_name: &str,
    ) -> Option<(u32, u32)> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::{HostAnalysisData, HostSourceData};

            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return None;
                }
            }
            let source_snap = self.scheduler.try_get_source(&canonical)?;
            let hd = source_snap.downcast_data::<HostSourceData>()?;
            let file_kind = hd.file_kind;
            drop(source_snap);

            let analysis_snap = self.scheduler.try_get_analysis(&canonical)?;
            let ad = analysis_snap.downcast_data::<HostAnalysisData>()?;

            Self::find_export_span(
                file_kind,
                &ad.script_analysis,
                &ad.export_signatures,
                binding_name,
            )
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let entry = files.get(&canonical)?;
            Self::find_export_span(
                entry.file_kind,
                &entry.script_analysis,
                &entry.export_signatures,
                binding_name,
            )
        }
    }

    /// Shared logic for finding an export span from analysis data.
    fn find_export_span(
        file_kind: FileKind,
        script_analysis: &verter_analysis::ScriptAnalysisSnapshot,
        export_signatures: &[verter_analysis::ExportSignature],
        binding_name: &str,
    ) -> Option<(u32, u32)> {
        if file_kind == FileKind::VueSfc {
            if let Some(binding) = script_analysis
                .bindings
                .iter()
                .find(|b| b.name == binding_name)
            {
                if binding.span.start > 0 || binding.span.end > 0 {
                    return Some((binding.span.start, binding.span.end));
                }
            }
            for mac in &script_analysis.macros {
                if mac.binding_name.as_deref() == Some(binding_name)
                    && (mac.span.start > 0 || mac.span.end > 0)
                {
                    return Some((mac.span.start, mac.span.end));
                }
            }
            if binding_name == "default" {
                if let Some(first_binding) = script_analysis.bindings.first() {
                    if first_binding.span.start > 0 || first_binding.span.end > 0 {
                        return Some((first_binding.span.start, first_binding.span.end));
                    }
                }
                if let Some(first_macro) = script_analysis.macros.first() {
                    if first_macro.span.start > 0 || first_macro.span.end > 0 {
                        return Some((first_macro.span.start, first_macro.span.end));
                    }
                }
                return Some((0, 0));
            }
            return None;
        }

        if let Some(sig) = export_signatures.iter().find(|s| s.name == binding_name) {
            if sig.span.start > 0 || sig.span.end > 0 {
                return Some((sig.span.start, sig.span.end));
            }
        }

        None
    }

    /// Follow re-exports to find the ultimate definition span.
    ///
    /// For a re-export like `export { default as Popup } from './Popup.vue'`,
    /// this follows the chain to find where `Popup` is actually defined.
    /// Returns `(canonical_id, start, end)` of the final definition.
    ///
    /// Uses cycle detection (visited set keyed on `(canonical_id, binding_name)`)
    /// instead of a depth counter. For local exports (no re-export), returns the
    /// span in the same file.
    pub fn get_export_span_follow_reexports(
        &self,
        canonical_or_alias: &str,
        binding_name: &str,
    ) -> Option<(String, u32, u32)> {
        self.get_export_span_follow_reexports_in_view(canonical_or_alias, binding_name, None)
    }

    pub(crate) fn get_export_span_follow_reexports_in_view(
        &self,
        canonical_or_alias: &str,
        binding_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<(String, u32, u32)> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        if let Some(view) = store_view {
            let resolver = HostExportGraphResolver {
                host: self,
                store_view: Some(view),
            };
            return resolver_get_export_span_follow_reexports_from_graph(
                &resolver,
                &canonical,
                binding_name,
            );
        }
        #[cfg(feature = "scheduler")]
        if let Some(cc) = self.compile_cache.get(&canonical) {
            if cc.evicted {
                return None;
            }
        }
        let resolver = HostExportGraphResolver {
            host: self,
            store_view: None,
        };
        resolver_get_export_span_follow_reexports_from_graph(&resolver, &canonical, binding_name)
    }

    /// Resolve an import specifier to its canonical ID using the host's file map,
    /// alias map, and parent's resolved dependencies.
    ///
    /// Returns `None` if the import cannot be resolved to a known file
    /// (e.g., bare specifiers like `vue` or unregistered files).
    pub fn resolve_import(&self, parent_canonical_id: &str, import_source: &str) -> Option<String> {
        let canonical_parent = self.resolve_alias_or_canonical(parent_canonical_id);
        #[cfg(feature = "scheduler")]
        if let Some(cc) = self.compile_cache.get(&canonical_parent) {
            if cc.evicted {
                return None;
            }
        }
        let ctx = verter_vfs::ResolutionContext {
            phase: verter_vfs::ResolvePhase::CodegenBlocker,
            kind: verter_vfs::ResolveRequestKind::EsmImport,
        };
        self.resolve_via_vfs(&canonical_parent, import_source, ctx)
    }

    /// Returns all exports of a file, following re-export chains to their ultimate source.
    ///
    /// For barrel files like `export { default as Button } from './Button.vue'`, this
    /// resolves through the chain to return the ultimate source file and name. For
    /// `export * from './module'`, it recursively resolves the target file's exports.
    ///
    /// Uses cycle detection to prevent infinite loops in circular re-exports.
    pub fn resolve_exports(&self, canonical_or_alias: &str) -> Vec<ResolvedExport> {
        self.resolve_exports_in_view(canonical_or_alias, None)
    }

    pub(crate) fn resolve_exports_in_view(
        &self,
        canonical_or_alias: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Vec<ResolvedExport> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        #[cfg(feature = "scheduler")]
        if let Some(cc) = self.compile_cache.get(&canonical) {
            if cc.evicted {
                return Vec::new();
            }
        }
        let resolver = HostExportGraphResolver {
            host: self,
            store_view,
        };
        let resolved = if store_view.is_some() {
            resolver_resolve_exports_from_graph(&resolver, &canonical)
        } else {
            resolver_resolve_exports_from_graph_best_effort(&resolver, &canonical)
        };
        resolved
            .into_iter()
            .map(|export| ResolvedExport {
                name: export.name,
                is_type: export.is_type,
                source_canonical_id: export.source_canonical_id,
                source_name: export.source_name,
            })
            .collect()
    }

}

#[cfg_attr(not(test), allow(dead_code))]
fn collect_required_owner_import_names(
    snapshot: &FileAnalysisSnapshot,
    owner_eval_source: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
) -> rustc_hash::FxHashSet<String> {
    let owner_snapshot = ImportedEvalOwnerSnapshot {
        imports: snapshot.imports.as_slice(),
        macros: snapshot.macros.as_ref(),
        bindings: snapshot.bindings.as_ref(),
        macro_type_deps: snapshot.macro_type_deps.as_ref(),
    };
    collect_required_owner_import_names_from_parts(&owner_snapshot, owner_eval_source, owner_env)
}

fn collect_required_owner_import_names_from_parts(
    owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
    owner_eval_source: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
) -> rustc_hash::FxHashSet<String> {
    let started = component_meta_debug_enabled().then(Instant::now);
    let mut required = rustc_hash::FxHashSet::default();
    if owner_eval_source.is_empty() {
        return required;
    }

    if component_meta_debug_enabled() {
        component_meta_debug(format!(
            "collect_required_imports:start macros={} bindings={} source_len={} type_symbols={} value_symbols={}",
            owner_snapshot.macros.len(),
            owner_snapshot.bindings.len(),
            owner_eval_source.len(),
            owner_env.type_symbols.len(),
            owner_env.value_symbols.len(),
        ));
    }
    let type_bindings = rustc_hash::FxHashMap::default();
    let mut active_locals = rustc_hash::FxHashSet::default();
    let macro_type_params =
        verter_analysis::type_eval_build::collect_define_macro_type_params(owner_eval_source);
    let mut define_props_index = 0usize;
    let mut define_emits_index = 0usize;
    let mut define_slots_index = 0usize;
    let imported_binding_names: rustc_hash::FxHashSet<&str> = owner_snapshot
        .imports
        .iter()
        .flat_map(|import| import.bindings.iter().map(|binding| binding.name.as_str()))
        .collect();
    let binding_type_annotations: rustc_hash::FxHashMap<&str, &str> = owner_snapshot
        .bindings
        .iter()
        .filter_map(|binding| {
            binding
                .type_annotation
                .as_deref()
                .map(|type_ann| (binding.name.as_str(), type_ann))
        })
        .collect();

    for (macro_index, mac) in owner_snapshot.macros.iter().enumerate() {
        // Prefer the owner-local surface walk. It can follow local aliases and
        // lazy indexed access without dragging in every imported generic arg
        // behind the macro root. Only fall back to shared macro deps when the
        // local macro analyzer captured no root type references.
        if mac.is_type_based {
            let is_define_slots = mac.kind == verter_analysis::AnalyzedMacroKind::DefineSlots;
            let macro_type_expr = match mac.kind {
                verter_analysis::AnalyzedMacroKind::DefineProps => {
                    let expr = macro_type_params.define_props.get(define_props_index);
                    define_props_index += 1;
                    expr
                }
                verter_analysis::AnalyzedMacroKind::DefineEmits => {
                    let expr = macro_type_params.define_emits.get(define_emits_index);
                    define_emits_index += 1;
                    expr
                }
                verter_analysis::AnalyzedMacroKind::DefineSlots => {
                    let expr = macro_type_params.define_slots.get(define_slots_index);
                    define_slots_index += 1;
                    expr
                }
                _ => None,
            };
            if let Some(expr) = macro_type_expr {
                if !expr.is_unknown() {
                    if is_define_slots {
                        collect_slot_eval_import_names_from_expr(
                            expr,
                            owner_env,
                            &type_bindings,
                            &mut active_locals,
                            &mut required,
                        );
                    } else {
                        collect_surface_eval_import_names_from_expr(
                            expr,
                            owner_env,
                            &type_bindings,
                            &mut active_locals,
                            &mut required,
                        );
                    }
                }
            }
            for type_reference in &mac.type_references {
                if is_define_slots {
                    collect_required_slot_import_names_for_symbol(
                        type_reference,
                        owner_env,
                        &type_bindings,
                        &imported_binding_names,
                        &mut active_locals,
                        &mut required,
                    );
                } else {
                    collect_required_import_names_for_symbol(
                        type_reference,
                        owner_env,
                        &type_bindings,
                        &imported_binding_names,
                        &mut active_locals,
                        &mut required,
                    );
                }
            }
            if mac.type_references.is_empty() {
                for dep in owner_snapshot
                    .macro_type_deps
                    .iter()
                    .filter(|dep| dep.macro_index == macro_index)
                {
                    if imported_binding_names.contains(dep.type_name.as_str()) {
                        required.insert(dep.type_name.clone());
                    }
                }
            }
        }

        for field in &mac.prop_fields {
            if let Some(type_ann) = field.type_annotation.as_deref() {
                let expr = verter_analysis::type_expr_lower::parse_type_annotation(type_ann);
                if !expr.is_unknown() {
                    collect_surface_eval_import_names_from_expr(
                        &expr,
                        owner_env,
                        &type_bindings,
                        &mut active_locals,
                        &mut required,
                    );
                }
            }
        }

        for field in &mac.emit_fields {
            if let Some(payload) = field.payload_type.as_deref() {
                let expr = verter_analysis::type_expr_lower::parse_type_annotation(payload);
                if !expr.is_unknown() {
                    collect_surface_eval_import_names_from_expr(
                        &expr,
                        owner_env,
                        &type_bindings,
                        &mut active_locals,
                        &mut required,
                    );
                }
            }
        }

        if mac.kind != verter_analysis::AnalyzedMacroKind::DefineSlots {
            for slot in &mac.slot_fields {
                for binding in &slot.bindings {
                    if let Some(type_ann) = binding.type_annotation.as_deref() {
                        let expr =
                            verter_analysis::type_expr_lower::parse_type_annotation(type_ann);
                        if !expr.is_unknown() {
                            collect_surface_eval_import_names_from_expr(
                                &expr,
                                owner_env,
                                &type_bindings,
                                &mut active_locals,
                                &mut required,
                            );
                        }
                    }
                }
            }
        }

        for field in &mac.expose_fields {
            let Some(type_ann) = binding_type_annotations.get(field.name.as_str()) else {
                continue;
            };
            let expr = verter_analysis::type_expr_lower::parse_type_annotation(type_ann);
            if expr.is_unknown() {
                continue;
            }
            collect_surface_eval_import_names_from_expr(
                &expr,
                owner_env,
                &type_bindings,
                &mut active_locals,
                &mut required,
            );
        }
    }

    if component_meta_debug_enabled() {
        component_meta_debug(format!(
            "collect_required_imports:end required_count={} required=[{}] total_took={:?}",
            required.len(),
            required.iter().cloned().collect::<Vec<_>>().join(", "),
            started.map(|start| start.elapsed()).unwrap_or_default(),
        ));
    }
    required
}

fn collect_required_import_names_for_type_decl(
    decl: &verter_analysis::type_eval::TypeDeclInfo,
    owner_env: &verter_analysis::type_eval::EvalEnv,
) -> rustc_hash::FxHashSet<String> {
    let mut required = rustc_hash::FxHashSet::default();
    let mut active_locals = rustc_hash::FxHashSet::default();
    let mut type_bindings = rustc_hash::FxHashMap::default();

    for param in &decl.type_parameters {
        type_bindings.insert(
            param.name.clone(),
            verter_analysis::type_expr::TypeExpr::named(param.name.clone()),
        );
        if let Some(constraint) = param.constraint.as_deref() {
            collect_surface_eval_import_names_from_expr(
                constraint,
                owner_env,
                &type_bindings,
                &mut active_locals,
                &mut required,
            );
        }
        if let Some(default) = param.default.as_deref() {
            collect_surface_eval_import_names_from_expr(
                default,
                owner_env,
                &type_bindings,
                &mut active_locals,
                &mut required,
            );
        }
    }

    collect_surface_eval_import_names_from_expr(
        &decl.body,
        owner_env,
        &type_bindings,
        &mut active_locals,
        &mut required,
    );
    required
}

fn collect_required_import_names_for_symbol(
    symbol: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    imported_binding_names: &rustc_hash::FxHashSet<&str>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    if owner_env.type_symbols.contains_key(symbol) || type_bindings.contains_key(symbol) {
        collect_surface_eval_import_names_from_expr(
            &verter_analysis::type_expr::TypeExpr::named(symbol),
            owner_env,
            type_bindings,
            active_locals,
            required,
        );
        return;
    }

    if let Some((root, _)) = symbol.split_once('.') {
        if imported_binding_names.contains(root) {
            required.insert(symbol.to_string());
            return;
        }
    }

    if imported_binding_names.contains(symbol) {
        required.insert(symbol.to_string());
    }
}

fn collect_required_slot_import_names_for_symbol(
    symbol: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    imported_binding_names: &rustc_hash::FxHashSet<&str>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    if owner_env.type_symbols.contains_key(symbol) || type_bindings.contains_key(symbol) {
        collect_slot_eval_import_names_from_expr(
            &verter_analysis::type_expr::TypeExpr::named(symbol),
            owner_env,
            type_bindings,
            active_locals,
            required,
        );
        return;
    }

    if let Some((root, _)) = symbol.split_once('.') {
        if imported_binding_names.contains(root) {
            required.insert(symbol.to_string());
            return;
        }
    }

    if imported_binding_names.contains(symbol) {
        required.insert(symbol.to_string());
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SlotImportWalkMode {
    Surface,
    KeySpace,
    Structural,
}

fn slot_import_guard(prefix: &str, mode: SlotImportWalkMode, name: &str) -> String {
    let mode = match mode {
        SlotImportWalkMode::Surface => "surface",
        SlotImportWalkMode::KeySpace => "key",
        SlotImportWalkMode::Structural => "struct",
    };
    format!("$slot-{prefix}-{mode}:{name}")
}

fn slot_member_walk_mode(mode: SlotImportWalkMode) -> SlotImportWalkMode {
    match mode {
        SlotImportWalkMode::Structural => SlotImportWalkMode::Structural,
        SlotImportWalkMode::Surface | SlotImportWalkMode::KeySpace => SlotImportWalkMode::KeySpace,
    }
}

fn collect_slot_eval_import_names_from_expr(
    expr: &verter_analysis::type_expr::TypeExpr,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    collect_slot_eval_import_names_from_expr_with_mode(
        expr,
        owner_env,
        type_bindings,
        active_locals,
        required,
        SlotImportWalkMode::Surface,
    );
}

fn collect_slot_eval_import_names_from_expr_with_mode(
    expr: &verter_analysis::type_expr::TypeExpr,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
    mode: SlotImportWalkMode,
) {
    use verter_analysis::type_expr::{LiteralValue, ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::Unknown { .. }
        | TypeExpr::TypeOf(_) => {}
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for ty in types {
                collect_slot_eval_import_names_from_expr_with_mode(
                    ty,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
            }
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => collect_slot_eval_import_names_from_expr_with_mode(
            element,
            owner_env,
            type_bindings,
            active_locals,
            required,
            mode,
        ),
        TypeExpr::KeyOf(element) => collect_slot_eval_import_names_from_expr_with_mode(
            element,
            owner_env,
            type_bindings,
            active_locals,
            required,
            SlotImportWalkMode::KeySpace,
        ),
        TypeExpr::Tuple { elements, .. } => {
            for element in elements {
                collect_slot_eval_import_names_from_expr_with_mode(
                    &element.ty,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
            }
        }
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match (mode, member) {
                    (
                        SlotImportWalkMode::Surface | SlotImportWalkMode::KeySpace,
                        ObjectMember::IndexSignature(idx),
                    ) => collect_slot_eval_import_names_from_expr_with_mode(
                        &idx.key_type,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                        SlotImportWalkMode::KeySpace,
                    ),
                    (SlotImportWalkMode::Structural, ObjectMember::Property(prop)) => {
                        collect_slot_eval_import_names_from_expr_with_mode(
                            &prop.ty,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                            SlotImportWalkMode::Structural,
                        );
                    }
                    (SlotImportWalkMode::Structural, ObjectMember::IndexSignature(idx)) => {
                        collect_slot_eval_import_names_from_expr_with_mode(
                            &idx.key_type,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                            SlotImportWalkMode::Structural,
                        );
                        collect_slot_eval_import_names_from_expr_with_mode(
                            &idx.value_type,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                            SlotImportWalkMode::Structural,
                        );
                    }
                    (
                        SlotImportWalkMode::Structural,
                        ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func),
                    ) => collect_slot_eval_import_names_from_function_structural(
                        func,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                    ),
                    (SlotImportWalkMode::Structural, ObjectMember::Method(method)) => {
                        collect_slot_eval_import_names_from_function_structural(
                            &method.function,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        );
                    }
                    _ => {}
                }
            }
        }
        TypeExpr::Function(func) => {
            if mode == SlotImportWalkMode::Structural {
                collect_slot_eval_import_names_from_function_structural(
                    func,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if let Some(bound) = type_bindings.get(name) {
                let binding_guard = slot_import_guard("type", mode, name);
                if !active_locals.insert(binding_guard.clone()) {
                    return;
                }
                collect_slot_eval_import_names_from_expr_with_mode(
                    bound,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
                active_locals.remove(&binding_guard);
                return;
            }

            if let Some(decl) = owner_env.type_symbols.get(name) {
                let decl_guard = slot_import_guard("decl", mode, name);
                if !active_locals.insert(decl_guard.clone()) {
                    return;
                }

                let mut local_bindings = type_bindings.clone();
                for (index, param) in decl.type_parameters.iter().enumerate() {
                    let arg = type_arguments
                        .get(index)
                        .cloned()
                        .or_else(|| param.default.as_ref().map(|default| (**default).clone()));
                    if let Some(arg) = arg {
                        local_bindings.insert(param.name.clone(), arg);
                    }
                }

                collect_slot_eval_import_names_from_expr_with_mode(
                    &decl.body,
                    owner_env,
                    &local_bindings,
                    active_locals,
                    required,
                    mode,
                );
                active_locals.remove(&decl_guard);
                return;
            }

            required.insert(name.clone());
            if should_recurse_surface_type_arguments(name) {
                for arg in type_arguments {
                    collect_slot_eval_import_names_from_expr_with_mode(
                        arg,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                        mode,
                    );
                }
            }
        }
        TypeExpr::IndexedAccess { object, index } => {
            let member_mode = slot_member_walk_mode(mode);
            if let TypeExpr::Literal(LiteralValue::String(key)) = index.as_ref() {
                collect_slot_eval_import_names_for_member(
                    object,
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    member_mode,
                );
            } else {
                collect_slot_eval_import_names_from_expr_with_mode(
                    object,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    member_mode,
                );
                collect_slot_eval_import_names_from_expr_with_mode(
                    index,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    member_mode,
                );
            }
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            collect_slot_eval_import_names_from_expr_with_mode(
                check,
                owner_env,
                type_bindings,
                active_locals,
                required,
                SlotImportWalkMode::Structural,
            );
            collect_slot_eval_import_names_from_expr_with_mode(
                extends,
                owner_env,
                type_bindings,
                active_locals,
                required,
                SlotImportWalkMode::Structural,
            );
            collect_slot_eval_import_names_from_expr_with_mode(
                true_type,
                owner_env,
                type_bindings,
                active_locals,
                required,
                mode,
            );
            collect_slot_eval_import_names_from_expr_with_mode(
                false_type,
                owner_env,
                type_bindings,
                active_locals,
                required,
                mode,
            );
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            if mode == SlotImportWalkMode::Structural {
                collect_slot_eval_import_names_from_expr_with_mode(
                    source,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    SlotImportWalkMode::Structural,
                );
                collect_slot_eval_import_names_from_expr_with_mode(
                    value,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    SlotImportWalkMode::Structural,
                );
                if let Some(name_type) = name_type.as_deref() {
                    collect_slot_eval_import_names_from_expr_with_mode(
                        name_type,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                        SlotImportWalkMode::Structural,
                    );
                }
            } else {
                collect_slot_eval_import_names_from_expr_with_mode(
                    source,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    SlotImportWalkMode::KeySpace,
                );
                if let Some(name_type) = name_type.as_deref() {
                    collect_slot_eval_import_names_from_expr_with_mode(
                        name_type,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                        SlotImportWalkMode::KeySpace,
                    );
                }
            }
        }
        TypeExpr::TemplateLiteral { expressions, .. } => {
            let nested_mode = if mode == SlotImportWalkMode::Structural {
                SlotImportWalkMode::Structural
            } else {
                SlotImportWalkMode::KeySpace
            };
            for expr in expressions {
                collect_slot_eval_import_names_from_expr_with_mode(
                    expr,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    nested_mode,
                );
            }
        }
    }
}

fn collect_slot_eval_import_names_from_function_structural(
    func: &verter_analysis::type_expr::FunctionExpr,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    let mut local_bindings = type_bindings.clone();
    for param in &func.type_parameters {
        local_bindings.insert(
            param.name.clone(),
            verter_analysis::type_expr::TypeExpr::named(param.name.clone()),
        );
        if let Some(constraint) = param.constraint.as_deref() {
            collect_slot_eval_import_names_from_expr_with_mode(
                constraint,
                owner_env,
                &local_bindings,
                active_locals,
                required,
                SlotImportWalkMode::Structural,
            );
        }
        if let Some(default) = param.default.as_deref() {
            collect_slot_eval_import_names_from_expr_with_mode(
                default,
                owner_env,
                &local_bindings,
                active_locals,
                required,
                SlotImportWalkMode::Structural,
            );
        }
    }

    for param in &func.parameters {
        collect_slot_eval_import_names_from_expr_with_mode(
            &param.ty,
            owner_env,
            &local_bindings,
            active_locals,
            required,
            SlotImportWalkMode::Structural,
        );
    }
    if let Some(return_type) = func.return_type.as_deref() {
        collect_slot_eval_import_names_from_expr_with_mode(
            return_type,
            owner_env,
            &local_bindings,
            active_locals,
            required,
            SlotImportWalkMode::Structural,
        );
    }
}

fn collect_slot_eval_import_names_for_member(
    object: &verter_analysis::type_expr::TypeExpr,
    key: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
    mode: SlotImportWalkMode,
) {
    use verter_analysis::type_expr::{LiteralValue, ObjectMember, TypeExpr};

    match object {
        TypeExpr::Object(obj) => {
            if let Some(member) = obj.properties.iter().find(|member| match member {
                ObjectMember::Property(prop) => prop.name == key,
                ObjectMember::Method(method) => method.name == key,
                _ => false,
            }) {
                match member {
                    ObjectMember::Property(prop) => {
                        collect_slot_eval_import_names_from_expr_with_mode(
                            &prop.ty,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                            mode,
                        );
                    }
                    ObjectMember::Method(method) if mode == SlotImportWalkMode::Structural => {
                        collect_slot_eval_import_names_from_function_structural(
                            &method.function,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        );
                    }
                    _ => {}
                }
            }
        }
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if let Some(bound) = type_bindings.get(name) {
                let binding_guard = slot_import_guard("type", mode, name);
                if !active_locals.insert(binding_guard.clone()) {
                    return;
                }
                collect_slot_eval_import_names_for_member(
                    bound,
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
                active_locals.remove(&binding_guard);
                return;
            }

            if let Some(decl) = owner_env.type_symbols.get(name) {
                let decl_guard = slot_import_guard("decl", mode, name);
                if !active_locals.insert(decl_guard.clone()) {
                    return;
                }

                let mut local_bindings = type_bindings.clone();
                for (index, param) in decl.type_parameters.iter().enumerate() {
                    let arg = type_arguments
                        .get(index)
                        .cloned()
                        .or_else(|| param.default.as_ref().map(|default| (**default).clone()));
                    if let Some(arg) = arg {
                        local_bindings.insert(param.name.clone(), arg);
                    }
                }

                collect_slot_eval_import_names_for_member(
                    &decl.body,
                    key,
                    owner_env,
                    &local_bindings,
                    active_locals,
                    required,
                    mode,
                );
                active_locals.remove(&decl_guard);
                return;
            }

            required.insert(name.clone());
            collect_slot_eval_import_names_for_builtin_member(
                name,
                type_arguments,
                key,
                owner_env,
                type_bindings,
                active_locals,
                required,
                mode,
            );
        }
        TypeExpr::Parenthesized(inner) => collect_slot_eval_import_names_for_member(
            inner,
            key,
            owner_env,
            type_bindings,
            active_locals,
            required,
            mode,
        ),
        TypeExpr::Intersection(types) | TypeExpr::Union(types) => {
            for ty in types {
                collect_slot_eval_import_names_for_member(
                    ty,
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
            }
        }
        TypeExpr::IndexedAccess { object, index } => {
            if let TypeExpr::Literal(LiteralValue::String(inner_key)) = index.as_ref() {
                collect_slot_eval_import_names_for_member(
                    object,
                    inner_key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
            } else {
                collect_slot_eval_import_names_from_expr_with_mode(
                    object,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
                collect_slot_eval_import_names_from_expr_with_mode(
                    index,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
            }
        }
        _ => collect_slot_eval_import_names_from_expr_with_mode(
            object,
            owner_env,
            type_bindings,
            active_locals,
            required,
            mode,
        ),
    }
}

fn collect_slot_eval_import_names_for_builtin_member(
    name: &str,
    type_arguments: &[verter_analysis::type_expr::TypeExpr],
    key: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
    mode: SlotImportWalkMode,
) {
    match name {
        "Partial" | "Required" | "Readonly" if type_arguments.len() == 1 => {
            collect_slot_eval_import_names_for_member(
                &type_arguments[0],
                key,
                owner_env,
                type_bindings,
                active_locals,
                required,
                mode,
            );
        }
        "Pick" if type_arguments.len() == 2 => {
            let keys = collect_string_literal_keys(&type_arguments[1]);
            if keys.contains(key) {
                collect_slot_eval_import_names_for_member(
                    &type_arguments[0],
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
            }
        }
        "Omit" if type_arguments.len() == 2 => {
            let keys = collect_string_literal_keys(&type_arguments[1]);
            if !keys.contains(key) {
                collect_slot_eval_import_names_for_member(
                    &type_arguments[0],
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
            }
        }
        _ => {}
    }
}

fn collect_surface_eval_import_names_from_expr(
    expr: &verter_analysis::type_expr::TypeExpr,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    use verter_analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::Unknown { .. } => {}
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for ty in types {
                collect_surface_eval_import_names_from_expr(
                    ty,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => collect_surface_eval_import_names_from_expr(
            element,
            owner_env,
            type_bindings,
            active_locals,
            required,
        ),
        TypeExpr::Tuple { elements, .. } => {
            for element in elements {
                collect_surface_eval_import_names_from_expr(
                    &element.ty,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => collect_surface_eval_import_names_from_expr(
                        &prop.ty,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                    ),
                    ObjectMember::IndexSignature(idx) => {
                        collect_surface_eval_import_names_from_expr(
                            &idx.key_type,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        );
                        collect_surface_eval_import_names_from_expr(
                            &idx.value_type,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        );
                    }
                    ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                        collect_surface_eval_import_names_from_function(
                            func,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        );
                    }
                    ObjectMember::Method(method) => {
                        collect_surface_eval_import_names_from_function(
                            &method.function,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        );
                    }
                }
            }
        }
        TypeExpr::Function(func) => collect_surface_eval_import_names_from_function(
            func,
            owner_env,
            type_bindings,
            active_locals,
            required,
        ),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if let Some(bound) = type_bindings.get(name) {
                let binding_guard = format!("$type:{name}");
                if !active_locals.insert(binding_guard.clone()) {
                    return;
                }
                collect_surface_eval_import_names_from_expr(
                    bound,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
                active_locals.remove(&binding_guard);
                return;
            }

            if let Some(decl) = owner_env.type_symbols.get(name) {
                if !active_locals.insert(name.clone()) {
                    return;
                }

                let mut local_bindings = type_bindings.clone();
                for (index, param) in decl.type_parameters.iter().enumerate() {
                    let arg = type_arguments
                        .get(index)
                        .cloned()
                        .or_else(|| param.default.as_ref().map(|default| (**default).clone()));
                    if let Some(arg) = arg {
                        local_bindings.insert(param.name.clone(), arg);
                    }
                }

                collect_surface_eval_import_names_from_expr(
                    &decl.body,
                    owner_env,
                    &local_bindings,
                    active_locals,
                    required,
                );
                active_locals.remove(name);
                return;
            }

            required.insert(name.clone());
            if should_recurse_surface_type_arguments(name) {
                for arg in type_arguments {
                    collect_surface_eval_import_names_from_expr(
                        arg,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                    );
                }
            }
        }
        TypeExpr::TypeOf(_) => {}
        TypeExpr::IndexedAccess { object, index } => {
            if let TypeExpr::Literal(verter_analysis::type_expr::LiteralValue::String(key)) =
                index.as_ref()
            {
                collect_surface_eval_import_names_for_member(
                    object,
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            } else {
                collect_surface_eval_import_names_from_expr(
                    object,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
                collect_surface_eval_import_names_from_expr(
                    index,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            for ty in [check, extends, true_type, false_type] {
                collect_surface_eval_import_names_from_expr(
                    ty,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            collect_surface_eval_import_names_from_expr(
                source,
                owner_env,
                type_bindings,
                active_locals,
                required,
            );
            collect_surface_eval_import_names_from_expr(
                value,
                owner_env,
                type_bindings,
                active_locals,
                required,
            );
            if let Some(name_type) = name_type.as_deref() {
                collect_surface_eval_import_names_from_expr(
                    name_type,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::TemplateLiteral { expressions, .. } => {
            for expr in expressions {
                collect_surface_eval_import_names_from_expr(
                    expr,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
    }
}

fn collect_surface_eval_import_names_for_member(
    object: &verter_analysis::type_expr::TypeExpr,
    key: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    use verter_analysis::type_expr::{LiteralValue, ObjectMember, TypeExpr};

    match object {
        TypeExpr::Object(obj) => {
            if let Some(member) = obj.properties.iter().find(|member| match member {
                ObjectMember::Property(prop) => prop.name == key,
                ObjectMember::Method(method) => method.name == key,
                _ => false,
            }) {
                match member {
                    ObjectMember::Property(prop) => collect_surface_eval_import_names_from_expr(
                        &prop.ty,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                    ),
                    ObjectMember::Method(method) => {
                        collect_surface_eval_import_names_from_function(
                            &method.function,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        )
                    }
                    _ => {}
                }
            }
        }
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if let Some(bound) = type_bindings.get(name) {
                let binding_guard = format!("$type:{name}");
                if !active_locals.insert(binding_guard.clone()) {
                    return;
                }
                collect_surface_eval_import_names_for_member(
                    bound,
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
                active_locals.remove(&binding_guard);
                return;
            }

            if let Some(decl) = owner_env.type_symbols.get(name) {
                if !active_locals.insert(name.clone()) {
                    return;
                }

                let mut local_bindings = type_bindings.clone();
                for (index, param) in decl.type_parameters.iter().enumerate() {
                    let arg = type_arguments
                        .get(index)
                        .cloned()
                        .or_else(|| param.default.as_ref().map(|default| (**default).clone()));
                    if let Some(arg) = arg {
                        local_bindings.insert(param.name.clone(), arg);
                    }
                }

                collect_surface_eval_import_names_for_member(
                    &decl.body,
                    key,
                    owner_env,
                    &local_bindings,
                    active_locals,
                    required,
                );
                active_locals.remove(name);
                return;
            }

            required.insert(name.clone());
            collect_surface_eval_import_names_for_builtin_member(
                name,
                type_arguments,
                key,
                owner_env,
                type_bindings,
                active_locals,
                required,
            );
        }
        TypeExpr::Parenthesized(inner) => collect_surface_eval_import_names_for_member(
            inner,
            key,
            owner_env,
            type_bindings,
            active_locals,
            required,
        ),
        TypeExpr::Intersection(types) | TypeExpr::Union(types) => {
            for ty in types {
                collect_surface_eval_import_names_for_member(
                    ty,
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::IndexedAccess { object, index } => {
            if let TypeExpr::Literal(LiteralValue::String(inner_key)) = index.as_ref() {
                collect_surface_eval_import_names_for_member(
                    object,
                    inner_key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            } else {
                collect_surface_eval_import_names_from_expr(
                    object,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        _ => collect_surface_eval_import_names_from_expr(
            object,
            owner_env,
            type_bindings,
            active_locals,
            required,
        ),
    }
}

fn collect_surface_eval_import_names_for_builtin_member(
    name: &str,
    type_arguments: &[verter_analysis::type_expr::TypeExpr],
    key: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    match name {
        "Partial" | "Required" | "Readonly" if type_arguments.len() == 1 => {
            collect_surface_eval_import_names_for_member(
                &type_arguments[0],
                key,
                owner_env,
                type_bindings,
                active_locals,
                required,
            );
        }
        "Pick" if type_arguments.len() == 2 => {
            let keys = collect_string_literal_keys(&type_arguments[1]);
            if keys.contains(key) {
                collect_surface_eval_import_names_for_member(
                    &type_arguments[0],
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        "Omit" if type_arguments.len() == 2 => {
            let keys = collect_string_literal_keys(&type_arguments[1]);
            if !keys.contains(key) {
                collect_surface_eval_import_names_for_member(
                    &type_arguments[0],
                    key,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        _ => {}
    }
}

fn collect_string_literal_keys(
    expr: &verter_analysis::type_expr::TypeExpr,
) -> rustc_hash::FxHashSet<String> {
    use verter_analysis::type_expr::{LiteralValue, TypeExpr};

    let mut keys = rustc_hash::FxHashSet::default();
    match expr {
        TypeExpr::Literal(LiteralValue::String(value)) => {
            keys.insert(value.clone());
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for ty in types {
                keys.extend(collect_string_literal_keys(ty));
            }
        }
        TypeExpr::Parenthesized(inner) => {
            keys.extend(collect_string_literal_keys(inner));
        }
        _ => {}
    }
    keys
}

fn collect_surface_eval_import_names_from_function(
    func: &verter_analysis::type_expr::FunctionExpr,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    for param in &func.parameters {
        collect_surface_eval_import_names_from_expr(
            &param.ty,
            owner_env,
            type_bindings,
            active_locals,
            required,
        );
    }
}

fn should_recurse_surface_type_arguments(name: &str) -> bool {
    matches!(
        name,
        "Partial"
            | "Required"
            | "Readonly"
            | "Pick"
            | "Omit"
            | "Record"
            | "Extract"
            | "Exclude"
            | "NonNullable"
            | "ReturnType"
            | "Parameters"
            | "ConstructorParameters"
            | "InstanceType"
            | "Awaited"
    )
}

/// Extract slot bindings from a type_text that encodes a slot's function signature.
///
/// Handles property signature types like `(props: { row: Item; index: number }) => any`.
/// Extract slot bindings and return type from a type_text encoding a slot function signature.
///
/// Handles both arrow-style (`(props: { row: Item }) => VNode[]`) and
/// method-style (`(props: { row: Item }): VNode[]`) signatures.
/// Returns `(bindings, return_type)`.
fn component_meta_resolved_macros(
    snapshot: &FileAnalysisSnapshot,
    resolved_macros: &[crate::meta_resolve::ResolvedMacroMeta],
) -> Vec<verter_analysis::component_meta::ResolvedMacroInput> {
    resolved_macros
        .iter()
        .filter(|resolved| {
            snapshot
                .macros
                .get(resolved.macro_index)
                .is_none_or(|mac| !raw_macro_surface_is_authoritative(mac))
        })
        .map(
            |resolved| verter_analysis::component_meta::ResolvedMacroInput {
                macro_index: resolved.macro_index,
                props: resolved.props.clone(),
                emits: resolved.emits.clone(),
                slots: resolved.slots.clone(),
            },
        )
        .collect()
}

fn raw_macro_surface_is_authoritative(mac: &verter_analysis::AnalyzedMacro) -> bool {
    match mac.kind {
        verter_analysis::AnalyzedMacroKind::DefineProps
        | verter_analysis::AnalyzedMacroKind::WithDefaults
        | verter_analysis::AnalyzedMacroKind::DefineModel => !mac.prop_fields.is_empty(),
        // Local emit parsing is often only a partial surface for type aliases
        // that intersect with imported helpers. Keep resolved emit members so
        // imported events can still merge in.
        verter_analysis::AnalyzedMacroKind::DefineEmits => false,
        verter_analysis::AnalyzedMacroKind::DefineSlots => !mac.slot_fields.is_empty(),
        verter_analysis::AnalyzedMacroKind::DefineExpose => !mac.expose_fields.is_empty(),
        verter_analysis::AnalyzedMacroKind::DefineOptions => false,
    }
}

fn component_meta_type_registry(
    resolved_type_registry: &[verter_analysis::component_meta::ResolvedTypeAnalysis],
) -> Vec<verter_analysis::component_meta::ResolvedTypeAnalysis> {
    let mut seen = rustc_hash::FxHashSet::default();
    let mut registry = Vec::new();

    for entry in resolved_type_registry {
        if seen.insert(entry.name.clone()) {
            registry.push(entry.clone());
        }
    }

    registry
}

/// Build a `ComponentMetaAnalysis` from a resolved-meta state.
/// Shared by `get_component_meta` and `get_component_meta_with_resolution`.
fn extract_component_meta_from_inputs(
    host: &VerterHost,
    canonical_or_alias: &str,
    snapshot: &FileAnalysisSnapshot,
    resolved_macros: &[verter_analysis::component_meta::ResolvedMacroInput],
    resolved_type_registry: &[verter_analysis::component_meta::ResolvedTypeAnalysis],
    evaluated_types: Option<&verter_analysis::type_expand::ExpandedComponentTypes>,
    include_fallthrough: bool,
) -> verter_analysis::component_meta::ComponentMetaAnalysis {
    let started = component_meta_debug_enabled().then(Instant::now);
    let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
    let input = verter_analysis::component_meta::ComponentMetaInput {
        macros: &snapshot.macros,
        bindings: &snapshot.bindings,
        imports: &snapshot.imports,
        template: snapshot.template.as_deref(),
        options_api: snapshot.options_api.as_ref(),
        analysis_flags: verter_analysis::types::AnalysisFlags::from_bits_truncate(
            snapshot.script_flags,
        ),
        styles: &snapshot.styles,
        vue_api_calls: &snapshot.vue_api_calls,
        store_usages: &snapshot.store_usages,
        resolved_macros,
        resolved_type_registry,
        evaluated_types,
        file_path: &canonical,
    };
    let mut meta = verter_analysis::component_meta::extract_component_meta(input);

    if include_fallthrough {
        if let Some(resolution) = host.resolve_fallthrough_surface(&canonical) {
            meta.accepted_props = resolution.accepted_props;
            meta.accepted_events = resolution.accepted_events;
            meta.accepted_surface_completeness = resolution.accepted_surface_completeness;
            meta.fallthrough_surface = resolution.fallthrough_surface;
        }
    }

    if let Some(started) = started {
        component_meta_debug(format!(
            "extract_component_meta owner={} include_fallthrough={} took {:?}",
            canonical,
            include_fallthrough,
            started.elapsed(),
        ));
    }

    meta
}

fn extract_component_meta_from_resolved(
    host: &VerterHost,
    canonical_or_alias: &str,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
    include_fallthrough: bool,
) -> verter_analysis::component_meta::ComponentMetaAnalysis {
    let resolved_macros =
        component_meta_resolved_macros(&resolved.snapshot, &resolved.resolved_macros);
    let resolved_type_registry = component_meta_type_registry(&resolved.resolved_type_registry);
    extract_component_meta_from_inputs(
        host,
        canonical_or_alias,
        &resolved.snapshot,
        &resolved_macros,
        &resolved_type_registry,
        resolved.evaluated_types.as_ref(),
        include_fallthrough,
    )
}

#[cfg(test)]
#[path = "host_manage_tests.rs"]
mod tests;

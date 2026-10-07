use super::*;
use verter_type_engine::resolver_core::request_ports::IndexedInputs;

use std::sync::Arc;
use verter_type_expr::TypeExpr;
use verter_workspace::{WorkspaceAccess, WorkspaceRead};

const LAZY_ANALYSIS_SFC: &str = r#"<template><div>{{ msg }}</div></template>
<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>
<style>
.foo { color: red; }
</style>"#;

fn make_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn strict_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn make_lazy_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::None,
        ..HostConfig::default()
    })
}

fn expected_imported_root(
    canonical_id: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    symbol_name: &str,
) -> Option<verter_session_query::type_solver::ResolvedRootIdentity> {
    Some(
        verter_session_query::type_solver::ResolvedRootIdentity::new_in_owner(
            canonical_id,
            owner,
            symbol_name,
        ),
    )
}

fn expected_imported_root_tuple(
    canonical_id: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    symbol_name: &str,
) -> (String, verter_type_expr::TopLevelOwnerId, String) {
    (canonical_id.to_string(), owner, symbol_name.to_string())
}

fn upsert_vue(host: &VerterHost, id: &str, src: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
}

fn upsert_non_sfc(host: &VerterHost, id: &str, src: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
}

fn template_class_facts_for(
    host: &VerterHost,
    canonical: &str,
) -> crate::host_manage::template_class_facts::SessionTemplateClassSemanticFacts {
    let source = host
        .scheduler
        .try_get_source(canonical)
        .expect("source snapshot");
    let data = source
        .downcast_data::<crate::host_executor::HostSourceData>()
        .expect("host source data");
    let raw = crate::parse::compile_template_data(
        &data.file_language,
        source.source.as_ref(),
        data.framework_parse.as_deref(),
        true,
        &host.provenance,
    )
    .expect("raw template data");
    let raw = raw.data;
    host.build_template_class_semantic_facts(
        canonical,
        data.parse.whole_hash,
        Arc::clone(&source.source),
        crate::host_manage::template_class_facts::TemplateClassScriptInputs {
            macros: &data.parse.script_analysis.macros,
            bindings: &data.parse.script_analysis.bindings,
        },
        &raw,
        crate::host_manage::template_class_facts::TemplateClassPublicationScope::BasePublishable,
    )
}

struct CountingWorkspace {
    inner: Arc<verter_workspace::MemoryWorkspace>,
    read_counts: parking_lot::Mutex<rustc_hash::FxHashMap<String, u64>>,
    exists_counts: parking_lot::Mutex<rustc_hash::FxHashMap<String, u64>>,
    manifest_read_counts: parking_lot::Mutex<rustc_hash::FxHashMap<String, u64>>,
    resolve_counts: parking_lot::Mutex<rustc_hash::FxHashMap<(String, String), u64>>,
}

impl CountingWorkspace {
    fn new() -> Self {
        Self {
            inner: Arc::new(verter_workspace::MemoryWorkspace::new(
                verter_workspace::MemoryOptions::default(),
            )),
            read_counts: parking_lot::Mutex::new(rustc_hash::FxHashMap::default()),
            exists_counts: parking_lot::Mutex::new(rustc_hash::FxHashMap::default()),
            manifest_read_counts: parking_lot::Mutex::new(rustc_hash::FxHashMap::default()),
            resolve_counts: parking_lot::Mutex::new(rustc_hash::FxHashMap::default()),
        }
    }

    fn inject_file(&self, path: &str, source: &str) {
        self.inner
            .inject_file(path.to_string(), Arc::<str>::from(source.to_string()));
    }

    fn remove_file(&self, path: &str) {
        self.inner.remove_file(path);
    }

    fn reset_reads(&self) {
        self.read_counts.lock().clear();
    }

    fn read_count(&self, path: &str) -> u64 {
        self.read_counts.lock().get(path).copied().unwrap_or(0)
    }

    fn reset_exists(&self) {
        self.exists_counts.lock().clear();
    }

    fn exists_count(&self, path: &str) -> u64 {
        self.exists_counts.lock().get(path).copied().unwrap_or(0)
    }

    fn reset_resolves(&self) {
        self.resolve_counts.lock().clear();
    }

    fn resolve_count(&self, importer_id: &str, specifier: &str) -> u64 {
        self.resolve_counts
            .lock()
            .get(&(importer_id.to_string(), specifier.to_string()))
            .copied()
            .unwrap_or(0)
    }
}

impl verter_workspace::WorkspaceRead for CountingWorkspace {
    fn capture_resolution_world(&self) -> Option<Arc<verter_workspace::CapturedResolutionWorld>> {
        verter_workspace::WorkspaceRead::capture_resolution_world(self.inner.as_ref())
    }

    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        *self
            .read_counts
            .lock()
            .entry(canonical_id.to_string())
            .or_default() += 1;
        self.inner.read_file(canonical_id)
    }

    fn take_last_read_file_trace_detail(&self, canonical_id: &str) -> Option<String> {
        self.inner.take_last_read_file_trace_detail(canonical_id)
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        *self
            .exists_counts
            .lock()
            .entry(canonical_id.to_string())
            .or_default() += 1;
        self.inner.file_exists(canonical_id)
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        self.inner.realpath(canonical_id)
    }

    fn read_package_manifest(
        &self,
        canonical_id: &str,
    ) -> Option<verter_workspace::PackageManifest> {
        *self
            .manifest_read_counts
            .lock()
            .entry(canonical_id.to_string())
            .or_default() += 1;
        self.inner.read_package_manifest(canonical_id)
    }

    fn classify_file(&self, canonical_id: &str) -> verter_language::FileLanguage {
        self.inner.classify_file(canonical_id)
    }

    fn resolve_import(
        &self,
        importer_id: &str,
        specifier: &str,
        ctx: verter_session_query::resolution::ResolutionContext,
    ) -> Option<verter_session_query::resolution::ResolveResult> {
        *self
            .resolve_counts
            .lock()
            .entry((importer_id.to_string(), specifier.to_string()))
            .or_default() += 1;
        self.inner.resolve_import(importer_id, specifier, ctx)
    }

    fn resolve_import_outcome(
        &self,
        importer_id: &str,
        specifier: &str,
        ctx: verter_session_query::resolution::ResolutionContext,
    ) -> verter_workspace::ResolutionOutcome {
        *self
            .resolve_counts
            .lock()
            .entry((importer_id.to_string(), specifier.to_string()))
            .or_default() += 1;
        self.inner
            .resolve_import_outcome(importer_id, specifier, ctx)
    }

    fn content_generation(&self) -> u64 {
        self.inner.content_generation()
    }

    /// The counting decorator is TRANSPARENT over its inner workspace: package
    /// classification must delegate like every other method, or a
    /// `CountingWorkspace`-backed host silently classifies nothing as
    /// package-backed and every exact package-route proof fails closed for a
    /// harness reason rather than a semantic one.
    fn is_package_backed(&self, canonical_id: &str) -> bool {
        self.inner.is_package_backed(canonical_id)
    }

    fn resolution_fact_generation(&self) -> u64 {
        self.inner.resolution_fact_generation()
    }

    fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.inner.reverse_deps_for(canonical_id)
    }

    fn forward_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.inner.forward_deps_for(canonical_id)
    }

    fn dependency_snapshot(
        &self,
        canonical_id: &str,
    ) -> Option<verter_workspace::DependencySnapshotView> {
        self.inner.dependency_snapshot(canonical_id)
    }

    fn read_dir(
        &self,
        dir: &str,
    ) -> Result<Vec<verter_workspace::DirEntry>, verter_workspace::VfsError> {
        self.inner.read_dir(dir)
    }

    fn walk(
        &self,
        root: &str,
        filter_dir: &dyn Fn(&str) -> bool,
        filter_file: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<String>, verter_workspace::VfsError> {
        self.inner.walk(root, filter_dir, filter_file)
    }

    fn is_dir(&self, path: &str) -> bool {
        self.inner.is_dir(path)
    }
}

impl verter_workspace::WorkspaceAccess for CountingWorkspace {
    fn record_parsed_edges(&self, canonical_id: &str, edges: &[verter_workspace::ParsedEdge]) {
        self.inner.record_parsed_edges(canonical_id, edges);
    }

    fn set_exact_resolutions(
        &self,
        canonical_id: &str,
        resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        self.inner.set_exact_resolutions(canonical_id, resolutions)
    }
    fn record_parsed_edges_with_exact_resolutions(
        &self,
        canonical_id: &str,
        edges: &[verter_workspace::ParsedEdge],
        resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        self.inner
            .record_parsed_edges_with_exact_resolutions(canonical_id, edges, resolutions)
    }

    // ── R6/R7: forwarding wrapper for new reverse-graph methods ──
    fn replace_semantic_transitive(
        &self,
        canonical_id: &str,
        deps: std::collections::BTreeSet<String>,
    ) {
        self.inner.replace_semantic_transitive(canonical_id, deps);
    }

    fn set_default_resolve_extensions(&self, host_extensions: Vec<String>) {
        self.inner.set_default_resolve_extensions(host_extensions);
    }

    fn record_ambient_dependency(&self, consumer: &str, virtual_id: &str) {
        self.inner.record_ambient_dependency(consumer, virtual_id);
    }

    fn notify_upsert(&self, canonical_id: &str, source: Arc<str>) {
        self.inner.notify_upsert(canonical_id, source);
    }

    fn notify_close(&self, canonical_id: &str) {
        self.inner.notify_close(canonical_id);
    }

    fn notify_delete(&self, canonical_id: &str) {
        self.inner.notify_delete(canonical_id);
    }

    fn configure_resolver(
        &self,
        projects: Vec<verter_session_query::resolution::IdeProjectConfig>,
    ) {
        self.inner.configure_resolver(projects);
    }

    fn write_file(&self, path: &str, content: &str) -> Result<(), verter_workspace::VfsError> {
        self.inner.write_file(path, content)
    }

    fn create_dir_all(&self, path: &str) -> Result<(), verter_workspace::VfsError> {
        self.inner.create_dir_all(path)
    }

    fn delete_file(&self, path: &str) -> Result<(), verter_workspace::VfsError> {
        self.inner.delete_file(path)
    }

    fn delete_dir_all(&self, path: &str) -> Result<(), verter_workspace::VfsError> {
        self.inner.delete_dir_all(path)
    }

    fn copy_file(&self, src: &str, dst: &str) -> Result<(), verter_workspace::VfsError> {
        self.inner.copy_file(src, dst)
    }
}

fn exact_dependency(specifier: &str, resolved: &str) -> DependencyResolution {
    DependencyResolution {
        specifier: specifier.to_string(),
        resolved_canonical_id: Some(resolved.to_string()),
        possible_canonical_ids: Vec::new(),
    }
}

#[cfg(target_arch = "wasm32")]
fn mutate_lazy_analysis_source(host: &VerterHost) {
    let mut files = crate::shared::write_lock(&host.files);
    let entry = files.get_mut("App.vue").expect("App.vue should exist");
    let broken = entry
        .source
        .replace("<script", "<scripx")
        .replace("</script>", "</scripx>")
        .replace("<style", "<styla")
        .replace("</style>", "</styla>");
    entry.source = Arc::from(broken);
}

#[cfg(target_arch = "wasm32")]
fn clear_framework_parse(host: &VerterHost) {
    let mut files = crate::shared::write_lock(&host.files);
    let entry = files.get_mut("App.vue").expect("App.vue should exist");
    entry.framework_parse = None;
}

fn upsert_ts(host: &VerterHost, id: &str, src: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
}

fn compile_template(host: &VerterHost, id: &str) {
    let _ = host
        .get_virtual_file(crate::types::VirtualQuery {
            raw_id: Some(format!("{id}?vue&type=template")),
            canonical_id: None,
            node_kind: None,
            compile_profile: crate::types::CompileProfile::default(),
        })
        .unwrap();
}

// ── Export signature tests ──────────────────────────────────────

fn upsert_ts_result(host: &VerterHost, id: &str, src: &str) -> crate::HostUpdateResult {
    host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: id.to_string(),
        source: Arc::from(src),
        file_language: FileLanguage::script_ts(),
        aliases: Vec::new(),
    })
    .unwrap()
}

fn resolve_expanded_state(
    host: &VerterHost,
    canonical_or_alias: &str,
) -> crate::meta_resolve::ResolvedComponentMetaState {
    host.resolve_component_meta(
        canonical_or_alias,
        verter_type_engine::semantic_query::ProjectionMode::Expanded,
    )
    .expect("expanded resolved state should exist")
}

fn resolved_macro_by_type<'a>(
    state: &'a crate::meta_resolve::ResolvedComponentMetaState,
    type_name: &str,
) -> &'a crate::meta_resolve::ResolvedMacroMeta {
    state
        .resolved_macros
        .iter()
        .find(|meta| meta.type_name == type_name)
        .unwrap_or_else(|| panic!("missing resolved macro for {type_name}"))
}

/// Resolve the typeinfo macro-surface DTOs for a resolved macro entry. The
/// published props/emits/slots/exposed surface is owned SOLELY by the typeinfo
/// macro-surface authority (`vue_macro_dtos`), keyed on the admitted macro
/// index; `ResolvedMacroMeta` supplies only the index + kind for provenance.
fn macro_dtos_for_resolved(
    host: &VerterHost,
    owner: &str,
    resolved: &crate::meta_resolve::ResolvedMacroMeta,
) -> std::sync::Arc<crate::typeinfo::framework_surface::MacroSurfaceDtos> {
    host.vue_macro_dtos(&crate::typeinfo::types::VueMacroSurfaceRequest {
        owner_canonical: std::sync::Arc::from(owner),
        macro_index: resolved.macro_index,
        macro_kind: resolved.macro_kind,
        root_identity: host.current_or_read_whole_hash(owner).unwrap_or([0u8; 16]),
        level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
    })
    .expect("the Vue adapter is admitted")
}

/// Typeinfo macro-surface DTOs for the macro matching `type_name`.
fn macro_dtos_by_type(
    host: &VerterHost,
    owner: &str,
    state: &crate::meta_resolve::ResolvedComponentMetaState,
    type_name: &str,
) -> std::sync::Arc<crate::typeinfo::framework_surface::MacroSurfaceDtos> {
    macro_dtos_for_resolved(host, owner, resolved_macro_by_type(state, type_name))
}

/// Typeinfo macro-surface DTO bundles for every resolved macro of `kind`
/// (deduped by macro index, mirroring the production producer).
fn dtos_for_kind(
    host: &VerterHost,
    owner: &str,
    state: &crate::meta_resolve::ResolvedComponentMetaState,
    kind: verter_session_query::analysis::types::AnalyzedMacroKind,
) -> Vec<std::sync::Arc<crate::typeinfo::framework_surface::MacroSurfaceDtos>> {
    let mut seen = rustc_hash::FxHashSet::default();
    state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == kind)
        .filter(|m| seen.insert(m.macro_index))
        .map(|m| macro_dtos_for_resolved(host, owner, m))
        .collect()
}

/// Aggregate prop/emit/slot names for every resolved macro of `kind` (deduped
/// by macro index, mirroring the production producer).
fn names_for_kind(
    host: &VerterHost,
    owner: &str,
    state: &crate::meta_resolve::ResolvedComponentMetaState,
    kind: verter_session_query::analysis::types::AnalyzedMacroKind,
    pick: fn(&crate::typeinfo::framework_surface::MacroSurfaceDtos) -> Vec<String>,
) -> Vec<String> {
    let mut seen = rustc_hash::FxHashSet::default();
    state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == kind)
        .filter(|m| seen.insert(m.macro_index))
        .flat_map(|m| pick(&macro_dtos_for_resolved(host, owner, m)))
        .collect()
}

fn hm_prop_names(
    host: &VerterHost,
    owner: &str,
    state: &crate::meta_resolve::ResolvedComponentMetaState,
) -> Vec<String> {
    names_for_kind(
        host,
        owner,
        state,
        verter_session_query::analysis::types::AnalyzedMacroKind::DefineProps,
        |d| {
            d.prop_fields()
                .iter()
                .map(|p| p.analysis.name.clone())
                .collect()
        },
    )
}

fn hm_slot_names(
    host: &VerterHost,
    owner: &str,
    state: &crate::meta_resolve::ResolvedComponentMetaState,
) -> Vec<String> {
    names_for_kind(
        host,
        owner,
        state,
        verter_session_query::analysis::types::AnalyzedMacroKind::DefineSlots,
        |d| d.slot_fields().iter().map(|s| s.name.clone()).collect(),
    )
}

fn assert_exact_value_reference_arg(
    args: &Arc<[verter_type_expr::facts::AuthoredReferenceArgLocator]>,
    canonical: &str,
    symbol: &str,
) {
    let [verter_type_expr::facts::AuthoredReferenceArgLocator::Value(locator)] = args.as_ref()
    else {
        panic!("expected one exact value-annotation argument locator for {symbol}");
    };
    assert_eq!(locator.anchor.canonical_id.as_ref(), canonical);
    assert_eq!(
        locator.anchor.owner,
        verter_type_expr::TopLevelOwnerId::instance(0)
    );
    assert_eq!(locator.anchor.symbol.as_ref(), symbol);
    assert_eq!(
        locator.anchor.space,
        verter_type_expr::locators::LocatorSymbolSpace::Value
    );
    assert!(locator.path.is_empty());
    assert_eq!(locator.arg_index, 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// Value-signature return wrapper role (T-A6)
//
// The demand boundary for the authored return-reference head. Every case below
// goes through the SAME shared route/demand machinery the template-class facts
// use — there is no return-specific classifier, and no `&str` is an input to any
// role decision.
// ═══════════════════════════════════════════════════════════════════════════

/// The vue package surface every return-wrapper fixture routes to.
///
/// Every wrapper is declared as a NON-FORWARDING declaration. That is a
/// deliberate fixture property, not a convenience: the shared authored-route
/// walk resolves THROUGH a transparent alias to its terminal, so a package
/// wrapper authored as `type Reactive<T> = Other<T>` routes past its own name
/// and lands on `Other` — outside the closed vocabulary. That boundary is a
/// property of the shared route walk (identical for the template-class path) and
/// is asserted explicitly by
/// `return_wrapper_role_fails_closed_for_a_transparent_alias_wrapper` below
/// rather than hidden by the fixture.
const RETURN_WRAPPER_VUE_DTS: &str = r#"
export interface Ref<T> { value: T }
export interface ShallowRef<T> { value: T }
export interface ComputedRef<T> { readonly value: T }
export interface WritableComputedRef<T> { value: T }
export interface ModelRef<T> { value: T }
export interface Reactive<T> { __reactive: T }
export interface ShallowReactive<T> { __shallowReactive: T }
"#;

/// Demand one exported value's whole-return wrapper role on the SAME
/// request-bound rail the production consumer uses: an installed
/// `RequestContext` (so the projection fuse is armed and
/// `ProjectSemanticDispatch::new` sees `is_request_bound() == true` rather than
/// counting a bare construction) over a cold-seed `HostResolverContext`.
fn return_wrapper_role_for(
    host: &VerterHost,
    canonical: &str,
    symbol: &str,
) -> (
    verter_type_expr::ReactiveWrapperRole,
    Option<verter_type_expr::ReactiveWrapperImportProvenance>,
) {
    let _ctx_guard =
        host.install_request_budget_context_if_none(host.next_request_id(), canonical, false);
    let view = crate::session_view::HostViewRef::new(host);
    let fixed = host.capture_batch_fixed_view(&view);
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx =
        crate::resolver_core::HostResolverContext::from_cold_seed(host, fixed.cold_seed(), overlay);
    let dispatch =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(&host_ctx);
    verter_type_engine::project_semantic_dispatch::reactive_wrapper::wrapper_role_for_sole_value_signature_return(
        &dispatch,
        canonical,
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        symbol,
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// A6-06 — the component-meta consumer of the whole-return wrapper role
// ═══════════════════════════════════════════════════════════════════════════

/// A BODILESS composable declaration. This is the capability, not a convenience:
/// the value-space authority (`build_composable_info` → `detect_composable_return
/// _shape`) is gated on a function BODY, so a declaration with none can never be
/// classified by it and today publishes the undecided `MaybeRef`. Only the
/// AUTHORED RETURN TYPE can answer it, and only the shared type resolver can
/// prove that `Ref` is `vue`'s.
const A6_BODILESS_COMPOSABLE_DTS: &str = r#"import type { Ref, ComputedRef } from 'vue'
export declare function useCounter(): Ref<number>
export declare function useTotal(): ComputedRef<number>
export declare function usePlain(): number
export declare function useOverloaded(): Ref<number>
export declare function useOverloaded(flag: boolean): Ref<string>
"#;

/// Wire the owner SFC's `./composables` edge plus the composable file's own
/// `vue` edge — the two-hop route every A6-06 fixture resolves through.
fn a6_wire_composable_host(host: &VerterHost, owner: &str, composables: &str, dts: &str) {
    upsert_non_sfc(
        host,
        "/workspace/node_modules/vue/index.d.ts",
        RETURN_WRAPPER_VUE_DTS,
    );
    upsert_non_sfc(host, composables, dts);
    host.set_import_dependencies(
        composables,
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );
    host.set_import_dependencies(owner, vec![exact_dependency("./composables", composables)]);
}

fn a6_binding<'a>(
    meta: &'a verter_session_query::analysis::component_meta::ComponentMetaAnalysis,
    name: &str,
) -> &'a verter_session_query::analysis::component_meta::BindingAnalysis {
    meta.bindings
        .iter()
        .find(|binding| binding.name == name)
        .unwrap_or_else(|| {
            panic!(
                "binding `{name}` must be published; got {:?}",
                meta.bindings
                    .iter()
                    .map(|binding| binding.name.as_str())
                    .collect::<Vec<_>>()
            )
        })
}

/// The file's authoritative current `whole_hash`, for probing whether the
/// content-addressed artifact store already holds its `IndexedReady`.
fn current_whole_hash(
    host: &VerterHost,
    canonical: &str,
) -> verter_session_query::analysis::types::Hash16 {
    let snapshot = host
        .scheduler
        .try_get_source(canonical)
        .expect("source snapshot");
    snapshot
        .downcast_data::<crate::host_executor::HostSourceData>()
        .expect("host source data")
        .parse
        .whole_hash
}

/// The persisted raw-template entry's `(template, class-fact signature)` pair,
/// or `None` when the slot declined.
fn persisted_raw_template(
    host: &VerterHost,
    canonical: &str,
) -> Option<(
    Arc<verter_session_query::analysis::template::TemplateAnalysisSnapshot>,
    verter_session_query::facts::fact_cache::ReadSetSignature,
)> {
    host.derived_raw_cache().get(canonical).and_then(|derived| {
        derived.raw_template_analysis().map(|entry| {
            (
                Arc::clone(&entry.template),
                entry.template_class_signature.clone(),
            )
        })
    })
}

/// Run the lazy raw-template lane once on a fresh host, optionally pre-warming
/// the file's `IndexedReady` artifact first so the lane takes the BASE fork
/// instead of the cold-seed fork. Returns the published class domain of the
/// first element and the persisted entry's class-fact signature — `None` when
/// the slot declined.
#[allow(clippy::type_complexity)]
fn lazy_template_lane_arm(
    canonical: &str,
    owner_source: &str,
    dependency: Option<(&str, &str, &str)>,
    prewarm_indexed: bool,
) -> (
    Vec<String>,
    Option<verter_session_query::facts::fact_cache::ReadSetSignature>,
) {
    let host = make_host();
    if let Some((_, dep_path, dep_source)) = dependency {
        upsert_non_sfc(&host, dep_path, dep_source);
    }
    upsert_vue(&host, canonical, owner_source);
    if let Some((specifier, dep_path, _)) = dependency {
        host.set_import_dependencies(canonical, vec![exact_dependency(specifier, dep_path)]);
    }

    let whole_hash = current_whole_hash(&host, canonical);
    if prewarm_indexed {
        let indexed = host
            .ensure_indexed_ready(canonical)
            .expect("the pre-warm indexing read must materialise IndexedReady");
        assert_eq!(
            indexed.whole_hash, whole_hash,
            "warm-arm invariant: the pre-warmed artifact is at the file's CURRENT hash",
        );
    }
    assert_eq!(
        host.exact_current_indexed_for_test(canonical, whole_hash)
            .is_some(),
        prewarm_indexed,
        "arm invariant: the artifact store's warmth for this whole_hash must be \
         exactly what the arm asked for (prewarm_indexed = {prewarm_indexed})",
    );

    let template = host
        .raw_template_analysis_for_file(canonical)
        .expect("the lazy lane must serve its template");
    let domain = template.elements[0].dynamic_classes.clone();
    (
        domain,
        persisted_raw_template(&host, canonical).map(|(_, signature)| signature),
    )
}

/// The CROSS-FILE facts a signature recorded — every fact attributed to a
/// canonical other than `owner`. These are the rails the owner's own
/// `source_generation` stamp cannot see, so losing one is the only way a
/// different observation granularity could actually weaken invalidation.
fn cross_file_facts(
    signature: &verter_session_query::facts::fact_cache::ReadSetSignature,
    owner: &str,
) -> rustc_hash::FxHashSet<verter_session_query::facts::fact_cache::FactVersionRef> {
    signature
        .facts
        .iter()
        .filter(|fact| fact.canonical_id().is_some_and(|id| id != owner))
        .cloned()
        .collect()
}

/// The owner-rooted `FileWholeHash` facts a signature recorded — the rail every
/// entry must carry.
fn owner_whole_hash_facts(
    signature: &verter_session_query::facts::fact_cache::ReadSetSignature,
    owner: &str,
) -> rustc_hash::FxHashSet<verter_session_query::facts::fact_cache::FactVersionRef> {
    signature
        .facts
        .iter()
        .filter(|fact| {
            matches!(fact, verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == owner)
        })
        .cloned()
        .collect()
}

struct DecliningOverlayView {
    base: crate::session_view::HostView,
    canonical: String,
    hash: verter_session_query::analysis::types::Hash16,
}

impl crate::session_view::SessionView for DecliningOverlayView {
    fn source(&self, canonical: &str) -> Option<Arc<str>> {
        if canonical == self.canonical {
            return None;
        }
        crate::session_view::SessionView::source(&self.base, canonical)
    }

    fn content_hash_for(
        &self,
        canonical: &str,
    ) -> Option<verter_session_query::analysis::types::Hash16> {
        crate::session_view::SessionView::content_hash_for(&self.base, canonical)
    }

    fn overlay_content_hash_for(
        &self,
        canonical: &str,
    ) -> Option<verter_session_query::analysis::types::Hash16> {
        (canonical == self.canonical).then_some(self.hash)
    }

    fn project_identity(&self) -> verter_session_query::resolution::ProjectIdentity {
        crate::session_view::SessionView::project_identity(&self.base)
    }

    fn env_hashes(&self) -> &verter_session_query::resolution::EnvHashes {
        crate::session_view::SessionView::env_hashes(&self.base)
    }

    fn resolved_import_facts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::resolved_import_facts::ResolvedImportFacts>> {
        crate::session_view::SessionView::resolved_import_facts(&self.base, canonical)
    }

    fn overlay_canonicals(&self) -> Vec<String> {
        vec![self.canonical.clone()]
    }
}

/// One checkpoint of the in-process churn lanes: live nodes, memo entries,
/// shape entries, the per-family memo breakdown and the node slot count.
type ChurnCycleCounters = (usize, usize, usize, Vec<(&'static str, usize)>, usize);

/// Fixture for the activity-gate tests: a consumer whose props resolve
/// through `/src/types/icon.ts`, resolved once so icon.ts has live nodes.
#[cfg(not(target_arch = "wasm32"))]
fn activity_gate_fixture() -> (Arc<CountingWorkspace>, VerterHost) {
    const CONSUMER: &str = r#"<script setup lang="ts">
import type { IconProps } from './types/icon'
defineProps<IconProps>()
</script>
<template><div /></template>"#;
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/Consumer.vue", CONSUMER);
    ws.inject_file(
        "/src/types/icon.ts",
        "export interface IconProps { name: string; size: number }\n",
    );
    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(host.ensure_loaded("/src/Consumer.vue"));
    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types/icon", "/src/types/icon.ts")],
    );
    let meta = host
        .get_component_meta("/src/Consumer.vue")
        .expect("component meta");
    assert!(
        meta.props.iter().any(|prop| prop.name == "name"),
        "fixture: the consumer's props resolve through icon.ts"
    );
    (ws, host)
}

/// Ids of the live nodes scoped to `canonical`.
#[cfg(not(target_arch = "wasm32"))]
fn live_node_ids_scoped_to(
    host: &VerterHost,
    canonical: &str,
) -> Vec<verter_type_engine::semantic_query::SemanticNodeId> {
    use verter_type_engine::semantic_query::SemanticNodeId;
    let graph = host.project_type_store().semantic_graph();
    (0..graph.node_slot_count() as u64)
        .map(SemanticNodeId)
        .filter(|id| {
            graph.node_is_live(*id)
                && graph
                    .node_scope(*id)
                    .and_then(|scope| scope.canonical_file())
                    .is_some_and(|file| file.as_ref() == canonical)
        })
        .collect()
}

mod analysis;
mod caching;
mod compilation;
mod general;
mod invalidation;
mod lifecycle;
mod resolution;
mod workspace;

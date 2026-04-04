//! `impl VerterHost` â€” file management, analysis, and diagnostics methods.
//!
//! Contains [`VerterHost::remove`], [`VerterHost::get_analysis`],
//! [`VerterHost::get_diagnostics`], and [`VerterHost::set_import_dependencies`].

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use verter_semantic::analysis::type_expr::TypeExpr;

use crate::hash::compile_profile_hash;
use crate::id::canonicalize_id;
use crate::resolver_core::{
    collect_dynamic_root_candidates_from_type,
    component_meta_resolved_macros as resolver_component_meta_resolved_macros,
    component_meta_type_registry as resolver_component_meta_type_registry, fallthrough_cache_key,
    get_export_span_follow_reexports_from_graph as resolver_get_export_span_follow_reexports_from_graph,
    known_spread_keys_from_type_expr, materialize_imported_runtime_values_into_env,
    push_partial_reason, resolve_exports_from_graph as resolver_resolve_exports_from_graph,
    resolve_exports_from_graph_best_effort as resolver_resolve_exports_from_graph_best_effort,
    resolve_fallthrough_surface as resolver_resolve_fallthrough_surface,
    resolve_named_export_from_graph as resolver_resolve_named_export_from_graph,
    resolve_usage_prop_type, DynamicRootCandidate, ExportGraphFileKind, ExportGraphResolver,
    ExportSurface, FallthroughComputeHost, FallthroughRequestHost, FallthroughResolutionView,
    FallthroughResolverHost, ImportedRuntimeValueResolver, ImportedSymbolDependency, RequestSource,
    ResolvedConsumedBindings, SingleflightRole, StoreView,
};
use crate::resolver_store::HostStoreView;
use crate::shared::{read_lock, write_lock};
use crate::types::*;
use crate::VerterHost;

/// Resolve a relative import specifier to a canonical ID.
///
/// If the specifier is relative (`./` or `../`), resolves it relative to
/// the parent directory of `base_canonical_id`. For non-relative specifiers,
/// tries the host's `resolve_eval_dependency_canonical_in_view` first, then
/// falls back to returning the specifier as-is.
#[allow(dead_code)]
pub(crate) fn resolve_relative_specifier_to_canonical(
    base_canonical_id: &str,
    specifier: &str,
    host: &crate::VerterHost,
) -> String {
    // First try the host's eval dependency resolution (handles companion
    // files, .js → .d.ts, etc.)
    if let Some(resolved) = host.resolve_eval_dependency_canonical_in_view(specifier, None) {
        return resolved;
    }

    if specifier.starts_with("./") || specifier.starts_with("../") {
        // Resolve relative to the base file's directory
        let base_dir = base_canonical_id
            .rsplit_once('/')
            .map(|(dir, _)| dir)
            .unwrap_or("");
        let resolved = resolve_relative_path(base_dir, specifier);
        // Try exact path first
        if host.current_eval_state_in_view(&resolved, None).is_some() {
            return resolved;
        }
        // Strip .js/.jsx/.mjs/.cjs extension and try TS declaration companions
        let stem = resolved
            .strip_suffix(".js")
            .or_else(|| resolved.strip_suffix(".jsx"))
            .or_else(|| resolved.strip_suffix(".mjs"))
            .or_else(|| resolved.strip_suffix(".cjs"));
        if let Some(stem) = stem {
            for ext in &[".d.ts", ".d.cts", ".d.mts", ".ts", ".tsx"] {
                let candidate = format!("{stem}{ext}");
                if host.current_eval_state_in_view(&candidate, None).is_some() {
                    return candidate;
                }
            }
        }
        // Try appending extensions
        for ext in &[".ts", ".d.ts", ".tsx", ".js", ".vue"] {
            let candidate = format!("{resolved}{ext}");
            if host.current_eval_state_in_view(&candidate, None).is_some() {
                return candidate;
            }
        }
        // Try /index variants
        for ext in &["/index.ts", "/index.d.ts", "/index.js"] {
            let candidate = format!("{resolved}{ext}");
            if host.current_eval_state_in_view(&candidate, None).is_some() {
                return candidate;
            }
        }
        resolved
    } else {
        specifier.to_string()
    }
}

/// Simple relative path resolution (no filesystem access).
#[allow(dead_code)]
fn resolve_relative_path(base_dir: &str, relative: &str) -> String {
    let mut parts: Vec<&str> = if base_dir.is_empty() {
        Vec::new()
    } else {
        base_dir.split('/').collect()
    };
    for segment in relative.split('/') {
        match segment {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let resolved = parts.join("/");
    if base_dir.starts_with('/') && !resolved.starts_with('/') {
        format!("/{resolved}")
    } else {
        resolved
    }
}

/// Build a dep_edges map (import specifier → resolved canonical ID) from
/// the host's `dependency_resolutions`. Used when building prepared
/// declarations so that `name_resolution` and `external_deps` contain
/// resolved canonical IDs rather than raw import specifiers.
fn dep_edges_from_resolutions(
    resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
) -> rustc_hash::FxHashMap<String, String> {
    let mut edges = rustc_hash::FxHashMap::default();
    for (specifier, res) in resolutions {
        if let Some(target) = res.effective_target() {
            edges.insert(specifier.clone(), target.to_string());
        }
    }
    edges
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComponentMetaTraceEvent {
    Start,
    End,
    Point,
}

impl ComponentMetaTraceEvent {
    fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
            Self::Point => "point",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ComponentMetaTraceContext {
    trace_id: u64,
    span_id: u64,
}

pub(crate) struct ComponentMetaTraceLine<'a> {
    trace_id: u64,
    span_id: u64,
    parent_span_id: Option<u64>,
    depth: usize,
    name: &'a str,
    detail: &'a str,
}

thread_local! {
    static COMPONENT_META_TRACE_STACK: RefCell<Vec<ComponentMetaTraceContext>> = const { RefCell::new(Vec::new()) };
    static COMPONENT_META_TRACE_ENABLED_OVERRIDE: RefCell<Option<bool>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ParsedEvalProgramCacheKey {
    host_instance_id: u64,
    canonical_id: String,
    source_type: oxc_span::SourceType,
}

#[derive(Clone)]
struct ParsedEvalProgramCacheEntry {
    whole_hash: Hash16,
    parse_failed: bool,
    program: Rc<crate::ParsedEvalProgram>,
}

#[derive(Clone)]
struct ParsedTypeResolutionContextCacheEntry {
    whole_hash: Hash16,
    type_context: Rc<crate::ParsedTypeResolutionContext>,
}

thread_local! {
    static HOST_PARSED_EVAL_PROGRAM_CACHE: RefCell<
        rustc_hash::FxHashMap<ParsedEvalProgramCacheKey, ParsedEvalProgramCacheEntry>
    > = RefCell::new(rustc_hash::FxHashMap::default());
    static HOST_PARSED_TYPE_CONTEXT_CACHE: RefCell<
        rustc_hash::FxHashMap<ParsedEvalProgramCacheKey, ParsedTypeResolutionContextCacheEntry>
    > = RefCell::new(rustc_hash::FxHashMap::default());
}

pub(crate) fn current_component_meta_trace_cursor(
) -> Option<crate::component_meta_host::ComponentMetaTraceCursor> {
    COMPONENT_META_TRACE_STACK.with(|stack| {
        let stack = stack.borrow();
        let current = stack.last().copied()?;
        let caller_id = stack
            .len()
            .checked_sub(2)
            .and_then(|index| stack.get(index).copied())
            .map(|ctx| ctx.span_id);
        Some(crate::component_meta_host::ComponentMetaTraceCursor {
            request_id: current.trace_id,
            span_id: current.span_id,
            caller_id,
            depth: stack.len().saturating_sub(1),
        })
    })
}

fn component_meta_trace_output_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn component_meta_trace_next_span_id() -> u64 {
    static NEXT_SPAN_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_SPAN_ID.fetch_add(1, Ordering::Relaxed)
}

pub(crate) fn component_meta_trace_enabled() -> bool {
    let override_enabled = COMPONENT_META_TRACE_ENABLED_OVERRIDE.with(|value| *value.borrow());
    if let Some(enabled) = override_enabled {
        return enabled;
    }
    std::env::var_os("VERTER_COMPONENT_META_TRACE").is_some()
        || std::env::var_os("VERTER_META_TRACE").is_some()
}

#[cfg(test)]
pub(crate) fn with_component_meta_trace_enabled_for_test<T>(
    enabled: bool,
    f: impl FnOnce() -> T,
) -> T {
    COMPONENT_META_TRACE_ENABLED_OVERRIDE.with(|value| {
        let previous = value.replace(Some(enabled));
        let result = f();
        value.replace(previous);
        result
    })
}

fn component_meta_trace_output_path() -> Option<std::path::PathBuf> {
    std::env::var_os("VERTER_COMPONENT_META_TRACE_PATH")
        .or_else(|| std::env::var_os("VERTER_META_TRACE_PATH"))
        .map(std::path::PathBuf::from)
}

pub(crate) fn format_component_meta_trace_line(
    event: ComponentMetaTraceEvent,
    line: ComponentMetaTraceLine<'_>,
    duration: Option<Duration>,
) -> String {
    let parent = line
        .parent_span_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "-".to_string());
    let mut line = format!(
        "[verter-meta-trace] event={} trace={} span={} parent={} request={} subrequest={} caller={} depth={} thread={:?} name={:?} detail={:?}",
        event.as_str(),
        line.trace_id,
        line.span_id,
        parent,
        line.trace_id,
        line.span_id,
        parent,
        line.depth,
        std::thread::current().id(),
        line.name,
        line.detail,
    );
    if let Some(duration) = duration {
        line.push_str(&format!(" dur_ms={:.3}", duration.as_secs_f64() * 1000.0));
    }
    line
}

fn component_meta_trace_write_line(line: &str) {
    use std::io::Write;

    let _lock = component_meta_trace_output_lock().lock();
    if let Some(path) = component_meta_trace_output_path() {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
            return;
        }
    }

    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "{line}");
    let _ = stderr.flush();
}

struct ComponentMetaTraceGuardState {
    trace_id: u64,
    span_id: u64,
    parent_span_id: Option<u64>,
    depth: usize,
    name: &'static str,
    detail: String,
    started: Instant,
}

pub(crate) struct ComponentMetaTraceGuard {
    state: Option<ComponentMetaTraceGuardState>,
}

impl ComponentMetaTraceGuard {
    pub(crate) fn noop() -> Self {
        Self { state: None }
    }
}

impl Drop for ComponentMetaTraceGuard {
    fn drop(&mut self) {
        let Some(state) = self.state.take() else {
            return;
        };

        COMPONENT_META_TRACE_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            let popped = stack.pop();
            debug_assert_eq!(popped.map(|ctx| ctx.span_id), Some(state.span_id));
        });

        component_meta_trace_write_line(&format_component_meta_trace_line(
            ComponentMetaTraceEvent::End,
            ComponentMetaTraceLine {
                trace_id: state.trace_id,
                span_id: state.span_id,
                parent_span_id: state.parent_span_id,
                depth: state.depth,
                name: state.name,
                detail: &state.detail,
            },
            Some(state.started.elapsed()),
        ));
    }
}

pub(crate) fn component_meta_trace_scope_impl(
    name: &'static str,
    detail: impl Into<String>,
) -> ComponentMetaTraceGuard {
    if !component_meta_trace_enabled() {
        return ComponentMetaTraceGuard { state: None };
    }

    let detail = detail.into();
    let span_id = component_meta_trace_next_span_id();
    let (trace_id, parent_span_id, depth) = COMPONENT_META_TRACE_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        let parent = stack.last().copied();
        let trace_id = parent.map(|ctx| ctx.trace_id).unwrap_or(span_id);
        let depth = stack.len();
        stack.push(ComponentMetaTraceContext { trace_id, span_id });
        (trace_id, parent.map(|ctx| ctx.span_id), depth)
    });

    component_meta_trace_write_line(&format_component_meta_trace_line(
        ComponentMetaTraceEvent::Start,
        ComponentMetaTraceLine {
            trace_id,
            span_id,
            parent_span_id,
            depth,
            name,
            detail: &detail,
        },
        None,
    ));

    ComponentMetaTraceGuard {
        state: Some(ComponentMetaTraceGuardState {
            trace_id,
            span_id,
            parent_span_id,
            depth,
            name,
            detail,
            started: Instant::now(),
        }),
    }
}

pub(crate) fn component_meta_trace_event_impl(name: &'static str, detail: impl Into<String>) {
    if !component_meta_trace_enabled() {
        return;
    }

    let detail = detail.into();
    let span_id = component_meta_trace_next_span_id();
    let (trace_id, parent_span_id, depth) = COMPONENT_META_TRACE_STACK.with(|stack| {
        let stack = stack.borrow();
        let parent = stack.last().copied();
        let trace_id = parent.map(|ctx| ctx.trace_id).unwrap_or(span_id);
        (trace_id, parent.map(|ctx| ctx.span_id), stack.len())
    });

    component_meta_trace_write_line(&format_component_meta_trace_line(
        ComponentMetaTraceEvent::Point,
        ComponentMetaTraceLine {
            trace_id,
            span_id,
            parent_span_id,
            depth,
            name,
            detail: &detail,
        },
        None,
    ));
}

macro_rules! component_meta_trace_scope {
    ($name:expr, $detail:expr $(,)?) => {{
        if $crate::host_manage::component_meta_trace_enabled() {
            $crate::host_manage::component_meta_trace_scope_impl($name, $detail)
        } else {
            $crate::host_manage::ComponentMetaTraceGuard::noop()
        }
    }};
}

pub(crate) use component_meta_trace_scope;

macro_rules! component_meta_trace_event {
    ($name:expr, $detail:expr $(,)?) => {{
        if $crate::host_manage::component_meta_trace_enabled() {
            $crate::host_manage::component_meta_trace_event_impl($name, $detail);
        }
    }};
}

pub(crate) use component_meta_trace_event;

const COMPONENT_META_MAX_SYMBOLIC_STEPS: usize = 2_000;
const STORE_VIEW_STABILITY_MAX_ATTEMPTS: usize = 3;

impl FallthroughResolutionView for crate::types::FallthroughResolution {
    fn accepted_props(&self) -> &[verter_semantic::analysis::component_meta::AcceptedPropAnalysis] {
        &self.accepted_props
    }

    fn accepted_events(
        &self,
    ) -> &[verter_semantic::analysis::component_meta::AcceptedEventAnalysis] {
        &self.accepted_events
    }

    fn fallthrough_surface(
        &self,
    ) -> &verter_semantic::analysis::component_meta::FallthroughSurface {
        &self.fallthrough_surface
    }

    fn fact_versions(&self) -> &[crate::resolver_core::FactVersionRef] {
        &self.fact_versions
    }
}

impl FallthroughRequestHost for VerterHost {
    type View = HostStoreView;
    type Resolution = crate::types::FallthroughResolution;

    fn generic_root_propagation(&self) -> bool {
        self.config.generic_root_propagation
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

    fn try_get_cached_fallthrough(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
        >,
        store_view: &Self::View,
    ) -> Option<Self::Resolution> {
        let cache_key = fallthrough_cache_key(
            canonical_id,
            self.config.generic_root_propagation,
            prop_type_overrides,
        );

        if let Some(node) = self
            .resolver_runtime()
            .fallthrough
            .get_cached_node(&cache_key, store_view)
        {
            if let Some(resolution) = self.runtime_branch_union_node_to_resolution(node) {
                let resolution = Arc::new(resolution);
                if prop_type_overrides.is_none() {
                    self.mirror_cached_fallthrough_arc(canonical_id, resolution.clone());
                }
                return Some((*resolution).clone());
            }
        }

        let root_follow_key = crate::resolver_core::fallthrough_resolver::root_follow_key(
            canonical_id,
            prop_type_overrides
                .map(crate::resolver_core::hash_prop_type_overrides)
                .unwrap_or_default(),
            self.config.generic_root_propagation,
        );
        if let Some(node) = self
            .resolver_runtime()
            .fallthrough
            .get_cached_node(&root_follow_key, store_view)
        {
            if let Some(resolution) = self.runtime_root_follow_node_to_resolution(node) {
                let resolution = Arc::new(resolution);
                self.resolver_runtime().fallthrough.store_node(
                    cache_key,
                    self.build_runtime_fallthrough_node(resolution.as_ref()),
                );
                if prop_type_overrides.is_none() {
                    self.mirror_cached_fallthrough_arc(canonical_id, resolution.clone());
                }
                return Some((*resolution).clone());
            }
        }

        if prop_type_overrides.is_none() {
            #[cfg(feature = "scheduler")]
            {
                if let Some(cc) = self.compile_cache.get(canonical_id) {
                    if let Some(ref cached) = cc.cached_fallthrough {
                        if cached.generic_root_propagation == self.config.generic_root_propagation
                            && cached
                                .fact_versions
                                .iter()
                                .all(|fact| store_view.validates(fact))
                        {
                            self.mirror_cached_fallthrough_arc(
                                canonical_id,
                                cached.resolution.clone(),
                            );
                            return Some((*cached.resolution).clone());
                        }
                    }
                }
            }
        }

        None
    }

    fn compute_fallthrough_surface_uncached(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
        >,
        visiting: &mut rustc_hash::FxHashSet<String>,
        store_view: Option<&Self::View>,
    ) -> Option<Self::Resolution> {
        VerterHost::compute_fallthrough_surface_uncached(
            self,
            canonical_id,
            prop_type_overrides,
            visiting,
            store_view,
        )
    }

    fn store_fallthrough_result(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
        >,
        result: &Self::Resolution,
    ) {
        self.cache_fallthrough_result(canonical_id, prop_type_overrides, result);
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
        canonical_id: &str,
        tag: &str,
    ) -> Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember> {
        debug_assert_eq!(self.parent_canonical_id, canonical_id);
        let (project_anchor, cache_generation) =
            self.host.project_intrinsic_cache_anchor(canonical_id);
        let cache_key = crate::resolver_core::fallthrough_resolver::intrinsic_surface_key(
            &project_anchor,
            cache_generation,
            tag,
        );

        if let Some(view) = self.store_view {
            if let Some(node) = self
                .host
                .resolver_runtime()
                .fallthrough
                .get_cached_node(&cache_key, view)
            {
                if let Some(members) = self.host.runtime_intrinsic_node_to_members(node) {
                    return members;
                }
            }
        }

        let members = self
            .host
            .project_intrinsic_members_for_tag_in_view(canonical_id, tag, self.store_view)
            .unwrap_or_else(|| self.host.intrinsic_members_for_tag(tag));
        self.host.resolver_runtime().fallthrough.store_node(
            cache_key,
            self.host.build_runtime_intrinsic_surface_node(&members),
        );
        members
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
            verter_workspace::ResolveRequestKind::EsmImport,
        )
    }

    fn current_dependency_fact_versions(
        &self,
        canonical_id: &str,
    ) -> Vec<crate::resolver_core::FactVersionRef> {
        self.host.current_dependency_fact_versions_in_view(
            canonical_id,
            &std::collections::BTreeSet::new(),
            self.store_view,
        )
    }

    fn resolve_child_fallthrough(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
        >,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Option<Self::ChildResolution> {
        let cache_key = crate::resolver_core::fallthrough_resolver::child_surface_key(
            canonical_id,
            prop_type_overrides
                .map(crate::resolver_core::hash_prop_type_overrides)
                .unwrap_or_default(),
        );

        if let Some(view) = self.store_view {
            if let Some(node) = self
                .host
                .resolver_runtime()
                .fallthrough
                .get_cached_node(&cache_key, view)
            {
                if let Some(resolution) = self.host.runtime_child_node_to_resolution(node) {
                    return Some(resolution);
                }
            }
        }

        let resolution = self
            .host
            .resolve_fallthrough_surface_internal_with_overrides_in_view(
                canonical_id,
                prop_type_overrides,
                visiting,
                self.store_view,
            );

        if let (Some(_view), Some(resolution)) = (self.store_view, resolution.as_ref()) {
            self.host.resolver_runtime().fallthrough.store_node(
                cache_key,
                self.host.build_runtime_child_surface_node(resolution),
            );
        }

        resolution
    }
}

impl FallthroughComputeHost for HostFallthroughResolver<'_> {
    type Snapshot = FileAnalysisSnapshot;
    type EvalEnv = verter_semantic::analysis::type_eval::EvalEnv;

    fn resolve_root_consumption(
        &self,
        canonical_id: &str,
        branch_key: &str,
        snapshot: &Self::Snapshot,
        element_index: u32,
        base: &verter_semantic::analysis::component_meta::ConsumedRootBindings,
        has_unknown_spread: bool,
        eval_env: &mut Option<Self::EvalEnv>,
    ) -> ResolvedConsumedBindings {
        let cache_key = crate::resolver_core::fallthrough_resolver::consumed_bindings_key(
            canonical_id,
            branch_key,
        );

        if let Some(view) = self.store_view {
            if let Some(node) = self
                .host
                .resolver_runtime()
                .fallthrough
                .get_cached_node(&cache_key, view)
            {
                if let Some(resolved) = self.host.runtime_consumed_bindings_to_resolution(node) {
                    return resolved;
                }
            }
        }

        let resolved = self.host.resolve_root_consumption(
            snapshot,
            element_index,
            base,
            has_unknown_spread,
            eval_env,
        );
        self.host.resolver_runtime().fallthrough.store_node(
            cache_key,
            self.host.build_runtime_consumed_bindings_node(&resolved),
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
    ) -> Option<rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>> {
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

pub(crate) type ComputedEvaluatedTypes = crate::resolver_core::ComputedEvaluatedTypes;

struct HostRuntimeValueResolver<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a crate::resolver_store::HostStoreView>,
}

struct HostExportGraphResolver<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a crate::resolver_store::HostStoreView>,
}

impl ExportGraphResolver for HostExportGraphResolver<'_> {
    fn export_surface(&self, canonical_id: &str) -> Option<ExportSurface> {
        let (file_kind, _, export_signatures) = self
            .host
            .load_export_graph_analysis_in_view(canonical_id, self.store_view)?;
        Some(ExportSurface {
            file_kind: match file_kind {
                FileKind::VueSfc => ExportGraphFileKind::VueSfc,
                FileKind::NonSfc => ExportGraphFileKind::NonSfc,
            },
            export_signatures,
        })
    }

    fn local_export_span(
        &self,
        canonical_id: &str,
        binding_name: &str,
    ) -> Option<verter_span::Span> {
        let (file_kind, script_analysis, export_signatures) = self
            .host
            .load_export_graph_analysis_in_view(canonical_id, self.store_view)?;
        VerterHost::find_export_span(
            file_kind,
            &script_analysis,
            &export_signatures,
            binding_name,
        )
        .map(|(start, end)| verter_span::Span::new(start, end))
    }

    fn resolve_reexport_target(
        &self,
        canonical_id: &str,
        source: &str,
        sig: &verter_semantic::analysis::ExportSignature,
    ) -> Option<String> {
        let declaration_file = canonical_id.ends_with(".d.ts")
            || canonical_id.ends_with(".d.mts")
            || canonical_id.ends_with(".d.cts");
        if sig.is_type || declaration_file {
            self.host.resolve_type_dependency_canonical_in_view(
                canonical_id,
                source,
                self.store_view,
            )
        } else {
            self.host.resolve_loaded_dependency_canonical_in_view(
                canonical_id,
                source,
                verter_workspace::ResolveRequestKind::EsmImport,
                self.store_view,
            )
        }
    }
}

impl ImportedRuntimeValueResolver for HostRuntimeValueResolver<'_> {
    fn dependency_eval_env(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_eval::EvalEnv>> {
        self.host
            .base_eval_env_arc_in_view(canonical_id, self.store_view)
    }

    fn prepared_value_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
        self.host
            .prepared_value_decl_in_view(canonical_id, symbol_name, self.store_view)
    }

    fn resolve_import_canonical_id(
        &self,
        owner_canonical_id: &str,
        import: &verter_semantic::analysis::types::AnalyzedImport,
    ) -> Option<String> {
        self.host
            .dependency_resolutions_for_eval_in_view(owner_canonical_id, self.store_view)
            .and_then(|resolutions| {
                resolutions
                    .get(&import.source)
                    .and_then(DependencyResolution::effective_target)
                    .map(str::to_string)
                    .or_else(|| {
                        resolutions
                            .get(&import.source)
                            .and_then(|resolution| resolution.resolved_canonical_id.clone())
                    })
            })
            .or_else(|| {
                self.host.resolve_loaded_dependency_canonical_in_view(
                    owner_canonical_id,
                    &import.source,
                    if import.is_type_only {
                        verter_workspace::ResolveRequestKind::TypeImport
                    } else {
                        verter_workspace::ResolveRequestKind::EsmImport
                    },
                    self.store_view,
                )
            })
    }

    fn resolve_value_export_target(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> Option<(String, String)> {
        self.host
            .resolve_value_export_target_in_view(dep_canonical_id, imported_name, self.store_view)
            .map(|target| (target.canonical_id, target.name))
    }
}

fn exact_resolution_uses_type_preferred_target(
    phase: verter_workspace::ResolvePhase,
    kind: verter_workspace::ResolveRequestKind,
) -> bool {
    matches!(
        (phase, kind),
        (
            verter_workspace::ResolvePhase::CodegenBlocker,
            verter_workspace::ResolveRequestKind::TypeImport
        ) | (verter_workspace::ResolvePhase::ProviderGraph, _)
    )
}

fn is_runtime_script_target(canonical_id: &str) -> bool {
    canonical_id.ends_with(".js")
        || canonical_id.ends_with(".jsx")
        || canonical_id.ends_with(".mjs")
        || canonical_id.ends_with(".cjs")
}

fn is_type_preferred_target(canonical_id: &str) -> bool {
    canonical_id.ends_with(".d.ts")
        || canonical_id.ends_with(".d.mts")
        || canonical_id.ends_with(".d.cts")
        || canonical_id.ends_with(".ts")
        || canonical_id.ends_with(".tsx")
        || canonical_id.ends_with(".mts")
        || canonical_id.ends_with(".cts")
        || canonical_id.ends_with(".vue")
}

fn is_builtin_type_symbol(name: &str) -> bool {
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
            | "Array"
            | "ReadonlyArray"
            | "Promise"
    )
}

pub(crate) fn collect_type_expr_symbol_refs(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    refs: &mut std::collections::BTreeSet<String>,
) {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            refs.insert(name.to_string());
            for arg in type_arguments.iter() {
                collect_type_expr_symbol_refs(arg, refs);
            }
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for ty in types.iter() {
                collect_type_expr_symbol_refs(ty, refs);
            }
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => collect_type_expr_symbol_refs(element, refs),
        TypeExpr::Tuple { elements, .. } => {
            for element in elements.iter() {
                collect_type_expr_symbol_refs(&element.ty, refs);
            }
        }
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => collect_type_expr_symbol_refs(&prop.ty, refs),
                    ObjectMember::IndexSignature(sig) => {
                        collect_type_expr_symbol_refs(&sig.key_type, refs);
                        collect_type_expr_symbol_refs(&sig.value_type, refs);
                    }
                    ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                        for param in &func.parameters {
                            collect_type_expr_symbol_refs(&param.ty, refs);
                        }
                        if let Some(return_type) = &func.return_type {
                            collect_type_expr_symbol_refs(return_type, refs);
                        }
                    }
                    ObjectMember::Method(method) => {
                        for param in &method.function.parameters {
                            collect_type_expr_symbol_refs(&param.ty, refs);
                        }
                        if let Some(return_type) = &method.function.return_type {
                            collect_type_expr_symbol_refs(return_type, refs);
                        }
                    }
                }
            }
        }
        TypeExpr::Function(func) => {
            for param in &func.parameters {
                collect_type_expr_symbol_refs(&param.ty, refs);
            }
            if let Some(return_type) = &func.return_type {
                collect_type_expr_symbol_refs(return_type, refs);
            }
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_type_expr_symbol_refs(object, refs);
            collect_type_expr_symbol_refs(index, refs);
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            collect_type_expr_symbol_refs(check, refs);
            collect_type_expr_symbol_refs(extends, refs);
            collect_type_expr_symbol_refs(true_type, refs);
            collect_type_expr_symbol_refs(false_type, refs);
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            collect_type_expr_symbol_refs(source, refs);
            collect_type_expr_symbol_refs(value, refs);
            if let Some(name_type) = name_type {
                collect_type_expr_symbol_refs(name_type, refs);
            }
        }
        TypeExpr::TemplateLiteral { expressions, .. } => {
            for expr in expressions.iter() {
                collect_type_expr_symbol_refs(expr, refs);
            }
        }
        TypeExpr::RecursiveRef { type_arguments, .. } => {
            for arg in type_arguments.iter() {
                collect_type_expr_symbol_refs(arg, refs);
            }
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::Unknown { .. } => {}
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ValueDeclIdentity {
    pub(crate) canonical_id: String,
    pub(crate) name: String,
}

impl VerterHost {
    fn exact_resolution_candidates(
        resolved_canonical_id: Option<&String>,
        possible_canonical_ids: &[String],
    ) -> Vec<String> {
        let mut candidates = Vec::with_capacity(
            possible_canonical_ids.len() + usize::from(resolved_canonical_id.is_some()),
        );
        if let Some(resolved) = resolved_canonical_id {
            candidates.push(resolved.clone());
        }
        for candidate in possible_canonical_ids {
            if !candidates.iter().any(|existing| existing == candidate) {
                candidates.push(candidate.clone());
            }
        }
        candidates
    }

    fn resolve_existing_canonical_in_workspace(&self, canonical_id: &str) -> Option<String> {
        if !self.ws().file_exists(canonical_id) {
            return None;
        }
        Some(
            self.ws()
                .realpath(canonical_id)
                .map(|path| canonicalize_id(&path).into_owned())
                .unwrap_or_else(|| canonical_id.to_string()),
        )
    }

    fn resolve_declaration_companion_in_workspace(&self, canonical_id: &str) -> Option<String> {
        let normalized = canonicalize_id(canonical_id);
        let normalized = normalized.as_ref();
        let (runtime_ext, companion_exts): (&str, &[&str]) = if normalized.ends_with(".mjs") {
            (".mjs", &[".d.mts", ".d.ts"])
        } else if normalized.ends_with(".cjs") {
            (".cjs", &[".d.cts", ".d.ts"])
        } else if normalized.ends_with(".jsx") {
            (".jsx", &[".d.ts"])
        } else if normalized.ends_with(".js") {
            (".js", &[".d.ts"])
        } else {
            return None;
        };

        let stem = normalized.strip_suffix(runtime_ext)?;
        companion_exts.iter().find_map(|companion_ext| {
            self.resolve_existing_canonical_in_workspace(&format!("{stem}{companion_ext}"))
        })
    }

    fn package_dir_for_resolved_target(canonical_id: &str) -> Option<String> {
        let normalized = canonicalize_id(canonical_id);
        let normalized = normalized.as_ref();
        let marker = "/node_modules/";
        let marker_index = normalized.rfind(marker)?;
        let package_start = marker_index + marker.len();
        let package_path = &normalized[package_start..];
        let mut segments = package_path.split('/');
        let first = segments.next()?;
        let package_suffix = if first.starts_with('@') {
            format!("{first}/{}", segments.next()?)
        } else {
            first.to_string()
        };
        Some(format!("{}{package_suffix}", &normalized[..package_start]))
    }

    fn resolve_manifest_types_entry_for_target(
        &self,
        resolved_canonical_id: &str,
    ) -> Option<String> {
        let package_dir = Self::package_dir_for_resolved_target(resolved_canonical_id)?;
        let package_json_path = format!("{package_dir}/package.json");
        let manifest = self.ws().read_package_manifest(&package_json_path)?;
        let type_targets = [manifest.types.clone(), manifest.typings.clone()];
        type_targets.into_iter().flatten().find_map(|target| {
            let candidate = if target.starts_with("./") {
                format!("{package_dir}/{}", target.trim_start_matches("./"))
            } else if target.starts_with('/') {
                target
            } else {
                format!("{package_dir}/{target}")
            };
            self.resolve_existing_canonical_in_workspace(&candidate)
        })
    }

    fn derive_type_preferred_exact_target(
        &self,
        resolution: &DependencyResolution,
    ) -> Option<String> {
        let candidates = Self::exact_resolution_candidates(
            resolution.resolved_canonical_id.as_ref(),
            &resolution.possible_canonical_ids,
        );
        if let Some(candidate) = candidates
            .iter()
            .find(|candidate| is_type_preferred_target(candidate))
            .cloned()
        {
            return Some(candidate);
        }

        let resolved = resolution.resolved_canonical_id.as_deref()?;
        if let Some(companion) = self.resolve_declaration_companion_in_workspace(resolved) {
            return Some(companion);
        }
        if resolved.contains("/node_modules/") && is_runtime_script_target(resolved) {
            return self.resolve_manifest_types_entry_for_target(resolved);
        }
        (!resolved.contains("/node_modules/")).then(|| resolved.to_string())
    }

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

    pub(crate) fn sfc_script_setup_type_params(
        source: &str,
        cached_parse: Option<&verter_compiler::parser::types::ParsedSfc>,
    ) -> Vec<verter_semantic::analysis::type_expr::TypeParam> {
        let Some(setup) = cached_parse.and_then(|parsed| parsed.script_setup()) else {
            return Vec::new();
        };
        let Some(generic_span) = setup.generic else {
            return Vec::new();
        };
        let clause = source[generic_span.start as usize..generic_span.end as usize].trim();
        if clause.is_empty() {
            return Vec::new();
        }
        verter_semantic::analysis::type_eval_build::parse_type_parameter_clause(clause)
    }

    fn apply_sfc_script_setup_type_params(
        env: &mut verter_semantic::analysis::type_eval::EvalEnv,
        source: &str,
        cached_parse: Option<&verter_compiler::parser::types::ParsedSfc>,
    ) {
        for param in Self::sfc_script_setup_type_params(source, cached_parse) {
            env.type_bindings.insert(
                param.name.clone(),
                Arc::new(verter_semantic::analysis::type_expr::TypeExpr::type_parameter(param)),
            );
        }
    }

    pub(crate) fn build_eval_script_source(
        source: &str,
        cached_parse: Option<&verter_compiler::parser::types::ParsedSfc>,
    ) -> String {
        crate::host_resolve::extract_vue_script_content(source, cached_parse)
            .unwrap_or_else(|| source.to_string())
    }

    fn imported_eval_source_type(
        canonical_id: &str,
        raw_source: &str,
        cached_parse: Option<&verter_compiler::parser::types::ParsedSfc>,
    ) -> oxc_span::SourceType {
        if canonical_id.ends_with(".vue") {
            cached_parse
                .map(|parsed| crate::parse::sfc_script_source_type(parsed, raw_source))
                .unwrap_or_else(oxc_span::SourceType::ts)
        } else {
            crate::parse::non_sfc_source_type(canonical_id)
        }
    }

    pub(crate) fn read_analysis_source(&self, canonical_id: &str) -> Option<Arc<str>> {
        let _trace =
            component_meta_trace_scope!("read_analysis_source", format!("owner={canonical_id}"));
        if let Some(source) = self.get_source(canonical_id) {
            component_meta_trace_event!(
                "read_analysis_source_result",
                format!(
                    "owner={} source=host-cache bytes={}",
                    canonical_id,
                    source.len(),
                ),
            );
            return Some(source);
        }

        let workspace_generation = self.ws().content_generation();
        if let Some(cached) =
            self.clone_cached_imported_dependency(canonical_id, workspace_generation)
        {
            component_meta_trace_event!(
                "read_analysis_source_result",
                format!(
                    "owner={} source=imported-dependency-cache bytes={}",
                    canonical_id,
                    cached.raw_source.len(),
                ),
            );
            return Some(Arc::clone(&cached.raw_source));
        }

        let source = self.ws().read_file(canonical_id);
        if let Some(source) = source.as_ref() {
            let whole_hash = crate::hash::hash_16(source.as_bytes());
            let _ =
                self.cache_imported_dependency_source(canonical_id, whole_hash, Arc::clone(source));
            component_meta_trace_event!(
                "read_analysis_source_result",
                format!(
                    "owner={} source=workspace bytes={}",
                    canonical_id,
                    source.len(),
                ),
            );
        } else {
            component_meta_trace_event!(
                "read_analysis_source_result",
                format!(
                    "owner={} source=workspace bytes=0 missing=true",
                    canonical_id
                ),
            );
        }
        source
    }

    pub(crate) fn read_analysis_source_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<str>> {
        let _trace = component_meta_trace_scope!(
            "read_analysis_source_in_view",
            format!("owner={} store_view={}", canonical_id, store_view.is_some()),
        );
        let source = self.read_analysis_source(canonical_id)?;
        let whole_hash = crate::hash::hash_16(source.as_bytes());
        if !self.store_view_allows_current_whole_hash(canonical_id, whole_hash, store_view) {
            component_meta_trace_event!(
                "read_analysis_source_in_view_result",
                format!(
                    "owner={} accepted=false bytes={} whole_hash={whole_hash:?}",
                    canonical_id,
                    source.len(),
                ),
            );
            return None;
        }
        component_meta_trace_event!(
            "read_analysis_source_in_view_result",
            format!(
                "owner={} accepted=true bytes={} whole_hash={whole_hash:?}",
                canonical_id,
                source.len(),
            ),
        );
        Some(source)
    }

    fn analysis_source_exists_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> bool {
        if canonical_id.is_empty() {
            return false;
        }

        #[cfg(feature = "scheduler")]
        if let Some(state) = self.effective_file_state(canonical_id, None) {
            return self.store_view_allows_current_whole_hash(
                canonical_id,
                state.whole_hash,
                store_view,
            );
        }

        #[cfg(not(feature = "scheduler"))]
        if let Some(source) = self.get_source(canonical_id) {
            let whole_hash = crate::hash::hash_16(source.as_bytes());
            return self.store_view_allows_current_whole_hash(canonical_id, whole_hash, store_view);
        }

        if self
            .clone_current_imported_dependency_entry(canonical_id, store_view)
            .is_some()
        {
            return true;
        }

        self.ws().file_exists(canonical_id)
    }

    fn clone_cached_eval_env_arc(
        &self,
        cache_key: &str,
        whole_hash: Hash16,
    ) -> Option<Arc<verter_semantic::analysis::type_eval::EvalEnv>> {
        self.eval_env_cache
            .lock()
            .get(cache_key)
            .and_then(|(cached_hash, cached_env)| {
                (*cached_hash == whole_hash).then(|| Arc::clone(cached_env))
            })
    }

    fn clone_cached_imported_dependency(
        &self,
        canonical_id: &str,
        workspace_generation: u64,
    ) -> Option<Arc<crate::ImportedDependencyCacheEntry>> {
        self.imported_dependency_cache
            .lock()
            .get(canonical_id)
            .and_then(|entry| {
                (entry.workspace_generation == workspace_generation).then(|| Arc::clone(entry))
            })
    }

    pub(crate) fn clone_current_imported_dependency_entry(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<crate::ImportedDependencyCacheEntry>> {
        let entry =
            self.clone_cached_imported_dependency(canonical_id, self.ws().content_generation())?;
        self.store_view_allows_current_whole_hash(canonical_id, entry.whole_hash, store_view)
            .then_some(entry)
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn materialize_imported_dependency_base_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<crate::ImportedDependencyCacheEntry>> {
        let _trace = component_meta_trace_scope!(
            "materialize_imported_dependency_base_in_view",
            format!("owner={} store_view={}", canonical_id, store_view.is_some()),
        );
        if let Some(entry) = self.clone_current_imported_dependency_entry(canonical_id, store_view)
        {
            component_meta_trace_event!(
                "materialize_imported_dependency_base_in_view_result",
                format!(
                    "owner={} hit=true bytes={} has_cached_parse={} has_eval_source={} whole_hash={:?}",
                    canonical_id,
                    entry.raw_source.len(),
                    entry.cached_parse.is_some(),
                    entry.eval_source.is_some(),
                    entry.whole_hash,
                ),
            );
            return Some(entry);
        }

        let (raw_source, cached_parse, whole_hash) =
            self.current_eval_state_in_view(canonical_id, store_view)?;
        if !self.store_view_allows_current_whole_hash(canonical_id, whole_hash, store_view) {
            component_meta_trace_event!(
                "materialize_imported_dependency_base_in_view_result",
                format!(
                    "owner={} hit=false accepted=false bytes={} whole_hash={whole_hash:?}",
                    canonical_id,
                    raw_source.len(),
                ),
            );
            return None;
        }
        let eval_source = Arc::<str>::from(Self::build_eval_script_source(
            raw_source.as_ref(),
            cached_parse.as_deref(),
        ));
        let snapshot = {
            let mut snapshot = self.build_snapshot_from_source_state(
                canonical_id,
                &raw_source,
                cached_parse.as_deref(),
            );
            self.resolve_snapshot_imports_in_view(canonical_id, &mut snapshot, store_view);
            Some(Arc::new(snapshot))
        };
        let dependency_resolutions = self
            .dependency_resolutions_for_eval_in_view(canonical_id, store_view)
            .filter(|resolutions| !resolutions.is_empty())
            .unwrap_or_else(|| {
                snapshot
                    .as_ref()
                    .map(|snapshot| {
                        self.dependency_resolutions_from_snapshot_in_view(
                            canonical_id,
                            snapshot.as_ref(),
                            store_view,
                        )
                    })
                    .unwrap_or_default()
            });
        let entry = self.cache_imported_dependency_state(
            canonical_id,
            whole_hash,
            Arc::clone(&raw_source),
            cached_parse.clone(),
            snapshot,
            Some(Arc::clone(&eval_source)),
            None,
            dependency_resolutions,
        );
        component_meta_trace_event!(
            "materialize_imported_dependency_base_in_view_result",
            format!(
                "owner={} hit=false accepted=true bytes={} eval_bytes={} has_cached_parse={} whole_hash={whole_hash:?}",
                canonical_id,
                raw_source.len(),
                eval_source.len(),
                cached_parse.is_some(),
            ),
        );
        Some(entry)
    }

    fn cached_parsed_eval_program_entry(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        eval_source: &Arc<str>,
        source_type: oxc_span::SourceType,
    ) -> ParsedEvalProgramCacheEntry {
        let cache_key = ParsedEvalProgramCacheKey {
            host_instance_id: self.instance_id,
            canonical_id: canonical_id.to_string(),
            source_type,
        };
        HOST_PARSED_EVAL_PROGRAM_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            if let Some(entry) = cache.get(&cache_key) {
                if entry.whole_hash == whole_hash {
                    component_meta_trace_event!(
                        "cached_parsed_eval_program_hit",
                        format!(
                            "owner={} bytes={} whole_hash={whole_hash:?} parse_failed={}",
                            canonical_id,
                            eval_source.len(),
                            entry.parse_failed,
                        ),
                    );
                    return entry.clone();
                }
            }

            let parsed = crate::ParsedEvalProgram::parse(Arc::clone(eval_source), source_type);
            let parse_failed = parsed.is_none();
            let program =
                Rc::new(parsed.unwrap_or_else(|| crate::ParsedEvalProgram::empty(source_type)));
            let entry = ParsedEvalProgramCacheEntry {
                whole_hash,
                parse_failed,
                program,
            };
            component_meta_trace_event!(
                "cached_parsed_eval_program_store",
                format!(
                    "owner={} bytes={} whole_hash={whole_hash:?} parse_failed={}",
                    canonical_id,
                    eval_source.len(),
                    entry.parse_failed,
                ),
            );
            cache.insert(cache_key, entry.clone());
            entry
        })
    }

    fn cached_type_resolution_context_entry(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        eval_source: &Arc<str>,
        source_type: oxc_span::SourceType,
    ) -> Option<Rc<crate::ParsedTypeResolutionContext>> {
        let cache_key = ParsedEvalProgramCacheKey {
            host_instance_id: self.instance_id,
            canonical_id: canonical_id.to_string(),
            source_type,
        };
        HOST_PARSED_TYPE_CONTEXT_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            if let Some(entry) = cache.get(&cache_key) {
                if entry.whole_hash == whole_hash {
                    component_meta_trace_event!(
                        "cached_type_resolution_context_hit",
                        format!(
                            "owner={} bytes={} whole_hash={whole_hash:?}",
                            canonical_id,
                            eval_source.len(),
                        ),
                    );
                    return Some(Rc::clone(&entry.type_context));
                }
            }

            let parsed_eval_program = self.cached_parsed_eval_program_entry(
                canonical_id,
                whole_hash,
                eval_source,
                source_type,
            );
            if parsed_eval_program.parse_failed {
                return None;
            }

            let type_context = Rc::new(crate::ParsedTypeResolutionContext::new(
                Rc::clone(&parsed_eval_program.program),
                |parsed_program| {
                    let program = parsed_program.borrow_dependent();
                    let mut ctx =
                        verter_compiler::utils::oxc::vue::resolve_type::build_type_context(
                            program,
                            parsed_program.source_bytes(),
                            0,
                        );
                    ctx.set_trace_label(canonical_id.to_string());
                    ctx
                },
            ));
            component_meta_trace_event!(
                "cached_type_resolution_context_store",
                format!(
                    "owner={} bytes={} whole_hash={whole_hash:?}",
                    canonical_id,
                    eval_source.len(),
                ),
            );
            cache.insert(
                cache_key,
                ParsedTypeResolutionContextCacheEntry {
                    whole_hash,
                    type_context: Rc::clone(&type_context),
                },
            );
            Some(type_context)
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cached_parsed_eval_program_for_imported_dependency_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Rc<crate::ParsedEvalProgram>> {
        let entry =
            self.ensure_shallow_imported_dependency_state_in_view(canonical_id, store_view)?;
        let eval_source = entry
            .eval_source
            .as_ref()
            .map(Arc::clone)
            .unwrap_or_else(|| Arc::clone(&entry.raw_source));
        let source_type = Self::imported_eval_source_type(
            &entry.resolved_canonical_id,
            entry.raw_source.as_ref(),
            entry.cached_parse.as_deref(),
        );
        Some(
            self.cached_parsed_eval_program_entry(
                &entry.resolved_canonical_id,
                entry.whole_hash,
                &eval_source,
                source_type,
            )
            .program,
        )
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cached_type_resolution_context_for_imported_dependency_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Rc<crate::ParsedTypeResolutionContext>> {
        let entry =
            self.ensure_shallow_imported_dependency_state_in_view(canonical_id, store_view)?;
        let eval_source = entry
            .eval_source
            .as_ref()
            .map(Arc::clone)
            .unwrap_or_else(|| Arc::clone(&entry.raw_source));
        let source_type = Self::imported_eval_source_type(
            &entry.resolved_canonical_id,
            entry.raw_source.as_ref(),
            entry.cached_parse.as_deref(),
        );
        self.cached_type_resolution_context_entry(
            &entry.resolved_canonical_id,
            entry.whole_hash,
            &eval_source,
            source_type,
        )
    }

    pub(crate) fn clear_thread_local_parsed_eval_program_cache(&self) {
        let host_instance_id = self.instance_id;
        HOST_PARSED_EVAL_PROGRAM_CACHE.with(|cache| {
            cache
                .borrow_mut()
                .retain(|key, _| key.host_instance_id != host_instance_id);
        });
        HOST_PARSED_TYPE_CONTEXT_CACHE.with(|cache| {
            cache
                .borrow_mut()
                .retain(|key, _| key.host_instance_id != host_instance_id);
        });
    }

    pub(crate) fn ensure_shallow_imported_export_state_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<crate::ImportedDependencyCacheEntry>> {
        if let Some(entry) = self.clone_current_imported_dependency_entry(canonical_id, store_view)
        {
            if entry.script_analysis.is_some() && entry.export_signatures.is_some() {
                return Some(entry);
            }
        }

        let (raw_source, cached_parse, whole_hash) =
            self.current_eval_state_in_view(canonical_id, store_view)?;
        let source_type = Self::imported_eval_source_type(
            canonical_id,
            raw_source.as_ref(),
            cached_parse.as_deref(),
        );
        let (eval_source, block_ranges) = if let Some(parsed) = cached_parse.as_deref() {
            let (script_source, ranges) =
                crate::parse::collect_sfc_script_content(parsed, raw_source.as_ref());
            (Arc::<str>::from(script_source), Some(ranges))
        } else {
            (
                Arc::<str>::from(Self::build_eval_script_source(
                    raw_source.as_ref(),
                    cached_parse.as_deref(),
                )),
                None,
            )
        };

        let (script_analysis, export_signatures) = if eval_source.is_empty() {
            (
                verter_semantic::analysis::ScriptAnalysisSnapshot::default(),
                Vec::new(),
            )
        } else {
            let parsed_eval_program = self.cached_parsed_eval_program_entry(
                canonical_id,
                whole_hash,
                &eval_source,
                source_type,
            );
            if parsed_eval_program.parse_failed {
                (
                    verter_semantic::analysis::ScriptAnalysisSnapshot::default(),
                    Vec::new(),
                )
            } else {
                let program = parsed_eval_program.program.borrow_dependent();
                let mut snapshot = crate::parse::build_non_sfc_snapshot_from_program(
                    canonical_id,
                    eval_source.as_ref(),
                    source_type,
                    program,
                );
                if let Some(block_ranges) = block_ranges.as_deref() {
                    crate::parse::adjust_analysis_spans(
                        &mut snapshot.script_analysis,
                        block_ranges,
                    );
                    crate::parse::adjust_export_signature_spans(
                        &mut snapshot.export_signatures,
                        block_ranges,
                    );
                }
                (snapshot.script_analysis, snapshot.export_signatures)
            }
        };

        // Export-only imported state must stay structural: keep export signatures for
        // route selection, but defer all dependency route publication until a specific
        // symbol lookup asks for it.
        let dependency_resolutions = rustc_hash::FxHashMap::default();

        Some(self.cache_imported_dependency_shallow_state(
            canonical_id,
            whole_hash,
            raw_source,
            cached_parse,
            Some(eval_source),
            Some(Arc::new(script_analysis)),
            Some(Arc::new(export_signatures)),
            None,
            dependency_resolutions,
        ))
    }

    pub(crate) fn ensure_shallow_imported_dependency_state_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<crate::ImportedDependencyCacheEntry>> {
        if let Some(entry) = self.clone_current_imported_dependency_entry(canonical_id, store_view)
        {
            if entry.external_type_analysis.is_some() {
                return Some(entry);
            }
        }

        let current = self.clone_current_imported_dependency_entry(canonical_id, store_view);
        let (raw_source, cached_parse, whole_hash) = if let Some(entry) = current.as_ref() {
            (
                Arc::clone(&entry.raw_source),
                entry.cached_parse.clone(),
                entry.whole_hash,
            )
        } else {
            self.current_eval_state_in_view(canonical_id, store_view)?
        };
        let eval_source = current
            .as_ref()
            .and_then(|entry| entry.eval_source.clone())
            .unwrap_or_else(|| {
                Arc::<str>::from(Self::build_eval_script_source(
                    raw_source.as_ref(),
                    cached_parse.as_deref(),
                ))
            });
        let external_type_analysis = self.build_external_type_analysis(
            canonical_id,
            whole_hash,
            raw_source.as_ref(),
            cached_parse.as_deref(),
            &eval_source,
        );

        // Shallow imported state must stay route-agnostic. Keep import/export
        // structure for later symbol-specific routing, but do not eagerly
        // resolve the imported file's dependency map up front.
        let (script_analysis, export_signatures, dependency_resolutions) =
            if let Some(entry) = current.as_ref() {
                (
                    entry.script_analysis.clone(),
                    entry.export_signatures.clone(),
                    entry.dependency_resolutions.clone(),
                )
            } else {
                (
                    None,
                    None,
                    store_view
                        .and_then(|view| view.dependency_resolutions(canonical_id).cloned())
                        .unwrap_or_default(),
                )
            };

        Some(self.cache_imported_dependency_shallow_state(
            canonical_id,
            whole_hash,
            raw_source,
            cached_parse,
            Some(eval_source),
            script_analysis,
            export_signatures,
            Some(external_type_analysis),
            dependency_resolutions,
        ))
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn materialize_imported_dependency_state_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<crate::ImportedDependencyCacheEntry>> {
        let _trace = component_meta_trace_scope!(
            "materialize_imported_dependency_state_in_view",
            format!("owner={} store_view={}", canonical_id, store_view.is_some()),
        );
        let base = self.materialize_imported_dependency_base_in_view(canonical_id, store_view)?;
        let eval_source = base.eval_source.clone().unwrap_or_else(|| {
            Arc::<str>::from(Self::build_eval_script_source(
                base.raw_source.as_ref(),
                base.cached_parse.as_deref(),
            ))
        });
        if base.snapshot.is_some()
            && base.eval_source.is_some()
            && base.external_type_analysis.is_some()
        {
            component_meta_trace_event!(
                "materialize_imported_dependency_state_in_view_result",
                format!(
                    "owner={} hit=true imports={} type_symbols={} value_symbols={} whole_hash={:?}",
                    canonical_id,
                    base.snapshot
                        .as_ref()
                        .map(|snapshot| snapshot.imports.len())
                        .unwrap_or(0),
                    base.shallow_file_state
                        .as_ref()
                        .map(|state| state.symbols.len())
                        .unwrap_or(0),
                    base.shallow_file_state
                        .as_ref()
                        .map(|state| state.value_symbols.len())
                        .unwrap_or(0),
                    base.whole_hash,
                ),
            );
            return Some(base);
        }

        if !base.resolved_canonical_id.ends_with(".vue")
            && base.snapshot.is_none()
            && base.external_type_analysis.is_none()
        {
            let parsed_eval_program = self.cached_parsed_eval_program_entry(
                &base.resolved_canonical_id,
                base.whole_hash,
                &eval_source,
                Self::imported_eval_source_type(
                    &base.resolved_canonical_id,
                    base.raw_source.as_ref(),
                    base.cached_parse.as_deref(),
                ),
            );
            if !parsed_eval_program.parse_failed {
                let program = parsed_eval_program.program.borrow_dependent();
                let source_type = crate::parse::non_sfc_source_type(&base.resolved_canonical_id);
                let mut snapshot = Self::build_snapshot_from_parse(
                    crate::parse::build_non_sfc_snapshot_from_program(
                        &base.resolved_canonical_id,
                        eval_source.as_ref(),
                        source_type,
                        program,
                    ),
                );
                self.resolve_snapshot_imports_in_view(
                    &base.resolved_canonical_id,
                    &mut snapshot,
                    store_view,
                );
                self.enrich_destructured_bindings(&mut snapshot);
                let snapshot = Arc::new(snapshot);
                let external_type_analysis = Arc::new(
                    verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_program(
                        program,
                    ),
                );
                let dependency_resolutions = if base.dependency_resolutions.is_empty() {
                    self.dependency_resolutions_from_snapshot_in_view(
                        &base.resolved_canonical_id,
                        snapshot.as_ref(),
                        store_view,
                    )
                } else {
                    base.dependency_resolutions.clone()
                };
                let entry = self.cache_imported_dependency_state(
                    &base.resolved_canonical_id,
                    base.whole_hash,
                    Arc::clone(&base.raw_source),
                    None,
                    Some(Arc::clone(&snapshot)),
                    Some(Arc::clone(&eval_source)),
                    Some(Arc::clone(&external_type_analysis)),
                    dependency_resolutions,
                );
                if base.resolved_canonical_id != canonical_id {
                    let _ = self.cache_imported_dependency_arc(canonical_id, Arc::clone(&entry));
                }
                component_meta_trace_event!(
                    "materialize_imported_dependency_state_in_view_result",
                    format!(
                        "owner={} hit=false imports={} type_symbols={} value_symbols={} whole_hash={:?} parsed_once=true",
                        canonical_id,
                        snapshot.imports.len(),
                        entry
                            .shallow_file_state
                            .as_ref()
                            .map(|state| state.symbols.len())
                            .unwrap_or(0),
                        entry
                            .shallow_file_state
                            .as_ref()
                            .map(|state| state.value_symbols.len())
                            .unwrap_or(0),
                        base.whole_hash,
                    ),
                );
                return Some(entry);
            }
        }

        let mut snapshot = base
            .snapshot
            .as_ref()
            .map(|snapshot| (**snapshot).clone())
            .unwrap_or_else(|| {
                self.build_snapshot_from_source_state(
                    &base.resolved_canonical_id,
                    &base.raw_source,
                    base.cached_parse.as_deref(),
                )
            });
        self.resolve_snapshot_imports_in_view(
            &base.resolved_canonical_id,
            &mut snapshot,
            store_view,
        );
        self.enrich_destructured_bindings(&mut snapshot);
        let snapshot = Arc::new(snapshot);

        let external_type_analysis = base.external_type_analysis.clone().unwrap_or_else(|| {
            self.build_external_type_analysis(
                &base.resolved_canonical_id,
                base.whole_hash,
                base.raw_source.as_ref(),
                base.cached_parse.as_deref(),
                &eval_source,
            )
        });
        let dependency_resolutions = if base.dependency_resolutions.is_empty() {
            self.dependency_resolutions_from_snapshot_in_view(
                &base.resolved_canonical_id,
                snapshot.as_ref(),
                store_view,
            )
        } else {
            base.dependency_resolutions.clone()
        };

        let entry = self.cache_imported_dependency_state(
            &base.resolved_canonical_id,
            base.whole_hash,
            Arc::clone(&base.raw_source),
            base.cached_parse.clone(),
            Some(Arc::clone(&snapshot)),
            Some(Arc::clone(&eval_source)),
            Some(Arc::clone(&external_type_analysis)),
            dependency_resolutions,
        );
        if base.resolved_canonical_id != canonical_id {
            let _ = self.cache_imported_dependency_arc(canonical_id, Arc::clone(&entry));
        }
        component_meta_trace_event!(
            "materialize_imported_dependency_state_in_view_result",
            format!(
                "owner={} hit=false imports={} type_symbols={} value_symbols={} whole_hash={:?}",
                canonical_id,
                snapshot.imports.len(),
                entry
                    .shallow_file_state
                    .as_ref()
                    .map(|state| state.symbols.len())
                    .unwrap_or(0),
                entry
                    .shallow_file_state
                    .as_ref()
                    .map(|state| state.value_symbols.len())
                    .unwrap_or(0),
                base.whole_hash,
            ),
        );
        Some(entry)
    }

    fn build_external_type_analysis(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        raw_source: &str,
        cached_parse: Option<&verter_compiler::parser::types::ParsedSfc>,
        eval_source: &Arc<str>,
    ) -> Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource> {
        let parsed_eval_program = self.cached_parsed_eval_program_entry(
            canonical_id,
            whole_hash,
            eval_source,
            Self::imported_eval_source_type(canonical_id, raw_source, cached_parse),
        );
        if parsed_eval_program.parse_failed {
            return Arc::new(
                verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(
                ),
            );
        }

        let program = parsed_eval_program.program.borrow_dependent();
        Arc::new(
            verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_program(program),
        )
    }

    fn build_eval_env_and_external_type_analysis(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        raw_source: &str,
        cached_parse: Option<&verter_compiler::parser::types::ParsedSfc>,
        eval_source: &Arc<str>,
    ) -> (
        Arc<verter_semantic::analysis::type_eval::EvalEnv>,
        Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource>,
    ) {
        let parsed_eval_program = self.cached_parsed_eval_program_entry(
            canonical_id,
            whole_hash,
            eval_source,
            Self::imported_eval_source_type(canonical_id, raw_source, cached_parse),
        );
        if parsed_eval_program.parse_failed {
            let mut env = verter_semantic::analysis::type_eval_build::parse_and_build_env(
                eval_source.as_ref(),
            );
            Self::apply_sfc_script_setup_type_params(&mut env, raw_source, cached_parse);
            return (
                Arc::new(env),
                Arc::new(
                    verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(
                    ),
                ),
            );
        }

        let program = parsed_eval_program.program.borrow_dependent();
        let mut env = verter_semantic::analysis::type_eval_build::build_eval_env(
            program,
            eval_source.as_ref(),
        );
        Self::apply_sfc_script_setup_type_params(&mut env, raw_source, cached_parse);
        (
            Arc::new(env),
            Arc::new(
                verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_program(
                    program,
                ),
            ),
        )
    }

    fn project_intrinsic_cache_anchor(&self, canonical_id: &str) -> (String, u64) {
        let ws = self.ws();
        let generation = ws.content_generation();
        let anchor = ws
            .owner_for_file(canonical_id)
            .map(|owner| {
                format!(
                    "{}|{}",
                    owner.project_root,
                    owner.tsconfig_path.unwrap_or_default()
                )
            })
            .unwrap_or_else(|| format!("host:{}", self.instance_id));
        (anchor, generation)
    }

    fn project_intrinsic_members_for_tag_in_view(
        &self,
        owner_canonical_id: &str,
        tag: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember>> {
        let vue_entry =
            self.resolve_project_intrinsic_entry_in_view(owner_canonical_id, "vue", store_view)?;
        let jsx_entry = self.resolve_project_intrinsic_entry_in_view(
            owner_canonical_id,
            "vue/jsx",
            store_view,
        )?;

        let fallback_members = self.expand_project_intrinsic_members_for_type_in_view(
            &vue_entry,
            "HTMLAttributes",
            store_view,
        );
        let tag_members =
            self.expand_project_intrinsic_tag_members_in_view(&jsx_entry, tag, store_view);

        match (
            tag_members.filter(|members| !members.is_empty()),
            fallback_members.filter(|members| !members.is_empty()),
        ) {
            (Some(tag_members), Some(fallback_members)) => {
                Some(Self::merge_intrinsic_members(tag_members, fallback_members))
            }
            (Some(tag_members), None) => Some(tag_members),
            (None, Some(fallback_members)) => Some(fallback_members),
            (None, None) => None,
        }
    }

    fn resolve_project_intrinsic_entry_in_view(
        &self,
        owner_canonical_id: &str,
        specifier: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<crate::ImportedDependencyCacheEntry>> {
        let ws = self.ws();
        let owner = ws.owner_for_file(owner_canonical_id)?;
        let resolved = ws.resolve_import_for_project(
            &owner,
            specifier,
            verter_workspace::ResolutionContext {
                phase: verter_workspace::ResolvePhase::ProviderGraph,
                kind: verter_workspace::ResolveRequestKind::TypeImport,
            },
        )?;
        self.materialize_imported_dependency_state_in_view(&resolved.source_id, store_view)
    }

    fn expand_project_intrinsic_members_for_type_in_view(
        &self,
        entry: &crate::ImportedDependencyCacheEntry,
        type_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember>> {
        let shape =
            self.expand_project_intrinsic_shape_for_type_in_view(entry, type_name, store_view)?;
        Some(Self::owned_intrinsic_members_from_shape(shape))
    }

    fn expand_project_intrinsic_tag_members_in_view(
        &self,
        entry: &crate::ImportedDependencyCacheEntry,
        tag: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember>> {
        let intrinsics_shape = self.expand_project_intrinsic_shape_for_type_in_view(
            entry,
            "JSX.IntrinsicElements",
            store_view,
        )?;
        let tag_type = intrinsics_shape
            .properties
            .into_iter()
            .find(|property| property.name == tag)
            .map(|property| property.ty)?;
        let tag_shape =
            self.expand_project_intrinsic_shape_for_expr_in_view(entry, &tag_type, store_view)?;
        Some(Self::owned_intrinsic_members_from_shape(tag_shape))
    }

    fn expand_project_intrinsic_shape_for_type_in_view(
        &self,
        entry: &crate::ImportedDependencyCacheEntry,
        type_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
        self.expand_project_intrinsic_shape_for_expr_in_view(
            entry,
            &verter_semantic::analysis::type_expr::TypeExpr::named(type_name),
            store_view,
        )
    }

    fn expand_project_intrinsic_shape_for_expr_in_view(
        &self,
        entry: &crate::ImportedDependencyCacheEntry,
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
        let solver_host = crate::resolver_core::SessionSolverHost::with_declaration_scope(
            self,
            store_view,
            &entry.resolved_canonical_id,
        );
        let result = verter_semantic::analysis::type_solver::solve::solve_type(expr, &solver_host);
        Some(verter_semantic::analysis::type_expand::type_expr_to_object_shape(&result.value))
    }

    fn owned_intrinsic_members_from_shape(
        shape: verter_semantic::analysis::type_expand::ExpandedObjectShape,
    ) -> Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember> {
        let mut members = rustc_hash::FxHashMap::default();
        for property in shape.properties {
            if let Some(event_name) =
                verter_semantic::analysis::html_intrinsics::on_prop_to_event_name(
                    property.name.as_str(),
                )
            {
                members.entry(format!("listener:{event_name}")).or_insert(
                    verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember {
                        name: event_name,
                        kind: verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener,
                        type_expr: property.ty,
                    },
                );
                continue;
            }

            if !verter_semantic::analysis::html_intrinsics::should_expose_intrinsic_member(
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr,
                property.name.as_str(),
            ) {
                continue;
            }

            members.entry(format!("attr:{}", property.name)).or_insert(
                verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember {
                    name: property.name,
                    kind: verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr,
                    type_expr: property.ty,
                },
            );
        }

        let mut members: Vec<_> = members.into_values().collect();
        members.sort_by(|left, right| {
            let left_rank = match left.kind {
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr => 0,
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener => 1,
            };
            let right_rank = match right.kind {
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr => 0,
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener => 1,
            };
            left_rank
                .cmp(&right_rank)
                .then_with(|| left.name.cmp(&right.name))
        });
        members
    }

    fn merge_intrinsic_members(
        primary: Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember>,
        fallback: Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember>,
    ) -> Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember> {
        let mut members = rustc_hash::FxHashMap::default();
        for member in fallback {
            members.insert(
                format!(
                    "{}:{}",
                    match member.kind {
                        verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr => "attr",
                        verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener => {
                            "listener"
                        }
                    },
                    member.name
                ),
                member,
            );
        }
        for member in primary {
            members.insert(
                format!(
                    "{}:{}",
                    match member.kind {
                        verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr => "attr",
                        verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener => {
                            "listener"
                        }
                    },
                    member.name
                ),
                member,
            );
        }

        let mut members: Vec<_> = members.into_values().collect();
        members.sort_by(|left, right| {
            let left_rank = match left.kind {
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr => 0,
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener => 1,
            };
            let right_rank = match right.kind {
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr => 0,
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener => 1,
            };
            left_rank
                .cmp(&right_rank)
                .then_with(|| left.name.cmp(&right.name))
        });
        members
    }

    fn clone_cached_imported_type_root(
        &self,
        canonical_id: &str,
        exported_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<(String, String)> {
        self.clone_current_imported_dependency_entry(canonical_id, store_view)?
            .resolved_type_roots
            .get(exported_name)
            .map(|root| (root.canonical_source.clone(), root.resolved_name.clone()))
    }

    fn prepared_cache_hash(canonical_id: &str, whole_hash: Hash16) -> (String, u64) {
        (
            canonical_id.to_string(),
            u64::from_le_bytes(whole_hash[..8].try_into().unwrap_or_default()),
        )
    }

    fn prepared_type_decl_is_valid(
        &self,
        decl: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> bool {
        if decl.cache_deps.defining_file.is_none() && decl.cache_deps.barrel_participants.is_empty()
        {
            return true;
        }

        let mut current_hashes = rustc_hash::FxHashMap::default();
        if let Some((ref canonical_id, _)) = decl.cache_deps.defining_file {
            if let Some(entry) =
                self.clone_current_imported_dependency_entry(canonical_id, store_view)
            {
                let (id, hash) = Self::prepared_cache_hash(canonical_id, entry.whole_hash);
                current_hashes.insert(id, hash);
            }
        }
        for (canonical_id, _) in &decl.cache_deps.barrel_participants {
            if let Some(entry) =
                self.clone_current_imported_dependency_entry(canonical_id, store_view)
            {
                let (id, hash) = Self::prepared_cache_hash(canonical_id, entry.whole_hash);
                current_hashes.insert(id, hash);
            }
        }
        decl.cache_deps.is_valid(&current_hashes)
    }

    fn prepared_value_decl_is_valid(
        &self,
        decl: &verter_semantic::analysis::type_solver::PreparedValueDecl,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> bool {
        if decl.cache_deps.defining_file.is_none() && decl.cache_deps.barrel_participants.is_empty()
        {
            return true;
        }

        let mut current_hashes = rustc_hash::FxHashMap::default();
        if let Some((ref canonical_id, _)) = decl.cache_deps.defining_file {
            if let Some(entry) =
                self.clone_current_imported_dependency_entry(canonical_id, store_view)
            {
                let (id, hash) = Self::prepared_cache_hash(canonical_id, entry.whole_hash);
                current_hashes.insert(id, hash);
            }
        }
        for (canonical_id, _) in &decl.cache_deps.barrel_participants {
            if let Some(entry) =
                self.clone_current_imported_dependency_entry(canonical_id, store_view)
            {
                let (id, hash) = Self::prepared_cache_hash(canonical_id, entry.whole_hash);
                current_hashes.insert(id, hash);
            }
        }
        decl.cache_deps.is_valid(&current_hashes)
    }

    fn cached_dependency_resolution_target(
        &self,
        canonical_id: &str,
        source_specifier: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<String> {
        self.clone_current_imported_dependency_entry(canonical_id, store_view)?
            .dependency_resolutions
            .get(source_specifier)
            .and_then(|resolution| resolution.effective_target().map(str::to_string))
    }

    fn shallow_dep_edges_for_state_in_view(
        &self,
        canonical_id: &str,
        state: &crate::resolver_core::ShallowFileState,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> rustc_hash::FxHashMap<String, String> {
        let mut edges = rustc_hash::FxHashMap::default();
        let dependency_resolutions = self
            .dependency_resolutions_for_eval_in_view(canonical_id, store_view)
            .unwrap_or_default();
        let mut seen_sources = rustc_hash::FxHashSet::default();
        for (source_specifier, _) in state.import_targets.values() {
            if !seen_sources.insert(source_specifier.clone()) {
                continue;
            }
            if let Some(resolved) = dependency_resolutions
                .get(source_specifier)
                .and_then(|resolution| {
                    resolution
                        .effective_target()
                        .map(str::to_string)
                        .or_else(|| resolution.resolved_canonical_id.clone())
                })
                .or_else(|| {
                    self.resolve_type_dependency_canonical_shallow_in_view(
                        canonical_id,
                        source_specifier,
                        store_view,
                    )
                })
            {
                edges.insert(source_specifier.clone(), resolved);
            }
        }
        edges
    }

    fn backfill_prepared_decl_caches_from_shallow_state_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) {
        let Some(entry) = self.clone_current_imported_dependency_entry(canonical_id, store_view)
        else {
            return;
        };
        if !entry.prepared_type_decls.is_empty() || !entry.prepared_value_decls.is_empty() {
            return;
        }
        let Some(state) = entry.shallow_file_state.as_ref() else {
            return;
        };
        if state.symbols.is_empty() && state.value_symbols.is_empty() {
            return;
        }

        let dep_edges =
            self.shallow_dep_edges_for_state_in_view(canonical_id, state.as_ref(), store_view);
        let dep_edges_ref = (!dep_edges.is_empty()).then_some(&dep_edges);
        let prepared_type_decls = crate::resolver_core::build_prepared_type_decl_cache(
            canonical_id,
            state,
            dep_edges_ref,
        );
        let prepared_value_decls = crate::resolver_core::build_prepared_value_decl_cache(
            canonical_id,
            state,
            dep_edges_ref,
        );

        let workspace_generation = self.ws().content_generation();
        let mut cache = self.imported_dependency_cache.lock();
        let Some(cached) = cache.get_mut(canonical_id) else {
            return;
        };
        if cached.workspace_generation != workspace_generation
            || cached.whole_hash != entry.whole_hash
        {
            return;
        }
        let cached = Arc::make_mut(cached);
        if cached.prepared_type_decls.is_empty() {
            cached.prepared_type_decls = prepared_type_decls;
        }
        if cached.prepared_value_decls.is_empty() {
            cached.prepared_value_decls = prepared_value_decls;
        }
    }

    fn clone_cached_prepared_type_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>> {
        let decl = self
            .clone_current_imported_dependency_entry(canonical_id, store_view)?
            .prepared_type_decls
            .get(symbol_name)
            .cloned()?;
        self.prepared_type_decl_is_valid(decl.as_ref(), store_view)
            .then_some(decl)
    }

    fn clone_cached_prepared_value_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
        let decl = self
            .clone_current_imported_dependency_entry(canonical_id, store_view)?
            .prepared_value_decls
            .get(symbol_name)
            .cloned()?;
        self.prepared_value_decl_is_valid(decl.as_ref(), store_view)
            .then_some(decl)
    }

    pub(crate) fn prepared_type_decl_in_view(
        &self,
        canonical_id: &str,
        symbol_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>> {
        if let Some(cached) =
            self.clone_cached_prepared_type_decl(canonical_id, symbol_name, store_view)
        {
            return Some(cached);
        }
        if self
            .clone_current_imported_dependency_entry(canonical_id, store_view)
            .is_some_and(|entry| !entry.prepared_type_decls.is_empty())
        {
            return None;
        }
        let _ = self.ensure_shallow_imported_dependency_state_in_view(canonical_id, store_view)?;
        self.backfill_prepared_decl_caches_from_shallow_state_in_view(canonical_id, store_view);
        self.clone_cached_prepared_type_decl(canonical_id, symbol_name, store_view)
    }

    pub(crate) fn prepared_value_decl_in_view(
        &self,
        canonical_id: &str,
        symbol_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
        if let Some(cached) =
            self.clone_cached_prepared_value_decl(canonical_id, symbol_name, store_view)
        {
            return Some(cached);
        }
        if self
            .clone_current_imported_dependency_entry(canonical_id, store_view)
            .is_some_and(|entry| !entry.prepared_value_decls.is_empty())
        {
            return None;
        }
        let _ = self.ensure_shallow_imported_dependency_state_in_view(canonical_id, store_view)?;
        self.backfill_prepared_decl_caches_from_shallow_state_in_view(canonical_id, store_view);
        self.clone_cached_prepared_value_decl(canonical_id, symbol_name, store_view)
    }

    fn clone_cached_required_import_names_for_exported_type(
        &self,
        canonical_id: &str,
        exported_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<rustc_hash::FxHashSet<String>> {
        self.clone_current_imported_dependency_entry(canonical_id, store_view)?
            .exported_required_import_names
            .get(exported_name)
            .map(|names| (**names).clone())
    }

    pub(crate) fn required_import_names_for_exported_type_in_view(
        &self,
        canonical_id: &str,
        exported_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> rustc_hash::FxHashSet<String> {
        if let Some(cached) = self.clone_cached_required_import_names_for_exported_type(
            canonical_id,
            exported_name,
            store_view,
        ) {
            return cached;
        }

        let required =
            if let Some(analysis) = self.external_type_analysis_in_view(canonical_id, store_view) {
                analysis.required_import_names(exported_name)
            } else {
                rustc_hash::FxHashSet::default()
            };
        // Walker-based import name discovery deleted. The solver resolves on demand.

        if self
            .clone_current_imported_dependency_entry(canonical_id, store_view)
            .is_some()
        {
            self.cache_required_import_names_for_exported_type(
                canonical_id,
                exported_name,
                required.clone(),
                store_view,
            );
        }

        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "required_import_names_for_exported_type source={} exported={} count={}",
                canonical_id,
                exported_name,
                required.len(),
            ));
        }

        required
    }

    fn imported_symbol_dependencies_in_view(
        &self,
        canonical_id: &str,
        exported_name: &str,
        decl_body: &verter_semantic::analysis::type_expr::TypeExpr,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Vec<ImportedSymbolDependency> {
        let analysis = match self.external_type_analysis_in_view(canonical_id, store_view) {
            Some(analysis) => analysis,
            None => return Vec::new(),
        };
        let mut dependencies = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        let mut referenced_names = std::collections::BTreeSet::new();
        collect_type_expr_symbol_refs(decl_body, &mut referenced_names);
        for referenced_name in referenced_names {
            let root_name = referenced_name
                .split('.')
                .next()
                .unwrap_or(referenced_name.as_str());
            if root_name == exported_name || is_builtin_type_symbol(root_name) {
                continue;
            }

            if let Some((import_source, imported_name)) =
                analysis.local_import_symbol_target(root_name)
            {
                let imported_member = if root_name == referenced_name {
                    imported_name.to_string()
                } else if let Some(suffix) = referenced_name.strip_prefix(root_name) {
                    format!("{}{}", imported_name, suffix)
                } else {
                    imported_name.to_string()
                };
                let Some(dep_canonical) = self.resolve_type_dependency_canonical_in_view(
                    canonical_id,
                    import_source,
                    store_view,
                ) else {
                    continue;
                };
                let (resolved_canonical, resolved_name) = self.resolve_imported_type_root_in_view(
                    dep_canonical.as_str(),
                    imported_member.as_str(),
                    store_view,
                );
                if seen.insert((
                    referenced_name.clone(),
                    resolved_canonical.clone(),
                    resolved_name.clone(),
                )) {
                    dependencies.push(ImportedSymbolDependency {
                        local_name: referenced_name,
                        canonical_id: resolved_canonical,
                        exported_name: resolved_name,
                    });
                }
                continue;
            }

            if analysis.local_symbol_span(root_name).is_some()
                && seen.insert((
                    root_name.to_string(),
                    canonical_id.to_string(),
                    root_name.to_string(),
                ))
            {
                dependencies.push(ImportedSymbolDependency {
                    local_name: root_name.to_string(),
                    canonical_id: canonical_id.to_string(),
                    exported_name: root_name.to_string(),
                });
            }
        }
        dependencies.sort_by(|left, right| {
            left.local_name
                .cmp(&right.local_name)
                .then_with(|| left.canonical_id.cmp(&right.canonical_id))
                .then_with(|| left.exported_name.cmp(&right.exported_name))
        });
        dependencies
    }

    pub(crate) fn imported_symbol_dependencies_for_expr_in_view(
        &self,
        canonical_id: &str,
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Vec<ImportedSymbolDependency> {
        self.cache_only_lookup_symbol_dependencies_for_expr_in_view(canonical_id, expr, store_view)
    }

    fn cache_only_lookup_symbol_dependencies_for_expr_in_view(
        &self,
        canonical_id: &str,
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Vec<ImportedSymbolDependency> {
        let mut dependencies =
            self.imported_symbol_dependencies_in_view(canonical_id, "", expr, store_view);
        dependencies.sort_by(|left, right| {
            left.local_name
                .cmp(&right.local_name)
                .then_with(|| left.canonical_id.cmp(&right.canonical_id))
                .then_with(|| left.exported_name.cmp(&right.exported_name))
        });
        dependencies
    }

    // Deleted: the loop body that used runtime_value_roots, analysis, canonical_id,
    // store_view was part of the walker-based dependency discovery path.
    // Placeholder to satisfy the compiler for any remaining references:
    fn structural_imported_symbol_dependencies_for_exported_type_in_view(
        &self,
        canonical_id: &str,
        exported_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Vec<ImportedSymbolDependency> {
        let Some(state) = self.symbol_shallow_file_state_in_view(canonical_id, store_view) else {
            return Vec::new();
        };
        let closure = state.local_closure(
            exported_name,
            crate::resolver_core::ResolutionBudgets::default().local_closure_steps,
        );
        let mut dependencies = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();

        for local_symbol in closure.local_symbols_used {
            if local_symbol == exported_name {
                continue;
            }
            if seen.insert((
                local_symbol.clone(),
                canonical_id.to_string(),
                local_symbol.clone(),
            )) {
                dependencies.push(ImportedSymbolDependency {
                    local_name: local_symbol.clone(),
                    canonical_id: canonical_id.to_string(),
                    exported_name: local_symbol,
                });
            }
        }

        for external in closure.unresolved_external {
            let Some(resolved_canonical_id) = self.cached_dependency_resolution_target(
                canonical_id,
                external.source_specifier.as_str(),
                store_view,
            ) else {
                continue;
            };
            if seen.insert((
                external.local_name.clone(),
                resolved_canonical_id.clone(),
                external.imported_name.clone(),
            )) {
                dependencies.push(ImportedSymbolDependency {
                    local_name: external.local_name,
                    canonical_id: resolved_canonical_id,
                    exported_name: external.imported_name,
                });
            }
        }

        dependencies.sort_by(|left, right| {
            left.local_name
                .cmp(&right.local_name)
                .then_with(|| left.canonical_id.cmp(&right.canonical_id))
                .then_with(|| left.exported_name.cmp(&right.exported_name))
        });
        dependencies
    }

    fn clone_cached_external_type_analysis(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource>>
    {
        self.clone_current_imported_dependency_entry(canonical_id, store_view)?
            .external_type_analysis
            .clone()
    }

    pub(crate) fn external_type_analysis_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource>>
    {
        let _trace = component_meta_trace_scope!(
            "external_type_analysis_in_view",
            format!("owner={} store_view={}", canonical_id, store_view.is_some()),
        );
        let resolved_canonical_id = self
            .resolve_eval_dependency_canonical_in_view(canonical_id, store_view)
            .unwrap_or_else(|| canonical_id.to_string());
        if let Some(cached) = self
            .clone_cached_external_type_analysis(canonical_id, store_view)
            .or_else(|| {
                if resolved_canonical_id != canonical_id {
                    self.clone_cached_external_type_analysis(
                        resolved_canonical_id.as_str(),
                        store_view,
                    )
                } else {
                    None
                }
            })
        {
            let stats = cached.stats();
            component_meta_trace_event!(
                "external_type_analysis_cache_hit",
                format!(
                    "owner={} statements={} bindings={} reexports={} wildcards={} import_locals={} local_type_symbols={} local_export_symbols={}",
                    canonical_id,
                    stats.top_level_statement_count,
                    stats.binding_count,
                    stats.direct_reexport_count,
                    stats.wildcard_reexport_count,
                    stats.import_local_count,
                    stats.local_type_symbol_count,
                    stats.local_export_symbol_count,
                ),
            );
            return Some(cached);
        }

        let entry = self.ensure_shallow_imported_dependency_state_in_view(
            resolved_canonical_id.as_str(),
            store_view,
        )?;
        let analysis = entry.external_type_analysis.clone()?;
        let stats = analysis.stats();
        component_meta_trace_event!(
            "external_type_analysis_built",
            format!(
                "owner={} statements={} bindings={} reexports={} wildcards={} import_locals={} local_type_symbols={} local_export_symbols={}",
                canonical_id,
                stats.top_level_statement_count,
                stats.binding_count,
                stats.direct_reexport_count,
                stats.wildcard_reexport_count,
                stats.import_local_count,
                stats.local_type_symbol_count,
                stats.local_export_symbol_count,
            ),
        );
        Some(analysis)
    }

    /// Get or build the canonical shallow type file state for an imported
    /// dependency.  The state is populated through the shared host ensure-path
    /// and cached on the `ImportedDependencyCacheEntry`.
    ///
    /// Consumed by the frontier engine (production cache-warming pass in
    /// `resolve_external_type_from_loaded_files_in_view`) and integration tests.
    pub(crate) fn shallow_file_state_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        let resolved_canonical_id = self
            .resolve_eval_dependency_canonical_in_view(canonical_id, store_view)
            .unwrap_or_else(|| canonical_id.to_string());

        // Fast path: check if already cached
        if let Some(entry) =
            self.clone_current_imported_dependency_entry(resolved_canonical_id.as_str(), store_view)
        {
            if let Some(ref state) = entry.shallow_file_state {
                if !state.symbols.is_empty() {
                    return Some(Arc::clone(state));
                }
            }
        }

        // Ensure shallow state is built and cached
        let entry = self.ensure_shallow_imported_dependency_state_in_view(
            resolved_canonical_id.as_str(),
            store_view,
        )?;

        // Build shallow type state from available data if not yet present
        if let Some(ref state) = entry.shallow_file_state {
            if !state.symbols.is_empty() {
                return Some(Arc::clone(state));
            }
        }

        // Build from analysis + eval source if available
        let analysis = entry.external_type_analysis.clone()?;
        let state = Arc::new(
            crate::resolver_core::ShallowFileState::from_analysis_with_source(
                entry.whole_hash,
                analysis,
                entry.eval_source.as_deref(),
                None,
            ),
        );

        // Cache the built state
        let workspace_generation = self.ws().content_generation();
        let mut cache = self.imported_dependency_cache.lock();
        if let Some(cached) = cache.get_mut(resolved_canonical_id.as_str()) {
            if cached.workspace_generation == workspace_generation
                && cached.whole_hash == entry.whole_hash
                && (cached.shallow_file_state.is_none()
                    || cached
                        .shallow_file_state
                        .as_ref()
                        .is_some_and(|state| state.symbols.is_empty()))
            {
                Arc::make_mut(cached).shallow_file_state = Some(Arc::clone(&state));
            }
        }

        Some(state)
    }

    pub(crate) fn symbol_shallow_file_state_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        let resolved_canonical_id = self
            .resolve_eval_dependency_canonical_in_view(canonical_id, store_view)
            .unwrap_or_else(|| canonical_id.to_string());

        if let Some(entry) =
            self.clone_current_imported_dependency_entry(resolved_canonical_id.as_str(), store_view)
        {
            if let Some(ref state) = entry.shallow_file_state {
                if !state.symbols.is_empty() {
                    return Some(Arc::clone(state));
                }
            }
        }

        let entry = self.materialize_imported_dependency_state_in_view(
            resolved_canonical_id.as_str(),
            store_view,
        )?;

        if let Some(ref state) = entry.shallow_file_state {
            if !state.symbols.is_empty() {
                return Some(Arc::clone(state));
            }
        }

        let analysis = entry.external_type_analysis.clone()?;
        let state = Arc::new(
            crate::resolver_core::ShallowFileState::from_analysis_with_source(
                entry.whole_hash,
                analysis,
                entry.eval_source.as_deref(),
                None,
            ),
        );

        let workspace_generation = self.ws().content_generation();
        let mut cache = self.imported_dependency_cache.lock();
        if let Some(cached) = cache.get_mut(resolved_canonical_id.as_str()) {
            if cached.workspace_generation == workspace_generation
                && cached.whole_hash == entry.whole_hash
                && (cached.shallow_file_state.is_none()
                    || cached
                        .shallow_file_state
                        .as_ref()
                        .is_some_and(|state| state.symbols.is_empty()))
            {
                Arc::make_mut(cached).shallow_file_state = Some(Arc::clone(&state));
            }
        }

        Some(state)
    }

    pub(crate) fn resolve_external_type_from_cached_dependency_state_in_view(
        &self,
        dep_canonical: &str,
        type_name: &str,
        imported_companions: &rustc_hash::FxHashMap<
            String,
            verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements,
        >,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements> {
        let _trace = component_meta_trace_scope!(
            "resolve_external_type_from_cached_dependency_state_in_view",
            format!(
                "owner={} type={} store_view={}",
                dep_canonical,
                type_name,
                store_view.is_some()
            ),
        );
        let entry =
            self.ensure_shallow_imported_dependency_state_in_view(dep_canonical, store_view)?;
        let analysis = entry.external_type_analysis.as_ref()?;
        let eval_source = entry
            .eval_source
            .as_ref()
            .map(Arc::clone)
            .unwrap_or_else(|| Arc::clone(&entry.raw_source));
        let source_type = Self::imported_eval_source_type(
            &entry.resolved_canonical_id,
            entry.raw_source.as_ref(),
            entry.cached_parse.as_deref(),
        );
        let Some(type_context) = self.cached_type_resolution_context_entry(
            &entry.resolved_canonical_id,
            entry.whole_hash,
            &eval_source,
            source_type,
        ) else {
            component_meta_trace_event!(
                "resolve_external_type_from_cached_dependency_state_in_view_result",
                format!(
                    "owner={} type={} hit=false local_symbol_target={} parse_failed_or_missing_type_context=true",
                    dep_canonical,
                    type_name,
                    analysis.has_local_symbol_target(type_name),
                ),
            );
            return None;
        };
        let program = type_context.borrow_owner().borrow_dependent();
        let base_ctx = type_context.borrow_dependent();
        let resolved = verter_compiler::utils::oxc::vue::resolve_type::resolve_external_type_in_context_with_analyzed_symbol_companion(
            type_name,
            program,
            type_context.borrow_owner().source_bytes(),
            base_ctx,
            analysis.as_ref(),
            imported_companions,
        );
        component_meta_trace_event!(
            "resolve_external_type_from_cached_dependency_state_in_view_result",
            format!(
                "owner={} type={} hit={} local_symbol_target={} parse_failed=false",
                dep_canonical,
                type_name,
                resolved.is_some(),
                analysis.has_local_symbol_target(type_name),
            ),
        );
        resolved
    }

    pub(crate) fn resolve_direct_type_reexport_target_in_view(
        &self,
        dep_canonical: &str,
        requested_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<(String, String)> {
        let _trace = component_meta_trace_scope!(
            "resolve_direct_type_reexport_target_in_view",
            format!("owner={} requested={}", dep_canonical, requested_name),
        );
        let analysis = self.external_type_analysis_in_view(dep_canonical, store_view)?;
        let (import_source, imported_name) = analysis.direct_reexport_target(requested_name)?;
        let next_canonical = self.resolve_type_dependency_canonical_in_view(
            dep_canonical,
            import_source,
            store_view,
        )?;
        component_meta_trace_event!(
            "resolve_direct_type_reexport_target_in_view_result",
            format!(
                "owner={} requested={} import_source={} target={} exported={}",
                dep_canonical, requested_name, import_source, next_canonical, imported_name
            ),
        );
        Some((next_canonical, imported_name.to_string()))
    }

    pub(crate) fn resolve_local_import_symbol_target_in_view(
        &self,
        dep_canonical: &str,
        resolved_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<(String, String)> {
        let _trace = component_meta_trace_scope!(
            "resolve_local_import_symbol_target_in_view",
            format!("owner={} requested={}", dep_canonical, resolved_name),
        );
        let analysis = self.external_type_analysis_in_view(dep_canonical, store_view)?;
        let (import_source, imported_name) = analysis.local_import_symbol_target(resolved_name)?;
        let next_canonical = self.resolve_type_dependency_canonical_in_view(
            dep_canonical,
            import_source,
            store_view,
        )?;
        component_meta_trace_event!(
            "resolve_local_import_symbol_target_in_view_result",
            format!(
                "owner={} requested={} import_source={} target={} exported={}",
                dep_canonical, resolved_name, import_source, next_canonical, imported_name
            ),
        );
        Some((next_canonical, imported_name.to_string()))
    }

    pub(crate) fn resolve_local_export_symbol_target_in_view(
        &self,
        canonical_source: &str,
        exported_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<String> {
        let _trace = component_meta_trace_scope!(
            "resolve_local_export_symbol_target_in_view",
            format!("owner={} requested={}", canonical_source, exported_name),
        );
        let analysis = self.external_type_analysis_in_view(canonical_source, store_view)?;
        let target = analysis
            .local_export_symbol_target(exported_name)
            .map(str::to_string);
        if let Some(target) = target.as_deref() {
            component_meta_trace_event!(
                "resolve_local_export_symbol_target_in_view_result",
                format!(
                    "owner={} requested={} target={}",
                    canonical_source, exported_name, target
                ),
            );
        }
        target
    }

    pub(crate) fn resolve_imported_type_root_in_view(
        &self,
        dep_canonical: &str,
        imported_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> (String, String) {
        let _trace = component_meta_trace_scope!(
            "resolve_imported_type_root",
            format!("canonical={} imported={}", dep_canonical, imported_name),
        );
        if let Some(cached) =
            self.clone_cached_imported_type_root(dep_canonical, imported_name, store_view)
        {
            component_meta_trace_event!(
                "resolve_imported_type_root_cache_hit",
                format!("canonical={} imported={}", dep_canonical, imported_name),
            );
            return cached;
        }

        let normalized_canonical = self
            .resolve_eval_dependency_canonical_in_view(dep_canonical, store_view)
            .unwrap_or_else(|| dep_canonical.to_string());

        let _ = self.ensure_shallow_imported_dependency_state_in_view(
            normalized_canonical.as_str(),
            store_view,
        );
        if let Some(cached) =
            self.clone_cached_imported_type_root(dep_canonical, imported_name, store_view)
        {
            component_meta_trace_event!(
                "resolve_imported_type_root_cache_hit",
                format!(
                    "canonical={} imported={} source=host_imported_dep",
                    dep_canonical, imported_name
                ),
            );
            return cached;
        }

        let resolved = self
            .resolve_named_type_export_target_in_view(
                normalized_canonical.as_str(),
                imported_name,
                store_view,
            )
            .map(|(canonical, exported_name)| {
                let canonical = self
                    .resolve_eval_dependency_canonical_in_view(canonical.as_str(), store_view)
                    .unwrap_or(canonical);
                (canonical, exported_name)
            })
            .unwrap_or_else(|| {
                let canonical = self
                    .resolve_eval_dependency_canonical_in_view(
                        normalized_canonical.as_str(),
                        store_view,
                    )
                    .unwrap_or(normalized_canonical);
                (canonical, imported_name.to_string())
            });
        self.cache_imported_type_root(dep_canonical, imported_name, resolved.clone(), store_view);
        resolved
    }

    fn cache_imported_dependency(
        &self,
        canonical_id: &str,
        entry: crate::ImportedDependencyCacheEntry,
    ) -> Arc<crate::ImportedDependencyCacheEntry> {
        let mut cache = self.imported_dependency_cache.lock();
        if let Some(cached) = cache.get_mut(canonical_id) {
            if cached.workspace_generation == entry.workspace_generation
                && cached.whole_hash == entry.whole_hash
            {
                let cached_entry = Arc::make_mut(cached);
                if cached_entry.resolved_canonical_id != entry.resolved_canonical_id {
                    cached_entry.resolved_canonical_id = entry.resolved_canonical_id.clone();
                }
                if cached_entry.cached_parse.is_none() && entry.cached_parse.is_some() {
                    cached_entry.cached_parse = entry.cached_parse.clone();
                }
                if cached_entry.script_analysis.is_none() && entry.script_analysis.is_some() {
                    cached_entry.script_analysis = entry.script_analysis.clone();
                }
                if cached_entry.export_signatures.is_none() && entry.export_signatures.is_some() {
                    cached_entry.export_signatures = entry.export_signatures.clone();
                }
                if cached_entry.external_type_analysis.is_none()
                    && entry.external_type_analysis.is_some()
                {
                    cached_entry.external_type_analysis = entry.external_type_analysis.clone();
                }
                if cached_entry.shallow_file_state.is_none() && entry.shallow_file_state.is_some() {
                    cached_entry.shallow_file_state = entry.shallow_file_state.clone();
                }
                if cached_entry.snapshot.is_none() && entry.snapshot.is_some() {
                    cached_entry.snapshot = entry.snapshot.clone();
                }
                if cached_entry.eval_source.is_none() && entry.eval_source.is_some() {
                    cached_entry.eval_source = entry.eval_source.clone();
                }
                if cached_entry.required_owner_import_names.is_none()
                    && entry.required_owner_import_names.is_some()
                {
                    cached_entry.required_owner_import_names =
                        entry.required_owner_import_names.clone();
                }
                if cached_entry.dependency_resolutions.is_empty()
                    && !entry.dependency_resolutions.is_empty()
                {
                    cached_entry.dependency_resolutions = entry.dependency_resolutions.clone();
                }
                for (name, required_names) in &entry.exported_required_import_names {
                    cached_entry
                        .exported_required_import_names
                        .entry(name.clone())
                        .or_insert_with(|| Arc::clone(required_names));
                }
                for (name, root) in &entry.resolved_type_roots {
                    cached_entry
                        .resolved_type_roots
                        .entry(name.clone())
                        .or_insert_with(|| root.clone());
                }
                for (name, declaration) in &entry.resolved_type_declarations {
                    cached_entry
                        .resolved_type_declarations
                        .entry(name.clone())
                        .or_insert_with(|| declaration.clone());
                }
                for (name, prepared) in &entry.prepared_type_decls {
                    cached_entry
                        .prepared_type_decls
                        .entry(name.clone())
                        .or_insert_with(|| Arc::clone(prepared));
                }
                for (name, prepared) in &entry.prepared_value_decls {
                    cached_entry
                        .prepared_value_decls
                        .entry(name.clone())
                        .or_insert_with(|| Arc::clone(prepared));
                }
                return Arc::clone(cached);
            }
        }
        let entry = Arc::new(entry);
        cache.insert(canonical_id.to_string(), Arc::clone(&entry));
        entry
    }

    fn cache_imported_dependency_arc(
        &self,
        canonical_id: &str,
        entry: Arc<crate::ImportedDependencyCacheEntry>,
    ) -> Arc<crate::ImportedDependencyCacheEntry> {
        let mut cache = self.imported_dependency_cache.lock();
        cache.insert(canonical_id.to_string(), Arc::clone(&entry));
        entry
    }

    fn cache_imported_type_root(
        &self,
        canonical_id: &str,
        exported_name: &str,
        root: (String, String),
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) {
        if self
            .clone_current_imported_dependency_entry(canonical_id, store_view)
            .is_none()
        {
            return;
        }

        let workspace_generation = self.ws().content_generation();
        let mut cache = self.imported_dependency_cache.lock();
        let Some(entry) = cache.get_mut(canonical_id) else {
            return;
        };
        if entry.workspace_generation != workspace_generation {
            return;
        }
        Arc::make_mut(entry).resolved_type_roots.insert(
            exported_name.to_string(),
            crate::ImportedTypeRootCacheEntry {
                canonical_source: root.0,
                resolved_name: root.1,
            },
        );
    }

    fn cache_required_import_names_for_exported_type(
        &self,
        canonical_id: &str,
        exported_name: &str,
        required_names: rustc_hash::FxHashSet<String>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) {
        if self
            .clone_current_imported_dependency_entry(canonical_id, store_view)
            .is_none()
        {
            return;
        }

        let workspace_generation = self.ws().content_generation();
        let mut cache = self.imported_dependency_cache.lock();
        let Some(entry) = cache.get_mut(canonical_id) else {
            return;
        };
        if entry.workspace_generation != workspace_generation {
            return;
        }
        Arc::make_mut(entry)
            .exported_required_import_names
            .insert(exported_name.to_string(), Arc::new(required_names));
    }

    fn cache_imported_dependency_source(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        raw_source: Arc<str>,
    ) -> Arc<crate::ImportedDependencyCacheEntry> {
        self.cache_imported_dependency(
            canonical_id,
            crate::ImportedDependencyCacheEntry {
                workspace_generation: self.ws().content_generation(),
                whole_hash,
                resolved_canonical_id: canonical_id.to_string(),
                raw_source,
                cached_parse: None,
                script_analysis: None,
                export_signatures: None,
                external_type_analysis: None,
                shallow_file_state: None,
                snapshot: None,
                eval_source: None,
                required_owner_import_names: None,
                exported_required_import_names: rustc_hash::FxHashMap::default(),
                resolved_type_roots: rustc_hash::FxHashMap::default(),
                resolved_type_declarations: rustc_hash::FxHashMap::default(),
                prepared_type_decls: rustc_hash::FxHashMap::default(),
                prepared_value_decls: rustc_hash::FxHashMap::default(),
                dependency_resolutions: rustc_hash::FxHashMap::default(),
            },
        )
    }

    fn cache_imported_dependency_shallow_state(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        raw_source: Arc<str>,
        cached_parse: Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
        eval_source: Option<Arc<str>>,
        script_analysis: Option<Arc<verter_semantic::analysis::ScriptAnalysisSnapshot>>,
        export_signatures: Option<Arc<Vec<verter_semantic::analysis::ExportSignature>>>,
        external_type_analysis: Option<
            Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource>,
        >,
        dependency_resolutions: rustc_hash::FxHashMap<String, DependencyResolution>,
    ) -> Arc<crate::ImportedDependencyCacheEntry> {
        let external_type_analysis = if let Some(ref eval_source) = eval_source {
            Some(external_type_analysis.unwrap_or_else(|| {
                self.build_external_type_analysis(
                    canonical_id,
                    whole_hash,
                    raw_source.as_ref(),
                    cached_parse.as_deref(),
                    eval_source,
                )
            }))
        } else {
            external_type_analysis
        };
        let shallow_file_state = external_type_analysis.as_ref().map(|eta| {
            Arc::new(
                crate::resolver_core::ShallowFileState::from_analysis_with_source(
                    whole_hash,
                    Arc::clone(eta),
                    eval_source.as_deref(),
                    None,
                ),
            )
        });
        self.cache_imported_dependency(
            canonical_id,
            crate::ImportedDependencyCacheEntry {
                workspace_generation: self.ws().content_generation(),
                whole_hash,
                resolved_canonical_id: canonical_id.to_string(),
                raw_source,
                cached_parse,
                script_analysis,
                export_signatures,
                external_type_analysis: external_type_analysis.clone(),
                shallow_file_state,
                snapshot: None,
                eval_source,
                required_owner_import_names: None,
                exported_required_import_names: rustc_hash::FxHashMap::default(),
                resolved_type_roots: rustc_hash::FxHashMap::default(),
                resolved_type_declarations: rustc_hash::FxHashMap::default(),
                prepared_type_decls: rustc_hash::FxHashMap::default(),
                prepared_value_decls: rustc_hash::FxHashMap::default(),
                dependency_resolutions,
            },
        )
    }

    pub(crate) fn cache_imported_dependency_state(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        raw_source: Arc<str>,
        cached_parse: Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
        snapshot: Option<Arc<FileAnalysisSnapshot>>,
        eval_source: Option<Arc<str>>,
        external_type_analysis: Option<
            Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource>,
        >,
        dependency_resolutions: rustc_hash::FxHashMap<String, DependencyResolution>,
    ) -> Arc<crate::ImportedDependencyCacheEntry> {
        let shallow_file_state = external_type_analysis.as_ref().map(|eta| {
            Arc::new(
                crate::resolver_core::ShallowFileState::from_analysis_with_source(
                    whole_hash,
                    Arc::clone(eta),
                    eval_source.as_deref(),
                    None,
                ),
            )
        });
        let dep_edges = dep_edges_from_resolutions(&dependency_resolutions);
        let dep_edges_ref = if dep_edges.is_empty() {
            None
        } else {
            Some(&dep_edges)
        };
        let prepared_type_decls = shallow_file_state
            .as_ref()
            .map(|state| {
                crate::resolver_core::build_prepared_type_decl_cache(
                    canonical_id,
                    state,
                    dep_edges_ref,
                )
            })
            .unwrap_or_default();
        let prepared_value_decls = shallow_file_state
            .as_ref()
            .map(|state| {
                crate::resolver_core::build_prepared_value_decl_cache(
                    canonical_id,
                    state,
                    dep_edges_ref,
                )
            })
            .unwrap_or_default();

        self.cache_imported_dependency(
            canonical_id,
            crate::ImportedDependencyCacheEntry {
                workspace_generation: self.ws().content_generation(),
                whole_hash,
                resolved_canonical_id: canonical_id.to_string(),
                raw_source,
                cached_parse,
                script_analysis: self
                    .effective_file_state(canonical_id, None)
                    .filter(|state| state.whole_hash == whole_hash)
                    .map(|state| Arc::new(state.script_analysis)),
                export_signatures: snapshot
                    .as_ref()
                    .map(|snapshot| Arc::clone(&snapshot.export_signatures)),
                external_type_analysis: external_type_analysis.clone(),
                shallow_file_state,
                snapshot,
                eval_source,
                required_owner_import_names: None,
                exported_required_import_names: rustc_hash::FxHashMap::default(),
                resolved_type_roots: rustc_hash::FxHashMap::default(),
                resolved_type_declarations: rustc_hash::FxHashMap::default(),
                prepared_type_decls,
                prepared_value_decls,
                dependency_resolutions,
            },
        )
    }

    fn cache_eval_env_arc(
        &self,
        cache_keys: &[String],
        whole_hash: Hash16,
        cached_env: Arc<verter_semantic::analysis::type_eval::EvalEnv>,
    ) -> Arc<verter_semantic::analysis::type_eval::EvalEnv> {
        let mut cache = self.eval_env_cache.lock();
        for cache_key in cache_keys {
            if let Some((cached_hash, existing_env)) = cache.get(cache_key) {
                if *cached_hash == whole_hash {
                    return Arc::clone(existing_env);
                }
            }
        }

        for cache_key in cache_keys {
            cache.insert(cache_key.clone(), (whole_hash, Arc::clone(&cached_env)));
        }
        cached_env
    }

    pub(crate) fn cache_raw_analysis_snapshot_arc(
        &self,
        canonical: &str,
        whole_hash: Hash16,
        snapshot: Arc<FileAnalysisSnapshot>,
    ) -> Arc<FileAnalysisSnapshot> {
        let mut cache = self.raw_analysis_snapshot_cache.lock();
        if let Some(entry) = cache.get(canonical) {
            if entry.whole_hash == whole_hash {
                return Arc::clone(&entry.snapshot);
            }
        }
        cache.insert(
            canonical.to_string(),
            crate::RawAnalysisSnapshotCacheEntry {
                whole_hash,
                snapshot: Arc::clone(&snapshot),
            },
        );
        snapshot
    }

    pub(crate) fn base_eval_env_arc_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<verter_semantic::analysis::type_eval::EvalEnv>> {
        let _trace = component_meta_trace_scope!(
            "base_eval_env",
            format!("owner={} store_view={}", canonical_id, store_view.is_some()),
        );
        let resolved_canonical_id = self
            .resolve_eval_dependency_canonical_in_view(canonical_id, store_view)
            .unwrap_or_else(|| canonical_id.to_string());
        let imported_cache_keys = if resolved_canonical_id == canonical_id {
            vec![canonical_id.to_string()]
        } else {
            vec![canonical_id.to_string(), resolved_canonical_id.clone()]
        };
        let (raw_source, cached_parse, whole_hash) =
            self.current_eval_state_in_view(resolved_canonical_id.as_str(), store_view)?;
        if let Some(cached_env) = self
            .clone_cached_eval_env_arc(canonical_id, whole_hash)
            .or_else(|| {
                if resolved_canonical_id != canonical_id {
                    self.clone_cached_eval_env_arc(resolved_canonical_id.as_str(), whole_hash)
                } else {
                    None
                }
            })
        {
            component_meta_trace_event!(
                "base_eval_env_cache_hit",
                format!("owner={} whole_hash={whole_hash:?}", canonical_id),
            );
            return Some(cached_env);
        }

        let eval_source = self
            .clone_current_imported_dependency_entry(resolved_canonical_id.as_str(), store_view)
            .and_then(|entry| entry.eval_source.clone())
            .unwrap_or_else(|| {
                Arc::<str>::from(Self::build_eval_script_source(
                    raw_source.as_ref(),
                    cached_parse.as_deref(),
                ))
            });
        let (cached_env, _) = self.build_eval_env_and_external_type_analysis(
            resolved_canonical_id.as_str(),
            whole_hash,
            raw_source.as_ref(),
            cached_parse.as_deref(),
            &eval_source,
        );
        component_meta_trace_event!(
            "base_eval_env_built",
            format!("owner={} whole_hash={whole_hash:?}", canonical_id),
        );
        Some(self.cache_eval_env_arc(&imported_cache_keys, whole_hash, cached_env))
    }

    pub(crate) fn local_type_declaration_id_in_view(
        &self,
        canonical_source: &str,
        resolved_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<verter_semantic::analysis::type_eval::DeclarationId> {
        if self
            .get_raw_analysis_snapshot_in_view(canonical_source, store_view)
            .is_some_and(|snapshot| {
                snapshot.imports.iter().any(|import| {
                    import
                        .bindings
                        .iter()
                        .any(|binding| binding.name == resolved_name)
                })
            })
        {
            return None;
        }

        self.base_eval_env_in_view(canonical_source, store_view)
            .and_then(|env| env.type_declaration_id(resolved_name))
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn base_eval_env_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<verter_semantic::analysis::type_eval::EvalEnv> {
        self.base_eval_env_arc_in_view(canonical_id, store_view)
            .map(|env| (*env).clone())
    }

    fn peel_value_decl_alias_in_view(
        &self,
        canonical_id: &str,
        name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> ValueDeclIdentity {
        let mut current = ValueDeclIdentity {
            canonical_id: canonical_id.to_string(),
            name: name.to_string(),
        };
        let mut visited = rustc_hash::FxHashSet::default();

        loop {
            if !visited.insert(current.clone()) {
                break;
            }

            let Some(env) =
                self.base_eval_env_arc_in_view(current.canonical_id.as_str(), store_view)
            else {
                break;
            };
            let Some(decl) = env.value_symbols.get(current.name.as_str()) else {
                break;
            };
            let Some(verter_semantic::analysis::type_expr::TypeExpr::TypeOf(value_ref)) =
                decl.type_annotation.as_ref()
            else {
                break;
            };
            let Some(next_name) = value_ref.path.first() else {
                break;
            };
            if value_ref.path.len() != 1 || *next_name == current.name {
                break;
            }
            if !env.value_symbols.contains_key(next_name.as_str()) {
                break;
            }

            current.name = next_name.clone();
        }

        current
    }

    pub(crate) fn resolve_value_export_target_in_view(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<ValueDeclIdentity> {
        let export = self.resolve_named_export_in_view(
            dep_canonical_id,
            imported_name,
            Some(false),
            store_view,
        )?;
        let source_canonical_id = export
            .source_canonical_id
            .unwrap_or_else(|| dep_canonical_id.to_string());
        Some(self.peel_value_decl_alias_in_view(
            source_canonical_id.as_str(),
            export.source_name.as_str(),
            store_view,
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

    pub(crate) fn dependency_resolutions_from_snapshot(
        snapshot: &FileAnalysisSnapshot,
    ) -> rustc_hash::FxHashMap<String, DependencyResolution> {
        snapshot
            .imports
            .iter()
            .map(|import| {
                (
                    import.source.clone(),
                    DependencyResolution {
                        specifier: import.source.clone(),
                        resolved_canonical_id: import.resolved_canonical_id.clone(),
                        possible_canonical_ids: Vec::new(),
                    },
                )
            })
            .collect()
    }

    fn dependency_resolutions_from_parts_in_view(
        &self,
        canonical_id: &str,
        imports: &[verter_semantic::analysis::AnalyzedImport],
        export_signatures: &[verter_semantic::analysis::ExportSignature],
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> rustc_hash::FxHashMap<String, DependencyResolution> {
        let mut resolutions: rustc_hash::FxHashMap<String, DependencyResolution> = imports
            .iter()
            .map(|import| {
                (
                    import.source.clone(),
                    DependencyResolution {
                        specifier: import.source.clone(),
                        resolved_canonical_id: import.resolved_canonical_id.clone(),
                        possible_canonical_ids: Vec::new(),
                    },
                )
            })
            .collect();
        let declaration_file = canonical_id.ends_with(".d.ts")
            || canonical_id.ends_with(".d.mts")
            || canonical_id.ends_with(".d.cts");

        let mut upsert_resolution =
            |specifier: &str,
             kind: verter_workspace::ResolveRequestKind,
             prefer_live_fallback: bool| {
                let existing = resolutions.get(specifier).cloned();
                let resolved: Option<String> = existing
                    .as_ref()
                    .and_then(|resolution| {
                        resolution
                            .resolved_canonical_id
                            .clone()
                            .or_else(|| resolution.effective_target().map(str::to_string))
                    })
                    .or_else(|| match kind {
                        // Snapshot dependency maps must stay structural: selecting a route may
                        // consult these cached targets, but only the eventual chosen target is
                        // allowed to deepen into eval-source fallback or companion loading.
                        verter_workspace::ResolveRequestKind::TypeImport => self
                            .resolve_type_dependency_canonical_shallow_in_view(
                                canonical_id,
                                specifier,
                                store_view,
                            ),
                        _ => self.resolve_loaded_dependency_canonical_in_view(
                            canonical_id,
                            specifier,
                            kind,
                            store_view,
                        ),
                    })
                    .or_else(|| {
                        if !prefer_live_fallback {
                            return None;
                        }
                        match kind {
                            verter_workspace::ResolveRequestKind::TypeImport => {
                                self.resolve_type_dependency_canonical(canonical_id, specifier)
                            }
                            _ => self.resolve_loaded_dependency_canonical(
                                canonical_id,
                                specifier,
                                kind,
                            ),
                        }
                    });

                let entry = resolutions.entry(specifier.to_string()).or_insert_with(|| {
                    DependencyResolution {
                        specifier: specifier.to_string(),
                        resolved_canonical_id: None,
                        possible_canonical_ids: Vec::new(),
                    }
                });
                if let Some(resolved) = resolved {
                    entry.resolved_canonical_id = Some(resolved.clone());
                    if entry.possible_canonical_ids.is_empty() {
                        entry.possible_canonical_ids.push(resolved);
                    }
                }
            };

        for import in imports {
            let kind = if import.is_type_only {
                verter_workspace::ResolveRequestKind::TypeImport
            } else {
                verter_workspace::ResolveRequestKind::EsmImport
            };
            upsert_resolution(import.source.as_str(), kind, false);
        }

        for sig in export_signatures {
            let Some(source) = sig.reexport_source.as_deref() else {
                continue;
            };
            let kind = if sig.is_type || declaration_file {
                verter_workspace::ResolveRequestKind::TypeImport
            } else {
                verter_workspace::ResolveRequestKind::EsmImport
            };
            upsert_resolution(source, kind, true);
        }

        resolutions
    }

    fn dependency_resolutions_from_snapshot_in_view(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> rustc_hash::FxHashMap<String, DependencyResolution> {
        self.dependency_resolutions_from_parts_in_view(
            canonical_id,
            &snapshot.imports,
            &snapshot.export_signatures,
            store_view,
        )
    }

    pub(crate) fn build_snapshot_from_source(
        &self,
        canonical: &str,
        source: &Arc<str>,
    ) -> FileAnalysisSnapshot {
        let _trace = component_meta_trace_scope!(
            "build_snapshot_from_source",
            format!("owner={} bytes={}", canonical, source.len()),
        );
        if canonical.ends_with(".vue") {
            let _parse_trace =
                component_meta_trace_scope!("parse_vue_snapshot", format!("owner={canonical}"));
            let (parse, _) =
                crate::parse::parse_vue_snapshot(canonical, source, self.config.effective_scope());
            component_meta_trace_event!(
                "parse_vue_snapshot_result",
                format!(
                    "owner={} imports={} macros={} export_signatures={}",
                    canonical,
                    parse.script_analysis.imports.len(),
                    parse.script_analysis.macros.len(),
                    parse.export_signatures.len(),
                ),
            );
            Self::build_snapshot_from_parse(parse)
        } else {
            let _parse_trace =
                component_meta_trace_scope!("parse_non_sfc_snapshot", format!("owner={canonical}"));
            let parse = crate::parse::parse_non_sfc_snapshot(canonical, source);
            component_meta_trace_event!(
                "parse_non_sfc_snapshot_result",
                format!(
                    "owner={} imports={} macros={} export_signatures={}",
                    canonical,
                    parse.script_analysis.imports.len(),
                    parse.script_analysis.macros.len(),
                    parse.export_signatures.len(),
                ),
            );
            Self::build_snapshot_from_parse(parse)
        }
    }

    fn build_snapshot_from_source_state(
        &self,
        canonical: &str,
        source: &Arc<str>,
        cached_parse: Option<&verter_compiler::parser::types::ParsedSfc>,
    ) -> FileAnalysisSnapshot {
        if canonical.ends_with(".vue") {
            if let Some(parsed) = cached_parse {
                let _trace = component_meta_trace_scope!(
                    "build_snapshot_from_cached_parse",
                    format!("owner={} bytes={}", canonical, source.len()),
                );
                let parse = crate::parse::build_vue_snapshot_from_parsed(
                    canonical,
                    source.as_ref(),
                    self.config.effective_scope(),
                    parsed,
                );
                component_meta_trace_event!(
                    "parse_vue_snapshot_cached_result",
                    format!(
                        "owner={} imports={} macros={} export_signatures={}",
                        canonical,
                        parse.script_analysis.imports.len(),
                        parse.script_analysis.macros.len(),
                        parse.export_signatures.len(),
                    ),
                );
                return Self::build_snapshot_from_parse(parse);
            }
        }

        self.build_snapshot_from_source(canonical, source)
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
        result: &verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    ) -> bool {
        result.is_empty()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn current_eval_state(
        &self,
        canonical_id: &str,
    ) -> Option<(
        Arc<str>,
        Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
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
        Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
        Hash16,
    )> {
        let _trace = component_meta_trace_scope!(
            "current_eval_state",
            format!("owner={} store_view={}", canonical_id, store_view.is_some()),
        );
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
                component_meta_trace_event!(
                    "current_eval_state_scheduler_hit",
                    format!(
                        "owner={} source_len={} has_cached_parse={} whole_hash={:?}",
                        canonical_id,
                        state.source.len(),
                        state.cached_parse.is_some(),
                        state.whole_hash,
                    ),
                );
                Some((state.source, state.cached_parse, state.whole_hash))
            } else {
                if let Some(imported_entry) =
                    self.clone_current_imported_dependency_entry(canonical_id, store_view)
                {
                    component_meta_trace_event!(
                        "current_eval_state_imported_cache_hit",
                        format!(
                            "owner={} source_len={} has_cached_parse={} whole_hash={:?}",
                            canonical_id,
                            imported_entry.raw_source.len(),
                            imported_entry.cached_parse.is_some(),
                            imported_entry.whole_hash,
                        ),
                    );
                    return Some((
                        Arc::clone(&imported_entry.raw_source),
                        imported_entry.cached_parse.clone(),
                        imported_entry.whole_hash,
                    ));
                }

                let source = self.read_analysis_source_in_view(canonical_id, store_view)?;
                let cached_parse = canonical_id
                    .ends_with(".vue")
                    .then(|| Arc::new(verter_compiler::compile::parse_sfc(&source, None, None)));
                let whole_hash = crate::hash::hash_16(source.as_bytes());
                let _ = self.cache_imported_dependency_state(
                    canonical_id,
                    whole_hash,
                    Arc::clone(&source),
                    cached_parse.clone(),
                    None,
                    None,
                    None,
                    rustc_hash::FxHashMap::default(),
                );
                component_meta_trace_event!(
                    "current_eval_state_fallback_load",
                    format!(
                        "owner={} source_len={} has_cached_parse={} whole_hash={whole_hash:?}",
                        canonical_id,
                        source.len(),
                        cached_parse.is_some(),
                    ),
                );
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
                component_meta_trace_event!(
                    "current_eval_state_cache_hit",
                    format!(
                        "owner={} source_len={} has_cached_parse={} whole_hash={:?}",
                        canonical_id,
                        entry.source.len(),
                        entry.cached_parse.is_some(),
                        entry.whole_hash,
                    ),
                );
                Some((
                    Arc::clone(&entry.source),
                    entry.cached_parse.clone(),
                    entry.whole_hash,
                ))
            } else {
                drop(files);
                if let Some(imported_entry) =
                    self.clone_current_imported_dependency_entry(canonical_id, store_view)
                {
                    component_meta_trace_event!(
                        "current_eval_state_imported_cache_hit",
                        format!(
                            "owner={} source_len={} has_cached_parse={} whole_hash={:?}",
                            canonical_id,
                            imported_entry.raw_source.len(),
                            imported_entry.cached_parse.is_some(),
                            imported_entry.whole_hash,
                        ),
                    );
                    return Some((
                        Arc::clone(&imported_entry.raw_source),
                        imported_entry.cached_parse.clone(),
                        imported_entry.whole_hash,
                    ));
                }

                let source = self.read_analysis_source_in_view(canonical_id, store_view)?;
                let cached_parse = canonical_id
                    .ends_with(".vue")
                    .then(|| Arc::new(verter_compiler::compile::parse_sfc(&source, None, None)));
                let whole_hash = crate::hash::hash_16(source.as_bytes());
                let _ = self.cache_imported_dependency_state(
                    canonical_id,
                    whole_hash,
                    Arc::clone(&source),
                    cached_parse.clone(),
                    None,
                    None,
                    None,
                    rustc_hash::FxHashMap::default(),
                );
                component_meta_trace_event!(
                    "current_eval_state_disk_load",
                    format!(
                        "owner={} source_len={} has_cached_parse={} whole_hash={whole_hash:?}",
                        canonical_id,
                        source.len(),
                        cached_parse.is_some(),
                    ),
                );
                Some((source.clone(), cached_parse, whole_hash))
            }
        }
    }

    pub(crate) fn dependency_resolutions_for_eval_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<rustc_hash::FxHashMap<String, DependencyResolution>> {
        let _trace = component_meta_trace_scope!(
            "dependency_resolutions_for_eval",
            format!("owner={} store_view={}", canonical_id, store_view.is_some()),
        );
        if let Some(view) = store_view {
            if !self.analysis_source_exists_in_view(canonical_id, Some(view)) {
                return None;
            }
            let mut resolutions = view
                .dependency_resolutions(canonical_id)
                .cloned()
                .unwrap_or_default();
            if let Some(snapshot) = self.get_raw_analysis_snapshot_in_view(canonical_id, Some(view))
            {
                let structural = self.dependency_resolutions_from_snapshot_in_view(
                    canonical_id,
                    &snapshot,
                    Some(view),
                );
                for (specifier, derived) in structural {
                    let replace = resolutions.get(&specifier).is_none_or(|existing| {
                        existing.resolved_canonical_id.is_none()
                            && existing.effective_target().is_none()
                            && existing.possible_canonical_ids.is_empty()
                    });
                    if replace {
                        resolutions.insert(specifier, derived);
                    }
                }
            }
            component_meta_trace_event!(
                "dependency_resolutions_for_eval_result",
                format!("owner={} count={}", canonical_id, resolutions.len()),
            );
            return Some(resolutions);
        }

        if !self.analysis_source_exists_in_view(canonical_id, store_view) {
            return None;
        }

        let fallback_from_snapshot = || {
            self.get_raw_analysis_snapshot_in_view(canonical_id, store_view)
                .map(|snapshot| {
                    self.dependency_resolutions_from_snapshot_in_view(
                        canonical_id,
                        &snapshot,
                        store_view,
                    )
                })
                .unwrap_or_default()
        };

        #[cfg(feature = "scheduler")]
        {
            let stored = self
                .compile_cache
                .get(canonical_id)
                .map(|entry| entry.dependency_resolutions.clone())
                .unwrap_or_default();
            let resolutions = if stored.is_empty() {
                self.clone_current_imported_dependency_entry(canonical_id, store_view)
                    .map(|entry| entry.dependency_resolutions.clone())
                    .filter(|resolutions| !resolutions.is_empty())
                    .unwrap_or_else(fallback_from_snapshot)
            } else {
                stored
            };
            component_meta_trace_event!(
                "dependency_resolutions_for_eval_result",
                format!("owner={} count={}", canonical_id, resolutions.len()),
            );
            Some(resolutions)
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let stored = files
                .get(canonical_id)
                .map(|entry| entry.dependency_resolutions.clone())
                .unwrap_or_default();
            let resolutions = if stored.is_empty() {
                drop(files);
                self.clone_current_imported_dependency_entry(canonical_id, store_view)
                    .map(|entry| entry.dependency_resolutions.clone())
                    .filter(|resolutions| !resolutions.is_empty())
                    .unwrap_or_else(fallback_from_snapshot)
            } else {
                stored
            };
            component_meta_trace_event!(
                "dependency_resolutions_for_eval_result",
                format!("owner={} count={}", canonical_id, resolutions.len()),
            );
            Some(resolutions)
        }
    }

    pub(crate) fn resolve_eval_dependency_canonical_in_view(
        &self,
        dep_canonical: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<String> {
        if dep_canonical.is_empty() {
            return None;
        }

        let read_candidate =
            |candidate: &str| self.analysis_source_exists_in_view(candidate, store_view);

        let prefers_type_companion = dep_canonical.ends_with(".js")
            || dep_canonical.ends_with(".jsx")
            || dep_canonical.ends_with(".mjs")
            || dep_canonical.ends_with(".cjs");

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

        if !prefers_type_companion && read_candidate(dep_canonical) {
            return Some(dep_canonical.to_string());
        }

        for candidate in candidates {
            if read_candidate(&candidate) {
                return Some(candidate);
            }
        }

        if prefers_type_companion && read_candidate(dep_canonical) {
            return Some(dep_canonical.to_string());
        }

        None
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

    pub(crate) fn compute_evaluated_types_with_tracking_from_owner_context_in_view(
        &self,
        canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        owner_eval_source: Option<&str>,
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<ComputedEvaluatedTypes> {
        let eval_source = owner_eval_source.map(str::to_string).or_else(|| {
            self.current_eval_state_in_view(canonical, store_view).map(
                |(source, cached_parse, _)| {
                    Self::build_eval_script_source(&source, cached_parse.as_deref())
                },
            )
        })?;
        self.compute_evaluated_types_from_owner_context_in_view(
            canonical,
            snapshot,
            &eval_source,
            dep_resolutions,
            store_view,
        )
    }

    fn seed_component_meta_owner_cache_without_eval_in_view(
        &self,
        canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        eval_source: &str,
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<crate::ImportedDependencyCacheEntry>> {
        let current = self.clone_current_imported_dependency_entry(canonical, store_view);
        let eval_source_arc = Arc::<str>::from(eval_source.to_string());
        let (raw_source, cached_parse, whole_hash) = if let Some(entry) = current.as_ref() {
            (
                Arc::clone(&entry.raw_source),
                entry.cached_parse.clone(),
                entry.whole_hash,
            )
        } else {
            self.current_eval_state_in_view(canonical, store_view)?
        };

        let external_type_analysis = if let Some(analysis) = current
            .as_ref()
            .and_then(|entry| entry.external_type_analysis.clone())
        {
            analysis
        } else if eval_source.is_empty() {
            Arc::new(
                verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(
                ),
            )
        } else {
            let source_type = Self::imported_eval_source_type(
                canonical,
                raw_source.as_ref(),
                cached_parse.as_deref(),
            );
            let parsed_eval_program = self.cached_parsed_eval_program_entry(
                canonical,
                whole_hash,
                &eval_source_arc,
                source_type,
            );
            let analysis = if parsed_eval_program.parse_failed {
                verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(
                )
            } else {
                let program = parsed_eval_program.program.borrow_dependent();
                verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_program(
                    program,
                )
            };
            Arc::new(analysis)
        };

        let shallow_file_state = Arc::new(
            crate::resolver_core::ShallowFileState::from_analysis_with_source(
                whole_hash,
                Arc::clone(&external_type_analysis),
                Some(eval_source),
                None,
            ),
        );
        let dep_edges = dep_edges_from_resolutions(dep_resolutions);
        let dep_edges_ref = if dep_edges.is_empty() {
            None
        } else {
            Some(&dep_edges)
        };

        let entry = Arc::new(crate::ImportedDependencyCacheEntry {
            workspace_generation: self.ws().content_generation(),
            whole_hash,
            resolved_canonical_id: current
                .as_ref()
                .map(|entry| entry.resolved_canonical_id.clone())
                .unwrap_or_else(|| canonical.to_string()),
            raw_source,
            cached_parse,
            script_analysis: current
                .as_ref()
                .and_then(|entry| entry.script_analysis.clone()),
            export_signatures: current
                .as_ref()
                .and_then(|entry| entry.export_signatures.clone())
                .or_else(|| Some(snapshot.export_signatures.clone())),
            external_type_analysis: Some(Arc::clone(&external_type_analysis)),
            shallow_file_state: Some(Arc::clone(&shallow_file_state)),
            snapshot: current
                .as_ref()
                .and_then(|entry| entry.snapshot.clone())
                .or_else(|| Some(Arc::new(snapshot.clone()))),
            eval_source: Some(eval_source_arc),
            required_owner_import_names: current
                .as_ref()
                .and_then(|entry| entry.required_owner_import_names.clone()),
            exported_required_import_names: current
                .as_ref()
                .map(|entry| entry.exported_required_import_names.clone())
                .unwrap_or_default(),
            resolved_type_roots: current
                .as_ref()
                .map(|entry| entry.resolved_type_roots.clone())
                .unwrap_or_default(),
            resolved_type_declarations: current
                .as_ref()
                .map(|entry| entry.resolved_type_declarations.clone())
                .unwrap_or_default(),
            prepared_type_decls: crate::resolver_core::build_prepared_type_decl_cache(
                canonical,
                &shallow_file_state,
                dep_edges_ref,
            ),
            prepared_value_decls: crate::resolver_core::build_prepared_value_decl_cache(
                canonical,
                &shallow_file_state,
                dep_edges_ref,
            ),
            dependency_resolutions: dep_resolutions.clone(),
        });

        Some(self.cache_imported_dependency_arc(canonical, entry))
    }

    fn component_meta_binding_type_entries_in_view(
        &self,
        canonical: &str,
        requested_binding_names: &rustc_hash::FxHashSet<String>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Vec<(String, verter_semantic::analysis::type_expr::TypeExpr)> {
        if requested_binding_names.is_empty() {
            return Vec::new();
        }

        let _ = self.shallow_file_state_in_view(canonical, store_view);

        requested_binding_names
            .iter()
            .filter_map(|name| {
                self.prepared_value_decl_in_view(canonical, name, store_view)
                    .and_then(|decl| decl.type_annotation.clone().map(|ty| (name.clone(), ty)))
            })
            .collect()
    }

    fn compute_evaluated_types_from_owner_context_in_view(
        &self,
        canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        eval_source: &str,
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<ComputedEvaluatedTypes> {
        {
            let _trace = component_meta_trace_scope!(
                "compute_evaluated_types_seed_owner_cache",
                format!("owner={} store_view={}", canonical, store_view.is_some()),
            );
            let _ = self.seed_component_meta_owner_cache_without_eval_in_view(
                canonical,
                snapshot,
                eval_source,
                dep_resolutions,
                store_view,
            );
        }
        let requested_binding_names =
            crate::resolver_core::collect_requested_binding_names(snapshot.macros.as_ref());
        let binding_entries = {
            let _trace = component_meta_trace_scope!(
                "compute_evaluated_types_binding_entries",
                format!(
                    "owner={} requested_bindings={} store_view={}",
                    canonical,
                    requested_binding_names.len(),
                    store_view.is_some(),
                ),
            );
            self.component_meta_binding_type_entries_in_view(
                canonical,
                &requested_binding_names,
                store_view,
            )
        };
        let solver_host = crate::resolver_core::SessionSolverHost::with_declaration_scope(
            self, store_view, canonical,
        );
        // Solver start precondition: ensure the owner's direct imports are
        // present in the host cache before macro expansion starts.
        //
        // Do not prewarm transitive frontier files here. The solver is
        // demand-driven and reads prepared declarations on demand; eagerly
        // materializing every touched transitive file reopens the old
        // request-wide frontier warmup path and dominates cold-query latency
        // on large declaration packages.
        {
            let _trace = component_meta_trace_scope!(
                "compute_evaluated_types_seed_direct_imports",
                format!(
                    "owner={} imports={} store_view={}",
                    canonical,
                    snapshot.imports.len(),
                    store_view.is_some()
                ),
            );
            for import in snapshot.imports.iter() {
                if let Some(ref canonical_id) = import.resolved_canonical_id {
                    let _ = self
                        .ensure_shallow_imported_dependency_state_in_view(canonical_id, store_view);
                }
            }
        }
        let result = {
            let _trace = component_meta_trace_scope!(
                "compute_evaluated_types_expand_macros",
                format!(
                    "owner={} macros={} bindings={} store_view={}",
                    canonical,
                    snapshot.macros.len(),
                    binding_entries.len(),
                    store_view.is_some(),
                ),
            );
            verter_semantic::analysis::type_eval_build::expand_macro_types_with_bindings(
                snapshot.macros.as_ref(),
                Some(eval_source),
                binding_entries.as_slice(),
                &solver_host,
            )
        };
        // Dependency tracking comes from the frontier/shallow-file-state path.
        let discovered_dependencies = std::collections::BTreeSet::<String>::new();
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "compute_evaluated_types owner={} props={} define_props={} emits={} slot_bindings={} bindings={}",
                canonical,
                result.props.len(),
                result.define_props.len(),
                result.emits.len(),
                result.slot_bindings.len(),
                result.bindings.len(),
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
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedComponentTypes> {
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
    ) -> Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis> {
        self.provenance
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let started = component_meta_debug_enabled().then(Instant::now);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let store_view = self.resolver_store_view();

        let resolved = self.resolve_component_meta_in_view(
            canonical.as_str(),
            crate::types::ResolverMode::Expanded,
            &store_view,
        )?;
        // Always include fallthrough — the solver path does not use walker
        // overflow as a gating signal.
        let meta = extract_component_meta_from_resolved(
            self,
            canonical.as_str(),
            &resolved,
            true, // include_fallthrough
            Some(&store_view),
        );
        if let Some(started) = started {
            component_meta_debug(format!(
                "get_component_meta owner={} took {:?}",
                canonical,
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
        verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
        crate::meta_resolve::ResolvedComponentMetaState,
    )> {
        self.provenance
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let store_view = self.resolver_store_view();

        let resolved = self.resolve_component_meta_in_view(
            canonical.as_str(),
            crate::types::ResolverMode::Expanded,
            &store_view,
        )?;
        // Always include fallthrough — the solver path does not use walker
        // overflow as a gating signal.
        let analysis = extract_component_meta_from_resolved(
            self,
            canonical.as_str(),
            &resolved,
            true, // include_fallthrough
            Some(&store_view),
        );
        Some((analysis, resolved))
    }

    /// Resolve the accepted surface for a component's fallthrough inheritance.
    ///
    /// This is an internal method â€” the host owns all inheritance semantics.
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
            &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
        >,
        visiting: &mut rustc_hash::FxHashSet<String>,
        store_view: Option<&HostStoreView>,
    ) -> Option<crate::types::FallthroughResolution> {
        use verter_semantic::analysis::component_meta::*;
        let started = component_meta_debug_enabled().then(Instant::now);
        let _trace = component_meta_trace_scope!(
            "resolve_fallthrough_surface",
            format!(
                "owner={} overrides={} visiting={} store_view={}",
                canonical_id,
                prop_type_overrides
                    .map(|overrides| overrides.len())
                    .unwrap_or_default(),
                visiting.len(),
                store_view.is_some(),
            ),
        );

        // Cycle detection
        if !visiting.insert(canonical_id.to_string()) {
            self.provenance
                .resolver_cycle_detections
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            component_meta_trace_event!(
                "resolve_fallthrough_cycle",
                format!("owner={} visiting={}", canonical_id, visiting.len()),
            );
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

        let result = crate::resolver_core::run_fallthrough_request(
            self,
            &self.resolver_runtime().top_level_fallthrough_singleflight,
            canonical_id,
            prop_type_overrides,
            visiting,
            store_view,
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
        if let Some(resolution) = result.value.as_ref() {
            component_meta_trace_event!(
                "resolve_fallthrough_result",
                format!(
                    "owner={} accepted_props={} accepted_events={} fact_versions={} completeness={:?}",
                    canonical_id,
                    resolution.accepted_props.len(),
                    resolution.accepted_events.len(),
                    resolution.fact_versions.len(),
                    resolution.accepted_surface_completeness,
                ),
            );
        }
        result.value
    }

    fn compute_fallthrough_surface_uncached(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
        >,
        visiting: &mut rustc_hash::FxHashSet<String>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<crate::types::FallthroughResolution> {
        // Try to reuse an already-cached Expanded resolved state before recomputing.
        // get_component_meta() typically resolves Expanded just before calling fallthrough,
        // so the cache should be warm.
        let resolved = if let Some(view) = store_view {
            self.try_get_cached_resolved_meta(
                canonical_id,
                crate::types::ResolverMode::Expanded,
                view,
            )
        } else {
            None
        };
        let resolved = if let Some(cached) = resolved {
            cached
        } else {
            let whole_hash = store_view
                .and_then(|view| view.whole_hash(canonical_id))
                .or_else(|| self.get_whole_hash(canonical_id))
                .unwrap_or_default();
            self.compute_component_meta_state(
                canonical_id,
                crate::types::ResolverMode::Expanded,
                whole_hash,
                store_view,
            )?
        };
        self.compute_fallthrough_surface_from_resolved_state(
            canonical_id,
            &resolved,
            prop_type_overrides,
            visiting,
            store_view,
        )
    }

    pub(crate) fn compute_fallthrough_surface_from_resolved_state(
        &self,
        canonical_id: &str,
        resolved: &crate::meta_resolve::ResolvedComponentMetaState,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
        >,
        visiting: &mut rustc_hash::FxHashSet<String>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<crate::types::FallthroughResolution> {
        let fallthrough_fact_versions = resolved.fact_versions.clone();

        let resolved_macros = resolver_component_meta_resolved_macros(
            resolved.snapshot.macros.as_ref(),
            &resolved.resolved_macros,
        );
        let resolved_type_registry =
            resolver_component_meta_type_registry(&resolved.resolved_type_registry);
        let input = verter_semantic::analysis::component_meta::ComponentMetaInput {
            macros: &resolved.snapshot.macros,
            bindings: &resolved.snapshot.bindings,
            imports: &resolved.snapshot.imports,
            template: resolved.snapshot.template.as_deref(),
            options_api: resolved.snapshot.options_api.as_ref(),
            analysis_flags: verter_semantic::analysis::types::AnalysisFlags::from_bits_truncate(
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
        let base_meta = verter_semantic::analysis::component_meta::extract_component_meta(input);
        let fallthrough_resolver = HostFallthroughResolver {
            host: self,
            parent_canonical_id: canonical_id,
            store_view,
        };
        // Build a lightweight fallthrough eval env: base owner env + runtime
        // values + prop overrides.
        let eval_env = self.build_fallthrough_eval_env_lightweight_in_view(
            canonical_id,
            &resolved.snapshot,
            prop_type_overrides,
            store_view,
        );

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

    /// Lightweight fallthrough eval env: base owner env + runtime values + overrides.
    fn build_fallthrough_eval_env_lightweight_in_view(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
        >,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<verter_semantic::analysis::type_eval::EvalEnv> {
        let _trace = component_meta_trace_scope!(
            "build_fallthrough_eval_env_lightweight",
            format!(
                "owner={} imports={} overrides={} store_view={}",
                canonical_id,
                snapshot.imports.len(),
                prop_type_overrides
                    .map(|overrides| overrides.len())
                    .unwrap_or_default(),
                store_view.is_some(),
            ),
        );
        let mut env = self
            .base_eval_env_arc_in_view(canonical_id, store_view)
            .map(|env| (*env).clone())?;

        // Hydrate required runtime values from imports.
        let required_runtime_value_names = collect_required_template_runtime_value_names(snapshot);
        let local_value_names: rustc_hash::FxHashSet<String> =
            env.value_symbols.keys().cloned().collect();
        self.materialize_imported_runtime_values_into_env_in_view(
            canonical_id,
            snapshot,
            &local_value_names,
            Some(&required_runtime_value_names),
            &mut env,
            store_view,
        );

        // Apply prop type overrides for generic root propagation.
        if let Some(overrides) = prop_type_overrides {
            crate::resolver_core::inject_prop_type_overrides(&mut env, overrides);
        }

        Some(env)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn resolve_shallow_symbol_dependency_alias_in_view(
        &self,
        canonical_id: &str,
        exported_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<(
        String,
        String,
        crate::resolver_core::CachedPreparedImportedTypeAlias,
    )> {
        let mut visiting = rustc_hash::FxHashSet::default();
        self.resolve_shallow_symbol_dependency_alias_with_visiting_in_view(
            canonical_id,
            exported_name,
            store_view,
            &mut visiting,
        )
    }

    pub(crate) fn resolve_prepared_symbol_dependency_alias_in_view(
        &self,
        canonical_id: &str,
        exported_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<(
        String,
        String,
        crate::resolver_core::CachedPreparedImportedTypeAlias,
    )> {
        let (resolved_canonical_id, resolved_exported_name) =
            self.resolve_imported_type_root_in_view(canonical_id, exported_name, store_view);
        let prepared = self.prepared_type_decl_in_view(
            resolved_canonical_id.as_str(),
            resolved_exported_name.as_str(),
            store_view,
        )?;
        let (body, keep_symbolic_utility_body) = match &prepared.body {
            TypeExpr::Ref { name, .. }
                if matches!(
                    verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(
                        name.as_ref()
                    ),
                    Some(
                        verter_semantic::analysis::type_solver::builtin::BuiltinUtility::Pick
                            | verter_semantic::analysis::type_solver::builtin::BuiltinUtility::Omit
                            | verter_semantic::analysis::type_solver::builtin::BuiltinUtility::Partial
                            | verter_semantic::analysis::type_solver::builtin::BuiltinUtility::Required
                            | verter_semantic::analysis::type_solver::builtin::BuiltinUtility::Readonly
                            | verter_semantic::analysis::type_solver::builtin::BuiltinUtility::Record
                    )
                ) =>
            {
                let reexported_package_utility = prepared.external_deps.iter().any(|dep| {
                    if !dep.canonical_id.contains("/node_modules/") {
                        return false;
                    }
                    let (final_canonical_id, final_symbol_name) =
                        self.resolve_imported_type_root_in_view(
                            dep.canonical_id.as_str(),
                            dep.symbol_name.as_str(),
                            store_view,
                        );
                    final_canonical_id != dep.canonical_id || final_symbol_name != dep.symbol_name
                });
                if reexported_package_utility {
                    (prepared.body.clone(), true)
                } else {
                    let solver_host =
                        crate::resolver_core::SessionSolverHost::with_declaration_scope(
                            self,
                            store_view,
                            resolved_canonical_id.as_str(),
                        );
                    let solved = verter_semantic::analysis::type_solver::solve::solve_type(
                        &TypeExpr::named(resolved_exported_name.as_str()),
                        &solver_host,
                    );
                    match solved.value {
                        TypeExpr::Object(_)
                        | TypeExpr::Intersection(_)
                        | TypeExpr::Union(_)
                        | TypeExpr::Tuple { .. }
                        | TypeExpr::Array { .. } => (solved.value, false),
                        _ => (prepared.body.clone(), true),
                    }
                }
            }
            _ => (prepared.body.clone(), false),
        };
        let symbol_dependencies = if keep_symbolic_utility_body {
            Vec::new()
        } else {
            self.structural_imported_symbol_dependencies_for_exported_type_in_view(
                resolved_canonical_id.as_str(),
                resolved_exported_name.as_str(),
                store_view,
            )
        };
        let cached = crate::resolver_core::CachedPreparedImportedTypeAlias {
            decl: verter_semantic::analysis::type_eval::TypeDeclInfo {
                name: resolved_exported_name.clone(),
                declaration_id: 0,
                kind: prepared.kind,
                type_parameters: prepared.type_parameters.clone(),
                body,
            },
            canonical_dependencies: {
                let mut deps = std::collections::BTreeSet::new();
                deps.insert(resolved_canonical_id.clone());
                deps
            },
            requires_source_merge: !symbol_dependencies.is_empty(),
            body_hydrated: false,
            symbol_dependencies,
        };

        Some((resolved_canonical_id, resolved_exported_name, cached))
    }

    fn resolve_shallow_symbol_dependency_alias_with_visiting_in_view(
        &self,
        canonical_id: &str,
        exported_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<(
        String,
        String,
        crate::resolver_core::CachedPreparedImportedTypeAlias,
    )> {
        let (resolved_canonical_id, resolved_exported_name, cached) = self
            .resolve_prepared_symbol_dependency_alias_in_view(
                canonical_id,
                exported_name,
                store_view,
            )?;
        let visit_key = (
            resolved_canonical_id.clone(),
            resolved_exported_name.clone(),
        );
        let inserted_visit = visiting.insert(visit_key.clone());

        let result = {
            let mut hydrated = cached.clone();
            hydrated.body_hydrated = true;

            Some((resolved_canonical_id, resolved_exported_name, hydrated))
        };

        if inserted_visit {
            visiting.remove(&visit_key);
        }

        result
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn materialize_imported_runtime_values_into_env_in_view(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        owner_local_value_names: &rustc_hash::FxHashSet<String>,
        required_runtime_value_names: Option<&rustc_hash::FxHashSet<String>>,
        env: &mut verter_semantic::analysis::type_eval::EvalEnv,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) {
        let _trace = component_meta_trace_scope!(
            "materialize_runtime_values",
            format!(
                "imports={} owner_local_values={} existing_value_symbols={} store_view={}",
                snapshot.imports.len(),
                owner_local_value_names.len(),
                env.value_symbols.len(),
                store_view.is_some(),
            ),
        );
        let started = component_meta_debug_enabled().then(Instant::now);
        let resolver = HostRuntimeValueResolver {
            host: self,
            store_view,
        };
        materialize_imported_runtime_values_into_env(
            canonical_id,
            snapshot.imports.as_slice(),
            owner_local_value_names,
            required_runtime_value_names,
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
        component_meta_trace_event!(
            "materialize_runtime_values_result",
            format!(
                "imports={} owner_local_values={} value_symbols={}",
                snapshot.imports.len(),
                owner_local_value_names.len(),
                env.value_symbols.len(),
            ),
        );
    }

    fn build_generic_child_prop_overrides(
        &self,
        snapshot: &FileAnalysisSnapshot,
        usage_index: u32,
        eval_env: &mut Option<verter_semantic::analysis::type_eval::EvalEnv>,
    ) -> Option<rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>> {
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
        base: &verter_semantic::analysis::component_meta::ConsumedRootBindings,
        has_unknown_spread: bool,
        eval_env: &mut Option<verter_semantic::analysis::type_eval::EvalEnv>,
    ) -> ResolvedConsumedBindings {
        use verter_semantic::analysis::component_meta::PartialBranchReason;

        let mut resolved = ResolvedConsumedBindings {
            bindings: verter_semantic::analysis::component_meta::ConsumedRootBindings {
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
                    verter_semantic::analysis::type_eval_build::evaluate_value_expression(
                        expression, env,
                    )
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
        eval_env: &mut Option<verter_semantic::analysis::type_eval::EvalEnv>,
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
            verter_semantic::analysis::type_eval_build::parse_value_expression_type(&expression)
        {
            candidates.extend(collect_dynamic_root_candidates_from_type(
                &lowered,
                snapshot.imports.as_slice(),
            ));
        }
        if let Some(env) = eval_env.as_mut() {
            if let Some(evaluated) =
                verter_semantic::analysis::type_eval_build::evaluate_value_expression(
                    &expression,
                    env,
                )
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
            &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
        >,
        result: &crate::types::FallthroughResolution,
    ) {
        let cache_key = fallthrough_cache_key(
            canonical_id,
            self.config.generic_root_propagation,
            prop_type_overrides,
        );
        let resolution = Arc::new(result.clone());
        self.resolver_runtime().fallthrough.store_node(
            crate::resolver_core::fallthrough_resolver::root_follow_key(
                canonical_id,
                prop_type_overrides
                    .map(crate::resolver_core::hash_prop_type_overrides)
                    .unwrap_or_default(),
                self.config.generic_root_propagation,
            ),
            self.build_runtime_root_follow_node(result),
        );
        self.resolver_runtime()
            .fallthrough
            .store_node(cache_key, self.build_runtime_fallthrough_node(result));
        if prop_type_overrides.is_none() {
            self.mirror_cached_fallthrough_arc(canonical_id, resolution);
        }
    }

    fn extract_runtime_branch_results(
        result: &crate::types::FallthroughResolution,
    ) -> Vec<crate::resolver_core::fallthrough_resolver::FallthroughBranchResult> {
        match &result.fallthrough_surface {
            verter_semantic::analysis::component_meta::FallthroughSurface::Branches { branches } => branches
                .iter()
                .map(
                    |branch| crate::resolver_core::fallthrough_resolver::FallthroughBranchResult {
                        branch_key: branch.branch_key.clone(),
                        inherited_prop_names: branch
                            .props
                            .iter()
                            .map(|prop| prop.name.clone())
                            .collect(),
                        inherited_event_names: branch
                            .events
                            .iter()
                            .map(|event| event.name.clone())
                            .collect(),
                        resolved: !matches!(
                            branch.status,
                            verter_semantic::analysis::component_meta::BranchStatus::Unresolved { .. }
                        ),
                    },
                )
                .collect(),
            verter_semantic::analysis::component_meta::FallthroughSurface::None { .. } => Vec::new(),
        }
    }

    fn build_runtime_fallthrough_node(
        &self,
        result: &crate::types::FallthroughResolution,
    ) -> crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
        let branches = Self::extract_runtime_branch_results(result);
        crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
            value: crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::BranchUnion(
                crate::resolver_core::fallthrough_resolver::BranchUnionResult {
                    accepted_props: result.accepted_props.clone(),
                    accepted_events: result.accepted_events.clone(),
                    accepted_surface_completeness: result.accepted_surface_completeness,
                    fallthrough_surface: result.fallthrough_surface.clone(),
                    all_resolved: matches!(
                        result.accepted_surface_completeness,
                        verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::Exact
                    ),
                    branches,
                },
            ),
            facts: result.fact_versions.clone(),
            diagnostics: Vec::new(),
        }
    }

    fn build_runtime_root_follow_node(
        &self,
        result: &crate::types::FallthroughResolution,
    ) -> crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
        let branches = Self::extract_runtime_branch_results(result);
        crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
            value: crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::RootFollow(
                crate::resolver_core::fallthrough_resolver::RootFollowResult {
                    accepted_props: result.accepted_props.clone(),
                    accepted_events: result.accepted_events.clone(),
                    accepted_surface_completeness: result.accepted_surface_completeness,
                    fallthrough_surface: result.fallthrough_surface.clone(),
                    has_single_root: matches!(
                        result.fallthrough_surface,
                        verter_semantic::analysis::component_meta::FallthroughSurface::Branches { ref branches } if branches.len() == 1
                    ),
                    branches,
                },
            ),
            facts: result.fact_versions.clone(),
            diagnostics: Vec::new(),
        }
    }

    fn build_runtime_child_surface_node(
        &self,
        result: &crate::types::FallthroughResolution,
    ) -> crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
        crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
            value: crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::ChildSurfaceFollow(
                crate::resolver_core::fallthrough_resolver::ChildSurfaceResult {
                    accepted_props: result.accepted_props.clone(),
                    accepted_events: result.accepted_events.clone(),
                    accepted_surface_completeness: result.accepted_surface_completeness,
                    fallthrough_surface: result.fallthrough_surface.clone(),
                    inherited_prop_names: result
                        .accepted_props
                        .iter()
                        .map(|prop| prop.name.clone())
                        .collect(),
                    inherited_event_names: result
                        .accepted_events
                        .iter()
                        .map(|event| event.name.clone())
                        .collect(),
                    resolved: matches!(
                        result.accepted_surface_completeness,
                        verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::Exact
                    ),
                },
            ),
            facts: result.fact_versions.clone(),
            diagnostics: Vec::new(),
        }
    }

    fn build_runtime_intrinsic_surface_node(
        &self,
        members: &[verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember],
    ) -> crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
        let mut attr_names = Vec::new();
        let mut event_names = Vec::new();
        for member in members {
            match member.kind {
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr => {
                    attr_names.push(member.name.clone());
                }
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener => {
                    event_names.push(member.name.clone());
                }
            }
        }

        crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
            value:
                crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::IntrinsicSurface(
                    crate::resolver_core::fallthrough_resolver::IntrinsicSurfaceResult {
                        members: members.to_vec(),
                        attr_names,
                        event_names,
                    },
                ),
            facts: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn build_runtime_consumed_bindings_node(
        &self,
        resolved: &ResolvedConsumedBindings,
    ) -> crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
        let mut consumed_names = resolved.bindings.attrs.clone();
        consumed_names.extend(resolved.bindings.listeners.iter().cloned());
        consumed_names.sort();
        consumed_names.dedup();

        crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
            value:
                crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::ConsumedBindings(
                    crate::resolver_core::fallthrough_resolver::ConsumedBindingsResult {
                        attrs: resolved.bindings.attrs.clone(),
                        listeners: resolved.bindings.listeners.clone(),
                        has_dynamic_attr_name: resolved.bindings.has_dynamic_attr_name,
                        has_dynamic_listener_name: resolved.bindings.has_dynamic_listener_name,
                        partial_reasons: resolved.partial_reasons.clone(),
                        consumed_names,
                    },
                ),
            facts: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn runtime_child_node_to_resolution(
        &self,
        node: crate::resolver_core::fallthrough_resolver::FallthroughNodeResult,
    ) -> Option<crate::types::FallthroughResolution> {
        match node.value {
            crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::ChildSurfaceFollow(
                child,
            ) => Some(crate::types::FallthroughResolution {
                accepted_props: child.accepted_props,
                accepted_events: child.accepted_events,
                accepted_surface_completeness: child.accepted_surface_completeness,
                fallthrough_surface: child.fallthrough_surface,
                fact_versions: node.facts,
            }),
            _ => None,
        }
    }

    fn runtime_branch_union_node_to_resolution(
        &self,
        node: crate::resolver_core::fallthrough_resolver::FallthroughNodeResult,
    ) -> Option<crate::types::FallthroughResolution> {
        match node.value {
            crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::BranchUnion(
                branch_union,
            ) => Some(crate::types::FallthroughResolution {
                accepted_props: branch_union.accepted_props,
                accepted_events: branch_union.accepted_events,
                accepted_surface_completeness: branch_union.accepted_surface_completeness,
                fallthrough_surface: branch_union.fallthrough_surface,
                fact_versions: node.facts,
            }),
            _ => None,
        }
    }

    fn runtime_root_follow_node_to_resolution(
        &self,
        node: crate::resolver_core::fallthrough_resolver::FallthroughNodeResult,
    ) -> Option<crate::types::FallthroughResolution> {
        match node.value {
            crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::RootFollow(
                root_follow,
            ) => Some(crate::types::FallthroughResolution {
                accepted_props: root_follow.accepted_props,
                accepted_events: root_follow.accepted_events,
                accepted_surface_completeness: root_follow.accepted_surface_completeness,
                fallthrough_surface: root_follow.fallthrough_surface,
                fact_versions: node.facts,
            }),
            _ => None,
        }
    }

    fn runtime_intrinsic_node_to_members(
        &self,
        node: crate::resolver_core::fallthrough_resolver::FallthroughNodeResult,
    ) -> Option<Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember>> {
        match node.value {
            crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::IntrinsicSurface(
                intrinsic,
            ) => Some(intrinsic.members),
            _ => None,
        }
    }

    fn runtime_consumed_bindings_to_resolution(
        &self,
        node: crate::resolver_core::fallthrough_resolver::FallthroughNodeResult,
    ) -> Option<ResolvedConsumedBindings> {
        match node.value {
            crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::ConsumedBindings(
                consumed,
            ) => Some(ResolvedConsumedBindings {
                bindings: verter_semantic::analysis::component_meta::ConsumedRootBindings {
                    attrs: consumed.attrs,
                    listeners: consumed.listeners,
                    has_dynamic_attr_name: consumed.has_dynamic_attr_name,
                    has_dynamic_listener_name: consumed.has_dynamic_listener_name,
                },
                partial_reasons: consumed.partial_reasons,
            }),
            _ => None,
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
        cached_parse: Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
        src_blocks: &[crate::SrcBlockInfo],
        external_requests: &[crate::ExternalSourceRequest],
        imports: &[verter_semantic::analysis::AnalyzedImport],
        macros: &[verter_semantic::analysis::AnalyzedMacro],
        bindings: &[verter_semantic::analysis::AnalyzedBinding],
    ) -> Option<Arc<verter_semantic::analysis::template::TemplateAnalysisSnapshot>> {
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
                    std::borrow::Cow::Owned(verter_compiler::compile::parse_sfc(
                        &merged_source,
                        None,
                        None,
                    ))
                })
        } else {
            std::borrow::Cow::Owned(verter_compiler::compile::parse_sfc(
                &merged_source,
                None,
                None,
            ))
        };

        let alloc = oxc_allocator::Allocator::new();
        let options = verter_compiler::compile::CodegenOptions {
            target: verter_compiler::compile::CompileTarget::META,
            filename: Some(canonical.to_string()),
            ..verter_compiler::compile::CodegenOptions::default()
        };
        let verter_opts = verter_compiler::compile::VerterCompileOptions {
            extract_template_data: true,
            ..verter_compiler::compile::VerterCompileOptions::default()
        };
        let compiled = verter_compiler::compile::compile_from_parsed(
            &merged_source,
            &parsed,
            &options,
            &verter_opts,
            &alloc,
        );

        let has_structural_errors = compiled.errors.iter().any(|d| {
            matches!(
                d.severity,
                verter_compiler::compile::CompileDiagnosticSeverity::Error
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
    /// `compile_from_parsed()` â€” bypassing the host `compile_entry()` which fails
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
                    std::borrow::Cow::Owned(verter_compiler::compile::parse_sfc(
                        &merged_source,
                        None,
                        None,
                    ))
                })
        } else {
            std::borrow::Cow::Owned(verter_compiler::compile::parse_sfc(
                &merged_source,
                None,
                None,
            ))
        };

        // Compile with META target â€” script codegen + template data, no JS/TSX output
        let alloc = oxc_allocator::Allocator::new();
        let options = verter_compiler::compile::CodegenOptions {
            target: verter_compiler::compile::CompileTarget::META,
            filename: Some(canonical.to_string()),
            ..verter_compiler::compile::CodegenOptions::default()
        };
        let verter_opts = verter_compiler::compile::VerterCompileOptions {
            extract_template_data: true,
            ..verter_compiler::compile::VerterCompileOptions::default()
        };
        let compiled = verter_compiler::compile::compile_from_parsed(
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
                verter_compiler::compile::CompileDiagnosticSeverity::Error
            ) && !d.code.starts_with("XInvalidMacroType")
                && !d.code.starts_with("XMissingMacroType")
        });
        if has_structural_errors {
            return;
        }

        // Convert RawTemplateData â†’ TemplateAnalysisSnapshot using existing converter
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
                .find(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps);
            if let Some(dp) = define_props {
                for field in &dp.prop_fields {
                    if let Some(type_ann) = &field.type_annotation {
                        let classes =
                            verter_semantic::analysis::parse_string_literal_union(type_ann);
                        if !classes.is_empty() {
                            unions.push((field.name.clone(), classes));
                        }
                    }
                }
            }
            for binding in &snapshot.bindings {
                if let Some(type_ann) = &binding.type_annotation {
                    let effective = verter_semantic::analysis::unwrap_reactive_type(type_ann)
                        .unwrap_or(type_ann);
                    let classes = verter_semantic::analysis::parse_string_literal_union(effective);
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
            if let Some(snap) = self.scheduler.try_get_source(canonical) {
                let hd = snap.downcast_data::<HostSourceData>()?;
                return Some(hd.parse.whole_hash);
            }
            let workspace_generation = self.ws().content_generation();
            self.imported_dependency_cache
                .lock()
                .get(canonical)
                .and_then(|entry| {
                    (entry.workspace_generation == workspace_generation).then_some(entry.whole_hash)
                })
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let whole_hash = files.get(canonical).map(|entry| entry.whole_hash);
            drop(files);
            let workspace_generation = self.ws().content_generation();
            whole_hash.or_else(|| {
                self.imported_dependency_cache
                    .lock()
                    .get(canonical)
                    .and_then(|entry| {
                        (entry.workspace_generation == workspace_generation)
                            .then_some(entry.whole_hash)
                    })
            })
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
            // Arc::clone â€” cheap pointer bump, no deep copy
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
        if let Some(source) = self.read_analysis_source(resolved_canonical_id) {
            return Some(source);
        }

        let dep_id = self
            .resolve_loaded_dependency_canonical(
                owner_canonical,
                specifier,
                verter_workspace::ResolveRequestKind::SfcSrcAttr,
            )
            .or_else(|| {
                self.resolve_loaded_dependency_canonical(
                    owner_canonical,
                    specifier,
                    verter_workspace::ResolveRequestKind::EsmImport,
                )
            })?;

        self.read_analysis_source(&dep_id)
    }

    /// Populate `resolved_canonical_id` on each import in the snapshot
    /// using the host's file map, alias map, and parent's dependency set.
    pub(crate) fn resolve_snapshot_imports(
        &self,
        parent_canonical_id: &str,
        snapshot: &mut FileAnalysisSnapshot,
    ) {
        self.resolve_snapshot_imports_in_view(parent_canonical_id, snapshot, None);
    }

    pub(crate) fn resolve_snapshot_imports_in_view(
        &self,
        parent_canonical_id: &str,
        snapshot: &mut FileAnalysisSnapshot,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) {
        for import in &mut snapshot.imports {
            if import.resolved_canonical_id.is_none() {
                import.resolved_canonical_id = if let Some(view) = store_view {
                    view.dependency_resolution(parent_canonical_id, &import.source)
                        .and_then(|resolution| {
                            resolution
                                .resolved_canonical_id
                                .clone()
                                .or_else(|| resolution.effective_target().map(str::to_string))
                        })
                } else {
                    let ctx = verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: if import.is_type_only {
                            verter_workspace::ResolveRequestKind::TypeImport
                        } else {
                            verter_workspace::ResolveRequestKind::EsmImport
                        },
                    };
                    self.resolve_via_vfs(parent_canonical_id, &import.source, ctx)
                };
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
        use verter_semantic::analysis::types::{
            BindingInitializer, ComposableReturn, ReactivityKind,
        };

        // Build a map of import source â†’ resolved canonical ID from the snapshot
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
            cc.cached_meta_payloads.clear();
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical) {
                entry.compile_slots.clear();
                entry.cached_resolved_meta.clear();
                entry.cached_meta_payloads.clear();
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
                    cc.cached_meta_payloads.clear();
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
                        file.cached_meta_payloads.clear();
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

        // Runtime codegen honors caller-provided targets directly. Type-preferring
        // contexts must either get a declaration-safe target or fall back to the
        // resolver instead of being pinned to runtime JS/CJS package entrypoints.
        let mut vfs_resolutions = Vec::new();
        for resolution in &resolutions {
            let resolved = resolution.resolved_canonical_id.as_ref().map(|id| {
                let norm = canonicalize_id(id);
                norm.into_owned()
            });
            let possible: Vec<String> = resolution
                .possible_canonical_ids
                .iter()
                .map(|candidate| {
                    let norm = canonicalize_id(candidate);
                    norm.into_owned()
                })
                .collect();
            let normalized_resolution = DependencyResolution {
                specifier: resolution.specifier.clone(),
                resolved_canonical_id: resolved.clone(),
                possible_canonical_ids: possible.clone(),
            };
            let mut exact_summaries = Vec::new();

            use verter_workspace::{ResolvePhase as P, ResolveRequestKind as K};
            for (phase, kind) in [
                (P::CodegenBlocker, K::EsmImport),
                (P::CodegenBlocker, K::TypeImport),
                (P::ProviderGraph, K::EsmImport),
                (P::ProviderGraph, K::TypeImport),
            ] {
                let exact = if exact_resolution_uses_type_preferred_target(phase, kind) {
                    self.derive_type_preferred_exact_target(&normalized_resolution)
                        .map(|target| verter_workspace::ExactResolution {
                            specifier: resolution.specifier.clone(),
                            phase,
                            kind,
                            resolved_canonical_id: Some(target),
                            possible_canonical_ids: Vec::new(),
                        })
                } else {
                    Some(verter_workspace::ExactResolution {
                        specifier: resolution.specifier.clone(),
                        phase,
                        kind,
                        resolved_canonical_id: resolved.clone(),
                        possible_canonical_ids: possible.clone(),
                    })
                };
                if let Some(exact) = exact {
                    exact_summaries.push(format!(
                        "{phase:?}/{kind:?}->{:?}",
                        exact
                            .resolved_canonical_id
                            .as_deref()
                            .or_else(|| exact.possible_canonical_ids.first().map(String::as_str))
                    ));
                    vfs_resolutions.push(exact);
                } else {
                    exact_summaries.push(format!("{phase:?}/{kind:?}-><resolver>"));
                }
            }
            if component_meta_debug_enabled() {
                component_meta_debug(format!(
                    "set_import_dependencies owner={} specifier={} resolved={:?} possible=[{}] exacts=[{}]",
                    canonical,
                    resolution.specifier,
                    normalized_resolution.resolved_canonical_id,
                    normalized_resolution.possible_canonical_ids.join(", "),
                    exact_summaries.join("; "),
                ));
            }
        }

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
    ) -> Option<Arc<verter_semantic::analysis::template::TemplateAnalysisSnapshot>> {
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
    ) -> Option<Arc<verter_semantic::analysis::template::TemplateAnalysisSnapshot>> {
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
    ) -> verter_semantic::analysis::CssVarFlow {
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

        let mut flow = verter_semantic::analysis::CssVarFlow {
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

    pub(crate) fn load_export_graph_analysis_in_view(
        &self,
        canonical_or_alias: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<(
        FileKind,
        verter_semantic::analysis::ScriptAnalysisSnapshot,
        Vec<verter_semantic::analysis::ExportSignature>,
    )> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return None;
                }
            }

            let current_hash = self.get_whole_hash(&canonical).unwrap_or_default();
            if self.store_view_allows_current_whole_hash(&canonical, current_hash, store_view) {
                if let (Some(source_snap), Some(analysis_snap)) = (
                    self.scheduler.try_get_source(&canonical),
                    self.scheduler.try_get_analysis(&canonical),
                ) {
                    let file_kind = source_snap
                        .downcast_data::<crate::host_executor::HostSourceData>()?
                        .file_kind;
                    let analysis =
                        analysis_snap.downcast_data::<crate::host_executor::HostAnalysisData>()?;
                    return Some((
                        file_kind,
                        analysis.script_analysis.clone(),
                        analysis.export_signatures.clone(),
                    ));
                }
            }
        }

        if let Some(entry) =
            self.ensure_shallow_imported_export_state_in_view(&canonical, store_view)
        {
            if let (Some(script_analysis), Some(export_signatures)) = (
                entry.script_analysis.as_ref(),
                entry.export_signatures.as_ref(),
            ) {
                return Some((
                    if canonical.ends_with(".vue") {
                        FileKind::VueSfc
                    } else {
                        FileKind::NonSfc
                    },
                    script_analysis.as_ref().clone(),
                    export_signatures.as_ref().clone(),
                ));
            }
        }
        None
    }

    /// Look up the byte span of an exported name in a target file.
    ///
    /// For `.vue` files: searches `ScriptAnalysisSnapshot.bindings` (script-setup
    /// auto-exports) â€” spans are SFC-absolute.
    /// For `.ts`/`.js` files: searches `FileEntry.export_signatures` â€” spans are
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
        script_analysis: &verter_semantic::analysis::ScriptAnalysisSnapshot,
        export_signatures: &[verter_semantic::analysis::ExportSignature],
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
            if sig.reexport_source.is_some() {
                return None;
            }
            let span = sig.local_span.unwrap_or(sig.span);
            if span.start > 0 || span.end > 0 {
                return Some((span.start, span.end));
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
        let ctx = verter_workspace::ResolutionContext {
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
        };
        self.resolve_loaded_dependency_canonical(&canonical_parent, import_source, ctx.kind)
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

    pub(crate) fn resolve_named_export_in_view(
        &self,
        canonical_or_alias: &str,
        binding_name: &str,
        is_type: Option<bool>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<ResolvedExport> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        #[cfg(feature = "scheduler")]
        if let Some(cc) = self.compile_cache.get(&canonical) {
            if cc.evicted {
                return None;
            }
        }
        let resolver = HostExportGraphResolver {
            host: self,
            store_view,
        };
        let is_declaration_file = canonical.ends_with(".d.ts")
            || canonical.ends_with(".d.mts")
            || canonical.ends_with(".d.cts");
        let resolved = resolver_resolve_named_export_from_graph(
            &resolver,
            &canonical,
            binding_name,
            is_type,
            store_view.is_some(),
        )
        .or_else(|| {
            if is_type == Some(true) && is_declaration_file {
                resolver_resolve_named_export_from_graph(
                    &resolver,
                    &canonical,
                    binding_name,
                    None,
                    store_view.is_some(),
                )
            } else {
                None
            }
        })?;
        Some(ResolvedExport {
            name: resolved.name,
            is_type: resolved.is_type,
            source_canonical_id: resolved.source_canonical_id,
            source_name: resolved.source_name,
        })
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

// Legacy TypeExpr walkers (collect_required_owner_import_names, collect_slot_eval_import_names_*,
// collect_surface_eval_import_names_*, collect_runtime_value_names_*, etc.) were deleted.
// The solver host now resolves cross-file types on demand through prepared-decl caches.

/// Collect the set of runtime value names referenced by the template.
/// This reads pre-analyzed snapshot data (binding_occurrences, prop.referenced_bindings),
/// NOT TypeExpr trees — it is not a walker.
fn collect_required_template_runtime_value_names(
    snapshot: &FileAnalysisSnapshot,
) -> rustc_hash::FxHashSet<String> {
    let mut required = rustc_hash::FxHashSet::default();
    let Some(template) = snapshot.template.as_ref() else {
        return required;
    };

    required.extend(
        template
            .binding_occurrences
            .iter()
            .map(|occurrence| occurrence.name.clone()),
    );

    for component in &template.components {
        for prop in &component.props {
            required.extend(prop.referenced_bindings.iter().cloned());
            if prop.is_shorthand {
                required.insert(prop.name.clone());
            }
        }
    }

    required
}

/// Extract slot bindings from a type_text that encodes a slot's function signature.
///
/// Handles property signature types like `(props: { row: Item; index: number }) => any`.
/// Extract slot bindings and return type from a type_text encoding a slot function signature.
///
/// Handles both arrow-style (`(props: { row: Item }) => VNode[]`) and
/// method-style (`(props: { row: Item }): VNode[]`) signatures.
/// Returns `(bindings, return_type)`.
/// Build a `ComponentMetaAnalysis` from a resolved-meta state.
/// Shared by `get_component_meta` and `get_component_meta_with_resolution`.
fn extract_component_meta_from_inputs(
    host: &VerterHost,
    canonical_or_alias: &str,
    snapshot: &FileAnalysisSnapshot,
    resolved_macros: &[verter_semantic::analysis::component_meta::ResolvedMacroInput],
    resolved_type_registry: &[verter_semantic::analysis::component_meta::ResolvedTypeAnalysis],
    evaluated_types: Option<&verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
) -> verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
    let started = component_meta_debug_enabled().then(Instant::now);
    let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
    let _trace = component_meta_trace_scope!(
        "extract_component_meta",
        format!(
            "owner={} macros={} resolved_macros={} resolved_type_registry={} has_evaluated_types={}",
            canonical,
            snapshot.macros.len(),
            resolved_macros.len(),
            resolved_type_registry.len(),
            evaluated_types.is_some(),
        ),
    );
    let input = verter_semantic::analysis::component_meta::ComponentMetaInput {
        macros: &snapshot.macros,
        bindings: &snapshot.bindings,
        imports: &snapshot.imports,
        template: snapshot.template.as_deref(),
        options_api: snapshot.options_api.as_ref(),
        analysis_flags: verter_semantic::analysis::types::AnalysisFlags::from_bits_truncate(
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
    let mut meta = verter_semantic::analysis::component_meta::extract_component_meta(input);
    component_meta_trace_event!(
        "extract_component_meta_declared_surface",
        format!(
            "owner={} props={} events={} slots={}",
            canonical,
            meta.props.len(),
            meta.events.len(),
            meta.slots.len(),
        ),
    );

    if let Some(started) = started {
        component_meta_debug(format!(
            "extract_component_meta owner={} took {:?}",
            canonical,
            started.elapsed(),
        ));
    }

    populate_public_instance_sidecar(&mut meta);
    populate_sfc_blocks_sidecar(host, &canonical, &mut meta);
    meta
}

fn parse_annotation_or_unknown_for_public_instance(
    raw: &str,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    let parsed = verter_semantic::analysis::type_expr_lower::parse_type_annotation(raw);
    if parsed.is_unknown() {
        verter_semantic::analysis::type_expr::TypeExpr::Unknown {
            raw: raw.to_string(),
        }
    } else {
        parsed
    }
}

fn build_public_instance_slot_type(
    slot: &verter_semantic::analysis::component_meta::SlotAnalysis,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    let parameter_type = verter_semantic::analysis::type_expr::TypeExpr::Object(Arc::new(
        verter_semantic::analysis::type_expr::ObjectExpr {
            properties: slot
                .bindings
                .iter()
                .map(|binding| {
                    verter_semantic::analysis::type_expr::ObjectMember::Property(
                        verter_semantic::analysis::type_expr::ObjectProperty {
                            name: binding.name.clone(),
                            ty: binding.type_expr.clone(),
                            optional: false,
                            readonly: false,
                        },
                    )
                })
                .collect(),
        },
    ));
    let return_type = slot
        .return_type
        .as_deref()
        .map(parse_annotation_or_unknown_for_public_instance)
        .unwrap_or(verter_semantic::analysis::type_expr::TypeExpr::Primitive(
            verter_semantic::analysis::type_expr::PrimitiveName::Unknown,
        ));
    let function = verter_semantic::analysis::type_expr::TypeExpr::Function(Arc::new(
        verter_semantic::analysis::type_expr::FunctionExpr {
            parameters: if slot.bindings.is_empty() {
                Vec::new()
            } else {
                vec![verter_semantic::analysis::type_expr::FunctionParam {
                    name: Some("props".to_string()),
                    ty: parameter_type,
                    optional: false,
                    rest: false,
                }]
            },
            return_type: Some(Arc::new(return_type)),
            type_parameters: Vec::new(),
        },
    ));
    if slot.is_required {
        function
    } else {
        verter_semantic::analysis::type_expr::TypeExpr::union(vec![
            function,
            verter_semantic::analysis::type_expr::TypeExpr::Primitive(
                verter_semantic::analysis::type_expr::PrimitiveName::Undefined,
            ),
        ])
    }
}

fn build_public_instance_slots_member(
    slots: &[verter_semantic::analysis::component_meta::SlotAnalysis],
) -> verter_semantic::analysis::component_meta::PublicInstanceMemberAnalysis {
    let slot_properties = slots
        .iter()
        .map(|slot| {
            verter_semantic::analysis::type_expr::ObjectMember::Property(
                verter_semantic::analysis::type_expr::ObjectProperty {
                    name: slot.name.clone(),
                    ty: build_public_instance_slot_type(slot),
                    optional: !slot.is_required,
                    readonly: false,
                },
            )
        })
        .collect();

    verter_semantic::analysis::component_meta::PublicInstanceMemberAnalysis {
        name: "$slots".to_string(),
        kind: verter_semantic::analysis::component_meta::PublicInstanceMemberKind::SlotContainer,
        type_expr: verter_semantic::analysis::type_expr::TypeExpr::Object(Arc::new(
            verter_semantic::analysis::type_expr::ObjectExpr {
                properties: slot_properties,
            },
        )),
        type_expansion: None,
        raw_type: None,
        description: None,
    }
}

fn string_from_span(source: &str, span: Option<verter_compiler::common::Span>) -> Option<String> {
    span.map(|span| source[span.start as usize..span.end as usize].to_string())
}

fn sfc_attributes_from_props(
    props: &[verter_compiler::types::NodeProp],
    source: &str,
) -> Vec<verter_semantic::analysis::component_meta::SfcAttributeAnalysis> {
    crate::parse::extract_attrs(props, source)
        .into_iter()
        .map(
            |(name, value)| verter_semantic::analysis::component_meta::SfcAttributeAnalysis {
                name: name.to_string(),
                value: if value.is_empty() {
                    None
                } else {
                    Some(value.to_string())
                },
            },
        )
        .collect()
}

fn sfc_custom_block_type(source: &str, tag_open: &verter_compiler::types::NodeTag) -> String {
    source[tag_open.start as usize + 1..tag_open.name_end as usize].to_string()
}

pub(crate) fn populate_sfc_blocks_sidecar(
    host: &VerterHost,
    canonical_id: &str,
    meta: &mut verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
) {
    if !canonical_id.ends_with(".vue") {
        return;
    }

    let Some((source, cached_parse, _)) = host.current_eval_state(canonical_id) else {
        return;
    };
    let Some(parsed) = cached_parse.as_deref() else {
        return;
    };
    let source = source.as_ref();

    let template = parsed.template_ast().map(|template| {
        let attrs = crate::parse::extract_attrs(&template.root.attributes, source);
        verter_semantic::analysis::component_meta::TemplateBlockAnalysis {
            lang: string_from_span(source, template.root.lang),
            src: crate::parse::find_attr(&attrs, "src"),
            attributes: sfc_attributes_from_props(&template.root.attributes, source),
        }
    });

    let script = parsed.script().map(|script| {
        let attrs = crate::parse::extract_attrs(&script.attributes, source);
        verter_semantic::analysis::component_meta::ScriptBlockAnalysis {
            lang: crate::parse::find_attr(&attrs, "lang").filter(|lang| lang != "true"),
            src: crate::parse::find_attr(&attrs, "src"),
            generic: string_from_span(source, script.generic),
            attrs_type: string_from_span(source, script.attrs),
            attributes: sfc_attributes_from_props(&script.attributes, source),
        }
    });

    let script_setup = parsed.script_setup().map(|script| {
        let attrs = crate::parse::extract_attrs(&script.attributes, source);
        verter_semantic::analysis::component_meta::ScriptBlockAnalysis {
            lang: crate::parse::find_attr(&attrs, "lang").filter(|lang| lang != "true"),
            src: crate::parse::find_attr(&attrs, "src"),
            generic: string_from_span(source, script.generic),
            attrs_type: string_from_span(source, script.attrs),
            attributes: sfc_attributes_from_props(&script.attributes, source),
        }
    });

    let styles = parsed
        .style_nodes()
        .iter()
        .enumerate()
        .map(|(index, style)| {
            let attrs = crate::parse::extract_attrs(&style.attributes, source);
            verter_semantic::analysis::component_meta::StyleBlockInfoAnalysis {
                index,
                lang: crate::parse::find_attr(&attrs, "lang").filter(|lang| lang != "true"),
                src: crate::parse::find_attr(&attrs, "src"),
                scoped: style.scoped,
                is_module: style.module,
                module_name: crate::parse::find_attr(&attrs, "module")
                    .filter(|value| value != "true"),
                attributes: sfc_attributes_from_props(&style.attributes, source),
            }
        })
        .collect();

    let custom = parsed
        .unknown_nodes()
        .iter()
        .enumerate()
        .map(|(index, block)| {
            let attrs = crate::parse::extract_attrs(&block.attributes, source);
            verter_semantic::analysis::component_meta::CustomBlockAnalysis {
                index,
                block_type: sfc_custom_block_type(source, &block.tag_open),
                lang: crate::parse::find_attr(&attrs, "lang").filter(|lang| lang != "true"),
                src: crate::parse::find_attr(&attrs, "src"),
                attributes: sfc_attributes_from_props(&block.attributes, source),
            }
        })
        .collect();

    meta.sfc_blocks = Some(
        verter_semantic::analysis::component_meta::SfcBlocksAnalysis {
            template,
            script,
            script_setup,
            styles,
            custom,
        },
    );
}

pub(crate) fn populate_public_instance_sidecar(
    meta: &mut verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
) {
    let mut members = Vec::new();

    if !meta.slots.is_empty() {
        members.push(build_public_instance_slots_member(&meta.slots));
    }

    members.extend(meta.props.iter().map(|prop| {
        verter_semantic::analysis::component_meta::PublicInstanceMemberAnalysis {
            name: prop.name.clone(),
            kind: verter_semantic::analysis::component_meta::PublicInstanceMemberKind::Prop,
            type_expr: prop.type_expr.clone(),
            type_expansion: prop.type_expansion.clone(),
            raw_type: prop.raw_type.clone(),
            description: prop.description.clone(),
        }
    }));

    for exposed in &meta.exposed {
        let next = verter_semantic::analysis::component_meta::PublicInstanceMemberAnalysis {
            name: exposed.name.clone(),
            kind: verter_semantic::analysis::component_meta::PublicInstanceMemberKind::Exposed,
            type_expr: exposed.type_expr.clone(),
            type_expansion: exposed.type_expansion.clone(),
            raw_type: None,
            description: exposed.description.clone(),
        };
        if let Some(existing) = members.iter_mut().find(|member| member.name == next.name) {
            *existing = next;
        } else {
            members.push(next);
        }
    }

    meta.public_instance = if members.is_empty() {
        None
    } else {
        Some(
            verter_semantic::analysis::component_meta::PublicInstanceAnalysis {
                members,
                completeness:
                    verter_semantic::analysis::component_meta::PublicInstanceCompleteness::Partial,
            },
        )
    };
}

pub(crate) fn extract_component_meta_from_resolved(
    host: &VerterHost,
    canonical_or_alias: &str,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
    include_fallthrough: bool,
    store_view: Option<&HostStoreView>,
) -> verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
    let resolved_macros = resolver_component_meta_resolved_macros(
        resolved.snapshot.macros.as_ref(),
        &resolved.resolved_macros,
    );
    let resolved_type_registry =
        resolver_component_meta_type_registry(&resolved.resolved_type_registry);
    let mut meta = extract_component_meta_from_inputs(
        host,
        canonical_or_alias,
        &resolved.snapshot,
        &resolved_macros,
        &resolved_type_registry,
        resolved.evaluated_types.as_ref(),
    );
    if include_fallthrough {
        let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
        let mut visiting = rustc_hash::FxHashSet::default();
        if let Some(resolution) = host.compute_fallthrough_surface_from_resolved_state(
            &canonical,
            resolved,
            None,
            &mut visiting,
            store_view,
        ) {
            meta.accepted_props = resolution.accepted_props;
            meta.accepted_events = resolution.accepted_events;
            meta.accepted_surface_completeness = resolution.accepted_surface_completeness;
            meta.fallthrough_surface = resolution.fallthrough_surface;
        }
    }
    meta
}

/// Like [`extract_component_meta_from_resolved`] with `include_fallthrough=true`,
/// but also returns the fallthrough resolution's fact versions (if available).
/// Used by the payload cache to store Full payloads with the correct fact set.
pub(crate) fn extract_component_meta_from_resolved_with_facts(
    host: &VerterHost,
    canonical_or_alias: &str,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
    store_view: Option<&HostStoreView>,
) -> (
    verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    Option<Vec<crate::resolver_core::FactVersionRef>>,
) {
    let resolved_macros = resolver_component_meta_resolved_macros(
        resolved.snapshot.macros.as_ref(),
        &resolved.resolved_macros,
    );
    let resolved_type_registry =
        resolver_component_meta_type_registry(&resolved.resolved_type_registry);
    let mut meta = extract_component_meta_from_inputs(
        host,
        canonical_or_alias,
        &resolved.snapshot,
        &resolved_macros,
        &resolved_type_registry,
        resolved.evaluated_types.as_ref(),
    );
    let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
    let mut visiting = rustc_hash::FxHashSet::default();
    let fallthrough_facts = if let Some(resolution) = host
        .compute_fallthrough_surface_from_resolved_state(
            &canonical,
            resolved,
            None,
            &mut visiting,
            store_view,
        ) {
        let facts = resolution.fact_versions.clone();
        meta.accepted_props = resolution.accepted_props;
        meta.accepted_events = resolution.accepted_events;
        meta.accepted_surface_completeness = resolution.accepted_surface_completeness;
        meta.fallthrough_surface = resolution.fallthrough_surface;
        Some(facts)
    } else {
        None
    };
    (meta, fallthrough_facts)
}

#[cfg(test)]
#[path = "host_manage_tests.rs"]
mod tests;

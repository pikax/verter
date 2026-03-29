//! `impl VerterHost` — file management, analysis, and diagnostics methods.
//!
//! Contains [`VerterHost::remove`], [`VerterHost::get_analysis`],
//! [`VerterHost::get_diagnostics`], and [`VerterHost::set_import_dependencies`].

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::hash::compile_profile_hash;
use crate::id::canonicalize_id;
use crate::resolver_store::HostStoreView;
use crate::shared::{read_lock, write_lock};
use crate::types::*;
use crate::VerterHost;
use verter_resolver::{
    build_imported_eval_inputs, build_imported_eval_inputs_with_owner_context,
    build_owner_eval_env_with_inputs, collect_dynamic_root_candidates_from_type,
    component_meta_resolved_macros as resolver_component_meta_resolved_macros,
    component_meta_type_registry as resolver_component_meta_type_registry,
    evaluate_imported_decl_with_owner_env as resolver_evaluate_imported_decl_with_owner_env,
    fallthrough_cache_key,
    get_export_span_follow_reexports_from_graph as resolver_get_export_span_follow_reexports_from_graph,
    known_spread_keys_from_type_expr, materialize_imported_runtime_values_into_env,
    push_partial_reason, resolve_exports_from_graph as resolver_resolve_exports_from_graph,
    resolve_exports_from_graph_best_effort as resolver_resolve_exports_from_graph_best_effort,
    resolve_fallthrough_surface as resolver_resolve_fallthrough_surface,
    resolve_named_export_from_graph as resolver_resolve_named_export_from_graph,
    resolve_usage_prop_type, CollectedImportedTypeAlias, DeclarationMetadataResolver,
    DynamicRootCandidate, ExportGraphFileKind, ExportGraphResolver, ExportSurface,
    FallthroughComputeHost, FallthroughRequestHost, FallthroughResolutionView,
    FallthroughResolverHost, ImportedDeclEvalResolver, ImportedEvalBinding,
    ImportedEvalCollectorResolver, ImportedEvalLookup, ImportedEvalLookupResolver,
    ImportedEvalOwnerContextResolver, ImportedEvalOwnerResolver, ImportedEvalOwnerSnapshot,
    ImportedEvalSourceMergeResolver, ImportedEvalTraversalBudget, ImportedRuntimeValueResolver,
    ImportedSymbolDependency, ImportedTypeAliasPrepareError, ImportedTypeAliasResolveRequest,
    ImportedTypeAliasResolver, OwnerEvalEnvAssembler, PreparedImportedDeclContext, RequestSource,
    ResolvedConsumedBindings, ResolvedExportTarget, SingleflightRole, StoreView,
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
    type_context: Option<Rc<crate::ParsedTypeResolutionContext>>,
}

thread_local! {
    static HOST_PARSED_EVAL_PROGRAM_CACHE: RefCell<
        rustc_hash::FxHashMap<ParsedEvalProgramCacheKey, ParsedEvalProgramCacheEntry>
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
const COMPONENT_META_RETRY_MAX_SYMBOLIC_STEPS: usize = 50_000;
const COMPONENT_META_MAX_IMPORTED_TYPE_ROOTS: usize = 2_000;
const STORE_VIEW_STABILITY_MAX_ATTEMPTS: usize = 3;

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
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
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

        let root_follow_key = verter_resolver::fallthrough_resolver::root_follow_key(
            canonical_id,
            prop_type_overrides
                .map(verter_resolver::hash_prop_type_overrides)
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
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
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
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
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
    ) -> Vec<verter_analysis::html_intrinsics::OwnedIntrinsicMember> {
        debug_assert_eq!(self.parent_canonical_id, canonical_id);
        let (project_anchor, cache_generation) =
            self.host.project_intrinsic_cache_anchor(canonical_id);
        let cache_key = verter_resolver::fallthrough_resolver::intrinsic_surface_key(
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
            verter_vfs::ResolveRequestKind::EsmImport,
        )
    }

    fn current_dependency_fact_versions(
        &self,
        canonical_id: &str,
    ) -> Vec<verter_resolver::FactVersionRef> {
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
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Option<Self::ChildResolution> {
        let cache_key = verter_resolver::fallthrough_resolver::child_surface_key(
            canonical_id,
            prop_type_overrides
                .map(verter_resolver::hash_prop_type_overrides)
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
    type EvalEnv = verter_analysis::type_eval::EvalEnv;

    fn resolve_root_consumption(
        &self,
        canonical_id: &str,
        branch_key: &str,
        snapshot: &Self::Snapshot,
        element_index: u32,
        base: &verter_analysis::component_meta::ConsumedRootBindings,
        has_unknown_spread: bool,
        eval_env: &mut Option<Self::EvalEnv>,
    ) -> ResolvedConsumedBindings {
        let cache_key =
            verter_resolver::fallthrough_resolver::consumed_bindings_key(canonical_id, branch_key);

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

fn component_meta_expansion_budget_with_max_symbolic_work(
    max_symbolic_work: usize,
) -> verter_analysis::type_expand::ExpansionBudget {
    verter_analysis::type_expand::ExpansionBudget {
        max_symbolic_work,
        ..Default::default()
    }
}

fn component_meta_expansion_budget() -> verter_analysis::type_expand::ExpansionBudget {
    component_meta_expansion_budget_with_max_symbolic_work(COMPONENT_META_MAX_SYMBOLIC_STEPS)
}

fn expanded_component_types_hit_symbolic_budget(
    types: &verter_analysis::type_expand::ExpandedComponentTypes,
) -> bool {
    use verter_analysis::type_expand::ExpansionStopReason;

    let field_has_budget = |field: &verter_analysis::type_expand::ExpandedField| {
        field
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.reason == ExpansionStopReason::BudgetExceeded)
    };
    let macro_has_budget = |shape: &verter_analysis::type_expand::ExpandedMacroObjectShape| {
        shape
            .result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.reason == ExpansionStopReason::BudgetExceeded)
    };
    let props_has_budget = |shape: &verter_analysis::type_expand::ExpandedMacroProps| {
        shape
            .result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.reason == ExpansionStopReason::BudgetExceeded)
    };

    types.props.iter().any(field_has_budget)
        || types.emits.iter().any(field_has_budget)
        || types.slot_bindings.iter().any(field_has_budget)
        || types.bindings.iter().any(field_has_budget)
        || types.define_props.iter().any(props_has_budget)
        || types.define_emits.iter().any(macro_has_budget)
        || types.define_slots.iter().any(macro_has_budget)
}

fn computed_evaluated_types_hit_symbolic_budget(computed: &ComputedEvaluatedTypes) -> bool {
    computed
        .evaluated_types
        .as_ref()
        .is_some_and(expanded_component_types_hit_symbolic_budget)
}

fn expanded_component_types_have_component_surface(
    types: &verter_analysis::type_expand::ExpandedComponentTypes,
) -> bool {
    !types.props.is_empty()
        || !types.define_props.is_empty()
        || !types.define_emits.is_empty()
        || !types.emits.is_empty()
        || !types.define_slots.is_empty()
        || !types.slot_bindings.is_empty()
}

fn should_retry_component_meta_expansion(computed: &ComputedEvaluatedTypes) -> bool {
    if !computed_evaluated_types_hit_symbolic_budget(computed) {
        return false;
    }

    computed
        .evaluated_types
        .as_ref()
        .is_none_or(|types| !expanded_component_types_have_component_surface(types))
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
    dep_resolutions: std::borrow::Cow<'a, rustc_hash::FxHashMap<String, DependencyResolution>>,
    alias_env_stack: rustc_hash::FxHashSet<String>,
    budget: ImportedEvalTraversalBudget,
    external_type_cache: verter_resolver::ExternalTypeBodyCache,
    prepare_failure_count: u64,
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
    #[cfg_attr(not(test), allow(dead_code))]
    fn new(
        host: &'a VerterHost,
        owner_canonical_id: &'a str,
        store_view: Option<&'a crate::resolver_store::HostStoreView>,
    ) -> Self {
        let mut alias_env_stack = rustc_hash::FxHashSet::default();
        alias_env_stack.insert(owner_canonical_id.to_string());
        Self {
            host,
            dep_resolutions: std::borrow::Cow::Owned(
                host.dependency_resolutions_for_eval_in_view(owner_canonical_id, store_view)
                    .unwrap_or_default(),
            ),
            alias_env_stack,
            budget: ImportedEvalTraversalBudget::new(
                owner_canonical_id,
                COMPONENT_META_MAX_IMPORTED_TYPE_ROOTS,
            ),
            external_type_cache: verter_resolver::ExternalTypeBodyCache::default(),
            prepare_failure_count: 0,
            store_view,
        }
    }

    fn with_dep_resolutions(
        host: &'a VerterHost,
        owner_canonical_id: &'a str,
        dep_resolutions: &'a rustc_hash::FxHashMap<String, DependencyResolution>,
        store_view: Option<&'a crate::resolver_store::HostStoreView>,
    ) -> Self {
        let mut alias_env_stack = rustc_hash::FxHashSet::default();
        alias_env_stack.insert(owner_canonical_id.to_string());
        Self {
            host,
            dep_resolutions: std::borrow::Cow::Borrowed(dep_resolutions),
            alias_env_stack,
            budget: ImportedEvalTraversalBudget::new(
                owner_canonical_id,
                COMPONENT_META_MAX_IMPORTED_TYPE_ROOTS,
            ),
            external_type_cache: verter_resolver::ExternalTypeBodyCache::default(),
            prepare_failure_count: 0,
            store_view,
        }
    }

    fn cached_evaluated_decl(
        &self,
        canonical_id: &str,
        exported_name: &str,
    ) -> Option<verter_resolver::CachedEvaluatedImportedDecl> {
        self.cached_dependency(canonical_id)
            .and_then(|dependency| dependency.evaluated_type_decls.get(exported_name).cloned())
            .or_else(|| {
                self.host.clone_cached_evaluated_imported_decl(
                    canonical_id,
                    exported_name,
                    self.store_view,
                )
            })
    }

    fn cache_evaluated_decl(
        &self,
        canonical_id: &str,
        exported_name: &str,
        cached: verter_resolver::CachedEvaluatedImportedDecl,
    ) {
        self.host.cache_evaluated_imported_decl(
            canonical_id,
            exported_name,
            cached,
            self.store_view,
        );
    }

    fn cached_dependency(
        &self,
        dep_canonical: &str,
    ) -> Option<Arc<crate::ImportedDependencyCacheEntry>> {
        self.host
            .materialize_imported_dependency_state_in_view(dep_canonical, self.store_view)
    }

    fn cached_snapshot(&self, canonical_id: &str) -> Option<Arc<FileAnalysisSnapshot>> {
        self.cached_dependency(canonical_id)
            .and_then(|dependency| dependency.snapshot.clone())
            .or_else(|| {
                self.host
                    .get_raw_analysis_snapshot_in_view(canonical_id, self.store_view)
                    .map(Arc::new)
            })
    }

    fn cached_eval_source(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.cached_dependency(canonical_id)
            .and_then(|dependency| dependency.eval_source.clone())
            .or_else(|| {
                self.host
                    .materialize_imported_dependency_base_in_view(canonical_id, self.store_view)
                    .and_then(|entry| entry.eval_source.clone())
            })
    }

    fn cached_type_dependency_canonical(
        &self,
        owner_canonical_id: &str,
        import_source: &str,
    ) -> Option<String> {
        self.host.resolve_type_dependency_canonical_in_view(
            owner_canonical_id,
            import_source,
            self.store_view,
        )
    }

    fn resolve_imported_dependency_canonical(
        &self,
        owner_canonical_id: &str,
        import_source: &str,
        resolved_hint: Option<&str>,
    ) -> Option<String> {
        self.host.resolve_imported_dependency_canonical_in_view(
            owner_canonical_id,
            import_source,
            resolved_hint,
            Some(self.dep_resolutions.as_ref()),
            self.store_view,
        )
    }

    fn cached_dependency_eval_env(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_analysis::type_eval::EvalEnv>> {
        self.host
            .base_eval_env_arc_in_view(canonical_id, self.store_view)
    }

    fn cached_prepared_type_alias(
        &self,
        canonical_id: &str,
        exported_name: &str,
    ) -> Option<verter_resolver::CachedPreparedImportedTypeAlias> {
        self.host.clone_cached_prepared_imported_type_alias(
            canonical_id,
            exported_name,
            self.store_view,
        )
    }

    fn cache_prepared_type_alias(
        &self,
        canonical_id: &str,
        exported_name: &str,
        cached: verter_resolver::CachedPreparedImportedTypeAlias,
    ) {
        self.host.cache_prepared_imported_type_alias(
            canonical_id,
            exported_name,
            cached,
            self.store_view,
        );
    }

    fn cached_required_owner_import_names(
        &self,
        owner_canonical_id: &str,
        owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
        owner_eval_source: &str,
        owner_env: &verter_analysis::type_eval::EvalEnv,
    ) -> rustc_hash::FxHashSet<String> {
        let _trace = component_meta_trace_scope!(
            "collect_required_owner_import_names",
            format!(
                "owner={} macros={} bindings={} source_len={} type_symbols={} value_symbols={}",
                owner_canonical_id,
                owner_snapshot.macros.len(),
                owner_snapshot.bindings.len(),
                owner_eval_source.len(),
                owner_env.type_symbols.len(),
                owner_env.value_symbols.len(),
            ),
        );
        if let Some(cached) = self
            .host
            .clone_cached_required_owner_import_names(owner_canonical_id, self.store_view)
        {
            component_meta_trace_event!(
                "collect_required_owner_import_names_result",
                format!(
                    "owner={} hit=true required={}",
                    owner_canonical_id,
                    cached.len(),
                ),
            );
            return cached;
        }

        let required = collect_required_owner_import_names_from_parts(
            owner_snapshot,
            owner_eval_source,
            owner_env,
        );
        if self.cached_dependency(owner_canonical_id).is_some() {
            self.host.cache_required_owner_import_names(
                owner_canonical_id,
                required.clone(),
                self.store_view,
            );
        }
        component_meta_trace_event!(
            "collect_required_owner_import_names_result",
            format!(
                "owner={} hit=false required={}",
                owner_canonical_id,
                required.len(),
            ),
        );
        required
    }

    fn cached_required_import_names_for_exported_type(
        &self,
        canonical_id: &str,
        exported_name: &str,
        eval_source: &str,
    ) -> rustc_hash::FxHashSet<String> {
        if let Some(cached) = self
            .host
            .clone_cached_required_import_names_for_exported_type(
                canonical_id,
                exported_name,
                self.store_view,
            )
        {
            return cached;
        }

        let analysis = self
            .host
            .external_type_analysis_in_view(canonical_id, self.store_view)
            .unwrap_or_else(|| {
                let alloc = oxc_allocator::Allocator::new();
                Arc::new(
                    verter_core::utils::oxc::vue::resolve_type::analyze_external_type_source(
                        eval_source,
                        &alloc,
                    ),
                )
            });

        let required = analysis.required_import_names(exported_name);
        if self.cached_dependency(canonical_id).is_some() {
            self.host.cache_required_import_names_for_exported_type(
                canonical_id,
                exported_name,
                required.clone(),
                self.store_view,
            );
        }
        required
    }
}

fn exact_resolution_uses_type_preferred_target(
    phase: verter_vfs::ResolvePhase,
    kind: verter_vfs::ResolveRequestKind,
) -> bool {
    matches!(
        (phase, kind),
        (
            verter_vfs::ResolvePhase::CodegenBlocker,
            verter_vfs::ResolveRequestKind::TypeImport
        ) | (verter_vfs::ResolvePhase::ProviderGraph, _)
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

fn collect_type_expr_symbol_refs(
    expr: &verter_analysis::type_expr::TypeExpr,
    refs: &mut std::collections::BTreeSet<String>,
) {
    use verter_analysis::type_expr::{ObjectMember, TypeExpr};

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
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::Unknown { .. } => {}
    }
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
        sig: &verter_analysis::ExportSignature,
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
    ) -> Option<Arc<verter_analysis::type_eval::EvalEnv>> {
        self.host
            .base_eval_env_arc_in_view(canonical_id, self.store_view)
    }
}

impl DeclarationMetadataResolver for HostImportedEvalResolver<'_> {
    fn resolve_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<ResolvedExportTarget> {
        self.host
            .resolve_named_type_export_target_in_view(
                dep_canonical,
                requested_name,
                self.store_view,
            )
            .map(|(canonical, name)| ResolvedExportTarget {
                source_canonical_id: (canonical != dep_canonical).then_some(canonical),
                source_name: name,
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
            .materialize_imported_dependency_base_in_view(canonical_source, self.store_view)
            .map(|entry| entry.raw_source.as_ref().to_string())
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
        self.host.local_type_declaration_id_in_view(
            canonical_source,
            resolved_name,
            self.store_view,
        )
    }

    fn resolve_type_dependency_canonical(
        &self,
        from_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        self.cached_type_dependency_canonical(from_canonical, import_source)
    }

    fn resolve_direct_type_reexport_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        self.host.resolve_direct_type_reexport_target_in_view(
            dep_canonical,
            requested_name,
            self.store_view,
        )
    }

    fn resolve_local_import_symbol_target(
        &self,
        dep_canonical: &str,
        resolved_name: &str,
    ) -> Option<(String, String)> {
        self.host.resolve_local_import_symbol_target_in_view(
            dep_canonical,
            resolved_name,
            self.store_view,
        )
    }

    fn resolve_local_export_symbol_target(
        &self,
        canonical_source: &str,
        exported_name: &str,
    ) -> Option<String> {
        self.host.resolve_local_export_symbol_target_in_view(
            canonical_source,
            exported_name,
            self.store_view,
        )
    }

    fn resolve_local_type_symbol_metadata(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<verter_resolver::ResolvedLocalTypeSymbolMetadata> {
        let analysis = self
            .host
            .external_type_analysis_in_view(canonical_source, self.store_view)?;
        let symbol = analysis.local_type_symbol(resolved_name)?;
        let kind = match symbol.kind {
            verter_core::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::TypeAlias => {
                verter_resolver::ResolvedDeclarationKind::TypeAlias
            }
            verter_core::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::Interface => {
                verter_resolver::ResolvedDeclarationKind::Interface
            }
            verter_core::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::Class => {
                verter_resolver::ResolvedDeclarationKind::Class
            }
        };
        Some(verter_resolver::ResolvedLocalTypeSymbolMetadata {
            kind,
            span: symbol.span,
        })
    }
}

impl ImportedEvalLookupResolver for HostImportedEvalResolver<'_> {
    fn resolve_import_canonical_id(
        &self,
        owner_canonical_id: &str,
        import: &verter_analysis::AnalyzedImport,
    ) -> Option<String> {
        self.resolve_imported_dependency_canonical(
            owner_canonical_id,
            &import.source,
            import.resolved_canonical_id.as_deref(),
        )
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
            .resolve_named_export_in_view(
                dep_canonical_id,
                imported_name,
                Some(false),
                self.store_view,
            )
            .map(|export| ResolvedExportTarget {
                source_canonical_id: export.source_canonical_id,
                source_name: export.source_name,
            })
    }

    fn resolve_imported_type_root(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> (String, String) {
        self.host.resolve_imported_type_root_in_view(
            dep_canonical_id,
            imported_name,
            self.store_view,
        )
    }

    fn dependency_eval_env(
        &self,
        canonical_id: &str,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        self.cached_dependency_eval_env(canonical_id)
            .map(|env| (*env).clone())
    }
}

impl ImportedTypeAliasResolver for HostImportedEvalResolver<'_> {
    fn dependency_eval_env(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_analysis::type_eval::EvalEnv>> {
        self.cached_dependency_eval_env(canonical_id)
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
        let _trace = component_meta_trace_scope!(
            "resolve_external_type_body",
            format!(
                "owner={} import={} name={} tracked_deps={} resolution_deps={} cache_entries={}",
                request.owner_canonical_id,
                request.import_source,
                request.imported_name,
                tracked_deps.len(),
                resolution_deps.len(),
                self.external_type_cache.len(),
            ),
        );
        let tracked_before = tracked_deps.len();
        let resolution_before = resolution_deps.len();
        let cache_before = self.external_type_cache.len();
        let mut visiting = rustc_hash::FxHashSet::default();
        let result = match self.host.resolve_external_type_from_loaded_files_in_view(
            request.owner_canonical_id.as_str(),
            request.import_source.as_str(),
            request.imported_name.as_str(),
            tracked_deps,
            resolution_deps,
            &mut self.external_type_cache,
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
        };
        component_meta_trace_event!(
            "resolve_external_type_body_result",
            format!(
                "owner={} import={} name={} status={} tracked_delta={} resolution_delta={} cache_delta={} visiting={}",
                request.owner_canonical_id,
                request.import_source,
                request.imported_name,
                match &result {
                    Ok(Some(_)) => "ok:resolved",
                    Ok(None) => "ok:none",
                    Err(ImportedTypeAliasPrepareError::StepLimitExceeded { .. }) => {
                        "err:step_limit"
                    }
                    Err(ImportedTypeAliasPrepareError::Other) => "err:other",
                },
                tracked_deps.len().saturating_sub(tracked_before),
                resolution_deps.len().saturating_sub(resolution_before),
                self.external_type_cache.len().saturating_sub(cache_before),
                visiting.len(),
            ),
        );
        result
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

    fn imported_symbol_dependencies(
        &self,
        source_canonical_id: &str,
        exported_name: &str,
        decl_body: &verter_analysis::type_expr::TypeExpr,
    ) -> Vec<ImportedSymbolDependency> {
        self.host.imported_symbol_dependencies_in_view(
            source_canonical_id,
            exported_name,
            decl_body,
            self.store_view,
        )
    }

    fn cached_prepared_imported_type_alias(
        &self,
        source_canonical_id: &str,
        exported_name: &str,
    ) -> Option<verter_resolver::CachedPreparedImportedTypeAlias> {
        let cached = self.cached_prepared_type_alias(source_canonical_id, exported_name);
        if cached.is_some() {
            component_meta_trace_event!(
                "prepare_imported_type_alias_cache_hit",
                format!("source={} exported={}", source_canonical_id, exported_name),
            );
        }
        cached
    }

    fn cache_prepared_imported_type_alias(
        &self,
        source_canonical_id: &str,
        exported_name: &str,
        cached: verter_resolver::CachedPreparedImportedTypeAlias,
    ) {
        self.cache_prepared_type_alias(source_canonical_id, exported_name, cached);
    }
}

impl ImportedEvalCollectorResolver for HostImportedEvalResolver<'_> {
    fn resolve_imported_type_dependency(
        &self,
        owner_canonical_id: &str,
        import: &verter_analysis::AnalyzedImport,
    ) -> Option<String> {
        self.resolve_imported_dependency_canonical(
            owner_canonical_id,
            &import.source,
            import.resolved_canonical_id.as_deref(),
        )
    }

    fn collect_imported_type_alias(
        &mut self,
        request: ImportedTypeAliasResolveRequest,
        canonical_dependencies: &mut std::collections::BTreeSet<String>,
        budget: &mut ImportedEvalTraversalBudget,
    ) -> Option<CollectedImportedTypeAlias> {
        let _trace = component_meta_trace_scope!(
            "collect_imported_type_alias",
            format!(
                "owner={} local={} imported={} source={} exported={}",
                request.owner_canonical_id,
                request.local_name,
                request.imported_name,
                request.source_canonical_id,
                request.exported_name,
            ),
        );
        if budget.is_exhausted() {
            return None;
        }

        canonical_dependencies.insert(request.source_canonical_id.clone());
        let merge_root = (
            request.source_canonical_id.clone(),
            request.exported_name.clone(),
        );
        let prepared = verter_resolver::prepare_imported_type_alias(
            self,
            ImportedTypeAliasResolveRequest {
                owner_canonical_id: request.owner_canonical_id.clone(),
                import_source: request.import_source.clone(),
                local_name: merge_root.1.clone(),
                imported_name: merge_root.1.clone(),
                source_canonical_id: merge_root.0.clone(),
                exported_name: merge_root.1.clone(),
            },
            canonical_dependencies,
        );
        let Some(prepared) = prepared else {
            self.prepare_failure_count += 1;
            return None;
        };

        Some(CollectedImportedTypeAlias {
            alias: ImportedTypeAlias {
                local_name: request.local_name,
                source_canonical_id: request.source_canonical_id,
                exported_name: request.exported_name,
                requires_source_merge: prepared.requires_source_merge,
                merge_root_canonical: merge_root.0,
                merge_root_exported: merge_root.1,
            },
            symbol_dependencies: prepared.symbol_dependencies,
        })
    }

    fn prepare_imported_type_alias_failure_count(&self) -> u64 {
        self.prepare_failure_count
    }
}

impl ImportedEvalOwnerResolver for HostImportedEvalResolver<'_> {
    fn collect_required_owner_import_names(
        &self,
        owner_canonical_id: &str,
        owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
        owner_eval_source: &str,
        owner_env: &verter_analysis::type_eval::EvalEnv,
    ) -> rustc_hash::FxHashSet<String> {
        self.cached_required_owner_import_names(
            owner_canonical_id,
            owner_snapshot,
            owner_eval_source,
            owner_env,
        )
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
            self.dep_resolutions.as_ref(),
            canonical_dependencies,
        );
    }
}

impl ImportedEvalOwnerContextResolver for HostImportedEvalResolver<'_> {
    fn load_owner_eval_source(
        &self,
        owner_canonical_id: &str,
        _owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
    ) -> String {
        self.cached_eval_source(owner_canonical_id)
            .map(|source| source.as_ref().to_string())
            .unwrap_or_default()
    }

    fn load_owner_eval_env(
        &self,
        owner_canonical_id: &str,
        _owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
        owner_eval_source: &str,
    ) -> verter_analysis::type_eval::EvalEnv {
        self.host
            .base_eval_env_in_view(owner_canonical_id, self.store_view)
            .unwrap_or_else(|| {
                verter_analysis::type_eval_build::parse_and_build_env(owner_eval_source)
            })
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
        let _trace = component_meta_trace_scope!(
            "load_imported_decl_context",
            format!("source={} exported={}", source_canonical_id, exported_name),
        );
        let (resolved_source_canonical_id, resolved_exported_name) =
            self.host.resolve_imported_type_root_in_view(
                source_canonical_id,
                exported_name,
                self.store_view,
            );

        let dependency = self.cached_dependency(resolved_source_canonical_id.as_str())?;
        let snapshot = dependency.snapshot.clone()?;
        let dep_eval_source = dependency.eval_source.clone()?;
        let dep_env = dependency.env.clone()?;
        let decl = dep_env
            .type_symbols
            .get(resolved_exported_name.as_str())?
            .clone();

        Some(PreparedImportedDeclContext {
            imports: snapshot.imports.to_vec(),
            macros: snapshot.macros.as_ref().to_vec(),
            bindings: snapshot.bindings.to_vec(),
            macro_type_deps: snapshot.macro_type_deps.as_ref().to_vec(),
            eval_source: dep_eval_source.as_ref().to_string(),
            env: (*dep_env).clone(),
            decl,
        })
    }

    fn required_import_names_for_exported_type(
        &self,
        source_canonical_id: &str,
        exported_name: &str,
        eval_source: &str,
    ) -> rustc_hash::FxHashSet<String> {
        let _trace = component_meta_trace_scope!(
            "required_import_names_for_exported_type",
            format!(
                "source={} exported={} source_len={}",
                source_canonical_id,
                exported_name,
                eval_source.len(),
            ),
        );
        self.cached_required_import_names_for_exported_type(
            source_canonical_id,
            exported_name,
            eval_source,
        )
    }

    fn required_import_names_for_decl(
        &self,
        source_canonical_id: &str,
        exported_name: &str,
        decl: &verter_analysis::type_eval::TypeDeclInfo,
        owner_env: &verter_analysis::type_eval::EvalEnv,
    ) -> rustc_hash::FxHashSet<String> {
        if let Some(cached) = self
            .host
            .clone_cached_required_import_names_for_exported_type(
                source_canonical_id,
                exported_name,
                self.store_view,
            )
        {
            return cached;
        }

        let required = collect_required_import_names_for_type_decl(decl, owner_env);
        if self.cached_dependency(source_canonical_id).is_some() {
            self.host.cache_required_import_names_for_exported_type(
                source_canonical_id,
                exported_name,
                required.clone(),
                self.store_view,
            );
        }
        required
    }

    fn build_imported_inputs_for_decl(
        &mut self,
        owner_canonical_id: &str,
        context: &PreparedImportedDeclContext,
        additional_required_import_names: &rustc_hash::FxHashSet<String>,
    ) -> ImportedEvalInputs {
        let _trace = component_meta_trace_scope!(
            "build_imported_inputs_for_decl",
            format!(
                "owner={} imports={} macros={} bindings={} additional_required={}",
                owner_canonical_id,
                context.imports.len(),
                context.macros.len(),
                context.bindings.len(),
                additional_required_import_names.len(),
            ),
        );
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
        let _trace = component_meta_trace_scope!(
            "build_owner_eval_env_for_decl",
            format!(
                "owner={} imports={} bindings={} imported_sources={} imported_aliases={}",
                canonical_id,
                context.imports.len(),
                context.bindings.len(),
                imported_inputs.sources.len(),
                imported_inputs.type_aliases.len(),
            ),
        );
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
                None,
                self.store_view,
            )
            .map(|built| built.env)
    }

    fn cached_evaluated_decl(
        &self,
        source_canonical_id: &str,
        exported_name: &str,
    ) -> Option<verter_resolver::CachedEvaluatedImportedDecl> {
        HostImportedEvalResolver::cached_evaluated_decl(self, source_canonical_id, exported_name)
    }

    fn cache_evaluated_decl(
        &self,
        source_canonical_id: &str,
        exported_name: &str,
        cached: verter_resolver::CachedEvaluatedImportedDecl,
    ) {
        HostImportedEvalResolver::cache_evaluated_decl(
            self,
            source_canonical_id,
            exported_name,
            cached,
        );
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
        let _trace = component_meta_trace_scope!(
            "record_eval_input_source",
            format!(
                "canonical={} seen_sources={} inputs={} canonical_dependencies={}",
                canonical_id,
                seen_sources.len(),
                inputs.len(),
                canonical_dependencies.len(),
            ),
        );
        let Some(dependency) = self.cached_dependency(canonical_id) else {
            canonical_dependencies.insert(canonical_id.to_string());
            return;
        };
        let Some(eval_source) = dependency.eval_source.clone() else {
            canonical_dependencies.insert(dependency.resolved_canonical_id.clone());
            return;
        };

        canonical_dependencies.insert(dependency.resolved_canonical_id.clone());
        if !seen_sources.insert(dependency.resolved_canonical_id.clone()) {
            return;
        }

        inputs.push(ImportedEvalSource {
            canonical_id: dependency.resolved_canonical_id.clone(),
            source: eval_source,
        });
    }

    fn load_eval_source_for_merge(&mut self, canonical_id: &str) -> Option<Arc<str>> {
        let _trace = component_meta_trace_scope!(
            "load_eval_source_for_merge",
            format!("canonical={canonical_id}"),
        );
        self.cached_eval_source(canonical_id)
    }

    fn required_import_names_for_exported_type(
        &self,
        canonical_id: &str,
        exported_name: &str,
        eval_source: &str,
    ) -> rustc_hash::FxHashSet<String> {
        self.cached_required_import_names_for_exported_type(
            canonical_id,
            exported_name,
            eval_source,
        )
    }

    fn import_bindings_for_merge(
        &mut self,
        canonical_id: &str,
        eval_source: &str,
    ) -> Vec<ImportedEvalBinding> {
        let _trace = component_meta_trace_scope!(
            "import_bindings_for_merge",
            format!(
                "canonical={} source_len={}",
                canonical_id,
                eval_source.len()
            ),
        );
        let snapshot = self.cached_snapshot(canonical_id);

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

        if let Some(analysis) = self
            .host
            .external_type_analysis_in_view(canonical_id, self.store_view)
        {
            return analysis
                .extracted
                .bindings
                .iter()
                .map(|binding| ImportedEvalBinding {
                    local_name: binding.local_name.clone(),
                    imported_name: if binding.is_namespace {
                        None
                    } else if binding.imported_name != binding.local_name {
                        Some(binding.imported_name.clone())
                    } else {
                        None
                    },
                    source: binding.source.clone(),
                    resolved_canonical_id: None,
                    is_namespace: binding.is_namespace,
                })
                .collect();
        }

        Vec::new()
    }

    fn resolve_import_binding_dependency(
        &self,
        owner_canonical_id: &str,
        binding: &ImportedEvalBinding,
    ) -> Option<String> {
        let _trace = component_meta_trace_scope!(
            "resolve_import_binding_dependency",
            format!(
                "owner={} local={} imported={} source={} namespace={}",
                owner_canonical_id,
                binding.local_name,
                binding.imported_name.as_deref().unwrap_or(""),
                binding.source,
                binding.is_namespace,
            ),
        );
        self.resolve_imported_dependency_canonical(
            owner_canonical_id,
            &binding.source,
            binding.resolved_canonical_id.as_deref(),
        )
    }

    fn resolve_imported_type_declaration(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> verter_resolver::ResolvedTypeDeclaration {
        self.host.resolve_imported_type_declaration_in_view(
            dep_canonical,
            imported_name,
            self.store_view,
        )
    }

    fn resolve_imported_type_root(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> (String, String) {
        self.host
            .resolve_imported_type_root_in_view(dep_canonical, imported_name, self.store_view)
    }
}

type OwnerEvalEnvBuild = verter_resolver::OwnerEvalEnvBuild;

struct HostOwnerEvalEnvAssembler<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a HostStoreView>,
}

impl OwnerEvalEnvAssembler for HostOwnerEvalEnvAssembler<'_> {
    type Snapshot = FileAnalysisSnapshot;

    fn base_eval_env(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_analysis::type_eval::EvalEnv>> {
        self.host
            .base_eval_env_arc_in_view(canonical_id, self.store_view)
    }

    fn materialize_imported_type_aliases(
        &self,
        _snapshot: &Self::Snapshot,
        owner_local_type_names: &rustc_hash::FxHashSet<String>,
        imported_inputs: &ImportedEvalInputs,
        env: &mut verter_analysis::type_eval::EvalEnv,
    ) {
        self.host
            .materialize_imported_type_aliases_into_env_in_view(
                owner_local_type_names,
                imported_inputs,
                env,
                self.store_view,
            );
    }

    fn materialize_imported_runtime_values(
        &self,
        snapshot: &Self::Snapshot,
        owner_local_value_names: &rustc_hash::FxHashSet<String>,
        required_runtime_value_names: Option<&rustc_hash::FxHashSet<String>>,
        env: &mut verter_analysis::type_eval::EvalEnv,
    ) {
        self.host
            .materialize_imported_runtime_values_into_env_in_view(
                snapshot,
                owner_local_value_names,
                required_runtime_value_names,
                env,
                self.store_view,
            );
    }
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

    fn sfc_script_setup_type_params(
        source: &str,
        cached_parse: Option<&verter_core::parser::types::ParsedSfc>,
    ) -> Vec<verter_analysis::type_expr::TypeParam> {
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
        verter_analysis::type_eval_build::parse_type_parameter_clause(clause)
    }

    fn apply_sfc_script_setup_type_params(
        env: &mut verter_analysis::type_eval::EvalEnv,
        source: &str,
        cached_parse: Option<&verter_core::parser::types::ParsedSfc>,
    ) {
        for param in Self::sfc_script_setup_type_params(source, cached_parse) {
            env.type_bindings.insert(
                param.name.clone(),
                Arc::new(verter_analysis::type_expr::TypeExpr::type_parameter(param)),
            );
        }
    }

    fn owner_generic_type_bindings(
        env: &verter_analysis::type_eval::EvalEnv,
    ) -> rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr> {
        env.type_bindings
            .iter()
            .map(|(name, bound)| (name.clone(), (**bound).clone()))
            .collect()
    }

    pub(crate) fn build_eval_script_source(
        source: &str,
        cached_parse: Option<&verter_core::parser::types::ParsedSfc>,
    ) -> String {
        crate::host_resolve::extract_vue_script_content(source, cached_parse)
            .unwrap_or_else(|| source.to_string())
    }

    fn imported_eval_source_type(
        canonical_id: &str,
        raw_source: &str,
        cached_parse: Option<&verter_core::parser::types::ParsedSfc>,
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

    fn clone_cached_eval_env_arc(
        &self,
        cache_key: &str,
        whole_hash: Hash16,
    ) -> Option<Arc<verter_analysis::type_eval::EvalEnv>> {
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
            let type_context = (!parse_failed).then_some({
                Rc::new(crate::ParsedTypeResolutionContext::new(
                    Rc::clone(&program),
                    |parsed_program| {
                        let program = parsed_program.borrow_dependent();
                        let mut ctx =
                            verter_core::utils::oxc::vue::resolve_type::build_type_context(
                                program,
                                parsed_program.source_bytes(),
                                0,
                            );
                        ctx.set_trace_label(canonical_id.to_string());
                        ctx
                    },
                ))
            });
            let entry = ParsedEvalProgramCacheEntry {
                whole_hash,
                parse_failed,
                program,
                type_context,
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

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cached_parsed_eval_program_for_imported_dependency_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Rc<crate::ParsedEvalProgram>> {
        let entry = self.materialize_imported_dependency_state_in_view(canonical_id, store_view)?;
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
        let entry = self.materialize_imported_dependency_state_in_view(canonical_id, store_view)?;
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
        self.cached_parsed_eval_program_entry(
            &entry.resolved_canonical_id,
            entry.whole_hash,
            &eval_source,
            source_type,
        )
        .type_context
    }

    pub(crate) fn clear_thread_local_parsed_eval_program_cache(&self) {
        let host_instance_id = self.instance_id;
        HOST_PARSED_EVAL_PROGRAM_CACHE.with(|cache| {
            cache
                .borrow_mut()
                .retain(|key, _| key.host_instance_id != host_instance_id);
        });
    }

    pub(crate) fn ensure_shallow_imported_dependency_state_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<crate::ImportedDependencyCacheEntry>> {
        if let Some(entry) = self.clone_current_imported_dependency_entry(canonical_id, store_view)
        {
            if entry.external_type_analysis.is_some()
                && entry.script_analysis.is_some()
                && entry.export_signatures.is_some()
            {
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

        let (script_analysis, export_signatures, external_type_analysis) = if eval_source.is_empty()
        {
            (
                verter_analysis::ScriptAnalysisSnapshot::default(),
                Vec::new(),
                verter_core::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(),
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
                    verter_analysis::ScriptAnalysisSnapshot::default(),
                    Vec::new(),
                    verter_core::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(
                    ),
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
                (
                    snapshot.script_analysis,
                    snapshot.export_signatures,
                    verter_core::utils::oxc::vue::resolve_type::analyze_external_type_program(
                        program,
                    ),
                )
            }
        };

        Some(self.cache_imported_dependency_shallow_state(
            canonical_id,
            whole_hash,
            raw_source,
            cached_parse,
            Some(eval_source),
            Some(Arc::new(script_analysis)),
            Some(Arc::new(export_signatures)),
            Some(Arc::new(external_type_analysis)),
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
            && base.env.is_some()
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
                    base.env
                        .as_ref()
                        .map(|env| env.type_symbols.len())
                        .unwrap_or(0),
                    base.env
                        .as_ref()
                        .map(|env| env.value_symbols.len())
                        .unwrap_or(0),
                    base.whole_hash,
                ),
            );
            return Some(base);
        }

        if !base.resolved_canonical_id.ends_with(".vue")
            && base.snapshot.is_none()
            && base.env.is_none()
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
                let env = Arc::new(verter_analysis::type_eval_build::build_eval_env(
                    program,
                    base.raw_source.as_ref(),
                ));
                let external_type_analysis = Arc::new(
                    verter_core::utils::oxc::vue::resolve_type::analyze_external_type_program(
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
                    Some(Arc::clone(&env)),
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
                        env.type_symbols.len(),
                        env.value_symbols.len(),
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

        let (env, external_type_analysis) =
            match (base.env.clone(), base.external_type_analysis.clone()) {
                (Some(env), Some(external_type_analysis)) => (env, external_type_analysis),
                _ => self.build_eval_env_and_external_type_analysis(
                    &base.resolved_canonical_id,
                    base.whole_hash,
                    base.raw_source.as_ref(),
                    base.cached_parse.as_deref(),
                    &eval_source,
                ),
            };
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
            Some(Arc::clone(&env)),
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
                env.type_symbols.len(),
                env.value_symbols.len(),
                base.whole_hash,
            ),
        );
        Some(entry)
    }

    fn build_eval_env_and_external_type_analysis(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        raw_source: &str,
        cached_parse: Option<&verter_core::parser::types::ParsedSfc>,
        eval_source: &Arc<str>,
    ) -> (
        Arc<verter_analysis::type_eval::EvalEnv>,
        Arc<verter_core::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource>,
    ) {
        let parsed_eval_program = self.cached_parsed_eval_program_entry(
            canonical_id,
            whole_hash,
            eval_source,
            Self::imported_eval_source_type(canonical_id, raw_source, cached_parse),
        );
        if parsed_eval_program.parse_failed {
            let mut env =
                verter_analysis::type_eval_build::parse_and_build_env(eval_source.as_ref());
            Self::apply_sfc_script_setup_type_params(&mut env, raw_source, cached_parse);
            return (
                Arc::new(env),
                Arc::new(
                    verter_core::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(
                    ),
                ),
            );
        }

        let program = parsed_eval_program.program.borrow_dependent();
        let mut env =
            verter_analysis::type_eval_build::build_eval_env(program, eval_source.as_ref());
        Self::apply_sfc_script_setup_type_params(&mut env, raw_source, cached_parse);
        (
            Arc::new(env),
            Arc::new(
                verter_core::utils::oxc::vue::resolve_type::analyze_external_type_program(program),
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
    ) -> Option<Vec<verter_analysis::html_intrinsics::OwnedIntrinsicMember>> {
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
            verter_vfs::ResolutionContext {
                phase: verter_vfs::ResolvePhase::ProviderGraph,
                kind: verter_vfs::ResolveRequestKind::TypeImport,
            },
        )?;
        self.materialize_imported_dependency_state_in_view(&resolved.source_id, store_view)
    }

    fn expand_project_intrinsic_members_for_type_in_view(
        &self,
        entry: &crate::ImportedDependencyCacheEntry,
        type_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Vec<verter_analysis::html_intrinsics::OwnedIntrinsicMember>> {
        let shape =
            self.expand_project_intrinsic_shape_for_type_in_view(entry, type_name, store_view)?;
        Some(Self::owned_intrinsic_members_from_shape(shape))
    }

    fn expand_project_intrinsic_tag_members_in_view(
        &self,
        entry: &crate::ImportedDependencyCacheEntry,
        tag: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Vec<verter_analysis::html_intrinsics::OwnedIntrinsicMember>> {
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
    ) -> Option<verter_analysis::type_expand::ExpandedObjectShape> {
        self.expand_project_intrinsic_shape_for_expr_in_view(
            entry,
            &verter_analysis::type_expr::TypeExpr::named(type_name),
            store_view,
        )
    }

    fn expand_project_intrinsic_shape_for_expr_in_view(
        &self,
        entry: &crate::ImportedDependencyCacheEntry,
        expr: &verter_analysis::type_expr::TypeExpr,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<verter_analysis::type_expand::ExpandedObjectShape> {
        let snapshot = entry.snapshot.as_ref()?;
        let mut env = entry.env.as_ref()?.as_ref().clone();
        let mut resolver = HostImportedEvalResolver::with_dep_resolutions(
            self,
            &entry.resolved_canonical_id,
            &entry.dependency_resolutions,
            store_view,
        );
        let mut lookup = ImportedEvalLookup::new(
            &mut resolver,
            &entry.resolved_canonical_id,
            snapshot.imports.as_slice(),
        );
        let expanded = verter_analysis::type_expand::expand_object_shape_with_lookup(
            expr,
            &mut env,
            &component_meta_expansion_budget(),
            &mut lookup,
        );
        Some(expanded.value)
    }

    fn owned_intrinsic_members_from_shape(
        shape: verter_analysis::type_expand::ExpandedObjectShape,
    ) -> Vec<verter_analysis::html_intrinsics::OwnedIntrinsicMember> {
        let mut members = rustc_hash::FxHashMap::default();
        for property in shape.properties {
            if let Some(event_name) =
                verter_analysis::html_intrinsics::on_prop_to_event_name(property.name.as_str())
            {
                members.entry(format!("listener:{event_name}")).or_insert(
                    verter_analysis::html_intrinsics::OwnedIntrinsicMember {
                        name: event_name,
                        kind: verter_analysis::html_intrinsics::IntrinsicMemberKind::Listener,
                        type_expr: property.ty,
                    },
                );
                continue;
            }

            if !verter_analysis::html_intrinsics::should_expose_intrinsic_member(
                verter_analysis::html_intrinsics::IntrinsicMemberKind::Attr,
                property.name.as_str(),
            ) {
                continue;
            }

            members.entry(format!("attr:{}", property.name)).or_insert(
                verter_analysis::html_intrinsics::OwnedIntrinsicMember {
                    name: property.name,
                    kind: verter_analysis::html_intrinsics::IntrinsicMemberKind::Attr,
                    type_expr: property.ty,
                },
            );
        }

        let mut members: Vec<_> = members.into_values().collect();
        members.sort_by(|left, right| {
            let left_rank = match left.kind {
                verter_analysis::html_intrinsics::IntrinsicMemberKind::Attr => 0,
                verter_analysis::html_intrinsics::IntrinsicMemberKind::Listener => 1,
            };
            let right_rank = match right.kind {
                verter_analysis::html_intrinsics::IntrinsicMemberKind::Attr => 0,
                verter_analysis::html_intrinsics::IntrinsicMemberKind::Listener => 1,
            };
            left_rank
                .cmp(&right_rank)
                .then_with(|| left.name.cmp(&right.name))
        });
        members
    }

    fn merge_intrinsic_members(
        primary: Vec<verter_analysis::html_intrinsics::OwnedIntrinsicMember>,
        fallback: Vec<verter_analysis::html_intrinsics::OwnedIntrinsicMember>,
    ) -> Vec<verter_analysis::html_intrinsics::OwnedIntrinsicMember> {
        let mut members = rustc_hash::FxHashMap::default();
        for member in fallback {
            members.insert(
                format!(
                    "{}:{}",
                    match member.kind {
                        verter_analysis::html_intrinsics::IntrinsicMemberKind::Attr => "attr",
                        verter_analysis::html_intrinsics::IntrinsicMemberKind::Listener => {
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
                        verter_analysis::html_intrinsics::IntrinsicMemberKind::Attr => "attr",
                        verter_analysis::html_intrinsics::IntrinsicMemberKind::Listener => {
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
                verter_analysis::html_intrinsics::IntrinsicMemberKind::Attr => 0,
                verter_analysis::html_intrinsics::IntrinsicMemberKind::Listener => 1,
            };
            let right_rank = match right.kind {
                verter_analysis::html_intrinsics::IntrinsicMemberKind::Attr => 0,
                verter_analysis::html_intrinsics::IntrinsicMemberKind::Listener => 1,
            };
            left_rank
                .cmp(&right_rank)
                .then_with(|| left.name.cmp(&right.name))
        });
        members
    }

    fn clone_cached_evaluated_imported_decl(
        &self,
        canonical_id: &str,
        exported_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<verter_resolver::CachedEvaluatedImportedDecl> {
        self.clone_current_imported_dependency_entry(canonical_id, store_view)?
            .evaluated_type_decls
            .get(exported_name)
            .cloned()
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

    fn clone_cached_imported_type_declaration(
        &self,
        canonical_id: &str,
        exported_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<verter_resolver::ResolvedTypeDeclaration> {
        self.clone_current_imported_dependency_entry(canonical_id, store_view)?
            .resolved_type_declarations
            .get(exported_name)
            .cloned()
    }

    fn clone_cached_prepared_imported_type_alias(
        &self,
        canonical_id: &str,
        exported_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<verter_resolver::CachedPreparedImportedTypeAlias> {
        self.clone_current_imported_dependency_entry(canonical_id, store_view)?
            .prepared_type_aliases
            .get(exported_name)
            .cloned()
    }

    fn clone_cached_required_owner_import_names(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<rustc_hash::FxHashSet<String>> {
        self.clone_current_imported_dependency_entry(canonical_id, store_view)?
            .required_owner_import_names
            .as_ref()
            .map(|names| (**names).clone())
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

    fn imported_symbol_dependencies_in_view(
        &self,
        canonical_id: &str,
        exported_name: &str,
        decl_body: &verter_analysis::type_expr::TypeExpr,
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

            if analysis.local_symbol_span(root_name).is_some() {
                if seen.insert((
                    root_name.to_string(),
                    canonical_id.to_string(),
                    root_name.to_string(),
                )) {
                    dependencies.push(ImportedSymbolDependency {
                        local_name: root_name.to_string(),
                        canonical_id: canonical_id.to_string(),
                        exported_name: root_name.to_string(),
                    });
                }
                continue;
            }

            let Some((import_source, imported_name)) =
                analysis.local_import_symbol_target(root_name)
            else {
                continue;
            };
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
    ) -> Option<Arc<verter_core::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource>> {
        self.clone_current_imported_dependency_entry(canonical_id, store_view)?
            .external_type_analysis
            .clone()
    }

    pub(crate) fn external_type_analysis_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<verter_core::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource>> {
        let _trace = component_meta_trace_scope!(
            "external_type_analysis_in_view",
            format!("owner={} store_view={}", canonical_id, store_view.is_some()),
        );
        if let Some(cached) = self.clone_cached_external_type_analysis(canonical_id, store_view) {
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

        let entry =
            self.ensure_shallow_imported_dependency_state_in_view(canonical_id, store_view)?;
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

    pub(crate) fn resolve_external_type_from_cached_dependency_state_in_view(
        &self,
        dep_canonical: &str,
        type_name: &str,
        imported_companions: &rustc_hash::FxHashMap<
            String,
            verter_core::utils::oxc::vue::resolve_type::ResolvedElements,
        >,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements> {
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
            self.materialize_imported_dependency_state_in_view(dep_canonical, store_view)?;
        let analysis = entry.external_type_analysis.as_ref()?;
        let eval_source = entry
            .eval_source
            .as_ref()
            .map(Arc::clone)
            .unwrap_or_else(|| Arc::clone(&entry.raw_source));
        let parsed_eval_program = self.cached_parsed_eval_program_entry(
            &entry.resolved_canonical_id,
            entry.whole_hash,
            &eval_source,
            Self::imported_eval_source_type(
                &entry.resolved_canonical_id,
                entry.raw_source.as_ref(),
                entry.cached_parse.as_deref(),
            ),
        );
        if parsed_eval_program.parse_failed {
            component_meta_trace_event!(
                "resolve_external_type_from_cached_dependency_state_in_view_result",
                format!(
                    "owner={} type={} hit=false local_symbol_target={} parse_failed=true",
                    dep_canonical,
                    type_name,
                    analysis.has_local_symbol_target(type_name),
                ),
            );
            return None;
        }
        let Some(type_context) = parsed_eval_program.type_context.as_ref() else {
            component_meta_trace_event!(
                "resolve_external_type_from_cached_dependency_state_in_view_result",
                format!(
                    "owner={} type={} hit=false local_symbol_target={} parse_failed=false missing_type_context=true",
                    dep_canonical,
                    type_name,
                    analysis.has_local_symbol_target(type_name),
                ),
            );
            return None;
        };
        let program = type_context.borrow_owner().borrow_dependent();
        let base_ctx = type_context.borrow_dependent();
        let resolved = verter_core::utils::oxc::vue::resolve_type::resolve_external_type_in_context_with_analyzed_symbol_companion(
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

    pub(crate) fn resolve_named_type_target_from_analysis_in_view(
        &self,
        dep_canonical: &str,
        requested_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
        visited: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<(String, String)> {
        let normalized_canonical = self
            .load_eval_dependency_canonical_with_fallback_in_view(dep_canonical, store_view)
            .unwrap_or_else(|| dep_canonical.to_string());
        let visit_key = (normalized_canonical.clone(), requested_name.to_string());
        if !visited.insert(visit_key) {
            return None;
        }

        let analysis =
            self.external_type_analysis_in_view(normalized_canonical.as_str(), store_view)?;

        if let Some((import_source, imported_name)) =
            analysis.direct_reexport_target(requested_name)
        {
            let next_canonical = self.resolve_type_dependency_canonical_in_view(
                normalized_canonical.as_str(),
                import_source,
                store_view,
            )?;
            return self
                .resolve_named_type_target_from_analysis_in_view(
                    &next_canonical,
                    imported_name,
                    store_view,
                    visited,
                )
                .or_else(|| Some((next_canonical, imported_name.to_string())));
        }

        let target_name = analysis.local_symbol_target_name(requested_name);
        if let Some((import_source, imported_name)) =
            analysis.local_import_symbol_target(target_name.as_str())
        {
            let next_canonical = self.resolve_type_dependency_canonical_in_view(
                normalized_canonical.as_str(),
                import_source,
                store_view,
            )?;
            return self
                .resolve_named_type_target_from_analysis_in_view(
                    &next_canonical,
                    imported_name,
                    store_view,
                    visited,
                )
                .or_else(|| Some((next_canonical, imported_name.to_string())));
        }

        analysis
            .local_type_symbol(target_name.as_str())
            .map(|_| (normalized_canonical, target_name))
    }

    pub(crate) fn resolve_imported_dependency_canonical_in_view(
        &self,
        owner_canonical_id: &str,
        import_source: &str,
        resolved_hint: Option<&str>,
        dep_resolutions: Option<&rustc_hash::FxHashMap<String, DependencyResolution>>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<String> {
        let _trace = component_meta_trace_scope!(
            "resolve_imported_dependency_canonical_in_view",
            format!(
                "owner={} source={} hint={} store_view={}",
                owner_canonical_id,
                import_source,
                resolved_hint.is_some(),
                store_view.is_some(),
            ),
        );
        let resolved = self
            .resolve_type_dependency_canonical_in_view(
                owner_canonical_id,
                import_source,
                store_view,
            )
            .or_else(|| resolved_hint.map(str::to_string))
            .or_else(|| {
                dep_resolutions
                    .and_then(|resolutions| resolutions.get(import_source))
                    .and_then(|resolution| resolution.resolved_canonical_id.clone())
            })
            .or_else(|| {
                dep_resolutions
                    .and_then(|resolutions| resolutions.get(import_source))
                    .and_then(DependencyResolution::effective_target)
                    .map(str::to_string)
            })
            .or_else(|| {
                (store_view.is_none() && import_source.starts_with('.'))
                    .then(|| crate::id::resolve_external(owner_canonical_id, import_source))
            });
        component_meta_trace_event!(
            "resolve_imported_dependency_canonical_in_view_result",
            format!(
                "owner={} source={} resolved={}",
                owner_canonical_id,
                import_source,
                resolved.as_deref().unwrap_or(""),
            ),
        );
        resolved
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
            .load_eval_dependency_canonical_with_fallback_in_view(dep_canonical, store_view)
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
                    .load_eval_dependency_canonical_with_fallback_in_view(
                        canonical.as_str(),
                        store_view,
                    )
                    .unwrap_or(canonical);
                (canonical, exported_name)
            })
            .unwrap_or_else(|| {
                let canonical = self
                    .load_eval_dependency_canonical_with_fallback_in_view(
                        normalized_canonical.as_str(),
                        store_view,
                    )
                    .unwrap_or(normalized_canonical);
                (canonical, imported_name.to_string())
            });
        self.cache_imported_type_root(dep_canonical, imported_name, resolved.clone(), store_view);
        resolved
    }

    pub(crate) fn resolve_imported_type_declaration_in_view(
        &self,
        dep_canonical: &str,
        imported_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> verter_resolver::ResolvedTypeDeclaration {
        let _trace = component_meta_trace_scope!(
            "resolve_imported_type_declaration",
            format!("canonical={} imported={}", dep_canonical, imported_name),
        );
        if let Some(cached) =
            self.clone_cached_imported_type_declaration(dep_canonical, imported_name, store_view)
        {
            component_meta_trace_event!(
                "resolve_imported_type_declaration_cache_hit",
                format!("canonical={} imported={}", dep_canonical, imported_name),
            );
            return cached;
        }

        let normalized_canonical = self
            .load_eval_dependency_canonical_with_fallback_in_view(dep_canonical, store_view)
            .unwrap_or_else(|| dep_canonical.to_string());

        let _ = self.ensure_shallow_imported_dependency_state_in_view(
            normalized_canonical.as_str(),
            store_view,
        );
        if let Some(cached) =
            self.clone_cached_imported_type_declaration(dep_canonical, imported_name, store_view)
        {
            component_meta_trace_event!(
                "resolve_imported_type_declaration_cache_hit",
                format!(
                    "canonical={} imported={} source=host_imported_dep",
                    dep_canonical, imported_name
                ),
            );
            return cached;
        }

        let resolved = crate::meta_resolve::resolve_type_declaration_in_view(
            self,
            normalized_canonical.as_str(),
            imported_name,
            store_view,
        );
        self.cache_imported_type_declaration(
            dep_canonical,
            imported_name,
            resolved.clone(),
            store_view,
        );
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
                if cached_entry.snapshot.is_none() && entry.snapshot.is_some() {
                    cached_entry.snapshot = entry.snapshot.clone();
                }
                if cached_entry.eval_source.is_none() && entry.eval_source.is_some() {
                    cached_entry.eval_source = entry.eval_source.clone();
                }
                if cached_entry.env.is_none() && entry.env.is_some() {
                    cached_entry.env = entry.env.clone();
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
                for (name, alias) in &entry.prepared_type_aliases {
                    cached_entry
                        .prepared_type_aliases
                        .entry(name.clone())
                        .or_insert_with(|| alias.clone());
                }
                for (name, evaluated) in &entry.evaluated_type_decls {
                    cached_entry
                        .evaluated_type_decls
                        .entry(name.clone())
                        .or_insert_with(|| evaluated.clone());
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

    fn cache_evaluated_imported_decl(
        &self,
        canonical_id: &str,
        exported_name: &str,
        cached: verter_resolver::CachedEvaluatedImportedDecl,
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
            .evaluated_type_decls
            .insert(exported_name.to_string(), cached);
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

    fn cache_imported_type_declaration(
        &self,
        canonical_id: &str,
        exported_name: &str,
        declaration: verter_resolver::ResolvedTypeDeclaration,
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
            .resolved_type_declarations
            .insert(exported_name.to_string(), declaration);
    }

    fn cache_prepared_imported_type_alias(
        &self,
        canonical_id: &str,
        exported_name: &str,
        cached: verter_resolver::CachedPreparedImportedTypeAlias,
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
            .prepared_type_aliases
            .insert(exported_name.to_string(), cached);
    }

    fn cache_required_owner_import_names(
        &self,
        canonical_id: &str,
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
        Arc::make_mut(entry).required_owner_import_names = Some(Arc::new(required_names));
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
                snapshot: None,
                eval_source: None,
                env: None,
                required_owner_import_names: None,
                exported_required_import_names: rustc_hash::FxHashMap::default(),
                resolved_type_roots: rustc_hash::FxHashMap::default(),
                resolved_type_declarations: rustc_hash::FxHashMap::default(),
                prepared_type_aliases: rustc_hash::FxHashMap::default(),
                evaluated_type_decls: rustc_hash::FxHashMap::default(),
                dependency_resolutions: rustc_hash::FxHashMap::default(),
            },
        )
    }

    fn cache_imported_dependency_shallow_state(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        raw_source: Arc<str>,
        cached_parse: Option<Arc<verter_core::parser::types::ParsedSfc>>,
        eval_source: Option<Arc<str>>,
        script_analysis: Option<Arc<verter_analysis::ScriptAnalysisSnapshot>>,
        export_signatures: Option<Arc<Vec<verter_analysis::ExportSignature>>>,
        external_type_analysis: Option<
            Arc<verter_core::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource>,
        >,
    ) -> Arc<crate::ImportedDependencyCacheEntry> {
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
                external_type_analysis,
                snapshot: None,
                eval_source,
                env: None,
                required_owner_import_names: None,
                exported_required_import_names: rustc_hash::FxHashMap::default(),
                resolved_type_roots: rustc_hash::FxHashMap::default(),
                resolved_type_declarations: rustc_hash::FxHashMap::default(),
                prepared_type_aliases: rustc_hash::FxHashMap::default(),
                evaluated_type_decls: rustc_hash::FxHashMap::default(),
                dependency_resolutions: rustc_hash::FxHashMap::default(),
            },
        )
    }

    pub(crate) fn cache_imported_dependency_state(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        raw_source: Arc<str>,
        cached_parse: Option<Arc<verter_core::parser::types::ParsedSfc>>,
        snapshot: Option<Arc<FileAnalysisSnapshot>>,
        eval_source: Option<Arc<str>>,
        env: Option<Arc<verter_analysis::type_eval::EvalEnv>>,
        external_type_analysis: Option<
            Arc<verter_core::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource>,
        >,
        dependency_resolutions: rustc_hash::FxHashMap<String, DependencyResolution>,
    ) -> Arc<crate::ImportedDependencyCacheEntry> {
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
                external_type_analysis,
                snapshot,
                eval_source,
                env,
                required_owner_import_names: None,
                exported_required_import_names: rustc_hash::FxHashMap::default(),
                resolved_type_roots: rustc_hash::FxHashMap::default(),
                resolved_type_declarations: rustc_hash::FxHashMap::default(),
                prepared_type_aliases: rustc_hash::FxHashMap::default(),
                evaluated_type_decls: rustc_hash::FxHashMap::default(),
                dependency_resolutions,
            },
        )
    }

    fn cache_eval_env_arc(
        &self,
        cache_keys: &[String],
        whole_hash: Hash16,
        cached_env: Arc<verter_analysis::type_eval::EvalEnv>,
    ) -> Arc<verter_analysis::type_eval::EvalEnv> {
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

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn base_eval_env(
        &self,
        canonical_id: &str,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        self.base_eval_env_in_view(canonical_id, None)
    }

    pub(crate) fn base_eval_env_arc_in_view(
        &self,
        canonical_id: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<verter_analysis::type_eval::EvalEnv>> {
        let _trace = component_meta_trace_scope!(
            "base_eval_env",
            format!("owner={} store_view={}", canonical_id, store_view.is_some()),
        );
        if let Some(imported_entry) =
            self.materialize_imported_dependency_state_in_view(canonical_id, store_view)
        {
            let whole_hash = imported_entry.whole_hash;
            if let Some(cached_env) = self.clone_cached_eval_env_arc(canonical_id, whole_hash) {
                component_meta_trace_event!(
                    "base_eval_env_cache_hit",
                    format!("owner={} whole_hash={whole_hash:?}", canonical_id),
                );
                return Some(cached_env);
            }

            if let Some(cached_env) = imported_entry.env.clone() {
                component_meta_trace_event!(
                    "base_eval_env_imported_cache_hit",
                    format!("owner={} whole_hash={whole_hash:?}", canonical_id),
                );
                return Some(self.cache_eval_env_arc(
                    &[canonical_id.to_string()],
                    whole_hash,
                    cached_env,
                ));
            }

            let eval_source = imported_entry.eval_source.clone().unwrap_or_else(|| {
                Arc::<str>::from(Self::build_eval_script_source(
                    imported_entry.raw_source.as_ref(),
                    imported_entry.cached_parse.as_deref(),
                ))
            });
            let env = imported_entry
                .env
                .as_ref()
                .map(Arc::clone)
                .unwrap_or_else(|| {
                    let mut env =
                        verter_analysis::type_eval_build::parse_and_build_env(eval_source.as_ref());
                    Self::apply_sfc_script_setup_type_params(
                        &mut env,
                        imported_entry.raw_source.as_ref(),
                        imported_entry.cached_parse.as_deref(),
                    );
                    Arc::new(env)
                });
            component_meta_trace_event!(
                "base_eval_env_built",
                format!(
                    "owner={} source_len={} eval_source_len={} type_symbols={} value_symbols={}",
                    canonical_id,
                    imported_entry.raw_source.len(),
                    eval_source.len(),
                    env.type_symbols.len(),
                    env.value_symbols.len(),
                ),
            );
            return Some(self.cache_eval_env_arc(&[canonical_id.to_string()], whole_hash, env));
        }

        let (resolved_canonical_id, eval_source) =
            self.load_eval_dependency_source_with_fallback_in_view(canonical_id, store_view)?;
        let whole_hash = crate::hash::hash_16(eval_source.as_bytes());

        if let Some(cached_env) = self.clone_cached_eval_env_arc(&resolved_canonical_id, whole_hash)
        {
            component_meta_trace_event!(
                "base_eval_env_dependency_cache_hit",
                format!(
                    "owner={} resolved_owner={} whole_hash={whole_hash:?}",
                    canonical_id, resolved_canonical_id
                ),
            );
            return Some(cached_env);
        }

        let env = Arc::new(verter_analysis::type_eval_build::parse_and_build_env(
            eval_source.as_ref(),
        ));
        component_meta_trace_event!(
            "base_eval_env_dependency_built",
            format!(
                "owner={} resolved_owner={} eval_source_len={} type_symbols={} value_symbols={}",
                canonical_id,
                resolved_canonical_id,
                eval_source.len(),
                env.type_symbols.len(),
                env.value_symbols.len(),
            ),
        );
        Some(self.cache_eval_env_arc(
            &[resolved_canonical_id, canonical_id.to_string()],
            whole_hash,
            env,
        ))
    }

    pub(crate) fn local_type_declaration_id_in_view(
        &self,
        canonical_source: &str,
        resolved_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<verter_analysis::type_eval::DeclarationId> {
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
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        self.base_eval_env_arc_in_view(canonical_id, store_view)
            .map(|env| (*env).clone())
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

    fn dependency_resolutions_from_snapshot_in_view(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> rustc_hash::FxHashMap<String, DependencyResolution> {
        let mut resolutions = Self::dependency_resolutions_from_snapshot(snapshot);
        let declaration_file = canonical_id.ends_with(".d.ts")
            || canonical_id.ends_with(".d.mts")
            || canonical_id.ends_with(".d.cts");

        let mut upsert_resolution =
            |specifier: &str, kind: verter_vfs::ResolveRequestKind, prefer_live_fallback: bool| {
                let existing = resolutions.get(specifier).cloned();
                let resolved = existing
                    .as_ref()
                    .and_then(|resolution| {
                        resolution
                            .resolved_canonical_id
                            .clone()
                            .or_else(|| resolution.effective_target().map(str::to_string))
                    })
                    .or_else(|| match kind {
                        verter_vfs::ResolveRequestKind::TypeImport => self
                            .resolve_type_dependency_canonical_in_view(
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
                            verter_vfs::ResolveRequestKind::TypeImport => {
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

        for import in &snapshot.imports {
            let kind = if import.is_type_only {
                verter_vfs::ResolveRequestKind::TypeImport
            } else {
                verter_vfs::ResolveRequestKind::EsmImport
            };
            upsert_resolution(import.source.as_str(), kind, false);
        }

        for sig in snapshot.export_signatures.iter() {
            let Some(source) = sig.reexport_source.as_deref() else {
                continue;
            };
            let kind = if sig.is_type || declaration_file {
                verter_vfs::ResolveRequestKind::TypeImport
            } else {
                verter_vfs::ResolveRequestKind::EsmImport
            };
            upsert_resolution(source, kind, true);
        }

        resolutions
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
        cached_parse: Option<&verter_core::parser::types::ParsedSfc>,
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
        result: &verter_analysis::type_expand::ExpandedComponentTypes,
    ) -> bool {
        result.is_empty()
    }

    #[cfg_attr(not(test), allow(dead_code))]
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
                    .then(|| Arc::new(verter_core::compile::parse_sfc(&source, None, None)));
                let whole_hash = crate::hash::hash_16(source.as_bytes());
                let _ = self.cache_imported_dependency_state(
                    canonical_id,
                    whole_hash,
                    Arc::clone(&source),
                    cached_parse.clone(),
                    None,
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
                    .then(|| Arc::new(verter_core::compile::parse_sfc(&source, None, None)));
                let whole_hash = crate::hash::hash_16(source.as_bytes());
                let _ = self.cache_imported_dependency_state(
                    canonical_id,
                    whole_hash,
                    Arc::clone(&source),
                    cached_parse.clone(),
                    None,
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

    #[cfg_attr(not(test), allow(dead_code))]
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
        let _trace = component_meta_trace_scope!(
            "dependency_resolutions_for_eval",
            format!("owner={} store_view={}", canonical_id, store_view.is_some()),
        );
        if let Some(view) = store_view {
            self.current_eval_state_in_view(canonical_id, Some(view))?;
            let resolutions = view
                .dependency_resolutions(canonical_id)
                .cloned()
                .unwrap_or_default();
            component_meta_trace_event!(
                "dependency_resolutions_for_eval_result",
                format!("owner={} count={}", canonical_id, resolutions.len()),
            );
            return Some(
                view.dependency_resolutions(canonical_id)
                    .cloned()
                    .unwrap_or_default(),
            );
        }

        self.current_eval_state_in_view(canonical_id, store_view)?;

        #[cfg(feature = "scheduler")]
        {
            let resolutions = self
                .compile_cache
                .get(canonical_id)
                .map(|entry| entry.dependency_resolutions.clone())
                .unwrap_or_default();
            component_meta_trace_event!(
                "dependency_resolutions_for_eval_result",
                format!("owner={} count={}", canonical_id, resolutions.len()),
            );
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
            let resolutions = files
                .get(canonical_id)
                .map(|entry| entry.dependency_resolutions.clone())
                .unwrap_or_default();
            component_meta_trace_event!(
                "dependency_resolutions_for_eval_result",
                format!("owner={} count={}", canonical_id, resolutions.len()),
            );
            Some(
                files
                    .get(canonical_id)
                    .map(|entry| entry.dependency_resolutions.clone())
                    .unwrap_or_default(),
            )
        }
    }

    fn load_eval_dependency_source_with_fallback_in_view(
        &self,
        dep_canonical: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<(String, Arc<str>)> {
        let read_candidate = |candidate: &str| -> Option<Arc<str>> {
            self.ensure_shallow_imported_dependency_state_in_view(candidate, store_view)
                .and_then(|entry| entry.eval_source.clone())
        };

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

        if !prefers_type_companion {
            if let Some(source) = read_candidate(dep_canonical) {
                return Some((dep_canonical.to_string(), source));
            }
        }

        for candidate in candidates {
            if let Some(source) = read_candidate(&candidate) {
                return Some((candidate, source));
            }
        }

        if prefers_type_companion {
            if let Some(source) = read_candidate(dep_canonical) {
                return Some((dep_canonical.to_string(), source));
            }
        }

        None
    }

    pub(crate) fn load_eval_dependency_canonical_with_fallback_in_view(
        &self,
        dep_canonical: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<String> {
        self.load_eval_dependency_source_with_fallback_in_view(dep_canonical, store_view)
            .map(|(canonical, _)| canonical)
    }

    fn load_eval_dependency_source_text_with_fallback_in_view(
        &self,
        dep_canonical: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Arc<str>> {
        self.load_eval_dependency_source_with_fallback_in_view(dep_canonical, store_view)
            .map(|(_, source)| source)
    }

    #[cfg_attr(not(test), allow(dead_code))]
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

    #[cfg_attr(not(test), allow(dead_code))]
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

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn imported_eval_inputs_with_owner_context_in_view(
        &self,
        owner_canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
        owner_eval_source: Option<&str>,
        owner_env: Option<&verter_analysis::type_eval::EvalEnv>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> ImportedEvalInputs {
        let _trace = component_meta_trace_scope!(
            "imported_eval_inputs",
            format!(
                "owner={} imports={} dep_resolutions={} owner_eval_source={} owner_env={} store_view={}",
                owner_canonical_id,
                snapshot.imports.len(),
                dep_resolutions.len(),
                owner_eval_source.is_some(),
                owner_env.is_some(),
                store_view.is_some(),
            ),
        );
        self.provenance
            .imported_eval_inputs_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let started = component_meta_debug_enabled().then(Instant::now);
        let owner_snapshot = ImportedEvalOwnerSnapshot {
            imports: snapshot.imports.as_slice(),
            macros: snapshot.macros.as_ref(),
            bindings: snapshot.bindings.as_ref(),
            macro_type_deps: snapshot.macro_type_deps.as_ref(),
        };
        let mut budget = ImportedEvalTraversalBudget::new(
            owner_canonical_id,
            COMPONENT_META_MAX_IMPORTED_TYPE_ROOTS,
        );
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
            dep_resolutions,
            store_view,
        );
        let imported_inputs = build_imported_eval_inputs_with_owner_context(
            &mut collector,
            owner_canonical_id,
            &owner_snapshot,
            owner_eval_source,
            owner_env,
            None,
            &mut budget,
        );
        self.provenance.imported_eval_worklist_seed_count.fetch_add(
            imported_inputs.stats.worklist_seed_count,
            std::sync::atomic::Ordering::Relaxed,
        );
        self.provenance
            .imported_eval_worklist_resolved_count
            .fetch_add(
                imported_inputs.stats.worklist_resolved_count,
                std::sync::atomic::Ordering::Relaxed,
            );
        self.provenance
            .imported_eval_worklist_enqueued_from_symbol_deps_count
            .fetch_add(
                imported_inputs
                    .stats
                    .worklist_enqueued_from_symbol_deps_count,
                std::sync::atomic::Ordering::Relaxed,
            );
        self.provenance
            .imported_eval_reached_merge_roots_count
            .fetch_add(
                imported_inputs.stats.reached_merge_roots_count,
                std::sync::atomic::Ordering::Relaxed,
            );
        self.provenance.imported_eval_sources_count.fetch_add(
            imported_inputs.stats.imported_sources_count,
            std::sync::atomic::Ordering::Relaxed,
        );
        self.provenance
            .imported_eval_normalized_type_root_calls
            .fetch_add(
                imported_inputs.stats.normalized_imported_type_root_calls,
                std::sync::atomic::Ordering::Relaxed,
            );
        self.provenance.imported_eval_prepare_failures.fetch_add(
            imported_inputs.stats.prepare_imported_type_alias_failures,
            std::sync::atomic::Ordering::Relaxed,
        );
        self.provenance
            .imported_eval_dropped_unreached_aliases
            .fetch_add(
                imported_inputs.stats.dropped_unreached_aliases,
                std::sync::atomic::Ordering::Relaxed,
            );
        component_meta_trace_event!(
            "imported_eval_inputs_result",
            format!(
                "owner={} type_aliases={} sources={} overflow={} canonical_dependencies={}",
                owner_canonical_id,
                imported_inputs.type_aliases.len(),
                imported_inputs.sources.len(),
                imported_inputs.overflow.is_some(),
                imported_inputs.canonical_dependencies.len(),
            ),
        );

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
        let _trace = component_meta_trace_scope!(
            "track_direct_eval_dependencies",
            format!(
                "owner={} macro_type_deps={} imports={} allow_relative_fallback={} dep_resolutions={}",
                owner_canonical_id,
                owner_snapshot.macro_type_deps.len(),
                owner_snapshot.imports.len(),
                allow_relative_fallback,
                dep_resolutions.len(),
            ),
        );
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

        component_meta_trace_event!(
            "track_direct_eval_dependencies_result",
            format!(
                "owner={} canonical_dependencies={}",
                owner_canonical_id,
                canonical_dependencies.len(),
            ),
        );
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
        let mut computed = self.compute_evaluated_types_with_budget_from_owner_context_in_view(
            canonical,
            snapshot,
            imported_inputs,
            &eval_source,
            owner_env.clone(),
            store_view,
            component_meta_expansion_budget(),
        )?;

        if COMPONENT_META_RETRY_MAX_SYMBOLIC_STEPS > COMPONENT_META_MAX_SYMBOLIC_STEPS
            && should_retry_component_meta_expansion(&computed)
        {
            component_meta_trace_event!(
                "compute_evaluated_types_retry",
                format!(
                    "owner={} from={} to={}",
                    canonical,
                    COMPONENT_META_MAX_SYMBOLIC_STEPS,
                    COMPONENT_META_RETRY_MAX_SYMBOLIC_STEPS,
                ),
            );
            if component_meta_debug_enabled() {
                component_meta_debug(format!(
                    "compute_evaluated_types owner={} retrying symbolic expansion budget from {} to {}",
                    canonical,
                    COMPONENT_META_MAX_SYMBOLIC_STEPS,
                    COMPONENT_META_RETRY_MAX_SYMBOLIC_STEPS,
                ));
            }
            if let Some(retried) = self
                .compute_evaluated_types_with_budget_from_owner_context_in_view(
                    canonical,
                    snapshot,
                    imported_inputs,
                    &eval_source,
                    owner_env,
                    store_view,
                    component_meta_expansion_budget_with_max_symbolic_work(
                        COMPONENT_META_RETRY_MAX_SYMBOLIC_STEPS,
                    ),
                )
            {
                computed = retried;
            }
        }

        Some(computed)
    }

    fn compute_evaluated_types_with_budget_from_owner_context_in_view(
        &self,
        canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        imported_inputs: &ImportedEvalInputs,
        eval_source: &str,
        owner_env: Option<verter_analysis::type_eval::EvalEnv>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
        budget: verter_analysis::type_expand::ExpansionBudget,
    ) -> Option<ComputedEvaluatedTypes> {
        let required_runtime_value_names = if let Some(owner_env) = owner_env.as_ref() {
            collect_required_runtime_value_names(snapshot, eval_source, owner_env)
        } else if let Some(base_env) = self.base_eval_env_in_view(canonical, store_view) {
            collect_required_runtime_value_names(snapshot, eval_source, &base_env)
        } else {
            rustc_hash::FxHashSet::default()
        };
        let built = self.build_owner_eval_env_with_inputs_from_owner_env_in_view(
            canonical,
            snapshot,
            imported_inputs,
            None,
            owner_env,
            Some(&required_runtime_value_names),
            store_view,
        )?;
        let mut env = built.env;
        let dep_resolutions = self
            .dependency_resolutions_for_eval_in_view(canonical, store_view)
            .unwrap_or_default();
        let mut resolver = HostImportedEvalResolver::with_dep_resolutions(
            self,
            canonical,
            &dep_resolutions,
            store_view,
        );
        let mut lookup =
            ImportedEvalLookup::new(&mut resolver, canonical, snapshot.imports.as_slice());

        let result = verter_analysis::type_eval_build::expand_macro_types_with_lookup(
            snapshot.macros.as_ref(),
            Some(eval_source),
            &mut env,
            Some(&built.requested_binding_names),
            &budget,
            &mut lookup,
        );
        let discovered_dependencies = lookup.into_discovered_dependencies();
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "compute_evaluated_types owner={} max_symbolic_work={} props={} define_props={} emits={} slot_bindings={} bindings={} discovered_deps={} budget_exhausted={}",
                canonical,
                budget.max_symbolic_work,
                result.props.len(),
                result.define_props.len(),
                result.emits.len(),
                result.slot_bindings.len(),
                result.bindings.len(),
                discovered_dependencies.len(),
                expanded_component_types_hit_symbolic_budget(&result),
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
        let include_fallthrough = match resolved.cached_eval_inputs.as_ref() {
            Some(inputs) => inputs.overflow.is_none(),
            None => true,
        };
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
        let include_fallthrough = match resolved.cached_eval_inputs.as_ref() {
            Some(inputs) => inputs.overflow.is_none(),
            None => true,
        };
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

        let result = verter_resolver::run_fallthrough_request(
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
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
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
        let fallthrough_fact_versions = resolved.fact_versions.clone();

        let resolved_macros = resolver_component_meta_resolved_macros(
            resolved.snapshot.macros.as_ref(),
            &resolved.resolved_macros,
        );
        let resolved_type_registry =
            resolver_component_meta_type_registry(&resolved.resolved_type_registry);
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
        let _trace = component_meta_trace_scope!(
            "build_fallthrough_eval_env",
            format!(
                "owner={} imports={} imported_sources={} overrides={} store_view={}",
                canonical_id,
                snapshot.imports.len(),
                imported_inputs.sources.len(),
                prop_type_overrides
                    .map(|overrides| overrides.len())
                    .unwrap_or_default(),
                store_view.is_some(),
            ),
        );
        let required_runtime_value_names = collect_required_template_runtime_value_names(snapshot);
        Some(
            self.build_owner_eval_env_with_inputs_from_owner_env_in_view(
                canonical_id,
                snapshot,
                imported_inputs,
                prop_type_overrides,
                None,
                Some(&required_runtime_value_names),
                store_view,
            )?
            .env,
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
        required_runtime_value_names: Option<&rustc_hash::FxHashSet<String>>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<OwnerEvalEnvBuild> {
        let _trace =
            component_meta_trace_scope!(
                "build_owner_eval_env",
                format!(
                "owner={} imports={} imported_sources={} overrides={} owner_env={} store_view={}",
                canonical_id,
                snapshot.imports.len(),
                imported_inputs.sources.len(),
                prop_type_overrides.map(|overrides| overrides.len()).unwrap_or_default(),
                owner_env.is_some(),
                store_view.is_some(),
            ),
            );
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
            required_runtime_value_names,
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
        component_meta_trace_event!(
            "build_owner_eval_env_result",
            format!(
                "owner={} type_symbols={} value_symbols={} imported_sources={}",
                canonical_id,
                built.env.type_symbols.len(),
                built.env.value_symbols.len(),
                imported_inputs.sources.len(),
            ),
        );
        Some(built)
    }

    fn materialize_imported_type_aliases_into_env_in_view(
        &self,
        owner_local_type_names: &rustc_hash::FxHashSet<String>,
        imported_inputs: &ImportedEvalInputs,
        env: &mut verter_analysis::type_eval::EvalEnv,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) {
        let _trace = component_meta_trace_scope!(
            "materialize_imported_type_aliases",
            format!(
                "aliases={} existing_type_symbols={} store_view={}",
                imported_inputs.type_aliases.len(),
                env.type_symbols.len(),
                store_view.is_some(),
            ),
        );
        let mut visiting = rustc_hash::FxHashSet::default();
        let mut inserted = 0usize;
        for alias in &imported_inputs.type_aliases {
            if owner_local_type_names.contains(&alias.local_name) {
                continue;
            }
            // Imported source envs are merged before alias hydration, so a raw dep symbol can
            // already occupy this local name. Drop non-owner placeholders and replace them with
            // the hydrated alias surface for the owner's import binding.
            env.type_symbols.remove(&alias.local_name);
            if self.materialize_shallow_type_symbol_into_env_in_view(
                env,
                alias.local_name.as_str(),
                alias.merge_root_canonical.as_str(),
                alias.merge_root_exported.as_str(),
                store_view,
                &mut visiting,
            ) {
                inserted += 1;
            }
        }
        component_meta_trace_event!(
            "materialize_imported_type_aliases_result",
            format!(
                "aliases={} inserted={} final_type_symbols={}",
                imported_inputs.type_aliases.len(),
                inserted,
                env.type_symbols.len(),
            ),
        );
    }

    pub(crate) fn resolve_shallow_symbol_dependency_alias_in_view(
        &self,
        canonical_id: &str,
        exported_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<(
        String,
        String,
        verter_resolver::CachedPreparedImportedTypeAlias,
    )> {
        let (resolved_canonical_id, resolved_exported_name) =
            self.resolve_imported_type_root_in_view(canonical_id, exported_name, store_view);
        let cached = if let Some(cached) = self.clone_cached_prepared_imported_type_alias(
            resolved_canonical_id.as_str(),
            resolved_exported_name.as_str(),
            store_view,
        ) {
            cached
        } else {
            let mut resolver =
                HostImportedEvalResolver::new(self, resolved_canonical_id.as_str(), store_view);
            let mut canonical_dependencies = std::collections::BTreeSet::new();
            let prepared = verter_resolver::prepare_imported_type_alias(
                &mut resolver,
                ImportedTypeAliasResolveRequest {
                    owner_canonical_id: resolved_canonical_id.clone(),
                    import_source: String::new(),
                    local_name: resolved_exported_name.clone(),
                    imported_name: resolved_exported_name.clone(),
                    source_canonical_id: resolved_canonical_id.clone(),
                    exported_name: resolved_exported_name.clone(),
                },
                &mut canonical_dependencies,
            )?;
            let cached = verter_resolver::CachedPreparedImportedTypeAlias {
                decl: prepared.decl,
                canonical_dependencies,
                symbol_dependencies: prepared.symbol_dependencies.clone(),
                requires_source_merge: prepared.requires_source_merge,
            };
            self.cache_prepared_imported_type_alias(
                resolved_canonical_id.as_str(),
                resolved_exported_name.as_str(),
                cached.clone(),
                store_view,
            );
            cached
        };

        let mut hydrated = cached.clone();
        if hydrated.requires_source_merge {
            let mut env = self
                .base_eval_env_in_view(resolved_canonical_id.as_str(), store_view)
                .unwrap_or_default();
            let mut visiting = rustc_hash::FxHashSet::default();
            for child in &hydrated.symbol_dependencies {
                self.materialize_shallow_type_symbol_into_env_in_view(
                    &mut env,
                    child.local_name.as_str(),
                    child.canonical_id.as_str(),
                    child.exported_name.as_str(),
                    store_view,
                    &mut visiting,
                );
            }
            let evaluated = verter_analysis::type_eval::evaluate(&hydrated.decl.body, &mut env);
            if let Some(preferred) = verter_resolver::choose_preferred_imported_type_body(
                Some(evaluated),
                Some(hydrated.decl.body.clone()),
            ) {
                hydrated.decl.body = preferred;
            }
        }

        Some((resolved_canonical_id, resolved_exported_name, hydrated))
    }

    fn materialize_shallow_type_symbol_into_env_in_view(
        &self,
        env: &mut verter_analysis::type_eval::EvalEnv,
        local_name: &str,
        canonical_id: &str,
        exported_name: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
        visiting: &mut rustc_hash::FxHashSet<(String, String, String)>,
    ) -> bool {
        if env.type_symbols.contains_key(local_name) {
            return false;
        }

        let visit_key = (
            local_name.to_string(),
            canonical_id.to_string(),
            exported_name.to_string(),
        );
        if !visiting.insert(visit_key.clone()) {
            return false;
        }

        let prepared = self.resolve_shallow_symbol_dependency_alias_in_view(
            canonical_id,
            exported_name,
            store_view,
        );

        let inserted =
            if let Some((resolved_canonical_id, _resolved_exported_name, prepared)) = prepared {
                if !prepared.requires_source_merge && !prepared.symbol_dependencies.is_empty() {
                    if let Some(dep_env) =
                        self.base_eval_env_in_view(resolved_canonical_id.as_str(), store_view)
                    {
                        env.extend_missing_from_ref(&dep_env);
                    }
                }
                for child in &prepared.symbol_dependencies {
                    if prepared.requires_source_merge {
                        self.materialize_shallow_type_symbol_into_env_in_view(
                            env,
                            child.local_name.as_str(),
                            child.canonical_id.as_str(),
                            child.exported_name.as_str(),
                            store_view,
                            visiting,
                        );
                    }
                }
                let mut decl = prepared.decl.clone();
                decl.name = local_name.to_string();
                env.add_type(decl);
                true
            } else {
                self.base_eval_env_in_view(canonical_id, store_view)
                    .and_then(|dep_env| {
                        let target = dep_env.type_symbols.get(exported_name).cloned();
                        target
                    })
                    .map(|mut decl| {
                        decl.name = local_name.to_string();
                        env.add_type(decl);
                        true
                    })
                    .unwrap_or(false)
            };

        visiting.remove(&visit_key);
        inserted
    }

    fn materialize_imported_runtime_values_into_env_in_view(
        &self,
        snapshot: &FileAnalysisSnapshot,
        owner_local_value_names: &rustc_hash::FxHashSet<String>,
        required_runtime_value_names: Option<&rustc_hash::FxHashSet<String>>,
        env: &mut verter_analysis::type_eval::EvalEnv,
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
        let cache_key = fallthrough_cache_key(
            canonical_id,
            self.config.generic_root_propagation,
            prop_type_overrides,
        );
        let resolution = Arc::new(result.clone());
        self.resolver_runtime().fallthrough.store_node(
            verter_resolver::fallthrough_resolver::root_follow_key(
                canonical_id,
                prop_type_overrides
                    .map(verter_resolver::hash_prop_type_overrides)
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
    ) -> Vec<verter_resolver::fallthrough_resolver::FallthroughBranchResult> {
        match &result.fallthrough_surface {
            verter_analysis::component_meta::FallthroughSurface::Branches { branches } => branches
                .iter()
                .map(
                    |branch| verter_resolver::fallthrough_resolver::FallthroughBranchResult {
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
                            verter_analysis::component_meta::BranchStatus::Unresolved { .. }
                        ),
                    },
                )
                .collect(),
            verter_analysis::component_meta::FallthroughSurface::None { .. } => Vec::new(),
        }
    }

    fn build_runtime_fallthrough_node(
        &self,
        result: &crate::types::FallthroughResolution,
    ) -> verter_resolver::fallthrough_resolver::FallthroughNodeResult {
        let branches = Self::extract_runtime_branch_results(result);
        verter_resolver::fallthrough_resolver::FallthroughNodeResult {
            value: verter_resolver::fallthrough_resolver::FallthroughNodeValue::BranchUnion(
                verter_resolver::fallthrough_resolver::BranchUnionResult {
                    accepted_props: result.accepted_props.clone(),
                    accepted_events: result.accepted_events.clone(),
                    accepted_surface_completeness: result.accepted_surface_completeness,
                    fallthrough_surface: result.fallthrough_surface.clone(),
                    all_resolved: matches!(
                        result.accepted_surface_completeness,
                        verter_analysis::component_meta::AcceptedSurfaceCompleteness::Exact
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
    ) -> verter_resolver::fallthrough_resolver::FallthroughNodeResult {
        let branches = Self::extract_runtime_branch_results(result);
        verter_resolver::fallthrough_resolver::FallthroughNodeResult {
            value: verter_resolver::fallthrough_resolver::FallthroughNodeValue::RootFollow(
                verter_resolver::fallthrough_resolver::RootFollowResult {
                    accepted_props: result.accepted_props.clone(),
                    accepted_events: result.accepted_events.clone(),
                    accepted_surface_completeness: result.accepted_surface_completeness,
                    fallthrough_surface: result.fallthrough_surface.clone(),
                    has_single_root: matches!(
                        result.fallthrough_surface,
                        verter_analysis::component_meta::FallthroughSurface::Branches { ref branches } if branches.len() == 1
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
    ) -> verter_resolver::fallthrough_resolver::FallthroughNodeResult {
        verter_resolver::fallthrough_resolver::FallthroughNodeResult {
            value: verter_resolver::fallthrough_resolver::FallthroughNodeValue::ChildSurfaceFollow(
                verter_resolver::fallthrough_resolver::ChildSurfaceResult {
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
                        verter_analysis::component_meta::AcceptedSurfaceCompleteness::Exact
                    ),
                },
            ),
            facts: result.fact_versions.clone(),
            diagnostics: Vec::new(),
        }
    }

    fn build_runtime_intrinsic_surface_node(
        &self,
        members: &[verter_analysis::html_intrinsics::OwnedIntrinsicMember],
    ) -> verter_resolver::fallthrough_resolver::FallthroughNodeResult {
        let mut attr_names = Vec::new();
        let mut event_names = Vec::new();
        for member in members {
            match member.kind {
                verter_analysis::html_intrinsics::IntrinsicMemberKind::Attr => {
                    attr_names.push(member.name.clone());
                }
                verter_analysis::html_intrinsics::IntrinsicMemberKind::Listener => {
                    event_names.push(member.name.clone());
                }
            }
        }

        verter_resolver::fallthrough_resolver::FallthroughNodeResult {
            value: verter_resolver::fallthrough_resolver::FallthroughNodeValue::IntrinsicSurface(
                verter_resolver::fallthrough_resolver::IntrinsicSurfaceResult {
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
    ) -> verter_resolver::fallthrough_resolver::FallthroughNodeResult {
        let mut consumed_names = resolved.bindings.attrs.clone();
        consumed_names.extend(resolved.bindings.listeners.iter().cloned());
        consumed_names.sort();
        consumed_names.dedup();

        verter_resolver::fallthrough_resolver::FallthroughNodeResult {
            value: verter_resolver::fallthrough_resolver::FallthroughNodeValue::ConsumedBindings(
                verter_resolver::fallthrough_resolver::ConsumedBindingsResult {
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
        node: verter_resolver::fallthrough_resolver::FallthroughNodeResult,
    ) -> Option<crate::types::FallthroughResolution> {
        match node.value {
            verter_resolver::fallthrough_resolver::FallthroughNodeValue::ChildSurfaceFollow(
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
        node: verter_resolver::fallthrough_resolver::FallthroughNodeResult,
    ) -> Option<crate::types::FallthroughResolution> {
        match node.value {
            verter_resolver::fallthrough_resolver::FallthroughNodeValue::BranchUnion(
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
        node: verter_resolver::fallthrough_resolver::FallthroughNodeResult,
    ) -> Option<crate::types::FallthroughResolution> {
        match node.value {
            verter_resolver::fallthrough_resolver::FallthroughNodeValue::RootFollow(
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
        node: verter_resolver::fallthrough_resolver::FallthroughNodeResult,
    ) -> Option<Vec<verter_analysis::html_intrinsics::OwnedIntrinsicMember>> {
        match node.value {
            verter_resolver::fallthrough_resolver::FallthroughNodeValue::IntrinsicSurface(
                intrinsic,
            ) => Some(intrinsic.members),
            _ => None,
        }
    }

    fn runtime_consumed_bindings_to_resolution(
        &self,
        node: verter_resolver::fallthrough_resolver::FallthroughNodeResult,
    ) -> Option<ResolvedConsumedBindings> {
        match node.value {
            verter_resolver::fallthrough_resolver::FallthroughNodeValue::ConsumedBindings(
                consumed,
            ) => Some(ResolvedConsumedBindings {
                bindings: verter_analysis::component_meta::ConsumedRootBindings {
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
        if let Some(source) = self.read_analysis_source(resolved_canonical_id) {
            return Some(source);
        }

        let dep_id = self
            .resolve_loaded_dependency_canonical(
                owner_canonical,
                specifier,
                verter_vfs::ResolveRequestKind::SfcSrcAttr,
            )
            .or_else(|| {
                self.resolve_loaded_dependency_canonical(
                    owner_canonical,
                    specifier,
                    verter_vfs::ResolveRequestKind::EsmImport,
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
                    let ctx = verter_vfs::ResolutionContext {
                        phase: verter_vfs::ResolvePhase::CodegenBlocker,
                        kind: if import.is_type_only {
                            verter_vfs::ResolveRequestKind::TypeImport
                        } else {
                            verter_vfs::ResolveRequestKind::EsmImport
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

            use verter_vfs::{ResolvePhase as P, ResolveRequestKind as K};
            for (phase, kind) in [
                (P::CodegenBlocker, K::EsmImport),
                (P::CodegenBlocker, K::TypeImport),
                (P::ProviderGraph, K::EsmImport),
                (P::ProviderGraph, K::TypeImport),
            ] {
                let exact = if exact_resolution_uses_type_preferred_target(phase, kind) {
                    self.derive_type_preferred_exact_target(&normalized_resolution)
                        .map(|target| verter_vfs::ExactResolution {
                            specifier: resolution.specifier.clone(),
                            phase,
                            kind,
                            resolved_canonical_id: Some(target),
                            possible_canonical_ids: Vec::new(),
                        })
                } else {
                    Some(verter_vfs::ExactResolution {
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

    pub(crate) fn load_export_graph_analysis_in_view(
        &self,
        canonical_or_alias: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<(
        FileKind,
        verter_analysis::ScriptAnalysisSnapshot,
        Vec<verter_analysis::ExportSignature>,
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
            self.ensure_shallow_imported_dependency_state_in_view(&canonical, store_view)
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

        let source = self.read_analysis_source_in_view(&canonical, store_view)?;
        if canonical.ends_with(".vue") {
            let (snapshot, _) = crate::parse::parse_vue_snapshot(
                &canonical,
                source.as_ref(),
                self.config.effective_scope(),
            );
            Some((
                FileKind::VueSfc,
                snapshot.script_analysis,
                snapshot.export_signatures,
            ))
        } else {
            let snapshot = crate::parse::parse_non_sfc_snapshot(&canonical, source.as_ref());
            Some((
                FileKind::NonSfc,
                snapshot.script_analysis,
                snapshot.export_signatures,
            ))
        }
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
        let ctx = verter_vfs::ResolutionContext {
            phase: verter_vfs::ResolvePhase::CodegenBlocker,
            kind: verter_vfs::ResolveRequestKind::EsmImport,
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
    let type_bindings = VerterHost::owner_generic_type_bindings(owner_env);
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

fn collect_required_runtime_value_names(
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
    collect_required_runtime_value_names_from_parts(&owner_snapshot, owner_eval_source, owner_env)
}

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

fn collect_required_runtime_value_names_from_parts(
    owner_snapshot: &ImportedEvalOwnerSnapshot<'_>,
    owner_eval_source: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
) -> rustc_hash::FxHashSet<String> {
    let mut required = rustc_hash::FxHashSet::default();
    if owner_eval_source.is_empty() {
        return required;
    }

    let type_bindings = VerterHost::owner_generic_type_bindings(owner_env);
    let mut active_locals = rustc_hash::FxHashSet::default();
    let macro_type_params =
        verter_analysis::type_eval_build::collect_define_macro_type_params(owner_eval_source);
    let mut define_props_index = 0usize;
    let mut define_emits_index = 0usize;
    let mut define_slots_index = 0usize;
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

    for mac in owner_snapshot.macros.iter() {
        if mac.is_type_based {
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
                    collect_runtime_value_names_from_expr(
                        expr,
                        owner_env,
                        &type_bindings,
                        &mut active_locals,
                        &mut required,
                    );
                }
            }
            for type_reference in &mac.type_references {
                collect_required_runtime_value_names_for_symbol(
                    type_reference,
                    owner_env,
                    &type_bindings,
                    &mut active_locals,
                    &mut required,
                );
            }
        }

        for field in &mac.prop_fields {
            if let Some(type_ann) = field.type_annotation.as_deref() {
                let expr = verter_analysis::type_expr_lower::parse_type_annotation(type_ann);
                if !expr.is_unknown() {
                    collect_runtime_value_names_from_expr(
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
                    collect_runtime_value_names_from_expr(
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
                            collect_runtime_value_names_from_expr(
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
            collect_runtime_value_names_from_expr(
                &expr,
                owner_env,
                &type_bindings,
                &mut active_locals,
                &mut required,
            );
        }
    }

    required
}

fn collect_required_runtime_value_names_for_symbol(
    symbol: &str,
    owner_env: &verter_analysis::type_eval::EvalEnv,
    type_bindings: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    active_locals: &mut rustc_hash::FxHashSet<String>,
    required: &mut rustc_hash::FxHashSet<String>,
) {
    if owner_env.type_symbols.contains_key(symbol) || type_bindings.contains_key(symbol) {
        collect_runtime_value_names_from_expr(
            &verter_analysis::type_expr::TypeExpr::named(symbol),
            owner_env,
            type_bindings,
            active_locals,
            required,
        );
    }
}

fn collect_runtime_value_names_from_function(
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
            verter_analysis::type_expr::TypeExpr::type_parameter(param.clone()),
        );
        if let Some(constraint) = param.constraint.as_deref() {
            collect_runtime_value_names_from_expr(
                constraint,
                owner_env,
                &local_bindings,
                active_locals,
                required,
            );
        }
        if let Some(default) = param.default.as_deref() {
            collect_runtime_value_names_from_expr(
                default,
                owner_env,
                &local_bindings,
                active_locals,
                required,
            );
        }
    }
    for param in &func.parameters {
        collect_runtime_value_names_from_expr(
            &param.ty,
            owner_env,
            &local_bindings,
            active_locals,
            required,
        );
    }
    if let Some(return_type) = func.return_type.as_deref() {
        collect_runtime_value_names_from_expr(
            return_type,
            owner_env,
            &local_bindings,
            active_locals,
            required,
        );
    }
}

fn collect_runtime_value_names_from_expr(
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
        TypeExpr::TypeParameter(param) => {
            if let Some(constraint) = param.constraint.as_deref() {
                collect_runtime_value_names_from_expr(
                    constraint,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
            if let Some(default) = param.default.as_deref() {
                collect_runtime_value_names_from_expr(
                    default,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for ty in types.iter() {
                collect_runtime_value_names_from_expr(
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
        | TypeExpr::Parenthesized(element) => collect_runtime_value_names_from_expr(
            element,
            owner_env,
            type_bindings,
            active_locals,
            required,
        ),
        TypeExpr::Tuple { elements, .. } => {
            for element in elements.iter() {
                collect_runtime_value_names_from_expr(
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
                    ObjectMember::Property(prop) => collect_runtime_value_names_from_expr(
                        &prop.ty,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                    ),
                    ObjectMember::IndexSignature(idx) => {
                        collect_runtime_value_names_from_expr(
                            &idx.key_type,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        );
                        collect_runtime_value_names_from_expr(
                            &idx.value_type,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        );
                    }
                    ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                        collect_runtime_value_names_from_function(
                            func,
                            owner_env,
                            type_bindings,
                            active_locals,
                            required,
                        );
                    }
                    ObjectMember::Method(method) => collect_runtime_value_names_from_function(
                        &method.function,
                        owner_env,
                        type_bindings,
                        active_locals,
                        required,
                    ),
                }
            }
        }
        TypeExpr::Function(func) => collect_runtime_value_names_from_function(
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
            if let Some(bound) = type_bindings.get(&**name) {
                let binding_guard = format!("$type:{name}");
                if !active_locals.insert(binding_guard.clone()) {
                    return;
                }
                collect_runtime_value_names_from_expr(
                    bound,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
                active_locals.remove(&binding_guard);
                return;
            }

            if let Some(decl) = owner_env.type_symbols.get(&**name) {
                if !active_locals.insert(name.to_string()) {
                    return;
                }

                let mut local_bindings = type_bindings.clone();
                for (index, param) in decl.type_parameters.iter().enumerate() {
                    if let Some(constraint) = param.constraint.as_deref() {
                        collect_runtime_value_names_from_expr(
                            constraint,
                            owner_env,
                            &local_bindings,
                            active_locals,
                            required,
                        );
                    }
                    if let Some(default) = param.default.as_deref() {
                        collect_runtime_value_names_from_expr(
                            default,
                            owner_env,
                            &local_bindings,
                            active_locals,
                            required,
                        );
                    }
                    let arg = type_arguments.get(index).cloned().or_else(|| {
                        Some(verter_analysis::type_expr::TypeExpr::type_parameter(
                            param.clone(),
                        ))
                    });
                    if let Some(arg) = arg {
                        local_bindings.insert(param.name.to_string(), arg);
                    }
                }

                collect_runtime_value_names_from_expr(
                    &decl.body,
                    owner_env,
                    &local_bindings,
                    active_locals,
                    required,
                );
                active_locals.remove(&**name);
                return;
            }

            for arg in type_arguments.iter() {
                collect_runtime_value_names_from_expr(
                    arg,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::TypeOf(value_ref) => {
            if let Some(root) = value_ref.path.first() {
                required.insert(root.clone());
            }
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_runtime_value_names_from_expr(
                object,
                owner_env,
                type_bindings,
                active_locals,
                required,
            );
            collect_runtime_value_names_from_expr(
                index,
                owner_env,
                type_bindings,
                active_locals,
                required,
            );
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            for ty in [check, extends, true_type, false_type] {
                collect_runtime_value_names_from_expr(
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
            collect_runtime_value_names_from_expr(
                source,
                owner_env,
                type_bindings,
                active_locals,
                required,
            );
            collect_runtime_value_names_from_expr(
                value,
                owner_env,
                type_bindings,
                active_locals,
                required,
            );
            if let Some(name_type) = name_type.as_deref() {
                collect_runtime_value_names_from_expr(
                    name_type,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::TemplateLiteral { expressions, .. } => {
            for expr in expressions.iter() {
                collect_runtime_value_names_from_expr(
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
            verter_analysis::type_expr::TypeExpr::type_parameter(param.clone()),
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
        TypeExpr::TypeParameter(param) => {
            if let Some(constraint) = param.constraint.as_deref() {
                collect_slot_eval_import_names_from_expr_with_mode(
                    constraint,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
            }
            if let Some(default) = param.default.as_deref() {
                collect_slot_eval_import_names_from_expr_with_mode(
                    default,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                    mode,
                );
            }
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for ty in types.iter() {
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
            for element in elements.iter() {
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
            if let Some(bound) = type_bindings.get(&**name) {
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

            if let Some(decl) = owner_env.type_symbols.get(&**name) {
                let decl_guard = slot_import_guard("decl", mode, name);
                if !active_locals.insert(decl_guard.clone()) {
                    return;
                }

                let mut local_bindings = type_bindings.clone();
                for (index, param) in decl.type_parameters.iter().enumerate() {
                    let arg = type_arguments.get(index).cloned().or_else(|| {
                        Some(verter_analysis::type_expr::TypeExpr::type_parameter(
                            param.clone(),
                        ))
                    });
                    if let Some(arg) = arg {
                        local_bindings.insert(param.name.to_string(), arg);
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

            required.insert(name.to_string());
            if should_recurse_surface_type_arguments(name) {
                for arg in type_arguments.iter() {
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
            for expr in expressions.iter() {
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
            verter_analysis::type_expr::TypeExpr::type_parameter(param.clone()),
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
            if let Some(bound) = type_bindings.get(&**name) {
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

            if let Some(decl) = owner_env.type_symbols.get(&**name) {
                let decl_guard = slot_import_guard("decl", mode, name);
                if !active_locals.insert(decl_guard.clone()) {
                    return;
                }

                let mut local_bindings = type_bindings.clone();
                for (index, param) in decl.type_parameters.iter().enumerate() {
                    let arg = type_arguments.get(index).cloned().or_else(|| {
                        Some(verter_analysis::type_expr::TypeExpr::type_parameter(
                            param.clone(),
                        ))
                    });
                    if let Some(arg) = arg {
                        local_bindings.insert(param.name.to_string(), arg);
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

            required.insert(name.to_string());
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
            for ty in types.iter() {
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

#[allow(clippy::too_many_arguments)]
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
        TypeExpr::TypeParameter(param) => {
            if let Some(constraint) = param.constraint.as_deref() {
                collect_surface_eval_import_names_from_expr(
                    constraint,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
            if let Some(default) = param.default.as_deref() {
                collect_surface_eval_import_names_from_expr(
                    default,
                    owner_env,
                    type_bindings,
                    active_locals,
                    required,
                );
            }
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            for ty in types.iter() {
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
            for element in elements.iter() {
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
            if let Some(bound) = type_bindings.get(&**name) {
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

            if let Some(decl) = owner_env.type_symbols.get(&**name) {
                if !active_locals.insert(name.to_string()) {
                    return;
                }

                let mut local_bindings = type_bindings.clone();
                for (index, param) in decl.type_parameters.iter().enumerate() {
                    let arg = type_arguments.get(index).cloned().or_else(|| {
                        Some(verter_analysis::type_expr::TypeExpr::type_parameter(
                            param.clone(),
                        ))
                    });
                    if let Some(arg) = arg {
                        local_bindings.insert(param.name.to_string(), arg);
                    }
                }

                collect_surface_eval_import_names_from_expr(
                    &decl.body,
                    owner_env,
                    &local_bindings,
                    active_locals,
                    required,
                );
                active_locals.remove(&**name);
                return;
            }

            required.insert(name.to_string());
            if should_recurse_surface_type_arguments(name) {
                for arg in type_arguments.iter() {
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
            for expr in expressions.iter() {
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
            if let Some(bound) = type_bindings.get(&**name) {
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

            if let Some(decl) = owner_env.type_symbols.get(&**name) {
                if !active_locals.insert(name.to_string()) {
                    return;
                }

                let mut local_bindings = type_bindings.clone();
                for (index, param) in decl.type_parameters.iter().enumerate() {
                    let arg = type_arguments.get(index).cloned().or_else(|| {
                        Some(verter_analysis::type_expr::TypeExpr::type_parameter(
                            param.clone(),
                        ))
                    });
                    if let Some(arg) = arg {
                        local_bindings.insert(param.name.to_string(), arg);
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
                active_locals.remove(&**name);
                return;
            }

            required.insert(name.to_string());
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
            for ty in types.iter() {
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
            for ty in types.iter() {
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
    let _trace = component_meta_trace_scope!(
        "extract_component_meta",
        format!(
            "owner={} macros={} resolved_macros={} resolved_type_registry={} has_evaluated_types={} include_fallthrough={}",
            canonical,
            snapshot.macros.len(),
            resolved_macros.len(),
            resolved_type_registry.len(),
            evaluated_types.is_some(),
            include_fallthrough,
        ),
    );
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

    if include_fallthrough {
        if let Some(resolution) = host.resolve_fallthrough_surface(&canonical) {
            meta.accepted_props = resolution.accepted_props;
            meta.accepted_events = resolution.accepted_events;
            meta.accepted_surface_completeness = resolution.accepted_surface_completeness;
            meta.fallthrough_surface = resolution.fallthrough_surface;
            component_meta_trace_event!(
                "extract_component_meta_fallthrough_surface",
                format!(
                    "owner={} accepted_props={} accepted_events={} completeness={:?}",
                    canonical,
                    meta.accepted_props.len(),
                    meta.accepted_events.len(),
                    meta.accepted_surface_completeness,
                ),
            );
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

pub(crate) fn extract_component_meta_from_resolved(
    host: &VerterHost,
    canonical_or_alias: &str,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
    include_fallthrough: bool,
) -> verter_analysis::component_meta::ComponentMetaAnalysis {
    let resolved_macros = resolver_component_meta_resolved_macros(
        resolved.snapshot.macros.as_ref(),
        &resolved.resolved_macros,
    );
    let resolved_type_registry =
        resolver_component_meta_type_registry(&resolved.resolved_type_registry);
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

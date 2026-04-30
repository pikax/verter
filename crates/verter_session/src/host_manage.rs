//! `impl VerterHost` â€” file management, analysis, and diagnostics methods.
//!
//! Contains [`VerterHost::remove`], [`VerterHost::get_analysis`],
//! [`VerterHost::get_diagnostics`], and [`VerterHost::set_import_dependencies`].

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use crate::hash::compile_profile_hash;
use crate::id::canonicalize_id;
use crate::resolver_core::{
    collect_dynamic_root_candidates_from_type,
    component_meta_resolved_macros as resolver_component_meta_resolved_macros,
    component_meta_type_registry as resolver_component_meta_type_registry, fallthrough_cache_key,
    get_export_span_follow_reexports_from_graph as resolver_get_export_span_follow_reexports_from_graph,
    known_spread_keys_from_type_expr, materialize_imported_runtime_values_into_env,
    push_partial_reason,
    resolve_exports_from_graph_best_effort as resolver_resolve_exports_from_graph_best_effort,
    resolve_fallthrough_surface as resolver_resolve_fallthrough_surface,
    resolve_named_export_from_graph as resolver_resolve_named_export_from_graph,
    resolve_usage_prop_type, DynamicRootCandidate, ExportGraphFileKind, ExportGraphResolver,
    ExportSurface, FallthroughComputeHost, FallthroughRequestHost, FallthroughResolutionView,
    FallthroughResolverHost, ImportedRuntimeValueResolver, RequestSource, ResolvedConsumedBindings,
    SingleflightRole, StoreView,
};
use crate::shared::write_lock;
use crate::types::*;
use crate::VerterHost;

// ──────────────────────────────────────────────────────────────────────────
// Phase 11c (parent §11.3): private sub-modules under `host_manage/`. Public
// surface remains rooted at `crate::host_manage::*`; siblings are
// `pub(crate) mod` so any `pub(crate)` items they re-expose stay scoped to
// the session crate.
// ──────────────────────────────────────────────────────────────────────────

pub(crate) mod analysis_io;
pub(crate) mod component_meta_entry;
pub(crate) mod eval_env;
pub(crate) mod eval_program;
pub(crate) mod fallthrough;
pub(crate) mod intrinsic_projection;
pub(crate) mod prepared_decl;

/// Resolve a relative import specifier to a canonical ID.
///
/// If the specifier is relative (`./` or `../`), resolves it relative to
/// the parent directory of `base_canonical_id`. For non-relative specifiers,
/// tries the host's `resolve_eval_dependency_canonical` first, then
/// falls back to returning the specifier as-is.
#[allow(dead_code)]
pub(crate) fn resolve_relative_specifier_to_canonical(
    base_canonical_id: &str,
    specifier: &str,
    host: &crate::VerterHost,
) -> String {
    // First try the host's eval dependency resolution (handles companion
    // files, .js → .d.ts, etc.)
    if let Some(resolved) = host.resolve_eval_dependency_canonical(specifier) {
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
        if host.is_evalable(&resolved) {
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
                if host.is_evalable(&candidate) {
                    return candidate;
                }
            }
        }
        // Try appending extensions
        for ext in &[".ts", ".d.ts", ".tsx", ".js", ".vue"] {
            let candidate = format!("{resolved}{ext}");
            if host.is_evalable(&candidate) {
                return candidate;
            }
        }
        // Try /index variants
        for ext in &["/index.ts", "/index.d.ts", "/index.js"] {
            let candidate = format!("{resolved}{ext}");
            if host.is_evalable(&candidate) {
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

pub(in crate::host_manage) fn read_analysis_source_result_detail(
    canonical_id: &str,
    source_kind: &str,
    bytes: usize,
    missing: bool,
) -> String {
    let mut detail = format!(
        "owner={} source={} bytes={}",
        canonical_id, source_kind, bytes,
    );
    if missing {
        detail.push_str(" missing=true");
    }
    detail
}

#[cfg(test)]
fn workspace_vfs_source_kind(detail: Option<String>) -> String {
    match detail {
        Some(detail) if !detail.is_empty() => format!("workspace-vfs {detail}"),
        _ => "workspace-vfs".to_string(),
    }
}

pub(crate) fn resolve_eval_dependency_canonical_with(
    dep_canonical: &str,
    mut has_candidate: impl FnMut(&str) -> bool,
) -> Option<String> {
    if dep_canonical.is_empty() {
        return None;
    }

    let has_explicit_extension = std::path::Path::new(dep_canonical).extension().is_some();

    let prefers_type_companion = dep_canonical.ends_with(".js")
        || dep_canonical.ends_with(".jsx")
        || dep_canonical.ends_with(".mjs")
        || dep_canonical.ends_with(".cjs");

    let mut candidates = Vec::new();
    for (suffix, companion_suffix) in [
        (".esm-bundler.js", ".d.ts"),
        (".esm-browser.js", ".d.ts"),
        (".esm-browser.prod.js", ".d.ts"),
        (".global.js", ".d.ts"),
        (".global.prod.js", ".d.ts"),
        (".cjs.js", ".d.ts"),
        (".cjs.prod.js", ".d.ts"),
    ] {
        if let Some(stem) = dep_canonical.strip_suffix(suffix) {
            candidates.push(format!("{stem}{companion_suffix}"));
        }
    }
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

    if !prefers_type_companion && has_explicit_extension && has_candidate(dep_canonical) {
        return Some(dep_canonical.to_string());
    }

    for candidate in candidates {
        if has_candidate(&candidate) {
            return Some(candidate);
        }
    }

    if !has_explicit_extension && has_candidate(dep_canonical) {
        return Some(dep_canonical.to_string());
    }

    if prefers_type_companion && has_candidate(dep_canonical) {
        return Some(dep_canonical.to_string());
    }

    None
}

/// Build a dep_edges map (import specifier → resolved canonical ID) from
/// the host's `import_routes`. Used when building prepared
/// declarations so that `name_resolution` and `external_deps` contain
/// resolved canonical IDs rather than raw import specifiers.
pub(in crate::host_manage) fn dep_edges_from_resolutions(
    resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
) -> rustc_hash::FxHashMap<String, String> {
    let mut edges = rustc_hash::FxHashMap::default();
    for (specifier, res) in resolutions {
        // Prefer the highest-priority candidate from possible_canonical_ids
        // (e.g. .d.ts over .js) when available, otherwise fall back to
        // resolved_canonical_id / effective_target.
        let target = if !res.possible_canonical_ids.is_empty() {
            res.possible_canonical_ids
                .iter()
                .min_by_key(|c| crate::types::extension_priority(c))
                .cloned()
        } else {
            res.resolved_canonical_id
                .clone()
                .or_else(|| res.effective_target().map(str::to_string))
        };
        if let Some(target) = target {
            edges.insert(specifier.clone(), target);
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

// Plan §3 Commit 5: the legacy file/stderr trace is deleted. The
// remaining infrastructure below is the thin shim that keeps
// component_meta_trace_scope! / component_meta_trace_event! macro
// call sites compiling — each now pushes `StructuredComponentMetaEvent::Custom`
// into the active request's accumulator via
// `component_meta_trace_structured!`. When no accumulator is
// installed, the push is a no-op.
//
// The former `ComponentMetaTraceEvent` / `ComponentMetaTraceContext` /
// `ComponentMetaTraceLine` types, the `COMPONENT_META_TRACE_STACK`
// TLS, and the `format_component_meta_trace_line` /
// `component_meta_trace_write_line` / `component_meta_trace_output_lock` /
// `component_meta_trace_output_path` / `component_meta_trace_next_span_id` /
// `component_meta_trace_enabled` helpers have all been removed
// (plan §0.1 clean-cut rule).

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(in crate::host_manage) struct ParsedEvalProgramCacheKey {
    pub(in crate::host_manage) host_instance_id: u64,
    pub(in crate::host_manage) canonical_id: String,
    pub(in crate::host_manage) source_type: oxc_span::SourceType,
    /// §4.6 Sub-task C: `whole_hash` in the key partitions entries by content
    /// version so a cross-generation request can't collide with a different
    /// version's stored entry. The post-lookup `entry.whole_hash == whole_hash`
    /// check becomes redundant (enforced at key level) but is kept as a
    /// defense-in-depth assertion.
    pub(in crate::host_manage) whole_hash: Hash16,
}

#[derive(Clone)]
pub(in crate::host_manage) struct ParsedEvalProgramCacheEntry {
    pub(in crate::host_manage) whole_hash: Hash16,
    pub(in crate::host_manage) parse_failed: bool,
    pub(in crate::host_manage) program: Rc<crate::ParsedEvalProgram>,
}

#[derive(Clone)]
pub(in crate::host_manage) struct ParsedTypeResolutionContextCacheEntry {
    pub(in crate::host_manage) whole_hash: Hash16,
    pub(in crate::host_manage) type_context: Rc<crate::ParsedTypeResolutionContext>,
}

/// Thin adapter that implements
/// [`verter_compiler::utils::oxc::vue::resolve_type::cache_keys::NamedTypeCache`]
/// on top of the project-global
/// [`SemanticGraphStore`](crate::semantic_query_memo::SemanticGraphStore)
/// via [`HostResolvedNamedTypeKey`](crate::project_type_store::HostResolvedNamedTypeKey).
/// Holds an `Arc<SemanticGraphStore>` plus the `(canonical_id, whole_hash)`
/// tuple for this context's entries. A new adapter is constructed per
/// `build_type_context` call so child contexts created by
/// `instantiate_type_params_ctx` share the same graph handle without
/// re-building scoping metadata.
///
/// Read-path contract: `get` performs one `DashMap::get` on the graph's
/// named-type identity map plus one node-arena read plus one `Arc::clone`.
/// There is no `execute_cooperative` round-trip, no `DepSignature`
/// allocation, and no `ProjectSemanticDispatch` dispatch on the hot path —
/// entries are whole-hash-scoped so reads are self-validating within one
/// project generation. Writes record the identity mapping and intern the
/// payload node in the graph arena.
#[derive(Debug)]
pub(in crate::host_manage) struct HostNamedTypeCacheAdapter {
    pub(in crate::host_manage) graph:
        std::sync::Arc<crate::semantic_query_memo::SemanticGraphStore>,
    /// Shared `Arc<str>` so adapter clones (one per child type context from
    /// `instantiate_type_params_ctx`) don't each allocate a fresh `String`.
    pub(in crate::host_manage) canonical_id: Arc<str>,
    pub(in crate::host_manage) whole_hash: Hash16,
}

impl verter_compiler::utils::oxc::vue::resolve_type::cache_keys::NamedTypeCache
    for HostNamedTypeCacheAdapter
{
    fn get(
        &self,
        key: &verter_compiler::utils::oxc::vue::resolve_type::cache_keys::ResolvedNamedTypeCacheKey,
    ) -> Option<std::sync::Arc<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements>>
    {
        // `canonical_id` is `Arc<str>` — clone is a refcount bump, no alloc.
        // `inner` still clones `Box<[u8]>` for `name` (cache-key shape
        // inherited from parser crate); future follow-up can lift that to
        // `Arc<[u8]>` symmetrically.
        let host_key = crate::project_type_store::HostResolvedNamedTypeKey {
            canonical_id: Arc::clone(&self.canonical_id),
            whole_hash: self.whole_hash,
            inner: key.clone(),
        };
        self.graph.get_resolved_named_type(&host_key)
    }

    fn insert(
        &self,
        key: verter_compiler::utils::oxc::vue::resolve_type::cache_keys::ResolvedNamedTypeCacheKey,
        value: std::sync::Arc<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements>,
    ) {
        let host_key = crate::project_type_store::HostResolvedNamedTypeKey {
            canonical_id: Arc::clone(&self.canonical_id),
            whole_hash: self.whole_hash,
            inner: key,
        };
        self.graph.insert_resolved_named_type(host_key, value);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ExternalTypeResolutionInputs {
    pub(crate) raw_source: Arc<str>,
    pub(crate) cached_parse: Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
    pub(crate) whole_hash: Hash16,
    pub(crate) eval_source: Arc<str>,
    pub(crate) analysis:
        Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource>,
    pub(crate) analysis_cache_hit: bool,
}

thread_local! {
    pub(in crate::host_manage) static HOST_PARSED_EVAL_PROGRAM_CACHE: RefCell<
        rustc_hash::FxHashMap<ParsedEvalProgramCacheKey, ParsedEvalProgramCacheEntry>
    > = RefCell::new(rustc_hash::FxHashMap::default());
    pub(in crate::host_manage) static HOST_PARSED_TYPE_CONTEXT_CACHE: RefCell<
        rustc_hash::FxHashMap<ParsedEvalProgramCacheKey, ParsedTypeResolutionContextCacheEntry>
    > = RefCell::new(rustc_hash::FxHashMap::default());
    // TODO(follow-up): Move HOST_PARSED_EVAL_PROGRAM_CACHE and HOST_PARSED_TYPE_CONTEXT_CACHE
    // to host-owned caches. Currently blocked by !Send types:
    // - ParsedEvalProgram uses self_cell with oxc_allocator::Allocator (!Send arena)
    // - ParsedTypeResolutionContext's dependent contains Rc<RefCell<>> fields
    // The proper fix requires converting OXC arena types to Send+Sync (upstream concern).
}

// ──────────────────────────────────────────────────────────────────────────
// Phase 3: Component-meta options + fingerprint + fence validator
// ──────────────────────────────────────────────────────────────────────────

/// Stable-shape options passed to [`VerterHost::get_component_meta`]. Only
/// output-affecting fields participate in the
/// [`ComponentMetaResultDb`](crate::component_meta_result_db::ComponentMetaResultDb)
/// fingerprint — request ids, trace flags, and caller metadata are not
/// represented here.
///
/// The struct is manually versioned via [`ComponentMetaOptions::SCHEMA`].
/// Any future field addition bumps the schema and participates in the
/// fingerprint, so a cache populated under one options shape cannot alias
/// another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentMetaOptions {
    /// Projection mode. `false` = native payload; `true` = compat
    /// projection. Today `false` is the only public path through
    /// `get_component_meta`; future compat integration expands this.
    pub compat: bool,
    /// Include fallthrough surface (accepted_props / accepted_events) in
    /// the returned analysis. Always true for the public API today.
    pub include_fallthrough: bool,
}

impl Default for ComponentMetaOptions {
    fn default() -> Self {
        Self {
            compat: false,
            include_fallthrough: true,
        }
    }
}

impl ComponentMetaOptions {
    /// Manual schema version. Bumping this field invalidates every cached
    /// entry even if every option field has the same runtime value.
    pub const SCHEMA: u32 = 1;
}

/// Compute a stable `Hash16` fingerprint over `options`. Uses a manually
/// stable encoding so the fingerprint cannot drift across refactors that
/// change Rust's struct layout.
pub(crate) fn component_meta_options_fingerprint(options: &ComponentMetaOptions) -> Hash16 {
    let mut buf: Vec<u8> = Vec::with_capacity(16);
    buf.extend_from_slice(&ComponentMetaOptions::SCHEMA.to_le_bytes());
    buf.push(u8::from(options.compat));
    buf.push(u8::from(options.include_fallthrough));
    crate::hash::hash_16(&buf)
}

/// [`FenceValidator`](crate::completion_fence::FenceValidator) backed by a
/// live [`VerterHost`]. Reports whether an observed dep-fact still matches
/// the host's current state — used by Phase 3 cache revalidation and
/// Phase 4 cold-build retry loops.
pub(crate) struct HostFenceValidator<'a> {
    pub host: &'a VerterHost,
}

impl crate::completion_fence::FenceValidator for HostFenceValidator<'_> {
    fn validate(&self, canonical_id: &str, version: &crate::semantic_query::DepVersion) -> bool {
        match version {
            crate::semantic_query::DepVersion::WholeHash(expected) => {
                // Phase 5a §6.6 / A8: ambient virtual ids
                // (`ambient:/<tag>/<canonical>`) bypass the shallow-file
                // map (they have no `ShallowFileState`) and route to the
                // workspace's ambient lib registry.
                if canonical_id.starts_with("ambient:/") {
                    return self.validate_ambient_whole_hash(canonical_id, *expected);
                }
                match self.host.shallow_file_state(canonical_id) {
                    Some(state) => state.whole_hash == *expected,
                    None => false,
                }
            }
            crate::semantic_query::DepVersion::ProjectGeneration(expected) => {
                self.host.project_type_store.project_generation() == *expected
            }
            // Route-generation facts are not yet emitted by the resolver;
            // treat them as valid so they do not spuriously invalidate
            // cache entries once emitters come online. Until the emission
            // site exists, no cache entry can carry this variant.
            crate::semantic_query::DepVersion::RouteGeneration(_) => true,
        }
    }
}

impl HostFenceValidator<'_> {
    /// Phase 5a §6.6 / A8: validate an ambient virtual id against the
    /// workspace's ambient lib registry. Returns `true` iff the parsed
    /// `ProjectStableKey + canonical` pair points at an entry whose
    /// content_hash matches `expected`.
    ///
    /// Returns `false` when the virtual id is malformed, the project is
    /// gone (re-registration with new content), or the entry was unregistered.
    fn validate_ambient_whole_hash(
        &self,
        virtual_id: &str,
        expected: crate::semantic_query::HashValue,
    ) -> bool {
        let Some(stripped) = virtual_id.strip_prefix("ambient:/") else {
            return false;
        };
        let Some((tag, canonical)) = stripped.split_once('/') else {
            return false;
        };
        let Some(stable_key) = verter_workspace::ProjectStableKey::parse_hex_tag(tag) else {
            return false;
        };
        let view = self.host.workspace().ambient_libs_view();
        view.by_project
            .get(&stable_key)
            .and_then(|p| p.libs.get(canonical))
            .map(|entry| entry.content_hash == expected)
            .unwrap_or(false)
    }
}

/// Push a structured event into the active request's accumulator.
/// No-op when no request context is installed.
pub fn push_structured_event(event: crate::component_meta_audit::StructuredComponentMetaEvent) {
    if let Some(acc) = crate::request_context::current_accumulator() {
        acc.push_structured_event(event);
    }
}

// ---------------------------------------------------------------------------
// Per-request counter helpers (plan §3.6)
// ---------------------------------------------------------------------------
//
// Cost contract: zero ops when `current_request_context().is_none()`;
// one `Relaxed` `fetch_add` when present. The hot-path counters never
// take a lock and never allocate.

/// Bump `materialize_structure_calls` on the current request's
/// context. No-op without a context. Plan §3.6.
pub fn record_materialize_structure_call() {
    if let Some(ctx) = crate::request_context::current_request_context() {
        ctx.materialize_structure_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Bump `materialize_structure_cache_hits` on the current request's
/// context. No-op without a context. Plan §3.6.
pub fn record_materialize_structure_cache_hit() {
    if let Some(ctx) = crate::request_context::current_request_context() {
        ctx.materialize_structure_cache_hits
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Plan §4.14 / B1 — emit a `MaterializeStructurePolicySkip` event
/// into the active audit accumulator. No-op when no request context
/// or accumulator is installed.
///
/// `base` is the input semantic node id; `scope_axis` is the
/// materialiser axis at the point the gate fired; `reason` identifies
/// which policy arm bailed. The audit framework's footprint miner
/// reads these events to attribute kept-symbolic shapes to specific
/// policy decisions.
pub(crate) fn emit_policy_skip(
    base: crate::semantic_query::SemanticNodeId,
    scope_axis: crate::component_meta_materialize::MaterializationScope,
    reason: crate::component_meta_audit::MaterializeSkipReason,
) {
    if crate::request_context::current_accumulator().is_some() {
        let base_str: std::sync::Arc<str> = std::sync::Arc::from(format!("Node#{}", base.0));
        push_structured_event(
            crate::component_meta_audit::StructuredComponentMetaEvent::MaterializeStructurePolicySkip {
                base: base_str,
                scope_axis: scope_axis.into(),
                reason,
            },
        );
    }
}

/// Bump `node_arena_lock_acquisitions` on the current request's
/// context. No-op without a context. Plan §3.6.
pub fn record_node_arena_lock_acquisition() {
    if let Some(ctx) = crate::request_context::current_request_context() {
        ctx.node_arena_lock_acquisitions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Bump `family_map_lock_acquisitions` on the current request's
/// context. No-op without a context. Plan §3.6.
pub fn record_family_map_lock_acquisition() {
    if let Some(ctx) = crate::request_context::current_request_context() {
        ctx.family_map_lock_acquisitions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Bump `dep_signature_merges` on the current request's context.
/// No-op without a context. Plan §3.6.
pub fn record_dep_signature_merge() {
    if let Some(ctx) = crate::request_context::current_request_context() {
        ctx.dep_signature_merges
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Bump `dep_signature_intern_hits` on the current request's context.
/// No-op without a context. Plan §3.6.
pub fn record_dep_signature_intern_hit() {
    if let Some(ctx) = crate::request_context::current_request_context() {
        ctx.dep_signature_intern_hits
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Construct and push a `StructuredComponentMetaEvent::Custom` into
/// the active request's accumulator. Single in-tree construction
/// site for the `Custom` variant — the
/// `every_custom_variant_construction_site_has_justification_comment`
/// grep test (plan §3.A Commit 6.E) checks each `Custom {` literal
/// has a preceding `// Custom justified:` comment; the rationale
/// below covers every call routed through this helper.
pub(crate) fn push_structured_custom(name: &'static str, detail: impl Into<String>) {
    let name = std::sync::Arc::<str>::from(name);
    let detail = std::sync::Arc::<str>::from(detail.into());
    // Custom justified: debug/trace sites across host_manage,
    // host_resolve, meta_resolve, component_meta_host, and
    // component_meta_audit do not map to typed variants of
    // `StructuredComponentMetaEvent` (RequestStart / VfsRead /
    // MaterializeMemberRoute{Start,End} / etc.). The `Custom`
    // variant exists precisely for ad-hoc structured logging; every
    // call site funnels through this single helper so the
    // justification is centralised and the grep gate in §3.A
    // Commit 6.E has one place to inspect.
    // Custom justified: single construction site for `Custom`
    // across the session crate — see the rationale in the
    // `push_structured_custom` doc comment above.
    push_structured_event(
        crate::component_meta_audit::StructuredComponentMetaEvent::Custom { name, detail },
    );
}

/// Push a typed `StructuredComponentMetaEvent` variant into the
/// current accumulator. Plan §2.3 — preferred for any call site
/// that maps to a named variant (`IndexedReadyBuilt`, `VfsRead`,
/// `MaterializeMemberRouteStart`, …).
#[macro_export]
macro_rules! component_meta_trace_structured {
    ($event:expr $(,)?) => {{
        $crate::host_manage::push_structured_event($event);
    }};
}

/// Convenience macro for debug/trace call-sites that don't fit a
/// typed `StructuredComponentMetaEvent` variant — the successor to
/// the deleted `component_meta_trace_scope!` /
/// `component_meta_trace_event!` macros. Expands to a single call
/// into [`push_structured_custom`]. Plan §3.A Commit 6.E.
macro_rules! component_meta_trace_custom {
    ($name:expr, $detail:expr $(,)?) => {{
        // The accumulator check gates the $detail expression so its
        // allocations (typically a format!) are skipped when no audit
        // run is in flight. Hot-path call sites depend on this.
        if $crate::request_context::current_accumulator().is_some() {
            $crate::host_manage::push_structured_custom($name, $detail);
        }
    }};
}

pub(crate) use component_meta_trace_custom;

const COMPONENT_META_MAX_SYMBOLIC_STEPS: usize = 2_000;
pub(in crate::host_manage) const STORE_VIEW_STABILITY_MAX_ATTEMPTS: usize = 3;

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
    type View = crate::resolver_store::HostStoreView;
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

        let live_view = self.resolver_store_view();
        if let Some(node) = self
            .resolver_runtime()
            .fallthrough
            .get_cached_node(&cache_key, &live_view)
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
            .get_cached_node(&root_follow_key, &live_view)
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
        _store_view: Option<&Self::View>,
    ) -> Option<Self::Resolution> {
        VerterHost::compute_fallthrough_surface_uncached(
            self,
            canonical_id,
            prop_type_overrides,
            visiting,
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

pub(in crate::host_manage) struct HostFallthroughResolver<'a> {
    pub(in crate::host_manage) host: &'a VerterHost,
    pub(in crate::host_manage) parent_canonical_id: &'a str,
    pub(in crate::host_manage) parent_snapshot: &'a FileAnalysisSnapshot,
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

        let live_view = self.host.resolver_store_view();
        if let Some(node) = self
            .host
            .resolver_runtime()
            .fallthrough
            .get_cached_node(&cache_key, &live_view)
        {
            if let Some(members) = self.host.runtime_intrinsic_node_to_members(node) {
                return members;
            }
        }

        let members = self
            .host
            .project_intrinsic_members_for_tag(canonical_id, tag)
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
        component_name: &str,
        import_source: &str,
        imported_name: Option<&str>,
        binding_kind: Option<crate::resolver_core::symbol_resolver::ImportBindingKind>,
    ) -> Option<String> {
        debug_assert_eq!(self.parent_canonical_id, parent_canonical);
        let dep_canonical = self.host.resolve_loaded_dependency_canonical(
            parent_canonical,
            import_source,
            verter_workspace::ResolveRequestKind::EsmImport,
        )?;

        let derived_import_binding = self
            .parent_snapshot
            .imports
            .iter()
            .find(|import| import.source == import_source)
            .and_then(|import| {
                import
                    .bindings
                    .iter()
                    .find(|binding| !binding.is_type_only && binding.name == component_name)
            })
            .map(|binding| {
                (
                    match binding.kind {
                        verter_semantic::analysis::types::ImportBindingKind::Named => {
                            crate::resolver_core::symbol_resolver::ImportBindingKind::Named
                        }
                        verter_semantic::analysis::types::ImportBindingKind::Default => {
                            crate::resolver_core::symbol_resolver::ImportBindingKind::Default
                        }
                        verter_semantic::analysis::types::ImportBindingKind::Namespace => {
                            crate::resolver_core::symbol_resolver::ImportBindingKind::Namespace
                        }
                    },
                    binding.imported_name.clone(),
                )
            });

        let binding_kind = binding_kind
            .or_else(|| derived_import_binding.as_ref().map(|(kind, _)| *kind))
            .unwrap_or(crate::resolver_core::symbol_resolver::ImportBindingKind::Named);
        let imported_name = imported_name
            .map(str::to_string)
            .or_else(|| derived_import_binding.and_then(|(_, imported_name)| imported_name));
        let requested_export_name = match binding_kind {
            crate::resolver_core::symbol_resolver::ImportBindingKind::Default => {
                imported_name.as_deref().unwrap_or("default")
            }
            crate::resolver_core::symbol_resolver::ImportBindingKind::Named => {
                imported_name.as_deref().unwrap_or(component_name)
            }
            crate::resolver_core::symbol_resolver::ImportBindingKind::Namespace => component_name,
        };

        self.host
            .resolve_value_export_target(dep_canonical.as_str(), requested_export_name)
            .map(|target| target.canonical_id)
            .or(Some(dep_canonical))
    }

    fn current_dependency_fact_versions(
        &self,
        canonical_id: &str,
    ) -> Vec<crate::resolver_core::FactVersionRef> {
        self.host
            .current_dependency_fact_versions(canonical_id, &std::collections::BTreeSet::new())
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

        let live_view = self.host.resolver_store_view();
        if let Some(node) = self
            .host
            .resolver_runtime()
            .fallthrough
            .get_cached_node(&cache_key, &live_view)
        {
            if let Some(resolution) = self.host.runtime_child_node_to_resolution(node) {
                return Some(resolution);
            }
        }

        let resolution = self
            .host
            .resolve_fallthrough_surface_internal_with_overrides(
                canonical_id,
                prop_type_overrides,
                visiting,
            );

        if let Some(resolution) = resolution.as_ref() {
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

        let live_view = self.host.resolver_store_view();
        if let Some(node) = self
            .host
            .resolver_runtime()
            .fallthrough
            .get_cached_node(&cache_key, &live_view)
        {
            if let Some(resolved) = self.host.runtime_consumed_bindings_to_resolution(node) {
                return resolved;
            }
        }

        let resolved = self.host.resolve_root_consumption(
            canonical_id,
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
        canonical_id: &str,
        snapshot: &Self::Snapshot,
        usage_index: u32,
        eval_env: &mut Option<Self::EvalEnv>,
    ) -> Option<rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>> {
        debug_assert_eq!(self.parent_canonical_id, canonical_id);
        self.host
            .build_generic_child_prop_overrides(canonical_id, snapshot, usage_index, eval_env)
    }

    fn resolve_dynamic_root_candidates(
        &self,
        canonical_id: &str,
        snapshot: &Self::Snapshot,
        usage_index: u32,
        eval_env: &mut Option<Self::EvalEnv>,
    ) -> Vec<DynamicRootCandidate> {
        debug_assert_eq!(self.parent_canonical_id, canonical_id);
        self.host
            .resolve_dynamic_root_candidates(canonical_id, snapshot, usage_index, eval_env)
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

pub(in crate::host_manage) fn log_snapshot_debug(
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ImportedSymbolDependency {
    pub(crate) local_name: String,
    pub(crate) canonical_id: String,
    pub(crate) exported_name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ComputedEvaluatedTypes {
    pub(crate) evaluated_types:
        Option<verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
    pub(crate) discovered_dependencies: std::collections::BTreeSet<String>,
    /// Step 9.1 / D32: surface-id sidecar populated during the
    /// `expand_macro_types_impl_with_expander` closure's per-field run.
    /// Vector-aligned with the same-FieldKind output vectors on
    /// `evaluated_types`. `None` when audit is off (the only consumer
    /// is the scoped origin export in Step 9.2 which is itself
    /// audit-gated).
    pub(crate) surface_identities: Option<crate::meta_resolve::SurfaceNodeIdentities>,
}

/// Host-backed import resolver for `ShallowFileState` construction.
///
/// Resolves import specifiers to canonical file IDs from already-cached
/// import routes. Used during shallow state materialization to
/// pre-canonicalize cross-file edges without live workspace reads.
pub(in crate::host_manage) struct HostShallowImportResolver<'a> {
    pub(in crate::host_manage) dep_edges: &'a rustc_hash::FxHashMap<String, String>,
}

impl crate::resolver_core::ShallowImportResolver for HostShallowImportResolver<'_> {
    fn resolve_canonical(&self, specifier: &str) -> Option<String> {
        self.dep_edges.get(specifier).cloned()
    }
}

pub(in crate::host_manage) struct HostRuntimeValueResolver<'a> {
    pub(in crate::host_manage) host: &'a VerterHost,
}

pub(in crate::host_manage) struct HostExportGraphResolver<'a> {
    pub(in crate::host_manage) host: &'a VerterHost,
}

impl ExportGraphResolver for HostExportGraphResolver<'_> {
    fn export_surface(&self, canonical_id: &str) -> Option<ExportSurface> {
        let (file_kind, _, export_signatures) =
            self.host.load_export_graph_analysis(canonical_id)?;
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
        let (file_kind, script_analysis, export_signatures) =
            self.host.load_export_graph_analysis(canonical_id)?;
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
        _sig: &verter_semantic::analysis::ExportSignature,
    ) -> Option<String> {
        if let Some(shallow) = self.host.shallow_file_state(canonical_id) {
            for target in shallow.exports.values() {
                if let crate::resolver_core::ExportTarget::Reexport {
                    source_specifier,
                    canonical_id: reexport_canonical,
                    ..
                } = target
                {
                    if source_specifier == source && !reexport_canonical.is_empty() {
                        return Some(reexport_canonical.clone());
                    }
                }
            }
        }

        self.host
            .resolve_loaded_dependency_canonical(
                canonical_id,
                source,
                verter_workspace::ResolveRequestKind::EsmImport,
            )
            .or_else(|| {
                self.host
                    .resolve_type_dependency_canonical(canonical_id, source)
            })
    }
}

impl ImportedRuntimeValueResolver for HostRuntimeValueResolver<'_> {
    fn dependency_eval_env(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_eval::EvalEnv>> {
        self.host.base_eval_env_arc(canonical_id)
    }

    fn prepared_value_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
        self.host.prepared_value_decl(canonical_id, symbol_name)
    }

    fn resolve_value_export_target(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> Option<(String, String)> {
        self.host
            .resolve_value_export_target(dep_canonical_id, imported_name)
            .map(|target| (target.canonical_id, target.name))
    }
}

pub(in crate::host_manage) fn exact_resolution_uses_type_preferred_target(
    phase: verter_workspace::ResolvePhase,
    kind: verter_workspace::ResolveRequestKind,
) -> bool {
    matches!(
        (phase, kind),
        (
            verter_workspace::ResolvePhase::CodegenBlocker,
            verter_workspace::ResolveRequestKind::TypeImport,
        ) | (verter_workspace::ResolvePhase::ProviderGraph, _)
    )
}

pub(in crate::host_manage) fn is_runtime_script_target(canonical_id: &str) -> bool {
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

fn has_file_like_extension(canonical_id: &str) -> bool {
    canonical_id.ends_with(".vue")
        || canonical_id.ends_with(".ts")
        || canonical_id.ends_with(".tsx")
        || canonical_id.ends_with(".mts")
        || canonical_id.ends_with(".cts")
        || canonical_id.ends_with(".d.ts")
        || canonical_id.ends_with(".d.mts")
        || canonical_id.ends_with(".d.cts")
        || canonical_id.ends_with(".js")
        || canonical_id.ends_with(".jsx")
        || canonical_id.ends_with(".mjs")
        || canonical_id.ends_with(".cjs")
        || canonical_id.ends_with(".json")
}

fn looks_like_windows_absolute_path(canonical_id: &str) -> bool {
    let bytes = canonical_id.as_bytes();
    bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'/' || bytes[2] == b'\\')
}

pub(crate) fn is_raw_import_specifier_id(canonical_id: &str) -> bool {
    if canonical_id.is_empty()
        || canonical_id.starts_with('/')
        || looks_like_windows_absolute_path(canonical_id)
        || has_file_like_extension(canonical_id)
    {
        return false;
    }

    if canonical_id.starts_with("./")
        || canonical_id.starts_with("../")
        || canonical_id.starts_with('#')
        || canonical_id.starts_with('@')
    {
        return true;
    }

    !canonical_id.contains('.')
}

pub(in crate::host_manage) fn is_builtin_type_symbol(name: &str) -> bool {
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
}

// Legacy TypeExpr walkers (collect_required_owner_import_names, collect_slot_eval_import_names_*,
// collect_surface_eval_import_names_*, collect_runtime_value_names_*, etc.) were deleted.
// The solver host now resolves cross-file types on demand through prepared-decl caches.

/// Collect the set of runtime value names referenced by the template.
/// This reads pre-analyzed snapshot data (binding_occurrences, prop.referenced_bindings),
/// NOT TypeExpr trees — it is not a walker.
pub(in crate::host_manage) fn collect_required_template_runtime_value_names(
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

pub(in crate::host_manage) fn collect_required_root_fallthrough_runtime_value_names(
    snapshot: &FileAnalysisSnapshot,
    root_reachability: &verter_semantic::analysis::component_meta::RootReachability,
) -> rustc_hash::FxHashSet<String> {
    use verter_semantic::analysis::component_meta::{RootReachability, RootTargetRef};
    use verter_semantic::analysis::template::BindingUsageKind;

    let mut required = rustc_hash::FxHashSet::default();
    let Some(template) = snapshot.template.as_ref() else {
        return required;
    };

    let RootReachability::Branches { branches } = root_reachability else {
        return required;
    };

    for branch in branches {
        let element_index = match &branch.target {
            RootTargetRef::NativeElement { element_index, .. }
            | RootTargetRef::DynamicComponentUsage { element_index, .. }
            | RootTargetRef::ComponentUsage { element_index, .. }
            | RootTargetRef::UnresolvedTarget { element_index, .. } => *element_index as usize,
        };

        let Some(element) = template.elements.get(element_index) else {
            continue;
        };

        for occurrence in &template.binding_occurrences {
            if occurrence.span.start < element.span.start
                || occurrence.span.end > element.tag_span_end
            {
                continue;
            }
            if matches!(
                occurrence.usage_kind,
                BindingUsageKind::DirectiveValue | BindingUsageKind::EventHandler,
            ) {
                required.insert(occurrence.name.clone());
            }
        }

        let usage_index = match &branch.target {
            RootTargetRef::DynamicComponentUsage { usage_index, .. }
            | RootTargetRef::ComponentUsage { usage_index, .. } => Some(*usage_index as usize),
            RootTargetRef::NativeElement { .. } | RootTargetRef::UnresolvedTarget { .. } => None,
        };

        let Some(usage) = usage_index.and_then(|usage_index| template.components.get(usage_index))
        else {
            continue;
        };

        for prop in &usage.props {
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
    component_meta_trace_custom!(
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
    let canonical_source = host.read_analysis_source(&canonical);
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
        canonical_source: canonical_source.as_deref(),
    };
    let mut meta = verter_semantic::analysis::component_meta::extract_component_meta(input);
    component_meta_trace_custom!(
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

fn merge_evaluated_prop_types_into_meta(
    host: &VerterHost,
    owner_canonical: &str,
    meta: &mut verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    evaluated_types: Option<&verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
) {
    let Some(evaluated_types) = evaluated_types else {
        return;
    };
    fn collect_imported_props_like_raw_refs(
        host: &VerterHost,
        owner_canonical: &str,
        raw_type: Option<&str>,
    ) -> rustc_hash::FxHashSet<(String, usize)> {
        fn collect(
            expr: &verter_semantic::analysis::type_expr::TypeExpr,
            refs: &mut rustc_hash::FxHashSet<(String, usize)>,
        ) {
            use verter_semantic::analysis::type_expr::TypeExpr;

            match expr {
                TypeExpr::Parenthesized(inner) => collect(inner, refs),
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } => {
                    if name.ends_with("Props") {
                        refs.insert((name.to_string(), type_arguments.len()));
                    }
                    for arg in type_arguments.iter() {
                        collect(arg, refs);
                    }
                }
                TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                    for ty in types.iter() {
                        collect(ty, refs);
                    }
                }
                TypeExpr::Array { element, .. } => collect(element, refs),
                TypeExpr::Tuple { elements, .. } => {
                    for element in elements.iter() {
                        collect(&element.ty, refs);
                    }
                }
                TypeExpr::IndexedAccess { object, index } => {
                    collect(object, refs);
                    collect(index, refs);
                }
                _ => {}
            }
        }

        let Some(raw_type) = raw_type else {
            return rustc_hash::FxHashSet::default();
        };
        let parsed = verter_semantic::analysis::type_expr_lower::parse_type_annotation(raw_type);
        let mut refs = rustc_hash::FxHashSet::default();
        collect(&parsed, &mut refs);
        refs.retain(|(name, _)| {
            host.resolve_local_import_symbol_target(owner_canonical, name)
                .is_some()
                || {
                    let declaration =
                        crate::meta_resolve::resolve_type_declaration(host, owner_canonical, name);
                    !declaration.canonical_source.is_empty()
                        && declaration.canonical_source != owner_canonical
                }
        });
        refs
    }

    fn expr_contains_public_ref(
        expr: &verter_semantic::analysis::type_expr::TypeExpr,
        name: &str,
        type_argument_arity: usize,
    ) -> bool {
        use verter_semantic::analysis::type_expr::TypeExpr;

        match expr {
            TypeExpr::Parenthesized(inner) => {
                expr_contains_public_ref(inner, name, type_argument_arity)
            }
            TypeExpr::Ref {
                name: current_name,
                type_arguments,
            } => {
                (current_name.as_ref() == name && type_arguments.len() == type_argument_arity)
                    || type_arguments
                        .iter()
                        .any(|arg| expr_contains_public_ref(arg, name, type_argument_arity))
            }
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
                .iter()
                .any(|ty| expr_contains_public_ref(ty, name, type_argument_arity)),
            TypeExpr::Array { element, .. } => {
                expr_contains_public_ref(element, name, type_argument_arity)
            }
            TypeExpr::Tuple { elements, .. } => elements
                .iter()
                .any(|element| expr_contains_public_ref(&element.ty, name, type_argument_arity)),
            TypeExpr::IndexedAccess { object, index } => {
                expr_contains_public_ref(object, name, type_argument_arity)
                    || expr_contains_public_ref(index, name, type_argument_arity)
            }
            _ => false,
        }
    }

    let evaluated_by_name = evaluated_types
        .props
        .iter()
        .map(|field| (field.name.as_str(), &field.r#type))
        .collect::<rustc_hash::FxHashMap<_, _>>();

    for prop in &mut meta.props {
        let Some(evaluated) = evaluated_by_name.get(prop.name.as_str()) else {
            continue;
        };
        let imported_props_like_refs =
            collect_imported_props_like_raw_refs(host, owner_canonical, prop.raw_type.as_deref());
        if !imported_props_like_refs.is_empty()
            && !imported_props_like_refs
                .iter()
                .all(|(name, type_argument_arity)| {
                    expr_contains_public_ref(evaluated, name, *type_argument_arity)
                })
        {
            // Allow the merge when the evaluated type is a materialized Object
            // and the current type_expr is a bare Ref — the evaluate_types path
            // already decided to materialize this imported type.
            let evaluated_is_materialized_form = matches!(
                (evaluated, &prop.type_expr),
                (
                    verter_semantic::analysis::type_expr::TypeExpr::Object(_),
                    verter_semantic::analysis::type_expr::TypeExpr::Ref {
                        type_arguments, ..
                    }
                ) if type_arguments.is_empty()
            );
            if !evaluated_is_materialized_form {
                continue;
            }
        }
        if crate::meta_resolve::compare_type_expr_improvement(evaluated, &prop.type_expr)
            || matches!(
                (&prop.type_expr, *evaluated),
                (
                    verter_semantic::analysis::type_expr::TypeExpr::Object(_),
                    verter_semantic::analysis::type_expr::TypeExpr::Union(_)
                        | verter_semantic::analysis::type_expr::TypeExpr::Primitive(_),
                )
            )
        {
            prop.type_expr = (*evaluated).clone();
        }
    }
}

fn fill_missing_component_meta_prop_descriptions_from_imported_roots(
    host: &VerterHost,
    owner_canonical: &str,
    meta: &mut verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
) -> bool {
    use verter_semantic::analysis::AnalyzedMacroKind;

    if !meta.props.iter().any(|prop| prop.description.is_none()) {
        return false;
    }

    let mut imported_roots = rustc_hash::FxHashSet::default();
    let mut jsdoc_by_name: rustc_hash::FxHashMap<
        String,
        (
            Option<String>,
            Vec<verter_semantic::analysis::types::JsdocTag>,
        ),
    > = rustc_hash::FxHashMap::default();

    for dep in resolved.snapshot.macro_type_deps.iter() {
        if !matches!(
            dep.macro_kind,
            AnalyzedMacroKind::DefineProps
                | AnalyzedMacroKind::WithDefaults
                | AnalyzedMacroKind::DefineModel,
        ) {
            continue;
        }
        let Some((dep_canonical, exported_name)) =
            host.resolve_local_import_symbol_target(owner_canonical, dep.type_name.as_str())
        else {
            continue;
        };
        if !imported_roots.insert((dep_canonical.clone(), exported_name.clone())) {
            continue;
        }
        collect_jsdoc_descriptions_from_root(
            host,
            &dep_canonical,
            &exported_name,
            &mut imported_roots,
            &mut jsdoc_by_name,
        );
    }

    for mac in resolved.snapshot.macros.iter() {
        if !matches!(
            mac.kind,
            AnalyzedMacroKind::DefineProps
                | AnalyzedMacroKind::WithDefaults
                | AnalyzedMacroKind::DefineModel,
        ) {
            continue;
        }
        for (resolved_index, resolved_local) in mac.resolved_local_types.iter().enumerate() {
            if !(resolved_index == 0
                || mac
                    .type_references
                    .iter()
                    .any(|type_name| type_name == &resolved_local.name))
            {
                continue;
            }
            let local_expr = resolved_local.type_expr.clone().unwrap_or_else(|| {
                verter_semantic::analysis::type_expr_lower::parse_type_annotation(
                    &resolved_local.expanded,
                )
            });
            let expanded_expr = verter_semantic::analysis::type_expr_lower::parse_type_annotation(
                &resolved_local.expanded,
            );
            for dependency in host
                .imported_symbol_dependencies_for_expr(owner_canonical, &expanded_expr)
                .into_iter()
                .chain(host.imported_symbol_dependencies_for_expr(owner_canonical, &local_expr))
            {
                if !imported_roots.insert((
                    dependency.canonical_id.clone(),
                    dependency.exported_name.clone(),
                )) {
                    continue;
                }
                collect_jsdoc_descriptions_from_root(
                    host,
                    &dependency.canonical_id,
                    &dependency.exported_name,
                    &mut imported_roots,
                    &mut jsdoc_by_name,
                );
            }
        }
    }

    let mut changed = false;
    for prop in &mut meta.props {
        if prop.description.is_some() {
            continue;
        }
        if let Some((description, tags)) = jsdoc_by_name.get(prop.name.as_str()) {
            if let Some(desc) = description {
                prop.description = Some(desc.clone());
            }
            if prop.tags.is_empty() && !tags.is_empty() {
                prop.tags = tags.clone();
            }
            changed = true;
        }
    }

    changed
}

/// Try to project JSDoc descriptions from a resolved dependency file.
/// When the type is not locally defined (barrel re-export), follows the
/// re-export chain via `resolve_imported_type_root` to reach the
/// actual defining file where JSDoc comments live.
fn collect_jsdoc_descriptions_from_root(
    host: &VerterHost,
    dep_canonical: &str,
    exported_name: &str,
    imported_roots: &mut rustc_hash::FxHashSet<(String, String)>,
    jsdoc_by_name: &mut rustc_hash::FxHashMap<
        String,
        (
            Option<String>,
            Vec<verter_semantic::analysis::types::JsdocTag>,
        ),
    >,
) {
    // Try projection in the initial resolved file first
    if try_project_jsdoc_descriptions(host, dep_canonical, exported_name, jsdoc_by_name) {
        // Projection succeeded, but inherited props from external types may still
        // lack JSDoc. Follow heritage imports (e.g., LinkProps extends NuxtLinkProps
        // extends Omit<RouterLinkProps, 'to'> where RouterLinkProps is from vue-router).
        follow_heritage_type_imports(
            host,
            dep_canonical,
            exported_name,
            imported_roots,
            jsdoc_by_name,
        );
        return;
    }

    // Type was not locally defined in dep_canonical (barrel re-export).
    // BFS through the import chain to find the defining file. Handles:
    //   `import { T } from './other'; export { T };`
    //   `export { T } from './other';`
    //   `export * from './other';` (wildcard — may fan out to multiple candidates)
    let mut queue: Vec<(String, String)> =
        vec![(dep_canonical.to_string(), exported_name.to_string())];
    let mut steps = 0usize;
    while let Some((current_canonical, current_name)) = queue.pop() {
        steps += 1;
        if steps > 16 {
            break;
        }
        // Ambient-view-first ensure_loaded guard: BFS may reach canonicals not
        // yet tracked by the request view (barrel re-export chains into
        // external packages).
        if !host.is_evalable(current_canonical.as_str())
            && !host.ensure_loaded(current_canonical.as_str())
        {
            continue;
        }
        let Some((raw_source, cached_parse, _)) =
            host.current_eval_state(current_canonical.as_str())
        else {
            continue;
        };
        let eval_source =
            VerterHost::build_eval_script_source(&raw_source, cached_parse.as_deref());
        let candidates =
            crate::resolver_core::surface_projector::find_type_import_sources_in_source(
                eval_source.as_ref(),
                current_name.as_str(),
            );
        if candidates.is_empty() {
            continue;
        }
        for (import_specifier, imported_name) in candidates {
            let next_canonical = host
                .resolve_route_type_edge(current_canonical.as_str(), import_specifier.as_str())
                .or_else(|| {
                    resolve_relative_type_specifier(
                        current_canonical.as_str(),
                        import_specifier.as_str(),
                        |path| host.is_evalable(path),
                    )
                });
            let Some(next_canonical) = next_canonical else {
                continue;
            };
            if next_canonical == current_canonical {
                continue;
            }
            if !imported_roots.insert((next_canonical.clone(), imported_name.clone())) {
                continue;
            }
            if try_project_jsdoc_descriptions(host, &next_canonical, &imported_name, jsdoc_by_name)
            {
                // Also follow heritage imports from this file
                follow_heritage_type_imports(
                    host,
                    &next_canonical,
                    &imported_name,
                    imported_roots,
                    jsdoc_by_name,
                );
                return;
            }
            queue.push((next_canonical, imported_name));
        }
    }
}

/// After projection succeeds on a file, follow the type's heritage chain imports
/// to collect JSDoc from external dependencies (e.g., vue-router's RouterLinkProps).
/// Uses `collect_jsdoc_descriptions_from_root` for each heritage import so barrel
/// chains within the external package are also followed.
fn follow_heritage_type_imports(
    host: &VerterHost,
    defining_canonical: &str,
    type_name: &str,
    imported_roots: &mut rustc_hash::FxHashSet<(String, String)>,
    jsdoc_by_name: &mut rustc_hash::FxHashMap<
        String,
        (
            Option<String>,
            Vec<verter_semantic::analysis::types::JsdocTag>,
        ),
    >,
) {
    // Ambient-view-first ensure_loaded guard: BFS may reach canonicals not yet
    // tracked by the request view (heritage imports from external packages).
    if !host.is_evalable(defining_canonical) && !host.ensure_loaded(defining_canonical) {
        return;
    }
    let Some((raw_source, cached_parse, _)) = host.current_eval_state(defining_canonical) else {
        return;
    };
    let eval_source = VerterHost::build_eval_script_source(&raw_source, cached_parse.as_deref());
    let heritage_imports =
        crate::resolver_core::surface_projector::find_heritage_type_imports_in_source(
            eval_source.as_ref(),
            type_name,
        );
    for (import_specifier, imported_name) in heritage_imports {
        let next_canonical = host
            .resolve_route_type_edge(defining_canonical, import_specifier.as_str())
            .or_else(|| {
                resolve_relative_type_specifier(
                    defining_canonical,
                    import_specifier.as_str(),
                    |path| host.is_evalable(path),
                )
            });
        let Some(next_canonical) = next_canonical else {
            continue;
        };
        if !imported_roots.insert((next_canonical.clone(), imported_name.clone())) {
            continue;
        }
        // Use full BFS collection so barrel chains within the external
        // package are also traversed (e.g., vue-router.d.ts → index-*.d.ts).
        collect_jsdoc_descriptions_from_root(
            host,
            &next_canonical,
            &imported_name,
            imported_roots,
            jsdoc_by_name,
        );
    }
}

/// Resolve a relative import specifier against a canonical file path
/// using TS-first extension mapping. For `./index3.js` from
/// `.../dist/index.d.ts`, tries `.../dist/index3.d.ts` first, then
/// `.../dist/index3.ts`, etc.
fn resolve_relative_type_specifier(
    owner_canonical: &str,
    specifier: &str,
    file_exists: impl Fn(&str) -> bool,
) -> Option<String> {
    if !specifier.starts_with("./") && !specifier.starts_with("../") {
        return None; // Only relative specifiers
    }

    // Get parent directory of the owner file
    let parent = owner_canonical.rsplit_once('/').map(|(dir, _)| dir)?;

    // Strip the extension from the specifier
    let base = specifier
        .strip_suffix(".js")
        .or_else(|| specifier.strip_suffix(".mjs"))
        .or_else(|| specifier.strip_suffix(".cjs"))
        .unwrap_or(specifier);

    // Strip leading ./ for path joining
    let relative = base.strip_prefix("./").unwrap_or(base);

    // TS-first extension candidates
    for ext in &[".d.ts", ".d.cts", ".d.mts", ".ts", ".tsx"] {
        let candidate = format!("{parent}/{relative}{ext}");
        if file_exists(&candidate) {
            return Some(candidate);
        }
    }

    None
}

/// Resolve `exported_name` from `dep_canonical` as DefineProps and collect
/// JSDoc descriptions/tags into `jsdoc_by_name`. Uses the host-cached parsed
/// program and host-cached external type analysis — never reparses raw
/// source. Returns true if the type was found and projected successfully.
fn try_project_jsdoc_descriptions(
    host: &VerterHost,
    dep_canonical: &str,
    exported_name: &str,
    jsdoc_by_name: &mut rustc_hash::FxHashMap<
        String,
        (
            Option<String>,
            Vec<verter_semantic::analysis::types::JsdocTag>,
        ),
    >,
) -> bool {
    use verter_semantic::analysis::AnalyzedMacroKind;

    let Some(projected) = host.project_imported_macro_surfaces(
        dep_canonical,
        exported_name,
        AnalyzedMacroKind::DefineProps,
    ) else {
        return false;
    };
    for prop in projected.props {
        if prop.description.is_some() || !prop.tags.is_empty() {
            jsdoc_by_name
                .entry(prop.name)
                .or_insert((prop.description, prop.tags));
        }
    }
    true
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
) -> verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
    let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
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
    merge_evaluated_prop_types_into_meta(
        host,
        canonical.as_str(),
        &mut meta,
        resolved.evaluated_types.as_ref(),
    );
    if fill_missing_component_meta_prop_descriptions_from_imported_roots(
        host,
        canonical.as_str(),
        &mut meta,
        resolved,
    ) {
        populate_public_instance_sidecar(&mut meta);
    }
    if include_fallthrough {
        let mut visiting = rustc_hash::FxHashSet::default();
        if let Some(resolution) = host.compute_fallthrough_surface_from_resolved_state(
            &canonical,
            resolved,
            None,
            &mut visiting,
        ) {
            meta.accepted_props = resolution.accepted_props;
            meta.accepted_events = resolution.accepted_events;
            meta.accepted_surface_completeness = resolution.accepted_surface_completeness;
            meta.fallthrough_surface = resolution.fallthrough_surface;
        }
    }
    // Phase 4B: apply the publication policy that commit `624b14d2` deleted.
    // The pass is PURE over (resolved_type_registry, resolved_type_registry_meta);
    // see docs/arch/debt-closure/06-step4b-consumer-surface.md.
    crate::component_meta_resolution_policy::apply_component_meta_resolution_policy(
        &mut meta,
        &resolved.resolved_type_registry,
        &resolved.resolved_type_registry_meta,
        host,
        canonical.as_str(),
    );
    meta
}

/// Like [`extract_component_meta_from_resolved`] with `include_fallthrough=true`,
/// but also returns the fallthrough resolution's fact versions (if available).
/// Used by the payload cache to store Full payloads with the correct fact set.
pub(crate) fn extract_component_meta_from_resolved_with_facts(
    host: &VerterHost,
    canonical_or_alias: &str,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
) -> (
    verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    Option<Vec<crate::resolver_core::FactVersionRef>>,
) {
    let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
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
    merge_evaluated_prop_types_into_meta(
        host,
        canonical.as_str(),
        &mut meta,
        resolved.evaluated_types.as_ref(),
    );
    let mut visiting = rustc_hash::FxHashSet::default();
    let fallthrough_facts = if let Some(resolution) = host
        .compute_fallthrough_surface_from_resolved_state(&canonical, resolved, None, &mut visiting)
    {
        let facts = resolution.fact_versions.clone();
        meta.accepted_props = resolution.accepted_props;
        meta.accepted_events = resolution.accepted_events;
        meta.accepted_surface_completeness = resolution.accepted_surface_completeness;
        meta.fallthrough_surface = resolution.fallthrough_surface;
        Some(facts)
    } else {
        None
    };
    // Phase 4B: apply the publication policy AFTER fallthrough merge so the
    // pass operates on the final accepted_props/events. PURE over
    // (resolved_type_registry, resolved_type_registry_meta).
    crate::component_meta_resolution_policy::apply_component_meta_resolution_policy(
        &mut meta,
        &resolved.resolved_type_registry,
        &resolved.resolved_type_registry_meta,
        host,
        canonical.as_str(),
    );
    (meta, fallthrough_facts)
}

#[cfg(test)]
#[path = "host_manage_tests.rs"]
mod tests;

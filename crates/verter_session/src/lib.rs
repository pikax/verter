#![allow(clippy::too_many_arguments)]
//! # verter_session â€” In-memory virtual file host for Vue SFC compilation
//!
//! Manages the lifecycle of Vue Single File Components in a stateful,
//! in-memory store. Each `.vue` file (or non-SFC dependency) is parsed,
//! hashed, cached, and compiled on demand. The host is the primary API
//! surface consumed by both the Vite bundler plugin (via `verter_napi`)
//! and the browser playground (via `verter_wasm`).
//!
//! ## Resolution
//!
//! All import resolution goes through `verter_workspace::WorkspaceAccess`. The host
//! does NOT perform any heuristic resolution (no extension guessing, no alias
//! maps, no basename matching). `resolve_via_vfs()` is the sole resolution path.
//!
//! ## Dependencies
//!
//! - **`verter_workspace`** â€” sole authority for file access and import resolution
//! - **`verter_compiler`** â€” SFC tokenizer, parser, and template/script/style codegen
//! - **`verter_semantic::analysis`** â€” static analysis (imports, bindings, macros, style analysis)
//!
//! ## Key types
//!
//! | Type | Purpose |
//! |------|---------|
//! | [`VerterHost`] | Main entry point â€” owns the file store and compile cache |
//! | [`HostConfig`] | Per-host configuration (dev mode, error policy, analysis level) |
//! | [`CompileProfile`] | Per-compilation variant (production, SSR, HMR strategy, etc.) |
//! | [`HostUpdateResult`] | Result of [`VerterHost::upsert`] â€” lists changed/removed virtual nodes |
//! | [`VirtualFileResponse`] | Result of [`VerterHost::get_virtual_file`] â€” compiled code + metadata |
//! | [`ResolvedId`] | Result of [`VerterHost::resolve`] â€” canonical + virtual IDs |
//!
//! ## Caching
//!
//! Each file stores per-profile compile slots keyed by a hash of the
//! [`CompileProfile`]. Slots are invalidated when the file's semantic hash
//! changes, and evicted LRU when the per-file profile cap is exceeded.
//! Smart dependency invalidation (tiered: Tier 1 full, Tier 2 export-level,
//! Tier 3 cross-file type resolution) minimizes unnecessary recompilation.
//!
//! ## Internal modules
//!
//! - [`cache`] â€” virtual node diffing, compile slot invalidation, LRU eviction
//! - [`compile`] â€” external source merging, main module assembly
//! - [`deps`] â€” dependency tracking, tiered smart invalidation
//! - [`hash`] â€” xxh3-based content hashing, profile hashing, semantic hashing
//! - [`id`] â€” canonical ID normalization, virtual ID rendering, import resolution
//! - [`parse`] â€” SFC tokenization â†’ [`ParseSnapshot`](types::ParseSnapshot), non-SFC hashing
//! - [`shared`] â€” feature-gated `RwLock`/`RefCell` abstraction
//! - [`upsert`] â€” change detection, result building, export signature diffing

#[cfg(test)]
mod audit_warm_cache_tests;
pub mod audited_request;
mod cache;
mod compile;
pub mod completion_fence;
pub mod component_meta_audit;
pub mod component_meta_host;
pub mod component_meta_result_db;
pub mod cooperative_admission;
pub mod cross_file;
#[cfg(test)]
mod d_cutover_characterization_tests;
mod deps;
mod hash;
pub mod host_executor;
mod host_manage;
mod host_resolve;
mod host_upsert;
pub(crate) mod i64_as_decimal_string;
mod id;
pub mod intrinsic_registry;
mod meta;
pub mod meta_resolve;
pub mod owner_import_surface;
#[cfg(test)]
mod parity_tests;
mod parse;
#[cfg(test)]
mod project_global_cache_tests;
pub mod project_semantic_dispatch;
pub mod project_type_store;
pub mod request_context;
pub mod resolver_core;
mod resolver_store;
pub mod scheduler_shim;
pub mod semantic_query;
pub mod semantic_query_memo;
pub(crate) mod session_runtime;
mod shared;
pub(crate) mod source_map_remap;
#[cfg(test)]
pub(crate) mod spike_instrumentation;
pub mod template_convert;
mod types;
pub(crate) mod u64_as_decimal_string;
mod upsert;

pub use types::*;

// Re-export for the LSP: standalone @verter/types .d.ts content.
pub use verter_compiler::utils::oxc::vue::resolve_type::ResolvedMemberVisibility;
pub use verter_compiler::VERTER_TYPES_STANDALONE_DTS;

// Re-export CompileTarget so downstream crates (LSP, MCP, FFI) can use it
// without adding verter_compiler as a direct dependency.
pub use crate::resolver_core::{type_expansion, type_expansion_host, type_text_parser};
pub use verter_compiler::compile::CompileTarget;

use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use id::canonicalize_id;
pub use id::resolve_external;
use rustc_hash::FxHashMap;
use shared::{default_shared, read_lock, write_lock, Shared};

type CachedEvalProgramAst<'a> = oxc_ast::ast::Program<'a>;
type CachedTypeResolutionContext<'a> =
    verter_compiler::utils::oxc::vue::resolve_type::TypeResolutionContext<'a, 'a>;

struct ParsedEvalProgramOwner {
    allocator: oxc_allocator::Allocator,
    source: Arc<str>,
    source_type: oxc_span::SourceType,
}

self_cell::self_cell!(
    pub(crate) struct ParsedEvalProgram {
        owner: ParsedEvalProgramOwner,

        #[covariant]
        dependent: CachedEvalProgramAst,
    }
);

self_cell::self_cell!(
    pub(crate) struct ParsedTypeResolutionContext {
        owner: Rc<ParsedEvalProgram>,

        #[covariant]
        dependent: CachedTypeResolutionContext,
    }
);

impl ParsedEvalProgram {
    pub(crate) fn parse(source: Arc<str>, source_type: oxc_span::SourceType) -> Option<Self> {
        let mut panicked = false;
        let parsed = Self::new(
            ParsedEvalProgramOwner {
                allocator: oxc_allocator::Allocator::new(),
                source,
                source_type,
            },
            |owner| {
                let result = oxc_parser::Parser::new(
                    &owner.allocator,
                    owner.source.as_ref(),
                    owner.source_type,
                )
                .with_options(oxc_parser::ParseOptions {
                    parse_regular_expression: false,
                    ..oxc_parser::ParseOptions::default()
                })
                .parse();
                panicked = result.panicked;
                result.program
            },
        );
        (!panicked).then_some(parsed)
    }

    pub(crate) fn empty(source_type: oxc_span::SourceType) -> Self {
        Self::parse(Arc::<str>::from(""), source_type)
            .expect("empty eval program should always parse")
    }

    pub(crate) fn source_bytes(&self) -> &[u8] {
        self.borrow_owner().source.as_bytes()
    }
}

fn next_host_instance_id() -> u64 {
    static NEXT_HOST_INSTANCE_ID: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(1);
    NEXT_HOST_INSTANCE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Consolidated resolver state: bundles the unified sub-node resolver runtime
/// with host-level top caches and singleflight groups.
///
/// Replaces the 4 individual cache/singleflight fields that were previously
/// scattered across `VerterHost`.
pub(crate) struct HostResolverState {
    /// Unified sub-node resolver runtime (symbol + fallthrough subsystems).
    pub runtime: crate::resolver_core::resolver_runtime::UnifiedResolverRuntime<
        crate::meta_resolve::ResolvedComponentMetaState,
        crate::types::FallthroughResolution,
    >,
}

impl HostResolverState {
    fn new() -> Self {
        Self {
            runtime: crate::resolver_core::resolver_runtime::UnifiedResolverRuntime::new(),
        }
    }

    fn reset_all(&self) {
        self.runtime.clear_caches();
    }
}

/// Central file store and compile cache for Vue SFC compilation.
///
/// `VerterHost` owns all tracked files, their parse snapshots, and per-profile
/// compile slots. It is designed to be long-lived (one per Vite dev server or
/// WASM session) and provides the full upsert-resolve-load lifecycle:
///
/// 1. [`upsert`](Self::upsert) â€” parse and store a file, returning change info
/// 2. [`resolve`](Self::resolve) â€” map a raw import ID to canonical + virtual IDs
/// 3. [`get_virtual_file`](Self::get_virtual_file) â€” compile on demand (or cache hit) and return code
///
/// Internal state is protected by `RwLock` for thread-safe concurrent access.
pub struct VerterHost {
    pub(crate) instance_id: u64,
    pub(crate) config: HostConfig,
    /// VFS workspace providing file reads, import resolution, and edge recording.
    /// Wrapped in Arc<RwLock> so the scheduler's SourceLoader can share the same
    /// lock and always read through the latest workspace after `set_workspace()`.
    pub(crate) workspace: Arc<parking_lot::RwLock<Arc<dyn verter_workspace::WorkspaceAccess>>>,
    pub(crate) alias_to_canonical: Shared<FxHashMap<String, String>>,
    pub(crate) reverse_dependencies: Shared<FxHashMap<String, BTreeSet<String>>>,
    pub(crate) tick: std::sync::atomic::AtomicU64,
    /// Coarse semantic mutation epoch used for snapshot-coherent resolver views.
    ///
    /// Unlike `tick`, which tracks compile/access recency, this counter only
    /// advances after host mutations that can change semantic resolution inputs.
    pub(crate) store_view_epoch: std::sync::atomic::AtomicU64,
    /// Last computed cross-file prop constness overrides.
    /// Used to detect changes on re-computation (Phase 7 invalidation).
    pub(crate) last_const_prop_overrides:
        Shared<rustc_hash::FxHashMap<String, rustc_hash::FxHashSet<String>>>,
    #[cfg(feature = "session_metrics")]
    pub(crate) metrics: HostMetrics,
    /// Scheduler for async per-file staging.
    ///
    /// The scheduler coordinates Sourceâ†’Analysisâ†’Artifact progression
    /// with generation tracking, priority queuing, and blocker management.
    /// It is the sole parser â€” upsert() delegates to the scheduler.
    pub(crate) scheduler: Arc<verter_scheduler::scheduler::Scheduler>,
    /// Per-file compile cache: overrides, compile slots, diagnostics, deps.
    /// Scheduler owns raw source + analysis; this cache owns per-profile state.
    pub(crate) compile_cache: dashmap::DashMap<String, CompileCacheEntry>,
    /// Provenance counters for component-meta observability.
    /// Shared with sessions via `Arc`.
    pub(crate) provenance: Arc<MetaProvenance>,
    /// Host-level shared resolved external type cache.
    /// Keyed by (dep_canonical_id, dep_source_hash, type_name, resolve_kind).
    /// Bounded to RESOLVED_TYPE_CACHE_CAP entries; cleared on close/clear_compile_cache.
    pub(crate) resolved_type_cache:
        parking_lot::Mutex<rustc_hash::FxHashMap<ResolvedTypeCacheKey, ResolvedTypeCacheEntry>>,
    /// Consolidated resolver state: sub-node caches (symbol + fallthrough),
    /// top-level host caches (meta + fallthrough), and singleflight groups.
    pub(crate) resolver: HostResolverState,
    /// Cached pristine eval environments keyed by canonical id + whole_hash.
    /// Stored pre-evaluation and cloned per query to avoid reparsing the same
    /// script/declaration sources across component-meta requests.
    pub(crate) eval_env_cache: parking_lot::Mutex<
        rustc_hash::FxHashMap<String, (Hash16, Arc<verter_semantic::analysis::type_eval::EvalEnv>)>,
    >,
    /// Semantic query database: revision-gated caches for component surfaces,
    /// binding facts, and reactivity provenance.
    pub(crate) semantic_db: parking_lot::Mutex<verter_semantic::db::SemanticDb>,
    /// Current query profile for execution policy decisions.
    pub(crate) query_profile: parking_lot::Mutex<verter_semantic::profile::QueryProfile>,
    /// Host-owned external type analysis cache (migrated from thread-local).
    /// Keyed by (canonical_id, source_type); content-hash validated per lookup.
    pub(crate) external_type_analysis_cache: parking_lot::Mutex<
        rustc_hash::FxHashMap<
            crate::host_manage::ExternalTypeAnalysisCacheKey,
            crate::host_manage::ExternalTypeAnalysisCacheEntry,
        >,
    >,
    /// Host-owned route-owned shallow state cache (migrated from thread-local).
    /// Keyed by canonical_id; workspace-generation and content-hash validated.
    pub(crate) route_owned_shallow_cache: parking_lot::Mutex<
        rustc_hash::FxHashMap<
            crate::host_resolve::RouteOwnedShallowStateCacheKey,
            crate::host_resolve::RouteOwnedShallowStateCacheEntry,
        >,
    >,
    /// Project-global type-resolution cache root (Phase 1+ of the cache
    /// overhaul). Owns `IndexedReady`, `AnalysisReady`, and the rehomed
    /// `RouteDb` / `ImportedRootDb`. See `project_type_store` module docs.
    pub(crate) project_type_store: Arc<crate::project_type_store::ProjectTypeStore>,
    /// Monotonic request-id generator for component-meta requests.
    /// Zero is reserved for "not populated" (plan §1.4); the counter
    /// starts at 0 and `next_request_id()` returns pre-increment + 1.
    pub(crate) request_id_counter: std::sync::atomic::AtomicU64,
    /// Bounded insert-ordered store of finished audit records. Per-request
    /// inserts happen in `emit_audit_trace`; consumers retrieve via
    /// `take_audit_record(request_id)`. Plan §2.5.
    pub(crate) audit_records: Arc<crate::component_meta_audit::AuditRecordsStore>,
}

// Manual Debug impl because Arc<dyn WorkspaceAccess> doesn't implement Debug.
impl std::fmt::Debug for VerterHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerterHost")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl VerterHost {
    /// Read-only access to the host's configuration. Consumers (LSP
    /// hover provenance, MCP diagnostics, etc.) use this to check
    /// flags like `audit_enabled` or `footprint_capture` without
    /// threading them at construction time.
    #[must_use]
    pub fn config(&self) -> &HostConfig {
        &self.config
    }

    /// Create a new host backed by the given workspace.
    ///
    /// The workspace provides file reads, import resolution, and edge recording
    /// through the [`WorkspaceAccess`](verter_workspace::WorkspaceAccess) trait.
    pub fn new(config: HostConfig, workspace: Arc<dyn verter_workspace::WorkspaceAccess>) -> Self {
        Self::new_with_scheduler_config(
            config,
            workspace,
            verter_scheduler::scheduler::SchedulerConfig::default(),
        )
    }

    /// Create a new host with an explicit [`SchedulerConfig`].
    ///
    /// Path C C12 (per plan §14.5): test harnesses construct hosts with
    /// `SchedulerConfig { cpu_threads: 1, ..SchedulerConfig::default() }`
    /// to avoid CPU oversubscription when many parallel test threads each
    /// spin up their own scheduler thread pools (see plan §13 diagnosis).
    pub fn new_with_scheduler_config(
        config: HostConfig,
        workspace: Arc<dyn verter_workspace::WorkspaceAccess>,
        scheduler_config: verter_scheduler::scheduler::SchedulerConfig,
    ) -> Self {
        let workspace_lock = Arc::new(parking_lot::RwLock::new(workspace));

        let scheduler = {
            let executor = Arc::new(host_executor::HostStageExecutor::new(
                config.clone(),
                Arc::clone(&workspace_lock),
            ));
            let loader = Arc::new(WorkspaceSourceLoader(Arc::clone(&workspace_lock)));
            // Native spawns a driver thread; WASM uses sync mode
            // (wait_or_drive on the host drives stages inline).
            #[cfg(not(target_arch = "wasm32"))]
            {
                verter_scheduler::scheduler::Scheduler::with_executor(
                    scheduler_config,
                    loader,
                    executor,
                )
            }
            #[cfg(target_arch = "wasm32")]
            {
                verter_scheduler::scheduler::Scheduler::new_sync_with_executor(
                    scheduler_config,
                    loader,
                    executor,
                )
            }
        };

        let provenance = Arc::new(MetaProvenance::default());
        let project_type_store = Arc::new(
            crate::project_type_store::ProjectTypeStore::with_provenance(Arc::clone(&provenance)),
        );
        Self {
            instance_id: next_host_instance_id(),
            config,
            workspace: workspace_lock,
            alias_to_canonical: default_shared(FxHashMap::default()),
            reverse_dependencies: default_shared(FxHashMap::default()),
            tick: std::sync::atomic::AtomicU64::new(1),
            store_view_epoch: std::sync::atomic::AtomicU64::new(1),
            last_const_prop_overrides: default_shared(rustc_hash::FxHashMap::default()),
            #[cfg(feature = "session_metrics")]
            metrics: HostMetrics::default(),
            scheduler,
            compile_cache: dashmap::DashMap::new(),
            provenance,
            resolved_type_cache: parking_lot::Mutex::new(rustc_hash::FxHashMap::default()),
            resolver: HostResolverState::new(),
            eval_env_cache: parking_lot::Mutex::new(rustc_hash::FxHashMap::default()),
            semantic_db: parking_lot::Mutex::new(verter_semantic::db::SemanticDb::new()),
            query_profile: parking_lot::Mutex::new(verter_semantic::profile::QueryProfile::Build),
            external_type_analysis_cache: parking_lot::Mutex::new(rustc_hash::FxHashMap::default()),
            route_owned_shallow_cache: parking_lot::Mutex::new(rustc_hash::FxHashMap::default()),
            project_type_store,
            request_id_counter: std::sync::atomic::AtomicU64::new(0),
            audit_records: Arc::new(crate::component_meta_audit::AuditRecordsStore::default()),
        }
    }

    /// Create a standalone host with an internal memory workspace.
    ///
    /// For backward compatibility with tests and simple use cases that don't
    /// need an external workspace. Creates a [`MemoryWorkspace`](verter_workspace::MemoryWorkspace)
    /// internally.
    pub fn new_standalone(config: HostConfig) -> Self {
        let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        Self::new(config, workspace)
    }

    /// Create a standalone host with an explicit [`SchedulerConfig`].
    ///
    /// Path C C12 test-harness entry point — see
    /// [`Self::new_with_scheduler_config`] for the rationale.
    pub fn new_standalone_with_scheduler_config(
        config: HostConfig,
        scheduler_config: verter_scheduler::scheduler::SchedulerConfig,
    ) -> Self {
        let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        Self::new_with_scheduler_config(config, workspace, scheduler_config)
    }

    /// Get a clone of the workspace Arc.
    pub fn workspace(&self) -> Arc<dyn verter_workspace::WorkspaceAccess> {
        self.workspace.read().clone()
    }

    /// Access the project-global type-resolution cache root. Owned exclusively
    /// by the host; shared through an `Arc` so downstream cache consumers can
    /// hold stable references without taking the host lock.
    pub fn project_type_store(&self) -> &Arc<crate::project_type_store::ProjectTypeStore> {
        &self.project_type_store
    }

    /// Current semantic revision marker based on session state.
    /// Set the query profile for this session.
    ///
    /// Query profiles control prewarming, budgets, and allowed query families.
    /// They do not change the semantic meaning of results â€” only execution policy.
    pub fn set_query_profile(&self, profile: verter_semantic::profile::QueryProfile) {
        *self.query_profile.lock() = profile;
    }

    /// Get the current query profile.
    pub fn query_profile(&self) -> verter_semantic::profile::QueryProfile {
        *self.query_profile.lock()
    }

    fn semantic_revision(&self) -> verter_semantic::revision::RevisionMarker {
        verter_semantic::revision::RevisionMarker {
            workspace_revision: self
                .store_view_epoch
                .load(std::sync::atomic::Ordering::Relaxed),
            parser_revision: self.tick.load(std::sync::atomic::Ordering::Relaxed),
            compiler_revision: 0,
            provider_revision: 0,
        }
    }

    /// Query the component surface for a file via the semantic DB.
    ///
    /// Extracts the declared surface from the file's script analysis,
    /// caches it in the semantic DB, and returns a `QueryResult`.
    /// Cross-file fallthrough is not resolved at this layer â€” the returned
    /// accepted surface equals the declared surface.
    pub fn semantic_component_surface(
        &self,
        canonical_id: &str,
    ) -> verter_semantic::query::QueryResult<
        Option<verter_semantic::facts::component::ComponentSurface>,
    > {
        use verter_semantic::query::QueryResult;
        use verter_semantic::refs::FileRef;

        let revision = self.semantic_revision();
        let file_ref = FileRef::new(canonical_id);

        // Check cache first
        {
            let db = self.semantic_db.lock();
            let cached = db.component_surface(&file_ref, revision);
            if cached.is_complete() {
                return cached;
            }
        }

        // Extract from analysis snapshot
        let analysis = self.scheduler_script_analysis(canonical_id);
        let surface = analysis.map(|a| verter_semantic::extract::extract_component_surface(&a));

        // Cache and return
        if let Some(ref s) = surface {
            let mut db = self.semantic_db.lock();
            db.set_component_surface(canonical_id.to_string(), revision, s.clone());
        }

        QueryResult::complete(surface, revision)
    }

    /// Query binding declarations and reactivity facts for a file.
    pub fn semantic_bindings(
        &self,
        canonical_id: &str,
    ) -> verter_semantic::query::QueryResult<
        Option<
            Vec<(
                verter_semantic::facts::binding::BindingDeclaration,
                verter_semantic::facts::reactivity::ReactivityFact,
            )>,
        >,
    > {
        use verter_semantic::query::QueryResult;
        use verter_semantic::refs::FileRef;

        let revision = self.semantic_revision();
        let file_ref = FileRef::new(canonical_id);

        // Check cache first
        {
            let db = self.semantic_db.lock();
            let cached = db.bindings(&file_ref, revision);
            if cached.is_complete() {
                return cached;
            }
        }

        // Extract from analysis
        let analysis = self.scheduler_script_analysis(canonical_id);

        let bindings = analysis.map(|a| verter_semantic::extract::extract_bindings(&a));

        if let Some(ref b) = bindings {
            let mut db = self.semantic_db.lock();
            db.set_bindings(canonical_id.to_string(), revision, b.clone());
        }

        QueryResult::complete(bindings, revision)
    }

    /// Get an aggregated semantic snapshot for a file.
    ///
    /// Combines component surface, bindings, reactivity, and import graph
    /// into a single [`FileSemanticSnapshot`]. Populates any missing caches.
    pub fn semantic_snapshot(
        &self,
        canonical_id: &str,
    ) -> verter_semantic::query::QueryResult<verter_semantic::snapshot::FileSemanticSnapshot> {
        use verter_semantic::query::QueryResult;
        use verter_semantic::snapshot::FileSemanticSnapshot;

        let revision = self.semantic_revision();

        // Get or compute each piece
        let surface_result = self.semantic_component_surface(canonical_id);
        let bindings_result = self.semantic_bindings(canonical_id);

        // Import graph
        let import_graph = {
            let file_ref = verter_semantic::refs::FileRef::new(canonical_id);
            let cached = self.semantic_db.lock().import_graph(&file_ref, revision);
            if cached.is_complete() {
                cached.value.unwrap_or_default()
            } else {
                // Extract from analysis
                let analysis = self.scheduler_script_analysis(canonical_id);
                let graph = analysis
                    .map(|a| verter_semantic::extract::extract_import_graph(&a))
                    .unwrap_or_default();
                self.semantic_db.lock().set_import_graph(
                    canonical_id.to_string(),
                    revision,
                    graph.clone(),
                );
                graph
            }
        };

        // Extract boundary edges from template analysis
        let boundary_edges = {
            let template: Option<verter_semantic::analysis::TemplateAnalysisSnapshot> = None;
            template
                .map(|t| {
                    verter_semantic::extract::extract_boundary_edges(
                        canonical_id,
                        &t,
                        &import_graph,
                    )
                })
                .unwrap_or_default()
        };

        let snapshot = FileSemanticSnapshot {
            file_id: canonical_id.to_string(),
            revision,
            component_surface: surface_result.value,
            bindings: bindings_result.value.unwrap_or_default(),
            import_graph,
            boundary_edges,
        };

        QueryResult::complete(snapshot, revision)
    }

    /// Find a binding's reactivity fact by name within a file.
    ///
    /// Uses stable binding name lookup through the semantic snapshot.
    pub fn binding_reactivity(
        &self,
        canonical_id: &str,
        binding_name: &str,
    ) -> verter_semantic::query::QueryResult<
        Option<verter_semantic::facts::reactivity::ReactivityFact>,
    > {
        use verter_semantic::query::QueryResult;

        let revision = self.semantic_revision();
        let bindings_result = self.semantic_bindings(canonical_id);

        let fact = bindings_result.value.and_then(|bindings| {
            bindings
                .into_iter()
                .find(|(decl, _)| decl.name == binding_name)
                .map(|(_, fact)| fact)
        });

        QueryResult::complete(fact, revision)
    }

    /// Get boundary analysis reports for a component via stable ref.
    ///
    /// Uses the semantic DB to resolve the component's surface and analyze
    /// all usages of it across the workspace. Returns boundary issues
    /// (unknown props, missing required, unknown events).
    pub fn boundary_reports(
        &self,
        component_ref: &verter_semantic::refs::ComponentRef,
    ) -> verter_semantic::query::QueryResult<Vec<verter_semantic::analyzers::boundary::BoundaryIssue>>
    {
        use verter_semantic::analyzers::boundary::analyze_boundary;
        use verter_semantic::query::QueryResult;

        let revision = self.semantic_revision();

        // Get the component's declared surface
        let surface_result = self.semantic_component_surface(&component_ref.file_id);
        let surface = match surface_result.value {
            Some(s) => s,
            None => return QueryResult::complete(vec![], revision),
        };

        // Get the semantic snapshot to access boundary edges
        let snapshot = self.semantic_snapshot(&component_ref.file_id);

        // Analyze each boundary edge targeting this component
        let mut all_issues = Vec::new();
        for edge in &snapshot.value.boundary_edges {
            if edge.child_file_id.as_deref() == Some(component_ref.file_id.as_str()) {
                all_issues.extend(analyze_boundary(edge, &surface));
            }
        }

        QueryResult::complete(all_issues, revision)
    }

    /// Access the unified resolver runtime for counter reads and diagnostics.
    /// Invalidate all cached semantic facts for a file.
    ///
    /// Called when the VFS reports a file change, a provider restarts,
    /// or project config changes. This clears the semantic DB cache for
    /// the given file, forcing re-extraction on the next query.
    /// Get the runtime schema for a component via stable ref.
    ///
    /// Returns a target-neutral schema suitable for generating runtime
    /// validators (Zod, io-ts) or documentation.
    pub fn component_runtime_schema(
        &self,
        component_ref: &verter_semantic::refs::ComponentRef,
    ) -> verter_semantic::query::QueryResult<
        Option<verter_semantic::facts::runtime_schema::ComponentRuntimeSchema>,
    > {
        use verter_semantic::facts::runtime_schema::extract_runtime_schema;
        use verter_semantic::query::QueryResult;

        let revision = self.semantic_revision();
        let surface_result = self.semantic_component_surface(&component_ref.file_id);

        let schema = surface_result.value.map(|s| extract_runtime_schema(&s));

        QueryResult::complete(schema, revision)
    }

    pub fn semantic_invalidate(&self, canonical_id: &str) {
        self.semantic_db.lock().invalidate(canonical_id);
    }

    /// Invalidate all semantic caches (e.g., after provider restart).
    ///
    /// Per plan: "provider restart, backend switch, project-config change,
    /// or external-type delta must invalidate dependent semantic queries."
    pub fn semantic_invalidate_all(&self) {
        *self.semantic_db.lock() = verter_semantic::db::SemanticDb::new();
    }

    pub fn resolver_runtime(
        &self,
    ) -> &crate::resolver_core::resolver_runtime::UnifiedResolverRuntime<
        crate::meta_resolve::ResolvedComponentMetaState,
        crate::types::FallthroughResolution,
    > {
        &self.resolver.runtime
    }

    /// Swap the workspace backing this host.
    ///
    /// The scheduler's SourceLoader shares the same `Arc<RwLock>`, so it
    /// automatically reads through the new workspace after this call.
    pub fn set_workspace(&self, workspace: Arc<dyn verter_workspace::WorkspaceAccess>) {
        *self.workspace.write() = workspace;
        self.bump_store_view_epoch();
    }

    /// Access provenance counters for component-meta observability.
    pub fn provenance(&self) -> &Arc<MetaProvenance> {
        &self.provenance
    }

    /// Snapshot provenance counters, including VFS counters from the active workspace.
    pub fn provenance_snapshot(&self) -> MetaProvenanceSnapshot {
        use std::sync::atomic::Ordering::Relaxed;
        let mut snapshot = self.provenance.snapshot();
        let vfs = self.ws().vfs_provenance_snapshot();
        snapshot.import_resolution_cache_hit_count = vfs.import_resolution_cache_hit_count;
        snapshot.import_resolution_cache_miss_count = vfs.import_resolution_cache_miss_count;
        snapshot.dir_index_hit_count = vfs.dir_index_hit_count;
        snapshot.dir_index_refresh_count = vfs.dir_index_refresh_count;
        snapshot.dir_index_dirty_rescan_count = vfs.dir_index_dirty_rescan_count;
        snapshot.native_fs_read_dir_count = vfs.native_fs_read_dir_count;
        snapshot.native_fs_read_file_miss_count = vfs.native_fs_read_file_miss_count;
        // Path C C1 aggregation: the scheduler owns its own counters in the
        // `verter_scheduler` crate; mirror them into the session-facing
        // snapshot so callers have a single observation surface.
        let sched_counters = self.scheduler.counters();
        snapshot.scheduler_submit_count = sched_counters.submit_count.load(Relaxed);
        snapshot.scheduler_inbox_depth_max = sched_counters.inbox_depth_max.load(Relaxed);
        // Path C C12 (per plan §14.5) retired `HEAVY_COMPONENT_META_TEST_*`
        // counters along with the mutex itself; nothing to mirror here.
        snapshot
    }

    /// Clone the workspace Arc for internal use.
    fn ws(&self) -> Arc<dyn verter_workspace::WorkspaceAccess> {
        self.workspace.read().clone()
    }

    pub(crate) fn current_store_view_epoch(&self) -> u64 {
        self.store_view_epoch
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn bump_store_view_epoch(&self) -> u64 {
        self.clear_thread_local_parsed_eval_program_cache();
        self.store_view_epoch
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1
    }

    /// Resolve an import through the workspace (VFS).
    pub fn resolve_import_via_workspace(
        &self,
        parent_canonical_id: &str,
        import_source: &str,
    ) -> Option<String> {
        self.ws()
            .resolve_import(
                parent_canonical_id,
                import_source,
                verter_workspace::ResolutionContext {
                    phase: verter_workspace::ResolvePhase::CodegenBlocker,
                    kind: verter_workspace::ResolveRequestKind::EsmImport,
                },
            )
            .map(|r| r.source_id)
    }

    /// Resolve an import through the VFS with full resolution context.
    /// Sole resolution path on all targets.
    pub(crate) fn resolve_via_vfs(
        &self,
        parent_canonical_id: &str,
        import_source: &str,
        ctx: verter_workspace::ResolutionContext,
    ) -> Option<String> {
        self.ws()
            .resolve_import(parent_canonical_id, import_source, ctx)
            .map(|r| r.source_id)
    }

    /// Compute the preferred alias-based import specifier for a target file.
    pub fn preferred_specifier(&self, importer_id: &str, target_id: &str) -> Option<String> {
        self.ws().preferred_specifier(importer_id, target_id)
    }

    // â”€â”€ effective_* helpers: override-aware state for compile-path consumers â”€â”€

    /// Override-aware file state for a profile.
    ///
    /// When a content override exists for `profile`, returns the override's
    /// synthetic source, meta, script_analysis, and cached_parse. Otherwise
    /// returns raw scheduler data. Returns `None` if file not in scheduler.
    pub(crate) fn effective_file_state(
        &self,
        canonical_id: &str,
        profile: Option<u64>,
    ) -> Option<EffectiveFileState> {
        {
            use crate::host_executor::HostSourceData;

            let snap = self.scheduler.try_get_source(canonical_id)?;
            let hd = snap.downcast_data::<HostSourceData>()?;

            if let Some(profile_hash) = profile {
                if let Some(cc) = self.compile_cache.get(canonical_id) {
                    if let Some(ovr) = cc.content_overrides.get(&profile_hash) {
                        return Some(EffectiveFileState {
                            source: ovr.source.clone(),
                            meta: ovr.parse.meta.clone(),
                            script_analysis: ovr.parse.script_analysis.clone(),
                            cached_parse: ovr.cached_parse.clone(),
                            whole_hash: ovr.parse.whole_hash,
                        });
                    }
                }
            }

            Some(EffectiveFileState {
                source: snap.source.clone(),
                meta: hd.parse.meta.clone(),
                script_analysis: hd.parse.script_analysis.clone(),
                cached_parse: hd.cached_parse.clone(),
                whole_hash: hd.parse.whole_hash,
            })
        }
    }

    /// Materialize native-side lifecycle state from the current scheduler snapshot.
    ///
    /// This is the scheduler-backed replacement for the old `files`-map ingress:
    /// it updates `compile_cache` identity/dependency state without re-submitting
    /// source back into the scheduler.
    pub(crate) fn integrate_scheduler_snapshot(&self, canonical_id: &str) -> bool {
        use crate::host_executor::HostSourceData;

        let snap = match self.scheduler.try_get_source(canonical_id) {
            Some(s) => s,
            None => return false,
        };
        let Some(hd) = snap.downcast_data::<HostSourceData>() else {
            return false;
        };

        let aliases = std::iter::once(canonical_id.to_string()).collect::<BTreeSet<_>>();
        let deps: BTreeSet<String> = hd
            .parse
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
            .collect();

        let (old_aliases, old_deps) = {
            let mut cc_ref = self
                .compile_cache
                .entry(canonical_id.to_string())
                .or_default();
            let cc = cc_ref.value_mut();
            let old_aliases = cc.aliases.clone();
            let old_deps = cc.dependencies.clone();
            cc.aliases = aliases.clone();
            cc.dependencies = deps.clone();
            cc.generation = snap.generation;
            cc.evicted = false;
            (old_aliases, old_deps)
        };

        self.update_alias_map(canonical_id, &old_aliases, &aliases);
        self.update_reverse_deps(canonical_id, &old_deps, &deps);
        true
    }

    /// Override-aware style analyses for a profile.
    ///
    /// Merges per-index overrides from `StyleOverrideWithAnalysis` with raw
    /// style analyses from the scheduler. Returns `None` if file not in scheduler.
    #[allow(dead_code)] // Used by css_var_flow migration (upcoming)
    pub(crate) fn effective_style_analyses(
        &self,
        canonical_id: &str,
        profile: Option<u64>,
    ) -> Option<Vec<verter_semantic::analysis::StyleBlockAnalysis>> {
        use crate::host_executor::HostAnalysisData;

        let analysis_snap = self.scheduler.try_get_analysis(canonical_id)?;
        let ad = analysis_snap.downcast_data::<HostAnalysisData>()?;
        let raw = &ad.style_analyses;

        if let Some(profile_hash) = profile {
            if let Some(cc) = self.compile_cache.get(canonical_id) {
                if let Some(so) = cc.style_overrides.get(&profile_hash) {
                    let merged: Vec<_> = raw
                        .iter()
                        .enumerate()
                        .map(|(idx, raw_sa)| {
                            if let Some(Some(override_sa)) = so.analyses.get(idx) {
                                override_sa.clone()
                            } else {
                                raw_sa.clone()
                            }
                        })
                        .collect();
                    return Some(merged);
                }
            }
        }

        Some(raw.as_ref().clone())
    }

    /// Override-aware meta for a profile.
    ///
    /// Applies `style_langs` overrides from `StyleOverrideWithAnalysis` to the
    /// raw meta. Returns `None` if file not in scheduler.
    pub(crate) fn effective_meta(
        &self,
        canonical_id: &str,
        profile: Option<u64>,
    ) -> Option<FileMeta> {
        use crate::host_executor::HostSourceData;

        let snap = self.scheduler.try_get_source(canonical_id)?;
        let hd = snap.downcast_data::<HostSourceData>()?;
        let mut meta = hd.parse.meta.clone();

        if let Some(profile_hash) = profile {
            if let Some(cc) = self.compile_cache.get(canonical_id) {
                if let Some(so) = cc.style_overrides.get(&profile_hash) {
                    for (idx, lang) in so.lang_overrides.iter().enumerate() {
                        if let Some(ref l) = lang {
                            if idx < meta.style_langs.len() {
                                meta.style_langs[idx] = Some(l.clone());
                            }
                        }
                    }
                }
            }
        }

        Some(meta)
    }

    /// Clear compile caches (compile slots, template analysis, type hashes)
    /// without removing files from the scheduler or alias maps.
    ///
    /// This is a lighter operation than [`close`](Self::close) â€” parsed source
    /// and analysis snapshots are preserved, only per-profile compile results
    /// are flushed. Useful for invalidating stale compile results while keeping
    /// the file set intact.
    pub fn clear_compile_cache(&self) {
        {
            for mut entry in self.compile_cache.iter_mut() {
                entry.compile_slots.clear();
                entry.raw_template_analysis = None;
                entry.cached_tsc_extract = None;
                entry.cached_resolved_meta.clear();
                entry.cached_meta_payloads.clear();
                entry.cached_fallthrough = None;
            }
        }
        self.resolved_type_cache.lock().clear();
        self.eval_env_cache.lock().clear();
        self.bump_store_view_epoch();
    }

    pub(crate) fn intrinsic_members_for_tag(
        &self,
        tag: &str,
    ) -> Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember> {
        verter_semantic::analysis::html_intrinsics::owned_intrinsic_members_for_tag(tag)
    }

    /// Release all cached data (files, aliases, dependency graph).
    ///
    /// After calling `close()` the host is empty but still usable (you could
    /// upsert files again). The primary purpose is to allow the Rust allocator
    /// to free the backing memory so that NAPI-RS-backed hosts don't keep the
    /// Node.js process alive waiting for GC finalisation.
    pub fn close(&self) {
        // Notify the workspace for each tracked file so overlays AND edge store
        // are cleared before scheduler nodes are removed. Use notify_delete (not
        // notify_close) to clear the VFS edge store entries.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let ws = self.ws();
            let ids = self.scheduler.node_ids();
            for id in &ids {
                ws.notify_delete(id);
            }
            for id in &ids {
                self.scheduler.close_file(id);
            }
        }

        write_lock(&self.alias_to_canonical).clear();
        write_lock(&self.reverse_dependencies).clear();
        write_lock(&self.last_const_prop_overrides).clear();

        {
            self.compile_cache.clear();
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.scheduler.reset();
                self.scheduler.restart_driver();
            }
        }
        self.resolved_type_cache.lock().clear();
        self.resolver.reset_all();
        self.eval_env_cache.lock().clear();
        self.provenance.reset();
        // Clear all semantic caches
        *self.semantic_db.lock() = verter_semantic::db::SemanticDb::new();
        self.bump_store_view_epoch();
    }

    /// Configure project-scoped path alias resolution.
    ///
    /// Accepts a list of [`IdeProjectConfig`] describing tsconfig paths,
    /// workspace aliases, and project references. The host uses these to
    /// resolve aliased import specifiers (e.g. `@/components/Foo.vue`,
    /// `#imports`) without relying on external caller-provided resolutions.
    ///
    /// Delegates to the VFS workspace's `configure_resolver()` which updates
    /// the project graph and publishes a new snapshot atomically.
    ///
    /// Pass an empty slice to clear the resolver.
    pub fn configure_projects(
        &self,
        projects: Vec<verter_semantic::analysis::project_resolver::IdeProjectConfig>,
    ) {
        self.ws().configure_resolver(projects);
        {
            for mut entry in self.compile_cache.iter_mut() {
                entry.import_routes.clear();
                entry.dependencies.clear();
            }
        }
        self.resolver.reset_all();
        self.resolved_type_cache.lock().clear();
        self.eval_env_cache.lock().clear();
        self.semantic_invalidate_all();
        self.bump_store_view_epoch();
    }

    #[cfg(feature = "session_metrics")]
    pub fn metrics_snapshot(&self) -> HostMetricsSnapshot {
        use std::collections::BTreeMap;
        use std::sync::atomic::Ordering::Relaxed;
        let upserts = self.metrics.upserts.load(Relaxed);
        let compile_requests = self.metrics.compile_requests.load(Relaxed);
        let compile_cache_hits = self.metrics.compile_cache_hits.load(Relaxed);
        let slice_hash_time_us_total = self.metrics.slice_hash_time_us_total.load(Relaxed);
        let compile_time_us_total = self.metrics.compile_time_us_total.load(Relaxed);

        let compile_time_us_total_by_profile: BTreeMap<u64, u64> = self
            .metrics
            .compile_time_us_total_by_profile
            .lock()
            .expect("metrics lock poisoned")
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect();
        let compile_count_by_profile: BTreeMap<u64, u64> = self
            .metrics
            .compile_count_by_profile
            .lock()
            .expect("metrics lock poisoned")
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect();

        HostMetricsSnapshot {
            upserts,
            compile_requests,
            compile_cache_hits,
            compile_cache_hit_rate: if compile_requests == 0 {
                0.0
            } else {
                compile_cache_hits as f64 / compile_requests as f64
            },
            virtual_loads: self.metrics.virtual_loads.load(Relaxed),
            resolves: self.metrics.resolves.load(Relaxed),
            style_override_calls: self.metrics.style_override_calls.load(Relaxed),
            slice_hash_time_us_total,
            avg_slice_hash_time_us: if upserts == 0 {
                0.0
            } else {
                slice_hash_time_us_total as f64 / upserts as f64
            },
            compile_time_us_total,
            compile_time_us_total_by_profile,
            compile_count_by_profile,
        }
    }

    /// Get the scheduler's source snapshot for a file (scheduler feature only).
    ///
    /// Returns `None` if the file hasn't been upserted or the snapshot is stale.
    /// This is a lock-free ArcSwap read â€” no contention with upsert/compile.
    pub fn scheduler_source(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_scheduler::node::SourceSnapshot>> {
        self.scheduler.try_get_source(canonical_id)
    }

    /// Get the scheduler's analysis snapshot for a file (scheduler feature only).
    pub fn scheduler_analysis(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_scheduler::node::AnalysisSnapshot>> {
        self.scheduler.try_get_analysis(canonical_id)
    }

    /// Get export signatures from the scheduler's analysis snapshot.
    ///
    /// This is the lock-free read path â€” returns data from the scheduler's
    /// ArcSwap snapshots without touching the `files` RwLock.
    pub fn scheduler_export_signatures(
        &self,
        canonical_id: &str,
    ) -> Option<Vec<verter_semantic::analysis::ExportSignature>> {
        let snap = self.scheduler.try_get_analysis(canonical_id)?;
        let data = snap.downcast_data::<host_executor::HostAnalysisData>()?;
        Some(data.export_signatures.clone())
    }

    /// Get script analysis from the scheduler's analysis snapshot.
    pub fn scheduler_script_analysis(
        &self,
        canonical_id: &str,
    ) -> Option<verter_semantic::analysis::ScriptAnalysisSnapshot> {
        let snap = self.scheduler.try_get_analysis(canonical_id)?;
        let data = snap.downcast_data::<host_executor::HostAnalysisData>()?;
        Some(data.script_analysis.clone())
    }

    /// Get compiled virtual files from the scheduler's artifact snapshot.
    ///
    /// Returns the compile output for a specific profile if available.
    #[allow(dead_code)] // Tested via scheduler_tests; LSP will call once migrated
    pub(crate) fn scheduler_artifact_outputs(
        &self,
        canonical_id: &str,
        profile_hash: u64,
    ) -> Option<rustc_hash::FxHashMap<crate::types::VirtualNodeKind, crate::types::CachedVirtualFile>>
    {
        let snap = self
            .scheduler
            .try_get_artifact(canonical_id, profile_hash)?;
        let data = snap.downcast_data::<host_executor::HostArtifactData>()?;
        Some(data.outputs.clone())
    }

    /// Get artifact diagnostics from the scheduler's artifact snapshot.
    #[allow(dead_code)] // Tested via scheduler_tests; LSP will call once migrated
    pub(crate) fn scheduler_artifact_diagnostics(
        &self,
        canonical_id: &str,
        profile_hash: u64,
    ) -> Option<DiagnosticsSnapshot> {
        let snap = self
            .scheduler
            .try_get_artifact(canonical_id, profile_hash)?;
        let data = snap.downcast_data::<host_executor::HostArtifactData>()?;
        Some(data.diagnostics.clone())
    }

    /// Get style analyses from the scheduler's analysis snapshot.
    pub fn scheduler_style_analyses(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<Vec<verter_semantic::analysis::StyleBlockAnalysis>>> {
        let snap = self.scheduler.try_get_analysis(canonical_id)?;
        let data = snap.downcast_data::<host_executor::HostAnalysisData>()?;
        Some(Arc::clone(&data.style_analyses))
    }

    /// Get the scheduler instance (scheduler feature only).
    pub fn scheduler(&self) -> &Arc<verter_scheduler::scheduler::Scheduler> {
        &self.scheduler
    }

    /// Evict a file's cached entry so the next access reloads from disk.
    ///
    /// Used by `did_close` to discard the editor-buffer version. Unlike
    /// `remove()`, this does NOT clean up aliases, reverse deps, or VFS
    /// state â€” the file still exists on disk, it just needs a fresh parse.
    ///
    /// On the scheduler path, sets `evicted = true` and clears profile state
    /// (compile_slots, overrides, diagnostics) but preserves deps/aliases for
    /// old-state diffing during reload. The eviction gate makes the file
    /// invisible to host accessors until `ensure_loaded()` re-integrates.
    pub fn evict(&self, canonical_id: &str) {
        self.ws().notify_close(canonical_id);
        self.semantic_db.lock().invalidate(canonical_id);

        {
            // Capture pre-evict whole_hash from the scheduler so `ensure_loaded`
            // can detect no-op reloads (identical content) and skip the
            // redundant `bump_store_view_epoch`. See §4.6 Sub-task B.
            let pre_evict_hash = self
                .scheduler
                .try_get_source(canonical_id)
                .map(|s| s.whole_hash);
            if let Some(mut cc) = self.compile_cache.get_mut(canonical_id) {
                cc.evicted = true;
                cc.evicted_whole_hash = pre_evict_hash;
                // Clear profile state but preserve deps/aliases for reload diffing
                cc.content_overrides.clear();
                cc.style_overrides.clear();
                cc.compile_slots.clear();
                cc.latest_diagnostics.clear();
                cc.cached_tsc_extract = None;
                cc.raw_template_analysis = None;
                cc.cached_resolved_meta.clear();
                cc.cached_meta_payloads.clear();
                cc.cached_fallthrough = None;
            }
        }
        self.bump_store_view_epoch();
    }

    /// Ensure a file is loaded into the host.
    ///
    /// The scheduler is the sole ingress authority: this method submits a
    /// `source: None` request to the scheduler (which loads content via the
    /// workspace-backed SourceLoader), waits for Analysis to commit, then
    /// materializes native-side lifecycle state from the committed scheduler
    /// snapshots without re-submitting the source.
    pub fn ensure_loaded(&self, canonical_id: &str) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        self.provenance.ensure_loaded_calls.fetch_add(1, Relaxed);
        let normalized_canonical = self.normalized_analysis_canonical(canonical_id);
        let canonical_id = normalized_canonical.as_ref();
        // Fast path: already in host and not evicted. Also verify the
        // scheduler still has the source — `set_import_dependencies` may
        // create an empty compile_cache stub before the file is loaded into
        // the scheduler; in that case we must proceed to submit a load
        // request.
        {
            if let Some(cc) = self.compile_cache.get(canonical_id) {
                if !cc.evicted && self.scheduler.try_get_source(canonical_id).is_some() {
                    return true;
                }
            }
        }

        {
            use verter_scheduler::job::CompletionState;

            let (reload_from_workspace, pre_evict_hash) = self
                .compile_cache
                .get(canonical_id)
                .filter(|cc| cc.evicted)
                .map(|cc| (true, cc.evicted_whole_hash))
                .unwrap_or((false, None));

            if reload_from_workspace {
                // Evicted files must force the scheduler off any stale committed
                // snapshot before we request a disk-backed reload.
                self.scheduler.close_file(canonical_id);
            }

            // Submit to scheduler â€” it loads via WorkspaceSourceLoader.
            // Thread the current-thread's `OpaqueRequestContext` (if
            // any) into the request so worker threads install it
            // before running stages — that way fan-out events from
            // `workspace.read_file` during `SourceStage` carry the
            // outer request_id and the session-side `SessionVfsSink`
            // picks them up. Plan §3.A Commit 6.D.
            let handle = self
                .scheduler
                .submit_request(verter_scheduler::scheduler::Request {
                    file_id: canonical_id.to_string(),
                    target: verter_scheduler::stage::TargetStage::Analysis,
                    priority: verter_scheduler::stage::Priority::Interactive,
                    source: None,
                    file_kind: None,
                    request_context: verter_scheduler::request_context::current_context(),
                });

            // Wait for the scheduler to reach Analysis. `wait_or_drive` drives
            // stages inline on WASM (no driver thread); on native it delegates
            // to `handle.wait()` when the driver thread is installed.
            // Path C C1 instrumentation: split wait (scheduler drive) vs
            // work (integrate_scheduler_snapshot) so C2's diagnosis can
            // tell load-path contention from post-load processing.
            let wait_start = Instant::now();
            match self.scheduler.wait_or_drive(&handle) {
                CompletionState::Ready(_) => {}
                _ => {
                    self.provenance
                        .ensure_loaded_wait_ns
                        .fetch_add(wait_start.elapsed().as_nanos() as u64, Relaxed);
                    return false;
                }
            }
            self.provenance
                .ensure_loaded_wait_ns
                .fetch_add(wait_start.elapsed().as_nanos() as u64, Relaxed);

            let work_start = Instant::now();
            let loaded = self.integrate_scheduler_snapshot(canonical_id);
            self.provenance
                .ensure_loaded_work_ns
                .fetch_add(work_start.elapsed().as_nanos() as u64, Relaxed);
            // First-time loads are purely additive: they populate host state for a
            // file that no previously-captured view tracks, so they cannot invalidate
            // any existing snapshot's facts. Only re-loads (content reload after an
            // evict) may have changed the file's hash relative to what older views
            // pinned, so only those need to bump the global mutation epoch.
            //
            // §4.6 Sub-task B: compare post-reload hash to the pre-evict hash; if
            // identical, the reload is a content no-op and we can skip the bump
            // entirely. This preserves the type-context cache across
            // load→evict→ensure_loaded cycles that don't actually change the file.
            // `pre_evict_hash == None` (e.g. evict triggered without a prior
            // scheduler snapshot) falls back to the conservative bump.
            if loaded && reload_from_workspace {
                let post_reload_hash = self
                    .scheduler
                    .try_get_source(canonical_id)
                    .map(|s| s.whole_hash);
                let hash_unchanged = match (pre_evict_hash, post_reload_hash) {
                    (Some(pre), Some(post)) => pre == post,
                    _ => false,
                };
                if !hash_unchanged {
                    self.bump_store_view_epoch();
                }
            }
            loaded
        }
    }

    /// Resolve an alias to its canonical ID, or normalize the ID if no alias exists.
    pub(crate) fn resolve_alias_or_canonical(&self, id: &str) -> String {
        let normalized = canonicalize_id(id);
        let alias_map = read_lock(&self.alias_to_canonical);
        alias_map
            .get(normalized.as_ref())
            .cloned()
            .unwrap_or_else(|| normalized.into_owned())
    }

    /// Sync the alias-to-canonical map: remove stale aliases, insert current ones.
    pub(crate) fn update_alias_map(
        &self,
        canonical_id: &str,
        old_aliases: &BTreeSet<String>,
        new_aliases: &BTreeSet<String>,
    ) {
        let mut alias_map = write_lock(&self.alias_to_canonical);
        for old_alias in old_aliases {
            if !new_aliases.contains(old_alias) {
                alias_map.remove(old_alias);
            }
        }
        for alias in new_aliases {
            alias_map.insert(alias.clone(), canonical_id.to_string());
        }
    }

    /// Sync the reverse dependency graph: remove stale edges, insert current ones.
    pub(crate) fn update_reverse_deps(
        &self,
        canonical_id: &str,
        old_deps: &BTreeSet<String>,
        new_deps: &BTreeSet<String>,
    ) {
        let mut rev = write_lock(&self.reverse_dependencies);
        for dep in old_deps {
            if !new_deps.contains(dep) {
                if let Some(owners) = rev.get_mut(dep) {
                    owners.remove(canonical_id);
                    if owners.is_empty() {
                        rev.remove(dep);
                    }
                }
            }
        }
        for dep in new_deps {
            rev.entry(dep.clone())
                .or_default()
                .insert(canonical_id.to_string());
        }
    }

    /// Smart invalidation: when a dependency changes, only invalidate dependent
    /// SFCs whose macro-consumed types were actually affected.
    pub(crate) fn smart_invalidate_dependents(
        &self,
        dependency_id: &str,
        old_export_signatures: &[verter_semantic::analysis::ExportSignature],
        new_export_signatures: &[verter_semantic::analysis::ExportSignature],
    ) {
        // Native path: read reverse deps from workspace (authoritative source),
        // then merge with the legacy reverse_dependencies map for backward
        // compatibility (standalone hosts, tests without exact resolutions).
        {
            let ws = self.ws();
            let mut owners: BTreeSet<String> =
                ws.reverse_deps_for(dependency_id).into_iter().collect();
            // Also check extensionless variant
            if let Some(stem) = deps::strip_configured_extension(
                dependency_id,
                &self.config.resolve_extensions,
                None,
            ) {
                for o in ws.reverse_deps_for(stem) {
                    owners.insert(o);
                }
            }

            // Merge with legacy reverse_dependencies (captures deps from
            // update_reverse_deps that the workspace may not have).
            {
                let rev = shared::read_lock(&self.reverse_dependencies);
                if let Some(legacy) = rev.get(dependency_id) {
                    for o in legacy {
                        owners.insert(o.clone());
                    }
                }
                if let Some(stem) = deps::strip_configured_extension(
                    dependency_id,
                    &self.config.resolve_extensions,
                    None,
                ) {
                    if let Some(more) = rev.get(stem) {
                        for o in more {
                            owners.insert(o.clone());
                        }
                    }
                }
            }

            // When a genuinely new dependency arrives (old signatures empty,
            // new non-empty), dependents may have cached "miss" import routes
            // for this dep. Evict their project-store entries unconditionally
            // so fresh accesses re-resolve import routes. For existing deps
            // where only the export surface changed, scope eviction to the
            // owners that were actually invalidated.
            let dep_is_newly_added =
                old_export_signatures.is_empty() && !new_export_signatures.is_empty();

            {
                let ws = self.workspace.read();
                let cleared = deps::smart_invalidate_dependents_via_scheduler(
                    &self.scheduler,
                    &self.compile_cache,
                    owners.clone(),
                    Some(ws.as_ref()),
                    &self.config,
                    dependency_id,
                    old_export_signatures,
                    new_export_signatures,
                );
                let evict_targets = if dep_is_newly_added || cleared.is_empty() {
                    &owners
                } else {
                    &cleared
                };
                if !evict_targets.is_empty() {
                    self.eval_env_cache.lock().clear();
                }
                for owner in evict_targets {
                    self.resolver.runtime.invalidate_canonical(owner);
                    self.project_type_store.evict_canonical(owner);
                }
            }
        }
    }
}

/// SourceLoader that delegates to the host's current workspace.
///
/// Holds a reference to the host's `RwLock<Arc<dyn WorkspaceAccess>>`
/// so it always reads through the latest workspace, even after
/// `set_workspace()` swaps it.
struct WorkspaceSourceLoader(Arc<parking_lot::RwLock<Arc<dyn verter_workspace::WorkspaceAccess>>>);

impl verter_scheduler::source_loader::SourceLoader for WorkspaceSourceLoader {
    fn load(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.0.read().read_file(canonical_id)
    }

    fn exists(&self, canonical_id: &str) -> bool {
        self.0.read().file_exists(canonical_id)
    }

    fn classify(&self, canonical_id: &str) -> verter_scheduler::source_loader::FileKind {
        match self.0.read().classify_file(canonical_id) {
            verter_workspace::FileKind::VueSfc => verter_scheduler::source_loader::FileKind::VueSfc,
            verter_workspace::FileKind::NonSfc => verter_scheduler::source_loader::FileKind::NonSfc,
        }
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        self.0.read().realpath(canonical_id)
    }
}

#[cfg(test)]
impl VerterHost {
    /// Seed `IndexedReadyDb` with pre-built data for tests.
    pub(crate) fn seed_indexed_ready_for_test(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        raw_source: Arc<str>,
        cached_parse: Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
        script_analysis: Option<Arc<verter_semantic::analysis::ScriptAnalysisSnapshot>>,
        export_signatures: Option<Arc<Vec<verter_semantic::analysis::ExportSignature>>>,
        external_type_analysis: Arc<
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource,
        >,
        shallow_state: Arc<crate::resolver_core::ShallowFileState>,
        snapshot: Option<Arc<FileAnalysisSnapshot>>,
        eval_source: Option<Arc<str>>,
        import_routes: rustc_hash::FxHashMap<String, crate::types::DependencyResolution>,
    ) {
        let effective_whole_hash = if whole_hash == Hash16::default() {
            crate::hash::hash_16(raw_source.as_bytes())
        } else {
            whole_hash
        };
        let snapshot = snapshot.unwrap_or_else(|| Arc::new(FileAnalysisSnapshot::default()));
        let eval_source = eval_source.unwrap_or_else(|| Arc::clone(&raw_source));
        let mut shallow_state = (*shallow_state).clone();
        shallow_state.whole_hash = effective_whole_hash;

        let route_target = |specifier: &str| {
            import_routes.get(specifier).and_then(|resolution| {
                resolution
                    .resolved_canonical_id
                    .clone()
                    .or_else(|| resolution.effective_target().map(str::to_string))
            })
        };
        for target in shallow_state.import_targets.values_mut() {
            if target.canonical_id.is_empty() {
                if let Some(resolved) = route_target(&target.source_specifier) {
                    target.canonical_id = resolved;
                }
            }
        }
        for export in shallow_state.exports.values_mut() {
            if let crate::resolver_core::ExportTarget::Reexport {
                source_specifier,
                canonical_id,
                ..
            } = export
            {
                if canonical_id.is_empty() {
                    if let Some(resolved) = route_target(source_specifier) {
                        *canonical_id = resolved;
                    }
                }
            }
        }
        for wildcard in &mut shallow_state.wildcard_reexports {
            if wildcard.canonical_id.is_empty() {
                if let Some(resolved) = route_target(&wildcard.source_specifier) {
                    wildcard.canonical_id = resolved;
                }
            }
        }

        // Insert import_routes into compile_cache if non-empty.
        if !import_routes.is_empty() {
            self.compile_cache
                .entry(canonical_id.to_string())
                .or_insert_with(crate::CompileCacheEntry::default)
                .import_routes = import_routes.clone();
        }

        let shallow_state = Arc::new(shallow_state);

        let import_route_hash = (!import_routes.is_empty())
            .then(|| crate::resolver_store::hash_import_route_targets(&import_routes));
        let import_routes_arc = Arc::new(import_routes.clone());

        // Step 8 / F5: route_hash mirror — see host_manage.rs equivalent
        // for the rationale (cache content-derived hash so
        // current_derived_fact_hash skips per-call rehashing).
        let route_hash = shallow_state
            .has_resolvable_surface()
            .then(|| crate::resolver_store::hash_route_surface(shallow_state.as_ref()));

        // Publish the canonical post-parse artifact into IndexedReadyDb. This
        // is the single authoritative cache consumers read from; the retired
        // `IndexedReadyDb` no longer exists.
        let indexed = crate::project_type_store::IndexedReady {
            whole_hash: effective_whole_hash,
            shallow_state: Arc::clone(&shallow_state),
            import_routes: Arc::clone(&import_routes_arc),
            import_route_hash,
            route_hash,
            raw_source,
            eval_source,
            cached_parse,
            script_analysis,
            export_signatures,
            snapshot,
            external_type_analysis,
        };
        self.project_type_store
            .indexed()
            .insert(Arc::from(canonical_id), Arc::new(indexed));

        let mut dep_edges = FxHashMap::default();
        for target in shallow_state.import_targets.values() {
            if !target.canonical_id.is_empty() {
                dep_edges
                    .entry(target.source_specifier.clone())
                    .or_insert_with(|| target.canonical_id.clone());
            }
        }
        for export in shallow_state.exports.values() {
            if let crate::resolver_core::ExportTarget::Reexport {
                source_specifier,
                canonical_id,
                ..
            } = export
            {
                if !canonical_id.is_empty() {
                    dep_edges
                        .entry(source_specifier.clone())
                        .or_insert_with(|| canonical_id.clone());
                }
            }
        }
        for wildcard in &shallow_state.wildcard_reexports {
            if !wildcard.canonical_id.is_empty() {
                dep_edges
                    .entry(wildcard.source_specifier.clone())
                    .or_insert_with(|| wildcard.canonical_id.clone());
            }
        }

        let bundle = Arc::new(
            crate::resolver_core::prepared_decl::build_prepared_decl_bundle(
                canonical_id,
                Arc::clone(&shallow_state),
                dep_edges,
                FxHashMap::default(),
            ),
        );
        let mut bundle_facts = vec![crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: canonical_id.to_string(),
            hash: effective_whole_hash,
        }];
        if !import_routes.is_empty() {
            bundle_facts.push(crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: canonical_id.to_string(),
                kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                hash: crate::resolver_store::hash_import_route_targets(&import_routes),
            });
        }
        self.resolver.runtime.prepared_decl_bundles.insert_arc(
            canonical_id.to_owned(),
            bundle,
            bundle_facts,
        );
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod lib_tests;

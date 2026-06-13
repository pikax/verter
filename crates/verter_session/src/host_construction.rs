//! `impl VerterHost` — constructors and configuration accessors.
//!
//! Owns:
//! - the four public construction entry points (`new`,
//!   `new_with_scheduler_config`, `new_standalone`,
//!   `new_standalone_with_scheduler_config`)
//! - the read accessors `config`, `workspace`, `workspace_read`,
//!   `project_type_store`
//! - the four `#[cfg(test)]` audit / dispatch hooks (`audit`,
//!   `dispatch_counter`, `dispatch_trace_for`, `semantic_dispatch`)
//!
//! Construction wires the scheduler, workspace, project-type-store, and
//! resolver runtime so they share `Arc` handles for the route /
//! imported-roots databases. The fields of `VerterHost` itself live on
//! the struct definition in `lib.rs`; the construction-time substrate
//! types ([`HostResolverState`], [`WorkspaceSourceLoader`],
//! [`next_host_instance_id`]) live here.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::shared::default_shared;
use crate::types::HostConfig;
#[cfg(feature = "session_metrics")]
use crate::types::HostMetrics;
use crate::{host_executor, VerterHost};

pub(crate) fn next_host_instance_id() -> u64 {
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
    /// Construct a `HostResolverState` whose
    /// inner [`UnifiedResolverRuntime`](crate::resolver_core::resolver_runtime::UnifiedResolverRuntime)
    /// shares its `RouteDb` / `ImportedRootDb` authority with the host's
    /// [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore)
    /// via `Arc` clones supplied by the host at construction time.
    fn new(
        routes: Arc<crate::resolver_core::RouteDb>,
        imported_roots: Arc<crate::resolver_core::ImportedRootDb>,
    ) -> Self {
        Self {
            runtime: crate::resolver_core::resolver_runtime::UnifiedResolverRuntime::new(
                routes,
                imported_roots,
            ),
        }
    }

    pub(crate) fn reset_all(&self) {
        self.runtime.clear_caches();
    }
}

/// SourceLoader that delegates to the host's current workspace.
///
/// Holds a reference to the host's `RwLock<Arc<dyn WorkspaceAccess>>`
/// so it always reads through the latest workspace, even after
/// `set_workspace()` swaps it.
///
/// This impl is the session-implemented trait seam through which
/// HOST-GATED classification reaches the scheduler: `classify` routes
/// through the host's [`crate::framework::HostLanguageClassifier`]
/// (static registry × project capability snapshot), not the pure
/// static fallback the scheduler's built-in loaders use.
pub(crate) struct WorkspaceSourceLoader {
    pub(crate) workspace: Arc<parking_lot::RwLock<Arc<dyn verter_workspace::WorkspaceAccess>>>,
    pub(crate) language_classifier: crate::framework::HostLanguageClassifier,
}

impl verter_scheduler::source_loader::SourceLoader for WorkspaceSourceLoader {
    fn load(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.workspace.read().read_file(canonical_id)
    }

    fn exists(&self, canonical_id: &str) -> bool {
        self.workspace.read().file_exists(canonical_id)
    }

    fn classify(&self, canonical_id: &str) -> verter_language::FileLanguage {
        self.language_classifier.classify(canonical_id)
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        self.workspace.read().realpath(canonical_id)
    }
}

impl VerterHost {
    /// Read-only access to the host's configuration.
    ///
    /// Consumers (LSP hover provenance, MCP diagnostics, etc.) use this
    /// to check flags like `audit_enabled` or `footprint_capture` without
    /// threading them at construction time.
    #[must_use]
    pub fn config(&self) -> &HostConfig {
        &self.config
    }

    /// The host-level language classification authority (static
    /// registry × project capability snapshot). Session-level consumers
    /// resolve a path's [`verter_language::FileLanguage`] through this,
    /// never through ad-hoc extension checks.
    #[must_use]
    pub fn language_classifier(&self) -> &crate::framework::HostLanguageClassifier {
        &self.language_classifier
    }

    /// Create a new host backed by the given workspace.
    ///
    /// The workspace provides file reads, import resolution, and edge
    /// recording through the
    /// [`WorkspaceAccess`](verter_workspace::WorkspaceAccess) trait.
    pub fn new(config: HostConfig, workspace: Arc<dyn verter_workspace::WorkspaceAccess>) -> Self {
        Self::new_with_scheduler_config(
            config,
            workspace,
            verter_scheduler::scheduler::SchedulerConfig::default(),
        )
    }

    /// Create a new host with an explicit [`SchedulerConfig`].
    ///
    /// Test harnesses construct hosts with
    /// `SchedulerConfig { cpu_threads: 1, ..SchedulerConfig::default() }`
    /// to avoid CPU oversubscription when many parallel test threads each
    /// spin up their own scheduler thread pools.
    pub fn new_with_scheduler_config(
        config: HostConfig,
        workspace: Arc<dyn verter_workspace::WorkspaceAccess>,
        scheduler_config: verter_scheduler::scheduler::SchedulerConfig,
    ) -> Self {
        // Register the session-side "clear all install_tls slots"
        // hook with the scheduler's substrate registry. Idempotent
        // across hosts; the scheduler crate uses a `OnceLock` and
        // silently observes that the hook is already registered on
        // subsequent host constructions.
        crate::request_context::install_clear_tls_hook();

        // Receive the Vue adapter's carrier registration proof (the
        // `.vue` `LanguageRegistry` carrier-row token, D-ba) at host
        // construction; the blessed `vue_parse()` accessor reuses it.
        crate::typeinfo::adapters::vue::receive_vue_carrier_token();

        // Thread the host's configured `resolve_extensions` into the
        // workspace at construction so reverse-dep stem stripping
        // honours the host policy from the start.
        workspace.set_default_resolve_extensions(config.resolve_extensions.clone());

        let workspace_lock = Arc::new(parking_lot::RwLock::new(workspace));

        // The host's language classification authority. The capability
        // snapshot is empty: no capability producer exists in the
        // session yet, so host-gated classification equals the static
        // registry resolution until one lands.
        let language_classifier = crate::framework::HostLanguageClassifier::with_built_in_registry(
            crate::framework::ProjectCapabilitySnapshot::empty(),
        );

        // The framework adapter registry, built ONCE here. The Vue carrier leg
        // receives a clone of the SAME minted carrier proof the blessed
        // `vue_parse()` accessor holds (received just above) — one mint channel,
        // value-equal receipt, no second mint.
        let framework_registry =
            std::sync::Arc::new(crate::framework::FrameworkAdapterRegistry::built_in(
                crate::typeinfo::adapters::vue::vue_carrier_token_clone(),
            ));
        let framework_script_caches =
            std::sync::Arc::new(crate::framework::script_facts::FrameworkScriptCaches::new());

        let scheduler = {
            let executor = Arc::new(host_executor::HostStageExecutor::new(
                config.clone(),
                Arc::clone(&workspace_lock),
            ));
            let loader = Arc::new(WorkspaceSourceLoader {
                workspace: Arc::clone(&workspace_lock),
                language_classifier: language_classifier.clone(),
            });
            // Native spawns a driver thread; WASM uses sync mode
            // (wait_or_drive on the host drives stages inline).
            #[cfg(not(target_arch = "wasm32"))]
            {
                // The host constructs and injects the scheduler's worker
                // pools — the scheduler owns no pool construction
                // (mirrors the `HostCpuPool::new` injection pattern
                // below). The two scheduler pools (CpuWorker/IoWorker)
                // coexist with the host coordinator pool (`HostCpuPool`,
                // tagged External) under the dual-pool isolation
                // invariant: host-coordinator work never runs scheduler
                // stage work, and scheduler stage workers never run the
                // outer batch fan-out. The IO transport capacity is sized from the
                // SAME resolved DAG budget the scheduler admits against
                // (`resolved_dag_budget().io`) so the IO channel never
                // becomes a second admission authority.
                let scheduler_cpu_pool =
                    verter_scheduler::SchedulerCpuPool::new(scheduler_config.cpu_threads);
                let scheduler_io_pool = verter_scheduler::SchedulerIoPool::new(
                    scheduler_config.io_threads,
                    scheduler_config.resolved_dag_budget().io as usize,
                );
                verter_scheduler::scheduler::Scheduler::with_executor(
                    scheduler_config,
                    loader,
                    executor,
                    scheduler_cpu_pool,
                    scheduler_io_pool,
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

        let provenance = Arc::new(crate::types::MetaProvenance::default());
        let project_type_store = Arc::new(
            crate::project_type_store::ProjectTypeStore::with_provenance(Arc::clone(&provenance)),
        );
        // Pull RouteDb / ImportedRootDb handles from the project-type-store
        // BEFORE constructing the resolver runtime so the runtime borrows
        // the project-shared `Arc`s. This keeps
        // `host.project_type_store.routes_handle()` and
        // `host.resolver.runtime.routes_handle()` `Arc::ptr_eq`-equal —
        // resolver hot-path mutations land on the same DB the project
        // store exposes.
        let routes_handle = project_type_store.routes_handle();
        let imported_roots_handle = project_type_store.imported_roots_handle();
        // Install the host-level test audit hook on the FileArtifactStore so
        // fresh `insert`s bump `total_shallow_processes` + `loaded_files`
        // cumulatively across requests on this host. Test-only;
        // production builds compile without this block.
        #[cfg(test)]
        let test_audit = Arc::new(crate::host_test_audit::HostTestAuditState::new());
        #[cfg(test)]
        project_type_store
            .indexed()
            .install_test_audit_hook(Arc::clone(&test_audit));
        // Build the audit records store ONCE and share its `Arc` between
        // the legacy `audit_records` field and the new `host_audit_runtime`
        // so writes through either surface land in the same map. The
        // legacy field becomes a thin `Arc::clone` of the runtime's
        // store accessor; this avoids a dual-store regression where
        // each surface accumulated its own records.
        let audit_records_init: Arc<crate::component_meta_audit::AuditRecordsStore> =
            Arc::new(crate::component_meta_audit::AuditRecordsStore::default());
        // Mirror the relevant `HostConfig` flags into the substrate's
        // `AuditConfig` snapshot. The substrate-side flag is what
        // `AuditRequestRegistration::new` and the sampler thread read,
        // so the wire-up MUST happen here at construction time.
        let audit_config = verter_audit::AuditConfig {
            audit_timing_capture: config.audit_timing_capture,
            ..verter_audit::AuditConfig::default()
        };
        let scratch_cache_capacity = config.typeinfo_scratch_cache_capacity;
        // Resolve the host CPU pool worker count BEFORE moving
        // `config` into the struct. The mapping (matching the
        // documented `HostConfig::host_cpu_threads` contract):
        //
        // - `None`                 -> `available_parallelism()` (default)
        // - `Some(0)`              -> `available_parallelism()` (same as
        //                             None; treated as default so a
        //                             misconfigured FFI / NAPI / TS
        //                             caller passing `0` still gets a
        //                             working pool rather than a panic)
        // - `Some(n)` where n > 0  -> `n` workers
        //
        // The `.filter(|&n| n > 0).unwrap_or_else(...)` pattern below
        // implements exactly this mapping in one pass.
        //
        // `available_parallelism()` may itself fail (return `Err`) on
        // some platforms; we final-fallback to `1` worker so
        // `HostCpuPool::new`'s positive-thread assertion never fires
        // from any host-construction path.
        #[cfg(not(target_arch = "wasm32"))]
        let host_cpu_threads = config
            .host_cpu_threads
            .filter(|&n| n > 0)
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(1)
            });
        #[cfg(not(target_arch = "wasm32"))]
        let host_cpu_pool = verter_scheduler::HostCpuPool::new(host_cpu_threads);
        Self {
            instance_id: next_host_instance_id(),
            config,
            language_classifier,
            workspace: workspace_lock,
            alias_to_canonical: default_shared(FxHashMap::default()),
            tick: std::sync::atomic::AtomicU64::new(1),
            store_view_epoch: std::sync::atomic::AtomicU64::new(1),
            load_generation: std::sync::atomic::AtomicU64::new(0),
            store_view_manager: crate::resolver_store::StoreViewManager::new(),
            last_const_prop_overrides: default_shared(rustc_hash::FxHashMap::default()),
            #[cfg(feature = "session_metrics")]
            metrics: HostMetrics::default(),
            scheduler,
            provenance,
            resolver: HostResolverState::new(routes_handle, imported_roots_handle),
            query_profile: parking_lot::Mutex::new(verter_semantic::profile::QueryProfile::Build),
            project_type_store,
            request_id_counter: std::sync::atomic::AtomicU64::new(0),
            audit_records: Arc::clone(&audit_records_init),
            host_audit_runtime: Arc::new(crate::host_audit_runtime::HostAuditRuntime::new(
                audit_config,
                Arc::clone(&audit_records_init),
            )),
            #[cfg(test)]
            test_audit,
            #[cfg(test)]
            last_upsert_priority: parking_lot::Mutex::new(None),
            #[cfg(test)]
            compile_one_call_count: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            compile_one_caller_kind_tag: std::sync::atomic::AtomicU8::new(0),
            // `usize::MAX` is the "unobserved" sentinel. A real worker
            // overwrites this with either its host-pool id or `usize::MAX`
            // again if no token is installed (the regression case).
            #[cfg(test)]
            compile_one_host_cpu_pool_token: std::sync::atomic::AtomicUsize::new(usize::MAX),
            typeinfo_scratch_cache: parking_lot::Mutex::new(match scratch_cache_capacity {
                Some(cap) => crate::typeinfo::scratch_cache::ScratchCache::with_capacity(cap),
                None => crate::typeinfo::scratch_cache::ScratchCache::with_default_capacity(),
            }),
            framework_registry,
            framework_script_caches,
            #[cfg(not(target_arch = "wasm32"))]
            host_cpu_pool,
            compile_force_overflow_observations: std::sync::atomic::AtomicUsize::new(0),
            materialize_force_overflow_observations: std::sync::atomic::AtomicUsize::new(0),
            materialize_force_in_scope_partial: std::sync::atomic::AtomicBool::new(false),
            materialize_force_mid_compute_generation_bump: std::sync::atomic::AtomicBool::new(
                false,
            ),
            relation_force_overflow_observations: std::sync::atomic::AtomicUsize::new(0),
            compile_tier_prefetch_invocations: std::sync::atomic::AtomicUsize::new(0),
            signature_overflow_at_install: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Create a standalone host with an internal memory workspace.
    ///
    /// For backward compatibility with tests and simple use cases that
    /// don't need an external workspace. Creates a
    /// [`MemoryWorkspace`](verter_workspace::MemoryWorkspace) internally.
    pub fn new_standalone(config: HostConfig) -> Self {
        let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        Self::new(config, workspace)
    }

    /// Create a standalone host with an explicit [`SchedulerConfig`].
    ///
    /// See [`Self::new_with_scheduler_config`] for the rationale.
    pub fn new_standalone_with_scheduler_config(
        config: HostConfig,
        scheduler_config: verter_scheduler::scheduler::SchedulerConfig,
    ) -> Self {
        let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        Self::new_with_scheduler_config(config, workspace, scheduler_config)
    }

    /// Get a clone of the workspace `Arc`.
    ///
    /// **Mutator caution:** mutator methods on the returned trait object
    /// (`notify_upsert`, `notify_close`, `notify_delete`,
    /// `set_exact_resolutions`, `configure_resolver`, `set_workspace`,
    /// etc.) bypass the host-side cache cascade — go through the
    /// dedicated wrappers (`host.notify_upsert(...)`,
    /// `host.notify_close(...)`, `host.set_exact_resolutions(...)`,
    /// `host.configure_projects(...)`) instead.
    ///
    /// Demoted to `pub(crate)` so external crates cannot reach mutators
    /// directly. External read consumers go through
    /// [`Self::workspace_read`] which returns the narrower
    /// `Arc<dyn WorkspaceRead>` trait object covering only file-access
    /// methods. Trait upcasting (Rust 1.86+) makes
    /// `Arc<dyn WorkspaceAccess>` → `Arc<dyn WorkspaceRead>` lock-free.
    pub(crate) fn workspace(&self) -> Arc<dyn verter_workspace::WorkspaceAccess> {
        self.workspace.read().clone()
    }

    /// Public read-only workspace access.
    ///
    /// Returns the narrower
    /// [`WorkspaceRead`](verter_workspace::WorkspaceRead) trait object
    /// covering all file-access methods. Mutators (`notify_close`,
    /// `notify_upsert`, `configure_projects`, `set_exact_resolutions`,
    /// `upsert`, etc.) are reachable only via host wrappers that run the
    /// cache-cascade discipline; direct `WorkspaceAccess` mutator calls
    /// are gated behind `pub(crate) workspace()` and not callable from
    /// external crates.
    ///
    /// The `WorkspaceAccess: WorkspaceRead` supertrait bound (Rust 1.86+
    /// trait upcasting) makes this a lock-free `Arc` clone + upcast — no
    /// separate adapter or tracing layer.
    #[must_use]
    pub fn workspace_read(&self) -> Arc<dyn verter_workspace::WorkspaceRead> {
        self.workspace.read().clone() as Arc<dyn verter_workspace::WorkspaceRead>
    }

    /// Test-only host audit view.
    ///
    /// Exposes cumulative loaded-files / total-reads / total-shallow-
    /// processes / total-lowerings counters for read-once /
    /// shallow-first / lazy-expansion tests. Gated by bare
    /// `#[cfg(test)]`; production builds compile without this method
    /// (no Cargo feature involved).
    #[cfg(test)]
    #[must_use]
    pub fn audit(&self) -> crate::host_test_audit::HostTestAudit<'_> {
        crate::host_test_audit::HostTestAudit::new(
            &self.test_audit,
            self.project_type_store.semantic_graph(),
        )
    }

    /// Test-only dispatch counter view.
    ///
    /// Returns a
    /// [`DispatchCounter`](crate::host_test_audit::DispatchCounter) with
    /// `family_cold(&key)` / `family_warm(&key)` accessors for
    /// cache-discipline tests. Counters are thread-local and monotonic;
    /// tests sample baselines and deltas across paired queries.
    #[cfg(test)]
    #[must_use]
    pub fn dispatch_counter(&self) -> crate::host_test_audit::DispatchCounter {
        crate::host_test_audit::DispatchCounter
    }

    /// Test-only per-key dispatch trace.
    ///
    /// Reads the warm cache to produce a
    /// [`DispatchTrace`](crate::host_test_audit::DispatchTrace) whose
    /// `path_decomposition()` enumerates each hop and the projection
    /// mode the cache carries for that hop. Intended for terminal-mode-
    /// only-expansion tests.
    ///
    /// For `ProjectPath` keys the decomposition has one entry per
    /// prefix length; for other variants it has a single terminal entry.
    #[cfg(test)]
    #[must_use]
    pub fn dispatch_trace_for(
        &self,
        key: &crate::semantic_query::SemanticQueryKey,
    ) -> crate::host_test_audit::DispatchTrace {
        crate::host_test_audit::DispatchTrace::from_key(
            self.project_type_store.semantic_graph(),
            key,
        )
    }

    /// Test / arch-guard [`ProjectSemanticDispatch`] accessor for
    /// dispatch tests.
    ///
    /// Production callers route dispatch through the component-meta
    /// resolver and engine; tests construct a hermetic host and
    /// dispatch directly via this accessor to exercise the
    /// cache-discipline / read-once / terminal-mode-only-expansion
    /// invariants without going through the surface materialiser.
    ///
    /// Visible to integration tests (no `#[cfg(test)]` gate) so
    /// `tests/cross_owner_materialise_reuse_production.rs` can drive
    /// `materialize_surface` from N owner scopes and observe the
    /// cross-owner reuse contract on the live
    /// `MaterializeStructureDb`. The accessor's contract is
    /// arch-guard / test-fixture; production resolver code MUST NOT
    /// construct this dispatcher directly — it goes through the
    /// component-meta resolver / engine. The accessor's existence is
    /// a documented test-bridge, not a public-API stability promise.
    #[must_use]
    pub fn semantic_dispatch(
        &self,
    ) -> crate::project_semantic_dispatch::ProjectSemanticDispatch<'_> {
        crate::project_semantic_dispatch::ProjectSemanticDispatch::new(self)
    }

    /// Access the project-global type-resolution cache root.
    ///
    /// Owned exclusively by the host; shared through an `Arc` so
    /// downstream cache consumers can hold stable references without
    /// taking the host lock.
    pub fn project_type_store(&self) -> &Arc<crate::project_type_store::ProjectTypeStore> {
        &self.project_type_store
    }

    /// Reference to the host-owned CPU pool used by every host batch
    /// API's outer coordinator (`compile_many` and the component-meta
    /// batch both fan out on it through the host batch coordinator).
    ///
    /// The pool is built once at host construction (worker count from
    /// [`crate::types::HostConfig::host_cpu_threads`], defaulting to
    /// `std::thread::available_parallelism`) and reused across every
    /// batch call. Distinct from the scheduler's own CPU pool — see
    /// [`verter_scheduler::HostCpuPool`] for the dual-pool isolation
    /// invariant.
    ///
    /// Not present on `wasm32` — `compile_many` is gated behind
    /// `#[cfg(not(target_arch = "wasm32"))]` and the host-pool field
    /// is gated alongside it.
    ///
    /// Crate-internal: the host pool is a batch-coordination
    /// implementation detail. Downstream crates that need to size the
    /// pool should pass `HostConfig::host_cpu_threads` at host
    /// construction (exposed end-to-end through
    /// `FfiHostConfig::host_cpu_threads` and
    /// `NapiHostConfig::hostCpuThreads`); they should not reach into
    /// the pool itself. Narrowing this visibility prevents
    /// dual-pool-isolation regressions where a downstream consumer
    /// could route its own CPU work onto the batch-coordinator pool
    /// (which would defeat the isolation invariant). Test code
    /// in this crate reads the pool through `host.host_cpu_pool()`
    /// for the `pool_id` token assertion.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub(crate) fn host_cpu_pool(&self) -> &Arc<verter_scheduler::HostCpuPool> {
        &self.host_cpu_pool
    }

    /// The host's batch-coordinator primitive, bound to the host-owned
    /// coordinator pool.
    ///
    /// Every host/runtime batch fan-out (component-meta batch, batch
    /// SFC compile) routes its outer wait through the returned
    /// [`crate::host_batch_coordinator::HostBatchCoordinator`] so the
    /// coordinator wait runs on the dedicated coordinator pool, never on
    /// the scheduler's stage-execution pool. This is the single
    /// host-side coordination rule; call sites must not re-implement an
    /// ad-hoc `host_cpu_pool().install(...)` fan-out.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub(crate) fn batch_coordinator(
        &self,
    ) -> crate::host_batch_coordinator::HostBatchCoordinator<'_> {
        crate::host_batch_coordinator::HostBatchCoordinator::new(self.host_cpu_pool())
    }

    /// wasm32 batch coordinator: there is no coordinator pool, so the
    /// returned primitive runs every batch inline / sequentially with
    /// identical observable ordering.
    #[cfg(target_arch = "wasm32")]
    #[must_use]
    pub(crate) fn batch_coordinator(
        &self,
    ) -> crate::host_batch_coordinator::HostBatchCoordinator<'_> {
        crate::host_batch_coordinator::HostBatchCoordinator::new()
    }

    /// Env-hash bundle (R21) attached to a [`HostStoreView`] at
    /// view-build time. Returns the workspace-default env-hash bundle
    /// composed from the workspace's parser fingerprint + resolve
    /// extensions + project-less identity. Per-canonical resolution
    /// uses [`Self::host_view_env_hashes_for`] instead so multi-project
    /// workspaces can pick the right project context.
    pub(crate) fn host_view_env_hashes(&self) -> crate::session_view::EnvHashes {
        env_hashes_from_array(self.workspace().workspace_default_env_hash_array())
    }

    /// Project identity (R21) attached to a [`HostStoreView`] at
    /// view-build time. Returns the workspace-default project identity
    /// (the project-less identity carried by every published workspace);
    /// per-canonical resolution uses
    /// [`Self::host_view_project_identity_for`].
    pub(crate) fn host_view_project_identity(&self) -> crate::file_artifact_store::ProjectIdentity {
        crate::file_artifact_store::ProjectIdentity(
            self.workspace().workspace_default_project_identity_hash(),
        )
    }

    /// Per-canonical env-hash bundle.
    ///
    /// Maps `canonical` to its owning project via the published
    /// snapshot's `owners_for_file().first()`, then looks up the
    /// project's env-hash array on `PublishedRoot::env_hashes_by_project`.
    /// Falls back to the workspace-default env-hash array when the
    /// canonical has no owning project (e.g. ambient libs, scratch
    /// canonicals, or a workspace that has not yet published a real
    /// snapshot).
    #[must_use]
    pub fn host_view_env_hashes_for(&self, canonical: &str) -> crate::session_view::EnvHashes {
        let workspace = self.workspace();
        let arr = self
            .resolve_project_for_canonical(canonical)
            .and_then(|p| workspace.env_hash_array_for_project(p))
            .unwrap_or_else(|| workspace.workspace_default_env_hash_array());
        env_hashes_from_array(arr)
    }

    /// Per-canonical project identity.
    ///
    /// Maps `canonical` to its owning project via the published
    /// snapshot's `owners_for_file().first()`, then looks up the
    /// project's identity hash on
    /// `PublishedRoot::project_identity_hashes`. Falls back to the
    /// workspace-default project identity when the canonical has no
    /// owning project.
    #[must_use]
    pub fn host_view_project_identity_for(
        &self,
        canonical: &str,
    ) -> crate::file_artifact_store::ProjectIdentity {
        let workspace = self.workspace();
        let hash = self
            .resolve_project_for_canonical(canonical)
            .and_then(|p| workspace.project_identity_hash_for_project(p))
            .unwrap_or_else(|| workspace.workspace_default_project_identity_hash());
        crate::file_artifact_store::ProjectIdentity(hash)
    }

    /// Resolve `canonical` to its owning project under the currently
    /// published workspace snapshot. Returns `None` when no snapshot is
    /// published yet, when no project claims the canonical, or when the
    /// workspace adapter does not maintain a published snapshot.
    ///
    /// Ambiguous owners (multi-project overlap) return the precedence-
    /// first entry — `WorkspaceSnapshot::owners_for_file` already orders
    /// owners by precedence (longest root first, Configured before
    /// Fallback, alphabetical tiebreak).
    #[must_use]
    pub fn resolve_project_for_canonical(
        &self,
        canonical: &str,
    ) -> Option<verter_workspace::workspace_snapshot::ProjectId> {
        let root = self.workspace().published_root()?;
        root.snapshot.owners_for_file(canonical).first().copied()
    }

    /// Host-owned scratch cache for the typeinfo
    /// `evaluate_type_expression` entry-point. Used internally by
    /// [`Self::evaluate_type_expression_with_audit`] to memoise the
    /// `(scratch_uri → SemanticNodeId)` mapping for cacheable
    /// requests.
    pub(crate) fn scratch_cache(
        &self,
    ) -> &parking_lot::Mutex<crate::typeinfo::scratch_cache::ScratchCache> {
        &self.typeinfo_scratch_cache
    }

    /// The Vue adapter's typed framework-surface DTO store — the host-owned
    /// cache of `.vue` macro-surface normalized DTOs.
    ///
    /// The store lives erased on the Vue registration row
    /// ([`crate::framework::registry::FrameworkRegistration::surface_store`]);
    /// this accessor performs the ONE downcast at store acquisition to the typed
    /// [`FrameworkSurfaceStore<VueSurfaceKey, MacroSurfaceDtos>`](crate::framework::surface_store::FrameworkSurfaceStore),
    /// exactly the public-hidden downcast doctrine the carriers use. Used by the
    /// relocated [`crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx`]
    /// to materialize each `.vue` macro surface once per `(canonical, content,
    /// macro, level)`.
    ///
    /// Panics only on a build defect (the Vue registration absent, or its
    /// surface store erased to the wrong concrete type) — neither is reachable
    /// on a correctly-constructed host (`framework_registry_complete` +
    /// `vue_registration_carries_every_leg` pin the registration).
    pub(crate) fn vue_surface_store(
        &self,
    ) -> &crate::framework::surface_store::FrameworkSurfaceStore<
        crate::typeinfo::framework_surface::VueSurfaceKey,
        crate::typeinfo::framework_surface::MacroSurfaceDtos,
    > {
        self.framework_registry()
            .get(&verter_language::FrameworkAdapterId::vue())
            .expect("the Vue adapter is registered")
            .surface_store
            .as_any()
            .downcast_ref()
            .expect(
                "the Vue surface store is FrameworkSurfaceStore<VueSurfaceKey, MacroSurfaceDtos>",
            )
    }

    /// The framework adapter registry — the executor / synth-injection /
    /// public-API-projection dispatch authority. Built once at host
    /// construction and immutable thereafter.
    pub(crate) fn framework_registry(&self) -> &crate::framework::FrameworkAdapterRegistry {
        &self.framework_registry
    }

    /// The framework script-fact caches — the resolved-validation half's
    /// content-addressed candidate store + resolved-fact store. Empty for every
    /// adapter in this program (no production provider registers).
    pub(crate) fn framework_script_caches(
        &self,
    ) -> &crate::framework::script_facts::FrameworkScriptCaches {
        &self.framework_script_caches
    }

    /// Inject the framework-synthesized `default` value symbol into a file's
    /// shallow state, dispatched through the registry's synthesis leg.
    ///
    /// The synth leg is selected by the canonical's resolved framework adapter
    /// id. A typeinfo evaluation scratch (`verter://typeinfo/…`) is a
    /// host-internal surface that inlines an arbitrary scope's eval-source as a
    /// prelude; it classifies by its own `.ts` suffix yet must synthesize the
    /// inlined scope's `default`, so it routes to the synthesizing framework's
    /// leg — Vue is the only framework that synthesizes a `default` in this
    /// program. A no-op when the canonical has no synth leg, when synthesis
    /// returns `None`, or when a userland `default` already exists (userland
    /// always wins).
    pub(crate) fn inject_component_default_into_shallow_state(
        &self,
        canonical_id: &str,
        state: &mut crate::resolver_core::ShallowFileState,
        macros: &[verter_semantic::analysis::types::AnalyzedMacro],
    ) {
        // Userland `default` always wins — never overwrite it.
        if state.value_symbol("default").is_some() {
            return;
        }
        let language = self.language_classifier.classify(canonical_id);
        let adapter_id = match language.adapter_id() {
            Some(id) => id.clone(),
            // A typeinfo evaluation scratch inlines its scope's eval-source; it
            // has no resolved framework language, so route it to the registry's
            // synthesizing framework leg (REGISTRY DATA, not a `vue()` literal).
            // No synthesizing adapter ⇒ no-op (no `default` to inject).
            None if crate::resolver_core::vue_default_synth::is_typeinfo_scratch(canonical_id) => {
                match self.framework_registry().synthesizing_adapter_id() {
                    Some(id) => id,
                    None => return,
                }
            }
            None => return,
        };
        let Some(synth) = self.framework_registry().synth_for(&adapter_id) else {
            return;
        };
        let candidates =
            verter_semantic::analysis::framework_facts::FrameworkScriptCandidateSet::default();
        let cx = crate::framework::synth::ComponentDefaultSynthCtx {
            canonical_id,
            language: &language,
            macros,
            script_candidates: &candidates,
        };
        if let Some(default_symbol) = synth.synthesise(cx) {
            state.insert_synthesised_value_symbol("default", default_symbol);
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // Rehomed off-store cache accessors.
    //
    // The four off-store fields (`compile_cache`, `resolved_type_cache`,
    // `eval_env_cache`, `semantic_db`) live on the `ProjectTypeStore`
    // typed-DB wrappers, not on `VerterHost`. These accessors return
    // references to the rehomed storage so call sites that previously
    // used `host.<field>.<method>()` keep their shape via
    // `host.<field>().<method>()`.
    // ──────────────────────────────────────────────────────────────────

    /// Reference to the profile-domain DB's underlying storage (D48 split).
    /// Stores [`crate::types::ProfileState`] keyed by canonical id; call
    /// sites use `host.compile_cache().entry(...)` / `.get(...)` / `.iter()`
    /// etc. to access per-profile compile outputs (`compile_slots`,
    /// `content_overrides`, `style_overrides`, `latest_diagnostics`,
    /// `diagnostics_generation`).
    #[must_use]
    pub(crate) fn compile_cache(&self) -> &dashmap::DashMap<String, crate::types::ProfileState> {
        self.project_type_store.compile_cache().entries()
    }

    /// Reference to the source-content-domain DB's underlying storage
    /// (D48 split). Stores [`crate::types::DerivedRawState`] keyed by
    /// canonical id; call sites access source-derived caches
    /// (`cached_tsc_extract`, `cached_resolved_meta`, `cached_meta_payload`,
    /// `raw_template_analysis`, `cached_fallthrough`, `import_routes`,
    /// `evicted`, `evicted_whole_hash`).
    #[must_use]
    pub(crate) fn derived_raw_cache(
        &self,
    ) -> &dashmap::DashMap<String, crate::types::DerivedRawState> {
        self.project_type_store.derived_raw_cache().entries()
    }

    /// Reference to the dependency-closure-domain DB's underlying storage
    /// (D48 split). Stores [`crate::types::DependencyState`] keyed by
    /// canonical id; call sites access resolution metadata
    /// (`dependencies`, `resolved_type_hashes`, `aliases`, `generation`).
    #[must_use]
    pub(crate) fn dependency_cache(
        &self,
    ) -> &dashmap::DashMap<String, crate::types::DependencyState> {
        self.project_type_store.dependency_cache().entries()
    }

    /// Aggregate "drop ALL three per-canonical compile-cache sub-states"
    /// helper — used by file-deletion and explicit-eviction paths that
    /// need to clear ProfileState + DerivedRawState + DependencyState in
    /// one call. Per D48 the matrix governs *automatic* cascade triggers;
    /// this helper covers the file-removal path which is outside the
    /// matrix (a deleted file no longer exists, so all three sub-states
    /// drop together).
    pub(crate) fn drop_all_per_canonical_compile_caches(&self, canonical: &str) {
        self.compile_cache().remove(canonical);
        self.derived_raw_cache().remove(canonical);
        self.dependency_cache().remove(canonical);
        // The content-addressed compile-output node lives on the
        // project-global store, not on the per-canonical ProfileState, so
        // it is not dropped by removing the three sub-states above; evict
        // the removed file's content entries explicitly.
        self.compile_output_pure_content()
            .remove_canonical(canonical);
    }

    /// True if the canonical is currently flagged as evicted on its
    /// DerivedRawState entry. The eviction flag (D48 split) lives in
    /// the source-content-domain DB; this helper centralizes the check
    /// so call sites that previously used `compile_cache().get(c).evicted`
    /// migrate cleanly. Returns `false` for unknown canonicals (not
    /// present in any DB) — matching the pre-split semantics where a
    /// missing entry was treated as not-evicted.
    #[must_use]
    pub(crate) fn is_canonical_evicted(&self, canonical: &str) -> bool {
        self.derived_raw_cache()
            .get(canonical)
            .map(|d| d.evicted)
            .unwrap_or(false)
    }

    /// Reference to the rehomed resolved-type cache wrapper. Use
    /// `host.resolved_type_cache().lookup(...)` and `.insert(...)`
    /// (the bounded clear-all-at-cap policy lives inside the DB).
    #[must_use]
    pub(crate) fn resolved_type_cache(&self) -> &crate::project_type_store::ResolvedTypeCacheDb {
        self.project_type_store.resolved_type_cache()
    }

    /// Reference to the rehomed eval-env / owned-program cache
    /// wrapper.
    #[must_use]
    pub(crate) fn eval_env_cache(&self) -> &crate::project_type_store::EvalEnvCacheDb {
        self.project_type_store.eval_env_cache()
    }

    /// `MutexGuard` access to the rehomed
    /// [`verter_semantic::db::SemanticDb`] handle. Call sites that
    /// previously used `host.semantic_db.lock()` now use
    /// `host.semantic_db()` and receive the same guard type.
    pub(crate) fn semantic_db(
        &self,
    ) -> parking_lot::MutexGuard<'_, verter_semantic::db::SemanticDb> {
        self.project_type_store.semantic_db()
    }
}

/// Unpack a `[Hash16; 4]` env-hash array (workspace layout
/// `[parse, resolve, type_, lib]`) into the session-side
/// [`crate::session_view::EnvHashes`] carrier. Used by the
/// `host_view_env_hashes*` accessors so the workspace-side layout and
/// the session-side carrier stay in lockstep.
fn env_hashes_from_array(
    arr: verter_workspace::ProjectEnvHashArray,
) -> crate::session_view::EnvHashes {
    crate::session_view::EnvHashes {
        parse_env_hash: arr[0],
        resolve_env_hash: arr[1],
        type_env_hash: arr[2],
        lib_env_hash: arr[3],
    }
}

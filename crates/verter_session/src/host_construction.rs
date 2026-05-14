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
//! the struct definition in `lib.rs`.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::shared::default_shared;
use crate::types::HostConfig;
#[cfg(feature = "session_metrics")]
use crate::types::HostMetrics;
use crate::{
    host_executor, next_host_instance_id, HostResolverState, VerterHost, WorkspaceSourceLoader,
};

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
        // Thread the host's configured `resolve_extensions` into the
        // workspace at construction so reverse-dep stem stripping
        // honours the host policy from the start.
        workspace.set_default_resolve_extensions(config.resolve_extensions.clone());

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
        Self {
            instance_id: next_host_instance_id(),
            config,
            workspace: workspace_lock,
            alias_to_canonical: default_shared(FxHashMap::default()),
            tick: std::sync::atomic::AtomicU64::new(1),
            store_view_epoch: std::sync::atomic::AtomicU64::new(1),
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
            typeinfo_scratch_cache: parking_lot::Mutex::new(match scratch_cache_capacity {
                Some(cap) => crate::typeinfo::scratch_cache::ScratchCache::with_capacity(cap),
                None => crate::typeinfo::scratch_cache::ScratchCache::with_default_capacity(),
            }),
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
    pub fn host_view_env_hashes_for(
        &self,
        canonical: &str,
    ) -> crate::session_view::EnvHashes {
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

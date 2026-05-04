//! `impl VerterHost` — lifecycle, workspace-bridge, and dependency
//! invalidation methods.
//!
//! Owns:
//! - workspace-bridge accessors (`set_workspace`, `provenance`,
//!   `provenance_snapshot`, `ws`, `bump_store_view_epoch`,
//!   `current_store_view_epoch`)
//! - import-resolution wrappers (`resolve_import_via_workspace`,
//!   `resolve_via_vfs`, `preferred_specifier`)
//! - cache-cascade methods (`integrate_scheduler_snapshot`,
//!   `clear_compile_cache`, `intrinsic_members_for_tag`)
//! - file lifecycle (`close`, `configure_projects`, `notify_close`,
//!   `notify_upsert`, `set_exact_resolutions`, `evict`,
//!   `ensure_loaded`)
//! - alias-map maintenance (`resolve_alias_or_canonical`,
//!   `update_alias_map`)
//! - dependency invalidation (`smart_invalidate_dependents`)
//! - feature-gated metrics snapshot (`metrics_snapshot`)
//!
//! These methods all share the host-cache cascade discipline: when a
//! mutation touches workspace state, the per-cache invalidation hooks
//! run alongside the workspace-side mutator so the caches and the
//! workspace stay in lockstep.

use std::collections::BTreeSet;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use crate::deps;
use crate::id::canonicalize_id;
use crate::shared::{read_lock, write_lock};
#[cfg(feature = "session_metrics")]
use crate::types::HostMetricsSnapshot;
use crate::types::{MetaProvenance, MetaProvenanceSnapshot};
use crate::VerterHost;

impl VerterHost {
    /// Swap the workspace backing this host.
    ///
    /// The scheduler's `SourceLoader` shares the same `Arc<RwLock>`, so
    /// it automatically reads through the new workspace after this call.
    ///
    /// Re-applies `HostConfig::resolve_extensions` to the new workspace
    /// so reverse-dep stem stripping continues to honour the host's
    /// configured extension list across LSP/test workspace swaps.
    pub fn set_workspace(&self, workspace: Arc<dyn verter_workspace::WorkspaceAccess>) {
        workspace.set_default_resolve_extensions(self.config.resolve_extensions.clone());
        *self.workspace.write() = workspace;
        // `set_workspace` is the most aggressive possible mutation: the
        // entire workspace authority swaps out, so every cache layer's
        // identity is potentially invalidated. Mirrors the
        // configure_projects cascade plus the resolver / resolved-type /
        // eval-env / semantic clears that close() runs.
        self.project_type_store.bump_project_generation_and_evict();
        self.project_type_store.route_owned_shallow().clear_all();
        self.resolver.reset_all();
        self.resolved_type_cache().clear();
        self.eval_env_cache().clear();
        self.semantic_invalidate_all();
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
        // The scheduler owns its own counters in the `verter_scheduler`
        // crate; mirror them into the session-facing snapshot so callers
        // have a single observation surface.
        let sched_counters = self.scheduler.counters();
        snapshot.scheduler_submit_count = sched_counters.submit_count.load(Relaxed);
        snapshot.scheduler_inbox_depth_max = sched_counters.inbox_depth_max.load(Relaxed);
        snapshot
    }

    /// Clone the workspace `Arc` for internal use.
    pub(crate) fn ws(&self) -> Arc<dyn verter_workspace::WorkspaceAccess> {
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

    /// Materialize native-side lifecycle state from the current scheduler snapshot.
    ///
    /// This is the scheduler-backed replacement for the old `files`-map
    /// ingress: it updates `compile_cache` identity/dependency state
    /// without re-submitting source back into the scheduler.
    ///
    /// Writes parsed edges into the workspace (workspace is sole
    /// authority for reverse-dep tracking). `cc.import_routes` are
    /// PRESERVED across integrate (bundlers may have populated them via
    /// `set_import_dependencies` before the source was loaded). After
    /// `record_parsed_edges` clears the workspace's `exact_resolved`
    /// set, exacts are re-applied via `set_exact_resolutions` from
    /// preserved `cc.import_routes` so the workspace mirrors host
    /// bundler state.
    pub(crate) fn integrate_scheduler_snapshot(&self, canonical_id: &str) -> bool {
        use crate::host_executor::HostSourceData;

        let snap = match self.scheduler.try_get_source(canonical_id) {
            Some(s) => s,
            None => return false,
        };
        let Some(hd) = snap.downcast_data::<HostSourceData>() else {
            return false;
        };

        let parsed_edges = Self::build_parsed_edges_from_analysis(
            canonical_id,
            &hd.parse.external_requests,
            &hd.parse.script_analysis.imports,
            &hd.parse.script_analysis.module_references,
        );

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

        let (old_aliases, preserved_routes) = {
            let mut cc_ref = self
                .compile_cache()
                .entry(canonical_id.to_string())
                .or_default();
            let cc = cc_ref.value_mut();
            let old_aliases = cc.aliases.clone();
            // PRESERVE cc.import_routes. Bundler may have set them via
            // set_import_dependencies before source was loaded. Cloning
            // here so we can re-apply to workspace below without holding
            // the cc lock.
            let preserved_routes = cc.import_routes.clone();
            cc.aliases = aliases.clone();
            cc.dependencies = deps;
            cc.generation = snap.generation;
            cc.evicted = false;
            // cc.import_routes is NOT cleared (preserves bundler
            // pre-load route flow).
            (old_aliases, preserved_routes)
        };

        self.update_alias_map(canonical_id, &old_aliases, &aliases);

        // Workspace is sole authority for reverse-dep tracking.
        // record_parsed_edges CLEARS workspace
        // exact_resolved/exact_resolutions/lazy_resolved/semantic_transitive.
        // ambient_resolved survives.
        self.ws().record_parsed_edges(canonical_id, &parsed_edges);

        // Re-apply workspace exacts from preserved cc.import_routes so
        // the workspace mirrors host bundler state. No-op when
        // cc.import_routes is empty (typical first-load case where
        // bundler hasn't touched the file).
        if !preserved_routes.is_empty() {
            let exact_resolutions =
                self.build_exact_resolutions_from_routes(canonical_id, &preserved_routes);
            self.ws()
                .set_exact_resolutions(canonical_id, exact_resolutions);
        }
        // Publish-fence: EdgeStore is RwLock-protected; concurrent
        // readers see pre-write or post-write state, never torn.
        true
    }

    /// Clear compile caches (compile slots, template analysis, type
    /// hashes) without removing files from the scheduler or alias maps.
    ///
    /// This is a lighter operation than [`close`](Self::close) — parsed
    /// source and analysis snapshots are preserved, only per-profile
    /// compile results are flushed. Useful for invalidating stale
    /// compile results while keeping the file set intact.
    pub fn clear_compile_cache(&self) {
        for mut entry in self.compile_cache().iter_mut() {
            entry.compile_slots.clear();
            entry.raw_template_analysis = None;
            entry.cached_tsc_extract = None;
            entry.cached_resolved_meta.clear();
            entry.cached_meta_payload = None;
            entry.cached_fallthrough = None;
        }
        self.resolved_type_cache().clear();
        self.eval_env_cache().clear();
        // Extend cascade with the new `RouteOwnedShallowDb` bulk
        // eviction. Mirrors the route-resolution invalidation discipline.
        self.project_type_store.route_owned_shallow().clear_all();
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
    /// After calling `close()` the host is empty but still usable (you
    /// could upsert files again). The primary purpose is to allow the
    /// Rust allocator to free the backing memory so that NAPI-RS-backed
    /// hosts don't keep the Node.js process alive waiting for GC
    /// finalisation.
    pub fn close(&self) {
        // Notify the workspace for each tracked file so overlays AND
        // edge store are cleared before scheduler nodes are removed.
        // Use notify_delete (not notify_close) to clear the VFS edge
        // store entries.
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
        write_lock(&self.last_const_prop_overrides).clear();

        self.compile_cache().clear();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.scheduler.reset();
            self.scheduler.restart_driver();
        }
        self.resolved_type_cache().clear();
        self.resolver.reset_all();
        self.eval_env_cache().clear();
        self.provenance.reset();
        // Clear all semantic caches
        *self.semantic_db() = verter_semantic::db::SemanticDb::new();
        // close-cascade extension for the `RouteOwnedShallowDb`.
        // `close()` already resets the resolver (which clears RouteDb /
        // ImportedRootDb), so route-resolution facts are gone; clear
        // the route-only shallow DB in lockstep.
        self.project_type_store.route_owned_shallow().clear_all();
        self.bump_store_view_epoch();
    }

    /// Configure project-scoped path alias resolution.
    ///
    /// Accepts a list of [`IdeProjectConfig`] describing tsconfig paths,
    /// workspace aliases, and project references. The host uses these
    /// to resolve aliased import specifiers (e.g. `@/components/Foo.vue`,
    /// `#imports`) without relying on external caller-provided
    /// resolutions.
    ///
    /// Delegates to the VFS workspace's `configure_resolver()` which
    /// updates the project graph and publishes a new snapshot
    /// atomically. Pass an empty slice to clear the resolver.
    pub fn configure_projects(
        &self,
        projects: Vec<verter_semantic::analysis::project_resolver::IdeProjectConfig>,
    ) {
        self.ws().configure_resolver(projects);
        for mut entry in self.compile_cache().iter_mut() {
            entry.import_routes.clear();
            entry.dependencies.clear();
        }
        self.resolver.reset_all();
        self.resolved_type_cache().clear();
        self.eval_env_cache().clear();
        self.semantic_invalidate_all();
        // `configure_projects` is a route-resolution mutation: the
        // project graph changes, which means the cached route-only
        // shallow entries' `project_generation` tag is now stale. Bump
        // project_generation (also evicts the project-shape cluster:
        // owner_import_surfaces, semantic_graph, component_meta_results,
        // etc.) and clear_all the route-only shallow DB. The
        // materialiser's tier-3 staleness gate is the safety net for
        // any in-flight cold publish that started before the bump.
        self.project_type_store.bump_project_generation_and_evict();
        self.project_type_store.route_owned_shallow().clear_all();
        self.bump_store_view_epoch();
    }

    /// Host wrapper for [`WorkspaceAccess::notify_close`] that runs the
    /// cache-eviction cascade alongside the workspace-side overlay
    /// clear. Replaces direct `host.workspace().notify_close(...)`
    /// calls (now `pub(crate)`-gated).
    ///
    /// EVICT FIRST. `notify_close` bumps `content_generation`; the
    /// materialiser's tier-2 gate catches stale entries via
    /// workspace_generation mismatch on subsequent reads. The
    /// pre-publish fence catches in-flight publishes by re-reading
    /// content_generation immediately before publish.
    pub fn notify_close(&self, canonical_id: &str) {
        self.project_type_store
            .route_owned_shallow()
            .remove(canonical_id);
        self.ws().notify_close(canonical_id);
    }

    /// Host wrapper for [`WorkspaceAccess::notify_upsert`] that runs
    /// the route-only cache eviction alongside the workspace-side
    /// overlay write. Replaces direct
    /// `host.workspace().notify_upsert(...)` calls.
    ///
    /// EVICT FIRST. `ws().notify_upsert` internally bumps
    /// `content_generation`, which feeds the materialiser's tier-2
    /// fallback gate. Eviction-first shrinks the race window. The
    /// residual race (a concurrent cold reader publishes an entry
    /// tagged with the pre-mutation workspace_generation immediately
    /// before this wrapper's `content_generation` bump lands) is
    /// tolerated: the next reader's tier-2 gate catches it via
    /// generation mismatch and re-materialises.
    ///
    /// Note: the full content-change cascade (resolved_type_cache,
    /// semantic_invalidate, etc.) belongs on `host.upsert(canonical,
    /// source)` — the authoritative content-change pipeline.
    /// `notify_upsert` is the overlay-signal hook only.
    pub fn notify_upsert(&self, canonical_id: &str, source: Arc<str>) {
        self.project_type_store
            .route_owned_shallow()
            .remove(canonical_id);
        self.ws().notify_upsert(canonical_id, source);
    }

    /// Host wrapper for [`WorkspaceAccess::set_exact_resolutions`] with
    /// the FULL `set_import_dependencies` cascade shape PLUS
    /// `bump_project_generation_and_evict` and
    /// `route_owned_shallow.clear_all`.
    ///
    /// `set_exact_resolutions` is a route-resolution mutation — the
    /// project graph changes but `content_generation` does NOT bump.
    /// Without bumping `project_generation`, an in-flight materialiser
    /// that captured the old generation could publish a stale entry,
    /// and the tier-3 gate would let the stale entry through on
    /// subsequent reads. Bumping `project_generation` in this wrapper
    /// closes that race.
    pub fn set_exact_resolutions(
        &self,
        canonical: &str,
        resolutions: Vec<verter_workspace::ExactResolution>,
    ) {
        // EVICT-FIRST: bump project_generation BEFORE the workspace
        // mutator so a concurrent in-flight materialiser's pre-read
        // project_generation capture is invalidated by tier-3 before
        // it can publish.
        self.project_type_store.bump_project_generation_and_evict();
        self.project_type_store.route_owned_shallow().clear_all();
        self.ws().set_exact_resolutions(canonical, resolutions);
        self.resolver.runtime.invalidate_canonical(canonical);
        self.project_type_store.evict_canonical(canonical); // belt-and-suspenders per-canonical
        self.resolved_type_cache().clear();
        self.semantic_invalidate(canonical);
        self.bump_store_view_epoch();
    }

    /// Snapshot of feature-gated host-level metrics counters.
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

    /// Evict a file's cached entry so the next access reloads from disk.
    ///
    /// Used by `did_close` to discard the editor-buffer version. Unlike
    /// `remove()`, this does NOT clean up aliases, reverse deps, or VFS
    /// state — the file still exists on disk, it just needs a fresh parse.
    ///
    /// On the scheduler path, sets `evicted = true` and clears profile
    /// state (compile_slots, overrides, diagnostics) but preserves
    /// deps/aliases for old-state diffing during reload. The eviction
    /// gate makes the file invisible to host accessors until
    /// `ensure_loaded()` re-integrates.
    pub fn evict(&self, canonical_id: &str) {
        self.ws().notify_close(canonical_id);
        self.semantic_db().invalidate(canonical_id);

        // Capture pre-evict whole_hash from the scheduler so
        // `ensure_loaded` can detect no-op reloads (identical content)
        // and skip the redundant `bump_store_view_epoch`.
        let pre_evict_hash = self
            .scheduler
            .try_get_source(canonical_id)
            .map(|s| s.whole_hash);
        if let Some(mut cc) = self.compile_cache().get_mut(canonical_id) {
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
            cc.cached_meta_payload = None;
            cc.cached_fallthrough = None;
        }
        self.bump_store_view_epoch();
    }

    /// Ensure a file is loaded into the host.
    ///
    /// The scheduler is the sole ingress authority: this method submits
    /// a `source: None` request to the scheduler (which loads content
    /// via the workspace-backed SourceLoader), waits for Analysis to
    /// commit, then materializes native-side lifecycle state from the
    /// committed scheduler snapshots without re-submitting the source.
    pub fn ensure_loaded(&self, canonical_id: &str) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        self.provenance.ensure_loaded_calls.fetch_add(1, Relaxed);
        let normalized_canonical = self.normalized_analysis_canonical(canonical_id);
        let canonical_id = normalized_canonical.as_ref();
        // Fast path: already in host and not evicted. Also verify the
        // scheduler still has the source — `set_import_dependencies`
        // may create an empty compile_cache stub before the file is
        // loaded into the scheduler; in that case we must proceed to
        // submit a load request.
        if let Some(cc) = self.compile_cache().get(canonical_id) {
            if !cc.evicted && self.scheduler.try_get_source(canonical_id).is_some() {
                return true;
            }
        }

        use verter_scheduler::job::CompletionState;

        let (reload_from_workspace, pre_evict_hash) = self
            .compile_cache()
            .get(canonical_id)
            .filter(|cc| cc.evicted)
            .map(|cc| (true, cc.evicted_whole_hash))
            .unwrap_or((false, None));

        if reload_from_workspace {
            // Evicted files must force the scheduler off any stale
            // committed snapshot before we request a disk-backed reload.
            self.scheduler.close_file(canonical_id);
        }

        // Submit to scheduler — it loads via WorkspaceSourceLoader.
        // Thread the current-thread's `OpaqueRequestContext` (if any)
        // into the request so worker threads install it before running
        // stages — that way fan-out events from `workspace.read_file`
        // during `SourceStage` carry the outer request_id and the
        // session-side `SessionVfsSink` picks them up.
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

        // Wait for the scheduler to reach Analysis. `wait_or_drive`
        // drives stages inline on WASM (no driver thread); on native it
        // delegates to `handle.wait()` when the driver thread is
        // installed. Split wait (scheduler drive) vs work
        // (integrate_scheduler_snapshot) so diagnosis can tell
        // load-path contention from post-load processing.
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
        // First-time loads are purely additive: they populate host
        // state for a file that no previously-captured view tracks, so
        // they cannot invalidate any existing snapshot's facts. Only
        // re-loads (content reload after an evict) may have changed
        // the file's hash relative to what older views pinned, so only
        // those need to bump the global mutation epoch.
        //
        // Compare post-reload hash to the pre-evict hash; if identical,
        // the reload is a content no-op and we can skip the bump
        // entirely. This preserves the type-context cache across
        // load→evict→ensure_loaded cycles that don't actually change
        // the file. `pre_evict_hash == None` (e.g. evict triggered
        // without a prior scheduler snapshot) falls back to the
        // conservative bump.
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

    /// Resolve an alias to its canonical ID, or normalize the ID if no
    /// alias exists.
    pub(crate) fn resolve_alias_or_canonical(&self, id: &str) -> String {
        let normalized = canonicalize_id(id);
        let alias_map = read_lock(&self.alias_to_canonical);
        alias_map
            .get(normalized.as_ref())
            .cloned()
            .unwrap_or_else(|| normalized.into_owned())
    }

    /// Sync the alias-to-canonical map: remove stale aliases, insert
    /// current ones.
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

    /// Smart invalidation: when a dependency changes, only invalidate
    /// dependent SFCs whose macro-consumed types were actually affected.
    ///
    /// Workspace `reverse_deps_for` is the sole authority. The
    /// workspace internally handles longest-suffix-first stem stripping
    /// against the configured `default_resolve_extensions`, so a single
    /// call covers both canonical and stem-axis hits.
    pub(crate) fn smart_invalidate_dependents(
        &self,
        dependency_id: &str,
        old_export_signatures: &[verter_semantic::analysis::ExportSignature],
        new_export_signatures: &[verter_semantic::analysis::ExportSignature],
    ) {
        let ws = self.ws();
        let owners: BTreeSet<String> = ws.reverse_deps_for(dependency_id).into_iter().collect();

        // When a genuinely new dependency arrives (old signatures
        // empty, new non-empty), dependents may have cached "miss"
        // import routes for this dep. Evict their project-store entries
        // unconditionally so fresh accesses re-resolve import routes.
        // For existing deps where only the export surface changed,
        // scope eviction to the owners that were actually invalidated.
        let dep_is_newly_added =
            old_export_signatures.is_empty() && !new_export_signatures.is_empty();

        let ws_ref = self.workspace.read();
        let cleared = deps::smart_invalidate_dependents_via_scheduler(
            &self.scheduler,
            self.compile_cache(),
            owners.clone(),
            Some(ws_ref.as_ref()),
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
            self.eval_env_cache().clear();
        }
        for owner in evict_targets {
            self.resolver.runtime.invalidate_canonical(owner);
            self.project_type_store.evict_canonical(owner);
        }
    }
}

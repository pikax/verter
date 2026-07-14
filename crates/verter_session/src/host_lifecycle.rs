//! `impl VerterHost` — lifecycle and workspace-bridge methods.
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
//! - workspace edge tracking (reverse-dep graph for memory-bound GC)
//! - feature-gated metrics snapshot (`metrics_snapshot`)
//!
//! These methods all share the host-cache cascade discipline: when a
//! mutation touches workspace state, the per-cache invalidation hooks
//! run alongside the workspace-side mutator so the caches and the
//! workspace stay in lockstep.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::id::canonicalize_id;
use crate::instant::Instant;
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
        // SWAP-FIRST, then clear — the order is load-bearing. Clearing
        // before the swap would be unsound: a concurrent reader could
        // repopulate a just-cleared cache from the OLD workspace, and
        // nothing after the swap clears it again, so old-authority state
        // would serve against the new workspace indefinitely. Swap-first
        // leaves only a transient window where a reader sees the NEW
        // workspace alongside not-yet-cleared cache state; that window
        // is read-side rejected: a flight that captured pre-swap state
        // is fenced at publish by the generation/epoch bumps below
        // (mutate-first fence ordering — each bump strictly follows the
        // state it announces), warm query-identity reads revalidate
        // their recorded fact signatures against the live store view,
        // and content-addressed entries are keyed by whole-hash, so a
        // same-path file with different content in the new workspace
        // misses them by key.
        //
        // `set_workspace` is the most aggressive possible mutation: the
        // entire workspace authority swaps out, so every cache layer's
        // identity is potentially invalidated. Runs the same
        // AUTHORITY-RESET cascade as close(): the wide
        // `bump_project_generation_and_evict` plus the artifact-store /
        // resolver / resolved-type / semantic clears.
        self.project_type_store.bump_project_generation_and_evict();
        // The CONTENT authority itself swapped out: every retained
        // `FileArtifactStore` artifact — content-addressed against the
        // OLD workspace — is orphaned. Artifact-only canonicals would
        // otherwise keep serving against a workspace they never came
        // from (a new workspace can even carry a same-path file with
        // different content, which the `file_exists` freshness gate
        // alone cannot distinguish). Scheduler-tracked canonicals
        // rebuild on demand from their retained scheduler sources.
        self.project_type_store.indexed().clear_all();
        self.resolver.reset_all();
        self.semantic_invalidate_all();
        // The workspace authority swapped, so the cached base-view snapshot
        // (built against the OLD workspace / project graph) is structurally
        // stale. Drop its `Arc` in lockstep with the other full-clear
        // cascade steps so the snapshot's per-file maps are released now
        // rather than lingering until the next store-view request replaces
        // the entry.
        self.store_view_manager().clear();
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

    /// Public accessor for the coarse semantic-mutation epoch counter.
    ///
    /// Exposed for cache-reuse invariant tests that assert
    /// byte-identical `upsert` calls do not bump the epoch (R1 —
    /// quintuple-unchanged upsert is a true cache-state no-op).
    #[must_use]
    pub fn store_view_epoch(&self) -> u64 {
        self.current_store_view_epoch()
    }

    pub(crate) fn bump_store_view_epoch(&self) -> u64 {
        self.store_view_epoch
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1
    }

    /// Current first-time additive-load generation. Folded into the
    /// `StoreViewValidationToken` so a `StoreViewManager`-cached base view
    /// built before a first-time load is invalidated once the load lands.
    #[must_use]
    pub(crate) fn current_load_generation(&self) -> u64 {
        self.load_generation
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Advance the first-time additive-load generation. Called from
    /// `ensure_loaded` on a successful first-time load (and a content-
    /// changing reload). Does NOT clear thread-local caches — the loaded
    /// canonical's content is additive, not a change to already-cached
    /// state. See the `load_generation` field docs for why this is a
    /// dedicated dimension excluded from `externally_superseded_by`.
    pub(crate) fn bump_load_generation(&self) -> u64 {
        self.load_generation
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

        // D48 split — DependencyState owns aliases/dependencies/generation;
        // DerivedRawState owns import_routes/evicted; ProfileState owns
        // per-profile compile outputs. Update each in turn (the
        // ProfileState `or_default()` materializes the entry so callers
        // observing post-load state see all three sub-state DBs populated
        // for the canonical).
        {
            let _ = self
                .compile_cache()
                .entry(canonical_id.to_string())
                .or_default();
        }
        let old_aliases = {
            let mut dep_ref = self
                .dependency_cache()
                .entry(canonical_id.to_string())
                .or_default();
            let dep = dep_ref.value_mut();
            let old_aliases = dep.aliases.clone();
            dep.aliases = aliases.clone();
            dep.dependencies = deps;
            dep.generation = snap.generation;
            old_aliases
        };
        // PRESERVE derived.import_routes. Bundler may have set them via
        // set_import_dependencies before source was loaded. Cloning
        // here so we can re-apply to workspace below without holding the
        // entry lock.
        let preserved_routes = {
            let mut derived_ref = self
                .derived_raw_cache()
                .entry(canonical_id.to_string())
                .or_default();
            let derived = derived_ref.value_mut();
            let preserved_routes = derived.import_routes.clone();
            derived.evicted = false;
            // derived.import_routes is NOT cleared (preserves bundler
            // pre-load route flow).
            preserved_routes
        };

        self.update_alias_map(canonical_id, &old_aliases, &aliases);

        // Workspace is sole authority for reverse-dep tracking. The
        // parsed-edge record CLEARS workspace
        // exact_resolved/exact_resolutions/lazy_resolved/semantic_transitive
        // (ambient_resolved survives), so when bundler-preserved
        // cc.import_routes exist the exacts re-apply must land in the
        // SAME edge-store critical section: a record-then-set two-call
        // sequence exposes an exacts-cleared window in which a concurrent
        // cold flight resolves against the half-applied table and
        // publishes a wrong route surface with no generation moved (the
        // pre-publish fence cannot see a mutation that moves no
        // generation, and content-identical re-records are value no-ops
        // on both stores — the torn window was the only hole).
        if preserved_routes.is_empty() {
            // Typical first-load case (bundler hasn't touched the file):
            // nothing to re-apply, and surviving directly-set workspace
            // exacts (host `set_exact_resolutions` without a host-side
            // route push) must not be clobbered by an empty re-apply.
            self.ws().record_parsed_edges(canonical_id, &parsed_edges);
        } else {
            let exact_resolutions =
                self.build_exact_resolutions_from_routes(canonical_id, &preserved_routes);
            self.ws().record_parsed_edges_with_exact_resolutions(
                canonical_id,
                &parsed_edges,
                exact_resolutions,
            );
        }
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
        // ProfileState (D48 — compile_cache_db): per-profile compile
        // outputs are flushed through the typed session node.
        let session_node = crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
        for mut entry in self.compile_cache().iter_mut() {
            session_node.clear_compile_outputs_for_file(&mut entry);
        }
        // The content-addressed compile-output node is a sibling
        // compile-output store. Flush it alongside the session slots so a
        // cache clear is complete: a subsequent `Content` request must not
        // warm-hit an entry the caller explicitly flushed.
        self.compile_output_pure_content().clear_all();
        // DerivedRawState (D48 — derived_raw_cache_db): source-derived
        // caches (raw template analysis, tsc extract, resolved meta,
        // fallthrough) are flushed. import_routes and evicted flag stay.
        for mut entry in self.derived_raw_cache().iter_mut() {
            entry.clear_raw_template_analysis();
            entry.cached_tsc_extract = None;
            entry.cached_resolved_meta.clear();
            entry.cached_meta_payload = None;
            entry.cached_fallthrough = None;
        }
        self.bump_store_view_epoch();
    }

    /// The generated static-catalog intrinsic surface for `tag`: each member
    /// fact carries its content-free catalog id; consumers recover the type
    /// shape / raise a graph handle on demand.
    pub(crate) fn intrinsic_members_for_tag(
        &self,
        tag: &str,
    ) -> Vec<crate::resolver_core::IntrinsicSurfaceMember> {
        verter_semantic::analysis::html_intrinsics::owned_intrinsic_members_for_tag(tag)
            .into_iter()
            .map(|fact| crate::resolver_core::IntrinsicSurfaceMember {
                name: fact.name,
                kind: fact.kind,
                source: crate::resolver_core::IntrinsicMemberTypeSource::Static(fact.type_id),
            })
            .collect()
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

        // AUTHORITY-RESET cascade: close() is a full teardown, one of
        // the two reserved `bump_project_generation_and_evict` callers
        // (with `set_workspace`). The wide per-canonical clears release
        // the compile / derived / dependency domains, the project-config
        // query-identity DB cluster, the resolved-type cache, and the
        // semantic DB — retained state would otherwise stay resident
        // against an authority that no longer exists — and the
        // project-generation move guarantees no `ProjectGeneration`-
        // rooted entry can validate against state populated before this
        // teardown.
        self.project_type_store.bump_project_generation_and_evict();
        // The content-addressed compile-output store is a sibling
        // compile-output cache; flush it so close() releases ALL cached
        // compile state and frees the backing memory. The session slots
        // released by the cascade above live on the per-file compile
        // cache; the content-addressed entries live on a separate store
        // that dropping the compile cache does not touch.
        self.compile_output_pure_content().clear_all();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.scheduler.reset();
            self.scheduler.restart_driver();
        }
        // close() owns the unified artifact lifecycle: every retained
        // `IndexedReady` (and its `Arc<EvalEnv>`) lives on the
        // `FileArtifactStore`, which neither the cascade above nor the
        // scheduler reset touches. Without this clear the payloads stay
        // resident (breaking the memory-release contract this method
        // exists for) and artifact-only canonicals — untouched by the
        // notify_delete loop, backing file still present — keep serving
        // through the artifact-only authority gate.
        self.project_type_store.indexed().clear_all();
        self.resolver.reset_all();
        self.provenance.reset();
        // Drop the StoreViewManager's cached base-view `Arc`. The epoch
        // bump below invalidates it as a warm-hit candidate, but a
        // token bump alone keeps the `Arc<StoreViewSnapshot>` (and its
        // per-file maps / fact `Arc`s) strongly held until a later
        // store-view request rebuilds and replaces it. A closed-not-reused
        // host (NAPI finalisation) never issues that next request, so
        // without the explicit clear the snapshot stays resident —
        // regressing close()'s memory-release contract. Clearing releases
        // it now.
        self.store_view_manager().clear();
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
        // Project-config change drops resolution-derived state:
        // `import_routes` (DerivedRawState) and `dependencies`
        // (DependencyState). The `import_routes_known_miss_recorded_at_generation`
        // sidecar is cleared in lockstep so a stale `content_generation`
        // stamp does not survive a project-graph reset: leaving it
        // behind would suppress re-resolution after the next admission
        // because the reader's `import_route_is_known_miss` predicate
        // would still consult a generation tag from the previous
        // project graph. Symmetric with
        // [`Self::finish_upsert_post_commit`], which clears both fields
        // when owner source content advances.
        for mut entry in self.derived_raw_cache().iter_mut() {
            entry.import_routes.clear();
            entry
                .import_routes_known_miss_recorded_at_generation
                .clear();
            entry.import_routes_positive_recorded_at_generation.clear();
        }
        for mut entry in self.dependency_cache().iter_mut() {
            entry.dependencies.clear();
        }
        self.resolver.reset_all();
        self.semantic_invalidate_all();
        // `configure_projects` is a route-resolution mutation: the
        // project graph changes. STAMP-ONLY bump — retained payloads
        // (`IndexedReady`, the per-canonical compile/derived/dependency
        // entries beyond the explicit route-field loops above) survive;
        // stale entries miss BY VALIDATION (the project-stamp read gate
        // routes route surfaces through the edge refresh — no re-parse;
        // query-identity caches reject on their `ProjectGeneration`
        // facts / `validated_at_generation` backstops). The wide
        // `bump_project_generation_and_evict` is reserved for content-
        // authority swaps (`set_workspace`, `close`) — wholesale-
        // clearing `derived_raw_cache` here flipped every
        // scheduler-tracked canonical into the artifact-only class.
        self.project_type_store.bump_project_generation();
        // The project graph changed (stamp-only `project_generation`
        // bump above — retained payloads survive, stale entries miss by
        // validation), so the cached base-view snapshot is rooted on a
        // stale project identity. Drop its `Arc` alongside the clears
        // above so its per-file maps release now rather than lingering
        // until the next store-view request.
        self.store_view_manager().clear();
        self.bump_store_view_epoch();
    }

    /// Host wrapper for [`WorkspaceAccess::notify_close`] that evicts an
    /// **artifact-only** canonical's `FileArtifactStore` payload
    /// alongside the workspace-side overlay clear. Replaces direct
    /// `host.workspace().notify_close(...)` calls (now
    /// `pub(crate)`-gated).
    ///
    /// An artifact-only canonical (no scheduler source) has the
    /// workspace as its sole content authority, so a workspace close is
    /// its content-death signal: the retained artifact must not serve
    /// afterwards. The eviction runs AFTER the workspace mutation
    /// (mutate-first — the close also advances `content_generation`,
    /// which the in-flight pre-publish fence reads). Scheduler-tracked
    /// canonicals are untouched here: the scheduler stays their content
    /// authority and the `evict()` / `close()` pipelines own their
    /// lifecycle.
    pub fn notify_close(&self, canonical_id: &str) {
        self.ws().notify_close(canonical_id);
        self.evict_artifact_only_canonical(canonical_id);
    }

    /// Host wrapper for [`WorkspaceAccess::notify_upsert`] that evicts an
    /// **artifact-only** canonical's `FileArtifactStore` payload
    /// alongside the workspace-side overlay write — for such a canonical
    /// the workspace IS the content authority, so an overlay write
    /// supersedes the retained artifact. Mutate-first, then evict (the
    /// workspace write advances `content_generation`, which the
    /// in-flight pre-publish fence reads).
    ///
    /// Scheduler-tracked canonicals are untouched: the scheduler is
    /// their content authority and serves the committed version until
    /// the authoritative content-change pipeline (`host.upsert`) runs —
    /// `notify_upsert` is the overlay-signal hook only.
    pub fn notify_upsert(&self, canonical_id: &str, source: Arc<str>) {
        self.ws().notify_upsert(canonical_id, source);
        self.evict_artifact_only_canonical(canonical_id);
    }

    /// Per-canonical artifact eviction for workspace signals
    /// (`notify_close` / `notify_upsert`) — fires ONLY for an
    /// artifact-only canonical
    /// ([`crate::VerterHost::is_artifact_only_scope`] — the same oracle
    /// the serving authorities consult), whose content authority is the
    /// workspace itself. Together with the single artifact-only
    /// authority gate (`artifact_only_authority_allows`) and the
    /// `set_workspace` / `close` artifact-store clears, this keeps
    /// artifact-only staleness signal-driven.
    fn evict_artifact_only_canonical(&self, canonical_id: &str) {
        let analysis_canonical = self.normalized_analysis_canonical(canonical_id);
        let analysis_canonical = analysis_canonical.as_ref();
        if !self.is_artifact_only_scope(analysis_canonical) {
            return;
        }
        self.project_type_store.evict_canonical(analysis_canonical);
    }

    /// Host wrapper for [`WorkspaceAccess::set_exact_resolutions`] —
    /// STAMP-ONLY freshness with OWNER-SCOPED route-state repair.
    ///
    /// `set_exact_resolutions` is a route-resolution mutation — the
    /// project graph changes but `content_generation` does NOT bump.
    /// Route-resolution mutations never wide-clear retained payloads:
    /// `IndexedReady` / `FileArtifactStore` payloads survive, a stale
    /// route surface fails `indexed_surface_is_current` and takes the
    /// edge-refresh on demand, and only the route mirror the mutation
    /// actually made stale (THIS owner's `DerivedRawState` route
    /// fields) is cleared. Wide clears are reserved for content-
    /// authority swaps (`set_workspace`, `close`).
    pub fn set_exact_resolutions(
        &self,
        canonical: &str,
        mut resolutions: Vec<verter_workspace::ExactResolution>,
    ) {
        // Key EVERY operation below — the workspace edge-store write AND
        // the host-side mirror repair — on the NORMALIZED canonical id
        // (the `set_import_dependencies` discipline). An alias-keyed
        // call must mutate the same workspace edge entry and the same
        // canonical-keyed `DerivedRawState` mirror as the
        // canonical-keyed call: keying the edge store on the alias while
        // the mirror repair targets the canonical splits the route state
        // across two ids (the alias-keyed edge entry is invisible to
        // canonical-keyed resolution, and the canonical mirror keeps
        // serving stale routes).
        let canonical = self.resolve_alias_or_canonical(canonical);
        let canonical = canonical.as_str();
        // Resolution targets are canonical ids for every consumer, and
        // the workspace edge store keeps them verbatim — canonicalize on
        // admission, exactly like `set_import_dependencies` does for its
        // `DependencyResolution` ids.
        for res in &mut resolutions {
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
        }
        // MUTATE-FIRST, then bump (the `configure_projects` /
        // `set_workspace` ordering). The pre-publish fence compares the
        // generation a flight captured at its start against the live
        // generation at publish time, so the bump must STRICTLY FOLLOW
        // the state it announces: bumping before the workspace mutator
        // opens a window where a flight captures the NEW generation,
        // resolves against the OLD resolution table, passes the fence,
        // and is served as current indefinitely. With mutate-first, a
        // flight born inside the window (post-mutation, pre-bump) reads
        // the NEW table and is at worst redundantly refreshed by the
        // stamp gate; a flight that read the OLD table is fenced.
        let result = self.ws().set_exact_resolutions(canonical, resolutions);
        if !result.changed {
            // Value-identical re-push (the duplicate-key-safe engine
            // gate): nothing moved, so the whole invalidation cascade —
            // including the project-wide stamp bump — is skipped.
            return;
        }
        // Owner-scoped route-mirror repair: the workspace exacts for
        // THIS owner just changed, so its derived route mirror (and the
        // generation sidecars that root it) is stale. Other canonicals'
        // mirrors are untouched — their routes did not move.
        if let Some(mut entry) = self.derived_raw_cache().get_mut(canonical) {
            entry.import_routes.clear();
            entry
                .import_routes_known_miss_recorded_at_generation
                .clear();
            entry.import_routes_positive_recorded_at_generation.clear();
        }
        self.project_type_store.bump_project_generation();
        self.resolver.runtime.invalidate_canonical(canonical);
        // Route mutation, content unchanged: drain the canonical's
        // derived layers but RETAIN its content-addressed `IndexedReady`
        // payload — the project-stamp read gate routes the next read
        // through the edge-refresh materialise (route surface rebuilt,
        // no re-parse).
        self.project_type_store
            .evict_canonical_for_route_mutation(canonical);
        // R4 producer: rebuild parse-domain facts for the reloaded
        // canonical so the next resolver pass sees the new content.
        self.register_facts_for_new_content(canonical);
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
        // ProfileState (compile_cache_db): clear per-profile compile
        // outputs. ProfileState has no `evicted` flag; the eviction
        // marker lives on DerivedRawState.
        if let Some(mut profile) = self.compile_cache().get_mut(canonical_id) {
            profile.content_overrides.clear();
            profile.style_overrides.clear();
            let session_node = crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
            session_node.clear_compile_outputs_for_file(&mut profile);
            profile.latest_diagnostics.clear();
        }
        // DerivedRawState (derived_raw_cache_db): set the evicted flag
        // and capture pre-evict whole_hash; clear all source-derived
        // caches.
        {
            let mut derived_ref = self
                .derived_raw_cache()
                .entry(canonical_id.to_string())
                .or_default();
            let derived = derived_ref.value_mut();
            derived.evicted = true;
            derived.evicted_whole_hash = pre_evict_hash;
            derived.cached_tsc_extract = None;
            derived.clear_raw_template_analysis();
            derived.cached_resolved_meta.clear();
            derived.cached_meta_payload = None;
            derived.cached_fallthrough = None;
        }
        // DependencyState (dependency_cache_db): preserve deps/aliases for
        // reload diffing — no mutation here.
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
        // may create an empty derived_raw_cache stub before the file is
        // loaded into the scheduler; in that case we must proceed to
        // submit a load request. The `evicted` flag lives on
        // DerivedRawState (D48 split).
        let evicted_flag = self
            .derived_raw_cache()
            .get(canonical_id)
            .map(|d| d.evicted)
            .unwrap_or(false);
        if !evicted_flag && self.scheduler.try_get_source(canonical_id).is_some() {
            return true;
        }

        use verter_scheduler::job::CompletionState;

        let (reload_from_workspace, pre_evict_hash) = self
            .derived_raw_cache()
            .get(canonical_id)
            .filter(|d| d.evicted)
            .map(|d| (true, d.evicted_whole_hash))
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
                file_language: None,
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
        // Every successful load — first-time additive OR reload — adds or
        // changes host state that `HostStoreView::build` snapshots BY
        // VALUE: a scheduler node + `whole_hashes` entry, the
        // `derived_raw_cache` known-miss tag the build folds into
        // `resolved_import_facts_known_miss_tags`, and the dependency/alias
        // maps. A `StoreViewManager`-cached base snapshot built BEFORE this
        // load does not track the newly-loaded canonical, so the token MUST
        // advance or the manager would hand a stale pre-load snapshot back
        // to the next caller (and the untracked-file `None => true`
        // optimistic-accept would fossilize against it).
        // `integrate_scheduler_snapshot` does NOT publish into
        // `FileArtifactStore`, so `artifact_generation` does not cover it.
        //
        // The dimension that advances depends on the load KIND:
        //
        // - FIRST-TIME additive load → `bump_load_generation()`. This is
        //   the compute's OWN work (a cold compute loads its dependencies),
        //   not an external content/project/env mutation, so it advances a
        //   DEDICATED `load_generation` dimension that the `StoreViewManager`
        //   reuse oracle includes (invalidating the cached base view) but
        //   that the publish fence's `externally_superseded_by` EXCLUDES —
        //   exactly like `artifact_generation`. Otherwise a scalar/batch
        //   cold compute that loads a dependency would self-fence its own
        //   result promotion. It also does NOT clear thread-local caches.
        //
        // - RELOAD (after evict) with CHANGED content → `bump_store_view_epoch()`.
        //   That is a genuine content change to an already-known file
        //   (an external-supersession class, like an upsert): older views'
        //   facts about that file are now stale, so it advances the epoch.
        //   `pre_evict_hash == None` (an evict with no prior scheduler
        //   snapshot) falls back to the conservative epoch bump.
        //
        // - RELOAD (after evict) with BYTE-IDENTICAL content →
        //   `bump_load_generation()`. The content is unchanged (R1: no
        //   epoch bump, the warm type-context cache survives the
        //   load→evict→ensure_loaded cycle), BUT the evict→present
        //   VISIBILITY transition IS validator-visible: `evict()` bumped
        //   the epoch and the canonical went invisible, so a concurrent
        //   `resolver_store_view()` built DURING the evict window caches a
        //   base snapshot that does NOT track the file under the
        //   post-evict token. Without a token advance on the reload, that
        //   mid-evict snapshot's token still matches the live token after
        //   the file is restored, so the manager keeps handing back a view
        //   that omits the reloaded canonical. The additive
        //   `load_generation` dimension covers this: it invalidates the
        //   manager-cached base view (it is in the reuse oracle) WITHOUT
        //   counting as an external supersession (excluded from
        //   `externally_superseded_by`), consistent with the
        //   bump-on-genuine-transition rule — an evict→reload IS a
        //   transition even when the bytes match.
        //
        // The within-wave churn the first-time-load bump introduces is
        // bounded: the batch engine acquires its fixed snapshot AFTER its
        // prefetch wave completes (so the token is stable then) and the
        // cold store-view build singleflights, so a load burst rebuilds the
        // manager cache at most once per wave rather than per load.
        if loaded {
            if reload_from_workspace {
                let post_reload_hash = self
                    .scheduler
                    .try_get_source(canonical_id)
                    .map(|s| s.whole_hash);
                let content_changed = match (pre_evict_hash, post_reload_hash) {
                    // Byte-identical reload — content no-op.
                    (Some(pre), Some(post)) => pre != post,
                    // Unknown pre/post hash — conservative content-change.
                    _ => true,
                };
                if content_changed {
                    self.bump_store_view_epoch();
                } else {
                    // Byte-identical reload-after-evict: no content change,
                    // but the evict→present visibility transition must
                    // advance the additive load_generation so a snapshot
                    // built mid-evict is invalidated.
                    self.bump_load_generation();
                }
            } else {
                // First-time additive load — advances the dedicated
                // load-generation dimension (own-work, excluded from the
                // publish fence's external-supersession check).
                self.bump_load_generation();
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
}

//! `impl VerterHost` — lifecycle and workspace-bridge methods.
//!
//! Owns:
//! - workspace-bridge accessors (`set_workspace`, `provenance`,
//!   `provenance_snapshot`, `ws`, `bump_store_view_epoch`,
//!   `current_store_view_epoch`)
//! - import-resolution wrappers (`resolve_import_transient`,
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
#[cfg(feature = "semantic-observe")]
use crate::meta_provenance::MetaProvenanceSnapshot;
use crate::shared::{read_lock, write_lock};
use crate::VerterHost;
use crate::{
    meta_provenance::MetaProvenance,
    types::{HostMetricsSnapshot, HostRetentionSnapshot},
};

impl VerterHost {
    /// Read the two selected host work counters without enabling full capture.
    ///
    /// Available with `soak-support` or `semantic-observe`. Other optional
    /// provenance is absent from this typed snapshot, never measured zero.
    #[cfg(any(feature = "soak-support", feature = "semantic-observe"))]
    #[must_use]
    pub fn work_snapshot(&self) -> crate::HostWorkSnapshot {
        use std::sync::atomic::Ordering::Relaxed;
        crate::HostWorkSnapshot {
            resolver_node_cache_misses: self.provenance.resolver_node_cache_misses.load(Relaxed),
            component_meta_resolved_state_recomputes: self
                .provenance
                .component_meta_resolved_state_recomputes
                .load(Relaxed),
        }
    }

    /// Swap the workspace backing this host.
    ///
    /// The scheduler's `SourceLoader` shares the same `Arc<RwLock>`, so
    /// it automatically reads through the new workspace after this call.
    ///
    /// Attaches the host's workspace-scoped services to the new workspace
    /// through the same route construction used, so reverse-dep stem
    /// stripping keeps honouring the configured extension list and the
    /// resolution state keeps charging the one retention adapter across
    /// LSP/test workspace swaps.
    pub fn set_workspace(&self, workspace: Arc<dyn verter_workspace::WorkspaceAccess>) {
        self.workspace_services.attach(workspace.as_ref());
        let freshness_readers = workspace.freshness_readers();
        {
            // Artifacts published from here on own their freshness evidence
            // in the new workspace; the clear below releases the old leases.
            // The history is installed under the same write as the workspace
            // it belongs to, so overlapping swaps can never leave one
            // workspace live with another's history installed.
            let mut live = self.workspace.write();
            *live = workspace;
            self.project_type_store
                .indexed()
                .install_freshness_readers(freshness_readers);
        }
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

    /// Access provenance counters for component-meta observability. The
    /// Full counters require `semantic-observe`; narrow work fields also
    /// have storage/readers with `soak-support`.
    pub fn provenance(&self) -> &Arc<MetaProvenance> {
        &self.provenance
    }

    /// Snapshot provenance counters, including VFS counters from the active
    /// workspace and the scheduler's contention counters. Exists only under
    /// `semantic-observe`: a build without it keeps no provenance.
    #[cfg(feature = "semantic-observe")]
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

    /// The authoritative per-canonical content-transition generation for this
    /// host's live workspace.
    ///
    /// External consumers that order async work produced from host content must
    /// read the revision from this authority rather than from a separate resolver
    /// snapshot workspace. [`Self::notify_upsert`] and the other host lifecycle
    /// mutations advance this generation in lockstep with the content they expose.
    #[must_use]
    pub fn last_content_transition_generation(&self, canonical_id: &str) -> u64 {
        self.ws().last_content_transition_generation(canonical_id)
    }

    /// Record a content transition for `canonical_id` at the current generation
    /// WITHOUT a byte change — delegated to the same workspace authority as
    /// [`Self::last_content_transition_generation`]. Used by the carrier-sync
    /// admission gate's equal-key differing-artifact refusal: the conflict
    /// proves the artifact rail under-counted, so the refused source's rail is
    /// advanced and the requeued transaction mints a strictly-newer key.
    pub fn record_content_transition(&self, canonical_id: &str) {
        self.ws().record_content_transition(canonical_id);
    }

    /// Own `canonical_id`'s content-transition evidence in the live
    /// workspace for as long as the returned lease lives.
    ///
    /// A consumer that captures [`Self::last_content_transition_generation`]
    /// and later compares it for equality takes this lease FIRST: while it
    /// lives, retiring unrelated workspace history never moves the
    /// canonical's answer, so only a transition of the canonical itself
    /// reads as a change. `None` when the workspace keeps no history.
    #[must_use]
    pub fn lease_content_transition(
        &self,
        canonical_id: &str,
    ) -> Option<verter_workspace::CanonicalFreshnessLease> {
        self.ws()
            .freshness_readers()
            .map(|readers| readers.lease_canonical(canonical_id))
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

    /// Resolve an import for an explicitly transient query.
    ///
    /// The returned target may be ReturnOnly and therefore must never be passed
    /// to provider sync, artifact materialization, or a durable graph/cache sink.
    pub fn resolve_import_transient(
        &self,
        parent_canonical_id: &str,
        import_source: &str,
    ) -> Option<String> {
        self.resolve_import_outcome_in(
            None,
            parent_canonical_id,
            import_source,
            verter_session_query::resolution::ResolutionContext {
                phase: verter_session_query::resolution::ResolvePhase::CodegenBlocker,
                kind: verter_session_query::resolution::ResolveRequestKind::EsmImport,
            },
        )
        .into_transient_result()
        .map(|result| result.source_id)
    }

    /// Record one owner-edge resolution's semantic trace onto the host's
    /// test audit.
    ///
    /// The resolution-currency contract observes the owner-edge lifecycle
    /// through four events (`ExactWitnessValidation`,
    /// `OwnerEdgeRecomputed`, `OwnerEdgePublished`, `OwnerEdgeReused`)
    /// and requires the store-view build to emit NONE of them. That assertion only
    /// discriminates if the sink is reachable from the resolution
    /// chokepoints the build would use if it regressed to routing work,
    /// so this hangs on EVERY host resolution entry point — the
    /// explicitly transient query AND both persistent-state entries —
    /// not just the one the contract tests happen to call. A build that
    /// re-indexed or re-resolved an owner would drive
    /// `resolve_for_persistent_state` and light the sink up.
    ///
    /// The observer itself stays `cfg(test)`; the emission adds nothing
    /// to a production build.
    #[cfg(test)]
    fn observe_owner_edge_outcome(
        &self,
        parent_canonical_id: &str,
        import_source: &str,
        outcome: &verter_workspace::ResolutionOutcome,
    ) {
        for target in outcome.trace().rejected_exact_targets() {
            self.observe_exact_witness_validation(
                parent_canonical_id,
                import_source,
                target.as_deref(),
                false,
            );
        }
        if outcome.trace().published() {
            let target = outcome.result().map(|result| result.source_id.as_str());
            self.observe_owner_edge_recomputed(parent_canonical_id, import_source, target);
            self.observe_owner_edge_published(parent_canonical_id, import_source, target);
        } else if outcome.trace().reused() {
            self.observe_owner_edge_reused(
                parent_canonical_id,
                import_source,
                outcome.result().map(|result| result.source_id.as_str()),
            );
        }
    }

    /// Resolve an import through the VFS with full resolution context.
    /// Sole resolution path on all targets.
    pub(crate) fn resolve_via_vfs(
        &self,
        parent_canonical_id: &str,
        import_source: &str,
        ctx: verter_session_query::resolution::ResolutionContext,
    ) -> verter_workspace::ResolutionPublication<String> {
        self.resolve_for_persistent_state(parent_canonical_id, import_source, ctx)
            .map_result(|result| result.source_id)
    }

    /// Resolve at a boundary that may retain the target in session state.
    ///
    /// The explicitly transient public query above may return a useful
    /// ReturnOnly target. Artifact, route, and dependency producers use this
    /// sibling so only a transaction-admitted result can reach their stores.
    pub fn resolve_for_persistent_state(
        &self,
        parent_canonical_id: &str,
        import_source: &str,
        ctx: verter_session_query::resolution::ResolutionContext,
    ) -> verter_workspace::ResolutionPublication {
        self.resolve_for_persistent_state_in(None, parent_canonical_id, import_source, ctx)
    }

    /// [`Self::resolve_for_persistent_state`] against one resolution
    /// snapshot: the workspace's own view (`None`), or a request overlay's
    /// effective view. ONE entry, so the importer mapping, the witness and
    /// the admission contract cannot differ between the two.
    pub(crate) fn resolve_for_persistent_state_in(
        &self,
        overlay: Option<&verter_workspace::ResolutionOverlaySnapshot>,
        parent_canonical_id: &str,
        import_source: &str,
        ctx: verter_session_query::resolution::ResolutionContext,
    ) -> verter_workspace::ResolutionPublication {
        let publication = self
            .resolve_import_outcome_in(overlay, parent_canonical_id, import_source, ctx)
            .into_publication();
        crate::host_manage::import_route_witness::record_resolution_witness(&publication);
        crate::host_manage::import_route_witness::note_unretained_resolution(&publication);
        publication
    }

    fn resolve_import_outcome_in(
        &self,
        overlay: Option<&verter_workspace::ResolutionOverlaySnapshot>,
        parent_canonical_id: &str,
        import_source: &str,
        ctx: verter_session_query::resolution::ResolutionContext,
    ) -> verter_workspace::ResolutionOutcome {
        // Typeinfo scratch files inline the active request scope and therefore
        // resolve their synthetic imports in that real scope's project
        // context. The request context supplies the actual published-project
        // identity; absent that context the synthetic URI remains unowned and
        // Engine refuses publication.
        let importer =
            if crate::resolver_core::vue_default_synth::is_typeinfo_scratch(parent_canonical_id) {
                verter_type_engine::request_context::current_request_context()
                    .map(|request| Arc::clone(&request.canonical_id))
                    .unwrap_or_else(|| Arc::from(parent_canonical_id))
            } else {
                Arc::from(parent_canonical_id)
            };
        let workspace = self.workspace();
        let root = workspace.published_root();
        if root.as_ref().is_some_and(|root| {
            !root.snapshot.owners_for_file(&importer).iter().any(|id| {
                matches!(
                    root.snapshot.project(*id).payload,
                    verter_workspace::ProjectPayload::Configured { .. }
                )
            })
        }) {
            self.ingest_configured_programs();
        }
        let root = workspace.published_root();
        let reachable_owner = root.as_ref().and_then(|root| {
            let project = root
                .snapshot
                .project(self.project_for_canonical_in_snapshot(&importer, &root.snapshot)?);
            match &project.payload {
                verter_workspace::ProjectPayload::Configured { tsconfig_path, .. } => {
                    Some(verter_session_query::resolution::ProjectOwnership {
                        project_root: project.root.as_str().to_owned(),
                        tsconfig_path: Some(tsconfig_path.as_str().to_owned()),
                    })
                }
                _ => None,
            }
        });
        let outcome = match (reachable_owner, overlay) {
            (Some(owner), overlay) => workspace.resolve_import_from_project_outcome(
                &owner,
                overlay,
                importer.as_ref(),
                import_source,
                ctx,
            ),
            (None, None) => self
                .ws()
                .resolve_import_outcome(importer.as_ref(), import_source, ctx),
            (None, Some(overlay)) => self.ws().resolve_import_outcome_with_overlay(
                overlay,
                importer.as_ref(),
                import_source,
                ctx,
            ),
        };
        #[cfg(test)]
        self.observe_owner_edge_outcome(importer.as_ref(), import_source, &outcome);
        outcome
    }

    /// The session view's resolution-visible overlay: its upserted
    /// sources plus its tombstones.
    ///
    /// Load-bearing since the parse artifact stopped baking resolved
    /// targets. A session's overlay-only dependency used to reach base
    /// readers implicitly — the overlay materialiser resolved it and the
    /// baked canonical travelled with the artifact — so a base-host
    /// resolve of the same specifier was never attempted. Now every
    /// consumer resolves on demand, and a session-bound consumer must
    /// resolve through ITS OWN overlay or an overlay-only target
    /// disappears.
    pub(crate) fn resolution_overlay_snapshot(
        &self,
        view: &dyn crate::session_view::SessionView,
    ) -> verter_workspace::ResolutionOverlaySnapshot {
        let upserts = view
            .overlay_canonicals()
            .into_iter()
            .filter_map(|canonical| view.source(&canonical).map(|source| (canonical, source)));
        verter_workspace::ResolutionOverlaySnapshot::new_under(
            view.resolution_authority().cloned(),
            upserts,
            view.tombstoned_canonicals(),
        )
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
    pub(crate) fn integrate_scheduler_snapshot(
        &self,
        canonical_id: &str,
        fence: &crate::host_load::HostLoadFence,
    ) -> Result<bool, crate::HostLoadError> {
        use crate::host_executor::HostSourceData;

        let snap = match self.scheduler.try_get_source(canonical_id) {
            Some(s) => s,
            None => return Err(crate::HostLoadError::Superseded),
        };
        let Some(hd) = snap.downcast_data::<HostSourceData>() else {
            return Ok(false);
        };
        #[cfg(test)]
        integrate_test_hooks::after_snapshot_capture();
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

        #[cfg(test)]
        self.test_force.host_load_publish_seam_hook.fire_once();

        // Source replacement and integration share one publication turn. The
        // captured source may already be superseded; refuse before any writes.
        let _admission = self.block_content.admission_fence.lock();
        let eviction_revision = self
            .derived_raw_cache()
            .get(canonical_id)
            .map_or(0, |state| state.eviction_revision());
        fence.check(self, canonical_id, eviction_revision)?;

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
        let old_aliases = self
            .dependency_cache()
            .get(canonical_id)
            .map(|dep| dep.aliases.clone())
            .unwrap_or_default();
        // PRESERVE derived.import_routes. Bundler may have set them via
        // set_import_dependencies before source was loaded. Cloning
        // here so we can re-apply to workspace below without holding the
        // entry lock.
        let preserved_routes = self
            .derived_raw_cache()
            .get(canonical_id)
            .map(|derived| derived.import_routes.clone())
            .unwrap_or_default();

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
        // A dependency row is the integrated-load marker. Publish it only once
        // aliases and workspace edges are complete, and then expose reloads.
        {
            let mut dep = self
                .dependency_cache()
                .entry(canonical_id.to_string())
                .or_default();
            dep.aliases = aliases;
            dep.dependencies = deps;
            dep.generation = snap.generation;
        }
        self.derived_raw_entry_or_default(canonical_id.to_string())
            .evicted = false;
        Ok(true)
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
        let session_node = crate::compile_output_node::CompileOutputNodeFactValidatedSession::new();
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
    ///
    /// Close releases every owner the host's caches hold, then gives back
    /// the storage those owners leave behind: retired artifact versions no
    /// live root reaches, the semantic store's destroyed-node sidecars,
    /// drained dependency-rail and dedup tables, and identity strings only
    /// the pool still references. Results a caller still holds stay valid
    /// and charged until the caller drops them; nothing is revoked, and
    /// another host's data and charges are untouched.
    ///
    /// Close does NOT wait for the retained-parse releases it issues: a
    /// lease drop only records its release for the owning worker. A caller
    /// that needs those releases applied — a retention measurement, a
    /// test comparing the account to its pre-workload baseline — waits on
    /// [`Self::release_completion`] after the close and after dropping the
    /// results it held.
    pub fn close(&self) {
        let _block_content_fence = self.block_content.admission_fence.lock();
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
        write_lock(&self.block_content.state).close_all();

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
        // The flow-slice substrate (graph bundles, slice plans, lowered
        // slices) is content-addressed and sits beside the artifact store:
        // neither the cascade above nor a scheduler reset reaches it, and
        // `evict_canonical` runs only on upsert/remove. Its bundles keep
        // their pinned graph arenas and skeleton name indexes charged, so
        // a closed host clears it here.
        self.project_type_store.flow_slice().clear_all();
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
        // Every cache owner above is released, so the store's hold on the
        // primitive vocabulary — the one set of permanent nodes, interned by
        // the first workload that needed them — goes too: an unheld
        // vocabulary node is destroyed with its chunk and sidecars, one a
        // reader still holds lives until that reader drops.
        let _ = self
            .project_type_store
            .semantic_graph()
            .release_vocabulary();
        // Every owner above is released; give back what only they reached.
        self.reclaim_released_storage();
        self.bump_store_view_epoch();
    }

    /// Give back the storage released owners leave behind, without touching
    /// anything a live owner or reader still reaches.
    ///
    /// Retired artifact versions go first, once nothing roots them: their
    /// decl memos hold parse leases and their prepared declarations pooled
    /// identities. Then the semantic store's destroyed-node sidecars,
    /// drained dependency-rail and dedup tables, and the identity strings
    /// only the pool still references. Each step frees only storage whose
    /// last referent is gone, so it is idempotent and safe on a live host.
    fn reclaim_released_storage(&self) {
        let _ = self.project_type_store.indexed().reclaim_retired_versions();
        let _ = self
            .project_type_store
            .semantic_graph()
            .release_drained_storage();
        let _ = self.project_type_store.identity_interner().shed_pool_only();
    }

    /// Open a completion boundary over the releases this host has issued
    /// so far.
    ///
    /// Dropping a parse lease (an artifact version, a decl memo) never
    /// blocks: the release is recorded for the decl-lowering worker that
    /// owns the snapshot, which applies it — freeing the parse and its pin
    /// — before its next job. Opening the boundary reclaims retired
    /// artifact versions nothing roots any more (so their leases are
    /// covered) and marks every worker shard; [`HostReleaseCompletion::wait`]
    /// returns once every shard has applied every release recorded before
    /// the boundary opened, then gives back the pool and sidecar storage
    /// those releases and any reader dropped before the boundary left
    /// behind. Releases issued after the boundary opened (a later close, a
    /// reader dropped concurrently) are covered only by a later boundary;
    /// an empty or repeated boundary completes at once.
    ///
    /// Close does not wait on its own releases. A caller measuring a closed
    /// host closes it, drops the results it held, then waits on this
    /// boundary before reading [`Self::retention_snapshot`]; readers dropped
    /// after the close are covered the same way. Never call it from work
    /// running on a decl-lowering worker.
    pub fn release_completion(&self) -> HostReleaseCompletion<'_> {
        let _ = self.project_type_store.indexed().reclaim_retired_versions();
        HostReleaseCompletion {
            host: self,
            parse: self.decl_lowering.release_completion(),
        }
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
        projects: Vec<verter_session_query::resolution::IdeProjectConfig>,
    ) {
        self.ws().configure_resolver(projects);
        // Project-config change drops resolution-derived state:
        // `import_routes` (DerivedRawState) and `dependencies`
        // (DependencyState). Symmetric with
        // [`Self::finish_upsert_post_commit`], which clears the same
        // fields when owner source content advances.
        for mut entry in self.derived_raw_cache().iter_mut() {
            entry.import_routes.clear();
        }
        for mut entry in self.dependency_cache().iter_mut() {
            entry.dependencies.clear();
        }
        self.resolver.reset_all();
        self.semantic_invalidate_all();
        // `configure_projects` changes project configuration and identity.
        // STAMP-ONLY project-shape bump — retained payloads
        // (`IndexedReady`, the per-canonical compile/derived/dependency
        // entries beyond the explicit route-field loops above) survive;
        // stale entries miss BY VALIDATION (the project-stamp read gate
        // rejects values from the previous project shape;
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
        self.ingest_configured_programs();
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
        verter_session_query::probe_scope!(NOTIFY_UPSERT);
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

    /// Host wrapper for [`WorkspaceAccess::set_exact_resolutions`] with
    /// owner-scoped route-state repair.
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
        // The workspace transaction publishes exact-resolution facts before
        // returning. The store-view epoch advanced below fences request-local
        // readers across the host-side repair; project generation is reserved
        // for project-shape resets and therefore remains unchanged here.
        let result = self.ws().set_exact_resolutions(canonical, resolutions);
        if !result.changed {
            // Value-identical re-push (the duplicate-key-safe engine
            // gate): nothing moved, so the whole invalidation cascade —
            // is skipped.
            return;
        }
        // Owner-scoped route-mirror repair: the workspace exacts for
        // THIS owner just changed, so its caller-supplied route table is
        // stale. Other canonicals' tables are untouched — their routes
        // did not move.
        if let Some(mut entry) = self.derived_raw_cache().get_mut(canonical) {
            entry.import_routes.clear();
        }
        self.resolver.runtime.invalidate_canonical(canonical);
        // Route mutation, content unchanged: drain the canonical's
        // derived layers but RETAIN its content-addressed `IndexedReady`
        // payload — exact-resolution facts make the next semantic read
        // observe the current route surface without a re-parse.
        self.project_type_store
            .evict_canonical_for_route_mutation(canonical);
        // R4 producer: rebuild parse-domain facts for the reloaded
        // canonical so the next resolver pass sees the new content.
        self.register_facts_for_new_content(canonical);
        self.bump_store_view_epoch();
    }

    /// Mint a workload identity bound to the aggregate account this host's
    /// semantic stores charge — the same account [`Self::retention_snapshot`]
    /// reads. ACTIVE reservations made through it
    /// (`SemanticRetentionAccount::reserve_as`, reached with
    /// `self.project_type_store().retention_account()`) are the workload an
    /// ACTIVE-occupancy window measures.
    #[cfg(any(test, feature = "test-support", feature = "soak-support"))]
    #[must_use]
    pub fn mint_active_occupancy_owner(
        &self,
    ) -> verter_session_query::retention::active_window::ActiveOwner {
        self.project_type_store
            .retention_account()
            .mint_active_owner()
    }

    /// Open an ACTIVE-occupancy measurement window for `owner` on the
    /// aggregate account this host's semantic stores charge.
    ///
    /// The window reports the most ACTIVE bytes `owner` held at once between
    /// its opening and its finish; other identified owners are excluded and
    /// unknown or mixed provenance rejects the witness (see
    /// [`verter_session_query::retention::active_window`]). Optional
    /// test/tooling state: present only under `test`, `test-support` or the
    /// default-off `soak-support` feature
    /// ([`verter_session_query::retention::ACTIVE_OCCUPANCY_WINDOW_AVAILABLE`]).
    #[cfg(any(test, feature = "test-support", feature = "soak-support"))]
    #[must_use = "a window measures only until it is finished or dropped"]
    pub fn open_active_occupancy_window(
        &self,
        owner: &verter_session_query::retention::active_window::ActiveOwner,
    ) -> verter_session_query::retention::active_window::ActiveOccupancyWindow {
        self.project_type_store
            .retention_account()
            .open_active_window(owner)
    }

    /// Bytes the retained parse snapshots pin against the account. An
    /// observation that retains nothing.
    #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
    #[must_use]
    pub fn parse_pin_bytes_for_tests(&self) -> usize {
        self.decl_lowering.snapshot_pin_bytes_for_tests()
    }

    /// What the host retains right now, as object counts and aggregate
    /// bytes ([`HostRetentionSnapshot`]).
    ///
    /// Read live from the owning structures — the indexed artifact store
    /// (live versions, retired-but-retained versions, captured roots), the
    /// decl-lowering service (parse-snapshot leases) and the aggregate
    /// semantic retention account (charged bytes and refusals). This is
    /// the object-lifetime instrument a long editing session is measured
    /// with: a session whose retained set is bounded shows every figure
    /// returning to a plateau after quiescence, while a leak shows one of
    /// them climbing with the number of document versions ever published.
    pub fn retention_snapshot(&self) -> HostRetentionSnapshot {
        let indexed = self.project_type_store.indexed();
        let account = self.project_type_store.retention_account().snapshot();
        let workspace = self.ws().resource_snapshot();
        let (capture_summaries, capture_summary_files) =
            verter_session_query::function_program::CaptureSummaryCounts::resident();
        HostRetentionSnapshot {
            capture_summaries,
            capture_summary_files,
            skeleton_name_indexes:
                verter_session_query::flow::skeleton::SkeletonNameIndexOccupancy::resident(),
            overlay_resolution_slots: workspace.overlay_resolution_slots,
            overlay_value_versions: workspace.overlay_value_versions,
            fallthrough_nodes: self.resolver.runtime.fallthrough.retained_node_count(),
            live_artifacts: indexed.live_artifact_count(),
            retained_retired_versions: indexed.retained_retired_version_count(),
            live_roots: indexed.live_root_count(),
            snapshot_leases: self.decl_lowering.live_lease_count(),
            carrier_candidates: self
                .carrier_publication
                .publication_store
                .retained_candidate_count(),
            publication_lanes: self
                .carrier_publication
                .publication_store
                .retained_lane_count(),
            semantic_graph_storage_bytes: self
                .project_type_store
                .semantic_graph()
                .graph_storage_charge_bytes(),
            semantic_bookkeeping_storage_bytes: self
                .project_type_store
                .semantic_graph()
                .bookkeeping_storage_charge_bytes(),
            semantic_nodes: self.project_type_store.semantic_graph().node_count(),
            semantic_node_ownership: self
                .project_type_store
                .semantic_graph()
                .node_ownership_counts(),
            semantic_node_slots: self
                .project_type_store
                .semantic_graph()
                .node_storage_slots(),
            semantic_memo_entries: self.project_type_store.semantic_graph().memo_entry_count(),
            semantic_memo_candidates: self
                .project_type_store
                .semantic_graph()
                .memo_candidate_count(),
            unresolved_reach: self
                .project_type_store
                .semantic_graph()
                .unresolved_reach_count(),
            union_views: self.project_type_store.semantic_graph().union_view_count(),
            stable_key_classes: self
                .project_type_store
                .semantic_graph()
                .stable_key_class_count(),
            identity_indexes:
                verter_type_engine::semantic_query_memo::IdentityIndexSnapshot::capture(),
            pending_destroyed_nodes: self
                .project_type_store
                .semantic_graph()
                .pending_destroyed_nodes(),
            resolved_import_facts: self
                .project_type_store
                .resolved_import_facts()
                .entry_count(),
            component_meta_states: self.resolver.runtime.component_meta_state_count(),
            identity_pool_entries: self.project_type_store.identity_interner().len(),
            #[cfg(any(test, feature = "test-support"))]
            identity_pool_bytes: self.project_type_store.identity_interner().retained_bytes(),
            registered_sources: self.carrier_publication.source_authority.len(),
            signature_records: self
                .project_type_store
                .semantic_graph()
                .signature_store()
                .interned_len(),
            signature_record_cap: self
                .project_type_store
                .semantic_graph()
                .signature_store()
                .record_cap(),
            closes: self.project_type_store.close_stats(),
            shape_cache_entries: self.project_type_store.shape_cache_db().live_count(),
            flow_graphs: self.project_type_store.flow_slice().graphs_entry_count(),
            flow_hash_entries: self
                .project_type_store
                .flow_slice()
                .hash_node()
                .entry_count(),
            flow_lowered_entries: self
                .project_type_store
                .flow_slice()
                .lowered_node()
                .entry_count(),
            mapper_fingerprints: self
                .project_type_store
                .mapper_binder_registry()
                .fingerprint_count(),
            framework_surface_entries: self.framework_registry().surface_entry_count(),
            semantic_memo_families: self
                .project_type_store
                .semantic_graph()
                .memo_entry_counts_by_family()
                .into_iter()
                .map(|(family, count)| (family.to_string(), count))
                .collect(),
            active_bytes: account.active_bytes,
            retained_bytes: account.retained_bytes,
            pinned_bytes: account.pinned_bytes,
            peak_total_bytes: account.peak_total_bytes,
            refusals_pressure: account.refusals_pressure,
            refusals_oversized: account.refusals_oversized,
            refusals_active: account.refusals_active,
        }
    }

    /// Ask the indexed artifact store to physically reclaim every retired
    /// version no live root can still reach, and return how many it freed.
    ///
    /// Retirement is logical: a superseded artifact version leaves the
    /// current membership but keeps its payload — and with it the parse
    /// snapshot lease and the decl memo the payload owns — until a sweep
    /// finds no captured root that still addresses it. The store runs that
    /// sweep on its own amortised schedule (every few dozen retirements),
    /// which bounds a pure edit loop but lets a document LIFECYCLE hold
    /// dozens of dead versions between sweeps: an open → edit → close cycle
    /// retires two versions of the document (the on-disk one when the edit
    /// publishes, the edited one when the reload republishes disk), and
    /// nothing about a close asks for the sweep. A document close is the
    /// moment its edited versions become unreachable, so the close
    /// lifecycle requests the sweep here. The request carries no
    /// reachability judgement — the store keeps every version a live root
    /// still sees.
    pub fn reclaim_retired_artifacts(&self) -> usize {
        self.project_type_store.indexed().reclaim_retired_versions()
    }

    /// Snapshot of host-level metrics counters.
    ///
    /// Every field reads zero unless `HostConfig::metrics_enabled` was
    /// `true` at the time the underlying activity happened — the
    /// counters are only written when the flag is on.
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
        let _admission = self.block_content.admission_fence.lock();
        self.ws().notify_close(canonical_id);
        self.evict_loaded_content(canonical_id);
    }

    /// Discard host content for a workspace-only replacement while preserving
    /// the workspace's active overlay and snapshot authority.
    pub(crate) fn evict_loaded_content(&self, canonical_id: &str) {
        let _admission = self.block_content.admission_fence.lock();
        self.semantic_db().invalidate(canonical_id);
        // Evict what the semantic substrate cached for this document: its
        // memo entries, union views, hash-cons memos, signature-token roots
        // and shape-cache entries. The editor buffer is gone and the next
        // reader re-lowers the file from disk, so no lookup reaches the
        // closed content again; its nodes are destroyed as soon as no caller
        // holds them, and a caller still holding one keeps reading it. This
        // is NOT `evict_canonical`: the file's artifacts stay for the reload.
        //
        // NOT for a synthesised typeinfo scratch (`verter://typeinfo/…`):
        // `evaluate_type_expression` evicts its scratch at the end of every
        // non-cacheable evaluation under the documented contract that the
        // eviction reclaims compile / derived state but PRESERVES the
        // scratch's semantic-graph memo — the cross-mode materialized-point
        // satisfaction a repeat request relies on.
        const TYPEINFO_SCRATCH_URI_PREFIX: &str = "verter://typeinfo/";
        if !canonical_id.starts_with(TYPEINFO_SCRATCH_URI_PREFIX) {
            let report = self.project_type_store.close_canonical(canonical_id);
            tracing::debug!(
                canonical_id,
                memo_entries_evicted = report.memo_entries_evicted,
                "evict released the document's semantic caches"
            );
        }
        // What the document's own content keyed: its resolved-import facts
        // (one key per content hash), the resolver's component-meta states
        // (one key per view fingerprint), its overlay source registrations
        // (one per view fingerprint) and its fallthrough nodes (keyed by the
        // component; an edit adds a candidate under the same key, bounded per
        // key). The close drops what is left.
        let import_facts_released = self
            .project_type_store
            .resolved_import_facts()
            .release_canonical(canonical_id);
        let meta_states_released = self
            .resolver
            .runtime
            .release_component_meta_states(canonical_id);
        let fallthrough_nodes_released = self
            .resolver
            .runtime
            .fallthrough
            .release_owner(canonical_id);
        let overlay_sources_retracted = self
            .carrier_publication
            .source_authority
            .retain_incarnations(
                &verter_language::registered_source_authority::CanonicalFileId::new(canonical_id),
                |registered| {
                    registered.get()
                        & crate::host_manage::overlay_materialize::OVERLAY_INCARNATION_BIT
                        == 0
                },
            );
        tracing::debug!(
            canonical_id,
            import_facts_released,
            meta_states_released,
            fallthrough_nodes_released,
            overlay_sources_retracted,
            "evict released the document's content-keyed caches"
        );

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
            let session_node =
                crate::compile_output_node::CompileOutputNodeFactValidatedSession::new();
            session_node.clear_compile_outputs_for_file(&mut profile);
            profile.latest_diagnostics.clear();
            // Clearing `latest_diagnostics` is a diagnostics change that moves
            // no document version — precisely what the epoch signals. Advance
            // it so a consumer holding a result derived from the pre-evict
            // diagnostics cannot revalidate it. Reopening the file onto the
            // byte-identical-content upsert fast path does not clear the
            // evicted flag, so without this the epoch would sit unmoved across
            // an evict/reopen pair.
            profile.diagnostics_generation += 1;
        }
        // DerivedRawState (derived_raw_cache_db): set the evicted flag
        // and capture pre-evict whole_hash; clear all source-derived
        // caches.
        {
            let mut derived_ref = self.derived_raw_entry_or_default(canonical_id.to_string());
            let derived = derived_ref.value_mut();
            derived.mark_evicted(pre_evict_hash);
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
        let scope = self.scheduler.enter_execution_scope();
        let mut load = self.begin_load(
            canonical_id,
            scope.task().clone(),
            self.cooperative_drive.cancellation().clone(),
        );
        let awaiting_scheduler = load.awaiting_scheduler();
        let wait_start = verter_type_engine::instant::Instant::now();
        let outcome = self
            .cooperative_drive
            .drive_load(&self.scheduler, &mut load);
        if awaiting_scheduler {
            self.provenance.ensure_loaded_wait_ns.fetch_add(
                wait_start.elapsed().as_nanos() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        outcome
            .and_then(|commit| commit.apply(self))
            .unwrap_or(false)
    }
    /// Resolve an alias to its canonical ID, or normalize the ID if no
    /// alias exists.
    ///
    /// Public because this is the SINGLE identity every host route keys
    /// on, and a caller that has to correlate its own inputs against a
    /// route's answers — the native batch binding pairing its output slots
    /// against `compile_request_many`'s entries — must ask the same
    /// question the host asks rather than re-deriving a normalization of
    /// its own.
    ///
    /// The answer is a fixpoint today because [`Self::upsert`] inserts
    /// each canonical id into its OWN alias set, so a registered canonical
    /// resolves to itself: canonicalizing at a boundary and passing the
    /// result in is the same demand as passing the raw id in. A caller
    /// that depends on more than one resolution agreeing should still fail
    /// SOFTLY if they ever disagree — the native batch binding fails the
    /// affected entry rather than the call — because nothing in the type
    /// system holds that fixpoint.
    pub fn resolve_alias_or_canonical(&self, id: &str) -> String {
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

/// A host release boundary ([`VerterHost::release_completion`]).
#[must_use = "a release boundary proves nothing until it is awaited"]
pub struct HostReleaseCompletion<'host> {
    host: &'host VerterHost,
    parse: verter_semantic_source::decl_lowering::ParseReleaseCompletion,
}

impl HostReleaseCompletion<'_> {
    /// Block until every decl-lowering worker has applied the parse
    /// releases recorded before the boundary opened, then give back the
    /// storage released owners left behind.
    ///
    /// # Errors
    ///
    /// [`crate::ParseReleaseError::WorkerStopped`] when a worker stopped
    /// before applying its releases; nothing is reclaimed then.
    pub fn wait(self) -> Result<(), crate::ParseReleaseError> {
        self.parse.wait()?;
        self.host.reclaim_released_storage();
        Ok(())
    }
}

/// A test seam inside snapshot integration: an action the calling thread runs
/// once, after the snapshot is captured and before anything is written.
#[cfg(test)]
pub(crate) mod integrate_test_hooks {
    use std::cell::RefCell;

    thread_local! {
        static AFTER_SNAPSHOT_CAPTURE: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    }

    /// Run `action` at this thread's next snapshot capture.
    pub(crate) fn arm_after_snapshot_capture(action: impl FnOnce() + 'static) {
        AFTER_SNAPSHOT_CAPTURE.with(|slot| *slot.borrow_mut() = Some(Box::new(action)));
    }

    pub(super) fn after_snapshot_capture() {
        if let Some(action) = AFTER_SNAPSHOT_CAPTURE.with(|slot| slot.borrow_mut().take()) {
            action();
        }
    }
}

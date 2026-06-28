//! The tsserver [`EngineBackend`] — the publish authority that mirrors carrier
//! snapshots onto the on-disk content-addressed store the Node
//! `@verter/typescript-plugin` reads synchronously (§2.2).
//!
//! This backend realises the publish half of the contract for the tsserver engine:
//! `ensure_project` registers a project's per-workspace store, and
//! `publish_snapshot` runs the two-phase publish through the [`CarrierPublishStore`].
//! The plugin (a SEPARATE process) reads the store; this backend never calls the
//! plugin.
//!
//! ## `query` / `diagnostics` are not yet wired to the live transport
//!
//! Answering a feature query / diagnostics against the live tsserver resolves a
//! ready companion's blob path, opens it on the user's tsserver session, issues the
//! request, and maps the result back through the carrier source map. That live
//! transport orchestration is a distinct concern from the publish store this module
//! owns. Per the project's Stub Prevention rule, an always-empty / always-`NoResult`
//! return that MASQUERADES as working is forbidden, so `query` / `diagnostics`
//! `unimplemented!()` (fail LOUDLY) until the live transport is wired, rather than
//! silently returning a degraded result.

use std::sync::Arc;

use verter_session::external_ts::{
    BoundProject, Diagnostics, DiagnosticsOutcome, EngineBackend, EngineCapabilities, EngineError,
    EnsureProject, PublishSnapshot, Query, QueryOutcome,
};

use crate::external_ts::carrier_publish_store::{
    carrier_store_dir_for, default_carrier_store_host_version, CarrierPublishStore, OwnedSetScope,
    OwnedSource, PublishBatch,
};
use crate::external_ts::membership_ledger::{MembershipLedger, ProjectUri};

/// A per-workspace publish store plus the project URIs ensured under it. The
/// project set lets `publish_snapshot` (which receives only the project URI on the
/// witness, not the workspace root) recover the owning workspace store.
#[derive(Debug)]
struct WorkspaceStore {
    workspace_root: Arc<str>,
    store: Arc<CarrierPublishStore>,
    /// Project (tsconfig) URIs ensured under this workspace root.
    projects: parking_lot::Mutex<std::collections::HashSet<Arc<str>>>,
}

/// The tsserver engine backend: the Rust carrier-publish authority.
///
/// Holds one [`CarrierPublishStore`] per workspace, keyed by workspace root, so a
/// multi-root session publishes each root's carriers into its own store dir. The
/// store is the SOLE on-disk mutation surface; the backend is otherwise stateless.
#[derive(Debug)]
pub struct TsserverEngineBackend {
    /// The negotiated TypeScript host version — the store's per-host-version dir
    /// segment (so an upgrade re-publishes into a fresh tree, never reusing stale
    /// blobs across host versions).
    host_version: Arc<str>,
    /// Per-workspace-root publish stores, keyed by workspace root.
    stores: dashmap::DashMap<Arc<str>, Arc<WorkspaceStore>>,
    /// The negotiated capabilities reported for every bound project. The shipped
    /// tsserver plugin model exposes NO static module-resolution-map endpoint (the
    /// `.x`→carrier redirect rides the host FS proxy) and the plugin read path is
    /// synchronous (no async/cancellable query lane), so both flags are `false`.
    capabilities: EngineCapabilities,
    /// The source-indexed active-membership ledger — INTERNAL transition bookkeeping
    /// ONLY. It is the reconciler's own state for a membership transition (its
    /// `current_session` / `record_snapshot` reads + the commit post-verification);
    /// the reconciler's `commit` is its SOLE writer. It has ZERO production readers of
    /// the advertised / serve set: live `getExternalFiles` is served CROSS-PROCESS
    /// from the on-disk STORE `ready_files` (the Node plugin's `index.ts` →
    /// `CarrierStoreReader.readyIdeCompanions` → `carrierStore.ts` reading the
    /// `carrier_publish_store` manifest), NOT this in-process ledger. Held here so the
    /// reconciler and the store share one ledger per session.
    membership_ledger: Arc<MembershipLedger>,
    /// Test-only-armed fault-injection seam for the owner-move stale-owner prune
    /// ([`Self::retract_source_everywhere_except`]). ALWAYS present (one byte), but
    /// ONLY ever armed by the `#[cfg(test)]` [`Self::arm_prune_except_failure`];
    /// production never sets it, so the prune behaviour is unchanged there. When
    /// armed, the next prune returns `Err` BEFORE any store mutation, exercising the
    /// `publish_owned_resolved` compensation/rollback path so a partial owner-move never leaves
    /// the cross-process `ready_files` stale or duplicated.
    fail_next_prune_except: std::sync::atomic::AtomicBool,
}

impl TsserverEngineBackend {
    /// Build the backend for a negotiated `host_version`.
    #[must_use]
    pub fn new(host_version: impl Into<Arc<str>>) -> Self {
        let host_version = host_version.into();
        Self {
            capabilities: EngineCapabilities {
                static_module_resolution_map: false,
                async_cancellable_queries: false,
                reported_version: Some(Arc::clone(&host_version)),
            },
            host_version,
            stores: dashmap::DashMap::new(),
            membership_ledger: Arc::new(MembershipLedger::with_initial_session()),
            fail_next_prune_except: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// The source-indexed active-membership ledger this backend owns — INTERNAL
    /// transition bookkeeping the reconciler shares (cheap `Arc`) with the store. It
    /// is NOT a production serve-set reader: live `getExternalFiles` is served from the
    /// on-disk store `ready_files`, not this ledger.
    #[must_use]
    pub fn membership_ledger(&self) -> &Arc<MembershipLedger> {
        &self.membership_ledger
    }

    /// The carrier-companion provider paths recorded under `project` in the in-process
    /// ledger — a TEST-SIDE / diagnostic view of the reconciler's membership
    /// bookkeeping, NOT the production `getExternalFiles` path. Live `getExternalFiles`
    /// is served CROSS-PROCESS from the on-disk store `ready_files` (the Node plugin
    /// reads the `carrier_publish_store` manifest); this in-process view is the value
    /// the reconciler keeps consistent with that store, used by the production-path
    /// tests to assert membership without the live plugin. NO production serve path
    /// reads it (pinned by the `ledger_is_off_the_serve_path` architecture guard).
    #[must_use]
    pub fn external_files_for_project(&self, project: &str) -> Vec<String> {
        self.membership_ledger
            .advertised_provider_paths_under(&ProjectUri::from(project))
            .into_iter()
            .map(|p| p.to_string())
            .collect()
    }

    /// Build the backend for the LSP-default carrier-store host version (the
    /// Verter LSP package version). This is the live-path constructor: the same
    /// host-version segment the tsserver spawn uses to derive the store dir, so
    /// the plugin reads exactly the store this backend writes.
    #[must_use]
    pub fn with_default_host_version() -> Self {
        Self::new(default_carrier_store_host_version())
    }

    /// The per-workspace carrier-store dir this backend publishes a workspace's
    /// carriers into — the SAME dir the tsserver spawn must deliver to the plugin
    /// via `VERTER_CARRIER_STORE_DIR`. The publish path and the spawn both go
    /// through this one derivation, so they never disagree on the location.
    #[must_use]
    pub fn store_dir_for(&self, workspace_root: &str) -> std::path::PathBuf {
        carrier_store_dir_for(&self.host_version, workspace_root)
    }

    /// The per-workspace store for a root, creating it on first use. `entry`
    /// collapses a concurrent first-use race onto one store.
    fn workspace_store(&self, workspace_root: &str) -> Arc<WorkspaceStore> {
        if let Some(ws) = self.stores.get(workspace_root) {
            return Arc::clone(ws.value());
        }
        let root: Arc<str> = Arc::from(workspace_root);
        let ws = Arc::new(WorkspaceStore {
            workspace_root: Arc::clone(&root),
            store: Arc::new(CarrierPublishStore::open(
                self.host_version.to_string(),
                workspace_root,
            )),
            projects: parking_lot::Mutex::new(std::collections::HashSet::new()),
        });
        self.stores.entry(root).or_insert(ws).value().clone()
    }

    /// The workspace store the bound `project` was ensured under: the one whose
    /// ensured-project set contains the project URI. `None` when the project was
    /// never ensured (fail closed — a publish for an un-ensured project is refused).
    fn workspace_store_for_project(&self, project: &BoundProject) -> Option<Arc<WorkspaceStore>> {
        for entry in self.stores.iter() {
            if entry.value().projects.lock().contains(project.project()) {
                return Some(Arc::clone(entry.value()));
            }
        }
        None
    }

    /// Publish ONLY a project's owned carrier set (no content) — the ownership-
    /// resolved registration the eager-index path uses before any carrier content
    /// exists. Requires the witness. The owned set enters `owned_sources`; nothing
    /// is advertised through `ready_files` until its content is published.
    pub fn register_owned(
        &self,
        project: &BoundProject,
        owned: Vec<OwnedSource>,
    ) -> Result<u64, EngineError> {
        let ws = self
            .workspace_store_for_project(project)
            .ok_or_else(|| ensure_failed("register_owned for an un-ensured project"))?;
        let empty = PublishSnapshot {
            project: Arc::from(project.project()),
            files: Vec::new(),
            resolution_map_version: 0,
            fs_generation: 0,
        };
        // The owned set passed here is the project's AUTHORITATIVE full carrier set
        // (the ownership-resolution registration) — so it rewrites `owned_sources`
        // and prunes any `ready_files` it no longer admits.
        let batch = PublishBatch::from_snapshot(
            ws.workspace_root.to_string(),
            empty,
            Some(owned),
            OwnedSetScope::ProjectAuthoritative,
        );
        ws.store
            .publish_batch(&batch)
            .map_err(|e| EngineError::EnsureFailed(Arc::from(e.to_string().as_str())))
    }

    /// Retract a SOURCE carrier from its project — the delete / no-owner /
    /// now-ambiguous transition. Removes the source's owned rows + advertised
    /// companions from the project's manifest entry so `getExternalFiles` stops
    /// serving it. Requires the witness (the project must be ensured under a
    /// workspace store). A no-op for a source the project never owned.
    pub fn retract_source(
        &self,
        project: &BoundProject,
        source_uri: &str,
    ) -> Result<u64, EngineError> {
        let ws = self
            .workspace_store_for_project(project)
            .ok_or_else(|| ensure_failed("retract_source for an un-ensured project"))?;
        ws.store
            .retract_sources(project.project(), &[source_uri])
            .map_err(|e| EngineError::Unavailable(Arc::from(e.to_string().as_str())))
    }

    /// Retract a SOURCE carrier from EVERY project across EVERY workspace store —
    /// the delete / owner-no-longer-resolvable transition where the prior owning
    /// project (and even the workspace) cannot be re-resolved (a deleted carrier has
    /// no resolvable owner). Each workspace store removes the source from all its
    /// projects. Best-effort across stores: the FIRST IO error is returned, but every
    /// store is attempted so a single transient failure does not leave the source
    /// advertised everywhere else.
    pub(in crate::external_ts) fn retract_source_everywhere(
        &self,
        source_uri: &str,
    ) -> Result<(), EngineError> {
        let mut first_err: Option<EngineError> = None;
        for entry in self.stores.iter() {
            if let Err(e) = entry
                .value()
                .store
                .retract_source_from_all_projects(source_uri)
            {
                let err = EngineError::Unavailable(Arc::from(e.to_string().as_str()));
                if first_err.is_none() {
                    first_err = Some(err);
                }
            }
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Retract a SOURCE carrier from every project across every workspace store
    /// EXCEPT `keep_project_uri` — the owner-CHANGE (A→B) prune that follows a
    /// fresh per-source publish into the new owning project. The publish unions the
    /// source into the new project (`OwnedSetScope::SourceDelta`, never prunes), so
    /// this removes its stale rows from every OTHER project (the old owner) while
    /// leaving the new owning project's just-published rows intact. Best-effort
    /// across stores: every store is attempted and the FIRST IO error is returned,
    /// so a single transient failure does not leave the source advertised in some
    /// other store.
    pub(in crate::external_ts) fn retract_source_everywhere_except(
        &self,
        source_uri: &str,
        keep_project_uri: &str,
    ) -> Result<(), EngineError> {
        // Test-only fault injection: when armed, fail the stale-owner prune BEFORE any
        // store mutation, so the `publish_owned_resolved` compensation/rollback path runs.
        // Production never arms it (the arming method is `#[cfg(test)]`), so this is a
        // no-op there.
        if self
            .fail_next_prune_except
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(EngineError::Unavailable(Arc::from(
                "injected owner-move stale-owner prune failure (test-only fault seam)",
            )));
        }
        let mut first_err: Option<EngineError> = None;
        for entry in self.stores.iter() {
            if let Err(e) = entry
                .value()
                .store
                .retract_source_from_all_projects_except(source_uri, keep_project_uri)
            {
                let err = EngineError::Unavailable(Arc::from(e.to_string().as_str()));
                if first_err.is_none() {
                    first_err = Some(err);
                }
            }
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Test-only: arm the NEXT [`Self::retract_source_everywhere_except`] (the
    /// owner-move stale-owner prune) to fail BEFORE any store mutation, so the
    /// `publish_owned_resolved` compensation/rollback path can be exercised without a real IO
    /// fault. Production never arms it.
    #[cfg(test)]
    pub(in crate::external_ts) fn arm_prune_except_failure(&self) {
        self.fail_next_prune_except
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl EngineBackend for TsserverEngineBackend {
    /// Ensure the project's per-workspace store exists and mint the [`BoundProject`]
    /// witness from the [`EnsureProject`] request (which is itself mintable only from
    /// a resolved `ProjectBinding`). The store dir is created lazily on the first
    /// publish; this call materialises the per-workspace store handle, records the
    /// project under its workspace root, and binds the witness to the request's
    /// project + env dims.
    fn ensure_project(&self, request: EnsureProject) -> Result<BoundProject, EngineError> {
        let ws = self.workspace_store(request.workspace_root());
        ws.projects.lock().insert(Arc::from(request.tsconfig_uri()));
        // Mint the witness through the contract's sealed `from_ensured` path — the
        // raw seal never leaves the contract module, so this does NOT add a pub
        // bypass that would violate `provider_op_requires_resolved_project`.
        Ok(BoundProject::from_ensured(
            &request,
            self.capabilities.clone(),
        ))
    }

    /// Two-phase publish the snapshot through the on-disk store. The published
    /// `provider_uri` set enters the manifest's `ready_files` ONLY after each blob
    /// write succeeds (the two-phase guarantee). The owned set is derived from the
    /// snapshot's own files (the published delta is the owned set for this publish);
    /// a larger owned set is registered separately via [`Self::register_owned`].
    fn publish_snapshot(
        &self,
        project: &BoundProject,
        snapshot: PublishSnapshot,
    ) -> Result<(), EngineError> {
        // The snapshot's project must match the witness it is published under.
        if &*snapshot.project != project.project() {
            return Err(ensure_failed(
                "publish_snapshot project does not match the bound project",
            ));
        }
        let ws = self
            .workspace_store_for_project(project)
            .ok_or_else(|| ensure_failed("publish_snapshot for an un-ensured project"))?;
        // A live publish carries ONLY the touched carrier's companions — a per-source
        // DELTA, not the project's full owned set. It must UNION its own rows and
        // NEVER prune sibling carriers (which it does not know about); a sibling that
        // leaves the project is retracted explicitly via `retract_source`.
        let batch = PublishBatch::from_snapshot(
            ws.workspace_root.to_string(),
            snapshot,
            None,
            OwnedSetScope::SourceDelta,
        );
        ws.store
            .publish_batch(&batch)
            .map_err(|e| EngineError::Unavailable(Arc::from(e.to_string().as_str())))?;
        Ok(())
    }

    /// The live tsserver transport (open the ready blob, issue the feature request,
    /// map back) is wired separately from this publish authority. NOT a silent stub:
    /// `unimplemented!()` fails loudly so a premature call is caught, never a
    /// forbidden always-`NoResult` nop.
    fn query(&self, _project: &BoundProject, _query: Query) -> Result<QueryOutcome, EngineError> {
        unimplemented!(
            "TsserverEngineBackend::query is answered by the live tsserver transport \
             (open the ready companion blob, issue the feature query, map the result \
             back through the carrier source map), which is wired separately from this \
             on-disk publish authority."
        )
    }

    /// Answered by the live tsserver transport, wired separately. See [`Self::query`].
    fn diagnostics(
        &self,
        _project: &BoundProject,
        _request: Diagnostics,
    ) -> Result<DiagnosticsOutcome, EngineError> {
        unimplemented!(
            "TsserverEngineBackend::diagnostics is answered by the live tsserver \
             transport, wired separately from this on-disk publish authority."
        )
    }

    fn capabilities(&self) -> EngineCapabilities {
        self.capabilities.clone()
    }
}

/// An [`EngineError::EnsureFailed`] from a static message.
fn ensure_failed(msg: &str) -> EngineError {
    EngineError::EnsureFailed(Arc::from(msg))
}

#[cfg(test)]
#[path = "tsserver_backend_tests.rs"]
mod tests;

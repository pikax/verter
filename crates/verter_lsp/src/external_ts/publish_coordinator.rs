//! The live carrier-publish coordinator — the seam that makes a framework
//! carrier a member of its REAL configured project for the tsserver engine.
//!
//! When a `.vue`/`.svelte` source syncs, the LSP resolves its owning configured
//! project, ensures that project on the [`TsserverEngineBackend`], and publishes
//! the carrier's IDE (`{name}.vue.tsx`) and public-API (`{name}.vue.verter.ts`)
//! companions into the on-disk content-addressed carrier-publish store. The
//! `@verter/typescript-plugin` (loaded into the user's tsserver) reads that store
//! synchronously and advertises the ready carriers to tsserver via
//! `getExternalFiles` + `extraFileExtensions`, so the carrier is a configured-
//! project member and sees the project's own `paths`/`baseUrl`/`types`/`lib`/
//! `jsx`/`moduleResolution`/references.
//!
//! This REPLACES the direct `provider.open_file` of carrier companions for the
//! tsserver engine: the LSP no longer opens the synthetic `.vue.tsx`/`.verter.ts`
//! buffers into tsserver itself — the plugin's store-backed membership serves
//! them. The user's REAL `.ts`/`.tsx` files and the `.vue` SOURCE documents the
//! editor opens still flow through the normal document-sync path.
//!
//! Project resolution is the contract's fail-closed gate: a `NoProject` /
//! `Ambiguous` / `NotReady` source produces NO publish (no carrier
//! membership), so a carrier that would shadow a real user file — or whose owner
//! is undecidable — is never advertised. Only a resolved
//! [`ProjectBinding`](verter_session::external_ts::ProjectBinding) can mint the
//! [`EnsureProject`](verter_session::external_ts::EnsureProject) →
//! [`BoundProject`](verter_session::external_ts::BoundProject) witness every
//! publish requires.

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::Mutex as AsyncMutex;

use verter_semantic::analysis::types::Hash16;
use verter_session::external_ts::{
    CarrierOwnershipResolution, EngineBackend, EnvDims, ExternalTsProjectResolver, OpenState,
    ProjectBinding, ScriptKind, SnapshotFile, SnapshotRole, WorkspaceProjectResolver,
};
use verter_session::VerterHost;
use verter_workspace::FilesystemWorkspace;

use crate::carrier_provider_projection::PreparedCarrierProviderContent;
use crate::external_ts::membership_ledger::AbsentReason;
use crate::external_ts::membership_reconciler::{
    CarrierMembershipCommitter, CommitFuture, MembershipReconciler, ReconcileErr, ReconcileOutcome,
    ReconcileReason,
};
use crate::external_ts::tsserver_backend::TsserverEngineBackend;
use crate::external_ts::CanonicalSource;
use crate::type_provider::traits::TypeProvider;

/// The bytes a companion publishes, carrying the projection when its role has
/// one. The IDE role's buffer is projected after compilation, so its bytes and
/// the mapper describing them travel as ONE value: a recorder must never be able
/// to pick up the content here and a mapper from somewhere else.
#[derive(Debug, Clone)]
pub(crate) enum CompanionContent {
    /// Roles the provider receives exactly as compiled (API / Shadow / Real).
    Verbatim(Arc<str>),
    /// The carrier IDE buffer, whose generated imports were projected.
    ProjectedIde(PreparedCarrierProviderContent),
}

/// One carrier companion to publish: its provider (companion) path, the carrier
/// content, its source-map JSON (if any), and the contract role/script-kind.
#[derive(Debug, Clone)]
pub struct CarrierCompanion {
    /// The companion provider path (`/proj/src/Comp.vue.tsx` or
    /// `/proj/src/Comp.vue.verter.ts`).
    pub provider_uri: Arc<str>,
    /// The carrier content bytes the store writes (the IDE TSX or API
    /// `.verter.ts`) — plus, for the projected IDE role, the mapper describing
    /// exactly those bytes. Private so the two can never be set apart.
    content: CompanionContent,
    /// The `CodeTransform` source-map JSON, threaded so the store writes a map
    /// blob the plugin reads for navigation remapping. `None` ⇒ no map blob.
    pub map_json: Option<Arc<str>>,
    /// Content-free carrier ownership facts for store-backed editor consumers.
    pub structure: Option<verter_session::external_ts::SnapshotStructureStamp>,
    /// The contract role of this companion (`CarrierIde` / `CarrierApi`).
    pub role: SnapshotRole,
    /// The companion's TypeScript script kind (TSX/JSX for IDE, TS for API).
    pub script_kind: ScriptKind,
    /// The monotonic provider generation/version for this companion.
    pub version: u64,
}

impl CarrierCompanion {
    /// A companion whose bytes reach the provider exactly as compiled. The IDE
    /// role never uses this constructor — its buffer is projected, and
    /// [`Self::carrier_ide`] is the only way to build it.
    #[must_use]
    pub(crate) fn verbatim(
        provider_uri: Arc<str>,
        content: Arc<str>,
        map_json: Option<Arc<str>>,
        role: SnapshotRole,
        script_kind: ScriptKind,
    ) -> Self {
        verter_debug_assert!(
            role != SnapshotRole::CarrierIde,
            "the IDE companion is projected; build it with CarrierCompanion::carrier_ide"
        );
        Self {
            provider_uri,
            content: CompanionContent::Verbatim(content),
            map_json,
            structure: None,
            role,
            script_kind,
            version: 0,
        }
    }

    /// The projected carrier IDE companion. Taking the prepared value whole is
    /// what keeps the published bytes and the recorded mapper the same answer.
    #[must_use]
    pub(crate) fn carrier_ide(
        provider_uri: Arc<str>,
        prepared: PreparedCarrierProviderContent,
        map_json: Option<Arc<str>>,
        script_kind: ScriptKind,
    ) -> Self {
        Self {
            provider_uri,
            content: CompanionContent::ProjectedIde(prepared),
            map_json,
            structure: None,
            role: SnapshotRole::CarrierIde,
            script_kind,
            version: 0,
        }
    }

    /// The exact bytes this companion publishes.
    #[must_use]
    pub fn content(&self) -> &Arc<str> {
        match &self.content {
            CompanionContent::Verbatim(content) => content,
            CompanionContent::ProjectedIde(prepared) => prepared.content(),
        }
    }

    /// The publishable content together with its projection, for the recorder.
    pub(crate) fn recorded_content(&self) -> &CompanionContent {
        &self.content
    }

    /// An IDE companion whose projection is prepared from `content` through the
    /// SAME preparer production uses, so a test never fabricates a content/mapper
    /// pair the projection owner would not have produced.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn carrier_ide_from_generated(
        provider_uri: Arc<str>,
        canonical_id: &str,
        generated: &str,
        map_json: Option<Arc<str>>,
        script_kind: ScriptKind,
        version: u64,
    ) -> Self {
        let prepared = crate::carrier_provider_projection::expect_admitted(
            crate::carrier_provider_projection::prepare_carrier_provider_imports(
                None,
                canonical_id,
                generated,
                tower_lsp_server::ls_types::PositionEncodingKind::UTF16,
            ),
            "the test preparer has no workspace resolution inputs",
        );
        let mut companion = Self::carrier_ide(provider_uri, prepared, map_json, script_kind);
        companion.version = version;
        companion
    }

    /// Substitute the surface the provider ACTUALLY received for this companion.
    /// Only a successful delivery mints the evidence, and it carries its own
    /// mapper, so the pair stays consistent through the substitution.
    pub(crate) fn replace_with_delivered_ide_surface(
        &mut self,
        delivered: PreparedCarrierProviderContent,
    ) {
        self.content = CompanionContent::ProjectedIde(delivered);
    }
}

/// The live carrier-publish coordinator for the tsserver engine.
///
/// Holds the shared [`TsserverEngineBackend`] (the on-disk publish authority — the
/// SAME store the tsserver spawn delivers to the plugin via
/// `VERTER_CARRIER_STORE_DIR`) and the active type provider (for the post-publish
/// negative-cache eviction). One per LSP session; cheap to clone (all `Arc`).
#[derive(Clone)]
pub struct CarrierPublishCoordinator {
    backend: Arc<TsserverEngineBackend>,
    /// Local managed-tsserver actor. The editor-owned plugin route has no local
    /// actor and consumes the durable store directly.
    provider: Option<Arc<dyn TypeProvider>>,
    /// The negotiated TypeScript version string carried on every minted binding.
    ts_version: Arc<str>,
    /// The ONE shared per-source membership-serialization gate map. The coordinator
    /// rebuilds a [`MembershipReconciler`] per transition ([`Self::reconciler`]), so it
    /// owns this map and hands the same `Arc` to every rebuilt reconciler — otherwise
    /// each per-call reconciler would get a fresh (useless) map and concurrent
    /// same-source transitions would not serialize. Shared across coordinator clones.
    source_gates: Arc<DashMap<String, Arc<AsyncMutex<()>>>>,
}

impl CarrierPublishCoordinator {
    /// Build the coordinator over the shared backend + active provider.
    #[must_use]
    pub fn new(
        backend: Arc<TsserverEngineBackend>,
        provider: Arc<dyn TypeProvider>,
        ts_version: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            backend,
            provider: Some(provider),
            ts_version: ts_version.into(),
            source_gates: Arc::new(DashMap::new()),
        }
    }

    /// Build the coordinator for an editor-owned tsserver plugin. Verter publishes
    /// the same durable membership store but owns no semantic child process.
    #[must_use]
    pub fn new_editor_owned(
        backend: Arc<TsserverEngineBackend>,
        ts_version: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            backend,
            provider: None,
            ts_version: ts_version.into(),
            source_gates: Arc::new(DashMap::new()),
        }
    }

    /// The shared publish backend (the same store the spawn points the plugin at).
    #[must_use]
    pub fn backend(&self) -> &Arc<TsserverEngineBackend> {
        &self.backend
    }

    /// Retract a carrier source from the publish store — the delete / owner-lost /
    /// now-ambiguous transition. Removes the source's owned rows + advertised
    /// companions from EVERY project that owned it, so `getExternalFiles` stops
    /// serving the carrier. The owning project need not be re-resolvable (a deleted
    /// source's owner cannot be), so this retracts across all projects. The
    /// provider's open companion BUFFER is closed separately by the LSP's
    /// `close_provider_state` (the buffer close and the store retraction together
    /// fully retract the carrier).
    ///
    /// Fail-closed: a backend retraction failure is PROPAGATED as `Err`, never
    /// swallowed — the caller must not report a fail-closed "not published" success
    /// while the carrier is still advertised in `getExternalFiles`. The pointer set
    /// is durable on `Ok` (the manifest swap is fsynced).
    ///
    /// Sealed: only the reconciler (via [`CarrierMembershipCommitter::retract`])
    /// reaches it, so no server path can retract the store without the ledger
    /// tombstone.
    pub(in crate::external_ts) fn retract_carrier(
        &self,
        source_canonical: &str,
    ) -> Result<(), CarrierPublishError> {
        self.backend
            .retract_source_everywhere(source_canonical)
            .map_err(|e| CarrierPublishError::Retract(format!("{e:?}")))
    }

    /// The membership reconciler over THIS coordinator's shared ledger
    /// (`backend.membership_ledger()`), the active provider (the resilient
    /// single-writer actor), and this coordinator as the on-disk membership-commit
    /// seam (the [`CarrierMembershipCommitter`] implementation for the tsserver
    /// engine). Cheap to build (all `Arc` clones); every production decision point
    /// routes its membership transition through it.
    #[must_use]
    pub(crate) fn reconciler(&self) -> MembershipReconciler {
        let ledger = Arc::clone(self.backend.membership_ledger());
        let committer = Arc::new(self.clone()) as Arc<dyn CarrierMembershipCommitter>;
        let source_gates = Arc::clone(&self.source_gates);
        match &self.provider {
            Some(provider) => {
                MembershipReconciler::new(ledger, Arc::clone(provider), committer, source_gates)
            }
            None => MembershipReconciler::new_store_only(ledger, committer, source_gates),
        }
    }

    /// Promote an already-published carrier into the managed tsserver working
    /// set. This consumes only ledger/store metadata and is intentionally
    /// independent of Verter semantic regeneration.
    pub(crate) async fn activate_published_source(
        &self,
        source_canonical: &str,
    ) -> Result<bool, verter_type_runtime::protocol::TypeProviderError> {
        self.reconciler()
            .activate_published_source(&CanonicalSource::from(source_canonical))
            .await
    }

    /// Promote a demand-discovered carrier frontier with one managed-provider
    /// project refresh. Missing/stale advertisements are skipped; the returned
    /// count is the number of current-session sources proven activatable.
    pub(crate) async fn activate_published_sources(
        &self,
        source_canonicals: &[String],
    ) -> Result<usize, verter_type_runtime::protocol::TypeProviderError> {
        let sources: Vec<CanonicalSource> = source_canonicals
            .iter()
            .map(|source| CanonicalSource::new(source.clone()))
            .collect();
        self.reconciler().activate_published_sources(&sources).await
    }

    /// Non-blockingly return the demanded sources that lack current-session IDE
    /// advertisements. Readiness checks use this before activation so an LSP
    /// request never waits behind background membership publication; the scanner
    /// also feeds this exact set through the existing retry drain at its carrier
    /// phase boundary.
    pub(crate) fn missing_published_activations(
        &self,
        source_canonicals: &[String],
    ) -> Vec<String> {
        let sources: Vec<CanonicalSource> = source_canonicals
            .iter()
            .map(|source| CanonicalSource::new(source.clone()))
            .collect();
        self.reconciler()
            .missing_published_activations(&sources)
            .into_iter()
            .map(|source| source.as_str().to_string())
            .collect()
    }

    /// The SINGLE membership-transition entry every production decision point routes
    /// through: resolve ownership ONCE (synchronously; the borrowing resolver is
    /// dropped before any await) and run the authoritative transition through the
    /// reconciler. `ownership_ready` is the caller's cold-vs-authoritative snapshot
    /// signal (a cold snapshot defers without thrash). On an owned resolution the
    /// carrier is published (durable store + provider buffer + ledger); on
    /// absent/owner-loss it is retracted; on cold it defers — all fail-closed.
    /// Resolve ownership from the published snapshot AND run the membership transition
    /// in one call. Production captures the [`CarrierOwnershipResolution`] ONCE (so the
    /// two engines and the tier scanner share one resolution) and calls
    /// [`Self::resolve_carrier_ownership`] + [`Self::reconcile_membership_with_resolution`]
    /// separately; this combined entry is the coordinator's integration-test seam that
    /// exercises both halves end-to-end from a host/VFS snapshot.
    #[cfg(test)]
    pub(crate) async fn reconcile_membership(
        &self,
        host: &VerterHost,
        vfs: &FilesystemWorkspace,
        source_canonical: &str,
        companions: Vec<CarrierCompanion>,
        ownership_ready: bool,
        reason: ReconcileReason,
    ) -> Result<ReconcileOutcome, ReconcileErr> {
        let resolution =
            self.resolve_carrier_ownership(host, vfs, source_canonical, ownership_ready);
        self.reconcile_membership_with_resolution(source_canonical, resolution, companions, reason)
            .await
    }

    /// Run the authoritative membership transition for a source over the ONE captured
    /// [`CarrierOwnershipResolution`] — the caller already resolved ownership (through
    /// the shared resolver) and hands it here, so the reconciler never re-resolves. An owned
    /// reconcile mints a readiness receipt with a placeholder intent epoch; the carrier-sync
    /// gateway stamps the transaction's captured epoch onto it before returning the owned
    /// decision (see [`ProviderReadyReceipt::stamped_with_intent_epoch`]).
    pub(crate) async fn reconcile_membership_with_resolution(
        &self,
        source_canonical: &str,
        resolution: CarrierOwnershipResolution,
        companions: Vec<CarrierCompanion>,
        reason: ReconcileReason,
    ) -> Result<ReconcileOutcome, ReconcileErr> {
        self.reconciler()
            .reconcile_source_membership(
                &CanonicalSource::from(source_canonical),
                resolution,
                companions,
                reason,
            )
            .await
    }

    /// Remove a source's external-TS membership — the explicit terminal-absent
    /// (delete / conflict-removed) transition. Routes through the reconciler's
    /// `remove_source_membership` (durable store retract + provider close + ledger
    /// tombstone), fail-closed.
    pub(crate) async fn remove_membership(
        &self,
        source_canonical: &str,
        reason: AbsentReason,
    ) -> Result<ReconcileOutcome, ReconcileErr> {
        self.reconciler()
            .remove_source_membership(&CanonicalSource::from(source_canonical), reason)
            .await
    }

    /// Refresh the managed provider once after a background publication batch.
    /// Store-backed providers coalesce their plugin token, changed-file eviction,
    /// and ordering fence across the complete companion set.
    pub(crate) async fn refresh_published_companions(
        &self,
        companion_paths: &[String],
    ) -> Result<(), verter_type_runtime::TypeProviderError> {
        let Some(provider) = &self.provider else {
            return Ok(());
        };
        provider.notify_carriers_changed(companion_paths).await
    }

    /// Resolve a carrier source's ownership ONCE, synchronously.
    ///
    /// Delegates to [`resolve_carrier_ownership_over_vfs`] over the coordinator's
    /// negotiated `ts_version`: it builds the shared [`WorkspaceProjectResolver`] over
    /// the published snapshot (ownership read from the host's published state, the
    /// carrier-path conflict pass, per-canonical R21 env dims — never fabricated) and
    /// returns the one [`CarrierOwnershipResolution`]. The borrowing resolver is built
    /// and dropped entirely inside this synchronous call, so it never crosses an
    /// `.await`; the caller hands the owned resolution to
    /// [`MembershipReconciler::reconcile_source_membership`]. `ownership_ready` is the
    /// caller's cold-vs-authoritative snapshot signal (a cold snapshot defers without
    /// thrash). A missing published snapshot is a cold bootstrap
    /// ([`CarrierOwnershipResolution::NotReady`]), never an owner-loss retract.
    #[must_use]
    pub(crate) fn resolve_carrier_ownership(
        &self,
        host: &VerterHost,
        vfs: &FilesystemWorkspace,
        source_canonical: &str,
        ownership_ready: bool,
    ) -> CarrierOwnershipResolution {
        resolve_carrier_ownership_over_vfs(
            host,
            vfs,
            source_canonical,
            ownership_ready,
            Arc::clone(&self.ts_version),
        )
    }

    /// Publish an ALREADY-resolved owned carrier's companions into the on-disk
    /// content-addressed store (blobs + manifest) — the durable half of an owned
    /// membership transition. Mints the engine witness from the resolved binding
    /// (NO re-resolution), runs the two-phase store publish, then prunes the source
    /// from every OTHER project so an owner change leaves nothing under the old
    /// project. Provider-buffer + ledger steps are the reconciler's; this is purely
    /// the durable store. Sealed: only the reconciler (via the on-disk
    /// [`CarrierMembershipCommitter`] impl) reaches it.
    pub(in crate::external_ts) fn publish_owned_resolved(
        &self,
        binding: &ProjectBinding,
        source_canonical: &str,
        companions: &[CarrierCompanion],
    ) -> Result<(), CarrierPublishError> {
        // Mint the witness through the contract type-state (a binding → an
        // `EnsureProject` → a `BoundProject`); a foreign caller cannot fabricate it.
        let bound = self
            .backend
            .ensure_project(binding.ensure_project_request())
            .map_err(|e| CarrierPublishError::Ensure(format!("{e:?}")))?;
        let owning_tsconfig = binding.tsconfig_uri().to_string();

        let files: Vec<SnapshotFile> = companions
            .iter()
            .map(|c| snapshot_file_of(source_canonical, c))
            .collect();
        let snapshot = verter_session::external_ts::PublishSnapshot {
            project: Arc::from(binding.tsconfig_uri()),
            files,
            resolution_map_version: 0,
            fs_generation: 0,
        };
        self.backend
            .publish_snapshot(&bound, snapshot)
            .map_err(|e| CarrierPublishError::Publish(format!("{e:?}")))?;
        // The per-edit publish is a `SourceDelta` (unions into the target, never
        // prunes siblings), so the stale-owner prune is a SEPARATE store write that
        // removes this source from every OTHER project — an owner A→B move must not
        // leave the carrier advertised in its OLD project. A partial failure HERE
        // (publish committed, prune failed) would leave the source advertised under
        // BOTH the new owner and the stale old owner: a duplicated / stale `ready_files`
        // set the plugin serves cross-process. COMPENSATE — roll the publish back by
        // retracting the source from EVERY project (including the just-published new
        // owner) so the store is never left half-applied (no stale row, no duplicate),
        // then propagate the prune error fail-closed (the reconciler does not commit
        // the ledger; the next publish retries cleanly).
        if let Err(prune_err) = self
            .backend
            .retract_source_everywhere_except(source_canonical, &owning_tsconfig)
        {
            if let Err(rollback_err) = self.backend.retract_source_everywhere(source_canonical) {
                return Err(CarrierPublishError::Retract(format!(
                    "owner-move stale-owner prune failed ({prune_err:?}); the compensating \
                     rollback retract ALSO failed ({rollback_err:?}) — the carrier may remain \
                     advertised under multiple projects"
                )));
            }
            return Err(CarrierPublishError::Retract(format!(
                "owner-move stale-owner prune failed; rolled the publish back to keep the \
                 cross-process ready_files set consistent (source left unadvertised): \
                 {prune_err:?}"
            )));
        }
        Ok(())
    }
}

/// The ON-DISK implementation of the engine-agnostic membership-commit seam for the
/// tsserver engine. The store mutation is a synchronous fsync'd swap, so these
/// futures move it onto Tokio's blocking pool and await the result. This preserves
/// two-phase durability and fail-closed errors without occupying an async runtime
/// worker while the filesystem flushes.
impl CarrierMembershipCommitter for CarrierPublishCoordinator {
    fn commit_owned<'a>(
        &'a self,
        binding: &'a ProjectBinding,
        source_canonical: &'a str,
        companions: &'a [CarrierCompanion],
    ) -> CommitFuture<'a> {
        let coordinator = self.clone();
        let binding = binding.clone();
        let source_canonical = source_canonical.to_string();
        let companions = companions.to_vec();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                coordinator.publish_owned_resolved(&binding, &source_canonical, &companions)
            })
            .await
            .map_err(|error| {
                CarrierPublishError::Publish(format!("publish task failed: {error}"))
            })?
        })
    }

    fn retract<'a>(&'a self, source_canonical: &'a str) -> CommitFuture<'a> {
        let coordinator = self.clone();
        let source_canonical = source_canonical.to_string();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || coordinator.retract_carrier(&source_canonical))
                .await
                .map_err(|error| {
                    CarrierPublishError::Retract(format!("retract task failed: {error}"))
                })?
        })
    }
}

/// An error from the live carrier publish.
#[derive(Debug, Clone)]
pub enum CarrierPublishError {
    /// `ensure_project` failed on the backend.
    Ensure(String),
    /// The two-phase store publish failed.
    Publish(String),
    /// A store retraction (owner-loss / cross-project A→B prune / publish-time
    /// re-validation) failed — the carrier may still be advertised, so this is
    /// PROPAGATED rather than swallowed (the fail-closed durability contract).
    Retract(String),
}

impl std::fmt::Display for CarrierPublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CarrierPublishError::Ensure(m) => write!(f, "ensure_project failed: {m}"),
            CarrierPublishError::Publish(m) => write!(f, "carrier publish failed: {m}"),
            CarrierPublishError::Retract(m) => write!(f, "carrier retract failed: {m}"),
        }
    }
}

impl std::error::Error for CarrierPublishError {}

/// Build the contract [`EnvDims`] for a configured project from the host's
/// per-canonical R21 env-hash + project-identity readers. The resolver feeds the
/// owning tsconfig URI; we map it to a representative owned canonical to read the
/// project's env hashes. The host NEVER fabricates env identity — these are the
/// authoritative per-project values (the `no_default_env_hashes_in_production`
/// rule), so the binding carries real dims.
/// Resolve a carrier source's ownership decision ONCE from a published VFS snapshot —
/// the shared ownership-resolution primitive BOTH engines' carrier-sync path uses
/// (tsserver via [`CarrierPublishCoordinator::resolve_carrier_ownership`], tsgo
/// direct-open). Builds the shared [`WorkspaceProjectResolver`] over
/// `vfs.load_published()` (ownership + carrier-path conflict pass + per-canonical R21
/// env dims from the host), so the scanner and the sync path share ONE resolution
/// source. A missing published snapshot is the transient
/// [`CarrierOwnershipResolution::NotReady`] (deferred without thrash, never owner-loss).
/// `ts_version` is carried onto a resolved binding's metadata; it is NOT load-bearing
/// for the witness identity or the `--api` op, so a bootstrap/empty value is safe.
#[must_use]
pub(crate) fn resolve_carrier_ownership_over_vfs(
    host: &VerterHost,
    vfs: &FilesystemWorkspace,
    source_canonical: &str,
    ownership_ready: bool,
    ts_version: Arc<str>,
) -> CarrierOwnershipResolution {
    let Some(published) = vfs.load_published() else {
        return CarrierOwnershipResolution::NotReady;
    };
    let env_dims_source =
        |member_canonical: &str| env_dims_for_project(host, &published.snapshot, member_canonical);
    let resolver = WorkspaceProjectResolver::new(
        &published.snapshot,
        vfs,
        ts_version,
        &env_dims_source,
        ownership_ready,
    );
    resolver.resolve(source_canonical, None)
}

fn env_dims_for_project(
    host: &VerterHost,
    snapshot: &verter_workspace::workspace_snapshot::WorkspaceSnapshot,
    member_canonical: &str,
) -> EnvDims {
    // The host env-hash readers are per-CANONICAL; a configured project's env hashes
    // are uniform across its members, so reading them for a MEMBER canonical of the
    // resolved project (the resolved carrier source) yields the project's dims. The
    // resolver passes an owned member, NOT the tsconfig path — the tsconfig file is
    // normally outside the project's membership set, so keying the readers on it
    // resolves to no owner and falls back to the workspace-default bundle.
    let _ = snapshot; // ownership resolution is internal to the host readers
    let env = host.host_view_env_hashes_for(member_canonical);
    let project_identity = host.host_view_project_identity_for(member_canonical);
    EnvDims {
        parse_env_hash: env.parse_env_hash,
        resolve_env_hash: env.resolve_env_hash,
        lib_env_hash: env.lib_env_hash,
        project_identity,
    }
}

/// Lower a [`CarrierCompanion`] into the contract [`SnapshotFile`] DTO, computing
/// the content-addressed `content_hash`/`map_hash` the store keys blobs on. The
/// companion is `Closed` (a store-served external member, not an editor buffer).
fn snapshot_file_of(source_canonical: &str, companion: &CarrierCompanion) -> SnapshotFile {
    let content_hash = hash16_of_str(companion.content());
    let map_hash = companion
        .map_json
        .as_deref()
        .map(hash16_of_str)
        .unwrap_or([0u8; 16]);
    SnapshotFile {
        source_uri: Arc::from(source_canonical),
        provider_uri: Arc::clone(&companion.provider_uri),
        role: companion.role,
        script_kind: companion.script_kind,
        content: Arc::clone(companion.content()),
        content_hash,
        map_hash,
        map_json: companion.map_json.clone(),
        structure: companion.structure.clone(),
        version: companion.version,
        open_state: OpenState::Closed,
    }
}

/// BLAKE3 → 16-byte content hash for a string (the same identity the store and
/// the provider-surface stamp use, so a published blob's hash matches the
/// recorded surface).
fn hash16_of_str(s: &str) -> Hash16 {
    let digest = blake3::hash(s.as_bytes());
    let mut h = [0u8; 16];
    h.copy_from_slice(&digest.as_bytes()[..16]);
    h
}

#[cfg(test)]
#[path = "publish_coordinator_tests.rs"]
mod tests;

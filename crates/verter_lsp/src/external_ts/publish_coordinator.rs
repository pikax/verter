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
//! `Ambiguous` / `SyntheticScratch` source produces NO publish (no carrier
//! membership), so a carrier that would shadow a real user file — or whose owner
//! is undecidable — is never advertised. Only a resolved
//! [`ProjectBinding`](verter_session::external_ts::ProjectBinding) can mint the
//! [`EnsureProject`](verter_session::external_ts::EnsureProject) →
//! [`BoundProject`](verter_session::external_ts::BoundProject) witness every
//! publish requires.

use std::sync::Arc;

use verter_semantic::analysis::types::Hash16;
use verter_session::external_ts::{
    EngineBackend, EnvDims, ExternalTsProjectResolver, OpenState, ProjectBinding, ScriptKind,
    SnapshotFile, SnapshotRole, WorkspaceProjectResolver,
};
use verter_session::VerterHost;
use verter_workspace::FilesystemWorkspace;

use crate::external_ts::membership_ledger::AbsentReason;
use crate::external_ts::membership_reconciler::{
    AuthorityState, BootstrapKind, CarrierMembershipCommitter, CommitFuture, MembershipReconciler,
    OwnershipDecision, PrecomputedOwnershipAuthority, ReconcileErr, ReconcileOutcome,
    ReconcileReason, ResolverOwnershipAuthority,
};
use crate::external_ts::tsserver_backend::TsserverEngineBackend;
use crate::external_ts::CanonicalSource;
use crate::type_provider::traits::TypeProvider;

/// One carrier companion to publish: its provider (companion) path, the carrier
/// content, its source-map JSON (if any), and the contract role/script-kind.
#[derive(Debug, Clone)]
pub struct CarrierCompanion {
    /// The companion provider path (`/proj/src/Comp.vue.tsx` or
    /// `/proj/src/Comp.vue.verter.ts`).
    pub provider_uri: Arc<str>,
    /// The carrier content bytes the store writes (the IDE TSX or API `.verter.ts`).
    pub content: Arc<str>,
    /// The `CodeTransform` source-map JSON, threaded so the store writes a map
    /// blob the plugin reads for navigation remapping. `None` ⇒ no map blob.
    pub map_json: Option<Arc<str>>,
    /// The contract role of this companion (`CarrierIde` / `CarrierApi`).
    pub role: SnapshotRole,
    /// The companion's TypeScript script kind (TSX/JSX for IDE, TS for API).
    pub script_kind: ScriptKind,
    /// The monotonic provider generation/version for this companion.
    pub version: u64,
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
    provider: Arc<dyn TypeProvider>,
    /// The negotiated TypeScript version string carried on every minted binding.
    ts_version: Arc<str>,
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
            provider,
            ts_version: ts_version.into(),
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
        MembershipReconciler::new(
            Arc::clone(self.backend.membership_ledger()),
            Arc::clone(&self.provider),
            Arc::new(self.clone()) as Arc<dyn CarrierMembershipCommitter>,
        )
    }

    /// The SINGLE membership-transition entry every production decision point routes
    /// through: resolve ownership ONCE (synchronously; the borrowing resolver is
    /// dropped before any await) and run the authoritative transition through the
    /// reconciler. `ownership_ready` is the caller's cold-vs-authoritative snapshot
    /// signal (a cold snapshot defers without thrash). On an owned resolution the
    /// carrier is published (durable store + provider buffer + ledger); on
    /// absent/owner-loss it is retracted; on cold it defers — all fail-closed.
    pub(crate) async fn reconcile_membership(
        &self,
        host: &VerterHost,
        vfs: &FilesystemWorkspace,
        source_canonical: &str,
        companions: Vec<CarrierCompanion>,
        ownership_ready: bool,
        reason: ReconcileReason,
    ) -> Result<ReconcileOutcome, ReconcileErr> {
        let authority_state = if ownership_ready {
            AuthorityState::Ready
        } else {
            AuthorityState::Bootstrap
        };
        let decision =
            self.resolve_ownership(host, vfs, source_canonical, authority_state, companions);
        let authority = PrecomputedOwnershipAuthority::new(decision);
        self.reconciler()
            .reconcile_source_membership(
                &CanonicalSource::from(source_canonical),
                &authority,
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

    /// Resolve a carrier source's ownership decision ONCE, synchronously.
    ///
    /// Builds the shared `WorkspaceProjectResolver` over the published snapshot
    /// (ownership read from the host's published state, the carrier-path conflict
    /// pass, per-canonical R21 env dims — never fabricated), maps its four
    /// [`ProjectResolution`] states through [`ResolverOwnershipAuthority`], and
    /// returns the resolved [`OwnershipDecision`]. The borrowing resolver is built
    /// and dropped entirely inside this synchronous call, so it never crosses an
    /// `.await`; the caller hands the owned decision to
    /// [`MembershipReconciler::reconcile_source_membership`] via a
    /// `PrecomputedOwnershipAuthority`. `authority_state` is the caller's
    /// cold-vs-authoritative snapshot signal (a cold snapshot defers without thrash).
    /// A missing published snapshot is a cold bootstrap (defer), never an owner-loss
    /// retract.
    #[must_use]
    pub(crate) fn resolve_ownership(
        &self,
        host: &VerterHost,
        vfs: &FilesystemWorkspace,
        source_canonical: &str,
        authority_state: AuthorityState,
        companions: Vec<CarrierCompanion>,
    ) -> OwnershipDecision {
        let Some(published) = vfs.load_published() else {
            // No published snapshot ⇒ ownership is not yet authoritative: a cold
            // bootstrap deferral, NOT an owner loss (deferring avoids thrashing a
            // carrier that re-resolves once the snapshot loads).
            return OwnershipDecision::Bootstrap {
                kind: BootstrapKind::ColdStart,
            };
        };
        let env_dims_source =
            |tsconfig_uri: &str| env_dims_for_project(host, &published.snapshot, tsconfig_uri);
        let resolver = WorkspaceProjectResolver::new(
            &published.snapshot,
            vfs,
            Arc::clone(&self.ts_version),
            &env_dims_source,
        );
        ResolverOwnershipAuthority::new(authority_state, |s| resolver.resolve(s), companions)
            .resolve_membership(&CanonicalSource::from(source_canonical))
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
/// tsserver engine. The commit is a synchronous fsync'd store swap (the
/// content-addressed blobs + atomic manifest the out-of-process
/// `@verter/typescript-plugin` reads), so the async `commit_owned` / `retract`
/// futures do that synchronous store work INLINE — the seam is async to admit a
/// future in-memory-overlay engine whose re-snapshot is genuinely asynchronous,
/// without changing this on-disk store's two-phase-publish / manifest /
/// negative-cache-evict behavior.
impl CarrierMembershipCommitter for CarrierPublishCoordinator {
    fn commit_owned<'a>(
        &'a self,
        binding: &'a ProjectBinding,
        source_canonical: &'a str,
        companions: &'a [CarrierCompanion],
    ) -> CommitFuture<'a> {
        Box::pin(async move { self.publish_owned_resolved(binding, source_canonical, companions) })
    }

    fn retract<'a>(&'a self, source_canonical: &'a str) -> CommitFuture<'a> {
        Box::pin(async move { self.retract_carrier(source_canonical) })
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
fn env_dims_for_project(
    host: &VerterHost,
    snapshot: &verter_workspace::workspace_snapshot::WorkspaceSnapshot,
    tsconfig_uri: &str,
) -> EnvDims {
    // The host env-hash readers are per-CANONICAL; a configured project's env hashes
    // are uniform across its members, so reading them for the tsconfig URI's owning
    // project via any of its files yields the project's dims. We read for the
    // tsconfig path itself (the published snapshot resolves it to its own project),
    // falling back to the workspace-default dims the host applies for an
    // unresolved canonical.
    let _ = snapshot; // ownership resolution is internal to the host readers
    let env = host.host_view_env_hashes_for(tsconfig_uri);
    let project_identity = host.host_view_project_identity_for(tsconfig_uri);
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
    let content_hash = hash16_of_str(&companion.content);
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
        content: Arc::clone(&companion.content),
        content_hash,
        map_hash,
        map_json: companion.map_json.clone(),
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

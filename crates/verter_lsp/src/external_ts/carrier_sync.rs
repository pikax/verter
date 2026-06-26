//! The sealed carrier-sync gateway — the SINGLE callable surface that fuses the
//! carrier MEMBERSHIP decision and the provider-buffer state COMMIT so a sync site
//! can never perform one without the other.
//!
//! ## The bug class this closes
//!
//! Carrier sync has two separable effects: (1) the membership decision (publish a
//! carrier into the on-disk store the `@verter/typescript-plugin` reads, or retract
//! it on owner loss) and (2) the provider-buffer state commit (the per-source
//! [`ProviderSyncState`] the LSP tracks). A site could perform (2) for a tsserver
//! carrier — whose buffer verbs (`load_dts`/`load_tsx`) are no-ops — and simply
//! FORGET (1), leaving the carrier unpublished/unretracted in the store. That is an
//! ABSENT call, which sealing the low-level store mutators cannot catch.
//!
//! ## The fusion
//!
//! Every carrier sync resolves through [`reconcile_carrier_source`]: it computes the
//! provider-buffer transition, builds companions, runs the membership reconciliation
//! through the single [`MembershipReconciler`](super::membership_reconciler) (publish
//! on owned, retract/defer on owner loss), and returns a SEALED
//! [`CarrierProviderCommit`] receipt. The provider-buffer commit
//! ([`commit_carrier_provider_state`]) REQUIRES that receipt, and the receipt's
//! constructor is private to this module — so a carrier [`ProviderSyncState`] can
//! NOT be committed without first routing through the membership decision.
//!
//! TSGO keeps its direct carrier-open path: the gateway returns
//! [`CarrierSyncDecision::DirectOpen`] carrying the transition, and the TSGO site
//! opens the companion buffers itself (the receipt still gates the commit). The
//! gateway governs tsserver carrier MEMBERSHIP; TSGO's direct-open stays its own
//! path.

use dashmap::DashMap;
use std::sync::Arc;

use verter_session::external_ts::{ScriptKind, SnapshotRole};
use verter_session::{IdeResponse, VerterHost};
use verter_workspace::FilesystemWorkspace;

use crate::documents::DocumentRegistry;
use crate::external_ts::{
    CarrierCompanion, CarrierPublishCoordinator, ReconcileOutcome, ReconcileReason,
};
use crate::project_resolver::NativeProjectResolver;
use crate::provider_surface_store::ProviderSurfaceStore;
use crate::provider_sync::{
    commit_sync_transition, prepare_sync_transition, ProviderOwnerBinding, ProviderPathKind,
    ProviderSyncState, ProviderSyncTransition,
};
use crate::server::block_in_place_guarded as block_in_place_if_available;

/// A SEALED receipt proving a carrier source's membership decision was made through
/// the gateway. It is the capability token required to commit a carrier
/// [`ProviderSyncState`] (see [`commit_carrier_provider_state`]).
///
/// The single private field makes the constructor inaccessible outside this module:
/// a receipt can ONLY be obtained from [`reconcile_carrier_source`], so a caller
/// cannot commit carrier provider state without a reconciler outcome. This is the
/// type-level half of the fusion; the guard
/// (`sealed_carrier_store_mutators_allowlist`) is the static-analysis backstop.
#[derive(Debug)]
pub(crate) struct CarrierProviderCommit {
    _seal: (),
}

impl CarrierProviderCommit {
    /// Mint a receipt. PRIVATE to this module — the gateway is the sole producer.
    fn mint() -> Self {
        Self { _seal: () }
    }

    /// A receipt for tests that seed a carrier [`ProviderSyncState`] directly
    /// (bypassing a live reconcile). Test-only: production carrier commits obtain
    /// their receipt from [`reconcile_carrier_source`].
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self { _seal: () }
    }
}

/// The engine-specific membership context for a carrier sync.
///
/// `Some` ⇒ the tsserver engine: the carrier reaches tsserver as a configured-project
/// member through the on-disk publish store + plugin, so the gateway runs the
/// membership reconciliation through the [`CarrierPublishCoordinator`]. `None` ⇒ TGO
/// (no store): the gateway returns a [`CarrierSyncDecision::DirectOpen`] and the site
/// opens the companion buffers directly.
pub(crate) struct CarrierMembershipCtx<'a> {
    /// The coordinator that resolves ownership + drives the membership reconcile.
    pub coordinator: &'a CarrierPublishCoordinator,
    /// The published filesystem workspace (the membership ownership-resolution source).
    pub vfs: &'a FilesystemWorkspace,
    /// Whether the captured ownership snapshot is authoritative (vs cold-bootstrap):
    /// the reconciler's cold-vs-ready signal so a cold sync defers without thrash.
    pub ownership_ready: bool,
}

/// The inputs to one carrier-sync gateway pass.
pub(crate) struct CarrierSyncRequest<'a> {
    /// The shared host (for the public-API artifact + surface recording).
    pub host: &'a VerterHost,
    /// The published native resolver (computes the owner-aware companion paths).
    pub resolver: &'a NativeProjectResolver,
    /// The per-source provider-state map the committed transition reads.
    pub provider_sync_states: &'a DashMap<String, ProviderSyncState>,
    /// The generation-stamped provider-surface store (records published companions).
    pub provider_surfaces: &'a ProviderSurfaceStore,
    /// The document registry, when the caller has one (the spawned background tasks
    /// resolve the carrier host/VFS-only and pass `None`).
    pub documents: Option<&'a DocumentRegistry>,
    /// The carrier source canonical id.
    pub canonical_id: &'a str,
    /// Whether the compiled IDE output is JSX (drives the `.jsx` vs `.tsx` path).
    pub is_jsx: bool,
    /// The compiled IDE output, when available (the IDE companion content).
    pub ide: Option<&'a IdeResponse>,
    /// The engine membership context. `None` ⇒ TGO direct-open.
    pub membership: Option<CarrierMembershipCtx<'a>>,
    /// Why this reconcile was triggered (source edit / config change / …).
    pub reason: ReconcileReason,
}

/// The gateway's decision for one carrier-sync pass.
pub(crate) enum CarrierSyncDecision {
    /// tsserver: the carrier was advertised under its owner. Commit `committed_state`
    /// (both kinds store-resident) with the `receipt`; NO direct buffer sync.
    Published {
        /// The provider state to commit (both kinds marked background-loaded).
        committed_state: ProviderSyncState,
        /// The receipt gating the commit.
        receipt: CarrierProviderCommit,
    },
    /// TGO: no store; the site does the per-kind direct open using `transition`, then
    /// commits the result with the `receipt`.
    DirectOpen {
        /// The prepared transition (next state + stale paths) for the direct open.
        transition: ProviderSyncTransition,
        /// The receipt gating the commit.
        receipt: CarrierProviderCommit,
    },
    /// No owner resolved: the membership was retracted (authoritative) or deferred
    /// (cold) INSIDE the gateway. The site handles the provider-state — open-document
    /// liveness preserve / non-open clear. An UNRESOLVED (owner-less) carrier state is
    /// membership-free (there is no publish to forget), so committing it needs NO
    /// receipt; the receipt gates only the OWNED-publish commit (the gap-E bug class).
    Unowned,
    /// Nothing was advertised this pass (cold defer, a not-advertised reconcile, or a
    /// fail-closed reconcile error). The site keeps the file queued and commits
    /// NOTHING. Any degradation was logged inside the gateway.
    Pending,
}

impl CarrierSyncDecision {
    /// The receipt gating an OWNED carrier commit, when this decision advertised one
    /// (tsserver [`Published`](Self::Published) / TGO [`DirectOpen`](Self::DirectOpen)).
    ///
    /// `None` for [`Unowned`](Self::Unowned) (membership-free open-document liveness
    /// commits need no receipt) and [`Pending`](Self::Pending) (nothing advertised —
    /// the carrier must NOT be committed as owned). A site that drives its own
    /// per-kind buffer I/O (the interactive IDE-only / API-only paths) uses this to
    /// obtain the receipt while discarding the gateway's coarse `committed_state` /
    /// `transition`.
    pub(crate) fn into_owned_receipt(self) -> Option<CarrierProviderCommit> {
        match self {
            CarrierSyncDecision::Published { receipt, .. }
            | CarrierSyncDecision::DirectOpen { receipt, .. } => Some(receipt),
            CarrierSyncDecision::Unowned | CarrierSyncDecision::Pending => None,
        }
    }
}

/// THE single carrier-sync entry: fuse the membership decision with the
/// provider-buffer transition + receipt.
///
/// * owner resolved + tsserver ⇒ build companions, record surfaces, reconcile
///   membership; on an advertised outcome return [`CarrierSyncDecision::Published`].
/// * owner resolved + TGO ⇒ return [`CarrierSyncDecision::DirectOpen`].
/// * no owner ⇒ retract/defer the membership (tsserver) and return
///   [`CarrierSyncDecision::Unowned`].
pub(crate) async fn reconcile_carrier_source(req: CarrierSyncRequest<'_>) -> CarrierSyncDecision {
    let Some(next_state) =
        carrier_sync_state_for_source(req.resolver, req.canonical_id, req.is_jsx)
    else {
        // No owner resolved. tsserver: retract (authoritative) or defer (cold) the
        // STORE/ledger membership through the single reconciler, so a previously
        // advertised carrier whose owner is gone stops being served. The empty
        // companion set drives the reconciler to Absent (retract) / Bootstrap
        // (defer). The provider-buffer side (open-doc liveness preserve / clear) is
        // the caller's, gated by the returned receipt.
        if let Some(membership) = req.membership.as_ref() {
            if let Err(error) = membership
                .coordinator
                .reconcile_membership(
                    req.host,
                    membership.vfs,
                    req.canonical_id,
                    Vec::new(),
                    membership.ownership_ready,
                    req.reason,
                )
                .await
            {
                tracing::warn!(
                    "carrier-sync gateway: owner-loss reconcile failed for {}: {error} \
                     (external-TS degraded for this source)",
                    req.canonical_id
                );
            }
        }
        return CarrierSyncDecision::Unowned;
    };

    let Some(membership) = req.membership.as_ref() else {
        // TGO: the carrier reaches the provider as directly-opened companion buffers.
        // Return the transition for the site's per-kind open; the receipt still gates
        // the commit.
        let transition =
            prepare_sync_transition(req.provider_sync_states, req.canonical_id, next_state);
        return CarrierSyncDecision::DirectOpen {
            transition,
            receipt: CarrierProviderCommit::mint(),
        };
    };

    // tsserver: PUBLISH the companions into the store the plugin reads (the carrier
    // becomes a configured-project member), NOT a direct buffer open.
    let transition =
        prepare_sync_transition(req.provider_sync_states, req.canonical_id, next_state);
    let mut committed_state = transition.next;
    let api = block_in_place_if_available(|| req.host.get_public_api(req.canonical_id));

    // A tsserver membership publish must advertise the COMPLETE companion set: the
    // store membership REPLACES (does not merge) a source's prior companions, so an
    // api-only / ide-only caller (the imported-carrier API refresh, the deferred API
    // sync) must NOT SHRINK a carrier's advertised set. Dropping the IDE companion
    // un-resolves a plain `.ts` import of the carrier (it stops being a program
    // member). Fetch the IDE companion when the caller didn't provide it, so every
    // publish — regardless of entry point — advertises both kinds.
    let fetched_ide = if req.ide.is_none() {
        req.documents.and_then(|documents| {
            let profile = documents.tsx_profile.read().clone();
            block_in_place_if_available(|| documents.host.get_ide(req.canonical_id, &profile))
        })
    } else {
        None
    };
    let ide = req.ide.or(fetched_ide.as_ref());

    let mut companions = build_carrier_companions(&committed_state, ide, api.as_ref());
    if companions.is_empty() {
        // The owned source produced NO companion content this pass — neither an IDE
        // surface nor a public-API artifact. When ownership is AUTHORITATIVE this is a
        // genuine compile-to-nothing, so a carrier that was PREVIOUSLY advertised must
        // be RETRACTED from the store; otherwise its stale `ready_files` stay served by
        // the plugin (a separate process) indefinitely. Drive the retract through the
        // single membership reconciler with the terminal `CompileFailed` reason — an
        // empty companion set tombstones the source across every project. When
        // ownership is NOT yet authoritative (a cold bootstrap) nothing was ever
        // published, so keep the file queued and defer WITHOUT thrash (no retract).
        // IDE error-recovery normally still emits a DEGRADED-but-current (non-empty)
        // companion that PUBLISHES, so only the genuinely-empty owned case reaches here.
        if membership.ownership_ready {
            if let Err(error) = membership
                .coordinator
                .reconcile_membership(
                    req.host,
                    membership.vfs,
                    req.canonical_id,
                    Vec::new(),
                    membership.ownership_ready,
                    ReconcileReason::CompileFailed,
                )
                .await
            {
                tracing::warn!(
                    "carrier-sync gateway: compile-failed retract reconcile failed for {}: \
                     {error} (external-TS degraded for this source)",
                    req.canonical_id
                );
            }
        }
        return CarrierSyncDecision::Pending;
    }

    // Record EVERY companion surface (IDE + API) and stamp each version from its
    // freshly-recorded generation, so navigation span-classification carries both
    // roles' content/map identity AND the IDE companion's `getScriptVersion` advances
    // on edits. The single publish-time recording+versioning path.
    crate::provider_surface_store::record_and_version_carrier_companions(
        req.provider_surfaces,
        req.documents,
        req.host,
        req.canonical_id,
        &mut companions,
    );

    match membership
        .coordinator
        .reconcile_membership(
            req.host,
            membership.vfs,
            req.canonical_id,
            companions,
            membership.ownership_ready,
            req.reason,
        )
        .await
    {
        Ok(ReconcileOutcome::Advertised { .. }) => {
            // The plugin serves both companions as configured-project members, so no
            // direct provider open is pending: mark both kinds store-resident.
            if committed_state.api_path.is_some() {
                committed_state.set_background_loaded(ProviderPathKind::Api, true);
            }
            if committed_state.ide_path.is_some() {
                committed_state.set_background_loaded(ProviderPathKind::Ide, true);
            }
            CarrierSyncDecision::Published {
                committed_state,
                receipt: CarrierProviderCommit::mint(),
            }
        }
        // Not advertised (fail-closed owner-loss retract / cold-start defer): the
        // carrier is intentionally not a member now. Keep the file queued.
        Ok(_) => CarrierSyncDecision::Pending,
        Err(error) => {
            tracing::warn!(
                "carrier-sync gateway: membership reconcile failed for {}: {error} \
                 (external-TS degraded for this source)",
                req.canonical_id
            );
            CarrierSyncDecision::Pending
        }
    }
}

/// Build the carrier companion set (public-API + IDE) from the owner-resolved
/// transition's provider paths and the freshly-compiled artifacts.
fn build_carrier_companions(
    next_state: &ProviderSyncState,
    ide: Option<&IdeResponse>,
    api: Option<&verter_session::TscResponse>,
) -> Vec<CarrierCompanion> {
    let mut companions: Vec<CarrierCompanion> = Vec::new();
    if let (Some(api), Some(dts_path)) = (api, next_state.api_path.as_ref()) {
        companions.push(CarrierCompanion {
            provider_uri: Arc::from(dts_path.as_str()),
            content: Arc::clone(&api.code),
            map_json: api.source_map.clone(),
            role: SnapshotRole::CarrierApi,
            script_kind: ScriptKind::Ts,
            version: 0,
        });
    }
    if let (Some(ide), Some(ide_path)) = (ide, next_state.ide_path.as_ref()) {
        companions.push(CarrierCompanion {
            provider_uri: Arc::from(ide_path.as_str()),
            content: Arc::clone(&ide.code),
            map_json: ide.source_map.clone(),
            role: SnapshotRole::CarrierIde,
            script_kind: if ide.is_jsx {
                ScriptKind::Jsx
            } else {
                ScriptKind::Tsx
            },
            version: 0,
        });
    }
    companions
}

/// Commit a carrier [`ProviderSyncState`] — GATED on the sealed receipt.
///
/// The `_receipt` makes this uncallable without a [`CarrierProviderCommit`], which
/// only [`reconcile_carrier_source`] mints — so a carrier provider state can never be
/// committed without first running the membership decision. This is the single
/// carrier provider-state commit; non-carrier (shadow / real-file) commits keep their
/// own path ([`crate::provider_sync::commit_sync_transition`]).
pub(crate) fn commit_carrier_provider_state(
    states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    state: ProviderSyncState,
    _receipt: &CarrierProviderCommit,
) {
    commit_sync_transition(states, canonical_id, state);
}

/// The owner-resolved carrier provider state (`Owned` binding + IDE/API paths) the
/// CURRENT snapshot resolver would assign to `source_id`, or `None` when no single
/// project owns it.
///
/// PRIVATE to the gateway module: this is the carrier owner-resolution + path
/// derivation that BOTH the membership reconcile ([`reconcile_carrier_source`]) and
/// the close-only path ([`carrier_close_target`]) build on. Keeping it module-private
/// is the language-level half of the fusion — a carrier `ProviderSyncState` can only
/// be derived through the gateway, so no site can compute carrier paths + commit them
/// while forgetting the membership decision. Non-carrier (shadow) state has its own
/// [`crate::provider_sync::non_carrier_sync_state_for_source`].
fn carrier_sync_state_for_source(
    resolver: &NativeProjectResolver,
    source_id: &str,
    is_jsx: bool,
) -> Option<ProviderSyncState> {
    let owner = resolver.owner_for_file(source_id)?;
    let owner_key = owner
        .tsconfig_path
        .clone()
        .unwrap_or_else(|| owner.root.clone());
    Some(ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned(owner_key),
        ide_path: resolver.provider_ide_id_for_source(source_id, is_jsx),
        api_path: resolver.provider_id_for_source(source_id),
        shadow_path: None,
        ide_background_loaded: false,
        api_background_loaded: false,
        shadow_background_loaded: false,
    })
}

/// The carrier provider paths (IDE + API) for `canonical_id`, for the CLOSE-only path
/// (delete / file-removed / owner-loss buffer cleanup).
///
/// This computes the would-be carrier provider paths so the caller can CLOSE them; it
/// is NOT a commit and needs no receipt. The owner-resolved sync+commit path routes
/// through [`reconcile_carrier_source`] instead.
pub(crate) fn carrier_close_target(
    resolver: &NativeProjectResolver,
    canonical_id: &str,
    is_jsx: bool,
) -> Option<ProviderSyncState> {
    carrier_sync_state_for_source(resolver, canonical_id, is_jsx)
}

#[cfg(test)]
#[path = "carrier_sync_tests.rs"]
mod tests;

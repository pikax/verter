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
//! Every carrier sync resolves through [`reconcile_carrier_source`]: it captures the
//! ONE carrier-ownership resolution, computes the provider-buffer transition, builds
//! companions, runs the membership reconciliation through the single
//! [`MembershipReconciler`](super::membership_reconciler) (publish on owned,
//! retract/defer on owner loss), and returns a SEALED
//! [`ProviderReadyReceipt`](super::membership_reconciler::ProviderReadyReceipt). The
//! provider-buffer commit ([`CarrierTransactionCoordinator::admit_owned`]) REQUIRES that
//! receipt, and the receipt can only be minted from a resolved
//! [`ProjectBinding`](verter_session::external_ts::ProjectBinding) — so a carrier
//! [`ProviderSyncState`] can NOT be committed without first routing through the
//! ownership decision.
//!
//! TSGO keeps its direct carrier-open path: the gateway returns
//! [`CarrierSyncDecision::DirectOpen`] carrying the transition plus a POST-open
//! authorization, and the TSGO site opens the companion buffers itself and only then
//! mints + commits the receipt (the receipt never precedes the buffer opens). The
//! gateway governs tsserver carrier MEMBERSHIP; TSGO's direct-open stays its own
//! path.

use dashmap::DashMap;
use std::sync::Arc;

use verter_session::external_ts::{CarrierOwnershipResolution, ScriptKind, SnapshotRole};
use verter_session::{IdeResponse, VerterHost};
use verter_workspace::FilesystemWorkspace;

use crate::documents::DocumentRegistry;
use crate::external_ts::{
    resolve_carrier_ownership_over_vfs, CarrierCompanion, CarrierPublishCoordinator,
    PendingProviderReady, ProviderReadyReceipt, ReconcileOutcome, ReconcileReason,
};
use crate::project_resolver::NativeProjectResolver;
use crate::provider_surface_store::ProviderSurfaceStore;
use crate::provider_sync::{
    prepare_sync_transition, CarrierCommitStamp, ProviderOwnerBinding, ProviderPathKind,
    ProviderSyncState, ProviderSyncTransition,
};
use crate::server::block_in_place_guarded as block_in_place_if_available;

/// How the semantic provider receives carrier companions after durable editor
/// membership has been reconciled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CarrierProviderDelivery {
    /// The managed provider consumes explicit open/sync calls. This is the
    /// managed/shared tsgo route; the editor's TypeScript service independently
    /// consumes the durable store.
    DirectOpen,
    /// The semantic provider is itself backed by the durable store/plugin, so
    /// no companion buffer is opened by the LSP.
    StoreBacked,
}

/// The editor-membership context for a carrier sync, intentionally independent
/// from how the managed semantic provider receives the same companions.
pub(crate) struct CarrierMembershipCtx<'a> {
    /// The coordinator that drives the membership reconcile (the store-publish half).
    /// Ownership is resolved from the request's `vfs` (shared with tsgo), NOT here.
    pub coordinator: &'a CarrierPublishCoordinator,
    /// Provider delivery remains direct for tsgo even though the editor store is
    /// published in the same ownership transaction.
    pub provider_delivery: CarrierProviderDelivery,
}

/// The inputs to one carrier-sync gateway pass.
pub(crate) struct CarrierSyncRequest<'a> {
    /// The shared host (for the public-API artifact + surface recording).
    pub host: &'a VerterHost,
    /// The published filesystem workspace — the SINGLE carrier-ownership resolution
    /// source both engines resolve against (the scanner reads the same published
    /// snapshot), so the scanner and the sync path can never disagree. `None` (no
    /// published workspace yet) is the transient bootstrap ⇒ `NotReady`.
    pub vfs: Option<&'a FilesystemWorkspace>,
    /// Whether the captured ownership snapshot is authoritative (vs cold-bootstrap):
    /// the resolver's cold-vs-ready signal so a cold sync defers (`NotReady`) without
    /// thrash.
    pub ownership_ready: bool,
    /// The published native resolver (computes the owner-aware companion paths — path
    /// transforms only, NOT the ownership authority).
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
    /// The engine membership context. `None` ⇒ tsgo direct-open.
    pub membership: Option<CarrierMembershipCtx<'a>>,
    /// The per-source carrier transaction coordinator — the admission-token / owner-loss
    /// barrier authority. The gateway reads the source's CURRENT intent epoch from it at
    /// transaction start and stamps it onto the minted token, so the admission gate can
    /// later refuse a token minted before an intervening owner-loss.
    pub admission: &'a CarrierTransactionCoordinator,
    /// Why this reconcile was triggered (source edit / config change / …).
    pub reason: ReconcileReason,
}

/// The gateway's decision for one carrier-sync pass — derived from the ONE captured
/// [`CarrierOwnershipResolution`], so the scanner and the sync path can never disagree
/// on an ambiguous carrier.
///
/// `#[must_use]`: a bare-statement drop of a gateway decision would silently lose the owned
/// commit as well as the non-owned requeue / owner-loss barrier advance — every caller must
/// consume it (match the owned arms + settle the [`CarrierNotOwned`], or
/// [`Self::into_owned_commit_authorization`]).
#[must_use = "a CarrierSyncDecision must be consumed: commit the owned arms via admit_owned and settle the CarrierNotOwned through the coordinator"]
pub(crate) enum CarrierSyncDecision {
    /// tsserver: the carrier was advertised under its owner. Commit `committed_state`
    /// (both kinds store-resident) with the `receipt` through
    /// [`CarrierTransactionCoordinator::admit_owned`]; NO direct buffer sync.
    Published {
        /// The provider state to commit (both kinds marked background-loaded).
        committed_state: ProviderSyncState,
        /// The readiness receipt gating the commit.
        receipt: ProviderReadyReceipt,
    },
    /// tsgo: no store; the site does the per-kind direct open using `transition`, then
    /// mints the receipt from `pending` (POST-open) and commits the result through
    /// [`CarrierTransactionCoordinator::admit_owned`].
    DirectOpen {
        /// The prepared transition (next state + stale paths) for the direct open.
        transition: ProviderSyncTransition,
        /// The POST-open authorization: the site opens the companion buffers and, only
        /// on success, calls [`PendingProviderReady::confirm_opened`] to mint the
        /// commit receipt — so a tsgo receipt never precedes its buffer opens.
        pending: PendingProviderReady,
    },
    /// The carrier is NOT owned this pass (a transient bootstrap defer, a terminal
    /// owner-loss, or a fail-closed advertise miss). The disposition — requeue the
    /// transient, advance the owner-loss barrier for the terminal — is OWNED by the
    /// coordinator: the carried [`CarrierNotOwned`] is opaque and `#[must_use]`, so a
    /// site can neither discard the requeue nor read the reason to route it itself. It
    /// is settled through [`CarrierTransactionCoordinator::settle`] (the non-owned arm's
    /// dropped-outcome closure — the primary shape; broader requeue-effectiveness is
    /// review-audited pending the carrier-sync-concurrency hardening block).
    NotOwned(CarrierNotOwned),
}

impl CarrierSyncDecision {
    /// The POST-open commit authorization for an OWNED carrier
    /// (tsserver [`Published`](Self::Published) / tsgo [`DirectOpen`](Self::DirectOpen)),
    /// or the opaque [`CarrierNotOwned`] the caller MUST hand to
    /// [`CarrierTransactionCoordinator::settle`].
    ///
    /// A site that drives its own per-kind buffer I/O (the interactive IDE-only / API-only
    /// paths) uses this to obtain the authorization while discarding the gateway's coarse
    /// `committed_state` / `transition`, then calls [`OwnedCommitAuthorization::confirm`]
    /// AFTER its opens to mint the receipt. The `Err(CarrierNotOwned)` arm cannot be
    /// dropped (it is `#[must_use]`), so the requeue / owner-loss barrier advance is never
    /// silently lost.
    pub(crate) fn into_owned_commit_authorization(
        self,
    ) -> Result<OwnedCommitAuthorization, CarrierNotOwned> {
        match self {
            CarrierSyncDecision::Published { receipt, .. } => {
                Ok(OwnedCommitAuthorization::Ready(receipt))
            }
            CarrierSyncDecision::DirectOpen { pending, .. } => {
                Ok(OwnedCommitAuthorization::PendingDirectOpen(pending))
            }
            CarrierSyncDecision::NotOwned(not_owned) => Err(not_owned),
        }
    }
}

/// The reason a carrier-sync pass produced no owned advertisement. Private — a site never
/// reads it; only [`CarrierTransactionCoordinator::settle`] interprets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotOwnedReason {
    /// Ownership is not yet authoritative (`NotReady` bootstrap): a TRANSIENT state, the
    /// sole retryable owner-loss state — the coordinator requeues it. tsserver membership
    /// was deferred WITHOUT thrash (no retract).
    NotReady,
    /// Ownership is authoritative but the carrier has NO usable owner — `NoProject` /
    /// `Ambiguous`. TERMINAL: the gateway retracted any prior membership; the coordinator
    /// advances the owner-loss barrier and settles terminal (never re-queued). The
    /// user-visible `verter(project)` diagnostic is published separately from the same
    /// resolution (see [`project_ownership_diagnostic`]).
    Unresolved,
    /// Nothing was advertised this pass (compile-to-nothing, a not-advertised reconcile, a
    /// fail-closed reconcile error, or a FAILED terminal retract): keep the file queued and
    /// commit nothing. The coordinator requeues it.
    Pending,
}

/// A NON-OWNED carrier-sync outcome whose disposition is owned by the coordinator.
///
/// `#[must_use]` + a private reason: a site can neither discard it (the requeue / owner-loss
/// barrier advance would be lost — the F3/F4 dropped-outcome class) nor read the reason to
/// route it with its own hand-rolled requeue. The ONLY consumer is
/// [`CarrierTransactionCoordinator::settle`], which performs the requeue / barrier advance
/// and hands back a [`SettleClass`] the site uses for its editor-liveness buffer conversion
/// + dequeue decision.
#[must_use = "a CarrierNotOwned must be settled through CarrierTransactionCoordinator::settle so the source is requeued / the owner-loss barrier advances"]
pub(crate) struct CarrierNotOwned {
    reason: NotOwnedReason,
}

impl CarrierNotOwned {
    fn not_ready() -> Self {
        Self {
            reason: NotOwnedReason::NotReady,
        }
    }
    fn unresolved() -> Self {
        Self {
            reason: NotOwnedReason::Unresolved,
        }
    }
    /// The `Pending` non-owned outcome (nothing advertised this pass / a pre-gateway
    /// bootstrap with no published snapshot). `pub(crate)` so the thin server-side gateway
    /// wrapper can represent its no-snapshot bootstrap as a settleable non-owned outcome;
    /// the reason stays private and the value is still `#[must_use]`, so it must be settled
    /// through the coordinator.
    pub(crate) fn pending() -> Self {
        Self {
            reason: NotOwnedReason::Pending,
        }
    }
}

/// The classified disposition [`CarrierTransactionCoordinator::settle`] hands back after it
/// has already performed the requeue / owner-loss barrier advance. The site reads it ONLY to
/// drive its editor-liveness buffer conversion (preserve an open document's TSX / clear a
/// closed one) and its dequeue decision — never the requeue itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettleClass {
    /// `NotReady` bootstrap: transient, requeued by the coordinator. The site preserves an
    /// open document's TSX / clears a closed one, then keeps the carrier queued.
    NotReady,
    /// Terminal (`NoProject` / `Ambiguous`): the owner-loss barrier was advanced. The site
    /// preserves an open document's TSX / clears a closed one, then DEQUEUES (never retried).
    Unresolved,
    /// Nothing advertised (`Pending` / failed retract): requeued by the coordinator, no
    /// buffer conversion (local state preserved).
    Pending,
}

impl SettleClass {
    /// Whether a settled no-owner class runs the editor-liveness buffer conversion
    /// (preserve an open document's TSX / clear a closed one). ONLY the settled no-owner
    /// classes (`NotReady` bootstrap / terminal `Unresolved`) run it; a `Pending` (a failed
    /// retract — the stale cross-process membership is still advertised) PRESERVES local
    /// state so the source is retried, never cleared/reclassified as-if-retracted.
    #[must_use]
    pub(crate) fn runs_buffer_cleanup(self) -> bool {
        matches!(self, SettleClass::NotReady | SettleClass::Unresolved)
    }
}

/// The POST-open commit authorization an owned carrier's interactive I/O path holds
/// across its own companion opens (obtained from
/// [`CarrierSyncDecision::into_owned_commit_authorization`]).
///
/// tsserver's receipt is already minted (the store publish is the transaction); tsgo's
/// is a [`PendingProviderReady`] the site must confirm AFTER its direct opens. The site
/// calls [`Self::confirm`] at its commit point (post-open) to obtain the receipt for
/// [`CarrierTransactionCoordinator::admit_owned`] — for tsgo this is the mint, keeping the
/// receipt strictly post-open on BOTH engines.
#[must_use = "an OwnedCommitAuthorization must be confirmed after the companion opens to obtain the commit receipt"]
pub(crate) enum OwnedCommitAuthorization {
    /// tsserver: the receipt was minted at the end of the ordered store-publish
    /// transaction (`apply_owned`).
    Ready(ProviderReadyReceipt),
    /// tsgo: mint the receipt only after the site's direct companion opens succeed.
    PendingDirectOpen(PendingProviderReady),
}

impl OwnedCommitAuthorization {
    /// Obtain the commit receipt at the site's post-open commit point, attesting EXACTLY
    /// the companion kinds that ACTUALLY opened this pass (`opened_kinds`). For tsgo
    /// ([`PendingDirectOpen`](Self::PendingDirectOpen)) this MINTS the receipt (the sole
    /// tsgo mint), so it must be called only after the companion buffers have opened, and
    /// the mint attests only the opened subset (a partial open never stamps an unopened
    /// surface). tsserver ([`Ready`](Self::Ready)) already minted its receipt at the END
    /// of the ordered `apply_owned` transaction, where BOTH companions are published to
    /// the store atomically, so its attestation is complete and `opened_kinds` is not
    /// re-applied.
    /// Confirm with sealed evidence of the exact IDE bytes delivered by a
    /// successful direct provider open. Store-backed tsserver receipts were
    /// already minted from their published bytes and ignore this value.
    pub(crate) fn confirm_with_ide_surface(
        self,
        opened_kinds: &[crate::provider_sync::ProviderPathKind],
        ide_surface: Option<crate::type_provider::project_sync::SyncedTsxSurface>,
    ) -> ProviderReadyReceipt {
        match self {
            OwnedCommitAuthorization::Ready(receipt) => receipt,
            OwnedCommitAuthorization::PendingDirectOpen(pending) => {
                pending.confirm_opened_with_ide_surface(opened_kinds, ide_surface)
            }
        }
    }
}

/// Build the user-visible `verter(project)` diagnostic for an UNRESOLVED open carrier,
/// or `None` when the carrier is `Bound` / `NotReady` (a transient bootstrap state is
/// not surfaced). `NoProject` reports the absent configured project; `Ambiguous` lists
/// the candidate configs (empty for a disk-layout carrier-path conflict). Driven from
/// the typed [`CarrierOwnershipResolution`] — never a path-shape heuristic.
pub(crate) fn project_ownership_diagnostic(
    resolution: &CarrierOwnershipResolution,
) -> Option<tower_lsp_server::ls_types::Diagnostic> {
    use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, Range};
    let message = match resolution {
        CarrierOwnershipResolution::NoProject => {
            "verter: no configured TypeScript project owns this carrier — its cross-file \
             types are unavailable. Add it to a tsconfig `include`/`files` entry."
                .to_string()
        }
        // A resolution now produces `Ambiguous` ONLY for a disk-layout carrier-path
        // conflict (a real file or same-stem module occupies the generated companion
        // path) — always with EMPTY candidates. The multiply-owned case resolves to the
        // single tsgo default owner (`Bound`), so `Ambiguous` never carries candidate
        // configs and the former multi-config candidate-listing branch is unreachable.
        CarrierOwnershipResolution::Ambiguous { .. } => {
            "verter: this carrier's owning TypeScript project is ambiguous (a real file or a \
             same-stem module occupies its generated companion path), so its cross-file types \
             are unavailable."
                .to_string()
        }
        CarrierOwnershipResolution::Bound(_) | CarrierOwnershipResolution::NotReady => {
            return None;
        }
    };
    Some(Diagnostic {
        range: Range::default(),
        severity: Some(DiagnosticSeverity::WARNING),
        source: Some("verter(project)".to_string()),
        message,
        ..Default::default()
    })
}

/// The `verter(project)` ownership diagnostics for a carrier `canonical_id`,
/// resolved from the ONE shared carrier-ownership authority. Empty for a
/// non-carrier document, and — via `ObservePublishedReadiness` — for a `Bound`
/// (now including a resolved multi-claimant carrier) or `NotReady` carrier: only
/// a genuine terminal `NoProject` or a disk-layout carrier-path conflict
/// surfaces a warning.
///
/// Shared by BOTH the full-diagnostics path
/// ([`crate::server::Server::compute_full_diagnostics`]) and the debounced
/// coordinator publish path ([`crate::sync_coordinator`]), so an unresolved
/// carrier is explained on `did_open` / `did_change`, not only on a
/// full-diagnostics request. Driven from the typed [`CarrierOwnershipResolution`]
/// — never a path-shape heuristic.
pub(crate) fn project_ownership_diagnostics_for(
    host: &VerterHost,
    canonical_id: &str,
) -> Vec<tower_lsp_server::ls_types::Diagnostic> {
    if !verter_workspace::resolver::path_is_carrier(canonical_id) {
        return Vec::new();
    }
    let Some((resolution, _generation)) = crate::tsgo::project_binding::resolve_carrier(
        host,
        canonical_id,
        std::sync::Arc::from(""),
        crate::tsgo::project_binding::OwnershipReadinessMode::ObservePublishedReadiness,
    ) else {
        return Vec::new();
    };
    project_ownership_diagnostic(&resolution)
        .into_iter()
        .collect()
}

/// Resolve the carrier's ownership EXACTLY ONCE for a sync pass — the single captured
/// [`CarrierOwnershipResolution`] both the branch decision and (tsserver) the
/// membership commit consume. tsserver resolves over the coordinator's negotiated
/// version; tsgo resolves over the host's published snapshot (a bootstrap version is
/// not load-bearing for tsgo). A missing published snapshot is the transient
/// `NotReady`.
fn capture_carrier_ownership(req: &CarrierSyncRequest<'_>) -> CarrierOwnershipResolution {
    // No published workspace yet ⇒ the transient bootstrap `NotReady` (the same rule
    // `resolve_carrier_ownership_over_vfs` applies for a missing published snapshot).
    let Some(vfs) = req.vfs else {
        return CarrierOwnershipResolution::NotReady;
    };
    match req.membership.as_ref() {
        // tsserver: resolve over the coordinator's negotiated ts_version (carried onto
        // the binding for the store publish).
        Some(membership) => membership.coordinator.resolve_carrier_ownership(
            req.host,
            vfs,
            req.canonical_id,
            req.ownership_ready,
        ),
        // tsgo direct-open: the SAME vfs-backed resolution over a bootstrap ts_version
        // (not load-bearing for the direct-open binding identity / `--api` op).
        None => resolve_carrier_ownership_over_vfs(
            req.host,
            vfs,
            req.canonical_id,
            req.ownership_ready,
            Arc::from(""),
        ),
    }
}

/// The decision for a TERMINAL owner-loss reconcile (`NoProject` / `Ambiguous`),
/// classified from the store-retract result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalRetractDecision {
    /// The store tombstone SUCCEEDED — report the terminal `Unresolved` (the call site
    /// clears local state as-if-retracted).
    Tombstoned,
    /// The store tombstone ERRORED — keep the carrier queued (`Pending`) so local state
    /// is preserved and the source retries.
    RetryPending,
}

/// Classify a terminal owner-loss retract result into its sync decision.
///
/// A SUCCESSFUL tombstone authorizes the terminal `Unresolved`; an ERRORED retract must
/// fall back to `Pending` (preserve local state + retry) so a failed cross-process
/// retract never masquerades as a completed one — reporting `Unresolved` on a failed
/// retract would strand the stale membership (still served under the former project)
/// AND stop the source retrying. Pure over the `Ok`/`Err` shape so it is unit-testable
/// without a live backend.
fn classify_terminal_retract<T, E>(retract: &Result<T, E>) -> TerminalRetractDecision {
    match retract {
        Ok(_) => TerminalRetractDecision::Tombstoned,
        Err(_) => TerminalRetractDecision::RetryPending,
    }
}

/// THE single carrier-sync entry: capture the ONE carrier-ownership resolution, then
/// fuse the membership decision with the provider-buffer transition + readiness
/// receipt.
///
/// * `Bound` + tsserver ⇒ build companions, record surfaces, reconcile membership; on
///   an advertised outcome return [`CarrierSyncDecision::Published`] carrying the
///   end-of-transaction receipt.
/// * `Bound` + tsgo ⇒ return [`CarrierSyncDecision::DirectOpen`] with a POST-open
///   authorization from the resolved binding (the site mints the receipt after its
///   direct companion opens).
/// * `NotReady` ⇒ defer without thrash (tsserver) and return
///   [`CarrierSyncDecision::NotReady`] (keep-queued/retry).
/// * `NoProject` / `Ambiguous` ⇒ retract any prior membership (tsserver) and return
///   [`CarrierSyncDecision::Unresolved`] (terminal; the caller emits the
///   `verter(project)` diagnostic).
pub(crate) async fn reconcile_carrier_source(req: CarrierSyncRequest<'_>) -> CarrierSyncDecision {
    let decl_path = req.host.declaration_carrier_path(req.canonical_id);
    // Capture the source's CURRENT owner-loss barrier value ONCE at transaction start (the
    // coherent capture point for the token's local intent epoch). An owner-loss / removal
    // between here and the eventual commit advances the barrier, so the admission gate
    // refuses the token minted from this epoch — even into a vacant/re-owned slot.
    let intent_epoch = req.admission.current_intent_epoch(req.canonical_id);
    let resolution = capture_carrier_ownership(&req);

    let binding = match resolution {
        CarrierOwnershipResolution::Bound(binding) => binding,
        CarrierOwnershipResolution::NotReady => {
            // Transient bootstrap: keep the carrier QUEUED for a later retry. tsserver
            // defers the membership WITHOUT thrash (no retract) so a cold sync never
            // drops an existing advertisement.
            if let Some(membership) = req.membership.as_ref() {
                if let Err(error) = membership
                    .coordinator
                    .reconcile_membership_with_resolution(
                        req.canonical_id,
                        CarrierOwnershipResolution::NotReady,
                        Vec::new(),
                        req.reason,
                    )
                    .await
                {
                    tracing::warn!(
                        "carrier-sync gateway: bootstrap defer reconcile failed for {}: {error}",
                        req.canonical_id
                    );
                }
            }
            return CarrierSyncDecision::NotOwned(CarrierNotOwned::not_ready());
        }
        terminal @ (CarrierOwnershipResolution::NoProject
        | CarrierOwnershipResolution::Ambiguous { .. }) => {
            // Authoritative but NO usable owner: TERMINAL, fail closed. tsserver
            // retracts any prior membership so a previously-advertised carrier whose
            // owner is gone / ambiguous stops being served. The empty companion set
            // drives the reconciler to a tombstone. The caller turns the carried
            // resolution into the user-visible `verter(project)` diagnostic.
            if let Some(membership) = req.membership.as_ref() {
                let retract = membership
                    .coordinator
                    .reconcile_membership_with_resolution(
                        req.canonical_id,
                        terminal.clone(),
                        Vec::new(),
                        req.reason,
                    )
                    .await;
                if classify_terminal_retract(&retract) == TerminalRetractDecision::RetryPending {
                    // The store retract ERRORED: the stale cross-process membership still
                    // serves the carrier under its former project. Reporting the terminal
                    // `Unresolved` here would make the call site clear local state
                    // as-if-retracted AND stop retrying, stranding that stale advertisement.
                    // Return `Pending` instead — preserve local state and keep the carrier
                    // queued for a later retry (the same keep-queued contract `CompileFailed`
                    // uses on its own retract).
                    if let Err(error) = &retract {
                        tracing::warn!(
                            "carrier-sync gateway: owner-loss retract reconcile failed for {}: \
                             {error} (external-TS degraded; keeping the carrier queued for retry)",
                            req.canonical_id
                        );
                    }
                    return CarrierSyncDecision::NotOwned(CarrierNotOwned::pending());
                }
            }
            // A SUCCESSFUL tombstone (or tsgo — no membership store to retract) authorizes
            // the terminal `Unresolved`.
            return CarrierSyncDecision::NotOwned(CarrierNotOwned::unresolved());
        }
    };

    // Owned. Build the owner-resolved provider state — the owner key is the resolved
    // tsconfig URI; the IDE/API paths are owner-independent path transforms.
    let owner_key = binding.tsconfig_uri().to_string();
    let next_state = carrier_owned_sync_state(
        req.resolver,
        req.canonical_id,
        req.is_jsx,
        decl_path,
        owner_key,
    );

    let Some(membership) = req.membership.as_ref() else {
        // tsgo: the carrier reaches the provider as directly-opened companion buffers.
        // Build the companion fingerprints from the freshly-compiled artifacts so the
        // readiness receipt can attest them, then return the transition for the site's
        // per-kind open plus a POST-open authorization. The receipt is NOT minted here:
        // the site opens the companions and calls `confirm_opened` on success, so a tsgo
        // receipt never precedes its buffer opens.
        let transition =
            prepare_sync_transition(req.provider_sync_states, req.canonical_id, next_state);
        let api = match block_in_place_if_available(|| req.host.get_public_api(req.canonical_id)) {
            Ok(api) => api,
            Err(error) => {
                crate::report_public_api_projection_error(
                    "carrier_sync.direct_open",
                    req.canonical_id,
                    &error,
                );
                return CarrierSyncDecision::NotOwned(CarrierNotOwned::pending());
            }
        };
        let companions = build_carrier_companions(&transition.next, req.ide, api.as_ref());
        // The source revision is the carrier source's AUTHORITATIVE per-canonical content
        // freshness rail (the workspace's `last_content_transition_generation`), captured
        // at OPEN time — a content edit advances it, so a prepare-then-open transaction
        // that a newer edit supersedes carries an OLDER revision than the newer pass and
        // is refused by the admission gate's compare-and-swap. (tsgo companion versions are
        // recorded post-open, so they are not a stable open-time revision.)
        let source_revision = carrier_source_revision(req.host, req.canonical_id);
        let pending = PendingProviderReady::authorize(
            &binding,
            source_revision,
            intent_epoch,
            "tsgo",
            &companions,
        );
        return CarrierSyncDecision::DirectOpen {
            transition,
            pending,
        };
    };

    // Publish the companions into the editor-owned store independently of how
    // the semantic provider consumes them. Managed tsgo continues with explicit
    // buffer opens after this durable transaction; tsserver consumes the store.
    let transition =
        prepare_sync_transition(req.provider_sync_states, req.canonical_id, next_state);
    let mut committed_state = transition.next.clone();
    let api = match block_in_place_if_available(|| req.host.get_public_api(req.canonical_id)) {
        Ok(api) => api,
        Err(error) => {
            crate::report_public_api_projection_error(
                "carrier_sync.publish",
                req.canonical_id,
                &error,
            );
            return CarrierSyncDecision::NotOwned(CarrierNotOwned::pending());
        }
    };

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
        // The owned source produced NO companion content this pass — a genuine
        // compile-to-nothing (ownership is AUTHORITATIVE here, since `Bound` only comes
        // from an authoritative resolution). A carrier that was PREVIOUSLY advertised
        // must be RETRACTED from the store; otherwise its stale `ready_files` stay
        // served by the plugin (a separate process) indefinitely. Drive the retract
        // through the single membership reconciler with the terminal `CompileFailed`
        // reason — an empty companion set tombstones the source across every project.
        // IDE error-recovery normally still emits a DEGRADED-but-current (non-empty)
        // companion that PUBLISHES, so only the genuinely-empty owned case reaches here.
        if let Err(error) = membership
            .coordinator
            .reconcile_membership_with_resolution(
                req.canonical_id,
                CarrierOwnershipResolution::Bound(binding.clone()),
                Vec::new(),
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
        return CarrierSyncDecision::NotOwned(CarrierNotOwned::pending());
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
        .reconcile_membership_with_resolution(
            req.canonical_id,
            CarrierOwnershipResolution::Bound(binding.clone()),
            companions.clone(),
            req.reason,
        )
        .await
    {
        Ok(ReconcileOutcome::Advertised { receipt, .. }) => {
            match membership.provider_delivery {
                CarrierProviderDelivery::StoreBacked => {
                    // The semantic provider consumes both store-resident
                    // companions, so no direct provider I/O is pending.
                    if committed_state.api_path.is_some() {
                        committed_state.set_background_loaded(ProviderPathKind::Api, true);
                    }
                    if committed_state.ide_path.is_some() {
                        committed_state.set_background_loaded(ProviderPathKind::Ide, true);
                    }
                    CarrierSyncDecision::Published {
                        committed_state,
                        receipt: receipt.stamped_with_intent_epoch(intent_epoch),
                    }
                }
                CarrierProviderDelivery::DirectOpen => {
                    // Store publication is complete, but the managed tsgo actor
                    // still requires direct companion buffers. Its independent
                    // receipt is minted only after those opens succeed and
                    // attests the exact provider-specialized IDE bytes.
                    let source_revision = carrier_source_revision(req.host, req.canonical_id);
                    let pending = PendingProviderReady::authorize(
                        &binding,
                        source_revision,
                        intent_epoch,
                        "tsgo",
                        &companions,
                    );
                    CarrierSyncDecision::DirectOpen {
                        transition,
                        pending,
                    }
                }
            }
        }
        // Not advertised (fail-closed reconcile): the carrier is intentionally not a
        // member now. Keep the file queued.
        Ok(_) => CarrierSyncDecision::NotOwned(CarrierNotOwned::pending()),
        Err(error) => {
            tracing::warn!(
                "carrier-sync gateway: membership reconcile failed for {}: {error} \
                 (external-TS degraded for this source)",
                req.canonical_id
            );
            CarrierSyncDecision::NotOwned(CarrierNotOwned::pending())
        }
    }
}

/// The carrier source's per-canonical content revision captured at OPEN time — the
/// workspace's AUTHORITATIVE `last_content_transition_generation` freshness rail, the
/// `source_revision` a tsgo readiness receipt attests and the admission gate's
/// compare-and-swap orders on. A content edit advances it, so a stale prepare-then-open
/// transaction carries an OLDER revision than a newer pass and is refused. `0` when no
/// published workspace is available (the transient bootstrap, where ownership resolves
/// `NotReady` and no owned commit is minted).
fn carrier_source_revision(host: &VerterHost, canonical_id: &str) -> u64 {
    host.last_content_transition_generation(canonical_id)
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

/// The per-source admission barrier the [`CarrierTransactionCoordinator`] maintains OUTSIDE
/// the (removable) [`ProviderSyncState`].
///
/// The barrier is the owner-loss tombstone: it survives a state removal / owned→unresolved
/// conversion (which the `ProviderSyncState` map does not), so a late owned token cannot
/// admit into a vacant or re-owned slot. Its `intent_epoch` advances monotonically on every
/// owner-loss / removal for the source; a transaction captures the epoch at start and the
/// admission gate refuses a token whose epoch no longer matches the current barrier.
#[derive(Debug, Clone, Copy, Default)]
struct CarrierAdmissionBarrier {
    /// Monotonic per-source owner-loss counter — advanced on every terminal owner-loss /
    /// removal / owned→unresolved conversion. A token captured before the loss carries an
    /// older epoch and is refused.
    intent_epoch: u64,
}

/// The outcome of an admission through [`CarrierTransactionCoordinator::admit_owned`].
///
/// `#[must_use]`: a `Superseded` outcome REQUIRES the caller to requeue the source for a
/// fresh transaction (the committed state was NOT overwritten), so it can never be silently
/// dropped.
#[must_use = "a Superseded admission means the commit was refused; the source must be requeued for a fresh transaction"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmitOutcome {
    /// The state + surface stamp were committed at the receipt's identity.
    Admitted,
    /// The receipt was refused — a cross-owner receipt, an older generation/revision, an
    /// equal generation/revision carrying a DIFFERENT artifact, or a token captured before
    /// an intervening owner-loss. NO state/stamp overwrite; requeue for a fresh transaction.
    Superseded,
}

/// The SINGLE per-source carrier transaction coordinator: the receipt-gated owned-state
/// installer for the PRIMARY carrier-sync paths (the receipt-gated commit + the
/// receipt-attested IDE surface stamp), the owner-loss BARRIER (the tombstone that survives
/// a state removal), and the non-owned RETRY DISPOSITION (requeue / owner-loss barrier
/// advance).
///
/// The primary carrier provider-state commits route through [`Self::admit_owned`]; the
/// non-owned gateway outcomes route through [`Self::settle`]; terminal owner-loss / removal
/// advances the barrier through [`Self::advance_barrier`] (directly or via `settle`). The
/// coordinator is the choke-point the call-site architecture guard AUDITS — it flags the
/// PRIMARY raw-commit / dropped-disposition shapes, not every bypass shape (a named or
/// discarded `AdmitOutcome`, a consumed `Superseded` that omits its requeue, and generic
/// struct-literal installs on non-primary paths stay REVIEW-AUDITED pending the dedicated
/// carrier-sync-concurrency hardening block). The local receipt fence DOES reject the simple
/// stale same-owner / older-generation commit; the full carrier-sync admission concurrency is
/// not yet closed.
///
/// The struct is `pub` only to satisfy the crate-internal `pub` sync/scanner config
/// structs that hold it; its constructor and every operation are `pub(crate)`, so it is
/// never usefully constructible or callable outside this crate.
#[derive(Debug, Default)]
pub struct CarrierTransactionCoordinator {
    /// The per-source owner-loss barriers (the tombstones). Never removed — a per-source
    /// `u64`, bounded by the workspace's carrier count — so a removed source's barrier
    /// survives to refuse a late token (removing then re-inserting the barrier would lose
    /// the tombstone).
    barriers: DashMap<String, CarrierAdmissionBarrier>,
}

impl CarrierTransactionCoordinator {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The source's CURRENT owner-loss barrier value — the local intent epoch a starting
    /// transaction stamps onto its token (see [`ProviderReadyReceipt::intent_epoch`]).
    #[must_use]
    pub(crate) fn current_intent_epoch(&self, source: &str) -> u64 {
        self.barriers
            .get(source)
            .map(|b| b.intent_epoch)
            .unwrap_or(0)
    }

    /// Advance the source's owner-loss barrier — called on every terminal owner-loss,
    /// removal, or owned→unresolved conversion. A token captured before this advance
    /// carries an older epoch and is refused by [`Self::admit_owned`], even into a vacant or
    /// re-owned slot.
    pub(crate) fn advance_barrier(&self, source: &str) {
        let mut barrier = self.barriers.entry(source.to_string()).or_default();
        barrier.intent_epoch = barrier.intent_epoch.saturating_add(1);
    }

    /// Advance the owner-loss barrier BEFORE removing a source's provider state — the
    /// advance-before-mutate removal primitive for every terminal removal / owner-loss
    /// cleanup site. A previously-committed carrier state (one carrying a
    /// [`CarrierCommitStamp`]) advances the barrier so a late owned token (captured before
    /// this removal) can never resurrect the obsolete owner into the vacated slot; a
    /// non-carrier / uncommitted state is removed without a spurious advance.
    ///
    /// The barrier shard-entry guard is held across the peek + advance + remove (the SAME
    /// barrier→states nesting [`Self::admit_owned`] uses; [`Self::advance_barrier`] is
    /// barrier-only), so an `admit_owned` for this source cannot observe the pre-advance
    /// epoch and slip into the slot between the advance and the removal. No `.await` is
    /// held across the guard. Returns the removed state (if any).
    ///
    /// This is a LOCAL protection: the peek + advance + remove critical section is atomic, but
    /// it does not close the pervasive detached-mutate-across-await pattern at the removal call
    /// sites (a caller may have read state across an `.await` before invoking this) — a
    /// coherent per-source async transaction model is deferred to the carrier-sync-concurrency
    /// hardening block.
    pub(crate) fn advance_barrier_and_remove(
        &self,
        states: &DashMap<String, ProviderSyncState>,
        source: &str,
    ) -> Option<ProviderSyncState> {
        let mut barrier = self.barriers.entry(source.to_string()).or_default();
        // Decide the advance from the LIVE state under the barrier guard: `admit_owned`
        // takes the same barrier entry first, so no carrier commit can interleave between
        // this peek and the removal below. The peek `Ref` is dropped before the remove
        // (its write on the same states shard would otherwise deadlock against a held read).
        let is_committed_carrier = states.get(source).is_some_and(|s| s.commit_stamp.is_some());
        if is_committed_carrier {
            barrier.intent_epoch = barrier.intent_epoch.saturating_add(1);
        }
        crate::provider_sync::remove_sync_state(states, source)
    }

    /// Convert a reused provider state to owner-UNRESOLVED for a membership-free
    /// editor-liveness / bootstrap commit: advance the owner-loss barrier when the state
    /// was a previously-committed carrier (so a late owned token captured before this
    /// conversion cannot resurrect the obsolete owner), then CLEAR the receipt-attested
    /// admission token ([`ProviderSyncState::commit_stamp`] /
    /// [`ProviderSyncState::committed_ide_surface`]) — which only an OWNED receipt-gated
    /// commit may carry — and force the binding to [`ProviderOwnerBinding::Unresolved`].
    /// Advance-before-mutate: the barrier advances before the token is cleared. The barrier is
    /// a PARTIAL protection — it fences the late owned token, but the caller holds this
    /// `&mut ProviderSyncState` in the pervasive detached-mutate-across-await pattern (a
    /// coherent per-source async transaction model is deferred to the carrier-sync-concurrency
    /// hardening block). This is the owned→unresolved token-clearing path the call-site guard
    /// audits (the primary assignment shape only), not a statically-closed one.
    pub(crate) fn convert_to_unresolved(&self, source: &str, state: &mut ProviderSyncState) {
        if state.commit_stamp.is_some() {
            self.advance_barrier(source);
        }
        state.owner_binding = ProviderOwnerBinding::Unresolved;
        state.commit_stamp = None;
        state.committed_ide_surface = None;
    }

    /// THE carrier provider-state admission gate — the sole RECEIPT-GATED owned-state
    /// installer of the committed IDE-surface stamp ([`CommittedCarrierIdeSurface`]) and the
    /// commit stamp ([`CarrierCommitStamp`]) for the PRIMARY carrier-sync paths. It is NOT
    /// the sole mutator of the whole [`ProviderSyncState`]: the declaration-overlay lifecycle
    /// mutates the `Decl` kind outside this gate (it must never touch the IDE stamp / commit
    /// stamp — the tracked decl-overlay exemption); non-carrier (shadow / unresolved
    /// editor-liveness) commits keep [`crate::provider_sync::commit_sync_transition`]. The
    /// declaration-overlay and the deferred async-transaction paths are hardened separately in
    /// the dedicated carrier-sync-concurrency hardening block; this gate closes the simple
    /// stale same-owner / older-generation commit, not the full carrier-sync admission
    /// concurrency.
    ///
    /// The `receipt` makes this uncallable without a [`ProviderReadyReceipt`], minted ONLY
    /// from a resolved [`ProjectBinding`](verter_session::external_ts::ProjectBinding). It
    /// then validates the receipt against the CURRENT live state under ONE atomic critical
    /// section (the barrier shard entry guard, held across the state install; barrier→states
    /// lock order — [`Self::advance_barrier`] is barrier-only — so no lock-ordering hazard,
    /// no `.await` inside), refusing on any mismatch:
    ///
    /// - OWNER: the state's owner key must equal the receipt's owning tsconfig (a stale /
    ///   cross-owner receipt is refused).
    /// - INTENT EPOCH (the owner-loss tombstone): the receipt's captured epoch must equal the
    ///   source's CURRENT barrier. An owner-loss / removal between capture and commit advances
    ///   the barrier, so a token minted before it is refused — even into a VACANT or re-owned
    ///   slot (the vacant-resurrection fence; the barrier lives outside the removable state).
    /// - GENERATION + REVISION: a receipt STRICTLY OLDER than the committed
    ///   [`CarrierCommitStamp`] is refused (the prepare-then-open supersession fence).
    /// - EQUAL-KEY ARTIFACT: at an EQUAL generation/revision a commit is idempotent ONLY when
    ///   it reproduces the identical committed artifact (the same receipt-attested IDE
    ///   surface). An equal-key commit carrying a DIFFERENT artifact is refused, so a
    ///   torn/superseded production sharing a revision can never overwrite the committed
    ///   surface. That refusal also PROVES the revision rail under-counted (two genuine
    ///   artifacts share one key — e.g. same source bytes compiled under a changed context,
    ///   or an ingress that resynced without advancing the rail), so the gate RECORDS a
    ///   content transition for the source through the workspace authority before returning:
    ///   the caller's requeue then mints a strictly-newer key and admits the live artifact
    ///   instead of livelocking on the equal key until an unrelated edit advances the rail
    ///   (the previously tracked known limitation — the interactive definition/hover
    ///   divergence under edit churn where a stale committed mapper was served while the
    ///   provider text was already current).
    ///
    /// Returns [`AdmitOutcome::Superseded`] (never overwriting) on any refusal; the caller
    /// must requeue. The primary interactive paths requeue on `Superseded`; the
    /// scanner/background requeue targets a one-shot drain — a known limitation tracked for
    /// the carrier-sync-concurrency hardening block.
    pub(crate) fn admit_owned(
        &self,
        host: &VerterHost,
        states: &DashMap<String, ProviderSyncState>,
        source: &str,
        mut state: ProviderSyncState,
        receipt: &ProviderReadyReceipt,
    ) -> AdmitOutcome {
        // OWNER admission (reads the receipt/state only — no map access).
        if let Some(owner_key) = state.owner_binding.owner_key() {
            let attested = receipt.binding().tsconfig_uri();
            if owner_key != attested {
                tracing::warn!(
                    "carrier provider-state commit refused for {source}: readiness receipt \
                     attests owner {attested} but the state is owned by {owner_key} (stale or \
                     cross-owner receipt) — not committing",
                );
                return AdmitOutcome::Superseded;
            }
        }

        // Hold the barrier shard entry guard across the epoch check AND the state install so
        // an owner-loss barrier advance cannot interleave between them. `advance_barrier`
        // takes only this barrier guard, and no path takes states-then-barrier, so the
        // barrier→states nesting is deadlock-free. No `.await` is held across it.
        let barrier = self.barriers.entry(source.to_string()).or_default();
        if receipt.intent_epoch() != barrier.intent_epoch {
            tracing::warn!(
                "carrier provider-state commit refused for {source}: readiness receipt was \
                 captured at intent epoch {} but an owner-loss/removal advanced the barrier to \
                 {} — refusing (would resurrect an obsolete owner into a vacant/re-owned slot)",
                receipt.intent_epoch(),
                barrier.intent_epoch,
            );
            return AdmitOutcome::Superseded;
        }

        let incoming = CarrierCommitStamp {
            ownership_generation: receipt.project_generation(),
            source_revision: receipt.source_revision(),
        };

        use dashmap::mapref::entry::Entry;
        match states.entry(source.to_string()) {
            Entry::Occupied(mut occupied) => {
                let current_stamp = occupied.get().commit_stamp;
                let prior_ide_surface = occupied.get().committed_ide_surface.clone();
                // Whether this commit keeps the SAME committed IDE path. The equal-key
                // idempotency check below applies ONLY to a SAME-PATH production: a genuine
                // path change (a jsx↔tsx flip) is a legitimate rebind, and the source revision
                // orders it. A flip requires a source edit, which advances the per-source
                // content revision (`notify_upsert` → `bump_content_generation_for`), so in the
                // rev-reliable case a flip is STRICTLY NEWER and admits via the branch above —
                // an equal (gen, rev) then necessarily reproduces the same content and therefore
                // the same path, so a differing path at an equal key never arises. (Only a
                // content-decoupled revision — no published vfs — can produce a same-key
                // differing path, and that is an unordered rebind, not a torn same-path
                // production; refusing it would drop a live path change. The reverted
                // "refuse same-key differing path" tightening is tracked as a follow-up fork.)
                let same_ide_path = occupied.get().ide_path == state.ide_path;
                // The IDE surface this commit would install — read the prior from the SAME
                // held entry (never a second `states.get`, which would deadlock under the
                // entry lock).
                let next_ide_surface =
                    committed_ide_surface_for_commit(Some(occupied.get()), &state, receipt);
                if let Some(current) = current_stamp {
                    if incoming.is_stale_against(&current) {
                        tracing::warn!(
                            "carrier provider-state commit refused for {source}: readiness \
                             receipt is stale (generation {:?}, revision {}) against the committed \
                             state (generation {:?}, revision {}) — a newer transaction already \
                             committed; not overwriting",
                            incoming.ownership_generation,
                            incoming.source_revision,
                            current.ownership_generation,
                            current.source_revision,
                        );
                        return AdmitOutcome::Superseded;
                    }
                    // Equal generation/revision at the SAME path is idempotent ONLY for the
                    // identical surface: a same-path DIFFERENT artifact is a torn/superseded
                    // production sharing a source revision and is refused, so it can never
                    // overwrite the committed surface. The conflict also proves the source's
                    // freshness rail under-counted (two genuine artifacts share one key), so
                    // the rail is advanced through the workspace authority — the caller's
                    // requeue mints a strictly-newer key and admits the live artifact instead
                    // of livelocking on the equal key.
                    if incoming.is_same_key(&current)
                        && same_ide_path
                        && next_ide_surface != prior_ide_surface
                    {
                        tracing::warn!(
                            "carrier provider-state commit refused for {source}: readiness \
                             receipt carries the committed generation/revision (generation {:?}, \
                             revision {}) but a DIFFERENT artifact at the SAME IDE path — an \
                             equal-key commit is idempotent only for the identical surface; not \
                             overwriting (recorded a content transition to heal the rail)",
                            incoming.ownership_generation,
                            incoming.source_revision,
                        );
                        host.record_content_transition(source);
                        return AdmitOutcome::Superseded;
                    }
                }
                state.committed_ide_surface = next_ide_surface;
                state.commit_stamp = Some(incoming);
                occupied.insert(state);
            }
            Entry::Vacant(vacant) => {
                state.committed_ide_surface =
                    committed_ide_surface_for_commit(None, &state, receipt);
                state.commit_stamp = Some(incoming);
                vacant.insert(state);
            }
        }
        AdmitOutcome::Admitted
    }

    /// Finalize a NON-OWNED gateway outcome — the SOLE consumer of the opaque
    /// [`CarrierNotOwned`]. Performs the disposition a site can neither drop nor route
    /// itself: a transient (`NotReady` / `Pending`) is REQUEUED (into `requeue` when the
    /// site tracks a pending set), and a terminal (`Unresolved`) ADVANCES the owner-loss
    /// barrier. Returns the [`SettleClass`] the site uses ONLY for its editor-liveness
    /// buffer conversion + dequeue decision (never the requeue itself).
    pub(crate) fn settle(
        &self,
        not_owned: CarrierNotOwned,
        source: &str,
        requeue: Option<&dashmap::DashSet<String>>,
    ) -> SettleClass {
        match not_owned.reason {
            NotOwnedReason::NotReady => {
                if let Some(set) = requeue {
                    set.insert(source.to_string());
                }
                SettleClass::NotReady
            }
            NotOwnedReason::Unresolved => {
                // Terminal owner-loss: advance the barrier so a late owned token (captured
                // before this loss) can never resurrect the obsolete owner. An INTERACTIVE
                // caller (one that tracks a pending set) keeps the source queued so an OPEN
                // unowned document is re-reconciled once a future config change resolves an
                // owner; the background drain passes `None` and instead DEQUEUES a terminal
                // via its `SyncOutcome::Terminal` (a settled terminal is never retried there).
                self.advance_barrier(source);
                if let Some(set) = requeue {
                    set.insert(source.to_string());
                }
                SettleClass::Unresolved
            }
            NotOwnedReason::Pending => {
                if let Some(set) = requeue {
                    set.insert(source.to_string());
                }
                SettleClass::Pending
            }
        }
    }
}

/// The committed IDE-surface stamp to install on `state` for this receipt-gated commit,
/// given the `prior` committed state (read from the SAME held map entry — never a second
/// `states.get`, which under the gate's entry lock would deadlock).
///
/// When the receipt attests a `CarrierIde` companion at the state's committed `ide_path`
/// (the normal publish / direct-open where the IDE buffer opened — it (re)published the
/// IDE surface), the stamp is that companion's content/map identity. When it does NOT (an
/// api-only refresh, OR a partial tsgo open whose IDE buffer FAILED, so the receipt
/// attests no IDE companion for this path), the PRIOR committed stamp is preserved iff the
/// live `ide_path` is unchanged — so an earlier successful IDE publish's identity is not
/// lost and the fail-closed capture keeps rejecting a newer uncommitted surface. `None`
/// when the state carries no IDE path.
fn committed_ide_surface_for_commit(
    prior: Option<&ProviderSyncState>,
    state: &ProviderSyncState,
    receipt: &ProviderReadyReceipt,
) -> Option<crate::provider_sync::CommittedCarrierIdeSurface> {
    let ide_path = state.ide_path.as_deref()?;
    // This commit (re)published the IDE surface at the committed path ⇒ stamp its
    // receipt-attested content/map identity (the exact bytes the provider serves).
    if let Some(stamp) = receipt
        .companions()
        .iter()
        .find(|companion| {
            companion.role == SnapshotRole::CarrierIde && companion.uri.as_ref() == ide_path
        })
        .map(
            |companion| crate::provider_sync::CommittedCarrierIdeSurface {
                content_hash: companion.content_hash,
                map_hash: companion.map_hash,
            },
        )
    {
        return Some(stamp);
    }
    // This commit did not re-advertise the IDE surface at `ide_path` (an api-only refresh,
    // or a partial open where the IDE buffer failed): preserve the prior committed IDE
    // stamp iff the live path is unchanged.
    let prior = prior?;
    if prior.ide_path.as_deref() == Some(ide_path) {
        prior.committed_ide_surface.clone()
    } else {
        None
    }
}

/// The owner-resolved carrier provider state for an OWNED carrier: the `Owned` binding
/// (keyed by the resolved tsconfig URI) plus the owner-INDEPENDENT IDE/API/decl path
/// transforms. Built only after the ONE captured [`CarrierOwnershipResolution`]
/// resolved to `Bound`, so a carrier `ProviderSyncState` is never derived while
/// bypassing the ownership decision. Non-carrier (shadow) state has its own
/// [`crate::provider_sync::non_carrier_sync_state_for_source`].
fn carrier_owned_sync_state(
    resolver: &NativeProjectResolver,
    source_id: &str,
    is_jsx: bool,
    decl_path: Option<String>,
    owner_key: String,
) -> ProviderSyncState {
    ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned(owner_key),
        ide_path: resolver.provider_ide_id_for_source(source_id, is_jsx),
        api_path: resolver.provider_id_for_source(source_id),
        // The consumer-facing declaration companion (`.d.<ext>.ts`) — supplied by
        // the host's framework-descriptor lookup. `None` for a carrier whose
        // adapter projects no declaration carrier.
        decl_path,
        shadow_path: None,
        ide_background_loaded: false,
        api_background_loaded: false,
        decl_background_loaded: false,
        shadow_background_loaded: false,
        // Stamped by `commit_carrier_provider_state` from the receipt at commit time.
        committed_ide_surface: None,
        commit_stamp: None,
    }
}

/// The carrier provider paths (IDE + API) for `canonical_id`, for the CLOSE-only path
/// (delete / file-removed / owner-loss buffer cleanup).
///
/// This computes the would-be carrier provider paths so the caller can CLOSE them; it
/// is NOT a commit and needs no receipt. The paths are owner-INDEPENDENT (pure path
/// transforms), so a carrier's buffers can be closed regardless of its ownership state
/// (e.g. after an owner loss). Returns `None` only for a non-carrier path (no IDE
/// companion). The owner-resolved sync+commit path routes through
/// [`reconcile_carrier_source`] instead.
pub(crate) fn carrier_close_target(
    resolver: &NativeProjectResolver,
    canonical_id: &str,
    is_jsx: bool,
    decl_path: Option<String>,
) -> Option<ProviderSyncState> {
    // `provider_ide_id_for_source` is `None` for a non-carrier path — the single
    // carrier-vs-not gate for this close target.
    let ide_path = resolver.provider_ide_id_for_source(canonical_id, is_jsx)?;
    Some(ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Unresolved,
        ide_path: Some(ide_path),
        api_path: resolver.provider_id_for_source(canonical_id),
        decl_path,
        shadow_path: None,
        ide_background_loaded: false,
        api_background_loaded: false,
        decl_background_loaded: false,
        shadow_background_loaded: false,
        committed_ide_surface: None,
        commit_stamp: None,
    })
}

#[cfg(test)]
#[path = "carrier_sync_tests.rs"]
mod tests;

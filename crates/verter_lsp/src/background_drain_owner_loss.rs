//! Owner-loss / unowned-carrier reconciliation for the background sync drain.
//!
//! A `#[path]` child module of the drain (`background_drain`); it shares the parent's
//! imports through `use super::*`, so the same `super::` / `crate::` names resolve and it
//! reads the parent's private drain primitives directly. It owns the two owner-loss
//! reconcile entry points reached when a carrier's cheap owner projection shows
//! unresolved: the buffer-side preserve-open / remove-closed handling, and the
//! gateway-routed retract/defer that classifies the resulting [`CarrierApplyOutcome`].

use super::*;

/// Buffer-side owner-loss handling (NO membership): preserve an OPEN document's
/// unresolved TSX, or drop a CLOSED document's stale state + provider paths (only
/// once `ownership_ready`). The STORE/ledger membership retract is the SINGLE
/// carrier-sync gateway's job (it resolves owner-absent → retract/defer); this is
/// the provider-buffer half its no-owner (`NotReady` / `Unresolved`) decision
/// hands back to the caller.
#[allow(clippy::too_many_arguments)]
pub(super) async fn reconcile_unowned_carrier_buffer(
    sync: &ProjectSync,
    documents: &DocumentRegistry,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    ide: Option<&verter_session::IdeResponse>,
    ownership_ready: bool,
    context: &str,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
) {
    if documents.canonical_id_to_uri(canonical_id).is_some() {
        // Open document: keep its TSX live as unresolved open-document state.
        // `is_jsx` is derived from the compiled IDE output (false when absent —
        // a transient compile miss preserves the prior path regardless).
        let is_jsx = ide.map(|output| output.is_jsx).unwrap_or(false);
        sync_open_unresolved_carrier_provider_file(
            sync,
            documents,
            provider_sync_states,
            canonical_id,
            is_jsx,
            ide,
            carrier_coordinator,
        )
        .await;
        return;
    }
    if ownership_ready {
        remove_provider_sync_state_and_close_paths(
            sync,
            documents.provider_surfaces(),
            provider_sync_states,
            canonical_id,
            context,
            carrier_coordinator,
        )
        .await;
    }
}

/// Pre-compile owner-loss reconciliation for a `.vue` file reached during a
/// background sync pass (the cheap owner projection already shows unresolved, so
/// the compile is skipped). Routes the membership RETRACT (authoritative no-owner)
/// / DEFER (bootstrap) through the SINGLE carrier-sync gateway, then runs the
/// buffer-side handling — but ONLY for a settled no-owner decision.
///
/// This caller is part of the reconcile-decision WHOLE-CLASS `Pending` contract: a
/// `Pending` from a failed retract PRESERVES local state (the buffer cleanup is SKIPPED)
/// and is retried, never cleared as-if-retracted (see
/// [`owner_loss_reconcile_runs_buffer_cleanup`]). Returns the classified
/// [`CarrierApplyOutcome`] (mirroring [`apply_owner_resolved_carrier_sync`]).
#[allow(clippy::too_many_arguments)]
pub(super) async fn reconcile_unowned_carrier_provider_file(
    sync: &ProjectSync,
    documents: &DocumentRegistry,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    snapshot: &crate::server::PublishedResolverSnapshot,
    canonical_id: &str,
    ide: Option<&verter_session::IdeResponse>,
    context: &str,
    carrier_publish: Option<&CarrierPublishCtx<'_>>,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
) -> CarrierApplyOutcome {
    // Owner-absent ⇒ route the membership RETRACT/DEFER through the gateway; it
    // resolves the empty companion set to Absent (retract) / Bootstrap (defer) and
    // returns a no-owner outcome (`Unresolved` terminal / `NotReady` transient), or
    // `Pending` when the store retract FAILED.
    let is_jsx = ide.map(|output| output.is_jsx).unwrap_or(false);
    let membership = carrier_publish
        .and_then(|publish| publish.coordinator)
        .map(|coordinator| crate::external_ts::CarrierMembershipCtx { coordinator });
    let decision =
        crate::external_ts::reconcile_carrier_source(crate::external_ts::CarrierSyncRequest {
            host: documents.host(),
            vfs: carrier_publish.map(|publish| publish.vfs.as_ref()),
            ownership_ready: carrier_publish.is_some_and(|publish| publish.ownership_ready),
            resolver: &snapshot.resolver,
            provider_sync_states,
            provider_surfaces: documents.provider_surfaces(),
            documents: Some(documents),
            canonical_id,
            is_jsx,
            ide,
            membership,
            admission: carrier_coordinator,
            reason: crate::external_ts::ReconcileReason::SourceSynced,
        })
        .await;
    match decision {
        // Settle the non-owned disposition through the coordinator — NEVER dropped, so a
        // failed retract (`Pending`) is always requeued (via the drain's keep-queued
        // outcome) rather than cleared as-if-retracted, and a terminal `Unresolved`
        // advances the owner-loss barrier. The buffer-side preserve-open / remove-closed
        // handling runs ONLY for a settled no-owner class (`NotReady` / `Unresolved`); a
        // `Pending` PRESERVES local state (cleanup skipped) so the failed retract is retried.
        crate::external_ts::CarrierSyncDecision::NotOwned(not_owned) => {
            match carrier_coordinator.settle(not_owned, canonical_id, None) {
                crate::external_ts::SettleClass::NotReady => {
                    reconcile_unowned_carrier_buffer(
                        sync,
                        documents,
                        provider_sync_states,
                        canonical_id,
                        ide,
                        snapshot.ownership_ready,
                        context,
                        carrier_coordinator,
                    )
                    .await;
                    CarrierApplyOutcome::NotReady
                }
                crate::external_ts::SettleClass::Unresolved => {
                    reconcile_unowned_carrier_buffer(
                        sync,
                        documents,
                        provider_sync_states,
                        canonical_id,
                        ide,
                        snapshot.ownership_ready,
                        context,
                        carrier_coordinator,
                    )
                    .await;
                    CarrierApplyOutcome::Unresolved
                }
                crate::external_ts::SettleClass::Pending => CarrierApplyOutcome::Pending,
            }
        }
        // The authoritative resolver resolved an OWNER (disagreeing with the cheap
        // projection that steered the file here): local state is PRESERVED and the source
        // retries; any owned commit is driven by the owner-resolved path, not this helper.
        // The owned arms' minted receipt / pending are intentionally not committed here.
        crate::external_ts::CarrierSyncDecision::Published { .. }
        | crate::external_ts::CarrierSyncDecision::DirectOpen { .. } => {
            CarrierApplyOutcome::Pending
        }
    }
}

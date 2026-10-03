//! Snapshot-driven provider-sync drain lifecycle.
//!
//! After `background_init` publishes the workspace snapshot, open and imported
//! files queued during the pre-snapshot bootstrap window are reconciled against
//! the resolved owners here: the drain syncs each file's IDE/API artifacts into
//! the type provider, upgrades unresolved open-document state to owner-aware
//! state, and retires genuinely-stale provider paths (close-after-sync).
//!
//! Split out of `background_init` (a sibling `#[path]` child module of
//! `server`); both share `use super::*;` so the same `super::` / `crate::`
//! paths resolve. This module owns the editor-liveness invariant for the drain:
//! an OPEN Vue document's provider state is preserved (never closed) while
//! ownership is unresolved, and a failed owner transition leaves the previous
//! open path alive.

use super::*;

#[path = "background_drain_owner_loss.rs"]
mod owner_loss;
use crate::provider_sync::close_stale_provider_paths_with;
use crate::sync_coordinator::ProjectSyncRedelivery;
use owner_loss::{reconcile_unowned_carrier_buffer, reconcile_unowned_carrier_provider_file};

/// Outcome of a single pending-file provider-sync pass, used by the drain loop
/// to decide whether to DEQUEUE the file or KEEP it for a later retry.
///
/// A pass may sync multiple kinds (IDE `.tsx` + API `.vue.ts`) independently;
/// per-kind partial-failure is real (one kind syncs, another fails and reverts).
/// Removing the file from the pending set whenever *any* kind synced would
/// permanently suppress the failed kind — it would never be retried. The drain
/// therefore dequeues only on [`SyncOutcome::FullyReconciled`] (every kind
/// synced) or [`SyncOutcome::Terminal`] (a settled terminal no-owner state) — see
/// [`sync_outcome_dequeues`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SyncOutcome {
    /// Every intended kind synced this pass (or there was nothing to sync and
    /// the committed state is already consistent). Safe to dequeue.
    FullyReconciled,
    /// The carrier settled into a TERMINAL no-owner state (`NoProject` /
    /// `Ambiguous`). Safe to dequeue: retrying a terminal ownership decision
    /// would only re-run the same fail-closed result. A later config change that
    /// resolves an owner is re-driven by that change's own reconcile, not by a
    /// stale drain retry.
    Terminal,
    /// At least one kind synced but at least one OTHER intended kind FAILED.
    /// Keep the file queued so the failed kind is retried on a later drain.
    Partial,
    /// Nothing synced this pass — a total sync failure, a transient advertise
    /// miss (`Pending`), or a still-transient `NotReady` bootstrap awaiting an
    /// authoritative owner. The sole RETRYABLE outcomes; keep queued.
    Nothing,
}

/// Whether a drain [`SyncOutcome`] should DEQUEUE the carrier from the pending set.
///
/// Dequeue on [`SyncOutcome::FullyReconciled`] (every kind synced) and
/// [`SyncOutcome::Terminal`] (a settled terminal no-owner state — never retried).
/// [`SyncOutcome::Partial`] and [`SyncOutcome::Nothing`] stay queued so a failed
/// kind or a still-transient (`NotReady` / `Pending`) carrier is retried.
pub(super) fn sync_outcome_dequeues(outcome: SyncOutcome) -> bool {
    matches!(
        outcome,
        SyncOutcome::FullyReconciled | SyncOutcome::Terminal
    )
}

/// The live carrier-publish context threaded into the drain for the tsserver
/// engine. When present, a carrier's companions are PUBLISHED into the on-disk
/// store the `@verter/typescript-plugin` reads (making the carrier a configured-
/// project member) INSTEAD of being opened directly into the provider via
/// `provider.open_file`. `None` for tsgo (whose carrier companions reach the
/// engine through the project-bound `--api` direct open — `open_project` +
/// `root_files`) and for unit tests that assert the mock provider's open/sync calls.
pub(super) struct CarrierPublishCtx<'a> {
    /// The tsserver publish coordinator (drives the store-publish membership), or
    /// `None` for tsgo direct-open. Ownership is resolved from `vfs` for BOTH engines.
    pub(super) coordinator: Option<&'a crate::external_ts::CarrierPublishCoordinator>,
    /// The managed semantic provider's delivery leg is independent from the
    /// editor membership store.
    pub(super) provider_delivery: crate::external_ts::CarrierProviderDelivery,
    /// The published filesystem workspace — the SINGLE ownership-resolution source for
    /// both engines.
    pub(super) vfs: Arc<verter_workspace::FilesystemWorkspace>,
    /// Whether the captured ownership snapshot is authoritative (vs cold-bootstrap):
    /// the reconciler's cold-vs-ready signal so a cold drain defers without thrash.
    pub(super) ownership_ready: bool,
}

/// The shared state one pending-sync drain pass needs, held behind `Arc`s so
/// a bounded successor pass can outlive the caller that armed it. The fields
/// are the same shared owners the server, background init, and the scanner
/// drain through — nothing here is drain-private state. The chain budget
/// (single-flight + exhaustion) lives on the shared
/// [`crate::external_ts::CarrierTransactionCoordinator`], so every context of
/// one server bounds the SAME queue no matter which site built it.
pub(crate) struct PendingSyncDrain {
    pub(crate) project_sync: Option<ProjectSync>,
    pub(crate) documents: Arc<DocumentRegistry>,
    pub(crate) vfs_workspace:
        Arc<parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>>,
    pub(crate) provider_sync_states: Arc<DashMap<String, ProviderSyncState>>,
    pub(crate) pending_snapshot_provider_sync: Arc<DashSet<String>>,
    pub(crate) is_tsgo: bool,
    pub(crate) mru_canonical_ids: Option<Arc<parking_lot::Mutex<Vec<String>>>>,
    pub(crate) carrier_publish_coordinator: Option<crate::external_ts::CarrierPublishCoordinator>,
    pub(crate) carrier_transaction_coordinator:
        Arc<crate::external_ts::CarrierTransactionCoordinator>,
}

/// The production redrive schedule: a pass that leaves entries queued arms one
/// successor pass after a backoff delay, up to `max_attempts` generations.
///
/// A transiently refused carrier sync (the provider is between epochs after a
/// replacement, a commit was superseded, compilation was transiently cold) has
/// no later external drain after startup's last scanner pass — the queue's
/// documented contract is "retried on a later drain", so the drain itself must
/// arm that later pass while entries remain. The chain is BOUNDED: it stops at
/// the attempt cap or an empty queue, whichever comes first. When it stops at
/// the cap with entries still queued, their attempt budget is exhausted — a
/// mere coordinator wake must not restart a chain for them (the same refused
/// entry would be retried for the rest of the session); only a RETRY SIGNAL
/// re-arms: an external drain pass (a publication, a scanner sweep — the
/// inputs may have been repaired), or an engine start ([`signal_pending_sync_redrive`]
/// consumers). Queued ids that are NOT in the exhausted cohort — work that
/// arrived after the last exhaustion — stay eligible for every chain.
pub(crate) const PENDING_SYNC_REDRIVE: PendingSyncRedrive = PendingSyncRedrive {
    initial_delay_ms: 500,
    backoff_factor: 2,
    max_attempts: 6,
};

/// Knobs of the bounded successor-pass chain. The delay before successor
/// `attempt` (2-based) is `initial_delay_ms * backoff_factor^(attempt-2)`.
#[derive(Clone, Copy)]
pub(crate) struct PendingSyncRedrive {
    pub(crate) initial_delay_ms: u64,
    pub(crate) backoff_factor: u32,
    pub(crate) max_attempts: u32,
}

impl PendingSyncRedrive {
    fn delay_for_successor(&self, attempt: u32) -> std::time::Duration {
        let scale = self
            .backoff_factor
            .saturating_pow(attempt.saturating_sub(2));
        std::time::Duration::from_millis(self.initial_delay_ms.saturating_mul(scale as u64))
    }
}

/// Drain the pending snapshot queue through the shared pass, then — because
/// this call IS a retry signal (an external drain ran: a publication, a
/// scanner sweep) — re-arm the bounded successor chain for anything the pass
/// left queued.
pub(crate) async fn drain_pending_snapshot_provider_sync_owned(
    drain: Arc<PendingSyncDrain>,
    redrive: PendingSyncRedrive,
    attempt: u32,
) {
    drain_pending_snapshot_provider_sync(
        drain.project_sync.as_ref(),
        &drain.documents,
        &drain.vfs_workspace,
        &drain.provider_sync_states,
        &drain.pending_snapshot_provider_sync,
        drain.is_tsgo,
        drain.mru_canonical_ids.as_deref(),
        drain.carrier_publish_coordinator.as_ref(),
        &drain.carrier_transaction_coordinator,
    )
    .await;
    if attempt >= redrive.max_attempts || drain.pending_snapshot_provider_sync.is_empty() {
        return;
    }
    signal_pending_sync_redrive(&drain, redrive);
}

/// The RETRY SIGNAL for the pending-snapshot re-drive chain: the inputs an
/// exhausted entry was refused on may have been repaired (a publication, a
/// scanner sweep, an engine that (re)started serving), so the exhaustion
/// budget is cleared and a fresh bounded chain armed. Distinct from
/// [`arm_pending_sync_redrive_once`], which respects an armed chain and an
/// unspent budget alike.
pub(crate) fn signal_pending_sync_redrive(
    drain: &Arc<PendingSyncDrain>,
    redrive: PendingSyncRedrive,
) {
    drain
        .carrier_transaction_coordinator
        .pending_redrive_clear_exhausted();
    arm_pending_sync_redrive_once(drain, redrive);
}

/// Arm this server's bounded successor chain unless one is already running
/// (the shared single-flight guard) or every queued entry has exhausted its
/// attempt budget. The chain re-runs the shared drain pass after a backoff
/// delay while entries remain; when it ends at the attempt cap with entries
/// still queued it records them as exhausted (unless a retry signal landed
/// mid-chain — the chain then yields the baton to the signal's fresh chain
/// instead), and when it ends on an empty queue or a later signal re-arms it,
/// the guard is released for the next chain.
pub(crate) fn arm_pending_sync_redrive_once(
    drain: &Arc<PendingSyncDrain>,
    redrive: PendingSyncRedrive,
) {
    if !drain
        .carrier_transaction_coordinator
        .pending_redrive_try_arm()
    {
        return;
    }
    let generation = drain
        .carrier_transaction_coordinator
        .pending_redrive_signal_generation();
    if !drain
        .carrier_transaction_coordinator
        .pending_redrive_eligible(&drain.pending_snapshot_provider_sync)
    {
        // No chain: every queued entry already spent its budget. The next
        // retry signal (an external drain pass or an engine start) clears
        // the budget and re-arms.
        drain
            .carrier_transaction_coordinator
            .pending_redrive_disarm();
        // A retry signal that landed between the eligibility check and the
        // disarm bumped the generation after it was captured above, and its
        // own arm stood down on this one's guard — without this re-arm its
        // cleared budget would be silently dropped and the parked entries
        // would wait for the next signal.
        if drain
            .carrier_transaction_coordinator
            .pending_redrive_signal_generation()
            != generation
        {
            arm_pending_sync_redrive_once(drain, redrive);
        }
        return;
    }
    let successor = Arc::clone(drain);
    tokio::spawn(async move {
        let mut attempt = 1u32;
        // The retry-signal generation as of the START of the latest pass. It
        // is captured immediately BEFORE each pass and never refreshed after
        // one: a signal landing while a pass runs (an engine start repairing
        // the inputs the pass is failing on) bumps the generation after this
        // capture, so the end-of-chain comparison still sees it even though
        // the pass itself ran on pre-repair inputs.
        let mut pass_generation = successor
            .carrier_transaction_coordinator
            .pending_redrive_signal_generation();
        let mut exhausted = false;
        loop {
            tokio::time::sleep(redrive.delay_for_successor(attempt + 1)).await;
            // An external drain (scanner pass, background init) may have
            // emptied the queue while this successor slept; the re-check keeps
            // the chain from issuing pointless provider work.
            if successor.pending_snapshot_provider_sync.is_empty() {
                break;
            }
            pass_generation = successor
                .carrier_transaction_coordinator
                .pending_redrive_signal_generation();
            drain_pending_snapshot_provider_sync(
                successor.project_sync.as_ref(),
                &successor.documents,
                &successor.vfs_workspace,
                &successor.provider_sync_states,
                &successor.pending_snapshot_provider_sync,
                successor.is_tsgo,
                successor.mru_canonical_ids.as_deref(),
                successor.carrier_publish_coordinator.as_ref(),
                &successor.carrier_transaction_coordinator,
            )
            .await;
            if attempt + 1 >= redrive.max_attempts {
                exhausted = !successor.pending_snapshot_provider_sync.is_empty();
                break;
            }
            if successor.pending_snapshot_provider_sync.is_empty() {
                break;
            }
            attempt += 1;
        }
        if exhausted {
            let still_queued: Vec<String> = successor
                .pending_snapshot_provider_sync
                .iter()
                .map(|entry| entry.key().clone())
                .collect();
            successor
                .carrier_transaction_coordinator
                .pending_redrive_record_exhausted(still_queued);
        }
        // Release the single-flight guard BEFORE the final signal check: a
        // retry signal that landed anywhere since this chain's last pass
        // STARTED — mid-pass, at the attempt cap, or between the exhaustion
        // record and this disarm — stood down on this chain's guard. Replaying
        // its clear+arm hands the cleared budget the fresh chain it asked for;
        // the replay is idempotent for a signal that instead landed after the
        // disarm, whose own arm then holds the guard and makes the replay's
        // arm stand down.
        successor
            .carrier_transaction_coordinator
            .pending_redrive_disarm();
        if successor
            .carrier_transaction_coordinator
            .pending_redrive_signal_generation()
            != pass_generation
        {
            signal_pending_sync_redrive(&successor, redrive);
        }
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn drain_pending_snapshot_provider_sync(
    project_sync: Option<&ProjectSync>,
    documents: &DocumentRegistry,
    vfs_workspace: &parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    pending_snapshot_provider_sync: &DashSet<String>,
    is_tsgo: bool,
    mru_canonical_ids: Option<&parking_lot::Mutex<Vec<String>>>,
    carrier_publish_coordinator: Option<&crate::external_ts::CarrierPublishCoordinator>,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
) {
    if project_sync.is_none() && carrier_publish_coordinator.is_none() {
        pending_snapshot_provider_sync.clear();
        carrier_coordinator.note_pending_sync_progress();
        return;
    }
    // Capture the published filesystem workspace once (the carrier-publish
    // ownership-resolution source) alongside the resolver snapshot, so the
    // tsserver publish path resolves against the same published snapshot.
    let (snapshot, vfs_handle) = {
        let ws = vfs_workspace.read();
        let Some(ws) = ws.as_ref() else {
            return;
        };
        let Some(snapshot) = published_resolver_snapshot(ws) else {
            return;
        };
        (snapshot, Arc::clone(ws))
    };
    // The carrier-publish context — ALWAYS present here (a published `vfs_handle` was
    // captured above), carrying the single ownership-resolution vfs for BOTH engines.
    // Its `coordinator` is `None` for tsgo (project-bound `--api` direct
    // carrier-companion open) and when no coordinator is wired.
    let carrier_publish = CarrierPublishCtx {
        coordinator: carrier_publish_coordinator,
        provider_delivery: if is_tsgo {
            crate::external_ts::CarrierProviderDelivery::DirectOpen
        } else {
            crate::external_ts::CarrierProviderDelivery::StoreBacked
        },
        vfs: Arc::clone(&vfs_handle),
        ownership_ready: snapshot.ownership_ready,
    };

    // Collect pending IDs and sort by MRU order
    let pending_ids: Vec<String> = {
        let all_pending: Vec<String> = pending_snapshot_provider_sync
            .iter()
            .map(|entry| entry.key().clone())
            .collect();

        if let Some(mru_lock) = mru_canonical_ids {
            let mru = mru_lock.lock();
            let mut ordered = Vec::with_capacity(all_pending.len());
            // MRU files first
            for mru_id in mru.iter() {
                if all_pending.contains(mru_id) {
                    ordered.push(mru_id.clone());
                }
            }
            // Then remaining files not in MRU
            for id in &all_pending {
                if !ordered.contains(id) {
                    ordered.push(id.clone());
                }
            }
            ordered
        } else {
            all_pending
        }
    };

    for canonical_id in pending_ids {
        // ONE lane per open document, shared with the interactive repair and the
        // coordinator. Asked here, at the point of delivery, so a drain pass that
        // started while the document was closed and finds it open now serializes
        // on the live transaction. A busy lane YIELDS: the id stays queued (the
        // dequeue below is never reached), and the pass moves on to the next
        // document instead of blocking behind an interactive repair.
        let outcome = sync_pending_snapshot_provider_file(
            project_sync,
            documents,
            &snapshot,
            provider_sync_states,
            &canonical_id,
            Some(&carrier_publish),
            carrier_coordinator,
            is_tsgo,
        )
        .await;

        // Dequeue when the file is fully reconciled (every intended kind synced),
        // when it settled into a TERMINAL no-owner state (never retried), or when
        // its source has vanished. A `Partial` outcome (a kind failed and was
        // reverted to its prior live path) MUST stay queued so the failed kind is
        // retried on a later drain — otherwise it is permanently suppressed.
        // `Nothing` (total failure / transient `NotReady` / `Pending`) also stays.
        if sync_outcome_dequeues(outcome) || documents.host().get_source(&canonical_id).is_none() {
            pending_snapshot_provider_sync.remove(&canonical_id);
        }
        // The pass above advanced this carrier's diagnostics generation before
        // re-syncing it. An open document whose last publication that outdated
        // — a complete receipt, or an owed one that landed incomplete because
        // this very commit had not happened yet — is re-armed here, since no
        // publication is in flight to notice and no editor signal follows. A
        // TERMINAL settle (no usable provider membership: no owner, or the
        // owning project excludes the generated units) owes the SAME re-arm:
        // native analysis remains available and the `verter(project)`
        // diagnostic for an unresolved owner is published from the same pass —
        // without it, an open excluded carrier would never see ANY
        // publishDiagnostics and level-2 clients wait forever.
        if matches!(
            outcome,
            SyncOutcome::FullyReconciled | SyncOutcome::Terminal
        ) {
            documents.refresh_owed_diagnostics(&canonical_id);
        }
    }
    carrier_coordinator.note_pending_sync_progress();
}

/// Re-resolve aliased imports for all currently open `.vue` files and sync any
/// newly-discovered imported `.vue.ts` files to the type provider.
///
/// During `did_open`, aliased imports (e.g., `@/components/MyComp.vue`) fail to
/// resolve because `project_registry` is `None` — it's populated later by
/// `background_init`. This function runs **after** the registry is committed and
/// re-runs the same import-collection pipeline, so the provider gets the missing
/// `.vue.ts` files before the E2E diagnostic check.
#[allow(clippy::too_many_arguments)]
pub(super) async fn resync_aliased_imports_for_open_files(
    documents: &DocumentRegistry,
    project_sync: Option<&ProjectSync>,
    vfs_workspace: &parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    is_tsgo: bool,
    carrier_publish_coordinator: Option<&crate::external_ts::CarrierPublishCoordinator>,
    decl_overlay_owner: &DeclOverlayOwner,
    pass_generation: u64,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
) -> bool {
    let Some(sync) = project_sync else {
        return false;
    };
    let (snapshot, vfs_handle) = {
        let ws = vfs_workspace.read();
        match ws.as_ref().and_then(|ws| {
            let published = ws.load_published()?;
            Some((
                super::PublishedResolverSnapshot {
                    resolver: published.snapshot.resolver.clone(),
                    resolution_view: Some(super::PublishedResolutionView {
                        workspace: Arc::clone(ws),
                        published: Arc::clone(&published),
                    }),
                    ownership_ready: published.ownership_ready,
                },
                Arc::clone(ws),
            ))
        }) {
            Some((snapshot, vfs)) => (Some(snapshot), Some(vfs)),
            None => (None, None),
        }
    };
    // The carrier-publish context for the aliased-import carrier sync — present
    // whenever a published `vfs` was captured (the single ownership-resolution source);
    // its `coordinator` is `None` for tsgo.
    let carrier_publish = vfs_handle.map(|vfs| CarrierPublishCtx {
        coordinator: carrier_publish_coordinator,
        provider_delivery: if is_tsgo {
            crate::external_ts::CarrierProviderDelivery::DirectOpen
        } else {
            crate::external_ts::CarrierProviderDelivery::StoreBacked
        },
        vfs,
        ownership_ready: snapshot
            .as_ref()
            .map(|s| s.ownership_ready)
            .unwrap_or(false),
    });
    let snapshot = match snapshot {
        Some(s) => s,
        None => return false,
    };

    let host = documents.host();
    let mut synced_any = false;
    let mut all_import_ids: Vec<String> = Vec::new();
    let mut seen = HashSet::new();

    for uri_str in documents.open_uris() {
        let Ok(uri) = uri_str.parse::<Uri>() else {
            continue;
        };
        let Some(canonical_id) = documents.get_canonical_id(&uri) else {
            continue;
        };
        if carrier_language_for(&canonical_id).is_none() {
            continue;
        }
        let Some(analysis) = host.get_analysis(&canonical_id) else {
            continue;
        };

        // Static imports (same pipeline as did_open line 6103)
        let ids = match collect_imported_carrier_priority_ids_from_imports_for_publication(
            &analysis.imports,
            Some(&canonical_id),
            |parent, specifier| resolve_import_specifier_standalone(&host, parent, specifier),
        ) {
            Ok(ids) => ids,
            Err(_) => return false,
        };

        // Dynamic imports via module_references
        let reader = LspProjectResolverReader::new(documents);
        let Some(dynamic_ids) = collect_priority_carrier_public_api_targets_from_module_references(
            Some(&snapshot),
            &reader,
            &canonical_id,
            &analysis.module_references,
        ) else {
            return false;
        };

        for id in ids.into_iter().chain(dynamic_ids) {
            if seen.insert(id.clone()) {
                all_import_ids.push(id);
            }
        }
    }

    // Lightweight sync: compile and sync the provider artifacts needed by the backend.
    for import_id in &all_import_ids {
        // ONE lane per open document. A busy lane YIELDS this import for a later
        // pass rather than queueing behind an interactive transaction — a
        // background sweep must never hold up, or wait on, a user's request.
        if let Some(state) = provider_sync_states.get(import_id.as_str()) {
            let already_loaded = if is_tsgo {
                state.ide_background_loaded && state.api_background_loaded
            } else {
                state.api_background_loaded
            };
            // R2-4: only short-circuit an already-loaded import when its
            // committed owner binding STILL matches the live snapshot resolution.
            // An OPEN `.vue` whose owner changed or disappeared must NOT be
            // skipped on a stale binding — it would otherwise stay stranded on
            // the dead owner (the `no ide_context` class). For a non-open import
            // a fully-loaded binding is left as-is (closed files have no editor-
            // liveness invariant to reconcile here).
            if already_loaded {
                let is_open = documents.canonical_id_to_uri(import_id).is_some();
                let binding_current = !is_open
                    || crate::provider_sync::committed_binding_matches_current(
                        &state,
                        &crate::provider_sync::current_owner_binding_for_source(
                            &snapshot.resolver,
                            import_id,
                        ),
                    );
                if binding_current {
                    continue;
                }
            }
        }

        // Load dependency into host (also feeds the scheduler via upsert).
        // The scheduler's extract_deps + auto-ingress handles recursive
        // dependency walking, replacing the old hydrate_cached flow.
        if !host.ensure_loaded(import_id) {
            continue;
        }

        // R5-2: detect owner-None / owner-loss and reconcile the open file's
        // BINDING BEFORE the compile gate below. Owner resolution is a pure
        // resolver query (it does not need compile output), so a COMPILE FAILURE
        // must not short-circuit the reconcile and strand a previously-`Owned`
        // OPEN `.vue` on its dead owner. The reconcile preserves an open file's
        // live TSX (Unresolved binding, owner-derived `.vue.ts` dropped+closed)
        // and corrects the binding even without fresh IDE output (`ide = None`).
        if crate::provider_sync::current_owner_binding_for_source(&snapshot.resolver, import_id)
            .is_unresolved()
        {
            if matches!(
                reconcile_unowned_carrier_provider_file(
                    sync,
                    documents,
                    provider_sync_states,
                    &snapshot,
                    import_id,
                    None,
                    None,
                    "aliased_resync",
                    carrier_publish.as_ref(),
                    carrier_coordinator,
                )
                .await,
                CarrierApplyOutcome::Pending
            ) {
                return false;
            }
            continue;
        }

        // Pin the open document's exact revision BEFORE compiling, if any is
        // open — see `DocumentRegistry::open_compile_pin`. `None` for a closed
        // import (no live document to race).
        let (import_open_uri, import_ide_compile_revision) = documents.open_compile_pin(import_id);
        let import_open_pin = match (&import_open_uri, &import_ide_compile_revision) {
            (Some(uri), Some(revision)) => Some((uri, revision)),
            _ => None,
        };

        // Compile to generate public API. IDE-sync: gate on the IDE/TSX surface
        // (not the runtime `Main`) so a Main-less carrier (Svelte) — which has a
        // `CachedTsx` but no `Main` — is not skipped. `Ok(false)` (no IDE
        // surface) skips; otherwise proceed to the owner-aware provider sync.
        let profile = documents.tsx_profile.read().clone();
        if !host
            .ensure_ide_compiled(import_id, &profile)
            .unwrap_or(false)
        {
            continue;
        }

        let ide = if is_tsgo {
            host.get_ide(import_id, &profile)
        } else {
            None
        };

        // Route the owner-resolved carrier (or an owner lost mid-flight) through the
        // SINGLE carrier-sync gateway: tsserver publishes the membership, tsgo opens
        // the companions directly, and a mid-flight owner loss is reconciled inside
        // (a `NotReady` / `Unresolved` no-owner outcome). Any synced kind is progress.
        if let CarrierApplyOutcome::Applied { synced, .. } = apply_owner_resolved_carrier_sync(
            Some(sync),
            documents,
            provider_sync_states,
            &snapshot,
            import_id,
            ide.as_ref(),
            import_open_pin,
            "aliased_resync",
            carrier_publish.as_ref(),
            carrier_coordinator,
        )
        .await
        {
            if !synced.is_empty() {
                synced_any = true;
            }
        }
    }

    // Pass 2 (TSGO only): Sync barrel imports discovered from template component usages.
    // When a component is imported through a barrel (non-carrier re-export file), the
    // carrier file collection above misses both the barrel and its carrier re-export
    // targets. This pass follows the barrel → carrier re-export chain and syncs both.
    if is_tsgo {
        let mut barrel_ids: Vec<String> = Vec::new();
        let mut barrel_carrier_deps: Vec<String> = Vec::new();
        let mut seen_barrels = HashSet::new();
        let mut seen_barrel_carrier = HashSet::new();

        for uri_str in documents.open_uris() {
            let Ok(uri) = uri_str.parse::<Uri>() else {
                continue;
            };
            let Some(canonical_id) = documents.get_canonical_id(&uri) else {
                continue;
            };
            if carrier_language_for(&canonical_id).is_none() {
                continue;
            }
            let Some(analysis) = host.get_analysis(&canonical_id) else {
                continue;
            };
            let Some(template) = analysis.template.as_ref() else {
                continue;
            };

            for component in &template.components {
                let Some(import_source) = component.import_source.as_deref() else {
                    continue;
                };
                let resolved = match resolve_import_specifier_standalone(
                    &host,
                    &canonical_id,
                    import_source,
                ) {
                    verter_workspace::ResolutionPublication::Admitted(admitted) => {
                        let Some(resolved) = admitted.into_result() else {
                            continue;
                        };
                        resolved
                    }
                    verter_workspace::ResolutionPublication::Refused(_) => return false,
                };
                if verter_semantic::resolver_core::path_is_carrier(&resolved) {
                    continue; // a directly-resolved carrier is already handled by the carrier pass
                }
                if !seen_barrels.insert(resolved.clone()) {
                    continue;
                }

                // Load the barrel into the host and scan its module references
                // for carrier (`.vue`, `.svelte`, …) specifiers. This avoids the
                // chicken-and-egg problem where get_export_span_follow_reexports
                // needs carrier files already loaded.
                host.ensure_loaded(&resolved);

                if let Some(barrel_analysis) = host.get_analysis(&resolved) {
                    for module_ref in barrel_analysis.module_references.iter() {
                        if let Some(specifier) = &module_ref.literal_specifier {
                            if verter_semantic::resolver_core::path_is_carrier(specifier) {
                                let carrier_id = match resolve_import_specifier_standalone(
                                    &host, &resolved, specifier,
                                ) {
                                    verter_workspace::ResolutionPublication::Admitted(admitted) => {
                                        let Some(carrier_id) = admitted.into_result() else {
                                            continue;
                                        };
                                        carrier_id
                                    }
                                    verter_workspace::ResolutionPublication::Refused(_) => {
                                        return false;
                                    }
                                };
                                if verter_semantic::resolver_core::path_is_carrier(&carrier_id)
                                    && seen_barrel_carrier.insert(carrier_id.clone())
                                {
                                    barrel_carrier_deps.push(carrier_id);
                                }
                            }
                        }
                    }
                }

                barrel_ids.push(resolved);
            }
        }

        // Sync Vue dependencies first (so TSGO has .vue.ts targets before barrel)
        for carrier_id in &barrel_carrier_deps {
            // Skip if already synced in the main Vue pass — but only when an OPEN
            // barrel-dep `.vue`'s committed binding STILL matches the live
            // resolution (R2-4). An owner change/loss on an open barrel dep must
            // fall through to reconciliation, never short-circuit on a stale
            // binding. A non-open dep keeps its fully-loaded binding as-is.
            if let Some(state) = provider_sync_states.get(carrier_id.as_str()) {
                if state.ide_background_loaded && state.api_background_loaded {
                    let is_open = documents.canonical_id_to_uri(carrier_id).is_some();
                    let binding_current = !is_open
                        || crate::provider_sync::committed_binding_matches_current(
                            &state,
                            &crate::provider_sync::current_owner_binding_for_source(
                                &snapshot.resolver,
                                carrier_id,
                            ),
                        );
                    if binding_current {
                        continue;
                    }
                }
            }

            if !host.ensure_loaded(carrier_id) {
                continue;
            }

            // R5-2: detect owner-None / owner-loss and reconcile the open file's
            // BINDING BEFORE the compile gate below (mirrors the aliased pass).
            // Owner resolution is a pure resolver query, so a COMPILE FAILURE
            // must not strand a previously-`Owned` open barrel-dep on its dead
            // owner. The reconcile corrects the binding without fresh IDE output.
            if crate::provider_sync::current_owner_binding_for_source(
                &snapshot.resolver,
                carrier_id,
            )
            .is_unresolved()
            {
                if matches!(
                    reconcile_unowned_carrier_provider_file(
                        sync,
                        documents,
                        provider_sync_states,
                        &snapshot,
                        carrier_id,
                        None,
                        None,
                        "barrel_carrier_dep",
                        carrier_publish.as_ref(),
                        carrier_coordinator,
                    )
                    .await,
                    CarrierApplyOutcome::Pending
                ) {
                    return false;
                }
                continue;
            }

            // Pin the open document's exact revision BEFORE compiling, if any
            // is open — see `DocumentRegistry::open_compile_pin`.
            let (barrel_dep_open_uri, barrel_dep_ide_compile_revision) =
                documents.open_compile_pin(carrier_id);
            let barrel_dep_open_pin = match (&barrel_dep_open_uri, &barrel_dep_ide_compile_revision)
            {
                (Some(uri), Some(revision)) => Some((uri, revision)),
                _ => None,
            };

            // IDE-sync: gate on the IDE/TSX surface (not the runtime `Main`) so
            // a Main-less carrier (Svelte) is not skipped here.
            let profile = documents.tsx_profile.read().clone();
            if !host
                .ensure_ide_compiled(carrier_id, &profile)
                .unwrap_or(false)
            {
                continue;
            }

            let ide = host.get_ide(carrier_id, &profile);

            // Route the owner-resolved carrier (or an owner lost mid-flight) through
            // the SINGLE carrier-sync gateway. Any synced kind counts as progress.
            if let CarrierApplyOutcome::Applied { synced, .. } = apply_owner_resolved_carrier_sync(
                Some(sync),
                documents,
                provider_sync_states,
                &snapshot,
                carrier_id,
                ide.as_ref(),
                barrel_dep_open_pin,
                "barrel_carrier_dep",
                carrier_publish.as_ref(),
                carrier_coordinator,
            )
            .await
            {
                if !synced.is_empty() {
                    synced_any = true;
                }
            }
        }

        // Sync barrel files (their rewritten imports now point to .vue.ts)
        for barrel_id in &barrel_ids {
            if sync_pending_non_carrier_provider_file(
                sync,
                documents,
                &snapshot,
                provider_sync_states,
                barrel_id,
            )
            .await
            {
                synced_any = true;
            }
        }
    }

    // Pass 3 (TSGO only): proactively open the transitive DECLARATION closure.
    //
    // tsgo resolves a bare framework-carrier import (`import B from "./B.vue"`)
    // to the virtual `B.d.<ext>.ts` declaration via its native basename-append
    // probe — but tsgo has NO module-resolution hook, so every declaration an
    // importing carrier (transitively) needs must already be OPEN as an overlay
    // when that carrier is type-checked, or the import fails with TS2307. This
    // pass walks the transitive closure of carrier dependencies reachable from
    // the OPEN carrier roots and opens each one's `.d.<ext>.ts`, recording the
    // reachability so the `did_close` lifecycle can release them.
    //
    // tsserver serves carrier companions through the publish store (not direct
    // overlay opens), so the proactive overlay graph is a tsgo-only concern —
    // scoped exactly like the carrier-open passes above.
    if is_tsgo {
        synced_any |= decl_overlay_owner
            .open_declaration_closure_for_open_files(
                sync,
                documents,
                provider_sync_states,
                &snapshot,
                pass_generation,
            )
            .await;
    }

    synced_any
}

/// The resolver snapshot of `ws`'s current publication, or `None` while it
/// has published nothing.
fn published_resolver_snapshot(
    ws: &Arc<verter_workspace::FilesystemWorkspace>,
) -> Option<super::PublishedResolverSnapshot> {
    let published = ws.load_published()?;
    Some(super::PublishedResolverSnapshot {
        resolver: published.snapshot.resolver.clone(),
        resolution_view: Some(super::PublishedResolutionView {
            workspace: Arc::clone(ws),
            published: Arc::clone(&published),
        }),
        ownership_ready: published.ownership_ready,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn sync_pending_snapshot_provider_file(
    sync: Option<&ProjectSync>,
    documents: &DocumentRegistry,
    snapshot: &super::PublishedResolverSnapshot,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    carrier_publish: Option<&CarrierPublishCtx<'_>>,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
    rewrite_import_specifiers: bool,
) -> SyncOutcome {
    if carrier_language_for(canonical_id).is_some() {
        sync_pending_carrier_provider_file(
            sync,
            documents,
            snapshot,
            provider_sync_states,
            canonical_id,
            carrier_publish,
            carrier_coordinator,
        )
        .await
    } else {
        let Some(sync) = sync else {
            return SyncOutcome::Nothing;
        };
        // A script NO configured project owns (a TypeScript lib file the user
        // navigated into, a file outside every workspace root) can never be
        // delivered. Once ownership is authoritative that is a TERMINAL answer:
        // retrying it would keep the pending set non-empty — and level 2
        // unannounced — for the rest of the session. While ownership is still
        // cold the same absence is merely unknown, so it stays queued.
        if snapshot.ownership_ready
            && snapshot
                .resolver
                .nearest_config_for_path(canonical_id)
                .is_none()
        {
            return SyncOutcome::Terminal;
        }
        // An OPEN self-file document (a plain script or a rune module) is
        // delivered through the same shared shadow sync the coordinator tick and
        // the editor ingress use. It records the provider surface and the
        // position mapper that match the delivered bytes, and it rewrites import
        // specifiers only for a provider that resolves the explicit source graph
        // itself. Delivering an open plain script here with specifiers rewritten
        // for tsserver would shift every provider position in it against the
        // editor's text, so its hovers and navigation would land on the wrong
        // tokens.
        if let (Some(file_language), Some(uri)) = (
            crate::server::self_file_language_for(canonical_id),
            documents.canonical_id_to_uri(canonical_id),
        ) {
            // A re-synced buffer must look new to the diagnostics cache and to any
            // receipt the open document still owes, exactly as the non-carrier
            // pass below does, so the drain's owed-diagnostics re-arm fires.
            documents.host().bump_diagnostics_generation(canonical_id);
            // The supersession check re-derives the projection after the provider
            // await, so it must read the publication current THEN: one landing
            // during the delivery has to be able to supersede it.
            let published = || match carrier_publish {
                Some(publish) => published_resolver_snapshot(&publish.vfs),
                None => Some(snapshot.clone()),
            };
            let delivered = crate::server::sync_self_file_shadow_state(
                documents,
                sync,
                provider_sync_states,
                &published,
                &uri,
                canonical_id,
                &file_language,
                rewrite_import_specifiers,
            )
            .await;
            return if delivered {
                SyncOutcome::FullyReconciled
            } else {
                SyncOutcome::Nothing
            };
        }
        // Non-carrier files have a single Shadow kind: synced fully or not at all.
        if sync_pending_non_carrier_provider_file(
            sync,
            documents,
            snapshot,
            provider_sync_states,
            canonical_id,
        )
        .await
        {
            SyncOutcome::FullyReconciled
        } else {
            SyncOutcome::Nothing
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn sync_pending_carrier_provider_file(
    sync: Option<&ProjectSync>,
    documents: &DocumentRegistry,
    snapshot: &super::PublishedResolverSnapshot,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    carrier_publish: Option<&CarrierPublishCtx<'_>>,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
) -> SyncOutcome {
    // Ensure the file and its deps are loaded. The scheduler's extract_deps
    // + auto-ingress handles recursive dependency walking.
    documents.host().ensure_loaded(canonical_id);
    // Hydration may load new dependencies (macro type deps, external templates)
    // that affect the compilation output. Invalidate compile slots so
    // ensure_compiled recompiles, and bump diagnostics_generation so the LSP
    // cache treats the next diagnostic request as a cache miss.
    documents.host().invalidate_compile_slots(canonical_id);
    documents.host().bump_diagnostics_generation(canonical_id);
    // Pin the open document's exact revision BEFORE compiling, if any is open
    // — see `DocumentRegistry::open_compile_pin`. `None` for a closed carrier
    // (no live document to race).
    let (open_uri, ide_compile_revision) = documents.open_compile_pin(canonical_id);
    let open_pin = match (&open_uri, &ide_compile_revision) {
        (Some(uri), Some(revision)) => Some((uri, revision)),
        _ => None,
    };
    let profile = documents.tsx_profile.read().clone();
    // IDE-sync: drive the IDE/TSX surface (not the runtime `Main`) so a
    // Main-less carrier (Svelte) populates its `CachedTsx` before `get_ide`.
    let _ = block_in_place_if_available(|| {
        documents.host().ensure_ide_compiled(canonical_id, &profile)
    });
    let ide = block_in_place_if_available(|| documents.host().get_ide(canonical_id, &profile));
    // The drain's compile is the OTHER path that can recover a carrier left
    // without a provider projection (the document commit never compiles). Cache
    // read only — the compile just above already ran — and a no-op once a
    // projection exists.
    documents.install_missing_carrier_projection(canonical_id);
    // Route through the SINGLE carrier-sync gateway: tsserver PUBLISHES the carrier
    // companions into the on-disk store the plugin reads (the configured-project
    // membership), tsgo opens the companions directly, and an owner loss RETRACTS the
    // membership + preserves an open document / removes a closed one. The receipt
    // gates every commit (the gap-E bug class).
    let outcome = apply_owner_resolved_carrier_sync(
        sync,
        documents,
        provider_sync_states,
        snapshot,
        canonical_id,
        ide.as_ref(),
        open_pin,
        "pending_snapshot",
        carrier_publish,
        carrier_coordinator,
    )
    .await;
    classify_carrier_apply_outcome(outcome)
}

/// Classify a carrier apply result into the drain's dequeue decision (R2-6):
///   * every intended kind synced → `FullyReconciled` (dequeue);
///   * some synced but an intended kind FAILED → `Partial` (retry on a later
///     drain — never permanently suppress the failed kind);
///   * a TERMINAL no-owner decision (`NoProject` / `Ambiguous`) → `Terminal`
///     (dequeue — a terminal ownership state is never retried; the buffer-side
///     preserve-open / remove-closed already ran and any `verter(project)`
///     diagnostic is published separately);
///   * a still-transient `NotReady` bootstrap, a `Pending` advertise miss, or a
///     total sync failure → `Nothing` (keep queued for a later retry).
///
/// This is the SINGLE point that distinguishes the sole retryable ownership state
/// (`NotReady`) from the terminal ones — a terminal carrier must never be retried
/// into a provider on every drain.
fn classify_carrier_apply_outcome(outcome: CarrierApplyOutcome) -> SyncOutcome {
    match outcome {
        CarrierApplyOutcome::Applied { attempted, synced } => {
            if synced.is_empty() {
                SyncOutcome::Nothing
            } else if attempted.iter().all(|kind| synced.contains(kind)) {
                SyncOutcome::FullyReconciled
            } else {
                SyncOutcome::Partial
            }
        }
        // Terminal no-owner (the gateway retracted the membership; the buffer-side
        // preserve-open / remove-closed already ran): settle + dequeue, never retry.
        CarrierApplyOutcome::Unresolved => SyncOutcome::Terminal,
        // Transient bootstrap (`NotReady`) or a `Pending` advertise/compile miss:
        // keep the file queued for a future snapshot/drain that may resolve it.
        CarrierApplyOutcome::NotReady | CarrierApplyOutcome::Pending => SyncOutcome::Nothing,
    }
}

/// Preserve (or create) an OPEN Vue document's unresolved provider state when
/// no project owns it, and keep its IDE TSX live in the provider.
///
/// Editor-liveness invariant: an open Vue document must keep a usable TSX in
/// the type provider even while its owning project is unresolved or ambiguous.
/// This helper:
///   * reuses the existing committed state's IDE path when present, otherwise
///     builds local unresolved `{source}.tsx`/`.jsx` state;
///   * opens (or updates, if already background-loaded) that TSX in the
///     provider so hover/completion keep working;
///   * commits the unresolved state and leaves the file QUEUED for a future
///     snapshot to upgrade once an owner resolves.
///
/// It never closes the open document's existing paths and never removes its
/// state. Returns `false` so the drain keeps the file in the pending set for
/// later owner reconciliation.
#[allow(
    clippy::too_many_arguments,
    reason = "the open-unresolved-preserve path needs the provider-surface pin alongside its sync inputs"
)]
pub(super) async fn sync_open_unresolved_carrier_provider_file(
    sync: &ProjectSync,
    documents: &DocumentRegistry,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    is_jsx: bool,
    ide: Option<&verter_session::IdeResponse>,
    // The open-document revision `ide` was compiled against, captured by the
    // CALLER before that compile ran — see `DocumentRegistry::open_compile_pin`.
    open_pin: Option<(&Uri, &crate::documents::DocumentSnapshotIdentity)>,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
) -> bool {
    let provider_surfaces = documents.provider_surfaces();
    // Build the DESIRED Unresolved target through the shared primitive: the
    // owner-independent desired-extension IDE path + the open-vs-update
    // syncability hint (`ide_background_loaded`). The binding is forced
    // `Unresolved` and the owner-derived API path is dropped — converting a
    // prior `Owned` binding (rather than reusing it) is what lets a later
    // snapshot re-bind the file via `needs_owner_reconcile`.
    let previous = provider_sync_states.get(canonical_id).map(|e| e.clone());
    // Converting a previously-committed OWNED carrier to Unresolved is an owner-loss for
    // the admission barrier: advance it so a late owned token — captured before this
    // conversion — can never resurrect the obsolete owner into the now-unstamped slot.
    if previous
        .as_ref()
        .is_some_and(|state| state.commit_stamp.is_some())
    {
        carrier_coordinator.advance_barrier(canonical_id);
    }
    let target = open_unresolved_carrier_state(previous.as_ref(), canonical_id, is_jsx);

    let Some(ide) = ide else {
        // No compiled IDE output this pass (e.g. a transient compile miss): no
        // IDE sync is attempted, so the IDE kind did NOT go live this pass.
        //
        // Route the commit through the SAME per-kind discipline the owner-
        // resolved path uses (`open_unresolved_carrier_commit`): a non-synced IDE
        // kind RETAINS the prior LIVE path (never dropped to a dead/None path
        // while the prior is still open in the provider), the binding is forced
        // `Unresolved`, and the owner-derived API path is dropped+closed.
        //
        // With NO prior state this commits the EMPTY `Unresolved` (ide_path=None,
        // binding=Unresolved, dropped_api=None → no close) — recording the open
        // file's unresolved status (queued for retry), uniform with the two
        // `preserve_open_unresolved_carrier` callers. This commit is unconditional so
        // all three unresolved-preserve entry points share ONE row-1 behavior; an
        // open file's unresolved state is then observable regardless of which
        // path handled it, and `needs_owner_reconcile` picks it up.
        let commit = open_unresolved_carrier_commit(previous.as_ref(), target, false);
        commit_sync_transition(provider_sync_states, canonical_id, commit.committed);
        close_dropped_owner_api_path(
            sync,
            provider_surfaces,
            commit.dropped_api.as_ref(),
            "open_unresolved",
        )
        .await;
        return false;
    };
    let Some(ide_path) = target.ide_path.clone() else {
        return false;
    };

    // Attempt the desired IDE sync: update-in-place when the desired path is
    // already live (a same-extension preserve), else first-open.
    let result = if target.ide_background_loaded {
        sync.sync_tsx_fenced(&ide_path, &ide.code, &|| {
            open_pin.is_none_or(|(uri, id)| documents.snapshot_identity_is_current(uri, id))
        })
        .await
    } else {
        sync.open_tsx_fenced(&ide_path, &ide.code, &|| {
            open_pin.is_none_or(|(uri, id)| documents.snapshot_identity_is_current(uri, id))
        })
        .await
    };
    let ide_synced = match result {
        Ok(delivery) => {
            // Record a fresh generation pinning the EXACT IDE bytes just synced
            // (interactive queries capture this surface), through the shared
            // fenced choke point: `open_pin` was captured by the caller BEFORE
            // this compile, so the just-run provider sync can only make the
            // record fail closed, never falsely pair `ide.code` with a source
            // it wasn't compiled from.
            if let Some(delivery) = delivery.ide_surface() {
                let delivered = delivery.delivered();
                crate::provider_surface_store::record_carrier_ide_surface_fenced(
                    provider_surfaces,
                    Some(documents),
                    &documents.host(),
                    canonical_id,
                    &ide_path,
                    delivered,
                    ide.source_map.as_deref(),
                    open_pin,
                );
                true
            } else {
                false
            }
        }
        Err(error) => {
            tracing::warn!(
                "pending_snapshot: failed to sync open unresolved Vue IDE path {ide_path}: {error}"
            );
            false
        }
    };

    // Build the committed state + close targets through the shared per-kind
    // discipline: the IDE kind reverts to the prior live path on a failed/absent
    // sync (rows 7 & 9), the owner-derived API is dropped+closed unconditionally,
    // and the orphaned prior IDE path is closed ONLY after a successful flip.
    let commit = open_unresolved_carrier_commit(previous.as_ref(), target, ide_synced);
    commit_sync_transition(provider_sync_states, canonical_id, commit.committed);
    close_dropped_owner_api_path(
        sync,
        provider_surfaces,
        commit.dropped_api.as_ref(),
        "open_unresolved",
    )
    .await;
    if let Some(stale) = commit.stale_ide_after_success.as_ref() {
        close_stale_provider_paths_with(
            sync,
            provider_surfaces,
            &non_decl_close_targets(std::slice::from_ref(stale)),
            "open_unresolved_ext_flip",
            Some(&ProjectSyncRedelivery::new(sync)),
        )
        .await;
    }
    // Stay queued: a future snapshot with a resolved owner upgrades this state.
    false
}

/// Close the owner-derived API path dropped by an open-document owned→unowned
/// conversion (see [`crate::provider_sync::dropped_api_path_on_unowned_conversion`]).
///
/// A no-op when nothing was dropped. The helper only ever yields an
/// [`ProviderPathKind::Api`] target — the open document's IDE TSX is preserved
/// and is never closed here — so this routes through the shared leaf
/// [`close_stale_provider_paths`] dispatch.
async fn close_dropped_owner_api_path(
    sync: &ProjectSync,
    provider_surfaces: &crate::provider_surface_store::ProviderSurfaceStore,
    dropped_api: Option<&(ProviderPathKind, String)>,
    context: &str,
) {
    if let Some(dropped) = dropped_api {
        close_stale_provider_paths_with(
            sync,
            provider_surfaces,
            &non_decl_close_targets(std::slice::from_ref(dropped)),
            context,
            Some(&ProjectSyncRedelivery::new(sync)),
        )
        .await;
    }
}

/// The applied result of an owner-resolved both-kinds carrier gateway sync.
enum CarrierApplyOutcome {
    /// Committed (tsserver `Published` membership, or tsgo `DirectOpen` buffer sync).
    /// `attempted` = the kinds with a target path this pass; `synced` = those that
    /// actually landed (a tsserver publish marks both store-resident ⇒ attempted ==
    /// synced).
    Applied {
        attempted: Vec<ProviderPathKind>,
        synced: Vec<ProviderPathKind>,
    },
    /// TERMINAL no-owner (`NoProject` / `Ambiguous`): the gateway retracted the
    /// membership and the buffer-side owner-loss handling already ran. Nothing
    /// committed. Never retried — a settled terminal ownership decision.
    Unresolved,
    /// Transient bootstrap (`NotReady`): ownership is not yet authoritative. The
    /// gateway deferred without thrash and the buffer-side preserve-open ran.
    /// The sole RETRYABLE ownership state — keep queued for a later snapshot.
    NotReady,
    /// Nothing advertised this pass (cold defer / transient miss / fail-closed).
    Pending,
}

/// Route an owner-resolved (or owner-lost) `.vue`/`.svelte` carrier through the
/// SINGLE carrier-sync gateway and APPLY the result. Shared by the main drain, the
/// aliased-import resync, and the barrel Vue-dependency pass:
///   * tsserver ⇒ `Published`: the store membership serves both companions; commit
///     the both-resident state with the receipt (no buffer I/O).
///   * tsgo ⇒ `DirectOpen`: per-kind open/sync (open if not background-loaded, else
///     update), revert any failed kind to its prior live path, commit with the
///     receipt, and close only the genuinely-stale paths.
///   * owner-loss ⇒ `Unresolved` (terminal `NoProject` / `Ambiguous`) or
///     `NotReady` (transient bootstrap): the gateway retracted/deferred the
///     membership; the buffer-side preserve-open / remove-closed handling runs
///     here. They differ only in the caller's dequeue decision.
///   * cold/transient advertise miss ⇒ `Pending`.
#[allow(
    clippy::too_many_arguments,
    reason = "carrier sync needs the provider-surface store + documents alongside the sync state"
)]
async fn apply_owner_resolved_carrier_sync(
    sync: Option<&ProjectSync>,
    documents: &DocumentRegistry,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    snapshot: &super::PublishedResolverSnapshot,
    canonical_id: &str,
    ide: Option<&verter_session::IdeResponse>,
    // The open-document revision `ide` was compiled against, captured by the
    // CALLER before that compile ran (never after) — see
    // `DocumentRegistry::open_compile_pin`. Threaded down to every record call
    // this pass can reach so a mid-flight edit fails closed instead of pairing
    // fresh bytes with a stale/torn source.
    open_pin: Option<(&Uri, &crate::documents::DocumentSnapshotIdentity)>,
    context: &str,
    carrier_publish: Option<&CarrierPublishCtx<'_>>,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
) -> CarrierApplyOutcome {
    let document_lane = match documents.try_delivery_lane(canonical_id) {
        crate::document_sync_lane::DeliveryLane::Acquired(guard) => Some(guard),
        crate::document_sync_lane::DeliveryLane::Closed => None,
        crate::document_sync_lane::DeliveryLane::Busy => return CarrierApplyOutcome::Pending,
    };
    if open_pin.is_none() && documents.canonical_id_to_uri(canonical_id).is_some() {
        return CarrierApplyOutcome::Pending;
    }
    // The dialect comes from the compile, falling back to the parse-level
    // script language when the compile is unavailable — never a `.tsx` guess
    // (the `.tsx` → `.jsx` companion flip tsserver's output-file check rejects).
    let is_jsx = documents.is_jsx_for_canonical(canonical_id);
    let membership = carrier_publish
        .and_then(|publish| publish.coordinator)
        .map(|coordinator| crate::external_ts::CarrierMembershipCtx {
            coordinator,
            provider_delivery: carrier_publish
                .expect("coordinator came from the publish context")
                .provider_delivery,
            activate_provider_member: documents.canonical_id_to_uri(canonical_id).is_some(),
        });
    match crate::external_ts::reconcile_carrier_source(crate::external_ts::CarrierSyncRequest {
        host: &documents.host(),
        vfs: carrier_publish.map(|publish| publish.vfs.as_ref()),
        ownership_ready: carrier_publish.is_some_and(|publish| publish.ownership_ready),
        resolver: &snapshot.resolver,
        provider_sync_states,
        provider_surfaces: documents.provider_surfaces(),
        documents: Some(documents),
        project_sync: sync,
        canonical_id,
        is_jsx,
        ide,
        open_pin,
        membership,
        admission: carrier_coordinator,
        reason: crate::external_ts::ReconcileReason::SourceSynced,
    })
    .await
    {
        crate::external_ts::CarrierSyncDecision::Published {
            committed_state,
            receipt,
        } => {
            // The plugin serves both store-resident companions: no buffer I/O.
            let mut kinds: Vec<ProviderPathKind> = Vec::new();
            if committed_state.api_path.is_some() {
                kinds.push(ProviderPathKind::Api);
            }
            if committed_state.ide_path.is_some() {
                kinds.push(ProviderPathKind::Ide);
            }
            if carrier_coordinator.admit_owned_fenced(
                &documents.host(),
                provider_sync_states,
                canonical_id,
                committed_state,
                &receipt,
                Some(documents),
                open_pin,
            ) == crate::external_ts::AdmitOutcome::Superseded
            {
                // A newer transaction already committed (or an owner-loss advanced the
                // barrier): nothing synced this pass — keep queued for a fresh transaction.
                return CarrierApplyOutcome::Pending;
            }
            CarrierApplyOutcome::Applied {
                attempted: kinds.clone(),
                synced: kinds,
            }
        }
        crate::external_ts::CarrierSyncDecision::DirectOpen {
            transition,
            pending,
        } => {
            let Some(sync) = sync else {
                tracing::error!(
                    "{context}: direct-open carrier decision has no managed provider sync"
                );
                return CarrierApplyOutcome::Pending;
            };
            let previous_state = provider_sync_states.get(canonical_id).map(|e| e.clone());
            let stale_paths = transition.stale_paths;
            let mut committed_state = transition.next;
            let mut attempted: Vec<ProviderPathKind> = Vec::new();
            let mut synced: Vec<ProviderPathKind> = Vec::new();
            // The receipt this pass's own IDE delivery produced, carried to the
            // commit below. `None` when no IDE companion opened.
            let mut ide_delivery: Option<crate::type_provider::project_sync::SyncedTsxSurface> =
                None;

            let ide_current =
                ide.zip(committed_state.ide_path.as_deref())
                    .is_some_and(|(ide, path)| {
                        crate::provider_sync::open_ide_leg_is_current(
                            sync,
                            documents,
                            previous_state.as_ref(),
                            canonical_id,
                            path,
                            &ide.code,
                            &committed_state.owner_binding,
                        )
                    });
            if let (Some(ide), Some(ide_path)) = (ide, committed_state.ide_path.clone()) {
                attempted.push(ProviderPathKind::Ide);
                if !ide_current {
                    let result = if committed_state.ide_background_loaded {
                        sync.sync_tsx_fenced(&ide_path, &ide.code, &|| {
                            open_pin.is_none_or(|(uri, id)| {
                                documents.snapshot_identity_is_current(uri, id)
                            })
                        })
                        .await
                    } else {
                        sync.open_tsx_fenced(&ide_path, &ide.code, &|| {
                            open_pin.is_none_or(|(uri, id)| {
                                documents.snapshot_identity_is_current(uri, id)
                            })
                        })
                        .await
                    };
                    let result = match result {
                        Ok(crate::type_provider::project_sync::CarrierDelivery::Refused)
                        | Ok(crate::type_provider::project_sync::CarrierDelivery::Delivered(
                            None,
                        )) => sync.synchronize_pending_tsx(&ide_path, &ide.code).await,
                        result => result,
                    };
                    match result {
                        Ok(delivery) => {
                            // Record a fresh generation pinning the EXACT IDE bytes just
                            // synced (interactive queries capture this surface), through
                            // the shared fenced choke point: `open_pin` was captured by
                            // the caller BEFORE this compile, so this just-run provider
                            // sync can only make the record fail closed, never falsely
                            // pair `ide.code` with a source it wasn't compiled from.
                            if let Some(delivery) = delivery.ide_surface() {
                                let delivered = delivery.delivered();
                                committed_state.set_background_loaded(ProviderPathKind::Ide, true);
                                synced.push(ProviderPathKind::Ide);
                                crate::provider_surface_store::record_carrier_ide_surface_fenced(
                                    documents.provider_surfaces(),
                                    Some(documents),
                                    &documents.host(),
                                    canonical_id,
                                    &ide_path,
                                    delivered,
                                    ide.source_map.as_deref(),
                                    open_pin,
                                );
                                // The commit seals the SAME content this pass
                                // delivered and recorded, carried forward whole
                                // instead of re-read from the path's ledger.
                                ide_delivery = Some(delivery);
                            }
                        }
                        Err(error) => {
                            tracing::warn!(
                                "{context}: failed to sync provider IDE path {ide_path}: {error}"
                            );
                        }
                    }
                }
            }

            if !synced.is_empty() {
                revert_unsynced_kinds(&mut committed_state, previous_state.as_ref(), &synced);
                let genuinely_stale =
                    genuinely_stale_after_sync(&stale_paths, &committed_state, &synced);
                // A kind opened: NOW mint the receipt (post-open), attesting EXACTLY the
                // kinds that actually opened this pass, and commit through the coordinator.
                // The IDE evidence is the SAME delivery content the record above pinned,
                // carried forward — not a re-read of the provider path's ledger.
                let receipt = pending.confirm_opened_with_ide_surface(&synced, ide_delivery);
                if carrier_coordinator.admit_owned_fenced(
                    &documents.host(),
                    provider_sync_states,
                    canonical_id,
                    committed_state,
                    &receipt,
                    Some(documents),
                    open_pin,
                ) == crate::external_ts::AdmitOutcome::Superseded
                {
                    // Superseded mid-flight: treat as no progress (keep queued).
                    return CarrierApplyOutcome::Pending;
                }
                close_stale_provider_paths_with(
                    sync,
                    documents.provider_surfaces(),
                    &non_decl_close_targets(&genuinely_stale),
                    context,
                    Some(&ProjectSyncRedelivery::new(sync)),
                )
                .await;
            }
            drop(document_lane);
            attempted.push(ProviderPathKind::Api);
            let api_queue = dashmap::DashSet::new();
            if sync_carrier_api_transaction(
                sync,
                snapshot,
                documents,
                carrier_publish.map(|publish| publish.vfs.as_ref()),
                provider_sync_states,
                canonical_id,
                is_jsx,
                carrier_coordinator,
                &api_queue,
            )
            .await
            {
                synced.push(ProviderPathKind::Api);
            }
            if ide_current {
                synced.push(ProviderPathKind::Ide);
            }
            CarrierApplyOutcome::Applied { attempted, synced }
        }
        crate::external_ts::CarrierSyncDecision::NotOwned(not_owned) => {
            // Settle the non-owned disposition through the coordinator (requeue the
            // transient, advance the owner-loss barrier for the terminal), then run the
            // SAME buffer-side owner-loss handling (preserve an open document's live TSX /
            // remove a closed one) for a settled no-owner class. The dequeue decision is the
            // returned class: `NotReady` transient (keep queued), `Unresolved` terminal
            // (settle + dequeue, never retry). `Pending` commits nothing and keeps queued.
            match carrier_coordinator.settle(not_owned, canonical_id, None) {
                crate::external_ts::SettleClass::NotReady => {
                    if let Some(sync) = sync {
                        reconcile_unowned_carrier_buffer(
                            sync,
                            documents,
                            provider_sync_states,
                            canonical_id,
                            ide,
                            open_pin,
                            snapshot.ownership_ready,
                            context,
                            carrier_coordinator,
                        )
                        .await;
                    }
                    CarrierApplyOutcome::NotReady
                }
                crate::external_ts::SettleClass::Unresolved => {
                    if let Some(sync) = sync {
                        reconcile_unowned_carrier_buffer(
                            sync,
                            documents,
                            provider_sync_states,
                            canonical_id,
                            ide,
                            open_pin,
                            snapshot.ownership_ready,
                            context,
                            carrier_coordinator,
                        )
                        .await;
                    }
                    CarrierApplyOutcome::Unresolved
                }
                // Both non-advertising classes keep the carrier queued and commit nothing;
                // `RetractFailed` additionally means the cross-process store may still
                // advertise it, so it is never treated as a settled disposition.
                crate::external_ts::SettleClass::Pending
                | crate::external_ts::SettleClass::RetractFailed => CarrierApplyOutcome::Pending,
            }
        }
    }
}

/// Background API-only (`.vue.ts`) provider sync for a `.vue` file.
///
/// This is the awaitable body spawned by
/// `VerterLanguageServer::sync_api_to_provider_in_background`. It manages ONLY
/// the API (`Api`) kind and routes through the shared
/// close-after-successful-sync discipline with `synced_kinds = [Api]`:
///   * sync the NEW API path first (open if not yet background-loaded, else
///     update);
///   * on success, [`revert_unsynced_kinds`] reverts every non-API kind
///     (notably the IDE `.tsx`) back to its PRIOR live path — this path NEVER
///     re-syncs, rebinds, or closes the IDE TSX (the IDE kind is owned by the
///     dedicated IDE-sync path);
///   * commit, then close ONLY the genuinely-stale API path
///     ([`genuinely_stale_after_sync`] gates on `synced_kinds`, so a stale IDE
///     path is never closed here);
///   * on API-sync failure, nothing is committed and nothing is closed — the
///     prior state and prior API path are retained intact (no close-before-sync).
#[allow(
    clippy::too_many_arguments,
    reason = "the spawned API-sync task needs the provider-surface store alongside its sync inputs"
)]
pub(super) async fn sync_api_to_provider_background_task(
    sync: ProjectSync,
    snapshot: super::PublishedResolverSnapshot,
    vfs: Option<Arc<verter_workspace::FilesystemWorkspace>>,
    provider_sync_states: Arc<DashMap<String, ProviderSyncState>>,
    canonical_id: String,
    is_jsx: bool,
    carrier_coordinator: Arc<crate::external_ts::CarrierTransactionCoordinator>,
    pending_snapshot_provider_sync: Arc<dashmap::DashSet<String>>,
    documents: Arc<DocumentRegistry>,
) {
    sync_carrier_api_transaction(
        &sync,
        &snapshot,
        &documents,
        vfs.as_deref(),
        &provider_sync_states,
        &canonical_id,
        is_jsx,
        &carrier_coordinator,
        &pending_snapshot_provider_sync,
    )
    .await;
}

#[allow(
    clippy::too_many_arguments,
    reason = "shared API sync retains the same gateway dependencies as the carrier transaction"
)]
pub(crate) async fn sync_carrier_api_transaction(
    sync: &ProjectSync,
    snapshot: &super::PublishedResolverSnapshot,
    documents: &DocumentRegistry,
    vfs: Option<&verter_workspace::FilesystemWorkspace>,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    is_jsx: bool,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
    pending_snapshot_provider_sync: &dashmap::DashSet<String>,
) -> bool {
    let host = documents.host_arc();
    let provider_surfaces = documents.provider_surfaces();
    let (open_uri, open_revision) = documents.open_compile_pin(canonical_id);
    let open_pin = open_uri.as_ref().zip(open_revision.as_ref());
    let document_lane = match documents.try_delivery_lane(canonical_id) {
        crate::document_sync_lane::DeliveryLane::Acquired(guard) => Some(guard),
        crate::document_sync_lane::DeliveryLane::Closed => None,
        crate::document_sync_lane::DeliveryLane::Busy => {
            pending_snapshot_provider_sync.insert(canonical_id.to_string());
            return false;
        }
    };
    // Route through the SINGLE carrier-sync gateway. This API-only background task
    // is the tsgo path (the tsserver coordinator route returns before spawning it),
    // so the gateway returns `DirectOpen` carrying the transition + a POST-open
    // authorization; the receipt is minted from `pending` only after the API buffer
    // opens (below). No membership context ⇒ no store publish. Ownership resolves from
    // the SAME published `vfs` the scanner reads.
    let (transition, pending) =
        match crate::external_ts::reconcile_carrier_source(crate::external_ts::CarrierSyncRequest {
            host: &host.host(),
            vfs,
            ownership_ready: snapshot.ownership_ready,
            resolver: &snapshot.resolver,
            provider_sync_states,
            provider_surfaces,
            documents: Some(documents),
            project_sync: Some(sync),
            canonical_id,
            is_jsx,
            ide: None,
            open_pin,
            membership: None,
            admission: carrier_coordinator,
            reason: crate::external_ts::ReconcileReason::SourceSynced,
        })
        .await
        {
            crate::external_ts::CarrierSyncDecision::DirectOpen {
                transition,
                pending,
            } => (transition, pending),
            // No owner (a settled non-owned outcome) or nothing to advertise: the dedicated
            // owner-loss / IDE-sync paths own the provider state. Settle the non-owned
            // disposition so the requeue / owner-loss barrier advance is not dropped; this
            // API-only background task does no buffer conversion.
            crate::external_ts::CarrierSyncDecision::NotOwned(not_owned) => {
                let _ = carrier_coordinator.settle(not_owned, canonical_id, None);
                return false;
            }
            // A tsserver `Published` outcome cannot occur here (`membership: None` ⇒ tsgo
            // direct-open only); the store publish is the tsserver path's job.
            crate::external_ts::CarrierSyncDecision::Published { .. } => return false,
        };
    deliver_api_transaction(
        sync,
        documents,
        vfs,
        provider_sync_states,
        canonical_id,
        transition,
        pending,
        carrier_coordinator,
        pending_snapshot_provider_sync,
        document_lane,
        open_pin,
    )
    .await
}

/// The API leg prepares under the document lane, delivers with only the path
/// lock, then validates and admits its own API evidence under the lane again.
/// An interactive IDE repair can run throughout the provider round trip.
#[allow(
    clippy::too_many_arguments,
    reason = "one API transaction carries its gateway authorization and exact document basis"
)]
pub(super) async fn deliver_api_transaction(
    sync: &ProjectSync,
    documents: &DocumentRegistry,
    vfs: Option<&verter_workspace::FilesystemWorkspace>,
    states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    transition: crate::provider_sync::ProviderSyncTransition,
    pending: crate::external_ts::PendingProviderReady,
    coordinator: &crate::external_ts::CarrierTransactionCoordinator,
    queue: &dashmap::DashSet<String>,
    document_lane: Option<tokio::sync::OwnedMutexGuard<()>>,
    open_pin: Option<(&Uri, &crate::documents::DocumentSnapshotIdentity)>,
) -> bool {
    if open_pin.is_none() && documents.canonical_id_to_uri(canonical_id).is_some() {
        queue.insert(canonical_id.to_string());
        return false;
    }
    let host = documents.host();
    let lanes = documents.document_lanes();
    let generation = lanes.open_generation(canonical_id);
    let revision = pending.source_revision();
    let binding = pending.binding().clone();
    let host_publication = host.workspace_read().published_root();
    let publication = vfs.and_then(|vfs| vfs.load_published());
    let basis_is_current = || {
        (match (host_publication.as_ref(), host.workspace_read().published_root().as_ref()) {
            (Some(before), Some(after)) => Arc::ptr_eq(before, after),
            (None, None) => true,
            _ => false,
        })
            && host.last_content_transition_generation(canonical_id) == revision
            && lanes.open_generation(canonical_id) == generation
            && open_pin.is_none_or(|(uri, identity)| documents.snapshot_identity_is_current(uri, identity))
            && match (publication.as_ref(), vfs.and_then(|vfs| vfs.load_published())) {
                (Some(captured), Some(current)) => Arc::ptr_eq(captured, &current),
                (None, None) => true,
                _ => false,
            }
            && vfs.is_some_and(|vfs| {
                matches!(crate::external_ts::resolve_carrier_ownership_over_vfs(
                    &host, vfs, canonical_id, true, Arc::from(binding.ensure_project_request().ts_version())),
                    verter_session::external_ts::CarrierOwnershipResolution::Bound(current) if current == binding)
            })
    };
    let mut state = transition.next;
    let Some(path) = state.api_path.clone() else {
        return true;
    };
    let Ok(Some(api)) = host.get_public_api(canonical_id) else {
        queue.insert(canonical_id.to_string());
        return false;
    };
    let code = api.code_for_companion_path(&path);
    let carrier_source =
        crate::provider_surface_store::resolve_carrier_source(Some(documents), &host, canonical_id);
    let previous = states.get(canonical_id).map(|entry| entry.clone());
    if !basis_is_current() {
        queue.insert(canonical_id.to_string());
        return false;
    }
    if crate::provider_sync::api_leg_is_current(
        sync,
        previous.as_ref(),
        &state.owner_binding,
        &path,
        code,
        api.source_map.as_deref(),
        canonical_id,
        documents,
    ) {
        return true;
    }
    drop(document_lane);
    let delivery = sync
        .deliver_api_fenced(&path, code, state.api_background_loaded, &basis_is_current)
        .await;
    let Ok(Some(delivery)) = delivery else {
        queue.insert(canonical_id.to_string());
        return false;
    };
    let _document_lane = match documents.try_delivery_lane(canonical_id) {
        crate::document_sync_lane::DeliveryLane::Acquired(guard) => Some(guard),
        crate::document_sync_lane::DeliveryLane::Closed => None,
        crate::document_sync_lane::DeliveryLane::Busy => {
            queue.insert(canonical_id.to_string());
            return false;
        }
    };
    let projection_is_current = host
        .get_public_api(canonical_id)
        .ok()
        .flatten()
        .is_some_and(|current| {
            current.code_for_companion_path(&path) == code && current.source_map == api.source_map
        });
    if !basis_is_current() || !projection_is_current || !delivery.is_current(sync) {
        queue.insert(canonical_id.to_string());
        return false;
    }
    state.mark_api_delivered(code);
    let receipt = pending.confirm_opened(&[ProviderPathKind::Api]);
    // The gateway's API fingerprint must describe this operation's exact bytes
    // and map; a second compile cannot silently borrow an earlier authorization.
    let fingerprint_matches = receipt.companions().iter().any(|companion| {
        companion.role == verter_session::external_ts::SnapshotRole::CarrierApi
            && companion.uri.as_ref() == path
            && companion.content_hash
                == crate::provider_surface_store::ContentHash::of(code).to_hash16()
            && companion.map_hash
                == api
                    .source_map
                    .as_deref()
                    .map(|map| crate::provider_surface_store::ContentHash::of(map).to_hash16())
                    .unwrap_or([0; 16])
    });
    if !fingerprint_matches {
        queue.insert(canonical_id.to_string());
        return false;
    }
    // Record only after admission, under the same document identity pin. The
    // source comes from that pin rather than a second live registry lookup.
    let admit = |source: Arc<str>| {
        let outcome = coordinator.admit_api_owned(&host, states, canonical_id, state, &receipt);
        if outcome == crate::external_ts::AdmitOutcome::Admitted {
            let _ = crate::provider_surface_store::record_carrier_companion_surface_with_source(
                documents.provider_surfaces(),
                canonical_id,
                &path,
                crate::provider_surface_store::RecordedProviderSurface::Verbatim {
                    kind: crate::provider_surface_store::ProviderSurfaceKind::CarrierApi,
                    code,
                },
                api.source_map.as_deref(),
                source,
            );
        }
        outcome
    };
    let outcome = match open_pin {
        Some((uri, identity)) => {
            documents.with_current_snapshot_identity(uri, identity, |document| {
                admit(Arc::clone(&document.source))
            })
        }
        None => carrier_source.map(admit),
    };
    if outcome != Some(crate::external_ts::AdmitOutcome::Admitted) {
        queue.insert(canonical_id.to_string());
        return false;
    }
    drop(_document_lane);
    let latest = states.get(canonical_id).map(|entry| entry.clone());
    if let Some(latest) = latest {
        let stale =
            genuinely_stale_after_sync(&transition.stale_paths, &latest, &[ProviderPathKind::Api]);
        close_stale_provider_paths_with(
            sync,
            documents.provider_surfaces(),
            &non_decl_close_targets(&stale),
            "api_transaction",
            Some(&ProjectSyncRedelivery::new(sync)),
        )
        .await;
    }
    true
}

pub(super) async fn sync_pending_non_carrier_provider_file(
    sync: &ProjectSync,
    documents: &DocumentRegistry,
    snapshot: &super::PublishedResolverSnapshot,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
) -> bool {
    let _document_lane = match documents.try_delivery_lane(canonical_id) {
        crate::document_sync_lane::DeliveryLane::Acquired(guard) => Some(guard),
        crate::document_sync_lane::DeliveryLane::Closed => None,
        crate::document_sync_lane::DeliveryLane::Busy => return false,
    };
    if _document_lane.is_none() && documents.canonical_id_to_uri(canonical_id).is_some() {
        return false;
    }
    let Some(source) = documents.host().get_source(canonical_id) else {
        return false;
    };
    // Bump diagnostics_generation for the same reason the carrier pass does:
    // the re-synced shadow buffer must look NEW to the diagnostics cache and
    // to any receipt the open document still owes — otherwise an incomplete
    // publication from before this pass (e.g. its provider query failed while
    // the engine was between epochs) is never re-driven, because the receipt
    // was not outdated by the pass that repaired its input.
    documents.host().bump_diagnostics_generation(canonical_id);
    // Framework carriers never sync to the provider as raw scripts.
    let Some(file_language) =
        crate::provider_sync::provider_script_language(&documents.host(), canonical_id)
    else {
        return false;
    };
    let module_references = block_in_place_if_available(|| {
        documents
            .host()
            .upsert(verter_session::UpsertRequest {
                canonical_id: Some(canonical_id.to_string()),
                input_id: canonical_id.to_string(),
                source: source.clone(),
                file_language,
                aliases: Vec::new(),
            })
            .map(|result| result.module_references)
            .unwrap_or_default()
    });
    let reader = LspProjectResolverReader::new(documents);
    let Some(prepared) = prepare_non_carrier_provider_sync(
        Some(snapshot),
        &reader,
        canonical_id,
        &source,
        &module_references,
    ) else {
        return false;
    };
    let Some(next_state) =
        crate::provider_sync::non_carrier_sync_state_for_source(&snapshot.resolver, canonical_id)
    else {
        return false;
    };

    let transition = prepare_sync_transition(provider_sync_states, canonical_id, next_state);
    close_stale_provider_paths_with(
        sync,
        documents.provider_surfaces(),
        &non_decl_close_targets(&transition.stale_paths),
        "pending_snapshot",
        Some(&ProjectSyncRedelivery::new(sync)),
    )
    .await;

    let mut committed_state = transition.next;
    match sync
        .sync_file(&prepared.provider_path, &prepared.rewritten)
        .await
    {
        Ok(()) => {
            committed_state.mark_shadow_delivered(&source);
            commit_sync_transition(provider_sync_states, canonical_id, committed_state);
            documents.host().set_import_dependencies(
                canonical_id,
                prepared
                    .resolved_dependencies
                    .iter()
                    .map(|entry| verter_session::DependencyResolution {
                        specifier: entry.provider_specifier.clone(),
                        resolved_canonical_id: Some(entry.source_id.clone()),
                        possible_canonical_ids: Vec::new(),
                    })
                    .collect(),
            );
            true
        }
        Err(error) => {
            tracing::warn!(
                "pending_snapshot: failed to sync provider shadow path {}: {error}",
                prepared.provider_path
            );
            false
        }
    }
}

async fn remove_provider_sync_state_and_close_paths(
    sync: &ProjectSync,
    provider_surfaces: &crate::provider_surface_store::ProviderSurfaceStore,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    context: &str,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
) {
    // Advance-before-mutate: the coordinator advances the owner-loss barrier BEFORE it
    // vacates the slot when the removed state was a previously-committed carrier, so a late
    // owned token captured before this removal can never resurrect the obsolete owner into
    // the vacated slot.
    if let Some(state) =
        carrier_coordinator.advance_barrier_and_remove(provider_sync_states, canonical_id)
    {
        // The declaration overlay (`Decl`), if any, is NOT closed here: its
        // lifecycle is owned by `DeclOverlayOwner` and released only when no open
        // carrier root still reaches it (via the `did_close` release). A background
        // state removal closes only the non-decl artifacts.
        close_stale_provider_paths_with(
            sync,
            provider_surfaces,
            &state.active_non_decl_paths(),
            context,
            Some(&ProjectSyncRedelivery::new(sync)),
        )
        .await;
    }
}

#[cfg(test)]
#[path = "background_drain_tests.rs"]
mod tests;

//! SyncCoordinator: single long-lived task that debounces type provider syncs.
//!
//! Instead of spawning a new tokio task per keystroke (which can flood TSGO during
//! fast typing), the coordinator receives signals via an mpsc channel and waits
//! for 300ms of silence before triggering a sync. This guarantees exactly one
//! sync per file after typing stops, regardless of keystroke timing.
//!
//! After syncing, the coordinator computes merged (Verter lint + TypeScript type)
//! diagnostics and publishes them via push. Push diagnostics stay visible during
//! typing — VS Code automatically adjusts their positions as the document changes.

use std::collections::HashMap;
use std::sync::Arc;

use dashmap::{DashMap, DashSet};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::Client;

use crate::documents::line_index::LineIndex;
use crate::documents::DocumentRegistry;
use crate::provider_sync::{
    commit_sync_transition, genuinely_stale_after_sync, non_decl_close_targets,
    open_unresolved_carrier_commit, open_unresolved_carrier_state, revert_unsynced_kinds,
    NonDeclProviderPathKind, ProviderPathKind, ProviderSyncState,
};
use crate::type_provider::merge;
use crate::type_provider::project_sync::ProjectSync;
use crate::type_provider::traits::TypeProvider;

/// Per-canonical bookkeeping for changes the server has RECEIVED but has not
/// finished processing.
///
/// A change is "in flight" from the instant its `did_change` handler is entered
/// — before the global document-commit mutex, before the document is committed,
/// before the debounce signal is deposited — until that handler is done. A
/// document with a change in flight is not quiet, whatever the receipt on its
/// last signal says, so the coordinator will not dispatch a sync for it.
#[derive(Default)]
struct CanonicalChangeState {
    /// Live [`ChangeInFlight`] tickets for this canonical id.
    tickets: u32,
}

/// Shared "changes received but not yet processed" map, read by the coordinator
/// loop and written by the `did_change` handlers.
type ChangeTracker = Arc<parking_lot::Mutex<HashMap<String, CanonicalChangeState>>>;

/// Handle for sending signals to the coordinator.
#[derive(Clone)]
pub struct SyncCoordinatorHandle {
    /// Capacity-one edge trigger. Repeated edits do not allocate queued messages.
    wake_tx: mpsc::Sender<()>,
    /// Latest URI per canonical document. Replacements coalesce while the actor
    /// is busy synchronizing another file or waiting on a provider commit.
    pending: Arc<parking_lot::Mutex<HashMap<String, PendingSignal>>>,
    /// Changes received but not yet processed. See [`CanonicalChangeState`].
    changes: ChangeTracker,
    /// TEST-ONLY: the coordinator's own progress receipts.
    #[cfg(test)]
    pub(crate) receipts: CoordinatorReceipts,
}

/// TEST-ONLY receipts a test waits on instead of polling or sleeping.
///
/// These are the coordinator's own transitions, published where they
/// happen. Nothing in production reads them.
#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct CoordinatorReceipts {
    /// One notify after each coordinator select-arm, so paused-time tests
    /// wait for an exact loop tick instead of a yield-count flush.
    pub(crate) loop_tick: Arc<tokio::sync::Notify>,
    /// One notify after `publish_merged_diagnostics` writes the verter-diag
    /// cache, which happens on a spawned task after the tick.
    pub(crate) diags_published: Arc<tokio::sync::Notify>,
    /// Live `publish_merged_diagnostics` tasks. Zero means every spawned
    /// publish has written the cache (or been aborted).
    pub(crate) diag_tasks_live: Arc<std::sync::atomic::AtomicUsize>,
    /// Monotonic count of COMPLETED dispatch ticks — incremented at the end
    /// of the quiet-window arm, after every ready document's `sync_file` has
    /// awaited. This is the only receipt that fences a whole dispatch
    /// decision: a single document's provider receipt lands MID-arm, and the
    /// ready set is iterated in `HashMap` order, so another document's
    /// dispatch can land on either side of it.
    pub(crate) dispatch_ticks: Arc<std::sync::atomic::AtomicUsize>,
    /// Monotonic count of completed `publish_merged_diagnostics` tasks. A
    /// live gauge cannot settle a tick: the task is spawned AFTER the
    /// provider sync it follows, so `diag_tasks_live == 0` is already true
    /// at the moment the sync receipt lands. A monotonic count makes "this
    /// tick's publish has finished" an exact predicate.
    pub(crate) diags_published_count: Arc<std::sync::atomic::AtomicUsize>,
}

#[derive(Clone, Debug)]
struct PendingSignal {
    uri: String,
    requires_sync: bool,
    force_diagnostics: bool,
    /// When the change this signal describes REACHED the server, not when the
    /// coordinator got round to draining it out of the inbox.
    ///
    /// A signal can sit in the inbox for seconds: `did_change` handlers commit
    /// their document under one global mutex before they signal, and the
    /// coordinator's own `sync_file(..).await` is inline in its loop. Stamping
    /// at drain charges all of that waiting to the user as fresh quiet time and
    /// restarts the full debounce window from zero.
    received_at: Instant,
}

impl SyncCoordinatorHandle {
    /// Signal that a file has changed and needs a debounced sync.
    ///
    /// `received_at` is when the change REACHED the server — the caller's own
    /// entry instant, never `Instant::now()` taken at some later point on the
    /// path. The debounce window is measured from it.
    pub fn signal(&self, canonical_id: String, uri_str: String, received_at: Instant) {
        self.pending
            .lock()
            .entry(canonical_id)
            .and_modify(|pending| {
                pending.uri = uri_str.clone();
                pending.requires_sync = true;
                // Newest receipt wins, and `max` rather than assignment: LSP
                // notification handlers run concurrently, so an older change can
                // deposit its signal after a newer one has already deposited
                // its own. Assigning would walk the window backwards and fire
                // the sync while the user is still typing.
                pending.received_at = pending.received_at.max(received_at);
            })
            .or_insert(PendingSignal {
                uri: uri_str,
                requires_sync: true,
                force_diagnostics: false,
                received_at,
            });
        // Full means a wake is already queued, which is exactly the desired
        // coalescing behavior. Closed means the server is shutting down.
        let _ = self.wake_tx.try_send(());
    }

    /// Signal that a file needs a debounced REPUBLISH but no provider sync.
    ///
    /// The edit that motivates this is the style-only one: it needs no provider
    /// sync, no dependency-frontier refresh and no import republication — but
    /// the host still CLEARED the file's diagnostics for it, and a clear that
    /// arms no recompute leaves the editor showing nothing. The debounced tick
    /// recompiles for every revision it is about to publish, so asking it to
    /// publish is what refills them.
    ///
    /// Merges rather than overwrites: it must never downgrade a pending
    /// `requires_sync` deposited by a real edit for the same quiet window.
    pub fn signal_diagnostics_only(
        &self,
        canonical_id: String,
        uri_str: String,
        received_at: Instant,
    ) {
        self.pending
            .lock()
            .entry(canonical_id)
            .and_modify(|pending| {
                pending.uri = uri_str.clone();
                pending.force_diagnostics = true;
                pending.received_at = pending.received_at.max(received_at);
            })
            .or_insert(PendingSignal {
                uri: uri_str,
                requires_sync: false,
                force_diagnostics: true,
                received_at,
            });
        let _ = self.wake_tx.try_send(());
    }

    /// Record that a change to `canonical_id` has been RECEIVED.
    ///
    /// Call this at `did_change` handler ENTRY, before the global commit mutex
    /// and before any document work. The returned ticket stamps the receipt
    /// instant the debounce window is measured from and holds the coordinator
    /// off this canonical id until the change has been processed.
    pub fn change_received(&self, canonical_id: String) -> ChangeInFlight {
        self.changes
            .lock()
            .entry(canonical_id.clone())
            .or_default()
            .tickets += 1;
        ChangeInFlight {
            handle: self.clone(),
            canonical_id,
            received_at: Instant::now(),
        }
    }

    /// Create an isolated handle/inbox pair for testing the coalescing contract.
    #[cfg(test)]
    pub fn new_for_test() -> (Self, mpsc::Receiver<()>) {
        let (wake_tx, wake_rx) = mpsc::channel(1);
        (
            Self {
                wake_tx,
                pending: Arc::new(parking_lot::Mutex::new(HashMap::new())),
                changes: Arc::new(parking_lot::Mutex::new(HashMap::new())),
                receipts: CoordinatorReceipts::default(),
            },
            wake_rx,
        )
    }

    #[cfg(test)]
    pub fn take_pending_for_test(&self) -> HashMap<String, String> {
        std::mem::take(&mut *self.pending.lock())
            .into_iter()
            .map(|(canonical_id, pending)| (canonical_id, pending.uri))
            .collect()
    }

    /// Wait for one coordinator loop tick. Interest is registered before
    /// the await so a tick between enable and poll cannot be missed.
    #[cfg(test)]
    pub(crate) async fn await_loop_tick(&self) {
        let notified = self.receipts.loop_tick.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        notified.await;
    }

    /// Wait until `ready` is true, waking on each coordinator loop tick.
    /// The 20s Instant is an outer watchdog only — it is not the thing
    /// that makes the wait succeed. Interest is enabled before the
    /// predicate is re-checked so a tick cannot sneak past. The watchdog
    /// is raced against the tick wait so a missing tick cannot hang.
    ///
    /// Do not use this under a paused clock: a `Notify` wait is not a
    /// timer, so auto-advance will not fire the debounce. Paused tests
    /// drive `await_loop_tick` + `advance` instead.
    #[cfg(test)]
    pub(crate) async fn await_until(&self, mut ready: impl FnMut() -> bool, fail: impl FnOnce()) {
        let deadline = Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if ready() {
                return;
            }
            let tick = self.receipts.loop_tick.notified();
            let published = self.receipts.diags_published.notified();
            tokio::pin!(tick);
            tokio::pin!(published);
            tick.as_mut().enable();
            published.as_mut().enable();
            if ready() {
                return;
            }
            tokio::select! {
                biased;
                () = tick => {}
                () = published => {}
                () = tokio::time::sleep_until(deadline) => {
                    // Watchdog firing is failure even if `ready()` is now
                    // true: that means the predicate flipped without a
                    // loop_tick/diags_published receipt (a poll of owned
                    // state). The 20s Instant must never be a success path.
                    fail();
                    panic!("sync coordinator loop_tick watchdog");
                }
            }
        }
    }

    /// TEST-ONLY: whether `canonical_id` still has an undrained signal in
    /// the handle-side inbox. Once this is false the coordinator has taken
    /// that signal into its own pending map, so a later dispatch decision
    /// provably saw it.
    #[cfg(test)]
    pub(crate) fn inbox_contains(&self, canonical_id: &str) -> bool {
        self.pending.lock().contains_key(canonical_id)
    }

    #[cfg(test)]
    pub(crate) fn diag_tasks_live(&self) -> usize {
        self.receipts
            .diag_tasks_live
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// TEST-ONLY: monotonic count of completed diagnostics publications.
    /// Pair it with [`Self::await_until`] to settle on "the publish this
    /// tick owed has finished" without a fixed sleep.
    /// TEST-ONLY: completed dispatch ticks. See
    /// [`CoordinatorReceipts::dispatch_ticks`].
    #[cfg(test)]
    pub(crate) fn dispatch_ticks(&self) -> usize {
        self.receipts
            .dispatch_ticks
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) fn diags_published_count(&self) -> usize {
        self.receipts
            .diags_published_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(test)]
struct DiagTaskLiveGuard(Arc<std::sync::atomic::AtomicUsize>);

#[cfg(test)]
impl DiagTaskLiveGuard {
    fn new(counter: Arc<std::sync::atomic::AtomicUsize>) -> Self {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self(counter)
    }
}

#[cfg(test)]
impl Drop for DiagTaskLiveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// A `did_change` handler's ticket for one received change to one document.
///
/// Created at handler entry; dropped when the handler is done. It owns the
/// receipt instant so no caller can substitute a later one by accident.
pub struct ChangeInFlight {
    handle: SyncCoordinatorHandle,
    canonical_id: String,
    received_at: Instant,
}

impl ChangeInFlight {
    /// Deposit this change's debounce signal, stamped with the instant the
    /// change was received rather than any later instant on the path.
    pub fn signal(&self, uri_str: String) {
        self.handle
            .signal(self.canonical_id.clone(), uri_str, self.received_at);
    }

    /// Deposit a REPUBLISH-only signal for this change, stamped with the same
    /// receipt instant. See [`SyncCoordinatorHandle::signal_diagnostics_only`].
    pub fn signal_diagnostics_only(&self, uri_str: String) {
        self.handle
            .signal_diagnostics_only(self.canonical_id.clone(), uri_str, self.received_at);
    }
}

impl Drop for ChangeInFlight {
    fn drop(&mut self) {
        {
            let mut changes = self.handle.changes.lock();
            if let Some(state) = changes.get_mut(&self.canonical_id) {
                state.tickets = state.tickets.saturating_sub(1);
                if state.tickets == 0 {
                    changes.remove(&self.canonical_id);
                }
            }
        }
        // Releasing a ticket is what makes a document quiescent again, and the
        // coordinator PARKS on a canonical id whose change is in flight (it is
        // excluded from the deadline computation, so an all-gated inbox leaves
        // no timer armed at all). Wake unconditionally so the release is always
        // observed — including on the handler paths that return without ever
        // signalling (a virtual document, a style-only edit).
        //
        // No wakeup is lost when the channel is already full: a queued wake is
        // one the coordinator has not consumed yet, so it will re-read this map
        // after this decrement.
        let _ = self.handle.wake_tx.try_send(());
    }
}

/// Shared state the coordinator needs to perform syncs and publish diagnostics.
#[derive(Clone)]
pub struct SyncCoordinatorDeps {
    pub documents: Arc<DocumentRegistry>,
    /// The in-process provider connection. `None` on routes with no in-process
    /// provider child (editor-owned tsserver plugin serving, verter-only
    /// mode): the debounced PUBLISH half still runs — Verter-owned
    /// diagnostics (lint, unused-declaration hints, template errors) never
    /// depend on a provider — while the provider-sync half is skipped.
    pub project_sync: Option<ProjectSync>,
    pub needs_provider_sync: Arc<DashSet<String>>,
    pub pending_snapshot_provider_sync: Arc<DashSet<String>>,
    pub client: Client,
    /// Type provider for fetching TS diagnostics after sync.
    pub type_provider: Option<Arc<dyn TypeProvider>>,
    /// Cached verter-only diagnostics (URI → (version, diag_gen, diagnostics)).
    /// Shared with the server so we can read cached verter diags after sync.
    pub cached_verter_diags: Arc<DashMap<String, crate::server::CachedVerterDiagEntry>>,
    /// Negotiated position encoding for building line indexes.
    pub position_encoding: Arc<parking_lot::RwLock<PositionEncodingKind>>,
    /// Source-keyed provider materialization state shared with the server.
    pub provider_sync_states: Arc<DashMap<String, ProviderSyncState>>,
    /// VFS workspace for published LspViews and resolver snapshot.
    pub vfs_workspace: Arc<parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>>,
    /// The active engine kind. For tsserver the debounced carrier sync PUBLISHES
    /// the carrier companions into the on-disk store (the membership mechanism)
    /// rather than opening them — the carrier-companion verbs on `project_sync`
    /// are no-ops for tsserver, so without the publish here the debounced tick
    /// could bypass the store and leave the carrier's store content stale.
    pub type_provider_kind: crate::TypeProviderKind,
    /// The live carrier-publish coordinator — `Some` only for tsserver. The
    /// debounced sync publishes a freshly-edited carrier's companions through it
    /// so the plugin serves up-to-date content on the next pull.
    pub carrier_publish_coordinator: Option<crate::external_ts::CarrierPublishCoordinator>,
    /// The per-source carrier transaction coordinator (admission gate, owner-loss barrier,
    /// non-owned retry disposition), shared with the server so the debounced sync's carrier
    /// commits and non-owned settlements serialize on the ONE barrier map.
    pub carrier_transaction_coordinator: Arc<crate::external_ts::CarrierTransactionCoordinator>,
}

/// Debounce interval: sync fires after [`crate::edit_quiet_window::EDIT_QUIET_WINDOW`]
/// of silence for a given file. Alias kept so existing tests can name the
/// millisecond form of the one quiet-window policy.
pub(crate) const DEBOUNCE_MS: u64 = crate::edit_quiet_window::EDIT_QUIET_WINDOW_MS;

/// Spawn the coordinator task and return a handle for sending signals.
pub fn spawn_sync_coordinator(deps: SyncCoordinatorDeps) -> SyncCoordinatorHandle {
    let (wake_tx, wake_rx) = mpsc::channel(1);
    let pending = Arc::new(parking_lot::Mutex::new(HashMap::new()));
    let changes: ChangeTracker = Arc::new(parking_lot::Mutex::new(HashMap::new()));
    let semantic_ready_rx = deps.documents.subscribe_semantic_ready();
    tracing::info!("sync_coordinator: spawned (debounce {DEBOUNCE_MS}ms)");
    #[cfg(test)]
    let receipts = CoordinatorReceipts::default();
    tokio::spawn(coordinator_loop(
        wake_rx,
        semantic_ready_rx,
        Arc::clone(&pending),
        Arc::clone(&changes),
        Arc::new(deps),
        #[cfg(test)]
        receipts.clone(),
    ));
    SyncCoordinatorHandle {
        wake_tx,
        pending,
        changes,
        #[cfg(test)]
        receipts,
    }
}

async fn coordinator_loop(
    mut wake_rx: mpsc::Receiver<()>,
    mut semantic_ready_rx: tokio::sync::broadcast::Receiver<crate::documents::SemanticReady>,
    inbox: Arc<parking_lot::Mutex<HashMap<String, PendingSignal>>>,
    changes: ChangeTracker,
    deps: Arc<SyncCoordinatorDeps>,
    #[cfg(test)] receipts: CoordinatorReceipts,
) {
    let debounce = crate::edit_quiet_window::EDIT_QUIET_WINDOW;
    // Map from canonical_id → (last_change_time, uri_str)
    let mut pending_files: HashMap<String, (Instant, PendingSignal)> = HashMap::new();
    // Provider-state commits are serialized and allowed to finish. Diagnostics
    // are immutable snapshot reads, so a new edit can cancel only the stale
    // diagnostic task without risking a half-committed provider surface.
    let mut diagnostic_tasks: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();

    // A canonical id whose change is still in flight is NOT quiet, so it is
    // excluded from BOTH the deadline computation and the dispatch set. It is
    // deliberately excluded from the deadline too: with no timer armed for it
    // the loop parks on the wake channel instead of spinning on an already-
    // elapsed `sleep_until`. `ChangeInFlight::drop` always wakes the loop, so
    // the file is re-examined the instant it becomes quiescent.
    let quiescent = |canonical_id: &str| !changes.lock().contains_key(canonical_id);

    loop {
        // Calculate next deadline from pending files
        let next_deadline = pending_files
            .iter()
            .filter(|(canonical_id, _)| quiescent(canonical_id))
            .map(|(_, (t, _))| *t + debounce)
            .min();

        tokio::select! {
            wake = wake_rx.recv() => {
                match wake {
                    Some(()) => {
                        let signals = std::mem::take(&mut *inbox.lock());
                        for (canonical_id, signal) in signals {
                            tracing::debug!("sync_coordinator: signal {canonical_id}");
                            if let Some(stale) = diagnostic_tasks.remove(&canonical_id) {
                                stale.abort();
                            }
                            // Reset the quiet window for the latest edit only —
                            // measured from when that edit REACHED the server,
                            // which the signal carries, NOT from now. Time spent
                            // waiting in the inbox is time the file was already
                            // quiet, and must not be charged to the user again.
                            // `max` for the same reason `signal` uses it: a
                            // concurrently-dispatched older handler can deposit
                            // after a newer one.
                            let received_at = signal.received_at;
                            pending_files
                                .entry(canonical_id)
                                .and_modify(|(changed_at, pending)| {
                                    *changed_at = (*changed_at).max(received_at);
                                    pending.uri = signal.uri.clone();
                                    pending.requires_sync |= signal.requires_sync;
                                    pending.force_diagnostics |= signal.force_diagnostics;
                                })
                                .or_insert((received_at, signal));
                        }
                    }
                    None => {
                        // All handles dropped — coordinator shutting down.
                        for (_, task) in diagnostic_tasks.drain() {
                            task.abort();
                        }
                        return;
                    }
                }
            }
            semantic_ready = semantic_ready_rx.recv() => {
                match semantic_ready {
                    Ok(ready) => {
                        if ready
                            .uri
                            .parse::<Uri>()
                            .ok()
                            .and_then(|uri| {
                                deps.documents.get(&uri).map(|document| {
                                    document.version == ready.version
                                        && document.document_revision == ready.document_revision
                                })
                            })
                            == Some(true)
                        {
                            // An earlier pre-semantic pass may have cached an empty
                            // result under this same document/host generation.
                            deps.cached_verter_diags.remove(&ready.uri);
                            if let Some(stale) = diagnostic_tasks.remove(&ready.canonical_id) {
                                stale.abort();
                            }
                            // Deliberately `Instant::now()`: unlike a keystroke
                            // signal, a semantic-ready event has no earlier
                            // receipt to honour — the reason to republish arose
                            // exactly here, so the quiet window starts here.
                            let received_at = Instant::now();
                            pending_files
                                .entry(ready.canonical_id)
                                .and_modify(|(changed_at, pending)| {
                                    *changed_at = received_at;
                                    pending.uri = ready.uri.clone();
                                    pending.force_diagnostics = true;
                                })
                                .or_insert((
                                    received_at,
                                    PendingSignal {
                                        uri: ready.uri,
                                        requires_sync: false,
                                        force_diagnostics: true,
                                        received_at,
                                    },
                                ));
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // Recover from a burst by invalidating and scheduling every
                        // open document. These are diagnostics-only passes.
                        // Deliberately `Instant::now()`: the recovery sweep is
                        // its own reason to republish and carries no earlier
                        // per-document receipt.
                        let received_at = Instant::now();
                        for uri in deps.documents.open_uris() {
                            let Ok(parsed) = uri.parse::<Uri>() else { continue; };
                            let Some(canonical_id) = deps.documents.get_canonical_id(&parsed) else { continue; };
                            deps.cached_verter_diags.remove(&uri);
                            pending_files.insert(
                                canonical_id,
                                (
                                    received_at,
                                    PendingSignal {
                                        uri,
                                        requires_sync: false,
                                        force_diagnostics: true,
                                        received_at,
                                    },
                                ),
                            );
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        // The registry owns the sender for the server lifetime.
                        // A closed channel therefore means the coordinator's
                        // dependency graph is shutting down; returning avoids a
                        // permanently-ready select branch spinning the runtime.
                        for (_, task) in diagnostic_tasks.drain() {
                            task.abort();
                        }
                        return;
                    }
                }
            }
            _ = async {
                match next_deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline).await,
                    None => std::future::pending::<()>().await,
                }
            } => {
                // Find files that have been quiet for >= debounce_ms. A file
                // whose change is still in flight is NOT quiet however old its
                // last receipt is: without this gate, receipt-time stamping
                // would dispatch once per handler for the whole backlog — one
                // provider sync per keystroke, the exact flood the debounce
                // exists to prevent.
                let now = Instant::now();
                let ready: Vec<(String, PendingSignal)> = pending_files
                    .iter()
                    .filter(|(id, (t, _))| now.duration_since(*t) >= debounce && quiescent(id))
                    .map(|(id, (_, signal))| (id.clone(), signal.clone()))
                    .collect();

                // The canonicals whose settled signal carried a REAL edit
                // (`requires_sync`) — the only signal class after which the
                // results of files that IMPORT this one can have changed. Their
                // open importers are re-armed after the loop, once this tick's
                // own work (including the edited file's provider sync, which
                // runs inline above the arming's debounce window) is done.
                let mut settled_edits: Vec<String> = Vec::new();

                for (canonical_id, signal) in ready {
                    pending_files.remove(&canonical_id);
                    let mut publish_diagnostics = signal.force_diagnostics;
                    let will_sync = signal.requires_sync
                        && deps.needs_provider_sync.remove(&canonical_id).is_some();
                    if signal.requires_sync {
                        settled_edits.push(canonical_id.clone());
                    }

                    // The carrier's IDE surface is owed by THIS tick, for every
                    // settled revision it is about to sync or publish — not by
                    // the provider sync it used to sit inside. A revision can
                    // need the recompile without needing any provider work at
                    // all: a style-only edit clears the file's diagnostics and
                    // asks only for a republish, and publishing without first
                    // recompiling would publish the emptiness the clear left
                    // behind. Skipped when this tick will do neither, so a
                    // signal whose work bit an interactive sync already consumed
                    // compiles nothing.
                    if will_sync || publish_diagnostics {
                        refresh_carrier_ide_surface(&deps, &canonical_id);
                    }

                    if will_sync {
                        let sync_version = signal.uri
                            .parse::<Uri>()
                            .ok()
                            .and_then(|uri| deps.documents.get(&uri).map(|document| document.version));
                        sync_file(&deps, &canonical_id, &signal.uri).await;
                        // A new editor revision can land while the non-cancellable
                        // provider-state commit is in flight. Fence diagnostics on
                        // the actual LSP version—not `needs_provider_sync`, which is
                        // also an API-reconciliation work bit and may legitimately be
                        // reinserted by an interactive sync for this SAME revision.
                        let current_version = signal.uri.parse::<Uri>().ok().and_then(|uri| {
                            deps.documents.get(&uri).map(|document| document.version)
                        });
                        if sync_version.is_none() || current_version != sync_version {
                            continue;
                        }
                        publish_diagnostics = true;
                    }

                    if publish_diagnostics {

                        if let Some(stale) = diagnostic_tasks.remove(&canonical_id) {
                            stale.abort();
                        }
                        let task_deps = Arc::clone(&deps);
                        let task_canonical_id = canonical_id.clone();
                        diagnostic_tasks.insert(
                            canonical_id,
                            tokio::spawn({
                                #[cfg(test)]
                                let diags_published = Arc::clone(&receipts.diags_published);
                                #[cfg(test)]
                                let diag_tasks_live = Arc::clone(&receipts.diag_tasks_live);
                                #[cfg(test)]
                                let diags_published_count = Arc::clone(&receipts.diags_published_count);
                                async move {
                                    {
                                        #[cfg(test)]
                                        let _live = DiagTaskLiveGuard::new(diag_tasks_live);
                                        publish_merged_diagnostics(
                                            &task_deps,
                                            &task_canonical_id,
                                            &signal.uri,
                                        )
                                        .await;
                                    }
                                    #[cfg(test)]
                                    {
                                        diags_published_count
                                            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                        diags_published.notify_waiters();
                                    }
                                }
                            }),
                        );
                    }
                }
                arm_open_importer_republish(&deps, &settled_edits, &mut pending_files);
                diagnostic_tasks.retain(|_, task| !task.is_finished());
                #[cfg(test)]
                receipts
                    .dispatch_ticks
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        #[cfg(test)]
        receipts.loop_tick.notify_waiters();
    }
}

/// Arm a debounced diagnostics-only republish for every OPEN importer of each
/// canonical whose settled REAL edit this tick just processed.
///
/// A child edit changes what its parents' diagnostics say — a renamed prop
/// turns a parent's usage into `verter/unknown-prop`; a changed prop type
/// turns the parent's generated TSX into a provider type error — but the edit
/// clears and re-arms only the CHILD: the host upsert clears the child's
/// `latest_diagnostics`, the `did_change` handler signals the child's
/// canonical, and this tick publishes only pending keys. Without this arming a
/// parent keeps whatever the editor last showed, forever. The workspace's
/// reverse import graph exists for exactly this consumer ("LSP affected-files
/// reporting + diagnostics" — `WorkspaceRead::affected_canonicals`, R22); it
/// is read-only bookkeeping here and is never wired to cache invalidation —
/// the parent's recompute reads host state that revalidates on read.
///
/// Depth: the TRANSITIVE closure (`affected_canonicals`), not direct importers
/// only — a parent that re-exports or wraps the child can change its own
/// public surface, so a grandparent's diagnostics can move too. The walk is an
/// in-memory reverse-axis BFS; the expensive work is bounded separately below.
///
/// What bounds the fan-out:
/// - armed at most once per settled quiet window per edited file — the arming
///   rides THIS tick, never the per-keystroke `did_change` path, so a burst of
///   keystrokes coalesces into one child settle and one arming;
/// - only OPEN importers arm (push diagnostics exist for open editors only, and
///   the tick's own refresh gate drops closed documents anyway), so the armed
///   work is bounded by the open-editor count, not by the import graph;
/// - armed importers enter the SAME debounced pending map with a fresh receipt
///   — they publish one debounce window later, coalescing with each other,
///   with their own pending edits, and with any further child settles;
/// - the republish is diagnostics-only (`requires_sync: false`): no provider
///   sync, and the tick's IDE refresh is a warm cache hit for a parent whose
///   own compile inputs did not change.
///
/// Fail-closed: the arming only schedules the parent through the existing
/// publish path, which already refuses to publish torn results (surface
/// re-validation, document-version fences). A CHILD edit moves neither the
/// parent's document version nor, on its own, the parent's diagnostics
/// generation — so without intervention the parent's version-keyed
/// document-half cache would warm-hit and republish yesterday's set as fresh.
///
/// The stale-cache fence is the GENERATION BUMP, not the entry drop. Dropping
/// the entry alone cannot fence a parent computation already in flight: that
/// computation snapshotted the pre-arm generation at entry, captured pre-edit
/// child state, and lands its cache write AFTER the drop — with the parent's
/// version and generation both still matching, every later read warm-hits it.
/// Bumping the parent's host diagnostics generation here advances the epoch
/// that every computation stamps into its write and every read re-validates
/// against the live counter (the same read-side-authoritative rail
/// host-driven recompiles already use), so a computation that began before
/// the arm can never satisfy a read that happens after it. The entry drop
/// stays only to free the known-dead value eagerly.
fn arm_open_importer_republish(
    deps: &SyncCoordinatorDeps,
    settled_edits: &[String],
    pending_files: &mut HashMap<String, (Instant, PendingSignal)>,
) {
    if settled_edits.is_empty() {
        return;
    }
    // The OPEN documents are the bound: enumerate them once (their canonical
    // ids and the client's own URI serialization, which keys the diagnostic
    // caches), then test each against the reverse closure — never a
    // document-map scan per closure member.
    let open_documents: Vec<(String, String)> = deps
        .documents
        .open_uris()
        .into_iter()
        .filter_map(|uri_str| {
            let uri: Uri = uri_str.parse().ok()?;
            Some((deps.documents.get_canonical_id(&uri)?, uri_str))
        })
        .collect();
    if open_documents.is_empty() {
        return;
    }
    let workspace = deps.documents.host().workspace_read();
    let received_at = Instant::now();
    for edited in settled_edits {
        let affected: std::collections::HashSet<String> =
            workspace.affected_canonicals(edited).into_iter().collect();
        for (importer, importer_uri) in &open_documents {
            if importer == edited || !affected.contains(importer) {
                continue;
            }
            tracing::debug!(
                "sync_coordinator: arming open importer {importer} after settled edit of {edited}"
            );
            deps.documents.host().bump_diagnostics_generation(importer);
            deps.cached_verter_diags.remove(importer_uri);
            pending_files
                .entry(importer.clone())
                .and_modify(|(changed_at, pending)| {
                    *changed_at = (*changed_at).max(received_at);
                    pending.force_diagnostics = true;
                    pending.received_at = pending.received_at.max(received_at);
                })
                .or_insert((
                    received_at,
                    PendingSignal {
                        uri: importer_uri.clone(),
                        requires_sync: false,
                        force_diagnostics: true,
                        received_at,
                    },
                ));
        }
    }
}

/// Refresh the carrier's IDE surface for the revision the debounce just
/// settled on: load it, recompile it, and install a provider projection if the
/// document has none.
///
/// This is the debounced tick's COMPILE half, called from the tick itself
/// rather than from the provider sync, and gated ONLY on the document still
/// being open. The document commit owes only the document's text — it stopped
/// compiling per keystroke, which is
/// https://github.com/pikax/verter/issues/96 — so this tick is what owes the
/// IDE surface for the settled revision.
///
/// That debt belongs to the REVISION, not to the provider and not to the sync:
/// `upsert` clears the file's `latest_diagnostics` on any semantic change, and
/// `get_diagnostics` is a pure cached read that never compiles, so without this
/// compile Verter's OWN template and parse diagnostics go EMPTY and stay empty.
/// Nothing about that involves a type provider — the shipping default route has
/// none (`--type-provider=editor-tsserver` installs no local provider; the
/// editor's own tsserver serves TypeScript) — and nothing about it requires a
/// provider sync either, which is why a style-only edit, whose whole point is
/// that it needs no provider work, still reaches this.
///
/// Carrier-agnostic: `ensure_ide_compiled` answers `Ok(false)` for anything
/// with no IDE surface, so a self-file document (rune module, plain script)
/// pays a source lookup and nothing else, while Vue and Svelte carriers refresh
/// identically.
///
/// Bounded by the debounce, never by keystroke: one quiet window, one compile.
fn refresh_carrier_ide_surface(deps: &SyncCoordinatorDeps, canonical_id: &str) {
    // Nothing is owed for a document that is no longer open. `did_change`
    // leaves a pending signal behind and `did_close` does not cancel it, so an
    // edit-then-close lands here after the close has already removed the
    // document AND evicted the host source. Without this gate `ensure_loaded`
    // would RESURRECT the file from disk and pull its dependency closure in
    // for a buffer nobody is looking at — and the publication that follows
    // finds no document and drops the result anyway. This is the open-document
    // check the recovery helper this replaced performed first.
    //
    // `sync_file`'s provider arm is deliberately NOT gated on it, and keeps its
    // own `ensure_ide_compiled`: retracting or clearing a closed carrier's
    // provider state is work a close still owes, and that arm also syncs
    // carriers that were never open (a workspace file whose `.tsx` an importer
    // needs).
    if deps.documents.canonical_id_to_uri(canonical_id).is_none() {
        return;
    }
    // The compile below reads the file and the dependency closure this loads.
    // A fast path for an already-loaded open document, which is every document
    // reaching this tick.
    deps.documents.host().ensure_loaded(canonical_id);
    let profile = deps.documents.tsx_profile.read().clone();
    let compiled = crate::server::block_in_place_guarded(|| {
        deps.documents
            .host
            .ensure_ide_compiled(canonical_id, &profile)
    });
    if !compiled.unwrap_or(false) {
        return;
    }
    // A carrier whose open-time compile FAILED has no provider projection and
    // fails closed on every capture until a repair path compiles one. The
    // interactive repair heals it on the next provider-backed request, but only
    // attempt-bounded and only when a request arrives — this tick is the
    // request-INDEPENDENT recovery. Cache read only — the compile just above
    // already ran — and a no-op once a projection exists, so it never disturbs
    // the steady-state carry.
    deps.documents
        .install_missing_carrier_projection(canonical_id);
}

/// Perform the actual sync: sync TSX/DTS to the type provider.
///
/// The carrier's IDE surface is NOT this function's job — the tick refreshes it
/// through [`refresh_carrier_ide_surface`] before calling here, because a
/// revision can owe that recompile without owing any provider work. A
/// provider-less route (`deps.project_sync == None`) therefore has nothing to
/// do here at all; the tick still publishes Verter-owned diagnostics
/// afterwards.
async fn sync_file(deps: &SyncCoordinatorDeps, canonical_id: &str, _uri_str: &str) {
    let Some(project_sync) = deps.project_sync.as_ref() else {
        return;
    };
    tracing::info!("sync_coordinator: SYNC_START {canonical_id}");
    // Re-readable: the self-file sync below revalidates the published snapshot
    // AFTER its provider await, so it needs the accessor, not one capture.
    let published_snapshot = || {
        let ws = deps.vfs_workspace.read();
        ws.as_ref().and_then(|ws| {
            let published = ws.load_published()?;
            Some(crate::server::PublishedResolverSnapshot {
                resolver: published.snapshot.resolver.clone(),
                resolution_view: Some(crate::server::PublishedResolutionView {
                    workspace: Arc::clone(ws),
                    published: Arc::clone(&published),
                }),
                ownership_ready: published.ownership_ready,
            })
        })
    };
    let Some(snapshot) = published_snapshot() else {
        tracing::debug!(
            "sync_coordinator: deferring sync without resolver snapshot {canonical_id}"
        );
        deps.pending_snapshot_provider_sync
            .insert(canonical_id.to_string());
        return;
    };

    // A self-file document (a `.svelte.ts` / `.svelte.js` rune module OR a
    // plain TS-family script) is NOT a carrier — it serves its OWN-path
    // provider buffer (`<rune prelude> + <rewritten module bytes>` for a rune
    // module, the source verbatim for a plain script), has no IDE TSX, and its
    // provider state lives in the Shadow slot keyed at its own canonical path.
    // Route it through the SHARED self-file shadow-sync path (the SAME one the
    // editor ingress uses) so the debounced tick (a) uses the generalized
    // projection for diagnostics and (b) never clobbers the Shadow state via
    // the carrier-miss `preserve_open_unresolved_carrier`, which would
    // overwrite it with an IDE-path state and break did_close cleanup.
    if let Some(file_language) = crate::server::self_file_language_for(canonical_id) {
        if let Some(uri) = deps.documents.canonical_id_to_uri(canonical_id) {
            crate::server::sync_self_file_shadow_state(
                &deps.documents,
                project_sync,
                &deps.provider_sync_states,
                &published_snapshot,
                &uri,
                canonical_id,
                &file_language,
                deps.type_provider_kind.requires_explicit_source_graph(),
            )
            .await;
        } else if snapshot.ownership_ready {
            // A genuinely non-open rune module is removed once ready.
            clear_provider_sync_state(
                project_sync,
                deps.documents.provider_surfaces(),
                &deps.provider_sync_states,
                canonical_id,
                &deps.carrier_transaction_coordinator,
            )
            .await;
        }
        return;
    }

    // Pin the open document's exact revision BEFORE compiling, if any is
    // open. Capturing the pin first — rather than after `get_ide` returns, as
    // this used to — guarantees the pinned revision is never LATER than the
    // revision that actually produces `ide.code` below: a `did_change` landing
    // during the compile can only advance the LIVE identity past this pin,
    // which makes the later current-identity check MISS (fail closed, safe) —
    // it can never falsely MATCH a torn pair. `None` for a closed carrier (no
    // live document to race against; the unguarded record stays correct there,
    // same as every other closed-file producer path).
    let uri_for_open_identity = deps.documents.canonical_id_to_uri(canonical_id);
    let ide_compile_revision = uri_for_open_identity
        .as_ref()
        .and_then(|uri| deps.documents.snapshot_identity(uri));

    // Sync IDE (TSX) output to type provider. IDE-sync: drive the IDE/TSX
    // surface (not the runtime `Main`) so a Main-less carrier (Svelte)
    // populates its `CachedTsx` before the `get_ide` read below.
    //
    // Kept even though `refresh_carrier_ide_surface` ran above, and NOT folded
    // into it: that refresh serves the OPEN document, while the provider arm
    // also syncs carriers with no open document at all (a workspace file whose
    // `.tsx` an importer needs). For an open document this is a warm hit on the
    // slot the refresh just filled — the same profile, so the same normalized
    // slot — and a warm hit starts no cold run.
    let profile = deps.documents.tsx_profile.read().clone();
    let _ = tokio::task::block_in_place(|| {
        deps.documents
            .host
            .ensure_ide_compiled(canonical_id, &profile)
    });
    tracing::info!("sync_coordinator: HOST_GET_IDE_START {canonical_id}");
    let ide = tokio::task::block_in_place(|| deps.documents.host.get_ide(canonical_id, &profile));
    let is_jsx = ide.as_ref().map(|ide| ide.is_jsx).unwrap_or(false);

    // TEST SEAM: a one-shot pause, keyed by canonical id, that fires HERE —
    // immediately after the compile above, at the exact source position the
    // pin capture USED to sit at before this fix moved it earlier. A test can
    // land a `did_change` during this pause to prove the pin (already
    // captured above, before the compile) stays anchored to the PRE-edit
    // revision instead of drifting to whatever capturing it here would
    // observe. See `test_hooks::block_after_ide_compile`.
    #[cfg(test)]
    test_hooks::maybe_pause_after_ide_compile(canonical_id).await;
    // The pin, in the shape the shared fenced-record entry point expects.
    let open_pin = match (&uri_for_open_identity, &ide_compile_revision) {
        (Some(uri), Some(revision)) => Some((uri, revision)),
        _ => None,
    };

    // The tsserver carrier-membership context: the debounced carrier reaches the
    // provider as a store-backed configured-project member. Clone the VFS handle in
    // its own statement so the `RwLockReadGuard` is dropped BEFORE any await. tsgo
    // (no coordinator) ⇒ `None` ⇒ the gateway returns a direct-open transition.
    let vfs = deps.vfs_workspace.read().clone();
    let membership = deps
        .carrier_publish_coordinator
        .as_ref()
        .map(|coordinator| crate::external_ts::CarrierMembershipCtx {
            coordinator,
            provider_delivery: if matches!(deps.type_provider_kind, crate::TypeProviderKind::Tsgo) {
                crate::external_ts::CarrierProviderDelivery::DirectOpen
            } else {
                crate::external_ts::CarrierProviderDelivery::StoreBacked
            },
            activate_provider_member: deps.documents.canonical_id_to_uri(canonical_id).is_some(),
        });

    // Route the freshly-debounced carrier through the SINGLE carrier-sync gateway:
    // tsserver PUBLISHES the companions into the store the plugin reads (refreshing
    // the store content for the next pull), tsgo opens the companions directly, and an
    // owner loss RETRACTS the membership + preserves an open document / clears a
    // closed one. The receipt gates every commit. Ownership resolves from the SAME
    // published `vfs` for both engines.
    match crate::external_ts::reconcile_carrier_source(crate::external_ts::CarrierSyncRequest {
        host: deps.documents.host(),
        vfs: vfs.as_deref(),
        ownership_ready: snapshot.ownership_ready,
        resolver: &snapshot.resolver,
        provider_sync_states: &deps.provider_sync_states,
        provider_surfaces: deps.documents.provider_surfaces(),
        documents: Some(&deps.documents),
        project_sync: Some(project_sync),
        canonical_id,
        is_jsx,
        ide: ide.as_ref(),
        open_pin,
        membership,
        admission: &deps.carrier_transaction_coordinator,
        reason: crate::external_ts::ReconcileReason::SourceSynced,
    })
    .await
    {
        crate::external_ts::CarrierSyncDecision::Published {
            committed_state,
            receipt,
        } => {
            // The plugin serves both store-resident companions: no buffer I/O.
            if deps.carrier_transaction_coordinator.admit_owned(
                deps.documents.host(),
                &deps.provider_sync_states,
                canonical_id,
                committed_state,
                &receipt,
            ) == crate::external_ts::AdmitOutcome::Superseded
            {
                deps.pending_snapshot_provider_sync
                    .insert(canonical_id.to_string());
            }
        }
        crate::external_ts::CarrierSyncDecision::DirectOpen {
            transition,
            pending,
        } => {
            // Close-AFTER-successful-sync (per-kind, skip-active): capture stale +
            // prior state, sync each kind, then commit and close only genuinely-
            // stale paths. The coordinator can touch an OPEN file, so a failed
            // replacement sync must never close the live path nor commit an unsynced
            // path. The readiness receipt is minted from `pending` only AFTER a kind
            // opened (below); if no kind syncs, the pending drops unconfirmed and
            // nothing commits.
            let previous_state = deps
                .provider_sync_states
                .get(canonical_id)
                .map(|entry| entry.clone());
            let stale_paths = transition.stale_paths;
            let mut committed_state = transition.next;
            let mut synced_kinds: Vec<ProviderPathKind> = Vec::new();

            if let Some(ide) = ide.as_ref() {
                if let Some(ide_path) = committed_state.ide_path.clone() {
                    tracing::info!("sync_coordinator: TSX_SYNC_START {ide_path}");
                    let result = if committed_state.ide_background_loaded {
                        project_sync.sync_tsx(&ide_path, &ide.code).await
                    } else {
                        project_sync.open_tsx(&ide_path, &ide.code).await
                    };
                    match result {
                        Ok(()) => {
                            committed_state.set_background_loaded(ProviderPathKind::Ide, true);
                            synced_kinds.push(ProviderPathKind::Ide);
                            // Record a fresh generation pinning the EXACT IDE bytes
                            // just synced (interactive queries capture this surface),
                            // through the shared fenced choke point: `open_pin` was
                            // captured BEFORE the compile above, so the two awaits
                            // just run (open_tsx/sync_tsx) giving a concurrent
                            // `did_change` a window to land can only make this
                            // record fail closed, never falsely pair `ide.code` with
                            // a source it wasn't compiled from. Same hazard, same
                            // fence shape, as
                            // `VerterLanguageServer::record_carrier_ide_snapshot_if_current`
                            // on the interactive repair path.
                            if let Some(delivered) =
                                project_sync.carrier_provider_surface(&ide_path, &ide.code)
                            {
                                crate::provider_surface_store::record_carrier_ide_surface_fenced(
                                    deps.documents.provider_surfaces(),
                                    Some(&deps.documents),
                                    deps.documents.host(),
                                    canonical_id,
                                    &ide_path,
                                    &delivered,
                                    ide.source_map.as_deref(),
                                    open_pin,
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!("sync_coordinator: tsx sync failed for {ide_path}: {e}")
                        }
                    }
                    tracing::info!("sync_coordinator: TSX_SYNC_DONE {ide_path}");
                }
            }

            let api = match tokio::task::block_in_place(|| {
                deps.documents.host.get_public_api(canonical_id)
            }) {
                Ok(api) => api,
                Err(error) => {
                    crate::report_public_api_projection_error(
                        "sync_coordinator",
                        canonical_id,
                        &error,
                    );
                    return;
                }
            };
            if let Some(api) = api {
                if let Some(dts_path) = committed_state.api_path.clone() {
                    // Destination-keyed rendering (the `.verter.ts` companion
                    // is TypeScript-labeled whatever the SFC's dialect);
                    // stamp/record the SAME bytes that were delivered.
                    let api_code = api.code_for_companion_path(&dts_path);
                    let result = if committed_state.api_background_loaded {
                        project_sync.sync_dts(&dts_path, api_code).await
                    } else {
                        project_sync.open_dts(&dts_path, api_code).await
                    };
                    match result {
                        Ok(()) => {
                            committed_state.mark_api_delivered(api_code);
                            synced_kinds.push(ProviderPathKind::Api);
                            // Record a fresh generation pinning the EXACT content
                            // just synced under this virtual path.
                            crate::provider_surface_store::record_carrier_api_surface(
                                deps.documents.provider_surfaces(),
                                Some(&deps.documents),
                                deps.documents.host(),
                                canonical_id,
                                &dts_path,
                                api_code,
                                api.source_map.as_deref(),
                            );
                        }
                        Err(e) => {
                            tracing::warn!("sync_coordinator: dts sync failed for {dts_path}: {e}")
                        }
                    }
                }
            }

            if !synced_kinds.is_empty() {
                revert_unsynced_kinds(&mut committed_state, previous_state.as_ref(), &synced_kinds);
                let genuinely_stale =
                    genuinely_stale_after_sync(&stale_paths, &committed_state, &synced_kinds);
                // A kind opened: NOW mint the receipt (post-open), attesting EXACTLY the
                // kinds that actually opened this pass, and commit through the coordinator.
                let ide_surface = committed_state
                    .ide_path
                    .as_deref()
                    .and_then(|path| project_sync.synced_tsx_surface(path));
                let receipt = pending.confirm_opened_with_ide_surface(&synced_kinds, ide_surface);
                // Gate the stale-path close on ADMISSION: a `Superseded` commit (a newer
                // transaction reclaimed the source, or an owner-loss advanced the barrier)
                // requeues and closes NOTHING — the computed stale paths may be the newer
                // transaction's LIVE buffers. Only an admitted commit closes them (and
                // `close_stale_paths` retires any closed `Api` surface's active generation in
                // the provider-surface store so a closed `{carrier}.ts` is never later
                // vouched as current by a rename).
                if deps.carrier_transaction_coordinator.admit_owned(
                    deps.documents.host(),
                    &deps.provider_sync_states,
                    canonical_id,
                    committed_state,
                    &receipt,
                ) == crate::external_ts::AdmitOutcome::Superseded
                {
                    deps.pending_snapshot_provider_sync
                        .insert(canonical_id.to_string());
                } else {
                    close_stale_paths(
                        project_sync,
                        deps.documents.provider_surfaces(),
                        &non_decl_close_targets(&genuinely_stale),
                    )
                    .await;
                }
            }
        }
        crate::external_ts::CarrierSyncDecision::NotOwned(not_owned) => {
            // Settle the non-owned disposition through the coordinator (requeue the
            // transient `NotReady`/`Pending`, advance the owner-loss barrier for the
            // terminal `Unresolved`), then run the editor-liveness buffer conversion for a
            // settled no-owner class. Editor-liveness invariant: an OPEN `.vue` keeps its
            // TSX live as Unresolved open-document state — NEVER clear+close; only a
            // genuinely non-open file is removed (and only once ready). The gateway already
            // RETRACTED the STORE/ledger membership.
            let class = deps.carrier_transaction_coordinator.settle(
                not_owned,
                canonical_id,
                Some(&deps.pending_snapshot_provider_sync),
            );
            if class.runs_buffer_cleanup() {
                if deps.documents.canonical_id_to_uri(canonical_id).is_some() {
                    preserve_open_unresolved_carrier(
                        deps,
                        project_sync,
                        canonical_id,
                        is_jsx,
                        ide.as_ref(),
                        open_pin,
                    )
                    .await;
                } else if snapshot.ownership_ready {
                    clear_provider_sync_state(
                        project_sync,
                        deps.documents.provider_surfaces(),
                        &deps.provider_sync_states,
                        canonical_id,
                        &deps.carrier_transaction_coordinator,
                    )
                    .await;
                }
            }
        }
    }
    if deps.carrier_publish_coordinator.is_some() {
        deps.client
            .send_notification::<crate::server::protocol_types::TypeProviderSyncComplete>(
                crate::server::protocol_types::TypeProviderSyncCompleteParams {
                    gen: deps
                        .documents
                        .host()
                        .last_content_transition_generation(canonical_id),
                },
            )
            .await;
    }
    tracing::info!("sync_coordinator: SYNC_DONE {canonical_id}");
}

/// Preserve (or create) an OPEN Vue document's unresolved provider state when
/// the coordinator's ready snapshot resolves no owner, keeping its IDE TSX live.
///
/// Editor-liveness invariant: builds the commit state through the shared
/// [`open_unresolved_carrier_state`] primitive (forces `Unresolved`, preserves the
/// owner-independent live IDE path, drops the owner-derived API path), syncs the
/// IDE TSX when fresh code is available, and commits. It NEVER removes the state
/// or closes the TSX.
async fn preserve_open_unresolved_carrier(
    deps: &SyncCoordinatorDeps,
    project_sync: &ProjectSync,
    canonical_id: &str,
    is_jsx: bool,
    ide: Option<&verter_session::IdeResponse>,
    open_pin: Option<(
        &tower_lsp_server::ls_types::Uri,
        &crate::documents::DocumentSnapshotIdentity,
    )>,
) {
    let previous = deps
        .provider_sync_states
        .get(canonical_id)
        .map(|entry| entry.clone());
    // Converting a previously-committed OWNED carrier to Unresolved is an owner-loss for the
    // admission barrier: advance it so a late owned token can never resurrect the obsolete
    // owner into the now-unstamped slot.
    if previous
        .as_ref()
        .is_some_and(|state| state.commit_stamp.is_some())
    {
        deps.carrier_transaction_coordinator
            .advance_barrier(canonical_id);
    }
    // The DESIRED Unresolved target: owner-independent desired-extension IDE
    // path + the open-vs-update syncability hint. Binding forced `Unresolved`,
    // owner-derived API dropped.
    let target = open_unresolved_carrier_state(previous.as_ref(), canonical_id, is_jsx);

    // Attempt the desired IDE sync when fresh code is available (update-in-place
    // when the desired path is already live, else first-open).
    let mut ide_synced = false;
    if let (Some(ide), Some(ide_path)) = (ide, target.ide_path.clone()) {
        let result = if target.ide_background_loaded {
            project_sync.sync_tsx(&ide_path, &ide.code).await
        } else {
            project_sync.open_tsx(&ide_path, &ide.code).await
        };
        match result {
            Ok(()) => {
                ide_synced = true;
                // Record a fresh generation pinning the EXACT IDE bytes just
                // synced (interactive queries capture this surface), through the
                // shared fenced choke point: `open_pin` was captured by the
                // caller BEFORE this compile, so the just-run provider await
                // can only make this record fail closed, never falsely pair
                // `ide.code` with a source it wasn't compiled from.
                if let Some(delivered) = project_sync.carrier_provider_surface(&ide_path, &ide.code)
                {
                    crate::provider_surface_store::record_carrier_ide_surface_fenced(
                        deps.documents.provider_surfaces(),
                        Some(&deps.documents),
                        deps.documents.host(),
                        canonical_id,
                        &ide_path,
                        &delivered,
                        ide.source_map.as_deref(),
                        open_pin,
                    );
                }
            }
            Err(error) => tracing::warn!(
                "sync_coordinator: failed to sync open unresolved IDE path {ide_path}: {error}"
            ),
        }
    }

    // Build the committed state + close targets through the SAME per-kind
    // discipline the owner-resolved path uses: a non-synced IDE kind RETAINS the
    // prior LIVE path (never dropped to a dead/None path while the prior is still
    // open — rows 7 & 9), the owner-derived API is dropped+closed unconditionally,
    // and the orphaned prior IDE path is closed ONLY after a successful flip.
    let commit = open_unresolved_carrier_commit(previous.as_ref(), target, ide_synced);
    commit_sync_transition(&deps.provider_sync_states, canonical_id, commit.committed);
    if let Some(dropped) = commit.dropped_api {
        close_stale_paths(
            project_sync,
            deps.documents.provider_surfaces(),
            &non_decl_close_targets(std::slice::from_ref(&dropped)),
        )
        .await;
    }
    if let Some(stale) = commit.stale_ide_after_success {
        close_stale_paths(
            project_sync,
            deps.documents.provider_surfaces(),
            &non_decl_close_targets(std::slice::from_ref(&stale)),
        )
        .await;
    }
}

async fn clear_provider_sync_state(
    sync: &ProjectSync,
    provider_surfaces: &crate::provider_surface_store::ProviderSurfaceStore,
    states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
) {
    // Advance-before-mutate: the coordinator advances the owner-loss barrier BEFORE it
    // vacates the slot when the removed state was a previously-committed carrier, so a late
    // owned token captured before this removal can never resurrect the obsolete owner.
    if let Some(state) = carrier_coordinator.advance_barrier_and_remove(states, canonical_id) {
        // The declaration overlay (`Decl`), if any, is released by `DeclOverlayOwner`
        // via the `did_close` lifecycle, never closed here — the generic close
        // touches only non-decl artifacts.
        close_stale_paths(sync, provider_surfaces, &state.active_non_decl_paths()).await;
    }
}

/// Close stale provider paths AND retire any closed `Api` surface's active
/// generation in the provider-surface store.
///
/// A closed `{carrier}.ts` API path is no longer the active synced virtual
/// surface: the store must `forget` it so a later cross-file rename's
/// `current_snapshot()` does not VOUCH the now-closed generation as current
/// (historical snapshots stay valid for any in-flight rename that already
/// captured them — `forget` only retires the active generation). This mirrors the
/// sibling [`crate::background_drain::close_stale_provider_paths`]; the
/// coordinator MUST forget too, or a coordinator-driven close leaves the store
/// vouching a stale surface (the fail-closed invariant relies on this).
async fn close_stale_paths(
    sync: &ProjectSync,
    provider_surfaces: &crate::provider_surface_store::ProviderSurfaceStore,
    stale_paths: &[(NonDeclProviderPathKind, String)],
) {
    for (kind, path) in stale_paths {
        // Retire EVERY closing store-backed surface (IDE / API / Shadow) under a
        // fresh close EPOCH (see the sibling
        // `background_drain::close_stale_provider_paths`): the `Closing` state
        // keeps the path failing closed until the provider close is CONFIRMED.
        // Retiring only the API role would leave a closed IDE / Shadow surface
        // `Current` — capturable by an interactive query against a CLOSED
        // provider buffer. Capture the epoch-stamped token so the finalize is
        // scoped to THIS close.
        let close_token = provider_surfaces.forget(path);
        // A declaration overlay (`Decl`) is unrepresentable here — its lifecycle is
        // owned by `DeclOverlayOwner`, never this generic close.
        let result = match kind {
            NonDeclProviderPathKind::Ide => sync.close_tsx(path).await,
            NonDeclProviderPathKind::Api => sync.close_dts(path).await,
            NonDeclProviderPathKind::Shadow => sync.close_file(path).await,
        };
        match result {
            // Only a CONFIRMED close finalizes, and only via THIS close's token —
            // a reopen (or newer close) during the await makes the epoch mismatch
            // and the finalize a no-op (the fresh snapshot survives). An error
            // drops the token, leaving the `Closing` state (fail closed).
            Ok(()) => {
                provider_surfaces.finalize_close(close_token);
            }
            Err(error) => {
                tracing::warn!(
                    "sync_coordinator: failed to close stale provider path {path}: {error}"
                );
            }
        }
    }
}

/// Merge a self-file document's debounced diagnostics through the generalized
/// SELF-FILE projection: query the type provider at the document's OWN
/// canonical path (the Shadow provider buffer — `<rune prelude> + <rewritten
/// module bytes>` for a rune module, the source verbatim for a plain script),
/// then map each type diagnostic back to the user-source position
/// through the rewrite-aware self-file mapper (prelude offset + per-line
/// rewrite delta).
///
/// Built EXCLUSIVELY from ONE captured immutable
/// [`ProviderSurfaceSnapshot`](crate::provider_surface_store::ProviderSurfaceSnapshot)
/// (`capture_committed_shadow_surface`): the provider buffer bytes, mapper, and
/// carrier source all come from the same recorded surface, so the tuple can
/// never be torn by a concurrent re-sync/close. After the provider await the
/// captured surface is RE-VALIDATED; on mismatch the provider diagnostics are
/// DROPPED and the Verter-only set publishes (fail closed). Falls back to the
/// verter diagnostics alone when no capturable Shadow surface exists or the
/// provider errors.
async fn self_file_diagnostics(
    deps: &SyncCoordinatorDeps,
    tp: &dyn TypeProvider,
    canonical_id: &str,
    verter_diags: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    let store = deps.documents.provider_surfaces();
    let Some(snapshot) = crate::provider_surface_store::capture_committed_shadow_surface(
        store,
        &deps.provider_sync_states,
        &deps.documents,
        canonical_id,
    ) else {
        return verter_diags;
    };
    // No usable mapper ⇒ the provider's offsets could not be mapped back onto
    // the module source ⇒ fail closed to Verter-only.
    let Some(mapper) = snapshot.source_map.as_ref().map(|m| (**m).clone()) else {
        return verter_diags;
    };

    let encoding = deps.position_encoding.read().clone();
    let provider_li = LineIndex::new(&snapshot.provider_content, encoding.clone());
    let source_li = LineIndex::new(&snapshot.carrier_source, encoding.clone());
    let encoding_for_related = encoding;

    match tp.get_diagnostics(canonical_id).await {
        Ok(type_diags) => {
            // Post-await validation: diagnostics produced against a surface that
            // no longer matches must be DROPPED (fail closed).
            if !crate::provider_surface_store::captured_surface_still_valid_for_canonical(
                store,
                &deps.documents,
                canonical_id,
                &snapshot,
            ) {
                tracing::debug!(
                    "sync_coordinator: dropping self-file provider diagnostics for \
                     {canonical_id} — captured surface no longer valid"
                );
                return verter_diags;
            }
            // Related-span map-back: a same-file related span maps through the
            // in-context mapper; a real `.ts` related span reads its own source via
            // the VFS reader. A FOREIGN carrier `.tsx` related span needs the
            // server-side external resolver (unavailable on this background path)
            // and drops fail-closed (`external_resolver: None`).
            let carrier_source_exists = |p: &str| deps.documents.host().get_source(p).is_some();
            merge::merge_diagnostics(
                verter_diags,
                type_diags,
                canonical_id,
                &provider_li,
                &mapper,
                &source_li,
                None,
                &carrier_source_exists,
                encoding_for_related,
                &|p: &str| {
                    crate::server::block_in_place_guarded(|| {
                        deps.documents.host().workspace_read().read_file(p)
                    })
                },
            )
        }
        Err(error) => {
            tracing::warn!(
                "sync_coordinator: type provider error for self-file document {canonical_id}: {error}"
            );
            verter_diags
        }
    }
}

/// Publish merged (Verter lint + TypeScript type) diagnostics for a synced file.
///
/// Recomputes fresh verter diagnostics (host errors + lint rules) for the current
/// document version, then merges with fresh TS diagnostics from the type provider.
/// This ensures lint violations introduced during typing appear without reopening.
async fn publish_merged_diagnostics(deps: &SyncCoordinatorDeps, canonical_id: &str, uri_str: &str) {
    let uri: Uri = match uri_str.parse() {
        Ok(u) => u,
        Err(_) => return,
    };
    let Some(snapshot) = deps.documents.snapshot_identity(&uri) else {
        return;
    };
    let verter_diagnostics = compute_verter_diagnostics(deps, canonical_id, &uri);
    tracing::debug!(
        "sync_coordinator: VERTER_DIAGNOSTICS_READY {canonical_id} count={} codes={}",
        verter_diagnostics.len(),
        verter_diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.code.as_ref())
            .map(|code| match code {
                NumberOrString::Number(value) => value.to_string(),
                NumberOrString::String(value) => value.clone(),
            })
            .collect::<Vec<_>>()
            .join("|")
    );
    if !deps.documents.snapshot_identity_is_current(&uri, &snapshot) {
        return;
    }
    // Framework/native diagnostics are an independent snapshot lane. Publish
    // them as soon as the current revision is available; a cold TypeScript
    // configured-project build must not starve lint, ownership, or framework
    // declaration hints. The provider result replaces this staged batch below.
    deps.client
        .publish_diagnostics(
            uri.clone(),
            verter_diagnostics.clone(),
            Some(snapshot.version),
        )
        .await;

    if deps.type_provider.is_none() {
        return;
    }
    let diagnostics = merge_provider_diagnostics(deps, canonical_id, verter_diagnostics).await;
    // Revalidate after every provider await. Provider synchronization and API
    // reconciliation deliberately share a work bit, so only the editor's LSP
    // version is a valid freshness authority for diagnostics publication.
    if !deps.documents.snapshot_identity_is_current(&uri, &snapshot) {
        return;
    }
    deps.client
        .publish_diagnostics(uri, diagnostics, Some(snapshot.version))
        .await;
}

/// Compute the merged (Verter lint + `verter(project)` ownership + TypeScript
/// type) diagnostics for a synced file WITHOUT publishing. Split from
/// [`publish_merged_diagnostics`] so tests can observe the merged set directly
/// (the coordinator otherwise pushes to the client socket, which a test cannot
/// read) — the same compute/publish split the request-side
/// `compute_full_diagnostics` uses.
#[cfg(test)]
async fn compute_merged_diagnostics(
    deps: &SyncCoordinatorDeps,
    canonical_id: &str,
    uri: &Uri,
) -> Vec<Diagnostic> {
    let verter_diags = compute_verter_diagnostics(deps, canonical_id, uri);
    merge_provider_diagnostics(deps, canonical_id, verter_diags).await
}

/// Compute diagnostics owned by Verter without entering the TypeScript
/// provider. This stage is synchronous with respect to provider I/O and is safe
/// to publish while the provider's background semantic pass is still pending.
fn compute_verter_diagnostics(
    deps: &SyncCoordinatorDeps,
    canonical_id: &str,
    uri: &Uri,
) -> Vec<Diagnostic> {
    // The complete Verter-owned set from the ONE shared composer. `did_open` /
    // `did_change` route through this coordinator, and every other publisher
    // (both background-init sweeps, the pull `textDocument/diagnostic` path)
    // replaces the client's whole list for the document — so all of them must
    // assemble the same categories or the last writer silently erases the rest.
    let mut verter_diags = {
        let vfs_ws = deps.vfs_workspace.read();
        crate::server::verter_owned_diagnostics(
            &deps.documents,
            uri,
            canonical_id,
            &deps.cached_verter_diags,
            vfs_ws.as_deref(),
            deps.project_sync.as_ref(),
        )
    };

    // When a type provider serves this session, suppress component usage
    // diagnostics (unknown-prop, unknown-model) since the provider validates
    // props via the generated TSX and is the source of truth. Gated on the
    // serving KIND, not the in-process provider object: the editor-owned
    // tsserver plugin route validates props through the editor's tsserver
    // while `type_provider` is `None` in-process.
    if !matches!(deps.type_provider_kind, crate::TypeProviderKind::None) {
        verter_diags.retain(|d| match &d.code {
            Some(NumberOrString::String(code)) => {
                code != "verter/unknown-prop" && code != "verter/unknown-model"
            }
            _ => true,
        });
    }

    verter_diags
}

async fn merge_provider_diagnostics(
    deps: &SyncCoordinatorDeps,
    canonical_id: &str,
    verter_diags: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    // A self-file document (rune module or plain TS-family script) has NO IDE
    // TSX — its provider buffer is served from its OWN canonical path. Route
    // its debounced diagnostics through the generalized self-file projection
    // (the document's rewrite-aware mapper + own-path provider buffer), so
    // type diagnostics land at the correctly offset source position — NOT
    // through the carrier IDE-source-map path below (which requires an
    // `ide_path` a self-file document never has).
    if let Some(tp) = &deps.type_provider {
        if crate::server::self_file_language_for(canonical_id).is_some() {
            return self_file_diagnostics(deps, tp.as_ref(), canonical_id, verter_diags).await;
        }
    }

    let Some(tp) = &deps.type_provider else {
        return verter_diags;
    };
    let encoding = deps.position_encoding.read().clone();
    carrier_provider_diagnostics(
        &deps.documents,
        &deps.provider_sync_states,
        tp.as_ref(),
        encoding,
        canonical_id,
        verter_diags,
    )
    .await
}

/// Merge a carrier's provider type diagnostics into `verter_diags` for a
/// BACKGROUND publish (the debounced coordinator and the post-init/post-scan
/// publishers).
///
/// Built EXCLUSIVELY from ONE captured immutable
/// [`ProviderSurfaceSnapshot`](crate::provider_surface_store::ProviderSurfaceSnapshot)
/// (`capture_committed_carrier_ide_surface`): the provider path, content,
/// mapper, and carrier source all come from the same recorded surface, so the
/// tuple can never be torn by a concurrent re-sync/close. After the provider
/// await the captured surface is RE-VALIDATED (still honored + open document
/// still matches); on mismatch the provider diagnostics are DROPPED and the
/// Verter-only set publishes (fail closed) — the debounced coordinator
/// republishes after the next sync lands. Returns `verter_diags` unchanged
/// when the query context is unavailable.
pub(crate) async fn carrier_provider_diagnostics(
    documents: &DocumentRegistry,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    tp: &dyn crate::type_provider::traits::TypeProvider,
    encoding: PositionEncodingKind,
    canonical_id: &str,
    verter_diags: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    let store = documents.provider_surfaces();
    let Some(snapshot) = crate::provider_surface_store::capture_committed_carrier_ide_surface(
        store,
        provider_sync_states,
        documents,
        canonical_id,
    ) else {
        tracing::debug!(
            "carrier_provider_diagnostics: no committed current IDE surface for {canonical_id}"
        );
        return verter_diags;
    };
    // No usable source map ⇒ the provider's offsets could not be mapped back
    // onto the carrier ⇒ fail closed to Verter-only.
    let Some(mapper) = snapshot.source_map.as_ref().map(|m| (**m).clone()) else {
        tracing::debug!(
            "carrier_provider_diagnostics: current IDE surface has no source map for {canonical_id}"
        );
        return verter_diags;
    };
    let tsx_path = snapshot.stamp.provider_path.to_string();
    let tsx_li = LineIndex::new(&snapshot.provider_content, encoding.clone());
    let carrier_li = LineIndex::new(&snapshot.carrier_source, encoding.clone());

    match tp.get_diagnostics(&tsx_path).await {
        Ok(type_diags) => {
            // Post-await validation: diagnostics produced against a surface that
            // no longer matches must be DROPPED (fail closed).
            if !crate::provider_surface_store::captured_surface_still_valid_for_canonical(
                store,
                documents,
                canonical_id,
                &snapshot,
            ) {
                tracing::debug!(
                    "carrier_provider_diagnostics: dropping provider diagnostics for {} — \
                     captured surface no longer valid",
                    canonical_id
                );
                return verter_diags;
            }
            tracing::debug!(
                "carrier_provider_diagnostics: merge {} verter + {} type diags for {}",
                verter_diags.len(),
                type_diags.len(),
                canonical_id
            );
            // Related-span map-back: same-file related spans map through the
            // in-context mapper; real `.ts` related spans read their own
            // source via the VFS reader. A FOREIGN carrier `.tsx` related
            // span needs the server-side external resolver (unavailable on
            // this background path) → drops fail-closed (`None`).
            let carrier_source_exists = |p: &str| documents.host().get_source(p).is_some();
            merge::merge_diagnostics(
                verter_diags,
                type_diags,
                &tsx_path,
                &tsx_li,
                &mapper,
                &carrier_li,
                None,
                &carrier_source_exists,
                encoding,
                &|p: &str| {
                    crate::server::block_in_place_guarded(|| {
                        documents.host().workspace_read().read_file(p)
                    })
                },
            )
        }
        Err(e) => {
            tracing::warn!(
                "carrier_provider_diagnostics: type provider error for {}: {e}",
                canonical_id
            );
            verter_diags
        }
    }
}

/// TEST SEAM: a global (process-wide, canonical-id-keyed) one-shot pause,
/// independent of [`SyncCoordinatorDeps`] so no test's struct literal needs a
/// new field. A test [`block_after_ide_compile`] a canonical id before
/// calling `sync_file`; the FIRST subsequent `sync_file` pass for that id
/// notifies `arrived`, then blocks on `release` right after the compile —
/// the exact source position the pin capture sat at BEFORE this fix moved it
/// earlier — giving the test a window to commit an interleaved `did_change`
/// there and prove the (now-earlier) pin capture stays anchored to the
/// pre-edit revision instead of drifting.
///
/// GLOBAL means process-wide, not per-test: `cargo test` runs tests
/// concurrently in the SAME process, so two tests racing `sync_file` for the
/// SAME canonical id can steal each other's registration (`insert` replaces,
/// `remove` takes whatever is currently there) — an observed flaky failure
/// when a test reused the common `"/workspace/src/App.vue"` fixture literal.
/// Every caller of [`block_after_ide_compile`] MUST use a canonical id unique
/// across the whole crate's test suite (a distinctive filename is enough; a
/// unique workspace root, as `unique_server_ws_root` produces, also works).
#[cfg(test)]
pub(crate) mod test_hooks {
    use std::sync::LazyLock;

    use dashmap::DashMap;
    use tokio::sync::Notify;

    type PauseGates = (std::sync::Arc<Notify>, std::sync::Arc<Notify>);

    static PAUSE_AFTER_IDE_COMPILE: LazyLock<DashMap<String, PauseGates>> =
        LazyLock::new(DashMap::new);

    /// Register a one-shot pause for `canonical_id`. Returns `(arrived,
    /// release)`: await `arrived.notified()` to know the pause point was
    /// reached, then `release.notify_one()` to let `sync_file` proceed.
    pub(crate) fn block_after_ide_compile(canonical_id: &str) -> PauseGates {
        let arrived = std::sync::Arc::new(Notify::new());
        let release = std::sync::Arc::new(Notify::new());
        PAUSE_AFTER_IDE_COMPILE.insert(
            canonical_id.to_string(),
            (
                std::sync::Arc::clone(&arrived),
                std::sync::Arc::clone(&release),
            ),
        );
        (arrived, release)
    }

    /// Consume and honor a registered pause for `canonical_id`, if any. A
    /// no-op (no await) when nothing was registered — so untouched tests pay
    /// zero cost.
    pub(crate) async fn maybe_pause_after_ide_compile(canonical_id: &str) {
        if let Some((_, (arrived, release))) = PAUSE_AFTER_IDE_COMPILE.remove(canonical_id) {
            arrived.notify_one();
            release.notified().await;
        }
    }
}

#[cfg(test)]
#[path = "sync_coordinator_tests.rs"]
mod tests;

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex as SyncMutex;
use tokio::sync::Mutex as AsyncMutex;

use verter_tsgo_api::relay::CARRIER_SYNC_BARRIER_TIMEOUT;
use verter_type_runtime::protocol::TypeProviderError;

/// A resolved carrier wire operation an injection sink performs — the ordered state
/// machine ([`CarrierSyncState::drive`]) decides the action + version and hands the
/// sink the COALESCED content; the sink maps it onto the shim CONTROL channel.
pub(crate) enum CarrierWireOp {
    /// The carrier's FIRST reservation — send `didOpen` (version 1) with its content.
    Open { version: i64, content: Arc<str> },
    /// An already-reserved carrier — send `didChange` at a monotonic version.
    Change { version: i64, content: Arc<str> },
    /// Send `didClose` — remove the carrier's overlay from the shared Program. Issued in
    /// THREE roles (all end the doc): a top-level carrier close; the best-effort retract of a
    /// possibly-open Program file after a first-open barrier failed (result ignored — the slot
    /// is already marked `PossiblyOpenUnsynced`); and the BOUNDED reconcile that precedes a
    /// fresh `didOpen` for a `PossiblyOpenUnsynced` shell (a reconcile-close FAILURE aborts the
    /// open and fails closed to OWNED).
    Close,
}

/// Await a carrier `didClose` through `sink`, BOUNDED by `bound` — the caller passes
/// [`CarrierSyncState::close_barrier_bound`], which defaults to [`CARRIER_SYNC_BARRIER_TIMEOUT`]
/// (the same bound the shim's own carrier-sync barrier uses). `carrier_did_close` can block on a
/// wedged control/writer path; an unbounded await would stall the per-carrier gate — and every
/// later op serialized behind it — indefinitely. A timeout is mapped to a fail-closed `Err`, so
/// every caller treats a timed-out close EXACTLY like a failed one: the reconcile close aborts
/// the open, the normal close leaves the slot non-serveable / reconcilable, and the best-effort
/// retract is simply dropped — all fail closed to OWNED. `bound` is injectable so a test can
/// prove the internal timeout FIRES (a never-answering `didClose` returns `Err` within a short
/// bound) — removing the wrapper would leave that test to hang unbounded.
async fn bounded_carrier_close_with_timeout<S, Fut>(
    sink: &S,
    bound: Duration,
) -> Result<(), TypeProviderError>
where
    S: Fn(CarrierWireOp) -> Fut,
    Fut: Future<Output = Result<(), TypeProviderError>>,
{
    match tokio::time::timeout(bound, sink(CarrierWireOp::Close)).await {
        Ok(barrier) => barrier,
        Err(_elapsed) => Err(TypeProviderError::new(
            "shared carrier didClose exceeded the sync-barrier bound (fail-closed to OWNED)",
        )),
    }
}

/// The lifecycle op a coalesced submission resolves to — an injection carrying its
/// coalesced content, or a close.
#[derive(Clone)]
pub(crate) enum PendingKind {
    /// Inject (open/change) the carrier at the coalesced content.
    Inject(Arc<str>),
    /// Close (retract) the carrier overlay.
    Close,
}

/// The latest-pending coalescing cell for one carrier. A gate holder drains the NEWEST
/// submitted lifecycle op (`latest_*` — an inject-with-content OR a close) rather than
/// replaying each intermediate submission, and skips entirely when the newest has
/// already been committed (`latest_seq <= committed_seq`) by an earlier gate holder — so
/// a burst of edits (and a trailing close) reaches the Program in ~one barrier and the
/// LATEST op always wins: a close SUPERSEDES an older queued injection, and a newer
/// injection SUPERSEDES an older close (a genuine reopen).
pub(crate) struct PendingSubmission {
    /// The newest submitted op + its global submission sequence.
    latest_seq: u64,
    latest_kind: PendingKind,
    /// The highest submission sequence a gate holder has SUCCESSFULLY committed
    /// (barrier-synced). Advanced only on a successful commit.
    committed_seq: u64,
}

/// The per-carrier ORDERED lifecycle state machine.
///
/// Concurrent open/change/close on the SAME carrier URI (the host has multiple provider
/// sync paths — did_change eager, the debounced coordinator, foreground / background /
/// import — plus the close path, and does NOT serialize per carrier) used to desync the
/// SHARED overlay: a `didChange` could race ahead of the first `didOpen`, a first-open
/// timeout could retract a slot a concurrent change had promoted, or a `didClose` could
/// interleave with an in-flight injection and reopen a closed carrier (an op after a
/// committed close). This state machine SERIALIZES each carrier's wire send + barrier +
/// commit behind a per-carrier ASYNC gate (a `tokio::sync::Mutex`, correctly held across
/// the barrier await — never a sync lock across `.await`), COALESCES a burst of edits
/// (and a trailing close) to the latest op, and keeps the local slot view consistent
/// with the shared Program on failure/timeout. Open, change, AND close all flow through
/// the SAME gate + coalescing cell, so the newest submission always wins: a close
/// supersedes an older queued injection (no op after a committed close) and a newer
/// injection supersedes an older close (a genuine reopen). Fail-closed: a broken
/// connection surfaces as an `Err` the composite treats as OWNED.
pub(crate) struct CarrierSyncState {
    /// The barrier-SYNCED carrier slots (the ONLY content served / positioned from),
    /// keyed by forward-slashed carrier path.
    pub(crate) injected: SyncMutex<HashMap<String, CarrierSlot>>,
    /// Per-carrier async gates serializing each carrier's wire send + barrier + commit
    /// so lifecycle ops commit in submission order (an Open barrier before any Change;
    /// a close after a committed injection, never reopening it).
    pub(crate) gates: SyncMutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    /// Per-carrier latest-pending coalescing cells.
    pub(crate) pending: SyncMutex<HashMap<String, PendingSubmission>>,
    /// A GLOBAL monotonic `didChange` version counter. LSP requires only that each
    /// `didChange` version exceed the document's previous version; a single monotonic
    /// counter guarantees that for EVERY carrier. `didOpen` uses version 1; this starts
    /// at 2.
    next_version: AtomicI64,
    /// A GLOBAL monotonic submission-sequence counter (the coalescing discriminant).
    next_seq: AtomicU64,
    /// The bound each carrier `didClose` wire barrier is wrapped in
    /// ([`bounded_carrier_close_with_timeout`]) — the reconcile close, the best-effort retract,
    /// and the top-level close all honour it, so a wedged control/writer path cannot stall the
    /// per-carrier gate (and every op serialized behind it) indefinitely. Defaults to
    /// [`CARRIER_SYNC_BARRIER_TIMEOUT`]; a test constructor injects a SHORT bound to prove the
    /// internal timeout FIRES (a never-answering `didClose` returns a fail-closed `Err` within
    /// the bound).
    close_barrier_bound: Duration,
}

impl CarrierSyncState {
    pub(crate) fn new() -> Self {
        Self::with_close_barrier_bound(CARRIER_SYNC_BARRIER_TIMEOUT)
    }

    /// Construct with an explicit `close_barrier_bound` — the production entry
    /// ([`Self::new`]) passes [`CARRIER_SYNC_BARRIER_TIMEOUT`]; a test injects a small bound to
    /// prove the internal close timeout fires (mirroring the session-end
    /// `retract_open_carriers_within(budget)` seam in
    /// [`verter_tsgo_api::control`]).
    pub(crate) fn with_close_barrier_bound(close_barrier_bound: Duration) -> Self {
        Self {
            injected: SyncMutex::new(HashMap::new()),
            gates: SyncMutex::new(HashMap::new()),
            pending: SyncMutex::new(HashMap::new()),
            next_version: AtomicI64::new(2),
            next_seq: AtomicU64::new(1),
            close_barrier_bound,
        }
    }

    /// The last barrier-SYNCED content for a carrier (by the engine's canonicalization
    /// first, then the injected key) — the ONLY content served / positioned from.
    pub(crate) fn synced_content(&self, engine_carrier: &str, carrier: &str) -> Option<Arc<str>> {
        synced_content(&self.injected, engine_carrier, carrier)
    }

    /// The per-carrier async gate (get-or-insert). A brief sync lock fetches the `Arc`;
    /// the caller then awaits the async gate — the sync lock is never held across the
    /// await.
    pub(crate) fn gate_for(&self, carrier: &str) -> Arc<AsyncMutex<()>> {
        Arc::clone(self.gates.lock().entry(carrier.to_string()).or_default())
    }

    /// Record `kind` as the carrier's latest pending submission at `seq` (a later
    /// submission overwrites an earlier one — the coalescing target). An inject and a
    /// close share the one cell, so the newest op supersedes an older one of EITHER
    /// kind (a close supersedes a queued inject; a reopen supersedes an older close).
    pub(crate) fn record_pending(&self, carrier: &str, seq: u64, kind: PendingKind) {
        let mut pending = self.pending.lock();
        match pending.get_mut(carrier) {
            Some(p) if seq > p.latest_seq => {
                p.latest_seq = seq;
                p.latest_kind = kind;
            }
            Some(_) => {}
            None => {
                pending.insert(
                    carrier.to_string(),
                    PendingSubmission {
                        latest_seq: seq,
                        latest_kind: kind,
                        committed_seq: 0,
                    },
                );
            }
        }
    }

    /// The newest pending op still needing a sync — `None` when the latest has already
    /// been committed (an earlier gate holder synced this-or-newer op).
    pub(crate) fn take_drainable(&self, carrier: &str) -> Option<(u64, PendingKind)> {
        let pending = self.pending.lock();
        let p = pending.get(carrier)?;
        (p.latest_seq > p.committed_seq).then(|| (p.latest_seq, p.latest_kind.clone()))
    }

    /// Mark `seq` (and everything before it) as committed (barrier-synced), so a later
    /// gate holder for the same-or-older content skips the redundant sync.
    pub(crate) fn mark_committed(&self, carrier: &str, seq: u64) {
        if let Some(p) = self.pending.lock().get_mut(carrier) {
            p.committed_seq = p.committed_seq.max(seq);
        }
    }

    /// Prune a carrier's per-carrier gate + pending state when the carrier is FULLY IDLE. Both
    /// prune sites — the close arm after a SUCCESSFUL (or no-op) close, and the coalesced-away
    /// EARLY-RETURN path (THIS drive found nothing to drain because an earlier gate holder
    /// already committed the latest op) — share this ONE predicate, so the `gates` / `pending`
    /// maps track the CURRENT open set rather than the cumulative touched set.
    ///
    /// The prune fires ONLY when ALL THREE hold, taken under `pending` → `gates`:
    ///
    /// - FULLY COMMITTED — `pending[carrier].latest_seq <= committed_seq`: no newer, un-synced
    ///   op is queued. A FAILED / timed-out close skips `mark_committed`, so `latest_seq >
    ///   committed_seq` and this gate SKIPS the prune (a failed close must never prune).
    /// - CLOSED — `!injected.contains_key(carrier)`: no live slot. A failed close leaves a
    ///   `PossiblyOpenUnsynced` shell (reconcilable, not idle), so this gate SKIPS the prune.
    /// - NO WAITER — `Arc::strong_count(gate) == 2`: exactly the `gates`-map entry plus THIS
    ///   draining op's single local `gate` clone (the `_guard` from `gate.lock()` borrows the
    ///   mutex — it does NOT clone the Arc). Any blocked waiter/holder fetched its OWN
    ///   `gate_for` clone before parking on `gate.lock()`, raising the count to at least 3 and
    ///   SKIPPING the prune — the map entry survives while any waiter exists, so a later reopen
    ///   reuses the SAME `Arc` and one carrier is never split across two live gates. That
    ///   invariant is brittle to any future incidental gate-Arc clone — keep gate clones tightly
    ///   scoped to the draining op.
    ///
    /// Locks in the SAME order the rest of `drive` acquires them (`pending` first, then a
    /// released `injected` probe, then `gates`); no code path locks `injected` / `gates` before
    /// `pending`, so there is no deadlock.
    pub(crate) fn prune_carrier_state_if_idle(&self, carrier: &str) {
        let mut pending = self.pending.lock();
        let fully_committed = pending
            .get(carrier)
            .is_some_and(|p| p.latest_seq <= p.committed_seq);
        if !fully_committed {
            return;
        }
        // The carrier must be CLOSED (no injected slot — a failed close leaves a
        // `PossiblyOpenUnsynced` shell, which is reconcilable, not idle). The probe releases
        // `injected` before `gates` is locked below.
        let carrier_still_present = self.injected.lock().contains_key(carrier);
        if carrier_still_present {
            return;
        }
        let mut gates = self.gates.lock();
        // Waiter-aware: the sole live clones of the gate Arc must be the map entry + this
        // draining op's single local clone. A blocked waiter's own `gate_for` clone raises the
        // count past 2, so the entry stays and a reopen reuses the SAME Arc (no split).
        let no_waiter = gates
            .get(carrier)
            .is_some_and(|gate| Arc::strong_count(gate) == 2);
        if !no_waiter {
            return;
        }
        pending.remove(carrier);
        gates.remove(carrier);
    }

    /// Ordered per-carrier lifecycle op: serialize + coalesce + commit, driving the
    /// wire send + barrier through `sink`. Open, change, AND close all flow through
    /// this ONE gate + coalescing cell.
    ///
    /// The submission is recorded as the carrier's latest pending op; the caller then
    /// acquires the per-carrier gate (a later op BLOCKS here until the in-flight op's
    /// barrier completes — ordered commits, no `didChange` ahead of `didOpen`, no
    /// `didClose` interleaved with an in-flight injection), drains the NEWEST pending op
    /// (coalescing a burst — the newest op wins, so a close supersedes an older queued
    /// injection and a reopen supersedes an older close), then performs it:
    ///
    /// - [`PendingKind::Inject`]: classify Open / Change / ReconcileThenOpen by slot state
    ///   (reserved under the gate — no TOCTOU). A `PossiblyOpenUnsynced` shell first sends a
    ///   BOUNDED reconcile `didClose` (fail closed to OWNED on failure), then sends the wire op,
    ///   awaits its barrier, and commits the local slot consistently. A first-open or
    ///   reconcile-open barrier failure marks the slot `PossiblyOpenUnsynced` and best-effort
    ///   retracts the possibly-open Program file; a `didChange` failure fails closed to the
    ///   non-serveable `OpenUnsyncedContent` slot (the doc is open but its text is now uncertain).
    /// - [`PendingKind::Close`]: classify by slot state under the gate. Send `didClose` ONLY
    ///   when the carrier is currently reserved (open/uncertain in the Program), transitioning
    ///   the slot to the non-serveable `PossiblyOpenUnsynced` shell BEFORE the bounded barrier
    ///   and removing it only on a SUCCESSFUL close (a failed/timed-out close leaves the shell
    ///   to reconcile — never Vacant, never a bare duplicate `didOpen`); a never-opened carrier
    ///   (or one an earlier gate holder already closed) is a no-op — the slot presence is the
    ///   authoritative open/closed decision under the gate.
    ///
    /// Returns the barrier `Result` (a broken connection is an `Err` the caller fails
    /// closed on).
    pub(crate) async fn drive<S, Fut>(
        &self,
        carrier: &str,
        kind: PendingKind,
        sink: S,
    ) -> Result<(), TypeProviderError>
    where
        S: Fn(CarrierWireOp) -> Fut,
        Fut: Future<Output = Result<(), TypeProviderError>>,
    {
        // 1. Record this submission as the carrier's latest pending op.
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        self.record_pending(carrier, seq, kind);

        // 2. Acquire the per-carrier gate — a later op BLOCKS here until the in-flight
        //    op's barrier completes (ordered commits; no didChange ahead of didOpen; no
        //    didClose interleaved with an in-flight injection).
        let gate = self.gate_for(carrier);
        let _guard = gate.lock().await;

        // 3. Drain the NEWEST pending op. If an earlier gate holder already committed
        //    this-or-newer op, this call is a no-op (coalesced away).
        let Some((drain_seq, drain_kind)) = self.take_drainable(carrier) else {
            // Nothing to drain (the latest op was already committed by an earlier gate holder).
            // Prune the now-idle per-carrier gate + pending state when the carrier is closed and
            // no waiter remains, so a coalesced-away op never retains map entries forever.
            self.prune_carrier_state_if_idle(carrier);
            return Ok(());
        };

        match drain_kind {
            PendingKind::Inject(drain_content) => {
                // Atomically reserve/transition the slot to its non-serveable in-flight state AND
                // classify the action, then ARM the cancellation rollback (keyed on the action)
                // BEFORE any barrier await. The classification is three-way (under the gate — no
                // TOCTOU): a vacant slot Opens (reserving a `PossiblyOpenUnsynced` shell); a
                // barrier-SYNCED / lost-confirmation slot Changes (transitioning to
                // `OpenUnsyncedContent` up front, so the in-flight refresh serves nothing); a
                // `PossiblyOpenUnsynced` shell (a prior first-open of uncertain wire state)
                // Reconciles-then-opens.
                let action = reserve_carrier_capturing(&self.injected, carrier);
                let mut rollback = InjectRollback::arm(&self.injected, carrier.to_string(), action);

                // ReconcileThenOpen: the slot is a `PossiblyOpenUnsynced` shell — the shared
                // Program MAY hold a stale `didOpen`. Reconcile it to a KNOWN-closed state with
                // a bounded `didClose` BEFORE the fresh open. The slot is already non-serveable
                // (`PossiblyOpenUnsynced`), so a wedged reconcile-close strands no serveable
                // state. On reconcile FAILURE, leave the shell (the next retry re-reconciles)
                // and return `Err` so the caller falls back to OWNED — never a `didChange` or a
                // bare `didOpen` onto an un-reconciled open.
                if matches!(action, InjectAction::ReconcileThenOpen) {
                    if let Err(err) =
                        bounded_carrier_close_with_timeout(&sink, self.close_barrier_bound).await
                    {
                        rollback.disarm();
                        mark_possibly_open_unsynced(&self.injected, carrier);
                        return Err(err);
                    }
                }

                let op = match action {
                    InjectAction::Open | InjectAction::ReconcileThenOpen => CarrierWireOp::Open {
                        version: 1,
                        content: Arc::clone(&drain_content),
                    },
                    InjectAction::Change => CarrierWireOp::Change {
                        version: self.next_version.fetch_add(1, Ordering::Relaxed),
                        content: Arc::clone(&drain_content),
                    },
                };

                // Wire send + barrier — the ONLY remaining await under the gate for an inject.
                // An OUTER overlay deadline that cancels this future mid-barrier drops the armed
                // `rollback` guard FIRST (a first-open / reconcile-open reservation becomes a
                // `PossiblyOpenUnsynced` shell — never removed — so the retry reconciles; a
                // refresh fails closed to `OpenUnsyncedContent`) THEN the gate guard, so a
                // cancelled inject never strands a serveable slot, and a later op re-drives from a
                // locally consistent slot view.
                let result = sink(op).await;

                // The barrier COMPLETED (no cancellation): reconcile the local slot with the
                // shared Program. This OWNS the committed slot, so the rollback is disarmed once
                // it runs. A first-open / reconcile-open FAILURE marks the slot
                // `PossiblyOpenUnsynced` (stops SHARED serving) BEFORE the awaited best-effort
                // retract `didClose`, so a wedged/cancelled retract can never leave serveable
                // state and the next inject reconciles; a `didChange` failure marks the slot
                // `OpenUnsyncedContent` (stops SHARED serving; the next inject retries a fresh
                // `didChange`).
                let commit = sync_commit(action, result.is_ok());
                apply_local_sync_commit(&self.injected, carrier, drain_content, commit);
                rollback.disarm();
                if matches!(commit, SyncCommit::RetractOpen) {
                    let _ =
                        bounded_carrier_close_with_timeout(&sink, self.close_barrier_bound).await;
                }
                if result.is_ok() {
                    self.mark_committed(carrier, drain_seq);
                }
                result
            }
            PendingKind::Close => {
                // Classify by slot state under the gate, mirroring the inject
                // reserve-before-await. A VACANT slot is a no-op (nothing open — create no
                // slot). Any occupied slot (`Synced` / `OpenUnsyncedContent` /
                // `PossiblyOpenUnsynced`) transitions to the non-serveable
                // `PossiblyOpenUnsynced` shell UP FRONT —
                // BEFORE the wire barrier — so a cancelled / failed / timed-out close leaves
                // the carrier RECONCILABLE (never Vacant, which would drive a bare duplicate
                // `didOpen` on a later inject; never serving). Slot PRESENCE is the
                // authoritative open/closed decision under the gate: `didClose` is sent ONLY
                // for a slot that was actually reserved (open in the Program).
                if begin_close_marking_unsynced(&self.injected, carrier) {
                    // Wire send + BOUNDED barrier — the ONLY await under the gate for a close.
                    // A wedged control/writer path cannot stall the gate (and every later op
                    // serialized behind it) past the bound.
                    match bounded_carrier_close_with_timeout(&sink, self.close_barrier_bound).await
                    {
                        Ok(()) => {
                            // The close barrier SYNCED: the carrier is fully closed. REMOVE the
                            // (non-serveable) shell, mark the close committed, and prune the
                            // now-idle per-carrier gate + pending cell (skipped when a newer op
                            // is queued or a waiter holds the gate — see
                            // [`Self::prune_carrier_state_if_idle`]).
                            remove_carrier_slot(&self.injected, carrier);
                            self.mark_committed(carrier, drain_seq);
                            self.prune_carrier_state_if_idle(carrier);
                            Ok(())
                        }
                        Err(err) => {
                            // The close FAILED / timed out — the shared Program may still hold
                            // the document open. LEAVE the `PossiblyOpenUnsynced` shell (do NOT
                            // remove, do NOT mark committed, do NOT prune) so a retry inject
                            // RECONCILES (a bounded `didClose`, then a fresh `didOpen`) — never a
                            // bare duplicate `didOpen`. Fail closed to OWNED.
                            Err(err)
                        }
                    }
                } else {
                    // Nothing was open (a never-opened carrier, or one an earlier gate holder
                    // already closed): a successful no-op close. Mark committed and prune the
                    // now-idle per-carrier state.
                    self.mark_committed(carrier, drain_seq);
                    self.prune_carrier_state_if_idle(carrier);
                    Ok(())
                }
            }
        }
    }
}

/// A tracked carrier overlay slot — a THREE-state machine distinguishing barrier-SYNCED
/// content from two distinct flavours of UNCERTAINTY: open-CERTAIN-but-content-uncertain
/// ([`Self::OpenUnsyncedContent`]) and open-UNCERTAIN ([`Self::PossiblyOpenUnsynced`]). The
/// distinction keeps the local overlay view from diverging from the shared Program on a sync
/// failure/timeout: SHARED serves ONLY [`Self::Synced`] content ([`synced_content`]), and each
/// uncertain state fails closed to OWNED while steering the next inject to the correct recovery
/// (a FRESH `didChange` for [`Self::OpenUnsyncedContent`]; a close+reopen reconcile for
/// [`Self::PossiblyOpenUnsynced`]). Cancellation rollback does NOT snapshot a slot: an
/// interrupted injection's [`InjectRollback`] guard REASSERTS the fail-closed state derived
/// from its in-flight [`InjectAction`] (never restores a cloned pre-reservation slot), so the
/// slot needs no `Clone`.
pub(crate) enum CarrierSlot {
    /// The shim's sync barrier CONFIRMED the shared Program ACCEPTED this content — the ONLY
    /// content served / positioned from ([`synced_content`]). Reached only through
    /// [`promote_synced`] after a barrier accepted the injection; the UTF-16 diagnostic index
    /// is built from THIS, never the optimistic reservation.
    Synced { content: Arc<str> },
    /// The doc is open-CERTAIN but its content is UNCERTAIN: a `didChange` refresh was
    /// dispatched onto an already-open doc, but its barrier FAILED / timed out / was cancelled,
    /// so the shared Program MAY already hold the new text while the confirmation was lost.
    /// NEVER serveable ([`synced_content`] yields `None`) — serving the PRIOR synced text would
    /// misposition SHARED diagnostics against a stale basis (the correctness law is fail-closed
    /// under POSSIBLE mismatch, not the common case), and the unaccepted new text is never
    /// served either. The next inject retries with a FRESH `didChange` at the latest text — the
    /// doc is open, so [`InjectAction::Change`], NEVER a close+reopen reconcile (contrast
    /// [`Self::PossiblyOpenUnsynced`], where the doc's open state itself is unproven).
    OpenUnsyncedContent,
    /// The doc's OPEN state itself is UNCERTAIN — the general fail-closed shell reached by every
    /// path that leaves the shared Program's open/closed state unproven: a cancelled/failed
    /// first-`didOpen` OR reconcile-then-`didOpen` (which MAY have reached the Program); a
    /// first-open barrier failure's best-effort retract; and a CLOSE's up-front transition (an
    /// open/change/close carrier all mark this shell BEFORE the close barrier), left in place by a
    /// cancelled / failed / timed-out `didClose`. NEVER serveable ([`synced_content`] yields
    /// `None`); because the open state is unproven, the next inject RECONCILES it (a bounded
    /// `didClose` to a known-closed state, THEN a fresh `didOpen` — see
    /// [`InjectAction::ReconcileThenOpen`]), never a blind duplicate `didOpen` and never a
    /// `didChange` onto an unconfirmed open. Contrast [`Self::OpenUnsyncedContent`], where the doc
    /// is known-OPEN and only its content is uncertain (recovered by a fresh `didChange`).
    PossiblyOpenUnsynced,
}

/// How a carrier injection reconciles with the current slot state — a three-way decision
/// taken atomically under the `injected` lock ([`reserve_carrier_capturing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InjectAction {
    /// The carrier slot was absent — this caller reserved a `PossiblyOpenUnsynced` shell and
    /// must send `didOpen` (version 1).
    Open,
    /// The carrier slot was an already-OPEN doc — [`CarrierSlot::Synced`] OR
    /// [`CarrierSlot::OpenUnsyncedContent`] (a prior refresh whose barrier was lost) — so send
    /// `didChange` (a refresh) at the latest text. On barrier SUCCESS the slot promotes to
    /// [`CarrierSlot::Synced`]; on FAILURE/cancel it fails closed to
    /// [`CarrierSlot::OpenUnsyncedContent`] (non-serveable) — the prior synced content is never
    /// re-served, since a lost-confirmation refresh may already hold the new text.
    Change,
    /// The carrier slot was a `PossiblyOpenUnsynced` shell (a prior first-open of uncertain
    /// wire state) — RECONCILE the shared Program to a known-closed state (a bounded
    /// `didClose`) BEFORE a fresh `didOpen`; never a duplicate bare `didOpen`, never a
    /// `didChange` onto an unconfirmed open.
    ReconcileThenOpen,
}

/// Atomically reserve/transition the carrier slot to its non-serveable in-flight state AND
/// return the [`InjectAction`] that drives the wire op — one `injected` lock acquisition.
///
/// This is the reserve-before-await that closes the inject TOCTOU: a SINGLE lock
/// inspects the entry, so exactly one concurrent first-open sees the absent slot,
/// reserves it, and sends `didOpen` version 1; every other caller sees the reserved
/// slot and Changes or Reconciles-then-opens (never a second `didOpen`) — no window
/// between "is it open?" and the wire send in which two first-opens both open.
///
/// The classification maps the pre-reservation slot state onto THREE actions, and EVERY action
/// leaves the slot in a NON-serveable in-flight state before the barrier — a slot is serveable
/// IFF it is [`CarrierSlot::Synced`] AND no barrier op is in flight for it:
///
/// - VACANT — reserve a fresh [`CarrierSlot::PossiblyOpenUnsynced`] shell and return
///   [`InjectAction::Open`]: a cancelled first-open rolls back to that shell (never removed), so
///   the retry RECONCILES instead of blindly re-`didOpen`ing.
/// - OCCUPIED [`CarrierSlot::Synced`] OR [`CarrierSlot::OpenUnsyncedContent`] — the doc is
///   already OPEN, so a refresh: transition the slot to the non-serveable
///   [`CarrierSlot::OpenUnsyncedContent`] UP FRONT — BEFORE the `didChange` await — and return
///   [`InjectAction::Change`]. A `didChange` is dispatched before its barrier, so keeping
///   `Synced { v1 }` would let a concurrent read position SHARED diagnostics against a basis the
///   shared Program may already have replaced. The barrier SUCCESS promotes it back to
///   `Synced { latest }` ([`promote_synced`]); a failure/cancel leaves it `OpenUnsyncedContent`.
///   Either way the refresh sends a FRESH `didChange` at the latest text (never a close+reopen).
/// - OCCUPIED [`CarrierSlot::PossiblyOpenUnsynced`] — a prior first-open of UNCERTAIN wire
///   state: leave the (already non-serveable) shell and return
///   [`InjectAction::ReconcileThenOpen`]; the driver reconciles the Program (a bounded
///   `didClose`) before the fresh `didOpen`.
///
/// Reservation NEVER promotes to `Synced` (the reserved text is not served until its barrier
/// is confirmed accepted — see [`promote_synced`]). The returned action is paired with
/// [`InjectRollback`], which owns the atomic reserve → arm → commit lifecycle and reads the
/// action to choose the fail-closed rollback target on a cancel.
pub(crate) fn reserve_carrier_capturing(
    injected: &SyncMutex<HashMap<String, CarrierSlot>>,
    carrier: &str,
) -> InjectAction {
    use std::collections::hash_map::Entry;
    match injected.lock().entry(carrier.to_string()) {
        Entry::Occupied(mut occupied) => {
            // A `PossiblyOpenUnsynced` shell (a prior first-open of uncertain wire state)
            // reconciles-then-opens; leave the (already non-serveable) shell intact.
            if matches!(occupied.get(), CarrierSlot::PossiblyOpenUnsynced) {
                InjectAction::ReconcileThenOpen
            } else {
                // An already-OPEN doc (`Synced`, OR a prior lost-confirmation
                // `OpenUnsyncedContent`) refreshes via a fresh `didChange`. Transition the slot to
                // the non-serveable `OpenUnsyncedContent` UP FRONT — BEFORE the `didChange` await —
                // so an in-flight refresh never serves the prior synced content while the shared
                // Program may already hold the new text (a POSSIBLE mismatch). A barrier SUCCESS
                // promotes it back to `Synced { latest }`; a failure/cancel leaves it
                // `OpenUnsyncedContent` (non-serveable).
                *occupied.get_mut() = CarrierSlot::OpenUnsyncedContent;
                InjectAction::Change
            }
        }
        Entry::Vacant(vacant) => {
            vacant.insert(CarrierSlot::PossiblyOpenUnsynced);
            InjectAction::Open
        }
    }
}

/// The cancellation-safe rollback for an in-flight carrier injection.
///
/// An injection reserves its slot BEFORE awaiting the sync barrier, so an OUTER overlay
/// deadline that cancels (drops) the drive future WHILE it is parked on the barrier — before
/// the straight-line commit runs — must leave the slot in its FAIL-CLOSED state (the
/// reservation already transitioned it there UP FRONT; this guard re-asserts it if the commit
/// never runs). While ARMED (from the atomic reserve until [`Self::disarm`] after the local
/// commit), `Drop` re-locks `injected` (a sync lock, never held across `.await`) and reconciles
/// the slot to its FAIL-CLOSED state by the in-flight [`InjectAction`] — every target is
/// NON-serveable, so a cancelled inject never strands a served, possibly-stale slot:
///
/// - [`InjectAction::Open`] (a cancelled first-open) → [`CarrierSlot::PossiblyOpenUnsynced`],
///   NEVER removed (removal would drive a blind duplicate `didOpen` on retry).
/// - [`InjectAction::Change`] (a cancelled refresh) → [`CarrierSlot::OpenUnsyncedContent`]: the
///   prior synced content is NOT restored, since a lost-confirmation `didChange` may already
///   hold the new text (re-serving the prior text could misposition SHARED diagnostics against a
///   stale basis).
/// - [`InjectAction::ReconcileThenOpen`] (a cancelled reconcile-then-open) →
///   [`CarrierSlot::PossiblyOpenUnsynced`] (re-reconciled on the next inject).
///
/// It emits NO wire `didClose` from `Drop` — an async send is impossible there, and a sent
/// `didOpen` is already session-end retract-eligible through the shim's `opened_carriers`,
/// so local-slot reconciliation is the only cancellation-safe obligation. It composes with
/// the per-carrier async gate: a cancellation drops this rollback guard FIRST (reconciling
/// the slot), THEN the gate guard (releasing the gate), so a later op re-drives from a
/// locally consistent slot view. A normal (non-cancelled) outcome disarms after the
/// reconciliation commit, so success AND a barrier failure the reconciliation already
/// handled are left untouched.
struct InjectRollback<'a> {
    injected: &'a SyncMutex<HashMap<String, CarrierSlot>>,
    carrier: String,
    action: InjectAction,
    armed: bool,
}

impl<'a> InjectRollback<'a> {
    /// Arm the rollback over the just-reserved slot and the in-flight action that classifies
    /// its fail-closed target.
    fn arm(
        injected: &'a SyncMutex<HashMap<String, CarrierSlot>>,
        carrier: String,
        action: InjectAction,
    ) -> Self {
        Self {
            injected,
            carrier,
            action,
            armed: true,
        }
    }

    /// Disarm once the straight-line reconciliation owns the committed slot — a normal
    /// outcome (success OR a reconciled barrier failure) keeps the committed slot.
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for InjectRollback<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        // Reconcile the interrupted inject to its FAIL-CLOSED state by the in-flight action.
        // Every branch lands on a NON-serveable state (the reservation already transitioned the
        // slot there UP FRONT; this re-asserts it), so a cancelled inject never strands a served,
        // possibly-stale slot.
        let fail_closed = match self.action {
            // A cancelled REFRESH: the `didChange` may have applied before the confirmation was
            // lost, so fail closed to `OpenUnsyncedContent`. The prior synced content is NOT
            // restored — it is only ever served when barrier-confirmed; re-serving it could
            // misposition SHARED diagnostics against a stale basis.
            InjectAction::Change => CarrierSlot::OpenUnsyncedContent,
            // A cancelled FIRST-OPEN or RECONCILE-THEN-OPEN: the `didOpen` (and/or the reconcile
            // close) may have reached the shared Program, so the doc's open state is UNCERTAIN —
            // leave a `PossiblyOpenUnsynced` shell (reconciled on the next inject), NEVER removed
            // (removal would drive a blind duplicate `didOpen` on retry).
            InjectAction::Open | InjectAction::ReconcileThenOpen => {
                CarrierSlot::PossiblyOpenUnsynced
            }
        };
        self.injected
            .lock()
            .insert(self.carrier.clone(), fail_closed);
    }
}

/// The local-slot action after an injection's sync barrier resolves — the
/// consistency oracle that keeps the local overlay view aligned with the shared
/// Program. PURE over `(action, barrier_ok)`; the caller applies it via
/// [`apply_local_sync_commit`] and (for [`SyncCommit::RetractOpen`]) issues the wire
/// `didClose`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncCommit {
    /// The barrier SYNCED — promote the reserved text to the slot's authoritative
    /// synced content (the only content served / positioned from).
    Promote,
    /// A first `didOpen` (or a `didOpen` issued after a reconcile) barrier FAILED/timed out — the
    /// Program MAY hold the `didOpen`. Stop serving by marking the slot
    /// [`CarrierSlot::PossiblyOpenUnsynced`] (never destroy prior content — there is none)
    /// and best-effort retract it (`didClose`); the uncertain state forces a reconcile on the
    /// next inject. A failed retract leaves the fail-closed shell for that reconcile.
    RetractOpen,
    /// A `didChange` (refresh) barrier FAILED/timed out — the `didChange` was dispatched onto
    /// an already-open doc BEFORE its barrier, so the Program MAY already hold the new text
    /// while its confirmation was lost. Fail closed by marking the slot
    /// [`CarrierSlot::OpenUnsyncedContent`] (non-serveable): never serve the PRIOR synced text
    /// (a POSSIBLE mismatch against the Program's actual text) and never the
    /// reserved-but-unaccepted new text. The next inject retries a FRESH `didChange`.
    MarkOpenUnsyncedContent,
}

/// Map an injection's sync-barrier outcome to the local-slot action that keeps the
/// local view consistent with the shared Program: any success promotes; a first-open
/// failure retracts the possibly-open Program file; a `didChange` failure fails closed to
/// the open-but-content-uncertain state.
pub(crate) fn sync_commit(action: InjectAction, barrier_ok: bool) -> SyncCommit {
    match (action, barrier_ok) {
        (_, true) => SyncCommit::Promote,
        // A first `Open` and a `ReconcileThenOpen` after a reconcile both end on a `didOpen`, so a
        // failure retracts the possibly-open Program file.
        (InjectAction::Open | InjectAction::ReconcileThenOpen, false) => SyncCommit::RetractOpen,
        // A `Change` (refresh) failure fails closed to `OpenUnsyncedContent` — the doc is open
        // but the refresh may already have applied, so the prior synced text is never re-served.
        (InjectAction::Change, false) => SyncCommit::MarkOpenUnsyncedContent,
    }
}

/// Apply the local-slot half of a [`SyncCommit`] (promote the synced content / mark the slot
/// `PossiblyOpenUnsynced` / mark the slot `OpenUnsyncedContent`). The wire `didClose` retract
/// for [`SyncCommit::RetractOpen`] is the caller's separate control call, issued AFTER this
/// local state update so SHARED serving stops before any awaited close.
pub(crate) fn apply_local_sync_commit(
    injected: &SyncMutex<HashMap<String, CarrierSlot>>,
    carrier: &str,
    text: Arc<str>,
    commit: SyncCommit,
) {
    match commit {
        SyncCommit::Promote => promote_synced(injected, carrier, text),
        SyncCommit::RetractOpen => mark_possibly_open_unsynced(injected, carrier),
        SyncCommit::MarkOpenUnsyncedContent => mark_open_unsynced_content(injected, carrier),
    }
}

/// Promote the barrier-SYNCED `text` to the slot's authoritative [`CarrierSlot::Synced`]
/// content (called only after the sync barrier ACCEPTED the injection). Only touches a
/// present slot — the slot is always reserved before the barrier, so this never resurrects a
/// removed slot as served.
pub(crate) fn promote_synced(
    injected: &SyncMutex<HashMap<String, CarrierSlot>>,
    carrier: &str,
    text: Arc<str>,
) {
    if let Some(slot) = injected.lock().get_mut(carrier) {
        *slot = CarrierSlot::Synced { content: text };
    }
}

/// Mark a carrier's local slot [`CarrierSlot::PossiblyOpenUnsynced`] — a first `didOpen`
/// barrier failed and the caller separately best-effort retracts the possibly-open Program
/// file. The slot stops serving immediately AND its uncertain wire state forces a reconcile
/// on the next inject (never a blind duplicate `didOpen`). Upserts so the fail-closed shell
/// is present even in the (gate-serialized, so impossible) absent case — never a phantom
/// clean first-open.
fn mark_possibly_open_unsynced(injected: &SyncMutex<HashMap<String, CarrierSlot>>, carrier: &str) {
    injected
        .lock()
        .insert(carrier.to_string(), CarrierSlot::PossiblyOpenUnsynced);
}

/// Mark a carrier's local slot [`CarrierSlot::OpenUnsyncedContent`] — a `didChange` refresh
/// barrier failed/timed out. The doc is still OPEN in the shared Program, but which text it
/// holds is UNCERTAIN (the refresh may have applied before its confirmation was lost), so the
/// slot stops serving (never the prior synced text — a possible mismatch — never the unaccepted
/// new text) while the next inject retries a FRESH `didChange` (never a close+reopen reconcile).
/// Upserts so the fail-closed slot is present even in the (gate-serialized, so impossible)
/// absent case.
fn mark_open_unsynced_content(injected: &SyncMutex<HashMap<String, CarrierSlot>>, carrier: &str) {
    injected
        .lock()
        .insert(carrier.to_string(), CarrierSlot::OpenUnsyncedContent);
}

/// Classify a carrier close by the current slot state AND transition an open/uncertain slot to
/// the non-serveable [`CarrierSlot::PossiblyOpenUnsynced`] shell UP FRONT — BEFORE the wire
/// barrier — mirroring the inject reserve-before-await ([`reserve_carrier_capturing`]). One
/// lock acquisition. Returns whether a `didClose` must be sent:
///
/// - VACANT — a no-op: nothing is open, so NO slot is created and `false` is returned (a
///   never-opened carrier, or one an earlier gate holder already closed).
/// - OCCUPIED — any occupied slot ([`CarrierSlot::Synced`] /
///   [`CarrierSlot::OpenUnsyncedContent`] / [`CarrierSlot::PossiblyOpenUnsynced`]) — the carrier
///   is open or of uncertain wire state: transition the slot to the non-serveable
///   `PossiblyOpenUnsynced` shell and return `true`. The up-front transition means a cancelled
///   / failed / timed-out close leaves the carrier RECONCILABLE (never Vacant — which would
///   drive a bare duplicate `didOpen` on a later inject — and never serving); a SUCCESSFUL
///   close then REMOVES the shell ([`remove_carrier_slot`]).
fn begin_close_marking_unsynced(
    injected: &SyncMutex<HashMap<String, CarrierSlot>>,
    carrier: &str,
) -> bool {
    use std::collections::hash_map::Entry;
    match injected.lock().entry(carrier.to_string()) {
        Entry::Occupied(mut occupied) => {
            *occupied.get_mut() = CarrierSlot::PossiblyOpenUnsynced;
            true
        }
        Entry::Vacant(_) => false,
    }
}

/// Remove a carrier's local slot after a SUCCESSFUL close barrier (the carrier is fully closed
/// in the shared Program) — one lock acquisition. Only reached after the up-front
/// [`begin_close_marking_unsynced`] transition, so it removes the `PossiblyOpenUnsynced` shell
/// the successful close established.
fn remove_carrier_slot(injected: &SyncMutex<HashMap<String, CarrierSlot>>, carrier: &str) {
    injected.lock().remove(carrier);
}

/// The last barrier-SYNCED content for a carrier — by the engine's canonicalization
/// first, then the injected key — the ONLY content served / positioned from. A
/// reserved-but-not-yet-synced slot returns `None` (fail-closed — no unaccepted text).
pub(crate) fn synced_content(
    injected: &SyncMutex<HashMap<String, CarrierSlot>>,
    engine_carrier: &str,
    carrier: &str,
) -> Option<Arc<str>> {
    let injected = injected.lock();
    injected
        .get(engine_carrier)
        .or_else(|| injected.get(carrier))
        .and_then(|slot| match slot {
            CarrierSlot::Synced { content } => Some(Arc::clone(content)),
            // Both uncertain states fail closed: an open-but-content-uncertain refresh
            // (`OpenUnsyncedContent`) and an open-uncertain first-open (`PossiblyOpenUnsynced`)
            // have no barrier-confirmed basis to position a SHARED result against.
            CarrierSlot::OpenUnsyncedContent | CarrierSlot::PossiblyOpenUnsynced => None,
        })
}

/// The fail-closed gate for SHARED diagnostics: a carrier with no barrier-SYNCED content has
/// no basis to position an (even empty) SHARED result against, so it must NOT serve — return
/// an `Err` the composite treats as OWNED, never an `Ok(empty)` SHARED result. A
/// `PossiblyOpenUnsynced` shell (a cancelled/failed/uncertain first-open) and a never-injected
/// carrier both yield `None` at [`synced_content`] and both fail closed here.
pub(crate) fn require_synced_carrier_content(
    content: Option<Arc<str>>,
) -> Result<Arc<str>, TypeProviderError> {
    content.ok_or_else(|| TypeProviderError::new("shared carrier has no barrier-synced content"))
}

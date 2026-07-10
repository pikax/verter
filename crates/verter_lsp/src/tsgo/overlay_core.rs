//! The lazy SHARED-overlay core: a content cache the OWNED lifecycle records into
//! OFF the critical path, plus the lazily-established transport cell the QUERY path
//! establishes + injects into.
//!
//! [`LazyOverlayCore`] separates the two timings the composite must keep independent:
//!
//! - **Lifecycle (OWNED-budgeted).** `open_file` / `update_file` etc. record the
//!   carrier's current content via [`LazyOverlayCore::record_content`] — a plain sync
//!   insert that NEVER establishes the SHARED transport. So opting into SHARED cannot
//!   trip the OWNED file-lifecycle timing (the foreground TSX sync is budgeted far
//!   below the SHARED establishment bound).
//! - **Query (OFF the foreground-sync budget).** `get_diagnostics` establishes the
//!   transport lazily ([`LazyOverlayCore::ensure`], bounded + singleflight +
//!   liveness-evicting) and injects the carrier's recorded content
//!   ([`LazyOverlayCore::inject_dirty`], only when it changed since the last
//!   injection). Fail-closed: until the transport establishes, the composite serves
//!   OWNED; the SHARED overlay self-heals on a later query.
//!
//! The per-carrier state (recorded content + injection markers + shadow-safety cache)
//! and the active transport EPOCH live under ONE lock ([`OverlayState`]), so a transport
//! RE-establishment resets the epoch and the injection markers atomically — the open
//! carrier set replays into the fresh transport (a reconnect is never served against a
//! transport that never received the open documents).
//!
//! Every injection is attributed to the EPOCH of the transport instance it runs against
//! ([`EstablishedTransport::identity`]), never a re-read of the current `active_epoch`,
//! and every per-carrier physical operation (an inject transaction plus any compensating
//! retract, and an unsafe-flip retract) runs under a stable per-path carrier gate — so a
//! stale injection through a since-replaced transport cannot mark the current epoch synced,
//! and an overlay physically landed against the STILL-current epoch is retracted before a
//! later re-inject of the same path. An overlay left on a since-replaced transport instance
//! (the epoch advanced past its run epoch) is owned by that instance's teardown/replacement
//! lifecycle, not retracted here.
//!
//! Generic over the transport `T` (through the [`OverlayTransport`] seam) so the
//! off-critical-path property is unit-testable with a transport double.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Weak};
use std::time::Duration;

use parking_lot::Mutex as SyncMutex;
use tokio::sync::Mutex as AsyncMutex;

use verter_type_runtime::traits::{ProviderFuture, TypeProvider};

use crate::tsgo::shared::TsgoSharedProvider;
use crate::tsgo::transport_cell::{EstablishedTransport, LazyTransport, TransportEpoch};

/// The bound on a compensating retract issued from the query-time injection path — both
/// the inject transaction's own not-committed-safe cleanup and the sweep's flip-to-unsafe
/// retract. A slow/dead relay retract cannot stall the sweep or the transaction. Symmetric
/// with the OWNED-close retract bound, and the whole sweep is additionally under the
/// composite's outer query deadline — so a wedged relay never delays diagnostics past
/// those bounds.
const OVERLAY_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

/// The transport seam the overlay core drives: inject / retract a carrier overlay,
/// report liveness (the transport-cell eviction predicate), and tear down. Kept small
/// and carrier-only so the core is unit-testable with a double — it never resolves
/// types or reads a store.
pub(crate) trait OverlayTransport: Send + Sync + 'static {
    /// Inject (or refresh) a carrier overlay — the ordered per-carrier state machine.
    fn inject(&self, path: &str, content: &str) -> ProviderFuture<'_, ()>;
    /// Retract a carrier overlay.
    fn retract(&self, path: &str) -> ProviderFuture<'_, ()>;
    /// Whether the attach is still LIVE (the live-death eviction predicate the
    /// transport cell reads to evict a dead transport).
    fn is_live(&self) -> bool;
    /// Tear the attach down (best-effort).
    fn teardown(&self) -> ProviderFuture<'_, ()>;
}

impl OverlayTransport for TsgoSharedProvider {
    fn inject(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        // The SHARED provider's carrier lifecycle IS the ordered per-carrier injection
        // state machine (`TypeProvider::open_file` → `inject_carrier`).
        self.open_file(path, content)
    }

    fn retract(&self, path: &str) -> ProviderFuture<'_, ()> {
        self.close_file(path)
    }

    fn is_live(&self) -> bool {
        // The inherent liveness (control + `--api` connection health).
        TsgoSharedProvider::is_alive(self)
    }

    fn teardown(&self) -> ProviderFuture<'_, ()> {
        self.shutdown()
    }
}

/// The content a query-time injection SUCCESSFULLY committed, tagged with the transport
/// EPOCH it was injected into. The epoch tag is what makes a re-established transport
/// (a new epoch) mark the carrier dirty — the open set replays — and what stops a stale
/// in-flight injection from marking a NEW epoch synced.
struct InjectedRecord {
    content: Arc<str>,
    epoch: TransportEpoch,
}

/// The cached shadow-safety decision for a carrier at a workspace content generation —
/// so a content-clean carrier can skip the disk-probing shadow-safety predicate until
/// the generation advances (a file-set transition could flip the decision).
struct ShadowSafetyCache {
    generation: u64,
    safe: bool,
}

/// The recorded state for one carrier companion: the LATEST content the lifecycle fed,
/// the last content a query-time injection committed (with its transport epoch), and the
/// cached shadow-safety decision (with the generation it was decided at).
struct ContentRecord {
    /// The latest content the lifecycle recorded.
    content: Arc<str>,
    /// The last content a query-time injection committed (with its epoch), or `None` if
    /// never injected / reset on a transport re-establishment.
    injected: Option<InjectedRecord>,
    /// The cached shadow-safety decision + the generation it was decided at, or `None`
    /// if never evaluated.
    shadow_safety: Option<ShadowSafetyCache>,
}

/// The overlay's per-carrier state under ONE lock: the ACTIVE transport epoch (observed
/// from the established transport identity) plus the per-carrier content records — so an
/// epoch reset and the injection-marker reset are ONE atomic critical section.
struct OverlayState {
    /// The epoch of the transport the recorded markers are injected against — set from
    /// the established transport's identity; a change to it resets every marker.
    active_epoch: Option<TransportEpoch>,
    /// The recorded content per carrier companion path.
    content: HashMap<String, ContentRecord>,
}

/// Whether a carrier's CURRENT recorded content is confirmed synced into the shared
/// Program for the ACTIVE transport epoch — its last committed injection carried the
/// current content AND was injected into the currently-active transport epoch. A carrier
/// injected under an OLD epoch (a since-reconnected transport) is NOT synced: it must
/// replay into the fresh transport.
///
/// A carrier that is currently cached shadow-UNSAFE is NEVER synced, regardless of the
/// `injected` marker: a real user file occupies its companion path, so it must fail closed
/// to OWNED and never be served SHARED (`carrier_never_shadows_real_user_file`). This is
/// the SERVED gate that closes the flip-to-unsafe window: the sweep caches `{safe:false}`
/// BEFORE the bounded retract await that clears the marker, so a concurrent query which
/// observes the still-set marker mid-retract sees `{safe:false}` here and fails closed. No
/// shadow-safety cache (a never-evaluated carrier) or a cached-SAFE decision leaves the
/// injected-content + epoch condition as the sole synced gate (a safe carrier is unchanged).
fn record_is_synced(rec: &ContentRecord, active_epoch: Option<TransportEpoch>) -> bool {
    rec.shadow_safety.as_ref().is_none_or(|c| c.safe)
        && rec.injected.as_ref().is_some_and(|inj| {
            inj.content.as_ref() == rec.content.as_ref() && Some(inj.epoch) == active_epoch
        })
}

/// The lazy SHARED-overlay core: a per-carrier content cache (recorded by the OWNED
/// lifecycle, off the establishment path) plus the lazily-established transport cell
/// (established + injected at query time). Generic over the transport `T`.
pub(crate) struct LazyOverlayCore<T: OverlayTransport> {
    /// The per-carrier recorded state + the active transport epoch under ONE sync lock —
    /// the OWNED lifecycle writes content WITHOUT touching the transport cell.
    state: SyncMutex<OverlayState>,
    /// The stable per-path async operation gates. One gate per carrier path, retained
    /// behind a `Weak` so it is dropped once no operation holds it; an in-flight operation
    /// on an old slot and a reopened operation for the SAME path upgrade the SAME live
    /// `Weak` and therefore serialize on ONE gate. Dead entries are pruned opportunistically.
    carrier_gates: SyncMutex<HashMap<String, Weak<AsyncMutex<()>>>>,
    /// The lazily-established, singleflight, bounded, re-arming, liveness-evicting
    /// transport cell — established + injected ONLY at query time.
    transport: LazyTransport<T>,
}

impl<T: OverlayTransport> LazyOverlayCore<T> {
    pub(crate) fn new() -> Self {
        Self {
            state: SyncMutex::new(OverlayState {
                active_epoch: None,
                content: HashMap::new(),
            }),
            carrier_gates: SyncMutex::new(HashMap::new()),
            transport: LazyTransport::new(),
        }
    }

    /// Record a carrier's current content OFF the establishment critical path — a
    /// plain sync insert the OWNED lifecycle calls. NEVER establishes the transport;
    /// the query path injects the recorded content lazily.
    pub(crate) fn record_content(&self, path: &str, content: &str) {
        let mut state = self.state.lock();
        match state.content.get_mut(path) {
            Some(rec) => rec.content = Arc::from(content),
            None => {
                state.content.insert(
                    path.to_string(),
                    ContentRecord {
                        content: Arc::from(content),
                        injected: None,
                        shadow_safety: None,
                    },
                );
            }
        }
    }

    /// Remove and return a carrier's recorded state on close. The removal and the capture of
    /// the record (including its injection marker) are ONE atomic state mutation, so a
    /// concurrent inject commit can never land a marker between the capture and the erase:
    /// the caller sees either the committed marker (and owns compensating for its physical
    /// overlay) or no marker (an in-flight inject transaction observes the removed record at
    /// its commit and compensates its own landing).
    fn take_content(&self, path: &str) -> Option<ContentRecord> {
        let removed = self.state.lock().content.remove(path);
        // Prune dead carrier-gate registry entries on the close path too (not only on a fresh
        // gate mint) — a completed operation leaves a dead `Weak`, and close churn should not
        // leak those between mints. A gate a live operation still holds is retained.
        self.carrier_gates
            .lock()
            .retain(|_, weak| weak.strong_count() > 0);
        removed
    }

    /// The live transport if already established, else `None` — NEVER establishes
    /// (used by the non-establishing retract / shutdown paths).
    pub(crate) async fn current(&self) -> Option<Arc<T>> {
        self.transport.current().await.map(|e| e.transport)
    }

    /// The stable per-path carrier gate: one async mutex per carrier path, shared across
    /// close/reopen of the same path so an in-flight operation on an old slot and a
    /// reopened operation cannot obtain different gates. Held across a carrier's whole
    /// physical transaction (inject + commit classification + any compensating retract,
    /// and the unsafe-flip retract), so those operations for one path are strictly ordered
    /// w.r.t. each other. Dead entries (no live operation holding the gate) are pruned
    /// opportunistically when a new gate is minted.
    fn carrier_gate(&self, path: &str) -> Arc<AsyncMutex<()>> {
        let mut gates = self.carrier_gates.lock();
        if let Some(existing) = gates.get(path).and_then(Weak::upgrade) {
            return existing;
        }
        // Prune dead weak entries before minting a fresh gate — the open-carrier set is
        // small, so a full sweep here is cheap and keeps the registry bounded.
        gates.retain(|_, weak| weak.strong_count() > 0);
        let gate = Arc::new(AsyncMutex::new(()));
        gates.insert(path.to_string(), Arc::downgrade(&gate));
        gate
    }

    /// Record a carrier's shadow-safety decision for `generation`, keyed under the bound
    /// transport epoch — generation-monotonic: the decision is accepted only when it is
    /// the CURRENT transport identity (`active_epoch == Some(run_epoch)`) and no newer
    /// generation is already cached; a same-generation contradictory result (a concurrent
    /// opposite decision at the same generation) fails closed to `safe:false`. Cached
    /// BEFORE the gated inject/retract so `is_synced` reflects the fresh decision the
    /// instant it is observed and the commit's admission gate can read it.
    fn cache_shadow_decision(
        &self,
        path: &str,
        run_epoch: TransportEpoch,
        generation: u64,
        safe: bool,
    ) {
        let mut state = self.state.lock();
        if state.active_epoch != Some(run_epoch) {
            return;
        }
        if let Some(rec) = state.content.get_mut(path) {
            let newer_cached = rec
                .shadow_safety
                .as_ref()
                .is_some_and(|c| generation < c.generation);
            if newer_cached {
                return;
            }
            let effective_safe = match rec.shadow_safety.as_ref() {
                // A same-generation contradiction fails closed to unsafe.
                Some(c) if c.generation == generation => c.safe && safe,
                _ => safe,
            };
            rec.shadow_safety = Some(ShadowSafetyCache {
                generation,
                safe: effective_safe,
            });
        }
    }

    /// Observe the established transport's identity MONOTONICALLY: adopt the epoch AND reset
    /// every injection marker ONLY when `active_epoch` is `None` OR the observed epoch is
    /// STRICTLY GREATER than the current one (a genuine reconnect to a newer transport) — the
    /// open carrier set is no longer synced into the (now dead / replaced) prior transport and
    /// must replay into the fresh one. Equal and OLDER epochs are IGNORED: a runtime worker
    /// preempted between `ensure`'s establish-return and this synchronous observe could deliver
    /// a stale `observe(E1)` after another worker already committed+observed E2, and a stale
    /// late observe must NOT regress `active_epoch` E2→E1 or reset the fresh markers. The
    /// shadow-safety caches are LEFT intact: they are keyed on the workspace content
    /// generation, orthogonal to the transport epoch.
    fn observe_transport_identity(&self, epoch: TransportEpoch) {
        let mut state = self.state.lock();
        let adopt = match state.active_epoch {
            None => true,
            Some(active) => epoch > active,
        };
        if adopt {
            state.active_epoch = Some(epoch);
            for rec in state.content.values_mut() {
                rec.injected = None;
            }
        }
    }

    /// Establish the transport for a bound carrier — bounded, singleflight, and
    /// LIVENESS-evicting (a dead `Live` transport is evicted and re-established per the
    /// generation/nonce re-arm discriminant). Reached ONLY from the query path, never
    /// the OWNED lifecycle. A `None` binding serves the baseline WITHOUT entering the
    /// cell (the transport-cell-poisoning gate). On success the established transport's
    /// identity/epoch is observed BEFORE the transport is returned — a reconnect resets
    /// the injection markers so the open set replays — and the identity-bound
    /// [`EstablishedTransport`] is returned so the injection path attributes work to the
    /// EXACT transport instance's epoch, not a re-read of `active_epoch`.
    pub(crate) async fn ensure<B, G, Ef, Fut>(
        &self,
        bound: Option<(B, u64)>,
        probe_generation: G,
        establish: Ef,
        timeout: Duration,
    ) -> Option<EstablishedTransport<T>>
    where
        G: Fn(u64) -> Option<String>,
        Ef: FnOnce(B, u64) -> Fut,
        Fut: Future<Output = Option<Arc<T>>>,
    {
        let established = self
            .transport
            .get_or_establish_bound(bound, probe_generation, |t| t.is_live(), establish, timeout)
            .await?;
        // Observe the identity BEFORE handing the transport to the injection path — a
        // reconnect (new epoch) resets the markers so the open set replays into it.
        self.observe_transport_identity(established.identity.epoch);
        Some(established)
    }

    /// Inject ONE carrier's recorded content into the SHARED transport IF it is not
    /// already synced for that transport's epoch — the single-carrier query-time
    /// injection off the OWNED lifecycle path.
    ///
    /// The injection is attributed to the BOUND transport's epoch
    /// (`established.identity.epoch`), never a re-read of the current `active_epoch`, so a
    /// stale invocation driven through a since-replaced transport cannot mark the current
    /// epoch synced. A direct single-carrier injection asserts the carrier is shadow-safe
    /// at `generation`; that decision is recorded before the gated transaction so the
    /// commit's admission gate is satisfiable. Best-effort + fail-closed: on failure the
    /// content stays dirty and a later query retries (self-healing).
    pub(crate) async fn inject_dirty(
        &self,
        established: &EstablishedTransport<T>,
        path: &str,
        generation: u64,
    ) {
        let run_epoch = established.identity.epoch;
        self.cache_shadow_decision(path, run_epoch, generation, true);
        let gate = self.carrier_gate(path);
        self.inject_dirty_bound(&established.transport, run_epoch, path, generation, &gate)
            .await;
    }

    /// The gated per-carrier injection transaction: physically inject the carrier's
    /// recorded content into `transport`, then classify the outcome atomically under
    /// `state`. The `carrier_gate` is held across the physical inject, the commit
    /// classification, AND any compensating retract, so any required compensating retract
    /// is ISSUED and ordered before any later re-inject of the same path can run, and a
    /// physically-landed overlay is never left untracked by overlay state. The retract
    /// itself is bounded best-effort: guaranteed Program removal is owned by the transport
    /// close lifecycle (documented below), not by overlay state. The `state` sync lock is
    /// NEVER held across an inject/retract await.
    ///
    /// DIRTY gate: reject before touching the transport unless `active_epoch ==
    /// Some(run_epoch)` (a stale invocation through a since-replaced transport is rejected
    /// before touching it), the EXACT `{ generation, safe:true }` admission is cached, and
    /// the carrier is not already synced for `run_epoch`. Any matching old `run_epoch` marker
    /// is CLEARED before the physical inject, so `is_synced` fails closed for the whole
    /// re-inject window (only this transaction's own successful commit re-sets the marker).
    ///
    /// COMMIT classification after a successful physical inject:
    /// - `active_epoch` still `Some(run_epoch)`, the recorded content unchanged, AND the
    ///   exact admission `{ generation, safe:true }` cached ⇒ store the synced marker
    ///   tagged `run_epoch`. The bound epoch keys both dirty attribution and the marker;
    ///   `active_epoch` is the independent current-identity veto.
    /// - `active_epoch` still `run_epoch` but the content changed, the record disappeared,
    ///   or the safe admission was lost/changed/unsafe ⇒ a compensating bounded retract:
    ///   the landed overlay is not committed-safe and must not linger (an untracked overlay
    ///   a later unsafe sweep could miss).
    /// - `active_epoch` advanced past `run_epoch` ⇒ no retract: the overlay is on the
    ///   replaced transport instance, whose teardown/replacement lifecycle owns removal; it
    ///   cannot affect the active Program.
    /// - the inject returned `Err` ⇒ retract IFF a prior overlay was physically landed for
    ///   `run_epoch` (its marker was cleared before this re-inject): that overlay is still open
    ///   in the shared Program and now untracked, so a bounded compensating retract removes it.
    ///   A FIRST injection (no prior overlay) that errors needs no retract — nothing landed.
    ///
    /// Every compensating retract (the commit veto's and the inject-`Err` path's) needs NO
    /// post-await marker clear: the marker was cleared before the physical inject (or never set
    /// for a first injection), a vetoed commit never re-set it, and the carrier gate serializes
    /// same-path work — so the marker is already absent through the retract.
    async fn inject_dirty_bound(
        &self,
        transport: &Arc<T>,
        run_epoch: TransportEpoch,
        path: &str,
        generation: u64,
        carrier_gate: &Arc<AsyncMutex<()>>,
    ) {
        let _gate = carrier_gate.lock().await;
        // DIRTY gate under a brief sync lock — never held across the inject await. A stale
        // A/EA invocation after B/EB is rejected before touching A.
        let (content, had_prior_overlay) = {
            let mut state = self.state.lock();
            if state.active_epoch != Some(run_epoch) {
                return;
            }
            let Some(rec) = state.content.get_mut(path) else {
                return;
            };
            // Require the EXACT `{generation, safe:true}` admission BEFORE touching the
            // transport — a carrier without the fresh safe decision for THIS generation is
            // never physically injected (the admission is the shadow-safe gate).
            let admitted = rec
                .shadow_safety
                .as_ref()
                .is_some_and(|c| c.generation == generation && c.safe);
            if !admitted {
                return;
            }
            if record_is_synced(rec, Some(run_epoch)) {
                return;
            }
            // Clear any matching old `run_epoch` marker BEFORE the physical inject — the
            // overlay is being re-landed, so keep the marker ABSENT through the inject AND
            // any compensating retract; only this transaction's own successful commit re-sets
            // it. `is_synced` therefore fails closed for the whole re-inject window. Capture
            // whether a prior overlay was physically landed for `run_epoch` (the marker just
            // cleared): if the re-inject then ERRORS, that overlay is still open but untracked
            // and must be compensating-retracted (else it leaks).
            let had_prior_overlay = rec
                .injected
                .as_ref()
                .is_some_and(|inj| inj.epoch == run_epoch);
            if had_prior_overlay {
                rec.injected = None;
            }
            (Arc::clone(&rec.content), had_prior_overlay)
        };
        // Physical inject — carrier gate held, state lock dropped.
        if transport.inject(path, &content).await.is_err() {
            // The inject reported NO new landing. A FIRST injection (no prior overlay) needs no
            // retract — nothing landed. But if a prior overlay was physically landed for
            // `run_epoch` (its marker was cleared above), that overlay is STILL open in the
            // shared Program and now UNTRACKED — a later unsafe sweep finds no marker and is
            // inert, and a gate-timed-out close has no compensator — so it would leak and could
            // shadow a real user file. Issue a bounded compensating retract under the ALREADY-
            // HELD carrier gate so no untracked overlay lingers; the marker is already cleared.
            if had_prior_overlay {
                let _ =
                    tokio::time::timeout(OVERLAY_CLEANUP_TIMEOUT, transport.retract(path)).await;
            }
            return;
        }
        // Classify the outcome atomically under the sync lock.
        let needs_retract = {
            let mut state = self.state.lock();
            if state.active_epoch != Some(run_epoch) {
                // The overlay landed on a since-replaced transport instance; its
                // teardown/replacement lifecycle owns removal.
                false
            } else if let Some(rec) = state.content.get_mut(path) {
                let admitted = rec
                    .shadow_safety
                    .as_ref()
                    .is_some_and(|c| c.generation == generation && c.safe);
                if admitted && rec.content.as_ref() == content.as_ref() {
                    rec.injected = Some(InjectedRecord {
                        content,
                        epoch: run_epoch,
                    });
                    false
                } else {
                    // Content changed, or the safe admission was lost/changed/unsafe — the
                    // landed overlay is not committed-safe and must be retracted.
                    true
                }
            } else {
                // The record disappeared (a close raced the inject) — retract the overlay.
                true
            }
        };
        if needs_retract {
            // The marker is already ABSENT — cleared before the physical inject, and a vetoed
            // commit never re-set it — and the carrier gate serializes same-path work, so the
            // compensating retract needs NO post-await marker clear.
            let _ = tokio::time::timeout(OVERLAY_CLEANUP_TIMEOUT, transport.retract(path)).await;
        }
    }

    /// The gated unsafe-flip retract: acquire the carrier gate, then, under ONE state lock,
    /// require ALL of — the run epoch is still current (`active_epoch == Some(run_epoch)`),
    /// the EXACT `{ generation, safe:false }` decision is cached, AND a marker tagged
    /// `run_epoch` is present — before clearing that marker and issuing the bounded retract.
    /// The marker is cleared BEFORE the retract await (never after), so `is_synced` stays
    /// fail-closed THROUGH the physical retract even if a newer SAFE decision arrives
    /// mid-retract: generation revalidation alone is insufficient (a flip-back-to-safe could
    /// race the retract), so the clear-before-retract is what closes the served-false-clean
    /// window. Bounded so a wedged relay cannot stall the sweep.
    ///
    /// Any failed check leaves the sweep FULLY INERT — no retract and no marker mutation — so
    /// a SUPERSEDED sweep (an older-generation flip arriving after a newer safe decision, or a
    /// run epoch no longer current) does nothing.
    ///
    /// A physically-landed but not-yet-committed first injection self-retracts under its own
    /// compensating retract (its commit is vetoed by the cached `{safe:false}`); this sweep
    /// then observes no `run_epoch` marker and is inert — so the compensating retract is
    /// ISSUED exactly once and the overlay is never left untracked by overlay state and never
    /// double-closed. Physical removal remains bounded best-effort, owned by the transport
    /// close lifecycle.
    async fn retract_unsafe_bound(
        &self,
        transport: &Arc<T>,
        run_epoch: TransportEpoch,
        path: &str,
        generation: u64,
        carrier_gate: &Arc<AsyncMutex<()>>,
    ) {
        let _gate = carrier_gate.lock().await;
        let should_retract = {
            let mut state = self.state.lock();
            if state.active_epoch != Some(run_epoch) {
                false
            } else if let Some(rec) = state.content.get_mut(path) {
                let decision_unsafe = rec
                    .shadow_safety
                    .as_ref()
                    .is_some_and(|c| c.generation == generation && !c.safe);
                let has_run_epoch_marker = rec
                    .injected
                    .as_ref()
                    .is_some_and(|inj| inj.epoch == run_epoch);
                if decision_unsafe && has_run_epoch_marker {
                    // Clear the marker BEFORE the retract await — `is_synced` stays fail-closed
                    // through the physical retract even if a newer SAFE decision races in.
                    rec.injected = None;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if should_retract {
            let _ = tokio::time::timeout(OVERLAY_CLEANUP_TIMEOUT, transport.retract(path)).await;
        }
    }

    /// Inject EVERY recorded carrier whose content is dirty for the bound transport epoch
    /// AND that `should_inject` admits — the query-time COMPLETENESS step. The queried
    /// carrier's diagnostics need its companion family (`.vue.tsx` + its `.vue.verter.ts`
    /// script) and any other already-open carrier it imports to be members of the SHARED
    /// Program, not just the single queried carrier; injecting the whole recorded open set
    /// at query time keeps the normal open→diagnostics flow correct (an unrelated carrier
    /// is a harmless extra open document).
    ///
    /// Work is attributed to the BOUND transport's epoch (`established.identity.epoch`),
    /// and every per-carrier physical operation runs under that carrier's gate (so a safe
    /// inject and a concurrent unsafe retract of the same path are strictly ordered).
    ///
    /// `should_inject` is the caller's shadow/conflict gate: a recorded path that is NOT
    /// a genuine generated carrier surface — e.g. a real user file occupying a
    /// carrier-companion path — is SKIPPED and, if previously injected, RETRACTED, never
    /// left overlay-shadowing the real file (`carrier_never_shadows_real_user_file`).
    ///
    /// Generation-cached shadow-safety with a dirty-first fast skip: when `generation`
    /// matches the cached one the shadow-safety decision cannot have changed, so the
    /// disk-probing `should_inject` predicate is skipped unless the carrier still needs
    /// work. A cached-SAFE carrier is re-checked only when its content is dirty (a fresh
    /// edit or a post-reconnect epoch replay). A cached-UNSAFE carrier stays cleared and is
    /// NOT re-injected even when content-dirty (an unsafe carrier never re-injects, and
    /// cannot flip back to safe without a generation advance). ANY advance of `generation`
    /// (the caller's workspace content generation — bumped by any file-set/overlay
    /// transition) re-evaluates every carrier, so a real user file appearing at a companion
    /// path is never missed (`carrier_never_shadows_real_user_file`).
    ///
    /// The SERVED invariant does NOT depend on an unsafe carrier's retract completing: the
    /// sweep caches `{safe:false}` BEFORE the retract, and `is_synced` consults
    /// shadow-safety, so a cached-UNSAFE carrier is never reported synced — even while its
    /// injected marker is transiently still set during the retract await — and the inject
    /// transaction's commit veto refuses to restore that marker. A bounded or timed-out
    /// retract may therefore leave that overlay open in the SHARED Program until the
    /// transport's own ordered close path completes, the session/transport is drained, or
    /// the transport is replaced — guaranteed Program removal is owned by the shared
    /// transport close lifecycle, not by overlay state.
    ///
    /// A transport RE-establishment resets the injection markers ([`Self::ensure`] →
    /// [`Self::observe_transport_identity`]), so the open carrier set replays into the fresh
    /// transport on the next query: every carrier is epoch-dirty, so a cached-SAFE carrier
    /// re-injects (a reconnect is never served against a transport that never received the
    /// open documents). A cached-UNSAFE carrier stays cleared even though it is content-
    /// dirty — its shadow-safety decision is keyed on the workspace content generation,
    /// orthogonal to the transport epoch, so while the real user file still occupies its
    /// companion path it must not be injected into the fresh transport.
    pub(crate) async fn inject_all_dirty<F>(
        &self,
        established: &EstablishedTransport<T>,
        generation: u64,
        should_inject: F,
    ) where
        F: Fn(&str) -> bool,
    {
        let run_epoch = established.identity.epoch;
        let transport = &established.transport;
        // Snapshot the candidate carriers under a brief lock — never held across the
        // predicate's disk probe or the inject/retract await.
        //
        // When the shadow-safety generation is CLEAN (it matches the cached one), the
        // shadow-safety decision cannot have changed, so the disk-probing predicate is
        // skipped: a cached-SAFE carrier is a candidate only if its content is dirty (a
        // fresh edit OR a post-reconnect epoch replay); a cached-UNSAFE carrier stays
        // cleared. When the generation is STALE, every carrier is a candidate
        // (re-evaluate) — so any file-set transition that could flip shadow-safety is never
        // missed (`carrier_never_shadows_real_user_file`).
        let candidates: Vec<String> = {
            let state = self.state.lock();
            state
                .content
                .iter()
                .filter_map(|(path, rec)| {
                    let content_dirty = !record_is_synced(rec, Some(run_epoch));
                    let (shadow_fresh, cached_safe) = match rec.shadow_safety.as_ref() {
                        Some(cache) if cache.generation == generation => (true, cache.safe),
                        _ => (false, false),
                    };
                    let candidate = if shadow_fresh {
                        // Generation-clean: re-inject a cached-SAFE carrier only when
                        // content-dirty; leave a cached-UNSAFE carrier cleared.
                        cached_safe && content_dirty
                    } else {
                        // Stale generation ⇒ re-evaluate shadow-safety.
                        true
                    };
                    candidate.then(|| path.clone())
                })
                .collect()
        };
        for path in candidates {
            // The FRESH shadow-safety decision for THIS generation is recorded BEFORE the
            // gated inject/retract (never held across it; monotonic, so an older-generation
            // run cannot regress a newer decision). A concurrent in-flight injection then
            // observes this generation's decision at ITS commit: a concurrent flip to
            // `{safe:false}` VETOES the stale commit, while a genuine re-inject after a
            // flip-back-to-safe (`{safe:true}`) is not spuriously vetoed by the PRIOR
            // generation's cached-unsafe decision.
            if should_inject(&path) {
                // The single-carrier entry records the `{safe:true}` admission and runs the
                // gated inject transaction against the bound epoch.
                self.inject_dirty(established, &path, generation).await;
            } else {
                // Flip-to-unsafe: cache `{safe:false}` (so `is_synced` fails closed the
                // instant it is observed) then retract the carrier's overlay so it leaves
                // the SHARED Program (its `ContentRecord` is KEPT so it re-injects if it
                // later flips back to safe), under the carrier gate so it is ordered
                // w.r.t. any in-flight inject of the same carrier.
                self.cache_shadow_decision(&path, run_epoch, generation, false);
                let gate = self.carrier_gate(&path);
                self.retract_unsafe_bound(transport, run_epoch, &path, generation, &gate)
                    .await;
            }
        }
    }

    /// Whether the carrier's CURRENT recorded content is confirmed synced into the
    /// shared Program for the active transport epoch — its latest dirty injection
    /// SUCCEEDED into the currently-active transport. A carrier whose current content
    /// failed to inject, was injected into a since-reconnected transport, OR is currently
    /// cached shadow-UNSAFE is NOT synced: the composite fails closed to OWNED for that
    /// query rather than serve SHARED diagnostics computed against stale/absent content or
    /// overlay-shadow a real user file (`carrier_never_shadows_real_user_file`). An
    /// unrecorded carrier is not synced.
    pub(crate) fn is_synced(&self, path: &str) -> bool {
        let state = self.state.lock();
        let active = state.active_epoch;
        state
            .content
            .get(path)
            .is_some_and(|rec| record_is_synced(rec, active))
    }

    /// Retract a carrier from the SHARED Program OFF the OWNED close critical path —
    /// BOUNDED and fail-closed. Drops the recorded content (a sync insert), then runs the
    /// whole physical close — the per-path carrier-gate acquisition, a reopen revalidation,
    /// the transport lookup, and the retract — under ONE deadline computed ONCE from
    /// `timeout` (NOT additive per-step timeouts), so a held gate or a slow/dead relay can
    /// neither hang nor delay the OWNED `close_file` path past that bound (a broken transport
    /// is torn down / evicted anyway). NEVER establishes the transport (a close must not
    /// trigger — or head-of-line-block on — an establishment).
    ///
    /// The carrier gate orders the close w.r.t. any in-flight injection / reopen of the same
    /// path, and after acquiring it the close decides from ONE state read:
    ///
    /// - the path is still ABSENT from the content map ⇒ the plain close: retract.
    /// - the path was REOPENED and the reopened record carries a COMMITTED injection marker
    ///   (the reopen's own inject landed + committed while this close was parked; any marker
    ///   on the reopened record post-dates the reopen) ⇒ do nothing: the reopen's physical
    ///   inject refreshed the path, so the overlay this close's erase untracked no longer
    ///   exists, and a retract here would delete the reopen's NEW overlay.
    /// - the path was REOPENED with NO committed marker (the reopen is shadow-unsafe, or its
    ///   inject has not run yet — it is queued behind this gate) ⇒ the overlay whose marker
    ///   the erase removed is ORPHANED — a marker-less unsafe sweep is inert on it — so
    ///   dispatch the compensating retract (bounded by the same close deadline), IFF the
    ///   captured marker's transport epoch is still current at that state read (an epoch
    ///   already advanced at the read means the overlay is on a since-replaced transport
    ///   instance, whose teardown/replacement lifecycle owns removal — mirroring the inject
    ///   commit classification). The epoch guard gates only that DECISION: the retract itself
    ///   dispatches on the CURRENTLY-established transport ([`Self::current`]) after the
    ///   state lock is released, which reaches the shared, transport-persistent Program — so
    ///   a transport replacement landing between the epoch read and the dispatch still
    ///   removes the orphaned overlay, and the held carrier gate keeps any reopen from
    ///   committing a NEW overlay at the path in that window, so no live overlay is wrongly
    ///   removed.
    ///
    /// The pre-erase injection marker (content incarnation + run epoch), captured atomically
    /// with the erase, is what tells OUR orphaned overlay apart from a different overlay a
    /// reopen committed. This compensates the demonstrated shadow-unsafe reopen orphan; it is
    /// NOT a "never orphaned" guarantee. TODO(follow-up): ownership of a physically-landed
    /// overlay is still lost when the close deadline expires before the gated section runs;
    /// when no transport is currently established at retract time (`current()` returns
    /// `None`); and when the dispatched retract itself times out or is cancelled with an
    /// unknown outcome. An epoch advance retires markers without sweeping the replaced
    /// instance's overlays ([`Self::observe_transport_identity`]), the dispatched retract is
    /// not transport-identity-bound (the dispatched transport's epoch/identity is never
    /// re-verified against the one the compensation decision was read under), and the
    /// transport wire carries no lease/incarnation token, so an overlay recreated at the
    /// same path is not distinguishable end-to-end. Each remains a tracked follow-up for a
    /// systematic ownership ledger rather than point compensation here.
    ///
    /// A gate-acquire timeout fails closed within the deadline — an in-flight gated inject
    /// will observe the absence and compensate, and a reopen's inject is ordered behind this
    /// gate.
    pub(crate) async fn retract_bounded(&self, path: &str, timeout: Duration) {
        // Drop the recorded content immediately so the carrier is closed locally regardless
        // of the transport retract outcome — capturing, atomically with the erase, the
        // injection marker the erase removes (the committed content incarnation + the
        // transport epoch it was injected into). Together with `path` it identifies the
        // physical overlay this close untracks; the reopened branch below needs it to tell
        // that orphan apart from a different overlay a reopen may have committed.
        let prior_overlay = self.take_content(path).and_then(|rec| rec.injected);
        // Bound the ENTIRE physical close by the ORIGINAL deadline computed ONCE — the gate
        // acquisition and the retract share it, never additive.
        let deadline = tokio::time::Instant::now() + timeout;
        let gate = self.carrier_gate(path);
        let _ = tokio::time::timeout_at(deadline, async {
            let _gate = gate.lock().await;
            let should_retract = {
                let state = self.state.lock();
                match state.content.get(path) {
                    // Still absent: the plain close retracts.
                    None => true,
                    // Reopened + a committed marker: the reopen's NEW overlay owns the
                    // path — never delete it.
                    Some(rec) if rec.injected.is_some() => false,
                    // Reopened, no committed overlay: compensate OUR orphaned overlay iff
                    // one was physically landed and its transport instance is still
                    // current.
                    Some(_) => prior_overlay
                        .as_ref()
                        .is_some_and(|prior| state.active_epoch == Some(prior.epoch)),
                }
            };
            if !should_retract {
                return;
            }
            if let Some(transport) = self.current().await {
                let _ = transport.retract(path).await;
            }
        })
        .await;
    }
}

#[cfg(test)]
#[path = "overlay_core_tests.rs"]
mod overlay_core_tests;

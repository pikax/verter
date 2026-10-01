//! The lazy SHARED-overlay core: a content cache the OWNED lifecycle records into
//! OFF the critical path, plus the per-carrier synchronization state the QUERY
//! path drives against the HUB-OWNED serving incarnation.
//!
//! [`LazyOverlayCore`] separates the two timings the composite must keep
//! independent:
//!
//! - **Lifecycle (OWNED-budgeted).** `open_file` / `update_file` etc. record the
//!   carrier's current content via [`LazyOverlayCore::record_content_at_priority`] —
//!   a plain sync insert that NEVER establishes the SHARED attach. So opting into
//!   SHARED cannot trip the OWNED file-lifecycle timing (the foreground TSX sync is
//!   budgeted far below the SHARED establishment bound).
//! - **Query (OFF the foreground-sync budget).** The composite establishes the
//!   SHARED attach lazily through its [`ProviderHub`](crate::provider_hub::ProviderHub)
//!   (singleflight, discriminant-re-arming, retire-on-death) and injects the
//!   carrier's recorded content ([`LazyOverlayCore::inject_all_dirty`], only when
//!   it changed since the last injection). Fail-closed: until the attach
//!   establishes, the composite serves OWNED; the overlay self-heals on a later
//!   query.
//!
//! The SERVING identity — the transport instance and its [`ProviderEpoch`] — is
//! minted by the hub actor; its overlay partition observes each successful install
//! ([`LazyOverlayCore::observe_serving_epoch`]) so a re-attachment resets the
//! injection markers atomically (the open carrier set replays into the fresh
//! attach; a reconnect is never served against an attach that never received the
//! open documents).
//!
//! Every injection is attributed to the EPOCH of the serving incarnation it runs
//! against, never a re-read of the current `active_epoch`, and every per-carrier
//! physical operation (an inject transaction plus any compensating retract, and
//! an unsafe-flip retract) runs under a stable per-path carrier gate — so a
//! stale injection through a since-replaced attach cannot mark the current epoch
//! synced, and an overlay physically landed against the STILL-current epoch is
//! retracted before a later re-inject of the same path. An overlay left on a
//! since-replaced attach instance (the epoch advanced past its run epoch) is
//! owned by that instance's teardown/replacement lifecycle, not retracted here.
//!
//! Generic over the transport `T` (through the [`OverlayTransport`] seam) so the
//! off-critical-path property is unit-testable with a transport double. The
//! PRODUCTION transport is [`HubAdmittedTransport`]: every inject consumes a
//! hub-issued admission.

use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::Duration;

use parking_lot::Mutex as SyncMutex;
use tokio::sync::Mutex as AsyncMutex;

use crate::protocol::TypeProviderError;
use crate::provider_hub::{
    AdmittedRequest, OverlayFileKind, OverlayPriority as HubOverlayPriority, ProviderEpoch,
    ProviderHub,
};
use crate::traits::{ProviderFuture, TypeProvider};

/// Exact bytes of one member successfully applied to one serving incarnation.
/// Construction stays in the hub's application partition.
#[derive(Clone)]
pub struct AppliedOverlayMember {
    path: Arc<str>,
    content: Arc<str>,
    epoch: ProviderEpoch,
}
impl AppliedOverlayMember {
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn content(&self) -> &Arc<str> {
        &self.content
    }
    pub fn epoch(&self) -> ProviderEpoch {
        self.epoch
    }
}

/// Successful work from one synchronization pass, never its candidate set.
pub struct SynchronizationReceipt {
    pub(super) epoch: ProviderEpoch,
    pub(super) members: Vec<AppliedOverlayMember>,
}
impl SynchronizationReceipt {
    pub fn epoch(&self) -> ProviderEpoch {
        self.epoch
    }
    pub fn members(&self) -> &[AppliedOverlayMember] {
        &self.members
    }
}

/// The bound on a compensating retract issued from the query-time injection path — both
/// the inject transaction's own not-committed-safe cleanup and the sweep's flip-to-unsafe
/// retract. A slow/dead relay retract cannot stall the sweep or the transaction. Symmetric
/// with the OWNED-close retract bound, and the whole sweep is additionally under the
/// composite's outer query deadline — so a wedged relay never delays diagnostics past
/// those bounds.
const OVERLAY_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

/// The proof that ONE generated unit may be written into the shared engine: a
/// HUB-ISSUED [`AdmittedRequest`] — the ProviderHub's admission that the
/// unit's whole write set is a member of its owning configured project, bound
/// to the exact serving provider and epoch.
///
/// The shared engine is an engine Verter does not own. A unit written there
/// without its configured project admitting it lands in an inferred project:
/// the wrong compiler options for Verter's answer, and extra load on the
/// editor's own TypeScript service. So the physical write path
/// ([`LazyOverlayCore::inject_permitted`]) takes a permit by value, and there
/// is no way to build one from a path or a `bool` — only the hub issues it,
/// and the hub-issued admission IS the write authorization.
pub struct GeneratedUnitWritePermit(WriteProof);

enum WriteProof {
    /// The hub-issued admission the production write routes through.
    Admitted(AdmittedRequest),
    /// Transport-double tests drive the state machine, not admission.
    #[cfg(test)]
    Bare,
}

impl GeneratedUnitWritePermit {
    /// The permit carrying a hub-issued admission — the ONLY production form.
    pub fn admitted(admission: AdmittedRequest) -> Self {
        Self(WriteProof::Admitted(admission))
    }

    /// The hub-issued admission this permit routes the write through, if any.
    pub fn admission(&self) -> Option<&AdmittedRequest> {
        match &self.0 {
            WriteProof::Admitted(admission) => Some(admission),
            #[cfg(test)]
            WriteProof::Bare => None,
        }
    }

    /// The permit a transport double's sweep carries (tests of the overlay
    /// state machine, never of admission).
    #[cfg(test)]
    pub fn bare() -> Self {
        Self(WriteProof::Bare)
    }
}

/// A sweep predicate's verdict for one recorded unit: a [`GeneratedUnitWritePermit`]
/// to write it, or none — skip it, and retract it if a prior sweep wrote it.
pub struct InjectionVerdict(Option<GeneratedUnitWritePermit>);

impl From<Option<GeneratedUnitWritePermit>> for InjectionVerdict {
    fn from(permit: Option<GeneratedUnitWritePermit>) -> Self {
        Self(permit)
    }
}

/// Transport-double tests drive the sweep with a bare `bool` verdict: they exercise the
/// overlay state machine, not admission. Production has no such conversion — its only
/// way to a verdict is a hub-issued admission.
#[cfg(test)]
impl From<bool> for InjectionVerdict {
    fn from(write: bool) -> Self {
        Self(write.then_some(GeneratedUnitWritePermit::bare()))
    }
}

/// The transport seam the overlay core drives: inject / retract a carrier
/// overlay. Kept small and carrier-only so the core is unit-testable with a
/// double — it never resolves types or reads a store. Liveness and teardown
/// are NOT part of the seam: the hub that owns the serving incarnation owns
/// its death detection and teardown.
pub trait OverlayTransport: Send + Sync + 'static {
    /// Inject (or refresh) a carrier overlay — the ordered per-carrier state
    /// machine, gated by the write permit's hub-issued admission in
    /// production.
    fn inject(
        &self,
        path: &str,
        content: &str,
        permit: &GeneratedUnitWritePermit,
    ) -> ProviderFuture<'_, ()>;
    /// Retract a carrier overlay (a withdrawal — it needs no admission).
    fn retract(&self, path: &str) -> ProviderFuture<'_, ()>;
}

/// The PRODUCTION [`OverlayTransport`]: a serving incarnation of the hub the
/// shared overlay owns. Every inject consumes the permit's hub-issued
/// admission through [`ProviderHub::forward_admitted_file`] — the production
/// shared write entry; a refused admission is a FAILED injection (the carrier
/// stays dirty and the next sweep re-admits at the fresh basis, with zero
/// writes). Retracts and teardown order directly through the serving
/// provider: a withdrawal owns no admission to consume.
pub struct HubAdmittedTransport<P: ?Sized> {
    provider: Arc<P>,
    epoch: ProviderEpoch,
    hub: Arc<ProviderHub<P>>,
}

impl<P> HubAdmittedTransport<P>
where
    P: TypeProvider + ?Sized + Send + Sync + 'static,
{
    /// Bind the hub's serving provider to the hub's admission door.
    pub fn new(provider: Arc<P>, hub: Arc<ProviderHub<P>>, epoch: ProviderEpoch) -> Self {
        Self {
            provider,
            hub,
            epoch,
        }
    }

    /// The serving provider this transport carries.
    pub fn provider(&self) -> &Arc<P> {
        &self.provider
    }
}

impl<P> OverlayTransport for HubAdmittedTransport<P>
where
    P: TypeProvider + ?Sized + Send + Sync + 'static,
{
    fn inject(
        &self,
        path: &str,
        content: &str,
        permit: &GeneratedUnitWritePermit,
    ) -> ProviderFuture<'_, ()> {
        let Some(admission) = permit.admission() else {
            return Box::pin(async {
                Err(TypeProviderError::new(
                    "no hub-issued admission carried by the write permit",
                ))
            });
        };
        let admission = admission.clone();
        let hub = Arc::clone(&self.hub);
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            hub.forward_admitted_file(
                &admission,
                &path,
                &content,
                OverlayFileKind::Open,
                HubOverlayPriority::Foreground,
            )
            .await
            .map_err(|reason| {
                TypeProviderError::new(format!("hub generated-unit write refused: {reason:?}"))
            })
        })
    }

    fn retract(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_owned();
        Box::pin(async move { self.hub.retract_overlay(self.epoch, &path).await })
    }
}

/// A hub-owned serving incarnation handed to the overlay core: the provider
/// plus the EXACT [`ProviderEpoch`] the hub minted for it. Every core
/// operation is attributed to that epoch, never a re-read of the current one.
pub struct ServingTransport<T> {
    pub transport: Arc<T>,
    pub epoch: ProviderEpoch,
}

/// The content a query-time injection SUCCESSFULLY committed, tagged with the transport
/// EPOCH it was injected into. The epoch tag is what makes a re-established transport
/// (a new epoch) mark the carrier dirty — the open set replays — and what stops a stale
/// in-flight injection from marking a NEW epoch synced.
struct InjectedRecord {
    content: Arc<str>,
    epoch: ProviderEpoch,
}

/// The cached shadow-safety decision for a carrier at a workspace content generation —
/// so a content-clean carrier can skip the disk-probing shadow-safety predicate until
/// the generation advances (a file-set transition could flip the decision).
struct ShadowSafetyCache {
    generation: u64,
    safe: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OverlayPriority {
    Background,
    Normal,
    Interactive,
}

/// The recorded state for one carrier companion: the LATEST content the lifecycle fed,
/// the last content a query-time injection committed (with its transport epoch), and the
/// cached shadow-safety decision (with the generation it was decided at).
struct ContentRecord {
    /// The latest content the lifecycle recorded.
    content: Arc<str>,
    /// The lifecycle authority that recorded `content`. A lower-priority producer
    /// cannot overwrite bytes from a higher-priority editor lane.
    priority: OverlayPriority,
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
    active_epoch: Option<ProviderEpoch>,
    /// The recorded content per carrier companion path.
    content: HashMap<String, ContentRecord>,
    /// Physical withdrawals survive missing transports, cancellation and deadlines.
    withdrawals: HashMap<String, Option<ProviderEpoch>>,
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
fn record_is_synced(rec: &ContentRecord, active_epoch: Option<ProviderEpoch>) -> bool {
    rec.shadow_safety.as_ref().is_none_or(|c| c.safe)
        && rec.injected.as_ref().is_some_and(|inj| {
            inj.content.as_ref() == rec.content.as_ref() && Some(inj.epoch) == active_epoch
        })
}

/// The exact terminal synchronization state for a queried carrier and the transport
/// instance an engagement is about to return. This is deliberately richer than a bool:
/// a refusal must distinguish missing content, a failed/dirty injection, a shadow-safety
/// veto, and a transport replacement that happened after the query captured its provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlaySyncState {
    /// No lifecycle lane recorded this companion.
    Unrecorded,
    /// The overlay has not observed any established transport yet.
    NoActiveTransport,
    /// The transport captured by this query is no longer the active transport.
    TransportEpochMismatch {
        expected: ProviderEpoch,
        active: ProviderEpoch,
    },
    /// A real-file/shadow conflict vetoed this companion at the named content generation.
    ShadowUnsafe { generation: u64 },
    /// The companion was recorded but no injection has committed.
    NeverInjected,
    /// The last committed injection belongs to another transport instance.
    InjectedIntoDifferentEpoch {
        expected: ProviderEpoch,
        injected: ProviderEpoch,
    },
    /// The recorded content advanced after the last committed injection.
    ContentDirty,
    /// Current content is barrier-confirmed in the exact transport requested by the query.
    Synced,
}

impl OverlaySyncState {
    #[must_use]
    pub fn is_synced(self) -> bool {
        self == Self::Synced
    }
}

/// The lazy SHARED-overlay core: a per-carrier content cache (recorded by the OWNED
/// lifecycle, off the establishment path) plus the per-carrier synchronization state
/// driven against the HUB-OWNED serving incarnation. Generic over the transport `T`.
pub struct LazyOverlayCore<T> {
    /// The per-carrier recorded state + the active serving epoch under ONE sync
    /// lock — the OWNED lifecycle writes content without any establishment.
    state: SyncMutex<OverlayState>,
    /// The stable per-path async operation gates. One gate per carrier path, retained
    /// behind a `Weak` so it is dropped once no operation holds it; an in-flight operation
    /// on an old slot and a reopened operation for the SAME path upgrade the SAME live
    /// `Weak` and therefore serialize on ONE gate. Dead entries are pruned opportunistically.
    carrier_gates: SyncMutex<HashMap<String, Weak<AsyncMutex<()>>>>,
    /// The transport type this core drives — type identity only. The SERVING
    /// instance is handed in per operation (the hub owns it); nothing of the
    /// transport is stored here.
    transport: std::marker::PhantomData<fn() -> T>,
}

impl<T: OverlayTransport> LazyOverlayCore<T> {
    pub(super) fn new() -> Self {
        Self {
            state: SyncMutex::new(OverlayState {
                active_epoch: None,
                content: HashMap::new(),
                withdrawals: HashMap::new(),
            }),
            carrier_gates: SyncMutex::new(HashMap::new()),
            transport: std::marker::PhantomData,
        }
    }

    /// Record a carrier's current content OFF the establishment critical path — a
    /// plain sync insert the OWNED lifecycle calls. NEVER establishes the transport;
    /// the query path injects the recorded content lazily. Test-only convenience:
    /// production routes through [`Self::record_content_at_priority`] with the
    /// caller's lifecycle-lane authority.
    #[cfg(test)]
    pub fn record_content(&self, path: &str, content: &str) {
        self.record_content_at_priority(path, content, OverlayPriority::Interactive);
    }

    /// The single publication point for recorded carrier content: a producer may
    /// only replace bytes published at an equal-or-lower authority. A late
    /// BACKGROUND record (sourced from an older disk snapshot) can never overwrite
    /// INTERACTIVE editor bytes; the editor lane keeps absolute priority until the
    /// record is retracted on close.
    pub fn record_content_at_priority(&self, path: &str, content: &str, priority: OverlayPriority) {
        let mut state = self.state.lock();
        match state.content.get_mut(path) {
            Some(rec) if priority >= rec.priority => {
                rec.content = Arc::from(content);
                rec.priority = priority;
            }
            Some(_) => {}
            None => {
                state.content.insert(
                    path.to_string(),
                    ContentRecord {
                        content: Arc::from(content),
                        priority,
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
        let removed = {
            let mut state = self.state.lock();
            let epoch = state.active_epoch;
            // A never-attached overlay has no physical document to withdraw.
            if epoch.is_some() {
                state.withdrawals.insert(path.to_string(), epoch);
            }
            state.content.remove(path)
        };
        // Prune dead carrier-gate registry entries on the close path too (not only on a fresh
        // gate mint) — a completed operation leaves a dead `Weak`, and close churn should not
        // leak those between mints. A gate a live operation still holds is retained.
        self.carrier_gates
            .lock()
            .retain(|_, weak| weak.strong_count() > 0);
        removed
    }

    /// Every recorded unit path, sorted — the candidate write set a caller groups by
    /// carrier source to decide generated-unit admission.
    pub fn recorded_paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = self.state.lock().content.keys().cloned().collect();
        paths.sort();
        paths
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
        run_epoch: ProviderEpoch,
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

    /// Observe the hub-minted serving epoch MONOTONICALLY: adopt the epoch AND reset
    /// every injection marker ONLY when `active_epoch` is `None` OR the observed epoch is
    /// STRICTLY GREATER than the current one (a genuine re-attachment to a fresh serving
    /// incarnation) — the open carrier set is no longer synced into the (now dead /
    /// replaced) prior attach and must replay into the fresh one. Equal and OLDER epochs
    /// are IGNORED: a runtime worker preempted between the hub's establishment return and
    /// this synchronous observe could deliver a stale `observe(E1)` after another worker
    /// already committed+observed E2, and a stale late observe must NOT regress
    /// `active_epoch` E2→E1 or reset the fresh markers. The shadow-safety caches are
    /// LEFT intact: they are keyed on the workspace content generation, orthogonal to
    /// the serving epoch.
    ///
    /// Called by the composite RIGHT AFTER the hub hands it a serving incarnation —
    /// before any injection runs against that epoch.
    pub(super) fn observe_serving_epoch(&self, epoch: ProviderEpoch) {
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

    /// Inject ONE carrier's recorded content into the serving incarnation IF it
    /// is not already synced for that epoch — the single-carrier query-time
    /// injection off the OWNED lifecycle path.
    ///
    /// The injection is attributed to the BOUND serving epoch
    /// (`serving.epoch`), never a re-read of the current `active_epoch`, so a
    /// stale invocation driven through a since-replaced incarnation cannot
    /// mark the current epoch synced. A direct single-carrier injection
    /// asserts the carrier is shadow-safe at `generation`; that decision is
    /// recorded before the gated transaction so the commit's admission gate
    /// is satisfiable. Best-effort + fail-closed: on failure the content
    /// stays dirty and a later query retries (self-healing).
    #[cfg(test)]
    pub async fn inject_dirty(&self, serving: &ServingTransport<T>, path: &str, generation: u64) {
        self.inject_permitted(serving, path, generation, GeneratedUnitWritePermit::bare())
            .await;
    }

    /// The single-carrier physical write entry. It CONSUMES a
    /// [`GeneratedUnitWritePermit`] — in production a HUB-ISSUED admission —
    /// so reaching the shared engine requires the ProviderHub's admission for
    /// the unit's whole write set; the permit also stands for the
    /// shadow-safety decision recorded here for `generation`.
    pub(super) async fn inject_permitted(
        &self,
        serving: &ServingTransport<T>,
        path: &str,
        generation: u64,
        permit: GeneratedUnitWritePermit,
    ) -> Option<AppliedOverlayMember> {
        let run_epoch = serving.epoch;
        self.cache_shadow_decision(path, run_epoch, generation, true);
        let gate = self.carrier_gate(path);
        self.inject_dirty_bound(
            &serving.transport,
            run_epoch,
            path,
            generation,
            &gate,
            &permit,
        )
        .await
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
        run_epoch: ProviderEpoch,
        path: &str,
        generation: u64,
        carrier_gate: &Arc<AsyncMutex<()>>,
        permit: &GeneratedUnitWritePermit,
    ) -> Option<AppliedOverlayMember> {
        let gate_wait = std::time::Instant::now();
        let _gate = carrier_gate.lock().await;
        let gate_waited = gate_wait.elapsed();
        if gate_waited > std::time::Duration::from_millis(500) {
            tracing::debug!(
                path,
                waited_ms = gate_waited.as_millis() as u64,
                "shared overlay: waited for the carrier gate"
            );
        }
        // DIRTY gate under a brief sync lock — never held across the inject await. A stale
        // A/EA invocation after B/EB is rejected before touching A.
        let (content, had_prior_overlay) = {
            let mut state = self.state.lock();
            if state.active_epoch != Some(run_epoch) {
                return None;
            }
            let rec = state.content.get_mut(path)?;
            // Require the EXACT `{generation, safe:true}` admission BEFORE touching the
            // transport — a carrier without the fresh safe decision for THIS generation is
            // never physically injected (the admission is the shadow-safe gate).
            let admitted = rec
                .shadow_safety
                .as_ref()
                .is_some_and(|c| c.generation == generation && c.safe);
            if !admitted {
                return None;
            }
            if record_is_synced(rec, Some(run_epoch)) {
                return None;
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
        // Physical inject — carrier gate held, state lock dropped. In
        // production the transport routes this through the hub-issued
        // admission the permit carries.
        let inject_started = std::time::Instant::now();
        let injected = transport.inject(path, &content, permit).await;
        if let Err(error) = &injected {
            tracing::debug!(
                path,
                elapsed_ms = inject_started.elapsed().as_millis() as u64,
                %error,
                "shared overlay: injection failed"
            );
        }
        if injected.is_err() {
            // The inject reported NO new landing. A FIRST injection (no prior overlay) needs no
            // retract — nothing landed. But if a prior overlay was physically landed for
            // `run_epoch` (its marker was cleared above), that overlay is STILL open in the
            // shared Program and now UNTRACKED — a later unsafe sweep finds no marker and is
            // inert, and a gate-timed-out close has no compensator — so it would leak and could
            // shadow a real user file. Issue a bounded compensating retract under the ALREADY-
            // HELD carrier gate so no untracked overlay lingers; the marker is already cleared.
            if had_prior_overlay {
                let _ = tokio::time::timeout(
                    OVERLAY_CLEANUP_TIMEOUT,
                    self.retract_tracked(path, transport, Some(run_epoch)),
                )
                .await;
            }
            return None;
        }
        // Classify the outcome atomically under the sync lock.
        let mut applied = None;
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
                    applied = Some(AppliedOverlayMember {
                        path: Arc::from(path),
                        content: Arc::clone(&content),
                        epoch: run_epoch,
                    });
                    rec.injected = Some(InjectedRecord {
                        content,
                        epoch: run_epoch,
                    });
                    state.withdrawals.remove(path);
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
            let _ = tokio::time::timeout(
                OVERLAY_CLEANUP_TIMEOUT,
                self.retract_tracked(path, transport, Some(run_epoch)),
            )
            .await;
        }
        applied
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
        run_epoch: ProviderEpoch,
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
            let _ = tokio::time::timeout(
                OVERLAY_CLEANUP_TIMEOUT,
                self.retract_tracked(path, transport, Some(run_epoch)),
            )
            .await;
        }
    }

    /// Inject every recorded carrier that `in_scope` admits, whose content is dirty for
    /// the bound transport epoch, and that `should_inject` admits — the query-time
    /// COMPLETENESS step.
    ///
    /// The queried carrier's diagnostics need its companion family (`.vue.tsx` + its
    /// `.vue.verter.ts` script) and the carriers it imports to be members of the SHARED
    /// Program, not just the single queried carrier. `in_scope` is what bounds that to
    /// the set the editor actually demanded: every carrier an editor lifecycle lane
    /// recorded — the open documents and the import closure the background import
    /// publication delivers — plus the queried carrier's own family.
    ///
    /// It is NOT the whole recorded set. A workspace scan records every carrier in the
    /// project on the BACKGROUND lane, and injecting those was a whole-project publish
    /// charged to whichever interactive request happened to arrive first: sequential, one
    /// relay round-trip per carrier, in arbitrary map order, with no priority for the
    /// request's own file. On a real project that took ~30 s, so every hover / definition
    /// / code-action in that window expired inside this call and returned a cancellation
    /// without ever reaching the engine — while the same work cost 5-8 ms once the sweep
    /// had drained. Speculatively-published carriers become members when the editor
    /// demands them (an open, or an import the publication pass delivers), which is what
    /// upgrades their recorded lane.
    ///
    /// Work is attributed to the BOUND transport's epoch (`established.identity.epoch`),
    /// and every per-carrier physical operation runs under that carrier's gate (so a safe
    /// inject and a concurrent unsafe retract of the same path are strictly ordered).
    ///
    /// `should_inject` is the caller's write gate, answering with a
    /// [`GeneratedUnitWritePermit`] or none. A recorded path with no permit — a unit its
    /// configured project does not admit, or one that is NOT a genuine generated carrier
    /// surface (a real user file occupying a carrier-companion path) — is SKIPPED and, if
    /// previously injected, RETRACTED: never written into an inferred project, never left
    /// overlay-shadowing the real file (`carrier_never_shadows_real_user_file`).
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
    /// A serving-epoch change resets the injection markers
    /// ([`Self::observe_serving_epoch`]), so the open carrier set replays into the
    /// fresh attach on the next query: every carrier is epoch-dirty, so a cached-SAFE
    /// carrier re-injects (a reconnect is never served against an attach that never
    /// received the open documents). A cached-UNSAFE carrier stays cleared even though
    /// it is content-dirty — its shadow-safety decision is keyed on the workspace
    /// content generation, orthogonal to the serving epoch, so while the real user file
    /// still occupies its companion path it must not be injected into the fresh attach.
    pub(super) async fn inject_all_dirty<S, F, V>(
        &self,
        serving: &ServingTransport<T>,
        generation: u64,
        in_scope: S,
        should_inject: F,
    ) -> SynchronizationReceipt
    where
        S: Fn(&str, OverlayPriority) -> bool,
        F: Fn(&str) -> V,
        V: Into<InjectionVerdict>,
    {
        self.inject_all_dirty_paced(serving, generation, in_scope, should_inject, usize::MAX)
            .await
    }

    /// [`Self::inject_all_dirty`] issuing at most `at_once` carriers together, and
    /// reporting how many carriers the pass had to consider.
    ///
    /// A REQUEST injects its whole scope together (`usize::MAX`): that is the work
    /// it is waiting for. The background pre-injection paces itself instead — the
    /// editor's engine absorbs an overlay in a few hundred milliseconds and the
    /// relay's control channel is serial, so a request arriving mid-sweep waits for
    /// at most one small group rather than for the rest of the project.
    pub(super) async fn inject_all_dirty_paced<S, F, V>(
        &self,
        serving: &ServingTransport<T>,
        generation: u64,
        in_scope: S,
        should_inject: F,
        at_once: usize,
    ) -> SynchronizationReceipt
    where
        S: Fn(&str, OverlayPriority) -> bool,
        F: Fn(&str) -> V,
        V: Into<InjectionVerdict>,
    {
        let run_epoch = serving.epoch;
        let transport = &serving.transport;
        let withdrawals: Vec<_> = self.state.lock().withdrawals.keys().cloned().collect();
        futures_util::future::join_all(withdrawals.iter().map(|path| async {
            let gate = self.carrier_gate(path);
            let _ = tokio::time::timeout(OVERLAY_CLEANUP_TIMEOUT, async {
                let _gate = gate.lock().await;
                if self.state.lock().active_epoch == Some(run_epoch) {
                    self.finish_withdrawal(path, transport).await;
                }
            })
            .await;
        }))
        .await;
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
                    if !in_scope(path, rec.priority) {
                        // Out of the editor-demand scope: a speculatively published
                        // carrier nobody has asked for. Not deferred work — work no
                        // request may be charged for.
                        return None;
                    }
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
        if !candidates.is_empty() {
            tracing::debug!(
                candidates = candidates.len(),
                epoch = ?run_epoch,
                "shared overlay: injecting dirty editor-demand carriers"
            );
        }
        // CONCURRENT, not one after another: each carrier is ordered by its own gate,
        // and writes issued together ride ONE sync barrier (the transport coalesces
        // them), where a sequential sweep pays the editor's engine a program update
        // per overlay — ~20 s for the import closure of one open file.
        //
        // Every candidate runs the caller's write gate: a CACHED `{safe:true}`
        // shadow decision stands for the shadow half (recorded only by a permitted
        // inject), but the WRITE AUTHORIZATION — the hub-issued admission the
        // production verdict carries — is minted per sweep, because it binds the
        // exact serving epoch and basis: a replacement incarnation, a publication,
        // or a membership change must re-admit, never replay a prior epoch's
        // authorization. Warmth comes from the gate's own memos (the per-source
        // shadow-safety memo and the hub's witness/request caches), not from a
        // permit cached in overlay state.
        let should_inject = &should_inject;
        let considered = candidates.len();
        let sweep_started = std::time::Instant::now();
        let predicate_micros = std::sync::atomic::AtomicU64::new(0);
        let predicate_micros = &predicate_micros;
        let mut members = Vec::new();
        for group in candidates.chunks(at_once.max(1)) {
            let applied =
                futures_util::future::join_all(group.iter().cloned().map(|path| async move {
                    // The FRESH shadow-safety decision for THIS generation is recorded BEFORE the
                    // gated inject/retract (never held across it; monotonic, so an older-generation
                    // run cannot regress a newer decision). A concurrent in-flight injection then
                    // observes this generation's decision at ITS commit: a concurrent flip to
                    // `{safe:false}` VETOES the stale commit, while a genuine re-inject after a
                    // flip-back-to-safe (`{safe:true}`) is not spuriously vetoed by the PRIOR
                    // generation's cached-unsafe decision.
                    let started = std::time::Instant::now();
                    let InjectionVerdict(permit) = should_inject(&path).into();
                    predicate_micros.fetch_add(
                        started.elapsed().as_micros() as u64,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    if let Some(permit) = permit {
                        // The single-carrier entry records the `{safe:true}` admission and runs the
                        // gated inject transaction against the bound epoch.
                        self.inject_permitted(serving, &path, generation, permit)
                            .await
                    } else {
                        // No permit — shadow-unsafe or hub-refused: cache `{safe:false}` (so
                        // `is_synced` fails closed the instant it is observed) then retract the
                        // carrier's overlay so it leaves the SHARED Program (its
                        // `ContentRecord` is KEPT so it re-injects if it later flips back to
                        // safe), under the carrier gate so it is ordered w.r.t. any
                        // in-flight inject of the same carrier.
                        self.cache_shadow_decision(&path, run_epoch, generation, false);
                        let gate = self.carrier_gate(&path);
                        self.retract_unsafe_bound(transport, run_epoch, &path, generation, &gate)
                            .await;
                        None
                    }
                }))
                .await;
            members.extend(applied.into_iter().flatten());
        }
        if considered > 0 {
            tracing::debug!(
                considered,
                predicate_ms = predicate_micros.load(std::sync::atomic::Ordering::Relaxed) / 1000,
                total_ms = sweep_started.elapsed().as_millis() as u64,
                "shared overlay: sweep finished"
            );
        }
        SynchronizationReceipt {
            epoch: run_epoch,
            members,
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
    #[cfg(test)]
    pub fn is_synced(&self, path: &str) -> bool {
        let state = self.state.lock();
        let active = state.active_epoch;
        state
            .content
            .get(path)
            .is_some_and(|rec| record_is_synced(rec, active))
    }

    /// Classify synchronization against the EXACT transport epoch the caller intends to
    /// serve from. Reading the global active epoch as a boolean is insufficient: a query
    /// can retain transport A while another query observes replacement B. In that window,
    /// B's marker makes the old `is_synced(path)` true even though returning A would cross
    /// the barrier. This terminal check fails closed on that mismatch.
    pub fn sync_state_for_epoch(
        &self,
        path: &str,
        expected_epoch: ProviderEpoch,
    ) -> OverlaySyncState {
        let state = self.state.lock();
        let Some(rec) = state.content.get(path) else {
            return OverlaySyncState::Unrecorded;
        };
        let Some(active_epoch) = state.active_epoch else {
            return OverlaySyncState::NoActiveTransport;
        };
        if active_epoch != expected_epoch {
            return OverlaySyncState::TransportEpochMismatch {
                expected: expected_epoch,
                active: active_epoch,
            };
        }
        if let Some(shadow) = rec.shadow_safety.as_ref().filter(|shadow| !shadow.safe) {
            return OverlaySyncState::ShadowUnsafe {
                generation: shadow.generation,
            };
        }
        let Some(injected) = rec.injected.as_ref() else {
            return OverlaySyncState::NeverInjected;
        };
        if injected.epoch != expected_epoch {
            return OverlaySyncState::InjectedIntoDifferentEpoch {
                expected: expected_epoch,
                injected: injected.epoch,
            };
        }
        if injected.content.as_ref() != rec.content.as_ref() {
            return OverlaySyncState::ContentDirty;
        }
        OverlaySyncState::Synced
    }

    /// Finish a withdrawal while holding the carrier gate. A committed reopen
    /// subsumes the old ownership; otherwise only successful transport close
    /// removes the pending withdrawal. Cancellation leaves it for the next sweep.
    async fn finish_withdrawal(&self, path: &str, transport: &Arc<T>) {
        let epoch = {
            let mut state = self.state.lock();
            let Some(epoch) = state.withdrawals.get(path).copied() else {
                return;
            };
            if epoch != state.active_epoch {
                // The hub owns teardown of the retired incarnation. Never send
                // its close to an unrelated replacement Program.
                state.withdrawals.remove(path);
                return;
            }
            if state
                .content
                .get(path)
                .is_some_and(|rec| rec.injected.is_some())
            {
                state.withdrawals.remove(path);
                return;
            }
            epoch
        };
        self.retract_tracked(path, transport, epoch).await;
    }

    /// Caller holds the per-path gate through physical completion.
    async fn retract_tracked(&self, path: &str, transport: &Arc<T>, epoch: Option<ProviderEpoch>) {
        {
            let mut state = self.state.lock();
            if state.active_epoch != epoch {
                return;
            }
            state.withdrawals.insert(path.to_string(), epoch);
        }
        if transport.retract(path).await.is_ok() {
            let mut state = self.state.lock();
            if state.withdrawals.get(path) == Some(&epoch) {
                state.withdrawals.remove(path);
            }
        }
    }

    /// Close local desired state immediately, retaining physical ownership until
    /// withdrawal succeeds. The gate and transport share one deadline; later
    /// synchronization retries an unfinished close even outside the demand scope.
    pub async fn retract_bounded(
        &self,
        path: &str,
        timeout: Duration,
        current: impl Fn(Option<ProviderEpoch>) -> Option<Arc<T>>,
    ) {
        self.take_content(path);
        let deadline = tokio::time::Instant::now() + timeout;
        let gate = self.carrier_gate(path);
        let _ = tokio::time::timeout_at(deadline, async {
            let _gate = gate.lock().await;
            let epoch = self.state.lock().withdrawals.get(path).copied().flatten();
            if let Some(transport) = current(epoch) {
                self.finish_withdrawal(path, &transport).await;
            }
        })
        .await;
    }
}

#[cfg(test)]
#[path = "overlay_core_tests.rs"]
mod overlay_core_tests;

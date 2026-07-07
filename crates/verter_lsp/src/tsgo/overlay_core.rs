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
//! Generic over the transport `T` (through the [`OverlayTransport`] seam) so the
//! off-critical-path property is unit-testable with a transport double.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex as SyncMutex;

use verter_type_runtime::traits::{ProviderFuture, TypeProvider};

use crate::tsgo::shared::TsgoSharedProvider;
use crate::tsgo::transport_cell::{LazyTransport, TransportEpoch};

/// The bound on a flip-to-unsafe retract issued from the query-time inject sweep: a
/// slow/dead relay retract cannot stall the sweep. Symmetric with the OWNED-close
/// retract bound, and the whole sweep is additionally under the composite's outer query
/// deadline — so a wedged relay never delays diagnostics past those bounds.
const RETRACT_ON_UNSAFE_TIMEOUT: Duration = Duration::from_secs(2);

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

    /// Drop a carrier's recorded content on close.
    pub(crate) fn remove_content(&self, path: &str) {
        self.state.lock().content.remove(path);
    }

    /// The live transport if already established, else `None` — NEVER establishes
    /// (used by the non-establishing retract / shutdown paths).
    pub(crate) async fn current(&self) -> Option<Arc<T>> {
        self.transport.current().await.map(|e| e.transport)
    }

    /// Observe the established transport's identity: if its epoch differs from the
    /// active one, adopt it AND reset every injection marker — the open carrier set is
    /// no longer synced into the (now dead / replaced) prior transport and must replay
    /// into the fresh one. The shadow-safety caches are LEFT intact: they are keyed on
    /// the workspace content generation, orthogonal to the transport epoch.
    fn observe_transport_identity(&self, epoch: TransportEpoch) {
        let mut state = self.state.lock();
        if state.active_epoch != Some(epoch) {
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
    /// the injection markers so the open set replays.
    pub(crate) async fn ensure<B, G, Ef, Fut>(
        &self,
        bound: Option<(B, u64)>,
        probe_generation: G,
        establish: Ef,
        timeout: Duration,
    ) -> Option<Arc<T>>
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
        Some(established.transport)
    }

    /// Inject the carrier's recorded content into the SHARED `transport` IF it is not
    /// already synced for the ACTIVE transport epoch (dirty = content changed since the
    /// last committed injection, OR the transport was re-established since) — the
    /// query-time injection off the OWNED lifecycle path. Best-effort: on success
    /// records the injected content + epoch so an unchanged subsequent query does not
    /// re-inject (no Program spam); on failure the content stays dirty and a later query
    /// retries (self-healing, fail-closed).
    ///
    /// `shadow_safety_generation` is the generation the CALLING sweep observed (the caller
    /// threads its own value through). The commit's shadow-safety veto trusts the cached
    /// decision only when this is `Some`: a `None` sweep never WROTE the cache, so it must
    /// not be vetoed by a stale `{safe:false}` a prior generation left behind.
    pub(crate) async fn inject_dirty(
        &self,
        transport: &Arc<T>,
        path: &str,
        shadow_safety_generation: Option<u64>,
    ) {
        // Read the dirty content + capture the run EPOCH under a brief sync lock — never
        // held across the inject await. Dirty = not synced for the active epoch.
        let (content, run_epoch) = {
            let state = self.state.lock();
            let Some(epoch) = state.active_epoch else {
                // No established transport identity observed — nothing to sync against.
                return;
            };
            let Some(rec) = state.content.get(path) else {
                return;
            };
            if record_is_synced(rec, Some(epoch)) {
                return;
            }
            (Arc::clone(&rec.content), epoch)
        };
        if transport.inject(path, &content).await.is_ok() {
            // Commit the marker ONLY if THREE conditions all still hold at commit time:
            //   1. the transport epoch is STILL the one this injection ran under (a
            //      reconnect mid-flight advanced it — a stale commit must NOT mark the new
            //      epoch synced);
            //   2. the recorded content has not changed AGAIN meanwhile (else the newer
            //      content stays dirty for the next query);
            //   3. the carrier has NOT become cached-UNSAFE while this inject was in flight
            //      — a concurrent flip-to-unsafe retracted the overlay and cached
            //      `{safe:false}`; restoring the synced marker would shadow the now-real
            //      user file (`carrier_never_shadows_real_user_file`). Epoch + content
            //      still-current is necessary but not sufficient; the shadow-safety veto is
            //      the third gate. A carrier with no cache (a content-dirty path) or a
            //      cached-SAFE decision commits normally — never over-vetoed.
            // The veto is trusted only for a sweep that observed a generation: a `None`
            // sweep never WROTE the cache, so a stale `{safe:false}` a prior generation
            // left behind must not veto it. (Unreachable in production — the composite
            // always threads `Some(content_generation)` — a defensive guard for the
            // `None` test path.)
            let mut state = self.state.lock();
            if state.active_epoch == Some(run_epoch) {
                if let Some(rec) = state.content.get_mut(path) {
                    let shadow_safe = shadow_safety_generation.is_none()
                        || rec.shadow_safety.as_ref().is_none_or(|c| c.safe);
                    if shadow_safe && rec.content.as_ref() == content.as_ref() {
                        rec.injected = Some(InjectedRecord {
                            content,
                            epoch: run_epoch,
                        });
                        // The `None`-sweep path (a defensive guard — the composite always
                        // threads `Some(content_generation)`) just committed the marker over
                        // a stale `{safe:false}` it could not validate: drop that cached
                        // decision (a `None` generation never trusts the cache) so the freshly
                        // re-injected safe carrier is not left reporting not-synced by the
                        // shadow-safety consult in `record_is_synced`. This maintains the
                        // steady-state invariant that a set injected marker never coexists with
                        // a cached `{safe:false}` — the invariant the served gate relies on
                        // outside the transient flip-to-unsafe retract window (where the sweep
                        // has cached `{safe:false}` but not yet cleared the marker).
                        if shadow_safety_generation.is_none() {
                            rec.shadow_safety = None;
                        }
                    }
                }
            }
        }
    }

    /// Inject EVERY recorded carrier whose content is dirty for the active transport
    /// epoch AND that `should_inject` admits — the query-time COMPLETENESS step. The
    /// queried carrier's diagnostics need its companion family (`.vue.tsx` + its
    /// `.vue.verter.ts` script) and any other already-open carrier it imports to be
    /// members of the SHARED Program, not just the single queried carrier; injecting the
    /// whole recorded open set at query time keeps the normal open→diagnostics flow
    /// correct (an unrelated carrier is a harmless extra open document).
    ///
    /// `should_inject` is the caller's shadow/conflict gate: a recorded path that is NOT
    /// a genuine generated carrier surface — e.g. a real user file occupying a
    /// carrier-companion path — is SKIPPED, never overlay-shadowed
    /// (`carrier_never_shadows_real_user_file`).
    ///
    /// Generation-cached shadow-safety with a dirty-first fast skip: when the
    /// `shadow_safety_generation` matches the cached one the shadow-safety decision cannot
    /// have changed, so the disk-probing `should_inject` predicate is skipped unless the
    /// carrier still needs work. A cached-SAFE carrier is re-checked only when its content
    /// is dirty (a fresh edit or a post-reconnect epoch replay). A cached-UNSAFE carrier
    /// stays cleared and is NOT re-injected even when content-dirty (an unsafe carrier
    /// never re-injects, and cannot flip back to safe without a generation advance). So a
    /// content-dirty carrier forces a re-check ONLY when it is not generation-cached
    /// unsafe — a content-clean, generation-clean, cached-safe carrier skips the predicate
    /// entirely (the warm-path optimization). ANY advance of `shadow_safety_generation`
    /// (the caller's workspace content generation — bumped by any file-set/overlay
    /// transition) re-evaluates every carrier, so a real user file appearing at a companion
    /// path is never missed (`carrier_never_shadows_real_user_file`). A `None` generation
    /// is never trusted as a cache key (always re-evaluated, fail-safe).
    ///
    /// A carrier that flips to UNSAFE and was PREVIOUSLY injected issues a BOUNDED
    /// best-effort `transport.retract` so its stale overlay leaves the SHARED Program (its
    /// local marker cleared afterwards and its `ContentRecord` KEPT so it re-injects on a
    /// flip back to safe). The SERVED invariant does NOT depend on that retract completing:
    /// the sweep caches `{safe:false}` BEFORE the retract, and `is_synced` consults
    /// shadow-safety, so a cached-UNSAFE carrier is never reported synced — even while its
    /// injected marker is transiently still set during the retract await — and the
    /// `inject_dirty` commit veto refuses to restore that marker; together they keep an
    /// unsafe carrier from ever being served SHARED even if the Program transiently still
    /// contains the stale overlay. A bounded or timed-out retract may therefore leave that
    /// overlay open in the SHARED Program as a MEMBERSHIP RESIDUAL until the transport's own
    /// ordered close path completes, the session/transport is drained, or the transport is
    /// replaced — guaranteed Program removal is owned by the shared transport close
    /// lifecycle, not by overlay state.
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
        transport: &Arc<T>,
        shadow_safety_generation: Option<u64>,
        should_inject: F,
    ) where
        F: Fn(&str) -> bool,
    {
        // Snapshot the candidate carriers under a brief lock — never held across the
        // predicate's disk probe or the inject/retract await. `prev_injected` is captured
        // so a flip-to-unsafe of a previously-injected carrier can be RETRACTED.
        //
        // When the shadow-safety generation is CLEAN (a generation is observable AND
        // matches the cached one), the shadow-safety decision cannot have changed, so the
        // disk-probing predicate is skipped: a cached-SAFE carrier is a candidate only if
        // its content is dirty (a fresh edit OR a post-reconnect epoch replay — it must
        // re-inject, and being content-dirty it still re-checks safety); a cached-UNSAFE
        // carrier stays cleared (it never re-injects and cannot flip back to safe without a
        // generation advance). When the generation is STALE or unobservable, every carrier
        // is a candidate (re-evaluate) — so any file-set transition that could flip
        // shadow-safety is never missed (`carrier_never_shadows_real_user_file`).
        struct Candidate {
            path: String,
            prev_injected: bool,
        }
        let candidates: Vec<Candidate> = {
            let state = self.state.lock();
            let active = state.active_epoch;
            state
                .content
                .iter()
                .filter_map(|(path, rec)| {
                    let content_dirty = !record_is_synced(rec, active);
                    let (shadow_fresh, cached_safe) =
                        match (shadow_safety_generation, rec.shadow_safety.as_ref()) {
                            // A generation observable AND matching the cache ⇒ the
                            // shadow-safety decision is generation-clean.
                            (Some(g), Some(cache)) if cache.generation == g => (true, cache.safe),
                            // No generation observable, no cache, or a generation advance
                            // ⇒ stale (re-evaluate).
                            _ => (false, false),
                        };
                    let candidate = if shadow_fresh {
                        // Generation-clean: skip the predicate. Re-inject a cached-SAFE
                        // carrier only when content-dirty; leave a cached-UNSAFE carrier
                        // cleared.
                        cached_safe && content_dirty
                    } else {
                        // Stale / no generation ⇒ re-evaluate shadow-safety.
                        true
                    };
                    candidate.then(|| Candidate {
                        path: path.clone(),
                        prev_injected: rec.injected.is_some(),
                    })
                })
                .collect()
        };
        for cand in candidates {
            let safe = should_inject(&cand.path);
            // Record the FRESH shadow-safety decision for THIS generation BEFORE the
            // inject/retract await (never held across it; monotonic, so an older-generation
            // run cannot regress a newer decision). A concurrent in-flight `inject_dirty`
            // then observes this generation's decision at ITS commit: a concurrent flip to
            // `{safe:false}` VETOES the stale commit, while a genuine re-inject after a
            // flip-back-to-safe (`{safe:true}`) is not spuriously vetoed by the PRIOR
            // generation's cached-unsafe decision (the cache reflects the current decision
            // by the time the commit reads it).
            if let Some(generation) = shadow_safety_generation {
                let mut state = self.state.lock();
                if let Some(rec) = state.content.get_mut(&cand.path) {
                    let regress = rec
                        .shadow_safety
                        .as_ref()
                        .is_some_and(|c| generation < c.generation);
                    if !regress {
                        rec.shadow_safety = Some(ShadowSafetyCache { generation, safe });
                    }
                }
            }
            if safe {
                self.inject_dirty(transport, &cand.path, shadow_safety_generation)
                    .await;
            } else if cand.prev_injected {
                // Flip-to-unsafe of a previously-injected carrier: it is (or may still be)
                // an open document in the SHARED Program shadowing the now-real user file —
                // issue a BOUNDED best-effort retract so it leaves the Program. The
                // `ContentRecord` is KEPT (below) so it re-injects if it later flips back to
                // safe. The served invariant does NOT depend on this retract completing: the
                // `{safe:false}` cached above (BEFORE this await) makes `is_synced` fail
                // closed — it consults shadow-safety, so a cached-UNSAFE carrier is never
                // reported synced even while its injected marker is transiently still set —
                // and the `inject_dirty` commit veto refuses to restore that marker; together
                // they keep an unsafe carrier from ever being served SHARED. A bounded/
                // timed-out retract may leave a membership residual until the transport's own
                // ordered close lifecycle removes it.
                //
                // Physical-Program cleanliness of this retract against a concurrent inject of
                // the same carrier is the transport's ordered per-carrier gate (a pre-existing
                // invariant): the overlay marker is the local mechanism, program ordering is
                // the transport's.
                let _ =
                    tokio::time::timeout(RETRACT_ON_UNSAFE_TIMEOUT, transport.retract(&cand.path))
                        .await;
            }
            // Re-lock briefly to commit: for an unsafe carrier ensure the local marker is
            // cleared (never leave a stale unsafe overlay looking synced).
            if !safe {
                let mut state = self.state.lock();
                if let Some(rec) = state.content.get_mut(&cand.path) {
                    rec.injected = None;
                }
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
    /// BOUNDED and fail-closed. Drops the recorded content (a sync insert) and, if the
    /// transport is already established, issues the retract under `timeout` so a
    /// slow/dead relay cannot hang or delay the OWNED `close_file` path (a broken
    /// transport is torn down / evicted anyway). NEVER establishes the transport (a
    /// close must not trigger — or head-of-line-block on — an establishment). The
    /// retract itself routes through the transport's ordered per-carrier gate, so it is
    /// correctly ordered w.r.t. any in-flight injection.
    pub(crate) async fn retract_bounded(&self, path: &str, timeout: Duration) {
        self.remove_content(path);
        if let Some(transport) = self.current().await {
            let _ = tokio::time::timeout(timeout, transport.retract(path)).await;
        }
    }
}

#[cfg(test)]
#[path = "overlay_core_tests.rs"]
mod overlay_core_tests;

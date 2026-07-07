//! A lazily-established, singleflight, bounded, re-arming transport cell.
//!
//! [`LazyTransport`] owns the concurrency discipline for a transport that is
//! established ONCE on first demand and reused after — decoupled from the concrete
//! transport type so the state machine is unit-testable with a fake establisher:
//!
//! - **Drop-lock singleflight.** The STATE lock is held ONLY for brief state
//!   reads/writes, NEVER across the establishment I/O; the single in-flight
//!   establishment runs under a separate establish-lock while the state lock is
//!   dropped, so a slow/broken establishment cannot head-of-line-block state reads
//!   (`current`) or the other lifecycle calls. Concurrent demands block cooperatively
//!   on the establish-lock and reuse the ONE established transport — never launch N
//!   establishments.
//! - **Bounded.** Establishment runs under a caller-supplied timeout; on elapse it
//!   yields NO transport (the caller fails closed to its baseline) rather than
//!   stalling forever.
//! - **Re-arming.** A failed establishment marks the cell unavailable AT the observed
//!   generation discriminant. A subsequent demand re-attempts ONLY when the observed
//!   generation ADVANCES (a fresh advertisement / editor generation) — never on every
//!   query within the same failed generation (which would be a handshake retry-storm).
//! - **Live-death eviction.** A `Live` transport that has DIED (the caller's
//!   `is_alive` predicate reports dead — e.g. the shim emitted `verter/fatal` or the
//!   control/`--api` connection closed) is handled on the next demand by comparing the
//!   CURRENT generation against the dead transport's establishment generation. A
//!   same-generation death is EVICTED to a re-armable `Unavailable` and the query fails
//!   CLOSED to the baseline (no stall, no dead-path re-hit, no re-establishment storm) —
//!   a still-dead shim stays fail-closed. When the generation has ALREADY ADVANCED (a
//!   reconnect republished a fresh advertisement before eviction observed the death),
//!   the dead live transitions STRAIGHT toward a fresh establishment attempt instead of
//!   stamping the fresh generation failed — so a reconnect re-establishes in the SAME
//!   demand rather than waiting for a FURTHER advance.
//! - **Transport identity/epoch.** Every successful establishment mints a monotonic
//!   [`TransportEpoch`] on the cell (advanced ONLY on the success commit). Callers
//!   receive the transport together with its [`TransportIdentity`] as an
//!   [`EstablishedTransport`], so a consumer can observe when the transport CHANGED
//!   identity (a reconnect) — the signal the overlay uses to replay its open document
//!   set into the fresh transport.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

/// A monotonic transport-identity epoch, minted ONLY when a transport is successfully
/// committed to `Live`. A consumer observes it to detect a transport RE-establishment
/// (a reconnect mints a new epoch) and react — e.g. replay an open document set into
/// the fresh transport.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) struct TransportEpoch(u64);

/// The identity of a successfully-established transport: its monotonic epoch plus the
/// generation discriminant it established at (the base a later dead-live eviction
/// compares the CURRENT generation against — a fresh generation means a reconnect
/// already exists, so the dead-live must not poison it).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TransportIdentity {
    pub(crate) epoch: TransportEpoch,
    pub(crate) establishment_generation: Option<String>,
}

/// A live transport handed out WITH its identity, so the caller can observe the epoch
/// (and thereby detect a reconnect).
pub(crate) struct EstablishedTransport<T> {
    pub(crate) transport: Arc<T>,
    pub(crate) identity: TransportIdentity,
}

/// The lazily-established transport state.
enum CellState<T> {
    /// Never attempted — the first demand establishes it.
    Pending,
    /// An establishment is in flight (a transient marker; waiters block on the
    /// establish-lock, not on this state).
    Connecting,
    /// Established and live — reused by every demand. Carries the transport's minted
    /// [`TransportIdentity`] (its epoch + the generation it established at).
    Live {
        transport: Arc<T>,
        identity: TransportIdentity,
    },
    /// Establishment failed at this generation discriminant. Re-arms only when the
    /// observed generation ADVANCES past `failed_generation`.
    Unavailable {
        /// The generation discriminant establishment last failed at (`None` when no
        /// generation was observable at failure time).
        failed_generation: Option<String>,
    },
}

/// The cell's locked inner state: the state machine plus the monotonic epoch counter.
/// `next_epoch` advances ONLY on a successful establishment commit — never on
/// `Pending` / `Connecting` / `Unavailable` / a dead-live eviction / a failure / a
/// timeout.
struct CellInner<T> {
    state: CellState<T>,
    next_epoch: u64,
}

/// The state-machine branch a demand takes after inspecting the cell — computed while
/// borrowing the state, then acted on after the borrow is released (so the state can be
/// mutated). A LIVE transport is returned inline before this is produced.
enum Demand {
    /// A dead `Live` whose stored establishment generation is this — the eviction
    /// compares it against the CURRENT generation.
    DeadLive(Option<String>),
    /// An `Unavailable` cell that failed at this generation — re-arm only on an advance.
    Unavailable(Option<String>),
    /// `Pending` / `Connecting` — fall through to establish.
    FallThrough,
}

/// A lazily-established transport `T`, handed out behind an [`Arc`] with its identity.
pub struct LazyTransport<T> {
    /// The cell inner state — locked ONLY for brief reads/writes, never across the
    /// establishment I/O.
    inner: Mutex<CellInner<T>>,
    /// The singleflight establish-lock — held for the duration of the ONE in-flight
    /// establishment so concurrent demands wait for it (cooperatively) instead of
    /// launching their own.
    establish_lock: Mutex<()>,
}

impl<T> Default for LazyTransport<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> LazyTransport<T> {
    /// An un-established cell.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(CellInner {
                state: CellState::Pending,
                next_epoch: 1,
            }),
            establish_lock: Mutex::new(()),
        }
    }

    /// The live transport (with its identity) if already established, else `None` —
    /// NEVER establishes. Used by the non-establishing lifecycle calls (retract /
    /// shutdown), which must not head-of-line-block on an establishment.
    pub(crate) async fn current(&self) -> Option<EstablishedTransport<T>> {
        match &self.inner.lock().await.state {
            CellState::Live {
                transport,
                identity,
            } => Some(EstablishedTransport {
                transport: Arc::clone(transport),
                identity: identity.clone(),
            }),
            _ => None,
        }
    }

    /// Get the live transport (with its identity), or establish it once — singleflight,
    /// bounded by `timeout`, re-arming on a generation advance.
    ///
    /// - Live ⇒ return it (no establishment).
    /// - Unavailable AND the observed generation has NOT advanced past the failed one
    ///   ⇒ `None` (fail closed; no retry within the same failed generation).
    /// - Otherwise acquire the establish-lock (concurrent demands WAIT here, never
    ///   launch N establishments), re-check, then run `establish` with the STATE LOCK
    ///   DROPPED under `timeout`. Success ⇒ Live (a fresh epoch minted) and returned;
    ///   failure or timeout ⇒ Unavailable at the current generation, `None`.
    ///
    /// `probe_generation` returns the CURRENT generation discriminant (e.g. the shim
    /// advertisement nonce), or `None` when none is observable.
    pub(crate) async fn get_or_establish<G, Lf, Ef, Fut>(
        &self,
        probe_generation: G,
        is_alive: Lf,
        establish: Ef,
        timeout: Duration,
    ) -> Option<EstablishedTransport<T>>
    where
        G: Fn() -> Option<String>,
        Lf: Fn(&T) -> bool,
        Ef: FnOnce() -> Fut,
        Fut: Future<Output = Option<Arc<T>>>,
    {
        // Fast path — a LIVE transport returns immediately, under the state lock and
        // WITHOUT probing the generation (the warm hot path never touches the FS). Every
        // other disposition releases the state lock BEFORE any generation probe, so the FS
        // advertisement read never head-of-line-blocks a concurrent state read (`current()`
        // / the lifecycle calls). A Live transport that has DIED (relay `verter/fatal` / a
        // closed connection) is evicted by the establish-lock section below, which already
        // probes the generation OFF the state lock before deciding evict-vs-rearm.
        {
            let demand = {
                let inner = self.inner.lock().await;
                match &inner.state {
                    CellState::Live {
                        transport,
                        identity,
                    } => {
                        if is_alive(transport) {
                            return Some(EstablishedTransport {
                                transport: Arc::clone(transport),
                                identity: identity.clone(),
                            });
                        }
                        // Dead Live: defer to the establish-lock eviction (which probes
                        // off-lock) rather than probing under this state lock.
                        Demand::FallThrough
                    }
                    CellState::Unavailable { failed_generation } => {
                        Demand::Unavailable(failed_generation.clone())
                    }
                    CellState::Pending | CellState::Connecting => Demand::FallThrough,
                }
            };
            // The state lock is released; probe the generation OFF it. An `Unavailable`
            // cell re-arms ONLY on a generation advance — a persistently-failed generation
            // fails closed here without acquiring the establish-lock (no retry-storm) and
            // without holding the state lock across the advertisement read. FallThrough (a
            // dead Live / Pending / Connecting) proceeds to the establish-lock section.
            if let Demand::Unavailable(failed_generation) = demand {
                if !generation_advanced(&failed_generation, &probe_generation()) {
                    return None;
                }
                // Re-arm: fall through to establish under a fresh generation.
            }
        }

        // Singleflight: only ONE establishment runs at a time; concurrent demands block
        // here cooperatively (never busy-spin) and re-check below.
        let _establish_guard = self.establish_lock.lock().await;

        // Re-check after acquiring the establish-lock: another establisher may have
        // just finished (Live), a Live transport may have died (evict + fail closed),
        // or an establishment may have failed at the current generation (Unavailable).
        let current_gen = probe_generation();
        {
            let mut inner = self.inner.lock().await;
            let demand = match &inner.state {
                CellState::Live {
                    transport,
                    identity,
                } => {
                    if is_alive(transport) {
                        return Some(EstablishedTransport {
                            transport: Arc::clone(transport),
                            identity: identity.clone(),
                        });
                    }
                    Demand::DeadLive(identity.establishment_generation.clone())
                }
                CellState::Unavailable { failed_generation } => {
                    Demand::Unavailable(failed_generation.clone())
                }
                CellState::Pending | CellState::Connecting => Demand::FallThrough,
            };
            match demand {
                Demand::FallThrough => {}
                Demand::DeadLive(est_gen) => {
                    // Same dead-live decision as the fast path, using the generation probed
                    // once after the establish-lock: an already-advanced generation
                    // transitions toward establishment; a same-generation death fails
                    // closed.
                    if generation_advanced(&est_gen, &current_gen) {
                        inner.state = CellState::Pending;
                    } else {
                        inner.state = CellState::Unavailable {
                            failed_generation: current_gen.clone(),
                        };
                        return None;
                    }
                }
                Demand::Unavailable(failed_generation) => {
                    if !generation_advanced(&failed_generation, &current_gen) {
                        return None;
                    }
                }
            }
        }

        // Mark Connecting (state lock dropped again before the establishment I/O).
        {
            let mut inner = self.inner.lock().await;
            inner.state = CellState::Connecting;
        }

        // Establish with the STATE LOCK DROPPED, bounded by `timeout`. On elapse the
        // establishment future is dropped (cancelled) and the cell fails closed.
        let established = match tokio::time::timeout(timeout, establish()).await {
            Ok(Some(t)) => Some(t),
            // A failed establishment (`Ok(None)`) or an elapsed bound (`Err`) both fail
            // closed — no transport, the cell goes Unavailable at this generation.
            Ok(None) | Err(_) => None,
        };

        // Commit the outcome.
        match established {
            Some(t) => {
                // Re-probe the generation AFTER a successful establishment, with the state
                // lock still DROPPED: production re-reads the advertisement internally
                // during the handshake, so the generation the transport actually connected
                // at is the one observable NOW, not the pre-establishment sample. The
                // identity must carry THIS generation so a later dead-live eviction compares
                // the current generation against the exact one the live transport
                // established at (an advertisement that advanced mid-handshake is reflected,
                // not stale).
                let established_generation = probe_generation();
                let mut inner = self.inner.lock().await;
                // Mint the identity — the ONLY place the epoch advances.
                let identity = TransportIdentity {
                    epoch: TransportEpoch(inner.next_epoch),
                    establishment_generation: established_generation,
                };
                inner.next_epoch += 1;
                inner.state = CellState::Live {
                    transport: Arc::clone(&t),
                    identity: identity.clone(),
                };
                Some(EstablishedTransport {
                    transport: t,
                    identity,
                })
            }
            None => {
                // A failed establishment re-arms at the PRE-establishment generation
                // (`current_gen`): a generation that advanced during the failed attempt
                // still reads as an advance for the next demand's re-arm, so recovery does
                // not wait for a further advance.
                let mut inner = self.inner.lock().await;
                inner.state = CellState::Unavailable {
                    failed_generation: current_gen,
                };
                None
            }
        }
    }

    /// Establish the transport ONLY when a per-query binding resolved — the
    /// transport-cell-poisoning guard.
    ///
    /// `bound` is the carrier's PRE-RESOLVED project binding + the config
    /// generation it resolved at, resolved BEFORE any cell interaction. A `None`
    /// binding (no project / `Ambiguous` / `SyntheticScratch`, or a not-yet-ready
    /// published snapshot) serves the baseline (`None`) WITHOUT entering or mutating
    /// the cell — so a carrier's transient non-binding NEVER records `Unavailable`
    /// on the carrier-INDEPENDENT transport (which attaches to the shim, not to any
    /// one carrier). Only a resolved binding enters the singleflight establishment
    /// ([`Self::get_or_establish`]), so `Unavailable` is recorded ONLY after an
    /// actual attach attempt (`establish`) fails or times out.
    ///
    /// `probe_generation` composes the re-arm discriminant from the carrier's config
    /// `generation` (and, in production, the shim advertisement nonce), so a failed
    /// attach re-arms on EITHER a fresh advertisement/editor generation OR a fresh
    /// published snapshot generation — never on every query within the same
    /// (nonce, generation).
    pub(crate) async fn get_or_establish_bound<B, G, Lf, Ef, Fut>(
        &self,
        bound: Option<(B, u64)>,
        probe_generation: G,
        is_alive: Lf,
        establish: Ef,
        timeout: Duration,
    ) -> Option<EstablishedTransport<T>>
    where
        G: Fn(u64) -> Option<String>,
        Lf: Fn(&T) -> bool,
        Ef: FnOnce(B, u64) -> Fut,
        Fut: Future<Output = Option<Arc<T>>>,
    {
        // THE GATE: resolve the binding BEFORE any cell interaction. A `None`
        // binding never enters the cell — the carrier-independent transport is
        // never poisoned by a carrier's transient non-binding, and `Unavailable` is
        // recorded ONLY after an actual `establish` attach attempt below.
        let (binding, generation) = bound?;
        self.get_or_establish(
            || probe_generation(generation),
            is_alive,
            || establish(binding, generation),
            timeout,
        )
        .await
    }
}

/// Whether the observed generation ADVANCED past the failed one — the re-arm signal.
///
/// Re-arm ONLY when the current generation is `Some(new)` that differs from the failed
/// discriminant. A missing current generation (`None` — no advertisement observable)
/// does NOT re-arm, so a flapping / absent advertisement never storms establishment.
fn generation_advanced(failed: &Option<String>, current: &Option<String>) -> bool {
    match current {
        Some(now) => Some(now) != failed.as_ref(),
        None => false,
    }
}

#[cfg(test)]
#[path = "transport_cell_tests.rs"]
mod transport_cell_tests;

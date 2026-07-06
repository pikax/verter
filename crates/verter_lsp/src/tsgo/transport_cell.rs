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
//!   control/`--api` connection closed) is EVICTED on the next demand to a re-armable
//!   `Unavailable` at the current generation, and the query fails CLOSED to the
//!   baseline (no stall, no dead-path re-hit). Re-establishment then follows the SAME
//!   generation/nonce re-arm discriminant on a SUBSEQUENT demand — a still-dead shim
//!   stays fail-closed, a reconnect (advanced discriminant) re-establishes.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

/// The lazily-established transport state.
enum CellState<T> {
    /// Never attempted — the first demand establishes it.
    Pending,
    /// An establishment is in flight (a transient marker; waiters block on the
    /// establish-lock, not on this state).
    Connecting,
    /// Established and live — reused by every demand.
    Live(Arc<T>),
    /// Establishment failed at this generation discriminant. Re-arms only when the
    /// observed generation ADVANCES past `failed_generation`.
    Unavailable {
        /// The generation discriminant establishment last failed at (`None` when no
        /// generation was observable at failure time).
        failed_generation: Option<String>,
    },
}

/// A lazily-established transport `T`, handed out behind an [`Arc`].
pub struct LazyTransport<T> {
    /// The cell state — locked ONLY for brief reads/writes, never across the
    /// establishment I/O.
    state: Mutex<CellState<T>>,
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
            state: Mutex::new(CellState::Pending),
            establish_lock: Mutex::new(()),
        }
    }

    /// The live transport if already established, else `None` — NEVER establishes.
    /// Used by the non-establishing lifecycle calls (retract / shutdown), which must
    /// not head-of-line-block on an establishment.
    pub async fn current(&self) -> Option<Arc<T>> {
        match &*self.state.lock().await {
            CellState::Live(t) => Some(Arc::clone(t)),
            _ => None,
        }
    }

    /// Get the live transport, or establish it once — singleflight, bounded by
    /// `timeout`, re-arming on a generation advance.
    ///
    /// - Live ⇒ return it (no establishment).
    /// - Unavailable AND the observed generation has NOT advanced past the failed one
    ///   ⇒ `None` (fail closed; no retry within the same failed generation).
    /// - Otherwise acquire the establish-lock (concurrent demands WAIT here, never
    ///   launch N establishments), re-check, then run `establish` with the STATE LOCK
    ///   DROPPED under `timeout`. Success ⇒ Live and returned; failure or timeout ⇒
    ///   Unavailable at the current generation, `None`.
    ///
    /// `probe_generation` returns the CURRENT generation discriminant (e.g. the shim
    /// advertisement nonce), or `None` when none is observable.
    pub async fn get_or_establish<G, Lf, Ef, Fut>(
        &self,
        probe_generation: G,
        is_alive: Lf,
        establish: Ef,
        timeout: Duration,
    ) -> Option<Arc<T>>
    where
        G: Fn() -> Option<String>,
        Lf: Fn(&T) -> bool,
        Ef: FnOnce() -> Fut,
        Fut: Future<Output = Option<Arc<T>>>,
    {
        // Fast path — a LIVE transport returns immediately; a Live transport that has
        // DIED (relay `verter/fatal` / a closed connection) is EVICTED here to a
        // re-armable `Unavailable` and the query fails CLOSED to OWNED (no stall). The
        // STATE lock is held only for this brief section.
        {
            let mut state = self.state.lock().await;
            match &*state {
                CellState::Live(t) => {
                    if is_alive(t) {
                        return Some(Arc::clone(t));
                    }
                    // Evict the dead Live to `Unavailable`, stamping the CURRENT generation
                    // as failed. Re-establishment follows the SAME generation/nonce re-arm
                    // discriminant on a SUBSEQUENT query: a still-dead shim (unchanged
                    // discriminant) stays fail-closed (no re-establishment storm), while a
                    // reconnect (advanced discriminant) re-establishes. NARROW residual
                    // (tracked as ROW F3): if the generation ALREADY advanced (a fresh
                    // advertisement) before this eviction observed it, stamping that fresh
                    // generation as `failed_generation` — without having attempted
                    // establishment at it — makes recovery wait for a FURTHER advance
                    // (bounded, fail-closed to OWNED meanwhile). Not stamping an already
                    // fresh generation as failed before an establishment attempt is deferred
                    // (tracked as ROW F3 in `docs/arch/external-ts-engine-architecture.md`).
                    *state = CellState::Unavailable {
                        failed_generation: probe_generation(),
                    };
                    return None;
                }
                CellState::Unavailable { failed_generation } => {
                    if !generation_advanced(failed_generation, &probe_generation()) {
                        return None;
                    }
                    // Re-arm: fall through to establish under a fresh generation.
                }
                CellState::Pending | CellState::Connecting => {}
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
            let mut state = self.state.lock().await;
            match &*state {
                CellState::Live(t) => {
                    if is_alive(t) {
                        return Some(Arc::clone(t));
                    }
                    *state = CellState::Unavailable {
                        failed_generation: current_gen.clone(),
                    };
                    return None;
                }
                CellState::Unavailable { failed_generation } => {
                    if !generation_advanced(failed_generation, &current_gen) {
                        return None;
                    }
                }
                CellState::Pending | CellState::Connecting => {}
            }
        }

        // Mark Connecting (state lock dropped again before the establishment I/O).
        {
            let mut state = self.state.lock().await;
            *state = CellState::Connecting;
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
        let mut state = self.state.lock().await;
        match &established {
            Some(t) => *state = CellState::Live(Arc::clone(t)),
            None => {
                *state = CellState::Unavailable {
                    failed_generation: current_gen,
                }
            }
        }
        established
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
    pub async fn get_or_establish_bound<B, G, Lf, Ef, Fut>(
        &self,
        bound: Option<(B, u64)>,
        probe_generation: G,
        is_alive: Lf,
        establish: Ef,
        timeout: Duration,
    ) -> Option<Arc<T>>
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

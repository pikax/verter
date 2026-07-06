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
//! Generic over the transport `T` (through the [`OverlayTransport`] seam) so the
//! off-critical-path property is unit-testable with a transport double.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex as SyncMutex;

use verter_type_runtime::traits::{ProviderFuture, TypeProvider};

use crate::tsgo::shared::TsgoSharedProvider;
use crate::tsgo::transport_cell::LazyTransport;

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

/// The recorded content for one carrier companion: the LATEST content the lifecycle
/// fed, plus the last content a query-time injection SUCCESSFULLY committed (so an
/// unchanged carrier is not re-injected on every diagnostics query).
struct ContentRecord {
    /// The latest content the lifecycle recorded.
    content: Arc<str>,
    /// The last content a query-time injection committed, or `None` if never injected.
    injected: Option<Arc<str>>,
}

/// The lazy SHARED-overlay core: a per-carrier content cache (recorded by the OWNED
/// lifecycle, off the establishment path) plus the lazily-established transport cell
/// (established + injected at query time). Generic over the transport `T`.
pub(crate) struct LazyOverlayCore<T: OverlayTransport> {
    /// The latest content the lifecycle recorded per carrier companion path — a plain
    /// sync map the OWNED lifecycle writes WITHOUT touching the transport cell.
    content: SyncMutex<HashMap<String, ContentRecord>>,
    /// The lazily-established, singleflight, bounded, re-arming, liveness-evicting
    /// transport cell — established + injected ONLY at query time.
    transport: LazyTransport<T>,
}

impl<T: OverlayTransport> LazyOverlayCore<T> {
    pub(crate) fn new() -> Self {
        Self {
            content: SyncMutex::new(HashMap::new()),
            transport: LazyTransport::new(),
        }
    }

    /// Record a carrier's current content OFF the establishment critical path — a
    /// plain sync insert the OWNED lifecycle calls. NEVER establishes the transport;
    /// the query path injects the recorded content lazily.
    pub(crate) fn record_content(&self, path: &str, content: &str) {
        let mut map = self.content.lock();
        match map.get_mut(path) {
            Some(rec) => rec.content = Arc::from(content),
            None => {
                map.insert(
                    path.to_string(),
                    ContentRecord {
                        content: Arc::from(content),
                        injected: None,
                    },
                );
            }
        }
    }

    /// Drop a carrier's recorded content on close.
    pub(crate) fn remove_content(&self, path: &str) {
        self.content.lock().remove(path);
    }

    /// The live transport if already established, else `None` — NEVER establishes
    /// (used by the non-establishing retract / shutdown paths).
    pub(crate) async fn current(&self) -> Option<Arc<T>> {
        self.transport.current().await
    }

    /// Establish the transport for a bound carrier — bounded, singleflight, and
    /// LIVENESS-evicting (a dead `Live` transport is evicted and re-established per the
    /// generation/nonce re-arm discriminant). Reached ONLY from the query path, never
    /// the OWNED lifecycle. A `None` binding serves the baseline WITHOUT entering the
    /// cell (the transport-cell-poisoning gate).
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
        self.transport
            .get_or_establish_bound(bound, probe_generation, |t| t.is_live(), establish, timeout)
            .await
    }

    /// Inject the carrier's recorded content into the SHARED `transport` IF it changed
    /// since the last successful injection (dirty) — the query-time injection off the
    /// OWNED lifecycle path. Best-effort: on success records the injected content so an
    /// unchanged subsequent query does not re-inject (no Program spam); on failure the
    /// content stays dirty and a later query retries (self-healing, fail-closed).
    pub(crate) async fn inject_dirty(&self, transport: &Arc<T>, path: &str) {
        // Read the dirty content (differs from the last-injected) under a brief sync
        // lock — never held across the inject await.
        let dirty = {
            let map = self.content.lock();
            map.get(path).and_then(|rec| {
                (rec.injected.as_deref() != Some(rec.content.as_ref()))
                    .then(|| Arc::clone(&rec.content))
            })
        };
        let Some(content) = dirty else {
            return;
        };
        if transport.inject(path, &content).await.is_ok() {
            // Mark injected only if the recorded content has not changed AGAIN meanwhile
            // (else the newer content stays dirty for the next query).
            let mut map = self.content.lock();
            if let Some(rec) = map.get_mut(path) {
                if rec.content.as_ref() == content.as_ref() {
                    rec.injected = Some(content);
                }
            }
        }
    }

    /// Inject EVERY recorded carrier whose content changed since its last successful
    /// injection AND that `should_inject` admits — the query-time COMPLETENESS step.
    /// The queried carrier's diagnostics need its companion family (`.vue.tsx` + its
    /// `.vue.verter.ts` script) and any other already-open carrier it imports to be
    /// members of the SHARED Program, not just the single queried carrier; injecting the
    /// whole recorded open set at query time keeps the normal open→diagnostics flow
    /// correct (an unrelated carrier is a harmless extra open document).
    ///
    /// `should_inject` is the caller's shadow/conflict gate: a recorded path that is NOT
    /// a genuine generated carrier surface — e.g. a real user file occupying a
    /// carrier-companion path — is SKIPPED, never overlay-shadowed
    /// (`carrier_never_shadows_real_user_file`). Best-effort + dirty-tracked (unchanged
    /// carriers are skipped, so repeated queries do not re-send).
    ///
    /// PROACTIVE replay of the open set on a transport RE-establishment (resetting the
    /// injected markers when the established transport identity changes) is not yet
    /// implemented; a re-established transport re-injects each carrier lazily as it is
    /// next queried, and the transport's own barrier-synced slot is the authority for
    /// the content actually served.
    pub(crate) async fn inject_all_dirty<F>(&self, transport: &Arc<T>, should_inject: F)
    where
        F: Fn(&str) -> bool,
    {
        let paths: Vec<String> = self.content.lock().keys().cloned().collect();
        for path in paths {
            if should_inject(&path) {
                self.inject_dirty(transport, &path).await;
            }
        }
    }

    /// Whether the carrier's CURRENT recorded content is confirmed synced into the
    /// shared Program — i.e. its latest dirty injection SUCCEEDED (the last-injected
    /// content equals the current recorded content). A carrier whose current content
    /// failed to inject (its dirty injection returned an error, so the shared Program
    /// holds stale/prior content or none) is NOT synced: the composite fails closed to
    /// OWNED for that query rather than serve SHARED diagnostics computed against
    /// stale/absent content. An unrecorded carrier is not synced.
    pub(crate) fn is_synced(&self, path: &str) -> bool {
        let map = self.content.lock();
        map.get(path)
            .is_some_and(|rec| rec.injected.as_deref() == Some(rec.content.as_ref()))
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
        if let Some(transport) = self.transport.current().await {
            let _ = tokio::time::timeout(timeout, transport.retract(path)).await;
        }
    }
}

#[cfg(test)]
#[path = "overlay_core_tests.rs"]
mod overlay_core_tests;

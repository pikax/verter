//! Discriminating unit tests for [`LazyOverlayCore`] — the off-critical-path content
//! recording + lazy query-time establishment/injection, driven with a FAKE transport
//! double (no engine, no host, no real establishment hang).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex as SyncMutex;

use verter_type_runtime::protocol::TypeProviderError;
use verter_type_runtime::traits::ProviderFuture;

use super::{LazyOverlayCore, OverlayTransport};

/// A transport double: records each injection/retraction and reports controllable
/// liveness. No real relay/engine. `inject_fails` models a barrier error (a failed
/// dirty injection); `retract_hangs` models a slow/dead relay whose close never answers.
struct FakeTransport {
    alive: AtomicBool,
    ops: SyncMutex<Vec<String>>,
    inject_fails: AtomicBool,
    retract_hangs: AtomicBool,
}

impl FakeTransport {
    fn alive() -> Self {
        Self {
            alive: AtomicBool::new(true),
            ops: SyncMutex::new(Vec::new()),
            inject_fails: AtomicBool::new(false),
            retract_hangs: AtomicBool::new(false),
        }
    }

    fn ops(&self) -> Vec<String> {
        self.ops.lock().clone()
    }

    /// Make subsequent injections FAIL with a barrier error (a failed dirty injection).
    fn set_inject_fails(&self, fails: bool) {
        self.inject_fails.store(fails, Ordering::SeqCst);
    }

    /// Make the retract HANG forever (a slow/dead relay close that never answers).
    fn set_retract_hangs(&self, hangs: bool) {
        self.retract_hangs.store(hangs, Ordering::SeqCst);
    }
}

impl OverlayTransport for FakeTransport {
    fn inject(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let fails = self.inject_fails.load(Ordering::SeqCst);
        let entry = format!("{path}={content}");
        Box::pin(async move {
            if fails {
                // A barrier error: the shared Program did NOT accept this content.
                return Err(TypeProviderError::new("fake inject barrier failed"));
            }
            self.ops.lock().push(entry);
            Ok(())
        })
    }

    fn retract(&self, path: &str) -> ProviderFuture<'_, ()> {
        let hangs = self.retract_hangs.load(Ordering::SeqCst);
        let entry = format!("close:{path}");
        Box::pin(async move {
            if hangs {
                // A never-answering relay close: pend forever. `retract_bounded`'s
                // timeout must fire and return regardless.
                std::future::pending::<()>().await;
            }
            self.ops.lock().push(entry);
            Ok(())
        })
    }

    fn is_live(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn teardown(&self) -> ProviderFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

/// The OWNED lifecycle content record must NOT block on the (up-to-15s) SHARED
/// establishment: it is OFF the OWNED critical path. A slow/never-establishing
/// transport does not delay `record_content`.
///
/// RED before the fix: the lifecycle established the SHARED transport inline (the
/// composite's `open_file` awaited `feed_open` → `ensure_transport`), so opting into
/// SHARED tripped the OWNED file-lifecycle timing — a `record_content` routed through
/// establishment BLOCKS behind an in-flight (singleflight) establishment.
#[tokio::test]
async fn lifecycle_record_does_not_block_on_establishment() {
    let core = Arc::new(LazyOverlayCore::<FakeTransport>::new());
    let establishing = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());

    // A background QUERY-path establishment that BLOCKS (models a slow/never-init
    // editor tsgo). It holds the singleflight establishment for up to 30s.
    let bg = {
        let core = Arc::clone(&core);
        let establishing = Arc::clone(&establishing);
        let release = Arc::clone(&release);
        tokio::spawn(async move {
            core.ensure(
                Some(((), 1u64)),
                |_g| Some("nonce-1".to_string()),
                move |(), _g| {
                    let establishing = Arc::clone(&establishing);
                    let release = Arc::clone(&release);
                    async move {
                        establishing.notify_one();
                        release.notified().await; // block "establishing"
                        Some(Arc::new(FakeTransport::alive()))
                    }
                },
                Duration::from_secs(30),
            )
            .await
        })
    };
    // Wait until the establishment is in flight (blocked).
    establishing.notified().await;

    // The OWNED lifecycle content record must NOT block behind the in-flight SHARED
    // establishment — it is OFF the OWNED critical path.
    let start = tokio::time::Instant::now();
    core.record_content("/ws/Foo.vue.tsx", "v1");
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(250),
        "the lifecycle content record must NOT block on an in-flight SHARED establishment \
         (off the OWNED critical path); took {elapsed:?}"
    );

    // Let the background establishment finish so the test task exits cleanly.
    release.notify_one();
    let _ = bg.await.unwrap();
}

/// The query path establishes the transport lazily (never the lifecycle path) and
/// injects the LATEST recorded content. SHARED appears on the diagnostics query once
/// established; until then the composite fails closed to OWNED.
#[tokio::test]
async fn query_path_establishes_and_injects_latest_recorded_content() {
    let core = LazyOverlayCore::<FakeTransport>::new();
    let establishes = Arc::new(AtomicUsize::new(0));

    // The OWNED lifecycle records content (off-path); an edit updates it. No establish.
    core.record_content("/ws/Foo.vue.tsx", "content-v1");
    core.record_content("/ws/Foo.vue.tsx", "content-v2");
    assert_eq!(
        establishes.load(Ordering::SeqCst),
        0,
        "recording content never establishes the transport (off the OWNED critical path)"
    );

    // The QUERY path establishes (bounded) + injects the LATEST recorded content.
    let est = Arc::clone(&establishes);
    let transport = core
        .ensure(
            Some(((), 1u64)),
            |_g| Some("nonce-1".to_string()),
            move |(), _g| {
                let est = Arc::clone(&est);
                async move {
                    est.fetch_add(1, Ordering::SeqCst);
                    Some(Arc::new(FakeTransport::alive()))
                }
            },
            Duration::from_secs(5),
        )
        .await
        .expect("query-time establishment");
    assert_eq!(
        establishes.load(Ordering::SeqCst),
        1,
        "the query path establishes the transport"
    );

    core.inject_dirty(&transport, "/ws/Foo.vue.tsx").await;
    assert_eq!(
        transport.ops(),
        vec!["/ws/Foo.vue.tsx=content-v2".to_string()],
        "the query path injects the LATEST recorded content (fail-closed to OWNED until established)"
    );
}

/// The query-time injection is dirty-tracked: unchanged content is injected ONCE
/// (not re-sent on every diagnostics query — no Program spam), while an edit re-arms
/// injection.
#[tokio::test]
async fn unchanged_content_is_not_reinjected_but_edits_are() {
    let core = LazyOverlayCore::<FakeTransport>::new();
    core.record_content("/ws/A.vue.tsx", "v1");
    let transport = core
        .ensure(
            Some(((), 1u64)),
            |_g| Some("nonce-1".to_string()),
            |(), _g| async { Some(Arc::new(FakeTransport::alive())) },
            Duration::from_secs(5),
        )
        .await
        .expect("establishes");

    // First query injects; a second query with UNCHANGED content does NOT re-inject.
    core.inject_dirty(&transport, "/ws/A.vue.tsx").await;
    core.inject_dirty(&transport, "/ws/A.vue.tsx").await;
    assert_eq!(
        transport.ops(),
        vec!["/ws/A.vue.tsx=v1".to_string()],
        "unchanged content is injected ONCE, not on every diagnostics query"
    );

    // An edit re-arms injection.
    core.record_content("/ws/A.vue.tsx", "v2");
    core.inject_dirty(&transport, "/ws/A.vue.tsx").await;
    assert_eq!(
        transport.ops(),
        vec![
            "/ws/A.vue.tsx=v1".to_string(),
            "/ws/A.vue.tsx=v2".to_string()
        ],
        "an edit re-injects the new content"
    );
}

/// A single diagnostics query injects the WHOLE open carrier set (the queried
/// carrier + its companion family + any other open carrier), so the queried carrier's
/// companion imports resolve in the SHARED Program — not just the single queried
/// carrier. This is the normal open→diagnostics flow (both `Widget.vue.tsx` and its
/// `Widget.vue.verter.ts` script are open; a query on the carrier must inject both).
///
/// RED before this: `inject_dirty` injected ONLY the queried carrier, so the carrier's
/// import of its recorded-but-uninjected companion spuriously failed (a live TS2307).
#[tokio::test]
async fn inject_all_dirty_injects_the_whole_open_carrier_set() {
    let core = LazyOverlayCore::<FakeTransport>::new();
    // The carrier + its script companion (same carrier source), both open.
    core.record_content("/ws/Widget.vue.tsx", "carrier");
    core.record_content("/ws/Widget.vue.verter.ts", "companion");
    let transport = core
        .ensure(
            Some(((), 1u64)),
            |_g| Some("nonce-1".to_string()),
            |(), _g| async { Some(Arc::new(FakeTransport::alive())) },
            Duration::from_secs(5),
        )
        .await
        .expect("establishes");

    // A single diagnostics query injects the WHOLE open set — so the carrier's import
    // of its companion resolves, not just the queried carrier.
    core.inject_all_dirty(&transport, |_| true).await;
    let mut ops = transport.ops();
    ops.sort();
    assert_eq!(
        ops,
        vec![
            "/ws/Widget.vue.tsx=carrier".to_string(),
            "/ws/Widget.vue.verter.ts=companion".to_string(),
        ],
        "a diagnostics query injects ALL recorded open carriers (the companion family)"
    );

    // A second query re-injects nothing (all clean — no Program spam).
    core.inject_all_dirty(&transport, |_| true).await;
    assert_eq!(
        transport.ops().len(),
        2,
        "unchanged carriers are not re-injected on the next query"
    );
}

/// A carrier whose CURRENT content failed to sync into the shared Program is NOT synced:
/// the composite fails closed to OWNED rather than serve SHARED diagnostics computed
/// against the stale/prior synced slot. `is_synced` is the oracle the query path reads.
///
/// RED before the fix: a failed dirty injection was swallowed and the query still served
/// SHARED from a prior synced slot — a STALE result. `is_synced` distinguishes a carrier
/// whose CURRENT content is confirmed synced from one whose latest injection failed.
#[tokio::test]
async fn failed_dirty_injection_is_not_synced_so_query_fails_closed() {
    let core = LazyOverlayCore::<FakeTransport>::new();
    core.record_content("/ws/Foo.vue.tsx", "v1");
    let transport = core
        .ensure(
            Some(((), 1u64)),
            |_g| Some("nonce-1".to_string()),
            |(), _g| async { Some(Arc::new(FakeTransport::alive())) },
            Duration::from_secs(5),
        )
        .await
        .expect("establishes");

    // An unrecorded carrier is never synced.
    assert!(
        !core.is_synced("/ws/Never.vue.tsx"),
        "an unrecorded carrier is not synced (fail closed to OWNED)"
    );

    // A successful first injection ⇒ the carrier's current content is synced.
    core.inject_dirty(&transport, "/ws/Foo.vue.tsx").await;
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "a successfully-injected carrier's current content is synced"
    );

    // An edit changes the content; the NEXT injection FAILS (a barrier error). The
    // shared Program still holds the PRIOR synced content, so the carrier's CURRENT
    // content is NOT synced — the composite must fail closed to OWNED for this query.
    transport.set_inject_fails(true);
    core.record_content("/ws/Foo.vue.tsx", "v2");
    core.inject_dirty(&transport, "/ws/Foo.vue.tsx").await;
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "a carrier whose CURRENT content failed to inject is NOT synced — never serve \
         SHARED against the stale prior slot"
    );

    // A later successful injection re-syncs the current content (self-healing).
    transport.set_inject_fails(false);
    core.inject_dirty(&transport, "/ws/Foo.vue.tsx").await;
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "a later successful injection re-syncs the current content"
    );
}

/// A SHARED carrier retract issued off the OWNED close path is BOUNDED and fail-closed:
/// a slow/dead relay whose retract never answers cannot hang or delay the composite
/// close — `retract_bounded` returns within its timeout and still drops the recorded
/// content locally.
///
/// RED before the fix: `feed_close` awaited an UNBOUNDED `transport.retract`, so a
/// never-answering relay hung the composite `close_file` (degrading the OWNED lifecycle).
#[tokio::test]
async fn retract_is_bounded_when_the_relay_close_never_answers() {
    let core = LazyOverlayCore::<FakeTransport>::new();
    core.record_content("/ws/Foo.vue.tsx", "v1");
    let _transport = core
        .ensure(
            Some(((), 1u64)),
            |_g| Some("nonce-1".to_string()),
            |(), _g| async {
                let t = FakeTransport::alive();
                t.set_retract_hangs(true);
                Some(Arc::new(t))
            },
            Duration::from_secs(5),
        )
        .await
        .expect("establishes");

    // The retract never answers; `retract_bounded` must return within its SHORT bound.
    let start = tokio::time::Instant::now();
    core.retract_bounded("/ws/Foo.vue.tsx", Duration::from_millis(150))
        .await;
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "a never-answering relay close must NOT hang the composite close (bounded, \
         fail-closed); took {elapsed:?}"
    );

    // The recorded content is dropped regardless — the carrier is closed locally.
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "retract_bounded drops the recorded content even when the relay close hangs"
    );
}

/// `inject_all_dirty` GATES each recorded carrier on the caller's shadow-safety
/// predicate: a companion the predicate rejects (e.g. a real user file occupying a
/// carrier-companion path) is NEVER injected / overlay-shadowed, while a genuine
/// generated companion IS. RED before the gate: `inject_all_dirty` injected EVERY
/// recorded path by shape alone, overlay-shadowing a real user file.
#[tokio::test]
async fn inject_all_dirty_skips_shadow_unsafe_carriers() {
    let core = LazyOverlayCore::<FakeTransport>::new();
    // A genuine generated companion + a real-user-file shadow at a companion path.
    core.record_content("/ws/Genuine.vue.tsx", "genuine");
    core.record_content("/ws/Shadow.vue.tsx", "real-user-file");
    let transport = core
        .ensure(
            Some(((), 1u64)),
            |_g| Some("nonce-1".to_string()),
            |(), _g| async { Some(Arc::new(FakeTransport::alive())) },
            Duration::from_secs(5),
        )
        .await
        .expect("establishes");

    // The shadow-safety predicate ADMITS the genuine companion, REJECTS the shadow.
    core.inject_all_dirty(&transport, |companion| companion != "/ws/Shadow.vue.tsx")
        .await;
    assert_eq!(
        transport.ops(),
        vec!["/ws/Genuine.vue.tsx=genuine".to_string()],
        "the shadow-unsafe companion is NEVER injected (never overlay-shadowed); only the \
         genuine generated companion is"
    );
}

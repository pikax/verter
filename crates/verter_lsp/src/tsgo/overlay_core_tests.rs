//! Discriminating unit tests for [`LazyOverlayCore`] — the off-critical-path content
//! recording + lazy query-time establishment/injection, driven with a FAKE transport
//! double (no engine, no host, no real establishment hang).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex as SyncMutex;
use tokio::sync::Notify;

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
    inject_gated: AtomicBool,
    inject_reached: Notify,
    inject_release: Notify,
    retract_gated: AtomicBool,
    retract_reached: Notify,
    retract_release: Notify,
}

impl FakeTransport {
    fn alive() -> Self {
        Self {
            alive: AtomicBool::new(true),
            ops: SyncMutex::new(Vec::new()),
            inject_fails: AtomicBool::new(false),
            retract_hangs: AtomicBool::new(false),
            inject_gated: AtomicBool::new(false),
            inject_reached: Notify::new(),
            inject_release: Notify::new(),
            retract_gated: AtomicBool::new(false),
            retract_reached: Notify::new(),
            retract_release: Notify::new(),
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

    /// Mark the transport DEAD — the live-death eviction predicate then evicts it.
    fn set_dead(&self) {
        self.alive.store(false, Ordering::SeqCst);
    }

    /// Arm the inject GATE: the next injection signals `inject_reached` and BLOCKS until
    /// `release_inject` is called — models an inject that is mid-await while the transport
    /// is re-established underneath it (the reconnect split-brain window).
    fn arm_inject_gate(&self) {
        self.inject_gated.store(true, Ordering::SeqCst);
    }

    /// Release a gated in-flight injection so it commits.
    fn release_inject(&self) {
        self.inject_release.notify_one();
    }

    /// Arm the retract GATE: the next retract signals `retract_reached` and BLOCKS until
    /// `release_retract` is called — models the flip-to-unsafe BOUNDED retract held
    /// mid-await while a concurrent query observes the carrier's sync state (the served
    /// window: the injected marker is still set and `{safe:false}` is already cached).
    fn arm_retract_gate(&self) {
        self.retract_gated.store(true, Ordering::SeqCst);
    }

    /// Release a gated in-flight retract so it completes.
    fn release_retract(&self) {
        self.retract_release.notify_one();
    }
}

impl OverlayTransport for FakeTransport {
    fn inject(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let fails = self.inject_fails.load(Ordering::SeqCst);
        let gated = self.inject_gated.load(Ordering::SeqCst);
        let entry = format!("{path}={content}");
        Box::pin(async move {
            if gated {
                // Signal the injection is in-flight, then block until released — the
                // transport may be re-established (a new epoch observed) meanwhile.
                self.inject_reached.notify_one();
                self.inject_release.notified().await;
            }
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
        let gated = self.retract_gated.load(Ordering::SeqCst);
        let entry = format!("close:{path}");
        Box::pin(async move {
            if gated {
                // Signal the retract is in-flight, then block until released — models the
                // flip-to-unsafe bounded retract held mid-await while a concurrent query
                // observes the carrier's sync state (marker still set, `{safe:false}`
                // already cached).
                self.retract_reached.notify_one();
                self.retract_release.notified().await;
            }
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

    core.inject_dirty(&transport, "/ws/Foo.vue.tsx", None).await;
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
    core.inject_dirty(&transport, "/ws/A.vue.tsx", None).await;
    core.inject_dirty(&transport, "/ws/A.vue.tsx", None).await;
    assert_eq!(
        transport.ops(),
        vec!["/ws/A.vue.tsx=v1".to_string()],
        "unchanged content is injected ONCE, not on every diagnostics query"
    );

    // An edit re-arms injection.
    core.record_content("/ws/A.vue.tsx", "v2");
    core.inject_dirty(&transport, "/ws/A.vue.tsx", None).await;
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
    core.inject_all_dirty(&transport, None, |_| true).await;
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
    core.inject_all_dirty(&transport, None, |_| true).await;
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
    core.inject_dirty(&transport, "/ws/Foo.vue.tsx", None).await;
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "a successfully-injected carrier's current content is synced"
    );

    // An edit changes the content; the NEXT injection FAILS (a barrier error). The
    // shared Program still holds the PRIOR synced content, so the carrier's CURRENT
    // content is NOT synced — the composite must fail closed to OWNED for this query.
    transport.set_inject_fails(true);
    core.record_content("/ws/Foo.vue.tsx", "v2");
    core.inject_dirty(&transport, "/ws/Foo.vue.tsx", None).await;
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "a carrier whose CURRENT content failed to inject is NOT synced — never serve \
         SHARED against the stale prior slot"
    );

    // A later successful injection re-syncs the current content (self-healing).
    transport.set_inject_fails(false);
    core.inject_dirty(&transport, "/ws/Foo.vue.tsx", None).await;
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
    core.inject_all_dirty(&transport, None, |companion| {
        companion != "/ws/Shadow.vue.tsx"
    })
    .await;
    assert_eq!(
        transport.ops(),
        vec!["/ws/Genuine.vue.tsx=genuine".to_string()],
        "the shadow-unsafe companion is NEVER injected (never overlay-shadowed); only the \
         genuine generated companion is"
    );
}

/// Establish a fresh ALIVE transport at `(generation, nonce)` through the query path.
async fn establish_alive(
    core: &LazyOverlayCore<FakeTransport>,
    generation: u64,
    nonce: &str,
) -> Arc<FakeTransport> {
    let nonce = nonce.to_string();
    core.ensure(
        Some(((), generation)),
        move |_g| Some(nonce.clone()),
        |(), _g| async { Some(Arc::new(FakeTransport::alive())) },
        Duration::from_secs(5),
    )
    .await
    .expect("establishes")
}

/// Re-establish a fresh ALIVE transport after the prior one died — driving demands at
/// advancing generations until the dead-live eviction + re-arm mints a new transport
/// (robust to whether eviction re-establishes in the same demand or a later one).
async fn reestablish_after_death(core: &LazyOverlayCore<FakeTransport>) -> Arc<FakeTransport> {
    for g in 2u64..=6 {
        let nonce = format!("nonce-{g}");
        if let Some(t) = core
            .ensure(
                Some(((), g)),
                move |_g| Some(nonce.clone()),
                |(), _g| async { Some(Arc::new(FakeTransport::alive())) },
                Duration::from_secs(5),
            )
            .await
        {
            return t;
        }
    }
    panic!("re-establishment did not mint a fresh transport");
}

/// A transport RE-established under a NEW identity/epoch replays the whole open
/// carrier set — every recorded carrier is re-injected into the fresh, empty transport,
/// not skipped as "already injected" against the DEAD prior transport.
///
/// RED before the fix: the injected markers are keyed only on content (not the transport
/// epoch), so after a reconnect the clean carriers look already-synced and the new,
/// empty transport receives NOTHING (the reconnect split-brain). GREEN: the new epoch
/// marks every carrier dirty, so the full open set replays.
#[tokio::test]
async fn reestablished_transport_replays_open_carrier_set() {
    let core = LazyOverlayCore::<FakeTransport>::new();
    core.record_content("/ws/A.vue.tsx", "a");
    core.record_content("/ws/B.vue.tsx", "b");

    // Establish transport A (epoch 1) and inject the whole open set.
    let transport_a = establish_alive(&core, 1, "nonce-1").await;
    core.inject_all_dirty(&transport_a, None, |_| true).await;
    let mut ops_a = transport_a.ops();
    ops_a.sort();
    assert_eq!(
        ops_a,
        vec!["/ws/A.vue.tsx=a".to_string(), "/ws/B.vue.tsx=b".to_string()],
        "transport A receives the whole open set"
    );
    // A second warm call into the SAME transport injects nothing (all clean for epoch 1).
    core.inject_all_dirty(&transport_a, None, |_| true).await;
    assert_eq!(
        transport_a.ops().len(),
        2,
        "a warm re-inject into the SAME transport is a no-op"
    );

    // The transport dies; a reconnect mints transport B at an ADVANCED generation
    // (epoch 2).
    transport_a.set_dead();
    let transport_b = reestablish_after_death(&core).await;
    assert!(
        !Arc::ptr_eq(&transport_a, &transport_b),
        "B is a genuinely fresh transport"
    );

    // Replay: the new epoch marks every carrier dirty, so B receives the FULL open set.
    core.inject_all_dirty(&transport_b, None, |_| true).await;
    let mut ops_b = transport_b.ops();
    ops_b.sort();
    assert_eq!(
        ops_b,
        vec!["/ws/A.vue.tsx=a".to_string(), "/ws/B.vue.tsx=b".to_string()],
        "a re-established transport replays the whole open carrier set"
    );
}

/// Split-brain guard: a STALE in-flight injection that began under epoch A cannot
/// mark the carrier synced for a NEW epoch B. The A-injection captures its run epoch;
/// once epoch B is observed (markers reset), the late A-commit is refused, so
/// `is_synced` stays false until a genuine B-epoch injection commits.
///
/// RED before the guard: the late A-commit stamps the CURRENT (B) epoch (or ignores the
/// epoch), falsely marking the carrier synced against a transport that never received it.
#[tokio::test]
async fn stale_inflight_injection_cannot_mark_new_epoch_synced() {
    let core = Arc::new(LazyOverlayCore::<FakeTransport>::new());
    core.record_content("/ws/Foo.vue.tsx", "content");

    // Establish transport A (epoch 1) with an ARMED inject gate: its injection blocks
    // mid-flight until released.
    let a = Arc::new({
        let t = FakeTransport::alive();
        t.arm_inject_gate();
        t
    });
    let a_for_est = Arc::clone(&a);
    let transport_a = core
        .ensure(
            Some(((), 1u64)),
            |_g| Some("nonce-1".to_string()),
            move |(), _g| {
                let a = Arc::clone(&a_for_est);
                async move { Some(a) }
            },
            Duration::from_secs(5),
        )
        .await
        .expect("establish A");
    assert!(Arc::ptr_eq(&transport_a, &a));

    // Begin the A-injection on a task; it captures run-epoch 1 and BLOCKS in the gate.
    let inj = {
        let core = Arc::clone(&core);
        let ta = Arc::clone(&transport_a);
        tokio::spawn(async move {
            core.inject_dirty(&ta, "/ws/Foo.vue.tsx", None).await;
        })
    };
    // Wait until the A-injection is in-flight (captured epoch 1, awaiting the gate).
    a.inject_reached.notified().await;

    // The transport dies; a reconnect mints transport B (epoch 2) — observing epoch 2
    // resets the injected markers.
    a.set_dead();
    let transport_b = reestablish_after_death(&core).await;

    // Release the stale A-injection; it now commits AFTER epoch 2 was observed.
    a.release_inject();
    inj.await.unwrap();

    // The stale A-commit must NOT have marked the carrier synced for epoch 2.
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "a stale in-flight epoch-A injection cannot mark the carrier synced for epoch B"
    );

    // A genuine B-epoch injection syncs it.
    core.inject_dirty(&transport_b, "/ws/Foo.vue.tsx", None)
        .await;
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "a genuine B-epoch injection syncs the carrier"
    );
}

/// A content-clean carrier whose shadow-safety generation is unchanged
/// SKIPS the (disk-touching) shadow-safety predicate entirely — the warm-query
/// optimization. RED (predicate re-evaluated every query): the counter advances on the
/// 2nd warm call. GREEN: 0 predicate calls on the 2nd warm call.
#[tokio::test]
async fn inject_all_dirty_skips_shadow_predicate_for_clean_cached_generation() {
    let core = LazyOverlayCore::<FakeTransport>::new();
    core.record_content("/ws/Foo.vue.tsx", "v1");
    let transport = establish_alive(&core, 1, "nonce-1").await;

    let calls = Arc::new(AtomicUsize::new(0));

    // First query at shadow generation 1 evaluates + injects (predicate → true).
    let c = Arc::clone(&calls);
    core.inject_all_dirty(&transport, Some(1), move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        true
    })
    .await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the first query evaluates shadow-safety"
    );
    assert!(core.is_synced("/ws/Foo.vue.tsx"));

    // A second query at the SAME shadow generation with no content edits SKIPS the
    // predicate entirely (content-clean AND shadow-generation-clean).
    let c = Arc::clone(&calls);
    core.inject_all_dirty(&transport, Some(1), move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        true
    })
    .await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "a content-clean, shadow-generation-clean carrier skips the shadow predicate (0 new calls)"
    );
}

/// Shadow-safety invalidation: a content-clean carrier is RE-CHECKED when the
/// shadow-safety generation ADVANCES (a workspace file-set transition could have flipped
/// its shadow-safety) — the generation-cache must INVALIDATE, not just optimize.
///
/// RED before the fix (a dirty-first sweep skipping clean carriers regardless of the
/// shadow generation): the predicate is NOT called on the gen-2 query → a shadow-safety
/// flip would be missed. GREEN: the predicate IS called for the clean carrier on the
/// generation advance.
#[tokio::test]
async fn shadow_generation_advance_rechecks_clean_carriers() {
    let core = LazyOverlayCore::<FakeTransport>::new();
    core.record_content("/ws/Foo.vue.tsx", "v1");
    let transport = establish_alive(&core, 1, "nonce-1").await;

    let calls = Arc::new(AtomicUsize::new(0));

    // Inject at shadow generation 1.
    let c = Arc::clone(&calls);
    core.inject_all_dirty(&transport, Some(1), move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        true
    })
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // NO content edit; a query at shadow generation 2 MUST re-check the clean carrier
    // (the file-set may have changed such that shadow-safety flipped).
    let c = Arc::clone(&calls);
    core.inject_all_dirty(&transport, Some(2), move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        true
    })
    .await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "a content-clean carrier is RE-CHECKED when the shadow-safety generation advances"
    );
}

/// Fail-safe: a `None` shadow-safety generation is NEVER trusted as a
/// cache key — the predicate is re-evaluated on EVERY query (the caller could not
/// observe a generation, so the cache cannot be proven fresh). RED (cache trusted when
/// present): the 2nd `None` query skips the predicate. GREEN: it re-evaluates.
#[tokio::test]
async fn inject_all_dirty_none_generation_always_rechecks() {
    let core = LazyOverlayCore::<FakeTransport>::new();
    core.record_content("/ws/Foo.vue.tsx", "v1");
    let transport = establish_alive(&core, 1, "nonce-1").await;

    let calls = Arc::new(AtomicUsize::new(0));

    let c = Arc::clone(&calls);
    core.inject_all_dirty(&transport, None, move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        true
    })
    .await;
    let c = Arc::clone(&calls);
    core.inject_all_dirty(&transport, None, move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        true
    })
    .await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "a None shadow-safety generation re-evaluates the predicate every query (never trusts the cache)"
    );
}

/// When a content-clean carrier's shadow-safety flips to UNSAFE on a
/// generation advance (a real user file appeared at its companion path), the stale
/// overlay must no longer be treated synced. RED (skip clean carriers): the carrier
/// stays synced. GREEN: `is_synced` becomes false.
#[tokio::test]
async fn shadow_generation_flip_to_unsafe_clears_synced_marker() {
    let core = LazyOverlayCore::<FakeTransport>::new();
    core.record_content("/ws/Foo.vue.tsx", "v1");
    let transport = establish_alive(&core, 1, "nonce-1").await;

    // Inject SAFE at shadow generation 1.
    core.inject_all_dirty(&transport, Some(1), |_| true).await;
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "the safe carrier is synced at gen 1"
    );

    // At shadow generation 2 the predicate flips to UNSAFE (a real file now occupies the
    // companion path). The stale overlay is not treated synced.
    core.inject_all_dirty(&transport, Some(2), |_| false).await;
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "a carrier whose shadow-safety flipped to unsafe is no longer synced"
    );
}

/// Upholds `carrier_never_shadows_real_user_file`: when a PREVIOUSLY
/// INJECTED carrier flips to shadow-UNSAFE, clearing the local marker is not enough —
/// the overlay is still an open document in the SHARED Program shadowing the now-real
/// user file. It MUST be RETRACTED from the transport, and the `ContentRecord` KEPT so
/// it re-injects if it later flips back to safe.
///
/// RED (clear-marker-only): no retract is issued → the overlay still shadows the real
/// file. GREEN: a retract is recorded, the record retained, and a flip-back re-injects.
#[tokio::test]
async fn shadow_flip_to_unsafe_retracts_previously_injected_carrier() {
    let core = LazyOverlayCore::<FakeTransport>::new();
    core.record_content("/ws/Foo.vue.tsx", "v1");
    let transport = establish_alive(&core, 1, "nonce-1").await;

    // Inject SAFE at shadow generation 1 (the transport records the inject).
    core.inject_all_dirty(&transport, Some(1), |_| true).await;
    assert_eq!(transport.ops(), vec!["/ws/Foo.vue.tsx=v1".to_string()]);

    // At shadow generation 2 the carrier flips to UNSAFE (a real user file appeared at
    // the companion path). The previously-injected overlay must be RETRACTED.
    core.inject_all_dirty(&transport, Some(2), |_| false).await;
    assert!(
        transport
            .ops()
            .contains(&"close:/ws/Foo.vue.tsx".to_string()),
        "a previously-injected carrier that flipped to unsafe is RETRACTED from the SHARED Program"
    );
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "the retracted carrier is no longer synced"
    );

    // The ContentRecord is retained: at shadow generation 3 the carrier flips BACK to
    // safe (the real file was removed) and RE-INJECTS.
    core.inject_all_dirty(&transport, Some(3), |_| true).await;
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "a carrier that flipped back to safe re-injects (the ContentRecord was retained)"
    );
    assert_eq!(
        transport
            .ops()
            .iter()
            .filter(|op| op.as_str() == "/ws/Foo.vue.tsx=v1")
            .count(),
        2,
        "the carrier is re-injected after flipping back to safe"
    );
}

/// Upholds `carrier_never_shadows_real_user_file` under a concurrent inject/retract race:
/// an in-flight injection that BEGAN while the carrier was shadow-SAFE must NOT restore the
/// synced marker after a CONCURRENT flip-to-unsafe retracted the overlay and cached it
/// unsafe. The stale in-flight commit sees the epoch + content still current, but the
/// carrier became cached-UNSAFE meanwhile — re-marking it synced would shadow the now-real
/// user file the concurrent retract just made room for.
///
/// BOTH sweeps drive the PRODUCTION entry `inject_all_dirty` (not a direct `inject_dirty`),
/// so the safe-branch commit that must be vetoed is the real one the composite runs. Run 1
/// is a SAFE sweep (`|_| true`) at generation 1 whose inject is held mid-flight by the
/// gate; run 2 is an UNSAFE sweep (`|_| false`) at the advanced generation 2 that RETRACTS
/// the previously-injected overlay, caches `{safe:false}`, and clears the marker; run 1's
/// held inject then completes and its `inject_all_dirty` safe-branch attempts to commit.
/// Epoch (1==1) and content (v2==v2) are BOTH still current, so a commit guard that checked
/// only epoch + content would restore `injected` and `is_synced` would wrongly return true
/// over a retracted/unsafe carrier.
///
/// RED against a hypothetical un-vetoed commit (epoch + content only): the marker is
/// restored and `is_synced` returns true. GREEN: the cached-unsafe veto refuses the stale
/// safe-branch commit and `is_synced` stays false.
#[tokio::test]
async fn inflight_inject_does_not_resync_after_concurrent_flip_to_unsafe() {
    let core = Arc::new(LazyOverlayCore::<FakeTransport>::new());
    core.record_content("/ws/Foo.vue.tsx", "v1");

    // Establish (epoch 1) and inject v1 at shadow generation 1 — the carrier is now
    // SAFE and PREVIOUSLY INJECTED (a later flip-to-unsafe must retract it).
    let transport = establish_alive(&core, 1, "nonce-1").await;
    core.inject_all_dirty(&transport, Some(1), |_| true).await;
    assert!(core.is_synced("/ws/Foo.vue.tsx"));
    assert_eq!(transport.ops(), vec!["/ws/Foo.vue.tsx=v1".to_string()]);

    // An edit makes the carrier content-dirty; arm the inject gate so the NEXT inject
    // blocks mid-flight (the in-flight injection window).
    core.record_content("/ws/Foo.vue.tsx", "v2");
    transport.arm_inject_gate();

    // Run 1: a SAFE sweep through the production `inject_all_dirty` begins under epoch 1 +
    // content v2 (the carrier is still cached-safe at generation 1) and its inject BLOCKS in
    // the gate — the real safe-branch commit that must be vetoed on release.
    let inj = {
        let core = Arc::clone(&core);
        let t = Arc::clone(&transport);
        tokio::spawn(async move {
            core.inject_all_dirty(&t, Some(1), |_| true).await;
        })
    };
    transport.inject_reached.notified().await;

    // Run 2 (concurrent): a SAFE→UNSAFE flip sweep through `inject_all_dirty` at the advanced
    // generation 2 (a real user file appeared at the companion path). The previously-injected
    // overlay is RETRACTED, cached `{safe:false}`, and the marker cleared.
    core.inject_all_dirty(&transport, Some(2), |_| false).await;
    assert!(
        transport
            .ops()
            .contains(&"close:/ws/Foo.vue.tsx".to_string()),
        "the concurrent flip-to-unsafe retracts the previously-injected carrier"
    );
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "the carrier is retracted + cached-unsafe while the inject is still in flight"
    );

    // Release the held inject: run 1's safe-branch commit runs AFTER the flip-to-unsafe
    // retract.
    transport.release_inject();
    inj.await.unwrap();

    // The stale in-flight safe-branch commit must NOT restore the synced marker over the
    // retracted/unsafe carrier (carrier_never_shadows_real_user_file).
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "an in-flight safe sweep that began when the carrier was safe must NOT re-sync it \
         after a concurrent flip-to-unsafe retract"
    );
}

/// Defensive: a `None`-generation sweep (test-only — the composite always threads
/// `Some(content_generation)`) never WROTE the shadow-safety cache, so its `inject_dirty`
/// commit must NOT be vetoed by a stale `{safe:false}` a prior `Some` generation left
/// behind. The `None` sweep re-evaluated the carrier as SAFE, so it injects and syncs — the
/// commit both restores the marker (the veto's `None` short-circuit) AND drops the stale
/// cached decision it could not validate, so `is_synced` (which now consults shadow-safety)
/// reports the freshly re-injected safe carrier synced rather than fail closed on the stale
/// `{safe:false}`.
///
/// RED against the veto consulting the cache irrespective of generation: the stale
/// `{safe:false}` vetoes the commit → `is_synced` false. GREEN: the `None` sweep ignores a
/// cache entry it could not have validated → `is_synced` true.
#[tokio::test]
async fn none_generation_sweep_is_not_vetoed_by_a_stale_cached_unsafe() {
    let core = LazyOverlayCore::<FakeTransport>::new();
    core.record_content("/ws/Foo.vue.tsx", "v1");
    let transport = establish_alive(&core, 1, "nonce-1").await;

    // A prior Some(1) sweep evaluates the carrier UNSAFE and caches {gen:1, safe:false}
    // (never injected → nothing to retract, just the cached decision).
    core.inject_all_dirty(&transport, Some(1), |_| false).await;
    assert!(!core.is_synced("/ws/Foo.vue.tsx"));

    // A later None sweep re-evaluates the carrier as SAFE. It never writes the cache, so its
    // inject_dirty commit must not be vetoed by the stale {1, false} it could not validate.
    core.inject_all_dirty(&transport, None, |_| true).await;
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "a None-generation sweep that re-evaluated the carrier safe must inject + sync it, \
         not be vetoed by a stale cached-unsafe decision it could not have validated"
    );
    assert!(
        transport.ops().contains(&"/ws/Foo.vue.tsx=v1".to_string()),
        "the None sweep injected the carrier"
    );
}

/// Upholds `carrier_never_shadows_real_user_file` at the SERVED level in the transient
/// flip-to-unsafe retract window. `is_synced` is the SOLE overlay-marker served gate the
/// composite reads (`if !is_synced { return None; }`); it MUST report a cached-UNSAFE
/// carrier as NOT synced even while its `injected` marker is still set. The flip-to-unsafe
/// sweep caches `{safe:false}` and THEN issues a BOUNDED retract with an AWAIT, clearing the
/// `injected` marker only AFTER that await — so between the cache write and the marker clear
/// the carrier has `injected` SET and `shadow_safety == {safe:false}`. A concurrent query
/// observing that window must fail closed to OWNED, never serve SHARED over the stale overlay
/// that shadows the now-real user file.
///
/// Driven through the PRODUCTION flip path (`inject_all_dirty` with an unsafe predicate) with
/// the transport's retract GATED mid-await, and the concurrent `is_synced` observation taken
/// while the retract is held — the exact served window, deterministically.
///
/// RED before the fix (`record_is_synced` consulted only the injected marker + epoch, NOT
/// shadow-safety): the still-set marker + matching content + matching epoch make `is_synced`
/// return TRUE for the unsafe carrier → SHARED is served → the overlay shadows the real user
/// file. GREEN after: the `{safe:false}` cache makes `is_synced` return FALSE the instant it
/// is written (before the retract await), so the concurrent query fails closed.
#[tokio::test]
async fn cached_unsafe_carrier_is_not_synced_even_with_a_set_injected_marker() {
    let core = Arc::new(LazyOverlayCore::<FakeTransport>::new());
    core.record_content("/ws/Foo.vue.tsx", "v1");

    // Establish (epoch 1) and inject v1 SAFE at shadow generation 1 — the carrier is now
    // SAFE, PREVIOUSLY INJECTED, and synced (marker set, content + epoch match).
    let transport = establish_alive(&core, 1, "nonce-1").await;
    core.inject_all_dirty(&transport, Some(1), |_| true).await;
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "the safe, previously-injected carrier is synced at gen 1"
    );
    assert_eq!(transport.ops(), vec!["/ws/Foo.vue.tsx=v1".to_string()]);

    // Arm the retract gate so the flip-to-unsafe sweep's BOUNDED retract blocks mid-await —
    // freezing the carrier in the window: `{safe:false}` cached, marker NOT yet cleared.
    transport.arm_retract_gate();

    // A flip-to-unsafe sweep at the advanced generation 2 (a real user file appeared at the
    // companion path) through the PRODUCTION entry: it caches `{safe:false}`, then issues the
    // retract which BLOCKS in the gate before the post-await marker clear.
    let flip = {
        let core = Arc::clone(&core);
        let t = Arc::clone(&transport);
        tokio::spawn(async move {
            core.inject_all_dirty(&t, Some(2), |_| false).await;
        })
    };
    // Wait until the flip sweep is holding the retract mid-await — we are now INSIDE the
    // window: injected marker STILL set, shadow-safety cached `{safe:false}`.
    transport.retract_reached.notified().await;

    // THE SERVED GATE, observed IN the window: a concurrent query's `is_synced` must fail
    // closed for the cached-unsafe carrier even though its injected marker is still set.
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "a cached-UNSAFE carrier is NOT synced even with a set injected marker (mid-retract \
         window) — never serve SHARED over the stale overlay shadowing the real user file"
    );

    // Release the held retract so the flip sweep completes (it then clears the marker).
    transport.release_retract();
    flip.await.unwrap();

    // Post-window: the carrier stays not-synced (marker cleared + still cached unsafe) and the
    // retract was issued.
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "the flipped-to-unsafe carrier stays not-synced after the retract completes"
    );
    assert!(
        transport
            .ops()
            .contains(&"close:/ws/Foo.vue.tsx".to_string()),
        "the previously-injected carrier was retracted from the SHARED Program"
    );
}

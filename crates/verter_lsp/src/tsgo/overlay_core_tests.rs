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
use crate::tsgo::transport_cell::EstablishedTransport;

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
    let established = core
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

    core.inject_dirty(&established, "/ws/Foo.vue.tsx", 1).await;
    assert_eq!(
        established.transport.ops(),
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
    let established = core
        .ensure(
            Some(((), 1u64)),
            |_g| Some("nonce-1".to_string()),
            |(), _g| async { Some(Arc::new(FakeTransport::alive())) },
            Duration::from_secs(5),
        )
        .await
        .expect("establishes");

    // First query injects; a second query with UNCHANGED content does NOT re-inject.
    core.inject_dirty(&established, "/ws/A.vue.tsx", 1).await;
    core.inject_dirty(&established, "/ws/A.vue.tsx", 1).await;
    assert_eq!(
        established.transport.ops(),
        vec!["/ws/A.vue.tsx=v1".to_string()],
        "unchanged content is injected ONCE, not on every diagnostics query"
    );

    // An edit re-arms injection.
    core.record_content("/ws/A.vue.tsx", "v2");
    core.inject_dirty(&established, "/ws/A.vue.tsx", 1).await;
    assert_eq!(
        established.transport.ops(),
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
    let established = core
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
    core.inject_all_dirty(&established, 1, |_| true).await;
    let mut ops = established.transport.ops();
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
    core.inject_all_dirty(&established, 1, |_| true).await;
    assert_eq!(
        established.transport.ops().len(),
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
    let established = core
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
    core.inject_dirty(&established, "/ws/Foo.vue.tsx", 1).await;
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "a successfully-injected carrier's current content is synced"
    );

    // An edit changes the content; the NEXT injection FAILS (a barrier error). The
    // shared Program still holds the PRIOR synced content, so the carrier's CURRENT
    // content is NOT synced — the composite must fail closed to OWNED for this query.
    established.transport.set_inject_fails(true);
    core.record_content("/ws/Foo.vue.tsx", "v2");
    core.inject_dirty(&established, "/ws/Foo.vue.tsx", 1).await;
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "a carrier whose CURRENT content failed to inject is NOT synced — never serve \
         SHARED against the stale prior slot"
    );

    // A later successful injection re-syncs the current content (self-healing).
    established.transport.set_inject_fails(false);
    core.inject_dirty(&established, "/ws/Foo.vue.tsx", 1).await;
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
    let established = core
        .ensure(
            Some(((), 1u64)),
            |_g| Some("nonce-1".to_string()),
            |(), _g| async { Some(Arc::new(FakeTransport::alive())) },
            Duration::from_secs(5),
        )
        .await
        .expect("establishes");

    // The shadow-safety predicate ADMITS the genuine companion, REJECTS the shadow.
    core.inject_all_dirty(&established, 1, |companion| {
        companion != "/ws/Shadow.vue.tsx"
    })
    .await;
    assert_eq!(
        established.transport.ops(),
        vec!["/ws/Genuine.vue.tsx=genuine".to_string()],
        "the shadow-unsafe companion is NEVER injected (never overlay-shadowed); only the \
         genuine generated companion is"
    );
}

/// Establish a fresh ALIVE transport at `(generation, nonce)` through the query path,
/// returning the identity-bound object (its epoch keys the injection attribution).
async fn establish_alive(
    core: &LazyOverlayCore<FakeTransport>,
    generation: u64,
    nonce: &str,
) -> EstablishedTransport<FakeTransport> {
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
async fn reestablish_after_death(
    core: &LazyOverlayCore<FakeTransport>,
) -> EstablishedTransport<FakeTransport> {
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
    let est_a = establish_alive(&core, 1, "nonce-1").await;
    let transport_a = Arc::clone(&est_a.transport);
    core.inject_all_dirty(&est_a, 1, |_| true).await;
    let mut ops_a = transport_a.ops();
    ops_a.sort();
    assert_eq!(
        ops_a,
        vec!["/ws/A.vue.tsx=a".to_string(), "/ws/B.vue.tsx=b".to_string()],
        "transport A receives the whole open set"
    );
    // A second warm call into the SAME transport injects nothing (all clean for epoch 1).
    core.inject_all_dirty(&est_a, 1, |_| true).await;
    assert_eq!(
        transport_a.ops().len(),
        2,
        "a warm re-inject into the SAME transport is a no-op"
    );

    // The transport dies; a reconnect mints transport B at an ADVANCED generation
    // (epoch 2).
    transport_a.set_dead();
    let est_b = reestablish_after_death(&core).await;
    let transport_b = Arc::clone(&est_b.transport);
    assert!(
        !Arc::ptr_eq(&transport_a, &transport_b),
        "B is a genuinely fresh transport"
    );

    // Replay: the new epoch marks every carrier dirty, so B receives the FULL open set.
    core.inject_all_dirty(&est_b, 1, |_| true).await;
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
    let established_a = core
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
    assert!(Arc::ptr_eq(&established_a.transport, &a));

    // Begin the A-injection on a task; it captures A's epoch (EA) and BLOCKS in the gate.
    let inj = {
        let core = Arc::clone(&core);
        tokio::spawn(async move {
            core.inject_dirty(&established_a, "/ws/Foo.vue.tsx", 1)
                .await;
        })
    };
    // Wait until the A-injection is in-flight (captured EA, awaiting the gate).
    a.inject_reached.notified().await;

    // The transport dies; a reconnect mints transport B (epoch 2) — observing epoch 2
    // resets the injected markers.
    a.set_dead();
    let established_b = reestablish_after_death(&core).await;

    // Release the stale A-injection; it now commits AFTER epoch 2 was observed.
    a.release_inject();
    inj.await.unwrap();

    // The stale A-commit must NOT have marked the carrier synced for epoch 2.
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "a stale in-flight epoch-A injection cannot mark the carrier synced for epoch B"
    );

    // A genuine B-epoch injection syncs it.
    core.inject_dirty(&established_b, "/ws/Foo.vue.tsx", 1)
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
    let established = establish_alive(&core, 1, "nonce-1").await;

    let calls = Arc::new(AtomicUsize::new(0));

    // First query at shadow generation 1 evaluates + injects (predicate → true).
    let c = Arc::clone(&calls);
    core.inject_all_dirty(&established, 1, move |_| {
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
    core.inject_all_dirty(&established, 1, move |_| {
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
    let established = establish_alive(&core, 1, "nonce-1").await;

    let calls = Arc::new(AtomicUsize::new(0));

    // Inject at shadow generation 1.
    let c = Arc::clone(&calls);
    core.inject_all_dirty(&established, 1, move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        true
    })
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // NO content edit; a query at shadow generation 2 MUST re-check the clean carrier
    // (the file-set may have changed such that shadow-safety flipped).
    let c = Arc::clone(&calls);
    core.inject_all_dirty(&established, 2, move |_| {
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

/// When a content-clean carrier's shadow-safety flips to UNSAFE on a
/// generation advance (a real user file appeared at its companion path), the stale
/// overlay must no longer be treated synced. RED (skip clean carriers): the carrier
/// stays synced. GREEN: `is_synced` becomes false.
#[tokio::test]
async fn shadow_generation_flip_to_unsafe_clears_synced_marker() {
    let core = LazyOverlayCore::<FakeTransport>::new();
    core.record_content("/ws/Foo.vue.tsx", "v1");
    let established = establish_alive(&core, 1, "nonce-1").await;

    // Inject SAFE at shadow generation 1.
    core.inject_all_dirty(&established, 1, |_| true).await;
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "the safe carrier is synced at gen 1"
    );

    // At shadow generation 2 the predicate flips to UNSAFE (a real file now occupies the
    // companion path). The stale overlay is not treated synced.
    core.inject_all_dirty(&established, 2, |_| false).await;
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
    let established = establish_alive(&core, 1, "nonce-1").await;
    let transport = Arc::clone(&established.transport);

    // Inject SAFE at shadow generation 1 (the transport records the inject).
    core.inject_all_dirty(&established, 1, |_| true).await;
    assert_eq!(transport.ops(), vec!["/ws/Foo.vue.tsx=v1".to_string()]);

    // At shadow generation 2 the carrier flips to UNSAFE (a real user file appeared at
    // the companion path). The previously-injected overlay must be RETRACTED.
    core.inject_all_dirty(&established, 2, |_| false).await;
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
    core.inject_all_dirty(&established, 3, |_| true).await;
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
/// an in-flight injection that BEGAN while the carrier was shadow-SAFE must NOT leave the
/// carrier synced after a CONCURRENT flip-to-unsafe. The two sweeps operate on the SAME
/// carrier path, so the per-path carrier gate serializes them: whichever holds the gate
/// first runs to completion before the other's physical operation, and exactly one of them
/// retracts the physically-landed overlay while the cached `{safe:false}` keeps the carrier
/// not-synced.
///
/// BOTH sweeps drive the PRODUCTION entry `inject_all_dirty`. Run 1 is a SAFE sweep
/// (`|_| true`) at generation 1 whose inject is held mid-flight by the gate (so it holds
/// the carrier gate); run 2 is an UNSAFE sweep (`|_| false`) at the advanced generation 2
/// that caches `{safe:false}` and then blocks on the carrier gate. Releasing run 1 lets the
/// two proceed in gate order; the carrier ends retracted + cached-unsafe regardless of
/// which side commits first.
///
/// RED against a hypothetical un-vetoed commit (epoch + content only): the marker is
/// restored and `is_synced` returns true. GREEN: the overlay is retracted and the
/// cached-unsafe decision keeps `is_synced` false.
#[tokio::test]
async fn inflight_inject_does_not_resync_after_concurrent_flip_to_unsafe() {
    let core = Arc::new(LazyOverlayCore::<FakeTransport>::new());
    core.record_content("/ws/Foo.vue.tsx", "v1");

    // Establish (epoch 1) and inject v1 at shadow generation 1 — the carrier is now
    // SAFE and PREVIOUSLY INJECTED (a later flip-to-unsafe must retract it).
    let established = establish_alive(&core, 1, "nonce-1").await;
    let transport = Arc::clone(&established.transport);
    core.inject_all_dirty(&established, 1, |_| true).await;
    assert!(core.is_synced("/ws/Foo.vue.tsx"));
    assert_eq!(transport.ops(), vec!["/ws/Foo.vue.tsx=v1".to_string()]);

    // An edit makes the carrier content-dirty; arm the inject gate so the NEXT inject
    // blocks mid-flight (the in-flight injection window).
    core.record_content("/ws/Foo.vue.tsx", "v2");
    transport.arm_inject_gate();

    // Run 1: a SAFE sweep through the production `inject_all_dirty` begins under epoch 1 +
    // content v2 (the carrier is still cached-safe at generation 1) and its inject BLOCKS in
    // the gate, HOLDING the carrier gate.
    let inj = {
        let core = Arc::clone(&core);
        let est = EstablishedTransport {
            transport: Arc::clone(&transport),
            identity: established.identity.clone(),
        };
        tokio::spawn(async move {
            core.inject_all_dirty(&est, 1, |_| true).await;
        })
    };
    transport.inject_reached.notified().await;

    // Run 2 (concurrent): a SAFE→UNSAFE flip sweep through `inject_all_dirty` at the advanced
    // generation 2 (a real user file appeared at the companion path). It caches `{safe:false}`
    // then blocks on the carrier gate held by run 1 — so it is spawned, not awaited inline.
    let flip = {
        let core = Arc::clone(&core);
        let est = EstablishedTransport {
            transport: Arc::clone(&transport),
            identity: established.identity.clone(),
        };
        tokio::spawn(async move {
            core.inject_all_dirty(&est, 2, |_| false).await;
        })
    };

    // Release run 1's held inject; the two sweeps now proceed in carrier-gate order.
    transport.release_inject();
    inj.await.unwrap();
    flip.await.unwrap();

    // The physically-landed overlay is retracted exactly once, and the stale safe sweep
    // must NOT leave the carrier synced over the cached-unsafe decision
    // (carrier_never_shadows_real_user_file).
    assert!(
        transport
            .ops()
            .contains(&"close:/ws/Foo.vue.tsx".to_string()),
        "the concurrent flip-to-unsafe retracts the (in-flight) injected carrier"
    );
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "an in-flight safe sweep that began when the carrier was safe must NOT re-sync it \
         after a concurrent flip-to-unsafe"
    );
}

/// Upholds `carrier_never_shadows_real_user_file` at the SERVED level: `record_is_synced`
/// MUST consult shadow-safety, so a cached-UNSAFE carrier is NOT synced EVEN WITH its
/// `injected` marker still SET (content + epoch matching). `is_synced` is the SOLE
/// overlay-marker served gate the composite reads (`if !is_synced { return None; }`).
///
/// A DIRECT discriminator of the shadow consult, with NO marker clear masking it: the carrier
/// is injected + committed SAFE (marker set, content + epoch match, synced), then a
/// SAME-generation flip-to-unsafe decision is cached DIRECTLY
/// (`cache_shadow_decision(.., safe:false)` folds `true && false` fail-closed to `{safe:false}`)
/// WITHOUT any clearing/retract sweep. The `injected` marker therefore stays SET while
/// `{safe:false}` is cached — the exact state where the shadow consult is the ONLY thing that
/// can make `is_synced` false. A negative control asserts NO `close` was issued, so the verdict
/// is the consult, not a cleared marker or a retract.
///
/// RED if `record_is_synced` drops its `shadow_safety.is_none_or(|c| c.safe)` consult: the
/// still-set marker + matching content + matching epoch make `is_synced` return TRUE for the
/// unsafe carrier → SHARED is served → the overlay shadows the real user file. GREEN with the
/// consult: `is_synced` returns FALSE the instant `{safe:false}` is cached.
#[tokio::test]
async fn cached_unsafe_carrier_is_not_synced_even_with_a_set_injected_marker() {
    let core = Arc::new(LazyOverlayCore::<FakeTransport>::new());
    core.record_content("/ws/Foo.vue.tsx", "v1");

    // Establish (epoch 1) and inject v1 SAFE at generation 1 — the carrier is SAFE,
    // PREVIOUSLY INJECTED, and synced (marker set, content + epoch match).
    let established = establish_alive(&core, 1, "nonce-1").await;
    let transport = Arc::clone(&established.transport);
    let run_epoch = established.identity.epoch;
    core.inject_all_dirty(&established, 1, |_| true).await;
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "the safe, previously-injected carrier is synced at gen 1 (marker set)"
    );
    assert_eq!(transport.ops(), vec!["/ws/Foo.vue.tsx=v1".to_string()]);

    // DIRECTLY cache a SAME-generation flip-to-unsafe decision — `true && false` folds
    // fail-closed to `{1, safe:false}`. NO clearing/retract sweep runs, so the `injected`
    // marker stays SET and the content + epoch still match: the shadow consult is now the
    // ONLY thing that can make `is_synced` false.
    core.cache_shadow_decision("/ws/Foo.vue.tsx", run_epoch, 1, false);

    // THE SERVED GATE: a cached-UNSAFE carrier is NOT synced even though its injected marker
    // is STILL set (matching content + epoch). This assertion FAILS if `record_is_synced`
    // drops the `shadow_safety.is_none_or(|c| c.safe)` consult.
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "a cached-UNSAFE carrier is NOT synced even with a set injected marker (matching \
         content + epoch) — never serve SHARED over the overlay shadowing the real user file"
    );

    // Negative control: the direct unsafe cache issued NO retract and cleared NO marker — so
    // the `is_synced=false` verdict above is the shadow consult, not a cleared marker.
    assert!(
        !transport.ops().iter().any(|o| o == "close:/ws/Foo.vue.tsx"),
        "the direct unsafe cache issues NO retract (is_synced=false is the shadow consult, \
         not a cleared marker or a retract)"
    );
}

/// A stale injection driven through the DEAD prior transport cannot mark the carrier
/// synced for the NEW epoch. Distinct from
/// [`stale_inflight_injection_cannot_mark_new_epoch_synced`], which captures the run
/// epoch PRE-reconnect and blocks mid-flight: here the reconnect to epoch B (markers
/// reset, `active_epoch = EB`) completes BEFORE the injection starts, and the injection
/// is driven through the RETAINED dead transport A. Its injection must be attributed to
/// A's epoch (EA), not a re-read of the current `active_epoch` (EB), so the commit
/// recheck against `active_epoch == EB` fails and the carrier stays not-synced.
///
/// RED before the fix: the run epoch is read from `active_epoch` at inject entry (== EB),
/// so the stale A-inject commits an EB-tagged marker → `is_synced` returns TRUE
/// (false-clean, against a Program that never received the carrier).
#[tokio::test]
async fn stale_injection_into_dead_transport_cannot_mark_new_epoch_synced() {
    let core = Arc::new(LazyOverlayCore::<FakeTransport>::new());
    core.record_content("/ws/Foo.vue.tsx", "content");

    // Establish transport A (epoch 1) and retain the identity-bound object.
    let est_a = establish_alive(&core, 1, "nonce-1").await;
    let transport_a = Arc::clone(&est_a.transport);

    // A dies; a reconnect mints transport B (epoch 2) BEFORE the stale A-inject —
    // observing epoch 2 resets the injected markers and advances `active_epoch` to EB.
    transport_a.set_dead();
    let est_b = reestablish_after_death(&core).await;
    let transport_b = Arc::clone(&est_b.transport);
    assert!(
        !Arc::ptr_eq(&transport_a, &transport_b),
        "B is a genuinely fresh transport"
    );

    // NOW inject through the DEAD, retained transport A. The injection is attributed to
    // A's epoch (EA) via the retained identity-bound object, not the current active_epoch
    // (EB).
    core.inject_dirty(&est_a, "/ws/Foo.vue.tsx", 1).await;

    // Discriminating: the stale A-inject must NOT mark the carrier synced for epoch B.
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "a stale injection into the dead prior transport cannot mark the carrier synced \
         for the new epoch (false-clean) — the injection is attributed to A's epoch, not \
         a re-read of the current active_epoch"
    );

    // Negative control: transport B received NOTHING from the stale A-inject.
    assert!(
        !transport_b
            .ops()
            .iter()
            .any(|o| o == "/ws/Foo.vue.tsx=content"),
        "the fresh transport B received NO injection from the stale A-inject"
    );

    // Positive control: a genuine B-epoch injection DOES sync the carrier.
    core.inject_dirty(&est_b, "/ws/Foo.vue.tsx", 1).await;
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "a genuine B-epoch injection syncs the carrier"
    );
}

/// A FIRST injection whose physical overlay LANDED but whose commit is then vetoed
/// shadow-unsafe MUST be retracted, even though its `injected` marker was never committed
/// (so a concurrent sweep's candidate snapshot sees `prev_injected == false`). A leaked
/// physical overlay shadows the now-real user file for OTHER shared-Program consumers.
///
/// RED before the fix: `inject_all_dirty` gates the retract on the stale `prev_injected`
/// snapshot (false for a not-yet-committed first injection) and the vetoed first-inject
/// commit issues no compensating retract, so the physical overlay leaks — `ops()` has the
/// inject but NO `close`.
#[tokio::test]
async fn first_injection_physical_overlay_is_retracted_when_flipped_unsafe() {
    let core = Arc::new(LazyOverlayCore::<FakeTransport>::new());
    core.record_content("/ws/Foo.vue.tsx", "v1");

    // Establish (epoch 1) with an ARMED inject gate: the first injection blocks
    // mid-flight (physically landed, marker NOT yet committed).
    let established = establish_alive(&core, 1, "nonce-1").await;
    let transport = Arc::clone(&established.transport);
    transport.arm_inject_gate();

    // A FIRST safe injection begins on a task; it physically lands the overlay and blocks
    // in the gate BEFORE committing — a concurrent sweep sees no committed marker for it.
    let inj = {
        let core = Arc::clone(&core);
        let est = EstablishedTransport {
            transport: Arc::clone(&transport),
            identity: established.identity.clone(),
        };
        tokio::spawn(async move {
            core.inject_dirty(&est, "/ws/Foo.vue.tsx", 1).await;
        })
    };
    transport.inject_reached.notified().await;

    // A concurrent flip-to-unsafe sweep at the ADVANCED generation 2 caches {safe:false},
    // then blocks on the carrier gate held by the in-flight first injection.
    let sweep = {
        let core = Arc::clone(&core);
        let est = EstablishedTransport {
            transport: Arc::clone(&transport),
            identity: established.identity.clone(),
        };
        tokio::spawn(async move {
            core.inject_all_dirty(&est, 2, |_| false).await;
        })
    };

    // Release the gated first injection: it lands physically, then its commit is vetoed
    // shadow-unsafe by the cached {safe:false}. The physical overlay must be RETRACTED.
    transport.release_inject();
    inj.await.unwrap();
    sweep.await.unwrap();

    // Discriminating: the leaked first-injection overlay is retracted (never left open).
    assert!(
        transport.ops().iter().any(|o| o == "close:/ws/Foo.vue.tsx"),
        "the physically-landed first-injection overlay is RETRACTED when its commit is \
         vetoed shadow-unsafe (never leaked to shadow the real user file)"
    );
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "the flipped-to-unsafe carrier is not synced"
    );
}

/// A RE-inject whose physical inject ERRORS must NOT leak the PRIOR committed overlay.
/// `inject_dirty_bound` clears the prior `run_epoch` marker BEFORE the physical re-inject (so
/// `is_synced` fails closed through the re-inject window); if the inject then errors, the
/// previously-landed physical overlay is STILL open in the shared Program but now UNTRACKED —
/// a later unsafe sweep finds no marker and is inert, and a timed-out close has no
/// compensator. It MUST be compensating-retracted so it cannot linger and shadow a real user
/// file (`carrier_never_shadows_real_user_file`, the R2 failure class).
///
/// RED before the fix (`Err` ⇒ "no physical landing reported — no marker, no retract"): the
/// prior overlay leaks — `ops()` has the first inject but NO `close`. GREEN after: the
/// Err-path issues a bounded compensating retract for the prior overlay whose marker it just
/// cleared. A FIRST injection (no prior overlay) that errors still issues no retract.
#[tokio::test]
async fn reinject_error_retracts_prior_committed_overlay_not_leaks_it() {
    let core = LazyOverlayCore::<FakeTransport>::new();
    core.record_content("/ws/Foo.vue.tsx", "v1");
    let established = establish_alive(&core, 1, "nonce-1").await;
    let transport = Arc::clone(&established.transport);

    // Inject + commit v1 at generation 1: the overlay physically LANDS and the marker is SET.
    core.inject_dirty(&established, "/ws/Foo.vue.tsx", 1).await;
    assert_eq!(
        transport.ops(),
        vec!["/ws/Foo.vue.tsx=v1".to_string()],
        "the first injection physically lands the prior overlay"
    );
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "the committed carrier is synced (a real prior marker is set)"
    );

    // An edit makes the carrier content-dirty; arm inject_fails so the RE-inject ERRORS.
    core.record_content("/ws/Foo.vue.tsx", "v2");
    transport.set_inject_fails(true);

    // RE-inject at the SAME run epoch: the DIRTY gate clears the prior `run_epoch` marker,
    // then the physical inject ERRORS — the prior v1 overlay (now untracked) must be
    // compensating-retracted, not leaked.
    core.inject_dirty(&established, "/ws/Foo.vue.tsx", 1).await;

    assert!(
        transport.ops().iter().any(|o| o == "close:/ws/Foo.vue.tsx"),
        "a RE-inject that errors compensating-retracts the PRIOR committed overlay (never \
         leaks an untracked overlay that could shadow a real user file)"
    );
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "the carrier is not synced after the failed re-inject (the prior marker was cleared)"
    );
}

/// A STALE older-generation UNSAFE sweep that arrives AFTER a newer SAFE decision already
/// committed must be FULLY INERT: it issues NO close and mutates NO marker
/// (`carrier_never_shadows_real_user_file` must not be turned INTO a spurious retract of a
/// since-re-validated overlay). The flip-to-unsafe retract is gated on the EXACT
/// `{generation, safe:false}` decision under the run epoch — a superseded sweep whose
/// generation no longer matches the cached (newer, safe) decision does nothing.
///
/// RED before the fix (`retract_unsafe_bound` gated only on the presence of a run-epoch
/// marker, ignoring the generation/decision): the stale gen-1 unsafe sweep still finds the
/// marker, retracts the freshly-re-validated overlay, and clears the marker — the carrier
/// spuriously drops to not-synced and a `close` is emitted.
#[tokio::test]
async fn superseded_unsafe_sweep_after_newer_safe_commit_is_inert() {
    let core = Arc::new(LazyOverlayCore::<FakeTransport>::new());
    core.record_content("/ws/Foo.vue.tsx", "v1");
    let established = establish_alive(&core, 1, "nonce-1").await;
    let transport = Arc::clone(&established.transport);

    // Inject SAFE at generation 1 → marker committed, shadow {1, safe:true}, synced.
    core.inject_all_dirty(&established, 1, |_| true).await;
    assert!(core.is_synced("/ws/Foo.vue.tsx"));
    assert_eq!(transport.ops(), vec!["/ws/Foo.vue.tsx=v1".to_string()]);

    // A NEWER SAFE decision commits at generation 2 (content clean → no physical re-inject,
    // but the shadow cache advances to {2, safe:true}; the marker stays set).
    core.inject_all_dirty(&established, 2, |_| true).await;
    assert!(core.is_synced("/ws/Foo.vue.tsx"));
    assert_eq!(
        transport.ops().len(),
        1,
        "the gen-2 safe re-check does not re-inject clean content"
    );

    // A STALE older-generation (generation 1) UNSAFE sweep arrives late — superseded by the
    // newer {2, safe:true} decision. It must be FULLY INERT: no close, no marker mutation.
    core.inject_all_dirty(&established, 1, |_| false).await;

    assert!(
        !transport.ops().iter().any(|o| o == "close:/ws/Foo.vue.tsx"),
        "a superseded (older-generation) unsafe sweep issues NO close"
    );
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "a superseded unsafe sweep does not clear the marker (the carrier stays synced)"
    );
}

/// The flip-to-unsafe retract must keep `is_synced` fail-closed THROUGH the physical retract
/// even if a newer SAFE decision arrives mid-retract: the marker is cleared BEFORE the
/// retract await (not after), so a concurrent flip-back-to-safe cache write during the
/// blocked retract cannot resurrect a synced verdict over an overlay that is physically
/// being retracted (no served-false-clean window).
///
/// RED before the fix (marker cleared only AFTER the retract await): during the blocked
/// retract the marker is still set; a newer `{safe:true}` cache write makes `is_synced`
/// return TRUE over the retracting overlay — a served-false-clean window.
#[tokio::test]
async fn newer_safe_cache_during_blocked_retract_leaves_carrier_unsynced() {
    let core = Arc::new(LazyOverlayCore::<FakeTransport>::new());
    core.record_content("/ws/Foo.vue.tsx", "v1");
    let established = establish_alive(&core, 1, "nonce-1").await;
    let transport = Arc::clone(&established.transport);
    let run_epoch = established.identity.epoch;

    // Inject SAFE at gen 1 → marker set, synced.
    core.inject_all_dirty(&established, 1, |_| true).await;
    assert!(core.is_synced("/ws/Foo.vue.tsx"));

    // Arm the retract gate: the flip-to-unsafe sweep's bounded retract blocks mid-await.
    transport.arm_retract_gate();

    // A flip-to-unsafe sweep at gen 2 caches {2, safe:false}, CLEARS the marker BEFORE the
    // retract (fail-closed through the physical retract), then the retract BLOCKS in the gate.
    let flip = {
        let core = Arc::clone(&core);
        let est = EstablishedTransport {
            transport: Arc::clone(&transport),
            identity: established.identity.clone(),
        };
        tokio::spawn(async move {
            core.inject_all_dirty(&est, 2, |_| false).await;
        })
    };
    transport.retract_reached.notified().await;

    // A NEWER SAFE decision arrives DURING the blocked retract (a flip-back-to-safe at the
    // advanced generation 3). Because the marker was cleared BEFORE the retract, is_synced
    // stays FALSE — no served-false-clean window over the physically-retracting overlay.
    core.cache_shadow_decision("/ws/Foo.vue.tsx", run_epoch, 3, true);
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "a newer SAFE cache during the blocked retract must NOT resurrect synced (the marker \
         is cleared before the physical retract) — no served-false-clean window"
    );

    // Release the retract; the sweep completes with NO post-await marker mutation.
    transport.release_retract();
    flip.await.unwrap();
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "the carrier stays unsynced after the retract completes (until a genuine reinjection)"
    );

    // A genuine reinjection at gen 3 (content-dirty via the cleared marker) re-syncs (self-heal).
    core.inject_all_dirty(&established, 3, |_| true).await;
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "a genuine reinjection re-syncs the carrier"
    );
}

/// `observe_transport_identity` is MONOTONIC: it adopts the epoch and resets the injection
/// markers only when `active_epoch` is `None` OR the observed epoch is STRICTLY GREATER than
/// the current one. A stale, delayed observe of an OLDER epoch (a runtime worker preempted
/// between `ensure`'s establish-return and the synchronous observe, while another worker
/// evicts E1 and commits+observes E2) must NOT regress `active_epoch` E2→E1 nor reset the
/// markers.
///
/// RED before the fix (`observe` adopted on ANY inequality `active_epoch != Some(epoch)`):
/// the late `observe(E1)` regresses `active_epoch` to E1 AND resets every marker, so the
/// E2-synced carrier is spuriously dropped to not-synced.
#[tokio::test]
async fn stale_observe_of_older_epoch_does_not_regress_active_epoch_or_reset_markers() {
    let core = Arc::new(LazyOverlayCore::<FakeTransport>::new());
    core.record_content("/ws/Foo.vue.tsx", "v1");

    // Establish transport A (epoch E1), then kill it and reconnect to transport B (epoch E2).
    let est_a = establish_alive(&core, 1, "nonce-1").await;
    let e1 = est_a.identity.epoch;
    est_a.transport.set_dead();
    let est_b = reestablish_after_death(&core).await;
    let e2 = est_b.identity.epoch;
    assert_ne!(e1, e2, "B is a genuinely fresh epoch");

    // Commit a marker under epoch E2 (active_epoch == E2, the carrier is synced).
    core.inject_dirty(&est_b, "/ws/Foo.vue.tsx", 1).await;
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "the carrier is synced under E2"
    );
    assert_eq!(
        core.state.lock().active_epoch,
        Some(e2),
        "active epoch is E2 after the reconnect"
    );

    // A STALE, delayed observe of the OLDER epoch E1 arrives (a preempted worker's late
    // synchronous observe). It must NOT regress active_epoch E2→E1 nor reset the markers.
    core.observe_transport_identity(e1);

    assert_eq!(
        core.state.lock().active_epoch,
        Some(e2),
        "a stale observe of an older epoch must NOT regress the active epoch (E2 preserved)"
    );
    assert!(
        core.is_synced("/ws/Foo.vue.tsx"),
        "a stale observe of an older epoch must NOT reset the injection markers (still synced)"
    );
}

/// The OWNED-close retract (`retract_bounded`) must be ordered w.r.t. a concurrent reopen of
/// the same path: after removing the content it acquires the per-path carrier gate and
/// retracts ONLY IF the path is still ABSENT from the content map. A reopen that re-inserted
/// the content between the close's `remove_content` and its gated revalidation makes THIS
/// close stale — its retract must NOT clobber the reopened overlay.
///
/// RED before the fix (`retract_bounded` removed content then unconditionally retracted,
/// with no gate and no reopen revalidation): the stale close retracts the just-reopened
/// carrier, clobbering the reopen's overlay in the SHARED Program.
#[tokio::test]
async fn retract_bounded_does_not_clobber_a_reopen_that_re_inserted_content() {
    let core = Arc::new(LazyOverlayCore::<FakeTransport>::new());
    core.record_content("/ws/Foo.vue.tsx", "v1");
    let established = establish_alive(&core, 1, "nonce-1").await;
    let transport = Arc::clone(&established.transport);

    // Inject v1 so there is an overlay + a live transport for the close path.
    core.inject_dirty(&established, "/ws/Foo.vue.tsx", 1).await;
    assert_eq!(transport.ops(), vec!["/ws/Foo.vue.tsx=v1".to_string()]);

    // Hold the carrier gate so a concurrent retract_bounded blocks in its gated section AFTER
    // its remove_content but BEFORE the reopen-presence revalidation — the exact
    // close/remove → reopen ordering window.
    let gate = core.carrier_gate("/ws/Foo.vue.tsx");
    let held = gate.lock().await;

    // Spawn the CLOSE: retract_bounded removes the content (sync), then blocks acquiring the
    // (held) carrier gate.
    let close = {
        let core = Arc::clone(&core);
        tokio::spawn(async move {
            core.retract_bounded("/ws/Foo.vue.tsx", Duration::from_secs(5))
                .await;
        })
    };

    // Wait until the close's remove_content has run (the content map no longer holds it) — the
    // close is now parked on the held gate, inside the window.
    let mut spins = 0;
    while core.state.lock().content.contains_key("/ws/Foo.vue.tsx") {
        tokio::task::yield_now().await;
        spins += 1;
        assert!(spins < 100_000, "the close's remove_content did not run");
    }

    // The REOPEN re-inserts content AFTER the close's remove (the exact race). The close must
    // now observe the carrier is present again and SKIP its retract.
    core.record_content("/ws/Foo.vue.tsx", "v2-reopened");

    // Release the gate; the close acquires it, sees content present, and does nothing.
    drop(held);
    close.await.unwrap();

    assert!(
        !transport.ops().iter().any(|o| o == "close:/ws/Foo.vue.tsx"),
        "a stale close whose path was reopened must NOT retract (never clobber the reopen)"
    );

    // The reopened content is intact and re-injects cleanly (self-heal).
    core.inject_dirty(&established, "/ws/Foo.vue.tsx", 1).await;
    assert!(core.is_synced("/ws/Foo.vue.tsx"));
    assert!(
        transport
            .ops()
            .iter()
            .any(|o| o == "/ws/Foo.vue.tsx=v2-reopened"),
        "the reopened overlay is injected"
    );
}

/// `retract_bounded` bounds its ENTIRE physical close — the carrier-gate acquisition AND the
/// retract — by the ORIGINAL close deadline computed ONCE. A carrier gate held by an in-flight
/// inject (or a hanging retract) must NOT let the close exceed its deadline.
///
/// This guards against a regression where the gate acquisition is placed OUTSIDE the
/// deadline (additive/unbounded): with the gate held forever, such an impl blocks the close
/// indefinitely, while the correct impl returns at the deadline.
#[tokio::test]
async fn retract_bounded_respects_the_close_deadline_when_the_gate_is_held() {
    let core = Arc::new(LazyOverlayCore::<FakeTransport>::new());
    core.record_content("/ws/Foo.vue.tsx", "v1");
    let established = establish_alive(&core, 1, "nonce-1").await;
    core.inject_dirty(&established, "/ws/Foo.vue.tsx", 1).await;

    // Hold the carrier gate FOREVER — a concurrent close can never acquire it.
    let gate = core.carrier_gate("/ws/Foo.vue.tsx");
    let held = gate.lock().await;

    let start = tokio::time::Instant::now();
    let close = {
        let core = Arc::clone(&core);
        tokio::spawn(async move {
            core.retract_bounded("/ws/Foo.vue.tsx", Duration::from_millis(150))
                .await;
        })
    };
    // A correct impl returns ~150ms (deadline); a regression acquiring the gate OUTSIDE the
    // deadline blocks forever on the held gate. Bound the observation well below "forever".
    let outcome = tokio::time::timeout(Duration::from_secs(2), close).await;
    assert!(
        outcome.is_ok(),
        "retract_bounded exceeded its close deadline (the held gate acquisition was not bounded)"
    );
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "the held carrier gate must not let the close exceed its original deadline; took {elapsed:?}"
    );
    drop(held);
}

/// Discriminating guard for the in-flight inject's COMMIT VETO, isolated from any concurrent
/// sweep. An inject is blocked AFTER it physically enters the transport; a newer UNSAFE
/// decision is written DIRECTLY (no clearing/retract sweep launched), so the ONLY thing that
/// can retract the physically-landed overlay is the inject's own commit veto. On release the
/// commit must observe the `{safe:false}` decision, refuse to set the marker, and issue
/// EXACTLY ONE compensating close.
///
/// This complements `inflight_inject_does_not_resync_after_concurrent_flip_to_unsafe` (which
/// runs a concurrent flip sweep whose own retract could mask a missing commit veto). Here NO
/// sweep runs, so the close is emitted iff the commit veto fires. It FAILS if the commit veto
/// is removed: without it the release commits the marker and emits NO close — and `is_synced`
/// alone would NOT catch it (the direct `{safe:false}` cache makes `is_synced` false either
/// way), so the EXACTLY-ONE-close assertion is the discriminator.
#[tokio::test]
async fn inflight_inject_commit_veto_retracts_exactly_once_on_direct_unsafe_cache() {
    let core = Arc::new(LazyOverlayCore::<FakeTransport>::new());
    core.record_content("/ws/Foo.vue.tsx", "v1");

    // Establish (epoch 1) with an ARMED inject gate: the inject blocks AFTER it physically
    // enters the transport but BEFORE its commit classification.
    let established = establish_alive(&core, 1, "nonce-1").await;
    let transport = Arc::clone(&established.transport);
    let run_epoch = established.identity.epoch;
    transport.arm_inject_gate();

    // The inject begins on a task; it physically lands the overlay and blocks in the gate
    // BEFORE committing.
    let inj = {
        let core = Arc::clone(&core);
        let est = EstablishedTransport {
            transport: Arc::clone(&transport),
            identity: established.identity.clone(),
        };
        tokio::spawn(async move {
            core.inject_dirty(&est, "/ws/Foo.vue.tsx", 1).await;
        })
    };
    transport.inject_reached.notified().await;

    // DIRECTLY cache a newer UNSAFE decision — NO clearing/retract sweep is launched, so the
    // ONLY thing that can retract the physically-landed overlay is the inject's own commit
    // veto.
    core.cache_shadow_decision("/ws/Foo.vue.tsx", run_epoch, 2, false);

    // Release the gated inject: its commit observes the {2, safe:false} decision, refuses to
    // set the marker, and issues EXACTLY ONE compensating retract.
    transport.release_inject();
    inj.await.unwrap();

    assert_eq!(
        transport
            .ops()
            .iter()
            .filter(|o| o.as_str() == "close:/ws/Foo.vue.tsx")
            .count(),
        1,
        "the vetoed in-flight inject issues EXACTLY ONE compensating close (the commit veto)"
    );
    assert!(
        !core.is_synced("/ws/Foo.vue.tsx"),
        "the vetoed carrier is not synced (no marker committed)"
    );
}

/// The carrier-gate registry is pruned of DEAD weak entries on `remove_content` too, not only
/// when a fresh gate is minted — so close churn cannot leak dead registry entries between
/// mints.
///
/// RED before the fix (`remove_content` dropped the content record but never swept the gate
/// registry): the dead gate entry left by a completed inject survives the close.
#[tokio::test]
async fn remove_content_prunes_dead_carrier_gate_entries() {
    let core = LazyOverlayCore::<FakeTransport>::new();
    core.record_content("/ws/A.vue.tsx", "v1");
    let established = establish_alive(&core, 1, "nonce-1").await;

    // An inject mints a carrier gate and drops it at the end of the transaction, leaving a
    // DEAD weak entry in the registry.
    core.inject_dirty(&established, "/ws/A.vue.tsx", 1).await;
    assert_eq!(
        core.carrier_gates.lock().len(),
        1,
        "the injected carrier left a (now dead) gate registry entry"
    );

    // remove_content prunes the dead entry (not only a fresh gate mint does).
    core.remove_content("/ws/A.vue.tsx");
    assert_eq!(
        core.carrier_gates.lock().len(),
        0,
        "remove_content prunes dead carrier-gate registry entries"
    );
}

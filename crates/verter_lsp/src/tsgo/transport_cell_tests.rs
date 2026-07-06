//! Discriminating unit tests for [`LazyTransport`] — the singleflight + bounded +
//! re-arming + live-death-EVICTING establishment state machine, driven with a FAKE
//! establisher (no engine, no real transport, no real hang).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::LazyTransport;

/// A trivial transport payload.
#[derive(Debug, PartialEq, Eq)]
struct FakeTransport(u32);

/// A fixed generation probe.
fn gen_const(g: Option<&str>) -> impl Fn() -> Option<String> + '_ {
    move || g.map(str::to_string)
}

/// A liveness probe that always reports alive — the default for tests not exercising
/// the live-death eviction path.
fn always_alive(_t: &FakeTransport) -> bool {
    true
}

/// FAIL-CLOSED: a slow establishment that never completes within the bound yields
/// NO transport within the bound — the caller fails closed instead of stalling.
#[tokio::test]
async fn slow_establishment_times_out_fail_closed() {
    let cell: LazyTransport<FakeTransport> = LazyTransport::new();
    let started = tokio::time::Instant::now();
    let established = cell
        .get_or_establish(
            gen_const(Some("gen-1")),
            always_alive,
            || async {
                // Far longer than the bound below — models a never-initializing editor.
                tokio::time::sleep(Duration::from_secs(30)).await;
                Some(Arc::new(FakeTransport(1)))
            },
            Duration::from_millis(50),
        )
        .await;
    assert!(
        established.is_none(),
        "a slow establishment must yield NO transport within the bound (fail closed)"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "establishment must fail closed at its small bound, not run to completion"
    );
    // And `current()` reflects no live transport.
    assert!(cell.current().await.is_none());
}

/// SINGLEFLIGHT: N concurrent demands establish the transport EXACTLY ONCE and all
/// observe the same established transport — never N establishments.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_demands_establish_exactly_once() {
    let cell: Arc<LazyTransport<FakeTransport>> = Arc::new(LazyTransport::new());
    let establish_count = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();
    for _ in 0..16 {
        let cell = Arc::clone(&cell);
        let count = Arc::clone(&establish_count);
        handles.push(tokio::spawn(async move {
            cell.get_or_establish(
                gen_const(Some("gen-1")),
                always_alive,
                || async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    // A small establishment delay to force overlap.
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    Some(Arc::new(FakeTransport(7)))
                },
                Duration::from_secs(5),
            )
            .await
        }));
    }
    let mut results = Vec::new();
    for h in handles {
        results.push(h.await.unwrap());
    }

    assert_eq!(
        establish_count.load(Ordering::SeqCst),
        1,
        "16 concurrent demands must establish EXACTLY ONCE (singleflight), never N times"
    );
    assert!(
        results
            .iter()
            .all(|r| matches!(r.as_deref(), Some(FakeTransport(7)))),
        "every concurrent demand observes the one established transport"
    );
}

/// REUSE: once established, subsequent demands return the same transport WITHOUT
/// re-establishing.
#[tokio::test]
async fn established_transport_is_reused() {
    let cell: LazyTransport<FakeTransport> = LazyTransport::new();
    let count = Arc::new(AtomicUsize::new(0));

    for _ in 0..3 {
        let count = Arc::clone(&count);
        let t = cell
            .get_or_establish(
                gen_const(Some("gen-1")),
                always_alive,
                || async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Some(Arc::new(FakeTransport(42)))
                },
                Duration::from_secs(5),
            )
            .await;
        assert_eq!(t.as_deref(), Some(&FakeTransport(42)));
    }
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "a live transport is reused — establishment runs once, not per demand"
    );
}

/// One establishment attempt at `generation` that succeeds or fails, counting each
/// time the establisher actually runs.
async fn run_attempt(
    cell: &LazyTransport<FakeTransport>,
    generation: &str,
    succeed: bool,
    count: &Arc<AtomicUsize>,
) -> Option<Arc<FakeTransport>> {
    let generation = generation.to_string();
    let count = Arc::clone(count);
    cell.get_or_establish(
        move || Some(generation.clone()),
        always_alive,
        move || async move {
            count.fetch_add(1, Ordering::SeqCst);
            if succeed {
                Some(Arc::new(FakeTransport(1)))
            } else {
                None
            }
        },
        Duration::from_secs(5),
    )
    .await
}

/// RE-ARM: a transient establishment failure does NOT retry within the SAME
/// generation (no handshake retry-storm), but DOES re-attempt when the observed
/// generation ADVANCES (a fresh advertisement / editor generation).
#[tokio::test]
async fn failed_establishment_rearms_only_on_generation_advance() {
    let cell: LazyTransport<FakeTransport> = LazyTransport::new();
    let count = Arc::new(AtomicUsize::new(0));

    // Generation 1: a transient failure.
    assert!(run_attempt(&cell, "gen-1", false, &count).await.is_none());
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "the first attempt establishes once"
    );

    // Same generation: NO retry (fail closed without re-attempting — no retry-storm).
    assert!(run_attempt(&cell, "gen-1", true, &count).await.is_none());
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "a repeat within the SAME failed generation must NOT re-attempt establishment"
    );

    // Advanced generation (a reconnect): RE-ARM and re-attempt — now succeeds.
    let live = run_attempt(&cell, "gen-2", true, &count).await;
    assert_eq!(
        live.as_deref(),
        Some(&FakeTransport(1)),
        "a fresh generation must re-arm and establish"
    );
    assert_eq!(
        count.load(Ordering::SeqCst),
        2,
        "the advanced generation re-attempts establishment exactly once more"
    );
}

/// Transport live-death eviction. A `Live` transport that goes DEAD (a relay
/// `verter/fatal` / a closed connection) must be EVICTED on the next demand: the query
/// fails closed to OWNED (`None`) WITHOUT stalling and WITHOUT re-establishing within
/// the same failed generation (no storm), then RE-ESTABLISHES on a demand at an
/// ADVANCED generation (the shim reconnected) per the existing re-arm discriminant.
///
/// RED before the fix: a `Live` transport is returned forever regardless of liveness —
/// the composite occupies a dead shared provider until LSP restart (post-death queries
/// keep hitting the dead path instead of a clean OWNED fall-through).
#[tokio::test]
async fn dead_live_transport_is_evicted_and_rearms() {
    let cell: LazyTransport<FakeTransport> = LazyTransport::new();
    let alive = Arc::new(AtomicBool::new(true));
    let establish_count = Arc::new(AtomicUsize::new(0));

    let liveness = {
        let alive = Arc::clone(&alive);
        move |_t: &FakeTransport| alive.load(Ordering::SeqCst)
    };

    // Establish a Live transport at gen-1.
    let ec = Arc::clone(&establish_count);
    let live = cell
        .get_or_establish(
            gen_const(Some("gen-1")),
            liveness.clone(),
            || async move {
                ec.fetch_add(1, Ordering::SeqCst);
                Some(Arc::new(FakeTransport(1)))
            },
            Duration::from_secs(5),
        )
        .await;
    assert_eq!(live.as_deref(), Some(&FakeTransport(1)));
    assert_eq!(establish_count.load(Ordering::SeqCst), 1);

    // The transport DIES (relay fatal / closed connection).
    alive.store(false, Ordering::SeqCst);

    // The next demand at the SAME generation must EVICT the dead Live and fail closed
    // (`None`) — no stall, no dead transport returned, and NO re-establishment within
    // the same failed generation (the shim has not republished a fresh advertisement).
    let started = tokio::time::Instant::now();
    let ec = Arc::clone(&establish_count);
    let after_death = cell
        .get_or_establish(
            gen_const(Some("gen-1")),
            liveness.clone(),
            || async move {
                ec.fetch_add(1, Ordering::SeqCst);
                Some(Arc::new(FakeTransport(2)))
            },
            Duration::from_secs(5),
        )
        .await;
    assert!(
        after_death.is_none(),
        "a dead Live transport must be EVICTED and fail closed to OWNED (None), never returned"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "eviction must fail closed promptly (no stall)"
    );
    assert_eq!(
        establish_count.load(Ordering::SeqCst),
        1,
        "eviction fails closed WITHOUT re-establishing within the same failed generation (no storm)"
    );
    // `current()` must also reflect the eviction — the dead Live is gone.
    assert!(
        cell.current().await.is_none(),
        "current() must not hand out the evicted dead transport"
    );

    // The shim reconnects (a FRESH advertisement/editor generation). The transport
    // re-establishes per the existing generation re-arm discriminant.
    alive.store(true, Ordering::SeqCst);
    let ec = Arc::clone(&establish_count);
    let reestablished = cell
        .get_or_establish(
            gen_const(Some("gen-2")),
            liveness.clone(),
            || async move {
                ec.fetch_add(1, Ordering::SeqCst);
                Some(Arc::new(FakeTransport(3)))
            },
            Duration::from_secs(5),
        )
        .await;
    assert_eq!(
        reestablished.as_deref(),
        Some(&FakeTransport(3)),
        "a fresh generation re-establishes after eviction"
    );
    assert_eq!(
        establish_count.load(Ordering::SeqCst),
        2,
        "re-establishment ran exactly once more on the advanced generation"
    );
}

/// The transport-cell-poisoning discriminator. A no-binding carrier
/// (`NoProject` / `Ambiguous` / `SyntheticScratch`, or a not-yet-ready published
/// snapshot) is modeled as `bound = None`. It must serve the baseline (`None`)
/// WITHOUT entering the singleflight cell (establishment never runs), so a
/// SUBSEQUENT bindable carrier at the SAME generation still establishes SHARED —
/// the no-binding miss never poisons the carrier-INDEPENDENT transport.
///
/// RED before the gate: routing the no-binding miss THROUGH the cell records
/// `Unavailable` at the shared (nonce, generation) discriminant, and the bindable
/// demand at that same discriminant is then denied (SHARED silently never engages).
#[tokio::test]
async fn no_binding_never_enters_cell_and_never_poisons() {
    let cell: LazyTransport<FakeTransport> = LazyTransport::new();
    let count = Arc::new(AtomicUsize::new(0));
    // The composite's re-arm discriminant is (advertisement nonce, config
    // generation). A no-binding carrier and the subsequent bindable carrier are
    // resolved from the SAME published snapshot, so they share ONE (nonce,
    // generation) discriminant — the no-binding demand CARRIES the same discriminant
    // the bindable demand re-checks. The probe returns that shared discriminant
    // independent of its argument (both demands are at generation `GEN`), so IF the
    // production code (wrongly) routed the no-binding miss THROUGH the cell it would
    // stamp `Unavailable` at the EXACT discriminant the bindable demand then re-checks
    // — the bindable would be BLOCKED (Unavailable, never advancing past a stale
    // failure), reddening this test. With a probe that VARIED by generation the
    // no-binding poison would land at a discriminant the bindable always advanced past,
    // and the poison would go undetected — the discrimination gap this fixture closes.
    const GEN: u64 = 5;
    let probe = |_g: u64| Some(format!("nonce-1\u{1f}{GEN}"));

    // A no-binding carrier at generation GEN → baseline (None), cell UNTOUCHED.
    let c = Arc::clone(&count);
    let r1 = cell
        .get_or_establish_bound(
            None::<((), u64)>,
            probe,
            always_alive,
            move |_b: (), _g| {
                let c = Arc::clone(&c);
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Some(Arc::new(FakeTransport(1)))
                }
            },
            Duration::from_secs(5),
        )
        .await;
    assert!(
        r1.is_none(),
        "a no-binding carrier serves the baseline (None)"
    );
    assert_eq!(
        count.load(Ordering::SeqCst),
        0,
        "establishment must NOT run for a non-binding carrier (the cell is not entered)"
    );

    // A subsequent BINDABLE carrier at the SAME generation GEN STILL establishes —
    // the no-binding miss did not poison the carrier-independent transport. (If it
    // had, this demand's discriminant `probe(GEN)` would equal the poisoned failed
    // discriminant and re-arm would be denied — see the fixture comment above.)
    let c = Arc::clone(&count);
    let r2 = cell
        .get_or_establish_bound(
            Some(((), GEN)),
            probe,
            always_alive,
            move |_b: (), _g| {
                let c = Arc::clone(&c);
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Some(Arc::new(FakeTransport(7)))
                }
            },
            Duration::from_secs(5),
        )
        .await;
    assert_eq!(
        r2.as_deref(),
        Some(&FakeTransport(7)),
        "a bindable carrier engages SHARED — the prior no-binding miss did NOT poison the cell"
    );
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "establishment ran exactly once — for the bindable demand only"
    );
}

/// A REAL handshake failure (a binding IS present, but establishment
/// returns `None`) DOES cache `Unavailable`, and re-arms ONLY on a
/// (nonce, generation) advance: (c) a repeat at the SAME (nonce, generation) does
/// not re-attempt (no handshake retry-storm), and (b) a later CONFIG GENERATION
/// re-arms even under the SAME shim nonce (the generation is part of the
/// discriminant — a fresh published snapshot retries a prior transient miss).
#[tokio::test]
async fn real_failure_rearms_only_on_generation_or_nonce_advance() {
    let cell: LazyTransport<FakeTransport> = LazyTransport::new();
    let count = Arc::new(AtomicUsize::new(0));
    let probe = |g: u64| Some(format!("nonce-1\u{1f}{g}"));

    // A REAL handshake failure at generation 5 (binding present, establish → None).
    let c = Arc::clone(&count);
    let r1 = cell
        .get_or_establish_bound(
            Some(((), 5u64)),
            probe,
            always_alive,
            move |_b: (), _g| {
                let c = Arc::clone(&c);
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    None::<Arc<FakeTransport>>
                }
            },
            Duration::from_secs(5),
        )
        .await;
    assert!(r1.is_none(), "a real handshake failure yields no transport");
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "the first attempt runs once"
    );

    // (c) The SAME (nonce, generation): NO retry (no handshake storm).
    let c = Arc::clone(&count);
    let r2 = cell
        .get_or_establish_bound(
            Some(((), 5u64)),
            probe,
            always_alive,
            move |_b: (), _g| {
                let c = Arc::clone(&c);
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Some(Arc::new(FakeTransport(1)))
                }
            },
            Duration::from_secs(5),
        )
        .await;
    assert!(
        r2.is_none(),
        "a repeat at the SAME (nonce, generation) must NOT re-attempt establishment"
    );
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "no re-attempt within the same (nonce, generation) — no retry-storm"
    );

    // (b) A later CONFIG GENERATION (same shim nonce) re-arms establishment.
    let c = Arc::clone(&count);
    let r3 = cell
        .get_or_establish_bound(
            Some(((), 6u64)),
            probe,
            always_alive,
            move |_b: (), _g| {
                let c = Arc::clone(&c);
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Some(Arc::new(FakeTransport(9)))
                }
            },
            Duration::from_secs(5),
        )
        .await;
    assert_eq!(
        r3.as_deref(),
        Some(&FakeTransport(9)),
        "a fresh config generation re-arms even under the SAME shim nonce"
    );
    assert_eq!(
        count.load(Ordering::SeqCst),
        2,
        "the advanced generation re-attempts establishment exactly once more"
    );
}

/// A missing generation (`None` — no advertisement observable) does NOT re-arm after a
/// failure, so a flapping / absent advertisement never storms establishment.
#[tokio::test]
async fn missing_generation_does_not_rearm() {
    let cell: LazyTransport<FakeTransport> = LazyTransport::new();
    let count = Arc::new(AtomicUsize::new(0));

    // Fail at Some("gen-1").
    let count_first = Arc::clone(&count);
    assert!(cell
        .get_or_establish(
            gen_const(Some("gen-1")),
            always_alive,
            || async move {
                count_first.fetch_add(1, Ordering::SeqCst);
                None::<Arc<FakeTransport>>
            },
            Duration::from_secs(5),
        )
        .await
        .is_none());

    // A subsequent demand where NO generation is observable (`None`) must NOT re-arm.
    let count_second = Arc::clone(&count);
    assert!(cell
        .get_or_establish(
            gen_const(None),
            always_alive,
            || async move {
                count_second.fetch_add(1, Ordering::SeqCst);
                Some(Arc::new(FakeTransport(1)))
            },
            Duration::from_secs(5),
        )
        .await
        .is_none());
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "an absent generation must not re-arm a prior failure (no storm)"
    );
}

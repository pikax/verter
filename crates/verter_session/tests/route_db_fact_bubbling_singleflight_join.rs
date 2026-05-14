//! RED test (closed by same-block implementation): a thread that joins
//! a route singleflight as a follower bubbles the leader's
//! `fact_dep_signature` into the follower's outer tracer scope, and
//! advances `route_coalesced_fact_bubble_emissions` exactly once on
//! the follower-role re-read.
//!
//! ## Discrimination contract
//!
//! This test FAILS unless ALL of the following are wired end-to-end:
//!
//! 1. The cold path of `get_or_resolve_route_observing_facts` runs
//!    the singleflight inline (rather than delegating to a helper
//!    that hides the role), so the follower role is visible at the
//!    re-read site.
//! 2. The follower-role re-read returns the leader's just-admitted
//!    entry and fans its facts via `observe_fact_signature`.
//! 3. The follower-role branch bumps
//!    `route_coalesced_fact_bubble_emissions` exactly once on that
//!    re-read.
//!
//! Reverting site (1) makes counter-delta assertion fail because the
//! role is lost. Reverting site (2) makes the tracer assertion fail.
//! Reverting site (3) makes the counter-delta assertion fail (the
//! cold counter would still advance, but the coalesced one wouldn't).
//!
//! ## Why this could not be expressed pre-cutover
//!
//! Before the consumer migration, the route cold path was reached
//! via `get_or_resolve_route_with_facts` which discarded the
//! singleflight `role`. The follower thread's call returned the
//! coalesced result Arc without observing or bubbling — the
//! follower's outer tracer would finalise empty even when the
//! leader admitted a non-empty signature. This test orchestrates a
//! leader+follower with explicit synchronisation, then asserts the
//! follower's outer tracer captures the leader's fact.
//!
//! ## Driver shape
//!
//! - **Leader thread** enters `get_or_resolve_route_observing_facts`
//!   on the shared key `K`. Its resolve closure blocks on an mpsc
//!   channel until the follower has had time to register as a
//!   waiter, then returns a `(RouteResult::Resolved, [fact])` pair.
//!   The admission stores the fact, the leader-role re-read advances
//!   the cold counter, and the follower wakes.
//! - **Follower thread** installs its own outer fact tracer, then
//!   enters `get_or_resolve_route_observing_facts` on the same key.
//!   Because the leader has already claimed the in-flight singleflight
//!   slot, the follower blocks on the singleflight condvar — its
//!   resolve closure is never called. When the follower wakes, its
//!   re-read sees the leader's just-admitted entry, fans the leader's
//!   facts into the follower's outer tracer, and bumps the coalesced
//!   counter.

use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use verter_session::for_tests::install_fact_tracer_for_tests;
use verter_session::resolver_core::{
    FactReadSetFinalise, FactVersionRef, PermissiveStoreView, RouteDb, RouteResult,
};
use verter_session::VerterHost;

/// Build the leader's fact: a `FileWholeHash` with a recognisable
/// 16-byte pattern. The same fact value re-appears in the follower's
/// outer tracer when the coalesced-join bubble fires.
fn leader_fact() -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: "join_dep.ts".to_string(),
        hash: [0x33u8; 16],
    }
}

fn resolved_route() -> RouteResult {
    RouteResult::Resolved {
        defining_canonical: "join_dep.ts".to_string(),
        defining_symbol: "JoinExport".to_string(),
    }
}

#[test]
fn follower_bubbles_leader_facts_and_advances_coalesced_counter() {
    let db = Arc::new(RouteDb::new());

    // Channels coordinate the interleave. The leader signals when it
    // has claimed the in-flight singleflight slot and entered its
    // resolve closure; the driver releases the leader only after the
    // follower has had time to register as a waiter.
    let (tx_leader_in_closure, rx_leader_in_closure) = mpsc::channel::<()>();
    let (tx_release_leader, rx_release_leader) = mpsc::channel::<()>();

    let leader_db = Arc::clone(&db);
    let leader = thread::spawn(move || {
        let view = PermissiveStoreView;
        leader_db.get_or_resolve_route_observing_facts("join_provider.ts", "Joined", &view, || {
            // We have claimed the singleflight slot and are
            // inside the resolve closure. Signal the driver, then
            // wait for the release before publishing.
            tx_leader_in_closure
                .send(())
                .expect("leader: signal in-closure");
            rx_release_leader
                .recv()
                .expect("leader: released by driver");
            Some((resolved_route(), vec![leader_fact()]))
        })
    });

    // Wait until the leader is inside its resolve closure. This
    // guarantees the follower reaches the singleflight condvar-wait
    // branch rather than executing its own cold resolve.
    rx_leader_in_closure
        .recv()
        .expect("leader entered resolve closure");

    // Spawn the follower. It installs its own outer tracer BEFORE
    // entering the observing entry-point so the post-admission
    // bubble has a tracer to deliver the leader's facts to.
    let follower_db = Arc::clone(&db);
    let follower = thread::spawn(move || {
        let host = VerterHost::new_standalone(Default::default());
        let view = PermissiveStoreView;
        let (route_result, finalise) = install_fact_tracer_for_tests(&host, || {
            follower_db.get_or_resolve_route_observing_facts(
                "join_provider.ts",
                "Joined",
                &view,
                || {
                    // The follower's resolve closure MUST NOT run.
                    // If it does, the test isn't exercising the
                    // singleflight-join path. Use `unreachable!` so
                    // a regression panics with a clear message.
                    unreachable!(
                        "follower's resolve closure must not run — \
                         singleflight should join the leader's flight"
                    )
                },
            )
        });
        (route_result, finalise)
    });

    // Give the follower time to register on the in-flight slot and
    // block on the singleflight condvar. 50 ms matches the heuristic
    // in `cross_thread_joiner_bubbles_facts.rs`. If a race ever
    // surfaces under load, the discriminator below will catch it
    // (counter stays at zero or follower runs its own resolve).
    thread::sleep(Duration::from_millis(50));

    // Release the leader. It publishes the result, admits the entry
    // (with the fact signature), notifies the singleflight condvar,
    // and bumps the cold counter on its re-read. The follower wakes,
    // re-reads the entry, fans the leader's facts into its outer
    // tracer, and bumps the coalesced counter.
    tx_release_leader.send(()).expect("release leader");

    let leader_result = leader.join().expect("leader joined");
    let (follower_result, follower_finalise) = follower.join().expect("follower joined");

    assert!(leader_result.is_some(), "leader must return Some");
    assert!(follower_result.is_some(), "follower must return Some");

    // The coalesced counter must have advanced exactly once for the
    // follower's coalesced join. The cold counter must have advanced
    // exactly once for the leader.
    assert_eq!(
        db.route_cold_fact_bubble_emissions(),
        1,
        "leader must have bumped `route_cold_fact_bubble_emissions` \
         exactly once on its leader-role re-read; got {}",
        db.route_cold_fact_bubble_emissions()
    );
    assert_eq!(
        db.route_coalesced_fact_bubble_emissions(),
        1,
        "follower must have bumped \
         `route_coalesced_fact_bubble_emissions` exactly once on its \
         follower-role re-read. If this counter is zero, the follower \
         either bypassed the singleflight (ran its own resolve — would \
         have hit the unreachable! above) OR did not re-read after \
         joining. got {}",
        db.route_coalesced_fact_bubble_emissions()
    );

    // The follower's outer tracer must contain the leader's fact.
    // This is the discriminator: it FAILS if the follower-role
    // re-read is missing or the bubble call is skipped.
    let want = leader_fact();
    match follower_finalise {
        FactReadSetFinalise::Ok(sig) => {
            assert!(
                sig.iter().any(|f| f == &want),
                "follower's outer tracer MUST contain the leader's \
                 fact (expected to contain {want:?}; got {sig:?}). \
                 If this fails, the follower-role re-read either did \
                 not find the leader's admitted entry OR did not call \
                 `observe_fact_signature` on the fanned-out facts."
            );
        }
        FactReadSetFinalise::Overflow => panic!("follower outer tracer overflowed"),
    }
}

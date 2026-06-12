//! ReturnOnly never publishes — ROUTE-SINGLEFLIGHT rendezvous arm. A
//! route resolve that the strict admission refused to persist (the
//! empty-fact-signature carrier: the fenced-walk arm of
//! `build_named_type_export_route_entry`, or an unrootable-wildcard
//! negative-cache resolve) must NOT be handed to burst followers as a
//! joinable rendezvous. The unadmitted value was served to the leader's
//! own caller only (its request pre-dates whatever made the result
//! unrootable); a follower that adopted it would receive a
//! possibly-superseded route with neither a fact signature to bubble
//! into its outer tracer nor any ReturnOnly signal on its own request.
//!
//! ## Discrimination contract
//!
//! Pre-fix, the route cold path ran under the retain-always
//! singleflight `run`, so the leader's unadmitted result was retained
//! as a joinable `Done`: the follower adopted the superseded route and
//! its own resolve closure never ran. Post-fix, retention mirrors
//! admission — the follower receives the unadmitted outcome BY VALUE,
//! re-elects itself leader on a fresh lane, runs its OWN resolve
//! against fresh state, and admits the live result.
//!
//! ## Driver shape
//!
//! - **Leader thread** enters `get_or_resolve_route_observing_facts`
//!   on the shared key. Its resolve closure parks on a channel until
//!   the follower is committed to the singleflight wait, then returns
//!   the never-persisted `(route, Vec::new())` empty-facts shape — the
//!   exact carrier the fenced frontier walk produces.
//! - **Follower thread** enters the same key while the leader is
//!   parked and blocks on the singleflight condvar. Its resolve
//!   closure returns the LIVE route with a non-empty fact signature.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use verter_session::resolver_core::{FactVersionRef, PermissiveStoreView, RouteDb, RouteResult};

/// Strong-reference count of the route singleflight in-flight entry
/// while only the leader is parked inside its resolve closure: the
/// leader's local `state` binding plus the `flights` map entry. A
/// follower that has joined and is committed to the condvar wait raises
/// the count by one (to 3).
const LEADER_ONLY_INFLIGHT_REFS: usize = 2;

/// Block the calling (driver) thread until a follower has been admitted
/// onto the route singleflight in-flight entry for `(provider, name)`.
/// Bounded: spins for at most ~10 s, then panics so a genuine hang
/// fails loudly rather than blocking the suite forever.
fn wait_for_route_follower_admitted<V>(db: &RouteDb, provider: &str, name: &str, view: &V)
where
    V: verter_session::resolver_core::StoreView + ?Sized,
{
    let deadline = Instant::now() + Duration::from_secs(10);
    while db.test_route_inflight_strong_count(provider, name, view) <= LEADER_ONLY_INFLIGHT_REFS {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the follower to be admitted onto the \
             route singleflight in-flight entry (strong count never \
             exceeded {LEADER_ONLY_INFLIGHT_REFS})",
        );
        thread::sleep(Duration::from_millis(1));
    }
}

/// Join `handle` and return its value, panicking if it does not
/// complete within ~10 s — a genuine singleflight deadlock must surface
/// loudly rather than hang the suite.
fn join_within<T: Send + 'static>(handle: thread::JoinHandle<T>, label: &str) -> T {
    let (tx, rx) = mpsc::sync_channel::<thread::Result<T>>(1);
    thread::spawn(move || {
        let _ = tx.send(handle.join());
    });
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(value)) => value,
        Ok(Err(_)) => panic!("{label} panicked"),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("{label} deadlocked (join did not complete within 10s)")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("{label} watchdog channel disconnected before reporting")
        }
    }
}

/// The route the parked leader serves: computed from a superseded
/// surface, refused admission (empty fact signature).
fn superseded_route() -> RouteResult {
    RouteResult::Resolved {
        defining_canonical: "superseded_dep.ts".to_string(),
        defining_symbol: "Burst".to_string(),
    }
}

/// The route a fresh resolve against live state produces, with the fact
/// signature that admits it.
fn live_route() -> RouteResult {
    RouteResult::Resolved {
        defining_canonical: "live_dep.ts".to_string(),
        defining_symbol: "Burst".to_string(),
    }
}

fn live_fact() -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: "live_dep.ts".to_string(),
        hash: [0x44u8; 16],
    }
}

#[test]
fn burst_follower_reresolves_instead_of_adopting_unadmitted_route() {
    let db = Arc::new(RouteDb::new());

    let (tx_leader_in_closure, rx_leader_in_closure) = mpsc::channel::<()>();
    let (tx_release_leader, rx_release_leader) = mpsc::channel::<()>();

    let leader_db = Arc::clone(&db);
    let leader = thread::spawn(move || {
        let view = PermissiveStoreView;
        leader_db.get_or_resolve_route_observing_facts("burst_provider.ts", "Burst", &view, || {
            tx_leader_in_closure
                .send(())
                .expect("leader: signal in-closure");
            match rx_release_leader.recv_timeout(Duration::from_secs(10)) {
                Ok(()) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    panic!("leader: timed out waiting for driver release (10s)")
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    panic!("leader: driver dropped the release channel before releasing")
                }
            }
            // The never-persisted empty-facts shape — the carrier the
            // fenced frontier walk hands the route singleflight.
            Some((superseded_route(), Vec::new()))
        })
    });

    match rx_leader_in_closure.recv_timeout(Duration::from_secs(10)) {
        Ok(()) => {}
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("timed out waiting for the leader to enter its resolve closure (10s)")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("leader thread dropped the in-closure channel before signalling")
        }
    }

    // The follower commits to the leader's in-flight lane BEFORE the
    // leader publishes — a genuine burst member.
    let follower_resolves = Arc::new(AtomicUsize::new(0));
    let follower_db = Arc::clone(&db);
    let follower_resolves_in_closure = Arc::clone(&follower_resolves);
    let follower = thread::spawn(move || {
        let view = PermissiveStoreView;
        follower_db.get_or_resolve_route_observing_facts(
            "burst_provider.ts",
            "Burst",
            &view,
            move || {
                follower_resolves_in_closure.fetch_add(1, Ordering::SeqCst);
                Some((live_route(), vec![live_fact()]))
            },
        )
    });

    let probe_view = PermissiveStoreView;
    wait_for_route_follower_admitted(&db, "burst_provider.ts", "Burst", &probe_view);
    tx_release_leader.send(()).expect("release leader");

    let leader_result = join_within(leader, "leader");
    let follower_result = join_within(follower, "follower");

    // The leader's own caller is still served the unadmitted result —
    // its request pre-dates whatever superseded the walk.
    assert_eq!(
        leader_result.as_deref(),
        Some(&superseded_route()),
        "the leader must still be served its own resolve's route",
    );

    // THE PIN: the follower must NOT adopt the unadmitted (empty-facts)
    // result as a joinable rendezvous — it re-resolves against fresh
    // state and returns the live route.
    assert_eq!(
        follower_resolves.load(Ordering::SeqCst),
        1,
        "the burst follower must re-run its OWN resolve instead of \
         adopting the leader's unadmitted route (0 resolves means the \
         never-persisted result was handed out as a rendezvous)",
    );
    assert_eq!(
        follower_result.as_deref(),
        Some(&live_route()),
        "the burst follower must return its own fresh resolve's route, \
         not the leader's unadmitted (possibly superseded) one",
    );

    // The follower's re-resolve carried a non-empty fact signature, so
    // it WAS admitted — the next read serves it warm.
    let warm = db.get_route("burst_provider.ts", "Burst", &probe_view);
    assert_eq!(
        warm.as_deref(),
        Some(&live_route()),
        "the follower's admitted re-resolve must serve the next read warm",
    );
}

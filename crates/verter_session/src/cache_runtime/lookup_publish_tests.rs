//! Discriminators for the split-lifecycle lookup-publish adapter
//! (`cooperative_admit_with_lookup_publish`).
//!
//! Two families:
//!   * basic three-way `ComputeAdmission` + revalidation contract
//!     (publish_core invoked once on Cacheable, never on ReturnOnly /
//!     revalidation-failure);
//!   * the split-lifecycle discriminators driven over a real
//!     [`ReverseIndexedCandidateStore`]: NO-DEADLOCK (a budgeted
//!     publish_core + deferred eviction re-enters the store guard-free)
//!     and RETENTION-vs-CLEAR ATOMICITY (the publish fence spans
//!     publish_core → evict_deferred, so a `clear` cannot interleave).

use super::*;
use crate::cache_runtime::admission::{Candidate, FactCandidateDiscriminant};
use crate::cache_runtime::candidate_store::ReverseIndexedCandidateStore;
use crate::cache_runtime::singleflight::{ComputeAdmission, InflightTable};
use crate::fact_signature_helpers::ReadSetSignature;
use crate::resolver_core::FactVersionRef;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Basic three-way contract — `Victims = ()`, no fence, no real store.
// ---------------------------------------------------------------------------

/// A `ReturnOnly` winner publishes NOTHING — `publish_core` is never
/// invoked and the store is never written. The cross-view joiner forks
/// and recomputes. Preserves the three-way `ComputeAdmission` contract
/// while keeping storage out of the singleflight protocol.
#[test]
fn lookup_publish_return_only_never_publishes() {
    let inflight: InflightTable<u32> = InflightTable::default();
    let publish_count = Arc::new(AtomicUsize::new(0));
    let publish_count_cl = Arc::clone(&publish_count);

    let v = cooperative_admit_with_lookup_publish(
        &inflight,
        1u32,
        || None::<String>,
        || ComputeAdmission::<String, String>::ReturnOnly("winner-only".to_string()),
        |entry: &String| entry.clone(),
        |_entry: &String| None,
        |_entry: &String| true,
        move |_entry: String| {
            publish_count_cl.fetch_add(1, Ordering::SeqCst);
        },
        |_victims: ()| {},
        None,
    );

    assert_eq!(
        v.as_deref(),
        Some("winner-only"),
        "the ReturnOnly winner receives its value directly"
    );
    assert_eq!(
        publish_count.load(Ordering::SeqCst),
        0,
        "a ReturnOnly outcome must NOT invoke publish_core — nothing enters the store"
    );
}

/// A `Cacheable` outcome invokes `publish_core` exactly once and projects
/// the value.
#[test]
fn lookup_publish_cacheable_publishes_once() {
    let inflight: InflightTable<u32> = InflightTable::default();
    let publish_count = Arc::new(AtomicUsize::new(0));
    let publish_count_cl = Arc::clone(&publish_count);

    let v = cooperative_admit_with_lookup_publish(
        &inflight,
        1u32,
        || None::<String>,
        || ComputeAdmission::<String, String>::Cacheable("built".to_string()),
        |entry: &String| entry.clone(),
        |_entry: &String| None,
        |_entry: &String| true,
        move |entry: String| {
            publish_count_cl.fetch_add(1, Ordering::SeqCst);
            assert_eq!(
                entry, "built",
                "publish_core receives the built entry by value"
            );
        },
        |_victims: ()| {},
        None,
    );

    assert_eq!(v.as_deref(), Some("built"));
    assert_eq!(
        publish_count.load(Ordering::SeqCst),
        1,
        "a Cacheable outcome must invoke publish_core exactly once"
    );
}

/// Post-compute revalidation failure (a mutation landed mid-compute)
/// skips the publish: `publish_core` is never invoked and the winner
/// returns `None`.
#[test]
fn lookup_publish_revalidation_failure_skips_publish() {
    let inflight: InflightTable<u32> = InflightTable::default();
    let publish_count = Arc::new(AtomicUsize::new(0));
    let publish_count_cl = Arc::clone(&publish_count);

    let v = cooperative_admit_with_lookup_publish(
        &inflight,
        1u32,
        || None::<String>,
        || ComputeAdmission::<String, String>::Cacheable("stale".to_string()),
        |entry: &String| entry.clone(),
        |_entry: &String| None,
        // Revalidation rejects: a mutation invalidated the entry during
        // the cold window.
        |_entry: &String| false,
        move |_entry: String| {
            publish_count_cl.fetch_add(1, Ordering::SeqCst);
        },
        |_victims: ()| {},
        None,
    );

    assert_eq!(
        v, None,
        "post-compute revalidation failure must surface None to the winner"
    );
    assert_eq!(
        publish_count.load(Ordering::SeqCst),
        0,
        "publish_core must be skipped when revalidation rejects the entry"
    );
}

/// When the caller opts in via `project_unadmitted`, an admission-REFUSED
/// winner returns its COMPUTED value (lowered by the opt-in projection)
/// instead of `None` — nothing is published, exactly like `ReturnOnly`.
/// Discarding the computed value would force the caller's fallback to
/// fabricate a substitute, making the caller-visible result a function of
/// admission timing.
#[test]
fn lookup_publish_admission_refusal_returns_computed_value_when_opted_in() {
    let inflight: InflightTable<u32> = InflightTable::default();
    let publish_count = Arc::new(AtomicUsize::new(0));
    let publish_count_cl = Arc::clone(&publish_count);

    let v = cooperative_admit_with_lookup_publish(
        &inflight,
        1u32,
        || None::<String>,
        || ComputeAdmission::<String, String>::Cacheable("computed".to_string()),
        |entry: &String| entry.clone(),
        // Opt-in: the unadmitted projection returns the computed entry,
        // lowered (here: tagged) to its non-cacheable form.
        |entry: &String| Some(format!("{entry}:unadmitted")),
        // Revalidation rejects: a mutation invalidated the entry during
        // the cold window.
        |_entry: &String| false,
        move |_entry: String| {
            publish_count_cl.fetch_add(1, Ordering::SeqCst);
        },
        |_victims: ()| {},
        None,
    );

    assert_eq!(
        v.as_deref(),
        Some("computed:unadmitted"),
        "an admission-refused winner with an opted-in node must return the \
         COMPUTED value through the unadmitted projection, never None"
    );
    assert_eq!(
        publish_count.load(Ordering::SeqCst),
        0,
        "publish_core must still be skipped — the refused entry is never admitted"
    );
}

/// A joiner waking on a PANICKED winner must NOT consume the slot as a
/// value-bearing or failure-bearing outcome: the winner produced no
/// completeness evidence, so the joiner forks and cold-recomputes for its
/// own view. (A deterministic panic then surfaces as the joiner's OWN
/// panic — honest — never as a fabricated complete-labelled value.)
#[test]
fn lookup_publish_joiner_forks_and_recomputes_after_winner_panic() {
    use std::sync::mpsc;
    use std::time::Instant;

    let inflight: Arc<InflightTable<u32>> = Arc::new(InflightTable::default());

    let (claimed_tx, claimed_rx) = mpsc::sync_channel::<()>(0);
    let release_barrier = Arc::new(Barrier::new(2));

    // Winner thread that panics inside compute.
    let inflight_w = Arc::clone(&inflight);
    let release_barrier_w = Arc::clone(&release_barrier);
    let winner = thread::spawn(move || {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cooperative_admit_with_lookup_publish(
                &inflight_w,
                7u32,
                || None::<String>,
                || -> ComputeAdmission<String, String> {
                    claimed_tx
                        .send(())
                        .expect("rendezvous receiver must outlive winner's compute");
                    release_barrier_w.wait();
                    panic!("simulated compute panic");
                },
                |entry: &String| entry.clone(),
                |_entry: &String| None,
                |_entry: &String| true,
                |_entry: String| {},
                |_victims: ()| {},
                None,
            )
        }));
    });

    claimed_rx
        .recv()
        .expect("winner's compute must signal claim before panicking");

    // Joiner — wakes on the panicked winner, forks, and cold-recomputes
    // its own value.
    let inflight_j = Arc::clone(&inflight);
    let joiner = thread::spawn(move || {
        cooperative_admit_with_lookup_publish(
            &inflight_j,
            7u32,
            || None::<String>,
            || ComputeAdmission::<String, String>::Cacheable("joiner recomputed".to_string()),
            |entry: &String| entry.clone(),
            |_entry: &String| None,
            |_entry: &String| true,
            |_entry: String| {},
            |_victims: ()| {},
            None,
        )
    });

    // Deterministic wait: the joiner holds its own slot Arc once the
    // strong count reaches 4 (table + winner.slot + winner.panic_guard +
    // joiner).
    let poll_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if inflight
            .slot_strong_count(&7u32)
            .is_some_and(|count| count >= 4)
        {
            break;
        }
        assert!(
            Instant::now() < poll_deadline,
            "joiner failed to acquire inflight slot Arc within 10s"
        );
        std::hint::spin_loop();
    }
    release_barrier.wait();

    winner.join().unwrap();
    let joiner_result = joiner.join().unwrap();
    assert_eq!(
        joiner_result.as_deref(),
        Some("joiner recomputed"),
        "a joiner observing a panicked winner must FORK and cold-recompute its own \
         value — surfacing None here lets a caller fallback fabricate a \
         complete-labelled substitute from a winner that produced no completeness \
         evidence"
    );
}

/// `evict_deferred` is invoked with the victims `publish_core` returned —
/// the split lifecycle threads the deferred victims from the core step to
/// the eviction step.
#[test]
fn lookup_publish_threads_deferred_victims_to_evict() {
    let inflight: InflightTable<u32> = InflightTable::default();
    let evicted = Arc::new(AtomicUsize::new(0));
    let evicted_cl = Arc::clone(&evicted);

    let v = cooperative_admit_with_lookup_publish(
        &inflight,
        1u32,
        || None::<String>,
        || ComputeAdmission::<String, String>::Cacheable("built".to_string()),
        |entry: &String| entry.clone(),
        |_entry: &String| None,
        |_entry: &String| true,
        // publish_core returns two deferred victims.
        |_entry: String| vec![10u8, 20u8],
        move |victims: Vec<u8>| {
            evicted_cl.fetch_add(victims.len(), Ordering::SeqCst);
            assert_eq!(victims, vec![10u8, 20u8]);
        },
        None,
    );

    assert_eq!(v.as_deref(), Some("built"));
    assert_eq!(
        evicted.load(Ordering::SeqCst),
        2,
        "evict_deferred must receive exactly the victims publish_core returned"
    );
}

// ---------------------------------------------------------------------------
// Helpers for the store-backed discriminators.
// ---------------------------------------------------------------------------

fn sig_for(canonical: &str, hash_byte: u8) -> ReadSetSignature {
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: canonical.to_string(),
        hash: [hash_byte; 16],
    }]);
    ReadSetSignature::new(facts)
}

fn candidate_for(
    sig: ReadSetSignature,
    value: &str,
) -> Candidate<FactCandidateDiscriminant, String> {
    Candidate {
        discriminant: FactCandidateDiscriminant {
            validated_at_generation: 0,
            facts: Arc::clone(&sig.facts),
        },
        value: value.to_string(),
        signature: sig,
        self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
        admission_seq: 0,
        validated_at_generation: 0,
    }
}

// ---------------------------------------------------------------------------
// NO-DEADLOCK: a budgeted publish_core + deferred eviction must not
// self-deadlock when evict_deferred re-enters the store.
// ---------------------------------------------------------------------------

/// A budgeted [`ReverseIndexedCandidateStore`] driven through the
/// split-lifecycle adapter must not self-deadlock when its over-budget
/// admission's deferred eviction RE-ENTERS the store (the eviction removes
/// a victim candidate from the same slot map / reverse index).
///
/// The store's `publish_core` runs under the slot/shard write guard; the
/// FIFO victim eviction runs in `evict_deferred` AFTER that guard drops.
/// If `evict_deferred` (or the victim removal it drives) ran while the
/// publish-core slot/shard guard were still held — and the victim hashed
/// to the same shard — `remove_candidate_by_seq`'s `remove_if` would block
/// forever on a guard this very thread already holds.
///
/// Deadlock-freedom is asserted with a WATCHDOG: the publish sequence runs
/// on a worker thread that signals completion over an `mpsc` channel; the
/// test waits with `recv_timeout(5s)`. Pre-fix (eviction under the
/// publish-core guard) the worker hangs and the timeout fires → FAIL.
/// Post-fix the deferred eviction acquires the now-free guard and the
/// worker completes well under the timeout. The test also asserts the
/// eviction actually fired (cap-1 budget; K2 the sole survivor) so it
/// discriminates on eviction-DURING-publication, not merely on "did not
/// hang".
#[test]
fn budgeted_split_publish_eviction_does_not_self_deadlock() {
    use std::sync::mpsc;

    // Budget cap 1: publishing the second key evicts the first from inside
    // the same publish lifecycle.
    let store = Arc::new(
        ReverseIndexedCandidateStore::<u32, String>::with_counter_and_budget(
            Arc::new(AtomicU64::new(0)),
            1,
        ),
    );
    let inflight: Arc<InflightTable<crate::cache_runtime::node::QueryFlightKey<u32>>> =
        Arc::new(InflightTable::default());

    let store_w = Arc::clone(&store);
    let inflight_w = Arc::clone(&inflight);
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let worker = thread::spawn(move || {
        // Publish two distinct keys in sequence on ONE thread. K1 fills the
        // cap; K2 overflows it and must evict K1 via the deferred eviction
        // while the worker is the only thread touching the store.
        for n in [1u32, 2u32] {
            let store_inner = Arc::clone(&store_w);
            let flight_key = crate::cache_runtime::node::QueryFlightKey {
                key: n,
                compat_token: crate::resolver_core::StoreViewCompatToken {
                    epoch: 0,
                    session: None,
                    validity_fingerprint: 0,
                },
            };
            let v = cooperative_admit_with_lookup_publish(
                &inflight_w,
                flight_key,
                || None::<String>,
                move || {
                    ComputeAdmission::<String, Candidate<FactCandidateDiscriminant, String>>::Cacheable(
                        candidate_for(sig_for(&format!("/k{n}.ts"), 1), &format!("v{n}")),
                    )
                },
                |c: &Candidate<FactCandidateDiscriminant, String>| c.value.clone(),
                |_c: &Candidate<FactCandidateDiscriminant, String>| None,
                |_c: &Candidate<FactCandidateDiscriminant, String>| true,
                {
                    let store_pub = Arc::clone(&store_inner);
                    move |c: Candidate<FactCandidateDiscriminant, String>| {
                        store_pub.publish_core(n, c).deferred_victims
                    }
                },
                {
                    let store_evict = Arc::clone(&store_inner);
                    move |victims: Vec<(u32, u64)>| {
                        // Re-enters the store to remove the victim — the
                        // exact re-entry that would self-deadlock under a
                        // held publish-core guard.
                        store_evict.evict_deferred(victims);
                    }
                },
                Some(store_inner.retention_gate()),
            );
            assert_eq!(v.as_deref(), Some(format!("v{n}").as_str()));
        }
        done_tx.send(()).expect("worker signals completion");
    });

    match done_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(()) => worker.join().expect("worker thread joins cleanly"),
        Err(_) => panic!(
            "budgeted split publish DEADLOCKED: evict_deferred re-entered the store to evict a \
             victim while the publish held the slot/shard guard"
        ),
    }

    // Eviction actually ran during publication — discriminates on
    // re-entrant-eviction-during-publish, not merely deadlock-freedom.
    assert_eq!(
        store.live_count(),
        1,
        "the cap-1 budget keeps exactly one candidate (K2) after K2's publish evicts K1"
    );
    assert_eq!(store.slot_len_for_test(&1), 0, "K1 was evicted");
    assert_eq!(store.slot_len_for_test(&2), 1, "K2 survives");
}

// ---------------------------------------------------------------------------
// RETENTION-vs-CLEAR ATOMICITY: the publish fence spans publish_core →
// evict_deferred, so a `clear` cannot interleave between them.
// ---------------------------------------------------------------------------

/// The split publish lifecycle holds the `publish_fence` read guard across
/// BOTH `publish_core` AND `evict_deferred`. A project-generation `clear`
/// (which holds the matching write guard across its whole map+budget
/// clear) therefore cannot interleave between the core publish and the
/// deferred eviction.
///
/// The test pins a cold winner at exactly the post-publish-core /
/// pre-evict gap via the adapter's `POST_PUBLISH_CORE_PRE_EVICT_HOOK`
/// injection point and asserts `retention_gate.try_write()` is `None` — a
/// concurrent `clear` reaching the write fence right now WOULD block.
///
/// DISCRIMINATES: if the fence covered only `publish_core` (dropped before
/// `evict_deferred`), the winner would hold NO guard at the hook point,
/// `try_write()` would succeed (`Some`), and the assertion would FAIL.
/// With the fence spanning the whole lifecycle it returns `None` and the
/// assertion PASSES.
#[test]
fn publish_fence_spans_publish_core_through_evict_deferred() {
    let store = Arc::new(
        ReverseIndexedCandidateStore::<u32, String>::with_counter_and_budget(
            Arc::new(AtomicU64::new(0)),
            16,
        ),
    );
    let inflight: Arc<InflightTable<crate::cache_runtime::node::QueryFlightKey<u32>>> =
        Arc::new(InflightTable::default());

    // party 1 = parked winner, party 2 = main.
    let parked = Arc::new(Barrier::new(2));

    let store_w = Arc::clone(&store);
    let inflight_w = Arc::clone(&inflight);
    let parked_w = Arc::clone(&parked);
    let winner = thread::spawn(move || {
        // Install the post-publish-core / pre-evict rendezvous on the
        // WINNER thread (the hook is thread-local). It parks the winner
        // inside the `publish_fence` region, AFTER `publish_core` and
        // BEFORE `evict_deferred`.
        let _hook = install_post_publish_core_pre_evict_hook(Box::new(move || {
            parked_w.wait();
            parked_w.wait();
        }));
        let flight_key = crate::cache_runtime::node::QueryFlightKey {
            key: 1u32,
            compat_token: crate::resolver_core::StoreViewCompatToken {
                epoch: 0,
                session: None,
                validity_fingerprint: 0,
            },
        };
        let store_inner = Arc::clone(&store_w);
        cooperative_admit_with_lookup_publish(
            &inflight_w,
            flight_key,
            || None::<String>,
            || {
                ComputeAdmission::<String, Candidate<FactCandidateDiscriminant, String>>::Cacheable(
                    candidate_for(sig_for("/fence.ts", 1), "v1"),
                )
            },
            |c: &Candidate<FactCandidateDiscriminant, String>| c.value.clone(),
            |_c: &Candidate<FactCandidateDiscriminant, String>| None,
            |_c: &Candidate<FactCandidateDiscriminant, String>| true,
            {
                let store_pub = Arc::clone(&store_inner);
                move |c: Candidate<FactCandidateDiscriminant, String>| {
                    store_pub.publish_core(1, c).deferred_victims
                }
            },
            {
                let store_evict = Arc::clone(&store_inner);
                move |victims: Vec<(u32, u64)>| store_evict.evict_deferred(victims)
            },
            Some(store_inner.retention_gate()),
        )
    });

    // The winner has run publish_core and is parked before evict_deferred,
    // holding the retention read guard.
    parked.wait();
    // DETERMINISTIC DISCRIMINATOR: the winner holds the retention read
    // guard right now, so a `clear` reaching the write fence WOULD block.
    assert!(
        store.test_retention_gate().try_write().is_none(),
        "RETENTION-vs-CLEAR ATOMICITY: the publish fence read guard does NOT span \
         publish_core → evict_deferred. A project-generation `clear` could take the write \
         guard in the gap, clearing the cache between the core publish and the deferred \
         eviction. The fence must span the whole split lifecycle."
    );

    // Release the winner: it runs evict_deferred and drops the fence.
    parked.wait();
    winner.join().expect("winner thread");

    // The candidate is published; the fence has dropped.
    assert_eq!(store.live_count(), 1);
    assert!(store.test_retention_gate().try_write().is_some());
}

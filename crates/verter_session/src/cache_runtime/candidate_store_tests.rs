//! Discriminators for the shared [`ReverseIndexedCandidateStore`].
//!
//! Exercised directly on the store (no `VerterHost`): the multi-candidate
//! R20 coexistence + cap, FIFO eviction, replacement-by-discriminant,
//! per-canonical reverse-index registration / identity-scoped removal,
//! deferred budget eviction, per-canonical drain, and the
//! publish-core/evict-deferred split lifecycle.

use super::*;
use crate::cache_runtime::admission::{Candidate, FactCandidateDiscriminant};
use crate::cache_runtime::node::QUERY_SLOT_CANDIDATE_CAP;
use crate::fact_signature_helpers::ReadSetSignature;
use crate::resolver_core::FactVersionRef;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

/// Build a `ReadSetSignature` over one `FileWholeHash` fact for
/// `canonical`. The hash byte distinguishes distinct content versions.
fn sig_for(canonical: &str, hash_byte: u8) -> ReadSetSignature {
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: canonical.to_string(),
        hash: [hash_byte; 16],
    }]);
    ReadSetSignature::new(facts)
}

/// Build a candidate carrying `sig` as both its discriminant fact set and
/// its validity signature, stamped at `generation`.
fn candidate_for(
    sig: ReadSetSignature,
    value: &str,
    generation: u64,
    self_roots: Arc<[Arc<str>]>,
) -> Candidate<FactCandidateDiscriminant, String> {
    Candidate {
        discriminant: FactCandidateDiscriminant {
            validated_at_generation: generation,
            facts: Arc::clone(&sig.facts),
        },
        value: value.to_string(),
        signature: sig,
        self_root_canonicals: self_roots,
        admission_seq: 0,
        validated_at_generation: generation,
    }
}

/// Publish-core + evict-deferred in one shot, mirroring the cooperative
/// adapter's winner-side call sequence.
fn publish(
    store: &ReverseIndexedCandidateStore<u32, String>,
    key: u32,
    candidate: Candidate<FactCandidateDiscriminant, String>,
) -> PublishOutcome {
    let outcome = store.publish_core(key, candidate);
    let result = outcome.outcome.clone();
    store.evict_deferred(outcome.deferred_victims);
    result
}

/// Two candidates for the SAME content-free key but DIFFERENT views
/// (distinct fact sets / generations) coexist as DISTINCT candidates —
/// neither overwrites the other (R20 overlay isolation). A re-publish
/// under the SAME view replaces in place.
#[test]
fn distinct_views_coexist_same_view_replaces() {
    let store =
        ReverseIndexedCandidateStore::<u32, String>::with_counter(Arc::new(AtomicU64::new(0)));
    let key = 7u32;
    let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from("/a.ts")]);

    // View 1 (base): hash byte 1.
    let c1 = candidate_for(sig_for("/a.ts", 1), "base", 0, Arc::clone(&self_roots));
    // View 2 (overlay): hash byte 2 — a DIFFERENT fact set, same key.
    let c2 = candidate_for(sig_for("/a.ts", 2), "overlay", 0, Arc::clone(&self_roots));

    assert_eq!(publish(&store, key, c1), PublishOutcome::Published);
    assert_eq!(
        publish(&store, key, c2),
        PublishOutcome::Published,
        "a DIFFERENT view (distinct facts) must admit a DISTINCT candidate, \
         not replace the base candidate (R20 overlay isolation)"
    );
    assert_eq!(
        store.slot_len_for_test(&key),
        2,
        "base and overlay candidates coexist in one slot"
    );
    assert_eq!(store.live_counter_for_test(), 2);

    // A re-publish under the SAME base view (same facts + generation)
    // REPLACES in place — occupancy unchanged.
    let c1_again = candidate_for(sig_for("/a.ts", 1), "base-v2", 0, Arc::clone(&self_roots));
    assert_eq!(
        publish(&store, key, c1_again),
        PublishOutcome::Replaced,
        "a re-publish under the same view (same facts+gen) replaces in place"
    );
    assert_eq!(
        store.slot_len_for_test(&key),
        2,
        "a same-view replace does not grow the slot"
    );
    assert_eq!(store.live_counter_for_test(), 2);

    // The refreshed base value is observable; the overlay candidate is
    // untouched.
    let base = store.lookup(&key, |c| (c.value == "base-v2").then(|| c.value.clone()));
    let overlay = store.lookup(&key, |c| (c.value == "overlay").then(|| c.value.clone()));
    assert_eq!(base.as_deref(), Some("base-v2"));
    assert_eq!(overlay.as_deref(), Some("overlay"));
}

/// A fresh candidate past the per-slot cap FIFO-evicts the oldest, and
/// the eviction runs the full removal cleanup (counter decrement +
/// reverse-index drain) for the victim exactly once.
#[test]
fn fifo_eviction_past_cap_runs_victim_cleanup() {
    let store =
        ReverseIndexedCandidateStore::<u32, String>::with_counter(Arc::new(AtomicU64::new(0)));
    let key = 1u32;
    let self_roots: Arc<[Arc<str>]> = Arc::from(Vec::<Arc<str>>::new());

    // Fill the slot to the cap with CAP distinct views, each naming a
    // distinct canonical so its reverse-index registration is observable.
    for i in 0..QUERY_SLOT_CANDIDATE_CAP as u8 {
        let canonical = format!("/c{i}.ts");
        let c = candidate_for(
            sig_for(&canonical, i),
            &format!("v{i}"),
            0,
            Arc::clone(&self_roots),
        );
        assert_eq!(publish(&store, key, c), PublishOutcome::Published);
    }
    assert_eq!(store.slot_len_for_test(&key), QUERY_SLOT_CANDIDATE_CAP);
    assert_eq!(
        store.live_counter_for_test(),
        QUERY_SLOT_CANDIDATE_CAP as u64
    );
    // The oldest candidate (v0) names /c0.ts in the reverse index.
    assert!(store.reverse_index_contains_key_for_test("/c0.ts", &key));

    // A (CAP+1)-th distinct view evicts the oldest (v0).
    let overflow = candidate_for(
        sig_for("/overflow.ts", 99),
        "overflow",
        0,
        Arc::clone(&self_roots),
    );
    assert_eq!(
        publish(&store, key, overflow),
        PublishOutcome::Evicted { count: 1 },
        "the (CAP+1)-th distinct view FIFO-evicts the oldest candidate"
    );
    assert_eq!(store.slot_len_for_test(&key), QUERY_SLOT_CANDIDATE_CAP);
    assert_eq!(
        store.live_counter_for_test(),
        QUERY_SLOT_CANDIDATE_CAP as u64,
        "the live counter nets +1 (overflow) -1 (v0) = unchanged at the cap"
    );
    // v0's reverse-index registration was drained by its eviction.
    assert!(
        !store.reverse_index_contains_key_for_test("/c0.ts", &key),
        "the FIFO victim's reverse-index registration must be drained on eviction"
    );
    // The just-published overflow candidate IS registered.
    assert!(store.reverse_index_contains_key_for_test("/overflow.ts", &key));
}

/// The global retention budget caps the TOTAL candidate count across
/// slots; an over-budget admission returns FIFO victims that
/// `evict_deferred` removes — and the removal is identity-scoped by
/// `(key, admission_seq)`.
#[test]
fn global_budget_evicts_oldest_across_slots() {
    // Budget cap 2; per-slot cap is the default 4, so the global budget —
    // not the per-slot cap — drives eviction here.
    let store = ReverseIndexedCandidateStore::<u32, String>::with_counter_and_budget(
        Arc::new(AtomicU64::new(0)),
        2,
    );
    let self_roots: Arc<[Arc<str>]> = Arc::from(Vec::<Arc<str>>::new());

    // Three DIFFERENT keys, each one candidate — the third overflows the
    // global cap and evicts the first key's candidate.
    let c1 = candidate_for(sig_for("/k1.ts", 1), "v1", 0, Arc::clone(&self_roots));
    let c2 = candidate_for(sig_for("/k2.ts", 1), "v2", 0, Arc::clone(&self_roots));
    let c3 = candidate_for(sig_for("/k3.ts", 1), "v3", 0, Arc::clone(&self_roots));

    assert_eq!(publish(&store, 1, c1), PublishOutcome::Published);
    assert_eq!(publish(&store, 2, c2), PublishOutcome::Published);
    assert_eq!(store.live_count(), 2);
    assert_eq!(store.retention_tracked_len(), 2);

    // The third admission overflows the cap of 2; the oldest (key 1) is a
    // deferred victim, evicted by `evict_deferred`.
    publish(&store, 3, c3);
    assert_eq!(
        store.live_count(),
        2,
        "the global budget caps the total candidate count at 2"
    );
    assert_eq!(store.retention_tracked_len(), 2);
    // Key 1's candidate is gone; its slot is detached.
    assert_eq!(store.slot_len_for_test(&1), 0);
    assert_eq!(store.slot_len_for_test(&2), 1);
    assert_eq!(store.slot_len_for_test(&3), 1);
    // Key 1's reverse-index registration was drained.
    assert!(!store.reverse_index_contains_key_for_test("/k1.ts", &1));
}

/// `invalidate_canonical` drains every candidate whose facts reference
/// the canonical, in O(K), and runs each removal's full cleanup. A
/// candidate not referencing the canonical survives.
#[test]
fn invalidate_canonical_drains_only_referencing_candidates() {
    let store =
        ReverseIndexedCandidateStore::<u32, String>::with_counter(Arc::new(AtomicU64::new(0)));
    let self_roots: Arc<[Arc<str>]> = Arc::from(Vec::<Arc<str>>::new());

    // Two keys reference /shared.ts; one references /other.ts only.
    let a = candidate_for(sig_for("/shared.ts", 1), "a", 0, Arc::clone(&self_roots));
    let b = candidate_for(sig_for("/shared.ts", 1), "b", 0, Arc::clone(&self_roots));
    let c = candidate_for(sig_for("/other.ts", 1), "c", 0, Arc::clone(&self_roots));
    publish(&store, 1, a);
    publish(&store, 2, b);
    publish(&store, 3, c);
    assert_eq!(store.live_count(), 3);

    let removed = store.invalidate_canonical("/shared.ts");
    assert_eq!(removed, 2, "exactly the two /shared.ts candidates drained");
    assert_eq!(store.live_count(), 1, "the /other.ts candidate survives");
    assert_eq!(store.slot_len_for_test(&3), 1);
    // The shared canonical's reverse-index shard is gone.
    assert!(!store.reverse_index_contains_key_for_test("/shared.ts", &1));
    assert!(!store.reverse_index_contains_key_for_test("/shared.ts", &2));
}

/// `invalidate_all` clears every slot, the reverse index, the budget
/// ledger, and the live counter in one write-guarded step.
#[test]
fn invalidate_all_clears_everything() {
    let store = ReverseIndexedCandidateStore::<u32, String>::with_counter_and_budget(
        Arc::new(AtomicU64::new(0)),
        16,
    );
    let self_roots: Arc<[Arc<str>]> = Arc::from(Vec::<Arc<str>>::new());
    for i in 0..3u32 {
        let c = candidate_for(
            sig_for(&format!("/f{i}.ts"), i as u8),
            &format!("v{i}"),
            0,
            Arc::clone(&self_roots),
        );
        publish(&store, i, c);
    }
    assert_eq!(store.live_count(), 3);
    assert_eq!(store.retention_tracked_len(), 3);

    let cleared = store.invalidate_all();
    assert_eq!(cleared, 3);
    assert_eq!(store.live_count(), 0);
    assert_eq!(store.live_counter_for_test(), 0);
    assert_eq!(store.retention_tracked_len(), 0);
    assert_eq!(store.canonical_index_shard_count_for_test(), 0);
}

/// F1-P1 closure: `publish_core` establishes the candidate's slot
/// membership, its live-counter contribution, AND its reverse-index
/// registration as ONE non-reentrant step under the slot guard — so a
/// concurrent remover can never observe a published candidate before its
/// counter / reverse-index registration exists.
///
/// This test drives `publish_core` concurrently with a stream of
/// `invalidate_canonical` removers on the same canonical and asserts the
/// store never lands in an inconsistent state: the live counter always
/// equals the live candidate count, and a candidate is reverse-indexed
/// iff it is live. The pre-F1-P1 hazard — a candidate visible in the slot
/// but not yet counted / indexed, which a concurrent remover would then
/// warm-remove leaving a live-counter underflow or a dangling
/// registration — cannot occur because `publish_core` does all three
/// under one guard.
#[test]
fn publish_core_atomic_install_counter_index_closes_f1p1() {
    use std::sync::atomic::Ordering;
    use std::sync::Barrier;
    use std::thread;

    let store = Arc::new(ReverseIndexedCandidateStore::<u32, String>::with_counter(
        Arc::new(AtomicU64::new(0)),
    ));
    let canonical = "/race.ts";

    // A pool of publisher threads and a pool of remover threads hammer one
    // canonical concurrently. The start barrier maximises interleaving.
    let start = Arc::new(Barrier::new(8));
    let mut handles = Vec::new();
    for t in 0..4u32 {
        let store = Arc::clone(&store);
        let start = Arc::clone(&start);
        handles.push(thread::spawn(move || {
            start.wait();
            for v in 0..50u8 {
                // Each publish uses a distinct view (hash byte) so it is a
                // distinct candidate (subject to the per-slot cap).
                let sig = sig_for(canonical, t as u8 * 50 + v);
                let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from(canonical)]);
                let candidate = candidate_for(sig, &format!("v{t}-{v}"), 0, self_roots);
                let outcome = store.publish_core(t, candidate);
                store.evict_deferred(outcome.deferred_victims);
            }
        }));
    }
    for _ in 0..4 {
        let store = Arc::clone(&store);
        let start = Arc::clone(&start);
        handles.push(thread::spawn(move || {
            start.wait();
            for _ in 0..50 {
                store.invalidate_canonical(canonical);
            }
        }));
    }
    for h in handles {
        h.join().expect("worker joins cleanly");
    }

    // FINAL CONSISTENCY: the live counter must equal the live candidate
    // count exactly. A pre-F1-P1 race (a remover warm-removing a candidate
    // before its counter bump landed) would leave the counter and the
    // live count out of sync.
    let live = store.live_count() as u64;
    let counter = store.live_counter_for_test();
    assert_eq!(
        counter, live,
        "F1-P1: the live counter ({counter}) must equal the live candidate \
         count ({live}) after a concurrent publish / invalidate storm — \
         publish_core establishes slot membership + counter + reverse index \
         as one atomic step under the slot guard, so no remover can observe \
         a published candidate before its counter registration exists"
    );

    // Every surviving candidate must still be reverse-indexed (no dangling
    // or missing registration). Drain the canonical and assert the drain
    // count equals the live count — a candidate is reverse-indexed iff it
    // is live.
    let drained = store.invalidate_canonical(canonical) as u64;
    assert_eq!(
        drained, live,
        "every live candidate must be reverse-indexed: the per-canonical \
         drain removed {drained} candidates but {live} were live — a \
         mismatch means a candidate was counted/slotted without its \
         reverse-index registration (or vice versa)"
    );
    assert_eq!(
        store.live_counter_for_test(),
        0,
        "after draining the only canonical the counter must be zero"
    );
    let _ = Ordering::Relaxed;
}

/// A FIFO eviction that empties a canonical's reverse-index shard must
/// drop the OUTER shard, so the reverse index does not grow unbounded
/// under churn across distinct canonicals.
#[test]
fn fifo_eviction_prunes_empty_reverse_index_shards() {
    // Budget cap 1, each key naming a distinct canonical. The (N+1)-th
    // admission evicts the oldest; its canonical's shard must be pruned.
    let store = ReverseIndexedCandidateStore::<u32, String>::with_counter_and_budget(
        Arc::new(AtomicU64::new(0)),
        1,
    );
    let self_roots: Arc<[Arc<str>]> = Arc::from(Vec::<Arc<str>>::new());

    let c1 = candidate_for(sig_for("/a.ts", 1), "a", 0, Arc::clone(&self_roots));
    publish(&store, 1, c1);
    assert_eq!(store.canonical_index_shard_count_for_test(), 1);

    // The second admission (distinct canonical) evicts the first; the
    // first's shard must be gone, leaving exactly one shard.
    let c2 = candidate_for(sig_for("/b.ts", 1), "b", 0, Arc::clone(&self_roots));
    publish(&store, 2, c2);
    assert_eq!(
        store.canonical_index_shard_count_for_test(),
        1,
        "the evicted candidate's reverse-index shard must be pruned — \
         a leaked empty shard grows the bounded index under churn"
    );
    assert!(!store.reverse_index_contains_key_for_test("/a.ts", &1));
    assert!(store.reverse_index_contains_key_for_test("/b.ts", &2));
}

/// A budget-victim eviction is identity-scoped by `(key, admission_seq)`:
/// a concurrent same-key re-publish carrying a DISTINCT seq survives an
/// eviction targeting the OLD seq.
///
/// DISCRIMINATION: a bare-key removal would evict the fresh re-publish
/// and strand its live ledger record (cache grows past the cap). With
/// `(key, seq)` scoping the fresh candidate survives and its ledger
/// record is counted.
#[test]
fn budget_victim_eviction_is_admission_seq_scoped() {
    let store =
        ReverseIndexedCandidateStore::<u32, String>::with_counter(Arc::new(AtomicU64::new(0)));
    let self_roots: Arc<[Arc<str>]> = Arc::from(Vec::<Arc<str>>::new());

    // Admit an OLD candidate under key 1, capturing its assigned seq via
    // the slot snapshot.
    let old = candidate_for(sig_for("/k.ts", 1), "old", 0, Arc::clone(&self_roots));
    let old_outcome = store.publish_core(1, old);
    store.evict_deferred(old_outcome.deferred_victims);
    let old_seq = {
        // The only candidate in slot 1 is the old one; read its seq via
        // a lookup that returns the admission_seq through the accept hook.
        let mut captured = None;
        store.lookup(&1, |c| {
            captured = Some(c.admission_seq);
            None::<String>
        });
        captured.expect("old candidate present")
    };

    // A fresh re-publish under the SAME key but a DIFFERENT view (distinct
    // facts) — admits a DISTINCT candidate with a distinct seq.
    let fresh = candidate_for(sig_for("/k.ts", 2), "fresh", 0, Arc::clone(&self_roots));
    let fresh_outcome = store.publish_core(1, fresh);
    store.evict_deferred(fresh_outcome.deferred_victims);
    assert_eq!(store.slot_len_for_test(&1), 2, "both candidates coexist");

    // Evict ONLY the old seq via the deferred path (modelling a budget
    // victim targeting the old admission).
    store.evict_deferred(vec![(1u32, old_seq)]);

    // The fresh candidate survives; the old one is gone.
    assert_eq!(
        store.slot_len_for_test(&1),
        1,
        "the seq-scoped eviction removed ONLY the old candidate"
    );
    let fresh_value = store.lookup(&1, |c| (c.value == "fresh").then(|| c.value.clone()));
    assert_eq!(
        fresh_value.as_deref(),
        Some("fresh"),
        "the fresh same-key re-publish must survive an eviction scoped to the OLD seq"
    );
    let old_gone = store.lookup(&1, |c| (c.value == "old").then(|| c.value.clone()));
    assert_eq!(old_gone, None, "the old candidate was evicted");
}

/// `publish_core` returns FIFO victims for DEFERRED eviction — it does NOT
/// evict them itself. The store still holds the over-cap candidate until
/// `evict_deferred` runs. This is the split that lets a budgeted eviction
/// run guard-free.
#[test]
fn publish_core_defers_budget_victims() {
    let store = ReverseIndexedCandidateStore::<u32, String>::with_counter_and_budget(
        Arc::new(AtomicU64::new(0)),
        1,
    );
    let self_roots: Arc<[Arc<str>]> = Arc::from(Vec::<Arc<str>>::new());

    let c1 = candidate_for(sig_for("/k1.ts", 1), "v1", 0, Arc::clone(&self_roots));
    publish(&store, 1, c1);
    assert_eq!(store.live_count(), 1);

    // The second admission overflows the cap-1 budget. publish_core must
    // RETURN the victim, not evict it — so immediately after publish_core
    // BOTH candidates are momentarily resident.
    let c2 = candidate_for(sig_for("/k2.ts", 1), "v2", 0, Arc::clone(&self_roots));
    let outcome = store.publish_core(2, c2);
    assert_eq!(
        outcome.deferred_victims.len(),
        1,
        "the over-budget admission returns exactly one deferred victim"
    );
    assert_eq!(
        store.live_count(),
        2,
        "publish_core must NOT evict the victim itself — both candidates \
         are resident until evict_deferred runs"
    );

    // evict_deferred removes the victim.
    store.evict_deferred(outcome.deferred_victims);
    assert_eq!(store.live_count(), 1, "evict_deferred removes the victim");
    assert_eq!(store.slot_len_for_test(&1), 0, "key 1 (oldest) was evicted");
    assert_eq!(store.slot_len_for_test(&2), 1);
}

/// **Race discriminator.** A `publish_core` parked between
/// `candidates.push(new_candidate)` and the per-canonical reverse-index
/// insert holds the slot write guard AND the `retention_gate.read()`
/// guard. A concurrent `invalidate_canonical` on a canonical the parked
/// candidate references must BLOCK on the read guard (because
/// `invalidate_canonical` takes `retention_gate.write()`) until the
/// publisher releases its guards. When the invalidator unblocks, the
/// candidate's `(key, seq)` entry IS in the canonical_index (the
/// publisher's reverse-index insert ran before its guards dropped), so
/// the drain removes it. The post-fix result: the candidate is gone.
///
/// Pre-fix behaviour (revert `invalidate_canonical` to take
/// `retention_gate.read()`): the invalidator races concurrently with
/// the parked publisher, observes the canonical_index empty (the
/// publisher has pushed but not yet inserted), returns 0, and leaves
/// the candidate live with pre-edit facts. The post-publish slot count
/// would be 1, NOT 0 — this assertion is what makes the test
/// discriminate.
#[test]
fn publisher_registration_visible_to_concurrent_invalidator() {
    use std::sync::Barrier;
    use std::thread;
    use std::time::{Duration, Instant};

    let store = Arc::new(ReverseIndexedCandidateStore::<u32, String>::with_counter(
        Arc::new(AtomicU64::new(0)),
    ));
    let key = 42u32;
    let canonical = "/race.ts";

    // Arm the test gate: the publisher will park AFTER its push, BEFORE
    // its reverse-index insert. Two participants — publisher + this
    // thread.
    let barrier = Arc::new(Barrier::new(2));
    store.test_arm_publish_post_push_pre_register_gate(Some(Arc::clone(&barrier)));

    // Spawn the publisher. It will push, then park at the gate.
    //
    // The publisher acquires `retention_gate.read()` BEFORE calling
    // publish_core, mirroring the production lookup_publish adapter
    // (which holds the read guard across post-compute revalidation,
    // publish_core, and evict_deferred). This is the read-side
    // discipline `invalidate_canonical.write()` is exclusive against;
    // without this acquire the test would not exercise the post-fix
    // gate behaviour.
    let store_pub = Arc::clone(&store);
    let self_roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from(canonical)]);
    let publisher = thread::spawn(move || {
        let _retention = store_pub.retention_gate().read();
        let candidate = candidate_for(sig_for(canonical, 1), "pre-edit", 0, self_roots);
        let outcome = store_pub.publish_core(key, candidate);
        store_pub.evict_deferred(outcome.deferred_victims);
    });

    // Wait for the publisher to reach the gate (first `barrier.wait()`
    // pairing). At this point the publisher holds slot.candidates.write
    // AND retention_gate.read, has pushed its candidate, and has NOT yet
    // run the canonical_index insert.
    barrier.wait();

    // Spawn the invalidator. Under the FIX it will block on
    // `retention_gate.write()` (because the publisher holds the read
    // side); under the BUG (read-side invalidate) it would race past,
    // observe canonical_index empty, and return 0.
    let store_inv = Arc::clone(&store);
    let inv_done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let inv_done_clone = Arc::clone(&inv_done);
    let invalidator = thread::spawn(move || {
        let removed = store_inv.invalidate_canonical(canonical);
        inv_done_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        removed
    });

    // The invalidator MUST be blocked while the publisher is parked —
    // give it a generous quantum to attempt a forbidden interleave.
    // Under the pre-fix `retention_gate.read()` invalidator, this thread
    // would have raced through `canonical_index.remove(canonical)` and
    // set `inv_done = true` long before the timeout elapses.
    let park_window = Duration::from_millis(150);
    let park_start = Instant::now();
    while park_start.elapsed() < park_window {
        if inv_done.load(std::sync::atomic::Ordering::SeqCst) {
            // The invalidator ran while the publisher was parked. That
            // is the bug pattern.
            barrier.wait(); // unblock publisher so the test can clean up
            publisher.join().unwrap();
            let _ = invalidator.join();
            panic!(
                "RACE GUARD VIOLATION — invalidate_canonical completed \
                 while the publisher was parked between candidates.push \
                 and canonical_index.insert. Either the gate is no longer \
                 exclusive against publish_core, or invalidate_canonical \
                 is taking retention_gate.read() instead of write()."
            );
        }
        thread::sleep(Duration::from_millis(2));
    }

    // Release the publisher — it completes its canonical_index insert
    // and drops its guards.
    barrier.wait();
    publisher.join().expect("publisher thread should not panic");

    // The invalidator now unblocks, acquires `retention_gate.write()`,
    // reads canonical_index (which CONTAINS the publisher's freshly
    // registered (key, seq)), drains it, and removes the candidate.
    let removed = invalidator.join().expect("invalidator should not panic");
    assert_eq!(
        removed, 1,
        "POST-FIX: the invalidator must have drained exactly the \
         publisher's candidate (seq just registered before publisher \
         dropped its guards). PRE-FIX (read-side invalidate): the \
         invalidator would have returned 0 because canonical_index was \
         empty when it ran."
    );
    assert_eq!(
        store.slot_len_for_test(&key),
        0,
        "POST-FIX: the candidate computed against pre-edit facts must \
         be gone after the invalidator runs. PRE-FIX: it would persist \
         with stale facts."
    );

    // Disarm the gate so subsequent tests are unaffected.
    store.test_arm_publish_post_push_pre_register_gate(None);
}

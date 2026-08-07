//! Contract for the BRACKETED generation protocol.
//!
//! The property under test is not "the counter increments". It is that
//! **no reader can ever pair a mutated store with an unmoved
//! generation** — the mutation/stamp gap a naive post-mutation
//! `fetch_add` leaves open.
//!
//! That is a concurrency property, so the tests exercise interleavings
//! rather than asserting a lock is held:
//!
//! * [`a_stable_read_is_impossible_for_the_whole_duration_of_a_mutation`]
//!   pins the window DETERMINISTICALLY with barriers — no sleeps, no
//!   rates. It is the assertion a naive post-mutation increment cannot
//!   satisfy at all, because such a protocol has no in-flight state to
//!   observe.
//! * [`a_reader_that_sees_a_stable_pair_never_straddles_a_mutation`]
//!   runs real concurrent mutations against real concurrent readers and
//!   asserts the pairing invariant directly on observed state.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};

use super::BracketedGeneration;

/// A mutation that is IN FLIGHT is observable as such for its whole
/// duration — there is no instant at which the counter reads stable
/// while the mutation body is still running.
///
/// This is the assertion that distinguishes a bracketed protocol from a
/// post-mutation increment: the latter has no in-flight state at all, so
/// `stable()` answers `Some(_)` throughout and this test cannot pass
/// against it.
///
/// Deterministic by construction — two barriers pin the observation
/// strictly inside the mutation body, so there is no rate and no flake.
///
/// Mutation recipe, EXECUTED: in `BracketedGeneration::mutate`, replace
/// the bracket with a post-mutation advance —
/// `let (value, changed) = mutation(); if changed { self.seq.fetch_add(2, ..); } value`
/// (dropping the enter-odd `fetch_add` and the guard). The
/// `stable().is_none()` assertion below fails.
#[test]
fn a_stable_read_is_impossible_for_the_whole_duration_of_a_mutation() {
    let generation = Arc::new(BracketedGeneration::default());
    let entered = Arc::new(Barrier::new(2));
    let observed = Arc::new(Barrier::new(2));

    let before = generation
        .stable()
        .expect("a quiescent generation reads stable");

    let writer = {
        let generation = Arc::clone(&generation);
        let entered = Arc::clone(&entered);
        let observed = Arc::clone(&observed);
        std::thread::spawn(move || {
            generation.mutate(|| {
                // The mutation body is now running. Hand control to the
                // reader and hold the window open until it has looked.
                entered.wait();
                observed.wait();
                ((), true)
            });
        })
    };

    entered.wait();
    let mid_flight = generation.stable();
    observed.wait();
    writer.join().expect("writer thread must not panic");

    assert!(
        mid_flight.is_none(),
        "a generation whose mutation is still running must NOT hand out a stable stamp: a reader \
         that snapshotted one here would pair the pre-mutation number with a store the mutation \
         may already have changed, which is exactly the gap the bracket exists to close"
    );
    let after = generation
        .stable()
        .expect("the generation is stable again once the mutation completes");
    assert_ne!(
        before, after,
        "a membership-CHANGING mutation must leave the generation advanced, or a reader spanning \
         it detects nothing"
    );
}

/// A mutation that reports NO membership change restores the generation
/// it entered with, so a reader spanning a genuine no-op sees no
/// movement and keeps its compaction.
///
/// Mutation recipe, EXECUTED: make `mutate` advance unconditionally
/// (drop the `changed` branch and always `fetch_add`). This test fails
/// while the sibling above stays green.
#[test]
fn a_mutation_that_changes_nothing_restores_the_generation_it_entered_with() {
    let generation = BracketedGeneration::default();
    let before = generation.stable().expect("quiescent");

    generation.mutate(|| ((), false));

    assert_eq!(
        generation.stable(),
        Some(before),
        "a refused admission or an identical-candidate skip changed no membership, so advancing \
         for it would refuse every concurrent reader's compaction for nothing"
    );

    generation.mutate(|| ((), true));
    assert_ne!(
        generation.stable(),
        Some(before),
        "control: the same protocol DOES advance when the mutation reports a change, so the \
         no-advance above is about the report and not about a dead counter"
    );
}

/// A mutation that PANICS leaves the generation advanced, never
/// restored and never stuck mid-flight.
///
/// On unwind the store's membership is unknown, so the conservative
/// direction is to claim a new generation: every reader spanning the
/// panic detects movement and refuses. Restoring would let a reader
/// admit a compacted witness over a store it cannot describe; staying
/// odd forever would disarm the domain permanently.
#[test]
fn a_panicking_mutation_advances_rather_than_restoring_or_wedging() {
    let generation = BracketedGeneration::default();
    let before = generation.stable().expect("quiescent");

    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        generation.mutate(|| -> ((), bool) { panic!("mutation body failed") });
    }));
    assert!(panicked.is_err(), "fixture invariant: the body must panic");

    let after = generation
        .stable()
        .expect("an unwound mutation must leave the generation STABLE, not wedged mid-flight");
    assert_ne!(
        after, before,
        "an unwound mutation leaves membership unknown, so it must claim a new generation and \
         force every spanning reader to refuse"
    );
}

/// **The pairing invariant, under real interleavings.**
///
/// Readers repeatedly snapshot a stable generation, read the store's
/// membership, and re-read the generation. Whenever the two generation
/// reads agree, the membership they saw MUST be the membership that
/// generation denotes.
///
/// The store here is a counter the mutation increments exactly once per
/// advance, so `membership == generation_advances` is an exact identity
/// a straddling reader violates. A reader that observed a mutated store
/// beside an unmoved generation fails the assertion directly — no
/// proxy, no correlation.
///
/// Mutation recipe, EXECUTED: apply the post-mutation-advance plant from
/// the first test. This test then fails on every run (3/3 observed), but
/// only BECAUSE its mutation body yields between the store write and the
/// mutation's exit — without that yield the plant survived a full run,
/// so the reliability is a property of the fixture, not of the
/// mutation's raw window. Both tests are kept: the deterministic one is
/// the rail, this one is the evidence that the rail describes real
/// interleavings rather than a barrier arrangement.
#[test]
fn a_reader_that_sees_a_stable_pair_never_straddles_a_mutation() {
    /// Stands in for the store: incremented exactly once per advancing
    /// mutation, INSIDE the bracket.
    struct Store {
        generation: BracketedGeneration,
        membership: AtomicU64,
    }

    let store = Arc::new(Store {
        generation: BracketedGeneration::default(),
        membership: AtomicU64::new(0),
    });
    let base = store.generation.stable().expect("quiescent");

    let rounds = 2_000_u64;
    let writer = {
        let store = Arc::clone(&store);
        std::thread::spawn(move || {
            for round in 0..rounds {
                store.generation.mutate(|| {
                    // Every other round is a genuine no-op, so the test
                    // exercises the restore path under contention too.
                    let changed = round % 2 == 0;
                    if changed {
                        store.membership.fetch_add(1, Ordering::AcqRel);
                    }
                    // WIDEN the interval between the store write and the
                    // mutation's exit. Under the correct protocol this
                    // interval is covered by the in-flight window, so
                    // readers landing in it simply see no stable stamp.
                    // Under a post-mutation advance it is a window in
                    // which the store is already changed and the counter
                    // has not moved — so widening it is what turns this
                    // test from a lucky sampler into a reliable
                    // discriminator. The yield lives in the TEST's own
                    // mutation body; production code is untouched.
                    std::thread::yield_now();
                    ((), changed)
                });
            }
        })
    };

    let mut stable_pairs = 0_u64;
    let mut unstable = 0_u64;
    for _ in 0..40_000 {
        let Some(before) = store.generation.stable() else {
            unstable += 1;
            continue;
        };
        let membership = store.membership.load(Ordering::Acquire);
        let Some(after) = store.generation.stable() else {
            unstable += 1;
            continue;
        };
        if before != after {
            continue;
        }
        stable_pairs += 1;
        assert_eq!(
            membership * 2,
            before - base,
            "a reader whose generation read was STABLE across its store read observed membership \
             {membership} at generation {before}, but that generation denotes a different \
             membership — the reader straddled a mutation while seeing no movement, which is the \
             stale-serve the bracket exists to make impossible"
        );
    }

    writer.join().expect("writer thread must not panic");

    assert!(
        stable_pairs > 0,
        "reachability: the reader loop must actually observe stable pairs, or the assertion above \
         never ran and this test proves nothing (unstable observations: {unstable})"
    );
    assert_eq!(
        store.membership.load(Ordering::Acquire),
        rounds / 2,
        "control: the writer performed the advancing mutations it claims to have performed"
    );
}

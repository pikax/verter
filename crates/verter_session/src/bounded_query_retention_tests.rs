//! Unit tests for the bounded query-identity retention substrate
//! ([`super`]). Covers the [`GlobalRetentionBudget`] FIFO cap, the
//! [`BoundedCandidateMap`] per-slot + global bounds, the reader-`Arc`
//! survival and slot-detach invariants, and the map/budget
//! write-side consistency-domain race tests (`admit` vs `clear`,
//! `clear` vs `admit`, and the admit-vs-admit ghost-record race).

use super::*;

/// `GlobalRetentionBudget` returns the oldest `(seq, key)` victims
/// once the ledger exceeds the cap, in FIFO order. DISCRIMINATES: an
/// unbounded ledger would return an empty eviction list forever. The
/// victim's `seq` is the admission identity the caller scopes its
/// removal to — proven here by asserting the exact `(seq, key)` pair.
#[test]
fn budget_evicts_oldest_first_past_cap() {
    let budget: GlobalRetentionBudget<u32> = GlobalRetentionBudget::new(3);
    assert!(budget.record_admission(1, 10).is_empty(), "1st within cap");
    assert!(budget.record_admission(2, 11).is_empty(), "2nd within cap");
    assert!(budget.record_admission(3, 12).is_empty(), "3rd within cap");
    // 4th admission overflows — the oldest (seq 1, key 10) is
    // evicted; the victim carries its admission seq.
    assert_eq!(
        budget.record_admission(4, 13),
        vec![(1, 10)],
        "4th admission must evict the oldest (seq, key) victim (FIFO)",
    );
    // 5th — (seq 2, key 11) is now oldest.
    assert_eq!(
        budget.record_admission(5, 14),
        vec![(2, 11)],
        "5th admission must evict the next-oldest (seq, key) victim",
    );
    assert_eq!(budget.tracked_len(), 3, "ledger stays bounded at cap");
}

/// `forget_key_under_exclusive_lock` drops a key so a later overflow
/// does not return it.
#[test]
fn budget_forget_key_drops_key_from_ledger() {
    let budget: GlobalRetentionBudget<u32> = GlobalRetentionBudget::new(2);
    let _ = budget.record_admission(1, 100);
    let _ = budget.record_admission(2, 101);
    budget.forget_key_under_exclusive_lock(&100);
    assert_eq!(budget.tracked_len(), 1, "forget removed key 100");
    // Next admission does NOT overflow (only one tracked entry left).
    assert!(
        budget.record_admission(3, 102).is_empty(),
        "after forget the ledger is within cap again",
    );
}

/// `forget_seq` drops EXACTLY one admission by its seq — a
/// re-admission of the SAME key under a different seq survives. This
/// is the removal-identity property: a per-canonical drain that
/// races a concurrent re-admission must forget only the stale
/// admission, never the fresh one.
///
/// DISCRIMINATES against a key-wide `forget`: a key-wide removal of
/// key 200 would drop BOTH the seq-1 and seq-3 records, leaving
/// `tracked_len() == 1`; the seq-scoped removal drops only seq 1, so
/// `tracked_len() == 2` and the fresh seq-3 record is still counted.
#[test]
fn budget_forget_seq_preserves_same_key_readmission() {
    let budget: GlobalRetentionBudget<u32> = GlobalRetentionBudget::new(8);
    // Two admissions of the SAME key under distinct seqs — models a
    // stale admission and a concurrent fresh re-admission.
    let _ = budget.record_admission(1, 200);
    let _ = budget.record_admission(2, 201);
    let _ = budget.record_admission(3, 200);
    assert_eq!(budget.tracked_len(), 3, "three admissions recorded");
    // Forget ONLY the stale seq-1 admission of key 200.
    budget.forget_seq(1);
    assert_eq!(
        budget.tracked_len(),
        2,
        "forget_seq must drop exactly the seq-1 record — the fresh \
         seq-3 re-admission of the same key 200 survives; a key-wide \
         forget would have dropped it too",
    );
}

/// A zero cap is clamped to one — the cache always keeps its newest
/// entry.
#[test]
fn budget_zero_cap_clamps_to_one() {
    let budget: GlobalRetentionBudget<u32> = GlobalRetentionBudget::new(0);
    assert_eq!(budget.cap(), 1);
    assert!(budget.record_admission(1, 1).is_empty());
    assert_eq!(
        budget.record_admission(2, 2),
        vec![(1, 1)],
        "second admission evicts the first (seq, key) victim under a cap of 1",
    );
}

/// `BoundedCandidateMap` keeps at most `per_slot_cap` candidates in
/// one slot, evicting the oldest. DISCRIMINATES: an unbounded slot
/// would retain all five.
#[test]
fn candidate_map_bounds_one_slot_at_per_slot_cap() {
    let map: BoundedCandidateMap<&str, u8, u32> = BoundedCandidateMap::with_caps(3, 100);
    for v in 0u8..5 {
        map.admit("owner", v, u32::from(v));
    }
    let slot = map.slot_candidates(&"owner");
    assert_eq!(
        slot.len(),
        3,
        "one slot must retain at most per_slot_cap (3) candidates",
    );
    // Oldest two discriminants (0, 1) evicted; newest three remain.
    let mut discs: Vec<u8> = slot.iter().map(|c| c.discriminant).collect();
    discs.sort_unstable();
    assert_eq!(discs, vec![2, 3, 4], "oldest candidates evicted first");
    assert_eq!(map.live_count(), 3);
}

/// Re-admitting the same discriminant replaces the candidate in
/// place — the slot does not grow.
#[test]
fn candidate_map_same_discriminant_replaces_in_place() {
    let map: BoundedCandidateMap<&str, u8, u32> = BoundedCandidateMap::with_caps(4, 100);
    map.admit("owner", 7, 1);
    map.admit("owner", 7, 2);
    map.admit("owner", 7, 3);
    let slot = map.slot_candidates(&"owner");
    assert_eq!(
        slot.len(),
        1,
        "same-discriminant re-admit must not grow the slot"
    );
    assert_eq!(slot[0].value, 3, "re-admit refreshes the payload");
}

/// The global budget caps the total candidate count across distinct
/// slots — the per-slot cap alone would not bound a workload that
/// edits many DISTINCT owners once each.
#[test]
fn candidate_map_global_budget_bounds_across_slots() {
    let map: BoundedCandidateMap<u32, u8, u32> = BoundedCandidateMap::with_caps(4, 5);
    // 12 distinct slots, one candidate each — per-slot cap never
    // triggers, only the global cap of 5 does.
    for owner in 0u32..12 {
        map.admit(owner, 0, owner);
    }
    assert!(
        map.live_count() <= 5,
        "global budget must bound total candidates across slots — got {}",
        map.live_count(),
    );
}

/// A reader's cloned candidate `Arc` survives a concurrent eviction
/// of that candidate — removal does not invalidate an in-flight
/// reader.
#[test]
fn candidate_map_reader_arc_survives_eviction() {
    let map: BoundedCandidateMap<&str, u8, u32> = BoundedCandidateMap::with_caps(4, 100);
    map.admit("owner", 1, 42);
    let held = map.get_candidate(&"owner", &1).expect("candidate present");
    let seq = held.seq;
    // Evict the exact candidate the reader is holding.
    assert!(map.evict_candidate(&"owner", seq), "candidate evicted");
    assert!(
        map.get_candidate(&"owner", &1).is_none(),
        "evicted candidate is gone from the map",
    );
    // The reader's clone is still valid and unchanged.
    assert_eq!(held.value, 42, "in-flight reader's Arc still resolves");
}

/// SLOT-DETACH REGRESSION — a candidate admitted into a slot that
/// was just emptied (and therefore eligible for opportunistic
/// detach) MUST remain reachable by `get_candidate` and counted by
/// `live_count`.
///
/// Sequence: admit one candidate; remove it — the slot is now empty
/// and `remove_candidate_by_seq`'s `remove_if` detaches it from the
/// outer map. Then admit a fresh candidate under the SAME key:
/// `admit` re-creates the slot via `entry().or_insert_with()`. The
/// fresh candidate must be visible. A pre-fix `admit` that cloned the
/// slot `Arc` and dropped the shard guard before pushing could push
/// into a slot a concurrent reaper had detached; this test pins the
/// post-detach re-admit path so a regression to that shape would
/// leave the re-admitted candidate invisible (`live_count() == 0`).
#[test]
fn candidate_map_readmit_into_emptied_slot_stays_visible() {
    let map: BoundedCandidateMap<&str, u8, u32> = BoundedCandidateMap::with_caps(4, 100);
    // Admit, then evict — the slot is emptied and detached.
    map.admit("owner", 1, 100);
    let first = map.get_candidate(&"owner", &1).expect("first admitted");
    assert!(
        map.evict_candidate(&"owner", first.seq),
        "first candidate evicted — slot now empty + detached",
    );
    assert_eq!(map.live_count(), 0, "slot emptied");
    assert!(map.slot_candidates(&"owner").is_empty());

    // Re-admit under the same key. The fresh candidate MUST be
    // reachable — `admit` re-attaches the slot under its shard guard.
    map.admit("owner", 2, 200);
    let readmitted = map
        .get_candidate(&"owner", &2)
        .expect("re-admitted candidate must be reachable after slot detach");
    assert_eq!(readmitted.value, 200, "re-admitted payload visible");
    assert_eq!(
        map.live_count(),
        1,
        "re-admitted candidate must be counted by live_count — a \
         candidate pushed into a detached slot would read as 0",
    );
    assert_eq!(
        map.slot_candidates(&"owner").len(),
        1,
        "the re-attached slot holds exactly the re-admitted candidate",
    );
}

/// SLOT-DETACH RACE under real thread contention — a reaper emptying
/// a slot must never strand a concurrent admitter's candidate. Many
/// rounds of `{admit / evict-then-readmit}` race on one key; after
/// every round the key's live candidate must be reachable. Drives
/// the shard-guard serialisation between `admit`'s push and
/// `remove_if`'s detach.
#[test]
fn candidate_map_concurrent_admit_vs_detach_never_strands() {
    use std::sync::Arc as StdArc;
    use std::thread;

    let map: StdArc<BoundedCandidateMap<u32, u8, u32>> =
        StdArc::new(BoundedCandidateMap::with_caps(4, 4096));
    // Seed every key so the first reaper round has something to
    // evict (and thus a slot to attempt to detach).
    for key in 0u32..8 {
        map.admit(key, 0, key);
    }

    let rounds = 400usize;
    let reaper = {
        let map = StdArc::clone(&map);
        thread::spawn(move || {
            for r in 0..rounds {
                let key = (r % 8) as u32;
                // Evict whatever candidate currently holds disc 0,
                // emptying + detaching the slot, then re-admit it.
                if let Some(c) = map.get_candidate(&key, &0) {
                    map.evict_candidate(&key, c.seq);
                }
                map.admit(key, 0, key);
            }
        })
    };
    let admitter = {
        let map = StdArc::clone(&map);
        thread::spawn(move || {
            for r in 0..rounds {
                let key = (r % 8) as u32;
                // Admit a second discriminant concurrently with the
                // reaper churning disc 0 on the same slot.
                map.admit(key, 1, key + 1000);
            }
        })
    };
    reaper.join().expect("reaper thread");
    admitter.join().expect("admitter thread");

    // Every key must still resolve disc 0 — the reaper's final
    // re-admit cannot have been stranded in a detached slot.
    for key in 0u32..8 {
        assert!(
            map.get_candidate(&key, &0).is_some(),
            "key {key}: disc-0 candidate stranded by a slot-detach race",
        );
    }
    // Total live count must reflect reachable candidates only — a
    // stranded candidate would be recorded in the budget but absent
    // from `live_count`, so `live_count` would not exceed the number
    // of reachable candidates. Assert at least the 8 disc-0 entries
    // are reachable and counted.
    assert!(
        map.live_count() >= 8,
        "live_count must count every reachable candidate — observed {}",
        map.live_count(),
    );
}

/// `evict_slot` drops every version in a slot and forgets them all
/// from the budget.
#[test]
fn candidate_map_evict_slot_drops_all_versions() {
    let map: BoundedCandidateMap<&str, u8, u32> = BoundedCandidateMap::with_caps(4, 100);
    map.admit("a", 0, 1);
    map.admit("a", 1, 2);
    map.admit("b", 0, 3);
    assert_eq!(map.evict_slot(&"a"), 2, "both versions of slot a removed");
    assert_eq!(map.live_count(), 1, "only slot b survives");
    assert!(map.slot_candidates(&"a").is_empty());
}

/// MAP / BUDGET DESYNC RACE (admit side) — an in-flight `admit`
/// must engage the `retention_gate` so a concurrent `clear` (which
/// mutates both `slots` and `budget`) cannot interleave its two
/// clears with the admit's two-phase slot-push + budget-admission.
///
/// Deterministic. The admitter is parked, via the `admit` injection
/// point, AFTER its `slots.push` + `budget.record_admission` have
/// landed but BEFORE `admit` returns — i.e. while `admit` still
/// holds its `retention_gate` read guard. With the admitter pinned
/// at that point the test asserts `retention_gate.try_write()` is
/// `None`: a `clear` reaching `retention_gate.write()` right now
/// WOULD block, so it cannot run its `slots.clear()` / `budget.clear()`
/// pair concurrently with the admit's half-applied update.
///
/// DISCRIMINATES. Against the un-gated `admit` (read guard removed)
/// the in-flight admit holds nothing, so `try_write()` succeeds
/// (`Some`) — a `clear` could interleave and strand a live slot
/// candidate with no budget record. The assertion `try_write()` is
/// `None` FAILS. With the gate the in-flight admit holds the read
/// guard, `try_write()` returns `None`, the assertion PASSES. The
/// follow-up sequential `clear` + `admit` confirms the gate is a
/// reset fence, not a permanent block.
#[test]
fn candidate_map_inflight_admit_engages_gate_against_clear() {
    use std::sync::Arc as StdArc;
    use std::sync::Barrier;
    use std::thread;

    let map: StdArc<BoundedCandidateMap<u32, u8, u32>> =
        StdArc::new(BoundedCandidateMap::with_caps(4, 4096));

    // admit_parked: party 1 = the parked `admit`, party 2 = main.
    let admit_parked = StdArc::new(Barrier::new(2));
    let _admit_guard = map.test_arm_admit_post_record_gate(StdArc::clone(&admit_parked));

    let map_admit = StdArc::clone(&map);
    let admitter = thread::spawn(move || map_admit.admit(7, 0, 700));

    // Wait until `admit` has pushed its candidate, recorded its
    // budget admission, and parked — still inside `admit`, still
    // holding the `retention_gate` read guard.
    admit_parked.wait();

    // A `clear` taking `retention_gate.write()` right now WOULD
    // block: the in-flight `admit` holds the read guard. `try_write`
    // returning `None` is the proof that `clear`'s `slots.clear()` /
    // `budget.clear()` cannot interleave the admit's half-applied
    // two-phase update.
    assert!(
        map.test_retention_gate().try_write().is_none(),
        "MAP/BUDGET DESYNC: an in-flight `admit` does NOT hold the \
         retention gate, so a concurrent `clear` could interleave its \
         slots.clear()/budget.clear() between the admit's slot push \
         and budget admission — stranding a live candidate with no \
         budget record. The `admit` must hold `retention_gate.read()` \
         across its whole map+budget mutation.",
    );

    // Release the parked admit; it drops the read guard and returns.
    admit_parked.wait();
    let outcome = admitter.join().expect("admitter thread");
    assert_eq!(
        outcome,
        AdmitOutcome {
            fresh: true,
            evicted: 0
        },
        "admit of a fresh key adds one candidate and evicts nothing",
    );
    assert_eq!(map.live_count(), 1, "the admitted candidate is live");
    assert_eq!(map.budget.tracked_len(), 1, "and tracked by the budget");

    // Disarm the `admit` injection point BEFORE the follow-up
    // mutations — otherwise the next `admit` would park on a stale
    // barrier with no second party.
    drop(_admit_guard);

    // The gate is a reset fence, not a permanent block: a `clear`
    // now runs to completion and leaves map + budget consistent.
    assert_eq!(map.clear(), 1, "clear drops the one live candidate");
    assert_eq!(map.live_count(), 0);
    assert_eq!(map.budget.tracked_len(), 0);
    map.admit(8, 0, 800);
    assert_eq!(
        map.live_count(),
        map.budget.tracked_len(),
        "post-clear admit keeps map and budget consistent",
    );
}

/// MAP / BUDGET DESYNC RACE (clear side) — an in-flight `clear`
/// must hold the `retention_gate` write guard across BOTH its
/// `slots.clear()` and `budget.clear()`, so a concurrent `admit`
/// cannot land a slot candidate + budget admission that straddle
/// the two clears.
///
/// Deterministic. The invalidator is parked, via the `clear`
/// injection point, BETWEEN `slots.clear()` and `budget.clear()` —
/// i.e. while `clear` still holds its `retention_gate` write guard.
/// With `clear` pinned there the test asserts `retention_gate.try_read()`
/// is `None`: an `admit` reaching `retention_gate.read()` right now
/// WOULD block, so it cannot push a candidate into the just-cleared
/// `slots` and record an admission the pending `budget.clear()`
/// would then erase.
///
/// DISCRIMINATES. Against the un-gated `clear` (write guard removed)
/// the in-flight clear holds nothing, so `try_read()` succeeds
/// (`Some`) — an `admit` could interleave into the gap. The
/// assertion `try_read()` is `None` FAILS. With the gate the
/// in-flight clear holds the write guard, `try_read()` returns
/// `None`, the assertion PASSES.
#[test]
fn candidate_map_inflight_clear_engages_gate_against_admit() {
    use std::sync::Arc as StdArc;
    use std::sync::Barrier;
    use std::thread;

    let map: StdArc<BoundedCandidateMap<u32, u8, u32>> =
        StdArc::new(BoundedCandidateMap::with_caps(4, 4096));
    // Seed one candidate so `clear` has a slot to drop.
    map.admit(1, 0, 100);

    // clear_parked: party 1 = the parked `clear`, party 2 = main.
    let clear_parked = StdArc::new(Barrier::new(2));
    let _clear_guard = map.test_arm_clear_midpoint_gate(StdArc::clone(&clear_parked));

    let map_clear = StdArc::clone(&map);
    let invalidator = thread::spawn(move || map_clear.clear());

    // Wait until `clear` has run `slots.clear()` and parked at its
    // midpoint — still inside `clear`, still holding the
    // `retention_gate` write guard, `budget.clear()` not yet run.
    clear_parked.wait();

    // An `admit` taking `retention_gate.read()` right now WOULD
    // block: the in-flight `clear` holds the write guard. `try_read`
    // returning `None` is the proof that `admit` cannot push a
    // candidate into the just-cleared `slots` and record a budget
    // admission the pending `budget.clear()` would then erase.
    assert!(
        map.test_retention_gate().try_read().is_none(),
        "MAP/BUDGET DESYNC: an in-flight `clear` does NOT hold the \
         retention write guard, so a concurrent `admit` could push a \
         candidate into the just-cleared slot map and record a budget \
         admission that the pending `budget.clear()` then erases — \
         stranding a live candidate with no budget record. `clear` \
         must hold `retention_gate.write()` across both clears.",
    );

    // Release the parked clear; it runs `budget.clear()`, drops the
    // write guard, and returns.
    clear_parked.wait();
    let removed = invalidator.join().expect("invalidator thread");
    assert_eq!(removed, 1, "clear removed the one seeded candidate");
    assert_eq!(
        map.live_count(),
        map.budget.tracked_len(),
        "after the clear, map and budget are both empty and consistent",
    );
}

/// ADMIT-VS-ADMIT GHOST-RECORD RACE — two concurrent admits at the
/// SAME content-free slot must not let one admit record a budget
/// ledger seq for a candidate the other admit replaced+forgot. The
/// slot mutation AND the budget `record_admission` must run inside
/// ONE continuously-held slot-lock critical section.
///
/// Deterministic — no `sleep`, no lock-timeout. The slot key is
/// content-free, so both admits target the same slot; `per_slot_cap`
/// is `1`, so a second candidate pushed into the slot FIFO-evicts
/// the first (oldest-by-seq).
///
/// Sequence. Admit-A is parked, via the pre-budget `admit` injection
/// point, AFTER it pushed its candidate (seq_A) into the slot and
/// ran its (empty) removed-seq `forget_seq`, but BEFORE its
/// `record_admission`. Admit-B is then started on the SAME slot.
/// The test probes, while A is still parked, whether the `DashMap`
/// shard for the key is write-locked.
///
/// DISCRIMINATES — deterministically — on the lock domain.
///   - POST-fix: `record_admission` runs inside the slot block, so
///     the parked admit-A still holds the `entry()` shard write
///     guard for the key. `test_key_shard_locked` returns `true`.
///     A concurrent admit-B therefore blocks at `slots.entry(key)`
///     and cannot record between A's slot mutation and A's
///     `record_admission`. The shard-locked assertion PASSES; when
///     A is released the two admits serialise on the slot lock and
///     the ledger ends consistent.
///   - PRE-fix: `admit` drops the slot lock + shard guard before
///     `record_admission`, so the parked admit-A holds nothing.
///     `test_key_shard_locked` returns `false` — the shard-locked
///     assertion FAILS. (And the gap is real: admit-B races into it,
///     evicts+forgets A's candidate, and A then records a ghost seq
///     — the end-state assertions would also fail, but the
///     deterministic discriminator is the shard guard.)
///
/// `budget.tracked_len()` is exactly the count
/// `GlobalRetentionBudget::record_admission` compares against the
/// global cap, so a ghost record is a wrongly-FIFO-counted entry.
#[test]
fn candidate_map_concurrent_admit_no_ghost_budget_record() {
    use std::sync::Arc as StdArc;
    use std::sync::Barrier;
    use std::thread;

    // per_slot_cap = 1: a second candidate evicts the first.
    let map: StdArc<BoundedCandidateMap<u32, u8, u32>> =
        StdArc::new(BoundedCandidateMap::with_caps(1, 4096));

    // admit_a_parked: party 1 = the parked admit-A, party 2 = main.
    let admit_a_parked = StdArc::new(Barrier::new(2));
    let guard = map.test_arm_admit_pre_budget_gate(StdArc::clone(&admit_a_parked));

    // Admit-A: pushes candidate (disc 0) into the empty slot, runs
    // its empty removed-seq `forget_seq`, then parks at the
    // pre-budget injection point — BEFORE `record_admission`.
    let map_a = StdArc::clone(&map);
    let admit_a = thread::spawn(move || map_a.admit(1, 0, 100));

    // Wait until admit-A has pushed its candidate and parked.
    admit_a_parked.wait();

    // Disarm the injection point BEFORE starting admit-B so B does
    // NOT park — B is the admit that contends for A's slot.
    drop(guard);

    // Admit-B on the SAME slot, a distinct discriminant.
    //   - POST-fix: A holds the slot lock + shard guard → B blocks
    //     at `slots.entry(1)` until A is released.
    //   - PRE-fix: A holds nothing → B runs into A's gap.
    let map_b = StdArc::clone(&map);
    let admit_b = thread::spawn(move || map_b.admit(1, 1, 200));

    // THE DETERMINISTIC DISCRIMINATOR — with admit-A still parked at
    // the pre-budget point, the key's `DashMap` shard MUST be
    // write-locked: the single-lock-domain `admit` holds the
    // `entry()` shard guard across `record_admission`. A pre-fix
    // `admit` released the shard guard before `record_admission`,
    // so the shard is unlocked here.
    assert!(
        map.test_key_shard_locked(&1),
        "ADMIT-VS-ADMIT GHOST RECORD: an `admit` parked just before \
         `record_admission` MUST still hold the `entry()` shard \
         write guard for its key — the slot mutation and the budget \
         `record_admission` must run inside one slot-lock critical \
         section. A pre-fix `admit` drops the slot lock + shard \
         guard before `record_admission`, so a concurrent same-slot \
         admit can replace+forget this admit's candidate before it \
         records, leaving a ghost ledger seq for an already-evicted \
         candidate.",
    );

    // Release the parked admit-A. POST-fix this is what lets B make
    // progress; the two admits then serialise on the slot lock.
    admit_a_parked.wait();

    let outcome_a = admit_a.join().expect("admit-A thread");
    let outcome_b = admit_b.join().expect("admit-B thread");

    // The slot holds exactly one candidate (per_slot_cap = 1) and it
    // is B's — B's admit evicted A's older candidate.
    let slot = map.slot_candidates(&1);
    assert_eq!(slot.len(), 1, "per_slot_cap = 1 — one candidate survives");
    assert_eq!(
        slot[0].discriminant, 1,
        "the surviving candidate is admit-B's (it evicted A's older one)",
    );
    assert_eq!(slot[0].value, 200, "admit-B's payload is the live one");

    // End-state consistency — no ghost ledger record: the budget
    // ledger holds exactly one record per live candidate.
    assert_eq!(
        map.live_count(),
        map.budget.tracked_len(),
        "the budget ledger must hold exactly one record per live \
         candidate — a ghost record from a non-atomic admit would \
         leave tracked_len() > live_count()",
    );
    assert_eq!(map.live_count(), 1, "exactly one live candidate");
    assert_eq!(map.budget.tracked_len(), 1, "exactly one ledger record");

    // Both admits report a fresh append. Across both, exactly one
    // candidate was per-slot-evicted (B's append evicted A's older
    // candidate); the per-admit split depends on scheduling order,
    // so only the invariant sum is asserted.
    assert!(outcome_a.fresh && outcome_b.fresh, "both admits appended");
    assert_eq!(
        outcome_a.evicted + outcome_b.evicted,
        1,
        "exactly one per-slot eviction across the two admits",
    );

    // The global cap stays honoured.
    assert!(map.live_count() <= map.global_cap(), "global cap honoured");
}

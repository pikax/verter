//! Aggregate semantic-retention account: admission, pressure, and
//! exactly-once release.
//!
//! The boundary these tests defend is the one the account exists for: a
//! process that holds many individually-legal cache entries must still
//! stop at the ratified aggregate ceiling, and every way a reservation
//! can end — publish, refusal, cancellation, a failed build, ownership
//! transfer, eviction — must release its bytes once and only once.
//!
//! Every test builds a PRIVATE account. The production account is
//! process-local and shared by every project; a test that drove pressure
//! on it would make a concurrently-running test's admission depend on
//! this one's timing.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use super::semantic_retention_account::{
    ChargeClass, RetainedFootprint, RetentionAdmission, RetentionCharge, RetentionLimits,
    RetentionRefusal, SemanticRetentionAccount,
};

fn account(aggregate: usize, active: usize, max_entry: usize) -> Arc<SemanticRetentionAccount> {
    SemanticRetentionAccount::new(RetentionLimits {
        aggregate_ceiling_bytes: aggregate,
        active_ceiling_bytes: active,
        max_entry_bytes: max_entry,
        // Pin pressure is a reporting threshold, not an admission gate,
        // so the admission tests park it at the aggregate ceiling and
        // `pin_pressure_is_reported_without_gating_admission` drives it
        // explicitly.
        pin_threshold_bytes: aggregate,
    })
}

/// Pin pressure is REPORTED, never enforced: crossing the threshold
/// changes the diagnosis (live handles, not cache growth) without
/// refusing the pin or changing any admission decision.
#[test]
fn pin_pressure_is_reported_without_gating_admission() {
    let account = SemanticRetentionAccount::new(RetentionLimits {
        aggregate_ceiling_bytes: 1_000,
        active_ceiling_bytes: 1_000,
        max_entry_bytes: 1_000,
        pin_threshold_bytes: 100,
    });
    assert!(!account.under_pin_pressure());
    let under = account.pin(100);
    assert!(
        !account.under_pin_pressure(),
        "the threshold is crossed strictly above, not at"
    );
    let over = account.pin(1);
    assert!(account.under_pin_pressure());
    // Admission still decides purely on headroom — 101 B pinned leaves
    // 899 B, and a reservation inside it is admitted despite the report.
    assert!(matches!(
        account.reserve(ChargeClass::Retained, 899),
        RetentionAdmission::Admitted(_)
    ));
    drop(over);
    drop(under);
    assert!(!account.under_pin_pressure());
}

// ──────────────────────────────────────────────────────────────────────
// MEM1-AC1 — the aggregate ceiling is never exceeded
// ──────────────────────────────────────────────────────────────────────

/// Many DISTINCT keys, each individually well under the per-entry cap,
/// must still stop at the aggregate ceiling. This is the discriminating
/// case for an aggregate account: a per-entry or per-family limit alone
/// admits every one of these and overshoots by 10x.
#[test]
fn many_individually_legal_entries_stop_at_the_aggregate_ceiling() {
    let account = account(1_000, 1_000, 1_000);
    let mut held: Vec<RetentionCharge> = Vec::new();
    let mut refusals = 0usize;
    for _ in 0..100 {
        match account.reserve(ChargeClass::Retained, 100) {
            RetentionAdmission::Admitted(charge) => held.push(charge),
            RetentionAdmission::Refused(RetentionRefusal::Pressure { .. }) => refusals += 1,
            RetentionAdmission::Refused(other) => panic!("unexpected refusal: {other}"),
        }
        assert!(
            account.snapshot().total_bytes() <= 1_000,
            "aggregate occupancy {} passed the ceiling",
            account.snapshot().total_bytes()
        );
    }
    assert_eq!(held.len(), 10, "exactly the ceiling's worth may be held");
    assert_eq!(refusals, 90, "every further entry must be refused");
    assert_eq!(account.snapshot().retained_bytes, 1_000);
}

/// Concurrent admitters must not jointly overshoot. A check-then-reserve
/// implementation passes the serial test above and fails this one: two
/// threads both observe headroom and both admit.
#[test]
fn concurrent_admissions_never_jointly_overshoot_the_ceiling() {
    const THREADS: usize = 8;
    const PER_THREAD: usize = 40;
    let account = account(4_000, 4_000, 4_000);
    let barrier = Arc::new(Barrier::new(THREADS));
    let admitted = Arc::new(AtomicUsize::new(0));
    let overshoot = Arc::new(AtomicUsize::new(0));
    std::thread::scope(|scope| {
        for _ in 0..THREADS {
            let account = Arc::clone(&account);
            let barrier = Arc::clone(&barrier);
            let admitted = Arc::clone(&admitted);
            let overshoot = Arc::clone(&overshoot);
            scope.spawn(move || {
                // Charges are HELD for the whole run so the ceiling is
                // genuinely contended rather than recycled between
                // reservations.
                let mut held: Vec<RetentionCharge> = Vec::new();
                barrier.wait();
                for _ in 0..PER_THREAD {
                    if let RetentionAdmission::Admitted(charge) =
                        account.reserve(ChargeClass::Retained, 100)
                    {
                        admitted.fetch_add(1, Ordering::Relaxed);
                        held.push(charge);
                    }
                    if account.snapshot().total_bytes() > 4_000 {
                        overshoot.fetch_add(1, Ordering::Relaxed);
                    }
                }
                std::mem::forget(held);
            });
        }
    });
    assert_eq!(
        overshoot.load(Ordering::Relaxed),
        0,
        "a concurrent admission observed occupancy past the ceiling"
    );
    assert_eq!(
        admitted.load(Ordering::Relaxed),
        40,
        "exactly ceiling/entry admissions may succeed across all threads"
    );
}

/// Live pins and discretionary admissions share the same aggregate
/// transition: pins leave only their actual headroom, never a stale
/// pre-pin allowance for retained entries.
#[test]
fn concurrent_pins_limit_retained_admissions_to_their_headroom() {
    const PINNERS: usize = 4;
    const ADMITTERS: usize = 8;
    const PER_THREAD: usize = 40;
    const CEILING: usize = 4_000;
    const PIN_BYTES: usize = 500;

    let account = account(CEILING, CEILING, CEILING);
    let pins_ready = Arc::new(Barrier::new(PINNERS + 1));
    let release_pins = Arc::new(Barrier::new(PINNERS + 1));
    let admitted = Arc::new(AtomicUsize::new(0));
    let overshoot = Arc::new(AtomicUsize::new(0));

    std::thread::scope(|scope| {
        let mut pin_handles = Vec::with_capacity(PINNERS);
        for _ in 0..PINNERS {
            let account = Arc::clone(&account);
            let pins_ready = Arc::clone(&pins_ready);
            let release_pins = Arc::clone(&release_pins);
            pin_handles.push(scope.spawn(move || {
                let _pin = account.pin(PIN_BYTES);
                pins_ready.wait();
                release_pins.wait();
            }));
        }
        pins_ready.wait();

        let mut admission_handles = Vec::with_capacity(ADMITTERS);
        for _ in 0..ADMITTERS {
            let account = Arc::clone(&account);
            let admitted = Arc::clone(&admitted);
            let overshoot = Arc::clone(&overshoot);
            admission_handles.push(scope.spawn(move || {
                let mut held: Vec<RetentionCharge> = Vec::new();
                for _ in 0..PER_THREAD {
                    if let RetentionAdmission::Admitted(charge) =
                        account.reserve(ChargeClass::Retained, 100)
                    {
                        admitted.fetch_add(1, Ordering::Relaxed);
                        held.push(charge);
                    }
                    if account.snapshot().total_bytes() > CEILING {
                        overshoot.fetch_add(1, Ordering::Relaxed);
                    }
                }
                std::mem::forget(held);
            }));
        }
        for handle in admission_handles {
            handle.join().expect("admitter thread must not panic");
        }
        release_pins.wait();
        for handle in pin_handles {
            handle.join().expect("pinner thread must not panic");
        }
    });

    assert_eq!(
        overshoot.load(Ordering::Relaxed),
        0,
        "a retained admission exceeded headroom consumed by live pins"
    );
    assert_eq!(
        admitted.load(Ordering::Relaxed),
        20,
        "only ceiling minus pinned bytes may be retained"
    );
    assert_eq!(account.snapshot().pinned_bytes, 0);
}

/// Pinned bytes are charged and consume headroom, so a process holding
/// live parse snapshots admits fewer discretionary cache entries.
#[test]
fn pinned_bytes_consume_headroom_for_refusable_admissions() {
    let account = account(1_000, 1_000, 1_000);
    let pin = account.pin(800);
    assert_eq!(account.snapshot().pinned_bytes, 800);
    assert!(matches!(
        account.reserve(ChargeClass::Retained, 400),
        RetentionAdmission::Refused(RetentionRefusal::Pressure { .. })
    ));
    let fits = account.reserve(ChargeClass::Retained, 200);
    assert!(matches!(fits, RetentionAdmission::Admitted(_)));
    drop(pin);
    // With the pin gone the headroom returns; the still-held 200 B
    // retained charge is the only remaining occupancy.
    assert_eq!(account.snapshot().pinned_bytes, 0);
    assert!(matches!(
        account.reserve(ChargeClass::Retained, 700),
        RetentionAdmission::Admitted(_)
    ));
}

/// A pin is a liveness obligation, not a policy choice: it is charged
/// unconditionally even past the ceiling, because refusing one would
/// force a live artifact to re-parse or revoke a live handle.
#[test]
fn a_pin_is_never_refused_even_past_the_ceiling() {
    let account = account(100, 100, 100);
    let first = account.pin(90);
    let second = account.pin(90);
    assert_eq!(account.snapshot().pinned_bytes, 180);
    assert!(
        account.refusable_headroom_bytes() == 0,
        "an over-ceiling pin must leave zero discretionary headroom"
    );
    drop(first);
    drop(second);
    assert_eq!(account.snapshot().pinned_bytes, 0);
}

// ──────────────────────────────────────────────────────────────────────
// MEM1-AC2 — refusal returns a complete value uncached
// ──────────────────────────────────────────────────────────────────────

/// An entry larger than the per-reservation cap is refused as oversized
/// WITHOUT consulting occupancy — an empty account still refuses it, so
/// one outlier can never evict an unbounded number of useful entries.
#[test]
fn an_oversized_entry_is_refused_on_an_empty_account() {
    let account = account(10_000, 10_000, 500);
    let refusal = account
        .reserve(ChargeClass::Retained, 501)
        .refusal()
        .expect("an entry past the per-entry cap must be refused");
    assert!(matches!(refusal, RetentionRefusal::Oversized { .. }));
    assert_eq!(
        account.snapshot().total_bytes(),
        0,
        "a refused reservation must charge nothing"
    );
    assert_eq!(account.snapshot().refusals_oversized, 1);
}

/// Exhausted ACTIVE work reports its own typed outcome, distinct from
/// ordinary retention pressure: the request is out of resource, not
/// merely unable to warm a cache.
#[test]
fn exhausted_active_work_reports_the_active_resource_outcome() {
    let account = account(10_000, 500, 10_000);
    let held = account
        .reserve(ChargeClass::Active, 400)
        .admitted()
        .expect("the first active reservation fits");
    let refusal = account
        .reserve(ChargeClass::Active, 200)
        .refusal()
        .expect("the active sub-limit must refuse");
    assert!(matches!(refusal, RetentionRefusal::ActiveExhausted { .. }));
    // The refused reservation must leave NO residue on either the active
    // class or the aggregate: a sub-limit rejection that forgot to unwind
    // its aggregate charge would leak 200 B here.
    let snapshot = account.snapshot();
    assert_eq!(snapshot.active_bytes, 400);
    assert_eq!(snapshot.total_bytes(), 400);
    // Retained work is unaffected by the active sub-limit.
    assert!(matches!(
        account.reserve(ChargeClass::Retained, 5_000),
        RetentionAdmission::Admitted(_)
    ));
    drop(held);
    assert_eq!(account.snapshot().active_bytes, 0);
}

/// Every refusal maps onto the locally-confined cache-runtime reason: a
/// retention refusal declines to STORE a complete value, so it must not
/// poison an enclosing derivation that consumed it.
#[test]
fn every_refusal_maps_to_a_locally_confined_non_admission_reason() {
    use crate::cache_runtime::admission::non_admission_propagation;
    use crate::resolver_core::fact_read_set::NonCacheablePropagation;
    for refusal in [
        RetentionRefusal::Oversized {
            requested: 2,
            max_entry_bytes: 1,
        },
        RetentionRefusal::Pressure {
            requested: 2,
            headroom: 1,
        },
        RetentionRefusal::ActiveExhausted {
            requested: 2,
            headroom: 1,
        },
    ] {
        assert_eq!(
            non_admission_propagation(refusal.non_admission_reason()),
            NonCacheablePropagation::LocalOnly,
            "{refusal} must stay confined to the refusing cache family"
        );
    }
}

// ──────────────────────────────────────────────────────────────────────
// MEM1-AC3 — exactly-once release across every ending
// ──────────────────────────────────────────────────────────────────────

/// A charge released by drop, by an abandoned build, and by an
/// ownership transfer each release EXACTLY once. A double release would
/// under-report occupancy and let the process drift past the ceiling; a
/// missed release would strand bytes forever.
#[test]
fn a_charge_releases_exactly_once_across_every_ending() {
    let account = account(10_000, 10_000, 10_000);

    // 1. Ordinary drop.
    {
        let _charge = account
            .reserve(ChargeClass::Retained, 100)
            .admitted()
            .expect("fits");
        assert_eq!(account.snapshot().retained_bytes, 100);
    }
    assert_eq!(account.snapshot().retained_bytes, 0);

    // 2. An abandoned build: the charge is created, the build bails out
    //    on an early return, and the bytes come back with no explicit
    //    release call anywhere on the error path.
    fn failed_build(account: &Arc<SemanticRetentionAccount>) -> Option<()> {
        let _charge = account
            .reserve(ChargeClass::Active, 100)
            .admitted()
            .expect("fits");
        None?;
        Some(())
    }
    assert!(failed_build(&account).is_none());
    assert_eq!(account.snapshot().total_bytes(), 0);

    // 3. Ownership transfer: moving the charge must NOT release, and the
    //    new owner's drop must release once.
    let charge = account
        .reserve(ChargeClass::Retained, 100)
        .admitted()
        .expect("fits");
    let moved = Some(charge);
    assert_eq!(account.snapshot().retained_bytes, 100);
    drop(moved);
    assert_eq!(account.snapshot().retained_bytes, 0);
    assert_eq!(account.snapshot().releases, 3);
}

/// Committing an in-flight charge to retained is gap-free: the bytes
/// stay charged across the transition (so a concurrent reservation can
/// never slip into an unaccounted window) while the ACTIVE sub-limit
/// pressure is released.
#[test]
fn committing_active_to_retained_never_drops_the_charge() {
    let account = account(10_000, 150, 10_000);
    let charge = account
        .reserve(ChargeClass::Active, 100)
        .admitted()
        .expect("fits");
    assert_eq!(account.snapshot().active_bytes, 100);
    let charge = charge.commit_retained();
    let snapshot = account.snapshot();
    assert_eq!(snapshot.active_bytes, 0, "active pressure is released");
    assert_eq!(snapshot.retained_bytes, 100, "bytes stay charged");
    assert_eq!(charge.class(), ChargeClass::Retained);
    // The freed active headroom is genuinely usable again.
    assert!(matches!(
        account.reserve(ChargeClass::Active, 140),
        RetentionAdmission::Admitted(_)
    ));
    drop(charge);
    assert_eq!(account.snapshot().retained_bytes, 0);
}

/// A charge shared by several owners counts ONCE. This is what a
/// same-payload cache backfill needs: the clone shares the payload's
/// allocations, so charging per clone would multiply one allocation by
/// its reference count.
#[test]
fn a_shared_charge_counts_once_and_frees_when_the_last_owner_drops() {
    let account = account(10_000, 10_000, 10_000);
    let shared = Arc::new(
        account
            .reserve(ChargeClass::Retained, 100)
            .admitted()
            .expect("fits"),
    );
    let clones: Vec<Arc<RetentionCharge>> = (0..8).map(|_| Arc::clone(&shared)).collect();
    assert_eq!(
        account.snapshot().retained_bytes,
        100,
        "eight owners of one payload charge its bytes once"
    );
    drop(clones);
    assert_eq!(
        account.snapshot().retained_bytes,
        100,
        "bytes stay charged while any owner — including a reader that \
         outlived an eviction — still holds the payload"
    );
    drop(shared);
    assert_eq!(account.snapshot().retained_bytes, 0);
}

// ──────────────────────────────────────────────────────────────────────
// Wiring: the stores actually charge the account
// ──────────────────────────────────────────────────────────────────────

/// Every project shares ONE process-local account, so loading N projects
/// does not multiply the ratified ceiling by N.
#[test]
fn every_project_store_shares_the_one_process_local_account() {
    let a = crate::project_type_store::ProjectTypeStore::new();
    let b = crate::project_type_store::ProjectTypeStore::new();
    assert!(
        Arc::ptr_eq(a.retention_account(), b.retention_account()),
        "two project stores must charge the same aggregate account"
    );
    assert!(Arc::ptr_eq(
        a.retention_account(),
        &SemanticRetentionAccount::process_local()
    ));
}

/// The identity intern pool carries no private byte quota: its payloads
/// are charged to the store's account and released with the store.
#[test]
fn the_project_store_charges_its_interned_identities() {
    let account = account(10_000_000, 10_000_000, 10_000_000);
    let store =
        crate::project_type_store::ProjectTypeStore::with_retention_account(Arc::clone(&account));
    let before = account.snapshot().retained_bytes;
    let interned = store
        .identity_interner()
        .intern("/src/components/Accounted.vue");
    assert_eq!(
        account.snapshot().retained_bytes - before,
        interned.len(),
        "an interned identity's payload must be charged to the store's account"
    );
    drop(store);
    assert_eq!(
        account.snapshot().retained_bytes,
        before,
        "dropping the store must release every byte it charged"
    );
}

/// A retained parse snapshot is charged once per SNAPSHOT — not once per
/// lease — and its bytes become reclaimable when the last lease drops.
/// Charging per LEASE would report a file three consumers hold as three
/// resident parse arenas and starve every discretionary cache.
///
/// The service pins against a PRIVATE account so the assertions are
/// absolute rather than deltas against whatever other tests hold.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_retained_parse_snapshot_charges_its_pin_once_per_snapshot() {
    use crate::decl_lowering::{DeclLoweringService, SnapshotKey};

    let account = account(usize::MAX, usize::MAX, usize::MAX);
    // ONE worker, so every job — including the fire-and-forget releases —
    // is serviced in submission order on a single channel. That ordering
    // is what makes the post-release observation below deterministic
    // rather than a sleep.
    let service = Arc::new(DeclLoweringService::new_with_account(
        false,
        1,
        Arc::clone(&account),
    ));
    let source: Arc<str> = Arc::from("export type Pinned = { a: string };");
    let key = SnapshotKey {
        canonical: Arc::from("/src/pinned.ts"),
        whole_hash: [7u8; 16],
        parse_env_hash: [8u8; 16],
    };
    let flush_key = SnapshotKey {
        canonical: Arc::from("/src/flush.ts"),
        whole_hash: [9u8; 16],
        parse_env_hash: [8u8; 16],
    };
    let source_type = oxc_span::SourceType::ts();

    assert_eq!(account.snapshot().pinned_bytes, 0);
    let first = service.acquire_lease(&key, &source, source_type);
    assert!(first.parsed_now, "the first acquisition parses");
    let charged = account.snapshot().pinned_bytes;
    assert!(charged > 0, "a retained snapshot must charge pinned bytes");

    let second = service.acquire_lease(&key, &source, source_type);
    assert!(
        !second.parsed_now,
        "a second lease reuses the retained snapshot"
    );
    assert_eq!(
        account.snapshot().pinned_bytes,
        charged,
        "a second lease on an existing snapshot must not charge again"
    );

    drop(second.lease);
    // A rendezvous acquire is serviced AFTER the queued release, so its
    // return proves the release has been applied.
    let flush = service.acquire_lease(&flush_key, &source, source_type);
    assert_eq!(
        account.snapshot().pinned_bytes,
        charged + charged,
        "bytes stay pinned while a lease still holds the snapshot \
         (only the flush key's own snapshot is added)"
    );

    drop(first.lease);
    drop(flush.lease);
    let settle = service.acquire_lease(&flush_key, &source, source_type);
    assert!(
        settle.parsed_now,
        "both snapshots must have been released before this re-acquire"
    );
    assert_eq!(
        account.snapshot().pinned_bytes,
        charged,
        "the last lease release makes a snapshot's bytes reclaimable"
    );
    drop(settle.lease);
}

/// The footprint estimate must scale with the entry it describes. A
/// constant estimator would make the aggregate ceiling meaningless:
/// a wide entry and a trivial one would cost the same, so the account
/// would bound entry COUNT under a byte-shaped name.
#[test]
fn the_result_entry_footprint_scales_with_the_entry() {
    fn entry(facts: usize) -> crate::component_meta_result_db::ComponentMetaResultEntry<u32> {
        let facts: Vec<crate::resolver_core::FactVersionRef> = (0..facts)
            .map(|i| crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: format!("/src/dep{i}.ts"),
                hash: [1u8; 16],
            })
            .collect();
        crate::component_meta_result_db::ComponentMetaResultEntry {
            payload: Arc::new(0u32),
            read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(Arc::from(
                facts,
            )),
            validated_at_generation: 0,
        }
    }

    let narrow = entry(1).retained_footprint_bytes();
    let wide = entry(101).retained_footprint_bytes();
    assert!(
        wide > narrow,
        "the estimate must grow with the entry ({wide} vs {narrow})"
    );
    assert!(
        wide - narrow >= 100,
        "a hundred extra dependency facts must cost at least a hundred bytes"
    );
}

/// A result cache wired to an exhausted account returns its complete
/// value uncached rather than storing it — and leaves whatever was
/// already resident untouched, so no reader is handed a stale candidate
/// in place of the refused one.
#[test]
fn a_result_cache_under_pressure_stores_nothing_and_disturbs_nothing() {
    use crate::component_meta_result_db::{ComponentMetaResultEntry, ComponentMetaResultKey};

    fn entry(value: u32) -> ComponentMetaResultEntry<u32> {
        ComponentMetaResultEntry {
            payload: Arc::new(value),
            read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(Arc::from(
                vec![crate::resolver_core::FactVersionRef::FileWholeHash {
                    canonical_id: "/src/Owner.vue".to_string(),
                    hash: [1u8; 16],
                }],
            )),
            validated_at_generation: 0,
        }
    }
    fn key(owner: &str) -> ComponentMetaResultKey {
        ComponentMetaResultKey {
            owner_canonical: Arc::from(owner),
            options_fingerprint: [0u8; 16],
            project_identity: crate::file_artifact_store::ProjectIdentity([0u8; 16]),
            parse_env_hash: [0u8; 16],
            resolve_env_hash: [0u8; 16],
            type_env_hash: [0u8; 16],
            lib_env_hash: [0u8; 16],
        }
    }

    // Sized so the first entry fits and the second does not.
    let first_bytes = entry(1).retained_footprint_bytes();
    let account = account(first_bytes + 1, first_bytes + 1, first_bytes + 1);
    let db = crate::component_meta_result_db::ComponentMetaResultDb::<u32>::with_account_for_test(
        Arc::clone(&account),
    );

    db.insert(key("/src/A.vue"), [1u8; 16], entry(1));
    assert!(db.get(&key("/src/A.vue"), [1u8; 16]).is_some());
    let charged = account.snapshot().retained_bytes;
    assert!(charged > 0, "the admitted entry must be charged");

    // A second, individually-legal entry has no aggregate headroom.
    db.insert(key("/src/B.vue"), [2u8; 16], entry(2));
    assert!(
        db.get(&key("/src/B.vue"), [2u8; 16]).is_none(),
        "a pressure-refused entry must not be stored"
    );
    assert_eq!(
        account.snapshot().retained_bytes,
        charged,
        "a refused admission must charge nothing"
    );
    let resident = db
        .get(&key("/src/A.vue"), [1u8; 16])
        .expect("the resident entry must survive a refused admission");
    assert_eq!(*resident.payload, 1, "no stale substitution");
    assert_eq!(account.snapshot().refusals_pressure, 1);
}

/// Every constructor that can produce a candidate-retaining store binds an
/// account, so no reachable store retains semantic bytes off the aggregate
/// ceiling.
///
/// The boundary: the memo store and the component-meta result cache both
/// used to carry an OPTIONAL account, and their account-less constructors
/// are public. A store built through one of them published candidates that
/// consumed no aggregate headroom — the process could therefore retain more
/// than the ratified ceiling while every individual admission looked legal.
/// Binding the one process-local account at construction is what makes an
/// off-account retaining store unrepresentable; this pins that the
/// account-less constructors really do resolve to THAT account rather than
/// to a private per-store quota.
#[test]
fn no_reachable_candidate_store_retains_off_the_aggregate_account() {
    let process_local = SemanticRetentionAccount::process_local();

    let memo = crate::semantic_query_memo::SemanticGraphStore::new();
    assert!(
        Arc::ptr_eq(memo.retention_account(), &process_local),
        "a memo store built without an explicit account must charge the one \
         process-local account, not retain candidates off-account"
    );

    let results = crate::component_meta_result_db::ComponentMetaResultDb::<u32>::new();
    assert!(
        Arc::ptr_eq(results.retention_account(), &process_local),
        "a component-meta result cache built without an explicit account must \
         charge the one process-local account"
    );
}

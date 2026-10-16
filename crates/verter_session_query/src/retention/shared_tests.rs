use std::sync::Arc;

use super::*;
use crate::retention::{RetentionLimits, SemanticRetentionAccount};

fn account(ceiling: usize) -> Arc<SemanticRetentionAccount> {
    SemanticRetentionAccount::new(RetentionLimits {
        aggregate_ceiling_bytes: ceiling,
        active_ceiling_bytes: ceiling,
        max_entry_bytes: ceiling,
        pin_threshold_bytes: ceiling,
    })
}

fn occupancy(account: &SemanticRetentionAccount) -> (usize, usize) {
    let snapshot = account.snapshot();
    (snapshot.retained_bytes, snapshot.pinned_bytes)
}

#[test]
fn cache_first_release_hands_the_one_charge_to_the_surviving_caller() {
    let account = account(1 << 20);
    let cache = CacheHold::reserve(&account, 700).expect("admitted");
    let caller = cache.caller();
    assert_eq!(occupancy(&account), (700, 0));

    drop(cache);
    assert_eq!(
        occupancy(&account),
        (0, 700),
        "the caller still holds the allocation: its bytes stay charged, pinned"
    );
    assert_eq!(caller.class(), ChargeClass::Pinned);

    drop(caller);
    assert_eq!(occupancy(&account), (0, 0));
    assert_eq!(account.snapshot().releases, 1, "released exactly once");
}

#[test]
fn caller_first_release_keeps_one_retained_charge_until_the_cache_drains() {
    let account = account(1 << 20);
    let cache = CacheHold::reserve(&account, 700).expect("admitted");
    let caller = cache.caller();
    let second_caller = caller.clone();

    drop(caller);
    drop(second_caller);
    assert_eq!(occupancy(&account), (700, 0));
    assert_eq!(cache.class(), ChargeClass::Retained);

    drop(cache);
    assert_eq!(occupancy(&account), (0, 0));
    assert_eq!(account.snapshot().releases, 1);
}

#[test]
fn a_sibling_cache_owner_shares_the_charge_and_keeps_it_retained() {
    let account = account(1 << 20);
    let cache = CacheHold::reserve(&account, 300).expect("admitted");
    let sibling = cache.clone();
    let caller = cache.caller();
    assert!(cache.shares_charge_with(&sibling));
    assert_eq!(occupancy(&account), (300, 0), "one allocation, one charge");

    drop(cache);
    assert_eq!(
        occupancy(&account),
        (300, 0),
        "a cache owner remains, so the charge stays discretionary retention"
    );
    drop(sibling);
    assert_eq!(occupancy(&account), (0, 300));
    drop(caller);
    assert_eq!(occupancy(&account), (0, 0));
}

#[test]
fn pressure_never_revokes_a_held_charge_and_refuses_only_its_readoption() {
    let account = account(150);
    let cache = CacheHold::reserve(&account, 100).expect("admitted");
    let caller = cache.caller();
    // A live handle elsewhere pins past the ceiling.
    let other_pin = account.pin(100);

    drop(cache);
    assert_eq!(
        occupancy(&account),
        (0, 200),
        "the transfer to the caller is never refused, even past the ceiling"
    );

    let refusal = caller.adopt().expect_err("no headroom to retain it again");
    assert!(matches!(refusal, RetentionRefusal::Pressure { .. }));
    assert_eq!(
        caller.class(),
        ChargeClass::Pinned,
        "the caller's pin is untouched"
    );
    assert_eq!(occupancy(&account), (0, 200));

    drop(other_pin);
    let readopted = caller.adopt().expect("headroom returned");
    assert_eq!(readopted.class(), ChargeClass::Retained);
    assert_eq!(
        occupancy(&account),
        (100, 0),
        "adoption moves the bytes, never adds them"
    );
    drop(readopted);
    drop(caller);
    assert_eq!(occupancy(&account), (0, 0));
}

#[test]
fn independent_owners_of_one_allocation_share_one_registered_charge() {
    let account = account(1 << 20);
    let registry = AliasCharges::new();
    let allocation: Arc<[u8]> = Arc::from(vec![7u8; 4096]);

    let first = registry
        .hold(&account, &allocation, || 4096)
        .expect("admitted");
    let second = registry
        .hold(&account, &allocation, || {
            panic!("an allocation already charged is not measured again")
        })
        .expect("shared");
    assert!(first.shares_charge_with(&second));
    assert_eq!(
        occupancy(&account),
        (4096 + registration_bytes(), 0),
        "two owners, one charge"
    );

    let other: Arc<[u8]> = Arc::from(vec![1u8; 16]);
    let third = registry.hold(&account, &other, || 16).expect("admitted");
    assert!(!third.shares_charge_with(&first));
    assert_eq!(registry.len(), 2);

    drop((first, second, third));
    assert!(
        registry.is_empty(),
        "a registration lives exactly as long as its charge"
    );
    assert_eq!(occupancy(&account), (0, 0));
}

#[test]
fn a_registered_charge_survives_its_cache_through_the_callers_and_is_readopted() {
    let account = account(1 << 20);
    let registry = AliasCharges::new();
    let allocation: Arc<str> = Arc::from("x".repeat(2000));
    let cache = registry
        .hold(&account, &allocation, || 2000)
        .expect("admitted");
    let caller = cache.caller();
    let bytes = 2000 + registration_bytes();

    drop(cache);
    assert_eq!(occupancy(&account), (0, bytes));
    assert_eq!(registry.len(), 1, "the caller keeps the registration alive");

    let again = registry
        .hold(&account, &allocation, || panic!("still charged"))
        .expect("adopted");
    assert_eq!(occupancy(&account), (bytes, 0));
    drop(caller);
    drop(again);
    assert_eq!(occupancy(&account), (0, 0));
    assert!(registry.is_empty());
}

#[test]
fn a_refused_first_hold_registers_nothing() {
    let account = account(64);
    let registry = AliasCharges::new();
    let allocation: Arc<[u8]> = Arc::from(vec![0u8; 1024]);
    registry
        .hold(&account, &allocation, || 1024)
        .expect_err("over the ceiling");
    assert!(registry.is_empty());
    assert_eq!(occupancy(&account), (0, 0));
}

#[test]
fn concurrent_holders_never_charge_one_allocation_twice_or_leave_it_charged() {
    let account = account(1 << 30);
    let registry = AliasCharges::new();
    let allocation: Arc<[u64]> = Arc::from(vec![0u64; 512]);
    let bytes = 4096 + registration_bytes();
    std::thread::scope(|scope| {
        for worker in 0..8u32 {
            let (account, registry, allocation) = (&account, &registry, &allocation);
            scope.spawn(move || {
                // bounded-loop: fixed iteration count per worker
                for round in 0..400u32 {
                    let cache = registry
                        .hold(account, allocation, || 4096)
                        .expect("no pressure");
                    let caller = cache.caller();
                    if (round + worker) % 2 == 0 {
                        drop(cache);
                        drop(caller);
                    } else {
                        drop(caller);
                        drop(cache);
                    }
                    let (retained, pinned) = occupancy(account);
                    assert!(
                        retained + pinned == 0 || retained + pinned == bytes,
                        "one allocation is charged once or not at all, never twice: \
                         {retained} retained + {pinned} pinned"
                    );
                }
            });
        }
    });
    assert_eq!(occupancy(&account), (0, 0));
    assert!(registry.is_empty());
}

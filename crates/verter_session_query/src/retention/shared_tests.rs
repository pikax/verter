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

/// A caller-first charge minted without a registry is adopted into
/// retention by a cache, and handed back to its caller when the cache
/// drains.
#[test]
fn an_unregistered_caller_pin_is_adopted_into_retention_and_handed_back() {
    let account = account(1 << 20);
    let caller = CallerHold::pin(&account, 400);
    assert_eq!(occupancy(&account), (0, 400));
    let cache = caller.adopt().expect("headroom");
    assert_eq!(occupancy(&account), (400, 0), "adoption moves the bytes");
    drop(cache);
    assert_eq!(occupancy(&account), (0, 400));
    drop(caller);
    assert_eq!(occupancy(&account), (0, 0));
}

#[test]
fn independent_owners_of_one_allocation_share_one_registered_charge() {
    let account = account(1 << 20);
    let registry = AliasCharges::new();
    let allocation: Arc<[u8]> = Arc::from(vec![7u8; 4096]);

    let first = registry
        .hold(&account, &allocation, |_| 4096)
        .expect("admitted");
    let second = registry
        .hold(&account, &allocation, |_| {
            panic!("an allocation already charged is not measured again")
        })
        .expect("shared");
    assert!(first.shares_charge_with(&second));
    assert_eq!(
        occupancy(&account),
        (4096 + registration_bytes(), registry.table_charged_bytes()),
        "two owners, one charge (beside the registry's own table)"
    );

    let other: Arc<[u8]> = Arc::from(vec![1u8; 16]);
    let third = registry.hold(&account, &other, |_| 16).expect("admitted");
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
        .hold(&account, &allocation, |_| 2000)
        .expect("admitted");
    let caller = cache.caller();
    let bytes = 2000 + registration_bytes();

    drop(cache);
    let table = registry.table_charged_bytes();
    assert_eq!(occupancy(&account), (0, bytes + table));
    assert_eq!(registry.len(), 1, "the caller keeps the registration alive");

    let again = registry
        .hold(&account, &allocation, |_| panic!("still charged"))
        .expect("adopted");
    assert_eq!(occupancy(&account), (bytes, table));
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
        .hold(&account, &allocation, |_| 1024)
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
                        .hold(account, allocation, |_| 4096)
                        .expect("no pressure");
                    let caller = cache.caller();
                    if (round + worker) % 2 == 0 {
                        drop(cache);
                        drop(caller);
                    } else {
                        drop(caller);
                        drop(cache);
                    }
                    // The registry's one-entry table is charged one granule
                    // while it holds the registration.
                    let (retained, pinned) = occupancy(account);
                    let table = REGISTRY_TABLE_GRANULE;
                    assert!(
                        [0, table, bytes, bytes + table].contains(&(retained + pinned)),
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

#[test]
fn a_drained_registry_gives_back_its_table_and_its_charge() {
    const ALLOCATIONS: usize = 2_000;
    let account = account(1 << 30);
    let registry = AliasCharges::new();
    let allocations: Vec<Arc<[u8]>> = (0..ALLOCATIONS)
        .map(|index| Arc::from(vec![index as u8; 8]))
        .collect();
    let holds: Vec<CacheHold> = allocations
        .iter()
        .map(|allocation| {
            registry
                .hold(&account, allocation, |_| 8)
                .expect("admitted")
        })
        .collect();
    let capacity = registry.table_capacity();
    assert!(capacity >= ALLOCATIONS);
    assert!(
        registry.table_charged_bytes() >= capacity * REGISTRY_BUCKET_BYTES,
        "the table is charged by its capacity: {} < {capacity} buckets",
        registry.table_charged_bytes()
    );
    assert_eq!(
        account.snapshot().pinned_bytes,
        registry.table_charged_bytes()
    );

    drop(holds);
    assert!(registry.is_empty());
    assert_eq!(
        registry.table_capacity(),
        0,
        "a drained table keeps no backing while the registry lives on"
    );
    assert_eq!(registry.table_charged_bytes(), 0);
    assert_eq!(occupancy(&account), (0, 0));
    drop(allocations);
}

#[test]
fn a_caller_first_charge_is_pinned_and_adopted_once_by_a_retaining_cache() {
    let account = account(1 << 20);
    let registry = AliasCharges::new();
    let allocation: Arc<str> = Arc::from("y".repeat(3000));
    let bytes = 3000 + registration_bytes();

    let caller = registry.pin_caller(&account, &allocation, |_| 3000);
    let table = registry.table_charged_bytes();
    assert_eq!(caller.class(), ChargeClass::Pinned);
    assert_eq!(occupancy(&account), (0, bytes + table));

    let second_caller = registry.pin_caller(&account, &allocation, |_| {
        panic!("a charged allocation is not measured again")
    });
    let cache = registry
        .hold(&account, &allocation, |_| panic!("adopted, not measured"))
        .expect("headroom");
    assert_eq!(cache.class(), ChargeClass::Retained);
    assert_eq!(
        occupancy(&account),
        (bytes, table),
        "adoption moves the caller's charge into retention, never adds one"
    );

    drop(caller);
    drop(cache);
    assert_eq!(occupancy(&account), (0, bytes + table));
    drop(second_caller);
    assert_eq!(occupancy(&account), (0, 0));
    assert!(registry.is_empty());
}

#[test]
fn a_caller_first_charge_is_never_refused() {
    let account = account(0);
    let registry = AliasCharges::new();
    let allocation: Arc<[u8]> = Arc::from(vec![0u8; 512]);
    let caller = registry.pin_caller(&account, &allocation, |_| 512);
    assert!(account.snapshot().pinned_bytes >= 512 + registration_bytes());
    registry
        .hold(&account, &allocation, |_| 512)
        .expect_err("no headroom to retain it");
    assert_eq!(caller.class(), ChargeClass::Pinned, "the pin is untouched");
    drop(caller);
    assert_eq!(occupancy(&account), (0, 0));
}

/// Two different carriers reaching one oversized leaf name share that
/// leaf's one charge, through either release order, and the leaf is
/// released with the last carrier holding it.
#[test]
fn a_leaf_two_carriers_share_is_charged_once_through_either_release_order() {
    use crate::retention::allocation::arc_str_list_bytes;
    const LEAF: usize = 50_000;
    for first_released in [0, 1] {
        let account = account(1 << 30);
        let registry = AliasCharges::new();
        let leaf: Arc<str> = Arc::from("z".repeat(LEAF));
        let small: Arc<str> = Arc::from("small");
        let carriers: [Arc<[Arc<str>]>; 2] = [
            Arc::from(vec![Arc::clone(&leaf)]),
            Arc::from(vec![Arc::clone(&leaf), Arc::clone(&small)]),
        ];
        let block = |carrier: &Arc<[Arc<str>]>| {
            arc_block_bytes(std::mem::size_of_val::<[Arc<str>]>(carrier)) + registration_bytes()
        };
        let leaf_charge = arc_block_bytes(LEAF) + registration_bytes();
        let small_charge = arc_block_bytes(small.len()) + registration_bytes();
        let mut holds: Vec<Option<CacheHold>> = carriers
            .iter()
            .map(|carrier| {
                Some(
                    registry
                        .hold(&account, carrier, |aliases| {
                            arc_str_list_bytes(carrier, aliases)
                        })
                        .expect("admitted"),
                )
            })
            .collect();
        assert_eq!(
            occupancy(&account).0,
            block(&carriers[0]) + block(&carriers[1]) + leaf_charge + small_charge,
            "each carrier pays its own block; the shared leaf is charged once"
        );
        assert_eq!(registry.len(), 4);

        holds[first_released] = None;
        let survivor = 1 - first_released;
        let expected =
            block(&carriers[survivor]) + leaf_charge + if survivor == 1 { small_charge } else { 0 };
        assert_eq!(
            occupancy(&account).0,
            expected,
            "the surviving carrier keeps the shared leaf charged, once"
        );
        holds[survivor] = None;
        assert_eq!(occupancy(&account), (0, registry.table_charged_bytes()));
        assert!(registry.is_empty());
        assert_eq!(occupancy(&account), (0, 0));
    }
}

/// A carrier's leaves follow its class: handed to its callers with it when
/// its cache drains, and adopted with it — all or nothing — when a cache
/// retains it again.
#[test]
fn a_carriers_leaves_move_with_its_class_and_adopt_all_or_nothing() {
    use crate::retention::allocation::arc_str_list_bytes;
    const LEAF: usize = 10_000;
    let account = account(1 << 20);
    let registry = AliasCharges::new();
    let carrier: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from("l".repeat(LEAF))]);
    let total = arc_block_bytes(std::mem::size_of_val::<[Arc<str>]>(&carrier))
        + arc_block_bytes(LEAF)
        + 2 * registration_bytes();
    let cache = registry
        .hold(&account, &carrier, |aliases| {
            arc_str_list_bytes(&carrier, aliases)
        })
        .expect("admitted");
    let caller = cache.caller();
    let table = || registry.table_charged_bytes();
    assert_eq!(occupancy(&account), (total, table()));

    drop(cache);
    assert_eq!(
        occupancy(&account),
        (0, total + table()),
        "the carrier and its leaf are the caller's now, pinned"
    );

    // A live handle elsewhere leaves room for the carrier but not its leaf.
    let other = account.pin((1 << 20) - total - table() + LEAF / 2);
    registry
        .hold(&account, &carrier, |_| panic!("registered"))
        .expect_err("no room to retain the carrier with its leaf");
    assert_eq!(
        occupancy(&account),
        (0, total + table() + other.bytes()),
        "a refused adoption leaves the carrier and every leaf pinned to the caller"
    );
    drop(other);

    let readopted = registry
        .hold(&account, &carrier, |_| panic!("registered"))
        .expect("headroom returned");
    assert_eq!(occupancy(&account), (total, table()));
    drop(caller);
    drop(readopted);
    assert_eq!(occupancy(&account), (0, 0));
}

/// A holder can find a carrier in the registry after its last holder gave
/// it up but before that holder's release took the registry lock. Reviving
/// it there — as a cache or as a caller — keeps the carrier and the leaf
/// only it holds charged exactly once, in the reviver's class, and neither
/// thread blocks on the other.
#[test]
fn a_carrier_revived_while_its_last_holder_releases_keeps_its_sole_leaf_charged_once() {
    use crate::retention::allocation::arc_str_list_bytes;
    const LEAF: usize = 20_000;
    for revive_as_caller in [false, true] {
        let account = account(1 << 20);
        let registry = AliasCharges::new();
        let carrier: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from("r".repeat(LEAF))]);
        let total = arc_block_bytes(std::mem::size_of_val::<[Arc<str>]>(&carrier))
            + arc_block_bytes(LEAF)
            + 2 * registration_bytes();
        let last = registry
            .hold(&account, &carrier, |aliases| {
                arc_str_list_bytes(&carrier, aliases)
            })
            .expect("admitted");
        let table_bytes = registry.table_charged_bytes();
        assert_eq!(occupancy(&account), (total, table_bytes));

        // Park the releaser: it gives its hold up, then waits on the
        // registry lock this thread holds.
        let mut table = registry.0.table.lock();
        let key = Arc::as_ptr(&carrier).cast::<()>() as usize;
        let entry = Arc::clone(table.entries.get(&key).expect("registered"));
        let releaser = std::thread::spawn(move || drop(last));
        while {
            let state = entry.state.lock();
            state.cache_holds + state.caller_holds != 0
        } {
            std::thread::yield_now();
        }

        // Revive it the way a concurrent holder would, under the same lock.
        let mut deferred = Vec::new();
        let revived: (Option<CacheHold>, Option<CallerHold>) = if revive_as_caller {
            (
                None,
                Some(registry.pin_in(&mut table, &account, &carrier, |_| {
                    panic!("the carrier is registered")
                })),
            )
        } else {
            (
                Some(
                    registry
                        .hold_in(
                            &mut table,
                            &account,
                            &carrier,
                            |_| panic!("the carrier is registered"),
                            &mut deferred,
                        )
                        .expect("a retained charge is shared, never refused"),
                ),
                None,
            )
        };
        drop(entry);
        drop(table);
        releaser.join().expect("the releaser finishes");
        assert!(deferred.is_empty());

        assert_eq!(registry.len(), 2, "the carrier and its leaf, once each");
        let expected = if revive_as_caller {
            (0, total + table_bytes)
        } else {
            (total, table_bytes)
        };
        assert_eq!(
            occupancy(&account),
            expected,
            "the revived carrier and its leaf are charged once, in the reviver's class"
        );
        drop(revived);
        assert!(registry.is_empty());
        assert_eq!(occupancy(&account), (0, 0));
    }
}

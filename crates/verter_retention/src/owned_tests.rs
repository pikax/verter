use std::sync::Arc;

use super::*;
use crate::allocation::{arc_str_bytes, vec_bytes};
use crate::shared::registration_bytes;
use crate::RetentionLimits;

fn account(ceiling: usize) -> Arc<SemanticRetentionAccount> {
    SemanticRetentionAccount::new(RetentionLimits {
        aggregate_ceiling_bytes: ceiling,
        active_ceiling_bytes: ceiling,
        max_entry_bytes: ceiling,
        pin_threshold_bytes: ceiling,
    })
}

/// `(retained, pinned)` bytes on `account`.
fn occupancy(account: &SemanticRetentionAccount) -> (usize, usize) {
    let snapshot = account.snapshot();
    (snapshot.retained_bytes, snapshot.pinned_bytes)
}

/// A carrier embedding an owned name, a raw leaf name and an owned list.
struct Carrier {
    name: OwnedStr,
    raw: Option<Arc<str>>,
    list: Vec<u64>,
}

impl Measured for Carrier {
    fn heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.name.offer(aliases)
            + self
                .raw
                .as_ref()
                .map_or(0, |raw| arc_str_bytes(raw, aliases))
            + vec_bytes(&self.list)
    }
}

fn carrier(name: &OwnedStr, list: usize) -> Owned<Carrier> {
    Owned::new(Carrier {
        name: name.clone(),
        raw: None,
        list: vec![0; list],
    })
}

fn own_bytes(list: usize) -> usize {
    Owned::<Carrier>::cell_block_bytes() + heap_block_bytes(list * 8)
}

/// A clone that outlives the cache that retained it and the read that
/// delivered it keeps the allocation's one charge — pinned — until the
/// clone itself drops, which restores the baseline at once: no sweep, no
/// reap. Holds both release orders.
#[test]
fn a_clone_outliving_every_holder_keeps_the_one_charge_until_its_own_drop() {
    const LIST: usize = 4096;
    for cache_first in [true, false] {
        let account = account(1 << 30);
        let registry = AliasCharges::new();
        let ctx = ChargeCtx::new(&account, &registry);
        let name = OwnedStr::new("n");
        let read = carrier(&name, LIST);
        read.pin(ctx);
        let cache = read.hold(ctx).expect("admitted");
        let total = own_bytes(LIST) + OwnedStr::charge_for_len(1);
        assert_eq!(occupancy(&account), (total, 0), "one charge, retained");

        let clone = read.clone();
        drop(read);
        drop(name);
        if cache_first {
            drop(cache);
            assert_eq!(
                occupancy(&account),
                (0, total),
                "the clone is the last holder: the charge is pinned, not released"
            );
        } else {
            assert_eq!(occupancy(&account), (total, 0));
            drop(cache);
            assert_eq!(occupancy(&account), (0, total));
        }
        assert_eq!(clone.charge_class(), Some(ChargeClass::Pinned));
        drop(clone);
        assert_eq!(occupancy(&account), (0, 0), "released with the last handle");
        assert_eq!(
            account.snapshot().releases,
            2,
            "the carrier and its name, once each"
        );
    }
}

/// A nested allocation two parents share is charged once, by its own
/// cell, and a clone of it alone survives both parents' teardown charged.
#[test]
fn a_shared_nested_string_is_charged_once_and_survives_its_parents() {
    const NAME: usize = 30_000;
    let account = account(1 << 30);
    let registry = AliasCharges::new();
    let ctx = ChargeCtx::new(&account, &registry);
    let name = OwnedStr::from("x".repeat(NAME));
    let first = carrier(&name, 1);
    let second = carrier(&name, 1);
    let first_hold = first.hold(ctx).expect("admitted");
    second.pin(ctx);
    let name_charge = OwnedStr::charge_for_len(NAME);
    assert_eq!(name.charge_bytes(), Some(name_charge));
    assert_eq!(
        occupancy(&account),
        (own_bytes(1) + name_charge, own_bytes(1)),
        "each carrier pays its own cell; the shared name is retained once with the cached carrier"
    );

    let survivor = name.clone();
    drop(name);
    drop(first_hold);
    drop(first);
    assert_eq!(
        occupancy(&account),
        (0, own_bytes(1) + name_charge),
        "the name is the second carrier's and the clone's now"
    );
    drop(second);
    assert_eq!(
        occupancy(&account),
        (0, name_charge),
        "the clone alone keeps it"
    );
    drop(survivor);
    assert_eq!(occupancy(&account), (0, 0));
}

/// A plain measurement of a value embedding an owned handle counts the
/// handle's allocation in full, once however often it is reached.
#[test]
fn an_uncollected_measurement_counts_a_nested_handle_once_in_full() {
    let name = OwnedStr::from("y".repeat(1000));
    let mut aliases = AliasSet::new();
    assert_eq!(name.offer(&mut aliases), OwnedStr::charge_for_len(1000));
    assert_eq!(name.clone().offer(&mut aliases), 0);
}

/// Pressure never refuses a caller's pin and never revokes it; a cache's
/// first hold or adoption is refused whole — the parent with its children
/// — and leaves every pin as it was.
#[test]
fn zero_headroom_pins_callers_and_refuses_retention_all_or_nothing() {
    let account = account(0);
    let registry = AliasCharges::new();
    let ctx = ChargeCtx::new(&account, &registry);
    let name = OwnedStr::from("z".repeat(500));
    let value = carrier(&name, 64);

    let fresh = carrier(&OwnedStr::new("q"), 8);
    fresh
        .hold(ctx)
        .expect_err("no headroom to retain a fresh allocation");
    assert_eq!(
        fresh.charge_bytes(),
        None,
        "a refused first hold charges nothing"
    );
    assert_eq!(occupancy(&account), (0, 0));

    value.pin(ctx);
    let pinned = occupancy(&account);
    assert_eq!(pinned, (0, own_bytes(64) + OwnedStr::charge_for_len(500)));
    value.hold(ctx).expect_err("no headroom to adopt the pin");
    assert_eq!(occupancy(&account), pinned, "the pins are untouched");
    assert_eq!(value.charge_class(), Some(ChargeClass::Pinned));
    assert_eq!(name.charge_class(), Some(ChargeClass::Pinned));
    drop(value);
    drop(name);
    assert_eq!(occupancy(&account), (0, 0));
}

/// Adoption is all or nothing across a parent and its child: with room for
/// the child but not the parent, neither moves.
#[test]
fn a_refused_adoption_hands_an_adopted_child_back_to_its_callers() {
    let name_len = 200;
    let list = 512;
    let name_charge = OwnedStr::charge_for_len(name_len);
    let account = account(name_charge + own_bytes(list) / 2);
    let registry = AliasCharges::new();
    let ctx = ChargeCtx::new(&account, &registry);
    let name = OwnedStr::from("w".repeat(name_len));
    let value = carrier(&name, list);
    value.pin(ctx);
    value
        .hold(ctx)
        .expect_err("room for the name, not the carrier");
    assert_eq!(name.charge_class(), Some(ChargeClass::Pinned));
    assert_eq!(occupancy(&account), (0, name_charge + own_bytes(list)));
}

/// A raw leaf name two owned values reach is charged once through the
/// registry, in the class of whichever holds it, and released with the
/// last of them.
#[test]
fn a_raw_leaf_two_owned_values_share_is_charged_once_through_the_registry() {
    const LEAF: usize = 20_000;
    let account = account(1 << 30);
    let registry = AliasCharges::new();
    let ctx = ChargeCtx::new(&account, &registry);
    let leaf: Arc<str> = Arc::from("l".repeat(LEAF));
    let name = OwnedStr::new("n");
    let with_leaf = |list| {
        Owned::new(Carrier {
            name: name.clone(),
            raw: Some(Arc::clone(&leaf)),
            list: vec![0; list],
        })
    };
    let first = with_leaf(1);
    let second = with_leaf(2);
    let hold = first.hold(ctx).expect("admitted");
    second.pin(ctx);
    let leaf_charge = arc_block_bytes(LEAF) + registration_bytes();
    let table = || registry.table_charged_bytes();
    assert_eq!(
        occupancy(&account),
        (
            own_bytes(1) + OwnedStr::charge_for_len(1) + leaf_charge,
            own_bytes(2) + table()
        ),
        "the leaf is retained once, with the cached value"
    );
    drop(hold);
    drop(first);
    assert_eq!(
        occupancy(&account),
        (
            0,
            own_bytes(2) + OwnedStr::charge_for_len(1) + leaf_charge + table()
        ),
        "the surviving value keeps the leaf, pinned"
    );
    drop(second);
    drop(name);
    assert!(registry.is_empty());
    assert_eq!(occupancy(&account), (0, 0));
}

/// Two caches retaining one allocation share its retained charge; it
/// becomes the callers' only when the last of them lets go.
#[test]
fn sibling_cache_holds_share_one_retained_charge() {
    let account = account(1 << 30);
    let registry = AliasCharges::new();
    let ctx = ChargeCtx::new(&account, &registry);
    let value = carrier(&OwnedStr::new("s"), 16);
    let total = own_bytes(16) + OwnedStr::charge_for_len(1);
    let first = value.hold(ctx).expect("admitted");
    let second = value.hold(ctx).expect("shared");
    assert_eq!(occupancy(&account), (total, 0));
    drop(first);
    assert_eq!(occupancy(&account), (total, 0));
    drop(second);
    assert_eq!(occupancy(&account), (0, total));
    drop(value);
    assert_eq!(occupancy(&account), (0, 0));
}

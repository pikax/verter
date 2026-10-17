use std::sync::Arc;

use super::*;
use crate::allocation::{arc_str_bytes, vec_bytes};

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

/// Bytes a pointer to a type-erased owned cell takes: one slot of a cell's
/// child registration, or one cache hold.
const CELL_POINTER: usize = 2 * std::mem::size_of::<usize>();

/// Bytes a cell reaching `children` owned allocations keeps for its child
/// registration for its whole life: the registration box and its slice.
fn registration(children: usize) -> usize {
    heap_block_bytes(std::mem::size_of::<Nested>()) + heap_block_bytes(children * CELL_POINTER)
}

/// Bytes the slice keeping `holds` cache holds takes while they live.
fn hold_slice(holds: usize) -> usize {
    heap_block_bytes(holds * CELL_POINTER)
}

/// A carrier's own charge: its cell, its list and the registration of its
/// one owned name.
fn own_bytes(list: usize) -> usize {
    Owned::<Carrier>::cell_block_bytes() + heap_block_bytes(list * 8) + registration(1)
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
        let ctx = ChargeCtx::new(&account);
        let name = OwnedStr::new("n");
        let read = carrier(&name, LIST);
        read.pin(ctx);
        let cache = read.hold(ctx).expect("admitted");
        let total = own_bytes(LIST) + OwnedStr::charge_for_len(1);
        let cached = total + hold_slice(1);
        assert_eq!(
            occupancy(&account),
            (cached, 0),
            "one charge, retained, with the cache's hold on the name"
        );

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
            assert_eq!(occupancy(&account), (cached, 0));
            drop(cache);
            assert_eq!(occupancy(&account), (0, total));
        }
        assert_eq!(clone.charge_class(), Some(ChargeClass::Pinned));
        drop(clone);
        assert_eq!(occupancy(&account), (0, 0), "released with the last handle");
        assert_eq!(
            account.snapshot().releases,
            3,
            "the carrier, its name and the cache's holds slice, once each"
        );
    }
}

/// A nested allocation two parents share is charged once, by its own
/// cell, and a clone of it alone survives both parents' teardown charged.
#[test]
fn a_shared_nested_string_is_charged_once_and_survives_its_parents() {
    const NAME: usize = 30_000;
    let account = account(1 << 30);
    let ctx = ChargeCtx::new(&account);
    let name = OwnedStr::from("x".repeat(NAME));
    let first = carrier(&name, 1);
    let second = carrier(&name, 1);
    let first_hold = first.hold(ctx).expect("admitted");
    second.pin(ctx);
    let name_charge = OwnedStr::charge_for_len(NAME);
    assert_eq!(name.charge_bytes(), Some(name_charge));
    assert_eq!(
        occupancy(&account),
        (own_bytes(1) + name_charge + hold_slice(1), own_bytes(1)),
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

/// An estimating measurement of a value embedding an owned handle counts
/// the handle's allocation in full, once however often it is reached.
#[test]
fn an_estimating_measurement_counts_a_nested_handle_once_in_full() {
    let name = OwnedStr::from("y".repeat(1000));
    let mut aliases = AliasSet::estimating();
    assert_eq!(name.offer(&mut aliases), OwnedStr::charge_for_len(1000));
    assert_eq!(name.clone().offer(&mut aliases), 0);
}

/// Pressure never refuses a caller's pin and never revokes it; a cache's
/// first hold or adoption is refused whole — the parent with its children
/// — and leaves every pin as it was.
#[test]
fn zero_headroom_pins_callers_and_refuses_retention_all_or_nothing() {
    let account = account(0);
    let ctx = ChargeCtx::new(&account);
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
    let ctx = ChargeCtx::new(&account);
    let name = OwnedStr::from("w".repeat(name_len));
    let value = carrier(&name, list);
    value.pin(ctx);
    value
        .hold(ctx)
        .expect_err("room for the name, not the carrier");
    assert_eq!(name.charge_class(), Some(ChargeClass::Pinned));
    assert_eq!(occupancy(&account), (0, name_charge + own_bytes(list)));
}

/// An owned value that reaches a raw `Arc` would let a caller clone that
/// allocation out and keep it past the value's charge: a debug build fails
/// the measurement that reaches one.
#[test]
#[should_panic(expected = "an owned value reaches a raw `Arc<str>`")]
#[cfg(debug_assertions)]
fn an_owned_value_reaching_a_raw_arc_fails_its_measurement() {
    let account = account(1 << 30);
    let value = Owned::new(Carrier {
        name: OwnedStr::new("n"),
        raw: Some(Arc::from("leaf")),
        list: Vec::new(),
    });
    value.pin(ChargeCtx::new(&account));
}

/// A measurement that reaches an owned handle hands it to the owner taking
/// the charge; one that drops it uncharged fails a debug build.
#[test]
#[should_panic(expected = "a measurement reached owned allocations its owner never charged")]
#[cfg(debug_assertions)]
fn a_measurement_dropping_a_reached_handle_fails() {
    let name = OwnedStr::new("dropped");
    let mut aliases = AliasSet::new();
    assert_eq!(name.offer(&mut aliases), 0, "collected, not counted");
    drop(aliases);
}

/// The handles a delivered carrier reaches are pinned for its caller and
/// held by a cache exactly as each handle alone would be.
#[test]
fn a_carriers_top_level_handles_are_charged_by_their_own_cells() {
    const NAME: usize = 4_000;
    let account = account(1 << 30);
    let ctx = ChargeCtx::new(&account);
    let name = OwnedStr::from("h".repeat(NAME));
    let list: OwnedSlice<u32> = vec![7; 100].into();
    let carrier = (name.clone(), list.clone());
    let handles = || OwnedHandles::of(|aliases| carrier.heap_bytes(aliases));
    assert_eq!(handles().len(), 2);
    handles().pin(ctx);
    let name_charge = OwnedStr::charge_for_len(NAME);
    let list_charge = OwnedSlice::<u32>::cell_block_bytes() + heap_block_bytes(400);
    assert_eq!(occupancy(&account), (0, name_charge + list_charge));
    let holds = handles().hold(ctx).expect("admitted");
    assert_eq!(holds.backing_bytes(), hold_slice(2));
    assert_eq!(
        occupancy(&account),
        (name_charge + list_charge + hold_slice(2), 0)
    );
    drop(holds);
    drop(carrier);
    drop(list);
    assert_eq!(
        occupancy(&account),
        (0, name_charge),
        "the name alone survives"
    );
    drop(name);
    assert_eq!(occupancy(&account), (0, 0));
}

/// Two caches retaining one allocation share its retained charge; it
/// becomes the callers' only when the last of them lets go.
#[test]
fn sibling_cache_holds_share_one_retained_charge() {
    let account = account(1 << 30);
    let ctx = ChargeCtx::new(&account);
    let value = carrier(&OwnedStr::new("s"), 16);
    let total = own_bytes(16) + OwnedStr::charge_for_len(1);
    let first = value.hold(ctx).expect("admitted");
    let second = value.hold(ctx).expect("shared");
    assert_eq!(occupancy(&account), (total + hold_slice(1), 0));
    drop(first);
    assert_eq!(occupancy(&account), (total + hold_slice(1), 0));
    drop(second);
    assert_eq!(occupancy(&account), (0, total));
    drop(value);
    assert_eq!(occupancy(&account), (0, 0));
}

/// A parent reaching many distinct owned children keeps bookkeeping as
/// wide as they are: the child registration for as long as the parent
/// lives, and the slice of a cache's holds on them for as long as the cache
/// retains it. Both are charged — through the pin, the cache's adoption,
/// the cache-first release and the final drop.
#[test]
fn a_wide_parent_is_charged_its_child_registration_and_its_cache_holds() {
    const CHILDREN: usize = 64;
    const NAME: usize = 40;
    let account = account(1 << 30);
    let ctx = ChargeCtx::new(&account);
    let names: Box<[OwnedStr]> = (0..CHILDREN)
        .map(|i| OwnedStr::from(format!("{i:0>NAME$}")))
        .collect();
    let parent = Owned::new(names);
    let children = CHILDREN * OwnedStr::charge_for_len(NAME);
    let own = Owned::<Box<[OwnedStr]>>::cell_block_bytes()
        + heap_block_bytes(CHILDREN * std::mem::size_of::<OwnedStr>())
        + heap_block_bytes(std::mem::size_of::<Nested>())
        + heap_block_bytes(CHILDREN * CELL_POINTER);
    let holds = heap_block_bytes(CHILDREN * CELL_POINTER);

    parent.pin(ctx);
    assert_eq!(parent.charge_bytes(), Some(own));
    assert_eq!(occupancy(&account), (0, own + children), "pinned");

    let cache = parent.hold(ctx).expect("adopted");
    assert_eq!(
        occupancy(&account),
        (own + children + holds, 0),
        "retained, with the slice of the cache's holds on every child"
    );

    let survivor = parent.clone();
    drop(parent);
    drop(cache);
    assert_eq!(
        occupancy(&account),
        (0, own + children),
        "the cache let go first: its holds' slice is freed, the registration stays"
    );
    drop(survivor);
    assert_eq!(occupancy(&account), (0, 0), "released with the last handle");
}

/// A cache charge taking over the holds on a carrier's handles takes their
/// slice's charge with them, and gives it back when the holds end before
/// the charge does.
#[test]
fn a_charge_holding_handles_is_charged_their_slice_until_the_holds_end() {
    const CHILDREN: usize = 48;
    let account = account(1 << 30);
    let ctx = ChargeCtx::new(&account);
    let names: Vec<OwnedStr> = (0..CHILDREN)
        .map(|i| OwnedStr::from(format!("{i}")))
        .collect();
    let children: usize = names
        .iter()
        .map(|name| OwnedStr::charge_for_len(name.len()))
        .sum();
    let handles = OwnedHandles::of(|aliases| names.iter().map(|name| name.offer(aliases)).sum());
    let own = 100;
    let charge = account
        .reserve(ChargeClass::Retained, own)
        .admitted()
        .expect("admitted");
    let mut charge = handles.retained_with(charge, ctx).expect("admitted");
    let slice = heap_block_bytes(CHILDREN * CELL_POINTER);
    assert_eq!(charge.bytes(), own + slice);
    assert_eq!(occupancy(&account), (own + slice + children, 0));

    charge.transfer_to_pinned();
    assert_eq!(
        charge.bytes(),
        own,
        "the holds ended, and their slice with them"
    );
    assert_eq!(occupancy(&account), (0, own + children));
    drop(charge);
    drop(names);
    assert_eq!(occupancy(&account), (0, 0));
}

/// At zero headroom the slice keeping a cache's holds is refused with them,
/// all or nothing: no child moves into retention.
#[test]
fn zero_headroom_refuses_the_holds_slice_with_the_holds() {
    let account = account(0);
    let ctx = ChargeCtx::new(&account);
    let names: Vec<OwnedStr> = (0..8).map(|i| OwnedStr::from(format!("{i}"))).collect();
    let handles = || OwnedHandles::of(|aliases| names.iter().map(|name| name.offer(aliases)).sum());
    handles().pin(ctx);
    let pinned = occupancy(&account);
    handles()
        .hold(ctx)
        .expect_err("no headroom for the holds or their slice");
    assert_eq!(
        occupancy(&account),
        pinned,
        "every child stays its callers'"
    );
    assert!(names
        .iter()
        .all(|name| name.charge_class() == Some(ChargeClass::Pinned)));
    drop(names);
    assert_eq!(occupancy(&account), (0, 0));
}

use std::sync::Arc;

use super::*;
use crate::allocation::{vec_bytes, AliasSet};
use crate::owned::{ChargeCtx, OwnedStr};
use crate::{RetentionLimits, SemanticRetentionAccount};

fn account() -> Arc<SemanticRetentionAccount> {
    SemanticRetentionAccount::new(RetentionLimits {
        aggregate_ceiling_bytes: 1 << 30,
        active_ceiling_bytes: 1 << 30,
        max_entry_bytes: 1 << 30,
        pin_threshold_bytes: 1 << 30,
    })
}

fn totals(account: &SemanticRetentionAccount) -> (usize, usize, usize) {
    let snapshot = account.snapshot();
    (
        snapshot.active_bytes,
        snapshot.retained_bytes,
        snapshot.pinned_bytes,
    )
}

/// A payload embedding one owned name beside a list it owns exclusively.
struct Payload {
    name: OwnedStr,
    list: Vec<u64>,
}

impl Measured for Payload {
    fn heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.name.offer(aliases) + vec_bytes(&self.list)
    }
}

fn payload(name: &OwnedStr, list: usize) -> Owned<Payload> {
    Owned::new(Payload {
        name: name.clone(),
        list: vec![0; list],
    })
}

fn observe(handles: &[&Owned<Payload>]) -> ChargeObservation {
    let mut observer = ChargeObserver::new();
    for handle in handles {
        observer.owned(handle);
    }
    observer.finish()
}

/// A child two payloads share is one entry, charged once by its own cell;
/// each payload's entry is its own charge, exactly what it measures, and the
/// observation agrees with the cells' own figures.
#[test]
fn a_shared_child_is_observed_once_beside_each_exactly_charged_parent() {
    let account = account();
    let ctx = ChargeCtx::new(&account);
    let name = OwnedStr::new("shared-name");
    let (left, right) = (payload(&name, 64), payload(&name, 512));
    left.pin(ctx);
    right.pin(ctx);

    let observed = observe(&[&left, &right]);
    assert_eq!(observed.len(), 3, "two parents and their one shared child");
    let child = observed
        .entries()
        .iter()
        .find(|entry| entry.reach == Reach::Nested)
        .expect("the shared name is observed as a nested child");
    assert_eq!(
        child.charged_bytes,
        OwnedStr::charge_for_len("shared-name".len())
    );
    for parent in [&left, &right] {
        let entry = observed
            .entries()
            .iter()
            .find(|entry| entry.allocation == AllocationId(Owned::addr(parent)))
            .expect("each parent is observed");
        assert_eq!(entry.reach, Reach::Direct);
        assert_eq!(entry.class, Some(ChargeClass::Pinned));
        assert_eq!(Some(entry.charged_bytes), parent.charge_bytes());
    }
    assert!(observed.is_exact());
    assert_ne!(
        observed.charged_bytes_in(ChargeClass::Pinned),
        0,
        "the figures are real charges"
    );
    assert_eq!(
        observed.charged_bytes(),
        totals(&account).2,
        "the three cells are everything the account holds"
    );

    // Given the same allocations again, through another handle set, the
    // observation is the same: identity is the allocation, not the handle.
    let mut again = ChargeObserver::new();
    again.owned(&left.clone());
    again.owned_handles(&OwnedHandles::of(|aliases| {
        right.offer(aliases) + left.offer(aliases)
    }));
    assert_eq!(again.finish(), observed);
}

/// Reading is free: it neither charges, adopts nor releases, and keeps no
/// handle — the allocation's last drop still releases its whole charge.
#[test]
fn reading_changes_no_class_and_extends_no_lifetime() {
    let account = account();
    let ctx = ChargeCtx::new(&account);
    let name = OwnedStr::new("n");
    let held = payload(&name, 128);
    drop(name);
    held.pin(ctx);
    let before = totals(&account);
    let strong = Owned::strong_count(&held);
    let first = observe(&[&held]);
    let second = observe(&[&held]);
    assert_eq!(first, second, "a repeat read observes the same charges");
    assert_eq!(totals(&account), before, "reading takes and frees nothing");
    assert_eq!(
        Owned::strong_count(&held),
        strong,
        "reading keeps no handle"
    );
    drop(held);
    assert_eq!(
        totals(&account),
        (0, 0, 0),
        "the last handle releases parent and child"
    );
    assert_eq!(first.len(), 2, "the observation outlived what it observed");
}

/// The measured figure is independent of the charge: an uncharged cell
/// measures what its first charge then holds, and a transfer between classes
/// moves the charge without changing what it must equal.
#[test]
fn the_measured_figure_follows_the_value_through_every_transfer() {
    let account = account();
    let ctx = ChargeCtx::new(&account);
    let name = OwnedStr::new("transfer");
    let read = payload(&name, 256);
    drop(name);
    let uncharged = observe(&[&read]);
    assert!(uncharged
        .entries()
        .iter()
        .all(|entry| entry.class.is_none()));
    assert_eq!(uncharged.charged_bytes(), 0);
    assert!(
        !uncharged.is_exact(),
        "an uncharged allocation is not exact"
    );

    read.pin(ctx);
    let pinned = observe(&[&read]);
    let cache = read.hold(ctx).expect("admitted");
    let retained = observe(&[&read]);
    drop(cache);
    let pinned_again = observe(&[&read]);
    for (stage, observed, class) in [
        ("pinned", &pinned, ChargeClass::Pinned),
        ("retained", &retained, ChargeClass::Retained),
        ("handed back", &pinned_again, ChargeClass::Pinned),
    ] {
        assert!(observed.is_exact(), "{stage}: exactly what it measures");
        assert_eq!(
            observed.allocations().collect::<Vec<_>>(),
            uncharged.allocations().collect::<Vec<_>>(),
            "{stage}: the same allocations"
        );
        assert_eq!(
            observed.measured_bytes(),
            uncharged.measured_bytes(),
            "{stage}: the same measured figure as before any charge"
        );
        assert_eq!(
            observed.charged_bytes_in(class),
            observed.charged_bytes(),
            "{stage}: every charge in {class:?}"
        );
    }
}

/// A caller hold's charge is observed by its shared identity, class and
/// bytes; clones name the same entry and an adoption moves its class.
#[test]
fn a_caller_hold_is_observed_by_its_shared_charge() {
    let account = account();
    let hold = CallerHold::pin(&account, 96);
    let clone = hold.clone();
    let mut observer = ChargeObserver::new();
    observer.caller_hold(&hold, 96);
    observer.caller_hold(&clone, 96);
    let pinned = observer.finish();
    assert_eq!(pinned.len(), 1, "both holds share one charge");
    assert!(pinned.is_exact());
    assert_eq!(pinned.charged_bytes_in(ChargeClass::Pinned), 96);

    let cache = hold.adopt().expect("admitted");
    let mut observer = ChargeObserver::new();
    observer.caller_hold(&clone, 96);
    let adopted = observer.finish();
    assert_eq!(adopted.charged_bytes_in(ChargeClass::Retained), 96);
    assert_eq!(
        adopted.allocations().collect::<Vec<_>>(),
        pinned.allocations().collect::<Vec<_>>()
    );
    drop((cache, hold, clone));
    assert_eq!(totals(&account), (0, 0, 0));
}

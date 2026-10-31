use super::{OrderedMemberAggregation, PropertyKeySet};
use crate::semantic_query::{
    key_scan_comparisons_for_tests, AuthoredPropertyKey, CanonicalIndexInt, DeclIdentity,
    PropertyKey, SemanticNodeId,
};

fn number(value: i64) -> AuthoredPropertyKey {
    AuthoredPropertyKey::Number(CanonicalIndexInt::from_canonical_i64(value).unwrap())
}

fn symbol(name: &str) -> AuthoredPropertyKey {
    let identity = DeclIdentity::synthetic(name);
    AuthoredPropertyKey::UniqueSymbol(verter_type_expr::facts::ValueDeclIdentityPart {
        canonical_id: identity.canonical_id.clone(),
        owner: identity.owner,
        symbol: identity.decl_name.clone(),
        member_path: verter_type_expr::OwnedSlice::from([]),
    })
}

/// Dual spellings of one property fold into one entry that keeps the first
/// authored key; a non-canonical numeric spelling, distinct unique symbols
/// and distinct computed keys stay apart; output order is first occurrence.
#[test]
fn aggregation_folds_exactly_the_keys_that_address_one_property() {
    let keys = [
        AuthoredPropertyKey::string("1"),
        number(1),
        AuthoredPropertyKey::string("01"),
        number(-2),
        AuthoredPropertyKey::string("-2"),
        symbol("a"),
        symbol("b"),
        symbol("a"),
        AuthoredPropertyKey::Computed(SemanticNodeId(7)),
        AuthoredPropertyKey::Computed(SemanticNodeId(8)),
        AuthoredPropertyKey::Computed(SemanticNodeId(7)),
        AuthoredPropertyKey::string("x"),
    ];
    let mut aggregation = OrderedMemberAggregation::new();
    for (index, key) in keys.iter().enumerate() {
        aggregation.entry(key, Vec::new).push(index);
    }
    let entries: Vec<(AuthoredPropertyKey, Vec<usize>)> = aggregation.into_entries();
    assert_eq!(
        entries,
        vec![
            (AuthoredPropertyKey::string("1"), vec![0, 1]),
            (AuthoredPropertyKey::string("01"), vec![2]),
            (number(-2), vec![3, 4]),
            (symbol("a"), vec![5, 7]),
            (symbol("b"), vec![6]),
            (
                AuthoredPropertyKey::Computed(SemanticNodeId(7)),
                vec![8, 10]
            ),
            (AuthoredPropertyKey::Computed(SemanticNodeId(8)), vec![9]),
            (AuthoredPropertyKey::string("x"), vec![11]),
        ]
    );
}

/// The work counted for a key sequence is a fact of the keys. Every count
/// past hashing is an equality the table's probe chose to make, so an index
/// seeded per instance would spend a different number on each run and turn
/// any linear-work bound into a coin toss.
#[test]
fn aggregating_the_same_keys_counts_the_same_work_on_every_index() {
    let keys: Vec<_> = (0..512)
        .map(|i| AuthoredPropertyKey::string(format!("key-{i}")))
        .collect();
    let work = || {
        let before = key_scan_comparisons_for_tests();
        let mut aggregation = OrderedMemberAggregation::new();
        for key in &keys {
            aggregation.entry(key, || ());
        }
        for key in &keys {
            assert!(aggregation.position(key).is_some());
        }
        key_scan_comparisons_for_tests() - before
    };
    let first = work();
    for _ in 0..8 {
        assert_eq!(work(), first, "one key sequence, one count");
    }
}

/// An index sized for its members hashes each key once: growing into the
/// same keys rehashes every key it already holds, at each doubling.
#[test]
fn a_sized_aggregation_hashes_each_key_once() {
    let keys: Vec<_> = (0..512)
        .map(|i| AuthoredPropertyKey::string(format!("key-{i}")))
        .collect();
    let before = key_scan_comparisons_for_tests();
    let mut aggregation = OrderedMemberAggregation::with_capacity(keys.len());
    for key in &keys {
        aggregation.entry(key, || ());
    }
    let sized = key_scan_comparisons_for_tests() - before;
    let before = key_scan_comparisons_for_tests();
    let mut aggregation = OrderedMemberAggregation::new();
    for key in &keys {
        aggregation.entry(key, || ());
    }
    let grown = key_scan_comparisons_for_tests() - before;
    assert!(
        sized + keys.len() / 2 < grown,
        "a sized index skips the rehashes: sized {sized}, grown {grown}"
    );
}

/// A key set answers under the same identity, and never for a computed key.
#[test]
fn key_set_membership_follows_property_identity() {
    let set = PropertyKeySet::new(&[PropertyKey::from("3"), PropertyKey::from("name")]);
    assert!(set.contains(&number(3)));
    assert!(set.contains(&AuthoredPropertyKey::string("3")));
    assert!(set.contains(&AuthoredPropertyKey::string("name")));
    assert!(!set.contains(&AuthoredPropertyKey::string("03")));
    assert!(!set.contains(&number(4)));
    assert!(!set.contains(&AuthoredPropertyKey::Computed(SemanticNodeId(3))));
}

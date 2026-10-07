use super::{MemberHeader, MemberHeaderList};
use verter_type_expr::{CanonicalIndexInt, TypeAuthoredPropertyKey};

fn header(key: TypeAuthoredPropertyKey, optional: bool) -> MemberHeader {
    MemberHeader {
        key,
        method_kind: None,
        has_implementation_body: false,
        optional,
        readonly: false,
    }
}

fn named(name: &str, optional: bool) -> MemberHeader {
    header(TypeAuthoredPropertyKey::string(name), optional)
}

fn numeric(value: i64) -> TypeAuthoredPropertyKey {
    TypeAuthoredPropertyKey::Number(
        CanonicalIndexInt::from_canonical_i64(value).expect("canonical index"),
    )
}

fn keys(list: &MemberHeaderList) -> Vec<TypeAuthoredPropertyKey> {
    list.iter().map(|member| member.key.clone()).collect()
}

/// Every recorded key resolves through the index to the member at its
/// source-order position.
fn assert_index_matches_order(list: &MemberHeaderList) {
    for member in list.iter() {
        assert!(
            std::ptr::eq(list.get(&member.key).expect("indexed key"), member),
            "index position for {:?} must name its ordered member",
            member.key
        );
    }
}

#[test]
fn first_wins_keeps_first_header_and_position() {
    let list: MemberHeaderList = [
        named("a", false),
        named("b", false),
        named("a", true),
        named("c", false),
        named("b", true),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        keys(&list),
        ["a", "b", "c"].map(TypeAuthoredPropertyKey::string)
    );
    assert!(list.iter().all(|member| !member.optional));
    assert_index_matches_order(&list);
}

#[test]
fn last_wins_keeps_last_header_at_last_position() {
    let list = MemberHeaderList::from_last_wins([
        named("a", false),
        named("b", false),
        named("a", true),
        named("c", false),
        named("b", true),
    ]);
    assert_eq!(
        keys(&list),
        ["a", "c", "b"].map(TypeAuthoredPropertyKey::string)
    );
    assert!(
        list.get(&TypeAuthoredPropertyKey::string("a"))
            .unwrap()
            .optional
    );
    assert!(
        list.get(&TypeAuthoredPropertyKey::string("b"))
            .unwrap()
            .optional
    );
    assert!(
        !list
            .get(&TypeAuthoredPropertyKey::string("c"))
            .unwrap()
            .optional
    );
    assert_index_matches_order(&list);
}

#[test]
fn numeric_and_string_spellings_stay_distinct_keys() {
    let one = numeric(1);
    let string_one = TypeAuthoredPropertyKey::string("1");
    let first = MemberHeaderList::from_iter([
        header(one.clone(), false),
        header(string_one.clone(), false),
        header(one.clone(), true),
    ]);
    assert_eq!(keys(&first), [one.clone(), string_one.clone()]);
    assert!(!first.get(&one).unwrap().optional);

    let last = MemberHeaderList::from_last_wins([
        header(one.clone(), false),
        header(string_one.clone(), false),
        header(one.clone(), true),
    ]);
    assert_eq!(keys(&last), [string_one, one.clone()]);
    assert!(last.get(&one).unwrap().optional);
    assert_index_matches_order(&last);
}

#[test]
fn union_into_empty_and_non_empty_lists_is_first_wins() {
    let mut list = MemberHeaderList::new();
    list.union_first_wins(MemberHeaderList::from_last_wins([
        named("x", false),
        named("y", false),
    ]));
    list.union_first_wins([named("y", true), named("z", false)].into_iter().collect());
    assert_eq!(
        keys(&list),
        ["x", "y", "z"].map(TypeAuthoredPropertyKey::string)
    );
    assert!(
        !list
            .get(&TypeAuthoredPropertyKey::string("y"))
            .unwrap()
            .optional
    );
    assert_index_matches_order(&list);
}

/// Key work grows with the member count, not with its square: each offered
/// header costs a bounded number of index operations regardless of how many
/// members are already recorded.
#[test]
fn wide_member_lists_cost_linear_key_work() {
    let widths = [128_u64, 256, 512, 1024];
    let mut first_wins = Vec::new();
    let mut last_wins = Vec::new();
    for width in widths {
        // Every member appears twice so each build also exercises the
        // duplicate path.
        let offered: Vec<MemberHeader> = (0..2 * width)
            .map(|i| named(&format!("m{}", i % width), false))
            .collect();
        let first: MemberHeaderList = offered.iter().cloned().collect();
        let last = MemberHeaderList::from_last_wins(offered);
        assert_eq!(first.len() as u64, width);
        assert_eq!(last.len() as u64, width);
        // `width` inserts (lookup + insert) and `width` duplicate lookups.
        assert_eq!(first.key_probes(), 3 * width);
        // One index operation per offered header.
        assert_eq!(last.key_probes(), 2 * width);
        first_wins.push(first.key_probes());
        last_wins.push(last.key_probes());
    }
    for pair in first_wins.windows(2).chain(last_wins.windows(2)) {
        assert_eq!(pair[1], 2 * pair[0], "doubling the width doubles key work");
    }
}

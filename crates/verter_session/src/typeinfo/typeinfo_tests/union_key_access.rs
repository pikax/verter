//! @ai-generated - Indexed access with a union key contracts.
//!
//! TDD-red tests for `T["a" | "b"]` and `T[keyof T]`, plus the
//! `Pick<T, "a" | "b">` companion.

use super::support::*;

const UNION_KEY_ACCESS: &str = include_str!("fixtures/union_key_access.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/union_key_access.ts", UNION_KEY_ACCESS);
}

#[test]
#[ignore = "the U2 IndexedAccess-reduction bridge has landed (operator-bodied alias reduction in resolve_named_symbol); the remaining blocker is union-key distribution — distributing indexed access over a union key into a union of member value types"]
fn union_key_access_two_key_union_projects_member_type_union() {
    // TS7 contract: `Surface["alpha" | "beta"]` = `Surface["alpha"] |
    // Surface["beta"]` = `number | string`. TS distributes indexed access
    // over the key union and unions the per-key results.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/union_key_access.ts",
        "AlphaBeta",
        &[],
        ProjectionMode::Expanded,
    );

    assert_union_contains_primitive(&expr, PrimitiveName::Number);
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    let TypeExpr::Union(types) = &expr else {
        panic!("expected union, got {expr:?}");
    };
    assert_eq!(
        types.len(),
        2,
        "expected exactly two arms (number, string), got {types:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "the U2 IndexedAccess-reduction bridge has landed (operator-bodied alias reduction in resolve_named_symbol); the remaining blocker is `T[keyof T]` value-union — reducing keyof-self indexed access to the structural value-type union of all members"]
fn union_key_access_keyof_self_projects_full_value_union() {
    // TS7 contract: `Surface[keyof Surface]` projects every member type into
    // a union: `number | string | boolean | null`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/union_key_access.ts",
        "EveryMember",
        &[],
        ProjectionMode::Expanded,
    );

    assert_union_contains_primitive(&expr, PrimitiveName::Number);
    assert_union_contains_primitive(&expr, PrimitiveName::String);
    assert_union_contains_primitive(&expr, PrimitiveName::Boolean);
    assert_union_contains_primitive(&expr, PrimitiveName::Null);
    let TypeExpr::Union(types) = &expr else {
        panic!("expected union, got {expr:?}");
    };
    assert_eq!(
        types.len(),
        4,
        "expected exactly four arms (number, string, boolean, null), got {types:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn union_key_access_pick_companion_publishes_selected_subset() {
    // TS7 contract: `Pick<Surface, "alpha" | "beta">` materialises both
    // selected members and excludes the others. This active test verifies the
    // existing path-precise Pick behaviour Verter already enforces.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/union_key_access.ts",
        "PickAlphaBeta",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["alpha", "beta"]);
    assert_primitive(&props["alpha"].ty, PrimitiveName::Number);
    assert_primitive(&props["beta"].ty, PrimitiveName::String);
    assert!(!props.contains_key("gamma"));
    assert!(!props.contains_key("delta"));
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

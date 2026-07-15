//! @ai-generated - `const` type-parameter contracts (TS7).
//!
//! TDD-red tests describing TS7 behaviour for `<const T extends …>`: literal
//! and tuple values passed at call sites are preserved as readonly literals
//! at the type level, without an explicit `as const` at the use site.

use super::support::*;

const CONST_TYPE_PARAM: &str = include_str!("fixtures/const_type_param.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/const_type_param.ts", CONST_TYPE_PARAM);
}

#[test]
#[ignore = "typeinfo currently does not apply the TS7 `<const T>` modifier when inferring T from a call-site array argument; keep as the future const-type-param route contract"]
fn const_type_param_route_call_preserves_readonly_tuple_with_literal_paths() {
    // TS7 contract: `makeRoute([{ path: "/home" }, { path: "/about" }])` with
    // `<const T extends readonly { path: string }[]>` infers T as the
    // readonly tuple of two readonly object literals:
    //   readonly [{ readonly path: "/home" }, { readonly path: "/about" }]
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/const_type_param.ts",
        "ConstRouteResult",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, readonly } = &expr else {
        panic!("expected readonly tuple, got {expr:?}");
    };
    assert!(readonly);
    assert_eq!(elements.len(), 2);

    let first = object_props(&elements[0].ty);
    assert_eq!(prop_names(&first), vec!["path"]);
    assert!(first["path"].readonly);
    assert_string_literal(&first["path"].ty, "/home");

    let second = object_props(&elements[1].ty);
    assert_eq!(prop_names(&second), vec!["path"]);
    assert!(second["path"].readonly);
    assert_string_literal(&second["path"].ty, "/about");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not apply the TS7 `<const T>` modifier to a readonly string-tuple call-site; keep as the future const-type-param string-tuple contract"]
fn const_type_param_string_call_preserves_readonly_literal_string_tuple() {
    // TS7 contract: `makeStrings(["a", "b", "c"])` with `<const T extends
    // readonly string[]>` infers T as `readonly ["a", "b", "c"]` — readonly
    // tuple of the literal strings, even without an explicit `as const` on
    // the call-site array.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/const_type_param.ts",
        "ConstStringsResult",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, readonly } = &expr else {
        panic!("expected readonly tuple, got {expr:?}");
    };
    assert!(readonly);
    assert_eq!(elements.len(), 3);
    assert_string_literal(&elements[0].ty, "a");
    assert_string_literal(&elements[1].ty, "b");
    assert_string_literal(&elements[2].ty, "c");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

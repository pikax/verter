//! @ai-generated - Mapped-type modifier contracts.
//!
//! Tests describing TS7 behaviour for the modifier adders and removers that
//! may appear inside a mapped type body:
//!
//!   * `+readonly` adder — marks every member readonly. (Verter handles.)
//!   * `-readonly` remover (Mutable<T>) — strips readonly from every member.
//!     (Verter handles.)
//!   * `+?` adder (AllOptional<T>) — marks every member optional.
//!     (Verter handles.)
//!   * `-?` remover (AllRequired<T>) — strips optional AND removes
//!     `undefined` from the slot type. (Verter handles.)
//!   * Combined `-readonly -?` — both modifier passes compose.
//!     (Verter handles.)
//!   * `as never` key filter — drops keys whose remap resolves to `never`.
//!     (Verter does NOT reduce yet; `#[ignore]`d as a future contract.)
//!   * Mapped type whose value is a conditional — TS7 keeps `never`-valued
//!     members in the projected surface (it does NOT prune them).
//!     (Verter does NOT reduce yet; `#[ignore]`d as a future contract.)

use super::support::*;

const MAPPED_MODIFIERS: &str = include_str!("fixtures/mapped_modifiers.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/mapped_modifiers.ts", MAPPED_MODIFIERS);
}

#[test]
fn mapped_modifier_plus_readonly_marks_every_member_readonly() {
    // TS7 contract: `AllReadonly<{ a: string; b: number }>` =
    // `{ readonly a: string; readonly b: number }`. The mapped form
    // `{ +readonly [K in keyof T]: T[K] }` explicitly adds readonly to every
    // projected member.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped_modifiers.ts",
        "AddReadonlyResult",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a", "b"]);
    assert!(props["a"].readonly);
    assert!(props["b"].readonly);
    assert!(!props["a"].optional);
    assert!(!props["b"].optional);
    assert_primitive(&props["a"].ty, PrimitiveName::String);
    assert_primitive(&props["b"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn mapped_modifier_minus_readonly_strips_readonly_from_every_member() {
    // TS7 contract: `Mutable<{ readonly a: string; readonly b: number }>` =
    // `{ a: string; b: number }`. The `-readonly` mapped form removes the
    // readonly modifier on every projected member.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped_modifiers.ts",
        "MutableResult",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a", "b"]);
    assert!(!props["a"].readonly);
    assert!(!props["b"].readonly);
    assert!(!props["a"].optional);
    assert!(!props["b"].optional);
    assert_primitive(&props["a"].ty, PrimitiveName::String);
    assert_primitive(&props["b"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn mapped_modifier_plus_optional_marks_every_member_optional() {
    // TS7 contract: `AllOptional<{ a: string; b: number }>` =
    // `{ a?: string; b?: number }`. The `+?` mapped form marks every projected
    // member optional. The slot type itself is NOT widened with `undefined` in
    // the structural surface (the optional marker is what TS records).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped_modifiers.ts",
        "AddOptionalResult",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a", "b"]);
    assert!(props["a"].optional);
    assert!(props["b"].optional);
    assert!(!props["a"].readonly);
    assert!(!props["b"].readonly);
    assert_primitive(&props["a"].ty, PrimitiveName::String);
    assert_primitive(&props["b"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not strip `undefined` from the slot type when applying the `-?` mapped modifier; it only toggles the optional marker. Verter publishes `string | undefined` for what should be bare `string`. Keep as the future `-?` undefined-stripping contract"]
fn mapped_modifier_minus_optional_strips_optional_and_undefined() {
    // TS7 contract: `AllRequired<{ a: string | undefined; b: number | undefined }>`
    // = `{ a: string; b: number }`. The `-?` mapped form strips the optional
    // marker AND removes `undefined` from the slot type.
    //
    // The fixture uses explicit `| undefined` slot types (instead of the `?`
    // shorthand) on purpose: with `a?: string` an implementer can pass by
    // only flipping `optional → false`. With `a: string | undefined`, the
    // resolver MUST ALSO rewrite the slot type to bare `string` — that's
    // the actual TS7 contract for `-?` and it's what this assertion locks
    // in (asserting `Primitive::String`, not `String | Undefined`).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped_modifiers.ts",
        "RemoveOptionalResult",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a", "b"]);
    assert!(!props["a"].optional);
    assert!(!props["b"].optional);
    // Asserting bare primitives (no `| undefined`) characterises the
    // undefined-stripping behaviour of `-?`.
    assert_primitive(&props["a"].ty, PrimitiveName::String);
    assert_primitive(&props["b"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn mapped_modifier_minus_readonly_minus_optional_strips_both() {
    // TS7 contract: `WritableRequired<{ readonly a?: string; readonly b?: number }>` =
    // `{ a: string; b: number }`. Both modifier removers compose in a single
    // mapped pass: readonly is stripped, optional is stripped, and the slot
    // type loses the optional-driven `undefined`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped_modifiers.ts",
        "WritableRequiredResult",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a", "b"]);
    assert!(!props["a"].readonly);
    assert!(!props["b"].readonly);
    assert!(!props["a"].optional);
    assert!(!props["b"].optional);
    assert_primitive(&props["a"].ty, PrimitiveName::String);
    assert_primitive(&props["b"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not evaluate `as never` key filters in a mapped-type key remap; keep as the future as-never key-filter contract"]
fn mapped_modifier_as_never_filter_drops_matching_keys() {
    // TS7 contract: `DropPrivate<{ _internal: string; visible: number; _hidden: boolean }>`
    // remaps each key through `K extends \`_${string}\` ? never : K`. Keys that
    // resolve to `never` are pruned. Only `visible` survives, carrying its
    // original value type.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped_modifiers.ts",
        "DropPrivateResult",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["visible"]);
    assert_primitive(&props["visible"].ty, PrimitiveName::Number);
    assert!(
        !props.contains_key("_internal"),
        "`as never` key filter must drop _internal from the surface"
    );
    assert!(
        !props.contains_key("_hidden"),
        "`as never` key filter must drop _hidden from the surface"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not evaluate a mapped type whose value position is a conditional with `T[K] extends string` discriminating each member; keep as the future conditional-value mapped-type contract"]
fn mapped_modifier_conditional_value_keeps_never_typed_members() {
    // TS7 contract: `StringValuesOnly<{ a: string; b: number; c: "literal" }>` =
    // `{ a: string; b: never; c: "literal" }`. The mapped value is a
    // conditional `T[K] extends string ? T[K] : never` — keys whose original
    // value is NOT a string survive the projection but their value collapses
    // to `never`. TS7 does NOT prune `never`-valued members from a mapped
    // type's surface (this differs from `as never` key remapping above).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped_modifiers.ts",
        "StringValuesOnlyResult",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a", "b", "c"]);
    assert_primitive(&props["a"].ty, PrimitiveName::String);
    assert_primitive(&props["b"].ty, PrimitiveName::Never);
    assert_string_literal(&props["c"].ty, "literal");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// Edge cases — generic-constrained key, modifier idempotence, `as` rename
// ---------------------------------------------------------------------------

#[test]
fn mapped_modifier_generic_constrained_key_projects_subset() {
    // TS7 contract: `Pick2<{ a: number; b: string; c: boolean }, "a" | "c">` =
    // `{ a: number; c: boolean }`. The mapped form
    // `{ [P in K]: T[P] }` instantiates `K = "a" | "c"` (constrained to
    // `keyof T`) and projects only those members from `T`. Equivalent to
    // built-in `Pick<T, K>`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped_modifiers.ts",
        "Pick2Result",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a", "c"]);
    assert_primitive(&props["a"].ty, PrimitiveName::Number);
    assert_primitive(&props["c"].ty, PrimitiveName::Boolean);
    assert!(
        !props.contains_key("b"),
        "Pick2<T, \"a\" | \"c\"> must drop `b` from the projected surface"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn mapped_modifier_plus_readonly_idempotent_over_readonly_source() {
    // TS7 contract: `AllReadonly<{ readonly a: string; readonly b: number }>` is
    // structurally identical to the source — `+readonly` over an already-readonly
    // source is a no-op. Both members survive, both stay readonly, neither is
    // optional. The structural surface does NOT double-mark readonly; the modifier
    // is idempotent.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped_modifiers.ts",
        "ReadonlyOverReadonly",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["a", "b"]);
    assert!(props["a"].readonly);
    assert!(props["b"].readonly);
    assert!(!props["a"].optional);
    assert!(!props["b"].optional);
    assert_primitive(&props["a"].ty, PrimitiveName::String);
    assert_primitive(&props["b"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not evaluate a mapped-type `as` key remap that rewrites every key through `Capitalize<K>`; the result surface keeps the original lowercase keys instead of the capitalised forms. Keep as the future as-rename-without-filter contract"]
fn mapped_modifier_as_rename_capitalize_rewrites_keys() {
    // TS7 contract: `CapitalizeKeys<{ alpha: number; beta: string }>` =
    // `{ Alpha: number; Beta: string }`. The `as` clause renames each key via
    // `Capitalize<K>` (every key survives because `K extends string` holds for
    // every string key). Value types are preserved from the source.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped_modifiers.ts",
        "CapitalizedResult",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["Alpha", "Beta"]);
    assert_primitive(&props["Alpha"].ty, PrimitiveName::Number);
    assert_primitive(&props["Beta"].ty, PrimitiveName::String);
    assert!(
        !props.contains_key("alpha"),
        "`as Capitalize<K>` key remap must drop the original lowercase `alpha` from the surface"
    );
    assert!(
        !props.contains_key("beta"),
        "`as Capitalize<K>` key remap must drop the original lowercase `beta` from the surface"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

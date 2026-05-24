//! @ai-generated - Template-literal-type pattern-matching contracts.
//!
//! TDD-red tests describing TS7 expected behaviour when `infer` is used inside
//! a template-literal pattern, including recursive split, prefix stripping,
//! key remap with `Capitalize`, and `infer X extends number` casts.

use super::support::*;

const TEMPLATE_LITERAL_INFERENCE: &str = include_str!("fixtures/template_literal_inference.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(
        host,
        "/fixtures/template_literal_inference.ts",
        TEMPLATE_LITERAL_INFERENCE,
    );
}

#[test]
#[ignore = "typeinfo currently does not iteratively split a string-literal type through a template pattern with `infer`; keep as the future template-literal split contract"]
fn template_literal_split_on_dot_produces_segment_tuple() {
    // TS7 contract: `SplitOn<"a.b.c", ".">` recursively peels the pattern
    // `${infer H}.${infer T}`, prepending each head to the result of splitting
    // the tail. Result: `["a", "b", "c"]` (a readonly-free tuple of literals).
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/template_literal_inference.ts",
        "DotSplitAbc",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Tuple { elements, readonly } = expr else {
        panic!("expected tuple, got {expr:?}");
    };
    assert!(!readonly);
    assert_eq!(elements.len(), 3);
    assert_string_literal(&elements[0].ty, "a");
    assert_string_literal(&elements[1].ty, "b");
    assert_string_literal(&elements[2].ty, "c");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not strip a literal prefix with `infer Rest` + Uncapitalize; keep as the future template-literal prefix-strip contract"]
fn template_literal_strip_on_prefix_uncapitalises_remainder() {
    // TS7 contract: `StripOnPrefix<"onClick">` matches `\`on${infer Rest}\``
    // (Rest = "Click"), then applies `Uncapitalize<"Click">` = `"click"`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/template_literal_inference.ts",
        "StripOnClick",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "click");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not select the false branch of a template-literal conditional when the pattern fails to match; keep as the future template-literal no-match contract"]
fn template_literal_strip_returns_input_unchanged_when_prefix_missing() {
    // TS7 contract: `StripOnPrefix<"submit">` fails the `\`on${infer Rest}\``
    // pattern check and selects the false branch, returning the input string
    // literal `"submit"` unchanged.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/template_literal_inference.ts",
        "StripOnUnused",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "submit");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not apply template-literal `as` key remap with Capitalize over a union of input keys; keep as the future event-handler key-remap contract"]
fn template_literal_key_remap_capitalises_each_event_key() {
    // TS7 contract: `EventHandlers<"inc" | "dec">` produces an object where
    // each key is remapped through `\`on${Capitalize<K>}\``, yielding
    // `{ onInc: (payload: "inc") => void; onDec: (payload: "dec") => void }`.
    // The remapped keys are `onInc` / `onDec`; the handlers preserve the
    // original key as the payload literal.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/template_literal_inference.ts",
        "CounterHandlers",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["onDec", "onInc"]);

    let on_inc = function_type(&props["onInc"].ty);
    assert_eq!(on_inc.parameters.len(), 1);
    assert_string_literal(&on_inc.parameters[0].ty, "inc");

    let on_dec = function_type(&props["onDec"].ty);
    assert_eq!(on_dec.parameters.len(), 1);
    assert_string_literal(&on_dec.parameters[0].ty, "dec");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not evaluate `infer X extends number` casts in template-literal patterns; keep as the future numeric-infer template contract"]
fn template_literal_numeric_infer_extends_number_casts_to_literal() {
    // TS7 contract: `\`${infer D extends number}\`` matches a numeric string
    // and casts the inferred segment to a numeric literal. For input "42",
    // `D = 42` (the literal number, not the string), so the projected type is
    // the numeric literal `42`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/template_literal_inference.ts",
        "Digit42",
        &[],
        ProjectionMode::Expanded,
    );

    assert_number_literal(&expr, 42.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

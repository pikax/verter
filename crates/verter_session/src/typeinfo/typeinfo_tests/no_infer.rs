//! @ai-generated - NoInfer<T> contracts (TS7).
//!
//! TDD-red tests describing TS7 behaviour for `NoInfer<T>` parameter
//! positions: T is fixed by the unwrapped argument and the NoInfer-wrapped
//! argument does NOT contribute to inference.

use super::support::*;

const NO_INFER: &str = include_str!("fixtures/no_infer.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/no_infer.ts", NO_INFER);
}

#[test]
#[ignore = "typeinfo currently does not honour the TS7 NoInfer<T> marker; keep as the future NoInfer literal-pinning contract"]
fn no_infer_literal_call_returns_pinned_literal_from_first_argument() {
    // TS7 contract: `pickValue("ok" as const, "ok")` infers `T = "ok"` from
    // the first argument only. The second argument is checked against
    // `NoInfer<T>` (which is structurally `T` but blocked from contributing
    // to inference). Result: `"ok"` literal.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/no_infer.ts",
        "NoInferFixedLiteralResult",
        &[],
        ProjectionMode::Expanded,
    );

    assert_string_literal(&expr, "ok");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo currently does not honour NoInfer<T> through wrapper helpers like Partial<T>; keep as the future NoInfer component-defaults contract"]
fn no_infer_component_helper_pins_variant_from_props_argument() {
    // TS7 contract: `makeComponent({ variant: "primary" as const, label: "Save" }, ...)`
    // pins `TVariant = "primary"` from the first argument. The second argument
    // is `NoInfer<Partial<ComponentProps<TVariant>>>` — checked but NOT
    // contributing to TVariant inference. The result is
    // `ComponentProps<"primary"> = { variant: "primary"; label: string }`.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/no_infer.ts",
        "NoInferComponentResult",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["label", "variant"]);
    assert_string_literal(&props["variant"].ty, "primary");
    assert_primitive(&props["label"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

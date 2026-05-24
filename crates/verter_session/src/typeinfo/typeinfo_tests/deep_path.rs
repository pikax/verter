//! @ai-generated - Synthetic deep indexed-access path typeinfo tests.

use super::support::*;

#[test]
#[ignore = "typeinfo currently preserves long indexed-access chains instead of reducing them to the terminal object; keep as the future deep-path projection contract"]
fn deep_path_projection_resolves_terminal_without_losing_shape() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/deep-path.ts", DEEP_PATH);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/deep-path.ts",
        "DeepProjectedTarget",
        &[],
        ProjectionMode::Expanded,
    );

    let target = object_props(&expr);
    assert_primitive(&target["id"].ty, PrimitiveName::String);
    assert_number_literal_union(&target["priority"].ty, &[1.0, 2.0, 3.0]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

//! @ai-generated - Synthetic mapped-type and template-literal key tests.

use super::support::*;

#[test]
fn mapped_type_without_key_remap_materializes_required_surface() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "PlainSlotMap",
        &[],
        ProjectionMode::Expanded,
    );

    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["empty", "item", "root"]);
    assert!(!props["root"].optional);
    let root = object_props(&props["root"].ty);
    assert_primitive(&root["id"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo does not yet materialize mapped types whose keys are remapped with a template-literal name; keep as the future key-remapping contract"]
fn mapped_type_with_template_literal_key_remap_resolves_remapped_slot() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "RootRemappedSlot",
        &[],
        ProjectionMode::Expanded,
    );

    let slot = function_type(&expr);
    assert_eq!(slot.parameters.len(), 1);
    let payload = object_props(&slot.parameters[0].ty);
    assert_primitive(&payload["id"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "typeinfo does not yet substitute remapped template-literal mapped keys for non-root members; keep as the future remapped member projection contract"]
fn mapped_type_with_template_literal_key_remap_resolves_item_slot() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "ItemRemappedSlot",
        &[],
        ProjectionMode::Expanded,
    );

    let slot = function_type(&expr);
    assert_eq!(slot.parameters.len(), 1);
    let payload = object_props(&slot.parameters[0].ty);
    assert_primitive(&payload["value"].ty, PrimitiveName::Number);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "reducer resolves this correctly (covered by the non-ignored `template_literal_key_reducer_projects_callable_slots` regression); NOT oracle-liftable — the reduced value is a function/callable renderer slot that the oracle §Q2 positive-allowlist rejects (Reject(Callable)). Lift pending an oracle admission + hover-grammar extension for clean function values"]
fn template_literal_key_alias_projects_static_template_slot() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "NameCellRenderer",
        &[],
        ProjectionMode::Expanded,
    );

    let renderer = function_type(&expr);
    assert_eq!(renderer.parameters.len(), 1);
    let payload = object_props(&renderer.parameters[0].ty);
    assert_primitive(&payload["value"].ty, PrimitiveName::String);
    assert_string_literal(&payload["column"].ty, "name");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "reducer resolves this correctly (covered by the non-ignored `template_literal_key_reducer_projects_callable_slots` regression); NOT oracle-liftable — the reduced value is a function/callable Record value slot that the oracle §Q2 positive-allowlist rejects (Reject(Callable)). Lift pending an oracle admission + hover-grammar extension for clean function values"]
fn record_with_template_literal_key_union_projects_root_slot() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "RecordTemplateRootSlot",
        &[],
        ProjectionMode::Expanded,
    );

    let slot = function_type(&expr);
    assert_eq!(slot.parameters.len(), 1);
    let payload = object_props(&slot.parameters[0].ty);
    assert_literal_union(&payload["name"].ty, &["item", "root"]);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
#[ignore = "reducer resolves this correctly (covered by the non-ignored `template_literal_key_reducer_projects_callable_slots` regression); NOT oracle-liftable — the reduced value is a union of function/callable renderer slots that the oracle §Q2 positive-allowlist rejects (Reject(Callable)). Lift pending an oracle admission + hover-grammar extension for clean function values"]
fn template_literal_union_key_projects_static_slot_union() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "StaticTemplateSlotUnion",
        &[],
        ProjectionMode::Expanded,
    );

    let TypeExpr::Union(arms) = &expr else {
        panic!("expected union of cell renderers, got {expr:?}");
    };
    assert_eq!(arms.len(), 2);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

/// Non-ignored reducer regression for the three callable-result template-literal
/// key rows (`template_literal_key_alias_projects_static_template_slot`,
/// `record_with_template_literal_key_union_projects_root_slot`,
/// `template_literal_union_key_projects_static_slot_union`). Those rows stay
/// `#[ignore]`d as oracle-lift contracts (their reduced values are function
/// surfaces the oracle §Q2 admission rejects), but the SHARED resolver reduces
/// each one correctly — this test runs the same assertions in the normal suite
/// so the U2.MAPPED_TEMPLATE reducer paths (template-literal alias key, Record
/// template-union keyspace enumeration, union-index distribution) stay guarded.
#[test]
fn template_literal_key_reducer_projects_callable_slots() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/mapped-template.ts", MAPPED_TEMPLATE);

    // (a) template-literal alias key → single static slot.
    let (alias, _) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "NameCellRenderer",
        &[],
        ProjectionMode::Expanded,
    );
    let renderer = function_type(&alias);
    let payload = object_props(&renderer.parameters[0].ty);
    assert_primitive(&payload["value"].ty, PrimitiveName::String);
    assert_string_literal(&payload["column"].ty, "name");

    // (b) Record over a template-literal-union keyspace → indexed value slot.
    let (record_slot, _) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "RecordTemplateRootSlot",
        &[],
        ProjectionMode::Expanded,
    );
    let slot = function_type(&record_slot);
    let slot_payload = object_props(&slot.parameters[0].ty);
    assert_literal_union(&slot_payload["name"].ty, &["item", "root"]);

    // (c) union template-literal index → union-distributed slot union.
    let (union_slots, _) = resolve_expr(
        &host,
        "/fixtures/mapped-template.ts",
        "StaticTemplateSlotUnion",
        &[],
        ProjectionMode::Expanded,
    );
    let TypeExpr::Union(arms) = &union_slots else {
        panic!("expected union-distributed cell renderers, got {union_slots:?}");
    };
    assert_eq!(arms.len(), 2, "union index must distribute into 2 slots");
    // Discriminate the reduction: BOTH arms must be FUNCTION renderers whose
    // payload object is the expected `{ value; column }` shape. A broken
    // reducer that returned any two-arm union would pass the bare `len == 2`
    // check above, so match each arm by its `column` literal (order-independent)
    // and assert the paired `value` primitive.
    let mut saw_name_arm = false;
    let mut saw_count_arm = false;
    for arm in arms.iter() {
        let renderer = function_type(arm);
        assert_eq!(
            renderer.parameters.len(),
            1,
            "each cell renderer takes one payload param, got {arm:?}"
        );
        let payload = object_props(&renderer.parameters[0].ty);
        assert_eq!(
            prop_names(&payload),
            vec!["column", "value"],
            "payload must carry exactly value + column"
        );
        match string_literal_value(&payload["column"].ty) {
            "name" => {
                assert_primitive(&payload["value"].ty, PrimitiveName::String);
                saw_name_arm = true;
            }
            "count" => {
                assert_primitive(&payload["value"].ty, PrimitiveName::Number);
                saw_count_arm = true;
            }
            other => panic!("unexpected column literal {other:?} in {arm:?}"),
        }
    }
    assert!(
        saw_name_arm && saw_count_arm,
        "must distribute into the `cell:name`(value:string) AND `cell:count`(value:number) renderers"
    );
}

/// FIX 1 guard — the template-literal keyspace product-width budget. A finite
/// template whose cartesian product `∏ |choice_set_i|` exceeds
/// `TEMPLATE_LITERAL_KEYSPACE_CAP` must CARRIER-STOP to the deferred
/// `TemplateLiteral` shell and the result must NOT be warm-admitted (it is a
/// non-cacheable budget-tainted partial). Discriminates the pre-fix unbounded
/// cartesian build (which enumerated the full union with `cache_suppress=false`).
#[test]
fn keyspace_budget_exceeded_admits_nothing() {
    use crate::semantic_query::{QueryResult, SemanticNodeData};

    let host = make_host_with_footprint();

    // Two fully-CLOSED finite unions whose product (40 × 40 = 1600) exceeds the
    // cap (1024). Each union is a wide set of distinct string literals.
    let union_a: Vec<String> = (0..40).map(|i| format!("a{i}")).collect();
    let union_b: Vec<String> = (0..40).map(|i| format!("b{i}")).collect();
    let (read, graph) = template_literal_reduce_read(&host, &["", "-", ""], &[union_a, union_b]);

    // (1) Carrier-stops: the value is the deferred TemplateLiteral SHELL, never
    //     an enumerated 1600-arm literal union.
    let value = match read.value {
        QueryResult::Value(id) => id,
        other => panic!("expected a value node, got {other:?}"),
    };
    let data = graph.node_data(value).expect("value node must exist");
    assert!(
        matches!(data.as_ref(), SemanticNodeData::TemplateLiteral { .. }),
        "over-cap keyspace must carrier-stop to the TemplateLiteral shell, got {:?}",
        data.as_ref()
    );
    assert!(
        !matches!(data.as_ref(), SemanticNodeData::Union(_)),
        "over-cap keyspace must NOT enumerate a literal union"
    );

    // (2) Not warm-admitted: the over-budget product is a non-cacheable,
    //     budget-tainted partial.
    assert!(
        read.cache_suppress,
        "an over-cap template product must be cache-suppressed (never warm-admitted)"
    );
    assert!(
        read.result_is_partial,
        "an over-cap template product must be flagged a budget-tainted partial"
    );

    // Control: an UNDER-cap product (2 × 2 = 4) still fully enumerates and is
    // admissible — the cap must not just always carrier-stop.
    let (small_read, small_graph) = template_literal_reduce_read(
        &host,
        &["", "-", ""],
        &[
            vec!["x".to_string(), "y".to_string()],
            vec!["1".to_string(), "2".to_string()],
        ],
    );
    let small_value = match small_read.value {
        QueryResult::Value(id) => id,
        other => panic!("expected a value node, got {other:?}"),
    };
    let small_data = small_graph.node_data(small_value).expect("node must exist");
    assert!(
        matches!(small_data.as_ref(), SemanticNodeData::Union(_)),
        "under-cap product must fully enumerate to a union, got {:?}",
        small_data.as_ref()
    );
    assert!(
        !small_read.cache_suppress,
        "under-cap product is a complete result and must not be suppressed"
    );
    assert!(
        !small_read.result_is_partial,
        "under-cap product is complete, not a partial"
    );
}

/// FIX 2 guard — the shared template reducer models TS numeric / boolean /
/// bigint literal interpolation. TS lexes `` `${1 | 2}` `` ⇒ `"1" | "2"`,
/// `` `${true}` `` ⇒ `"true"`, and a bigint literal as its base-10 digits.
/// Discriminates the pre-fix fail-closed code (which admitted ONLY string
/// literals and carrier-stopped these to the deferred shell).
#[test]
fn template_literal_reduce_models_ts_numeric_bigint_lexing() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/template-lexing.ts",
        "export type NumTpl = `${1 | 2}`;\n\
         export type BoolTpl = `${true}`;\n\
         export type BigTpl = `id-${1n}`;\n",
    );

    // `${1 | 2}` ⇒ "1" | "2" (numeric literal union distributes).
    let (num, _) = resolve_expr(
        &host,
        "/fixtures/template-lexing.ts",
        "NumTpl",
        &[],
        ProjectionMode::Expanded,
    );
    assert_literal_union(&num, &["1", "2"]);

    // `${true}` ⇒ "true" (boolean literal stringifies, single literal folds).
    let (boolean, _) = resolve_expr(
        &host,
        "/fixtures/template-lexing.ts",
        "BoolTpl",
        &[],
        ProjectionMode::Expanded,
    );
    assert_string_literal(&boolean, "true");

    // `id-${1n}` ⇒ "id-1" (bigint literal → base-10 digits, no `n`).
    let (big, _) = resolve_expr(
        &host,
        "/fixtures/template-lexing.ts",
        "BigTpl",
        &[],
        ProjectionMode::Expanded,
    );
    assert_string_literal(&big, "id-1");
}

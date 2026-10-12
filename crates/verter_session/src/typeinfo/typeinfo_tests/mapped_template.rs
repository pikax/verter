//! @ai-generated - Synthetic mapped-type and template-literal key tests.

use super::oracle;
use super::support::*;
use std::collections::BTreeSet;
use verter_session_oracle_macro::oracle_row;

fn mapped_index_wire_answer(
    host: &crate::VerterHost,
    name: &str,
    arguments: &[Arc<TypeExpr>],
) -> TypeExpr {
    let (outcome, _) = host
        .resolve_named_symbol_wire_with_audit(
            "/indices.ts",
            name,
            arguments,
            Some(ProjectionMode::Expanded),
        )
        .into_parts();
    let node = outcome.expect("resolution succeeds").expect("type exists");
    let bytes = host
        .project_node_to_type_expr_json_bytes(node)
        .unwrap_or_else(|| panic!("{name} must survive JSON projection"));
    serde_json::from_slice(&bytes).expect("valid TypeExpr JSON")
}

#[test]
fn mapped_index_unique_symbol_survives_json_projection() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/indices.ts", "declare const tag: unique symbol; export type Control = typeof tag; export type Direct = { [K in typeof tag]: K }[typeof tag]; export type Filtered = { [K in typeof tag]: K }[typeof tag & symbol]; export type Deferred<S> = { [K in keyof S]: S[K] }[typeof tag];");
    let free = Arc::new(TypeExpr::TypeParameter(verter_type_expr::TypeParam {
        name: "Free".into(),
        constraint: None,
        default: None,
        is_const: false,
    }));
    for _ in 0..2 {
        let control = mapped_index_wire_answer(&host, "Control", &[]);
        assert!(matches!(control, TypeExpr::TypeOf(_)));
        for name in ["Direct", "Filtered"] {
            assert_eq!(
                mapped_index_wire_answer(&host, name, &[]),
                control,
                "{name}"
            );
        }
        let deferred = mapped_index_wire_answer(&host, "Deferred", &[Arc::clone(&free)]);
        let TypeExpr::IndexedAccess { index, .. } = &deferred else {
            panic!("open mapping must retain the nominal index: {deferred:?}");
        };
        assert_eq!(index.as_ref(), &control);
    }
}

#[test]
fn mapped_index_broad_domains_read_dictionary_values() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/indices.ts", "interface S { [key: string]: 1 } interface Named { [key: string]: 1; named: 1 } export type Dictionary = { [K in keyof S & string]: S[K] }[keyof S & string]; export type AllKeys = { [K in keyof S]: S[K] }[keyof S]; export type NamedDictionary = { [K in keyof Named & string]: Named[K] }[keyof Named & string]; export type Strings = { [K in string]: K }[string]; export type Numbers = { [K in number]: 1 }[number]; export type Symbols = { [K in symbol]: 1 }[symbol];");
    for _ in 0..2 {
        assert_primitive(
            &mapped_index_wire_answer(&host, "Strings", &[]),
            PrimitiveName::String,
        );
        for name in [
            "Dictionary",
            "AllKeys",
            "NamedDictionary",
            "Numbers",
            "Symbols",
        ] {
            assert_eq!(
                mapped_index_wire_answer(&host, name, &[]),
                TypeExpr::Literal(verter_type_expr::LiteralValue::Number(1.0)),
                "{name}"
            );
        }
    }
}

#[test]
fn mapped_index_empty_filtered_domains_are_never() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/indices.ts", "interface Numeric { 0: 1 } interface Empty {} export type NumericOnly = { [K in keyof Numeric & string]: Numeric[K] }[keyof Numeric & string]; export type NoKeys = { [K in keyof Empty & string]: Empty[K] }[keyof Empty & string]; export type Disjoint = { [K in string & number]: K }[string & number];");
    for _ in 0..2 {
        for name in ["NumericOnly", "NoKeys", "Disjoint"] {
            assert_primitive(
                &mapped_index_wire_answer(&host, name, &[]),
                PrimitiveName::Never,
            );
        }
    }
}

#[test]
fn mapped_index_deferred_access_retains_trailing_indices() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/indices.ts", "type Wrap<T> = { value: T }; export type Probe<S> = { [K in keyof S]: Wrap<S[K]> }[keyof S]['value'];");
    let free = Arc::new(TypeExpr::TypeParameter(verter_type_expr::TypeParam {
        name: "Free".into(),
        constraint: None,
        default: None,
        is_const: false,
    }));
    for _ in 0..2 {
        let answer = mapped_index_wire_answer(&host, "Probe", &[Arc::clone(&free)]);
        let TypeExpr::IndexedAccess { object, index } = &answer else {
            panic!("trailing access must remain: {answer:?}");
        };
        assert_string_literal(index, "value");
        let TypeExpr::IndexedAccess { object, index } = object.as_ref() else {
            panic!("mapped access must remain: {object:?}");
        };
        assert!(matches!(index.as_ref(), TypeExpr::KeyOf(_)));
        let TypeExpr::Mapped { value, .. } = object.as_ref() else {
            panic!("mapping must remain: {object:?}");
        };
        let TypeExpr::Ref {
            name,
            type_arguments,
        } = value.as_ref()
        else {
            panic!("alias application must remain: {value:?}");
        };
        assert_eq!(name.as_ref(), "Wrap");
        assert_eq!(type_arguments.len(), 1);
        assert!(matches!(&type_arguments[0], TypeExpr::IndexedAccess { .. }));
    }
}

#[test]
fn mapped_index_materializes_key_independent_template_unions() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/constant.ts", "type Label<T> = T extends number ? 'num' : never; export type Probe = { [K in 'a' | 'b']: 'direct' | `nested.${Label<1>}` }['a' | 'b'];");
    let (answer, _) = resolve_expr(
        &host,
        "/constant.ts",
        "Probe",
        &[],
        ProjectionMode::Expanded,
    );
    assert_literal_union(&answer, &["direct", "nested.num"]);
}

#[test]
fn generic_mapped_index_keeps_the_access_and_alias_arguments() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/generic.ts", "export type Wrap<T> = { value: T }; export type Deferred<S> = { [K in keyof S]: Wrap<S[K]> }[keyof S];");
    let parameter = Arc::new(TypeExpr::TypeParameter(verter_type_expr::TypeParam {
        name: "S".into(),
        constraint: None,
        default: None,
        is_const: false,
    }));
    let (answer, _) = resolve_expr(
        &host,
        "/generic.ts",
        "Deferred",
        &[parameter],
        ProjectionMode::Expanded,
    );
    let TypeExpr::IndexedAccess { object, index } = &answer else {
        panic!("an open mapped index must retain its access, got {answer:?}");
    };
    assert!(
        matches!(index.as_ref(), TypeExpr::KeyOf(_)),
        "the deferred index must retain keyof S: {index:?}"
    );
    let TypeExpr::Mapped { value, .. } = object.as_ref() else {
        panic!("the deferred object must retain its mapping: {object:?}");
    };
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = value.as_ref()
    else {
        panic!("the mapped value must retain its alias application: {value:?}");
    };
    assert_eq!(name.as_ref(), "Wrap");
    assert_eq!(type_arguments.len(), 1);
    assert!(
        matches!(&type_arguments[0], TypeExpr::IndexedAccess { .. }),
        "the alias argument must retain S[K]: {type_arguments:?}"
    );
}

#[test]
fn recursive_mapped_index_publishes_dotted_paths_across_modules() {
    for (count, groups) in [(20, 3), (100, 5), (300, 5)] {
        let mut files = vec![(
            "/paths/utils.ts".to_string(),
            "export interface Meta<T> { tag: T; created: number }\n\
             export type Paths<T, D extends unknown[] = []> = D['length'] extends 3 ? never\n\
             : T extends object ? { [K in keyof T & string]: K | `${K}.${Paths<T[K], [...D, 0]>}` }[keyof T & string] : never;".to_string(),
        )];
        for group in 0..groups {
            let mut source = "import type { Meta } from './utils';\n".to_string();
            for other in 0..groups {
                if other != group {
                    let imports = (other..count)
                        .step_by(groups)
                        .map(|i| format!("E{i}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    source.push_str(&format!(
                        "import type {{ {imports} }} from './entities-{other}';\n"
                    ));
                }
            }
            for i in (group..count).step_by(groups) {
                source.push_str(&format!("export interface E{i} {{ id: {i}; name: string; next: E{}; owner: E{}; meta: Meta<{i}> }}\n", (i + 1) % count, (i * 7 + 3) % count));
            }
            files.push((format!("/paths/entities-{group}.ts"), source));
        }
        let mut api = String::new();
        for group in 0..groups {
            let imports = (group..count)
                .step_by(groups)
                .map(|i| format!("E{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            api.push_str(&format!(
                "import type {{ {imports} }} from './entities-{group}';\n"
            ));
        }
        api.push_str("export interface Api {\n");
        for i in 0..count {
            api.push_str(&format!("e{i}: E{i};\n"));
        }
        api.push_str("}\n");
        files.push(("/paths/api.ts".to_string(), api));
        files.push(("/paths/scenario.ts".to_string(), "import type { Api } from './api'; import type { Paths } from './utils'; export type Probe = Paths<Api>;".to_string()));
        let borrowed = files
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect::<Vec<_>>();
        // Complete enumeration of the larger closures needs an explicit
        // request allowance; the smallest layout exercises the default fuse.
        let host = make_host_with_workspace_files_config(
            &borrowed,
            crate::HostConfig {
                audit_enabled: true,
                projection_op_budget: if count == 20 { 0 } else { 50_000 },
                ..crate::HostConfig::default()
            },
        );
        for (path, source) in &borrowed {
            upsert_ts(&host, path, source);
        }
        let (answer, record) = resolve_expr(
            &host,
            "/paths/scenario.ts",
            "Probe",
            &[],
            ProjectionMode::Expanded,
        );
        let mut expected = BTreeSet::new();
        for i in 0..count {
            expected.insert(format!("e{i}"));
            for field in ["id", "name", "next", "owner", "meta"] {
                expected.insert(format!("e{i}.{field}"));
            }
            for field in ["next", "owner"] {
                for child in ["id", "name", "next", "owner", "meta"] {
                    expected.insert(format!("e{i}.{field}.{child}"));
                }
            }
            for child in ["tag", "created"] {
                expected.insert(format!("e{i}.meta.{child}"));
            }
        }
        let TypeExpr::Union(arms) = &answer else {
            panic!("{count} entities/{groups} modules must publish a path union, got {answer:?}; audit: {:?}", record.kind_payload)
        };
        let actual = arms
            .iter()
            .map(|arm| {
                let TypeExpr::Literal(verter_type_expr::LiteralValue::String(path)) = arm else {
                    panic!("path must be a string literal, got {arm:?}")
                };
                path.clone()
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected, "{count} entities/{groups} modules");
    }
}

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
#[ignore = "reducer resolves this correctly (covered by the non-ignored `template_literal_key_reducer_projects_callable_slots` regression); NOT oracle-liftable — the declared source body `StaticTemplateSlots[TemplateLiteralCellName]` is a non-carve-out indexed-access (the index is a template-literal alias `Ref`, not a same-file string-literal chain) the oracle source-side positive allowlist rejects (measured Reject(DeferredConstruct(indexed-access))); the reduced function value is itself admissible. Lift pending an oracle source-walk carve-out for template-literal-keyed indexed access"]
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

// LIFTED: `RecordTemplateRootSlot = RecordTemplateSlots["slot:root"]` indexes a
// `Record<`slot:${"root"|"item"}`, …>` by the same-file string-literal key
// `slot:root` (the string-literal index-chain source-walk carve-out) and reduces
// to `(payload: { name: "item" | "root" }) => VNode[]`. The lifted body is the
// registry-keyed `oracle::run_row` shared-driver call comparing Verter's
// `Expanded` projection against the checked-in tsgo snapshot.
#[oracle_row]
#[test]
fn record_with_template_literal_key_union_projects_root_slot() {}

#[test]
#[ignore = "reducer resolves this correctly (covered by the non-ignored `template_literal_key_reducer_projects_callable_slots` regression); NOT oracle-liftable — the declared source body indexes `StaticTemplateSlots` by a template-literal union key, a non-carve-out indexed-access the oracle source-side positive allowlist rejects (measured Reject(DeferredConstruct(indexed-access))); the reduced union of function values is itself admissible. Lift pending an oracle source-walk carve-out for template-literal-keyed indexed access"]
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
/// `template_literal_union_key_projects_static_slot_union`). All three reduce to
/// admissible function values — none is blocked on a function surface. The
/// `record_*_root_slot` row is now oracle-LIFTED (`#[oracle_row]`): it admits both
/// sides and its `Expanded` projection converges against the checked-in tsgo
/// snapshot. The two `StaticTemplateSlots[…]` alias/union rows stay `#[ignore]`d,
/// blocked by their non-carve-out indexed-access source bodies. The SHARED
/// resolver reduces each one correctly — this test runs the same assertions in the
/// normal suite so the U2.MAPPED_TEMPLATE reducer paths (template-literal alias
/// key, Record template-union keyspace enumeration, union-index distribution) stay
/// guarded.
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

/// A template whose concatenations the connected-work ledger refuses must
/// CARRIER-STOP to the deferred `TemplateLiteral` shell, and the result must
/// NOT be warm-admitted (it is a non-cacheable budget-tainted partial). The
/// checker has no such limit below its own TS2590 product: the same product
/// under the production ledger enumerates.
#[test]
pub(crate) fn keyspace_budget_exceeded_admits_nothing() {
    use verter_type_engine::semantic_query::{QueryResult, SemanticNodeData};

    let host = make_host_with_footprint();

    // Two fully-CLOSED finite unions whose 1,600 concatenations exceed a
    // 256-unit work limit. Each union is a wide set of distinct string
    // literals.
    let union_a: Vec<String> = (0..40).map(|i| format!("a{i}")).collect();
    let union_b: Vec<String> = (0..40).map(|i| format!("b{i}")).collect();
    let (read, graph) = template_literal_reduce_read_with_work_limit(
        &host,
        &["", "-", ""],
        &[union_a.clone(), union_b.clone()],
        Some(256),
    );

    // (1) Carrier-stops: the value is the deferred TemplateLiteral SHELL, never
    //     an enumerated 1600-arm literal union.
    let value = match read.value {
        QueryResult::Value(id) => id,
        other => panic!("expected a value node, got {other:?}"),
    };
    let data = graph.node_data(value).expect("value node must exist");
    assert!(
        matches!(data.as_ref(), SemanticNodeData::TemplateLiteral { .. }),
        "a refused construction must carrier-stop to the TemplateLiteral shell, got {:?}",
        data.as_ref()
    );
    assert!(
        !matches!(data.as_ref(), SemanticNodeData::Union(_)),
        "a refused construction must NOT enumerate a literal union"
    );

    // (2) Not warm-admitted: the refused product is a non-cacheable,
    //     budget-tainted partial.
    assert!(
        read.cache_suppress,
        "a refused template product must be cache-suppressed (never warm-admitted)"
    );
    assert!(
        read.result_is_partial,
        "a refused template product must be flagged a budget-tainted partial"
    );

    // Control: the same 1,600-concatenation product under the production
    // ledger fully enumerates and is admissible.
    let (small_read, small_graph) =
        template_literal_reduce_read(&host, &["", "-", ""], &[union_a, union_b]);
    let small_value = match small_read.value {
        QueryResult::Value(id) => id,
        other => panic!("expected a value node, got {other:?}"),
    };
    let small_data = small_graph.node_data(small_value).expect("node must exist");
    assert!(
        matches!(small_data.as_ref(), SemanticNodeData::Union(_)),
        "the product must fully enumerate to a union, got {:?}",
        small_data.as_ref()
    );
    assert!(
        !small_read.cache_suppress,
        "the enumerated product is a complete result and must not be suppressed"
    );
    assert!(
        !small_read.result_is_partial,
        "the enumerated product is complete, not a partial"
    );
}

/// FIX 2 guard — the shared template reducer models TS numeric / boolean /
/// bigint literal interpolation. TS lexes `` `${1 | 2}` `` ⇒ `"1" | "2"`,
/// `` `${true}` `` ⇒ `"true"`, and a bigint literal as its base-10 digits.
/// Discriminates the pre-fix fail-closed code (which admitted ONLY string
/// literals and carrier-stopped these to the deferred shell).
#[test]
pub(crate) fn template_literal_reduce_models_ts_numeric_bigint_lexing() {
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

//! `unique symbol` property keys as key types: `keyof` over a surface with
//! a `unique symbol` key holds the symbol's nominal type beside the
//! property names, and a mapped type binds its key to that type.

use super::checker_probe_lane_tests::{default_probe_host, with_probe_on_host, PROBE_FILE};
use verter_type_engine::semantic_query::{SemanticNodeData, SemanticNodeId};

const MERGED_KEYS_BASE: &str =
    "interface Keys { plain: number }\ndeclare const keys: Keys;\ndeclare const other: Keys;\n";
const MERGED_KEYS_EXTENSION: &str =
    "interface Keys { readonly a: unique symbol; readonly b: unique symbol }\n";

#[test]
fn members_of_a_merged_global_annotation_keep_their_declaring_symbol() {
    use super::differential_harness_tests::{Matrix, ALL};
    let files = [
        ("base.d.ts", MERGED_KEYS_BASE),
        ("extension.d.ts", MERGED_KEYS_EXTENSION),
    ];
    let source = r#"
interface keys { local: true }
interface Keys { a: symbol; b: symbol }
declare const local: Keys;
export function disjoint() { return null! as (typeof keys.a extends typeof keys.b ? 1 : 2); }
export function shared() { return null! as (typeof keys.a extends typeof other.a ? 1 : 2); }
export function shadowed() { return null! as (typeof local.a extends typeof local.b ? 1 : 2); }
export function keyed() { return null! as ({ [keys.a]: 1; [keys.b]: 2 }[typeof keys.a]); }
"#;
    let failures = Matrix::new(source).settings(&ALL).files(&files).returns(&[
        ("disjoint", "2"),
        ("shared", "1"),
        ("shadowed", "1"),
        ("keyed", "1"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_merged_global_member_edit_rejects_its_warm_nominal_certification() {
    use super::carrier_head_resolution_tests::{host, upsert_ts};
    let host = host();
    upsert_ts(&host, "/base.d.ts", MERGED_KEYS_BASE);
    host.ensure_indexed_ready("/base.d.ts")
        .expect("base indexed");
    upsert_ts(
        &host,
        "/probe.ts",
        "type Result = typeof keys.a extends typeof keys.b ? 1 : 2; export {};\n",
    );
    for (source, expected) in [
        (MERGED_KEYS_EXTENSION, 2.0),
        (
            "interface Keys { readonly a: symbol; readonly b: symbol }\n",
            1.0,
        ),
        (MERGED_KEYS_EXTENSION, 2.0),
    ] {
        upsert_ts(&host, "/extension.d.ts", source);
        host.ensure_indexed_ready("/extension.d.ts")
            .expect("extension indexed");
        for request in ["cold", "warm"] {
            let result = host
                .resolve_named_symbol("/probe.ts", "Result", None)
                .expect("result resolves");
            let data = host
                .project_type_store()
                .semantic_graph()
                .node_data(result.id());
            assert!(
                matches!(data.as_deref(), Some(SemanticNodeData::Literal(verter_type_expr::LiteralValue::Number(value))) if *value == expected),
                "the {request} result follows the current member annotations: {data:?}"
            );
        }
    }
}

/// The key type `keyof` gives a `unique symbol` property key is a node of
/// the declaring file's (the nominal `typeof` carrier of the symbol's
/// declaration, one per declaration): the key union retains it as a child
/// (`for_each_retained_child`), and closing the declaring document
/// releases both with the rest of the file's nodes.
///
/// Oracle: over `declare const sym: unique symbol; interface HasSym {
/// [sym]: number; x: 1 }`, tsc 7.0.2 holds `keyof HasSym` mutually
/// assignable to `"x" | typeof sym` in all four settings; the probe reads
/// the keys through `Keys<T> = T extends string | symbol ? T : never`,
/// which distributes over them.
#[test]
fn a_unique_symbol_key_type_is_retained_by_its_key_union_and_released_on_close() {
    let host = default_probe_host();
    let source = "declare const sym: unique symbol;\ninterface HasSym { [sym]: number; x: 1 }\n\
                  type Keys<T> = T extends string | symbol ? T : never;\n";
    let (keys, key_type) = with_probe_on_host(
        &host,
        Default::default(),
        source,
        "Keys<keyof HasSym>",
        |dispatch, keys| {
            let graph = dispatch.graph_arc();
            let members: Vec<SemanticNodeId> = match graph.node_data(keys).as_deref() {
                Some(SemanticNodeData::Union(members)) => members.iter().copied().collect(),
                other => panic!("two keys make a union, got {other:?}"),
            };
            let key_type = members
                .iter()
                .copied()
                .find(|member| {
                    matches!(
                        graph.node_data(*member).as_deref(),
                        Some(SemanticNodeData::TypeOfNominal(_))
                    )
                })
                .expect("the symbol key is its nominal type");
            let mut retained = Vec::new();
            graph
                .node_data(keys)
                .expect("the key union")
                .for_each_retained_child(|child| retained.push(child));
            assert!(
                retained.contains(&key_type),
                "the key union retains the symbol's key type: {retained:?}"
            );
            (keys, key_type)
        },
    );
    let graph = std::sync::Arc::clone(host.project_type_store().semantic_graph());
    assert!(graph.node_is_live(keys) && graph.node_is_live(key_type));
    host.evict(PROBE_FILE);
    assert!(
        !graph.node_is_live(key_type) && !graph.node_is_live(keys),
        "closing {PROBE_FILE} releases the symbol's key type and the union holding it"
    );
}

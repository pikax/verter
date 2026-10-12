//! Complete merged-interface indexed access and its total demand work.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_type_engine::semantic_query::{LiteralValue, ProjectionMode, SemanticNodeData};

fn upsert_ts(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_owned()),
            input_id: canonical.to_owned(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert contributor");
}

fn probe(size: usize, global: bool) -> (usize, usize, usize) {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let host = VerterHost::new(HostConfig::default(), workspace.clone());
    let mut files: Vec<_> = (0..size)
        .map(|i| {
            if global {
                format!("reg-{i:04}.ts")
            } else {
                format!("plugin-{i:04}.ts")
            }
        })
        .collect();
    let mut random = 17usize;
    for i in (1..size).rev() {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        files.swap(i, random % (i + 1));
    }
    files.extend(["core.ts".to_owned(), "scenario.ts".to_owned()]);
    workspace.inject_file(
        "/tsconfig.json".into(),
        Arc::from(
            serde_json::json!({"compilerOptions": {"noLib": true}, "files": files}).to_string(),
        ),
    );
    let mut project =
        verter_workspace::ide_project_config("/".into(), "/".into(), Some("/tsconfig.json".into()));
    project.compiler_options =
        verter_workspace::load_compiler_options(workspace.as_ref(), "/tsconfig.json");
    project.membership.spec.files = files
        .iter()
        .map(|file| verter_workspace::CanonicalPath::new(&format!("/{file}")))
        .collect();
    host.configure_projects(vec![project]);
    if !global {
        upsert_ts(
            &host,
            "/core.ts",
            "export interface Props { base: -1 }\nexport {};\n",
        );
    }
    for i in 0..size {
        let (canonical, source) = if global {
            (
                format!("/reg-{i:04}.ts"),
                format!("interface BenchRegistry {{ r{i}: {{ id: {i}; tag: \"t{i}\" }} }}"),
            )
        } else {
            (
                format!("/plugin-{i:04}.ts"),
                format!(
                    "declare module './core' {{ interface Props {{ p{i}: {i} }} }}\nexport {{}};"
                ),
            )
        };
        upsert_ts(&host, &canonical, &source);
        host.ensure_indexed_ready(&canonical)
            .expect("contributor indexed");
    }
    let source = if global {
        "type Probe = BenchRegistry[keyof BenchRegistry]['tag'];"
    } else {
        "import type { Props } from './core'; type Probe = Props[keyof Props];"
    };
    upsert_ts(&host, "/scenario.ts", source);
    let graph = host.project_type_store().semantic_graph();
    let before_scans = verter_type_engine::semantic_query::key_scan_comparisons_for_tests();
    let node = host
        .resolve_named_symbol("/scenario.ts", "Probe", Some(ProjectionMode::Expanded))
        .expect("probe answers");
    let scans = verter_type_engine::semantic_query::key_scan_comparisons_for_tests() - before_scans;
    let mut pending = vec![node];
    let mut actual = BTreeSet::new();
    while let Some(node) = pending.pop() {
        match graph.node_data(node).as_deref().expect("live result") {
            SemanticNodeData::Union(members) => pending.extend(members.iter().copied()),
            SemanticNodeData::Alias(inner) => pending.push(*inner),
            SemanticNodeData::Literal(LiteralValue::String(value)) if global => {
                actual.insert(value.to_string());
            }
            SemanticNodeData::Literal(LiteralValue::Number(value)) if !global => {
                actual.insert(value.to_string());
            }
            other => panic!(
                "size={size}, global={global}: incomplete answer {other:?}; stats={:?}",
                graph.stats_snapshot()
            ),
        }
    }
    let mut expected: BTreeSet<String> = (0..size)
        .map(|i| {
            if global {
                format!("t{i}")
            } else {
                i.to_string()
            }
        })
        .collect();
    if !global {
        expected.insert("-1".to_owned());
    }
    assert_eq!(
        actual, expected,
        "size={size}, global={global}: every contributor must answer"
    );
    let ownership = graph.node_ownership_counts();
    let collection_entries = host
        .project_type_store()
        .indexed()
        .global_contributor_index()
        .snapshot()
        .materialized_view_entry_count() as usize;
    let ordering = host
        .project_type_store()
        .indexed()
        .global_contributor_index()
        .snapshot()
        .ordering_work_count() as usize;
    let cold_queries = graph.stats_snapshot().misses as usize;
    eprintln!(
        "size={size}, global={global}: scans={scans}, ordering={ordering}, nodes={}, edges={}, collection={collection_entries}, queries={cold_queries}",
        ownership.live_nodes,
        ownership.internal_edges,
    );
    (
        scans + ownership.internal_edges + collection_entries + ordering + cold_queries,
        scans,
        ordering,
    )
}

#[test]
fn merged_interface_indexed_access_work_is_linear() {
    for global in [false, true] {
        let mut work = Vec::new();
        let mut ordering_work = Vec::new();
        for size in [128, 512, 2048] {
            let (total_work, scans, ordering) = probe(size, global);
            ordering_work.push(ordering);
            work.push((size, total_work));
            assert!(
                scans <= 40 * size,
                "surface scans must be linear: size={size}, scans={scans}"
            );
        }
        for pair in ordering_work.windows(2) {
            assert!(
                pair[1] <= 4 * pair[0] + 256,
                "ordering work must grow linearly: {ordering_work:?}"
            );
        }
        for pair in work.windows(2) {
            assert!(
                pair[1].1 <= 4 * pair[0].1 + 256,
                "collection plus probe work must grow linearly: {work:?}"
            );
        }
    }
}

#[test]
fn global_interface_bases_share_the_merged_instantiation() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for (canonical, source) in [
        ("/a.ts", "interface Shared { a: 1 }"),
        ("/b.ts", "interface Shared { b: 2 }"),
        (
            "/c.ts",
            "export {}; declare global { interface Shared { c: 3 } }",
        ),
    ] {
        upsert_ts(&host, canonical, source);
        host.ensure_indexed_ready(canonical).expect("indexed");
    }
    let graph = host.project_type_store().semantic_graph();
    let first = host
        .resolve_named_symbol("/a.ts", "Shared", Some(ProjectionMode::Navigate))
        .expect("first base");
    let builds = graph.stats_snapshot().instantiate_count;
    for canonical in ["/b.ts", "/c.ts", "/a.ts"] {
        assert_eq!(
            host.resolve_named_symbol(canonical, "Shared", Some(ProjectionMode::Navigate)),
            Some(first)
        );
    }
    assert_eq!(
        graph.stats_snapshot().instantiate_count,
        builds,
        "other bases reuse the one merged build"
    );
    upsert_ts(&host, "/b.ts", "interface Shared { b: 4 }");
    let updated = host
        .resolve_named_symbol("/c.ts", "Shared", Some(ProjectionMode::Navigate))
        .expect("edited contributor");
    assert_ne!(
        first, updated,
        "a contributor edit rejects the shared answer"
    );
    let raised = host
        .project_node_to_type_expr_for_test(updated)
        .expect("merged surface");
    let verter_type_expr::TypeExpr::Object(shape) = &raised else {
        panic!("object expected")
    };
    assert_eq!(
        shape.properties.len(),
        3,
        "all bases remain present after an edit"
    );
}

#[test]
fn a_cold_batch_consumer_of_a_warm_subject_observes_contributor_edits_and_additions() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(&host, "/core.ts", "export interface Props { base: -1 }");
    upsert_ts(
        &host,
        "/plugin.ts",
        "export {}; declare module './core' { interface Props { p: 0 } }",
    );
    host.ensure_indexed_ready("/plugin.ts")
        .expect("indexed augmenter");
    upsert_ts(&host, "/read.ts", "import type { Props } from './core'; type First = Props[keyof Props]; type Second = Props[keyof Props]; type Third = Props[keyof Props];");
    let numbers = |name| {
        let graph = host.project_type_store().semantic_graph();
        let node = host
            .resolve_named_symbol("/read.ts", name, Some(ProjectionMode::Expanded))
            .expect("batch");
        let mut pending = vec![node];
        let mut values = BTreeSet::new();
        while let Some(node) = pending.pop() {
            match graph.node_data(node).as_deref().expect("live value") {
                SemanticNodeData::Union(arms) => pending.extend(arms.iter().copied()),
                SemanticNodeData::Alias(inner) => pending.push(*inner),
                SemanticNodeData::Literal(LiteralValue::Number(number)) => {
                    values.insert(number.to_string());
                }
                other => panic!("complete number union expected: {other:?}"),
            }
        }
        values
    };
    assert_eq!(
        numbers("First"),
        BTreeSet::from(["-1".to_owned(), "0".to_owned()])
    );
    upsert_ts(
        &host,
        "/plugin.ts",
        "export {}; declare module './core' { interface Props { p: 10 } }",
    );
    host.ensure_indexed_ready("/plugin.ts")
        .expect("edited augmenter");
    assert_eq!(
        numbers("Second"),
        BTreeSet::from(["-1".to_owned(), "10".to_owned()])
    );
    upsert_ts(
        &host,
        "/later.ts",
        "export {}; declare module './core' { interface Props { later: 20 } }",
    );
    host.ensure_indexed_ready("/later.ts")
        .expect("new unimported augmenter");
    let expected = BTreeSet::from(["-1".to_owned(), "10".to_owned(), "20".to_owned()]);
    assert_eq!(numbers("Third"), expected);
    assert_eq!(
        numbers("First"),
        expected,
        "the original warm batch also invalidates"
    );
}

#[test]
fn finite_batch_reads_match_single_keys_across_projection_modes() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(&host, "/read.ts", "
        interface Named { x: 1 }
        interface Values { optional?: 1; get access(): 2; method(x: string): 3; method(x: number): 4; named: Named; 5: 6 }
        type Batch = Values[keyof Values];
        type Singles = Values['optional'] | Values['access'] | Values['method'] | Values['named'] | Values['5'];
    ");
    use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
    use verter_type_engine::semantic_query::{IndexKey, QueryResult, SemanticQueryKey};
    let base = host
        .resolve_named_symbol("/read.ts", "Values", Some(ProjectionMode::Navigate))
        .expect("object subject");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    // Supply the accessor kind directly at the surface boundary: this
    // control isolates the consumer from authored signature-kind lowering.
    let data = graph.node_data(base).expect("surface");
    let SemanticNodeData::Object(surface) = data.as_ref() else {
        panic!("object subject")
    };
    let mut members = surface.positive_members().to_vec();
    let getter = members
        .iter_mut()
        .find(|member| {
            member.key.cloned_known()
                == Some(verter_type_engine::semantic_query::PropertyKey::identifier(
                    "access",
                ))
        })
        .expect("getter member");
    getter.method_kind = Some(verter_type_expr::ObjectMethodKind::Get);
    let base = graph.intern_node_with_scope(
        SemanticNodeData::Object(
            verter_type_engine::semantic_query::SurfaceView::from_members(members, None),
        ),
        graph.node_scope(base).expect("subject scope"),
    );
    let key_names = ["optional", "access", "method", "named", "5"];
    let keys: Vec<_> = key_names
        .iter()
        .map(|key| {
            graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                (*key).to_owned(),
            )))
        })
        .collect();
    let read = |key| {
        let read = dispatch.execute_read(key);
        assert!(
            !read.result_is_partial,
            "complete member read: {:?}",
            read.partial_reasons
        );
        match read.value {
            QueryResult::Value(node) => node,
            other => panic!("value expected: {other:?}"),
        }
    };
    let index = read(SemanticQueryKey::ReduceUnion {
        members: keys.into(),
        nullability: verter_session_query::flow::policy::NullabilityPolicy::Strict,
    });
    for mode in [
        ProjectionMode::Navigate,
        ProjectionMode::Shallow,
        ProjectionMode::Expanded,
    ] {
        let batch = read(SemanticQueryKey::IndexedAccess {
            base,
            index: IndexKey::Computed(index),
            mode,
        });
        let getter_is_value = match graph.node_data(batch).as_deref() {
            Some(SemanticNodeData::Union(arms)) => arms.iter().any(|arm| {
                matches!(
                graph.node_data(*arm).as_deref(),
                Some(SemanticNodeData::Literal(LiteralValue::Number(value))) if *value == 2.0)
            }),
            _ => false,
        };
        assert!(
            getter_is_value,
            "the getter reads its return value in {mode:?}"
        );
        let singles: Vec<_> = key_names
            .iter()
            .map(|key| {
                read(SemanticQueryKey::IndexedAccess {
                    base,
                    index: IndexKey::String(Arc::from(*key)),
                    mode,
                })
            })
            .collect();
        let singles = read(SemanticQueryKey::ReduceUnion {
            members: singles.into(),
            nullability: verter_session_query::flow::policy::NullabilityPolicy::Strict,
        });
        assert_eq!(host.project_node_to_type_expr_for_test(batch), host.project_node_to_type_expr_for_test(singles),
            "optional reads, accessors, overloads, named terminals and numeric spelling agree in {mode:?}");
    }
}

#[test]
fn merged_interface_indexed_access_keeps_every_contributor() {
    for size in [100, 500, 2000, 4000] {
        for global in [false, true] {
            probe(size, global);
        }
    }
}

#[test]
fn global_interface_default_arguments_survive_shared_instantiation() {
    for (first, second) in [
        ("<T = string>", "<T = string>"),
        ("<T>", "<T = string>"),
        ("<T = string, U = T[]>", "<T = string, U = T[]>"),
    ] {
        let host = VerterHost::new_standalone(HostConfig::default());
        upsert_ts(
            &host,
            "/a.ts",
            &format!("interface Shared{first} {{ first: T }}"),
        );
        let member = if second.contains("U =") { "U" } else { "T" };
        upsert_ts(&host, "/b.ts", &format!("interface Shared{second} {{ second: {member} }} type Probe = Shared['second']; type Explicit = Shared<number>['second'];"));
        for (name, expected) in [("Probe", "string"), ("Explicit", "number")] {
            let node = host
                .resolve_named_symbol("/b.ts", name, Some(ProjectionMode::Expanded))
                .expect("projection");
            let ty = host
                .project_node_to_type_expr_for_test(node)
                .expect("raised member");
            let expected = verter_type_expr::TypeExpr::Primitive(if expected == "string" {
                verter_type_expr::PrimitiveName::String
            } else {
                verter_type_expr::PrimitiveName::Number
            });
            let expected = if member == "U" {
                verter_type_expr::TypeExpr::Array {
                    element: Arc::new(expected),
                    readonly: false,
                }
            } else {
                expected
            };
            assert_eq!(ty, expected, "{first} / {second}: {name}");
        }
    }
}

//! Namespace objects use terminal export bindings, with local shadowing and
//! ambiguity determined before value evaluation.

use super::checker_probe_lane_tests::{mismatches_in, ProbeProject};

use super::checker_probe_lane_tests::{probe_host, with_probe_on_host};
use super::ProjectSemanticDispatch;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use verter_type_engine::semantic_query::{
    IndexKey, LiteralValue, ProjectionMode, ProjectionReductionContext, QueryResult, ScopeId,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey, ValueRootKey,
};

fn barrel_files(count: usize) -> Vec<(String, String)> {
    let mut files = Vec::new();
    for i in 0..count {
        files.push((format!("leaf-{i}.ts"), format!(
            "export const v{i} = {i} as const; export interface T{i} {{ id: {i} }} export type K{i} = 'k{i}';"
        )));
    }
    for group in 0..count / 20 {
        files.push((
            format!("group-{group}.ts"),
            (group * 20..(group + 1) * 20)
                .map(|i| format!("export * from './leaf-{i}';\n"))
                .collect(),
        ));
    }
    let mut index: String = (0..count / 20)
        .map(|i| format!("export * from './group-{i}';\n"))
        .collect();
    index.push_str("export const v0 = 'shadow' as const;\n");
    files.push(("index.ts".to_owned(), index));
    files
}

fn assert_literals(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    node: SemanticNodeId,
    count: usize,
) {
    let data = dispatch.graph().node_data(node).expect("a union node");
    let SemanticNodeData::Union(arms) = data.as_ref() else {
        panic!("expected {count} literal arms, got {data:?}");
    };
    assert_eq!(arms.len(), count);
    let mut numbers = std::collections::BTreeSet::new();
    let mut shadow = false;
    for arm in arms.iter() {
        match dispatch.graph().node_data(*arm).as_deref() {
            Some(SemanticNodeData::Literal(LiteralValue::Number(value))) => {
                assert_eq!(*value, value.trunc());
                assert!(numbers.insert(*value as usize));
            }
            Some(SemanticNodeData::Literal(LiteralValue::String(value))) if value == "shadow" => {
                assert!(!shadow);
                shadow = true;
            }
            other => panic!("every member stays literal, got {other:?}"),
        }
    }
    assert!(shadow);
    assert_eq!(numbers, (1..count).collect());
}

#[test]
fn namespace_barrel_probes_keep_every_literal_at_scale() {
    for count in [100, 500] {
        let owned = barrel_files(count);
        let files: Vec<_> = owned
            .iter()
            .map(|(name, source)| (name.as_str(), source.as_str()))
            .collect();
        let project = ProbeProject {
            files: &files,
            ..ProbeProject::default()
        };
        let host = probe_host(project);
        with_probe_on_host(
            &host,
            project,
            "import * as ns from './index'; type __Probe = (typeof ns)[keyof typeof ns];",
            "(typeof ns)[keyof typeof ns]",
            |dispatch, node| assert_literals(dispatch, node, count),
        );
    }
}

#[test]
fn export_tables_and_namespace_evaluation_have_linear_work() {
    for count in [100, 500, 2_000] {
        let owned = barrel_files(count);
        let files: Vec<_> = owned
            .iter()
            .map(|(name, source)| (name.as_str(), source.as_str()))
            .collect();
        let host = probe_host(ProbeProject {
            files: &files,
            ..ProbeProject::default()
        });
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            "/wb/scenario.ts",
            "import * as ns from './index'; type __Probe = (typeof ns)[keyof typeof ns];",
            crate::FileLanguage::script_ts(),
        );
        let view = host.resolver_store_view_read().into_owned_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);
        let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
        let root = ValueRootKey {
            scope: ScopeId::file(
                Arc::from("/wb/scenario.ts"),
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
            ),
            name: Arc::from("ns"),
        };
        let (_guard, _) = dispatch.enter_connected_demand(false);
        let read = |key| {
            let result = dispatch.execute_read(key);
            assert!(!result.result_is_partial, "complete namespace demand");
            match result.value {
                QueryResult::Value(node) => node,
                other => panic!("namespace demand: {other:?}"),
            }
        };
        let object = read(dispatch.typeof_key_for(root, context));
        let keys = read(SemanticQueryKey::KeyOf {
            base: object,
            context,
        });
        let union = read(SemanticQueryKey::IndexedAccess {
            base: object,
            index: IndexKey::Computed(keys),
            mode: ProjectionMode::Expanded,
        });
        assert_literals(&dispatch, union, count);
        let alias = read(SemanticQueryKey::ResolveDecl(super::resolve_decl_key(
            "/wb/scenario.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "__Probe",
        )));
        let alias = dispatch
            .normalize_node_keeping_declaration_refs_for_tests(alias, context)
            .into_usable_node()
            .expect("complete authored probe alias");
        assert_literals(&dispatch, alias, count);
        let table_work = host
            .resolver
            .runtime
            .routes
            .module_export_work
            .load(Ordering::Relaxed);
        let query_work = dispatch.connected_demand().work_used_for_tests();
        println!(
            "leaves={count} table_work={table_work} query_work={query_work} nodes={}",
            dispatch.graph().node_count()
        );
        assert!(
            table_work >= count as u64 && table_work <= 16 * count as u64,
            "table work {table_work} for {count} leaves"
        );
        assert!(
            query_work >= count && query_work <= 64 * count,
            "probe work {query_work} for {count} leaves"
        );
        // A new request shares every table: there is no second table build.
        let warm = ProjectSemanticDispatch::new(&host_ctx);
        let table = warm
            .ctx
            .module_export_table("/wb/index.ts")
            .expect("warm table");
        assert_eq!(table.bindings.len(), count);
        assert_eq!(
            host.resolver
                .runtime
                .routes
                .module_export_work
                .load(Ordering::Relaxed),
            table_work
        );
    }
}

#[test]
fn star_exports_exclude_ambiguous_names_and_default() {
    let files = [
        (
            "a.ts",
            "export const x = 1 as const; export const a = 'a' as const; export default 3;",
        ),
        (
            "b.ts",
            "export const x = 2 as const; export const b = 'b' as const;",
        ),
        ("index.ts", "export * from './a'; export * from './b';"),
        (
            "shadow.ts",
            "export * from './a'; export * from './b'; export const x = 'shadow' as const;",
        ),
        (
            "diamond.ts",
            "export * from './a'; export * from './alias';",
        ),
        ("alias.ts", "export * from './a';"),
        (
            "namespace-a.ts",
            "import * as first from './a'; export { first as nested };",
        ),
        (
            "namespace-b.ts",
            "import * as second from './a'; export { second as nested };",
        ),
        (
            "namespace-diamond.ts",
            "export * from './namespace-a'; export * from './namespace-b';",
        ),
    ];
    let failures = mismatches_in(
        ProbeProject { files: &files, ..ProbeProject::default() },
        "import * as ns from './index'; import * as shadow from './shadow'; import * as diamond from './diamond'; import * as namespaceDiamond from './namespace-diamond';",
        &[
            ("keyof typeof ns", "\"a\" | \"b\""),
            ("(typeof ns)[keyof typeof ns]", "\"a\" | \"b\""),
            ("(typeof shadow)['x']", "\"shadow\""),
            ("(typeof diamond)['x']", "1"),
            ("keyof typeof namespaceDiamond", "\"nested\""),
            ("(typeof namespaceDiamond)['nested']['x']", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn export_tables_resolve_aliases_cycles_and_namespace_reexports() {
    let files = [
        ("a.ts", "export * from './b'; export const a = 1 as const; export interface TypeOnly {}"),
        ("b.ts", "export * from './a'; export const b = 2 as const; export default 3;"),
        ("index.ts", "import { hidden } from './types'; export type { hidden }; import type * as Ghost from './types'; export { Ghost }; export * from './a'; export { b as renamed } from './b'; export * as nested from './b'; export type * from './types';"),
        ("types.ts", "export const hidden = 4;"),
    ];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            ..ProbeProject::default()
        },
        "import * as ns from './index';",
        &[
            (
                "keyof typeof ns",
                "\"a\" | \"b\" | \"nested\" | \"renamed\"",
            ),
            ("(typeof ns)['renamed']", "2"),
            ("(typeof ns)['nested']['default']", "3"),
            ("keyof (typeof ns)['nested']", "\"a\" | \"b\" | \"default\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn export_tables_invalidate_on_leaf_edits_retargets_and_new_modules() {
    let files = [
        ("a.ts", "export const a = 1 as const;"),
        ("b.ts", "export const b = 2 as const;"),
        (
            "index.ts",
            "export * from './a'; export * from './pending';",
        ),
    ];
    let project = ProbeProject {
        files: &files,
        ..ProbeProject::default()
    };
    let host = probe_host(project);
    let check = |expected: Vec<LiteralValue>| {
        with_probe_on_host(
            &host,
            project,
            "import * as ns from './index';",
            "(typeof ns)[keyof typeof ns]",
            |dispatch, node| {
                let data = dispatch.graph().node_data(node).expect("value node");
                let nodes = match data.as_ref() {
                    SemanticNodeData::Union(arms) => arms.to_vec(),
                    SemanticNodeData::Literal(_) => vec![node],
                    other => panic!("literal exports, got {other:?}"),
                };
                let actual: Vec<_> = nodes
                    .iter()
                    .map(|id| match dispatch.graph().node_data(*id).as_deref() {
                        Some(SemanticNodeData::Literal(value)) => value.clone(),
                        other => panic!("literal, got {other:?}"),
                    })
                    .collect();
                assert_eq!(actual.len(), expected.len());
                assert!(
                    expected.iter().all(|value| actual.contains(value)),
                    "got {actual:?}, wanted {expected:?}"
                );
            },
        );
    };
    check(vec![LiteralValue::Number(1.0)]);
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        "/wb/a.ts",
        "export const a = 7 as const;",
        crate::FileLanguage::script_ts(),
    );
    check(vec![LiteralValue::Number(7.0)]);
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        "/wb/index.ts",
        "export * from './b'; export * from './pending';",
        crate::FileLanguage::script_ts(),
    );
    check(vec![LiteralValue::Number(2.0)]);
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        "/wb/pending.ts",
        "export const ready = 'ready' as const;",
        crate::FileLanguage::script_ts(),
    );
    check(vec![
        LiteralValue::Number(2.0),
        LiteralValue::String("ready".to_owned()),
    ]);
}

#[test]
fn a_namespace_reexport_can_refer_to_its_own_module() {
    let files = [(
        "self.ts",
        "export * as self from './self'; export const x = 1 as const;",
    )];
    let failures = mismatches_in(
        ProbeProject {
            files: &files,
            ..ProbeProject::default()
        },
        "import * as ns from './self';",
        &[
            ("keyof (typeof ns)['self']['self']", "\"self\" | \"x\""),
            ("(typeof ns)['self']['x']", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_growing_namespace_shares_unchanged_tables_and_retains_linear_nodes() {
    let host = probe_host(ProbeProject::default());
    let mut first_group = None;
    for count in [500, 2_000] {
        for (name, source) in barrel_files(count) {
            crate::u6_flow_shape_corpus_tests::upsert(
                &host,
                &format!("/wb/{name}"),
                &source,
                crate::FileLanguage::script_ts(),
            );
        }
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            "/wb/scenario.ts",
            "import * as ns from './index'; type __Probe = (typeof ns)[keyof typeof ns];",
            crate::FileLanguage::script_ts(),
        );
        let view = host.resolver_store_view_read().into_owned_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);
        let started = std::time::Instant::now();
        let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
        let table = dispatch
            .ctx
            .module_export_table("/wb/index.ts")
            .expect("current export table");
        assert_eq!(table.bindings.len(), count, "export table freshness");
        let object = match dispatch
            .execute_read(dispatch.typeof_key_for(
                ValueRootKey {
                    scope: ScopeId::file(
                        Arc::from("/wb/scenario.ts"),
                        verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    ),
                    name: Arc::from("ns"),
                },
                context,
            ))
            .value
        {
            QueryResult::Value(node) => node,
            other => panic!("namespace object: {other:?}"),
        };
        match dispatch.graph().node_data(object).as_deref() {
            Some(SemanticNodeData::Object(surface)) => assert_eq!(
                surface.positive_members().len(),
                count,
                "namespace object freshness"
            ),
            other => panic!("namespace object: {other:?}"),
        }
        let alias = match dispatch
            .execute_read(SemanticQueryKey::ResolveDecl(super::resolve_decl_key(
                "/wb/scenario.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "__Probe",
            )))
            .value
        {
            QueryResult::Value(node) => node,
            other => panic!("authored probe: {other:?}"),
        };
        let node = dispatch
            .normalize_node_keeping_declaration_refs_for_tests(alias, context)
            .into_usable_node()
            .expect("complete authored probe");
        assert_literals(&dispatch, node, count);
        let group = dispatch
            .ctx
            .module_export_table("/wb/group-0.ts")
            .expect("shared group");
        if let Some(first) = &first_group {
            assert!(Arc::ptr_eq(first, &group), "unchanged groups remain shared");
        } else {
            first_group = Some(group);
        }
        let nodes = dispatch.graph().node_count();
        assert!(
            nodes <= 12 * count,
            "retained {nodes} nodes for {count} leaves"
        );
        println!(
            "session_leaves={count} elapsed={:?} rss_bytes={} nodes={nodes}",
            started.elapsed(),
            verter_audit::memory::current_process_rss()
        );
    }
}

#[test]
fn overlapping_cold_namespaces_build_shared_modules_once() {
    use verter_type_engine::resolver_core::request_ports::RouteLookup;

    let mut owned = barrel_files(500);
    owned.push(("other.ts".into(), "export * from './index';".into()));
    let files: Vec<_> = owned
        .iter()
        .map(|(name, source)| (name.as_str(), source.as_str()))
        .collect();
    let host = probe_host(ProbeProject {
        files: &files,
        ..ProbeProject::default()
    });
    for root in ["/wb/index.ts", "/wb/other.ts"] {
        assert_eq!(
            host.module_export_table(root)
                .expect("serial table")
                .bindings
                .len(),
            500
        );
    }
    let routes = &host.resolver.runtime.routes;
    let serial = routes.module_export_work.load(Ordering::Relaxed);
    routes.clear();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let handles: Vec<_> = ["/wb/index.ts", "/wb/other.ts"]
            .into_iter()
            .map(|root| {
                let host = &host;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    host.module_export_table(root)
                        .expect("concurrent table")
                        .bindings
                        .len()
                })
            })
            .collect();
        for handle in handles {
            assert_eq!(handle.join().expect("table worker"), 500);
        }
    });
    let concurrent = routes.module_export_work.load(Ordering::Relaxed);
    assert_eq!(
        concurrent, serial,
        "overlapping demands share every module build"
    );
}

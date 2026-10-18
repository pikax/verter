//! Complete merged-interface indexed access and its total demand work.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_type_engine::semantic_query::{LiteralValue, ProjectionMode, SemanticNodeData};

const ROOT_FACT_VISITS_PER_CONTRIBUTOR: usize = 40;

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

/// A host holding `size` contributors of one merged interface — global
/// script declarations, or module augmentations of `./core` — in a
/// shuffled tsconfig `files` order, and the scenario reading it.
fn contributor_host(size: usize, global: bool) -> VerterHost {
    contributor_host_with(size, size, global)
}

/// The text of contributor `i`: its file name and the source declaring it.
fn contributor(i: usize, global: bool) -> (String, String) {
    if global {
        (
            format!("/reg-{i:04}.ts"),
            format!("interface BenchRegistry {{ r{i}: {{ id: {i}; tag: \"t{i}\" }} }}"),
        )
    } else {
        (
            format!("/plugin-{i:04}.ts"),
            format!("declare module './core' {{ interface Props {{ p{i}: {i} }} }}\nexport {{}};"),
        )
    }
}

/// A host whose project lists `size` contributor files, of which the first
/// `active` contribute; the others hold a declaration-free file of the same
/// kind (a global script, or a module) until an edit fills them in.
fn contributor_host_with(size: usize, active: usize, global: bool) -> VerterHost {
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
        let (canonical, mut source) = contributor(i, global);
        if i >= active {
            source = if global {
                "// an empty contributor slot\n".to_owned()
            } else {
                "export {};\n".to_owned()
            };
        }
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
    host
}

fn probe(size: usize, global: bool) -> (usize, usize, usize, usize) {
    let host = contributor_host(size, global);
    let graph = host.project_type_store().semantic_graph();
    let before_scans = verter_type_engine::semantic_query::key_scan_comparisons_for_tests();
    let before_roots = verter_type_engine::fact_signature_helpers::root_fact_visits_for_tests();
    let node = host
        .resolve_named_symbol("/scenario.ts", "Probe", Some(ProjectionMode::Expanded))
        .expect("probe answers");
    let scans = verter_type_engine::semantic_query::key_scan_comparisons_for_tests() - before_scans;
    let root_visits =
        verter_type_engine::fact_signature_helpers::root_fact_visits_for_tests() - before_roots;
    assert!(
        root_visits <= ROOT_FACT_VISITS_PER_CONTRIBUTOR * size,
        "root-fact sealing must be linear: size={size}, global={global}, visits={root_visits}"
    );
    let mut pending = vec![node.id()];
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
        "size={size}, global={global}: scans={scans}, roots={root_visits}, ordering={ordering}, nodes={}, edges={}, collection={collection_entries}, queries={cold_queries}",
        ownership.live_nodes,
        ownership.internal_edges,
    );
    (
        scans
            + root_visits
            + ownership.internal_edges
            + collection_entries
            + ordering
            + cold_queries,
        scans,
        ordering,
        root_visits,
    )
}

#[test]
fn merged_interface_indexed_access_work_is_linear() {
    for global in [false, true] {
        let mut work = Vec::new();
        let mut completed_work = Vec::new();
        let mut ordering_work = Vec::new();
        for size in [128, 512, 2048] {
            let (total_work, scans, ordering, root_visits) = probe(size, global);
            ordering_work.push(ordering);
            work.push((size, total_work - root_visits));
            completed_work.push((size, total_work));
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
        // Strict-world compaction starts only above an evidence page's width.
        // Preserve the structural-work ratios, and bound the completed probe
        // by one linear envelope that includes the compaction allowance at
        // every size, including sizes where compaction does no work.
        let (first_size, first_work) = work[0];
        for (size, total_work) in &completed_work {
            let allowance =
                first_work * size / first_size + ROOT_FACT_VISITS_PER_CONTRIBUTOR * size + 256;
            assert!(
                *total_work <= allowance,
                "collection plus completed probe work must be linear: {completed_work:?}"
            );
        }
    }
}

/// A cold read of a merged interface lowers each contributing statement
/// once — its facts and its body come from one lowering — and hands the
/// contributors' lowering to the decl-lowering workers in batches: the
/// rendezvous count stays flat as the contributor count grows. A warm
/// repeat lowers nothing and hands nothing off.
#[test]
fn contributor_bodies_lower_once_in_batches_per_shard() {
    for global in [false, true] {
        let mut handoffs = Vec::new();
        for size in [64, 256] {
            let host = contributor_host(size, global);
            let read = |host: &VerterHost| {
                let statements = host.provenance().snapshot().decl_statements_lowered;
                let rendezvous = host.decl_lowering.handoff_count();
                host.resolve_named_symbol("/scenario.ts", "Probe", Some(ProjectionMode::Expanded))
                    .expect("probe answers");
                (
                    host.provenance().snapshot().decl_statements_lowered - statements,
                    host.decl_lowering.handoff_count() - rendezvous,
                )
            };
            let (statements, rendezvous) = read(&host);
            assert!(
                statements as usize <= size + 8,
                "size={size}, global={global}: every contributing statement lowers once, \
                 lowered {statements}"
            );
            handoffs.push(rendezvous);
            assert_eq!(
                read(&host),
                (0, 0),
                "size={size}, global={global}: a warm repeat lowers nothing"
            );
        }
        assert!(
            handoffs[1] <= handoffs[0] + 16,
            "global={global}: worker rendezvous must not grow with the contributors: {handoffs:?}"
        );
    }
}

/// A merged declaration read through `M[keyof M]` is instantiated under
/// more than one projection context in one request (the index evaluates
/// `keyof M` published, the access reads `M` in structural transit). Its
/// contributors are folded once per request: a later context projects the
/// planned bodies without re-reading any contributor's shape.
#[test]
fn a_merged_declaration_folds_its_contributors_once_per_request() {
    for global in [false, true] {
        let size = 256;
        let host = contributor_host(size, global);
        let graph = host.project_type_store().semantic_graph();
        let before = graph.stats_snapshot();
        host.resolve_named_symbol("/scenario.ts", "Probe", Some(ProjectionMode::Expanded))
            .expect("probe answers");
        let after = graph.stats_snapshot();
        // Re-folding under a second context would re-read every
        // contributor's shape as a warm hit: `size` hits per context.
        let shape_rereads = after.hits - before.hits;
        assert!(
            shape_rereads < size as u64 / 4,
            "global={global}: a later projection context must not re-read the \
             contributors, read {shape_rereads} warm results for {size} contributors"
        );
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
            Some(first.clone())
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
        .project_node_to_type_expr_for_test(updated.id())
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
        let mut pending = vec![node.id()];
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
        interface Values { optional?: 1; get access(): 2; method(x: string): 3; method(x: number): 4; named: Named; namedAgain: Named; 5: 6 }
        type Batch = Values[keyof Values];
        type Singles = Values['optional'] | Values['access'] | Values['method'] | Values['named'] | Values['namedAgain'] | Values['5'];
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
    let data = graph.node_data(base.id()).expect("surface");
    let SemanticNodeData::Object(surface) = data.as_ref() else {
        panic!("object subject")
    };
    let mut members = surface
        .positive_members()
        .iter()
        .map(verter_type_engine::semantic_query::SurfaceMember::copy_for_tests)
        .collect::<Vec<_>>();
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
        graph.node_scope(base.id()).expect("subject scope"),
    );
    let key_names = ["optional", "access", "method", "named", "namedAgain", "5"];
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
            !read.result_is_partial(),
            "complete member read: {:?}",
            read.partial_reasons()
        );
        match read.value().clone() {
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
                    index: IndexKey::String(verter_type_expr::OwnedStr::from(*key)),
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
fn project_global_reads_retain_unowned_dependencies_and_exclude_other_projects() {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let host = VerterHost::new(HostConfig::default(), workspace.clone());
    let projects = ["app", "other"].map(|name| {
        let root = format!("/{name}");
        let config = format!("{root}/tsconfig.json");
        workspace.inject_file(
            config.clone(),
            Arc::from(r#"{"compilerOptions":{"noLib":true},"include":["**/*.ts"]}"#),
        );
        let mut project =
            verter_workspace::ide_project_config(root.clone(), root, Some(config.clone()));
        project.compiler_options =
            verter_workspace::load_compiler_options(workspace.as_ref(), &config);
        project
    });
    host.configure_projects(projects.to_vec());
    for (canonical, source) in [
        ("/app/global.ts", "interface Shared { own: 1 }"),
        (
            "/app/node_modules/pkg/index.d.ts",
            "export {}; declare global { interface Shared { dependency: 2 } }",
        ),
        ("/other/global.ts", "interface Shared { foreign: 3 }"),
    ] {
        upsert_ts(&host, canonical, source);
        host.ensure_indexed_ready(canonical)
            .expect("indexed contributor");
    }
    let root = host.workspace().published_root().expect("configured root");
    assert!(!root.snapshot.owners_for_file("/app/global.ts").is_empty());
    assert!(root
        .snapshot
        .owners_for_file("/app/node_modules/pkg/index.d.ts")
        .is_empty());
    assert_ne!(
        root.snapshot.owners_for_file("/app/global.ts"),
        root.snapshot.owners_for_file("/other/global.ts")
    );
    upsert_ts(
        &host,
        "/app/read.ts",
        "import 'pkg';\ntype Probe = Shared['dependency'];",
    );
    use verter_type_engine::resolver_core::request_ports::OwnedLowering;
    let contributors = host
        .global_contributor_answer(
            "/app/read.ts",
            "Shared",
            verter_session_query::facts::SymbolSpace::Type,
        )
        .contributors;
    let canonicals: BTreeSet<_> = contributors
        .entries
        .iter()
        .map(|entry| entry.artifact_key.canonical.as_ref())
        .collect();
    assert_eq!(
        canonicals,
        BTreeSet::from(["/app/global.ts", "/app/node_modules/pkg/index.d.ts"])
    );
    let node = host
        .resolve_named_symbol("/app/read.ts", "Probe", Some(ProjectionMode::Expanded))
        .expect("global read");
    assert_eq!(
        host.project_node_to_type_expr_for_test(node.id()),
        Some(verter_type_expr::TypeExpr::Literal(
            verter_type_expr::LiteralValue::Number(2.0)
        ))
    );
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
                .project_node_to_type_expr_for_test(node.id())
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

#[test]
fn global_interface_added_parameters_keep_explicit_arguments() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for (canonical, declaration) in [
        ("/a.ts", "interface Shared<T> { first: T }"),
        ("/b.ts", "interface Shared<T, U = number> { second: U }"),
    ] {
        upsert_ts(
            &host,
            canonical,
            &format!(
                "{declaration}
            type Default = Shared<string>['second'];
            type Explicit = Shared<string, boolean>['second'];
            type Other = Shared<string, string>['second'];"
            ),
        );
        host.ensure_indexed_ready(canonical).expect("indexed peer");
    }
    for canonical in ["/b.ts", "/a.ts"] {
        for (name, expected) in [
            ("Default", verter_type_expr::PrimitiveName::Number),
            ("Explicit", verter_type_expr::PrimitiveName::Boolean),
            ("Other", verter_type_expr::PrimitiveName::String),
        ] {
            let node = host
                .resolve_named_symbol(canonical, name, Some(ProjectionMode::Expanded))
                .expect("projection");
            assert_eq!(
                host.project_node_to_type_expr_for_test(node.id()),
                Some(verter_type_expr::TypeExpr::Primitive(expected)),
                "{canonical}: {name}"
            );
        }
    }
}

#[test]
fn merged_global_instantiation_preserves_approximation_from_every_declaration() {
    use super::ProjectSemanticDispatch;
    use verter_type_engine::semantic_query::{
        FactInterpretation, FactStatus, InstantiateKey, ProjectionReductionContext, QueryResult,
        SemanticQueryKey,
    };
    for partial in [true, false] {
        let host = VerterHost::new_standalone(HostConfig::default());
        let gap = if partial {
            "ReturnType<typeof makeGap>"
        } else {
            "string"
        };
        upsert_ts(
            &host,
            "/a.ts",
            &format!(
                "function makeGap() {{ let q = 'x'; (() => {{ q = 'y' }})(); return {{ q }} }} \
                 interface Shared {{ known: 1; gap: {gap} }}"
            ),
        );
        upsert_ts(&host, "/b.ts", "interface Shared { sibling: 2 }");
        for file in ["/a.ts", "/b.ts"] {
            host.ensure_indexed_ready(file).unwrap();
        }
        let dispatch = ProjectSemanticDispatch::new(&host);
        let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
        let mut first = None;
        for file in ["/a.ts", "/b.ts", "/a.ts"] {
            let key = SemanticQueryKey::Instantiate(InstantiateKey::new(
                dispatch.type_slot_for(
                    verter_type_expr::OwnedStr::from(file),
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    verter_type_expr::OwnedStr::from("Shared"),
                ),
                Arc::from([]),
                dispatch.instantiate_context_for(file, context),
            ));
            let read = dispatch.execute_read(key.clone());
            if partial {
                assert!(
                    matches!(
                        read.fact_status(),
                        FactStatus::Approximate(_, Some(FactInterpretation::Positional))
                    ),
                    "{file}: {read:?}"
                );
                assert!(read.cache_suppress());
                assert!(dispatch.graph().get_validated(&key, dispatch.ctx).is_none());
            } else {
                assert_eq!(
                    read.fact_status().clone(),
                    FactStatus::Complete,
                    "{file}: {read:?}"
                );
                assert!(dispatch.graph().get_validated(&key, dispatch.ctx).is_some());
            }
            let QueryResult::Value(node) = read.value().clone() else {
                panic!("{file}: {read:?}")
            };
            let mut pending = vec![node];
            let mut members = rustc_hash::FxHashMap::default();
            while let Some(contributor) = pending.pop() {
                let data = dispatch.graph().node_data(contributor).unwrap();
                match data.as_ref() {
                    SemanticNodeData::MergedDecl { contributors } => {
                        pending.extend(contributors.iter().copied())
                    }
                    SemanticNodeData::Object(surface) => {
                        for member in surface.positive_members() {
                            members
                                .insert(member.key.as_string().unwrap().to_owned(), member.value);
                        }
                    }
                    other => panic!("{file}: {other:?}"),
                }
            }
            assert_eq!(members.len(), 3);
            for (name, expected) in [("known", 1), ("sibling", 2)] {
                assert!(
                    matches!(dispatch.graph().node_data(members[name]).as_deref(), Some(SemanticNodeData::Literal(LiteralValue::Number(value))) if *value == f64::from(expected))
                );
            }
            if !partial {
                assert!(matches!(
                    dispatch.graph().node_data(members["gap"]).as_deref(),
                    Some(SemanticNodeData::Primitive(
                        verter_type_engine::semantic_query::PrimitiveKind::String
                    ))
                ));
            }
            let answer = (node, *read.fact_status());
            if let Some(first) = first {
                assert_eq!(
                    answer, first,
                    "the anchor cannot change availability or causes"
                );
            } else {
                first = Some(answer);
            }
        }
    }
}

/// Answer the probe and require one literal per contributor at `size`.
fn probe_answer(host: &VerterHost, size: usize, global: bool) {
    let graph = host.project_type_store().semantic_graph();
    let node = host
        .resolve_named_symbol("/scenario.ts", "Probe", Some(ProjectionMode::Expanded))
        .expect("probe answers");
    let mut pending = vec![node.id()];
    let mut actual = BTreeSet::new();
    while let Some(node) = pending.pop() {
        match graph.node_data(node).as_deref().expect("live result") {
            SemanticNodeData::Union(arms) => pending.extend(arms.iter().copied()),
            SemanticNodeData::Alias(inner) => pending.push(*inner),
            SemanticNodeData::Literal(LiteralValue::String(value)) if global => {
                actual.insert(value.to_string());
            }
            SemanticNodeData::Literal(LiteralValue::Number(value)) if !global => {
                actual.insert(value.to_string());
            }
            other => panic!("size={size}, global={global}: incomplete answer {other:?}"),
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
        "size={size}, global={global}: every contributor must answer with its own value"
    );
}

/// What one engine retains after a step, read from the host's own
/// retention figures and the aggregate account.
#[derive(Debug, Clone, Copy)]
struct Retained {
    retained_bytes: usize,
    pinned_bytes: usize,
    live_artifacts: usize,
    retired_versions: usize,
    snapshot_leases: usize,
    memo_candidates: usize,
}

fn retained(host: &VerterHost) -> Retained {
    let snapshot = host.retention_snapshot();
    Retained {
        retained_bytes: snapshot.retained_bytes,
        // Parse-snapshot pins: the graph's own storage pin grows with the
        // nodes each answer interned, and the memo's bookkeeping pin with
        // the candidates it indexes, both of which the candidate bounds
        // cover.
        pinned_bytes: snapshot.pinned_bytes
            - snapshot.semantic_graph_storage_bytes
            - snapshot.semantic_bookkeeping_storage_bytes,
        live_artifacts: snapshot.live_artifacts,
        retired_versions: snapshot.retained_retired_versions,
        snapshot_leases: snapshot.snapshot_leases,
        memo_candidates: snapshot.semantic_memo_candidates,
    }
}

/// Answering a merged interface at `FIRST` contributors and then, after
/// edits add the rest, at `GROWN` leaves the engine retaining what a fresh
/// engine answering only `GROWN` retains, plus the first answer.
///
/// Per-file state — artifacts, superseded versions, their parse-snapshot
/// leases and pinned bytes — must equal the fresh engine's. The first
/// demand's own cached answers may survive as superseded candidates, so
/// the candidate excess is bounded by that demand's answers (its
/// candidates beyond one lowering per contributor), and the retained-byte
/// excess by what the first step retained. A per-contributor retention
/// grows with the contributor count and fails both.
#[test]
fn growing_merged_contributors_between_answers_retains_only_the_answers() {
    const FIRST: usize = 64;
    const GROWN: usize = 256;
    for global in [true, false] {
        // The account is process-wide: the baseline engine is measured and
        // dropped before the growing one starts.
        let baseline = {
            let host = contributor_host_with(GROWN, GROWN, global);
            probe_answer(&host, GROWN, global);
            retained(&host)
        };
        let host = contributor_host_with(GROWN, FIRST, global);
        let setup = retained(&host);
        probe_answer(&host, FIRST, global);
        let first = retained(&host);
        for i in FIRST..GROWN {
            let (canonical, source) = contributor(i, global);
            upsert_ts(&host, &canonical, &source);
            host.ensure_indexed_ready(&canonical)
                .expect("contributor indexed");
        }
        probe_answer(&host, GROWN, global);
        let grown = retained(&host);
        eprintln!("global={global}: baseline={baseline:?} first={first:?} grown={grown:?}");
        let label = format!("global={global}, baseline={baseline:?}, grown={grown:?}");
        assert_eq!(grown.live_artifacts, baseline.live_artifacts, "{label}");
        assert!(
            grown.retired_versions <= baseline.retired_versions,
            "superseded versions must not stay retained: {label}"
        );
        assert!(
            grown.snapshot_leases <= baseline.snapshot_leases,
            "superseded parse snapshots must not stay pinned: {label}"
        );
        // A cell the first step's surviving answers still reach — a name a
        // superseded declaration charged — passes from that declaration's
        // bundle to the answers keeping it, pinned, when the bundle is
        // superseded; nothing else may stay.
        assert!(
            grown.pinned_bytes <= baseline.pinned_bytes + (first.pinned_bytes - setup.pinned_bytes),
            "pinned bytes may exceed a fresh engine's only by what the first step handed out: \
             first={first:?}, {label}"
        );
        let first_answers = first
            .memo_candidates
            .saturating_sub(setup.memo_candidates + FIRST);
        assert!(
            grown.memo_candidates <= baseline.memo_candidates + first_answers,
            "only the first demand's answers may survive: first={first:?}, {label}"
        );
        assert!(
            grown.retained_bytes
                <= baseline.retained_bytes + (first.retained_bytes - setup.retained_bytes),
            "retained bytes may exceed a fresh engine's only by the first step's: first={first:?}, {label}"
        );
    }
}

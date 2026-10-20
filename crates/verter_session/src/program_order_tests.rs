use std::sync::Arc;

use crate::{HostConfig, LanguageRegistry, VerterHost};

const ROOT: &str = "/program";

fn host(tsconfig: serde_json::Value, files: &[(&str, &str)]) -> VerterHost {
    let host = VerterHost::new_standalone_with_tsconfig_projects(
        HostConfig::default(),
        &[(ROOT, &tsconfig.to_string())],
    );
    for (name, source) in files {
        upsert(&host, name, source);
    }
    host
}

fn upsert(host: &VerterHost, name: &str, source: &str) {
    let path = format!("{ROOT}/{name}");
    let language = LanguageRegistry::global()
        .classify_static(&path)
        .static_resolution();
    crate::u6_flow_shape_corpus_tests::upsert(host, &path, source, language);
}

/// The program's files in order, by their path under [`ROOT`].
fn listed(host: &VerterHost) -> Vec<String> {
    let order = host.program_file_order(&format!("{ROOT}/main.ts"), &mut std::iter::empty());
    let mut files: Vec<(u32, String)> = [
        "main.ts",
        "a.ts",
        "b.ts",
        "es2015.ts",
        "node_modules/@types/bus/index.d.ts",
        "node_modules/@types/scope__pkg/index.d.ts",
        "lazy.ts",
        "typed.ts",
        "augmented.ts",
    ]
    .into_iter()
    .map(|name| (order.rank(&format!("{ROOT}/{name}")), name.to_owned()))
    .filter(|(rank, _)| *rank != u32::MAX)
    .collect();
    files.sort();
    files.into_iter().map(|(_, name)| name).collect()
}

/// `/// <reference types>` places the `@types` package's declarations
/// (scoped names mangled `@scope/pkg` → `scope__pkg`) before the file,
/// after its path references; `/// <reference lib>` names a library file,
/// never a program file; dynamic imports and import types follow the
/// static imports; a module augmentation's target is not pulled in.
#[test]
fn references_and_imports_place_files_in_typescript_visit_order() {
    let host = host(
        serde_json::json!({ "compilerOptions": { "noLib": true }, "files": ["main.ts"] }),
        &[
            (
                "main.ts",
                "/// <reference types=\"@scope/pkg\" />\n\
                 /// <reference lib=\"es2015\" />\n\
                 /// <reference types=\"bus\" />\n\
                 /// <reference path=\"./a.ts\" />\n\
                 import './b';\n\
                 const lazy = () => import('./lazy');\n\
                 type T = import('./typed').T;\n\
                 declare module './augmented' { interface A { a: 1 } }\n\
                 export {};\n",
            ),
            ("a.ts", "export {};\n"),
            ("b.ts", "export {};\n"),
            ("es2015.ts", "export {};\n"),
            ("lazy.ts", "export {};\n"),
            ("typed.ts", "export type T = 1;\n"),
            ("augmented.ts", "export interface A {}\n"),
            (
                "node_modules/@types/bus/package.json",
                "{\"name\":\"@types/bus\",\"types\":\"index.d.ts\"}",
            ),
            ("node_modules/@types/bus/index.d.ts", "interface Bus {}\n"),
            (
                "node_modules/@types/scope__pkg/package.json",
                "{\"name\":\"@types/scope__pkg\",\"types\":\"index.d.ts\"}",
            ),
            (
                "node_modules/@types/scope__pkg/index.d.ts",
                "interface Pkg {}\n",
            ),
        ],
    );
    assert_eq!(
        listed(&host),
        [
            "a.ts",
            "node_modules/@types/scope__pkg/index.d.ts",
            "node_modules/@types/bus/index.d.ts",
            "b.ts",
            "lazy.ts",
            "typed.ts",
            "main.ts",
        ]
    );
}

/// The published order's fingerprint in the live workspace, as the warm
/// validator reads it.
fn validated(host: &VerterHost) -> Option<verter_session_query::analysis::types::Hash16> {
    let workspace = host.workspace();
    let stamp = crate::program_order::ProgramOrderStamp::live(workspace.as_ref())?;
    host.project_type_store()
        .indexed()
        .program_order_index()
        .published_fingerprint(&format!("{ROOT}/tsconfig.json"), stamp, workspace.as_ref())
}

fn reads(host: &VerterHost) -> u64 {
    host.project_type_store()
        .indexed()
        .program_order_index()
        .reference_reads()
}

/// A program order is built once, reading each file once, and stays the
/// published order while the workspace lists the same program: an edit to
/// a file outside it keeps the order valid without a rebuild; an edit to
/// one of its files that changes no reference rebuilds re-reading only that
/// file and keeps the published order; a changed reference reorders it.
///
/// Mutation: stamping the order by the workspace's content generation
/// alone refuses it after the edit outside the program.
#[test]
fn program_order_is_reused_while_the_workspace_lists_the_same_program() {
    let host = host(
        serde_json::json!({ "compilerOptions": { "noLib": true }, "files": ["main.ts", "b.ts"] }),
        &[
            ("main.ts", "export {};\n"),
            ("b.ts", "export {};\n"),
            ("outside.ts", "export {};\n"),
        ],
    );
    let anchor = format!("{ROOT}/main.ts");
    let ingested_reads = reads(&host);
    let first = host.program_file_order(&anchor, &mut std::iter::empty());
    assert_eq!(
        reads(&host),
        ingested_reads,
        "lookup reuses ingestion's program order"
    );
    assert!(Arc::ptr_eq(
        &first,
        &host.program_file_order(&anchor, &mut std::iter::empty())
    ));

    upsert(&host, "outside.ts", "export const outside = 1;\n");
    assert_eq!(
        validated(&host),
        Some(first.fingerprint()),
        "an edit outside the program"
    );
    assert!(Arc::ptr_eq(
        &first,
        &host.program_file_order(&anchor, &mut std::iter::empty())
    ));
    assert_eq!(reads(&host), ingested_reads, "no file is re-read");

    upsert(&host, "b.ts", "export const b = 1;\n");
    assert_eq!(
        validated(&host),
        Some(first.fingerprint()),
        "unchanged references keep the order valid"
    );
    let rebuilt = host.program_file_order(&anchor, &mut std::iter::empty());
    assert!(
        Arc::ptr_eq(&first, &rebuilt),
        "the same sequence keeps the order"
    );
    assert_eq!(
        reads(&host),
        ingested_reads + 1,
        "only the edited file is re-read"
    );
    assert_eq!(validated(&host), Some(first.fingerprint()));

    upsert(&host, "b.ts", "import './main';\nexport {};\n");
    assert_eq!(listed(&host), ["main.ts", "b.ts"]);
    upsert(&host, "main.ts", "import './b';\nexport {};\n");
    assert_eq!(listed(&host), ["b.ts", "main.ts"]);
    assert_ne!(
        host.program_file_order(&anchor, &mut std::iter::empty())
            .fingerprint(),
        first.fingerprint()
    );
}

/// Root membership alone cannot prove even an empty global population:
/// imports and references can reach declarations that the directory walk
/// excludes. Discovery must settle before the first population snapshot.
#[test]
fn disk_program_discovers_transitive_contributors_before_the_first_read() {
    use verter_workspace::WorkspaceAccess;
    for edge in [
        "import './bridge';",
        "/// <reference path=\"./bridge.ts\" />",
    ] {
        for initial in [
            "export {};",
            "export {}; declare global { interface Discovered { root: 1 } }",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let directory = dir.path().canonicalize().unwrap();
            let root = directory.to_str().unwrap();
            let config = directory.join("tsconfig.json");
            std::fs::write(
                &config,
                r#"{"compilerOptions":{"noLib":true},"files":["main.ts"]}"#,
            )
            .unwrap();
            std::fs::write(directory.join("main.ts"), format!("{edge}\n{initial}")).unwrap();
            std::fs::write(
                directory.join("bridge.ts"),
                "/// <reference path=\"./leaf.d.ts\" />\nexport {};",
            )
            .unwrap();
            std::fs::write(
                directory.join("leaf.d.ts"),
                "interface Discovered { leaf: 2 }",
            )
            .unwrap();
            let make = || {
                let workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
                    verter_workspace::FilesystemOptions::default(),
                ));
                let project = verter_workspace::configured_ide_project(
                    workspace.as_ref(),
                    root.into(),
                    root.into(),
                    config.to_str().unwrap(),
                );
                workspace.configure_resolver(vec![project]);
                VerterHost::new(HostConfig::default(), workspace)
            };
            let count = |host: &VerterHost| {
                host.ingest_program_ambient_roots();
                host.project_type_store()
                    .indexed()
                    .global_contributor_index()
                    .snapshot()
                    .lookup(
                        &crate::file_artifact_store::AugmentationTargetKind::GlobalAugmentation,
                        "Discovered",
                        None,
                        true,
                    )
                    .entries
                    .len()
            };
            let host = make();
            // An import makes main.ts a module; a path reference does not.
            let roots = usize::from(initial.contains("root: 1"));
            assert_eq!(count(&host), roots + 1, "first read: {edge}, {initial}");
            assert_eq!(count(&host), roots + 1, "repeated read");
            assert_eq!(count(&make()), roots + 1, "independently cold host");
            let leaf = directory.join("leaf.d.ts");
            crate::u6_flow_shape_corpus_tests::upsert(
                &host,
                leaf.to_str().unwrap(),
                "interface Replaced { leaf: 3 }",
                verter_language::FileLanguage::script_ts(),
            );
            std::fs::write(&leaf, "interface Replaced { leaf: 3 }").unwrap();
            assert_eq!(count(&host), roots, "contributor edit");
            assert_eq!(count(&make()), roots, "cold after edit");
        }
    }
}

#[test]
fn imported_unlisted_files_keep_the_programs_aliases_and_options() {
    use verter_workspace::WorkspaceAccess;
    let dir = tempfile::tempdir().unwrap();
    let directory = dir.path().canonicalize().unwrap();
    let canonical_root = crate::id::canonicalize_id(directory.to_str().unwrap());
    let root = canonical_root.as_ref();
    std::fs::create_dir_all(directory.join("src")).unwrap();
    let config = directory.join("tsconfig.json");
    std::fs::write(&config, r#"{"compilerOptions":{"strict":true,"baseUrl":".","paths":{"@/*":["src/*"]}},"files":["src/main.ts"]}"#).unwrap();
    for (name, source) in [
        ("main.ts", "import './props'; export {};"),
        (
            "props.ts",
            "import type { Item } from '@/types'; export type P = Item;",
        ),
        ("types.ts", "export interface Item { label: 'item' }"),
        ("outside.ts", "export {};"),
    ] {
        std::fs::write(directory.join("src").join(name), source).unwrap();
    }
    let workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let project = verter_workspace::configured_ide_project(
        workspace.as_ref(),
        root.into(),
        root.into(),
        config.to_str().unwrap(),
    );
    workspace.configure_resolver(vec![project]);
    let host = VerterHost::new(HostConfig::default(), workspace);
    let path = |name: &str| format!("{root}/src/{name}");
    assert_eq!(
        host.resolve_type_dependency_canonical(&path("props.ts"), "@/types")
            .as_deref(),
        Some(path("types.ts").as_str())
    );
    assert_eq!(
        host.resolve_import_transient(&path("props.ts"), "@/types"),
        Some(path("types.ts"))
    );
    assert_eq!(
        host.host_view_env_hashes_for(&path("props.ts")),
        host.host_view_env_hashes_for(&path("main.ts"))
    );
    assert_eq!(
        host.resolve_project_for_canonical(&path("props.ts")),
        host.resolve_project_for_canonical(&path("main.ts"))
    );
    assert_eq!(
        host.resolve_project_for_canonical(&path("outside.ts")),
        None
    );
    assert!(host
        .resolve_type_dependency_canonical(&path("outside.ts"), "@/types")
        .as_deref()
        .is_none());
}

/// An edge can keep its target while acquiring a different negative probe.
/// Creating that nearer candidate later must retire the restamped order.
#[test]
fn equal_edges_refresh_reference_probe_dependencies() {
    let host = host(
        serde_json::json!({ "compilerOptions": { "noLib": true }, "files": ["main.ts"] }),
        &[
            ("main.ts", "/// <reference path=\"./a.tsx\" />"),
            ("a.tsx", "export {};"),
        ],
    );
    let anchor = format!("{ROOT}/main.ts");
    let first = host.program_file_order(&anchor, &mut std::iter::empty());
    upsert(&host, "main.ts", "/// <reference path=\"./a\" />");
    assert_eq!(validated(&host), Some(first.fingerprint()));
    upsert(&host, "a.ts", "export {};");
    let changed = host.program_file_order(&anchor, &mut std::iter::empty());
    assert_ne!(first.fingerprint(), changed.fingerprint());
    assert!(changed.lists(&format!("{ROOT}/a.ts")));
    assert!(!changed.lists(&format!("{ROOT}/a.tsx")));
}

/// A captured configuration cannot publish its roots under a newer
/// configuration's stamp, even if the positional project ID is unchanged.
#[test]
fn a_superseded_snapshot_cannot_publish_program_membership() {
    let host = host(
        serde_json::json!({ "compilerOptions": { "noLib": true }, "files": ["main.ts", "a.ts"] }),
        &[
            ("main.ts", "export {};"),
            ("a.ts", "export {};"),
            ("b.ts", "export {};"),
        ],
    );
    let workspace = host.workspace();
    let old = workspace.published_root().unwrap();
    let project = old.snapshot.owners_for_file(&format!("{ROOT}/main.ts"))[0];
    let mut configs = old.snapshot.resolver.configs().to_vec();
    configs[0].membership.materialized_files.clear();
    configs[0].membership.spec.files = ["main.ts", "b.ts"]
        .map(|name| verter_workspace::CanonicalPath::new(&format!("{ROOT}/{name}")))
        .to_vec();
    workspace.configure_resolver(configs);
    let _ = host.project_program_order(&old.snapshot, project);
    let current = host.program_file_order(&format!("{ROOT}/main.ts"), &mut std::iter::empty());
    assert!(current.lists(&format!("{ROOT}/b.ts")));
    assert!(!current.lists(&format!("{ROOT}/a.ts")));
}

/// Opening a program whose files import each other in a chain reads each
/// file's references a bounded number of times: an upsert extends discovery
/// from the changed file to files the program has not discovered yet, never
/// re-reading the import closure an earlier upsert already settled. The
/// first order built afterwards still lists the whole chain.
///
/// Mutation: descending into every child a fresh read returns re-reads the
/// whole chain below each upserted file, `n²/2` reads for `n` files.
#[test]
fn opening_an_import_chain_reads_each_file_a_bounded_number_of_times() {
    for n in [16, 32, 64] {
        let names: Vec<String> = (0..n).map(|i| format!("m{i}.ts")).collect();
        let sources: Vec<String> = (0..n)
            .map(|i| match i {
                0 => "export const v = 0;\n".to_owned(),
                _ => format!("export * from './m{}';\n", i - 1),
            })
            .collect();
        let files: Vec<(&str, &str)> = names
            .iter()
            .zip(&sources)
            .map(|(name, source)| (name.as_str(), source.as_str()))
            .chain([("main.ts", "export {};\n")])
            .collect();
        let mut listed_files = names.clone();
        listed_files.push("main.ts".to_owned());
        let host = host(
            serde_json::json!({ "compilerOptions": { "noLib": true }, "files": listed_files }),
            &files,
        );
        let order = host.program_file_order(&format!("{ROOT}/main.ts"), &mut std::iter::empty());
        let ranks: Vec<u32> = names
            .iter()
            .map(|name| order.rank(&format!("{ROOT}/{name}")))
            .collect();
        assert!(
            ranks.windows(2).all(|pair| pair[0] < pair[1]) && ranks[n - 1] != u32::MAX,
            "the chain is listed in import order: {ranks:?}"
        );
        let reads = reads(&host);
        assert!(
            reads <= 4 * n as u64,
            "{reads} reference reads to open a {n}-file import chain"
        );
    }
}

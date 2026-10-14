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
    let first = host.program_file_order(&anchor, &mut std::iter::empty());
    assert_eq!(reads(&host), 2, "a cold walk reads each program file once");
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
    assert_eq!(reads(&host), 2, "no file is re-read");

    upsert(&host, "b.ts", "export const b = 1;\n");
    assert_eq!(validated(&host), None, "a program file changed");
    let rebuilt = host.program_file_order(&anchor, &mut std::iter::empty());
    assert!(
        Arc::ptr_eq(&first, &rebuilt),
        "the same sequence keeps the order"
    );
    assert_eq!(reads(&host), 3, "only the edited file is re-read");
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

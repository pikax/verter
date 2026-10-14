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
/// never a program file; dynamic imports, import types and augmented
/// module names follow the static imports.
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
            "augmented.ts",
            "main.ts",
        ]
    );
}

/// A program order is built once per workspace state: an unchanged
/// workspace serves the published order; any edit retires it, and the
/// rebuilt order keeps its fingerprint while the sequence is unchanged.
#[test]
fn program_order_is_published_until_the_workspace_moves() {
    let host = host(
        serde_json::json!({ "compilerOptions": { "noLib": true }, "files": ["main.ts", "b.ts"] }),
        &[("main.ts", "export {};\n"), ("b.ts", "export {};\n")],
    );
    let anchor = format!("{ROOT}/main.ts");
    let first = host.program_file_order(&anchor, &mut std::iter::empty());
    assert!(Arc::ptr_eq(
        &first,
        &host.program_file_order(&anchor, &mut std::iter::empty())
    ));
    upsert(&host, "b.ts", "export const b = 1;\n");
    let rebuilt = host.program_file_order(&anchor, &mut std::iter::empty());
    assert!(!Arc::ptr_eq(&first, &rebuilt));
    assert_eq!(first.fingerprint(), rebuilt.fingerprint());
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

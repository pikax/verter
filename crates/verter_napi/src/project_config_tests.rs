//! The NAPI project-config ingress: a project configured from a tsconfig
//! carries that tsconfig's compiler options and `files` / `include` /
//! `exclude` membership — the program's root files in their authored order —
//! while a tsconfig-less project stays match-all under its root.

use std::sync::Arc;

use super::*;
use verter_session_query::resolution::IdeProjectConfig;
use verter_workspace::{CanonicalPath, MemoryOptions, MemoryWorkspace};

const ROOT: &str = "/napi-program";

fn workspace_with(files: &[(&str, &str)]) -> MemoryWorkspace {
    let workspace = MemoryWorkspace::new(MemoryOptions::default());
    for (name, source) in files {
        workspace.inject_file(format!("{ROOT}/{name}"), Arc::from(*source));
    }
    workspace
}

fn to_ide(workspace: &MemoryWorkspace, config: NapiIdeProjectConfig) -> IdeProjectConfig {
    napi_project_config_to_ide(workspace, config)
}

fn project(tsconfig: Option<&str>) -> NapiIdeProjectConfig {
    NapiIdeProjectConfig {
        root: ROOT.to_owned(),
        workspaceRoot: ROOT.to_owned(),
        tsconfigPath: tsconfig.map(|name| format!("{ROOT}/{name}")),
        ..NapiIdeProjectConfig::default()
    }
}

/// The program roots `ide` lists among `candidates` and the files its
/// membership materialized — the candidate set the program order reads —
/// relative to [`ROOT`].
fn program_roots(ide: &IdeProjectConfig, candidates: &[&str]) -> Vec<String> {
    let candidates: Vec<CanonicalPath> = candidates
        .iter()
        .map(|name| CanonicalPath::new(&format!("{ROOT}/{name}")))
        .chain(ide.membership.materialized_files.iter().cloned())
        .collect();
    ide.membership
        .spec
        .program_root_files(&candidates)
        .iter()
        .map(|root| {
            root.as_str()
                .trim_start_matches(&format!("{ROOT}/"))
                .to_owned()
        })
        .collect()
}

const CANDIDATES: &[&str] = &[
    "bus-0000.ts",
    "bus-0001.ts",
    "bus-0002.ts",
    "extra.ts",
    "lib.bench.d.ts",
    "scenario.ts",
];

/// A tsconfig listing its `files` in reverse path order lists the program's
/// roots in that authored order and owns nothing else; its compiler options
/// come from the file.
///
/// Mutation: building the project from the match-all constructor lists every
/// candidate in directory-walk order (`bus-0000.ts` first, `extra.ts`
/// included) and loads no compiler option.
#[test]
fn a_tsconfig_project_lists_its_authored_files_and_owns_nothing_else() {
    let workspace = workspace_with(&[(
        "tsconfig.json",
        r#"{
            "compilerOptions": { "strict": true, "noLib": true, "noEmit": true },
            "files": ["lib.bench.d.ts", "bus-0002.ts", "bus-0001.ts", "bus-0000.ts", "scenario.ts"]
        }"#,
    )]);
    let ide = to_ide(&workspace, project(Some("tsconfig.json")));

    assert_eq!(
        program_roots(&ide, CANDIDATES),
        [
            "lib.bench.d.ts",
            "bus-0002.ts",
            "bus-0001.ts",
            "bus-0000.ts",
            "scenario.ts"
        ],
        "the program roots are the tsconfig's `files`, as authored"
    );
    assert!(
        !ide.matches_file(&format!("{ROOT}/extra.ts")),
        "a file the tsconfig neither lists nor includes is not a member"
    );
    assert!(ide.matches_file(&format!("{ROOT}/bus-0001.ts")));
    assert_eq!(
        ide.tsconfig_path.as_deref(),
        Some("/napi-program/tsconfig.json")
    );
    assert!(ide.compiler_options.semantic.no_lib, "`noLib` is loaded");
}

/// `include` roots follow the authored entries and `exclude` filters them;
/// the loader enumerates the matches by walking the project, so a file no
/// one has read yet is still a root.
///
/// Mutation: leaving the materialized set empty lists no include match when
/// the caller knows no candidate.
#[test]
fn a_tsconfig_project_orders_include_matches_by_authored_entry() {
    let mut files: Vec<(&str, &str)> = CANDIDATES.iter().map(|name| (*name, "")).collect();
    files.push((
        "tsconfig.json",
        r#"{ "include": ["scenario.ts", "bus-*.ts"], "exclude": ["bus-0001.ts"] }"#,
    ));
    let workspace = workspace_with(&files);
    let ide = to_ide(&workspace, project(Some("tsconfig.json")));

    assert_eq!(
        program_roots(&ide, &[]),
        ["scenario.ts", "bus-0000.ts", "bus-0002.ts"]
    );
    assert!(!ide.matches_file(&format!("{ROOT}/bus-0001.ts")));
}

/// The tsconfig's own `paths` survive an ingress that names no override, and
/// an explicit override still layers on top.
///
/// Mutation: assigning the ingress's absent `baseUrl` unconditionally drops
/// the tsconfig's `baseUrl`.
#[test]
fn tsconfig_paths_survive_and_explicit_overrides_layer_on_top() {
    let workspace = workspace_with(&[(
        "tsconfig.json",
        r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@/*": ["src/*"] } } }"#,
    )]);

    let loaded = to_ide(
        &workspace,
        NapiIdeProjectConfig {
            compilerOptions: Some(NapiIdeProjectCompilerOptions::default()),
            ..project(Some("tsconfig.json"))
        },
    );
    assert_eq!(loaded.compiler_options.base_url.as_deref(), Some(ROOT));
    assert_eq!(
        loaded
            .compiler_options
            .paths
            .iter()
            .map(|(pattern, _)| pattern.as_str())
            .collect::<Vec<_>>(),
        ["@/*"],
        "the tsconfig's paths are loaded"
    );

    let overridden = to_ide(
        &workspace,
        NapiIdeProjectConfig {
            compilerOptions: Some(NapiIdeProjectCompilerOptions {
                baseUrl: None,
                paths: Some(vec![NapiTsConfigPath {
                    pattern: "~/*".to_owned(),
                    targets: vec!["lib/*".to_owned()],
                }]),
            }),
            ..project(Some("tsconfig.json"))
        },
    );
    assert_eq!(
        overridden.compiler_options.paths,
        [("~/*".to_owned(), vec!["lib/*".to_owned()])]
    );
}

/// A project with no tsconfig owns every file under its root and lists them
/// in directory-walk order.
#[test]
fn a_tsconfig_less_project_stays_match_all() {
    let workspace = workspace_with(&[]);
    let ide = to_ide(&workspace, project(None));

    assert_eq!(ide.tsconfig_path, None);
    assert!(ide.matches_file(&format!("{ROOT}/extra.ts")));
    assert_eq!(program_roots(&ide, CANDIDATES), CANDIDATES);
}

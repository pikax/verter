use super::preferred_specifier_candidates;

fn project_with_paths_and_aliases(
    root: &str,
    tsconfig: &str,
    paths: Vec<(&str, Vec<&str>)>,
    aliases: &[(&str, &str)],
) -> crate::resolver_core::IdeProjectConfig {
    let mut project = crate::resolver_core::IdeProjectConfig::new(
        root.to_string(),
        root.to_string(),
        Some(tsconfig.to_string()),
    );
    project.compiler_options.paths = paths
        .into_iter()
        .map(|(pattern, targets)| {
            (
                pattern.to_string(),
                targets.into_iter().map(str::to_string).collect(),
            )
        })
        .collect();
    project.workspace_aliases = aliases
        .iter()
        .map(|(find, replacement)| crate::resolver_core::WorkspaceAlias {
            find: find.to_string(),
            replacement: replacement.to_string(),
        })
        .collect();
    project
}

#[test]
fn collects_a_reverse_tsconfig_paths_candidate() {
    let project = project_with_paths_and_aliases(
        "/proj",
        "/proj/tsconfig.json",
        vec![("@app/*", vec!["./src/app/*"])],
        &[],
    );
    let projects = vec![project];

    let candidates =
        preferred_specifier_candidates(&projects, "/proj/src/main.ts", "/proj/src/app/thing.ts")
            .expect("importer is owned");
    assert_eq!(candidates, vec!["@app/thing.ts".to_string()]);
}

#[test]
fn collects_a_reverse_workspace_alias_candidate() {
    let project = project_with_paths_and_aliases(
        "/proj",
        "/proj/tsconfig.json",
        vec![],
        &[("@/", "/proj/src")],
    );
    let projects = vec![project];

    let candidates =
        preferred_specifier_candidates(&projects, "/proj/src/main.ts", "/proj/src/util.ts")
            .expect("importer is owned");
    assert_eq!(candidates, vec!["@/util.ts".to_string()]);
}

#[test]
fn collects_both_a_paths_and_an_alias_candidate_in_source_order() {
    let project = project_with_paths_and_aliases(
        "/proj",
        "/proj/tsconfig.json",
        vec![("@app/*", vec!["./src/app/*"])],
        &[("@/", "/proj/src")],
    );
    let projects = vec![project];

    let candidates =
        preferred_specifier_candidates(&projects, "/proj/src/main.ts", "/proj/src/app/thing.ts")
            .expect("importer is owned");
    // Paths-derived candidates collected before alias-derived ones,
    // matching the legacy function's own "1. paths, 2. aliases" order.
    assert_eq!(
        candidates,
        vec!["@app/thing.ts".to_string(), "@/app/thing.ts".to_string()]
    );
}

#[test]
fn misses_when_the_importer_has_no_owning_project() {
    let project = project_with_paths_and_aliases(
        "/proj",
        "/proj/tsconfig.json",
        vec![("@app/*", vec!["./src/app/*"])],
        &[],
    );
    let projects = vec![project];

    let candidates =
        preferred_specifier_candidates(&projects, "/elsewhere/main.ts", "/proj/src/app/thing.ts");
    assert!(candidates.is_none());
}

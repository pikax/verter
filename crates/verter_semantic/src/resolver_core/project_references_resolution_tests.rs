use std::sync::Arc;

use super::resolve_project_references;
use crate::resolver_core::{
    AttemptOutcome, CompletedAttempt, ResolutionBasis, ResolutionObservationSnapshot,
    ResolutionWorldBasis, ResolverAttemptView,
};

fn basis() -> ResolutionBasis {
    ResolutionBasis::new(
        ResolutionWorldBasis::new(
            crate::resolver_core::WorkspaceAuthorityId::test_only(1),
            crate::resolver_core::ResolutionPopulation::Base,
            crate::resolver_core::ResolutionWorldId::test_only(1),
            None,
        ),
        None,
    )
}

fn esm_import_ctx() -> crate::resolver_core::ResolutionContext {
    crate::resolver_core::ResolutionContext {
        phase: crate::resolver_core::ResolvePhase::CodegenBlocker,
        kind: crate::resolver_core::ResolveRequestKind::EsmImport,
    }
}

fn known_world_view(files: &[&str]) -> ResolverAttemptView {
    let mut snapshot = ResolutionObservationSnapshot::with_stable_absent_defaults_for_test();
    for path in files {
        snapshot.insert_path_probe((*path).to_string(), crate::resolver_core::PathProbe::File);
        snapshot.insert_real_path((*path).to_string(), Some(Arc::from(*path)));
    }
    ResolverAttemptView::from_resolution_snapshot(Arc::new(snapshot), basis())
}

fn project_config(
    root: &str,
    tsconfig_path: &str,
    references: &[&str],
    aliases: &[(&str, &str)],
    paths: &[(&str, Vec<&str>)],
) -> crate::resolver_core::IdeProjectConfig {
    let mut project = crate::resolver_core::IdeProjectConfig::new(
        root.to_string(),
        root.to_string(),
        Some(tsconfig_path.to_string()),
    );
    project.references = references.iter().map(|r| r.to_string()).collect();
    project.workspace_aliases = aliases
        .iter()
        .map(|(find, replacement)| crate::resolver_core::WorkspaceAlias {
            find: find.to_string(),
            replacement: replacement.to_string(),
        })
        .collect();
    project.compiler_options.paths = paths
        .iter()
        .map(|(pattern, targets)| {
            (
                pattern.to_string(),
                targets.iter().map(|t| t.to_string()).collect(),
            )
        })
        .collect();
    project
}

fn unwrap_hit(outcome: AttemptOutcome<CompletedAttempt<Option<String>>>) -> String {
    match outcome {
        AttemptOutcome::Complete(CompletedAttempt { value: Some(v), .. }) => v,
        other => panic!("expected Complete(Some(_)), got {other:?}"),
    }
}

fn assert_miss(outcome: AttemptOutcome<CompletedAttempt<Option<String>>>) {
    assert!(
        matches!(
            outcome,
            AttemptOutcome::Complete(CompletedAttempt { value: None, .. })
        ),
        "expected Complete(None), got {outcome:?}"
    );
}

#[test]
fn resolves_via_a_referenced_projects_workspace_alias() {
    let view = known_world_view(&["/proj/b/src/thing.ts"]);
    let project_a = project_config(
        "/proj/a",
        "/proj/a/tsconfig.json",
        &["/proj/b/tsconfig.json"],
        &[],
        &[],
    );
    let project_b = project_config(
        "/proj/b",
        "/proj/b/tsconfig.json",
        &[],
        &[("@b/", "/proj/b/src")],
        &[],
    );
    let projects = vec![project_a.clone(), project_b];

    let outcome = resolve_project_references(
        &view,
        basis(),
        &projects,
        &project_a,
        "@b/thing",
        esm_import_ctx(),
    );
    assert_eq!(unwrap_hit(outcome), "/proj/b/src/thing.ts");
}

#[test]
fn resolves_via_a_referenced_projects_tsconfig_paths_when_no_alias_matches() {
    let view = known_world_view(&["/proj/b/src/app/thing.ts"]);
    let project_a = project_config(
        "/proj/a",
        "/proj/a/tsconfig.json",
        &["/proj/b/tsconfig.json"],
        &[],
        &[],
    );
    let project_b = project_config(
        "/proj/b",
        "/proj/b/tsconfig.json",
        &[],
        &[],
        &[("@app/*", vec!["./src/app/*"])],
    );
    let projects = vec![project_a.clone(), project_b];

    let outcome = resolve_project_references(
        &view,
        basis(),
        &projects,
        &project_a,
        "@app/thing",
        esm_import_ctx(),
    );
    assert_eq!(unwrap_hit(outcome), "/proj/b/src/app/thing.ts");
}

#[test]
fn descends_transitively_when_the_direct_reference_has_no_match_of_its_own() {
    let view = known_world_view(&["/proj/c/src/thing.ts"]);
    let project_a = project_config(
        "/proj/a",
        "/proj/a/tsconfig.json",
        &["/proj/b/tsconfig.json"],
        &[],
        &[],
    );
    // B has neither a matching alias nor matching paths of its own — the
    // walk must descend into B's OWN references to reach C.
    let project_b = project_config(
        "/proj/b",
        "/proj/b/tsconfig.json",
        &["/proj/c/tsconfig.json"],
        &[],
        &[],
    );
    let project_c = project_config(
        "/proj/c",
        "/proj/c/tsconfig.json",
        &[],
        &[("@c/", "/proj/c/src")],
        &[],
    );
    let projects = vec![project_a.clone(), project_b, project_c];

    let outcome = resolve_project_references(
        &view,
        basis(),
        &projects,
        &project_a,
        "@c/thing",
        esm_import_ctx(),
    );
    assert_eq!(unwrap_hit(outcome), "/proj/c/src/thing.ts");
}

#[test]
fn a_reference_cycle_terminates_without_hiding_a_reachable_sibling() {
    let view = known_world_view(&["/proj/c/src/thing.ts"]);
    let project_a = project_config(
        "/proj/a",
        "/proj/a/tsconfig.json",
        &["/proj/b/tsconfig.json"],
        &[],
        &[],
    );
    // B references BACK to A (a cycle) AND references C. The back-edge
    // to A (already active — seeded from the importer's own tsconfig)
    // must be skipped without recursing, and the walk must still reach
    // C via B's other reference.
    let project_b = project_config(
        "/proj/b",
        "/proj/b/tsconfig.json",
        &["/proj/a/tsconfig.json", "/proj/c/tsconfig.json"],
        &[],
        &[],
    );
    let project_c = project_config(
        "/proj/c",
        "/proj/c/tsconfig.json",
        &[],
        &[("@c/", "/proj/c/src")],
        &[],
    );
    let projects = vec![project_a.clone(), project_b, project_c];

    let outcome = resolve_project_references(
        &view,
        basis(),
        &projects,
        &project_a,
        "@c/thing",
        esm_import_ctx(),
    );
    assert_eq!(unwrap_hit(outcome), "/proj/c/src/thing.ts");
}

#[test]
fn a_declared_reference_with_no_matching_project_is_skipped() {
    let view = known_world_view(&["/proj/b/src/thing.ts"]);
    let project_a = project_config(
        "/proj/a",
        "/proj/a/tsconfig.json",
        &["/proj/missing/tsconfig.json", "/proj/b/tsconfig.json"],
        &[],
        &[],
    );
    let project_b = project_config(
        "/proj/b",
        "/proj/b/tsconfig.json",
        &[],
        &[("@b/", "/proj/b/src")],
        &[],
    );
    // "projects" deliberately omits any config for "/proj/missing/tsconfig.json".
    let projects = vec![project_a.clone(), project_b];

    let outcome = resolve_project_references(
        &view,
        basis(),
        &projects,
        &project_a,
        "@b/thing",
        esm_import_ctx(),
    );
    assert_eq!(unwrap_hit(outcome), "/proj/b/src/thing.ts");
}

#[test]
fn no_matching_reference_anywhere_misses() {
    let view = known_world_view(&[]);
    let project_a = project_config(
        "/proj/a",
        "/proj/a/tsconfig.json",
        &["/proj/b/tsconfig.json"],
        &[],
        &[],
    );
    let project_b = project_config(
        "/proj/b",
        "/proj/b/tsconfig.json",
        &[],
        &[("@b/", "/proj/b/src")],
        &[],
    );
    let projects = vec![project_a.clone(), project_b];

    let outcome = resolve_project_references(
        &view,
        basis(),
        &projects,
        &project_a,
        "@nowhere/thing",
        esm_import_ctx(),
    );
    assert_miss(outcome);
}

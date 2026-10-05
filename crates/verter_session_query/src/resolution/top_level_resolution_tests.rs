use std::sync::Arc;

use super::{resolve_for_project_with_reader, resolve_with_reader};
use crate::resolution::{
    AttemptOutcome, CompletedAttempt, ResolutionBasis, ResolutionObservationSnapshot,
    ResolutionWorldBasis, ResolverAttemptView,
};

fn basis() -> ResolutionBasis {
    ResolutionBasis::new(
        ResolutionWorldBasis::new(
            crate::resolution::WorkspaceAuthorityId::test_only(1),
            crate::resolution::ResolutionPopulation::Base,
            crate::resolution::ResolutionWorldId::test_only(1),
            None,
        ),
        None,
    )
}

fn esm_import_ctx() -> crate::resolution::ResolutionContext {
    crate::resolution::ResolutionContext {
        phase: crate::resolution::ResolvePhase::CodegenBlocker,
        kind: crate::resolution::ResolveRequestKind::EsmImport,
    }
}

fn known_world_view(files: &[&str]) -> ResolverAttemptView {
    let mut snapshot = ResolutionObservationSnapshot::with_stable_absent_defaults_for_test();
    for path in files {
        snapshot.insert_path_probe((*path).to_string(), crate::resolution::PathProbe::File);
        snapshot.insert_real_path((*path).to_string(), Some(Arc::from(*path)));
    }
    ResolverAttemptView::from_resolution_snapshot(Arc::new(snapshot), basis())
}

fn configured_project(
    root: &str,
    tsconfig: &str,
    aliases: &[(&str, &str)],
) -> crate::resolution::IdeProjectConfig {
    let mut project = crate::resolution::IdeProjectConfig::new(
        root.to_string(),
        root.to_string(),
        Some(tsconfig.to_string()),
    );
    project.workspace_aliases = aliases
        .iter()
        .map(|(find, replacement)| crate::resolution::WorkspaceAlias {
            find: find.to_string(),
            replacement: replacement.to_string(),
        })
        .collect();
    project
}

// ── resolve_with_reader ──

#[test]
fn resolve_with_reader_resolves_an_owned_importer_via_a_workspace_alias() {
    let view = known_world_view(&["/proj/src/util.ts"]);
    let projects = vec![configured_project(
        "/proj",
        "/proj/tsconfig.json",
        &[("@/", "/proj/src")],
    )];
    let request = crate::resolution::ResolveRequest {
        importer_id: "/proj/src/main.ts".to_string(),
        specifier: "@/util".to_string(),
        kind: crate::resolution::ResolveRequestKind::EsmImport,
        phase: crate::resolution::ResolvePhase::CodegenBlocker,
    };

    let outcome = resolve_with_reader(&view, basis(), &projects, &request);
    match outcome {
        AttemptOutcome::Complete(CompletedAttempt {
            value: Some(result),
            ..
        }) => {
            assert_eq!(result.source_id, "/proj/src/util.ts");
            assert_eq!(
                result.resolution_kind,
                crate::resolution::ResolutionKind::WorkspaceAlias
            );
            assert_eq!(result.provider_id, "/proj/src/util.ts");
            assert_eq!(result.provider_specifier, "@/util");
            assert_eq!(
                result.owner_tsconfig_path.as_deref(),
                Some("/proj/tsconfig.json")
            );
        }
        other => panic!("expected Complete(Some(_)), got {other:?}"),
    }
}

#[test]
fn resolve_with_reader_falls_to_the_unowned_dispatch_when_no_project_claims_the_importer() {
    let view = known_world_view(&["/outside/sibling.ts"]);
    let projects: Vec<crate::resolution::IdeProjectConfig> = vec![];
    let request = crate::resolution::ResolveRequest {
        importer_id: "/outside/main.ts".to_string(),
        specifier: "./sibling.ts".to_string(),
        kind: crate::resolution::ResolveRequestKind::EsmImport,
        phase: crate::resolution::ResolvePhase::CodegenBlocker,
    };

    let outcome = resolve_with_reader(&view, basis(), &projects, &request);
    match outcome {
        AttemptOutcome::Complete(CompletedAttempt {
            value: Some(result),
            ..
        }) => {
            assert_eq!(result.source_id, "/outside/sibling.ts");
            assert_eq!(
                result.resolution_kind,
                crate::resolution::ResolutionKind::Relative
            );
            assert_eq!(
                result.provider_target,
                crate::resolution::ProviderTarget::SourceFile
            );
            assert_eq!(result.provider_specifier, "./sibling.ts");
            assert!(result.owner_tsconfig_path.is_none());
        }
        other => panic!("expected Complete(Some(_)), got {other:?}"),
    }
}

#[test]
fn resolve_with_reader_misses_when_nothing_in_the_whole_chain_resolves() {
    let view = known_world_view(&[]);
    let projects = vec![configured_project("/proj", "/proj/tsconfig.json", &[])];
    let request = crate::resolution::ResolveRequest {
        importer_id: "/proj/src/main.ts".to_string(),
        specifier: "totally-missing".to_string(),
        kind: crate::resolution::ResolveRequestKind::EsmImport,
        phase: crate::resolution::ResolvePhase::CodegenBlocker,
    };

    let outcome = resolve_with_reader(&view, basis(), &projects, &request);
    assert!(matches!(
        outcome,
        AttemptOutcome::Complete(CompletedAttempt { value: None, .. })
    ));
}

// ── resolve_for_project_with_reader ──

#[test]
fn resolve_for_project_with_reader_resolves_relative_to_the_owned_project_root() {
    let view = known_world_view(&["/proj/sibling.ts"]);
    let projects = vec![configured_project("/proj", "/proj/tsconfig.json", &[])];
    let owner = crate::resolution::ProjectOwnership {
        project_root: "/proj".to_string(),
        tsconfig_path: Some("/proj/tsconfig.json".to_string()),
    };

    let outcome = resolve_for_project_with_reader(
        &view,
        basis(),
        &projects,
        &owner,
        "./sibling.ts",
        esm_import_ctx(),
    );
    match outcome {
        AttemptOutcome::Complete(CompletedAttempt {
            value: Some(result),
            ..
        }) => {
            assert_eq!(result.source_id, "/proj/sibling.ts");
            assert_eq!(
                result.resolution_kind,
                crate::resolution::ResolutionKind::Relative
            );
            // build_project_resolve_result always keeps the literal
            // specifier, never a computed relative one.
            assert_eq!(result.provider_specifier, "./sibling.ts");
        }
        other => panic!("expected Complete(Some(_)), got {other:?}"),
    }
}

#[test]
fn resolve_for_project_with_reader_misses_immediately_when_ownership_does_not_match() {
    let view = known_world_view(&["/proj/sibling.ts"]);
    let projects = vec![configured_project("/proj", "/proj/tsconfig.json", &[])];
    let owner = crate::resolution::ProjectOwnership {
        project_root: "/proj/missing".to_string(),
        tsconfig_path: None,
    };

    let outcome = resolve_for_project_with_reader(
        &view,
        basis(),
        &projects,
        &owner,
        "./sibling.ts",
        esm_import_ctx(),
    );
    match outcome {
        AttemptOutcome::Complete(CompletedAttempt {
            value: None,
            output,
        }) => {
            // Discriminates: an ownership miss short-circuits before
            // ever touching the observation view.
            assert!(output.consumed_resolution_observations().is_empty());
        }
        other => panic!("expected Complete(None), got {other:?}"),
    }
}

#[test]
fn resolve_for_project_with_reader_misses_when_resolution_itself_fails() {
    let view = known_world_view(&[]);
    let projects = vec![configured_project("/proj", "/proj/tsconfig.json", &[])];
    let owner = crate::resolution::ProjectOwnership {
        project_root: "/proj".to_string(),
        tsconfig_path: Some("/proj/tsconfig.json".to_string()),
    };

    let outcome = resolve_for_project_with_reader(
        &view,
        basis(),
        &projects,
        &owner,
        "nope-nowhere",
        esm_import_ctx(),
    );
    assert!(matches!(
        outcome,
        AttemptOutcome::Complete(CompletedAttempt { value: None, .. })
    ));
}

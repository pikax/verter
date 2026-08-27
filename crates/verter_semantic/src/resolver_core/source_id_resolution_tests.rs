use std::sync::Arc;

use serde_json::json;

use super::{resolve_source_id, resolve_source_id_for_project, resolve_source_id_unowned};
use crate::resolver_core::{
    AttemptOutcome, CompletedAttempt, ConsumedResolutionObservationKey, ResolutionBasis,
    ResolutionObservationSnapshot, ResolutionPackageManifest, ResolutionWorldBasis,
    ResolverAttemptView,
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

fn empty_manifest() -> ResolutionPackageManifest {
    ResolutionPackageManifest {
        main: None,
        module: None,
        types: None,
        typings: None,
        exports: None,
        imports: None,
    }
}

fn known_world_view(
    files: &[&str],
    manifests: &[(&str, ResolutionPackageManifest)],
) -> ResolverAttemptView {
    let mut snapshot = ResolutionObservationSnapshot::with_stable_absent_defaults_for_test();
    for path in files {
        snapshot.insert_path_probe((*path).to_string(), crate::resolver_core::PathProbe::File);
        snapshot.insert_real_path((*path).to_string(), Some(Arc::from(*path)));
    }
    for (directory, manifest) in manifests {
        snapshot.insert_path_probe(
            format!("{directory}/package.json"),
            crate::resolver_core::PathProbe::File,
        );
        snapshot
            .insert_package_manifest((*directory).to_string(), Some(Arc::new(manifest.clone())));
    }
    ResolverAttemptView::from_resolution_snapshot(Arc::new(snapshot), basis())
}

type Hit = (String, crate::resolver_core::ResolutionKind);

fn unwrap_hit(
    outcome: AttemptOutcome<CompletedAttempt<Option<Hit>>>,
) -> (Hit, crate::resolver_core::AttemptOutput) {
    match outcome {
        AttemptOutcome::Complete(CompletedAttempt {
            value: Some(hit),
            output,
        }) => (hit, output),
        other => panic!("expected Complete(Some(_)), got {other:?}"),
    }
}

fn assert_miss(outcome: AttemptOutcome<CompletedAttempt<Option<Hit>>>) {
    assert!(
        matches!(
            outcome,
            AttemptOutcome::Complete(CompletedAttempt { value: None, .. })
        ),
        "expected Complete(None), got {outcome:?}"
    );
}

#[test]
fn resolves_a_relative_specifier_when_the_importer_is_outside_node_modules() {
    let view = known_world_view(&["/proj/src/sibling.ts"], &[]);

    let outcome = resolve_source_id_unowned(
        &view,
        basis(),
        "/proj/src/main.ts",
        "./sibling.ts",
        esm_import_ctx(),
    );
    let (result, output) = unwrap_hit(outcome);
    assert_eq!(
        result,
        (
            "/proj/src/sibling.ts".to_string(),
            crate::resolver_core::ResolutionKind::Relative
        )
    );
    // Discriminates: an importer outside node_modules must never even
    // consult a manifest for the re-entry boundary check — the "trivial
    // true" path is unconditional, not a check that happened to pass.
    assert!(output
        .consumed_resolution_observations()
        .iter()
        .all(|key| !matches!(
            key,
            ConsumedResolutionObservationKey::PackageManifest { .. }
        )));
}

#[test]
fn resolves_an_absolute_specifier_ignoring_the_importer_directory() {
    let view = known_world_view(&["/abs/target.ts"], &[]);

    let outcome = resolve_source_id_unowned(
        &view,
        basis(),
        "/proj/src/main.ts",
        "/abs/target.ts",
        esm_import_ctx(),
    );
    let (result, _) = unwrap_hit(outcome);
    assert_eq!(
        result,
        (
            "/abs/target.ts".to_string(),
            crate::resolver_core::ResolutionKind::Relative
        )
    );
}

#[test]
fn a_relative_specifier_that_resolves_nowhere_misses() {
    let view = known_world_view(&[], &[]);

    let outcome = resolve_source_id_unowned(
        &view,
        basis(),
        "/proj/src/main.ts",
        "./missing.ts",
        esm_import_ctx(),
    );
    assert_miss(outcome);
}

#[test]
fn confirms_a_relative_follow_that_stays_within_the_owning_package() {
    let view = known_world_view(
        &["/proj/node_modules/pkgname/src/sibling.js"],
        &[("/proj/node_modules/pkgname", empty_manifest())],
    );

    let outcome = resolve_source_id_unowned(
        &view,
        basis(),
        "/proj/node_modules/pkgname/src/index.js",
        "./sibling.js",
        esm_import_ctx(),
    );
    let (result, _) = unwrap_hit(outcome);
    assert_eq!(
        result,
        (
            "/proj/node_modules/pkgname/src/sibling.js".to_string(),
            crate::resolver_core::ResolutionKind::Relative
        )
    );
}

#[test]
fn rejects_a_relative_follow_that_escapes_the_owning_package_boundary() {
    // "../../../outside/leak.js" from inside pkgname/src collapses to
    // "/proj/outside/leak.js" — a real file, but OUTSIDE
    // "/proj/node_modules/pkgname". Even though the manifest exists and
    // the probe succeeds, the re-entry boundary check must reject it.
    let view = known_world_view(
        &["/proj/outside/leak.js"],
        &[("/proj/node_modules/pkgname", empty_manifest())],
    );

    let outcome = resolve_source_id_unowned(
        &view,
        basis(),
        "/proj/node_modules/pkgname/src/index.js",
        "../../../outside/leak.js",
        esm_import_ctx(),
    );
    assert_miss(outcome);
}

#[test]
fn rejects_a_relative_follow_when_the_owning_package_manifest_is_missing() {
    let view = known_world_view(&["/proj/node_modules/pkgname/src/sibling.js"], &[]);

    let outcome = resolve_source_id_unowned(
        &view,
        basis(),
        "/proj/node_modules/pkgname/src/index.js",
        "./sibling.js",
        esm_import_ctx(),
    );
    assert_miss(outcome);
}

#[test]
fn resolves_a_hash_specifier_via_the_unbounded_imports_walk() {
    let view = known_world_view(
        &["/proj/src/utils/format.ts"],
        &[(
            "/proj",
            ResolutionPackageManifest {
                imports: Some(json!({ "#utils/*": "./src/utils/*.ts" })),
                ..empty_manifest()
            },
        )],
    );

    let outcome = resolve_source_id_unowned(
        &view,
        basis(),
        "/proj/src/main.ts",
        "#utils/format",
        esm_import_ctx(),
    );
    let (result, _) = unwrap_hit(outcome);
    assert_eq!(
        result,
        (
            "/proj/src/utils/format.ts".to_string(),
            crate::resolver_core::ResolutionKind::PackageImports
        )
    );
}

#[test]
fn resolves_a_bare_specifier_via_the_unbounded_node_modules_walk() {
    let view = known_world_view(
        &["/proj/node_modules/lodash/index.js"],
        &[(
            "/proj/node_modules/lodash",
            ResolutionPackageManifest {
                main: Some("./index.js".to_string()),
                ..empty_manifest()
            },
        )],
    );

    let outcome = resolve_source_id_unowned(
        &view,
        basis(),
        "/proj/src/main.ts",
        "lodash",
        esm_import_ctx(),
    );
    let (result, _) = unwrap_hit(outcome);
    assert_eq!(
        result,
        (
            "/proj/node_modules/lodash/index.js".to_string(),
            crate::resolver_core::ResolutionKind::NodeModules
        )
    );
}

// ── resolve_source_id / resolve_source_id_for_project ──

fn project_config(
    root: &str,
    tsconfig_path: &str,
    workspace_root: &str,
    aliases: &[(&str, &str)],
) -> crate::resolver_core::IdeProjectConfig {
    let mut project = crate::resolver_core::IdeProjectConfig::new(
        root.to_string(),
        workspace_root.to_string(),
        Some(tsconfig_path.to_string()),
    );
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
fn resolve_source_id_resolves_a_relative_specifier_without_the_reentry_guard() {
    // Discriminates from resolve_source_id_unowned: resolve_source_id
    // never calls package_follow_is_confirmed, so a relative follow that
    // would be REJECTED by that guard must still succeed here.
    let view = known_world_view(&["/proj/outside/leak.js"], &[]);
    let owner = project_config("/proj", "/proj/tsconfig.json", "/proj", &[]);

    let outcome = resolve_source_id(
        &view,
        basis(),
        std::slice::from_ref(&owner),
        &owner,
        "/proj/node_modules/pkgname/src/index.js",
        "../../../outside/leak.js",
        esm_import_ctx(),
    );
    let (result, _) = unwrap_hit(outcome);
    assert_eq!(
        result,
        (
            "/proj/outside/leak.js".to_string(),
            crate::resolver_core::ResolutionKind::Relative
        )
    );
}

#[test]
fn resolve_source_id_resolves_via_a_workspace_alias_before_imports_or_node_modules() {
    let view = known_world_view(&["/proj/src/thing.ts"], &[]);
    let owner = project_config(
        "/proj",
        "/proj/tsconfig.json",
        "/proj",
        &[("@/", "/proj/src")],
    );

    let outcome = resolve_source_id(
        &view,
        basis(),
        std::slice::from_ref(&owner),
        &owner,
        "/proj/src/main.ts",
        "@/thing",
        esm_import_ctx(),
    );
    let (result, _) = unwrap_hit(outcome);
    assert_eq!(
        result,
        (
            "/proj/src/thing.ts".to_string(),
            crate::resolver_core::ResolutionKind::WorkspaceAlias
        )
    );
}

#[test]
fn resolve_source_id_falls_through_to_the_bounded_imports_walk() {
    let view = known_world_view(
        &["/proj/src/utils/format.ts"],
        &[(
            "/proj",
            ResolutionPackageManifest {
                imports: Some(json!({ "#utils/*": "./src/utils/*.ts" })),
                ..empty_manifest()
            },
        )],
    );
    let owner = project_config("/proj", "/proj/tsconfig.json", "/proj", &[]);

    let outcome = resolve_source_id(
        &view,
        basis(),
        std::slice::from_ref(&owner),
        &owner,
        "/proj/src/main.ts",
        "#utils/format",
        esm_import_ctx(),
    );
    let (result, _) = unwrap_hit(outcome);
    assert_eq!(
        result,
        (
            "/proj/src/utils/format.ts".to_string(),
            crate::resolver_core::ResolutionKind::PackageImports
        )
    );
}

#[test]
fn resolve_source_id_falls_through_to_the_bounded_node_modules_walk() {
    let view = known_world_view(
        &["/proj/node_modules/lodash/index.js"],
        &[(
            "/proj/node_modules/lodash",
            ResolutionPackageManifest {
                main: Some("./index.js".to_string()),
                ..empty_manifest()
            },
        )],
    );
    let owner = project_config("/proj", "/proj/tsconfig.json", "/proj", &[]);

    let outcome = resolve_source_id(
        &view,
        basis(),
        std::slice::from_ref(&owner),
        &owner,
        "/proj/src/main.ts",
        "lodash",
        esm_import_ctx(),
    );
    let (result, _) = unwrap_hit(outcome);
    assert_eq!(
        result,
        (
            "/proj/node_modules/lodash/index.js".to_string(),
            crate::resolver_core::ResolutionKind::NodeModules
        )
    );
}

#[test]
fn resolve_source_id_misses_when_nothing_in_the_whole_chain_resolves() {
    let view = known_world_view(&[], &[]);
    let owner = project_config("/proj", "/proj/tsconfig.json", "/proj", &[]);

    let outcome = resolve_source_id(
        &view,
        basis(),
        std::slice::from_ref(&owner),
        &owner,
        "/proj/src/main.ts",
        "nowhere",
        esm_import_ctx(),
    );
    match outcome {
        AttemptOutcome::Complete(CompletedAttempt { value: None, .. }) => {}
        other => panic!("expected Complete(None), got {other:?}"),
    }
}

#[test]
fn resolve_source_id_for_project_resolves_relative_to_the_project_root_not_an_importer() {
    let view = known_world_view(&["/proj/sibling.ts"], &[]);
    let project = project_config("/proj", "/proj/tsconfig.json", "/proj", &[]);

    let outcome = resolve_source_id_for_project(
        &view,
        basis(),
        std::slice::from_ref(&project),
        &project,
        "./sibling.ts",
        esm_import_ctx(),
    );
    let (result, _) = unwrap_hit(outcome);
    assert_eq!(
        result,
        (
            "/proj/sibling.ts".to_string(),
            crate::resolver_core::ResolutionKind::Relative
        )
    );
}

#[test]
fn resolve_source_id_for_project_falls_through_to_the_bounded_node_modules_walk() {
    let view = known_world_view(
        &["/proj/node_modules/lodash/index.js"],
        &[(
            "/proj/node_modules/lodash",
            ResolutionPackageManifest {
                main: Some("./index.js".to_string()),
                ..empty_manifest()
            },
        )],
    );
    let project = project_config("/proj", "/proj/tsconfig.json", "/proj", &[]);

    let outcome = resolve_source_id_for_project(
        &view,
        basis(),
        std::slice::from_ref(&project),
        &project,
        "lodash",
        esm_import_ctx(),
    );
    let (result, _) = unwrap_hit(outcome);
    assert_eq!(
        result,
        (
            "/proj/node_modules/lodash/index.js".to_string(),
            crate::resolver_core::ResolutionKind::NodeModules
        )
    );
}

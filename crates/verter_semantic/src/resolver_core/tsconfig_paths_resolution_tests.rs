use std::sync::Arc;

use serde_json::json;

use super::{apply_tsconfig_target, resolve_path_mapping_target, resolve_tsconfig_paths};
use crate::resolver_core::{
    AttemptOutcome, CompletedAttempt, ResolutionBasis, ResolutionObservationSnapshot,
    ResolutionPackageManifest, ResolutionWorldBasis, ResolverAttemptView,
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

fn type_import_ctx() -> crate::resolver_core::ResolutionContext {
    crate::resolver_core::ResolutionContext {
        phase: crate::resolver_core::ResolvePhase::CodegenBlocker,
        kind: crate::resolver_core::ResolveRequestKind::TypeImport,
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

fn unwrap_hit(outcome: AttemptOutcome<CompletedAttempt<Option<String>>>) -> String {
    match outcome {
        AttemptOutcome::Complete(CompletedAttempt { value: Some(v), .. }) => v,
        other => panic!("expected Complete(Some(_)), got {other:?}"),
    }
}

fn project_with_paths(
    root: &str,
    paths: Vec<(&str, Vec<&str>)>,
) -> crate::resolver_core::IdeProjectConfig {
    let mut project =
        crate::resolver_core::IdeProjectConfig::new(root.to_string(), root.to_string(), None);
    project.compiler_options.paths = paths
        .into_iter()
        .map(|(pattern, targets)| {
            (
                pattern.to_string(),
                targets.into_iter().map(str::to_string).collect(),
            )
        })
        .collect();
    project
}

// ── apply_tsconfig_target ──

#[test]
fn apply_tsconfig_target_substitutes_the_wildcard() {
    assert_eq!(
        apply_tsconfig_target("/base", "./x/*", "foo"),
        "/base/x/foo"
    );
}

#[test]
fn apply_tsconfig_target_ignores_captured_when_the_target_has_no_wildcard() {
    assert_eq!(apply_tsconfig_target("/base", "./y", "ignored"), "/base/y");
}

// ── resolve_path_mapping_target ──

#[test]
fn resolves_via_package_exports_when_a_manifest_is_present() {
    let view = known_world_view(
        &["/proj/alias-target/index.js"],
        &[(
            "/proj/alias-target",
            ResolutionPackageManifest {
                exports: Some(json!({ ".": "./index.js" })),
                ..empty_manifest()
            },
        )],
    );

    let outcome =
        resolve_path_mapping_target(&view, basis(), "/proj/alias-target", esm_import_ctx());
    assert_eq!(unwrap_hit(outcome), "/proj/alias-target/index.js");
}

#[test]
fn falls_back_to_the_manifest_types_entry_when_exports_misses_and_declarations_are_preferred() {
    let view = known_world_view(
        &["/proj/alias-target/types.d.ts"],
        &[(
            "/proj/alias-target",
            ResolutionPackageManifest {
                exports: Some(json!({ ".": "./missing.js" })),
                types: Some("./types.d.ts".to_string()),
                ..empty_manifest()
            },
        )],
    );

    let outcome =
        resolve_path_mapping_target(&view, basis(), "/proj/alias-target", type_import_ctx());
    assert_eq!(unwrap_hit(outcome), "/proj/alias-target/types.d.ts");
}

#[test]
fn falls_through_to_legacy_at_the_same_directory_when_exports_and_types_both_miss() {
    // Discriminates from node_modules_resolution's per-directory step:
    // there is only ONE candidate directory here — an exports miss must
    // still try `resolve_legacy_package` at the SAME directory, never
    // "move to the next candidate" (there isn't one).
    let view = known_world_view(
        &["/proj/alias-target/main.js"],
        &[(
            "/proj/alias-target",
            ResolutionPackageManifest {
                exports: Some(json!({ ".": "./missing.js" })),
                main: Some("./main.js".to_string()),
                ..empty_manifest()
            },
        )],
    );

    let outcome =
        resolve_path_mapping_target(&view, basis(), "/proj/alias-target", esm_import_ctx());
    assert_eq!(unwrap_hit(outcome), "/proj/alias-target/main.js");
}

#[test]
fn falls_through_to_the_bare_probe_when_the_manifest_present_but_legacy_also_misses() {
    let view = known_world_view(
        &["/proj/alias-target.ts"],
        &[(
            "/proj/alias-target",
            ResolutionPackageManifest {
                main: Some("./missing-main.js".to_string()),
                ..empty_manifest()
            },
        )],
    );

    let outcome =
        resolve_path_mapping_target(&view, basis(), "/proj/alias-target", esm_import_ctx());
    assert_eq!(unwrap_hit(outcome), "/proj/alias-target.ts");
}

#[test]
fn probes_directly_when_no_manifest_is_present_at_all() {
    let view = known_world_view(&["/proj/no-manifest-here.js"], &[]);

    let outcome =
        resolve_path_mapping_target(&view, basis(), "/proj/no-manifest-here", esm_import_ctx());
    assert_eq!(unwrap_hit(outcome), "/proj/no-manifest-here.js");
}

// ── resolve_tsconfig_paths ──

#[test]
fn tries_each_target_of_a_matching_pattern_in_declared_order() {
    let view = known_world_view(&["/proj/src/second/util.ts"], &[]);
    let project = project_with_paths(
        "/proj",
        vec![("@app/*", vec!["./src/first/*", "./src/second/*"])],
    );

    let outcome = resolve_tsconfig_paths(&view, basis(), &project, "@app/util", esm_import_ctx());
    assert_eq!(unwrap_hit(outcome), "/proj/src/second/util.ts");
}

#[test]
fn falls_through_to_the_next_pattern_when_an_earlier_pattern_does_not_match() {
    let view = known_world_view(&["/proj/src/app/thing.ts"], &[]);
    let project = project_with_paths(
        "/proj",
        vec![
            ("@missing/*", vec!["./nowhere/*"]),
            ("@app/*", vec!["./src/app/*"]),
        ],
    );

    let outcome = resolve_tsconfig_paths(&view, basis(), &project, "@app/thing", esm_import_ctx());
    assert_eq!(unwrap_hit(outcome), "/proj/src/app/thing.ts");
}

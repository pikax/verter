use std::sync::Arc;

use serde_json::json;

use super::{
    read_package_manifest_if_present, resolve_legacy_package, resolve_manifest_types_entry,
    resolve_package_exports, resolve_package_target,
};
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

fn type_import_ctx() -> crate::resolver_core::ResolutionContext {
    crate::resolver_core::ResolutionContext {
        phase: crate::resolver_core::ResolvePhase::CodegenBlocker,
        kind: crate::resolver_core::ResolveRequestKind::TypeImport,
    }
}

fn require_ctx() -> crate::resolver_core::ResolutionContext {
    crate::resolver_core::ResolutionContext {
        phase: crate::resolver_core::ResolvePhase::CodegenBlocker,
        kind: crate::resolver_core::ResolveRequestKind::RequireCall,
    }
}

/// A FULLY-KNOWN-WORLD test view — every path not explicitly listed as
/// present answers the stable `Absent` fact directly (never `NeedInputs`).
fn known_world_view(files: &[&str]) -> ResolverAttemptView {
    let mut snapshot = ResolutionObservationSnapshot::with_stable_absent_defaults_for_test();
    for path in files {
        snapshot.insert_path_probe((*path).to_string(), crate::resolver_core::PathProbe::File);
        snapshot.insert_real_path((*path).to_string(), Some(Arc::from(*path)));
    }
    ResolverAttemptView::from_resolution_snapshot(Arc::new(snapshot), basis())
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

/// A world with a manifest present (or absent) at exactly one directory.
fn manifest_only_view(
    directory: &'static str,
    manifest: Option<ResolutionPackageManifest>,
) -> ResolverAttemptView {
    let mut snapshot = ResolutionObservationSnapshot::with_stable_absent_defaults_for_test();
    if manifest.is_some() {
        snapshot.insert_path_probe(
            format!("{directory}/package.json"),
            crate::resolver_core::PathProbe::File,
        );
    }
    snapshot.insert_package_manifest(directory.to_string(), manifest.map(Arc::new));
    ResolverAttemptView::from_resolution_snapshot(Arc::new(snapshot), basis())
}

// ── read_package_manifest_if_present ──

#[test]
fn read_package_manifest_if_present_hit_records_the_directory_as_consumed() {
    let manifest = ResolutionPackageManifest {
        main: Some("./index.js".into()),
        module: None,
        types: None,
        typings: None,
        exports: None,
        imports: None,
    };
    let view = manifest_only_view("/pkg", Some(manifest.clone()));

    match read_package_manifest_if_present(&view, "/pkg") {
        AttemptOutcome::Complete(CompletedAttempt { value, output }) => {
            assert_eq!(value.as_deref(), Some(&manifest));
            assert!(output.consumed_resolution_observations().contains(
                &ConsumedResolutionObservationKey::PackageManifest {
                    directory: Arc::from("/pkg")
                }
            ));
        }
        other => panic!("expected Complete(Some(_)), got {other:?}"),
    }
}

#[test]
fn read_package_manifest_if_present_miss_records_only_the_absent_path_probe() {
    let view = manifest_only_view("/pkg", None);

    match read_package_manifest_if_present(&view, "/pkg") {
        AttemptOutcome::Complete(CompletedAttempt {
            value: None,
            output,
        }) => {
            assert!(output.consumed_resolution_observations().contains(
                &ConsumedResolutionObservationKey::PathProbe {
                    path: Arc::from("/pkg/package.json")
                }
            ));
            assert!(output
                .consumed_resolution_observations()
                .iter()
                .all(|key| !matches!(
                    key,
                    ConsumedResolutionObservationKey::PackageManifest { .. }
                )));
        }
        other => panic!("expected Complete(None), got {other:?}"),
    }
}

// ── resolve_package_target ──

#[test]
fn resolve_package_target_string_probes_the_resolved_path() {
    let view = known_world_view(&["/pkg/dist/index.js"]);
    let value = json!("./dist/index.js");

    let outcome = resolve_package_target(&view, basis(), "/pkg", &value, None, esm_import_ctx());
    assert_eq!(unwrap_hit(outcome), "/pkg/dist/index.js");
}

#[test]
fn resolve_package_target_array_returns_first_hit() {
    let view = known_world_view(&["/pkg/dist/second.js"]);
    // First item ("./dist/first.js") is absent; second exists — proves the
    // array branch is a priority fallthrough, not an all-or-nothing probe.
    let value = json!(["./dist/first.js", "./dist/second.js"]);

    let outcome = resolve_package_target(&view, basis(), "/pkg", &value, None, esm_import_ctx());
    assert_eq!(unwrap_hit(outcome), "/pkg/dist/second.js");
}

#[test]
fn resolve_package_target_object_picks_first_matching_condition_in_declared_order() {
    let view = known_world_view(&["/pkg/import.js", "/pkg/default.js"]);
    // Object key order is JSON-source order; `package_conditions` for
    // (CodegenBlocker, EsmImport) is ["import", "default"] — "import"
    // must win even though "default" is listed first in the map.
    let value = json!({
        "default": "./default.js",
        "import": "./import.js",
    });

    let outcome = resolve_package_target(&view, basis(), "/pkg", &value, None, esm_import_ctx());
    assert_eq!(unwrap_hit(outcome), "/pkg/import.js");
}

#[test]
fn resolve_package_target_object_falls_through_a_missing_condition() {
    let view = known_world_view(&["/pkg/default.js"]);
    let value = json!({ "default": "./default.js" });

    let outcome = resolve_package_target(&view, basis(), "/pkg", &value, None, esm_import_ctx());
    assert_eq!(unwrap_hit(outcome), "/pkg/default.js");
}

#[test]
fn resolve_package_target_require_call_uses_require_then_default_conditions() {
    let view = known_world_view(&["/pkg/cjs.js"]);
    let value = json!({
        "import": "./esm.js",
        "require": "./cjs.js",
        "default": "./default.js",
    });

    let outcome = resolve_package_target(&view, basis(), "/pkg", &value, None, require_ctx());
    assert_eq!(unwrap_hit(outcome), "/pkg/cjs.js");
}

#[test]
fn resolve_package_target_applies_captured_wildcard() {
    let view = known_world_view(&["/pkg/dist/utils/format.js"]);
    let value = json!("./dist/*.js");

    let outcome = resolve_package_target(
        &view,
        basis(),
        "/pkg",
        &value,
        Some("utils/format"),
        esm_import_ctx(),
    );
    assert_eq!(unwrap_hit(outcome), "/pkg/dist/utils/format.js");
}

// ── resolve_package_exports ──

#[test]
fn resolve_package_exports_dot_root_string_form() {
    let view = known_world_view(&["/pkg/index.js"]);
    let exports = json!("./index.js");

    let outcome = resolve_package_exports(&view, basis(), "/pkg", &exports, ".", esm_import_ctx());
    assert_eq!(unwrap_hit(outcome), "/pkg/index.js");
}

#[test]
fn resolve_package_exports_non_dot_key_misses_a_bare_string_form() {
    let view = known_world_view(&["/pkg/index.js"]);
    let exports = json!("./index.js");

    // Discriminates: a string/array `exports` value has no subpath
    // mappings at all — any key other than "." must miss, never fall
    // back to the root target.
    let outcome =
        resolve_package_exports(&view, basis(), "/pkg", &exports, "./sub", esm_import_ctx());
    assert_miss(outcome);
}

#[test]
fn resolve_package_exports_subpath_map_matches_by_pattern() {
    let view = known_world_view(&["/pkg/dist/feature.js"]);
    let exports = json!({
        ".": "./index.js",
        "./feature": "./dist/feature.js",
    });

    let outcome = resolve_package_exports(
        &view,
        basis(),
        "/pkg",
        &exports,
        "./feature",
        esm_import_ctx(),
    );
    assert_eq!(unwrap_hit(outcome), "/pkg/dist/feature.js");
}

#[test]
fn resolve_package_exports_object_without_dot_keys_treated_as_conditions_map() {
    let view = known_world_view(&["/pkg/index.js"]);
    // No key starts with "." — this is a bare conditions object for the
    // root export, not a subpath map.
    let exports = json!({ "import": "./index.js", "default": "./index.js" });

    let outcome = resolve_package_exports(&view, basis(), "/pkg", &exports, ".", esm_import_ctx());
    assert_eq!(unwrap_hit(outcome), "/pkg/index.js");
}

// ── resolve_legacy_package ──

#[test]
fn resolve_legacy_package_esm_prefers_module_over_main() {
    let view = known_world_view(&["/pkg/esm.js", "/pkg/cjs.js"]);
    let manifest = ResolutionPackageManifest {
        main: Some("./cjs.js".into()),
        module: Some("./esm.js".into()),
        types: None,
        typings: None,
        exports: None,
        imports: None,
    };

    let outcome = resolve_legacy_package(&view, basis(), "/pkg", &manifest, "", esm_import_ctx());
    assert_eq!(unwrap_hit(outcome), "/pkg/esm.js");
}

#[test]
fn resolve_legacy_package_require_call_uses_main_only() {
    let view = known_world_view(&["/pkg/esm.js", "/pkg/cjs.js"]);
    let manifest = ResolutionPackageManifest {
        main: Some("./cjs.js".into()),
        module: Some("./esm.js".into()),
        types: None,
        typings: None,
        exports: None,
        imports: None,
    };

    // Discriminates: a require() call must never consult "module" even
    // though a hit exists there — RequireCall's key list is ["main"] only.
    let outcome = resolve_legacy_package(&view, basis(), "/pkg", &manifest, "", require_ctx());
    assert_eq!(unwrap_hit(outcome), "/pkg/cjs.js");
}

#[test]
fn resolve_legacy_package_type_import_prefers_types_over_main() {
    let view = known_world_view(&["/pkg/index.d.ts", "/pkg/index.js"]);
    let manifest = ResolutionPackageManifest {
        main: Some("./index.js".into()),
        module: None,
        types: Some("./index.d.ts".into()),
        typings: None,
        exports: None,
        imports: None,
    };

    let outcome = resolve_legacy_package(&view, basis(), "/pkg", &manifest, "", type_import_ctx());
    assert_eq!(unwrap_hit(outcome), "/pkg/index.d.ts");
}

#[test]
fn resolve_legacy_package_falls_back_to_index_when_every_manifest_key_misses() {
    let view = known_world_view(&["/pkg/index.ts"]);
    let manifest = ResolutionPackageManifest {
        main: Some("./missing-main.js".into()),
        module: None,
        types: None,
        typings: None,
        exports: None,
        imports: None,
    };

    let outcome = resolve_legacy_package(&view, basis(), "/pkg", &manifest, "", esm_import_ctx());
    assert_eq!(unwrap_hit(outcome), "/pkg/index.ts");
}

#[test]
fn resolve_legacy_package_nonempty_subpath_probes_directly_and_skips_manifest_keys() {
    let view = known_world_view(&["/pkg/lib/helper.js"]);
    let manifest = ResolutionPackageManifest {
        main: Some("./should-not-be-consulted.js".into()),
        module: None,
        types: None,
        typings: None,
        exports: None,
        imports: None,
    };

    let outcome = resolve_legacy_package(
        &view,
        basis(),
        "/pkg",
        &manifest,
        "lib/helper",
        esm_import_ctx(),
    );
    assert_eq!(unwrap_hit(outcome), "/pkg/lib/helper.js");
}

// ── resolve_manifest_types_entry ──

#[test]
fn resolve_manifest_types_entry_prefers_types_over_typings() {
    let view = known_world_view(&["/pkg/types.d.ts", "/pkg/typings.d.ts"]);
    let manifest = ResolutionPackageManifest {
        main: None,
        module: None,
        types: Some("./types.d.ts".into()),
        typings: Some("./typings.d.ts".into()),
        exports: None,
        imports: None,
    };

    let outcome = resolve_manifest_types_entry(&view, basis(), "/pkg", &manifest);
    assert_eq!(unwrap_hit(outcome), "/pkg/types.d.ts");
}

#[test]
fn resolve_manifest_types_entry_misses_when_neither_field_present() {
    let view = known_world_view(&[]);
    let manifest = ResolutionPackageManifest {
        main: Some("./index.js".into()),
        module: None,
        types: None,
        typings: None,
        exports: None,
        imports: None,
    };

    let outcome = resolve_manifest_types_entry(&view, basis(), "/pkg", &manifest);
    assert_miss(outcome);
}

#[test]
fn resolve_manifest_types_entry_never_substitutes_a_source_sibling() {
    // A world where a ".ts" source sibling of the (already-declaration)
    // ".d.ts" target exists — bare `probe_path` never runs the
    // source-sibling substitution (that only applies to JS-family runtime
    // extensions, and ".d.ts" isn't one), so this must resolve straight
    // to the declared "types" target.
    let view = known_world_view(&["/pkg/index.d.ts"]);
    let manifest = ResolutionPackageManifest {
        main: None,
        module: None,
        types: Some("./index.d.ts".into()),
        typings: None,
        exports: None,
        imports: None,
    };

    let outcome = resolve_manifest_types_entry(&view, basis(), "/pkg", &manifest);
    assert_eq!(unwrap_hit(outcome), "/pkg/index.d.ts");
}

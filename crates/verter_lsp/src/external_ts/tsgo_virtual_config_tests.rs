//! Hermetic tests for tsgo virtual-tsconfig materialization + diagnostic
//! invisibility. The real-engine `.vue`-include membership proof lives in the
//! gated integration test `crates/verter_lsp/tests/tsgo_virtual_membership.rs`.

use std::sync::Arc;

use verter_tsgo_api::proto::types::Diagnostic;
use verter_tsgo_api::snapshot::{AccessibleEntries, ReadFileResult, RealDirSource};

use super::{
    augment_tsconfig_bytes, build_virtual_overlay_snapshot, strip_injected_root_diagnostics,
};

const TSCONFIG: &str = "d:/ws/tsconfig.json";
const COMPANION: &str = "d:/ws/src/Foo.vue.tsx";

/// A real-dir source that knows one directory's entries (the rest fall through).
#[derive(Debug)]
struct OneDirSource {
    dir: String,
    entries: AccessibleEntries,
}
impl RealDirSource for OneDirSource {
    fn real_entries(&self, dir: &str) -> Option<AccessibleEntries> {
        if dir == self.dir {
            Some(self.entries.clone())
        } else {
            None
        }
    }
}

fn diag(code: u32, file: Option<&str>) -> Diagnostic {
    Diagnostic {
        code,
        category: 1,
        text: format!("diag {code}"),
        pos: 0,
        end: 0,
        file_name: file.map(str::to_string),
    }
}

// ── augment_tsconfig_bytes ──────────────────────────────────────────────────

#[test]
fn augment_injects_companion_into_files_and_preserves_everything_else() {
    let user = r#"{
  "compilerOptions": { "strict": true, "noEmit": true },
  "include": ["src/**/*.vue"]
}"#;
    let augmented = augment_tsconfig_bytes(user, &[COMPANION.to_string()]);

    let parsed: serde_json::Value =
        serde_json::from_str(&augmented).expect("augmented config is valid JSON");

    // The companion is now in `files`.
    let files = parsed
        .get("files")
        .and_then(|v| v.as_array())
        .expect("augmented config has a `files` array");
    assert!(
        files.iter().any(|f| f.as_str() == Some(COMPANION)),
        "the companion path is injected into `files`: {files:?}"
    );

    // Everything else is preserved: compilerOptions untouched, include intact.
    assert_eq!(
        parsed.pointer("/compilerOptions/strict"),
        Some(&serde_json::Value::Bool(true)),
        "compilerOptions.strict is preserved"
    );
    assert_eq!(
        parsed.pointer("/compilerOptions/noEmit"),
        Some(&serde_json::Value::Bool(true)),
        "compilerOptions.noEmit is preserved"
    );
    let include = parsed
        .get("include")
        .and_then(|v| v.as_array())
        .expect("include preserved");
    assert!(
        include.iter().any(|i| i.as_str() == Some("src/**/*.vue")),
        "the user's include is preserved untouched: {include:?}"
    );
}

#[test]
fn augment_merges_into_existing_files_without_duplicating() {
    let user = r#"{ "files": ["src/existing.ts", "d:/ws/src/Foo.vue.tsx"] }"#;
    let augmented = augment_tsconfig_bytes(user, &[COMPANION.to_string()]);
    let parsed: serde_json::Value = serde_json::from_str(&augmented).unwrap();
    let files = parsed.get("files").and_then(|v| v.as_array()).unwrap();

    // Pre-existing user `files` entry preserved.
    assert!(files.iter().any(|f| f.as_str() == Some("src/existing.ts")));
    // The companion appears exactly once (no duplicate against the existing one).
    assert_eq!(
        files
            .iter()
            .filter(|f| f.as_str() == Some(COMPANION))
            .count(),
        1,
        "the companion must not be duplicated when already present: {files:?}"
    );
}

/// NEGATIVE: an empty companion set is a no-op transform that still yields valid
/// JSON equal in meaning to the source (no `files` key fabricated).
#[test]
fn augment_with_no_companions_does_not_fabricate_files() {
    let user = r#"{ "include": ["src/**/*.vue"] }"#;
    let augmented = augment_tsconfig_bytes(user, &[]);
    let parsed: serde_json::Value = serde_json::from_str(&augmented).unwrap();
    assert!(
        parsed.get("files").is_none(),
        "no companions ⇒ no synthesized `files` key: {augmented}"
    );
}

// ── build_virtual_overlay_snapshot ──────────────────────────────────────────

#[test]
fn overlay_serves_augmented_tsconfig_and_falls_through_for_non_virtual() {
    let user = r#"{ "include": ["src/**/*.vue"] }"#;
    let augmented = augment_tsconfig_bytes(user, &[COMPANION.to_string()]);
    let real = Arc::new(OneDirSource {
        dir: "d:/ws/src".to_string(),
        entries: AccessibleEntries {
            files: vec!["Foo.vue".to_string()],
            directories: vec![],
        },
    });
    let snapshot = build_virtual_overlay_snapshot(
        TSCONFIG,
        &augmented,
        &[(COMPANION.to_string(), "export const x = 1;".to_string())],
        real,
    );

    // The overlay serves the AUGMENTED bytes for the tsconfig path.
    match snapshot.read_file(TSCONFIG) {
        ReadFileResult::Found(content) => {
            assert!(
                content.contains(COMPANION),
                "the served tsconfig contains the injected companion: {content}"
            );
        }
        other => panic!("expected the augmented tsconfig to be served, got {other:?}"),
    }

    // The companion surface is served too (so the injected `files` entry resolves).
    assert_eq!(
        snapshot.read_file(COMPANION),
        ReadFileResult::Found("export const x = 1;".to_string()),
        "the companion .tsx surface is served by the overlay"
    );

    // A path NOT in the overlay (a real user file) falls through to the real FS.
    assert_eq!(
        snapshot.read_file("d:/ws/src/Foo.vue"),
        ReadFileResult::FallThrough,
        "a non-virtual path falls through to the real config/source"
    );

    // The merged enumeration still surfaces the real on-disk file AND the
    // injected companion (so the engine discovers both).
    let entries = snapshot
        .get_accessible_entries("d:/ws/src")
        .expect("src dir is known");
    assert!(
        entries.files.contains(&"Foo.vue".to_string()),
        "real carrier source still enumerated: {entries:?}"
    );
    assert!(
        entries.files.contains(&"Foo.vue.tsx".to_string()),
        "injected companion enumerated in the merged listing: {entries:?}"
    );
}

// ── strip_injected_root_diagnostics (diagnostic invisibility) ───────────────

#[test]
fn injected_root_diagnostic_is_stripped_but_real_config_error_survives() {
    let real_config_error = diag(5024, Some(TSCONFIG)); // a real tsconfig options error
    let injected_root_diag = diag(6059, Some(COMPANION)); // points at the injected root
    let global_options_diag = diag(5023, None); // no fileName (global option)
    let real_source_diag = diag(2345, Some("d:/ws/src/other.ts")); // a real user file

    let filtered = strip_injected_root_diagnostics(
        vec![
            real_config_error.clone(),
            injected_root_diag.clone(),
            global_options_diag.clone(),
            real_source_diag.clone(),
        ],
        &[COMPANION.to_string()],
    );

    // DISCRIMINATING: the injected-root diagnostic is gone …
    assert!(
        !filtered
            .iter()
            .any(|d| d.file_name.as_deref() == Some(COMPANION)),
        "an injected-companion diagnostic must NOT survive: {filtered:?}"
    );
    // … AND the real config error is retained (the user must still see it).
    assert!(
        filtered.contains(&real_config_error),
        "a real config/options error MUST survive: {filtered:?}"
    );
    // … AND the global options diagnostic (no fileName) is retained.
    assert!(
        filtered.contains(&global_options_diag),
        "a global options diagnostic (no fileName) MUST survive: {filtered:?}"
    );
    // … AND a real user-source diagnostic is retained.
    assert!(
        filtered.contains(&real_source_diag),
        "a real user-source diagnostic MUST survive: {filtered:?}"
    );
    assert_eq!(
        filtered.len(),
        3,
        "exactly the injected-root diag is removed"
    );
}

/// NEGATIVE: with no injected paths, nothing is stripped (identity).
#[test]
fn strip_with_no_injected_paths_is_identity() {
    let diags = vec![diag(5024, Some(TSCONFIG)), diag(2345, Some("d:/ws/a.ts"))];
    let filtered = strip_injected_root_diagnostics(diags.clone(), &[]);
    assert_eq!(filtered, diags, "no injected paths ⇒ no diagnostic removed");
}

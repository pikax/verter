//! Unit tests for [`TsgoOwnedProvider`] internals that do not require a live tsgo
//! process. The live one-instance provider proof (diagnostics via `--api`,
//! features via `--lsp`, ONE process) lives in `tests/owned_provider_live.rs`
//! (gated on `VERTER_REQUIRE_TSGO`).

use super::*;
use crate::protocol::TypeDiagnosticSeverity;

#[test]
fn path_eq_normalizes_slashes_and_distinguishes_distinct_files() {
    // Backslash vs forward-slash is always normalized away (same file, any OS).
    assert!(path_eq(r"C:\ws\src\A.ts", "c:/ws/src/A.ts"));
    // Distinct files (different basename) never match.
    assert!(!path_eq("/ws/src/A.ts", "/ws/src/B.ts"));
}

/// On a case-INSENSITIVE filesystem (Windows) a case fold in the path — drive
/// letter OR a segment — must still match (the engine may report `C:` while the
/// configured path uses `c:`, and NTFS folds case).
#[cfg(target_os = "windows")]
#[test]
fn path_eq_folds_case_on_case_insensitive_fs() {
    assert!(path_eq("c:/WS/Src/A.ts", "C:/ws/src/a.ts"));
}

/// On a case-SENSITIVE filesystem (Linux / case-sensitive APFS) two files
/// differing ONLY by case are DISTINCT and must NOT be conflated — the pre-fix
/// unconditional `eq_ignore_ascii_case` wrongly merged them. Discriminating: this
/// assertion holds only with the platform-conditional comparison.
#[cfg(not(target_os = "windows"))]
#[test]
fn path_eq_is_case_sensitive_on_case_sensitive_fs() {
    assert!(!path_eq("/ws/src/A.ts", "/ws/src/a.ts"));
    // Same exact path still matches.
    assert!(path_eq("/ws/src/A.ts", "/ws/src/A.ts"));
}

/// `TypeDiagnosticSeverity` does not derive `PartialEq`, so match it by name.
fn severity_name(s: &TypeDiagnosticSeverity) -> &'static str {
    match s {
        TypeDiagnosticSeverity::Error => "error",
        TypeDiagnosticSeverity::Warning => "warning",
        TypeDiagnosticSeverity::Info => "info",
        TypeDiagnosticSeverity::Hint => "hint",
    }
}

#[test]
fn map_api_diagnostic_maps_category_and_offsets() {
    let d = verter_tsgo_api::proto::types::Diagnostic {
        code: 2322,
        category: 1,
        text: "Type 'string' is not assignable to type 'number'.".to_string(),
        pos: 10,
        end: 16,
        file_name: Some("c:/ws/src/A.ts".to_string()),
    };
    let mapped = map_api_diagnostic(&d);
    assert_eq!(mapped.code.as_deref(), Some("2322"));
    assert_eq!(mapped.start, 10);
    assert_eq!(mapped.end, 16);
    assert_eq!(severity_name(&mapped.severity), "error");
    assert!(mapped.message.contains("not assignable"));
}

#[test]
fn map_api_diagnostic_severity_table() {
    let base = verter_tsgo_api::proto::types::Diagnostic {
        code: 1,
        category: 0,
        text: "w".to_string(),
        pos: 0,
        end: 1,
        file_name: None,
    };
    assert_eq!(
        severity_name(&map_api_diagnostic(&base).severity),
        "warning"
    );

    let suggestion = map_api_diagnostic(&verter_tsgo_api::proto::types::Diagnostic {
        category: 2,
        ..base.clone()
    });
    assert_eq!(severity_name(&suggestion.severity), "hint");

    let message = map_api_diagnostic(&verter_tsgo_api::proto::types::Diagnostic {
        category: 3,
        ..base.clone()
    });
    assert_eq!(severity_name(&message.severity), "info");
}

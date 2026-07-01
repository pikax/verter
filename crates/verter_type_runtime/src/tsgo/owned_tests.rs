//! Unit tests for [`TsgoOwnedProvider`] internals that do not require a live tsgo
//! process. The live one-instance provider proof (diagnostics via `--api`,
//! features via `--lsp`, ONE process) lives in `tests/owned_provider_live.rs`
//! (gated on `VERTER_REQUIRE_TSGO`).

use super::*;
use crate::protocol::TypeDiagnosticSeverity;

#[test]
fn fs_paths_equal_normalizes_slashes_and_distinguishes_distinct_files() {
    // Backslash vs forward-slash is normalized away on every OS (identical case here,
    // so the filesystem case policy does not enter the comparison).
    assert!(fs_paths_equal(r"C:\ws\src\A.ts", "C:/ws/src/A.ts"));
    // Distinct files (different basename) never match on any OS.
    assert!(!fs_paths_equal("/ws/src/A.ts", "/ws/src/B.ts"));
}

/// On a case-INSENSITIVE filesystem (Windows / NTFS) a case fold in the path — drive
/// letter OR a segment — must still match: the engine may report `C:` while the
/// configured path uses `c:`, and NTFS folds case, so membership must hold.
#[cfg(target_os = "windows")]
#[test]
fn fs_paths_equal_folds_case_on_windows_case_insensitive_fs() {
    assert!(fs_paths_equal("c:/WS/Src/A.ts", "C:/ws/src/a.ts"));
}

/// On macOS the default APFS volume is case-INSENSITIVE, so a carrier whose
/// engine-reported `root_files` path differs only by case from the configured path
/// is the SAME file and MUST stay a configured-project member. The pre-unification
/// `cfg!(target_os = "windows")` predicate compared case-SENSITIVELY here, so a
/// macOS case variant missed `root_files` membership and silently dropped its
/// diagnostics. Discriminating: this assertion FAILS on the old Windows-only
/// predicate and passes only under the unified `fs_is_case_insensitive()` policy.
#[cfg(target_os = "macos")]
#[test]
fn fs_paths_equal_folds_case_on_macos_case_insensitive_fs() {
    assert!(fs_paths_equal("/ws/Src/A.ts", "/ws/src/a.ts"));
}

/// On Linux (case-SENSITIVE) two files differing ONLY by case are DISTINCT and must
/// NOT be conflated — an unconditional `eq_ignore_ascii_case` would wrongly merge
/// them. Discriminating: this holds only with the case-sensitive Linux branch of the
/// unified policy.
#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
#[test]
fn fs_paths_equal_is_case_sensitive_on_linux() {
    assert!(!fs_paths_equal("/ws/src/A.ts", "/ws/src/a.ts"));
    // Same exact path still matches.
    assert!(fs_paths_equal("/ws/src/A.ts", "/ws/src/A.ts"));
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
    // ASCII content ⇒ UTF-16 offset == byte offset, so start/end are unchanged.
    let content = "const abcdefghijklmnop = 1;\n";
    let mapped = map_api_diagnostic(&d, Some(content));
    assert_eq!(mapped.code.as_deref(), Some("2322"));
    assert_eq!(mapped.start, 10);
    assert_eq!(mapped.end, 16);
    assert_eq!(severity_name(&mapped.severity), "error");
    assert!(mapped.message.contains("not assignable"));
}

/// DISCRIMINATING (PERF-3-offset regression): the `--api` diagnostic `pos`/`end`
/// are UTF-16 code units, but `TypeDiagnostic.start`/`end` is a BYTE contract. With
/// a multi-byte character (an em-dash `—`, U+2014 — 3 UTF-8 bytes / 1 UTF-16 unit)
/// before the diagnostic, the byte offset is 2 GREATER than the UTF-16 offset.
///
/// RED before the fix: `map_api_diagnostic` copied `d.pos`/`d.end` straight through
/// (byte == UTF-16 offset), so `start`/`end` were 2 too small → LSP position drift.
/// GREEN after: the UTF-16 → byte conversion (shared `verter_tsgo_api`/`verter_span`
/// helper) yields the true byte offsets. This asserts the CONVERTED byte values,
/// which the passthrough could never produce.
#[test]
fn map_api_diagnostic_converts_utf16_offsets_to_bytes_on_non_ascii() {
    // Carrier comment carries an em-dash, then a diagnostic on `x`.
    // "// — note\nconst x: string = 1;\n"
    // UTF-16 units: '/'(0) '/'(1) ' '(2) '—'(3) ' '(4) 'n'(5) 'o'(6) 't'(7) 'e'(8)
    //   '\n'(9) 'c'(10) ...; the em-dash is 1 UTF-16 unit but 3 UTF-8 bytes.
    let content = "// \u{2014} note\nconst x: string = 1;\n";
    // The `1` literal: find it as a UTF-16 offset by walking chars.
    let one_byte = content.find('1').unwrap() as u32; // byte offset of `1`
                                                      // Compute the UTF-16 offset of `1` (what tsgo `--api` reports).
    let one_utf16 = content[..one_byte as usize].encode_utf16().count() as u32;
    assert!(
        one_utf16 < one_byte,
        "sanity: the em-dash makes the UTF-16 offset ({one_utf16}) strictly smaller \
         than the byte offset ({one_byte})"
    );

    let d = verter_tsgo_api::proto::types::Diagnostic {
        code: 2322,
        category: 1,
        text: "Type 'number' is not assignable to type 'string'.".to_string(),
        pos: one_utf16,
        end: one_utf16 + 1,
        file_name: Some("c:/ws/src/A.vue.tsx".to_string()),
    };
    let mapped = map_api_diagnostic(&d, Some(content));
    // The mapped BYTE start must equal the true byte offset of `1`, NOT the raw
    // UTF-16 `pos` (which a passthrough would have produced).
    assert_eq!(
        mapped.start, one_byte,
        "start must be the BYTE offset of `1` ({one_byte}), not the UTF-16 pos ({one_utf16})"
    );
    assert_eq!(mapped.end, one_byte + 1);
    assert_ne!(
        mapped.start, one_utf16,
        "a straight UTF-16 passthrough (start == pos) is the bug this test forbids"
    );
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
        severity_name(&map_api_diagnostic(&base, Some("w")).severity),
        "warning"
    );

    let suggestion = map_api_diagnostic(
        &verter_tsgo_api::proto::types::Diagnostic {
            category: 2,
            ..base.clone()
        },
        Some("w"),
    );
    assert_eq!(severity_name(&suggestion.severity), "hint");

    let message = map_api_diagnostic(
        &verter_tsgo_api::proto::types::Diagnostic {
            category: 3,
            ..base.clone()
        },
        Some("w"),
    );
    assert_eq!(severity_name(&message.severity), "info");
}

/// Build a fake `--api` snapshot with a single configured project whose root set is
/// `root_files`. Mirrors what `update_snapshot_open_project` returns from the engine,
/// constructed offline (all DTO fields are public).
fn fake_snapshot(
    config_file: &str,
    root_files: &[&str],
) -> verter_tsgo_api::api_attach::AttachSnapshot {
    use verter_tsgo_api::proto::types::{OpaqueHandle, ProjectResponse};
    verter_tsgo_api::api_attach::AttachSnapshot {
        snapshot: OpaqueHandle(7),
        projects: vec![ProjectResponse {
            id: "p.tsconfig".to_string(),
            config_file_name: config_file.to_string(),
            compiler_options: serde_json::Map::new(),
            root_files: root_files.iter().map(|f| (*f).to_string()).collect(),
        }],
    }
}

/// The tsgo carrier path is PROJECT-BOUND: `select_configured_project_carrier` returns
/// the engine carrier IFF the carrier is in the CONFIGURED project's root set —
/// ABSENCE is `None` (fail closed), NEVER a fallback to an inferred/single-file
/// project. (The boundary that matters, per the project-bound contract: not "was
/// open_project called", but "membership is required and its absence is an error.")
#[test]
fn carrier_membership_in_configured_project_root_files_is_required_not_inferred() {
    let tsconfig = "/ws/tsconfig.json";
    let carrier = "/ws/src/Widget.vue.tsx";

    // (1) Carrier IS in the configured project's root set ⇒ Some(engine carrier).
    let snap = fake_snapshot(tsconfig, &[carrier, "/ws/src/Other.ts"]);
    let resolved = select_configured_project_carrier(&snap, tsconfig, carrier);
    assert_eq!(
        resolved,
        Some(("p.tsconfig".to_string(), carrier.to_string())),
        "a carrier present in the configured project's root_files resolves to the \
         engine carrier under that project"
    );

    // (2) Carrier ABSENT from the configured project's root set ⇒ None (fail closed).
    // This is the discriminating boundary: a fallback-to-inferred design would still
    // return Some by minting a single-file project; project-bound returns None.
    let snap_absent = fake_snapshot(tsconfig, &["/ws/src/Other.ts"]);
    assert_eq!(
        select_configured_project_carrier(&snap_absent, tsconfig, carrier),
        None,
        "a carrier ABSENT from the configured project's root_files is None — NOT a \
         fallback to an inferred/single-file project"
    );

    // (3) No project matches the tsconfig ⇒ None (the configured project gate fails
    // closed; never serve a different / inferred project).
    let snap_wrong_config = fake_snapshot("/other/tsconfig.json", &[carrier]);
    assert_eq!(
        select_configured_project_carrier(&snap_wrong_config, tsconfig, carrier),
        None,
        "no project matching the requested tsconfig is None — never a wrong-project / \
         inferred fallback"
    );

    // (4) The selected project is THE configured one for the tsconfig: a snapshot
    // whose only project is for a DIFFERENT config does not satisfy a request for
    // `tsconfig`, even though that project DOES contain the carrier.
    assert!(
        select_configured_project_carrier(&snap_wrong_config, tsconfig, carrier).is_none(),
        "membership is checked against the project SELECTED BY the requested tsconfig, \
         not any project that happens to contain the carrier"
    );
}

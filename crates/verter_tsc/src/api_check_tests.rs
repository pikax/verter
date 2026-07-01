//! Unit tests for the `--api` diagnostic mapping (`map_one`) — the pure
//! offset → (line,col) → source-map remap + filtering logic, exercised without a
//! live engine. The end-to-end engine path is covered by the Rail B parity
//! oracle (`tests/diagnostic_set_parity.rs`).

use std::collections::HashMap;

use super::*;
use verter_tsgo_api::proto::types::Diagnostic as ApiDiagnostic;

fn api_diag(code: u32, category: u32, text: &str, pos: u32, file: &str) -> ApiDiagnostic {
    ApiDiagnostic {
        code,
        category,
        text: text.to_string(),
        pos,
        end: pos,
        file_name: Some(file.to_string()),
    }
}

fn lookup_of(files: &[OverlayFile]) -> HashMap<String, &OverlayFile> {
    files.iter().map(|f| (norm_key(&f.path), f)).collect()
}

/// A `NativeFs` for tests that need one to satisfy `map_one`'s disk-read arg. The
/// unit tests here exercise overlay-carrier + global cases (no real-disk read),
/// so an empty-project FS is sufficient; the real-disk non-root read path is
/// covered end-to-end by the Rail B parity oracle.
fn empty_disk() -> NativeFs {
    NativeFs::new()
}

#[test]
fn passthrough_stub_keeps_file_and_converts_offset() {
    let stub = OverlayFile {
        path: "/proj/Foo_00ab.vue.ts".to_string(),
        content: "export const x: string = 1;\n".to_string(),
        remap: RemapKind::Passthrough,
    };
    let files = vec![stub];
    let lookup = lookup_of(&files);

    // Byte offset of the `1` literal (ASCII single line ⇒ col == offset + 1).
    let pos = files[0].content.find('1').unwrap() as u32;
    let d = api_diag(
        2322,
        1,
        "Type 'number' is not assignable to type 'string'.",
        pos,
        "/proj/Foo_00ab.vue.ts",
    );

    let mapped = map_one(&d, &lookup, &empty_disk()).expect("stub diag maps");
    assert_eq!(mapped.file, "/proj/Foo_00ab.vue.ts");
    assert_eq!(mapped.line, 1);
    assert_eq!(mapped.col, pos + 1);
    assert_eq!(mapped.ts_code, 2322);
    assert!(matches!(mapped.severity, Severity::Error));
    assert!(mapped.message.contains("not assignable"));
}

#[test]
fn suggestion_and_message_categories_are_dropped() {
    let stub = OverlayFile {
        path: "/proj/Foo.vue.ts".to_string(),
        content: "const x = 1;\n".to_string(),
        remap: RemapKind::Passthrough,
    };
    let files = vec![stub];
    let lookup = lookup_of(&files);

    // category 2 = suggestion, 3 = message — never printed by tsgo --project.
    let sug = api_diag(
        6133,
        2,
        "'x' is declared but never used.",
        6,
        "/proj/Foo.vue.ts",
    );
    assert!(map_one(&sug, &lookup, &empty_disk()).is_none());
    let msg = api_diag(4114, 3, "some message", 6, "/proj/Foo.vue.ts");
    assert!(map_one(&msg, &lookup, &empty_disk()).is_none());
}

#[test]
fn warning_category_maps_to_warning_severity() {
    let stub = OverlayFile {
        path: "/proj/Foo.vue.ts".to_string(),
        content: "const x = 1;\n".to_string(),
        remap: RemapKind::Passthrough,
    };
    let files = vec![stub];
    let lookup = lookup_of(&files);

    let warn = api_diag(
        6133,
        0,
        "'x' is declared but never used.",
        6,
        "/proj/Foo.vue.ts",
    );
    let mapped = map_one(&warn, &lookup, &empty_disk()).expect("warning maps");
    assert!(matches!(mapped.severity, Severity::Warning));
}

#[test]
fn vue_jsx_type_gap_children_is_suppressed() {
    let tsx = OverlayFile {
        path: "/proj/Foo.tsx".to_string(),
        content: "const a = 1;\n".to_string(),
        remap: RemapKind::Passthrough,
    };
    let files = vec![tsx];
    let lookup = lookup_of(&files);

    // A TS2322 about `children` against Vue's HTMLAttributes is a known gap.
    let gap = api_diag(
        2322,
        1,
        "Property 'children' does not exist on type 'HTMLAttributes'.",
        6,
        "/proj/Foo.tsx",
    );
    assert!(
        map_one(&gap, &lookup, &empty_disk()).is_none(),
        "the children/HTMLAttributes gap must be suppressed"
    );
}

#[test]
fn sourcemapped_carrier_without_map_falls_back_to_vue_at_one_one() {
    // A SourceMapped TSX carrier with NO inline source map: the position cannot
    // be remapped, so the diagnostic reports at the .vue source line 1, col 1.
    let tsx = OverlayFile {
        path: "/proj/Foo_dead.tsx".to_string(),
        content: "const a: string = 1;\n".to_string(),
        remap: RemapKind::SourceMapped {
            vue_path: "/proj/src/Foo.vue".to_string(),
        },
    };
    let files = vec![tsx];
    let lookup = lookup_of(&files);

    let d = api_diag(
        2322,
        1,
        "Type 'number' is not assignable to type 'string'.",
        10,
        "/proj/Foo_dead.tsx",
    );
    let mapped = map_one(&d, &lookup, &empty_disk()).expect("maps with fallback");
    assert_eq!(mapped.file, "/proj/src/Foo.vue");
    assert_eq!(mapped.line, 1);
    assert_eq!(mapped.col, 1);
    assert_eq!(mapped.ts_code, 2322);
}

#[test]
fn global_diagnostic_without_file_name_is_surfaced_not_dropped() {
    // A global / compiler-options diagnostic carries no `file_name`. In
    // whole-program mode it must be RETAINED (surfaced at a synthetic position),
    // never dropped — a bad-`target` (TS6046) would otherwise vanish.
    let files: Vec<OverlayFile> = vec![];
    let lookup = lookup_of(&files);

    let mut d = api_diag(6046, 1, "Argument for '--target' option must be ...", 0, "");
    d.file_name = None;
    let mapped =
        map_one(&d, &lookup, &empty_disk()).expect("a global (no-file) diagnostic is surfaced");
    assert_eq!(mapped.ts_code, 6046);
    assert_eq!(mapped.file, "<compiler options>");
    assert_eq!(mapped.line, 1);
    assert_eq!(mapped.col, 1);
}

#[test]
fn non_root_real_file_diagnostic_is_surfaced_under_its_own_path_not_a_carrier() {
    // WHOLE-PROGRAM ATTRIBUTION (discriminating). A SourceMapped `.vue` carrier is
    // in the overlay; the engine reports a diagnostic whose `file_name` is a
    // DIFFERENT, real, non-root imported `.ts` we did NOT generate a carrier for.
    // In whole-program mode it MUST be surfaced under its OWN path (a passthrough),
    // NEVER re-homed onto the carrier (which would remap the unrelated file's
    // UTF-16 offset through the WRONG carrier's source map).
    //
    // RED before the whole-program change: `map_one` DROPPED a present-but-unknown
    // `file_name` (the old root-attribution invariant), so this returned `None`
    // and the non-root error vanished — exactly the rootscope gap. GREEN after:
    // the diagnostic is surfaced at its own path.
    let tsx = OverlayFile {
        path: "/proj/Foo_ab12.tsx".to_string(),
        content: "const a: string = 1;\n".to_string(),
        remap: RemapKind::SourceMapped {
            vue_path: "/proj/src/Foo.vue".to_string(),
        },
    };
    let files = vec![tsx];
    let lookup = lookup_of(&files);

    // A present `file_name` that is NOT one of our overlay carriers (a real
    // non-root imported `.ts`, not on disk in this unit test ⇒ (1,1) fallback).
    let d = api_diag(
        2322,
        1,
        "Type 'number' is not assignable to type 'string'.",
        6,
        "/proj/src/imported-types.ts",
    );
    let mapped = map_one(&d, &lookup, &empty_disk())
        .expect("a real non-root file diagnostic is surfaced (whole-program), not dropped");
    assert_eq!(
        mapped.file, "/proj/src/imported-types.ts",
        "the non-root diagnostic is homed on its OWN path, never re-attributed to the carrier"
    );
    assert_ne!(
        mapped.file, "/proj/src/Foo.vue",
        "it must NOT be re-homed onto the carrier's .vue source"
    );
    assert_eq!(mapped.ts_code, 2322);
}

// ── Offset-conversion regression coverage ────────────────────────────────────
//
// verter-tsc converts each `--api` diagnostic's UTF-16 code-unit `pos` to a
// 1-based `(line, col)` (with a UTF-16 column) through the SHARED
// `verter_tsgo_api::api_offset_to_line_col` boundary — there is no verter-tsc-local
// offset walk. These pin the conversion semantics the inline-source-map remap
// depends on (a wrong line/col would remap through the wrong source-map token).
// They exercise the shared entry the crate now calls, keeping the CRLF / non-ASCII
// edge coverage that guarded the retired local `offset_map` here at the consuming
// layer.

use verter_tsgo_api::api_offset_to_line_col;

#[test]
fn shared_offset_conversion_ascii_and_multiline() {
    // "abc\ndef": offset 5 is 'e' on line 2, col 2; offset 4 is 'd' at line 2 col 1.
    let s = "abc\ndef";
    assert_eq!(api_offset_to_line_col(s, 5), (2, 2));
    assert_eq!(api_offset_to_line_col(s, 4), (2, 1));
    assert_eq!(api_offset_to_line_col(s, 0), (1, 1));
}

#[test]
fn shared_offset_conversion_em_dash_is_utf16_not_byte() {
    // The generated carriers carry em-dashes (U+2014: 3 UTF-8 bytes / 1 UTF-16
    // unit) in comments; a byte reading would drift the line. "a—b\ncd": UTF-16
    // units a(0) —(1) b(2) \n(3) c(4) d(5).
    let s = "a\u{2014}b\ncd";
    assert_eq!(api_offset_to_line_col(s, 4), (2, 1)); // 'c' — line 2, not line 1
    assert_eq!(api_offset_to_line_col(s, 2), (1, 3)); // 'b'
}

#[test]
fn shared_offset_conversion_crlf_is_single_terminator() {
    // Windows carriers use `\r\n`, which TypeScript treats as ONE terminator:
    // the char after `\r\n` is line 2 col 1, NOT line 3. "ab\r\ncd": units
    // a(0) b(1) \r(2) \n(3) c(4) d(5).
    let s = "ab\r\ncd";
    assert_eq!(api_offset_to_line_col(s, 2), (1, 3)); // at `\r`, still line 1
    assert_eq!(api_offset_to_line_col(s, 3), (1, 4)); // at `\n`, still line 1
    assert_eq!(api_offset_to_line_col(s, 4), (2, 1)); // after `\r\n`, line 2
}

#[test]
fn shared_offset_conversion_supplementary_pair_and_clamp() {
    // A supplementary-plane char is 2 UTF-16 units; a past-end offset clamps.
    assert_eq!(api_offset_to_line_col("\u{10437}x", 2), (1, 3)); // 'x' after the pair
    assert_eq!(api_offset_to_line_col("abc", 999), (1, 4)); // past-end → final col
}

// ── Whole-program config-diagnostic filtering (FAIL-CLOSED invariant) ─────────
//
// The whole-program path applies `strip_injected_root_diagnostics` to the
// config-parse stream with the injected-companion set (the generated carriers +
// the synthetic tsconfig). The FAIL-CLOSED invariant: ONLY a diagnostic whose
// `file_name` is a KNOWN injected companion may be dropped; a real user-config
// error, a real non-root file diagnostic, and a `fileName:None` global/options
// diagnostic are ALL retained (never silently dropped). This pins that verter-tsc
// passes the right injected set and never over-filters.

#[test]
fn config_filtering_drops_only_injected_companions_and_retains_real_and_global() {
    let injected_tsx = "/proj/Foo_ab12.vue.tsx".to_string();
    let injected_stub = "/proj/Foo_ab12.vue.ts".to_string();
    let synthetic_tsconfig = "/proj/verter-tsc-check.tsconfig.json".to_string();
    let injected_paths = vec![
        injected_tsx.clone(),
        injected_stub.clone(),
        synthetic_tsconfig.clone(),
    ];

    // A config diagnostic pointing at an injected companion (a virtualization
    // artifact) — MUST be dropped.
    let on_injected_tsx = api_diag(6059, 1, "File is not under 'rootDir'", 0, &injected_tsx);
    // A config diagnostic pointing at the synthetic tsconfig itself — dropped.
    let on_synthetic_config = api_diag(
        18003,
        1,
        "No inputs were found in config file",
        0,
        &synthetic_tsconfig,
    );
    // A REAL user-config error (points at the user's own tsconfig) — RETAINED.
    let real_user_config = api_diag(
        5024,
        1,
        "Compiler option requires a value",
        0,
        "/proj/tsconfig.json",
    );
    // A REAL non-root source diagnostic — RETAINED.
    let real_source = api_diag(2322, 1, "not assignable", 0, "/proj/src/types.ts");
    // A GLOBAL options diagnostic (no fileName) — RETAINED.
    let mut global_opt = api_diag(6046, 1, "Argument for '--target' option", 0, "");
    global_opt.file_name = None;

    let filtered = strip_injected_root_diagnostics(
        vec![
            on_injected_tsx.clone(),
            on_synthetic_config.clone(),
            real_user_config.clone(),
            real_source.clone(),
            global_opt.clone(),
        ],
        &injected_paths,
    );

    // The two injected-companion config diagnostics are GONE.
    assert!(
        !filtered
            .iter()
            .any(|d| d.file_name.as_deref() == Some(injected_tsx.as_str())),
        "a config diagnostic on an injected carrier must be dropped: {filtered:?}"
    );
    assert!(
        !filtered
            .iter()
            .any(|d| d.file_name.as_deref() == Some(synthetic_tsconfig.as_str())),
        "a config diagnostic on the synthetic tsconfig must be dropped: {filtered:?}"
    );
    // The real user-config error, real source diagnostic, and global diagnostic
    // are ALL retained (fail-closed: never silently drop a real/global diagnostic).
    assert!(
        filtered.contains(&real_user_config),
        "a real user-config error MUST survive: {filtered:?}"
    );
    assert!(
        filtered.contains(&real_source),
        "a real non-root source diagnostic MUST survive: {filtered:?}"
    );
    assert!(
        filtered
            .iter()
            .any(|d| d.file_name.is_none() && d.code == 6046),
        "a global (fileName:None) options diagnostic MUST survive: {filtered:?}"
    );
    assert_eq!(
        filtered.len(),
        3,
        "exactly the two injected-companion diagnostics are removed, nothing else"
    );
}

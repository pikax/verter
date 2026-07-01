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

    let mapped = map_one(&d, "/proj/Foo_00ab.vue.ts", &lookup).expect("stub diag maps");
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
    assert!(map_one(&sug, "/proj/Foo.vue.ts", &lookup).is_none());
    let msg = api_diag(4114, 3, "some message", 6, "/proj/Foo.vue.ts");
    assert!(map_one(&msg, "/proj/Foo.vue.ts", &lookup).is_none());
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
    let mapped = map_one(&warn, "/proj/Foo.vue.ts", &lookup).expect("warning maps");
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
        map_one(&gap, "/proj/Foo.tsx", &lookup).is_none(),
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
    let mapped = map_one(&d, "/proj/Foo_dead.tsx", &lookup).expect("maps with fallback");
    assert_eq!(mapped.file, "/proj/src/Foo.vue");
    assert_eq!(mapped.line, 1);
    assert_eq!(mapped.col, 1);
    assert_eq!(mapped.ts_code, 2322);
}

#[test]
fn omitted_file_name_is_read_as_the_queried_root() {
    // A per-file getter may omit the (redundant) `file_name`; map_one reads the
    // diagnostic AS the queried root in that case.
    let stub = OverlayFile {
        path: "/proj/Foo.vue.ts".to_string(),
        content: "const x = 1;\n".to_string(),
        remap: RemapKind::Passthrough,
    };
    let files = vec![stub];
    let lookup = lookup_of(&files);

    // file_name omitted ⇒ read as the queried root.
    let mut d = api_diag(2304, 1, "Cannot find name.", 6, "/proj/Foo.vue.ts");
    d.file_name = None;
    let mapped = map_one(&d, "/proj/Foo.vue.ts", &lookup).expect("omitted file_name reads as root");
    assert_eq!(mapped.file, "/proj/Foo.vue.ts");
}

#[test]
fn diagnostic_for_unknown_non_root_file_is_not_misattributed_to_queried_root() {
    // ROOT-ATTRIBUTION INVARIANT (discriminating). The queried root is a
    // SourceMapped `.vue` carrier; the engine reports a diagnostic whose
    // `file_name` is a DIFFERENT file we did NOT generate a carrier for (an
    // imported `.ts`, a `node_modules` file, a global/options diagnostic). map_one
    // must DROP it — never re-home it onto the queried root (which would both
    // misattribute the file AND remap the unrelated file's UTF-16 offset through
    // the WRONG carrier's source map).
    //
    // RED before the fix: the old `.or_else(lookup.get(queried_root))` fallback
    // re-homed the unknown-file diagnostic onto the root carrier ⇒ `map_one`
    // returned `Some(diagnostic @ /proj/src/Foo.vue)` and this `.is_none()`
    // assertion FAILED. GREEN after: the fallback is removed, so the unknown file
    // resolves to no carrier ⇒ `None`.
    let tsx = OverlayFile {
        path: "/proj/Foo_ab12.tsx".to_string(),
        content: "const a: string = 1;\n".to_string(),
        remap: RemapKind::SourceMapped {
            vue_path: "/proj/src/Foo.vue".to_string(),
        },
    };
    let files = vec![tsx];
    let lookup = lookup_of(&files);

    // A present `file_name` that is NOT one of our overlay carriers.
    let d = api_diag(
        2322,
        1,
        "Type 'number' is not assignable to type 'string'.",
        6,
        "/proj/src/imported-types.ts",
    );
    assert!(
        map_one(&d, "/proj/Foo_ab12.tsx", &lookup).is_none(),
        "a diagnostic for a non-overlay imported/global file must NOT be misattributed to the \
         queried root — it must be dropped (root-attribution invariant)"
    );
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

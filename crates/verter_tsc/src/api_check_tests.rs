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
fn unknown_carrier_path_falls_back_to_queried_root() {
    // The engine echoes a path differing only in separator/case from the overlay
    // key; the normalized lookup still resolves it.
    let stub = OverlayFile {
        path: "/proj/Foo.vue.ts".to_string(),
        content: "const x = 1;\n".to_string(),
        remap: RemapKind::Passthrough,
    };
    let files = vec![stub];
    let lookup = lookup_of(&files);

    // file_name omitted ⇒ fall back to the queried root.
    let mut d = api_diag(2304, 1, "Cannot find name.", 6, "/proj/Foo.vue.ts");
    d.file_name = None;
    let mapped = map_one(&d, "/proj/Foo.vue.ts", &lookup).expect("falls back to queried root");
    assert_eq!(mapped.file, "/proj/Foo.vue.ts");
}

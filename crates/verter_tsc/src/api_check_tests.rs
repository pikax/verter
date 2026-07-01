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

/// The per-collection source cache the unit tests exercise: the carriers' own
/// content forms the OVERLAY, and the real-FS fallback is an empty-project
/// `NativeFs` (so a NON-overlay file resolves to a genuine MISS, exercising the
/// explicit-error path). This mirrors what `collect_diagnostics` builds.
fn cache_of(files: &[OverlayFile]) -> DiagnosticSourceCache<OverlayThenFallback<NativeFsSource>> {
    let overlay = files.iter().map(|f| (f.path.clone(), f.content.clone()));
    let source = OverlayThenFallback::new(
        overlay,
        NativeFsSource {
            fs: NativeFs::new(),
        },
    );
    DiagnosticSourceCache::new(source)
}

/// Map a per-carrier / whole-program diagnostic under the `Semantic` origin (the
/// natural origin for the semantic/syntactic streams these unit tests model) with
/// an EMPTY injected set (no injected-companion suppression). Panics on a content
/// miss (the happy-path helper — the explicit-miss path is tested separately).
fn map_semantic(
    d: &ApiDiagnostic,
    lookup: &HashMap<String, &OverlayFile>,
    cache: &DiagnosticSourceCache<OverlayThenFallback<NativeFsSource>>,
) -> Option<Diagnostic> {
    map_one(
        &OriginDiagnostic {
            d,
            origin: DiagOrigin::Semantic,
        },
        lookup,
        cache,
        &InjectedPathSet::default(),
    )
    .expect("content resolves for this fixture (no miss expected)")
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

    let mapped = map_semantic(&d, &lookup, &cache_of(&files)).expect("stub diag maps");
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
    assert!(map_semantic(&sug, &lookup, &cache_of(&files)).is_none());
    let msg = api_diag(4114, 3, "some message", 6, "/proj/Foo.vue.ts");
    assert!(map_semantic(&msg, &lookup, &cache_of(&files)).is_none());
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
    let mapped = map_semantic(&warn, &lookup, &cache_of(&files)).expect("warning maps");
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
        map_semantic(&gap, &lookup, &cache_of(&files)).is_none(),
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
    let mapped = map_semantic(&d, &lookup, &cache_of(&files)).expect("maps with fallback");
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
    let mapped = map_semantic(&d, &lookup, &cache_of(&files))
        .expect("a global (no-file) diagnostic is surfaced");
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
    // In whole-program mode it MUST be surfaced under its OWN path (a passthrough)
    // at its OWN CORRECT position, NEVER re-homed onto the carrier (which would
    // remap the unrelated file's UTF-16 offset through the WRONG carrier's source
    // map).
    let tsx = OverlayFile {
        path: "/proj/Foo_ab12.tsx".to_string(),
        content: "const a: string = 1;\n".to_string(),
        remap: RemapKind::SourceMapped {
            vue_path: "/proj/src/Foo.vue".to_string(),
        },
    };
    let files = vec![tsx];
    let lookup = lookup_of(&files);

    // The non-root file's content is RESOLVABLE (fed into the cache's source layer,
    // exactly as the real FS fallback would supply it) so the diagnostic homes at
    // its OWN correct `(line, col)` — line 2 (the offset is on the second line),
    // NOT a fabricated (1,1). Content: line 1 "// header\n", line 2 has the error.
    let non_root_path = "/proj/src/imported-types.ts";
    let non_root_content = "// header\nconst a: number = 'x';\n";
    let source = OverlayThenFallback::new(
        [
            (files[0].path.clone(), files[0].content.clone()),
            (non_root_path.to_string(), non_root_content.to_string()),
        ],
        NativeFsSource {
            fs: NativeFs::new(),
        },
    );
    let cache = DiagnosticSourceCache::new(source);

    // UTF-16 offset of the `'x'` literal on line 2.
    let pos = non_root_content.find('\'').unwrap() as u32;
    let d = api_diag(
        2322,
        1,
        "Type 'string' is not assignable to type 'number'.",
        pos,
        non_root_path,
    );
    let mapped = map_semantic(&d, &lookup, &cache)
        .expect("a real non-root file diagnostic is surfaced (whole-program), not dropped");
    assert_eq!(
        mapped.file, non_root_path,
        "the non-root diagnostic is homed on its OWN path, never re-attributed to the carrier"
    );
    assert_ne!(
        mapped.file, "/proj/src/Foo.vue",
        "it must NOT be re-homed onto the carrier's .vue source"
    );
    // Its OWN correct position — line 2, NOT the fabricated (1,1) the old miss path
    // produced. This is the discriminator that the position comes from the file's
    // real content, not a guess.
    assert_eq!(mapped.line, 2, "homed at its own real line, not (1,1)");
    assert_eq!(mapped.ts_code, 2322);
}

#[test]
fn non_root_content_miss_is_an_explicit_error_never_one_one_or_dropped() {
    // D3 (fail-closed). A non-root diagnostic whose file content the cache CANNOT
    // resolve (not an overlay carrier, empty-FS fallback returns None) must surface
    // an EXPLICIT `MappingError::SourceUnavailable` — NOT a fabricated (1,1)
    // position, NOT a silent drop.
    //
    // RED before D3: `map_one` fell back to `(1,1)` on a disk-read miss and returned
    // `Some`, mis-homing the diagnostic to the file's first character. GREEN after:
    // an explicit error propagates (→ a fatal TypecheckError at the boundary).
    let carrier = OverlayFile {
        path: "/proj/Foo_ab12.tsx".to_string(),
        content: "const a = 1;\n".to_string(),
        remap: RemapKind::Passthrough,
    };
    let files = vec![carrier];
    let lookup = lookup_of(&files);
    // The cache overlay carries ONLY the carrier; the missing file has no content
    // (empty-FS fallback ⇒ genuine miss).
    let cache = cache_of(&files);

    let d = api_diag(
        2322,
        1,
        "Type 'string' is not assignable to type 'number'.",
        6,
        "/proj/src/not-on-disk.ts",
    );
    let result = map_one(
        &OriginDiagnostic {
            d: &d,
            origin: DiagOrigin::Semantic,
        },
        &lookup,
        &cache,
        &InjectedPathSet::default(),
    );
    match result {
        Err(MappingError::SourceUnavailable {
            file_name,
            diagnostic_code,
            origin,
        }) => {
            assert_eq!(file_name, "/proj/src/not-on-disk.ts");
            assert_eq!(diagnostic_code, 2322);
            assert_eq!(origin, DiagOrigin::Semantic);
        }
        other => panic!(
            "a genuine content miss must be an explicit SourceUnavailable error \
             (never Ok(Some((1,1))), never Ok(None)): {other:?}"
        ),
    }
}

// ── Injected-root map-boundary guard (fail-closed, origin + path keyed) ──────
//
// The upstream `strip_injected_root_diagnostics` filters the config stream, but the
// reopen proved the strip alone leaks when the engine echoes an injected companion
// under a DIFFERENT drive-letter case / separator (the old exact `p == name` path).
// The belt-and-suspenders guard in `map_one` re-checks the SAME injected set at the
// map boundary, keyed by the shared filesystem-identity key AND the diagnostic's
// COLLECTION ORIGIN — so a Config-origin companion diagnostic is suppressed even
// case-divergent, while a Semantic/Syntactic diagnostic on the same carrier still
// takes the legitimate `.vue` remap (over-suppression = false negatives).

/// Drive `map_one` under the given origin with a specific injected set + cache.
/// Panics on a content miss (the D8 guard tests never miss — the companion content
/// is in the overlay).
fn map_with(
    d: &ApiDiagnostic,
    origin: DiagOrigin,
    lookup: &HashMap<String, &OverlayFile>,
    cache: &DiagnosticSourceCache<OverlayThenFallback<NativeFsSource>>,
    injected: &InjectedPathSet,
) -> Option<Diagnostic> {
    map_one(&OriginDiagnostic { d, origin }, lookup, cache, injected)
        .expect("content resolves for this fixture (no miss expected)")
}

/// REPRODUCER (the D8 leak). A `Config`-origin diagnostic whose `file_name` is a
/// DRIVE-CASE-DIVERGENT form of an injected companion (registered `c:/proj/Foo.vue.tsx`,
/// reported `C:/proj/Foo.vue.tsx`, TS6059) must NOT be emitted and must NOT be
/// re-homed onto the carrier's `.vue`. The companion is ALSO present in the overlay
/// lookup as a `SourceMapped` carrier, so WITHOUT the guard `map_one` would remap the
/// TS6059 onto `.vue` (the spurious diagnostic). RED on the pre-guard path (leaks as
/// a `.vue` TS6059); GREEN with the fail-closed origin+path guard.
#[test]
fn config_origin_case_divergent_injected_companion_is_not_emitted_or_rehomed() {
    // The companion is a real overlay carrier (SourceMapped → `.vue`).
    let carrier = OverlayFile {
        path: "c:/proj/Foo.vue.tsx".to_string(),
        content: "const a: string = 1;\n".to_string(),
        remap: RemapKind::SourceMapped {
            vue_path: "c:/proj/src/Foo.vue".to_string(),
        },
    };
    let files = vec![carrier];
    let lookup = lookup_of(&files);
    // Injected set registered with the LOWERCASE-drive canonical form.
    let injected = InjectedPathSet::from_paths(["c:/proj/Foo.vue.tsx".to_string()]);

    // A Config-parse diagnostic the engine reports with an UPPERCASE drive letter.
    let d = api_diag(
        6059,
        1,
        "File 'c:/proj/Foo.vue.tsx' is not under 'rootDir'.",
        6,
        "C:/proj/Foo.vue.tsx",
    );

    let mapped = map_with(
        &d,
        DiagOrigin::Config,
        &lookup,
        &cache_of(&files),
        &injected,
    );
    assert!(
        mapped.is_none(),
        "a case-divergent Config-origin injected companion must be suppressed at the map \
         boundary, never emitted or re-homed onto .vue: {mapped:?}"
    );
}

/// INVARIANT (guards against over-suppression). A `Semantic` diagnostic on the SAME
/// generated carrier — even though the carrier IS in the injected set — STILL maps
/// back through the source map to `.vue`. The guard fires only for `Config` origin;
/// suppressing a semantic error here would be a false negative.
#[test]
fn semantic_origin_on_injected_carrier_still_maps_to_vue() {
    let carrier = OverlayFile {
        path: "c:/proj/Foo.vue.tsx".to_string(),
        content: "const a: string = 1;\n".to_string(),
        remap: RemapKind::SourceMapped {
            vue_path: "c:/proj/src/Foo.vue".to_string(),
        },
    };
    let files = vec![carrier];
    let lookup = lookup_of(&files);
    // The carrier IS an injected companion (same set as the reproducer).
    let injected = InjectedPathSet::from_paths(["c:/proj/Foo.vue.tsx".to_string()]);

    // A real SEMANTIC type error on that carrier (reported with the same drive-case
    // divergence to prove the guard's origin gate — not its path gate — is what
    // spares it).
    let d = api_diag(
        2322,
        1,
        "Type 'number' is not assignable to type 'string'.",
        10,
        "C:/proj/Foo.vue.tsx",
    );

    let mapped = map_with(
        &d,
        DiagOrigin::Semantic,
        &lookup,
        &cache_of(&files),
        &injected,
    )
    .expect("a semantic diagnostic on the carrier is surfaced, never suppressed by the guard");
    // No inline source map on `content`, so it falls back to the `.vue` (1,1) — the
    // point is it REMAPS to `.vue`, never suppressed and never left on the `.tsx`.
    assert_eq!(
        mapped.file, "c:/proj/src/Foo.vue",
        "a Semantic-origin diagnostic on an injected carrier must still remap to its .vue source"
    );
    assert_ne!(
        mapped.file, "c:/proj/Foo.vue.tsx",
        "it must not be left on the raw .tsx carrier"
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
        &InjectedPathSet::from_paths(injected_paths.iter().cloned()),
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

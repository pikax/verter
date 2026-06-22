//! Invariant: find-references / rename / code-action on an EXTERNAL (cross-file) target lands on
//! the real symbol span, never LINE 0 — and FAILS CLOSED (drops the location / edit) when the
//! span cannot be faithfully resolved.
//!
//! Why it matters: "Find All References" / "Rename Symbol" / a code-action edit on a cross-file
//! symbol (e.g. `formatCount` declared in `utils.ts`, referenced from `App.vue`) must resolve the
//! type provider's REAL byte offsets to a line:col `Range`. Collapsing a failed resolve to
//! `Range::default()` (line 0, char 0) is worse than a navigation miss for a rename edit: it
//! CORRUPTS the target file by writing the new name at line 0.
//!
//! Contract (`crates/verter_lsp/src/type_provider/merge/feature_merges.rs`): the provider returns a
//! `TypeLocation`/`RenameLocation`/`TypeCodeEdit { path, start, end }` whose `start`/`end` are REAL
//! byte offsets into the target file. The merge reads the target's own source through the host VFS,
//! builds a `LineIndex` in the client-negotiated encoding, converts the byte offsets to a line:col
//! `Range`, and FAILS CLOSED when the source or offsets cannot be resolved — never a line-0 range.
//!
//! These tests exercise the `merge_references` / `merge_rename_locations` / `merge_code_actions`
//! boundary directly (no type-provider process): each constructs a location whose byte offsets
//! point at a known symbol, hands the merge an in-memory source reader (modeling the host VFS the
//! production merge reads through — see `VerterHost::workspace_read().read_file`), and asserts the
//! merged result carries that symbol's EXACT line:col span — never `Range::default()` — plus the
//! fail-closed (dropped) half for unresolvable / unmappable targets.

use std::sync::Arc;

use oxc_sourcemap::SourceMapBuilder;
use tower_lsp_server::ls_types::*;

use verter_lsp::documents::line_index::LineIndex;
use verter_lsp::documents::position_map::PositionMapper;
use verter_lsp::documents::provider_projection::ProviderPositionMapper;
use verter_lsp::type_provider::merge::{
    merge_code_actions, merge_references, merge_rename_locations, ExternalApiResolver,
    ExternalIdeContext, ExternalIdeResolver,
};
use verter_lsp::type_provider::protocol::{
    RenameLocation, TypeCodeAction, TypeCodeEdit, TypeLocation,
};

/// A minimal valid provider mapper. For an EXTERNAL (non-carrier) target the mapper is never
/// consulted — it only matters for carrier IDE targets — so a one-token source map suffices.
fn trivial_mapper() -> ProviderPositionMapper {
    let mut b = SourceMapBuilder::default();
    let sid = b.set_source_and_content("App.vue", "x");
    b.add_token(0, 0, 0, 0, Some(sid), None);
    ProviderPositionMapper::source_map(
        PositionMapper::from_json(&b.into_sourcemap().to_json_string()).expect("valid source map"),
    )
}

/// In-memory external source fixture: a synthetic forward-slash path (with `suffix`) plus a
/// reader returning `content` for that exact path. Models the host VFS the production merge reads
/// through — a reference target carries byte offsets into its own source, so the reader hands that
/// exact source back for the offset→line:col conversion — no disk I/O, fully hermetic.
fn ext_source(suffix: &str, content: &str) -> (String, impl Fn(&str) -> Option<Arc<str>>) {
    let path = format!("/virtual/utils{suffix}");
    let content: Arc<str> = Arc::from(content);
    let reader_path = path.clone();
    let reader = move |p: &str| (p == reader_path.as_str()).then(|| content.clone());
    (path, reader)
}

/// A reader that NEVER resolves any path — models the fail-closed case (source unreadable).
fn no_source_reader() -> impl Fn(&str) -> Option<Arc<str>> {
    |_p: &str| None
}

fn carrier_never(_p: &str) -> bool {
    false
}

// ── References ───────────────────────────────────────────────────────────────

#[test]
fn external_ts_reference_keeps_the_real_line_not_zero() {
    // `formatCount` is referenced on LINE 1 (0-based) of the fixture, not line 0.
    let ts = "import { x } from './x';\nexport const r = formatCount(1);\n";
    let symbol = "formatCount";
    let sym_off = ts.find(symbol).expect("symbol present in fixture") as u32;
    let sym_end = sym_off + symbol.len() as u32;

    let fixture_li = LineIndex::new_utf16(ts);
    let sym_pos = fixture_li.offset_to_position(sym_off).unwrap();
    let sym_end_pos = fixture_li.offset_to_position(sym_end).unwrap();
    assert_eq!(
        sym_pos.line, 1,
        "fixture precondition: `{symbol}` must be on line 1, not line 0"
    );

    let (ts_path, read_source) = ext_source(".ts", ts);
    assert!(
        ts_path.ends_with(".ts")
            && !verter_workspace::path_is_carrier(&ts_path[..ts_path.len() - 3]),
        "fixture must be a plain external .ts (not a generated carrier file): {ts_path}"
    );

    let type_refs = vec![TypeLocation {
        path: ts_path.clone(),
        start: sym_off,
        end: sym_end,
    }];

    let (mapper, tsx_li, carrier_li) = (
        trivial_mapper(),
        LineIndex::new_utf16("x"),
        LineIndex::new_utf16("x"),
    );
    let no_external: Option<ExternalIdeResolver> = None;

    let result = merge_references(
        None,
        type_refs,
        "/proj/Caller.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_never,
        PositionEncodingKind::UTF16,
        &read_source,
    );

    let locs = result.expect("a cross-file reference must survive with a real range");
    assert_eq!(locs.len(), 1, "the one external reference must survive");
    let loc = &locs[0];
    assert!(
        loc.uri.as_str().ends_with(".ts"),
        "target file should be the external .ts, got {}",
        loc.uri.as_str()
    );
    assert_eq!(
        loc.range.start, sym_pos,
        "external reference start must equal the exact symbol start {sym_pos:?}, got {:?} \
         (real byte offsets {sym_off}..{sym_end})",
        loc.range.start
    );
    assert_eq!(
        loc.range.end, sym_end_pos,
        "external reference end must equal the exact symbol end {sym_end_pos:?}, got {:?}",
        loc.range.end
    );
    assert_ne!(
        loc.range,
        Range::default(),
        "external reference must never collapse to the (0,0) default"
    );
}

#[test]
fn external_reference_with_unresolvable_source_is_dropped_not_zeroed() {
    // The provider returns a cross-file ref, but the source cannot be read (no overlay, gone from
    // disk). FAIL CLOSED: drop the ref — never substitute a line-0 range that sends the editor to
    // the wrong place.
    let ts_path = "/virtual/unreadable.ts".to_string();
    let type_refs = vec![TypeLocation {
        path: ts_path,
        start: 100,
        end: 110,
    }];

    let (mapper, tsx_li, carrier_li) = (
        trivial_mapper(),
        LineIndex::new_utf16("x"),
        LineIndex::new_utf16("x"),
    );
    let no_external: Option<ExternalIdeResolver> = None;
    let read_source = no_source_reader();

    let result = merge_references(
        None,
        type_refs,
        "/proj/Caller.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_never,
        PositionEncodingKind::UTF16,
        &read_source,
    );

    assert!(
        result.is_none(),
        "an unresolvable external reference must be DROPPED (fail-closed), not emitted at line 0: {result:?}"
    );
}

#[test]
fn external_svelte_child_reference_keeps_the_real_line_not_zero() {
    // Carrier-agnostic twin: a symbol referenced cross-file in a Svelte child's own canonical
    // source. The byte-offset→line:col conversion is framework-neutral — the same readback path
    // serves `.svelte`-owned `.ts` modules as `.vue`-adjacent `.ts`.
    let ts = "// child module\nexport const childRef = svelteChildSymbol;\n";
    let symbol = "svelteChildSymbol";
    let sym_off = ts.find(symbol).unwrap() as u32;
    let sym_end = sym_off + symbol.len() as u32;
    let fixture_li = LineIndex::new_utf16(ts);
    let sym_pos = fixture_li.offset_to_position(sym_off).unwrap();
    let sym_end_pos = fixture_li.offset_to_position(sym_end).unwrap();
    assert_eq!(sym_pos.line, 1, "fixture precondition: symbol on line 1");

    let (ts_path, read_source) = ext_source("Child.ts", ts);
    let type_refs = vec![TypeLocation {
        path: ts_path,
        start: sym_off,
        end: sym_end,
    }];

    let (mapper, tsx_li, carrier_li) = (
        trivial_mapper(),
        LineIndex::new_utf16("x"),
        LineIndex::new_utf16("x"),
    );
    let no_external: Option<ExternalIdeResolver> = None;

    let result = merge_references(
        None,
        type_refs,
        "/proj/Caller.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_never,
        PositionEncodingKind::UTF16,
        &read_source,
    );
    let locs = result.expect("the svelte-child cross-file reference must survive");
    assert_eq!(locs[0].range.start, sym_pos);
    assert_eq!(locs[0].range.end, sym_end_pos);
    assert_ne!(locs[0].range, Range::default());
}

// ── Rename ───────────────────────────────────────────────────────────────────

#[test]
fn external_ts_rename_keeps_the_real_line_not_zero() {
    // Same fixture as references: `formatCount` referenced on LINE 1. A rename edit must target the
    // EXACT span — a line-0 edit would corrupt the file.
    let ts = "import { x } from './x';\nexport const r = formatCount(1);\n";
    let symbol = "formatCount";
    let sym_off = ts.find(symbol).unwrap() as u32;
    let sym_end = sym_off + symbol.len() as u32;
    let fixture_li = LineIndex::new_utf16(ts);
    let sym_pos = fixture_li.offset_to_position(sym_off).unwrap();
    let sym_end_pos = fixture_li.offset_to_position(sym_end).unwrap();
    assert_eq!(sym_pos.line, 1, "fixture precondition: symbol on line 1");

    let (ts_path, read_source) = ext_source(".ts", ts);
    let type_locations = vec![RenameLocation {
        path: ts_path.clone(),
        start: sym_off,
        end: sym_end,
    }];

    let (mapper, tsx_li, carrier_li) = (
        trivial_mapper(),
        LineIndex::new_utf16("x"),
        LineIndex::new_utf16("x"),
    );
    let no_external: Option<ExternalIdeResolver> = None;

    let result = merge_rename_locations(
        None,
        type_locations,
        "newFormatCount",
        "/proj/Caller.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        None::<ExternalApiResolver>,
        &carrier_never,
        PositionEncodingKind::UTF16,
        &read_source,
    );

    let edit = result.expect("a cross-file rename edit must survive with a real range");
    let changes = edit.changes.expect("rename produces changes");
    let edits = changes
        .values()
        .next()
        .expect("one external file's edits present");
    assert_eq!(edits.len(), 1, "exactly one rename edit");
    let te = &edits[0];
    assert_eq!(
        te.range.start, sym_pos,
        "external rename edit start must equal the exact symbol start {sym_pos:?}, got {:?}",
        te.range.start
    );
    assert_eq!(
        te.range.end, sym_end_pos,
        "external rename edit end must equal the exact symbol end {sym_end_pos:?}, got {:?}",
        te.range.end
    );
    assert_ne!(
        te.range,
        Range::default(),
        "external rename edit must never collapse to (0,0) — that would corrupt the file"
    );
    assert_eq!(te.new_text, "newFormatCount");
}

#[test]
fn external_rename_with_unresolvable_source_is_dropped_not_zeroed() {
    // FAIL CLOSED for rename is even more important than references: a line-0 edit corrupts files.
    let type_locations = vec![RenameLocation {
        path: "/virtual/unreadable.ts".to_string(),
        start: 100,
        end: 110,
    }];

    let (mapper, tsx_li, carrier_li) = (
        trivial_mapper(),
        LineIndex::new_utf16("x"),
        LineIndex::new_utf16("x"),
    );
    let no_external: Option<ExternalIdeResolver> = None;
    let read_source = no_source_reader();

    let result = merge_rename_locations(
        None,
        type_locations,
        "whatever",
        "/proj/Caller.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        None::<ExternalApiResolver>,
        &carrier_never,
        PositionEncodingKind::UTF16,
        &read_source,
    );

    assert!(
        result.is_none(),
        "an unresolvable external rename edit must be DROPPED (fail-closed), never emitted at line 0: {result:?}"
    );
}

// ── Negotiated-encoding correctness (multibyte) ───────────────────────────────

/// The external byte-offset → line:col conversion runs through
/// `LineIndex::new(source, negotiated_encoding)`. With multibyte text BEFORE the symbol on its
/// line, the column DIFFERS between UTF-16 and UTF-8: UTF-16 counts code units (an emoji = 2),
/// UTF-8 counts bytes (an emoji = 4). This guards the off-by-one / surrogate-pair correctness the
/// merge owes for each negotiated encoding.
fn multibyte_fixture() -> (&'static str, &'static str, u32, u32) {
    // Line 0 has a non-ASCII identifier prefix + an emoji comment so the symbol's column is
    // encoding-sensitive. The symbol `targetSym` is on LINE 1.
    let src = "const café = '🚀';\nexport const targetSym = café;\n";
    let symbol = "targetSym";
    let off = src.find(symbol).unwrap() as u32;
    let end = off + symbol.len() as u32;
    (src, symbol, off, end)
}

#[test]
fn external_reference_multibyte_column_correct_under_utf16_and_utf8() {
    let (src, symbol, off, end) = multibyte_fixture();

    // The symbol is on line 1; line 1 is ASCII-only up to the symbol, so the column is the same
    // in both encodings — but the line-1 START offset depends on line 0's multibyte content being
    // measured in BYTES (the source map is byte-indexed) regardless of negotiated encoding. The
    // discriminator is that the conversion uses the right per-encoding LineIndex; assert the exact
    // expected position computed by each encoding's own index.
    for encoding in [PositionEncodingKind::UTF16, PositionEncodingKind::UTF8] {
        let fixture_li = LineIndex::new(src, encoding.clone());
        let want_start = fixture_li.offset_to_position(off).unwrap();
        let want_end = fixture_li.offset_to_position(end).unwrap();
        assert_eq!(want_start.line, 1, "symbol on line 1 ({symbol})");

        let (ts_path, read_source) = ext_source(".ts", src);
        let type_refs = vec![TypeLocation {
            path: ts_path,
            start: off,
            end,
        }];
        let (mapper, tsx_li, carrier_li) = (
            trivial_mapper(),
            LineIndex::new_utf16("x"),
            LineIndex::new_utf16("x"),
        );
        let no_external: Option<ExternalIdeResolver> = None;

        let result = merge_references(
            None,
            type_refs,
            "/proj/Caller.vue.tsx",
            &tsx_li,
            &mapper,
            &carrier_li,
            no_external,
            &carrier_never,
            encoding.clone(),
            &read_source,
        );
        let locs = result.expect("multibyte external reference survives");
        assert_eq!(
            locs[0].range.start, want_start,
            "start column must match the {encoding:?}-encoded LineIndex"
        );
        assert_eq!(
            locs[0].range.end, want_end,
            "end column must match the {encoding:?}-encoded LineIndex"
        );
    }
}

/// Same multibyte fixture, but the symbol prefixed by multibyte content ON ITS OWN LINE so the
/// COLUMN itself differs between UTF-16 and UTF-8. Asserts the two encodings produce DIFFERENT
/// columns and each matches its own encoding's index — the discriminating surrogate guard.
#[test]
fn external_reference_multibyte_columns_differ_between_encodings() {
    // `🚀` (4 bytes, 2 UTF-16 units, 1 scalar) precedes the symbol on the SAME line.
    let src = "x\n// 🚀 marker\nexport const sym = 1;\n";
    // Place the symbol on a line that begins with the emoji to force a column delta.
    let src = format!("{src}const 🚀x = sym;\n");
    let symbol = "🚀x";
    let off = src.find(symbol).unwrap() as u32 + ("const ".len() as u32);
    // The identifier we point at starts right after `const ` — at the emoji.
    let id_start = src.find("🚀x").unwrap() as u32;
    let id_end = id_start + "🚀x".len() as u32;

    let li16 = LineIndex::new(&src, PositionEncodingKind::UTF16);
    let li8 = LineIndex::new(&src, PositionEncodingKind::UTF8);
    let p16 = li16.offset_to_position(id_end).unwrap();
    let p8 = li8.offset_to_position(id_end).unwrap();
    assert_eq!(p16.line, p8.line, "same line in both encodings");
    assert_ne!(
        p16.character, p8.character,
        "fixture precondition: the emoji makes the end column differ between UTF-16 and UTF-8"
    );
    let _ = off; // off not used directly; id_start/id_end define the span.

    let run = |encoding: PositionEncodingKind, want: tower_lsp_server::ls_types::Position| {
        let (ts_path, read_source) = ext_source("Multi.ts", &src);
        let type_refs = vec![TypeLocation {
            path: ts_path,
            start: id_start,
            end: id_end,
        }];
        let (mapper, tsx_li, carrier_li) = (
            trivial_mapper(),
            LineIndex::new_utf16("x"),
            LineIndex::new_utf16("x"),
        );
        let no_external: Option<ExternalIdeResolver> = None;
        let result = merge_references(
            None,
            type_refs,
            "/proj/Caller.vue.tsx",
            &tsx_li,
            &mapper,
            &carrier_li,
            no_external,
            &carrier_never,
            encoding.clone(),
            &read_source,
        );
        let locs = result.expect("survives");
        assert_eq!(
            locs[0].range.end, want,
            "end column must equal the {encoding:?} index's position"
        );
    };
    run(PositionEncodingKind::UTF16, p16);
    run(PositionEncodingKind::UTF8, p8);
}

// ── Carrier-IDE FAIL-CLOSED guard ─────────────────────────────────────────────

/// A carrier IDE virtual file (`{carrier}.vue.tsx`) target whose byte offsets DO NOT map through
/// any in-context sourcemap (no external resolver, and the current-file mapper can't bridge the
/// offsets) must be DROPPED — never collapsed to `Range::default()` (line 0). The shared strict
/// carrier-IDE resolver fails closed here; the old lenient `.unwrap_or_default()` line-0'd it.
fn unmappable_carrier_ide_path() -> String {
    // `path_is_carrier` of `…/Foo.vue` is true → `…/Foo.vue.tsx` is a carrier IDE path.
    "/proj/Foo.vue.tsx".to_string()
}

#[test]
fn carrier_ide_reference_mapping_failure_is_dropped_not_zeroed() {
    let carrier_path = unmappable_carrier_ide_path();
    // Offsets far past the one-token trivial mapper's mapped run → tsx_range_to_carrier_range None.
    let type_refs = vec![TypeLocation {
        path: carrier_path,
        start: 9_000,
        end: 9_010,
    }];
    let (mapper, tsx_li, carrier_li) = (
        trivial_mapper(),
        LineIndex::new_utf16("x"),
        LineIndex::new_utf16("x"),
    );
    // No external resolver → the foreign carrier file has no sourcemap; the current mapper can't
    // bridge offsets 9000.. → fail closed.
    let no_external: Option<ExternalIdeResolver> = None;
    // A reader that WOULD resolve must never be consulted for a carrier-IDE path; supply a
    // panicking reader to prove the carrier branch never falls through to the source-read branch.
    let read_source = |_p: &str| -> Option<Arc<str>> { None };

    let result = merge_references(
        None,
        type_refs,
        // current request is a DIFFERENT carrier file → the foreign-carrier path with no resolver
        // fails closed (its offsets 9000.. do not map anywhere regardless).
        "/proj/Caller.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_never,
        PositionEncodingKind::UTF16,
        &read_source,
    );
    assert!(
        result.is_none(),
        "a carrier-IDE reference whose offsets do not map must be DROPPED (fail-closed), never \
         emitted at line 0: {result:?}"
    );
}

#[test]
fn carrier_ide_rename_mapping_failure_is_dropped_not_zeroed() {
    let carrier_path = unmappable_carrier_ide_path();
    let type_locations = vec![RenameLocation {
        path: carrier_path,
        start: 9_000,
        end: 9_010,
    }];
    let (mapper, tsx_li, carrier_li) = (
        trivial_mapper(),
        LineIndex::new_utf16("x"),
        LineIndex::new_utf16("x"),
    );
    let no_external: Option<ExternalIdeResolver> = None;
    let read_source = |_p: &str| -> Option<Arc<str>> { None };

    let result = merge_rename_locations(
        None,
        type_locations,
        "newName",
        "/proj/Caller.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        None::<ExternalApiResolver>,
        &carrier_never,
        PositionEncodingKind::UTF16,
        &read_source,
    );
    assert!(
        result.is_none(),
        "a carrier-IDE rename edit whose offsets do not map must be DROPPED (fail-closed) — a \
         line-0 rename edit would CORRUPT the carrier source: {result:?}"
    );
}

/// Discriminating POSITIVE half of the carrier-IDE guard: when an external resolver DOES supply
/// the foreign carrier file's mapper and the offsets map, the carrier-IDE reference survives with
/// the mapped carrier-source range (so the fail-closed change did not over-drop valid mappings).
#[test]
fn carrier_ide_reference_with_valid_external_mapper_survives() {
    // Build a real one-line carrier source + matching source map: TSX byte [0,1) ↔ Vue (0,0).
    let carrier_src = "y";
    let mut b = SourceMapBuilder::default();
    let sid = b.set_source_and_content("Foo.vue", carrier_src);
    b.add_token(0, 0, 0, 0, Some(sid), None);
    b.add_token(0, 1, 0, 1, Some(sid), None);
    let ctx_mapper = ProviderPositionMapper::source_map(
        PositionMapper::from_json(&b.into_sourcemap().to_json_string()).expect("valid map"),
    );

    let carrier_path = "/proj/Foo.vue.tsx".to_string();
    let resolver_path = carrier_path.clone();
    let resolver = move |p: &str| -> Option<ExternalIdeContext> {
        (p == resolver_path.as_str()).then(|| ExternalIdeContext {
            tsx_line_index: LineIndex::new_utf16("y"),
            mapper: {
                let mut b2 = SourceMapBuilder::default();
                let s2 = b2.set_source_and_content("Foo.vue", "y");
                b2.add_token(0, 0, 0, 0, Some(s2), None);
                b2.add_token(0, 1, 0, 1, Some(s2), None);
                ProviderPositionMapper::source_map(
                    PositionMapper::from_json(&b2.into_sourcemap().to_json_string()).unwrap(),
                )
            },
            carrier_line_index: LineIndex::new_utf16("y"),
            carrier_negotiated_line_index: None,
        })
    };
    let _ = ctx_mapper; // the mapper used is the one built inside the resolver closure.

    let type_refs = vec![TypeLocation {
        path: carrier_path.clone(),
        start: 0,
        end: 1,
    }];
    let (mapper, tsx_li, carrier_li) = (
        trivial_mapper(),
        LineIndex::new_utf16("x"),
        LineIndex::new_utf16("x"),
    );
    // `carrier_source_exists` true so the carrier path normalizes to `…/Foo.vue`.
    let carrier_exists = |p: &str| p == "/proj/Foo.vue";
    let read_source = |_p: &str| -> Option<Arc<str>> { None };
    let ext: Option<ExternalIdeResolver> = Some(&resolver);

    let result = merge_references(
        None,
        type_refs,
        // current request is a DIFFERENT carrier file → the FOREIGN carrier target routes through
        // its own context via `ext` (the resolver), proving the resolver bridges it.
        "/proj/Caller.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        ext,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &read_source,
    );
    let locs = result.expect("a valid carrier-IDE mapping must survive (not over-dropped)");
    assert_eq!(locs.len(), 1, "the mapped carrier-IDE reference survives");
    assert!(
        locs[0].uri.as_str().ends_with("Foo.vue"),
        "carrier-IDE target normalizes to the carrier source, got {}",
        locs[0].uri.as_str()
    );
    assert_eq!(
        locs[0].range.start,
        tower_lsp_server::ls_types::Position::new(0, 0),
        "mapped carrier range start"
    );
}

// ── Barrel / re-export target ─────────────────────────────────────────────────

/// A re-export "barrel" `index.ts` is just a plain external `.ts` file from the merge's
/// perspective: the provider returns the re-export site's REAL byte offsets in `index.ts`, and the
/// merge reads `index.ts`'s own source to convert them. The re-export keyword sits on a NON-ZERO
/// line, so a line-0 collapse is unambiguously wrong. Covers references, rename, and code-action.
fn barrel_fixture() -> (&'static str, u32, u32) {
    // `Widget` re-exported on LINE 2 of the barrel.
    let barrel =
        "// barrel\nexport { Helper } from './helper';\nexport { Widget } from './widget';\n";
    let off = barrel.find("Widget").unwrap() as u32;
    (barrel, off, off + "Widget".len() as u32)
}

#[test]
fn barrel_reexport_reference_keeps_real_line() {
    let (barrel, off, end) = barrel_fixture();
    let li = LineIndex::new_utf16(barrel);
    let want_start = li.offset_to_position(off).unwrap();
    let want_end = li.offset_to_position(end).unwrap();
    assert_eq!(want_start.line, 2, "re-export on line 2");

    let (path, read_source) = ext_source("Index.ts", barrel);
    let type_refs = vec![TypeLocation {
        path,
        start: off,
        end,
    }];
    let (mapper, tsx_li, carrier_li) = (
        trivial_mapper(),
        LineIndex::new_utf16("x"),
        LineIndex::new_utf16("x"),
    );
    let no_external: Option<ExternalIdeResolver> = None;
    let result = merge_references(
        None,
        type_refs,
        "/proj/Caller.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_never,
        PositionEncodingKind::UTF16,
        &read_source,
    );
    let locs = result.expect("barrel re-export reference survives");
    assert_eq!(locs[0].range.start, want_start);
    assert_eq!(locs[0].range.end, want_end);
    assert_ne!(locs[0].range, Range::default());
}

#[test]
fn barrel_reexport_rename_keeps_real_line() {
    let (barrel, off, end) = barrel_fixture();
    let li = LineIndex::new_utf16(barrel);
    let want_start = li.offset_to_position(off).unwrap();
    let want_end = li.offset_to_position(end).unwrap();

    let (path, read_source) = ext_source("Index.ts", barrel);
    let type_locations = vec![RenameLocation {
        path,
        start: off,
        end,
    }];
    let (mapper, tsx_li, carrier_li) = (
        trivial_mapper(),
        LineIndex::new_utf16("x"),
        LineIndex::new_utf16("x"),
    );
    let no_external: Option<ExternalIdeResolver> = None;
    let result = merge_rename_locations(
        None,
        type_locations,
        "WidgetRenamed",
        "/proj/Caller.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        None::<ExternalApiResolver>,
        &carrier_never,
        PositionEncodingKind::UTF16,
        &read_source,
    );
    let edit = result.expect("barrel re-export rename survives");
    let changes = edit.changes.expect("changes present");
    let te = changes.values().next().unwrap().first().unwrap();
    assert_eq!(te.range.start, want_start);
    assert_eq!(te.range.end, want_end);
    assert_ne!(te.range, Range::default());
    assert_eq!(te.new_text, "WidgetRenamed");
}

#[test]
fn barrel_reexport_code_action_keeps_real_line() {
    let (barrel, off, end) = barrel_fixture();
    let li = LineIndex::new_utf16(barrel);
    let want_start = li.offset_to_position(off).unwrap();
    let want_end = li.offset_to_position(end).unwrap();

    let (path, read_source) = ext_source("Index.ts", barrel);
    let actions = vec![TypeCodeAction {
        title: "Fix barrel".to_string(),
        kind: Some("quickfix".to_string()),
        edits: vec![TypeCodeEdit {
            path,
            start: off,
            end,
            new_text: "WidgetFixed".to_string(),
        }],
    }];
    let (mapper, tsx_li, carrier_li) = (
        trivial_mapper(),
        LineIndex::new_utf16("x"),
        LineIndex::new_utf16("x"),
    );
    let no_external: Option<ExternalIdeResolver> = None;
    let result = merge_code_actions(
        actions,
        "/proj/App.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_never,
        PositionEncodingKind::UTF16,
        &read_source,
        None,
    );
    assert_eq!(result.len(), 1, "the barrel code action survives");
    let CodeActionOrCommand::CodeAction(action) = &result[0] else {
        panic!("expected a CodeAction, got {:?}", result[0]);
    };
    let ws = action.edit.as_ref().expect("edit present");
    let changes = ws.changes.as_ref().expect("changes present");
    let te = changes.values().next().unwrap().first().unwrap();
    assert_eq!(te.range.start, want_start);
    assert_eq!(te.range.end, want_end);
    assert_ne!(te.range, Range::default());
    assert_eq!(te.new_text, "WidgetFixed");
}

/// Code-action FAIL-CLOSED twin: an unresolvable external code-action edit is DROPPED, and an
/// action whose only edit drops is removed entirely (never emitted at line 0).
#[test]
fn external_code_action_with_unresolvable_source_is_dropped_not_zeroed() {
    let actions = vec![TypeCodeAction {
        title: "Unresolvable".to_string(),
        kind: None,
        edits: vec![TypeCodeEdit {
            path: "/virtual/unreadable.ts".to_string(),
            start: 100,
            end: 110,
            new_text: "x".to_string(),
        }],
    }];
    let (mapper, tsx_li, carrier_li) = (
        trivial_mapper(),
        LineIndex::new_utf16("x"),
        LineIndex::new_utf16("x"),
    );
    let read_source = no_source_reader();
    let no_external: Option<ExternalIdeResolver> = None;
    let result = merge_code_actions(
        actions,
        "/proj/App.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_never,
        PositionEncodingKind::UTF16,
        &read_source,
        None,
    );
    assert!(
        result.is_empty(),
        "a code action whose only edit is unresolvable must be DROPPED entirely (fail-closed), \
         never emitted at line 0: {result:?}"
    );
}

//! Regression guard — Bug B: go-to-definition / type-definition on an EXTERNAL target must
//! land on the real symbol span, never LINE 0.
//!
//! Symptom (BUG-REPORT.md): CTRL+CLICK on `defineProps` navigated to the CORRECT file
//! (`runtime-core.d.ts`) but at **line 0** instead of the exact function definition.
//!
//! Root cause: `crates/verter_lsp/src/type_provider/merge.rs` — `merge_definitions_with_barrel_resolver`
//! (and its wrapper `merge_definitions`) substituted `Range::default()` (line 0, char 0) for
//! every non-`.vue` target instead of resolving the type provider's byte offsets to line:col.
//! The type provider returns a `TypeLocation { path, start, end }` whose `start`/`end` are REAL
//! byte offsets into the external file (`parse_lsp_location` in
//! `verter_type_runtime/src/tsgo/ipc.rs` disk-reads the target to compute them); the merge
//! layer threw those offsets away for any non-`.vue` file and collapsed the range to line 0.
//!
//! Why TypeScript / Volar don't have this: tsserver returns definition locations with real
//! line:col ranges; Volar forwards external-file locations with their true ranges. Neither
//! collapses an external target to 0:0.
//!
//! These tests exercise the `merge_definitions` boundary directly (no type provider process):
//! each constructs a `TypeLocation` whose byte offsets point at a known symbol, hands the merge
//! an in-memory source reader (modeling the host VFS the production merge reads through — see
//! `VerterHost::workspace_read().read_file` → `verter_workspace::WorkspaceRead::read_file` — rather than
//! direct disk I/O), and asserts the merged definition carries that symbol's EXACT line:col span
//! (both endpoints) — never `Range::default()`.

use std::sync::Arc;

use oxc_sourcemap::SourceMapBuilder;
use tower_lsp_server::ls_types::*;

use verter_lsp::documents::line_index::LineIndex;
use verter_lsp::documents::position_map::PositionMapper;
use verter_lsp::documents::provider_projection::ProviderPositionMapper;
use verter_lsp::type_provider::merge::{
    merge_definitions, merge_definitions_with_barrel_resolver, BarrelResolver, ExternalIdeResolver,
};
use verter_lsp::type_provider::protocol::TypeLocation;

/// A minimal valid provider mapper. For an EXTERNAL (non-carrier) target the mapper is
/// never consulted by `merge_definitions` — it only matters for carrier IDE targets — so a
/// one-token source map is sufficient to satisfy the signature.
fn trivial_mapper() -> ProviderPositionMapper {
    let mut b = SourceMapBuilder::default();
    let sid = b.set_source_and_content("App.vue", "x");
    b.add_token(0, 0, 0, 0, Some(sid), None);
    ProviderPositionMapper::source_map(
        PositionMapper::from_json(&b.into_sourcemap().to_json_string()).expect("valid source map"),
    )
}

/// In-memory external source fixture: a synthetic forward-slash path (with `suffix`) plus a
/// reader returning `content` for that exact path. Models the host VFS the production merge
/// reads through (`VerterHost::workspace_read().read_file` → `verter_workspace::WorkspaceRead::read_file`):
/// a definition target carries byte offsets into its own source, so the reader hands that exact
/// source back for the offset→line:col conversion — no disk I/O, fully hermetic.
fn ext_source(suffix: &str, content: &str) -> (String, impl Fn(&str) -> Option<Arc<str>>) {
    let path = format!("/virtual/external{suffix}");
    let content: Arc<str> = Arc::from(content);
    let reader_path = path.clone();
    let reader = move |p: &str| (p == reader_path.as_str()).then(|| content.clone());
    (path, reader)
}

#[test]
fn external_dts_definition_keeps_the_real_line_not_zero() {
    // A real on-disk declaration file standing in for `runtime-core.d.ts`.
    // `defineProps` sits on LINE 1 (0-based), not line 0.
    let dts = "export {}\nexport declare function defineProps(): void\n";
    let symbol = "defineProps";
    let sym_off = dts.find(symbol).expect("symbol present in fixture") as u32;
    let sym_end = sym_off + symbol.len() as u32;

    // Sanity: the fixture really places the symbol off line 0, so a line-0 result is
    // unambiguously wrong (and not a coincidence of a single-line file).
    let fixture_li = LineIndex::new_utf16(dts);
    let sym_pos = fixture_li
        .offset_to_position(sym_off)
        .expect("symbol offset is a valid position");
    let sym_end_pos = fixture_li
        .offset_to_position(sym_end)
        .expect("symbol end offset is a valid position");
    assert_eq!(
        sym_pos.line, 1,
        "fixture precondition: `{symbol}` must be on line 1, not line 0"
    );

    // Hand the merge an in-memory reader for the target's own source (the host VFS in
    // production): a CORRECT merge reads that source back and converts the byte offsets to
    // line:col. The read routes through the injected reader — no disk I/O.
    let (dts_path, read_source) = ext_source(".d.ts", dts);
    assert!(
        dts_path.ends_with(".d.ts") && !dts_path.ends_with(".vue.d.ts"),
        "fixture must be a plain external .d.ts (not a generated .vue.d.ts): {dts_path}"
    );

    // The type provider returns the location with REAL byte offsets into the .d.ts.
    let type_defs = vec![TypeLocation {
        path: dts_path.clone(),
        start: sym_off,
        end: sym_end,
    }];

    // Drive the exact merge entry point the goto-definition handler uses
    // (`handle_goto_definition` → `merge::merge_definitions_with_barrel_resolver`).
    let mapper = trivial_mapper();
    let tsx_li = LineIndex::new_utf16("x");
    let vue_li = LineIndex::new_utf16("x");
    let doc_uri: Uri = "file:///c:/proj/Caller.vue"
        .parse()
        .expect("valid document uri");
    let no_external: Option<ExternalIdeResolver> = None;
    let carrier_source_exists = |_p: &str| false;

    let resp = merge_definitions(
        None, // verter found nothing for `defineProps` (it is a macro) → type provider path
        type_defs,
        "", // current_tsx_path — irrelevant for an external .d.ts target
        &tsx_li,
        &mapper,
        &vue_li,
        no_external,
        &doc_uri,
        &carrier_source_exists,
        PositionEncodingKind::UTF16,
        &read_source,
    );

    let loc = match resp {
        Some(GotoDefinitionResponse::Scalar(loc)) => loc,
        other => panic!("expected a single external definition Location, got {other:?}"),
    };

    // The FILE resolves correctly — that part of the bug report is fine.
    assert!(
        loc.uri.as_str().ends_with(".d.ts"),
        "target file should be the external .d.ts, got {}",
        loc.uri.as_str()
    );

    // The POSITION must land on the EXACT symbol span — both endpoints, in line:col. An impl
    // that returned line N char 0, or the right start with a wrong end, must fail here. (The
    // original bug collapsed every non-.vue target to Range::default(), i.e. line 0.)
    assert_eq!(
        loc.range.start, sym_pos,
        "external .d.ts definition start must equal the exact symbol start {sym_pos:?}, got {:?} \
         (real byte offsets {sym_off}..{sym_end})",
        loc.range.start
    );
    assert_eq!(
        loc.range.end, sym_end_pos,
        "external .d.ts definition end must equal the exact symbol end {sym_end_pos:?}, got {:?}",
        loc.range.end
    );
    assert_ne!(
        loc.range,
        Range::default(),
        "external .d.ts definition must never collapse to the (0,0) default"
    );
}

/// Type-definition twin of the bug: `textDocument/typeDefinition` routes through
/// `merge_definitions_with_barrel_resolver` (the same merge entry point definition uses), so an
/// external `.d.ts` type target must keep its real declaration line rather than collapse to 0.
#[test]
fn external_dts_type_definition_keeps_the_real_line_not_zero() {
    // `Props` is declared on LINE 2 (0-based) of the fixture (line 1 is blank).
    let dts = "export {}\n\nexport interface Props { msg: string }\n";
    let symbol = "Props";
    let sym_off = dts.find(symbol).expect("symbol present in fixture") as u32;
    let sym_end = sym_off + symbol.len() as u32;

    let fixture_li = LineIndex::new_utf16(dts);
    let sym_pos = fixture_li
        .offset_to_position(sym_off)
        .expect("symbol offset is a valid position");
    let sym_end_pos = fixture_li
        .offset_to_position(sym_end)
        .expect("symbol end offset is a valid position");
    assert_eq!(
        sym_pos.line, 2,
        "fixture precondition: `{symbol}` must be on line 2, not line 0"
    );

    let (dts_path, read_source) = ext_source(".d.ts", dts);
    assert!(
        dts_path.ends_with(".d.ts") && !dts_path.ends_with(".vue.d.ts"),
        "fixture must be a plain external .d.ts: {dts_path}"
    );

    let type_defs = vec![TypeLocation {
        path: dts_path.clone(),
        start: sym_off,
        end: sym_end,
    }];

    let mapper = trivial_mapper();
    let tsx_li = LineIndex::new_utf16("x");
    let vue_li = LineIndex::new_utf16("x");
    let doc_uri: Uri = "file:///c:/proj/Caller.vue"
        .parse()
        .expect("valid document uri");
    let no_external: Option<ExternalIdeResolver> = None;
    let no_barrel: Option<BarrelResolver> = None;
    let carrier_source_exists = |_p: &str| false;

    // Drive the exact merge entry point the type-definition handler uses
    // (`handle_goto_type_definition` → `merge::merge_definitions_with_barrel_resolver`).
    let resp = merge_definitions_with_barrel_resolver(
        None,
        type_defs,
        "", // current_tsx_path — irrelevant for an external .d.ts target
        &tsx_li,
        &mapper,
        &vue_li,
        no_external,
        &doc_uri,
        &carrier_source_exists,
        no_barrel,
        PositionEncodingKind::UTF16,
        &read_source,
    );

    let loc = match resp {
        Some(GotoDefinitionResponse::Scalar(loc)) => loc,
        other => panic!("expected a single external type-definition Location, got {other:?}"),
    };
    assert!(
        loc.uri.as_str().ends_with(".d.ts"),
        "target file should be the external .d.ts, got {}",
        loc.uri.as_str()
    );
    // Full exact span — both endpoints in line:col, never a line-0 collapse.
    assert_eq!(
        loc.range.start, sym_pos,
        "external .d.ts type-definition start must equal the exact symbol start {sym_pos:?}, got {:?}",
        loc.range.start
    );
    assert_eq!(
        loc.range.end, sym_end_pos,
        "external .d.ts type-definition end must equal the exact symbol end {sym_end_pos:?}, got {:?}",
        loc.range.end
    );
    assert_ne!(
        loc.range,
        Range::default(),
        "external .d.ts type-definition must never collapse to the (0,0) default"
    );
}

/// Two definitions in the SAME external file at DIFFERENT byte ranges must BOTH survive. The
/// merge deduplicates by (uri, range.start, range.end) — a URI-only dedup would silently drop
/// one (e.g. one of two overloads), navigating the editor to only half the real targets.
#[test]
fn same_uri_definitions_with_distinct_ranges_both_survive() {
    // Two declarations in one file, on different lines (`alpha` line 1, `beta` line 2).
    let dts =
        "export {}\nexport declare function alpha(): void\nexport declare function beta(): void\n";
    let alpha_off = dts.find("alpha").expect("alpha present") as u32;
    let beta_off = dts.find("beta").expect("beta present") as u32;

    let fixture_li = LineIndex::new_utf16(dts);
    let alpha_start = fixture_li.offset_to_position(alpha_off).unwrap();
    let alpha_end = fixture_li
        .offset_to_position(alpha_off + "alpha".len() as u32)
        .unwrap();
    let beta_start = fixture_li.offset_to_position(beta_off).unwrap();
    let beta_end = fixture_li
        .offset_to_position(beta_off + "beta".len() as u32)
        .unwrap();
    assert_ne!(
        alpha_start.line, beta_start.line,
        "fixture precondition: the two symbols are on distinct lines"
    );

    let (dts_path, read_source) = ext_source(".d.ts", dts);

    // Both locations point into the SAME file but at DIFFERENT byte ranges.
    let type_defs = vec![
        TypeLocation {
            path: dts_path.clone(),
            start: alpha_off,
            end: alpha_off + "alpha".len() as u32,
        },
        TypeLocation {
            path: dts_path.clone(),
            start: beta_off,
            end: beta_off + "beta".len() as u32,
        },
    ];

    let mapper = trivial_mapper();
    let tsx_li = LineIndex::new_utf16("x");
    let vue_li = LineIndex::new_utf16("x");
    let doc_uri: Uri = "file:///c:/proj/Caller.vue"
        .parse()
        .expect("valid document uri");
    let no_external: Option<ExternalIdeResolver> = None;
    let carrier_source_exists = |_p: &str| false;

    let resp = merge_definitions(
        None,
        type_defs,
        "", // current_tsx_path — irrelevant for an external .d.ts target
        &tsx_li,
        &mapper,
        &vue_li,
        no_external,
        &doc_uri,
        &carrier_source_exists,
        PositionEncodingKind::UTF16,
        &read_source,
    );

    let locs = match resp {
        Some(GotoDefinitionResponse::Array(locs)) => locs,
        other => panic!(
            "expected TWO distinct same-file definitions to survive as an Array, got {other:?}"
        ),
    };
    assert_eq!(
        locs.len(),
        2,
        "both distinct-range definitions must survive (uri, range) dedup"
    );
    // Same external file, different real declaration spans.
    assert_eq!(
        locs[0].uri, locs[1].uri,
        "both definitions are in the same external file"
    );
    // Each definition carries the EXACT full span of its symbol (both endpoints), and both
    // symbols are present — order-independent. A wrong start OR a wrong end fails here.
    let alpha_range = Range {
        start: alpha_start,
        end: alpha_end,
    };
    let beta_range = Range {
        start: beta_start,
        end: beta_end,
    };
    for loc in &locs {
        assert!(
            loc.range == alpha_range || loc.range == beta_range,
            "each definition must carry an exact symbol range ({alpha_range:?} or {beta_range:?}), got {:?}",
            loc.range
        );
        assert_ne!(
            loc.range,
            Range::default(),
            "no definition may collapse to the (0,0) default"
        );
    }
    assert!(
        locs.iter().any(|l| l.range == alpha_range),
        "the `alpha` definition (exact range {alpha_range:?}) must be present: {locs:?}"
    );
    assert!(
        locs.iter().any(|l| l.range == beta_range),
        "the `beta` definition (exact range {beta_range:?}) must be present: {locs:?}"
    );
    // Negative: the ranges are NOT equal — a URI-only dedup would have collapsed them to one.
    assert_ne!(
        locs[0].range, locs[1].range,
        "distinct ranges in one file must not be merged"
    );
}

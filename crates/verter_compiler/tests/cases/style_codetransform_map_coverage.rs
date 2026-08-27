// @ai-generated - J1-A12: stage-local `CodeTransform` map-coverage proof for the Vue
// (`style_planner.rs`) `CodeTransform` route — exact coordinate mapping across
// byte-length-changing edits, unmapped synthetic regions, UTF-16/non-ASCII positions,
// multiline transforms, and option-off/no-map behavior, for EVERY Vue stage:
// `transform_vue_v_bind`, `transform_vue_scoped_css`, `transform_vue_css_modules`.
//
// The Svelte half of A12 lives IN-CRATE (`svelte/runtime/css/render_tests.rs`), not here:
// `render_stylesheet` is `pub(crate)` and the whole `css` module tree under
// `svelte/runtime/` is crate-private (`mod css;`, not `pub mod css;`) per this charter's own
// A11d acceptance ID — an external `tests/cases` file (a separate compiled crate that only
// sees `verter_compiler`'s PUBLIC API) cannot name it. This split is deliberate, not an
// oversight: making the module `pub` to route around the boundary would violate A11d.
//
// Every assertion below decodes the REAL source-map JSON `style_planner.rs` emits (via
// `oxc_sourcemap::OwnedSourceMap`, a normal `verter_compiler` dependency, not test-gated) and
// checks actual decoded token positions — never a "the map string is non-empty" placeholder.
//
// `oxc_sourcemap::lookup_token` returns the GREATEST-LOWER-BOUND token for a queried generated
// position, not necessarily a token anchored EXACTLY there — so every "exact coordinate"
// assertion below also confirms `token.get_dst_line()`/`get_dst_col()` equal the QUERIED
// position, not merely that the returned token's SOURCE coordinates look plausible. Without
// that check, a token emitted at the wrong generated position could still pass by coincidence
// (the GLB lookup would just fall back to an earlier, unrelated token).
use std::sync::Arc;

use oxc_allocator::Allocator;
use verter_compiler::framework_common::carrier_compiler::{
    CarrierCompiler, RuntimeBlockContentInput, RuntimeBlockContentInputs, RuntimeCompileOptions,
};
use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
use verter_compiler::framework_common::{CarrierCompilerRegistry, FrameworkParseArtifact};
use verter_compiler::style_planner::{
    transform_vue_css_modules, transform_vue_scoped_css, transform_vue_v_bind, AuthoredStyleInput,
    PlainCssInput, StyleRewriteOutcome,
};
use verter_css_syntax::CssDialect;
use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
    FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};

fn rewritten(outcome: StyleRewriteOutcome) -> (String, String) {
    match outcome {
        StyleRewriteOutcome::Rewritten {
            code, source_map, ..
        } => (code, source_map),
        StyleRewriteOutcome::Unchanged { .. } => panic!("expected a rewrite"),
    }
}

fn decode(map_json: &str) -> oxc_sourcemap::OwnedSourceMap {
    oxc_sourcemap::OwnedSourceMap::from_json_string(map_json).expect("valid source-map JSON")
}

/// UTF-16 code-unit length of `s` — the column unit the source-map spec uses.
/// An INDEPENDENT reimplementation (not a call into `style_planner`/`CodeTransform`), so a
/// regression that swaps `CodeTransform`'s UTF-16 column tracking for a raw byte count is
/// something these tests can actually catch.
fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

/// 0-based (line, UTF-16 column) for a BYTE offset into `text`.
fn byte_offset_to_line_col(text: &str, byte_offset: usize) -> (u32, u32) {
    let mut line: u32 = 0;
    let mut line_start: usize = 0;
    for (i, b) in text.as_bytes().iter().enumerate() {
        if i == byte_offset {
            break;
        }
        if *b == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let col = utf16_len(&text[line_start..byte_offset]);
    (line, col)
}

/// Look up the token at EXACTLY `(gen_line, gen_col)` and assert it is truly anchored there
/// (`get_dst_line`/`get_dst_col` match the query), not a greatest-lower-bound fallback to some
/// earlier, unrelated token. Returns the confirmed-exact token.
fn exact_token(
    map: &oxc_sourcemap::OwnedSourceMap,
    lookup: &[&[oxc_sourcemap::Token]],
    gen_line: u32,
    gen_col: u32,
) -> oxc_sourcemap::Token {
    let token = map
        .lookup_token(lookup, gen_line, gen_col)
        .unwrap_or_else(|| panic!("no token found at or before generated {gen_line}:{gen_col}"));
    assert_eq!(
        (token.get_dst_line(), token.get_dst_col()),
        (gen_line, gen_col),
        "lookup_token returned a GREATEST-LOWER-BOUND token anchored at {}:{}, not the queried \
         generated position {gen_line}:{gen_col} — the returned token's source coordinates \
         cannot be trusted as \"the mapping at this exact position\"",
        token.get_dst_line(),
        token.get_dst_col()
    );
    token
}

// ─── category 1: exact coordinate mapping across a byte-length-changing edit ──

#[test]
fn vue_v_bind_growing_overwrite_trailing_text_maps_to_correct_shifted_position() {
    // `v-bind(size)` (13 bytes) is replaced by `var(--sc1-size)` (15 bytes) — a
    // byte-length-GROWING edit. The trailing, UNCHANGED `height: 10px` text must still map to
    // its correct ORIGINAL position, not an offset carried over from before the edit shifted
    // the generated bytes.
    let source = ".icon { width: v-bind(size); height: 10px; }";
    let input = AuthoredStyleInput::new(
        source,
        CssDialect::Css,
        "vbind.css",
        "space:vbind",
        "artifact:vbind",
    );
    let (code, map_json) = rewritten(transform_vue_v_bind(input, "sc1").expect("v-bind rewrites"));
    assert_ne!(
        code.len(),
        source.len(),
        "fixture sanity: the replacement must actually change byte length"
    );
    let map = decode(&map_json);

    // Query EXACTLY at the trailing Original chunk's own start (the semicolon immediately after
    // the overwritten `v-bind(size)` span) — the point a source-map `Token` actually anchors, so
    // the assertion needs no intra-segment offset arithmetic: `token.get_src_col()` must equal
    // the query position exactly, not merely "close to" it.
    let trailing = "; height: 10px; }";
    let gen_pos = code
        .find(trailing)
        .expect("trailing text survives verbatim");
    let (gen_line, gen_col) = byte_offset_to_line_col(&code, gen_pos);
    let src_pos = source
        .find(trailing)
        .expect("trailing text exists in source");
    let expected = byte_offset_to_line_col(source, src_pos);

    let lookup = map.generate_lookup_table();
    let token = exact_token(&map, &lookup, gen_line, gen_col);
    assert_eq!(token.get_source_id(), Some(0));
    assert_eq!(
        (token.get_src_line(), token.get_src_col()),
        expected,
        "trailing unchanged text must map to its ORIGINAL position after a byte-length-changing edit"
    );
}

#[test]
fn vue_v_bind_multiline_argument_overwrite_collapses_lines_and_trailing_text_still_maps_correctly()
{
    // The v-bind() ARGUMENT spans three source lines; the replacement `var(--sc1-size)` is a
    // single line — a byte-length- AND LINE-COUNT-shrinking edit. `height: 10px` (after the
    // edit) must map to its correct ORIGINAL line, not a line number desynced by the collapse.
    let source = ".a {\n  width: v-bind(\n    size\n  );\n  height: 10px;\n}";
    let input = AuthoredStyleInput::new(
        source,
        CssDialect::Css,
        "multiline.css",
        "space:multiline",
        "artifact:multiline",
    );
    let (code, map_json) =
        rewritten(transform_vue_v_bind(input, "sc1").expect("multiline v-bind rewrites"));
    assert!(
        code.lines().count() < source.lines().count(),
        "fixture sanity: the replacement must collapse line count"
    );
    let map = decode(&map_json);

    // A `Token` anchors an EXACT generated position — querying mid-segment (e.g. "the middle of
    // the word `height`") returns the token for the segment it falls IN, not an interpolated
    // position, so this queries EXACTLY at the newline-crossing boundary the trailing Original
    // chunk's internal `\n` scan produces: the generated column right after that `\n` (column 0
    // of the `height` line), which is where a fresh `Token` is anchored on both sides.
    let marker = "\n  height: 10px;\n}";
    let gen_marker = code
        .find(marker)
        .expect("trailing declaration survives verbatim")
        + 1;
    let (gen_line, gen_col) = byte_offset_to_line_col(&code, gen_marker);
    let src_marker = source.find(marker).expect("marker exists in source") + 1;
    let expected @ (expected_line, expected_col) = byte_offset_to_line_col(source, src_marker);
    assert_eq!(
        expected_line, 4,
        "fixture sanity: `height` sits on source line 4"
    );
    assert_eq!(
        expected_col, 0,
        "fixture sanity: querying right at the line start, column 0"
    );

    let lookup = map.generate_lookup_table();
    let token = exact_token(&map, &lookup, gen_line, gen_col);
    assert_eq!(token.get_source_id(), Some(0));
    assert_eq!(
        (token.get_src_line(), token.get_src_col()),
        expected,
        "a multi-line-collapsing overwrite must not desync generated/source LINE tracking for later text"
    );
}

#[test]
fn vue_css_modules_growing_overwrite_trailing_text_maps_to_correct_shifted_position() {
    // `active` (6 bytes) is replaced by the hashed class name `active_<8 hex chars>` (15
    // bytes) — a byte-length-GROWING edit, the CSS-Modules stage's own edit shape (distinct
    // from v-bind's function-call overwrite and scoped-css's pure insertion). The trailing,
    // UNCHANGED declaration text must still map to its correct ORIGINAL position.
    let source = ".active { color: red; width: 1px; }";
    let input = PlainCssInput::try_new(
        source,
        CssDialect::Css,
        "modules.css",
        "space:modules",
        "artifact:modules",
    )
    .expect("plain CSS stage input");
    let (code, map_json) =
        rewritten(transform_vue_css_modules(input, "sc1").expect("css modules rewrite"));
    assert_ne!(
        code.len(),
        source.len(),
        "fixture sanity: the hashed class name must actually change byte length"
    );
    assert!(
        code.contains("active_"),
        "fixture sanity: the class name is hashed with an `_`-joined suffix: {code}"
    );
    let map = decode(&map_json);

    // Query EXACTLY at the trailing Original chunk's own start — the space right after the
    // overwritten class-name span, before the byte-length-changing edit's replacement.
    let trailing = " { color: red; width: 1px; }";
    let gen_pos = code
        .find(trailing)
        .expect("trailing declaration survives verbatim");
    let (gen_line, gen_col) = byte_offset_to_line_col(&code, gen_pos);
    let src_pos = source
        .find(trailing)
        .expect("trailing declaration exists in source");
    let expected = byte_offset_to_line_col(source, src_pos);

    let lookup = map.generate_lookup_table();
    let token = exact_token(&map, &lookup, gen_line, gen_col);
    assert_eq!(token.get_source_id(), Some(0));
    assert_eq!(
        (token.get_src_line(), token.get_src_col()),
        expected,
        "trailing unchanged text must map to its ORIGINAL position after the CSS-Modules \
         class-name overwrite"
    );
}

// ─── category 2: unmapped synthetic regions stay unmapped ─────────────────────

#[test]
fn vue_scoped_selector_inserted_attribute_stays_unmapped_across_its_entire_span() {
    // The `[data-v-<id>]` scope attribute is a pure synthetic insertion with no character-level
    // correspondence to the source — it must never claim a false source position ANYWHERE
    // across its span, not merely at its first column (a later position inside the same
    // insertion incorrectly resolving to a mapped source position is a real, distinct bug a
    // first-column-only check cannot see).
    let source = ".foo { color: red; }";
    let input = PlainCssInput::try_new(
        source,
        CssDialect::Css,
        "scoped.css",
        "space:scoped",
        "artifact:scoped",
    )
    .expect("plain CSS stage input");
    let (code, map_json) =
        rewritten(transform_vue_scoped_css(input, "test1234").expect("scoped rewrite"));
    let attr = "[data-v-test1234]";
    assert_eq!(code, format!(".foo{attr} {{ color: red; }}"));
    let insert_pos = code.find(attr).expect("scope attribute inserted");

    let map = decode(&map_json);
    let lookup = map.generate_lookup_table();

    // Walk every generated position across the ENTIRE synthetic attribute span.
    for offset in insert_pos..insert_pos + attr.len() {
        let (gen_line, gen_col) = byte_offset_to_line_col(&code, offset);
        let token = map
            .lookup_token(&lookup, gen_line, gen_col)
            .unwrap_or_else(|| {
                panic!(
                "no token covers generated position {gen_line}:{gen_col} (byte {offset}, inside \
                 the synthetic attribute)"
            )
            });
        assert_eq!(
            token.get_source_id(),
            None,
            "generated position {gen_line}:{gen_col} (byte {offset}, inside the synthetic \
             inserted scope attribute) must NOT claim a source position"
        );
    }

    // The position immediately AFTER the synthetic region resumes real mapping — proves the
    // unmapped span is exactly bounded, not an accidental "everything from here on is unmapped".
    let after = insert_pos + attr.len();
    let (gen_line, gen_col) = byte_offset_to_line_col(&code, after);
    let token = exact_token(&map, &lookup, gen_line, gen_col);
    assert_eq!(
        token.get_source_id(),
        Some(0),
        "the position right after the synthetic attribute must resume mapping to real source"
    );
}

// ─── category 3: UTF-16 / non-ASCII positions ──────────────────────────────────

#[test]
fn vue_v_bind_overwrite_anchor_uses_utf16_columns_not_byte_offsets() {
    // `"héllo→😀"` mixes a BMP-combining character (`é`: 2 UTF-8 bytes / 1 UTF-16 unit) and an
    // astral character (`😀`: 4 UTF-8 bytes / 2 UTF-16 units) BEFORE the `v-bind()` call on the
    // SAME line — a byte-offset-as-column bug and correct UTF-16-code-unit columns disagree
    // here, so this discriminates a regression in `CodeTransform`'s column tracking.
    let source = ".a::before { content: \"h\u{e9}llo\u{2192}\u{1f600}\"; color: v-bind(shade); }";
    let input = AuthoredStyleInput::new(
        source,
        CssDialect::Css,
        "utf16.css",
        "space:utf16",
        "artifact:utf16",
    );
    let (code, map_json) = rewritten(transform_vue_v_bind(input, "sc1").expect("v-bind rewrites"));
    let map = decode(&map_json);

    let vbind_byte_offset = source.find("v-bind(").expect("v-bind call present");
    let expected @ (expected_line, expected_col) =
        byte_offset_to_line_col(source, vbind_byte_offset);
    assert_eq!(expected_line, 0);
    assert_ne!(
        vbind_byte_offset as u32, expected_col,
        "fixture sanity: the byte offset and UTF-16 column must actually differ here, or this \
         test cannot discriminate a byte-vs-UTF-16 regression"
    );

    let gen_pos = code.find("var(").expect("var() replacement present");
    let (gen_line, gen_col) = byte_offset_to_line_col(&code, gen_pos);
    let lookup = map.generate_lookup_table();
    let token = exact_token(&map, &lookup, gen_line, gen_col);
    assert_eq!(token.get_source_id(), Some(0));
    assert_eq!(
        (token.get_src_line(), token.get_src_col()),
        expected,
        "the overwrite's mapped source column must be the UTF-16 code-unit width preceding it, \
         not its UTF-8 byte offset"
    );
}

#[test]
fn vue_css_modules_overwrite_anchor_uses_utf16_columns_not_byte_offsets() {
    // A CSS comment mixing the same BMP-combining + astral characters sits BEFORE `.active` on
    // the SAME line the CSS-Modules class-name overwrite anchors against. Comment bytes are
    // ordinary source bytes for position purposes (the resolver operates on raw source, not
    // reconstructed/parsed text), so this discriminates the SAME byte-vs-UTF-16 regression class
    // for the CSS-Modules stage's own overwrite anchor.
    let source = "/* h\u{e9}llo\u{2192}\u{1f600} */ .active { color: red; width: 1px; }";
    let input = PlainCssInput::try_new(
        source,
        CssDialect::Css,
        "modules-utf16.css",
        "space:modules-utf16",
        "artifact:modules-utf16",
    )
    .expect("plain CSS stage input");
    let (code, map_json) =
        rewritten(transform_vue_css_modules(input, "sc1").expect("css modules rewrite"));
    let map = decode(&map_json);

    let class_byte_offset = source.find(".active").expect("class selector present") + 1;
    let expected @ (expected_line, expected_col) =
        byte_offset_to_line_col(source, class_byte_offset);
    assert_eq!(expected_line, 0);
    assert_ne!(
        class_byte_offset as u32, expected_col,
        "fixture sanity: the byte offset and UTF-16 column must actually differ here, or this \
         test cannot discriminate a byte-vs-UTF-16 regression"
    );

    let gen_pos = code
        .find("active_")
        .expect("hashed class replacement present");
    let (gen_line, gen_col) = byte_offset_to_line_col(&code, gen_pos);
    let lookup = map.generate_lookup_table();
    let token = exact_token(&map, &lookup, gen_line, gen_col);
    assert_eq!(token.get_source_id(), Some(0));
    assert_eq!(
        (token.get_src_line(), token.get_src_col()),
        expected,
        "the CSS-Modules overwrite's mapped source column must be the UTF-16 code-unit width \
         preceding it, not its UTF-8 byte offset"
    );
}

// ─── category 5: option-off / no-map behavior ──────────────────────────────────

#[test]
fn vue_v_bind_no_rewrite_targets_never_reaches_map_generation_work() {
    // This is deliberately NOT named/framed as an "option-off" proof: `style_planner.rs`'s Vue
    // transform stages expose no boolean "generate a map or not" toggle at all — see the GENUINE
    // A/B option-off test below, which drives the real toggle that DOES exist
    // (`RuntimeCompileOptions.source_map`, at the bridge layer). What THIS test proves is
    // narrower: an input with nothing to rewrite reaches `StyleRewriteOutcome::Unchanged`
    // BEFORE any `CodeTransform` is even constructed, so `build_string`/map-generation work is
    // never run at all for it — never silently run and then discarded.
    //
    // The `build_string` invocation counter is `#[cfg(test)]` on the library and therefore
    // invisible to this integration crate; the Unchanged discriminant is the caller-visible
    // no-map proof here. The compile_bundle A/B below is the genuine option-off toggle.
    let source = ".a { color: red; width: 1px; }"; // no v-bind() anywhere
    let input = AuthoredStyleInput::new(
        source,
        CssDialect::Css,
        "nomap.css",
        "space:nomap",
        "artifact:nomap",
    );
    let outcome =
        transform_vue_v_bind(input, "sc1").expect("an input with no v-bind targets still succeeds");
    match outcome {
        StyleRewriteOutcome::Unchanged { .. } => {}
        StyleRewriteOutcome::Rewritten { .. } => {
            panic!("an input with no v-bind targets must not rewrite")
        }
    }
}

fn registered_artifact(canonical: &str, source: &str) -> FrameworkParseArtifact {
    let language = verter_language::FileLanguage::vue();
    let source_authority = RegisteredSourceAuthority::new().expect("source authority");
    let snapshot = source_authority
        .register_source(
            CanonicalFileId::new(canonical),
            FileIncarnation::new(1),
            SourceGeneration::new(1),
            language.clone(),
            Arc::from(source),
        )
        .expect("registered source");
    let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
    let config =
        CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>()).expect("vue grammar");
    grammar_authority
        .register_carrier_grammar(
            language,
            FrameworkAdapterSemanticVersion::new(1).expect("adapter version"),
            CarrierParserGrammarVersion::new(1).expect("grammar version"),
            config.clone(),
        )
        .expect("grammar registration");
    let accepted = grammar_authority
        .accept_registered_source(&source_authority, &snapshot, &config)
        .expect("accepted source");
    CarrierCompilerRegistry::built_in()
        .project_registered(&accepted)
        .expect("registered projection")
        .into_framework_parse_artifact()
}

/// The GENUINE option-off/on A/B proof, driven through the real public entry point the toggle
/// actually reaches: `CarrierCompiler::compile_bundle`'s `RuntimeCompileOptions.source_map`
/// field (`vue_bridge.rs`), with a SUPPLIED external style block (`<style src="...">`) — the
/// only style shape `compile_bundle`'s own `opts.block_content.styles` override loop touches at
/// all (an authored, non-`src` inline style compiles through a different internal path that
/// never reaches this particular field; that path is exercised elsewhere in this file via
/// `style_planner`'s own public functions directly).
///
/// `style_planner::emit()` (`style_planner.rs`) takes a `want_source_map: bool` threaded down
/// from `RuntimeCompileOptions::source_map` through `vue_bridge.rs`'s cascade calls; when it is
/// `false`, `emit` skips `generate_map`/`to_json_string()` entirely for that stage rather than
/// building a map and discarding it afterward. The caller-OBSERVABLE contract this test proves
/// (identical code either way; no map hand-back when off; a valid map hand-back when on) holds
/// either way.
#[test]
fn vue_style_source_map_toggle_is_a_genuine_caller_facing_ab_option_through_compile_bundle() {
    // Single-stage rewrite only. A v-bind+scoped fixture abandons map
    // composition (the cascade will not present a later stage's local map as
    // the whole-cascade map), so `source_map: true` would still hand the
    // caller `None` and this A/B would not discriminate the toggle.
    let source =
        "<template><div class=\"x\"/></template><style scoped src=\"./theme.css\"></style>";
    let make_opts = |source_map: bool| RuntimeCompileOptions {
        filename: Some("ToggleStyle.vue".to_string()),
        component_id: Some("scope123".to_string()),
        source_map,
        block_content: RuntimeBlockContentInputs {
            styles: vec![Some(RuntimeBlockContentInput {
                code: Arc::from(".x { color: red; }"),
                source_map: None,
                lang: "css".to_string(),
                content_artifact_token: "artifact:theme-css".to_string(),
                source_space_token: "space:theme-css".to_string(),
                parsed: None,
            })],
            ..Default::default()
        },
        ..Default::default()
    };
    let compiler = VueCarrierCompiler;
    let alloc = Allocator::default();

    let artifact_off = registered_artifact("file:///ToggleOff.vue", source);
    let off = compiler
        .compile_bundle(source, &artifact_off, &make_opts(false), &alloc)
        .expect("compiles with source_map: false")
        .into_produced()
        .expect("runtime surface produced");
    let style_off = off.styles.first().expect("style output");

    let artifact_on = registered_artifact("file:///ToggleOn.vue", source);
    let on = compiler
        .compile_bundle(source, &artifact_on, &make_opts(true), &alloc)
        .expect("compiles with source_map: true")
        .into_produced()
        .expect("runtime surface produced");
    let style_on = on.styles.first().expect("style output");

    // Sanity: a real byte-changing rewrite actually happened (the scoped
    // stage ran) — otherwise this would not be a rewriting fixture at all.
    assert!(
        style_off.code.contains(".x[data-v-scope123]"),
        "{}",
        style_off.code
    );
    assert_ne!(
        style_off.code.as_str(),
        ".x { color: red; }",
        "the scoped rewrite must change the authored bytes"
    );

    // (a) the toggle never changes emitted code bytes.
    assert_eq!(
        style_off.code, style_on.code,
        "the source_map toggle must never change emitted code bytes"
    );
    // (b) with the toggle off, no map is handed back to the caller.
    assert_eq!(
        style_off.source_map, None,
        "source_map: false must not hand the caller a map"
    );
    // (c) with the toggle on, a valid decoded map is handed back.
    let map_json = style_on
        .source_map
        .as_deref()
        .expect("source_map: true must hand the caller a map");
    let map = decode(map_json);
    assert!(
        map.get_tokens().next().is_some(),
        "the returned map must carry real mappings"
    );
}

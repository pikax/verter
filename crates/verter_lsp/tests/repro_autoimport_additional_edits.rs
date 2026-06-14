//! Auto-import on completion accept — structural-translation gates.
//!
//! Symptom (BUG-REPORT.md, `<script setup>` → "Auto import not working"): accepting a
//! completion that needs an import does not add the auto-import line.
//!
//! Root cause: tsserver/TSGO return the auto-import as a `completionItem/resolve`
//! `additionalTextEdit` whose offsets are into the GENERATED TSX. For a brand-new import
//! TypeScript places that edit at the top-of-file / sorted-import boundary, which in Verter's
//! generated TSX lands inside the synthetic, UNMAPPED helper-import preamble
//! (`import { defineComponent } …`). The handler previously mapped every edit through the
//! strict [`PositionMapper`] and silently `filter_map`-dropped any that failed — so the import
//! edit (offset 0, synthetic) was dropped and no import was inserted.
//!
//! The strict mapper is correct by design and MUST NOT be weakened: a query in
//! synthetic/unmapped content returns `None` (see `position_mapper_strict.rs`). The fix is
//! handler-side structural translation:
//!   * an edit that round-trips through the strict mapper targets real user source and is applied
//!     verbatim;
//!   * a mapper miss is re-anchored at the Vue `<script setup>` insertion site ONLY when it is
//!     provably a zero-width auto-import insertion in the synthetic helper-import preamble; every
//!     other miss is rejected structurally — never spliced into user source;
//!   * the insertion anchor is computed from the SFC's own block/import facts, consuming the
//!     **SFC-absolute** `AnalyzedImport.span` ends directly (no double-offset) and filtering to
//!     the selected `<script setup>` block (Volar parity).
//!
//! These gates exercise the pure translation surface (`verter_lsp::tsgo::auto_import`) directly on
//! a faithful generated-TSX source map. They are GREEN with the fix and would be RED (or
//! structurally different) without it. The full provider round-trip is covered by the VS Code e2e
//! suite.

use oxc_sourcemap::SourceMapBuilder;
use tower_lsp_server::ls_types::TextEdit;

use verter_lsp::documents::line_index::LineIndex;
use verter_lsp::documents::position_map::PositionMapper;
use verter_lsp::documents::sfc_scanner::scan_sfc_blocks;
use verter_lsp::tsgo::auto_import::{
    resolve_script_import_anchor, translate_completion_import_edits, AutoImportEditMappingError,
    ProviderImportEdit, ScriptImportInsertionAnchor,
};
use verter_lsp::tsgo::merge::tsx_range_to_vue_range;

/// Inject the `x_verter_helper_preamble_end` source-map metadata member exactly as Verter's IDE
/// codegen does — a leading object member carrying the generated-TSX position immediately after
/// the last helper import (`crates/verter_compiler/src/code_transform/source_map.rs`). The strict
/// `PositionMapper` recovers it into a typed boundary; `OwnedSourceMap` ignores the unknown field.
/// Faithful fixtures carry it so the structural preamble classifier has a real boundary to gate on.
fn with_preamble_end(json: &str, line: u32, character: u32) -> String {
    assert!(
        json.starts_with('{'),
        "oxc source-map JSON starts with an object"
    );
    format!(
        "{{\"x_verter_helper_preamble_end\":{{\"line\":{line},\"character\":{character}}},{}",
        &json[1..]
    )
}

/// A faithful generated-TSX source map for a no-user-import `<script setup>`: a synthetic
/// helper-import preamble (line 0, unmapped), user code (line 1, mapped → vue line 1), and a
/// synthetic export (line 2, unmapped). The map carries the explicit preamble-end boundary at
/// `(line 1, col 0)` — immediately after the single helper-import line — exactly as IDE codegen
/// emits it. Returns `(vue_source, tsx, mapper)`.
fn faithful_no_import_fixture() -> (&'static str, &'static str, PositionMapper) {
    let vue_source = concat!(
        "<script setup lang=\"ts\">\n", // line 0
        "const count = 0\n",            // line 1 (user code)
        "</script>\n",                  // line 2
    );
    let tsx = concat!(
        "import { defineComponent } from 'vue';\n", // line 0 — SYNTHETIC preamble, unmapped
        "const count = 0;\n",                       // line 1 — user code, mapped → vue line 1
        "export default defineComponent({});\n",    // line 2 — SYNTHETIC export, unmapped
    );

    let mut b = SourceMapBuilder::default();
    let sid = b.set_source_and_content("App.vue", vue_source);
    b.add_token(0, 0, 0, 0, None, None); // synthetic preamble: explicitly unmapped
    b.add_token(1, 0, 1, 0, Some(sid), None); // user code maps to Vue line 1
    b.add_token(2, 0, 0, 0, None, None); // synthetic export: explicitly unmapped
                                         // Boundary after the single helper-import line (line 0) ⇒ user source begins at (line 1, 0).
    let json = with_preamble_end(&b.into_sourcemap().to_json_string(), 1, 0);
    let mapper = PositionMapper::from_json(&json).expect("valid source map");
    (vue_source, tsx, mapper)
}

/// A faithful generated-TSX source map for an EMPTY `<script setup>`: a helper-import preamble
/// (lines 0–1) followed by a trailing synthetic component wrapper (lines 2+), with NO user code
/// and therefore NO mapped runs at all (the map has no mapped tokens to lean on).
/// The map carries the explicit `x_verter_helper_preamble_end` boundary at `(line 2, col 0)` —
/// immediately after the last helper import — so the classifier can still distinguish the leading
/// preamble from the trailing synthetic region without any mapped run to lean on. Returns
/// `(vue_source, tsx, mapper)`.
fn faithful_no_mapped_run_fixture() -> (&'static str, &'static str, PositionMapper) {
    let vue_source = concat!(
        "<script setup lang=\"ts\">\n", // line 0
        "</script>\n",                  // line 1 (no user code at all)
    );
    let tsx = concat!(
        "import type { Prettify } from \"@verter/types\";\n", // line 0 — preamble, unmapped
        "import { shallowUnwrapRef } from \"@verter/types\";\n", // line 1 — preamble, unmapped
        "export function ___VERTER___TemplateBindingFN() {\n", // line 2 — trailing synthetic
        "return {};\n",                                       // line 3 — trailing synthetic
        "}\n",                                                // line 4 — trailing synthetic
    );

    let mut b = SourceMapBuilder::default();
    let _sid = b.set_source_and_content("App.vue", vue_source);
    // No mapped tokens: an empty <script setup> has no user code, so nothing maps. The whole file
    // is synthetic; the boundary is the ONLY signal separating preamble from trailing synthetic.
    let json = with_preamble_end(&b.into_sourcemap().to_json_string(), 2, 0);
    let mapper = PositionMapper::from_json(&json).expect("valid source map");
    (vue_source, tsx, mapper)
}

/// Apply LSP `TextEdit`s to `source` and return the result. Test-only: edits are applied
/// right-to-left so earlier byte offsets stay valid; the fixtures here produce non-overlapping
/// edits.
fn apply_edits(source: &str, edits: &[TextEdit]) -> String {
    let li = LineIndex::new_utf16(source);
    let mut spans: Vec<(usize, usize, &str)> = edits
        .iter()
        .map(|e| {
            let s = li
                .position_to_offset(&e.range.start)
                .expect("edit start is a valid Vue position") as usize;
            let end = li
                .position_to_offset(&e.range.end)
                .expect("edit end is a valid Vue position") as usize;
            (s, end, e.new_text.as_str())
        })
        .collect();
    spans.sort_by_key(|s| std::cmp::Reverse(s.0));
    let mut out = source.to_string();
    for (s, end, text) in spans {
        out.replace_range(s..end, text);
    }
    out
}

/// Guard: the strict mapper does NOT (and must never) resolve the synthetic preamble at
/// generated offset 0. This is correct-by-design — it is exactly why an auto-import edit there
/// must be translated structurally rather than positionally. A "fix" that made this `Some`
/// would be weakening the mapper, which is forbidden.
#[test]
fn strict_mapper_does_not_resolve_synthetic_preamble_offset() {
    let (vue_source, tsx, mapper) = faithful_no_import_fixture();
    let tsx_li = LineIndex::new_utf16(tsx);
    let vue_li = LineIndex::new_utf16(vue_source);

    // The mapped user-code range DOES round-trip (so the map is real, not vacuous).
    let user_code_off = tsx.find("const count").unwrap() as u32;
    assert!(
        tsx_range_to_vue_range(user_code_off, user_code_off + 5, &tsx_li, &mapper, &vue_li)
            .is_some(),
        "a mapped user-code range must round-trip"
    );

    // The synthetic preamble at offset 0 must NOT map (strict-in-run mapper).
    assert!(
        tsx_range_to_vue_range(0, 0, &tsx_li, &mapper, &vue_li).is_none(),
        "the synthetic preamble at offset 0 must stay unmapped — the mapper is strict by design"
    );
}

/// An auto-import inserted by the provider at generated offset 0 (the synthetic preamble) is
/// NOT dropped: it is re-anchored into the `<script setup>` block at a real Vue position, with
/// EXACT import text and exactly one import in the applied source.
#[test]
fn auto_import_at_synthetic_offset_is_reanchored_into_script_setup() {
    let (vue_source, tsx, mapper) = faithful_no_import_fixture();
    let tsx_li = LineIndex::new_utf16(tsx);
    let vue_li = LineIndex::new_utf16(vue_source);

    // The provider inserts a brand-new import at offset 0 (inside the synthetic preamble),
    // exactly as tsserver/TSGO do when no user import exists to anchor to.
    let provider_edits = vec![ProviderImportEdit {
        start: 0,
        end: 0,
        new_text: "import { useRoute } from 'vue-router'\n".to_string(),
    }];

    // No user imports → anchor at the script-setup content start.
    let anchor = resolve_script_import_anchor(vue_source, &[]);
    assert!(
        matches!(
            anchor,
            ScriptImportInsertionAnchor::ExistingScriptSetup { .. }
        ),
        "an existing <script setup> must resolve to an in-block anchor, got {anchor:?}"
    );

    let edits = translate_completion_import_edits(
        &provider_edits,
        Some(&anchor),
        &tsx_li,
        &mapper,
        &vue_li,
    )
    .expect("the auto-import edit must translate, not be dropped");

    assert_eq!(edits.len(), 1, "the single auto-import edit survives");
    let edit = &edits[0];
    // EXACT inserted text (not just substring presence).
    assert_eq!(
        edit.new_text, "import { useRoute } from 'vue-router'\n",
        "the surviving edit is the verbatim import insertion"
    );
    // Inserted into the `<script setup>` content (line 1), NOT the template or past EOF.
    assert_eq!(
        edit.range.start.line, 1,
        "the import lands in the <script setup> block on its own line, got line {}",
        edit.range.start.line
    );
    assert_eq!(edit.range.start.character, 0);

    // Applied-source: exactly one import, no duplicate/spurious line, structure preserved.
    let applied = apply_edits(vue_source, &edits);
    assert_eq!(
        applied
            .matches("import { useRoute } from 'vue-router'")
            .count(),
        1,
        "exactly one import is inserted, no duplicate:\n{applied}"
    );
    assert!(
        applied.contains("<script setup lang=\"ts\">") && applied.contains("const count = 0"),
        "the original block and user code are preserved:\n{applied}"
    );
}

/// Run the production import analyzer (`verter_semantic::analysis::build_script_analysis`) over a
/// **position-preserving SFC-offset** script source — `<script setup>` content kept at its raw SFC
/// byte offsets, every other byte whitespace-blanked (newlines preserved) — mirroring
/// `verter_session`'s `extract_vue_script_content` (`crates/verter_session/src/parse.rs`). The
/// returned `AnalyzedImport.span`s are therefore SFC-absolute exactly as production produces them,
/// so a test can pin the real span source instead of a hand-built `vue.find` offset.
fn analyze_setup_import_spans(vue: &str) -> Vec<(u32, u32)> {
    let blocks = scan_sfc_blocks(vue);
    let setup = blocks
        .iter()
        .find(|b| b.is_setup())
        .expect("a <script setup> block");
    let (content_start, content_end) = setup.content_range();

    let mut src = String::with_capacity(vue.len());
    for (i, ch) in vue.char_indices() {
        let i = i as u32;
        if i >= content_start && i < content_end {
            src.push(ch);
        } else if ch == '\n' {
            src.push('\n');
        } else {
            // Preserve byte offsets for any non-script byte (ASCII fixtures: one space each).
            for _ in 0..ch.len_utf8() {
                src.push(' ');
            }
        }
    }

    let alloc = oxc_allocator::Allocator::new();
    verter_semantic::analysis::build_script_analysis(&src, oxc_span::SourceType::ts(), &alloc)
        .imports
        .iter()
        .map(|imp| (imp.span.start, imp.span.end))
        .collect()
}

/// [P1] The anchor authority consumes the **SFC-absolute** `AnalyzedImport.span` end directly.
///
/// Drives the real production coordinate space: the import span is produced by the PRODUCTION
/// analyzer (`build_script_analysis` over a position-preserving SFC-offset source — see
/// [`analyze_setup_import_spans`]), not a hand-built `vue.find` offset, so the test pins the actual
/// span source. The new import must land at the START of the line after the last existing import. A
/// double-offset regression (re-adding the block's `content_start` to an already-absolute end)
/// overshoots the block and falls back to the script content start — landing one line too early.
#[test]
fn existing_import_anchor_consumes_sfc_absolute_span_ends() {
    let vue = concat!(
        "<script setup lang=\"ts\">\n", // line 0
        "import { ref } from 'vue'\n",  // line 1 (existing user import)
        "const count = ref(0)\n",       // line 2
        "</script>\n",                  // line 3
    );
    // SFC-absolute (start, end) spans straight from the production analyzer.
    let import_spans = analyze_setup_import_spans(vue);
    assert_eq!(
        import_spans.len(),
        1,
        "exactly one import analyzed, got {import_spans:?}"
    );
    // Cross-check the analyzer's span end against the source `'vue'` terminator to pin that the
    // analyzer reports SFC-absolute (not content-relative) offsets.
    let expected_end = (vue.find("'vue'").unwrap() + "'vue'".len()) as u32;
    assert_eq!(
        import_spans[0].1, expected_end,
        "AnalyzedImport.span.end is SFC-absolute (ends at the `'vue'` specifier)"
    );

    let anchor = resolve_script_import_anchor(vue, &import_spans);
    let ScriptImportInsertionAnchor::ExistingScriptSetup { offset } = anchor else {
        panic!("expected ExistingScriptSetup, got {anchor:?}");
    };

    let vue_li = LineIndex::new_utf16(vue);
    let pos = vue_li
        .offset_to_position(offset)
        .expect("the anchor offset is a valid Vue position");
    // Line 0 = tag, line 1 = existing import, line 2 = `const count` → the new import is inserted
    // at the start of line 2 (after the import block, before the first statement). A double-offset
    // regression would fall back to the content start (line 1).
    assert_eq!(
        pos.line, 2,
        "new import anchors after the last existing import (line 2), got line {} — a double-offset \
         regression falls back to the script content start",
        pos.line
    );
    assert_eq!(pos.character, 0);

    // Applied-source: the existing import is preserved (not duplicated) and the new one is added.
    let edit = anchor
        .build_edit(&["import { computed } from 'vue'\n".to_string()], &vue_li)
        .expect("anchor builds a valid edit");
    assert_eq!(edit.new_text, "import { computed } from 'vue'\n");
    let applied = apply_edits(vue, &[edit]);
    assert_eq!(
        applied.matches("import { ref } from 'vue'").count(),
        1,
        "the existing import is not duplicated or removed:\n{applied}"
    );
    assert_eq!(
        applied.matches("import { computed } from 'vue'").count(),
        1,
        "the new import is inserted exactly once:\n{applied}"
    );
}

/// [P1] Imports in a separate non-setup `<script>` block must NOT anchor the `<script setup>`
/// insertion — they are filtered out by the selected block's content range.
#[test]
fn imports_outside_the_setup_block_do_not_anchor_it() {
    let vue = concat!(
        "<script>\n",                    // line 0
        "import { foo } from './foo'\n", // line 1 (NON-setup block import)
        "export default {}\n",           // line 2
        "</script>\n",                   // line 3
        "<script setup lang=\"ts\">\n",  // line 4
        "const x = 1\n",                 // line 5
        "</script>\n",                   // line 6
    );
    // SFC-absolute span of the non-setup import.
    let foo_start = vue.find("import { foo }").unwrap() as u32;
    let foo_end = (vue.find("'./foo'").unwrap() + "'./foo'".len()) as u32;

    let anchor = resolve_script_import_anchor(vue, &[(foo_start, foo_end)]);
    let ScriptImportInsertionAnchor::ExistingScriptSetup { offset } = anchor else {
        panic!("expected ExistingScriptSetup, got {anchor:?}");
    };

    let vue_li = LineIndex::new_utf16(vue);
    let pos = vue_li
        .offset_to_position(offset)
        .expect("the anchor offset is a valid Vue position");
    // The non-setup import is filtered out → the anchor falls back to the `<script setup>` content
    // start (line 5), NOT after the non-setup import (line 2).
    assert_eq!(
        pos.line, 5,
        "a non-setup <script> import must not anchor the <script setup> insertion, got line {}",
        pos.line
    );
    assert_eq!(pos.character, 0);
}

/// No `<script setup>` source at all: the anchor synthesizes a real `<script setup>` block
/// wrapping the import (Volar parity), rather than dropping the edit.
#[test]
fn auto_import_without_script_setup_creates_a_real_block() {
    let vue = concat!(
        "<template>\n",           //
        "  <div>{{ x }}</div>\n", //
        "</template>\n",          //
    );

    let anchor = resolve_script_import_anchor(vue, &[]);
    match &anchor {
        ScriptImportInsertionAnchor::CreateScriptSetup {
            offset, open_tag, ..
        } => {
            assert_eq!(
                *offset, 0,
                "the new block is inserted at the top of the SFC"
            );
            assert!(
                open_tag.contains("<script setup"),
                "creates a real <script setup> block, got {open_tag:?}"
            );
        }
        other => panic!("expected CreateScriptSetup, got {other:?}"),
    }

    let vue_li = LineIndex::new_utf16(vue);
    let edit = anchor
        .build_edit(
            &["import { useRoute } from 'vue-router'\n".to_string()],
            &vue_li,
        )
        .expect("anchor builds a valid edit");
    assert_eq!(edit.range.start.line, 0);
    assert_eq!(edit.range.start.character, 0);
    assert!(edit.new_text.contains("<script setup"));
    assert!(edit.new_text.contains("import { useRoute }"));
    assert!(edit.new_text.contains("</script>"));
}

/// [P2] A NON-zero-width unmapped edit (a replacement of synthetic code) is rejected, even though
/// it overlaps the preamble region — only zero-width insertions are re-anchored.
#[test]
fn non_zerowidth_unmapped_edit_is_rejected() {
    let (vue_source, tsx, mapper) = faithful_no_import_fixture();
    let tsx_li = LineIndex::new_utf16(tsx);
    let vue_li = LineIndex::new_utf16(vue_source);

    let provider_edits = vec![ProviderImportEdit {
        start: 0,
        end: 5,
        new_text: "xxxxx".to_string(),
    }];
    let anchor = resolve_script_import_anchor(vue_source, &[]);
    let result = translate_completion_import_edits(
        &provider_edits,
        Some(&anchor),
        &tsx_li,
        &mapper,
        &vue_li,
    );
    assert_eq!(
        result,
        Err(AutoImportEditMappingError::UnmappableEdit { start: 0, end: 5 }),
        "a non-zero-width unmapped edit must reject the resolve, not be anchored as an import"
    );
}

/// [P2] A zero-width unmapped edit in the TRAILING synthetic region (the `export default …` line)
/// is rejected — it is not the helper-import preamble, so it must not be spliced as an import.
#[test]
fn zerowidth_edit_in_trailing_synthetic_region_is_rejected() {
    let (vue_source, tsx, mapper) = faithful_no_import_fixture();
    let tsx_li = LineIndex::new_utf16(tsx);
    let vue_li = LineIndex::new_utf16(vue_source);

    let export_off = tsx.find("export default").unwrap() as u32;
    let provider_edits = vec![ProviderImportEdit {
        start: export_off,
        end: export_off,
        new_text: "import { x } from 'y'\n".to_string(),
    }];
    let anchor = resolve_script_import_anchor(vue_source, &[]);
    let result = translate_completion_import_edits(
        &provider_edits,
        Some(&anchor),
        &tsx_li,
        &mapper,
        &vue_li,
    );
    assert_eq!(
        result,
        Err(AutoImportEditMappingError::UnmappableEdit {
            start: export_off,
            end: export_off,
        }),
        "a zero-width edit in a non-preamble synthetic region must be rejected"
    );
}

/// [P2] An out-of-range unmapped edit (offset past the generated TSX) is rejected.
#[test]
fn out_of_range_unmapped_edit_is_rejected() {
    let (vue_source, tsx, mapper) = faithful_no_import_fixture();
    let tsx_li = LineIndex::new_utf16(tsx);
    let vue_li = LineIndex::new_utf16(vue_source);

    let oob = (tsx.len() + 100) as u32;
    let provider_edits = vec![ProviderImportEdit {
        start: oob,
        end: oob,
        new_text: "import { x } from 'y'\n".to_string(),
    }];
    let anchor = resolve_script_import_anchor(vue_source, &[]);
    let result = translate_completion_import_edits(
        &provider_edits,
        Some(&anchor),
        &tsx_li,
        &mapper,
        &vue_li,
    );
    assert_eq!(
        result,
        Err(AutoImportEditMappingError::UnmappableEdit {
            start: oob,
            end: oob,
        }),
        "an out-of-range edit must be rejected, not anchored"
    );
}

/// [P2] NO mapped runs (an empty `<script setup>`): a zero-width edit in the TRAILING synthetic
/// region (the component wrapper body, strictly AFTER the helper-import preamble) is REJECTED.
///
/// This is the convergent re-review finding. With no mapped runs there is no first-mapped-run to
/// bound the preamble, so the previous classifier treated the WHOLE unmapped file as preamble and
/// re-anchored ANY zero-width edit — splicing trailing synthetic code into the user's import block.
/// The fix is an explicit, typed helper-import-preamble end boundary carried on the source map: an
/// edit past it is no longer a preamble insertion. RED against the old `None => true`; GREEN with
/// the boundary gate.
#[test]
fn no_mapped_run_trailing_synthetic_edit_is_rejected() {
    let (vue_source, tsx, mapper) = faithful_no_mapped_run_fixture();
    let tsx_li = LineIndex::new_utf16(tsx);
    let vue_li = LineIndex::new_utf16(vue_source);

    // Strictly AFTER the boundary (line 2, col 0): the `return {};` body line of the synthetic
    // wrapper. A real auto-import never lands here; only mis-classification would re-anchor it.
    let trailing_off = tsx.find("return {};").unwrap() as u32;
    let provider_edits = vec![ProviderImportEdit {
        start: trailing_off,
        end: trailing_off,
        new_text: "import { x } from 'y'\n".to_string(),
    }];
    let anchor = resolve_script_import_anchor(vue_source, &[]);
    let result = translate_completion_import_edits(
        &provider_edits,
        Some(&anchor),
        &tsx_li,
        &mapper,
        &vue_li,
    );
    assert_eq!(
        result,
        Err(AutoImportEditMappingError::UnmappableEdit {
            start: trailing_off,
            end: trailing_off,
        }),
        "with no mapped runs, a zero-width edit past the helper-import preamble must be rejected, \
         not re-anchored into user source"
    );
}

/// [P2] NO mapped runs (an empty `<script setup>`): a zero-width auto-import INSIDE the
/// helper-import preamble is still classified as a preamble insertion and re-anchored into the Vue
/// `<script setup>` block (it is not dropped). The success twin of the rejection above.
#[test]
fn no_mapped_run_preamble_edit_is_reanchored() {
    let (vue_source, tsx, mapper) = faithful_no_mapped_run_fixture();
    let tsx_li = LineIndex::new_utf16(tsx);
    let vue_li = LineIndex::new_utf16(vue_source);

    // Offset 0 — inside the leading helper-import preamble (line 0).
    let provider_edits = vec![ProviderImportEdit {
        start: 0,
        end: 0,
        new_text: "import { useRoute } from 'vue-router'\n".to_string(),
    }];
    let anchor = resolve_script_import_anchor(vue_source, &[]);
    let edits = translate_completion_import_edits(
        &provider_edits,
        Some(&anchor),
        &tsx_li,
        &mapper,
        &vue_li,
    )
    .expect("a preamble auto-import must re-anchor even with no mapped runs, not be dropped");

    assert_eq!(edits.len(), 1, "the single auto-import edit survives");
    assert_eq!(
        edits[0].new_text, "import { useRoute } from 'vue-router'\n",
        "the re-anchored import keeps exact text"
    );
    assert_eq!(
        edits[0].range.start, edits[0].range.end,
        "re-anchored as a zero-width insertion into the <script setup> block"
    );
}

/// All-or-nothing: when a resolve returns several edits and an unmapped preamble import cannot be
/// re-anchored (no Vue insertion anchor available), the WHOLE resolve is rejected — the old
/// `filter_map` would have silently kept the mapped edit and dropped the unmapped one.
#[test]
fn multiple_edits_fail_structurally_when_unmapped_edit_cannot_anchor() {
    let (vue_source, tsx, mapper) = faithful_no_import_fixture();
    let tsx_li = LineIndex::new_utf16(tsx);
    let vue_li = LineIndex::new_utf16(vue_source);

    let user_code_off = tsx.find("const count").unwrap() as u32;
    let provider_edits = vec![
        // A mapped edit targeting real user source (round-trips through the strict mapper).
        ProviderImportEdit {
            start: user_code_off,
            end: user_code_off + 5,
            new_text: "const".to_string(),
        },
        // An unmapped auto-import in the synthetic preamble.
        ProviderImportEdit {
            start: 0,
            end: 0,
            new_text: "import { useRoute } from 'vue-router'\n".to_string(),
        },
    ];

    // No anchor available → the unmapped preamble import cannot be placed → reject the whole
    // resolve (NOT `UnmappableEdit`: the edit IS a valid preamble insertion; it simply has nowhere
    // to land).
    let result =
        translate_completion_import_edits(&provider_edits, None, &tsx_li, &mapper, &vue_li);
    assert_eq!(
        result,
        Err(AutoImportEditMappingError::NoInsertionAnchor),
        "an unplaceable preamble import must reject the whole resolve, not produce a partial set"
    );
}

/// The success twin of the all-or-nothing rule: with an anchor available, a mapped edit is
/// applied verbatim AND the unmapped preamble auto-import is re-anchored — both survive with
/// exact text, and the applied source has exactly one import.
#[test]
fn multiple_edits_map_completely_when_anchor_available() {
    let (vue_source, tsx, mapper) = faithful_no_import_fixture();
    let tsx_li = LineIndex::new_utf16(tsx);
    let vue_li = LineIndex::new_utf16(vue_source);

    // A mapped edit renaming the identifier `count` (col 6..11 on the user-code line); it does
    // not overlap the import anchor at col 0, so the applied source is unambiguous.
    let count_off = tsx.find("count").unwrap() as u32;
    let provider_edits = vec![
        ProviderImportEdit {
            start: count_off,
            end: count_off + "count".len() as u32,
            new_text: "value".to_string(),
        },
        ProviderImportEdit {
            start: 0,
            end: 0,
            new_text: "import { useRoute } from 'vue-router'\n".to_string(),
        },
    ];

    let anchor = resolve_script_import_anchor(vue_source, &[]);
    let edits = translate_completion_import_edits(
        &provider_edits,
        Some(&anchor),
        &tsx_li,
        &mapper,
        &vue_li,
    )
    .expect("both edits map");

    assert_eq!(
        edits.len(),
        2,
        "the mapped edit and the re-anchored import both survive"
    );
    // Exactly one edit is the import insertion, with EXACT text.
    let import_edits: Vec<&TextEdit> = edits
        .iter()
        .filter(|e| e.new_text.contains("import {"))
        .collect();
    assert_eq!(
        import_edits.len(),
        1,
        "exactly one import edit, no duplicate"
    );
    assert_eq!(
        import_edits[0].new_text, "import { useRoute } from 'vue-router'\n",
        "the re-anchored import has exact text"
    );

    // Applied-source: the rename lands, the import is inserted exactly once.
    let applied = apply_edits(vue_source, &edits);
    assert_eq!(
        applied
            .matches("import { useRoute } from 'vue-router'")
            .count(),
        1,
        "exactly one import in the applied source:\n{applied}"
    );
    assert!(
        applied.contains("const value = 0"),
        "the mapped rename was applied verbatim:\n{applied}"
    );
}

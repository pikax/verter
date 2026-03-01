//! Source map span remapping for preprocessed style overrides.
//!
//! When a `<style lang="sass">` block is transpiled to CSS by the extension,
//! the CSS analysis spans reference byte offsets in the compiled CSS. This module
//! remaps those spans back to byte offsets in the original preprocessor source
//! (relative to the style block content start in the SFC).

use sourcemap::SourceMap;
use verter_analysis::style::CssAnalysis;

/// Remap all spans in a `CssAnalysis` from compiled CSS byte offsets to
/// original source byte offsets using a source map.
///
/// - `analysis`: CSS analysis with spans relative to compiled CSS content start.
/// - `compiled_css`: The compiled CSS content (used to compute line/col from byte offsets).
/// - `source_map_json`: The source map JSON string from the preprocessor.
/// - `original_content`: The original preprocessor source content (used to compute byte offsets from line/col).
///
/// Returns `true` if remapping succeeded, `false` if the source map couldn't be parsed.
pub fn remap_css_analysis_spans(
    analysis: &mut CssAnalysis,
    compiled_css: &str,
    source_map_json: &str,
    original_content: &str,
) -> bool {
    let sm = match SourceMap::from_slice(source_map_json.as_bytes()) {
        Ok(sm) => sm,
        Err(_) => return false,
    };

    let compiled_lines = LineStarts::new(compiled_css);
    let original_lines = LineStarts::new(original_content);

    // Remap selector spans
    for sel in &mut analysis.selectors {
        remap_span(
            &mut sel.span.start,
            &mut sel.span.end,
            &sm,
            &compiled_lines,
            &original_lines,
        );
    }

    // Remap class spans
    for cls in &mut analysis.classes {
        remap_span(
            &mut cls.span.start,
            &mut cls.span.end,
            &sm,
            &compiled_lines,
            &original_lines,
        );
    }

    // Remap ID spans
    for id in &mut analysis.ids {
        remap_span(
            &mut id.span.start,
            &mut id.span.end,
            &sm,
            &compiled_lines,
            &original_lines,
        );
    }

    true
}

/// Remap a single span (start, end) from compiled CSS offsets to original source offsets.
fn remap_span(
    start: &mut u32,
    end: &mut u32,
    sm: &SourceMap,
    compiled_lines: &LineStarts,
    original_lines: &LineStarts,
) {
    if let Some(new_start) = remap_offset(*start, sm, compiled_lines, original_lines) {
        if let Some(new_end) = remap_offset(*end, sm, compiled_lines, original_lines) {
            *start = new_start;
            *end = new_end;
        }
    }
}

/// Remap a single byte offset from compiled CSS to original source.
///
/// 1. Convert byte offset → (line, col) in compiled CSS
/// 2. Look up in source map → nearest preceding token
/// 3. Compute delta from the token's generated position to our actual position
/// 4. Apply delta to the token's original position → byte offset in original source
///
/// The delta preservation is critical: source maps only have discrete mappings
/// (e.g., at the start of a selector `.foo`), but CSS analysis spans may point
/// to sub-token positions (e.g., the class name `foo` after the `.`). Without
/// delta preservation, all positions within a mapped region collapse to the
/// token's start position.
fn remap_offset(
    byte_offset: u32,
    sm: &SourceMap,
    compiled_lines: &LineStarts,
    original_lines: &LineStarts,
) -> Option<u32> {
    let (gen_line, gen_col) = compiled_lines.offset_to_line_col(byte_offset as usize)?;

    // Source map uses 0-based line/col
    let token = sm.lookup_token(gen_line as u32, gen_col as u32)?;

    // Compute delta: how far past the token's generated position our offset is.
    // This preserves sub-token precision (e.g., class name offset after '.').
    let col_delta = if token.get_dst_line() == gen_line as u32 {
        gen_col as u32 - token.get_dst_col()
    } else {
        // Different line — can't compute meaningful column delta
        0
    };

    let src_line = token.get_src_line();
    let src_col = token.get_src_col() + col_delta;

    original_lines.line_col_to_offset(src_line as usize, src_col as usize)
}

/// Pre-computed line start offsets for fast byte-offset ↔ line:col conversion.
struct LineStarts {
    /// Byte offset of each line start. `starts[0] = 0` always.
    starts: Vec<usize>,
}

impl LineStarts {
    fn new(text: &str) -> Self {
        let mut starts = vec![0usize];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        Self { starts }
    }

    /// Convert a byte offset to 0-based (line, col).
    fn offset_to_line_col(&self, offset: usize) -> Option<(usize, usize)> {
        let line = match self.starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.checked_sub(1)?,
        };
        let col = offset - self.starts[line];
        Some((line, col))
    }

    /// Convert 0-based (line, col) to a byte offset.
    fn line_col_to_offset(&self, line: usize, col: usize) -> Option<u32> {
        let line_start = *self.starts.get(line)?;
        Some((line_start + col) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sourcemap::SourceMapBuilder;

    #[test]
    fn line_starts_basic() {
        let text = "abc\ndef\nghi";
        let ls = LineStarts::new(text);
        assert_eq!(ls.starts, vec![0, 4, 8]);
        assert_eq!(ls.offset_to_line_col(0), Some((0, 0)));
        assert_eq!(ls.offset_to_line_col(3), Some((0, 3)));
        assert_eq!(ls.offset_to_line_col(4), Some((1, 0)));
        assert_eq!(ls.offset_to_line_col(7), Some((1, 3)));
        assert_eq!(ls.offset_to_line_col(8), Some((2, 0)));
    }

    #[test]
    fn line_col_round_trip() {
        let text = "abc\ndef\nghi";
        let ls = LineStarts::new(text);
        for offset in 0..text.len() {
            let (line, col) = ls.offset_to_line_col(offset).unwrap();
            assert_eq!(ls.line_col_to_offset(line, col), Some(offset as u32));
        }
    }

    /// Build a source map JSON string from a list of (dst_line, dst_col, src_line, src_col) tuples.
    fn build_source_map(original: &str, mappings: &[(u32, u32, u32, u32)]) -> String {
        let mut builder = SourceMapBuilder::new(Some("output.css"));
        let src_id = builder.add_source("input.sass");
        builder.set_source_contents(src_id, Some(original));

        for &(dst_line, dst_col, src_line, src_col) in mappings {
            builder.add_raw(
                dst_line,
                dst_col,
                src_line,
                src_col,
                Some(src_id),
                None,
                false,
            );
        }

        let sm = builder.into_sourcemap();
        let mut buf = Vec::new();
        sm.to_writer(&mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    // ── Test 1: Source map accuracy for transpiled selectors ──

    /// Simulates Sass → CSS transpilation with a source map and verifies that
    /// CSS analysis spans are correctly remapped to original Sass positions.
    #[test]
    fn remap_selector_spans_with_source_map() {
        // Original Sass (indented syntax):
        //  line 0: ".container"
        //  line 1: "  color: red"
        //  line 2: "  .child"
        //  line 3: "    display: flex"
        let original = ".container\n  color: red\n  .child\n    display: flex\n";

        // Compiled CSS:
        //  line 0: ".container {"
        //  line 1: "  color: red;"
        //  line 2: "}"
        //  line 3: ".container .child {"
        //  line 4: "  display: flex;"
        //  line 5: "}"
        let compiled = ".container {\n  color: red;\n}\n.container .child {\n  display: flex;\n}\n";

        // Source map: maps compiled positions → original Sass positions
        // .container at compiled (0, 0) → original (0, 0)
        // .container .child at compiled (3, 0) → original (2, 2)
        let sm_json = build_source_map(
            original,
            &[
                (0, 0, 0, 0), // .container → line 0, col 0
                (1, 2, 1, 2), // color: red → line 1, col 2
                (3, 0, 2, 2), // .container .child → line 2, col 2
                (4, 2, 3, 4), // display: flex → line 3, col 4
            ],
        );

        // Run CSS analysis on compiled CSS
        let analysis = verter_analysis::build_css_style_analysis(
            compiled,
            verter_analysis::VueStyleInput::default(),
            false,
            false,
            None,
            0,
        );

        let mut css = analysis.css.unwrap();

        // Before remapping, verify spans are in compiled CSS space
        assert!(
            css.selectors.len() >= 2,
            "should find at least 2 selectors, found {}",
            css.selectors.len()
        );

        // Apply remapping
        let success = remap_css_analysis_spans(&mut css, compiled, &sm_json, original);
        assert!(success, "source map remapping should succeed");

        // After remapping, selector spans should point to original Sass
        let container_sel = css.selectors.iter().find(|s| s.text == ".container");
        assert!(container_sel.is_some(), ".container selector should exist");
        let container_sel = container_sel.unwrap();
        // .container starts at offset 0 in original
        assert_eq!(
            container_sel.span.start, 0,
            ".container should start at offset 0 in original"
        );

        let child_sel = css.selectors.iter().find(|s| s.text.contains(".child"));
        assert!(child_sel.is_some(), ".child selector should exist");
        let child_sel = child_sel.unwrap();
        // .child is at line 2, col 2 in original = offset 2 on that line
        // Line 2 starts at offset 23 (.container\n=11, + "  color: red\n"=13 => 24..wait)
        // Let me recount: ".container\n" = 11 bytes, "  color: red\n" = 13 bytes => line 2 starts at 24
        // ".child" at col 2 => offset 24 + 2 = 26
        assert_eq!(
            child_sel.span.start, 26,
            ".child selector should start at offset 26 in original (line 2, col 2)"
        );
    }

    // ── Test 2: Diagnostic position correctness ──

    /// Verifies that after remapping, CSS class spans point to the correct
    /// positions in the original source (for unused CSS diagnostic targets).
    #[test]
    fn remap_class_spans_for_diagnostics() {
        // Original (Sass-like):
        //  line 0: ".used"
        //  line 1: "  color: red"
        //  line 2: ".unused"
        //  line 3: "  color: blue"
        let original = ".used\n  color: red\n.unused\n  color: blue\n";

        // Compiled CSS:
        //  line 0: ".used { color: red; }"
        //  line 1: ".unused { color: blue; }"
        let compiled = ".used { color: red; }\n.unused { color: blue; }\n";

        let sm_json = build_source_map(
            original,
            &[
                (0, 0, 0, 0), // .used → line 0, col 0
                (1, 0, 2, 0), // .unused → line 2, col 0
            ],
        );

        let analysis = verter_analysis::build_css_style_analysis(
            compiled,
            verter_analysis::VueStyleInput::default(),
            false,
            false,
            None,
            0,
        );
        let mut css = analysis.css.unwrap();

        let success = remap_css_analysis_spans(&mut css, compiled, &sm_json, original);
        assert!(success);

        // .used class should be at offset 1 (after '.') in original, i.e. byte 1
        let used_cls = css.classes.iter().find(|c| c.name == "used");
        assert!(used_cls.is_some(), ".used class should exist in analysis");
        let used_cls = used_cls.unwrap();
        assert_eq!(
            used_cls.span.start, 1,
            ".used class name should start at offset 1 (after '.')"
        );

        // .unused class at line 2 => line starts at offset 18 (".used\n"=6 + "  color: red\n"=13 => 19? wait)
        // ".used\n" = 6, "  color: red\n" = 14. Line 2 starts at 20.
        // ".unused" → "unused" after '.' = offset 21
        let unused_cls = css.classes.iter().find(|c| c.name == "unused");
        assert!(
            unused_cls.is_some(),
            ".unused class should exist in analysis"
        );
        let unused_cls = unused_cls.unwrap();
        // Verify it points into the original, not the compiled CSS
        let expected_offset = original.find(".unused").unwrap() as u32 + 1; // +1 for '.'
        assert_eq!(
            unused_cls.span.start, expected_offset,
            ".unused class should point to offset {} in original",
            expected_offset
        );
    }

    // ── Test 3: Go-to-definition correctness ──

    /// Verifies that class spans are correctly remapped for template→CSS navigation.
    #[test]
    fn remap_preserves_selector_text_and_structure() {
        let original = ".header\n  font-size: 16px\n.footer\n  font-size: 12px\n";
        let compiled = ".header { font-size: 16px; }\n.footer { font-size: 12px; }\n";

        let sm_json = build_source_map(
            original,
            &[
                (0, 0, 0, 0), // .header → original line 0
                (1, 0, 2, 0), // .footer → original line 2
            ],
        );

        let analysis = verter_analysis::build_css_style_analysis(
            compiled,
            verter_analysis::VueStyleInput::default(),
            false,
            false,
            None,
            0,
        );
        let mut css = analysis.css.unwrap();

        let success = remap_css_analysis_spans(&mut css, compiled, &sm_json, original);
        assert!(success);

        // Selector text should be preserved (not affected by remapping)
        assert_eq!(css.selectors[0].text, ".header");
        assert_eq!(css.selectors[1].text, ".footer");

        // Selector structure should be preserved
        assert!(css.selectors[0].structure.is_some());
        assert!(css.selectors[1].structure.is_some());

        // .header span should point to original offset 0
        assert_eq!(css.selectors[0].span.start, 0);

        // .footer at line 2 in original = ".header\n" (8) + "  font-size: 16px\n" (18) = 26
        let footer_offset = original.find(".footer").unwrap() as u32;
        assert_eq!(css.selectors[1].span.start, footer_offset);
    }

    // ── Test 4: Round-trip position mapping ──

    /// Verifies no off-by-one errors: for a trivial 1:1 mapping (no transformation),
    /// remapped offsets should equal the originals.
    #[test]
    fn identity_source_map_preserves_offsets() {
        // Same content for both — identity mapping
        let content = ".a { color: red; }\n.b { color: blue; }\n";

        // Identity source map: each line maps to itself
        let sm_json = build_source_map(
            content,
            &[
                (0, 0, 0, 0),
                (0, 1, 0, 1),
                (0, 4, 0, 4),
                (1, 0, 1, 0),
                (1, 1, 1, 1),
                (1, 4, 1, 4),
            ],
        );

        let analysis = verter_analysis::build_css_style_analysis(
            content,
            verter_analysis::VueStyleInput::default(),
            false,
            false,
            None,
            0,
        );
        let mut css = analysis.css.clone().unwrap();
        let original_css = analysis.css.unwrap();

        let success = remap_css_analysis_spans(&mut css, content, &sm_json, content);
        assert!(success);

        // With identity mapping, spans should be unchanged
        for (remapped, original) in css.selectors.iter().zip(original_css.selectors.iter()) {
            assert_eq!(
                remapped.span.start, original.span.start,
                "selector '{}' start should be unchanged with identity map",
                remapped.text
            );
            assert_eq!(
                remapped.span.end, original.span.end,
                "selector '{}' end should be unchanged with identity map",
                remapped.text
            );
        }

        for (remapped, original) in css.classes.iter().zip(original_css.classes.iter()) {
            assert_eq!(
                remapped.span.start, original.span.start,
                "class '{}' start should be unchanged with identity map",
                remapped.name
            );
        }
    }

    // ── Test 5: Invalid source map graceful fallback ──

    /// Verifies that an invalid source map doesn't crash and returns false.
    #[test]
    fn invalid_source_map_returns_false() {
        let compiled = ".a { color: red; }\n";
        let original = ".a\n  color: red\n";

        let analysis = verter_analysis::build_css_style_analysis(
            compiled,
            verter_analysis::VueStyleInput::default(),
            false,
            false,
            None,
            0,
        );
        let mut css = analysis.css.unwrap();

        let result = remap_css_analysis_spans(&mut css, compiled, "not valid json", original);
        assert!(!result, "should return false for invalid source map");
    }
}

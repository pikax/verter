//! Shared CSS selector-walking utility for normalized CSS.
//!
//! Walks through CSS that has been normalized by lightningcss (no nested rules,
//! well-formed comments/strings), finds selectors before `{`, and applies a
//! caller-provided transformation function to each selector list.

/// Walk normalized CSS, transforming selectors before each `{`.
///
/// For every selector list found before an opening brace, calls `transform_fn`
/// with the trimmed selector text and expects the transformed selector back.
/// Skips `@`-rule headers (e.g. `@media (...)`, `@supports (...)`), but
/// transforms selectors INSIDE those blocks. Also skips `@keyframes` selectors
/// (`from`, `to`, `0%`, etc.), comments, and strings.
///
/// **Precondition:** The input CSS must be normalized by lightningcss first
/// (via [`super::normalize_css`]). The normalization flattens nested rules and
/// ensures well-formed comments/strings, which this walker relies on.
pub fn walk_and_transform_selectors(
    css: &str,
    mut transform_fn: impl FnMut(&str) -> String,
) -> String {
    let mut output = String::with_capacity(css.len() + 256);
    let mut chars = css.char_indices().peekable();
    let mut in_string = false;
    let mut string_char = '"';
    let mut in_comment = false;
    let mut last_block_end: usize = 0;
    // Track nesting depth inside @keyframes blocks so selectors within
    // (from, to, 0%, 50%, etc.) are not transformed.
    let mut keyframes_depth: usize = 0;
    let mut brace_depth: usize = 0;
    // The brace depth at which a @keyframes block was entered. We track
    // multiple levels in case of (unlikely) nested at-rules.
    let mut keyframes_entry_depths: Vec<usize> = Vec::new();

    while let Some((_i, c)) = chars.next() {
        match c {
            // Track comments
            '/' if !in_string && !in_comment => {
                if let Some(&(_, '*')) = chars.peek() {
                    in_comment = true;
                    output.push('/');
                    if let Some((_, c2)) = chars.next() {
                        output.push(c2);
                    }
                    continue;
                }
                output.push(c);
                continue;
            }
            '*' if in_comment => {
                output.push(c);
                if let Some(&(_, '/')) = chars.peek() {
                    in_comment = false;
                    if let Some((_, c2)) = chars.next() {
                        output.push(c2);
                    }
                }
                continue;
            }
            _ if in_comment => {
                output.push(c);
                continue;
            }
            // Track strings (with escape handling)
            '\\' if in_string => {
                output.push(c);
                if let Some((_, next)) = chars.next() {
                    output.push(next);
                }
            }
            '"' | '\'' if !in_string => {
                in_string = true;
                string_char = c;
                output.push(c);
            }
            c if in_string && c == string_char => {
                in_string = false;
                output.push(c);
            }
            // Track block boundaries
            '}' if !in_string => {
                output.push(c);
                brace_depth = brace_depth.saturating_sub(1);
                // Check if we're closing a @keyframes block
                if keyframes_entry_depths.last() == Some(&brace_depth) {
                    keyframes_entry_depths.pop();
                    keyframes_depth -= 1;
                }
                last_block_end = output.len();
            }
            // Track semicolons inside blocks for CSS nesting support.
            // Declarations end with ';', so updating last_block_end here
            // ensures nested selectors (e.g. `& .child`) are correctly
            // isolated from preceding declarations.
            ';' if !in_string && !in_comment && brace_depth > 0 => {
                output.push(c);
                last_block_end = output.len();
            }
            // Handle rule blocks
            '{' if !in_string => {
                let selector_end = output.len();
                let selector_start = last_block_end;

                if selector_start < selector_end {
                    let raw_text = output[selector_start..selector_end].to_string();
                    let trimmed = raw_text.trim();

                    // Detect @keyframes entry
                    if trimmed.starts_with("@keyframes")
                        || trimmed.starts_with("@-webkit-keyframes")
                    {
                        keyframes_depth += 1;
                        keyframes_entry_depths.push(brace_depth);
                    }

                    // Skip @-rules and selectors inside @keyframes blocks
                    if !trimmed.starts_with('@') && !trimmed.is_empty() && keyframes_depth == 0 {
                        let transformed = transform_fn(trimmed);
                        output.truncate(selector_start);
                        // Preserve leading whitespace
                        let leading_ws = &raw_text[..raw_text.len() - raw_text.trim_start().len()];
                        output.push_str(leading_ws);
                        output.push_str(&transformed);
                    }
                }

                output.push('{');
                brace_depth += 1;
                // Update last_block_end after pushing '{' so that the first
                // selector inside an @-rule block starts AFTER the '{', not
                // from the previous '}'. Without this, the first inner selector's
                // raw_text includes the @-rule prefix and gets incorrectly skipped.
                last_block_end = output.len();
            }
            _ => output.push(c),
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: collect selectors without transforming the CSS.
    fn collect_selectors(css: &str) -> Vec<String> {
        let mut selectors = Vec::new();
        walk_and_transform_selectors(css, |sel| {
            selectors.push(sel.to_string());
            sel.to_string()
        });
        selectors
    }

    // --- Basic selector extraction ---

    #[test]
    fn test_single_class_selector() {
        let selectors = collect_selectors(".box { color: red; }");
        assert_eq!(selectors, vec![".box"]);
    }

    #[test]
    fn test_multiple_rules() {
        let selectors = collect_selectors(".a { color: red; } .b { color: blue; }");
        assert_eq!(selectors, vec![".a", ".b"]);
    }

    #[test]
    fn test_comma_separated_selectors() {
        let selectors = collect_selectors(".a, .b { color: red; }");
        assert_eq!(selectors, vec![".a, .b"]);
    }

    #[test]
    fn test_descendant_selector() {
        let selectors = collect_selectors(".parent .child { color: red; }");
        assert_eq!(selectors, vec![".parent .child"]);
    }

    // --- @-rules are skipped ---

    #[test]
    fn test_at_rule_prefix_not_collected() {
        // @-rules themselves should not appear as selectors, but inner selectors
        // inside @media, @supports, etc. MUST be collected and transformed.
        let selectors = collect_selectors("@media (min-width: 600px) { .box { color: red; } }");
        assert_eq!(selectors, vec![".box"]);
    }

    // --- Comments ---

    #[test]
    fn test_comment_preserved_in_output() {
        let result = walk_and_transform_selectors("/* comment */ .box { color: red; }", |sel| {
            sel.to_string()
        });
        assert!(
            result.contains("/* comment */"),
            "Comment should be preserved. Got: {}",
            result
        );
        assert!(
            result.contains(".box"),
            "Selector should be present. Got: {}",
            result
        );
    }

    // --- Strings are not treated as block boundaries ---

    #[test]
    fn test_string_with_braces() {
        let selectors = collect_selectors(".box { content: '{ not a block }'; }");
        assert_eq!(selectors, vec![".box"]);
    }

    #[test]
    fn test_double_quoted_string() {
        let selectors = collect_selectors(".box { content: \"hello\"; }");
        assert_eq!(selectors, vec![".box"]);
    }

    #[test]
    fn test_escaped_quote_in_string() {
        let selectors = collect_selectors(r#".box { content: 'it\'s'; }"#);
        assert_eq!(selectors, vec![".box"]);
    }

    // --- Transformation works ---

    #[test]
    fn test_transform_adds_suffix() {
        let result =
            walk_and_transform_selectors(".box { color: red; }", |sel| format!("{}[scoped]", sel));
        assert!(result.contains(".box[scoped]"), "Got: {}", result);
        assert!(result.contains("{ color: red; }"), "Got: {}", result);
    }

    #[test]
    fn test_transform_multiple_rules() {
        let result =
            walk_and_transform_selectors(".a { color: red; } .b { color: blue; }", |sel| {
                format!("{}[x]", sel)
            });
        assert!(result.contains(".a[x]"), "Got: {}", result);
        assert!(result.contains(".b[x]"), "Got: {}", result);
    }

    // --- Whitespace preservation ---

    #[test]
    fn test_leading_whitespace_preserved() {
        let result = walk_and_transform_selectors("\n  .box { color: red; }", |sel| {
            format!("{}[scoped]", sel)
        });
        assert!(result.contains(".box[scoped]"), "Got: {}", result);
    }

    #[test]
    fn test_newline_between_rules() {
        let result =
            walk_and_transform_selectors(".a { color: red; }\n.b { color: blue; }", |sel| {
                format!("{}[x]", sel)
            });
        assert!(result.contains(".a[x]"), "Got: {}", result);
        assert!(result.contains(".b[x]"), "Got: {}", result);
    }

    // --- Edge cases ---

    #[test]
    fn test_empty_input() {
        let selectors = collect_selectors("");
        assert!(selectors.is_empty());
    }

    #[test]
    fn test_only_comment() {
        let selectors = collect_selectors("/* just a comment */");
        assert!(selectors.is_empty());
    }

    #[test]
    fn test_nested_at_rule_with_selector() {
        // Selectors inside @-rules must be found and transformable.
        let selectors = collect_selectors("@media screen { .inner { color: red; } }");
        assert_eq!(selectors, vec![".inner"]);
    }

    // --- Keyframe selectors should NOT be transformed ---

    /// @ai-generated - keyframe selectors (from/to) should not be transformed
    #[test]
    fn keyframe_selectors_not_transformed() {
        let css = "@keyframes fade { from { opacity: 1; } to { opacity: 0; } }";
        let result = walk_and_transform_selectors(css, |sel| format!("{}[scoped]", sel));
        assert!(
            !result.contains("from[scoped]"),
            "from should not be transformed. Got: {}",
            result
        );
        assert!(
            !result.contains("to[scoped]"),
            "to should not be transformed. Got: {}",
            result
        );
        assert!(result.contains("from"), "from should still be present");
        assert!(result.contains("to"), "to should still be present");
    }

    /// @ai-generated - keyframe percentage selectors should not be transformed
    #[test]
    fn keyframe_percentage_selectors_not_transformed() {
        let css = "@keyframes x { 0% { opacity: 0; } 50%, 100% { opacity: 1; } }";
        let result = walk_and_transform_selectors(css, |sel| format!("{}[scoped]", sel));
        assert!(
            !result.contains("0%[scoped]"),
            "0% should not be transformed. Got: {}",
            result
        );
        assert!(
            !result.contains("50%[scoped]"),
            "50% should not be transformed. Got: {}",
            result
        );
        assert!(
            !result.contains("100%[scoped]"),
            "100% should not be transformed. Got: {}",
            result
        );
    }

    /// @ai-generated - selectors after @keyframes block should still be transformed
    #[test]
    fn normal_selectors_after_keyframes_still_transformed() {
        let css =
            "@keyframes fade { from { opacity: 1; } to { opacity: 0; } } .box { color: red; }";
        let result = walk_and_transform_selectors(css, |sel| format!("{}[scoped]", sel));
        assert!(
            result.contains(".box[scoped]"),
            ".box after @keyframes should be transformed. Got: {}",
            result
        );
        assert!(
            !result.contains("from[scoped]"),
            "from should not be transformed. Got: {}",
            result
        );
    }

    // ===================================================================
    // @ai-generated - CSS nesting tests
    // ===================================================================

    /// Nested selectors with `&` must be found by the walker.
    #[test]
    fn css_nesting_nested_selectors_found() {
        let css = ".parent { color: red; & .child { color: blue; } }";
        let selectors = collect_selectors(css);
        assert_eq!(selectors, vec![".parent", "& .child"]);
    }

    /// Multiple nested selectors in one block.
    #[test]
    fn css_nesting_multiple_nested_selectors() {
        let css = ".parent { color: red; & .a { } & .b { } }";
        let selectors = collect_selectors(css);
        assert_eq!(selectors, vec![".parent", "& .a", "& .b"]);
    }

    /// Nested selector with `&:hover` pseudo-class.
    #[test]
    fn css_nesting_pseudo_class() {
        let css = ".btn { color: red; &:hover { color: blue; } }";
        let selectors = collect_selectors(css);
        assert_eq!(selectors, vec![".btn", "&:hover"]);
    }

    /// Nested selector with `&.modifier` (no space).
    #[test]
    fn css_nesting_modifier() {
        let css = ".card { padding: 1rem; &.active { border: 1px solid; } }";
        let selectors = collect_selectors(css);
        assert_eq!(selectors, vec![".card", "&.active"]);
    }

    /// Deeply nested CSS (nesting within nesting).
    #[test]
    fn css_nesting_deep() {
        let css = ".a { color: red; & .b { font-size: 14px; & .c { margin: 0; } } }";
        let selectors = collect_selectors(css);
        assert_eq!(selectors, vec![".a", "& .b", "& .c"]);
    }

    /// Nested selectors inside @media.
    #[test]
    fn css_nesting_inside_media() {
        let css =
            "@media (max-width: 768px) { .parent { color: red; & .child { display: none; } } }";
        let selectors = collect_selectors(css);
        assert_eq!(selectors, vec![".parent", "& .child"]);
    }

    /// Declarations should be preserved in output, not treated as selectors.
    #[test]
    fn css_nesting_declarations_preserved() {
        let css = ".parent { color: red; font-size: 14px; & .child { color: blue; } }";
        let result = walk_and_transform_selectors(css, |sel| format!("{}[s]", sel));
        assert!(
            result.contains("color: red"),
            "Declaration must be preserved. Got: {}",
            result
        );
        assert!(
            result.contains("font-size: 14px"),
            "Declaration must be preserved. Got: {}",
            result
        );
        assert!(
            result.contains("& .child[s]"),
            "Nested selector must be transformed. Got: {}",
            result
        );
    }

    /// @ai-generated - webkit keyframes should also skip selectors
    #[test]
    fn webkit_keyframes_selectors_not_transformed() {
        let css = "@-webkit-keyframes slide { 0% { left: 0; } 100% { left: 100%; } }";
        let result = walk_and_transform_selectors(css, |sel| format!("{}[scoped]", sel));
        assert!(
            !result.contains("0%[scoped]"),
            "0% in webkit keyframes should not be transformed. Got: {}",
            result
        );
    }

    #[test]
    fn test_attribute_selector() {
        let selectors = collect_selectors("input[type=\"text\"] { color: red; }");
        assert_eq!(selectors, vec!["input[type=\"text\"]"]);
    }

    #[test]
    fn test_pseudo_class_in_selector() {
        let selectors = collect_selectors(".btn:hover { color: red; }");
        assert_eq!(selectors, vec![".btn:hover"]);
    }

    // ===================================================================
    // @ai-generated - @-rule inner selector extraction tests
    // ===================================================================

    /// Selectors inside @media must be collected.
    #[test]
    fn media_inner_selectors_collected() {
        let selectors = collect_selectors(
            "@media (max-width: 768px) { .a { color: red; } .b { color: blue; } }",
        );
        assert_eq!(selectors, vec![".a", ".b"]);
    }

    /// Selectors inside @supports must be collected.
    #[test]
    fn supports_inner_selectors_collected() {
        let selectors = collect_selectors("@supports (display: grid) { .grid { display: grid; } }");
        assert_eq!(selectors, vec![".grid"]);
    }

    /// Selectors inside @layer must be collected.
    #[test]
    fn layer_inner_selectors_collected() {
        let selectors = collect_selectors("@layer base { .box { color: red; } }");
        assert_eq!(selectors, vec![".box"]);
    }

    /// Multiple @media blocks, each with multiple selectors.
    #[test]
    fn multiple_media_blocks_selectors() {
        let selectors = collect_selectors(
            ".top { color: red; } \
             @media (max-width: 768px) { .a { } .b { } } \
             @media (min-width: 1200px) { .c { } } \
             .bottom { color: blue; }",
        );
        assert_eq!(selectors, vec![".top", ".a", ".b", ".c", ".bottom"]);
    }

    /// Nested @media and @supports — deeply nested selector must be collected.
    #[test]
    fn nested_at_rules_inner_selectors() {
        let selectors = collect_selectors(
            "@media (min-width: 768px) { @supports (display: grid) { .nested { } } }",
        );
        assert_eq!(selectors, vec![".nested"]);
    }

    /// @font-face has no selectors — nothing should be collected.
    #[test]
    fn font_face_no_selectors() {
        let selectors = collect_selectors(
            "@font-face { font-family: MyFont; src: url('f.woff'); } .text { color: red; }",
        );
        assert_eq!(selectors, vec![".text"]);
    }

    /// Transform inside @media actually applies the transformation.
    #[test]
    fn media_inner_selectors_transformed() {
        let result = walk_and_transform_selectors(
            "@media (max-width: 768px) { .box { color: red; } }",
            |sel| format!("{}[scoped]", sel),
        );
        assert!(
            result.contains(".box[scoped]"),
            ".box inside @media should be transformed. Got: {}",
            result
        );
        assert!(
            result.contains("@media"),
            "@media rule must be preserved. Got: {}",
            result
        );
    }

    /// @charset is removed by lightningcss normalization before the walker
    /// runs, so we don't test it directly. The walker requires normalized CSS.
    /// This test verifies that @charset followed by a selector works after
    /// lightningcss normalization (which removes @charset).
    #[test]
    fn after_charset_removal_selector_works() {
        // After lightningcss normalization, @charset is removed, leaving just:
        let selectors = collect_selectors(".box { color: red; }");
        assert_eq!(selectors, vec![".box"]);
    }
}

//! Shared CSS selector-walking utility for normalized CSS.
//!
//! Walks through CSS that has been normalized by lightningcss (no nested rules,
//! well-formed comments/strings), finds selectors before `{`, and applies a
//! caller-provided transformation function to each selector list.

/// Walk normalized CSS, transforming selectors before each `{`.
///
/// For every selector list found before an opening brace, calls `transform_fn`
/// with the trimmed selector text and expects the transformed selector back.
/// Skips `@`-rules (media, keyframes, etc.), comments, and strings.
///
/// **Precondition:** The input CSS must be normalized by lightningcss first
/// (via [`super::normalize_css`]). The normalization flattens nested rules and
/// ensures well-formed comments/strings, which this walker relies on. Selectors
/// inside `@media`, `@supports`, etc. are intentionally skipped — lightningcss
/// normalization handles hoisting them so they appear as top-level rules.
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
                last_block_end = output.len();
            }
            // Handle rule blocks
            '{' if !in_string => {
                let selector_end = output.len();
                let selector_start = last_block_end;

                if selector_start < selector_end {
                    let raw_text = output[selector_start..selector_end].to_string();
                    let trimmed = raw_text.trim();

                    // Skip @-rules (media, keyframes, etc.)
                    if !trimmed.starts_with('@') && !trimmed.is_empty() {
                        let transformed = transform_fn(trimmed);
                        output.truncate(selector_start);
                        // Preserve leading whitespace
                        let leading_ws = &raw_text[..raw_text.len() - raw_text.trim_start().len()];
                        output.push_str(leading_ws);
                        output.push_str(&transformed);
                    }
                }

                output.push('{');
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
    fn test_at_rule_skipped() {
        // The walker skips @-rules entirely. Selectors nested inside @media blocks
        // are also skipped because the @-rule prefix is included in the raw_text.
        // In the real pipeline, lightningcss normalization is applied first, and
        // the scoped/modules transforms handle @media content correctly.
        let selectors = collect_selectors("@media (min-width: 600px) { .box { color: red; } }");
        assert!(selectors.is_empty());
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
        // Same as test_at_rule_skipped: nested selectors inside @-rules are not found.
        let selectors = collect_selectors("@media screen { .inner { color: red; } }");
        assert!(selectors.is_empty());
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
}

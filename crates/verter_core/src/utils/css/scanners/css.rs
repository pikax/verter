//! Standard CSS scanner.
//!
//! Handles plain CSS syntax:
//! - Block comments `/* */` only (no line comments)
//! - Standard at-rules (`@media`, `@keyframes`, `@supports`, `@import`, `@layer`, `@container`)
//! - CSS Nesting (`&` parent selector)
//! - No variable declarations

use crate::{
    common::Span,
    syntax::types::{CssParsedClass, CssParsedRule, CssParsedVBind},
};

use crate::utils::css::common::*;
use crate::utils::css::shared::*;

/// Scan standard CSS content for rules, v-bind expressions, and class selectors.
pub fn scan(
    css: &[u8],
    offset: u32,
    rules: &mut Vec<CssParsedRule>,
    all_v_binds: &mut Vec<CssParsedVBind>,
    all_classes: &mut Vec<CssParsedClass>,
) {
    let len = css.len();
    let mut i = 0;

    while i < len {
        if is_ws(css[i]) {
            i += 1;
            continue;
        }

        // Block comments /* */ only — no line comments in standard CSS
        if i + 1 < len && css[i] == b'/' && css[i + 1] == b'*' {
            i = skip_block_comment(css, i);
            continue;
        }

        // At-rules
        if css[i] == b'@' {
            i = scan_at_rule(css, i, offset, rules, all_v_binds, all_classes);
            continue;
        }

        // Selector + rule body
        if is_selector_start_char(css[i]) {
            i = scan_rule(css, i, offset, rules, all_v_binds, all_classes);
            continue;
        }

        i += 1;
    }
}

/// Scan a style rule: selector list + `{ declarations }`.
fn scan_rule(
    css: &[u8],
    start: usize,
    offset: u32,
    rules: &mut Vec<CssParsedRule>,
    all_v_binds: &mut Vec<CssParsedVBind>,
    all_classes: &mut Vec<CssParsedClass>,
) -> usize {
    let len = css.len();
    let mut i = start;

    // Find the opening '{' while skipping comments and strings
    while i < len && css[i] != b'{' {
        if i + 1 < len && css[i] == b'/' && css[i + 1] == b'*' {
            i = skip_block_comment(css, i);
            continue;
        }
        if css[i] == b'"' || css[i] == b'\'' {
            i = skip_string(css, i);
            continue;
        }
        if css[i] == b';' {
            return i + 1;
        }
        i += 1;
    }

    if i >= len {
        return len;
    }

    // Parse the selector list
    let selector_bytes = &css[start..i];
    let trimmed_end = rtrim_pos(selector_bytes);
    let trimmed_start = ltrim_pos(selector_bytes);

    let selector_span = if trimmed_start < trimmed_end {
        Span::new(
            offset + (start + trimmed_start) as u32,
            offset + (start + trimmed_end) as u32,
        )
    } else {
        Span::new(offset + start as u32, offset + i as u32)
    };

    let selectors = parse_selector_list(selector_bytes, offset + start as u32, all_classes);

    // Scan rule body for v-bind() (no line comments in CSS)
    let mut rule_v_binds = Vec::new();
    let body_end = scan_rule_body(css, i, offset, &mut rule_v_binds, false);

    for v in &rule_v_binds {
        all_v_binds.push(v.clone());
    }

    rules.push(CssParsedRule {
        selector_span,
        selectors,
        v_binds: rule_v_binds,
        classes: Vec::new(),
    });

    body_end
}

/// Scan a CSS at-rule. Handles block at-rules (`@media {}`) and statement at-rules (`@import ;`).
fn scan_at_rule(
    css: &[u8],
    start: usize,
    offset: u32,
    rules: &mut Vec<CssParsedRule>,
    all_v_binds: &mut Vec<CssParsedVBind>,
    all_classes: &mut Vec<CssParsedClass>,
) -> usize {
    let len = css.len();
    let mut i = start + 1; // skip '@'

    // Find end of at-rule prelude (either '{' or ';')
    while i < len && css[i] != b'{' && css[i] != b';' {
        if i + 1 < len && css[i] == b'/' && css[i + 1] == b'*' {
            i = skip_block_comment(css, i);
            continue;
        }
        if css[i] == b'"' || css[i] == b'\'' {
            i = skip_string(css, i);
            continue;
        }
        i += 1;
    }

    if i >= len {
        return len;
    }

    if css[i] == b';' {
        return i + 1;
    }

    // Block at-rule — check if it's @keyframes
    let at_rule_name = extract_at_rule_name(css, start);
    let is_keyframes = at_rule_name == b"keyframes"
        || at_rule_name == b"-webkit-keyframes"
        || at_rule_name == b"-moz-keyframes";

    i += 1; // skip '{'
    let mut depth = 1u32;

    if is_keyframes {
        // Skip keyframes body — no real selectors
        while i < len && depth > 0 {
            match css[i] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                b'"' | b'\'' => {
                    i = skip_string(css, i);
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
    } else {
        // Recurse into block at-rules (@media, @supports, @layer, @container)
        while i < len && depth > 0 {
            if is_ws(css[i]) {
                i += 1;
                continue;
            }
            if i + 1 < len && css[i] == b'/' && css[i + 1] == b'*' {
                i = skip_block_comment(css, i);
                continue;
            }
            if css[i] == b'}' {
                depth -= 1;
                i += 1;
                continue;
            }
            if css[i] == b'@' {
                i = scan_at_rule(css, i, offset, rules, all_v_binds, all_classes);
                continue;
            }
            if is_selector_start_char(css[i]) {
                i = scan_rule(css, i, offset, rules, all_v_binds, all_classes);
                continue;
            }
            i += 1;
        }
    }

    i
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_css(input: &[u8]) -> (Vec<CssParsedRule>, Vec<CssParsedVBind>, Vec<CssParsedClass>) {
        let mut rules = Vec::new();
        let mut v_binds = Vec::new();
        let mut classes = Vec::new();
        scan(input, 0, &mut rules, &mut v_binds, &mut classes);
        (rules, v_binds, classes)
    }

    // --- Rules ---

    #[test]
    fn test_single_rule() {
        let (rules, _, _) = scan_css(b".box { color: red; }");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selectors.len(), 1);
    }

    #[test]
    fn test_multiple_rules() {
        let (rules, _, _) = scan_css(b".a { color: red; } .b { color: blue; }");
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn test_selector_list() {
        let (rules, _, _) = scan_css(b".a, .b { color: red; }");
        assert_eq!(rules[0].selectors.len(), 2);
    }

    #[test]
    fn test_element_selector() {
        let (rules, _, _) = scan_css(b"div { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_universal_selector() {
        let (rules, _, _) = scan_css(b"* { margin: 0; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_id_selector() {
        let (rules, _, _) = scan_css(b"#app { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_attribute_selector() {
        let (rules, _, _) = scan_css(b"[data-foo] { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_pseudo_class() {
        let (rules, _, _) = scan_css(b".btn:hover { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_pseudo_element() {
        let css = b".text::before { content: ''; }";
        let (rules, _, _) = scan_css(css);
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_combinator_selectors() {
        let (rules, _, _) = scan_css(b".a > .b + .c ~ .d { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_descendant_selector() {
        let (rules, _, _) = scan_css(b".parent .child { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_css_nesting_ampersand() {
        let (rules, _, _) = scan_css(b"&.active { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    // --- Block comments (no line comments!) ---

    #[test]
    fn test_block_comment_skipped() {
        let (rules, _, _) = scan_css(b"/* .hidden { display: none; } */ .box { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_no_line_comment_support() {
        // In CSS, `//` is NOT a comment — it would be parsed as part of a selector or value
        // This is intentional: standard CSS does not have line comments
        let css = b"// .a { color: red; }\n.b { color: blue; }";
        let (rules, _, _) = scan_css(css);
        // The `//` is not treated as a comment, so parser sees unexpected tokens
        // The important thing is that .b is still found
        assert!(rules.iter().any(|r| !r.selectors.is_empty()));
    }

    // --- v-bind ---

    #[test]
    fn test_v_bind_in_rule() {
        let (rules, v_binds, _) = scan_css(b".box { color: v-bind(color); }");
        assert_eq!(rules.len(), 1);
        assert_eq!(v_binds.len(), 1);
        assert_eq!(rules[0].v_binds.len(), 1);
    }

    #[test]
    fn test_v_bind_multiple() {
        let (_, v_binds, _) = scan_css(b".box { color: v-bind(a); background: v-bind(b); }");
        assert_eq!(v_binds.len(), 2);
    }

    // --- Classes ---

    #[test]
    fn test_class_extraction() {
        let (_, _, classes) = scan_css(b".btn { color: red; }");
        assert_eq!(classes.len(), 1);
    }

    #[test]
    fn test_multiple_classes() {
        let (_, _, classes) = scan_css(b".a, .b { color: red; } .c { }");
        assert_eq!(classes.len(), 3);
    }

    #[test]
    fn test_chained_classes() {
        let (_, _, classes) = scan_css(b".a.b { color: red; }");
        assert_eq!(classes.len(), 2);
    }

    // --- Special pseudos ---

    #[test]
    fn test_deep_pseudo() {
        let (rules, _, _) = scan_css(b":deep(.inner) { color: red; }");
        assert_eq!(rules[0].selectors[0].specials.len(), 1);
    }

    #[test]
    fn test_global_pseudo() {
        let (rules, _, _) = scan_css(b":global(.cls) { color: red; }");
        assert_eq!(rules[0].selectors[0].specials.len(), 1);
    }

    #[test]
    fn test_slotted_pseudo() {
        let (rules, _, _) = scan_css(b":slotted(.slot) { color: red; }");
        assert_eq!(rules[0].selectors[0].specials.len(), 1);
    }

    // --- At-rules ---

    #[test]
    fn test_at_media_nested() {
        let (rules, _, _) = scan_css(b"@media (max-width: 600px) { .box { color: red; } }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_at_keyframes_skipped() {
        let (rules, _, _) =
            scan_css(b"@keyframes fade { from { opacity: 0; } to { opacity: 1; } }");
        assert_eq!(rules.len(), 0);
    }

    #[test]
    fn test_at_import_statement() {
        let (rules, _, _) = scan_css(b"@import url('base.css'); .box { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_v_bind_in_at_media() {
        let (_, v_binds, _) = scan_css(b"@media screen { .box { color: v-bind(color); } }");
        assert_eq!(v_binds.len(), 1);
    }

    // --- Edge cases ---

    #[test]
    fn test_empty_input() {
        let (rules, v_binds, classes) = scan_css(b"");
        assert!(rules.is_empty());
        assert!(v_binds.is_empty());
        assert!(classes.is_empty());
    }

    #[test]
    fn test_string_in_content() {
        let css = br#".box::before { content: "{ not a rule }"; color: red; }"#;
        let (rules, _, _) = scan_css(css);
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_not_pseudo() {
        let (rules, _, _) = scan_css(b":not(.hidden) { display: block; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_nth_child() {
        let (rules, _, _) = scan_css(b"li:nth-child(2n+1) { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_selector_span_correct() {
        let css = b".box { color: red; }";
        let (rules, _, _) = scan_css(css);
        let sel = &rules[0].selectors[0];
        assert_eq!(
            &css[sel.span.start as usize..sel.span.end as usize],
            b".box"
        );
    }
}

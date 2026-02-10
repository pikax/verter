//! Language-agnostic CSS scanning utilities used by all per-language scanners.
//!
//! Contains: v-bind extraction, selector list parsing, class extraction,
//! special pseudo-selector extraction, at-rule name extraction, and rule body scanning.

use crate::{
    common::Span,
    syntax_kai::types::{
        CssParsedClass, CssParsedSelector, CssParsedSpecialPseudo, CssParsedSpecialPseudoKind,
        CssParsedVBind,
    },
};

use super::common::*;

// =============================================================================
// Rule body scanning
// =============================================================================

/// Scan a rule body between `{ ... }` for `v-bind()` calls.
///
/// Called by each language scanner after it finds a rule's opening brace.
/// Handles nested braces, block comments, optional line comments, and strings.
///
/// `start` should be the position of the opening `{`.
/// Returns position after the closing `}`.
pub fn scan_rule_body(
    css: &[u8],
    start: usize,
    offset: u32,
    v_binds: &mut Vec<CssParsedVBind>,
    support_line_comments: bool,
) -> usize {
    let len = css.len();
    let mut i = start + 1; // skip '{'
    let mut depth = 1u32;

    while i < len && depth > 0 {
        if i + 1 < len && css[i] == b'/' && css[i + 1] == b'*' {
            i = skip_block_comment(css, i);
            continue;
        }
        if support_line_comments && i + 1 < len && css[i] == b'/' && css[i + 1] == b'/' {
            i = skip_line_comment(css, i);
            continue;
        }
        if css[i] == b'"' || css[i] == b'\'' {
            i = skip_string(css, i);
            continue;
        }
        if css[i] == b'{' {
            depth += 1;
            i += 1;
            continue;
        }
        if css[i] == b'}' {
            depth -= 1;
            if depth == 0 {
                break;
            }
            i += 1;
            continue;
        }
        // Check for v-bind()
        if i + 7 <= len && &css[i..i + 7] == b"v-bind(" {
            if let Some(v) = scan_v_bind(css, i, offset) {
                i = (v.full_span.end - offset) as usize;
                v_binds.push(v);
                continue;
            }
        }
        i += 1;
    }

    // Skip closing '}'
    if i < len && css[i] == b'}' {
        i += 1;
    }

    i
}

// =============================================================================
// v-bind() scanner
// =============================================================================

/// Scan a `v-bind(...)` expression at position `start` in `css`.
/// Returns the parsed v-bind info, or `None` if malformed.
pub fn scan_v_bind(css: &[u8], start: usize, offset: u32) -> Option<CssParsedVBind> {
    // css[start..start+7] == b"v-bind("
    let expr_start = start + 7;
    let mut depth = 1u32;
    let mut j = expr_start;

    while j < css.len() && depth > 0 {
        match css[j] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth > 0 {
            j += 1;
        }
    }

    if depth != 0 {
        return None;
    }

    let expr_end = j;
    let full_end = j + 1; // after closing ')'

    let expr_bytes = &css[expr_start..expr_end];
    let trimmed = trim_bytes(expr_bytes);

    // Check if expression is quoted
    let (_unquoted, quoted) = if trimmed.len() >= 2
        && ((trimmed[0] == b'\'' && trimmed[trimmed.len() - 1] == b'\'')
            || (trimmed[0] == b'"' && trimmed[trimmed.len() - 1] == b'"'))
    {
        (&trimmed[1..trimmed.len() - 1], true)
    } else {
        (trimmed, false)
    };

    // Compute the trimmed expression span in source coordinates
    let trim_start = expr_bytes
        .iter()
        .position(|&b| !is_ws(b))
        .unwrap_or(expr_bytes.len());
    let trim_end = expr_bytes
        .iter()
        .rposition(|&b| !is_ws(b))
        .map_or(trim_start, |e| e + 1);

    let (expr_span_start, expr_span_end) = if quoted {
        // Point to the unquoted content
        (
            offset + (expr_start + trim_start + 1) as u32,
            offset + (expr_start + trim_end - 1) as u32,
        )
    } else {
        (
            offset + (expr_start + trim_start) as u32,
            offset + (expr_start + trim_end) as u32,
        )
    };

    Some(CssParsedVBind {
        full_span: Span::new(offset + start as u32, offset + full_end as u32),
        expression: Span::new(expr_span_start, expr_span_end),
        quoted,
    })
}

// =============================================================================
// Selector list parser
// =============================================================================

/// Parse a selector list (bytes before `{`) into individual `CssParsedSelector`s.
/// Also extracts class selectors for CSS modules.
pub fn parse_selector_list(
    selector_bytes: &[u8],
    offset: u32,
    all_classes: &mut Vec<CssParsedClass>,
) -> Vec<CssParsedSelector> {
    let mut selectors = Vec::new();
    let mut i = 0;
    let len = selector_bytes.len();
    let mut sel_start = 0;

    while i <= len {
        // Split by ',' but respect parentheses and strings
        let at_comma = i < len && selector_bytes[i] == b',';
        let at_end = i == len;

        if at_comma || at_end {
            let sel_bytes = &selector_bytes[sel_start..i];
            let trimmed_start = ltrim_pos(sel_bytes);
            let trimmed_end = rtrim_pos(sel_bytes);

            if trimmed_start < trimmed_end {
                let span = Span::new(
                    offset + (sel_start + trimmed_start) as u32,
                    offset + (sel_start + trimmed_end) as u32,
                );

                let trimmed = &sel_bytes[trimmed_start..trimmed_end];

                let specials =
                    extract_special_pseudos(trimmed, offset + (sel_start + trimmed_start) as u32);

                extract_classes(
                    trimmed,
                    offset + (sel_start + trimmed_start) as u32,
                    all_classes,
                );

                selectors.push(CssParsedSelector { span, specials });
            }

            sel_start = i + 1; // skip ','
            i += 1;
            continue;
        }

        // Skip parenthesized groups (for pseudo-selectors like :not(.foo))
        if i < len && selector_bytes[i] == b'(' {
            i = skip_parens(selector_bytes, i);
            continue;
        }

        // Skip strings
        if i < len && (selector_bytes[i] == b'"' || selector_bytes[i] == b'\'') {
            i = skip_string(selector_bytes, i);
            continue;
        }

        // Skip attribute selectors [...]
        if i < len && selector_bytes[i] == b'[' {
            i = skip_brackets(selector_bytes, i);
            continue;
        }

        i += 1;
    }

    selectors
}

// =============================================================================
// Special pseudo extraction
// =============================================================================

/// Extract Vue special pseudo-selectors (`:deep`, `:global`, `:slotted`) from a selector.
pub fn extract_special_pseudos(selector: &[u8], offset: u32) -> Vec<CssParsedSpecialPseudo> {
    let mut specials = Vec::new();
    let len = selector.len();
    let mut i = 0;

    while i < len {
        // Skip strings
        if selector[i] == b'"' || selector[i] == b'\'' {
            i = skip_string(selector, i);
            continue;
        }

        // Look for `:deep(`, `:global(`, `:slotted(`
        if selector[i] == b':' {
            if let Some((kind, prefix_len)) = match_special_pseudo(&selector[i..]) {
                let pseudo_start = i;
                // Find matching closing ')'
                let paren_start = i + prefix_len;
                if paren_start < len && selector[paren_start] == b'(' {
                    let inner_start = paren_start + 1;
                    let mut depth = 1u32;
                    let mut j = inner_start;
                    while j < len && depth > 0 {
                        match selector[j] {
                            b'(' => depth += 1,
                            b')' => depth -= 1,
                            _ => {}
                        }
                        if depth > 0 {
                            j += 1;
                        }
                    }
                    if depth == 0 {
                        let inner_end = j;
                        let pseudo_end = j + 1;

                        let inner_trimmed_start = ltrim_pos(&selector[inner_start..inner_end]);
                        let inner_trimmed_end = rtrim_pos(&selector[inner_start..inner_end]);

                        let inner = if inner_trimmed_start < inner_trimmed_end {
                            Some(Span::new(
                                offset + (inner_start + inner_trimmed_start) as u32,
                                offset + (inner_start + inner_trimmed_end) as u32,
                            ))
                        } else {
                            None
                        };

                        specials.push(CssParsedSpecialPseudo {
                            kind,
                            span: Span::new(
                                offset + pseudo_start as u32,
                                offset + pseudo_end as u32,
                            ),
                            inner,
                        });

                        i = pseudo_end;
                        continue;
                    }
                }
            }
        }

        i += 1;
    }

    specials
}

/// Match a special Vue pseudo-selector at the start of `bytes`.
/// Returns `(kind, prefix_length_before_paren)` or `None`.
pub fn match_special_pseudo(bytes: &[u8]) -> Option<(CssParsedSpecialPseudoKind, usize)> {
    if bytes.starts_with(b":deep") {
        Some((CssParsedSpecialPseudoKind::Deep, 5))
    } else if bytes.starts_with(b":global") {
        Some((CssParsedSpecialPseudoKind::Global, 7))
    } else if bytes.starts_with(b":slotted") {
        Some((CssParsedSpecialPseudoKind::Slotted, 8))
    } else {
        None
    }
}

// =============================================================================
// Class extraction
// =============================================================================

/// Extract class selectors (`.className`) from a selector (for CSS modules).
pub fn extract_classes(selector: &[u8], offset: u32, classes: &mut Vec<CssParsedClass>) {
    let len = selector.len();
    let mut i = 0;

    while i < len {
        // Skip strings
        if selector[i] == b'"' || selector[i] == b'\'' {
            i = skip_string(selector, i);
            continue;
        }

        // Skip attribute selectors [...]
        if selector[i] == b'[' {
            i = skip_brackets(selector, i);
            continue;
        }

        // Class selector: '.' followed by CSS ident chars
        // In selectors, '.' always starts a class (even chained like .a.b)
        if selector[i] == b'.' && i + 1 < len && is_css_ident_start_char(selector[i + 1]) {
            let class_start = i + 1;
            let mut class_end = class_start;
            while class_end < len && is_css_ident_char(selector[class_end]) {
                class_end += 1;
            }
            if class_end > class_start {
                classes.push(CssParsedClass {
                    name_span: Span::new(offset + class_start as u32, offset + class_end as u32),
                });
            }
            i = class_end;
            continue;
        }

        i += 1;
    }
}

// =============================================================================
// At-rule name extraction
// =============================================================================

/// Extract the at-rule name from `@name ...` at position `at_pos`.
pub fn extract_at_rule_name(css: &[u8], at_pos: usize) -> &[u8] {
    let start = at_pos + 1; // skip '@'
    let mut end = start;
    while end < css.len() && is_css_ident_char(css[end]) {
        end += 1;
    }
    &css[start..end]
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- v-bind ---

    #[test]
    fn test_scan_v_bind_simple() {
        let css = b"v-bind(color)";
        let vb = scan_v_bind(css, 0, 100).unwrap();
        assert_eq!(vb.full_span.start, 100);
        assert_eq!(vb.full_span.end, 113);
        assert_eq!(vb.expression.start, 107);
        assert_eq!(vb.expression.end, 112);
        assert!(!vb.quoted);
    }

    #[test]
    fn test_scan_v_bind_quoted() {
        let css = b"v-bind('foo.bar')";
        let vb = scan_v_bind(css, 0, 0).unwrap();
        assert!(vb.quoted);
        assert_eq!(
            &css[vb.expression.start as usize..vb.expression.end as usize],
            b"foo.bar"
        );
    }

    #[test]
    fn test_scan_v_bind_nested_parens() {
        let css = b"v-bind(calc(a + b))";
        let vb = scan_v_bind(css, 0, 0).unwrap();
        assert_eq!(
            &css[vb.expression.start as usize..vb.expression.end as usize],
            b"calc(a + b)"
        );
    }

    #[test]
    fn test_scan_v_bind_unclosed() {
        let css = b"v-bind(unclosed";
        assert!(scan_v_bind(css, 0, 0).is_none());
    }

    // --- Selector list ---

    #[test]
    fn test_parse_single_selector() {
        let mut classes = Vec::new();
        let sels = parse_selector_list(b".box", 0, &mut classes);
        assert_eq!(sels.len(), 1);
        assert_eq!(sels[0].span.start, 0);
        assert_eq!(sels[0].span.end, 4);
    }

    #[test]
    fn test_parse_comma_separated() {
        let mut classes = Vec::new();
        let sels = parse_selector_list(b".a, .b, .c", 0, &mut classes);
        assert_eq!(sels.len(), 3);
    }

    #[test]
    fn test_parse_selector_with_parens() {
        let mut classes = Vec::new();
        let sels = parse_selector_list(b":not(.hidden), .visible", 0, &mut classes);
        assert_eq!(sels.len(), 2);
    }

    // --- Special pseudos ---

    #[test]
    fn test_extract_deep() {
        let specials = extract_special_pseudos(b":deep(.inner)", 0);
        assert_eq!(specials.len(), 1);
        assert_eq!(specials[0].kind, CssParsedSpecialPseudoKind::Deep);
        assert_eq!(specials[0].inner.unwrap().start, 6);
        assert_eq!(specials[0].inner.unwrap().end, 12);
    }

    #[test]
    fn test_extract_global() {
        let specials = extract_special_pseudos(b":global(.cls)", 0);
        assert_eq!(specials.len(), 1);
        assert_eq!(specials[0].kind, CssParsedSpecialPseudoKind::Global);
    }

    #[test]
    fn test_extract_slotted() {
        let specials = extract_special_pseudos(b":slotted(.slot)", 0);
        assert_eq!(specials.len(), 1);
        assert_eq!(specials[0].kind, CssParsedSpecialPseudoKind::Slotted);
    }

    #[test]
    fn test_no_special_pseudos() {
        let specials = extract_special_pseudos(b".btn:hover", 0);
        assert!(specials.is_empty());
    }

    // --- Class extraction ---

    #[test]
    fn test_extract_single_class() {
        let mut classes = Vec::new();
        extract_classes(b".btn", 10, &mut classes);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].name_span.start, 11);
        assert_eq!(classes[0].name_span.end, 14);
    }

    #[test]
    fn test_extract_chained_classes() {
        let mut classes = Vec::new();
        extract_classes(b".a.b.c", 0, &mut classes);
        assert_eq!(classes.len(), 3);
    }

    #[test]
    fn test_extract_class_skips_attr_selector() {
        let mut classes = Vec::new();
        extract_classes(b"[class='.fake'] .real", 0, &mut classes);
        assert_eq!(classes.len(), 1);
    }

    // --- At-rule name ---

    #[test]
    fn test_extract_at_rule_name() {
        assert_eq!(
            extract_at_rule_name(b"@media (min-width: 600px)", 0),
            b"media"
        );
        assert_eq!(extract_at_rule_name(b"@keyframes fade", 0), b"keyframes");
        assert_eq!(extract_at_rule_name(b"@import url()", 0), b"import");
    }

    // --- Rule body scanning ---

    #[test]
    fn test_scan_rule_body_empty() {
        let css = b"{ }";
        let mut v_binds = Vec::new();
        let end = scan_rule_body(css, 0, 0, &mut v_binds, false);
        assert_eq!(end, 3);
        assert!(v_binds.is_empty());
    }

    #[test]
    fn test_scan_rule_body_with_v_bind() {
        let css = b"{ color: v-bind(primary); }";
        let mut v_binds = Vec::new();
        let end = scan_rule_body(css, 0, 0, &mut v_binds, false);
        assert_eq!(end, css.len());
        assert_eq!(v_binds.len(), 1);
    }

    #[test]
    fn test_scan_rule_body_nested_braces() {
        let css = b"{ .inner { color: red; } }";
        let mut v_binds = Vec::new();
        let end = scan_rule_body(css, 0, 0, &mut v_binds, false);
        assert_eq!(end, css.len());
    }

    #[test]
    fn test_scan_rule_body_with_line_comments() {
        let css = b"{ // comment\n color: v-bind(c); }";
        let mut v_binds = Vec::new();
        let end = scan_rule_body(css, 0, 0, &mut v_binds, true);
        assert_eq!(end, css.len());
        assert_eq!(v_binds.len(), 1);
    }
}

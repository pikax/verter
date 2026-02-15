//! Stylus scanner (braces-only mode).
//!
//! Handles Stylus written with explicit braces (common in Vue SFCs):
//! - Line comments `//`
//! - Block comments `/* */`
//! - Nesting with `&`
//! - No special variable handling (`var = value` is not confused with selectors)
//!
//! Indentation-based Stylus (no braces) is **not supported**.

use crate::{
    common::Span,
    syntax::types::{CssParsedClass, CssParsedRule, CssParsedVBind},
};

use crate::utils::css::common::*;
use crate::utils::css::shared::*;

/// Scan Stylus content (braces mode) for rules, v-bind expressions, and class selectors.
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

        // Block comments
        if i + 1 < len && css[i] == b'/' && css[i + 1] == b'*' {
            i = skip_block_comment(css, i);
            continue;
        }

        // Line comments
        if i + 1 < len && css[i] == b'/' && css[i + 1] == b'/' {
            i = skip_line_comment(css, i);
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

/// Scan a Stylus style rule.
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

    // Find the opening '{'
    while i < len && css[i] != b'{' {
        if i + 1 < len && css[i] == b'/' && css[i + 1] == b'*' {
            i = skip_block_comment(css, i);
            continue;
        }
        if i + 1 < len && css[i] == b'/' && css[i + 1] == b'/' {
            i = skip_line_comment(css, i);
            continue;
        }
        if css[i] == b'"' || css[i] == b'\'' {
            i = skip_string(css, i);
            continue;
        }
        if css[i] == b';' {
            return i + 1;
        }
        if css[i] == b'}' {
            return i;
        }
        i += 1;
    }

    if i >= len {
        return len;
    }

    // Parse selector list
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

    // Scan rule body with line comment support
    let mut rule_v_binds = Vec::new();
    let body_end = scan_rule_body(css, i, offset, &mut rule_v_binds, true);

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

/// Scan a Stylus at-rule.
fn scan_at_rule(
    css: &[u8],
    start: usize,
    offset: u32,
    rules: &mut Vec<CssParsedRule>,
    all_v_binds: &mut Vec<CssParsedVBind>,
    all_classes: &mut Vec<CssParsedClass>,
) -> usize {
    let len = css.len();
    let mut i = start + 1;

    // Find end of at-rule prelude
    while i < len && css[i] != b'{' && css[i] != b';' {
        if i + 1 < len && css[i] == b'/' && css[i + 1] == b'*' {
            i = skip_block_comment(css, i);
            continue;
        }
        if i + 1 < len && css[i] == b'/' && css[i + 1] == b'/' {
            i = skip_line_comment(css, i);
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

    // Block at-rule
    let at_rule_name = extract_at_rule_name(css, start);
    let is_keyframes = at_rule_name == b"keyframes"
        || at_rule_name == b"-webkit-keyframes"
        || at_rule_name == b"-moz-keyframes";

    i += 1; // skip '{'
    let mut depth = 1u32;

    if is_keyframes {
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
        while i < len && depth > 0 {
            if is_ws(css[i]) {
                i += 1;
                continue;
            }
            if i + 1 < len && css[i] == b'/' && css[i + 1] == b'*' {
                i = skip_block_comment(css, i);
                continue;
            }
            if i + 1 < len && css[i] == b'/' && css[i + 1] == b'/' {
                i = skip_line_comment(css, i);
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

    fn scan_stylus(input: &[u8]) -> (Vec<CssParsedRule>, Vec<CssParsedVBind>, Vec<CssParsedClass>) {
        let mut rules = Vec::new();
        let mut v_binds = Vec::new();
        let mut classes = Vec::new();
        scan(input, 0, &mut rules, &mut v_binds, &mut classes);
        (rules, v_binds, classes)
    }

    #[test]
    fn test_single_rule() {
        let (rules, _, _) = scan_stylus(b".box { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_line_comment() {
        let (rules, _, _) = scan_stylus(b"// comment\n.box { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_v_bind() {
        let (_, v_binds, _) = scan_stylus(b".box { color: v-bind(primary); }");
        assert_eq!(v_binds.len(), 1);
    }

    #[test]
    fn test_class_extraction() {
        let (_, _, classes) = scan_stylus(b".btn { color: red; }");
        assert_eq!(classes.len(), 1);
    }

    #[test]
    fn test_at_media() {
        let (rules, _, _) = scan_stylus(b"@media (max-width: 600px) { .box { color: red; } }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_keyframes_skipped() {
        let (rules, _, _) =
            scan_stylus(b"@keyframes fade { from { opacity: 0; } to { opacity: 1; } }");
        assert_eq!(rules.len(), 0);
    }

    #[test]
    fn test_deep_pseudo() {
        let (rules, _, _) = scan_stylus(b":deep(.inner) { color: red; }");
        assert_eq!(rules[0].selectors[0].specials.len(), 1);
    }

    #[test]
    fn test_empty_input() {
        let (rules, v_binds, classes) = scan_stylus(b"");
        assert!(rules.is_empty());
        assert!(v_binds.is_empty());
        assert!(classes.is_empty());
    }
}

//! Less scanner.
//!
//! Handles Less-specific syntax on top of standard CSS:
//! - Line comments `//`
//! - `@variable: value;` declarations (disambiguated from at-rules)
//! - Less mixins: `.mixin()` calls in rule bodies
//! - `@{var}` interpolation in selectors
//! - Nesting with `&` parent selector

use crate::{
    common::Span,
    syntax_kai::types::{CssParsedClass, CssParsedRule, CssParsedVBind},
};

use crate::utils::css::common::*;
use crate::utils::css::shared::*;

/// Scan Less content for rules, v-bind expressions, and class selectors.
pub fn scan(
    css: &[u8],
    offset: u32,
    rules: &mut Vec<CssParsedRule>,
    all_v_binds: &mut Vec<CssParsedVBind>,
    all_classes: &mut Vec<CssParsedClass>,
) {
    scan_block(css, 0, css.len(), offset, rules, all_v_binds, all_classes);
}

/// Scan a region of Less bytes for rules and at-rules.
fn scan_block(
    css: &[u8],
    mut i: usize,
    end: usize,
    offset: u32,
    rules: &mut Vec<CssParsedRule>,
    all_v_binds: &mut Vec<CssParsedVBind>,
    all_classes: &mut Vec<CssParsedClass>,
) -> usize {
    while i < end {
        if is_ws(css[i]) {
            i += 1;
            continue;
        }

        // Block comments /* */
        if i + 1 < end && css[i] == b'/' && css[i + 1] == b'*' {
            i = skip_block_comment(css, i);
            continue;
        }

        // Line comments //
        if i + 1 < end && css[i] == b'/' && css[i + 1] == b'/' {
            i = skip_line_comment(css, i);
            continue;
        }

        // @ — could be a Less variable or a CSS at-rule
        if css[i] == b'@' {
            if is_less_variable(css, i) {
                i = skip_less_variable(css, i);
            } else {
                i = scan_at_rule(css, i, offset, rules, all_v_binds, all_classes);
            }
            continue;
        }

        // Closing brace
        if css[i] == b'}' {
            return i;
        }

        // Selector + rule body
        if is_selector_start_char(css[i]) {
            i = scan_rule(css, i, offset, rules, all_v_binds, all_classes);
            continue;
        }

        i += 1;
    }

    i
}

/// Check if `@` at position `at_pos` is a Less variable declaration (`@name: value;`).
///
/// Disambiguation: look for `:` before `{` or `;` (ignoring strings and parens).
/// Also handles `@{var}` interpolation (not a variable declaration).
fn is_less_variable(css: &[u8], at_pos: usize) -> bool {
    let len = css.len();

    // @{var} interpolation — not a variable
    if at_pos + 1 < len && css[at_pos + 1] == b'{' {
        return false;
    }

    // Extract the name after @
    let name = extract_at_rule_name(css, at_pos);
    if name.is_empty() {
        return false;
    }

    // Known CSS at-rules are NOT Less variables
    const CSS_AT_RULES: &[&[u8]] = &[
        b"media",
        b"keyframes",
        b"-webkit-keyframes",
        b"-moz-keyframes",
        b"supports",
        b"import",
        b"charset",
        b"namespace",
        b"font-face",
        b"page",
        b"layer",
        b"container",
        b"scope",
        b"starting-style",
        b"property",
        b"counter-style",
    ];

    // Less-specific at-rules
    const LESS_AT_RULES: &[&[u8]] = &[b"plugin", b"import"];

    if CSS_AT_RULES.contains(&name) || LESS_AT_RULES.contains(&name) {
        return false;
    }

    // Look ahead: if we find `:` before `{` or `;`, it's a variable
    let mut j = at_pos + 1 + name.len();
    while j < len {
        match css[j] {
            b':' => return true,
            b'{' | b';' => return false,
            b'"' | b'\'' => {
                j = skip_string(css, j);
                continue;
            }
            b'(' => {
                j = skip_parens(css, j);
                continue;
            }
            _ => {}
        }
        j += 1;
    }
    false
}

/// Skip a Less variable declaration `@name: value;`.
fn skip_less_variable(css: &[u8], start: usize) -> usize {
    let len = css.len();
    let mut i = start + 1;

    while i < len {
        if css[i] == b';' {
            return i + 1;
        }
        if css[i] == b'"' || css[i] == b'\'' {
            i = skip_string(css, i);
            continue;
        }
        if css[i] == b'(' {
            i = skip_parens(css, i);
            continue;
        }
        i += 1;
    }
    len
}

/// Scan a Less style rule: selector list + `{ declarations + nested rules }`.
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

    // Scan rule body — Less also supports nesting
    let mut rule_v_binds = Vec::new();
    let body_end = scan_less_rule_body(
        css,
        i,
        offset,
        rules,
        &mut rule_v_binds,
        all_v_binds,
        all_classes,
    );

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

/// Scan a Less rule body with nesting support.
fn scan_less_rule_body(
    css: &[u8],
    start: usize,
    offset: u32,
    rules: &mut Vec<CssParsedRule>,
    rule_v_binds: &mut Vec<CssParsedVBind>,
    all_v_binds: &mut Vec<CssParsedVBind>,
    all_classes: &mut Vec<CssParsedClass>,
) -> usize {
    let len = css.len();
    let mut i = start + 1; // skip '{'
    let mut depth = 1u32;

    while i < len && depth > 0 {
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
                rule_v_binds.push(v);
                continue;
            }
        }

        // Less variable inside rule
        if css[i] == b'@' {
            if is_less_variable(css, i) {
                i = skip_less_variable(css, i);
                continue;
            }
            // Could be @media or @{interpolation} inside a rule
            if i + 1 < len && css[i + 1] == b'{' {
                // @{var} interpolation — skip @{...}
                i += 2;
                while i < len && css[i] != b'}' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
                continue;
            }
            // Nested at-rule (@media inside a rule)
            i = scan_at_rule(css, i, offset, rules, all_v_binds, all_classes);
            continue;
        }

        // Nested rule (Less nesting)
        if is_selector_start_char(css[i]) && looks_like_nested_rule(css, i) {
            i = scan_rule(css, i, offset, rules, all_v_binds, all_classes);
            continue;
        }

        i += 1;
    }

    if i < len && css[i] == b'}' {
        i += 1;
    }

    i
}

/// Heuristic: does position `start` look like a nested rule?
fn looks_like_nested_rule(css: &[u8], start: usize) -> bool {
    let len = css.len();
    let mut j = start;
    let mut depth = 0u32;

    while j < len {
        match css[j] {
            b'{' => {
                if depth == 0 {
                    return true;
                }
                depth += 1;
            }
            b'}' => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            b';' if depth == 0 => return false,
            b'"' | b'\'' => {
                j = skip_string(css, j);
                continue;
            }
            b'(' => {
                j = skip_parens(css, j);
                continue;
            }
            _ => {}
        }
        j += 1;
    }
    false
}

/// Scan a Less at-rule.
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
        if css[i] == b'(' {
            i = skip_parens(css, i);
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
        // Recurse into @media, @supports, etc.
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
                if is_less_variable(css, i) {
                    i = skip_less_variable(css, i);
                    continue;
                }
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

    fn scan_less_input(
        input: &[u8],
    ) -> (Vec<CssParsedRule>, Vec<CssParsedVBind>, Vec<CssParsedClass>) {
        let mut rules = Vec::new();
        let mut v_binds = Vec::new();
        let mut classes = Vec::new();
        scan(input, 0, &mut rules, &mut v_binds, &mut classes);
        (rules, v_binds, classes)
    }

    // --- Basic rules ---

    #[test]
    fn test_single_rule() {
        let (rules, _, _) = scan_less_input(b".box { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_multiple_rules() {
        let (rules, _, _) = scan_less_input(b".a { color: red; } .b { color: blue; }");
        assert_eq!(rules.len(), 2);
    }

    // --- Line comments ---

    #[test]
    fn test_line_comment() {
        let (rules, _, _) = scan_less_input(b"// comment\n.box { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    // --- Less variables (disambiguation) ---

    #[test]
    fn test_variable_skipped() {
        let (rules, _, _) = scan_less_input(b"@primary: blue;\n.box { color: @primary; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_variable_multiple() {
        let (rules, _, _) = scan_less_input(b"@a: 1;\n@b: 2;\n@c: 3;\n.box { width: @a; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_variable_not_at_media() {
        // @media should NOT be treated as a variable
        let (rules, _, _) = scan_less_input(b"@media (max-width: 600px) { .box { color: red; } }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_variable_not_at_keyframes() {
        let (rules, _, _) =
            scan_less_input(b"@keyframes fade { from { opacity: 0; } to { opacity: 1; } }");
        assert_eq!(rules.len(), 0);
    }

    #[test]
    fn test_variable_not_at_import() {
        let (rules, _, _) = scan_less_input(b"@import 'base.less';\n.box { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_variable_inside_rule() {
        let (rules, _, _) = scan_less_input(b".box { @local-var: red; color: @local-var; }");
        assert_eq!(rules.len(), 1);
    }

    // --- Interpolation ---

    #[test]
    fn test_interpolation_in_selector() {
        let (rules, _, _) = scan_less_input(b".icon-@{name} { display: inline; }");
        // @{name} interpolation should not break parsing
        assert!(rules.len() >= 1);
    }

    // --- Less nesting ---

    #[test]
    fn test_nested_rule() {
        let (rules, _, _) = scan_less_input(b".parent { .child { color: red; } }");
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn test_ampersand_nesting() {
        let (rules, _, _) = scan_less_input(b".btn { &:hover { color: blue; } }");
        assert_eq!(rules.len(), 2);
    }

    // --- v-bind ---

    #[test]
    fn test_v_bind_in_less() {
        let (_, v_binds, _) = scan_less_input(b".box { color: v-bind(color); }");
        assert_eq!(v_binds.len(), 1);
    }

    #[test]
    fn test_v_bind_nested() {
        let (_, v_binds, _) = scan_less_input(b".parent { .child { color: v-bind(c); } }");
        assert_eq!(v_binds.len(), 1);
    }

    // --- Classes ---

    #[test]
    fn test_class_extraction() {
        let (_, _, classes) = scan_less_input(b".btn { color: red; }");
        assert_eq!(classes.len(), 1);
    }

    // --- Special pseudos ---

    #[test]
    fn test_deep_in_less() {
        let (rules, _, _) = scan_less_input(b":deep(.inner) { color: red; }");
        assert_eq!(rules[0].selectors[0].specials.len(), 1);
    }

    // --- Complex patterns ---

    #[test]
    fn test_real_world_less() {
        let less = b"\
@primary: #333;\n\
@secondary: #666;\n\
\n\
.container {\n\
  color: @primary;\n\
  \n\
  h2 {\n\
    color: @secondary;\n\
  }\n\
  \n\
  .btn {\n\
    &:hover {\n\
      color: v-bind(hoverColor);\n\
    }\n\
  }\n\
}\n\
\n\
@media (max-width: 768px) {\n\
  .container {\n\
    padding: 10px;\n\
  }\n\
}";
        let (rules, v_binds, _) = scan_less_input(less);
        // .container, h2, .btn, &:hover, .container (in @media)
        assert_eq!(rules.len(), 5);
        assert_eq!(v_binds.len(), 1);
    }
}

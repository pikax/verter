//! SCSS scanner.
//!
//! Handles SCSS-specific syntax on top of standard CSS:
//! - Line comments `//`
//! - `$variable: value;` declarations (skipped)
//! - SCSS at-rules: `@mixin`, `@include`, `@extend`, `@use`, `@forward`, `@if`/`@else`/`@for`/`@while`/`@each`
//! - Nesting with `&` parent selector
//! - `#{$expr}` interpolation (already handled — `#` is not a special char in the scanner)

use crate::{
    common::Span,
    syntax::types::{CssParsedClass, CssParsedRule, CssParsedVBind},
};

use crate::utils::css::common::*;
use crate::utils::css::shared::*;

/// SCSS statement at-rules that end with `;` (no block body).
const SCSS_STATEMENT_AT_RULES: &[&[u8]] = &[
    b"include",
    b"extend",
    b"use",
    b"forward",
    b"import",
    b"charset",
    b"namespace",
    b"debug",
    b"warn",
    b"error",
];

/// SCSS block at-rules that should NOT have their inner selectors parsed
/// (their bodies contain SCSS logic, not style rules).
const SCSS_SKIP_BODY_AT_RULES: &[&[u8]] = &[
    b"mixin",
    b"function",
    b"keyframes",
    b"-webkit-keyframes",
    b"-moz-keyframes",
];

/// SCSS control flow at-rules with blocks that may contain style rules.
const SCSS_CONTROL_FLOW_AT_RULES: &[&[u8]] =
    &[b"if", b"else", b"for", b"while", b"each", b"at-root"];

/// Scan SCSS content for rules, v-bind expressions, and class selectors.
pub fn scan(
    css: &[u8],
    offset: u32,
    rules: &mut Vec<CssParsedRule>,
    all_v_binds: &mut Vec<CssParsedVBind>,
    all_classes: &mut Vec<CssParsedClass>,
) {
    scan_block(css, 0, css.len(), offset, rules, all_v_binds, all_classes);
}

/// Scan a region of SCSS bytes for rules and at-rules.
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

        // SCSS variable declarations: $name: value;
        if css[i] == b'$' {
            i = skip_scss_variable(css, i);
            continue;
        }

        // At-rules
        if css[i] == b'@' {
            i = scan_at_rule(css, i, offset, rules, all_v_binds, all_classes);
            continue;
        }

        // Closing brace (when scanning inside a block)
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

/// Skip a SCSS variable declaration `$name: value;`.
fn skip_scss_variable(css: &[u8], start: usize) -> usize {
    let len = css.len();
    let mut i = start + 1; // skip '$'

    // Find ';' or end of input
    while i < len {
        if css[i] == b';' {
            return i + 1;
        }
        if css[i] == b'{' {
            // Edge case: $map: ( key: value ); or SCSS map with block
            // Skip nested block
            let mut depth = 1u32;
            i += 1;
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
            // Look for trailing ';'
            while i < len && is_ws(css[i]) {
                i += 1;
            }
            if i < len && css[i] == b';' {
                return i + 1;
            }
            return i;
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

/// Scan a SCSS style rule: selector list + `{ declarations + nested rules }`.
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
        // Stop if we hit '}' — we're inside a nested block and this is not our rule
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

    // Scan rule body for v-bind() and nested rules
    let mut rule_v_binds = Vec::new();
    let body_end = scan_scss_rule_body(
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

/// Scan a SCSS rule body. Unlike plain CSS, SCSS rule bodies can contain nested rules.
fn scan_scss_rule_body(
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

        // SCSS variable inside rule
        if css[i] == b'$' {
            i = skip_scss_variable(css, i);
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

        // Check for nested at-rules (@include, @media inside rule, etc.)
        if css[i] == b'@' {
            let at_name = extract_at_rule_name(css, i);
            if SCSS_STATEMENT_AT_RULES.contains(&at_name) {
                // Statement at-rule: skip to ';'
                while i < len && css[i] != b';' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
                continue;
            }
            // Block at-rule nested inside a rule (e.g., @media inside .parent)
            i = scan_at_rule(css, i, offset, rules, all_v_binds, all_classes);
            continue;
        }

        // Check for nested selector (SCSS nesting)
        // A nested selector starts with a selector char and contains '{' before '}'
        if is_selector_start_char(css[i]) {
            // Look ahead: if there's a '{' before ';' or '}', this is a nested rule
            if looks_like_nested_rule(css, i) {
                i = scan_rule(css, i, offset, rules, all_v_binds, all_classes);
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

/// Heuristic: does position `start` look like the beginning of a nested rule?
/// Returns true if there's a `{` before `;` or end of block.
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

/// Scan a SCSS at-rule.
fn scan_at_rule(
    css: &[u8],
    start: usize,
    offset: u32,
    rules: &mut Vec<CssParsedRule>,
    all_v_binds: &mut Vec<CssParsedVBind>,
    all_classes: &mut Vec<CssParsedClass>,
) -> usize {
    let len = css.len();
    let at_rule_name = extract_at_rule_name(css, start);

    // Statement at-rules: skip to ';'
    if SCSS_STATEMENT_AT_RULES.contains(&at_rule_name) {
        let mut i = start + 1;
        while i < len && css[i] != b';' {
            if css[i] == b'"' || css[i] == b'\'' {
                i = skip_string(css, i);
                continue;
            }
            i += 1;
        }
        return if i < len { i + 1 } else { len };
    }

    // Find end of at-rule prelude
    let mut i = start + 1;
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

    // Block at-rule with body
    let should_skip_body = SCSS_SKIP_BODY_AT_RULES.contains(&at_rule_name);
    let is_control_flow = SCSS_CONTROL_FLOW_AT_RULES.contains(&at_rule_name);

    if should_skip_body {
        // Skip body entirely (mixin definitions, keyframes, functions)
        return skip_brace_block(css, i);
    }

    // For @media, @supports, @layer, @container, and control flow — recurse
    i += 1; // skip '{'
    let mut depth = 1u32;

    if is_control_flow {
        // Control flow: scan for nested rules
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
            if css[i] == b'$' {
                i = skip_scss_variable(css, i);
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
    } else {
        // @media, @supports, etc. — recurse
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
            if css[i] == b'$' {
                i = skip_scss_variable(css, i);
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

/// Skip a brace-delimited block `{ ... }`, handling nesting, strings, and comments.
fn skip_brace_block(css: &[u8], start: usize) -> usize {
    let len = css.len();
    let mut i = start + 1; // skip '{'
    let mut depth = 1u32;

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

    i
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_scss(input: &[u8]) -> (Vec<CssParsedRule>, Vec<CssParsedVBind>, Vec<CssParsedClass>) {
        let mut rules = Vec::new();
        let mut v_binds = Vec::new();
        let mut classes = Vec::new();
        scan(input, 0, &mut rules, &mut v_binds, &mut classes);
        (rules, v_binds, classes)
    }

    // --- Basic rules ---

    #[test]
    fn test_single_rule() {
        let (rules, _, _) = scan_scss(b".box { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_multiple_rules() {
        let (rules, _, _) = scan_scss(b".a { color: red; } .b { color: blue; }");
        assert_eq!(rules.len(), 2);
    }

    // --- Line comments ---

    #[test]
    fn test_line_comment_top_level() {
        let (rules, _, _) = scan_scss(b"// this is a comment\n.box { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_line_comment_in_rule() {
        let (rules, _, _) = scan_scss(b".box {\n  // comment\n  color: red;\n}");
        assert_eq!(rules.len(), 1);
    }

    // --- SCSS variables ---

    #[test]
    fn test_variable_skipped() {
        let (rules, _, _) = scan_scss(b"$primary: blue;\n.box { color: $primary; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_variable_with_parens() {
        let (rules, _, _) = scan_scss(b"$sizes: (sm: 10px, md: 20px);\n.box { width: 10px; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_variable_in_rule_body() {
        let (rules, _, _) = scan_scss(b".box { $local: red; color: $local; }");
        assert_eq!(rules.len(), 1);
    }

    // --- SCSS nesting ---

    #[test]
    fn test_nested_rule() {
        let (rules, _, _) = scan_scss(b".parent { .child { color: red; } }");
        assert_eq!(rules.len(), 2); // .parent + .child
    }

    #[test]
    fn test_ampersand_nesting() {
        let (rules, _, _) = scan_scss(b".btn { &:hover { color: blue; } }");
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn test_ampersand_suffix() {
        let (rules, _, _) = scan_scss(b".card { &-header { font-size: 1.5em; } }");
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn test_deep_nesting() {
        let (rules, _, _) = scan_scss(b".a { .b { .c { color: red; } } }");
        assert_eq!(rules.len(), 3);
    }

    // --- SCSS at-rules ---

    #[test]
    fn test_mixin_definition_skipped() {
        let (rules, _, _) =
            scan_scss(b"@mixin button-base { display: inline-block; }\n.btn { color: red; }");
        // Mixin body is skipped, only .btn extracted
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_include_skipped() {
        let (rules, _, _) = scan_scss(b".btn { @include button-base; color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_extend_skipped() {
        let (rules, _, _) = scan_scss(b".btn { @extend .base; color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_use_forward() {
        let (rules, _, _) =
            scan_scss(b"@use 'variables';\n@forward 'mixins';\n.box { color: red; }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_function_definition_skipped() {
        let (rules, _, _) =
            scan_scss(b"@function double($n) { @return $n * 2; }\n.box { width: double(10px); }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_at_media_with_scss_nesting() {
        let (rules, _, _) =
            scan_scss(b"@media (max-width: 600px) { .box { .inner { color: red; } } }");
        assert_eq!(rules.len(), 2); // .box + .inner
    }

    #[test]
    fn test_if_else_control_flow() {
        let (rules, _, _) =
            scan_scss(b"@if $dark { .box { color: white; } } @else { .box { color: black; } }");
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn test_each_loop() {
        let (rules, _, _) =
            scan_scss(b"@each $color in red, green, blue { .#{$color} { color: $color; } }");
        assert_eq!(rules.len(), 1); // .#{$color} rule
    }

    // --- v-bind ---

    #[test]
    fn test_v_bind_in_scss() {
        let (_, v_binds, _) = scan_scss(b".box { color: v-bind(color); }");
        assert_eq!(v_binds.len(), 1);
    }

    #[test]
    fn test_v_bind_nested() {
        let (_, v_binds, _) = scan_scss(b".parent { .child { color: v-bind(c); } }");
        assert_eq!(v_binds.len(), 1);
    }

    // --- Classes ---

    #[test]
    fn test_class_extraction() {
        let (_, _, classes) = scan_scss(b".btn { color: red; } .link { color: blue; }");
        assert_eq!(classes.len(), 2);
    }

    // --- Special pseudos ---

    #[test]
    fn test_deep_in_scss() {
        let (rules, _, _) = scan_scss(b":deep(.inner) { color: red; }");
        assert_eq!(rules[0].selectors[0].specials.len(), 1);
    }

    // --- Keyframes ---

    #[test]
    fn test_keyframes_skipped() {
        let (rules, _, _) =
            scan_scss(b"@keyframes fade { from { opacity: 0; } to { opacity: 1; } }");
        assert_eq!(rules.len(), 0);
    }

    // --- Interpolation ---

    #[test]
    fn test_interpolation_in_selector() {
        let (rules, _, _) = scan_scss(b".icon-#{$name} { display: inline; }");
        assert_eq!(rules.len(), 1);
    }

    // --- Complex real-world patterns ---

    #[test]
    fn test_real_world_scss() {
        let scss = b"\
$primary: #333;\n\
$spacing: 16px;\n\
\n\
@mixin flex-center {\n\
  display: flex;\n\
  align-items: center;\n\
}\n\
\n\
.card {\n\
  padding: $spacing;\n\
  \n\
  &-header {\n\
    @include flex-center;\n\
    font-weight: bold;\n\
  }\n\
  \n\
  &-body {\n\
    color: v-bind(textColor);\n\
    \n\
    .highlight {\n\
      background: $primary;\n\
    }\n\
  }\n\
}\n\
\n\
@media (max-width: 768px) {\n\
  .card {\n\
    padding: $spacing / 2;\n\
  }\n\
}";
        let (rules, v_binds, classes) = scan_scss(scss);
        // Rules: .card, &-header, &-body, .highlight, .card (in @media)
        assert_eq!(rules.len(), 5);
        assert_eq!(v_binds.len(), 1);
        assert!(classes.len() >= 2); // .card, .highlight, .card (in @media)
    }
}

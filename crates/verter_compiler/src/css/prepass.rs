//! String-level pre-pass for Vue-specific CSS syntax.
//!
//! Transforms non-standard Vue syntax into valid CSS before lightningcss parsing:
//! - `v-bind(expr)` → `var(--{scopeId}-{sanitized})`
//! - `:deep(.inner)` → `[__v_deep__] .inner`
//! - `:slotted(.inner)` → `.inner[__v_slotted__]`
//! - `:global()` → left as-is (lightningcss handles natively)

use std::borrow::Cow;

use super::types::VBindVar;

/// Marker attribute used to represent `:deep()` after pre-pass.
/// The scoped visitor replaces this with the actual `[data-v-xxx]`.
pub const DEEP_MARKER: &str = "[__v_deep__]";

/// Marker attribute used to represent `:slotted()` after pre-pass.
/// The scoped visitor replaces this with `[data-v-xxx-s]`.
pub const SLOTTED_MARKER: &str = "[__v_slotted__]";

/// Pre-pass result containing transformed CSS and extracted v-bind info.
///
/// `css` borrows the input when no Vue syntax was found (zero-copy); it is owned
/// only when a `v-bind`/`:deep`/`:slotted` rewrite actually occurred.
///
/// `has_deep` / `has_slotted` are the structural facts the pre-pass discovered
/// while scanning: each is `true` iff a `:deep()`/`::v-deep()` or
/// `:slotted()`/`::v-slotted()` selector was actually rewritten. The owner path
/// forwards these facts on its result instead of re-scanning the CSS.
pub struct PrepassResult<'a> {
    /// CSS with Vue syntax replaced by valid CSS markers — borrowed when untouched.
    pub css: Cow<'a, str>,
    /// Extracted v-bind() replacements
    pub v_bind_vars: Vec<VBindVar>,
    /// Whether a `:deep()` / `::v-deep()` selector was found and rewritten.
    pub has_deep: bool,
    /// Whether a `:slotted()` / `::v-slotted()` selector was found and rewritten.
    pub has_slotted: bool,
}

/// Run the pre-pass on CSS, replacing Vue-specific syntax with valid CSS.
///
/// # Safety of byte-level iteration
///
/// This function iterates over `css.as_bytes()` rather than `char_indices()` for
/// performance. This is safe because:
/// 1. All matched patterns (`v-bind(`, `:deep(`, `:slotted(`, `/*`, `*/`, quotes)
///    are pure ASCII bytes.
/// 2. UTF-8 guarantees that continuation bytes (0x80–0xBF) never equal ASCII bytes
///    (0x00–0x7F), so scanning for ASCII delimiters byte-by-byte cannot produce
///    false matches inside multi-byte characters.
/// 3. Segment boundaries (`seg_start`, `i`) are only set at positions that follow
///    ASCII pattern matches, which are always valid UTF-8 character boundaries.
pub fn prepass<'a>(css: &'a str, scope_id: &str) -> PrepassResult<'a> {
    // The output buffer is allocated lazily on the first rewrite. If the loop
    // completes without touching anything, the input is returned borrowed.
    let mut output: Option<String> = None;
    let mut v_bind_vars = Vec::new();
    // Structural facts surfaced to the caller: set when an actual rewrite of the
    // corresponding selector kind happens (an empty/malformed `:deep()` that
    // passes through unchanged does not set the flag).
    let mut has_deep = false;
    let mut has_slotted = false;
    let bytes = css.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    // Track start of the current unchanged segment to flush via push_str
    let mut seg_start = 0;

    while i < len {
        // Skip block comments
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2; // skip */
            }
            continue;
        }

        // Skip strings
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            while i < len && bytes[i] != quote {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 1; // skip escaped char
                }
                i += 1;
            }
            if i < len {
                i += 1; // skip closing quote
            }
            continue;
        }

        // Macro to flush the pending unchanged segment, write a replacement,
        // record the selector-kind fact, and advance.
        macro_rules! try_transform {
            ($check_len:expr, $pattern:expr, $transform:expr, $on_hit:expr) => {
                if i + $check_len <= len && &bytes[i..i + $check_len] == $pattern {
                    if let Some(result) = $transform {
                        // First rewrite allocates; flush unchanged text before this match
                        let out =
                            output.get_or_insert_with(|| String::with_capacity(css.len() + 128));
                        out.push_str(&css[seg_start..i]);
                        out.push_str(&result.0);
                        $on_hit;
                        i = result.1;
                        seg_start = i;
                        continue;
                    }
                }
            };
        }

        // Check for v-bind(
        if i + 7 <= len && &bytes[i..i + 7] == b"v-bind(" {
            if let Some((replacement, new_pos, var)) = transform_v_bind(css, i, scope_id) {
                let out = output.get_or_insert_with(|| String::with_capacity(css.len() + 128));
                out.push_str(&css[seg_start..i]);
                out.push_str(&replacement);
                v_bind_vars.push(var);
                i = new_pos;
                seg_start = i;
                continue;
            }
        }

        // Check for :deep( / ::v-deep(
        try_transform!(6, b":deep(", transform_deep(css, i), has_deep = true);
        try_transform!(9, b"::v-deep(", transform_deep(css, i), has_deep = true);

        // Check for :slotted( / ::v-slotted(
        try_transform!(
            9,
            b":slotted(",
            transform_slotted(css, i),
            has_slotted = true
        );
        try_transform!(
            12,
            b"::v-slotted(",
            transform_slotted(css, i),
            has_slotted = true
        );

        i += 1;
    }

    match output {
        // At least one rewrite happened: flush the trailing unchanged segment.
        Some(mut out) => {
            out.push_str(&css[seg_start..]);
            PrepassResult {
                css: Cow::Owned(out),
                v_bind_vars,
                has_deep,
                has_slotted,
            }
        }
        // Nothing matched: borrow the input verbatim (zero-copy passthrough).
        None => PrepassResult {
            css: Cow::Borrowed(css),
            v_bind_vars,
            has_deep,
            has_slotted,
        },
    }
}

/// Transform `v-bind(expr)` → `var(--{scopeId}-{sanitized})`.
/// Returns (replacement_string, new_position, VBindVar) or None if malformed.
fn transform_v_bind(css: &str, start: usize, scope_id: &str) -> Option<(String, usize, VBindVar)> {
    let bytes = css.as_bytes();
    let paren_start = start + 6; // position of '(' in "v-bind("
    let expr_start = paren_start + 1;

    // Find matching closing paren
    let mut depth = 1u32;
    let mut j = expr_start;
    while j < bytes.len() && depth > 0 {
        match bytes[j] {
            b'\'' | b'"' | b'`' => {
                let quote = bytes[j];
                j += 1;
                while j < bytes.len() && bytes[j] != quote {
                    if bytes[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
            }
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

    let expr = css[expr_start..expr_end].trim();

    // Remove quotes if present (need at least 2 chars for open+close quote)
    let expr_clean = if expr.len() >= 2
        && ((expr.starts_with('\'') && expr.ends_with('\''))
            || (expr.starts_with('"') && expr.ends_with('"')))
    {
        &expr[1..expr.len() - 1]
    } else {
        expr
    };

    let var_name = generate_var_name(scope_id, expr_clean);
    let replacement = format!("var({})", var_name);

    let v_bind_var = VBindVar {
        expression: expr_clean.to_string(),
        var_name: var_name.clone(),
    };

    Some((replacement, full_end, v_bind_var))
}

/// Generate CSS variable name from scope ID and expression.
/// Sanitizes the expression for use as a CSS variable name.
///
/// Matches Vue's upstream behavior: replaces non-word characters (except hyphens)
/// with `_`. Word characters are ASCII `[a-zA-Z0-9_]`.
pub fn generate_var_name(scope_id: &str, expr: &str) -> String {
    let mut sanitized = String::with_capacity(expr.len());
    for c in expr.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            sanitized.push(c);
        } else {
            sanitized.push('_');
        }
    }
    format!("--{}-{}", scope_id, sanitized)
}

/// Transform `:deep(.inner)` or `::v-deep(.inner)` → `[__v_deep__] .inner`.
///
/// Handles comma-separated selectors inside `:deep()`:
/// `:deep(.a, .b)` → `[__v_deep__] .a, [__v_deep__] .b`
///
/// Returns (replacement_string, new_position) or None if malformed.
fn transform_deep(css: &str, start: usize) -> Option<(String, usize)> {
    let bytes = css.as_bytes();

    // Find the opening paren
    let paren_pos = css[start..].find('(')?;
    let paren_start = start + paren_pos;
    let inner_start = paren_start + 1;

    // Find matching closing paren
    let mut depth = 1u32;
    let mut j = inner_start;
    while j < bytes.len() && depth > 0 {
        match bytes[j] {
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

    let inner_end = j;
    let full_end = j + 1;

    let inner = css[inner_start..inner_end].trim();

    // WS 5.3: empty :deep() — pass through unchanged
    if inner.is_empty() {
        return None;
    }

    // WS 5.1: Split by top-level commas inside :deep()
    let parts = split_by_top_level_commas(inner);
    let replacement = if parts.len() > 1 {
        // :deep(.a, .b) → [__v_deep__] .a, [__v_deep__] .b
        parts
            .iter()
            .map(|part| format!("{} {}", DEEP_MARKER, part.trim()))
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        format!("{} {}", DEEP_MARKER, inner)
    };

    Some((replacement, full_end))
}

/// Split a string by top-level commas, respecting parentheses nesting.
fn split_by_top_level_commas(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0u32;

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Transform `:slotted(.inner)` or `::v-slotted(.inner)` → `.inner[__v_slotted__]`.
/// Returns (replacement_string, new_position) or None if malformed.
fn transform_slotted(css: &str, start: usize) -> Option<(String, usize)> {
    let bytes = css.as_bytes();

    // Find the opening paren
    let paren_pos = css[start..].find('(')?;
    let paren_start = start + paren_pos;
    let inner_start = paren_start + 1;

    // Find matching closing paren
    let mut depth = 1u32;
    let mut j = inner_start;
    while j < bytes.len() && depth > 0 {
        match bytes[j] {
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

    let inner_end = j;
    let full_end = j + 1;

    let inner = css[inner_start..inner_end].trim();

    // :slotted(.inner) → .inner[__v_slotted__]
    let replacement = format!("{}{}", inner, SLOTTED_MARKER);

    Some((replacement, full_end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v_bind_simple() {
        let result = prepass(".box { color: v-bind(color); }", "a4f2eed6");
        assert_eq!(&*result.css, ".box { color: var(--a4f2eed6-color); }");
        assert_eq!(result.v_bind_vars.len(), 1);
        assert_eq!(result.v_bind_vars[0].expression, "color");
        assert_eq!(result.v_bind_vars[0].var_name, "--a4f2eed6-color");
    }

    #[test]
    fn test_v_bind_quoted() {
        let result = prepass(".box { color: v-bind('theme.color'); }", "a4f2eed6");
        assert_eq!(&*result.css, ".box { color: var(--a4f2eed6-theme_color); }");
        assert_eq!(result.v_bind_vars[0].expression, "theme.color");
    }

    #[test]
    fn test_v_bind_nested_parens() {
        let result = prepass(".box { width: v-bind(calc(a + b)); }", "a4f2eed6");
        assert!(result.css.contains("var(--a4f2eed6-calc_a___b_)"));
    }

    #[test]
    fn test_deep_selector() {
        let result = prepass(":deep(.inner) { color: red; }", "a4f2eed6");
        assert_eq!(&*result.css, "[__v_deep__] .inner { color: red; }");
    }

    #[test]
    fn test_deep_with_prefix() {
        let result = prepass(".parent :deep(.inner) { color: red; }", "a4f2eed6");
        assert_eq!(&*result.css, ".parent [__v_deep__] .inner { color: red; }");
    }

    #[test]
    fn test_v_deep_legacy() {
        let result = prepass("::v-deep(.inner) { color: red; }", "a4f2eed6");
        assert_eq!(&*result.css, "[__v_deep__] .inner { color: red; }");
    }

    #[test]
    fn test_slotted_selector() {
        let result = prepass(":slotted(.slot) { color: red; }", "a4f2eed6");
        assert_eq!(&*result.css, ".slot[__v_slotted__] { color: red; }");
    }

    #[test]
    fn test_v_slotted_legacy() {
        let result = prepass("::v-slotted(.slot) { color: red; }", "a4f2eed6");
        assert_eq!(&*result.css, ".slot[__v_slotted__] { color: red; }");
    }

    #[test]
    fn test_global_passthrough() {
        // :global() should be left as-is for lightningcss to handle
        let result = prepass(":global(.reset) { margin: 0; }", "a4f2eed6");
        assert_eq!(&*result.css, ":global(.reset) { margin: 0; }");
    }

    #[test]
    fn test_v_bind_in_string_not_transformed() {
        let result = prepass(".box::before { content: 'v-bind(color)'; }", "a4f2eed6");
        assert!(result.css.contains("'v-bind(color)'"));
        assert!(result.v_bind_vars.is_empty());
    }

    #[test]
    fn test_v_bind_in_comment_not_transformed() {
        let result = prepass("/* v-bind(color) */ .box { color: red; }", "a4f2eed6");
        assert!(result.css.contains("/* v-bind(color) */"));
        assert!(result.v_bind_vars.is_empty());
    }

    #[test]
    fn test_multiple_v_binds() {
        let result = prepass(
            ".box { color: v-bind(fg); background: v-bind(bg); }",
            "a4f2eed6",
        );
        assert!(result.css.contains("var(--a4f2eed6-fg)"));
        assert!(result.css.contains("var(--a4f2eed6-bg)"));
        assert_eq!(result.v_bind_vars.len(), 2);
    }

    #[test]
    fn test_mixed_transforms() {
        let result = prepass(":deep(.inner) { color: v-bind(color); }", "a4f2eed6");
        assert_eq!(
            &*result.css,
            "[__v_deep__] .inner { color: var(--a4f2eed6-color); }"
        );
        assert_eq!(result.v_bind_vars.len(), 1);
    }

    #[test]
    fn test_v_bind_single_quote_char() {
        // Edge case: v-bind with a single quote char — should not panic
        let result = prepass(".box { color: v-bind('); }", "a4f2eed6");
        // Malformed — the quote is treated as expr content, not stripped
        assert!(result.v_bind_vars.is_empty() || !result.v_bind_vars[0].expression.is_empty());
    }

    #[test]
    fn test_v_bind_empty_parens() {
        // Edge case: v-bind() with empty expression
        let result = prepass(".box { color: v-bind(); }", "a4f2eed6");
        assert_eq!(result.v_bind_vars.len(), 1);
        assert_eq!(result.v_bind_vars[0].expression, "");
    }

    #[test]
    fn test_v_bind_unclosed() {
        // Edge case: unclosed v-bind( — should not transform
        let result = prepass(".box { color: v-bind(color; }", "a4f2eed6");
        assert!(result.css.contains("v-bind(color"));
        assert!(result.v_bind_vars.is_empty());
    }

    #[test]
    fn test_non_ascii_in_comment() {
        // Non-ASCII characters in CSS comments should pass through unmodified
        let result = prepass("/* 日本語コメント */ .box { color: red; }", "a4f2eed6");
        assert!(result.css.contains("日本語コメント"));
        assert!(result.css.contains(".box"));
    }

    #[test]
    fn test_non_ascii_in_content() {
        // Non-ASCII content should pass through unmodified
        let result = prepass(".box::before { content: '你好'; }", "a4f2eed6");
        assert!(result.css.contains("你好"));
    }

    #[test]
    fn test_v_bind_optional_chaining() {
        let result = prepass(".box { color: v-bind(props?.color); }", "a4f2eed6");
        assert_eq!(
            &*result.css,
            ".box { color: var(--a4f2eed6-props__color); }"
        );
        assert_eq!(result.v_bind_vars[0].expression, "props?.color");
    }

    #[test]
    fn test_v_bind_array_access() {
        let result = prepass(".box { color: v-bind(arr[0]); }", "a4f2eed6");
        assert_eq!(&*result.css, ".box { color: var(--a4f2eed6-arr_0_); }");
    }

    #[test]
    fn test_v_bind_arithmetic() {
        let result = prepass(".box { width: v-bind(a + b); }", "a4f2eed6");
        assert_eq!(&*result.css, ".box { width: var(--a4f2eed6-a___b); }");
    }

    #[test]
    fn test_v_bind_function_call() {
        let result = prepass(".box { color: v-bind(fn(x)); }", "a4f2eed6");
        assert_eq!(&*result.css, ".box { color: var(--a4f2eed6-fn_x_); }");
    }

    #[test]
    fn test_v_bind_dollar_sign() {
        let result = prepass(".box { color: v-bind($color); }", "a4f2eed6");
        assert_eq!(&*result.css, ".box { color: var(--a4f2eed6-_color); }");
    }

    #[test]
    fn test_v_bind_hyphen_preserved() {
        let result = prepass(".box { color: v-bind(my-color); }", "a4f2eed6");
        assert_eq!(&*result.css, ".box { color: var(--a4f2eed6-my-color); }");
    }

    #[test]
    fn test_deep_without_parens() {
        // :deep without parens should pass through unchanged
        let result = prepass(":deep { color: red; }", "a4f2eed6");
        assert!(result.css.contains(":deep"));
        assert!(!result.css.contains("__v_deep__"));
    }

    #[test]
    fn test_deep_with_comma_separated_selectors() {
        // WS 5.1: :deep(.a, .b) should expand each selector separately
        let result = prepass(":deep(.a, .b) { color: red; }", "a4f2eed6");
        assert_eq!(
            &*result.css,
            "[__v_deep__] .a, [__v_deep__] .b { color: red; }"
        );
    }

    #[test]
    fn test_deep_with_comma_and_nested_parens() {
        // Comma inside :not() should NOT split
        let result = prepass(":deep(:not(.a), .b) { color: red; }", "a4f2eed6");
        assert_eq!(
            &*result.css,
            "[__v_deep__] :not(.a), [__v_deep__] .b { color: red; }"
        );
    }

    #[test]
    fn test_deep_empty_parens_passthrough() {
        // WS 5.3: :deep() with empty parens should pass through unchanged
        let result = prepass(":deep() { color: red; }", "a4f2eed6");
        assert!(
            result.css.contains(":deep()"),
            "empty :deep() should pass through: {}",
            result.css
        );
        assert!(
            !result.css.contains("__v_deep__"),
            "empty :deep() should not produce deep marker"
        );
    }
}

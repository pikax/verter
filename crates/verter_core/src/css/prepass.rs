//! String-level pre-pass for Vue-specific CSS syntax.
//!
//! Transforms non-standard Vue syntax into valid CSS before lightningcss parsing:
//! - `v-bind(expr)` → `var(--{scopeId}-{sanitized})`
//! - `:deep(.inner)` → `[__v_deep__] .inner`
//! - `:slotted(.inner)` → `.inner[__v_slotted__]`
//! - `:global()` → left as-is (lightningcss handles natively)

use super::types::VBindVar;

/// Marker attribute used to represent `:deep()` after pre-pass.
/// The scoped visitor replaces this with the actual `[data-v-xxx]`.
pub const DEEP_MARKER: &str = "[__v_deep__]";

/// Marker attribute used to represent `:slotted()` after pre-pass.
/// The scoped visitor replaces this with `[data-v-xxx-s]`.
pub const SLOTTED_MARKER: &str = "[__v_slotted__]";

/// Pre-pass result containing transformed CSS and extracted v-bind info.
pub struct PrepassResult {
    /// CSS with Vue syntax replaced by valid CSS markers
    pub css: String,
    /// Extracted v-bind() replacements
    pub v_bind_vars: Vec<VBindVar>,
}

/// Run the pre-pass on CSS, replacing Vue-specific syntax with valid CSS.
pub fn prepass(css: &str, scope_id: &str) -> PrepassResult {
    let mut output = String::with_capacity(css.len() + 128);
    let mut v_bind_vars = Vec::new();
    let bytes = css.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Skip block comments
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2; // skip */
            }
            output.push_str(&css[start..i]);
            continue;
        }

        // Skip strings
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            let start = i;
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
            output.push_str(&css[start..i]);
            continue;
        }

        // Check for v-bind(
        if i + 7 <= len && &bytes[i..i + 7] == b"v-bind(" {
            if let Some((replacement, new_pos, var)) = transform_v_bind(css, i, scope_id) {
                output.push_str(&replacement);
                v_bind_vars.push(var);
                i = new_pos;
                continue;
            }
        }

        // Check for :deep(
        if i + 6 <= len && &bytes[i..i + 6] == b":deep(" {
            if let Some((replacement, new_pos)) = transform_deep(css, i) {
                output.push_str(&replacement);
                i = new_pos;
                continue;
            }
        }

        // Check for ::v-deep(
        if i + 9 <= len && &bytes[i..i + 9] == b"::v-deep(" {
            if let Some((replacement, new_pos)) = transform_deep(css, i) {
                output.push_str(&replacement);
                i = new_pos;
                continue;
            }
        }

        // Check for :slotted(
        if i + 9 <= len && &bytes[i..i + 9] == b":slotted(" {
            if let Some((replacement, new_pos)) = transform_slotted(css, i) {
                output.push_str(&replacement);
                i = new_pos;
                continue;
            }
        }

        // Check for ::v-slotted(
        if i + 12 <= len && &bytes[i..i + 12] == b"::v-slotted(" {
            if let Some((replacement, new_pos)) = transform_slotted(css, i) {
                output.push_str(&replacement);
                i = new_pos;
                continue;
            }
        }

        output.push(css.as_bytes()[i] as char);
        i += 1;
    }

    PrepassResult {
        css: output,
        v_bind_vars,
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

    // Remove quotes if present
    let expr_clean = if (expr.starts_with('\'') && expr.ends_with('\''))
        || (expr.starts_with('"') && expr.ends_with('"'))
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
fn generate_var_name(scope_id: &str, expr: &str) -> String {
    let sanitized = expr.replace([' ', '\'', '"'], "").replace('.', "-");
    format!("--{}-{}", scope_id, sanitized)
}

/// Transform `:deep(.inner)` or `::v-deep(.inner)` → `[__v_deep__] .inner`.
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

    // :deep(.inner) → [__v_deep__] .inner
    let replacement = format!("{} {}", DEEP_MARKER, inner);

    Some((replacement, full_end))
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
        assert_eq!(result.css, ".box { color: var(--a4f2eed6-color); }");
        assert_eq!(result.v_bind_vars.len(), 1);
        assert_eq!(result.v_bind_vars[0].expression, "color");
        assert_eq!(result.v_bind_vars[0].var_name, "--a4f2eed6-color");
    }

    #[test]
    fn test_v_bind_quoted() {
        let result = prepass(".box { color: v-bind('theme.color'); }", "a4f2eed6");
        assert_eq!(result.css, ".box { color: var(--a4f2eed6-theme-color); }");
        assert_eq!(result.v_bind_vars[0].expression, "theme.color");
    }

    #[test]
    fn test_v_bind_nested_parens() {
        let result = prepass(".box { width: v-bind(calc(a + b)); }", "a4f2eed6");
        assert!(result.css.contains("var(--a4f2eed6-calc(a+b))"));
    }

    #[test]
    fn test_deep_selector() {
        let result = prepass(":deep(.inner) { color: red; }", "a4f2eed6");
        assert_eq!(result.css, "[__v_deep__] .inner { color: red; }");
    }

    #[test]
    fn test_deep_with_prefix() {
        let result = prepass(".parent :deep(.inner) { color: red; }", "a4f2eed6");
        assert_eq!(result.css, ".parent [__v_deep__] .inner { color: red; }");
    }

    #[test]
    fn test_v_deep_legacy() {
        let result = prepass("::v-deep(.inner) { color: red; }", "a4f2eed6");
        assert_eq!(result.css, "[__v_deep__] .inner { color: red; }");
    }

    #[test]
    fn test_slotted_selector() {
        let result = prepass(":slotted(.slot) { color: red; }", "a4f2eed6");
        assert_eq!(result.css, ".slot[__v_slotted__] { color: red; }");
    }

    #[test]
    fn test_v_slotted_legacy() {
        let result = prepass("::v-slotted(.slot) { color: red; }", "a4f2eed6");
        assert_eq!(result.css, ".slot[__v_slotted__] { color: red; }");
    }

    #[test]
    fn test_global_passthrough() {
        // :global() should be left as-is for lightningcss to handle
        let result = prepass(":global(.reset) { margin: 0; }", "a4f2eed6");
        assert_eq!(result.css, ":global(.reset) { margin: 0; }");
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
            result.css,
            "[__v_deep__] .inner { color: var(--a4f2eed6-color); }"
        );
        assert_eq!(result.v_bind_vars.len(), 1);
    }
}

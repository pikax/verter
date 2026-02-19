//! v-bind() scanner for style blocks.
//!
//! Scans CSS content for `v-bind(expr)` patterns and pushes
//! overwrite operations to [`CodeGenOutput`]. The overwrites replace
//! `v-bind(expr)` with `var(--{scope_id}-{sanitized})` at their
//! absolute SFC source positions.

use crate::css::prepass::generate_var_name;
use crate::css::types::VBindVar;
use crate::new_impl::template::code_gen::types::CodeGenOutput;

/// Scan CSS content for `v-bind()` patterns and push overwrites.
///
/// Each `v-bind(expr)` is replaced by `var(--{scope_id}-{sanitized})` via
/// an overwrite pushed to `out`. The positions are absolute SFC offsets
/// (computed from `content_offset` + local position).
///
/// Skips v-bind inside CSS comments (`/* ... */`) and CSS strings
/// (`'...'` / `"..."`).
///
/// Extracted variable info is pushed to `v_bind_vars` for later use
/// by script codegen (_useCssVars injection).
pub fn scan_v_bind<'alloc>(
    css: &str,
    content_offset: u32,
    scope_id: &str,
    out: &mut CodeGenOutput<'alloc>,
    v_bind_vars: &mut Vec<VBindVar>,
) {
    let bytes = css.as_bytes();
    let len = bytes.len();
    let mut i = 0;

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

        // Check for v-bind(
        if i + 7 <= len && &bytes[i..i + 7] == b"v-bind(" {
            if let Some((replacement, new_pos, var)) = extract_v_bind(css, i, scope_id) {
                let abs_start = content_offset + i as u32;
                let abs_end = content_offset + new_pos as u32;
                out.overwrite(abs_start, abs_end, &replacement);
                v_bind_vars.push(var);
                i = new_pos;
                continue;
            }
        }

        i += 1;
    }
}

/// Extract a single `v-bind(expr)` starting at `start`.
/// Returns (replacement, new_position_after, VBindVar) or None if malformed.
fn extract_v_bind(css: &str, start: usize, scope_id: &str) -> Option<(String, usize, VBindVar)> {
    let bytes = css.as_bytes();
    let expr_start = start + 7; // after "v-bind("

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
        var_name,
    };

    Some((replacement, full_end, v_bind_var))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;

    fn scan(css: &str, offset: u32, scope_id: &str) -> (Vec<(u32, u32, String)>, Vec<VBindVar>) {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut vars = Vec::new();
        scan_v_bind(css, offset, scope_id, &mut out, &mut vars);
        let overwrites = out
            .overwrites
            .iter()
            .map(|(s, e, c)| (*s, *e, c.to_string()))
            .collect();
        (overwrites, vars)
    }

    #[test]
    fn simple_v_bind() {
        let css = ".box { color: v-bind(color); }";
        let (ow, vars) = scan(css, 0, "a4f2eed6");
        assert_eq!(ow.len(), 1);
        assert_eq!(ow[0].0, 14); // start of "v-bind(color)"
        assert_eq!(ow[0].1, 27); // end (after ")")
        assert_eq!(ow[0].2, "var(--a4f2eed6-color)");
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].expression, "color");
        assert_eq!(vars[0].var_name, "--a4f2eed6-color");
    }

    #[test]
    fn v_bind_with_content_offset() {
        // Simulates style content starting at offset 100 in the SFC
        let css = "color: v-bind(fg);";
        let (ow, _) = scan(css, 100, "abc");
        assert_eq!(ow.len(), 1);
        assert_eq!(ow[0].0, 107); // 100 + 7 ("color: " = 7 chars)
        assert_eq!(ow[0].1, 117); // 100 + 17 (end of "v-bind(fg)")
        assert_eq!(ow[0].2, "var(--abc-fg)");
    }

    #[test]
    fn quoted_expression() {
        let css = ".box { color: v-bind('theme.color'); }";
        let (ow, vars) = scan(css, 0, "a4f2eed6");
        assert_eq!(ow.len(), 1);
        assert_eq!(ow[0].2, "var(--a4f2eed6-theme_color)");
        assert_eq!(vars[0].expression, "theme.color");
    }

    #[test]
    fn multiple_v_binds() {
        let css = ".box { color: v-bind(fg); background: v-bind(bg); }";
        let (ow, vars) = scan(css, 0, "a4f2eed6");
        assert_eq!(ow.len(), 2);
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].expression, "fg");
        assert_eq!(vars[1].expression, "bg");
    }

    #[test]
    fn v_bind_in_comment_skipped() {
        let css = "/* v-bind(color) */ .box { color: red; }";
        let (ow, vars) = scan(css, 0, "a4f2eed6");
        assert!(ow.is_empty());
        assert!(vars.is_empty());
    }

    #[test]
    fn v_bind_in_string_skipped() {
        let css = ".box::before { content: 'v-bind(color)'; }";
        let (ow, vars) = scan(css, 0, "a4f2eed6");
        assert!(ow.is_empty());
        assert!(vars.is_empty());
    }

    #[test]
    fn unclosed_v_bind_skipped() {
        let css = ".box { color: v-bind(color; }";
        let (ow, vars) = scan(css, 0, "a4f2eed6");
        assert!(ow.is_empty());
        assert!(vars.is_empty());
    }

    #[test]
    fn nested_parens() {
        let css = ".box { width: v-bind(calc(a + b)); }";
        let (ow, vars) = scan(css, 0, "a4f2eed6");
        assert_eq!(ow.len(), 1);
        assert!(ow[0].2.contains("var(--a4f2eed6-calc_a___b_)"));
        assert_eq!(vars[0].expression, "calc(a + b)");
    }

    #[test]
    fn empty_v_bind() {
        let css = ".box { color: v-bind(); }";
        let (ow, vars) = scan(css, 0, "a4f2eed6");
        assert_eq!(ow.len(), 1);
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].expression, "");
    }
}

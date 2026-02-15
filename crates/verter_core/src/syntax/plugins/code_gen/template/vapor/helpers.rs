//! Free helper functions for vapor code generation.
//!
//! These are pure functions that don't depend on `VaporTemplateGenerator` state.

use super::types::VaporTextPart;
use crate::syntax::plugins::code_gen::template::shared::helper::escape_js_string;

/// Build a `_setText(xN, ...)` call from text parts.
pub(crate) fn build_set_text_call(text_ref: u32, parts: &[VaporTextPart]) -> String {
    let args = parts
        .iter()
        .map(|p| match p {
            VaporTextPart::Static(s) => format!("\"{}\"", escape_js_string(s)),
            VaporTextPart::Dynamic(expr) => expr.clone(),
        })
        .collect::<Vec<_>>()
        .join(" + ");
    format!("_setText(x{}, {})", text_ref, args)
}

/// Check if a string is a simple JavaScript member expression (identifier with optional dot-access).
///
/// Returns `true` for `foo`, `foo.bar`, `$setup.x`, `_ctx.msg.length`, etc.
/// Returns `false` for expressions like `a + b`, `fn()`, `a[0]`, etc.
pub(crate) fn is_member_expression(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != '_' && first != '$' {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.')
}

/// Parse an effect string like `_setProp(n0, "attr", expr)` or `_setClass(n0, expr)`
/// into a component prop entry like `attr: () => (expr)`.
///
/// **Deprecated**: Superseded by [`VaporEffect::to_component_prop()`] which uses
/// structured data instead of string parsing. Kept for tests and documentation.
#[cfg(test)]
pub(crate) fn parse_effect_as_component_prop(effect: &str) -> Option<String> {
    // _setProp(n{X}, "attr", expr)
    if let Some(rest) = effect.strip_prefix("_setProp(") {
        let rest = rest.strip_suffix(')')?.to_string();
        // Split: n{X}, "attr", expr
        let first_comma = rest.find(", ")?;
        let after_first = &rest[first_comma + 2..];
        // Find the attr name in quotes.
        if let Some(stripped) = after_first.strip_prefix('"') {
            let end_quote = stripped.find('"')?;
            let attr_name = &stripped[..end_quote];
            let expr = stripped[end_quote + 3..].to_string(); // skip `", `
            return Some(format!("{}: () => ({})", attr_name, expr));
        }
    }
    // _setClass(n{X}, expr)
    if let Some(rest) = effect.strip_prefix("_setClass(") {
        let rest = rest.strip_suffix(')')?.to_string();
        let first_comma = rest.find(", ")?;
        let expr = &rest[first_comma + 2..];
        return Some(format!("class: () => ({})", expr));
    }
    // _setStyle(n{X}, expr)
    if let Some(rest) = effect.strip_prefix("_setStyle(") {
        let rest = rest.strip_suffix(')')?.to_string();
        let first_comma = rest.find(", ")?;
        let expr = &rest[first_comma + 2..];
        return Some(format!("style: () => ({})", expr));
    }
    None
}

/// Replace node reference `n{old}` with `n{new}` in an effect/statement string,
/// using whole-word boundary matching to avoid replacing `n1` inside `n10`, `n11`, etc.
///
/// A word boundary is defined as: the character before/after the match is NOT
/// an ASCII alphanumeric character or `_`.
pub(crate) fn replace_node_ref(s: &str, old_ref: u32, new_ref: u32) -> String {
    let old_token = format!("n{}", old_ref);
    let new_token = format!("n{}", new_ref);

    let mut result = String::with_capacity(s.len());
    let mut remaining = s;
    let bytes = old_token.as_bytes();
    let token_len = bytes.len();

    while let Some(pos) = remaining.find(&old_token) {
        let end = pos + token_len;

        // Check left boundary: char before must not be alphanumeric or '_'
        let left_ok = if pos == 0 {
            true
        } else {
            let prev = remaining.as_bytes()[pos - 1];
            !prev.is_ascii_alphanumeric() && prev != b'_'
        };

        // Check right boundary: char after must not be alphanumeric or '_'
        let right_ok = if end >= remaining.len() {
            true
        } else {
            let next = remaining.as_bytes()[end];
            !next.is_ascii_alphanumeric() && next != b'_'
        };

        if left_ok && right_ok {
            result.push_str(&remaining[..pos]);
            result.push_str(&new_token);
        } else {
            result.push_str(&remaining[..end]);
        }
        remaining = &remaining[end..];
    }
    result.push_str(remaining);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── replace_node_ref tests ──────────────────────────────────────────

    #[test]
    fn test_replace_node_ref_simple() {
        assert_eq!(
            replace_node_ref("_setClass(n1, _ctx.cls)", 1, 5),
            "_setClass(n5, _ctx.cls)"
        );
    }

    #[test]
    fn test_replace_node_ref_does_not_replace_n1_inside_n10() {
        // This is the critical bug: n1 must NOT match inside n10
        assert_eq!(
            replace_node_ref("_setProp(n10, \"attr\", _ctx.val)", 1, 5),
            "_setProp(n10, \"attr\", _ctx.val)"
        );
    }

    #[test]
    fn test_replace_node_ref_does_not_replace_n1_inside_n11() {
        assert_eq!(
            replace_node_ref("_setClass(n11, _ctx.cls)", 1, 5),
            "_setClass(n11, _ctx.cls)"
        );
    }

    #[test]
    fn test_replace_node_ref_does_not_replace_n1_inside_n100() {
        assert_eq!(
            replace_node_ref("_setStyle(n100, _ctx.sty)", 1, 5),
            "_setStyle(n100, _ctx.sty)"
        );
    }

    #[test]
    fn test_replace_node_ref_replaces_n1_but_not_n10_in_same_string() {
        // Both n1 and n10 appear — only n1 should be replaced
        assert_eq!(
            replace_node_ref("_setProp(n1, \"a\", n10)", 1, 5),
            "_setProp(n5, \"a\", n10)"
        );
    }

    #[test]
    fn test_replace_node_ref_multiple_occurrences() {
        assert_eq!(
            replace_node_ref("_setClass(n1, _ctx.cls); _setProp(n1, \"x\", y)", 1, 5),
            "_setClass(n5, _ctx.cls); _setProp(n5, \"x\", y)"
        );
    }

    #[test]
    fn test_replace_node_ref_at_end_of_string() {
        assert_eq!(replace_node_ref("ref = n1", 1, 5), "ref = n5");
    }

    #[test]
    fn test_replace_node_ref_at_start_of_string() {
        assert_eq!(
            replace_node_ref("n1.$evtclick = handler", 1, 5),
            "n5.$evtclick = handler"
        );
    }

    #[test]
    fn test_replace_node_ref_no_match() {
        assert_eq!(
            replace_node_ref("_setClass(n2, _ctx.cls)", 1, 5),
            "_setClass(n2, _ctx.cls)"
        );
    }

    #[test]
    fn test_replace_node_ref_double_digit() {
        assert_eq!(
            replace_node_ref("_setClass(n10, _ctx.cls)", 10, 20),
            "_setClass(n20, _ctx.cls)"
        );
    }

    #[test]
    fn test_replace_node_ref_n10_not_confused_with_n1() {
        // Replacing n10 should not affect n1 or n100
        assert_eq!(
            replace_node_ref("n1 + n10 + n100", 10, 20),
            "n1 + n20 + n100"
        );
    }

    // ── parse_effect_as_component_prop tests ────────────────────────────

    #[test]
    fn test_parse_effect_set_prop() {
        assert_eq!(
            parse_effect_as_component_prop("_setProp(n0, \"title\", _ctx.msg)"),
            Some("title: () => (_ctx.msg)".to_string())
        );
    }

    #[test]
    fn test_parse_effect_set_class() {
        assert_eq!(
            parse_effect_as_component_prop("_setClass(n0, _ctx.cls)"),
            Some("class: () => (_ctx.cls)".to_string())
        );
    }

    #[test]
    fn test_parse_effect_set_style() {
        assert_eq!(
            parse_effect_as_component_prop("_setStyle(n0, _ctx.sty)"),
            Some("style: () => (_ctx.sty)".to_string())
        );
    }

    #[test]
    fn test_parse_effect_nested_parens() {
        // This tests the fragile string parsing — expressions with nested parens
        // should still work correctly
        assert_eq!(
            parse_effect_as_component_prop("_setProp(n0, \"title\", fn(a, b))"),
            Some("title: () => (fn(a, b))".to_string())
        );
    }

    #[test]
    fn test_parse_effect_unknown_returns_none() {
        assert_eq!(
            parse_effect_as_component_prop("_setHtml(n0, _ctx.html)"),
            None
        );
    }
}

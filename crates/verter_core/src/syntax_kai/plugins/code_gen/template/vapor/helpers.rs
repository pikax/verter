//! Free helper functions for vapor code generation.
//!
//! These are pure functions that don't depend on `VaporTemplateGenerator` state.

use super::types::VaporTextPart;
use crate::syntax_kai::plugins::code_gen::template::shared::helper::escape_js_string;

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

/// Check if a string is a simple JavaScript identifier (possibly with dot-access).
pub(crate) fn is_simple_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != '_' && first != '$' {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.')
}

/// Apply v-for / v-slot variable mappings to an expression string.
/// Replaces standalone occurrences of original variable names with their mapped values.
/// E.g., `item` → `_for_item0.value`, `data` → `_slotProps0.data`.
pub(crate) fn apply_var_mappings(expr: &str, mappings: &[(String, String)]) -> String {
    if mappings.is_empty() {
        return expr.to_string();
    }
    let mut result = expr.to_string();
    // Apply mappings in reverse order of name length (longest first) to avoid
    // partial replacements (e.g., `item` matching inside `itemCount`).
    let mut sorted: Vec<&(String, String)> = mappings.iter().collect();
    sorted.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    for (orig, mapped) in sorted {
        // Replace whole-word occurrences only.
        // Use a simple boundary check: the char before/after must not be alphanumeric or _.
        let mut new_result = String::new();
        let mut remaining = result.as_str();
        while let Some(pos) = remaining.find(orig.as_str()) {
            // Check left boundary.
            let left_ok = if pos == 0 {
                true
            } else {
                let prev = remaining.as_bytes()[pos - 1];
                !prev.is_ascii_alphanumeric() && prev != b'_' && prev != b'$' && prev != b'.'
            };
            // Check right boundary.
            let end = pos + orig.len();
            let right_ok = if end >= remaining.len() {
                true
            } else {
                let next = remaining.as_bytes()[end];
                !next.is_ascii_alphanumeric() && next != b'_' && next != b'$'
            };
            if left_ok && right_ok {
                new_result.push_str(&remaining[..pos]);
                new_result.push_str(mapped);
                remaining = &remaining[end..];
            } else {
                new_result.push_str(&remaining[..end]);
                remaining = &remaining[end..];
            }
        }
        new_result.push_str(remaining);
        result = new_result;
    }
    result
}

/// Parse an effect string like `_setProp(n0, "attr", expr)` or `_setClass(n0, expr)`
/// into a component prop entry like `attr: () => (expr)`.
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

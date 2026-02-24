//! Structural directive → JSX constructs for TSX template codegen.
//!
//! Converts Vue structural directives to JSX-compatible constructs:
//! - `v-if="cond"` → `{cond ? (...) : null}`
//! - `v-else-if="cond"` → chained ternary
//! - `v-else` → final branch of ternary
//! - `v-for="item in items"` → `{items.map((item) => (...))}`
//! - `v-show="expr"` → `style={{display: expr ? undefined : 'none'}}`

use oxc_allocator::Allocator;

use crate::ast::types::{ElementNode, ElementNodeConditionKind};
use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::types::CodeGenOutput;
use crate::template::oxc::types::OxcParsedElement;

/// Emit the opening of a v-if/v-else-if/v-else ternary.
pub fn emit_v_if_open<'alloc>(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    _alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
) {
    let condition = match &el.v_condition {
        Some(c) => c,
        None => return,
    };

    match condition.kind {
        ElementNodeConditionKind::If => {
            // v-if="cond" → {cond ? (
            if let (Some(vs), Some(ve)) = (condition.prop.value_start, condition.prop.value_end) {
                let cond_expr = &source[vs as usize..ve as usize];
                out.prepend_alloc(el.tag_open.start, &format!("{{{} ? (", cond_expr));

                // Apply binding patches to condition expression
                if let Some(oxc_el) = oxc_el {
                    if let Some(ref cond) = oxc_el.condition {
                        if let Some(ref bindings) = cond.bindings {
                            resolver.collect_binding_patches(bindings, out);
                        }
                    }
                }
            }
        }
        ElementNodeConditionKind::ElseIf => {
            // v-else-if="cond" → cond ? (
            if let (Some(vs), Some(ve)) = (condition.prop.value_start, condition.prop.value_end) {
                let cond_expr = &source[vs as usize..ve as usize];
                out.prepend_alloc(el.tag_open.start, &format!("{} ? (", cond_expr));

                // Apply binding patches
                if let Some(oxc_el) = oxc_el {
                    if let Some(ref cond) = oxc_el.condition {
                        if let Some(ref bindings) = cond.bindings {
                            resolver.collect_binding_patches(bindings, out);
                        }
                    }
                }
            }
        }
        ElementNodeConditionKind::Else => {
            // v-else → ( (just open paren for final branch)
            out.prepend_alloc(el.tag_open.start, "(");
        }
    }
}

/// Emit the closing of a v-if/v-else-if/v-else ternary.
pub fn emit_v_if_close(el: &ElementNode, _source: &str, out: &mut CodeGenOutput<'_>) {
    let condition = match &el.v_condition {
        Some(c) => c,
        None => return,
    };

    let el_end = el
        .tag_close
        .as_ref()
        .map(|tc| tc.end)
        .unwrap_or(el.tag_open.end);

    match condition.kind {
        ElementNodeConditionKind::If => {
            // Check if next sibling is v-else-if or v-else
            // If not, close the ternary: ) : null}
            // The actual chaining is handled by sibling processing
            out.prepend_alloc(el_end, ") : null}");
        }
        ElementNodeConditionKind::ElseIf => {
            // Close this branch, open next: ) :
            out.prepend_alloc(el_end, ") : null}");
        }
        ElementNodeConditionKind::Else => {
            // Close final branch: )}
            out.prepend_alloc(el_end, ")}");
        }
    }
}

/// Emit the opening of a v-for map expression.
///
/// `v-for="item in items"` → `{items.map((item) => (`
pub fn emit_v_for_open<'alloc>(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    _alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
) {
    let v_for_prop = match &el.v_for {
        Some(p) => p,
        None => return,
    };

    if let (Some(vs), Some(ve)) = (v_for_prop.value_start, v_for_prop.value_end) {
        let v_for_expr = &source[vs as usize..ve as usize];

        // Parse "item in items" or "(item, index) in items"
        if let Some((params, source_expr)) = parse_v_for_expr(v_for_expr) {
            out.prepend_alloc(
                el.tag_open.start,
                &format!("{{{}.map(({}) => (", source_expr.trim(), params.trim()),
            );

            // Apply binding patches to source expression
            if let Some(oxc_el) = oxc_el {
                if let Some(ref v_for) = oxc_el.v_for {
                    for span in &v_for.parsed.references {
                        // These are already file-relative positions
                        let name = &source[span.start as usize..span.end as usize];
                        let prefix = resolver.resolve_prefix(name);
                        let suffix = resolver.resolve_suffix(name);
                        if !prefix.is_empty() {
                            out.prepend_static(span.start, prefix);
                        }
                        if !suffix.is_empty() {
                            out.prepend_static(span.end, suffix);
                        }
                    }
                }
            }
        }
    }
}

/// Emit the closing of a v-for map expression.
pub fn emit_v_for_close(el: &ElementNode, _source: &str, out: &mut CodeGenOutput<'_>) {
    let el_end = el
        .tag_close
        .as_ref()
        .map(|tc| tc.end)
        .unwrap_or(el.tag_open.end);

    out.prepend_alloc(el_end, "))}");
}

/// Emit v-show as a style attribute.
///
/// `v-show="expr"` → appends `style={{display: expr ? undefined : 'none'}}`
pub fn emit_v_show<'alloc>(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    _alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
) {
    // Find v-show prop
    let show_prop = el.props.iter().enumerate().find(|(_, p)| {
        p.is_directive && {
            let name = &source[p.start as usize..p.name_end as usize];
            name == "v-show"
        }
    });

    let (show_idx, show) = match show_prop {
        Some((idx, p)) => (idx, p),
        None => return,
    };

    if let (Some(vs), Some(ve)) = (show.value_start, show.value_end) {
        let expr = &source[vs as usize..ve as usize];
        let prop_end = super::props::get_prop_end(show);

        // Replace v-show directive with style attribute
        out.overwrite(
            show.start,
            prop_end,
            &format!("style={{{{display: {} ? undefined : 'none'}}}}", expr),
        );

        // Apply binding patches
        if let Some(oxc_el) = oxc_el {
            if let Some(oxc_prop) = oxc_el.props.iter().find(|p| p.prop_index == show_idx) {
                if let Some(ref exp) = oxc_prop.exp {
                    if let Some(ref bindings) = exp.bindings {
                        resolver.collect_binding_patches(bindings, out);
                    }
                }
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────

/// Parse a v-for expression into (params, source).
///
/// Handles:
/// - `item in items` → ("item", "items")
/// - `(item, index) in items` → ("item, index", "items")
/// - `item of items` → ("item", "items")
/// - `(value, key, index) in obj` → ("value, key, index", "obj")
fn parse_v_for_expr(expr: &str) -> Option<(&str, &str)> {
    // Find " in " or " of " separator
    let sep_pos = expr
        .find(" in ")
        .map(|pos| (pos, 4)) // " in " is 4 chars
        .or_else(|| expr.find(" of ").map(|pos| (pos, 4))); // " of " is 4 chars

    let (sep_start, sep_len) = sep_pos?;

    let params = expr[..sep_start].trim();
    let source = expr[sep_start + sep_len..].trim();

    // Strip parentheses from params if present
    let params = params
        .strip_prefix('(')
        .and_then(|p| p.strip_suffix(')'))
        .unwrap_or(params);

    Some((params, source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_v_for_basic() {
        let (params, source) = parse_v_for_expr("item in items").unwrap();
        assert_eq!(params, "item");
        assert_eq!(source, "items");
    }

    #[test]
    fn parse_v_for_with_index() {
        let (params, source) = parse_v_for_expr("(item, index) in items").unwrap();
        assert_eq!(params, "item, index");
        assert_eq!(source, "items");
    }

    #[test]
    fn parse_v_for_object() {
        let (params, source) = parse_v_for_expr("(value, key, index) in obj").unwrap();
        assert_eq!(params, "value, key, index");
        assert_eq!(source, "obj");
    }

    #[test]
    fn parse_v_for_of() {
        let (params, source) = parse_v_for_expr("item of items").unwrap();
        assert_eq!(params, "item");
        assert_eq!(source, "items");
    }

    #[test]
    fn parse_v_for_numeric() {
        let (params, source) = parse_v_for_expr("n in 10").unwrap();
        assert_eq!(params, "n");
        assert_eq!(source, "10");
    }
}

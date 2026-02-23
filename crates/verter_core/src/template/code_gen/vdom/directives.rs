//! VDOM structural directive code generation.
//!
//! Handles v-if/v-else-if/v-else, v-for, and runtime directive wrapping
//! (v-model, v-show, custom directives).
//!
//! These are pure helper functions that build output strings. The caller
//! (VdomCodeGen enter_element/leave_element) integrates them into overwrites.

use crate::ast::types::ElementNodeConditionKind;
use crate::template::oxc::types::OxcParsedElement;
use crate::types::NodeProp;

use super::super::binding::BindingResolver;
use super::super::shared::helpers::{extract_directive_value, parse_v_for_expression, VdomHelper};
use super::super::types::{CodeGenOutput, ScopeClose};

// ======================== v-if / v-else-if / v-else ========================

/// Determine the `ScopeClose` variant for a v-if/v-else-if/v-else directive.
///
/// - `If` → `ScopeClose::IfTernary`
/// - `ElseIf` → `ScopeClose::ElseIfTernary`
/// - `Else` → `ScopeClose::Else`
#[inline]
pub fn condition_scope_close(kind: &ElementNodeConditionKind) -> ScopeClose {
    match kind {
        ElementNodeConditionKind::If => ScopeClose::IfTernary,
        ElementNodeConditionKind::ElseIf => ScopeClose::ElseIfTernary,
        ElementNodeConditionKind::Else => ScopeClose::Else,
    }
}

// ======================== v-for ========================

/// Build the scope prefix string for a v-for directive.
///
/// Returns the full prefix to prepend before the element's VNode call.
///
/// `v-for="(item, i) in items"` →
/// `"(_openBlock(true), _createElementBlock(_Fragment, null, _renderList($setup.items, (item, i) => {return "`
///
/// The iterable and parameter parts are extracted from source using v-for
/// parsing conventions: `value_start..value_end` contains the full expression
/// like `"(item, i) in items"` or `"item in items"`.
///
/// When `oxc` and `resolver` are provided, the iterable expression is resolved
/// through the binding resolver for correct `$setup.`/`$props.` prefixes.
pub fn build_for_prefix<'alloc>(
    v_for: &NodeProp,
    source: &str,
    is_keyed: bool,
    oxc: Option<&OxcParsedElement<'alloc>>,
    resolver: &BindingResolver<'alloc>,
) -> (String, ScopeClose) {
    let full_expr = extract_directive_value(v_for, source);

    // Parse v-for expression: "(params) in/of iterable"
    let (params, iterable) = parse_v_for_expression(full_expr);

    // Resolve the iterable expression through the binding resolver.
    // For simple identifiers like `items`, this adds `$setup.` prefix.
    // For compound expressions, use OXC v-for data if available.
    let resolved_iterable = if let Some(oxc_el) = oxc {
        if let Some(oxc_vfor) = &oxc_el.v_for {
            // Use reference spans from OXC to build prefixed iterable.
            // References are external bindings (not loop locals) that need prefixes.
            let refs = &oxc_vfor.parsed.references;
            if refs.is_empty() {
                iterable.to_string()
            } else {
                build_prefixed_iterable(iterable, source, v_for, &oxc_vfor.parsed, resolver)
            }
        } else {
            resolver.resolve_simple_expr(iterable)
        }
    } else {
        resolver.resolve_simple_expr(iterable)
    };

    let mut prefix = String::with_capacity(128);
    prefix.push_str("(_openBlock(true), _createElementBlock(_Fragment, null, _renderList(");
    prefix.push_str(&resolved_iterable);
    prefix.push_str(", (");
    prefix.push_str(params);
    prefix.push_str(") => {return ");

    (prefix, ScopeClose::For { is_keyed })
}

/// Build a prefixed iterable string using v-for reference spans.
///
/// Walks the external references extracted by OXC and inserts binding prefixes
/// at their positions within the iterable text.
fn build_prefixed_iterable(
    iterable: &str,
    source: &str,
    v_for: &NodeProp,
    vfor_parsed: &crate::utils::oxc::vue::VForWithBindings<'_>,
    resolver: &BindingResolver<'_>,
) -> String {
    let refs = &vfor_parsed.references;
    // The iterable starts after " in " or " of " in the full v-for value.
    // Calculate the file-relative offset of the iterable substring.
    let value_start = v_for.value_start.unwrap_or(0) as usize;
    let full_expr = extract_directive_value(v_for, source);
    let iterable_offset_in_expr = full_expr.len() - iterable.len();
    let iterable_file_offset = value_start + iterable_offset_in_expr;

    let mut result = String::with_capacity(iterable.len() + refs.len() * 8);
    let mut last_end = 0usize;

    for ref_span in refs {
        let ref_start = ref_span.start as usize;
        let ref_end = ref_span.end as usize;

        // Check if this reference falls within the iterable portion
        if ref_start < iterable_file_offset || ref_end > iterable_file_offset + iterable.len() {
            continue;
        }

        let rel_start = ref_start - iterable_file_offset;
        let rel_end = ref_end - iterable_file_offset;
        let name = &source[ref_start..ref_end];

        // Append text before this reference
        if rel_start > last_end {
            result.push_str(&iterable[last_end..rel_start]);
        }

        let prefix = resolver.resolve_prefix(name);
        let suffix = resolver.resolve_suffix(name);
        result.push_str(prefix);
        result.push_str(name);
        result.push_str(suffix);

        last_end = rel_end;
    }

    // Append remaining text
    if last_end < iterable.len() {
        result.push_str(&iterable[last_end..]);
    }

    result
}

// ======================== Scope close emission ========================

/// Build the closing string for a scope close marker.
///
/// Returns the string to append after the element's VNode call.
///
/// - `IfTernary` → ` : _createCommentVNode("v-if", true)`
/// - `ElseIfTernary` → ` : `
/// - `Else` → (empty — no suffix needed)
/// - `For { is_keyed: true }` → `}), 128 /* KEYED_FRAGMENT */))`
/// - `For { is_keyed: false }` → `}), 256 /* UNKEYED_FRAGMENT */))`
pub fn format_scope_close(close: &ScopeClose, is_production: bool) -> &'static str {
    match close {
        ScopeClose::IfTernary => " : _createCommentVNode(\"v-if\", true)",
        ScopeClose::ElseIfTernary => " : ",
        ScopeClose::Else => "",
        ScopeClose::For { is_keyed } => match (is_keyed, is_production) {
            (true, false) => "}), 128 /* KEYED_FRAGMENT */))",
            (true, true) => "}), 128))",
            (false, false) => "}), 256 /* UNKEYED_FRAGMENT */))",
            (false, true) => "}), 256))",
        },
        ScopeClose::SlotWrapper => ")",
    }
}

/// Collect runtime helper imports needed for scope directives.
pub fn collect_scope_imports(close: &ScopeClose, out: &mut CodeGenOutput<'_>) {
    match close {
        ScopeClose::IfTernary => {
            out.add_vdom_import(VdomHelper::CreateCommentVNode);
        }
        ScopeClose::ElseIfTernary | ScopeClose::Else => {}
        ScopeClose::For { .. } => {
            out.add_vdom_import(VdomHelper::OpenBlock);
            out.add_vdom_import(VdomHelper::CreateElementBlock);
            out.add_vdom_import(VdomHelper::Fragment);
            out.add_vdom_import(VdomHelper::RenderList);
        }
        ScopeClose::SlotWrapper => {}
    }
}

// ======================== Runtime directive wrapping ========================

/// Format a runtime directive entry for `_withDirectives()`.
///
/// Each directive entry is: `[identifier, value?, arg?, modifiers?]`
///
/// Examples:
/// - `[_vModelText, msg]` (v-model on input)
/// - `[_vShow, show]` (v-show)
/// - `[_directive_focus]` (custom, no value)
/// - `[_vModelText, msg, void 0, { trim: true }]` (v-model with modifiers)
pub fn format_directive_entry(directive: &str, value: &str, arg: &str, modifiers: &str) -> String {
    let mut buf = String::with_capacity(32);
    buf.push('[');
    buf.push_str(directive);

    if !value.is_empty() || !arg.is_empty() || !modifiers.is_empty() {
        buf.push_str(", ");
        if value.is_empty() {
            buf.push_str("void 0");
        } else {
            buf.push_str(value);
        }
    }

    if !arg.is_empty() || !modifiers.is_empty() {
        buf.push_str(", ");
        if arg.is_empty() {
            buf.push_str("void 0");
        } else {
            buf.push_str(arg);
        }
    }

    if !modifiers.is_empty() {
        buf.push_str(", ");
        buf.push_str(modifiers);
    }

    buf.push(']');
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;
    use rustc_hash::FxHashMap;
    use smallvec::SmallVec;

    fn make_directive_prop(value_start: Option<u32>, value_end: Option<u32>) -> NodeProp {
        NodeProp {
            start: 0,
            name_end: 4,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start,
            value_end,
            modifiers: SmallVec::new(),
        }
    }

    fn make_empty_resolver() -> BindingResolver<'static> {
        BindingResolver::new(FxHashMap::default(), false)
    }

    // ==================== parse_v_for_expression ====================

    #[test]
    fn parse_v_for_simple_in() {
        let (params, iterable) = parse_v_for_expression("item in items");
        assert_eq!(params, "item");
        assert_eq!(iterable, "items");
    }

    #[test]
    fn parse_v_for_with_parens() {
        let (params, iterable) = parse_v_for_expression("(item, index) in items");
        assert_eq!(params, "item, index");
        assert_eq!(iterable, "items");
    }

    #[test]
    fn parse_v_for_three_params() {
        let (params, iterable) = parse_v_for_expression("(val, key, idx) in obj");
        assert_eq!(params, "val, key, idx");
        assert_eq!(iterable, "obj");
    }

    #[test]
    fn parse_v_for_of_keyword() {
        let (params, iterable) = parse_v_for_expression("item of items");
        assert_eq!(params, "item");
        assert_eq!(iterable, "items");
    }

    #[test]
    fn parse_v_for_complex_iterable() {
        let (params, iterable) = parse_v_for_expression("item in items.filter(x => x.active)");
        assert_eq!(params, "item");
        assert_eq!(iterable, "items.filter(x => x.active)");
    }

    #[test]
    fn parse_v_for_nested_parens_in_iterable() {
        let (params, iterable) = parse_v_for_expression("(a, b) in fn(x, y)");
        assert_eq!(params, "a, b");
        assert_eq!(iterable, "fn(x, y)");
    }

    // ==================== condition_scope_close ====================

    #[test]
    fn condition_scope_close_if() {
        assert!(matches!(
            condition_scope_close(&ElementNodeConditionKind::If),
            ScopeClose::IfTernary
        ));
    }

    #[test]
    fn condition_scope_close_else_if() {
        assert!(matches!(
            condition_scope_close(&ElementNodeConditionKind::ElseIf),
            ScopeClose::ElseIfTernary
        ));
    }

    #[test]
    fn condition_scope_close_else() {
        assert!(matches!(
            condition_scope_close(&ElementNodeConditionKind::Else),
            ScopeClose::Else
        ));
    }

    // ==================== build_for_prefix ====================

    #[test]
    fn for_prefix_simple() {
        let resolver = make_empty_resolver();
        let prop = make_directive_prop(Some(7), Some(21));
        let source = "v-for=\"item in items\"";
        let (prefix, close) = build_for_prefix(&prop, source, false, None, &resolver);

        assert!(prefix
            .starts_with("(_openBlock(true), _createElementBlock(_Fragment, null, _renderList("));
        assert!(prefix.contains("items"));
        assert!(prefix.contains("(item)"));
        assert!(prefix.ends_with("{return "));
        assert!(matches!(close, ScopeClose::For { is_keyed: false }));
    }

    #[test]
    fn for_prefix_keyed() {
        let resolver = make_empty_resolver();
        let prop = make_directive_prop(Some(7), Some(21));
        let source = "v-for=\"item in items\"";
        let (_, close) = build_for_prefix(&prop, source, true, None, &resolver);
        assert!(matches!(close, ScopeClose::For { is_keyed: true }));
    }

    #[test]
    fn for_prefix_with_index() {
        let resolver = make_empty_resolver();
        let prop = make_directive_prop(Some(7), Some(29));
        let source = "v-for=\"(item, index) in items\"";
        let (prefix, _) = build_for_prefix(&prop, source, false, None, &resolver);

        assert!(prefix.contains("items, (item, index)"));
    }

    // ==================== format_scope_close ====================

    #[test]
    fn scope_close_if_ternary() {
        let result = format_scope_close(&ScopeClose::IfTernary, false);
        assert_eq!(result, " : _createCommentVNode(\"v-if\", true)");
    }

    #[test]
    fn scope_close_else_if_ternary() {
        let result = format_scope_close(&ScopeClose::ElseIfTernary, false);
        assert_eq!(result, " : ");
    }

    #[test]
    fn scope_close_else_empty() {
        let result = format_scope_close(&ScopeClose::Else, false);
        assert_eq!(result, "");
    }

    #[test]
    fn scope_close_for_unkeyed_dev() {
        let result = format_scope_close(&ScopeClose::For { is_keyed: false }, false);
        assert_eq!(result, "}), 256 /* UNKEYED_FRAGMENT */))");
    }

    #[test]
    fn scope_close_for_keyed_dev() {
        let result = format_scope_close(&ScopeClose::For { is_keyed: true }, false);
        assert_eq!(result, "}), 128 /* KEYED_FRAGMENT */))");
    }

    #[test]
    fn scope_close_for_unkeyed_production() {
        let result = format_scope_close(&ScopeClose::For { is_keyed: false }, true);
        assert_eq!(result, "}), 256))");
    }

    #[test]
    fn scope_close_for_keyed_production() {
        let result = format_scope_close(&ScopeClose::For { is_keyed: true }, true);
        assert_eq!(result, "}), 128))");
    }

    #[test]
    fn scope_close_returns_static_str() {
        let result = format_scope_close(&ScopeClose::For { is_keyed: false }, false);
        assert_eq!(result, "}), 256 /* UNKEYED_FRAGMENT */))");
    }

    // ==================== format_directive_entry ====================

    #[test]
    fn directive_entry_simple() {
        let result = format_directive_entry("_vShow", "show", "", "");
        assert_eq!(result, "[_vShow, show]");
    }

    #[test]
    fn directive_entry_no_value() {
        let result = format_directive_entry("_directive_focus", "", "", "");
        assert_eq!(result, "[_directive_focus]");
    }

    #[test]
    fn directive_entry_with_arg() {
        let result = format_directive_entry("_vModelText", "msg", "\"arg\"", "");
        assert_eq!(result, "[_vModelText, msg, \"arg\"]");
    }

    #[test]
    fn directive_entry_with_modifiers() {
        let result = format_directive_entry("_vModelText", "msg", "", "{ trim: true }");
        assert_eq!(result, "[_vModelText, msg, void 0, { trim: true }]");
    }

    #[test]
    fn directive_entry_all_parts() {
        let result = format_directive_entry(
            "_vModelText",
            "msg",
            "\"name\"",
            "{ trim: true, number: true }",
        );
        assert_eq!(
            result,
            "[_vModelText, msg, \"name\", { trim: true, number: true }]"
        );
    }

    #[test]
    fn directive_entry_void_0_for_missing_value_with_arg() {
        let result = format_directive_entry("_directive_custom", "", "\"arg\"", "");
        assert_eq!(result, "[_directive_custom, void 0, \"arg\"]");
    }

    // ==================== collect_scope_imports ====================

    #[test]
    fn imports_for_if_ternary() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        collect_scope_imports(&ScopeClose::IfTernary, &mut out);
        assert!(out.vdom_imports().has(VdomHelper::CreateCommentVNode));
    }

    #[test]
    fn imports_for_for_scope() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        collect_scope_imports(&ScopeClose::For { is_keyed: false }, &mut out);
        assert!(out.vdom_imports().has(VdomHelper::OpenBlock));
        assert!(out.vdom_imports().has(VdomHelper::CreateElementBlock));
        assert!(out.vdom_imports().has(VdomHelper::Fragment));
        assert!(out.vdom_imports().has(VdomHelper::RenderList));
    }

    #[test]
    fn imports_for_else_if_empty() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        collect_scope_imports(&ScopeClose::ElseIfTernary, &mut out);
        assert!(out.vdom_imports().is_empty());
    }

    // ==================== extract_directive_value ====================

    #[test]
    fn extract_value_with_span() {
        let prop = make_directive_prop(Some(6), Some(10));
        let source = "v-if=\"show\"";
        assert_eq!(extract_directive_value(&prop, source), "show");
    }

    #[test]
    fn extract_value_no_span() {
        let prop = make_directive_prop(None, None);
        let source = "v-else";
        assert_eq!(extract_directive_value(&prop, source), "");
    }
}

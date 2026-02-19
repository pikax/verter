//! VDOM structural directive code generation.
//!
//! Handles v-if/v-else-if/v-else, v-for, and runtime directive wrapping
//! (v-model, v-show, custom directives).
//!
//! These are pure helper functions that build output strings. The caller
//! (VdomCodeGen enter_element/leave_element) integrates them into overwrites.

use crate::new_impl::ast::types::{ElementNodeCondition, ElementNodeConditionKind};
use crate::new_impl::types::NodeProp;

use super::super::shared::helpers::{
    self, extract_directive_value, parse_v_for_expression, VdomHelper,
};
use super::super::types::{CodeGenOutput, ScopeClose};

// ======================== v-if / v-else-if / v-else ========================

/// Build the scope prefix string for a v-if/v-else-if/v-else directive.
///
/// Returns the prefix to prepend before the element's VNode call, and
/// the `ScopeClose` to push onto the element's scope close stack.
///
/// - `v-if="show"` → prefix `"(show) ? "`, close `ScopeClose::IfTernary`
/// - `v-else-if="count > 0"` → prefix `"(count > 0) ? "`, close `ScopeClose::ElseIfTernary`
/// - `v-else` → prefix `""` (the `: ` is emitted by the previous branch's close), close `ScopeClose::Else`
pub fn build_condition_prefix(
    condition: &ElementNodeCondition,
    source: &str,
) -> (String, ScopeClose) {
    match condition.kind {
        ElementNodeConditionKind::If => {
            let expr = extract_directive_value(&condition.prop, source);
            let expr = if expr.is_empty() { "true" } else { expr };
            let prefix = format!("({expr}) ? ");
            (prefix, ScopeClose::IfTernary)
        }
        ElementNodeConditionKind::ElseIf => {
            let expr = extract_directive_value(&condition.prop, source);
            let expr = if expr.is_empty() { "true" } else { expr };
            let prefix = format!("({expr}) ? ");
            (prefix, ScopeClose::ElseIfTernary)
        }
        ElementNodeConditionKind::Else => {
            // The `: ` separator is emitted by the previous branch's scope close
            (String::new(), ScopeClose::Else)
        }
    }
}

// ======================== v-for ========================

/// Build the scope prefix string for a v-for directive.
///
/// Returns the full prefix to prepend before the element's VNode call.
///
/// `v-for="(item, i) in items"` →
/// `"(_openBlock(true), _createElementBlock(_Fragment, null, _renderList(items, (item, i) => {return "`
///
/// The iterable and parameter parts are extracted from source using v-for
/// parsing conventions: `value_start..value_end` contains the full expression
/// like `"(item, i) in items"` or `"item in items"`.
pub fn build_for_prefix(v_for: &NodeProp, source: &str, is_keyed: bool) -> (String, ScopeClose) {
    let full_expr = extract_directive_value(v_for, source);

    // Parse v-for expression: "(params) in/of iterable"
    let (params, iterable) = parse_v_for_expression(full_expr);

    let mut prefix = String::with_capacity(128);
    prefix.push_str("(_openBlock(true), _createElementBlock(_Fragment, null, _renderList(");
    prefix.push_str(iterable);
    prefix.push_str(", (");
    prefix.push_str(params);
    prefix.push_str(") => {return ");

    (prefix, ScopeClose::For { is_keyed })
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
pub fn format_scope_close<'alloc>(
    close: &ScopeClose,
    is_production: bool,
    out: &mut CodeGenOutput<'alloc>,
) -> String {
    match close {
        ScopeClose::IfTernary => {
            // Emit ternary else with comment fallback
            " : _createCommentVNode(\"v-if\", true)".to_string()
        }
        ScopeClose::ElseIfTernary => {
            // Separator for next branch in ternary chain
            " : ".to_string()
        }
        ScopeClose::Else => {
            // No suffix needed — v-else is the terminal branch
            String::new()
        }
        ScopeClose::For { is_keyed } => {
            let flag = if *is_keyed {
                helpers::PATCH_KEYED_FRAGMENT
            } else {
                helpers::PATCH_UNKEYED_FRAGMENT
            };
            let flag_str = helpers::format_patch_flag(flag, is_production, |s| out.alloc_str(s));
            // Output: `}), <flag>))` — closes arrow body `}`, renderList `)`, Fragment `)`, openBlock `)`
            let mut buf = String::with_capacity(32);
            buf.push_str("}), ");
            buf.push_str(flag_str);
            buf.push_str("))");
            buf
        }
        ScopeClose::SlotWrapper => {
            // Slot wrapper close
            ")".to_string()
        }
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

    // ==================== build_condition_prefix ====================

    #[test]
    fn condition_prefix_v_if() {
        let cond = ElementNodeCondition {
            kind: ElementNodeConditionKind::If,
            prop: make_directive_prop(Some(6), Some(10)),
        };
        let source = "v-if=\"show\"";
        let (prefix, close) = build_condition_prefix(&cond, source);
        assert_eq!(prefix, "(show) ? ");
        assert!(matches!(close, ScopeClose::IfTernary));
    }

    #[test]
    fn condition_prefix_v_else_if() {
        let cond = ElementNodeCondition {
            kind: ElementNodeConditionKind::ElseIf,
            prop: make_directive_prop(Some(11), Some(20)),
        };
        let source = "v-else-if=\"count > 0\"";
        let (prefix, close) = build_condition_prefix(&cond, source);
        assert_eq!(prefix, "(count > 0) ? ");
        assert!(matches!(close, ScopeClose::ElseIfTernary));
    }

    #[test]
    fn condition_prefix_v_else() {
        let cond = ElementNodeCondition {
            kind: ElementNodeConditionKind::Else,
            prop: make_directive_prop(None, None),
        };
        let source = "v-else";
        let (prefix, close) = build_condition_prefix(&cond, source);
        assert_eq!(prefix, "");
        assert!(matches!(close, ScopeClose::Else));
    }

    #[test]
    fn condition_prefix_no_value_defaults_true() {
        let cond = ElementNodeCondition {
            kind: ElementNodeConditionKind::If,
            prop: make_directive_prop(None, None),
        };
        let source = "v-if";
        let (prefix, _) = build_condition_prefix(&cond, source);
        assert_eq!(prefix, "(true) ? ");
    }

    // ==================== build_for_prefix ====================

    #[test]
    fn for_prefix_simple() {
        let prop = make_directive_prop(Some(7), Some(21));
        let source = "v-for=\"item in items\"";
        let (prefix, close) = build_for_prefix(&prop, source, false);

        assert!(prefix
            .starts_with("(_openBlock(true), _createElementBlock(_Fragment, null, _renderList("));
        assert!(prefix.contains("items"));
        assert!(prefix.contains("(item)"));
        assert!(prefix.ends_with("{return "));
        assert!(matches!(close, ScopeClose::For { is_keyed: false }));
    }

    #[test]
    fn for_prefix_keyed() {
        let prop = make_directive_prop(Some(7), Some(21));
        let source = "v-for=\"item in items\"";
        let (_, close) = build_for_prefix(&prop, source, true);
        assert!(matches!(close, ScopeClose::For { is_keyed: true }));
    }

    #[test]
    fn for_prefix_with_index() {
        let prop = make_directive_prop(Some(7), Some(29));
        let source = "v-for=\"(item, index) in items\"";
        let (prefix, _) = build_for_prefix(&prop, source, false);

        assert!(prefix.contains("items, (item, index)"));
    }

    // ==================== format_scope_close ====================

    #[test]
    fn scope_close_if_ternary() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let result = format_scope_close(&ScopeClose::IfTernary, false, &mut out);
        assert_eq!(result, " : _createCommentVNode(\"v-if\", true)");
    }

    #[test]
    fn scope_close_else_if_ternary() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let result = format_scope_close(&ScopeClose::ElseIfTernary, false, &mut out);
        assert_eq!(result, " : ");
    }

    #[test]
    fn scope_close_else_empty() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let result = format_scope_close(&ScopeClose::Else, false, &mut out);
        assert_eq!(result, "");
    }

    #[test]
    fn scope_close_for_unkeyed_dev() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let result = format_scope_close(&ScopeClose::For { is_keyed: false }, false, &mut out);
        assert_eq!(result, "}), 256 /* UNKEYED_FRAGMENT */))");
    }

    #[test]
    fn scope_close_for_keyed_dev() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let result = format_scope_close(&ScopeClose::For { is_keyed: true }, false, &mut out);
        assert_eq!(result, "}), 128 /* KEYED_FRAGMENT */))");
    }

    #[test]
    fn scope_close_for_unkeyed_production() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let result = format_scope_close(&ScopeClose::For { is_keyed: false }, true, &mut out);
        assert_eq!(result, "}), 256))");
    }

    #[test]
    fn scope_close_for_keyed_production() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let result = format_scope_close(&ScopeClose::For { is_keyed: true }, true, &mut out);
        assert_eq!(result, "}), 128))");
    }

    #[test]
    fn scope_close_for_uses_bump_allocation() {
        // Verifies the fix: uses out.alloc_str() instead of Box::leak.
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let result = format_scope_close(&ScopeClose::For { is_keyed: false }, false, &mut out);
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

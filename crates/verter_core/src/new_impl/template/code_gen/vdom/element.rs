//! VDOM element code generation.
//!
//! Handles element open/close tag transformation, props object generation,
//! children wrapping and separators, whitespace resolution, and patch flag emission.
//!
//! The main entry point is [`process_element_leave`], called from
//! `VdomCodeGen::leave_element` after all children have been visited.

use crate::new_impl::ast::types::{ChildrenMode, ElementNode};
use crate::new_impl::template::oxc::types::{ExpressionFlag, OxcParsedElement};

use super::super::shared::helpers::{self, VdomHelper};
use super::super::types::{ChildKind, ChildRecord, CodeGenOutput};
use super::super::TemplateCodeGenOptions;
use super::props;

// ======================== Whitespace resolution ========================

/// Resolve whitespace candidate children based on Vue's condense mode rules.
///
/// - Leading/trailing whitespace-only nodes → removed (overwritten to "")
/// - Interior `WhitespaceNewline` → removed
/// - Interior `WhitespaceSpace` → converted to single-space Text
pub fn resolve_whitespace<'alloc>(
    children: &mut Vec<ChildRecord>,
    out: &mut CodeGenOutput<'alloc>,
) {
    // Remove leading whitespace (drain-based, O(n) instead of O(n^2))
    let leading = children
        .iter()
        .take_while(|c| is_whitespace_kind(c.kind))
        .count();
    for removed in children.drain(..leading) {
        out.overwrite(removed.start, removed.end, "");
    }

    // Remove trailing whitespace
    while children.last().is_some_and(|c| is_whitespace_kind(c.kind)) {
        let removed = children.pop().unwrap();
        out.overwrite(removed.start, removed.end, "");
    }

    // Resolve interior whitespace
    let mut i = 0;
    while i < children.len() {
        match children[i].kind {
            ChildKind::WhitespaceNewline => {
                let removed = children.remove(i);
                out.overwrite(removed.start, removed.end, "");
            }
            ChildKind::WhitespaceSpace => {
                out.overwrite(children[i].start, children[i].end, " ");
                children[i].kind = ChildKind::Text;
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
}

#[inline]
fn is_whitespace_kind(kind: ChildKind) -> bool {
    matches!(
        kind,
        ChildKind::WhitespaceNewline | ChildKind::WhitespaceSpace
    )
}

/// Public version of [`is_whitespace_kind`] for use by `leave_template`.
#[inline]
pub fn is_whitespace_kind_pub(kind: ChildKind) -> bool {
    is_whitespace_kind(kind)
}

// ======================== Children separators ========================

/// Determine the text-concat separator between two adjacent children.
///
/// Used in `TextOnlyStatic` and `TextOnlyDynamic` modes where children
/// are joined with `+` operators.
fn text_separator(prev: ChildKind, next: ChildKind) -> &'static str {
    match (prev, next) {
        (ChildKind::Text, ChildKind::Text) => "\" + \"",
        (ChildKind::Text, ChildKind::Interpolation) => "\" + _toDisplayString",
        (ChildKind::Interpolation, ChildKind::Text) => " + \"",
        (ChildKind::Interpolation, ChildKind::Interpolation) => " + _toDisplayString",
        (ChildKind::Text, ChildKind::Comment) => "\" + ",
        (ChildKind::Comment, ChildKind::Text) => " + \"",
        (ChildKind::Interpolation, ChildKind::Comment) => " + ",
        (ChildKind::Comment, ChildKind::Interpolation) => " + _toDisplayString",
        (ChildKind::Comment, ChildKind::Comment) => " + ",
        _ => ", ",
    }
}

/// Add separator prepends between children.
///
/// For text-concat mode: `" + "`, `" + _toDisplayString`, etc.
/// For array mode: delegates to [`wrap_array_text_runs`].
fn add_children_separators<'alloc>(
    children: &[ChildRecord],
    children_mode: ChildrenMode,
    out: &mut CodeGenOutput<'alloc>,
    options: &TemplateCodeGenOptions,
) {
    if children.is_empty() {
        return;
    }

    match children_mode {
        ChildrenMode::TextOnlyStatic | ChildrenMode::TextOnlyDynamic => {
            for i in 1..children.len() {
                let sep = text_separator(children[i - 1].kind, children[i].kind);
                if !sep.is_empty() {
                    out.prepend_static(children[i].start, sep);
                }
                // Track _toDisplayString import
                if children[i].kind == ChildKind::Interpolation {
                    out.add_vdom_import(VdomHelper::ToDisplayString);
                }
            }
        }
        ChildrenMode::Mixed | ChildrenMode::MultiElement | ChildrenMode::SingleElement => {
            wrap_array_text_runs(children, out, options);
        }
        _ => {}
    }
}

/// Add array-mode separators for root-level children (used by `leave_template`).
///
/// Delegates to [`wrap_array_text_runs`] which handles text/interpolation
/// grouping into `_createTextVNode()` calls and `, ` element separators.
pub fn add_children_separators_array<'alloc>(
    children: &[ChildRecord],
    out: &mut CodeGenOutput<'alloc>,
    options: &TemplateCodeGenOptions,
) {
    if children.is_empty() {
        return;
    }
    wrap_array_text_runs(children, out, options);
}

/// Wrap text runs in array children with `_createTextVNode()` calls.
///
/// In array mode (`Mixed`/`MultiElement`), consecutive Text/Interpolation
/// children form "text runs" that must be wrapped in `_createTextVNode(...)`.
/// Element/Comment children are standalone array items separated by `, `.
///
/// **Separator positioning**: Comma separators are prepended at the *previous*
/// item's end position rather than the current item's start. This avoids
/// conflicts with v-if condition prefixes `(expr) ? ` that are prepended at
/// the element's start during `enter_element`. Without this, two prepends at
/// the same position produce `(expr) ? , _createVNode(...)` instead of the
/// correct `, (expr) ? _createVNode(...)`.
///
/// Examples:
/// - Static text: `_createTextVNode("hello")`
/// - Dynamic: `_createTextVNode(_toDisplayString(msg), 1 /* TEXT */)`
/// - Mixed: `_createTextVNode("hi " + _toDisplayString(msg), 1 /* TEXT */)`
fn wrap_array_text_runs<'alloc>(
    children: &[ChildRecord],
    out: &mut CodeGenOutput<'alloc>,
    options: &TemplateCodeGenOptions,
) {
    let mut i = 0;
    let mut is_first_item = true;
    // Track the end position of the previous array slot, used for placing
    // comma separators BEFORE the next item's condition prefix.
    let mut prev_item_end: u32 = 0;

    while i < children.len() {
        let kind = children[i].kind;

        if kind == ChildKind::Text || kind == ChildKind::Interpolation {
            // Found start of a text run — scan for the end
            let run_start = i;
            let mut has_dynamic = kind == ChildKind::Interpolation;
            i += 1;
            while i < children.len() {
                match children[i].kind {
                    ChildKind::Text | ChildKind::Interpolation => {
                        if children[i].kind == ChildKind::Interpolation {
                            has_dynamic = true;
                        }
                        i += 1;
                    }
                    _ => break,
                }
            }
            let run_end = i;

            // Comma separator at previous item's end
            if !is_first_item {
                out.prepend_static(prev_item_end, ", ");
            }

            // Build prefix: _createTextVNode( + first child content prefix
            let mut prefix = String::new();
            prefix.push_str("_createTextVNode(");
            out.add_vdom_import(VdomHelper::CreateTextVNode);

            // First child in run: add opening quote for text, _toDisplayString for interpolation
            if children[run_start].kind == ChildKind::Text {
                prefix.push('"');
            } else {
                prefix.push_str("_toDisplayString");
                out.add_vdom_import(VdomHelper::ToDisplayString);
            }
            out.prepend_alloc(children[run_start].start, &prefix);

            // Inner separators (text-concat style within the run)
            for j in (run_start + 1)..run_end {
                let sep = text_separator(children[j - 1].kind, children[j].kind);
                if !sep.is_empty() {
                    out.prepend_static(children[j].start, sep);
                }
                if children[j].kind == ChildKind::Interpolation {
                    out.add_vdom_import(VdomHelper::ToDisplayString);
                }
            }

            // Build suffix: closing quote (if text) + patch flag + close paren
            let last = &children[run_end - 1];
            let mut suffix = String::new();
            if last.kind == ChildKind::Text {
                suffix.push('"');
            }
            // Interpolation already ends with ) from interpolation.rs
            if has_dynamic {
                if options.is_production {
                    suffix.push_str(", 1");
                } else {
                    suffix.push_str(", 1 /* TEXT */");
                }
            }
            suffix.push(')'); // close _createTextVNode
            out.prepend_alloc(last.end, &suffix);

            prev_item_end = last.end;
            is_first_item = false;
        } else {
            // Element or Comment — standalone array item.
            // v-else-if / v-else children are connected to their v-if via
            // the ternary `: ` operator emitted by the scope close — they
            // do NOT get a comma separator.
            let is_continuation = children[i].condition
                == Some(super::super::types::ConditionChainRole::Continuation);
            if !is_first_item && !is_continuation {
                out.prepend_static(prev_item_end, ", ");
            }
            // Always update prev_item_end (even for continuations, since
            // the chain's "end" extends through all continuations).
            prev_item_end = children[i].end;
            i += 1;
            // A continuation doesn't start a new array slot — the chain's
            // v-if already marked is_first_item = false.
            if !is_continuation {
                is_first_item = false;
            }
        }
    }
}

// ======================== VNode construction ========================

/// Determine the helper for creating this element.
fn vnode_helper(element: &ElementNode) -> VdomHelper {
    if element.tag_type.is_component() {
        VdomHelper::CreateVNode
    } else {
        VdomHelper::CreateElementVNode
    }
}

/// Build the static props object into a buffer.
///
/// Handles static attributes only (first iteration). Dynamic props with
/// binding patches will use the multi-overwrite approach in a later iteration.
fn build_props_object_into(buf: &mut String, element: &ElementNode, source: &str) {
    // Collect spread expressions from v-bind/v-on without args.
    // These need _mergeProps() wrapping when mixed with regular props.
    let mut spreads: Vec<&str> = Vec::new();
    let mut has_regular_props = false;

    for prop in &element.props {
        if prop.is_directive {
            let directive_name = &source[prop.start as usize..prop.name_end as usize];
            let is_bind = directive_name == ":" || directive_name == "v-bind";
            let is_on = directive_name == "@" || directive_name == "v-on";

            if (is_bind || is_on) && prop.arg_start.is_none() {
                // v-bind="expr" or v-on="expr" without arg → spread
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    spreads.push(&source[vs as usize..ve as usize]);
                }
                continue;
            }

            if prop.arg_start.is_some() {
                has_regular_props = true;
            }
            // Directives with no arg and not v-bind/v-on (e.g., v-ripple, v-show)
            // are not rendered as props — they use withDirectives() wrapping.
        } else {
            has_regular_props = true;
        }
    }

    // If we have spreads and regular props, use _mergeProps
    let use_merge = !spreads.is_empty() && has_regular_props;
    if use_merge {
        buf.push_str("_mergeProps(");
    }

    // Emit regular props object
    if has_regular_props || spreads.is_empty() {
        buf.push_str("{ ");
        let mut first = true;
        for prop in &element.props {
            if prop.is_directive {
                let directive_name = &source[prop.start as usize..prop.name_end as usize];
                let is_bind = directive_name == ":" || directive_name == "v-bind";
                let is_on = directive_name == "@" || directive_name == "v-on";

                if (is_bind || is_on) && prop.arg_start.is_none() {
                    continue; // Handled as spread above
                }

                if let (Some(arg_start), Some(arg_end)) = (prop.arg_start, prop.arg_end) {
                    let arg_name = &source[arg_start as usize..arg_end as usize];

                    if !first {
                        buf.push_str(", ");
                    }
                    first = false;

                    if is_on || directive_name == "@" {
                        // Event directive: @click → onClick, @update:modelValue → "onUpdate:modelValue"
                        // Build key in temp position, then check if quoting is needed
                        let key_start = buf.len();
                        props::format_event_handler_key_into(buf, arg_name);
                        if props::needs_quoted_key(&buf[key_start..]) {
                            buf.insert(key_start, '"');
                            buf.push('"');
                        }
                    } else {
                        let camelized = props::camelize(arg_name);
                        if props::needs_quoted_key(&camelized) {
                            buf.push('"');
                            buf.push_str(&camelized);
                            buf.push('"');
                        } else {
                            buf.push_str(&camelized);
                        }
                    }
                } else {
                    // Directive with no arg (v-ripple, v-show, etc.) — skip in props
                    continue;
                }
            } else {
                // Static attribute
                if !first {
                    buf.push_str(", ");
                }
                first = false;

                let name = &source[prop.start as usize..prop.name_end as usize];
                if props::needs_quoted_key(name) {
                    buf.push('"');
                    buf.push_str(name);
                    buf.push('"');
                } else {
                    buf.push_str(name);
                }
            }

            buf.push_str(": ");

            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let value = &source[vs as usize..ve as usize];
                if prop.is_directive {
                    buf.push_str(value);
                } else {
                    buf.push('"');
                    helpers::escape_js_string_into(buf, value);
                    buf.push('"');
                }
            } else {
                // Boolean attribute (no value)
                buf.push_str("\"\"");
            }
        }
        buf.push_str(" }");
    }

    // Emit spread expressions
    for spread in &spreads {
        if has_regular_props || use_merge {
            buf.push_str(", ");
        }
        buf.push_str(spread);
    }

    if use_merge {
        buf.push(')');
    } else if !spreads.is_empty() && !has_regular_props {
        // Only spreads, no regular props — just emit the first spread
        // (if multiple, use _mergeProps)
        if spreads.len() > 1 {
            // Rewrite: wrap all spreads in _mergeProps
            // This case is unlikely but handle it properly
            let content =
                buf.split_off(buf.len() - spreads.iter().map(|s| s.len() + 2).sum::<usize>() + 2);
            let _ = content;
            buf.push_str("_mergeProps(");
            for (i, spread) in spreads.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                buf.push_str(spread);
            }
            buf.push(')');
        }
    }
}

/// Process the leave phase of a VDOM element node.
///
/// 1. Resolves whitespace in children
/// 2. Constructs VNode call via overwrites
/// 3. Handles props, children, patch flags
/// 4. Returns a ChildRecord for the parent's children list
pub fn process_element_leave<'alloc>(
    element: &ElementNode,
    oxc: Option<&OxcParsedElement<'alloc>>,
    children: &mut Vec<ChildRecord>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    options: &TemplateCodeGenOptions,
    buf: &mut String,
) -> ChildRecord {
    let tag_open = &element.tag_open;
    debug_assert!((tag_open.start as usize + 1) <= source.len());
    debug_assert!((tag_open.name_end as usize) <= source.len());
    let tag_name = &source[tag_open.start as usize + 1..tag_open.name_end as usize];
    let helper = vnode_helper(element);

    // Step 1: Resolve whitespace
    resolve_whitespace(children, out);

    let has_props = !element.props.is_empty();
    let has_children = !children.is_empty();

    // Step 2: Compute patch flags
    let expr_flag = oxc
        .map(|o| o.expression_flag)
        .unwrap_or(ExpressionFlag::empty());
    let patch_flag = props::compute_patch_flags(
        element.prop_flag,
        expr_flag,
        element.children_mode,
        false, // TODO: detect other dynamic binds during prop iteration
    );

    // Step 3: Build open tag overwrite (reusing caller's buffer)
    buf.clear();
    buf.push_str(helper.name());
    buf.push_str("(\"");
    buf.push_str(tag_name);
    buf.push('"');

    // Add import for the helper
    out.add_vdom_import(helper);

    // Props
    if has_props {
        buf.push_str(", ");
        build_props_object_into(buf, element, source);
    } else if has_children || patch_flag != 0 {
        // Need null placeholder for props when there are children or patch flags
        buf.push_str(", null");
    }

    // Children opening
    if has_children {
        buf.push_str(", ");
        match element.children_mode {
            ChildrenMode::TextOnlyStatic | ChildrenMode::TextOnlyDynamic => {
                // First child prefix
                if children[0].kind == ChildKind::Text {
                    buf.push('"');
                } else if children[0].kind == ChildKind::Interpolation {
                    buf.push_str("_toDisplayString");
                    out.add_vdom_import(VdomHelper::ToDisplayString);
                }
            }
            ChildrenMode::Mixed | ChildrenMode::MultiElement => {
                buf.push('[');
            }
            _ => {}
        }
    }

    // Self-closing or no close tag: single overwrite for entire tag
    if element.is_self_closing || element.tag_close.is_none() {
        buf.push(')');
        out.overwrite(tag_open.start, tag_open.end, buf);

        return ChildRecord {
            start: tag_open.start,
            end: tag_open.end,
            kind: ChildKind::Element,
            condition: None,
            condition_prefix: None,
        };
    }

    // Overwrite open tag
    out.overwrite(tag_open.start, tag_open.end, buf);

    // Step 4: Add children separators (and text run wrapping for array modes)
    add_children_separators(children, element.children_mode, out, options);

    // Step 5: Build close tag overwrite (reuse buffer)
    buf.clear();
    let tag_close = element.tag_close.as_ref().unwrap();

    // Children closing
    if has_children {
        match element.children_mode {
            ChildrenMode::TextOnlyStatic | ChildrenMode::TextOnlyDynamic => {
                // Last child suffix
                let last_kind = children.last().map(|c| c.kind).unwrap_or(ChildKind::Text);
                if last_kind == ChildKind::Text {
                    buf.push('"');
                }
                // Interpolation already ends with ) from interpolation.rs
            }
            ChildrenMode::Mixed | ChildrenMode::MultiElement => {
                buf.push(']');
            }
            _ => {}
        }
    }

    // Patch flag (only if non-zero)
    if patch_flag != 0 {
        buf.push_str(", ");
        let flag_str =
            helpers::format_patch_flag(patch_flag, options.is_production, |s| out.alloc_str(s));
        buf.push_str(flag_str);
    }

    buf.push(')');
    out.overwrite(tag_close.start, tag_close.end, buf);

    // Return child record for the parent
    ChildRecord {
        start: tag_open.start,
        end: tag_close.end,
        kind: ChildKind::Element,
        condition: None,
        condition_prefix: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::new_impl::ast::types::*;
    use crate::new_impl::types::{NodeProp, NodeTag};
    use oxc_allocator::Allocator;
    use smallvec::SmallVec;

    fn make_tag(start: u32, end: u32, name_end: u32) -> NodeTag {
        NodeTag {
            start,
            end,
            name_end,
        }
    }

    fn make_element(
        tag_open: NodeTag,
        tag_close: Option<NodeTag>,
        content: Option<ElementContent>,
    ) -> ElementNode {
        let is_self_closing = tag_close.is_none();
        let children_flag = ChildrenFlag::empty();
        ElementNode {
            tag_open,
            tag_close,
            tag_type: TagType::Element,
            is_self_closing,
            props: Vec::new(),
            content,
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag,
            children_mode: children_flag.mode(),
        }
    }

    fn make_options() -> TemplateCodeGenOptions {
        TemplateCodeGenOptions {
            is_production: false,
            ..Default::default()
        }
    }

    /// Apply CodeGenOutput to source and return the result string.
    fn apply_output<'a>(source: &str, out: CodeGenOutput<'a>, alloc: &'a Allocator) -> String {
        let mut ct = crate::code_transform::CodeTransform::new(source, alloc);
        out.apply_to(&mut ct);
        ct.build_string()
    }

    // ==================== Whitespace resolution ====================

    #[test]
    fn resolve_ws_removes_leading_newline() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut children = vec![
            ChildRecord {
                start: 0,
                end: 3,
                kind: ChildKind::WhitespaceNewline,
                condition: None,
            },
            ChildRecord {
                start: 3,
                end: 8,
                kind: ChildKind::Text,
                condition: None,
            },
        ];

        resolve_whitespace(&mut children, &mut out);

        assert_eq!(children.len(), 1);
        assert_eq!(children[0].kind, ChildKind::Text);
        assert_eq!(out.overwrites.len(), 1);
        assert_eq!(out.overwrites[0], (0, 3, ""));
    }

    #[test]
    fn resolve_ws_removes_trailing_newline() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut children = vec![
            ChildRecord {
                start: 0,
                end: 5,
                kind: ChildKind::Text,
                condition: None,
            },
            ChildRecord {
                start: 5,
                end: 8,
                kind: ChildKind::WhitespaceNewline,
                condition: None,
            },
        ];

        resolve_whitespace(&mut children, &mut out);

        assert_eq!(children.len(), 1);
        assert_eq!(children[0].kind, ChildKind::Text);
        assert_eq!(out.overwrites[0], (5, 8, ""));
    }

    #[test]
    fn resolve_ws_removes_leading_space() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut children = vec![
            ChildRecord {
                start: 0,
                end: 2,
                kind: ChildKind::WhitespaceSpace,
                condition: None,
            },
            ChildRecord {
                start: 2,
                end: 7,
                kind: ChildKind::Text,
                condition: None,
            },
        ];

        resolve_whitespace(&mut children, &mut out);

        assert_eq!(children.len(), 1);
        assert_eq!(out.overwrites[0], (0, 2, ""));
    }

    #[test]
    fn resolve_ws_interior_newline_removed() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut children = vec![
            ChildRecord {
                start: 0,
                end: 5,
                kind: ChildKind::Element,
                condition: None,
            },
            ChildRecord {
                start: 5,
                end: 8,
                kind: ChildKind::WhitespaceNewline,
                condition: None,
            },
            ChildRecord {
                start: 8,
                end: 13,
                kind: ChildKind::Element,
                condition: None,
            },
        ];

        resolve_whitespace(&mut children, &mut out);

        assert_eq!(children.len(), 2);
        assert_eq!(children[0].kind, ChildKind::Element);
        assert_eq!(children[1].kind, ChildKind::Element);
        assert_eq!(out.overwrites[0], (5, 8, ""));
    }

    #[test]
    fn resolve_ws_interior_space_becomes_text() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut children = vec![
            ChildRecord {
                start: 0,
                end: 5,
                kind: ChildKind::Element,
                condition: None,
            },
            ChildRecord {
                start: 5,
                end: 6,
                kind: ChildKind::WhitespaceSpace,
                condition: None,
            },
            ChildRecord {
                start: 6,
                end: 11,
                kind: ChildKind::Element,
                condition: None,
            },
        ];

        resolve_whitespace(&mut children, &mut out);

        assert_eq!(children.len(), 3);
        assert_eq!(children[1].kind, ChildKind::Text);
        assert_eq!(out.overwrites[0], (5, 6, " "));
    }

    #[test]
    fn resolve_ws_all_whitespace_removed() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut children = vec![
            ChildRecord {
                start: 0,
                end: 3,
                kind: ChildKind::WhitespaceNewline,
                condition: None,
            },
            ChildRecord {
                start: 3,
                end: 5,
                kind: ChildKind::WhitespaceSpace,
                condition: None,
            },
        ];

        resolve_whitespace(&mut children, &mut out);

        assert!(children.is_empty());
        assert_eq!(out.overwrites.len(), 2);
    }

    #[test]
    fn resolve_ws_no_whitespace_unchanged() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut children = vec![
            ChildRecord {
                start: 0,
                end: 5,
                kind: ChildKind::Text,
                condition: None,
            },
            ChildRecord {
                start: 5,
                end: 10,
                kind: ChildKind::Interpolation,
                condition: None,
            },
        ];

        resolve_whitespace(&mut children, &mut out);

        assert_eq!(children.len(), 2);
        assert!(out.overwrites.is_empty());
    }

    // ==================== Self-closing element ====================

    #[test]
    fn self_closing_no_props() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "<br />";
        let element = make_element(make_tag(0, 6, 3), None, None);
        let options = make_options();
        let mut children = Vec::new();

        let mut buf = String::new();
        let record = process_element_leave(
            &element,
            None,
            &mut children,
            source,
            &mut out,
            &options,
            &mut buf,
        );

        assert_eq!(record.kind, ChildKind::Element);
        assert_eq!(record.start, 0);
        assert_eq!(record.end, 6);

        let result = apply_output(source, out, &alloc);
        assert_eq!(result, "_createElementVNode(\"br\")");
    }

    // ==================== Empty element ====================

    #[test]
    fn empty_element_no_props() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "<div></div>";
        let element = make_element(
            make_tag(0, 5, 4),
            Some(make_tag(5, 11, 10)),
            Some(ElementContent {
                start: 5,
                end: 5,
                children: SmallVec::new(),
            }),
        );
        let options = make_options();
        let mut children = Vec::new();

        let mut buf = String::new();
        let record = process_element_leave(
            &element,
            None,
            &mut children,
            source,
            &mut out,
            &options,
            &mut buf,
        );

        assert_eq!(record.kind, ChildKind::Element);
        let result = apply_output(source, out, &alloc);
        assert_eq!(result, "_createElementVNode(\"div\")");
    }

    // ==================== Text-only static children ====================

    #[test]
    fn element_with_static_text() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "<div>hello</div>";
        let mut element = make_element(
            make_tag(0, 5, 4),
            Some(make_tag(10, 16, 15)),
            Some(ElementContent {
                start: 5,
                end: 10,
                children: SmallVec::new(),
            }),
        );
        element.children_mode = ChildrenMode::TextOnlyStatic;
        let options = make_options();
        let mut children = vec![ChildRecord {
            start: 5,
            end: 10,
            kind: ChildKind::Text,
            condition: None,
        }];

        let mut buf = String::new();
        process_element_leave(
            &element,
            None,
            &mut children,
            source,
            &mut out,
            &options,
            &mut buf,
        );

        let result = apply_output(source, out, &alloc);
        assert_eq!(result, "_createElementVNode(\"div\", null, \"hello\")");
    }

    // ==================== Static props ====================

    #[test]
    fn element_with_static_class() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        // <div class="foo"></div>
        let source = "<div class=\"foo\"></div>";
        let mut element = make_element(
            make_tag(0, 17, 4),
            Some(make_tag(17, 23, 22)),
            Some(ElementContent {
                start: 17,
                end: 17,
                children: SmallVec::new(),
            }),
        );
        element.props.push(NodeProp {
            start: 5,
            name_end: 10,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: Some(12),
            value_end: Some(15),
            modifiers: SmallVec::new(),
        });
        element.prop_flag = PropFlag::empty().add(PropFlags::HasStaticClass);
        let options = make_options();
        let mut children = Vec::new();

        let mut buf = String::new();
        process_element_leave(
            &element,
            None,
            &mut children,
            source,
            &mut out,
            &options,
            &mut buf,
        );

        let result = apply_output(source, out, &alloc);
        assert_eq!(result, "_createElementVNode(\"div\", { class: \"foo\" })");
    }

    #[test]
    fn element_with_multiple_static_props() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        // <div class="foo" id="bar"></div>
        let source = "<div class=\"foo\" id=\"bar\"></div>";
        let mut element = make_element(
            make_tag(0, 26, 4),
            Some(make_tag(26, 32, 31)),
            Some(ElementContent {
                start: 26,
                end: 26,
                children: SmallVec::new(),
            }),
        );
        element.props.push(NodeProp {
            start: 5,
            name_end: 10,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: Some(12),
            value_end: Some(15),
            modifiers: SmallVec::new(),
        });
        element.props.push(NodeProp {
            start: 17,
            name_end: 19,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: Some(21),
            value_end: Some(24),
            modifiers: SmallVec::new(),
        });
        let options = make_options();
        let mut children = Vec::new();

        let mut buf = String::new();
        process_element_leave(
            &element,
            None,
            &mut children,
            source,
            &mut out,
            &options,
            &mut buf,
        );

        let result = apply_output(source, out, &alloc);
        assert_eq!(
            result,
            "_createElementVNode(\"div\", { class: \"foo\", id: \"bar\" })"
        );
    }

    #[test]
    fn element_with_props_and_text_child() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        // <div class="foo">hello</div>
        let source = "<div class=\"foo\">hello</div>";
        let mut element = make_element(
            make_tag(0, 17, 4),
            Some(make_tag(22, 28, 27)),
            Some(ElementContent {
                start: 17,
                end: 22,
                children: SmallVec::new(),
            }),
        );
        element.props.push(NodeProp {
            start: 5,
            name_end: 10,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: Some(12),
            value_end: Some(15),
            modifiers: SmallVec::new(),
        });
        element.prop_flag = PropFlag::empty().add(PropFlags::HasStaticClass);
        element.children_mode = ChildrenMode::TextOnlyStatic;
        let options = make_options();
        let mut children = vec![ChildRecord {
            start: 17,
            end: 22,
            kind: ChildKind::Text,
            condition: None,
        }];

        let mut buf = String::new();
        process_element_leave(
            &element,
            None,
            &mut children,
            source,
            &mut out,
            &options,
            &mut buf,
        );

        let result = apply_output(source, out, &alloc);
        assert_eq!(
            result,
            "_createElementVNode(\"div\", { class: \"foo\" }, \"hello\")"
        );
    }

    // ==================== Boolean attribute ====================

    #[test]
    fn element_with_boolean_attr() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        // <input disabled />
        let source = "<input disabled />";
        let mut element = make_element(make_tag(0, 18, 6), None, None);
        element.props.push(NodeProp {
            start: 7,
            name_end: 15,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: None,
            value_end: None,
            modifiers: SmallVec::new(),
        });
        let options = make_options();
        let mut children = Vec::new();

        let mut buf = String::new();
        process_element_leave(
            &element,
            None,
            &mut children,
            source,
            &mut out,
            &options,
            &mut buf,
        );

        let result = apply_output(source, out, &alloc);
        assert_eq!(result, "_createElementVNode(\"input\", { disabled: \"\" })");
    }

    // ==================== Event handler key transformation ====================

    #[test]
    fn element_with_click_handler() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        // <div @click="handler"></div>
        let source = r#"<div @click="handler"></div>"#;
        let mut element = make_element(
            make_tag(0, 21, 4),
            Some(make_tag(21, 27, 26)),
            Some(ElementContent {
                start: 21,
                end: 21,
                children: SmallVec::new(),
            }),
        );
        element.props.push(NodeProp {
            start: 5,
            name_end: 6, // "@"
            is_directive: true,
            arg_start: Some(6),
            arg_end: Some(11), // "click"
            is_dynamic: None,
            value_start: Some(13),
            value_end: Some(20), // "handler"
            modifiers: SmallVec::new(),
        });
        element.prop_flag = PropFlag::empty().add(PropFlags::HasEventListener);
        let options = make_options();
        let mut children = Vec::new();

        let mut buf = String::new();
        process_element_leave(
            &element,
            None,
            &mut children,
            source,
            &mut out,
            &options,
            &mut buf,
        );

        let result = apply_output(source, out, &alloc);
        assert!(
            result.contains("onClick"),
            "Expected onClick key, got: {}",
            result
        );
        assert!(result.contains("handler"));
    }

    // ==================== Whitespace + children ====================

    #[test]
    fn element_with_leading_trailing_whitespace_removed() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        // <div>\n  hello\n  </div>
        let source = "<div>\n  hello\n  </div>";
        let mut element = make_element(
            make_tag(0, 5, 4),
            Some(make_tag(16, 22, 21)),
            Some(ElementContent {
                start: 5,
                end: 16,
                children: SmallVec::new(),
            }),
        );
        element.children_mode = ChildrenMode::TextOnlyStatic;
        let options = make_options();
        let mut children = vec![
            ChildRecord {
                start: 5,
                end: 8,
                kind: ChildKind::WhitespaceNewline,
                condition: None,
            },
            ChildRecord {
                start: 8,
                end: 13,
                kind: ChildKind::Text,
                condition: None,
            },
            ChildRecord {
                start: 13,
                end: 16,
                kind: ChildKind::WhitespaceNewline,
                condition: None,
            },
        ];

        let mut buf = String::new();
        process_element_leave(
            &element,
            None,
            &mut children,
            source,
            &mut out,
            &options,
            &mut buf,
        );

        let result = apply_output(source, out, &alloc);
        assert_eq!(result, "_createElementVNode(\"div\", null, \"hello\")");
    }

    // ==================== text_separator ====================

    #[test]
    fn text_sep_text_text() {
        assert_eq!(text_separator(ChildKind::Text, ChildKind::Text), "\" + \"");
    }

    #[test]
    fn text_sep_text_interpolation() {
        assert_eq!(
            text_separator(ChildKind::Text, ChildKind::Interpolation),
            "\" + _toDisplayString"
        );
    }

    #[test]
    fn text_sep_interpolation_text() {
        assert_eq!(
            text_separator(ChildKind::Interpolation, ChildKind::Text),
            " + \""
        );
    }

    #[test]
    fn text_sep_interpolation_interpolation() {
        assert_eq!(
            text_separator(ChildKind::Interpolation, ChildKind::Interpolation),
            " + _toDisplayString"
        );
    }
}

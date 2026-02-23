//! Children separator and text-run wrapping for VDOM code generation.
//!
//! Extracted from `element.rs`: text-concat separator logic, array-mode
//! separators, and `_createTextVNode()` wrapping for text runs.

use crate::ast::types::ChildrenMode;

use super::super::shared::helpers::VdomHelper;
use super::super::types::{ChildKind, ChildRecord, CodeGenOutput, ConditionChainRole};
use super::super::TemplateCodeGenOptions;

/// Determine the text-concat separator between two adjacent children.
///
/// Used in `TextOnlyStatic` and `TextOnlyDynamic` modes where children
/// are joined with `+` operators.
pub(super) fn text_separator(prev: ChildKind, next: ChildKind) -> &'static str {
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
pub(super) fn add_children_separators<'alloc>(
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
pub(super) fn add_children_separators_array<'alloc>(
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
pub(super) fn wrap_array_text_runs<'alloc>(
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
            // Found start of a text run -- scan for the end
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
            // Element or Comment -- standalone array item.
            // v-else-if / v-else children are connected to their v-if via
            // the ternary `: ` operator emitted by the scope close -- they
            // do NOT get a comma separator.
            let is_continuation = children[i].condition == Some(ConditionChainRole::Continuation);
            let needs_comma = !is_first_item && !is_continuation;
            let has_prefix = children[i].condition_prefix.is_some();

            if needs_comma && has_prefix {
                // Combined comma + condition prefix as a single prepend at
                // child.start. This ensures correct ordering: `, (expr) ? `.
                // Safe because v-if and v-for can't coexist on the same element,
                // so there's no conflicting v-for prefix at child.start.
                let mut sep =
                    String::with_capacity(4 + children[i].condition_prefix.as_ref().unwrap().len());
                sep.push_str(", ");
                sep.push_str(children[i].condition_prefix.as_ref().unwrap());
                out.prepend_alloc(children[i].start, &sep);
            } else if needs_comma {
                // Plain comma at prev_item_end to avoid conflicts with v-for
                // prefixes that are prepended at child.start by enter_element.
                out.prepend_static(prev_item_end, ", ");
            } else if has_prefix {
                // Just condition prefix, no comma (e.g., first child with v-if).
                out.prepend_alloc(
                    children[i].start,
                    children[i].condition_prefix.as_ref().unwrap(),
                );
            }

            // Always update prev_item_end (even for continuations, since
            // the chain's "end" extends through all continuations).
            prev_item_end = children[i].end;
            i += 1;
            // A continuation doesn't start a new array slot -- the chain's
            // v-if already marked is_first_item = false.
            if !is_continuation {
                is_first_item = false;
            }
        }
    }
}

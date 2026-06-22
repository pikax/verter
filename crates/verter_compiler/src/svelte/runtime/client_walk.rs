//! DOM-walk descent geometry helpers for the Svelte client emitter.
//!
//! The chained walk reaches each dynamic DOM position from a [`WalkBase`] — a
//! single-element clone-root descends into its clone var via `$.child`; a multi-root
//! fragment reaches the first dynamic position via `$.first_child`, then each
//! subsequent position via `$.sibling(prev, delta)` (delta omitted when 1). These
//! free functions compute the descent expressions and the per-position
//! name-needed / `$.remove_input_defaults` decisions from the typed IR — never a
//! source scan.

use super::ir::{AttrIr, IrNode, NodeId, SvelteRuntimeIr};
use super::whitespace::CleanItem;

/// The base a walk descent starts from.
#[derive(Debug, Clone, Copy)]
pub(super) enum WalkBase<'n> {
    /// The cloned multi-root fragment (descend via `$.first_child`).
    Fragment(&'n str),
    /// A named element (descend into its children via `$.child`).
    Element(&'n str),
}

/// The first descent expression from a base to cleaned-sequence position `idx`.
///
/// When the descended-to position is a pure-interp TEXT node (`is_text`), the
/// official trailing `true` boolean is emitted on the helper that LANDS on the
/// text node — `$.child(node, true)` / `$.first_child(frag, true)`.
///
/// The `$.sibling` step that advances PAST the first child applies the same
/// offset-omission rule [`sibling_descent`] does: `$.sibling(node, 1)` collapses to
/// `$.sibling(node)` (the `count` default is `1`) — UNLESS `is_text`, which forces
/// the explicit offset so the trailing `true` boolean stays positioned (the
/// oracle's `$.sibling($.child(div), 1, true)` form). A higher offset stays
/// explicit.
pub(super) fn first_descent(base: WalkBase, idx: usize, is_text: bool) -> String {
    let text_arg = if is_text { ", true" } else { "" };
    match base {
        WalkBase::Fragment(name) => {
            if idx == 0 {
                format!("$.first_child({name}{text_arg})")
            } else {
                // `$.sibling($.first_child(fragment)[, idx][, true])` — descend then
                // advance; the offset is omitted at `idx == 1` (non-text), explicit
                // otherwise (or when the text flag must trail it).
                let inner = format!("$.first_child({name})");
                sibling_descent(&inner, idx, is_text)
            }
        }
        WalkBase::Element(name) => {
            if idx == 0 {
                format!("$.child({name}{text_arg})")
            } else {
                let inner = format!("$.child({name})");
                sibling_descent(&inner, idx, is_text)
            }
        }
    }
}

/// A `$.sibling(prev[, delta][, true])` descent (delta omitted when 1 UNLESS the
/// landed-on node is a pure-interp text — official forces the explicit offset when
/// `is_text`, e.g. `$.sibling(prev, 1, true)`).
pub(super) fn sibling_descent(prev: &str, delta: usize, is_text: bool) -> String {
    if is_text {
        // `is_text` forces the explicit offset, then the trailing `true`.
        format!("$.sibling({prev}, {delta}, true)")
    } else if delta == 1 {
        format!("$.sibling({prev})")
    } else {
        format!("$.sibling({prev}, {delta})")
    }
}

/// Whether a cleaned DOM position needs a named walk var (it is dynamic, or hosts
/// a dynamic descendant).
pub(super) fn item_needs_name(ir: &SvelteRuntimeIr, item: &CleanItem) -> bool {
    match item {
        CleanItem::TextRun { interps, .. } => !interps.is_empty(),
        CleanItem::Node(node) => node_or_descendant_dynamic(ir, *node),
    }
}

/// Whether any cleaned position in the sequence needs a named walk var (so a
/// `$.reset(parent)` is emitted after the parent's children).
pub(super) fn any_item_needs_name(ir: &SvelteRuntimeIr, items: &[CleanItem]) -> bool {
    items.iter().any(|item| item_needs_name(ir, item))
}

/// Whether a node is dynamic or hosts a dynamic descendant.
pub(super) fn node_or_descendant_dynamic(ir: &SvelteRuntimeIr, node_id: NodeId) -> bool {
    match ir.node(node_id) {
        IrNode::Interpolation { .. } => true,
        IrNode::Element(el) => {
            el.attrs.iter().any(super::html::attr_is_dynamic_surface)
                || el
                    .children
                    .iter()
                    .any(|&c| node_or_descendant_dynamic(ir, c))
        }
        _ => false,
    }
}

/// Whether an `<input>` element needs `$.remove_input_defaults` — the official
/// `RegularElement.js` rule: an `<input>` with a `value` / `checked` / `group`
/// binding (or `files`) and NO static `defaultValue` / `defaultChecked`
/// attribute. (The non-spread `bind:value` branch is handled; the rule keys on
/// the typed `AttrIr`, never a source scan.)
pub(super) fn input_needs_remove_defaults(el: &super::ir::ElementIr) -> bool {
    if el.tag != "input" {
        return false;
    }
    let has_value_bind = el.attrs.iter().any(|a| {
        matches!(a, AttrIr::Bind { target, .. }
            if matches!(target.as_str(), "value" | "checked" | "group" | "files"))
    });
    if !has_value_bind {
        return false;
    }
    // A static `defaultValue` / `defaultChecked` attribute suppresses the helper
    // (the default is set explicitly).
    let has_static_default = el.attrs.iter().any(|a| {
        matches!(a, AttrIr::Static { name, .. }
            if matches!(name.as_str(), "defaultValue" | "defaultChecked"))
    });
    !has_static_default
}

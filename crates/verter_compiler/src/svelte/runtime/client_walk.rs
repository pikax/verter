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
        // A `{@html}` raw-markup tag is a dynamic node the DOM walk must reach (its
        // `$.html` op operates on a `<!>` anchor var, or — when it is the sole controlled
        // child — on the parent element followed by `$.reset(parent)`).
        IrNode::Tag(super::ir::TagIr::Html { .. }) => true,
        // A `{@debug}` tag emits a reactive `$.template_effect` at its DOCUMENT position,
        // so an element hosting one must be NAMED + walked (its effect is interleaved into
        // the child walk). The debug occupies no DOM position itself (it is dropped from the
        // clean sequence — never a `$.reset`-triggering named child), so an element whose
        // ONLY dynamic descendant is a `{@debug}` is named without a reset.
        IrNode::Tag(super::ir::TagIr::Debug { .. }) => true,
        // A control-flow block (`{#if}`/`{#each}`/`{#await}`/`{#key}`) is a dynamic node the
        // walk must reach — its `<!>` anchor var hosts the `$.if`/`$.each`/`$.await`/`$.key`
        // call. A `{#snippet}` is a non-rendering DECLARATION (refused upstream / dropped
        // from the clean sequence), so it never reaches here.
        IrNode::Block(block) => !matches!(block, super::ir::BlockIr::Snippet { .. }),
        // A `{@render}` tag is a dynamic node the walk must reach — its `<!>` anchor var
        // hosts the static snippet call / `$.snippet`. (`{@attach}` stays refused — the
        // attachment-directive surface is not yet supported.)
        IrNode::Tag(super::ir::TagIr::Render { .. }) => true,
        // A component invocation (`<Foo>` / `<svelte:component>` / `<svelte:self>` /
        // `<svelte:fragment>`) is a dynamic node — its `<!>` anchor var hosts the
        // `Child(node, …)` call.
        IrNode::Component(_) => true,
        IrNode::Special(s) => matches!(
            s.kind,
            super::ir::SpecialKind::Component
                | super::ir::SpecialKind::SelfRef
                | super::ir::SpecialKind::Fragment
        ),
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

/// The per-host bind PRELUDE cleanup an element emits before its `$.bind_*` call —
/// the official `RegularElement.js` default-clearing statement, decided DATA-DRIVEN
/// from the bind's runtime routing (the shared bind contract), never a hand-rolled
/// per-name list.
///
/// An `<input>` carrying a `value`/`checked`/`group` bind emits
/// `$.remove_input_defaults` (the routing's [`BindPrelude::RemoveInputDefaults`]); a
/// `<textarea bind:value>` emits `$.remove_textarea_child` (the routing's
/// [`BindPrelude::RemoveTextareaChild`]). A static `defaultValue` / `defaultChecked`
/// attribute on an `<input>` suppresses the helper (the default is set explicitly).
/// Returns `None` when the element has no bind whose routing carries a prelude.
///
/// The decision keys on the typed `AttrIr` bind directives + the shared routing,
/// never a source scan.
pub(super) fn bind_host_prelude(
    el: &super::ir::ElementIr,
) -> Option<crate::svelte::bind_contract::BindPrelude> {
    use crate::svelte::bind_contract::{resolve_runtime_bind, BindPrelude};
    // A static `defaultValue` / `defaultChecked` attribute on the element suppresses
    // the input-defaults helper (the default is set explicitly in the skeleton).
    let has_static_default = el.attrs.iter().any(|a| {
        matches!(a, AttrIr::Static { name, .. }
            if matches!(name.as_str(), "defaultValue" | "defaultChecked"))
    });
    for attr in &el.attrs {
        let AttrIr::Bind { target, .. } = attr else {
            continue;
        };
        let Some(routing) = resolve_runtime_bind(target, &el.tag) else {
            continue;
        };
        match routing.prelude {
            BindPrelude::None => continue,
            BindPrelude::RemoveInputDefaults if has_static_default => continue,
            prelude => return Some(prelude),
        }
    }
    None
}

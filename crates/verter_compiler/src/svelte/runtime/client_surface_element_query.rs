//! Standalone structural element-query helpers for the default-deny client syntax
//! classifier, extracted from `client_surface.rs` to keep it under the file-size
//! guard.
//!
//! These are the small pure structural queries the classifier runs over the typed IR:
//! namespace derivation ([`element_own_namespace`]) and the attribute / directive
//! presence predicates ([`element_carries_is_attribute`], [`element_has_spread`],
//! [`element_has_class_directive`], [`element_has_group_bind`],
//! [`element_has_style_directive`]). Each is driven from the typed `AttrIr` / `IrNode`
//! inventory — never a raw-source scan — and has no side effects: a presence query
//! returns a `bool`, the namespace query returns the element's own [`Namespace`]. They
//! feed the classifier's accept / refuse and attribute-strategy decisions.

use super::ir::{AttrIr, IrNode, NodeId, SvelteRuntimeIr};
use super::whitespace::Namespace;

/// The DOM namespace an element renders in, given the namespace inherited from its
/// parent. An element ALREADY inside an SVG / MathML subtree (`inherited != Html`)
/// stays in that namespace (so an svg `<a>` / `<title>` is SVG); at the HTML level,
/// only `<svg>` / `<math>` introduce a non-HTML namespace. The overlapping
/// `SVG_ELEMENTS` / `MATHML_ELEMENTS` names (`a` / `script` / `title` / `style`) are
/// NOT namespace introducers at the HTML root — matching the official namespace
/// classification (a root `<a>` stays HTML; a root `<svg>` is SVG; an `<a>` inside
/// `<svg>` is SVG by inheritance).
pub(super) fn element_own_namespace(inherited: Namespace, tag: &str) -> Namespace {
    if inherited != Namespace::Html {
        return inherited;
    }
    match tag {
        "svg" => Namespace::Svg,
        "math" => Namespace::Mathml,
        _ => Namespace::Html,
    }
}

/// Whether an element carries an `is` attribute (in ANY attribute form — static,
/// dynamic, or mixed). An `is=` element is a customized built-in (the web-components
/// surface); it is rejected at the custom-element owner BEFORE the attr walk,
/// regardless of whether its tag is hyphenated. Driven from the typed `AttrIr`
/// inventory, never a source scan.
pub(super) fn element_carries_is_attribute(el: &super::ir::ElementIr) -> bool {
    el.attrs.iter().any(|a| match a {
        AttrIr::Static { name, .. } | AttrIr::Dynamic { name, .. } | AttrIr::Mixed { name, .. } => {
            name == "is"
        }
        _ => false,
    })
}

/// Whether an element carries any spread attribute (`{...x}`) — the trigger that
/// switches its WHOLE attribute strategy to the single `$.attribute_effect` fold.
pub(super) fn element_has_spread(el: &super::ir::ElementIr) -> bool {
    el.attrs.iter().any(|a| matches!(a, AttrIr::Spread { .. }))
}

/// Whether the element at `node_id` carries any `class:` directive (so a static
/// `class` on it is the base value of the merged `$.set_class`, not a baked attr).
pub(super) fn element_has_class_directive(ir: &SvelteRuntimeIr, node_id: NodeId) -> bool {
    matches!(ir.node(node_id), IrNode::Element(el)
        if el.attrs.iter().any(|a| matches!(a, AttrIr::Class { .. })))
}

/// Whether the element carries a `bind:group` directive — the trigger that turns a
/// co-located static `value="X"` into the per-input `input.value = input.__value =
/// 'X'` group-value write (rather than a baked static attr).
pub(super) fn element_has_group_bind(ir: &SvelteRuntimeIr, node_id: NodeId) -> bool {
    matches!(ir.node(node_id), IrNode::Element(el)
        if el.attrs.iter().any(|a| matches!(a, AttrIr::Bind { target, .. } if target == "group")))
}

/// Whether the element at `node_id` carries any `style:` directive (so a static
/// `style` on it is the base value of the merged `$.set_style`, not a baked attr).
pub(super) fn element_has_style_directive(ir: &SvelteRuntimeIr, node_id: NodeId) -> bool {
    matches!(ir.node(node_id), IrNode::Element(el)
        if el.attrs.iter().any(|a| matches!(a, AttrIr::Style { .. })))
}

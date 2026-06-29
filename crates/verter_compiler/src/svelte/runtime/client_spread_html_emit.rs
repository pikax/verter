//! The element-spread `$.attribute_effect` + the `{@html}` `$.html` EMISSION helpers.
//!
//! Extracted from `client.rs` (the file-size guard boundary): these are the
//! [`ClientEmitter`] methods that build the emitted call text for the two coalesced
//! runtime surfaces a spread element / a `{@html}` tag produces. They read the narrow
//! plan op set + the walk-populated DOM var maps (never a broad-IR emission decision).

use super::client::ClientEmitter;
use super::client_plan::ClientRuntimeOp;
use super::ir::{IrNode, NodeId};

impl ClientEmitter<'_> {
    /// Emit the `$.attribute_effect(el, () => ({ <body> })[, void 0, void 0, void 0,
    /// void 0, true])` spread fold for a spread element. The trailing argument tail is
    /// present only for a void / self-closing element (an `<input>`, whose
    /// value/defaultValue handling the trailing `true` flags).
    pub(super) fn emit_attribute_effect(
        &self,
        target: NodeId,
        fold_body: &str,
        input_trailing: bool,
    ) -> String {
        let var = self.dom_var(target);
        if input_trailing {
            format!(
                "$.attribute_effect({var}, () => ({{ {fold_body} }}), void 0, void 0, void 0, void 0, true)"
            )
        } else {
            format!("$.attribute_effect({var}, () => ({{ {fold_body} }}))")
        }
    }

    /// Emit the only-child `$.html(el, payload, true)` raw-markup call (the `$.reset(el)`
    /// follows via the parent's child walk). The `el` is the PARENT element var.
    pub(super) fn emit_html_only_child(&self, parent: NodeId, payload: &str) -> String {
        let var = self.dom_var(parent);
        format!("$.html({var}, {payload}, true)")
    }

    /// The `payload` (second argument) of the `{@html}` op targeting `node`, or `None`
    /// when no such op exists (a non-`{@html}` node). Read from the narrow op set.
    pub(super) fn html_op_payload(&self, node: NodeId) -> Option<String> {
        self.plan().all_ops().find_map(|op| match op {
            ClientRuntimeOp::Html {
                target, payload, ..
            } if NodeId(target.0) == node => Some(payload.clone()),
            _ => None,
        })
    }

    /// Whether the `{@html}` op targeting `node` is the only-child (controlled) form.
    pub(super) fn html_op_is_only_child(&self, node: NodeId) -> bool {
        self.plan().all_ops().any(|op| {
            matches!(op, ClientRuntimeOp::Html { target, only_child: true, .. }
                if NodeId(target.0) == node)
        })
    }

    /// The PARENT element node id whose direct children contain the `{@html}` node — used
    /// to place the only-child `$.html(parent, …, true)` at the parent's init position.
    /// `None` for a root-level `{@html}` (no parent element).
    pub(super) fn html_only_child_parent(&self, html_node: NodeId) -> Option<NodeId> {
        for (idx, node) in self.ir().nodes.iter().enumerate() {
            if let IrNode::Element(el) = node {
                if el.children.contains(&html_node) {
                    return Some(NodeId(idx as u32));
                }
            }
        }
        None
    }
}

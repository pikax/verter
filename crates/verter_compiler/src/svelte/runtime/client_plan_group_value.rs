//! Dynamic `bind:group` value planning.

use super::client::UnsupportedSvelteRuntimeSurface;
use super::client_plan::SupportedClientIr;
use super::client_plan_types::AttrValue;
use super::ir::{IrNode, NodeId};

impl SupportedClientIr<'_> {
    /// Build the `bind:group` DYNAMIC/mixed value ([`GroupDynamicValue`]) for each recorded
    /// group-input node — the structured value (via the shared [`attr_value_for`](Self::attr_value_for))
    /// plus its reactivity (`has_state || has_call`, the official `RegularElement.js` rule). A
    /// node whose `value` attr is not an emittable dynamic/mixed value fails closed (the
    /// classifier only records a node that carried one, so the `?` is defensive).
    ///
    /// [`GroupDynamicValue`]: super::client_plan_types::GroupDynamicValue
    pub(super) fn collect_group_dynamic_values(
        &self,
        nodes: &[NodeId],
    ) -> Result<
        Vec<(NodeId, super::client_plan_types::GroupDynamicValue)>,
        UnsupportedSvelteRuntimeSurface,
    > {
        let mut out = Vec::with_capacity(nodes.len());
        for &node in nodes {
            let IrNode::Element(el) = self.ir.node(node) else {
                continue;
            };
            let (value, has_state) = self.attr_value_for(
                el,
                "value",
                super::client_legacy_value::AuthoredValueSurface::AttributeValue,
            )?;
            let reactive = has_state || value.has_call();
            // The outer `?? ''` group-value coercion is gated on DEFINEDNESS (official
            // `evaluated.is_defined`), NOT single-vs-mixed: a provably-defined SINGLE value
            // omits it. Reuse the SAME `mixed_chunk_nullish_wrap` definedness analysis the
            // mixed-attribute parts run (no new analysis path) — meaningful only for a single
            // value (a mixed value is already a string and never carries the outer coercion).
            let single_value_defined = matches!(value, AttrValue::Single { .. })
                && self.group_value_single_is_defined(el)?;
            out.push((
                node,
                super::client_plan_types::GroupDynamicValue {
                    value,
                    reactive,
                    single_value_defined,
                },
            ));
        }
        Ok(out)
    }
}

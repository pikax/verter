//! Demand-time display view for a child contract slot's synthetic-binding
//! carriers — the hover-boundary half of shallow-by-default slot publication.
//!
//! The contract publishes concrete-inline slot-binding types as the shallow
//! [`TypeExpr::SyntheticSlotBinding`] carrier; rendering that carrier is a
//! typed refusal. A slot-name hover is the consumer's explicit terminal
//! demand, so the server deepens the RANK-MATCHED slot's carrier bindings —
//! and only that slot's — through the one sanctioned session route
//! (`VerterHost::deepen_synthetic_slot_binding`) and threads the resolved
//! views to the hover builder. Everything here is typed-IR: carrier
//! extraction is a variant match, never display text, and a binding whose
//! deepen fails keeps the refusal (fail-closed).

use std::sync::Arc;

use verter_session::framework::{ComponentContractAvailability, PublicSlot};
use verter_type_expr::{SyntheticCarrierKey, TypeExpr};

/// Per-request deepened display views for ONE rank-matched slot's bindings,
/// keyed by binding name. Empty when no binding deepened (every consumer then
/// renders the published form — the fail-closed refusal for carriers).
#[derive(Debug, Default)]
pub struct SlotBindingDeepenView {
    deepened: std::collections::HashMap<Arc<str>, TypeExpr>,
}

impl SlotBindingDeepenView {
    /// The deepened display type for `binding_name`, when its carrier deepened.
    #[must_use]
    pub fn deepened(&self, binding_name: &str) -> Option<&TypeExpr> {
        self.deepened.get(binding_name)
    }

    /// Record one binding's deepened view.
    pub fn insert(&mut self, binding_name: Arc<str>, deepened: TypeExpr) {
        self.deepened.insert(binding_name, deepened);
    }
}

/// Select the contract slot the authored `slot_name` addresses — the ONE
/// rank-match selection shared by the hover builder and the deepen scoping
/// (kebab↔camel equivalence via `attr_name_match_rank`, deterministic
/// tie-break via `select_best_ranked_candidate`).
#[must_use]
pub fn select_contract_slot<'contract>(
    slots: &'contract [PublicSlot],
    slot_name: &str,
) -> Option<&'contract PublicSlot> {
    crate::server::select_best_ranked_candidate(slots.iter().enumerate().filter_map(
        |(index, slot)| {
            crate::server::attr_name_match_rank(slot_name, &slot.name).map(|rank| {
                (
                    rank,
                    verter_span::Span::new(index as u32, index as u32),
                    slot,
                )
            })
        },
    ))
    .map(|(_, _, slot)| slot)
}

/// The RANK-MATCHED slot's synthetic-carrier bindings — `(binding_name, key)`
/// pairs extracted by a typed-IR variant match on each binding's published
/// materialized type. Path-precise by construction: only the one slot the
/// authored name addresses contributes; other slots' carriers never enter the
/// deepen fan-out. Non-carrier bindings are absent (they render their
/// published form directly).
#[must_use]
pub fn matched_slot_carrier_bindings<'contract>(
    availability: &'contract ComponentContractAvailability,
    slot_name: &str,
) -> Vec<(&'contract Arc<str>, &'contract Arc<SyntheticCarrierKey>)> {
    let ComponentContractAvailability::Supported(contract) = availability else {
        return Vec::new();
    };
    let Some(slot) = select_contract_slot(&contract.slots, slot_name) else {
        return Vec::new();
    };
    slot.input
        .bindings
        .iter()
        .filter_map(|binding| match binding.ty.publication.materialized_type() {
            Some(TypeExpr::SyntheticSlotBinding(key)) => Some((&binding.name, key)),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
#[path = "hover_slot_deepen_tests.rs"]
mod hover_slot_deepen_tests;

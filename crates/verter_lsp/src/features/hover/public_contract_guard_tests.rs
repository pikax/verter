use tower_lsp_server::ls_types::Hover;
use verter_session::framework::ComponentContractAvailability;

use super::{
    build_child_component_hover, build_child_event_hover, build_child_slot_hover,
    ComponentUsagePropInfo,
};
use crate::features::hover_slot_deepen::SlotBindingDeepenView;

#[test]
fn child_summary_function_boundaries_accept_only_contract_availability() {
    let _: fn(&str, &str, &ComponentContractAvailability, &[ComponentUsagePropInfo]) -> Hover =
        build_child_component_hover;
    let _: fn(&str, &ComponentContractAvailability) -> Option<Hover> = build_child_event_hover;
    // The slot builder additionally accepts the rank-matched slot's
    // demand-resolved carrier views — a projection OF the same contract rows
    // through the one sanctioned deepen route, never a second meaning source
    // (no analysis snapshot, no generated-code text).
    let _: fn(&str, &str, &ComponentContractAvailability, &SlotBindingDeepenView) -> Option<Hover> =
        build_child_slot_hover;
}

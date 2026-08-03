use tower_lsp_server::ls_types::Hover;
use verter_session::framework::ComponentContractAvailability;

use super::{
    build_child_component_hover, build_child_event_hover, build_child_slot_hover,
    ComponentUsagePropInfo,
};

#[test]
fn child_summary_function_boundaries_accept_only_contract_availability() {
    let _: fn(&str, &str, &ComponentContractAvailability, &[ComponentUsagePropInfo]) -> Hover =
        build_child_component_hover;
    let _: fn(&str, &ComponentContractAvailability) -> Option<Hover> = build_child_event_hover;
    let _: fn(&str, &str, &ComponentContractAvailability) -> Option<Hover> = build_child_slot_hover;
}

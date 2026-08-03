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

#[test]
fn retired_declaration_parser_and_response_transport_stay_absent() {
    let hover_source = include_str!("../hover.rs");
    let resolver_source = include_str!("../../server/component_resolve.rs");
    for retired in [
        "parse_public_api_",
        "handler_signature_for_event",
        "summarize_event_handler_signature",
        "public_api_code",
        "public_api_summary",
    ] {
        assert!(
            !hover_source.contains(retired),
            "retired child-summary symbol `{retired}` re-entered hover production code"
        );
    }
    for forbidden in [
        ".get_public_api(",
        ".ts_labeled_code(",
        "projection.response",
        "public_api_code",
    ] {
        assert!(
            !resolver_source.contains(forbidden),
            "child-summary resolver transported declaration response via `{forbidden}`"
        );
    }
}

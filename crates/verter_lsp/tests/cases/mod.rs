//! Private sorted manifest of the consolidated `verter_lsp` integration
//! test entries. Each `mod` is a former top-level `tests/<entry>.rs`
//! target, now a submodule of the single `main` binary. Keep sorted.

mod carrier_routing_no_vue_gate;
mod carrier_routing_no_vue_named_primitive;
mod closed_carrier_in_autoimport_index;
mod cross_file_navigation_ranges_fail_closed;
mod decl_overlay_close_ownership;
mod editor_liveness_guards;
mod generated_only_spans_suppressed;
mod lsp_audit_cancellation_finalizes_with_marker;
mod lsp_audit_diagnostics_completion;
mod lsp_audit_hover_record;
mod lsp_audit_query_methods;
mod lsp_audit_tls_propagation;
mod lsp_component_meta_output_error;
mod lsp_component_meta_wire_equivalence;
mod owned_binding_gate;
mod position_mapper_strict;
mod repro_autoimport_additional_edits;
mod repro_external_defn_line;
mod repro_sfc_member_completion;
mod repro_sfc_tag_completion_double_lt;
mod shared_provider_live;
mod single_provider_surface_store;
mod stale_generation_result_dropped;
mod stdio_launch_smoke;
mod tsgo_virtual_membership;
mod tsserver_e2e_generated_outputs;

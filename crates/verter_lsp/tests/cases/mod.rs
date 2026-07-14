//! Private sorted manifest of the consolidated `verter_lsp` integration
//! test entries. Each `mod` is a former top-level `tests/<entry>.rs`
//! target, now a submodule of the single `main` binary. Keep sorted.

mod carrier_routing_no_vue_gate;
mod carrier_routing_no_vue_named_primitive;
mod editor_liveness_guards;
mod lsp_audit_cancellation_finalizes_with_marker;
mod lsp_audit_diagnostics_completion;
mod lsp_audit_hover_record;
mod lsp_audit_query_methods;
mod lsp_audit_tls_propagation;
mod lsp_component_meta_output_error;
mod lsp_component_meta_wire_equivalence;
mod position_mapper_strict;
mod repro_autoimport_additional_edits;
mod repro_external_defn_line;
mod repro_sfc_member_completion;

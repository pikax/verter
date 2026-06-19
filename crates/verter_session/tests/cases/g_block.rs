//! Consolidated integration-test group `block`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
//!
//! Shared test harnesses are declared ONCE at the binary root so the leaf
//! modules import them via `use super::harness` / `use super::canary_harness`
//! instead of each `#[path]`-loading the same file (which trips
//! `clippy::duplicate_mod`).
#[path = "g_block/block_1_f_p2_isolation_and_staleness.rs"]
mod block_1_f_p2_isolation_and_staleness;
#[path = "g_block/block_1_i_discriminators.rs"]
mod block_1_i_discriminators;
#[path = "g_block/block_2_canary_compile_tier.rs"]
mod block_2_canary_compile_tier;
#[path = "g_block/block_2_canary_component_meta.rs"]
mod block_2_canary_component_meta;
#[path = "g_block/block_2_canary_lifecycle.rs"]
mod block_2_canary_lifecycle;
#[path = "g_block/block_2_canary_owner_self_edit.rs"]
mod block_2_canary_owner_self_edit;
#[path = "g_block/block_6h_architecture_guards.rs"]
mod block_6h_architecture_guards;
#[path = "g_block/block_6i_f1_dep_signature_cross_file.rs"]
mod block_6i_f1_dep_signature_cross_file;
#[path = "g_block/block_6i_identity_mapper_unchanged.rs"]
mod block_6i_identity_mapper_unchanged;
#[path = "g_block/block_6i_leak_chatmessages_audit.rs"]
mod block_6i_leak_chatmessages_audit;
#[path = "g_block/block_6i_leak_closure.rs"]
mod block_6i_leak_closure;
#[path = "g_block/block_6i_leak_option_a_discriminator.rs"]
mod block_6i_leak_option_a_discriminator;
#[path = "g_block/block_6i_mapped_source_declref_enum.rs"]
mod block_6i_mapped_source_declref_enum;
#[path = "g_block/block_6i_round10_chain_x_path_admission_non_emitting.rs"]
mod block_6i_round10_chain_x_path_admission_non_emitting;
#[path = "g_block/block_6i_round10_chain_y_routed_surface_demand_split.rs"]
mod block_6i_round10_chain_y_routed_surface_demand_split;
#[path = "g_block/block_6i_round10_chain_z_slot_compound_fallback.rs"]
mod block_6i_round10_chain_z_slot_compound_fallback;
#[path = "g_block/block_6i_round10_inherited_emits_invariant.rs"]
mod block_6i_round10_inherited_emits_invariant;
#[path = "g_block/block_6i_round11_chain_v_generic_carrier_publish.rs"]
mod block_6i_round11_chain_v_generic_carrier_publish;
#[path = "g_block/block_6i_round11_inherited_emits_invariant.rs"]
mod block_6i_round11_inherited_emits_invariant;
#[path = "g_block/block_6i_round13_chain_w_record_target_normalization.rs"]
mod block_6i_round13_chain_w_record_target_normalization;
#[path = "g_block/block_6i_round7_emits_unresolved_diagnostic.rs"]
mod block_6i_round7_emits_unresolved_diagnostic;
#[path = "g_block/block_6i_round7_inherited_emits_branch_merge.rs"]
mod block_6i_round7_inherited_emits_branch_merge;
#[path = "g_block/block_6i_round7_no_leak_audit_preservation.rs"]
mod block_6i_round7_no_leak_audit_preservation;
#[path = "g_block/block_6i_round7_selected_key_callable_realization.rs"]
mod block_6i_round7_selected_key_callable_realization;
#[path = "g_block/block_6i_round7_slots_unresolved_diagnostic.rs"]
mod block_6i_round7_slots_unresolved_diagnostic;
#[path = "g_block/block_6i_round9_inherited_emits_branch_merge_survives.rs"]
mod block_6i_round9_inherited_emits_branch_merge_survives;
#[path = "g_block/block_6i_round9_pattern_a_extends_inheritance.rs"]
mod block_6i_round9_pattern_a_extends_inheritance;
#[path = "g_block/block_6i_round9_pattern_b_generic_param.rs"]
mod block_6i_round9_pattern_b_generic_param;
#[path = "g_block/block_6i_runtime_arch_guards.rs"]
mod block_6i_runtime_arch_guards;
#[path = "g_block/block_6i_slot_callable_realization.rs"]
mod block_6i_slot_callable_realization;
#[path = "g_block/block_6i_static_guards.rs"]
mod block_6i_static_guards;
#[path = "g_block/cache_runtime_no_external_cooperative.rs"]
mod cache_runtime_no_external_cooperative;
#[path = "g_block/cache_runtime_singleflight_rehome.rs"]
mod cache_runtime_singleflight_rehome;
#[path = "block_2_canary/harness.rs"]
mod canary_harness;
#[path = "g_block/compile_slots_encapsulation.rs"]
mod compile_slots_encapsulation;
#[path = "g_block/finalise_signature_or_empty_is_gone.rs"]
mod finalise_signature_or_empty_is_gone;
#[path = "g_block/framework_surface_executor.rs"]
mod framework_surface_executor;
#[path = "component_meta_audit/harness.rs"]
mod harness;
#[path = "g_block/r6_query_identity_keys_content_free.rs"]
mod r6_query_identity_keys_content_free;
#[path = "g_block/separation_of_concerns.rs"]
mod separation_of_concerns;
#[path = "g_block/typeinfo_audit_contract_guards.rs"]
mod typeinfo_audit_contract_guards;
#[path = "g_block/typeinfo_graph_contract_guards.rs"]
mod typeinfo_graph_contract_guards;
#[path = "g_block/typeinfo_graph_taxonomy.rs"]
mod typeinfo_graph_taxonomy;
#[path = "g_block/typeinfo_request_contract_guards.rs"]
mod typeinfo_request_contract_guards;
#[path = "g_block/typeinfo_wire_surface_guards.rs"]
mod typeinfo_wire_surface_guards;
#[path = "g_block/u2_demand_lattice_guards.rs"]
mod u2_demand_lattice_guards;
#[path = "g_block/u2_display_projection_guards.rs"]
mod u2_display_projection_guards;
#[path = "g_block/u2_spec_table_guards.rs"]
mod u2_spec_table_guards;
#[path = "g_block/u2_value_domain_design_guards.rs"]
mod u2_value_domain_design_guards;
#[path = "g_block/u2b5_class_ns_enum_overload_guards.rs"]
mod u2b5_class_ns_enum_overload_guards;
#[path = "g_block/u2b6_apparent_template_guards.rs"]
mod u2b6_apparent_template_guards;
#[path = "g_block/u2b7_flow_contextual_guards.rs"]
mod u2b7_flow_contextual_guards;
#[path = "g_block/u2b8_relate_upgrade_guards.rs"]
mod u2b8_relate_upgrade_guards;

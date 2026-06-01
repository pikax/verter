//! Consolidated integration-test group `block`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
//!
//! Shared test harnesses are declared ONCE at the binary root so the leaf
//! modules import them via `use crate::harness` / `use crate::canary_harness`
//! instead of each `#[path]`-loading the same file (which trips
//! `clippy::duplicate_mod`).
#[path = "component_meta_audit/harness.rs"]
mod harness;
#[path = "block_2_canary/harness.rs"]
mod canary_harness;
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

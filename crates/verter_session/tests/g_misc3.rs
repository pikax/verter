//! Consolidated integration-test group `misc3`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
//!
//! Consolidation intentionally pulls sibling submodules that each
//! `#[path = "../<shared-dir>/harness.rs"]`-include the same harness file
//! into one binary, so `clippy::duplicate_mod` fires at every load after
//! the first. The duplication is an intentional consequence of merging
//! formerly-independent test binaries to amortise link cost.
#![allow(clippy::duplicate_mod)]
#[path = "g_misc3/correctness.rs"]
mod correctness;
#[path = "g_misc3/cutover_state_arch_guard.rs"]
mod cutover_state_arch_guard;
#[path = "g_misc3/derived_raw_state_cached_fallthrough_matrix_member.rs"]
mod derived_raw_state_cached_fallthrough_matrix_member;
#[path = "g_misc3/derived_raw_state_cached_meta_payload_matrix_import_ref.rs"]
mod derived_raw_state_cached_meta_payload_matrix_import_ref;
#[path = "g_misc3/derived_raw_state_cached_meta_payload_matrix_route_surface.rs"]
mod derived_raw_state_cached_meta_payload_matrix_route_surface;
#[path = "g_misc3/derived_raw_state_cached_resolved_meta_matrix_member_presence.rs"]
mod derived_raw_state_cached_resolved_meta_matrix_member_presence;
#[path = "g_misc3/external_corpus_drift.rs"]
mod external_corpus_drift;
#[path = "g_misc3/import_route_writer_guard.rs"]
mod import_route_writer_guard;
#[path = "g_misc3/legacy_accumulate_dispatch_dep_signature_gone.rs"]
mod legacy_accumulate_dispatch_dep_signature_gone;
#[path = "g_misc3/mapper_fingerprint_content_addressed.rs"]
mod mapper_fingerprint_content_addressed;
#[path = "g_misc3/module_augmentation_stitching.rs"]
mod module_augmentation_stitching;
#[path = "g_misc3/no_declared_component_meta.rs"]
mod no_declared_component_meta;
#[path = "g_misc3/no_legacy_trace_surface.rs"]
mod no_legacy_trace_surface;
#[path = "g_misc3/origin_graph_audit_contract.rs"]
mod origin_graph_audit_contract;
#[path = "g_misc3/path_precise_invalidation_baseline.rs"]
mod path_precise_invalidation_baseline;
#[path = "g_misc3/repo_first_pass_diagnosis_corpus.rs"]
mod repo_first_pass_diagnosis_corpus;
#[path = "g_misc3/shallow_walk_no_over_materialise.rs"]
mod shallow_walk_no_over_materialise;
#[path = "g_misc3/signature_overflow_pre_canary.rs"]
mod signature_overflow_pre_canary;
#[path = "g_misc3/signature_size_bound.rs"]
mod signature_size_bound;
#[path = "g_misc3/slot_binding_graph_fact_tracer_emission.rs"]
mod slot_binding_graph_fact_tracer_emission;
#[path = "g_misc3/slot_binding_graph_matrix_member_presence.rs"]
mod slot_binding_graph_matrix_member_presence;
#[path = "g_misc3/slot_binding_graph_unrelated_edit_stays_warm.rs"]
mod slot_binding_graph_unrelated_edit_stays_warm;
#[path = "g_misc3/store_view_validates_fact_signature.rs"]
mod store_view_validates_fact_signature;
#[path = "g_misc3/storeview_per_domain_dispatch.rs"]
mod storeview_per_domain_dispatch;
#[path = "g_misc3/structural_carrier_no_get_any_guard.rs"]
mod structural_carrier_no_get_any_guard;
#[path = "g_misc3/tracer_stack_fan_out_to_all_levels.rs"]
mod tracer_stack_fan_out_to_all_levels;
#[path = "g_misc3/tracer_stack_nesting_supported.rs"]
mod tracer_stack_nesting_supported;
#[path = "g_misc3/typeinfo_public_api.rs"]
mod typeinfo_public_api;
#[path = "g_misc3/vue_macro_define_surface_dispatch_only.rs"]
mod vue_macro_define_surface_dispatch_only;
#[path = "g_misc3/wait_audit_lock_contention_observable.rs"]
mod wait_audit_lock_contention_observable;
#[path = "g_misc3/wait_audit_off_when_flag_off.rs"]
mod wait_audit_off_when_flag_off;
#[path = "g_misc3/wait_audit_queue_wait_derives_from_scheduler_audit.rs"]
mod wait_audit_queue_wait_derives_from_scheduler_audit;
#[path = "g_misc3/world_snapshot_is_not_a_cache_key.rs"]
mod world_snapshot_is_not_a_cache_key;

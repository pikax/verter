//! Consolidated integration-test group `misc2`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
//!
//! The shared `component_meta_audit` harness is declared ONCE at the binary
//! root so leaf modules import it via `use super::harness` instead of each
//! `#[path]`-loading the same file (which trips `clippy::duplicate_mod`).
#[path = "g_misc2/corpus_regression_capture_harness.rs"]
mod corpus_regression_capture_harness;
#[path = "g_misc2/cross_file_provenance_fixtures_tests.rs"]
mod cross_file_provenance_fixtures_tests;
#[path = "g_misc2/derived_raw_state_cached_fallthrough_matrix_import_ref.rs"]
mod derived_raw_state_cached_fallthrough_matrix_import_ref;
#[path = "g_misc2/derived_raw_state_cached_fallthrough_matrix_route_surface.rs"]
mod derived_raw_state_cached_fallthrough_matrix_route_surface;
#[path = "g_misc2/derived_raw_state_cached_meta_payload_matrix_module_aug_index_shape.rs"]
mod derived_raw_state_cached_meta_payload_matrix_module_aug_index_shape;
#[path = "g_misc2/derived_raw_state_cached_resolved_meta_matrix_member.rs"]
mod derived_raw_state_cached_resolved_meta_matrix_member;
#[path = "g_misc2/dispatch_cold_build_has_one_call_site.rs"]
mod dispatch_cold_build_has_one_call_site;
// Each entry module intentionally gets its own copy of this stateless
// fixture helper (no statics/atomics/OnceCell), so the per-entry scopes
// stay disjoint and share no state. The "duplicate mod" the lint reports
// is the intended layout, not an accident — keep the allow at every site.
#[allow(clippy::duplicate_mod)]
#[path = "component_meta_audit/harness.rs"]
mod harness;
#[path = "g_misc2/host_store_view_validates_real.rs"]
mod host_store_view_validates_real;
#[path = "g_misc2/is_facts_irrelevant_eligibility.rs"]
mod is_facts_irrelevant_eligibility;
#[path = "g_misc2/macro_surface_no_breadth_walk_audit.rs"]
mod macro_surface_no_breadth_walk_audit;
#[path = "g_misc2/module_augmentation.rs"]
mod module_augmentation;
#[path = "g_misc2/no_carrier_verdict_db.rs"]
mod no_carrier_verdict_db;
#[path = "g_misc2/no_empty_fact_signature_on_warm_write.rs"]
mod no_empty_fact_signature_on_warm_write;
#[path = "g_misc2/no_post_cutover_deferrals.rs"]
mod no_post_cutover_deferrals;
#[path = "g_misc2/path_precise_invalidation.rs"]
mod path_precise_invalidation;
#[path = "g_misc2/pe4_evaluate_depth_budget.rs"]
mod pe4_evaluate_depth_budget;
#[path = "g_misc2/pe4_mapped_type_k_independent_hoist.rs"]
mod pe4_mapped_type_k_independent_hoist;
#[path = "g_misc2/pe4_substitute_hash_cons.rs"]
mod pe4_substitute_hash_cons;
#[path = "g_misc2/phase5_decomposition_tests.rs"]
mod phase5_decomposition_tests;
#[path = "g_misc2/phase5_native_payload_parity.rs"]
mod phase5_native_payload_parity;
#[path = "g_misc2/phase5_q20_benchmark.rs"]
mod phase5_q20_benchmark;
#[path = "g_misc2/plan_rule_namespace.rs"]
mod plan_rule_namespace;
#[path = "g_misc2/process_rss_wasm.rs"]
mod process_rss_wasm;
#[path = "g_misc2/process_rss_windows.rs"]
mod process_rss_windows;
#[path = "g_misc2/r21_c5_cross_file_provenance.rs"]
mod r21_c5_cross_file_provenance;
#[path = "g_misc2/recursive_substitute_memo.rs"]
mod recursive_substitute_memo;
#[path = "g_misc2/request_budget_context.rs"]
mod request_budget_context;
#[path = "g_misc2/request_context_tls.rs"]
mod request_context_tls;
#[path = "g_misc2/request_critical_path_aggregates_correctly.rs"]
mod request_critical_path_aggregates_correctly;
#[path = "g_misc2/sampler_ignores_filtered_requests.rs"]
mod sampler_ignores_filtered_requests;
#[path = "g_misc2/scheduler_audit_attributes_worker.rs"]
mod scheduler_audit_attributes_worker;
#[path = "g_misc2/scheduler_audit_parent_request_id.rs"]
mod scheduler_audit_parent_request_id;
#[path = "g_misc2/scheduler_audit_queue_dwell_under_load.rs"]
mod scheduler_audit_queue_dwell_under_load;
#[path = "g_misc2/scheduler_worker_tls_propagation.rs"]
mod scheduler_worker_tls_propagation;
#[path = "g_misc2/second_query_hits_cache.rs"]
mod second_query_hits_cache;
#[path = "g_misc2/shallow_walk_invariant.rs"]
mod shallow_walk_invariant;
#[path = "g_misc2/slot_binding_graph_matrix_member.rs"]
mod slot_binding_graph_matrix_member;
#[path = "g_misc2/store_view_compat_token_concurrency_only.rs"]
mod store_view_compat_token_concurrency_only;
#[path = "g_misc2/synthetic_carrier_explicit_deepen_routes_through_shape_cache_key.rs"]
mod synthetic_carrier_explicit_deepen_routes_through_shape_cache_key;
#[path = "g_misc2/u3c_chatmessages_audit.rs"]
mod u3c_chatmessages_audit;
#[path = "g_misc2/workspace_bookkeeping_invariants.rs"]
mod workspace_bookkeeping_invariants;

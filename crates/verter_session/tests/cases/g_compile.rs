//! Consolidated integration-test group `compile`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
#[path = "g_compile/admission_overflow_routes_to_return_only.rs"]
mod admission_overflow_routes_to_return_only;
#[path = "g_compile/compile_audit_css_analysis.rs"]
mod compile_audit_css_analysis;
#[path = "g_compile/compile_audit_filtered_and_parent.rs"]
mod compile_audit_filtered_and_parent;
#[path = "g_compile/compile_audit_ide.rs"]
mod compile_audit_ide;
#[path = "g_compile/compile_audit_no_hot_loop_instrumentation.rs"]
mod compile_audit_no_hot_loop_instrumentation;
#[path = "g_compile/compile_audit_sourcemap.rs"]
mod compile_audit_sourcemap;
#[path = "g_compile/compile_audit_vdom.rs"]
mod compile_audit_vdom;
#[path = "g_compile/compile_audit_vue_only_guard.rs"]
mod compile_audit_vue_only_guard;
#[path = "g_compile/compile_cache_mode_classifier.rs"]
mod compile_cache_mode_classifier;
#[path = "g_compile/compile_cache_mode_content_reuse.rs"]
mod compile_cache_mode_content_reuse;
#[path = "g_compile/compile_cache_mode_downgrade_audit.rs"]
mod compile_cache_mode_downgrade_audit;
#[path = "g_compile/compile_cache_mode_pipeline_consumption.rs"]
mod compile_cache_mode_pipeline_consumption;
#[path = "g_compile/compile_cache_mode_priority_order.rs"]
mod compile_cache_mode_priority_order;
#[path = "g_compile/compile_cache_mode_session_fact_validation.rs"]
mod compile_cache_mode_session_fact_validation;
#[path = "g_compile/compile_cache_mode_session_only_prefetch.rs"]
mod compile_cache_mode_session_only_prefetch;
#[path = "g_compile/compile_cache_mode_stateless_bypass.rs"]
mod compile_cache_mode_stateless_bypass;
#[path = "g_compile/compile_cache_overflow_return_only.rs"]
mod compile_cache_overflow_return_only;
#[path = "g_compile/compile_empty_macro_type_deps_clears_semantic_axis.rs"]
mod compile_empty_macro_type_deps_clears_semantic_axis;
#[path = "g_compile/compile_fail.rs"]
mod compile_fail;
#[path = "g_compile/compile_force_overflow_is_host_scoped.rs"]
mod compile_force_overflow_is_host_scoped;
#[path = "g_compile/compile_slot_single_candidate.rs"]
mod compile_slot_single_candidate;
#[path = "g_compile/compile_tier_fact_validation.rs"]
mod compile_tier_fact_validation;
#[path = "g_compile/compile_tier_producer_observes_cross_file_facts.rs"]
mod compile_tier_producer_observes_cross_file_facts;

//! Consolidated integration-test group `family`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
#[path = "g_family/family_a_fact_validation.rs"]
mod family_a_fact_validation;
#[path = "g_family/family_bcd_cross_request_no_stale.rs"]
mod family_bcd_cross_request_no_stale;
#[path = "g_family/family_bcd_cross_thread_joiner_no_stale.rs"]
mod family_bcd_cross_thread_joiner_no_stale;
#[path = "g_family/family_bcd_fact_validation.rs"]
mod family_bcd_fact_validation;
#[path = "g_family/family_bcd_nested_tracers_safe.rs"]
mod family_bcd_nested_tracers_safe;
#[path = "g_family/family_bcd_overflow_refuses_cache.rs"]
mod family_bcd_overflow_refuses_cache;
#[path = "g_family/family_bcd_top_level_tracer_admits_cache.rs"]
mod family_bcd_top_level_tracer_admits_cache;
#[path = "g_family/family_slots_multi_candidate.rs"]
mod family_slots_multi_candidate;

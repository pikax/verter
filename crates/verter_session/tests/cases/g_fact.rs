//! Consolidated integration-test group `fact`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
#[path = "g_fact/class_dual_space.rs"]
mod class_dual_space;
#[path = "g_fact/fact_emission_parse_time_budget.rs"]
mod fact_emission_parse_time_budget;
#[path = "g_fact/fact_fingerprint_stability.rs"]
mod fact_fingerprint_stability;
#[path = "g_fact/fact_lane_correctness.rs"]
mod fact_lane_correctness;
#[path = "g_fact/fact_read_set_finalise_overflow.rs"]
mod fact_read_set_finalise_overflow;
#[path = "g_fact/fact_semantic_display_split.rs"]
mod fact_semantic_display_split;
#[path = "g_fact/fact_tracer_arch_guard.rs"]
mod fact_tracer_arch_guard;
#[path = "g_fact/fact_tracer_callsite_inventory.rs"]
mod fact_tracer_callsite_inventory;
#[path = "g_fact/fact_tracer_observe.rs"]
mod fact_tracer_observe;

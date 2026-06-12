//! Consolidated integration-test group `route`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
#[path = "g_route/route_db_fact_bubbling_cold_resolve.rs"]
mod route_db_fact_bubbling_cold_resolve;
#[path = "g_route/route_db_fact_bubbling_singleflight_join.rs"]
mod route_db_fact_bubbling_singleflight_join;
#[path = "g_route/route_db_fact_bubbling_warm_hit.rs"]
mod route_db_fact_bubbling_warm_hit;
#[path = "g_route/route_db_fact_validation.rs"]
mod route_db_fact_validation;
#[path = "g_route/route_db_get_or_resolve_route_observing_facts.rs"]
mod route_db_get_or_resolve_route_observing_facts;
#[path = "g_route/route_db_get_route_with_facts.rs"]
mod route_db_get_route_with_facts;
#[path = "g_route/route_db_unadmitted_resolve_not_burst_rendezvous.rs"]
mod route_db_unadmitted_resolve_not_burst_rendezvous;
#[path = "g_route/route_db_unrelated_route_edit_stays_warm.rs"]
mod route_db_unrelated_route_edit_stays_warm;
#[path = "g_route/route_generation_admission_guard.rs"]
mod route_generation_admission_guard;

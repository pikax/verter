//! Consolidated integration-test group `resolved`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
#[path = "g_resolved/resolved_import_facts_invariants.rs"]
mod resolved_import_facts_invariants;
#[path = "g_resolved/resolved_import_facts_key_shape.rs"]
mod resolved_import_facts_key_shape;
#[path = "g_resolved/resolved_import_facts_lane_population.rs"]
mod resolved_import_facts_lane_population;
#[path = "g_resolved/resolved_import_facts_namespace_space_admitted.rs"]
mod resolved_import_facts_namespace_space_admitted;
#[path = "g_resolved/resolved_import_facts_producer_real.rs"]
mod resolved_import_facts_producer_real;
#[path = "g_resolved/resolved_import_facts_unresolved_admitted.rs"]
mod resolved_import_facts_unresolved_admitted;
#[path = "g_resolved/resolved_import_facts_validator_real_path.rs"]
mod resolved_import_facts_validator_real_path;
#[path = "g_resolved/resolved_no_residual_operator_leaves.rs"]
mod resolved_no_residual_operator_leaves;

//! Consolidated integration-test group `file`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
#[path = "g_file/file_artifact_store_smoke.rs"]
mod file_artifact_store_smoke;
#[path = "g_file/file_audit_role_attribution.rs"]
mod file_audit_role_attribution;
#[path = "g_file/file_audit_timing_gated_by_flag.rs"]
mod file_audit_timing_gated_by_flag;
#[path = "g_file/file_audit_timing_only_for_triggering_request.rs"]
mod file_audit_timing_only_for_triggering_request;
#[path = "g_file/file_load_count_within_decl_graph.rs"]
mod file_load_count_within_decl_graph;

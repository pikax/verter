//! Consolidated integration-test group `type`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
#[path = "g_type/type_resolution_audit_cache_reuse_across_entrypoints.rs"]
mod type_resolution_audit_cache_reuse_across_entrypoints;
#[path = "g_type/type_resolution_audit_diamond_repeated_prop.rs"]
mod type_resolution_audit_diamond_repeated_prop;
#[path = "g_type/type_resolution_audit_intermediate_navigate_terminal_caller_mode.rs"]
mod type_resolution_audit_intermediate_navigate_terminal_caller_mode;
#[path = "g_type/type_resolution_audit_long_chain_stack_safe.rs"]
mod type_resolution_audit_long_chain_stack_safe;
#[path = "g_type/type_resolution_audit_no_unrelated_imports.rs"]
mod type_resolution_audit_no_unrelated_imports;
#[path = "g_type/type_resolution_audit_pathological_recursion.rs"]
mod type_resolution_audit_pathological_recursion;
#[path = "g_type/type_resolution_audit_read_once.rs"]
mod type_resolution_audit_read_once;
#[path = "g_type/type_resolution_audit_tls_propagation.rs"]
mod type_resolution_audit_tls_propagation;
#[path = "g_type/typeinfo_request_validation.rs"]
mod typeinfo_request_validation;

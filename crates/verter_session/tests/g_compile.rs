//! Consolidated integration-test group `compile`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
#[path = "g_compile/compile_audit_css_analysis.rs"]
mod compile_audit_css_analysis;
#[path = "g_compile/compile_audit_ide.rs"]
mod compile_audit_ide;
#[path = "g_compile/compile_audit_no_hot_loop_instrumentation.rs"]
mod compile_audit_no_hot_loop_instrumentation;
#[path = "g_compile/compile_audit_sourcemap.rs"]
mod compile_audit_sourcemap;
#[path = "g_compile/compile_audit_vdom.rs"]
mod compile_audit_vdom;
#[path = "g_compile/compile_fail.rs"]
mod compile_fail;
#[path = "g_compile/compile_slot_single_candidate.rs"]
mod compile_slot_single_candidate;
#[path = "g_compile/compile_tier_fact_validation.rs"]
mod compile_tier_fact_validation;
#[path = "g_compile/compile_tier_producer_observes_cross_file_facts.rs"]
mod compile_tier_producer_observes_cross_file_facts;

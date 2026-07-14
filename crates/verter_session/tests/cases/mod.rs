//! Private sorted manifest of the consolidated integration-test
//! entries. One `mod <entry>;` per former top-level
//! `tests/<entry>.rs` target (and the former
//! `tests/component_meta_audit_corpus/` directory target). Each
//! entry is its own module so per-entry helpers stay in disjoint
//! scopes — do NOT centralise shared helpers here, and keep this
//! list sorted. Support-only includes (e.g.
//! `support/audit_hot_loop_denylist.rs`) are reached via `#[path]`
//! from the entries that consume them and are intentionally absent
//! from this manifest.

mod architecture_guards;
mod carrier_byte_parity;
mod carrier_coordinator_route_guard;
mod carrier_encapsulation_guards;
mod carrier_routing_no_vue_gate;
mod client_framework_manifest_ts_freshness;
mod component_meta_audit;
mod component_meta_audit_corpus;
mod corpus_audit_tests;
mod defect_b_corpus_prevention_gate;
mod fact_matrix;
mod family_warm_read_releases_mutex_before_validate;
mod framework_corpus_svelte;
mod framework_known_bug_manifest;
mod g_audit;
mod g_block;
mod g_cache;
mod g_compile;
mod g_component;
mod g_extts;
mod g_fact;
mod g_family;
mod g_file;
mod g_misc0;
mod g_misc1;
mod g_misc2;
mod g_misc3;
mod g_resolved;
mod g_route;
mod g_session;
mod g_type;
mod handle_capable_consumer_guards;
mod host_preset_policy;
mod integration_test_layout_guard;
mod oracle_driver;
mod oracle_query_specs_shared;
mod oracle_tsgo_forbidden;
mod output_projector_residual_guards;
mod parse_sfc_chokepoint_guard;
mod residual_type_expr_body_reader_inventory;
mod svelte_compiler_block1;
mod svelte_compiler_block1_guards;
mod svelte_jsx_shim_freshness;
mod svelte_rune_module_guards;
mod svelte_typecheck_gate;
mod terminal_type_expr_authority_manifest;
mod tracked_paths_are_portable;
mod tracked_paths_no_machine_roots;
mod ts_compat_single_spec;
mod typeinfo_ignored_test_manifest;
mod typeinfo_manifest_freshness;
mod virtual_file_naming_characterization;
mod virtual_file_naming_ts_freshness;
mod whole_env_consumer_graph_native_inventory;

//! Private sorted manifest of the consolidated integration-test
//! entries. One `mod <entry>;` per consolidated top-level test entry.
//! Each entry is its own module so per-entry helpers stay in disjoint
//! scopes — do NOT centralise shared helpers here, and keep this
//! list sorted. Support-only includes (e.g.
//! `support/audit_hot_loop_denylist.rs`) are reached via `#[path]`
//! from the entries that consume them and are intentionally absent
//! from this manifest.

mod architecture_guards;
mod capability_matrix_css_family_rows_ratified;
mod carrier_byte_parity;
mod carrier_compile_routing_gate;
mod carrier_coordinator_route_guard;
mod carrier_encapsulation_guards;
mod carrier_routing_no_vue_gate;
mod client_framework_manifest_ts_freshness;
mod component_meta_audit;
mod corpus_audit_layout;
mod corpus_audit_tests;
mod css_attribution_chargeable;
mod defect_b_corpus_prevention_gate;
mod exposed_binding_regression;
mod fact_matrix;
mod family_warm_read_releases_mutex_before_validate;
mod flow_product_lattice;
mod flow_solve_completeness;
mod framework_corpus_svelte;
mod g_audit;
mod g_binder;
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
mod host_backend_routing_guards;
mod host_preset_policy;
mod integration_test_layout_guard;
mod native_content_handoff;
mod nextest_slow_timeout_matches_advertised_budget;
mod one_parse_per_style_block;
mod oracle_driver;
mod oracle_query_specs_shared;
mod oracle_tsgo_forbidden;
mod preprocessor_boundary_contract;
mod preprocessor_round_trip_parse_count;
mod relation_nominal_authority;
mod runtime_constructor_matrix;
mod shared_process_contract;
mod style_dialect_admission;
mod style_native_analysis_preprocessor_boundary;
mod svelte_jsx_shim_freshness;
mod svelte_rune_module_guards;
mod svelte_typecheck_gate;
mod ts_compat_single_spec;
mod typeinfo_ignored_test_manifest;
mod typeinfo_manifest_freshness;
mod virtual_file_naming_characterization;
mod virtual_file_naming_ts_freshness;
mod vue_macro_tsc_typecheck_gate;
mod warm_style_parse_reuse;

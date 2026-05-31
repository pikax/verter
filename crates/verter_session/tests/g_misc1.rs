//! Consolidated integration-test group `misc1`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
#[path = "g_misc1/corpus_generator_parity.rs"]
mod corpus_generator_parity;
#[path = "g_misc1/cross_consumer_fact_matrix_complete.rs"]
mod cross_consumer_fact_matrix_complete;
#[path = "g_misc1/derived_raw_state_cached_fallthrough_fact_validation.rs"]
mod derived_raw_state_cached_fallthrough_fact_validation;
#[path = "g_misc1/derived_raw_state_cached_fallthrough_matrix_module_aug_index_shape.rs"]
mod derived_raw_state_cached_fallthrough_matrix_module_aug_index_shape;
#[path = "g_misc1/derived_raw_state_cached_meta_payload_matrix_member_presence.rs"]
mod derived_raw_state_cached_meta_payload_matrix_member_presence;
#[path = "g_misc1/derived_raw_state_cached_resolved_meta_matrix_import_ref.rs"]
mod derived_raw_state_cached_resolved_meta_matrix_import_ref;
#[path = "g_misc1/derived_raw_state_cached_resolved_meta_matrix_route_surface.rs"]
mod derived_raw_state_cached_resolved_meta_matrix_route_surface;
#[path = "g_misc1/hermetic_checkout.rs"]
mod hermetic_checkout;
#[path = "g_misc1/host_view_env_hashes_real.rs"]
mod host_view_env_hashes_real;
#[path = "g_misc1/host_view_project_identity_real.rs"]
mod host_view_project_identity_real;
#[path = "g_misc1/incidental_fields_trait.rs"]
mod incidental_fields_trait;
#[path = "g_misc1/indexed_ready_app_config_flag.rs"]
mod indexed_ready_app_config_flag;
#[path = "g_misc1/invalidation_public_surface_gone.rs"]
mod invalidation_public_surface_gone;
#[path = "g_misc1/lib_env_hash_excluded_from_resolved_import_facts.rs"]
mod lib_env_hash_excluded_from_resolved_import_facts;
#[path = "g_misc1/member_fact_store_accessors.rs"]
mod member_fact_store_accessors;
#[path = "g_misc1/member_presence_vs_member.rs"]
mod member_presence_vs_member;
#[path = "g_misc1/memo_traced_parse_fact_survives_warm_hit.rs"]
mod memo_traced_parse_fact_survives_warm_hit;
#[path = "g_misc1/memory_peak_rss_per_host.rs"]
mod memory_peak_rss_per_host;
#[path = "g_misc1/memory_peak_rss_zero_on_wasm.rs"]
mod memory_peak_rss_zero_on_wasm;
#[path = "g_misc1/memory_peak_rss_zero_when_flag_off.rs"]
mod memory_peak_rss_zero_when_flag_off;
#[path = "g_misc1/merged_symbol_identity.rs"]
mod merged_symbol_identity;
#[path = "g_misc1/multi_candidate_storage.rs"]
mod multi_candidate_storage;
#[path = "g_misc1/multi_project_no_collision_resolved_imports.rs"]
mod multi_project_no_collision_resolved_imports;
#[path = "g_misc1/no_bare_host_resolver_shims.rs"]
mod no_bare_host_resolver_shims;
#[path = "g_misc1/no_eager_invalidation.rs"]
mod no_eager_invalidation;
#[path = "g_misc1/no_lib_rs_growth.rs"]
mod no_lib_rs_growth;
#[path = "g_misc1/owner_import_surface_and_negative_route_facts.rs"]
mod owner_import_surface_and_negative_route_facts;
#[path = "g_misc1/parse_resolve_domain_separation.rs"]
mod parse_resolve_domain_separation;
#[path = "g_misc1/parse_stable_hash_invariance.rs"]
mod parse_stable_hash_invariance;
#[path = "g_misc1/path_precision_navigate_then_terminal.rs"]
mod path_precision_navigate_then_terminal;
#[path = "g_misc1/pe4_evaluate_deferred_memo.rs"]
mod pe4_evaluate_deferred_memo;
#[path = "g_misc1/plan_h_to_r_mapping.rs"]
mod plan_h_to_r_mapping;
#[path = "g_misc1/semantic_graph_signature_builder_provenance.rs"]
mod semantic_graph_signature_builder_provenance;
#[path = "g_misc1/slot_binding_graph_matrix_import_ref.rs"]
mod slot_binding_graph_matrix_import_ref;
#[path = "g_misc1/slot_binding_graph_matrix_route_surface.rs"]
mod slot_binding_graph_matrix_route_surface;
#[path = "g_misc1/ts_bindings.rs"]
mod ts_bindings;
#[path = "g_misc1/whole_hash_migration_audit.rs"]
mod whole_hash_migration_audit;

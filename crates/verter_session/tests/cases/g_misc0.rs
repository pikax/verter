//! Consolidated integration-test group `misc0`: each module below was
//! a separate top-level tests/*.rs binary, merged to cut test-link count.
#[path = "g_misc0/admission_guard.rs"]
mod admission_guard;
#[path = "g_misc0/audited_request_e2e.rs"]
mod audited_request_e2e;
#[path = "g_misc0/batch_api_shared_admissions.rs"]
mod batch_api_shared_admissions;
#[path = "g_misc0/batch_meta_cold_deps_no_pool_starvation.rs"]
mod batch_meta_cold_deps_no_pool_starvation;
#[path = "g_misc0/bounded_query_identity_retention.rs"]
mod bounded_query_identity_retention;
#[path = "g_misc0/bubble_fact_signature_fans_out.rs"]
mod bubble_fact_signature_fans_out;
#[path = "g_misc0/byte_identical_upsert_no_op.rs"]
mod byte_identical_upsert_no_op;
#[path = "g_misc0/capture_token_smoke.rs"]
mod capture_token_smoke;
#[path = "g_misc0/closure_boundary_invalidation.rs"]
mod closure_boundary_invalidation;
#[path = "g_misc0/critical_rules_have_guards.rs"]
mod critical_rules_have_guards;
#[path = "g_misc0/cross_owner_materialise_reuse.rs"]
mod cross_owner_materialise_reuse;
#[path = "g_misc0/cross_owner_materialise_reuse_production.rs"]
mod cross_owner_materialise_reuse_production;
#[path = "g_misc0/cycle_safety.rs"]
mod cycle_safety;
#[path = "g_misc0/declaration_merge_facts.rs"]
mod declaration_merge_facts;
#[path = "g_misc0/declaration_merge_guards.rs"]
mod declaration_merge_guards;
#[path = "g_misc0/dep_signature_to_fact_signature.rs"]
mod dep_signature_to_fact_signature;
#[path = "g_misc0/derived_raw_state_cached_fallthrough_matrix_member_presence.rs"]
mod derived_raw_state_cached_fallthrough_matrix_member_presence;
#[path = "g_misc0/derived_raw_state_cached_fallthrough_unrelated_edit_stays_warm.rs"]
mod derived_raw_state_cached_fallthrough_unrelated_edit_stays_warm;
#[path = "g_misc0/derived_raw_state_cached_meta_payload_fact_validation.rs"]
mod derived_raw_state_cached_meta_payload_fact_validation;
#[path = "g_misc0/derived_raw_state_cached_meta_payload_matrix_member.rs"]
mod derived_raw_state_cached_meta_payload_matrix_member;
#[path = "g_misc0/derived_raw_state_cached_meta_payload_unrelated_edit_stays_warm.rs"]
mod derived_raw_state_cached_meta_payload_unrelated_edit_stays_warm;
#[path = "g_misc0/derived_raw_state_cached_resolved_meta_fact_validation.rs"]
mod derived_raw_state_cached_resolved_meta_fact_validation;
#[path = "g_misc0/derived_raw_state_cached_resolved_meta_matrix_module_aug_index_shape.rs"]
mod derived_raw_state_cached_resolved_meta_matrix_module_aug_index_shape;
#[path = "g_misc0/derived_raw_state_cached_resolved_meta_unrelated_edit_stays_warm.rs"]
mod derived_raw_state_cached_resolved_meta_unrelated_edit_stays_warm;
#[path = "g_misc0/dispatch_bridges_convert_project_generation.rs"]
mod dispatch_bridges_convert_project_generation;
#[path = "g_misc0/env_hash_isolation.rs"]
mod env_hash_isolation;
#[path = "g_misc0/env_hashes_swap_atomically_on_snapshot_bump.rs"]
mod env_hashes_swap_atomically_on_snapshot_bump;
#[path = "g_misc0/eviction_policy.rs"]
mod eviction_policy;
#[path = "g_misc0/framework_adapter_guards.rs"]
mod framework_adapter_guards;
#[path = "g_misc0/framework_carrier_compiler_guards.rs"]
mod framework_carrier_compiler_guards;
#[path = "g_misc0/framework_carrier_confinement.rs"]
mod framework_carrier_confinement;
#[path = "g_misc0/getcomponentmeta_fallthrough_audit_cleanliness.rs"]
mod getcomponentmeta_fallthrough_audit_cleanliness;
#[path = "g_misc0/golden_semantic_dump.rs"]
mod golden_semantic_dump;
#[path = "g_misc0/host_tests.rs"]
mod host_tests;
#[path = "g_misc0/insert_arc_strict_admission_required.rs"]
mod insert_arc_strict_admission_required;
#[path = "g_misc0/invalidation_coverage.rs"]
mod invalidation_coverage;
#[path = "g_misc0/invalidation_perf.rs"]
mod invalidation_perf;
#[path = "g_misc0/jsdoc_provenance_p2.rs"]
mod jsdoc_provenance_p2;
#[path = "g_misc0/known_but_unsupported_language.rs"]
mod known_but_unsupported_language;
#[path = "g_misc0/language_routing_characterization.rs"]
mod language_routing_characterization;
#[path = "g_misc0/legacy_dep_signature_field_gone.rs"]
mod legacy_dep_signature_field_gone;
#[path = "g_misc0/legacy_walker_parity_baseline.rs"]
mod legacy_walker_parity_baseline;
#[path = "g_misc0/materialiser_observes_or_dies.rs"]
mod materialiser_observes_or_dies;
#[path = "g_misc0/materializations_lane_wired.rs"]
mod materializations_lane_wired;
#[path = "g_misc0/mcp_audit_e2e.rs"]
mod mcp_audit_e2e;
#[path = "g_misc0/mcp_audit_tls_propagation.rs"]
mod mcp_audit_tls_propagation;
#[path = "g_misc0/neutral_script_analysis_not_under_vue_path.rs"]
mod neutral_script_analysis_not_under_vue_path;
#[path = "g_misc0/no_default_env_hashes_in_production.rs"]
mod no_default_env_hashes_in_production;
#[path = "g_misc0/no_legacy_compile_many_upsert_fanout.rs"]
mod no_legacy_compile_many_upsert_fanout;
#[path = "g_misc0/no_legacy_walker.rs"]
mod no_legacy_walker;
#[path = "g_misc0/no_production_caller_of_zero_env_slot_constructors.rs"]
mod no_production_caller_of_zero_env_slot_constructors;
#[path = "g_misc0/origin_graph_consumer_contract.rs"]
mod origin_graph_consumer_contract;
#[path = "g_misc0/overlay_prepared_decl_no_base_cache_pollution.rs"]
mod overlay_prepared_decl_no_base_cache_pollution;
#[path = "g_misc0/plain_script_dialect_from_file_language.rs"]
mod plain_script_dialect_from_file_language;
#[path = "g_misc0/r20_admission_refuses_empty_signature.rs"]
mod r20_admission_refuses_empty_signature;
#[path = "g_misc0/relative_path_session_parity.rs"]
mod relative_path_session_parity;
#[path = "g_misc0/request_kind_payload_parity.rs"]
mod request_kind_payload_parity;
#[path = "g_misc0/resolver_context_active_session_view.rs"]
mod resolver_context_active_session_view;
#[path = "g_misc0/sampler_thread_joined_at_host_drop.rs"]
mod sampler_thread_joined_at_host_drop;
#[path = "g_misc0/selective_component_meta_api.rs"]
mod selective_component_meta_api;
#[path = "g_misc0/semantic_analysis_audit_e2e.rs"]
mod semantic_analysis_audit_e2e;
#[path = "g_misc0/semantic_analysis_audit_tls_propagation.rs"]
mod semantic_analysis_audit_tls_propagation;
#[path = "g_misc0/semantic_graph_production_reads_validated.rs"]
mod semantic_graph_production_reads_validated;
#[path = "g_misc0/single_language_classifier.rs"]
mod single_language_classifier;
#[path = "g_misc0/slot_binding_graph_matrix_module_aug_index_shape.rs"]
mod slot_binding_graph_matrix_module_aug_index_shape;
#[path = "g_misc0/slot_binding_shallow_publication_tests.rs"]
mod slot_binding_shallow_publication_tests;
#[path = "g_misc0/synthetic_carrier_explicit_deepen_proof.rs"]
mod synthetic_carrier_explicit_deepen_proof;
#[path = "g_misc0/terminal_partial_field_type_publishes_finite_utility_shape.rs"]
mod terminal_partial_field_type_publishes_finite_utility_shape;
#[path = "g_misc0/tls_harness_cross_crate.rs"]
mod tls_harness_cross_crate;
#[path = "g_misc0/tls_harness_in_crate.rs"]
mod tls_harness_in_crate;
#[path = "g_misc0/tracer_stack_reentrant_observe_safe.rs"]
mod tracer_stack_reentrant_observe_safe;
#[path = "g_misc0/uniqueness_check_release_active.rs"]
mod uniqueness_check_release_active;
#[path = "g_misc0/upsert_always_canonicalizes_supplied_canonical_id.rs"]
mod upsert_always_canonicalizes_supplied_canonical_id;
#[path = "g_misc0/vue_relocation_no_shim.rs"]
mod vue_relocation_no_shim;
#[path = "g_misc0/walker_parity_baselines_have_full_coverage.rs"]
mod walker_parity_baselines_have_full_coverage;
#[path = "g_misc0/workspace_audit_production_callsite.rs"]
mod workspace_audit_production_callsite;
#[path = "g_misc0/workspace_audit_tls_propagation.rs"]
mod workspace_audit_tls_propagation;

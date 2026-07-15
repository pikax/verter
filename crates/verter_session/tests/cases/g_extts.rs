//! Consolidated integration-test group `extts`: architecture guards for the
//! project-bound external-TypeScript-engine contract + the TS-correct carrier
//! ownership resolver. Each module below is a discriminating guard for a
//! workstream invariant; they run as one group binary (matching the `g_*`
//! cluster convention).
#[path = "../g_extts/shared.rs"]
mod shared;

#[path = "../g_extts/carrier_companion_suffix_collision_free.rs"]
mod carrier_companion_suffix_collision_free;
#[path = "../g_extts/carrier_never_shadows_real_user_file.rs"]
mod carrier_never_shadows_real_user_file;
#[path = "../g_extts/carrier_ownership_extension_rules.rs"]
mod carrier_ownership_extension_rules;
#[path = "../g_extts/component_bare_import_resolves_to_declaration_carrier.rs"]
mod component_bare_import_resolves_to_declaration_carrier;
#[path = "../g_extts/in_band_witness_feeds_gate.rs"]
mod in_band_witness_feeds_gate;
#[path = "../g_extts/ledger_is_off_the_serve_path.rs"]
mod ledger_is_off_the_serve_path;
#[path = "../g_extts/no_fallback_to_inferred_anywhere.rs"]
mod no_fallback_to_inferred_anywhere;
#[path = "../g_extts/non_owning_attach_lifecycle.rs"]
mod non_owning_attach_lifecycle;
#[path = "../g_extts/provider_op_requires_resolved_project.rs"]
mod provider_op_requires_resolved_project;
#[path = "../g_extts/resilient_single_writer_actor_shape.rs"]
mod resilient_single_writer_actor_shape;
#[path = "../g_extts/same_stem_svelte_component_rune_fails_closed.rs"]
mod same_stem_svelte_component_rune_fails_closed;
#[path = "../g_extts/sealed_carrier_store_mutators_allowlist.rs"]
mod sealed_carrier_store_mutators_allowlist;
#[path = "../g_extts/shared_mode_failover_is_per_reference_closure.rs"]
mod shared_mode_failover_is_per_reference_closure;
#[path = "../g_extts/shared_mode_no_unmapped_carrier_path_leak.rs"]
mod shared_mode_no_unmapped_carrier_path_leak;
#[path = "../g_extts/shared_mode_requires_full_ts_lsp_proxy.rs"]
mod shared_mode_requires_full_ts_lsp_proxy;
#[path = "../g_extts/shared_provider_live_wiring.rs"]
mod shared_provider_live_wiring;
#[path = "../g_extts/tsgo_capability_gate_on_version.rs"]
mod tsgo_capability_gate_on_version;
#[path = "../g_extts/tsgo_shared_mode_carrier_injection.rs"]
mod tsgo_shared_mode_carrier_injection;

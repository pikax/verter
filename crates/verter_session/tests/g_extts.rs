//! Consolidated integration-test group `extts`: architecture guards for the
//! project-bound external-TypeScript-engine contract + the TS-correct carrier
//! ownership resolver. Each module below is a discriminating guard for a
//! workstream invariant; they run as one group binary (matching the `g_*`
//! cluster convention).
#[path = "g_extts/shared.rs"]
mod shared;

#[path = "g_extts/carrier_companion_suffix_collision_free.rs"]
mod carrier_companion_suffix_collision_free;
#[path = "g_extts/carrier_never_shadows_real_user_file.rs"]
mod carrier_never_shadows_real_user_file;
#[path = "g_extts/carrier_ownership_extension_rules.rs"]
mod carrier_ownership_extension_rules;
#[path = "g_extts/component_carrier_is_bare_import_probe_compatible.rs"]
mod component_carrier_is_bare_import_probe_compatible;
#[path = "g_extts/ledger_is_off_the_serve_path.rs"]
mod ledger_is_off_the_serve_path;
#[path = "g_extts/no_inferred_project_knobs_on_tsserver.rs"]
mod no_inferred_project_knobs_on_tsserver;
#[path = "g_extts/provider_op_requires_resolved_project.rs"]
mod provider_op_requires_resolved_project;
#[path = "g_extts/resilient_single_writer_actor_shape.rs"]
mod resilient_single_writer_actor_shape;
#[path = "g_extts/same_stem_svelte_component_rune_fails_closed.rs"]
mod same_stem_svelte_component_rune_fails_closed;
#[path = "g_extts/sealed_carrier_store_mutators_allowlist.rs"]
mod sealed_carrier_store_mutators_allowlist;

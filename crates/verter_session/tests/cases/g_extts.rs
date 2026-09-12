//! Consolidated integration-test group `extts`: behavioral cases for the
//! project-bound external-TypeScript-engine contract + the TS-correct carrier
//! ownership resolver. Each module below discriminates an outcome of the real
//! substrate; they run as one group binary (matching the `g_*` cluster
//! convention).
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
#[path = "../g_extts/same_stem_svelte_component_rune_fails_closed.rs"]
mod same_stem_svelte_component_rune_fails_closed;
#[path = "../g_extts/sealed_carrier_store_mutators_allowlist.rs"]
mod sealed_carrier_store_mutators_allowlist;
#[path = "../g_extts/shared_mode_failover_is_per_reference_closure.rs"]
mod shared_mode_failover_is_per_reference_closure;

//! Consolidated integration-test group `extts`: architecture guards for the
//! project-bound external-TypeScript-engine contract + the TS-correct carrier
//! ownership resolver. Each module below is a discriminating guard for a
//! workstream invariant; they run as one group binary (matching the `g_*`
//! cluster convention).
#[path = "g_extts/shared.rs"]
mod shared;

#[path = "g_extts/carrier_never_shadows_real_user_file.rs"]
mod carrier_never_shadows_real_user_file;
#[path = "g_extts/carrier_ownership_extension_rules.rs"]
mod carrier_ownership_extension_rules;
#[path = "g_extts/provider_op_requires_resolved_project.rs"]
mod provider_op_requires_resolved_project;
#[path = "g_extts/same_stem_svelte_component_rune_fails_closed.rs"]
mod same_stem_svelte_component_rune_fails_closed;

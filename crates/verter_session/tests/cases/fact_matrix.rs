//! Cross-consumer × fact-kind matrix.
//!
//! 9 caches × 5 fact-kinds = 45 slices. Each slice characterises
//! one matrix cell. The Family B/C/D caches use a host-pipeline
//! counter-delta discriminator. The Family A consumers
//! (`compile_tier`, `component_meta`, `fallthrough`,
//! `route_surface`, `slot_binding_graph`) use
//! substrate-validation / fact-tracer fan-out discriminators.
//!
//! The completeness arch guard in
//! `tests/cases/g_misc1/cross_consumer_fact_matrix_complete.rs` enforces that
//! every `<consumer>_<fact_kind>.rs` cell exists; adding a new
//! cache-bearing consumer without filing its 5 slices is caught at
//! workspace test time.
//!
//! See `tests/fact_matrix/README.md` for the cache/fact-kind matrix
//! and the consumer/fact-kind mappings.

#[path = "fact_matrix/harness.rs"]
mod harness;

// ── app_config_proof × * ────────────────────────────────────────────
#[path = "fact_matrix/app_config_proof_import_ref.rs"]
mod app_config_proof_import_ref;
#[path = "fact_matrix/app_config_proof_member.rs"]
mod app_config_proof_member;
#[path = "fact_matrix/app_config_proof_member_presence.rs"]
mod app_config_proof_member_presence;
#[path = "fact_matrix/app_config_proof_module_augmentation_index_shape.rs"]
mod app_config_proof_module_augmentation_index_shape;
#[path = "fact_matrix/app_config_proof_observes_no_route_facts.rs"]
mod app_config_proof_observes_no_route_facts;

// ── materialize_structure × * ───────────────────────────────────────
#[path = "fact_matrix/materialize_structure_barrel_route.rs"]
mod materialize_structure_barrel_route;
#[path = "fact_matrix/materialize_structure_import_ref.rs"]
mod materialize_structure_import_ref;
#[path = "fact_matrix/materialize_structure_member.rs"]
mod materialize_structure_member;
#[path = "fact_matrix/materialize_structure_member_presence.rs"]
mod materialize_structure_member_presence;
#[path = "fact_matrix/materialize_structure_module_augmentation_index_shape.rs"]
mod materialize_structure_module_augmentation_index_shape;

// ── memo_entry × * ───────────────────────────────────────────────────
#[path = "fact_matrix/memo_entry_barrel_route_dispatch.rs"]
mod memo_entry_barrel_route_dispatch;
#[path = "fact_matrix/memo_entry_import_ref.rs"]
mod memo_entry_import_ref;
#[path = "fact_matrix/memo_entry_member.rs"]
mod memo_entry_member;
#[path = "fact_matrix/memo_entry_member_presence.rs"]
mod memo_entry_member_presence;
#[path = "fact_matrix/memo_entry_module_augmentation_index_shape.rs"]
mod memo_entry_module_augmentation_index_shape;

// ── owner_import_surface × * ─────────────────────────────────────────
#[path = "fact_matrix/owner_import_surface_barrel_route.rs"]
mod owner_import_surface_barrel_route;
#[path = "fact_matrix/owner_import_surface_import_ref.rs"]
mod owner_import_surface_import_ref;
#[path = "fact_matrix/owner_import_surface_member.rs"]
mod owner_import_surface_member;
#[path = "fact_matrix/owner_import_surface_member_presence.rs"]
mod owner_import_surface_member_presence;
#[path = "fact_matrix/owner_import_surface_module_augmentation_index_shape.rs"]
mod owner_import_surface_module_augmentation_index_shape;

// ── compile_tier × * ─────────────────────────────────────────────────
#[path = "fact_matrix/compile_tier_import_ref.rs"]
mod compile_tier_import_ref;
#[path = "fact_matrix/compile_tier_member.rs"]
mod compile_tier_member;
#[path = "fact_matrix/compile_tier_member_presence.rs"]
mod compile_tier_member_presence;
#[path = "fact_matrix/compile_tier_module_augmentation_index_shape.rs"]
mod compile_tier_module_augmentation_index_shape;

// ── component_meta × * ───────────────────────────────────────────────
#[path = "fact_matrix/component_meta_import_ref.rs"]
mod component_meta_import_ref;
#[path = "fact_matrix/component_meta_member.rs"]
mod component_meta_member;
#[path = "fact_matrix/component_meta_member_presence.rs"]
mod component_meta_member_presence;
#[path = "fact_matrix/component_meta_module_augmentation_index_shape.rs"]
mod component_meta_module_augmentation_index_shape;

// ── fallthrough × * ──────────────────────────────────────────────────
#[path = "fact_matrix/fallthrough_import_ref.rs"]
mod fallthrough_import_ref;
#[path = "fact_matrix/fallthrough_member.rs"]
mod fallthrough_member;
#[path = "fact_matrix/fallthrough_member_presence.rs"]
mod fallthrough_member_presence;
#[path = "fact_matrix/fallthrough_module_augmentation_index_shape.rs"]
mod fallthrough_module_augmentation_index_shape;

// ── route_surface × * ────────────────────────────────────────────────
#[path = "fact_matrix/route_surface_import_ref.rs"]
mod route_surface_import_ref;
#[path = "fact_matrix/route_surface_member.rs"]
mod route_surface_member;
#[path = "fact_matrix/route_surface_member_presence.rs"]
mod route_surface_member_presence;
#[path = "fact_matrix/route_surface_module_augmentation_index_shape.rs"]
mod route_surface_module_augmentation_index_shape;

// ── slot_binding_graph × * ───────────────────────────────────────────
#[path = "fact_matrix/slot_binding_graph_import_ref.rs"]
mod slot_binding_graph_import_ref;
#[path = "fact_matrix/slot_binding_graph_member.rs"]
mod slot_binding_graph_member;
#[path = "fact_matrix/slot_binding_graph_member_presence.rs"]
mod slot_binding_graph_member_presence;
#[path = "fact_matrix/slot_binding_graph_module_augmentation_index_shape.rs"]
mod slot_binding_graph_module_augmentation_index_shape;

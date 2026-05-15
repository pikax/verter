//! Block 1.H — cross-consumer × fact-kind matrix.
//!
//! 5 caches × 5 fact-kinds = 25 slices. Each slice characterises
//! one matrix cell with a representative fixture and a counter-delta
//! discriminator (or a documented degenerate-cell assertion).
//!
//! See `tests/fact_matrix/README.md` for the cache/fact-kind matrix
//! and the Block 1.8 follow-up TODO list.

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
#[path = "fact_matrix/app_config_proof_route_surface.rs"]
mod app_config_proof_route_surface;

// ── materialize_structure × * ───────────────────────────────────────
#[path = "fact_matrix/materialize_structure_import_ref.rs"]
mod materialize_structure_import_ref;
#[path = "fact_matrix/materialize_structure_member.rs"]
mod materialize_structure_member;
#[path = "fact_matrix/materialize_structure_member_presence.rs"]
mod materialize_structure_member_presence;
#[path = "fact_matrix/materialize_structure_module_augmentation_index_shape.rs"]
mod materialize_structure_module_augmentation_index_shape;
#[path = "fact_matrix/materialize_structure_route_surface.rs"]
mod materialize_structure_route_surface;

// ── ref_cycle × * ────────────────────────────────────────────────────
#[path = "fact_matrix/ref_cycle_import_ref.rs"]
mod ref_cycle_import_ref;
#[path = "fact_matrix/ref_cycle_member.rs"]
mod ref_cycle_member;
#[path = "fact_matrix/ref_cycle_member_presence.rs"]
mod ref_cycle_member_presence;
#[path = "fact_matrix/ref_cycle_module_augmentation_index_shape.rs"]
mod ref_cycle_module_augmentation_index_shape;
#[path = "fact_matrix/ref_cycle_route_surface.rs"]
mod ref_cycle_route_surface;

// ── memo_entry × * ───────────────────────────────────────────────────
#[path = "fact_matrix/memo_entry_import_ref.rs"]
mod memo_entry_import_ref;
#[path = "fact_matrix/memo_entry_member.rs"]
mod memo_entry_member;
#[path = "fact_matrix/memo_entry_member_presence.rs"]
mod memo_entry_member_presence;
#[path = "fact_matrix/memo_entry_module_augmentation_index_shape.rs"]
mod memo_entry_module_augmentation_index_shape;
#[path = "fact_matrix/memo_entry_route_surface.rs"]
mod memo_entry_route_surface;

// ── owner_import_surface × * ─────────────────────────────────────────
#[path = "fact_matrix/owner_import_surface_import_ref.rs"]
mod owner_import_surface_import_ref;
#[path = "fact_matrix/owner_import_surface_member.rs"]
mod owner_import_surface_member;
#[path = "fact_matrix/owner_import_surface_member_presence.rs"]
mod owner_import_surface_member_presence;
#[path = "fact_matrix/owner_import_surface_module_augmentation_index_shape.rs"]
mod owner_import_surface_module_augmentation_index_shape;
#[path = "fact_matrix/owner_import_surface_route_surface.rs"]
mod owner_import_surface_route_surface;

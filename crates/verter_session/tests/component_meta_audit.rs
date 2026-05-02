//! Authored correctness suite (plan §3 Commit 7 / F6).
//!
//! Each submodule resolves a curated SFC fixture under a hermetic
//! [`AuditedRequest`] and asserts on the resulting
//! [`RustAuditRecord`](verter_session::component_meta_audit::RustAuditRecord).
//!
//! See [`harness`] for the shared helpers + fixture constants.

#[path = "component_meta_audit/harness.rs"]
mod harness;

// Pathological regression snapshots — shape-pinned via
// `mask_incidental_spans`, not exact set-match.
#[path = "component_meta_audit/pathological_editor_toolbar_array_or_nested.rs"]
mod pathological_editor_toolbar_array_or_nested;
#[path = "component_meta_audit/pathological_table_loading_animation.rs"]
mod pathological_table_loading_animation;
#[path = "component_meta_audit/pathological_tabs_dynamic_helper.rs"]
mod pathological_tabs_dynamic_helper;

// Corpus representatives — `_exactly` set-match on loaded files.
#[path = "component_meta_audit/corpus_representatives/accordion.rs"]
mod corpus_representatives_accordion;
#[path = "component_meta_audit/corpus_representatives/alert.rs"]
mod corpus_representatives_alert;
#[path = "component_meta_audit/corpus_representatives/app.rs"]
mod corpus_representatives_app;
#[path = "component_meta_audit/corpus_representatives/auth_form.rs"]
mod corpus_representatives_auth_form;
#[path = "component_meta_audit/corpus_representatives/avatar.rs"]
mod corpus_representatives_avatar;
#[path = "component_meta_audit/corpus_representatives/avatar_group.rs"]
mod corpus_representatives_avatar_group;

// Standalone — one audit-surface facet each.
#[path = "component_meta_audit/barrel_chain.rs"]
mod barrel_chain;
#[path = "component_meta_audit/closed_conditional.rs"]
mod closed_conditional;
#[path = "component_meta_audit/external_type.rs"]
mod external_type;
#[path = "component_meta_audit/open_conditional.rs"]
mod open_conditional;
#[path = "component_meta_audit/path_precise_projection.rs"]
mod path_precise_projection;
#[path = "component_meta_audit/single_file_generic.rs"]
mod single_file_generic;

// Phase 5b §5.A — TDD seed characterisation tests for the 5 resolver
// coverage gaps the variant + dispatch helpers close. 1 of 5
// (`slot_shapes`) flips green inside Phase 5b after commits 2+3 land
// the `ResolveMacroPayload` variant body. The other 4 remain RED
// until callsite migrations land in 5d/5e/5f. Each seed must FAIL on
// the pre-Phase-5b tree — if any passes, STOP (gap is not real or
// closed elsewhere).
#[path = "component_meta_audit/resolver_coverage_indexed_paths.rs"]
mod resolver_coverage_indexed_paths;
#[path = "component_meta_audit/resolver_coverage_inherited_emits.rs"]
mod resolver_coverage_inherited_emits;
#[path = "component_meta_audit/resolver_coverage_mapped_types.rs"]
mod resolver_coverage_mapped_types;
#[path = "component_meta_audit/resolver_coverage_package_backed.rs"]
mod resolver_coverage_package_backed;
#[path = "component_meta_audit/resolver_coverage_slot_shapes.rs"]
mod resolver_coverage_slot_shapes;

// Phase 5 §5.C (commit N+1) — lib parity tests. Exercise the
// `MaterializeSurface` variant against ambient-lib and userland
// mapped types, plus userland-shadowing-pick. Run via
// `build_hermetic_host_with_lib`.
#[path = "component_meta_audit/lib_parity.rs"]
mod lib_parity;

// Phase 4 (Issue #3) — field-level fast-path counterfixtures.
// Asserts the Phase 4 gate's two sub-assertions
// (Expanded-mode dispatch counter, heritage canonical not in
// loaded-files) plus a counterfixture for the predicate's
// negative branch and an owner-edit invalidation regression.
#[path = "component_meta_audit/phase_4_field_fast_path.rs"]
mod phase_4_field_fast_path;

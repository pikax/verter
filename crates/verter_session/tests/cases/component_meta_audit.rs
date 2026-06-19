//! Authored correctness suite.
//!
//! Each submodule resolves a curated SFC fixture under a hermetic
//! [`AuditedRequest`] and asserts on the resulting
//! [`RequestAuditRecord`](verter_session::component_meta_audit::RequestAuditRecord).
//!
//! See [`harness`] for the shared helpers + fixture constants.

// Each entry module intentionally gets its own copy of this stateless
// fixture helper (no statics/atomics/OnceCell), so the per-entry scopes
// stay disjoint and share no state. The "duplicate mod" the lint reports
// is the intended layout, not an accident — keep the allow at every site.
#[allow(clippy::duplicate_mod)]
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

// Characterisation tests for the 5 resolver coverage gaps the variant
// + dispatch helpers close, exercising the `ResolveMacroPayload`
// variant body and the inherited-emits / indexed-path / mapped-type /
// package-backed callsites.
#[path = "component_meta_audit/resolver_coverage_cross_package_wildcard_reexport.rs"]
mod resolver_coverage_cross_package_wildcard_reexport;
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

// Lib parity tests. Exercise the `MaterializeSurface` variant against
// ambient-lib and userland mapped types, plus userland-shadowing-pick.
// Run via `build_hermetic_host_with_lib`.
#[path = "component_meta_audit/lib_parity.rs"]
mod lib_parity;

// Field-level fast-path counterfixtures. Asserts the fast-path's two
// sub-assertions (Expanded-mode dispatch counter, heritage canonical
// not in loaded-files) plus a counterfixture for the predicate's
// negative branch and an owner-edit invalidation regression.
#[path = "component_meta_audit/field_fast_path_counterfixtures.rs"]
mod field_fast_path_counterfixtures;

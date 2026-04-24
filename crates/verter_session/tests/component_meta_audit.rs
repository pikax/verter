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

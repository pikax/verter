use std::{fs, path::PathBuf};
// Shared denylist consumed by both `audit_no_hot_loop_instrumentation`
// (defined below) and the focused regression test in
// `tests/cases/g_compile/compile_audit_no_hot_loop_instrumentation.rs`.
// Each entry module intentionally gets its own copy of this stateless
// denylist helper (no statics/atomics/OnceCell), so the per-entry scopes
// stay disjoint and share no state. The "duplicate mod" the lint reports
// is the intended layout, not an accident — keep the allow at every site.
#[allow(clippy::duplicate_mod)]
#[path = "../support/audit_hot_loop_denylist.rs"]
mod audit_hot_loop_denylist;

mod cache;
mod capabilities;
mod dependencies;
mod lifecycle;
mod mappings;
mod production_features;

use cache::component_meta_scope_shadowing_memo;
#[path = "foundations/mod.rs"]
pub(crate) mod foundations_guards;
mod support;
use support::*;

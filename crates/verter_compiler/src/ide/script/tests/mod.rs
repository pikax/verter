//! IDE TSX script-generation test cohort (Phase 11d test sibling root).
//!
//! `tests/common.rs` hosts the shared `gen_tsx_script*` and `gen_jsx_script`
//! helpers; the per-cohort sibling files alongside this `mod.rs` resolve
//! both the helpers and the production-side symbols via `use super::*;`
//! because the helpers are re-exported into the `tests` module scope
//! below, and the parent `script` module re-export brings in every
//! sibling-internal symbol the tests assert against (e.g.,
//! `IdeScriptOptions`, `VERTER_TYPES_AMBIENT_MODULE`, the
//! `resolve_all_prop_refs_in_expr` test-only re-export).

// Re-export the production-module surface so each cohort's
// `use super::*;` picks up `IdeScriptOptions`, `BindingType`,
// `FxHashMap`, the @verter/types constants, etc.
pub(super) use super::*;
pub(super) use crate::code_transform::CodeTransform;
pub(super) use crate::ide::CssModuleInfo;

mod common;
pub(super) use common::{
    gen_jsx_script, gen_tsx_script, gen_tsx_script_full, gen_tsx_script_full_with_options,
    gen_tsx_script_full_with_opts, gen_tsx_script_narrowing,
};

mod comp_emit_tests;
mod integration_tests;
mod macros_tests;
mod options_api_tests;
mod setup_tests;
mod template_ref_tests;
mod wrapper_tests;

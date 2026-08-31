//! Single consolidated integration-test binary for `verter_lsp`.
//!
//! Every former top-level `tests/<entry>.rs` integration target now lives as a
//! private submodule under [`cases`], so Cargo compiles them into one binary.
//! Canonical nextest execution still starts one process per `#[test]`; process-
//! local reuse therefore exists only inside an aggregate table/chunk test. Each
//! former entry stays its own module so per-entry helpers and `include!`d data
//! tables keep disjoint scopes. The `VERTER_LSP_AUDIT_TRACE_OUT` trace-out test,
//! which mutates a process-global env var, stays in the separate
//! `lsp_audit_trace_out_env_var` binary so its single test runs in its own
//! process.

#[macro_use]
extern crate verter_debug_assert;

mod cases;

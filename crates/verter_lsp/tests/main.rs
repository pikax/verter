//! Single consolidated integration-test binary for `verter_lsp`.
//!
//! Every former top-level `tests/<entry>.rs` integration target now
//! lives as a private submodule under [`cases`]; this binary compiles
//! and runs them all in one process. Each former entry stays its own
//! module so per-entry helpers and `include!`d data tables keep
//! disjoint scopes and no statics are shared across formerly-separate
//! targets. The `VERTER_LSP_AUDIT_TRACE_OUT` trace-out test, which
//! mutates a process-global env var, stays in the separate
//! `lsp_audit_trace_out_env_var` binary so its single test runs in its
//! own process and cannot leak the env into this co-resident pool.

mod cases;

//! Single consolidated integration-test binary for `verter_workspace`.
//!
//! Every former top-level `tests/<entry>.rs` integration target now lives
//! as a private submodule under [`cases`]; this binary compiles and runs
//! them all in one process. Each former entry stays its own module so any
//! per-entry helpers keep disjoint scopes and no statics are shared across
//! formerly-separate targets.

mod cases;

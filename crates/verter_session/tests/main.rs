//! Single consolidated integration-test binary for `verter_session`.
//!
//! Every former top-level `tests/<entry>.rs` integration target now
//! lives as a private submodule under [`cases`]; this binary compiles
//! and runs them all in one process. Each former entry stays its own
//! module so per-entry helper modules (`mod harness;`, `include!`d
//! data tables) keep disjoint scopes and no statics are shared across
//! formerly-separate targets. The allocation canaries, which require a
//! dedicated `#[global_allocator]`, stay in the separate
//! `allocator_canaries` binary.

mod cases;

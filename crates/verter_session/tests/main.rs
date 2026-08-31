//! Single consolidated integration-test binary for `verter_session`.
//!
//! Every former top-level `tests/<entry>.rs` integration target now lives as a
//! private submodule under [`cases`], so Cargo compiles them into one binary.
//! Canonical nextest execution still starts one process per `#[test]`; process-
//! local reuse therefore exists only inside an aggregate table/chunk test. Each
//! former entry stays its own module so per-entry helper modules (`mod
//! harness;`, `include!`d data tables) keep disjoint scopes. The allocation
//! canaries, which require a dedicated `#[global_allocator]`, stay in the
//! separate `allocator_canaries` binary.

#[macro_use]
extern crate verter_debug_assert;

mod cases;

//! Consolidated integration-test target for `verter_source_policy_gate`.
//!
//! Canonical Surface 1 uses Nextest, so separate `#[test]` entries still run in
//! separate processes even though they compile into this one target. The
//! scan-heavy production policies therefore dispatch from one aggregate test:
//! it walks and reads workspace production sources once, caches the shared
//! residual/whole-env inventories and hot-path fact model, and retains
//! policy-level parallelism inside that process. Rules with distinct fact
//! shapes derive those facts from the same immutable source bytes. Cheap
//! synthetic discriminators and typed/generated-surface checks remain granular.
//!
//! These guards are independent of `debug_assertions` and do not execute a
//! `VerterHost` or compiled request. Keeping them in this package means Surface
//! 1 executes them, while the package-filtered session surfaces do not replay
//! the same repository-policy work.

mod cases;

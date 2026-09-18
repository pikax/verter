//! Consolidated integration-test target for `verter_source_policy_gate`.
//!
//! Canonical Surface 1 uses Nextest, so separate `#[test]` entries still run in
//! separate processes even though they compile into this one target.
//! Remaining consumers are schema, portability, known-bug bijection, and
//! compile-time capability witnesses against `verter_session`'s public API.
//! Cheap synthetic discriminators stay granular.
//!
//! These guards are independent of `debug_assertions` and do not execute a
//! `VerterHost` or compiled request. Keeping them in this package means Surface
//! 1 executes them, while the package-filtered session surfaces do not replay
//! the same repository-policy work.

mod cases;

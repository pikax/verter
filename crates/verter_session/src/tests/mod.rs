//! Test-support modules accessible to the integration tests under
//! `crates/verter_session/tests/*.rs`.
//!
//! This module is gated `cfg(any(test, debug_assertions))` at its
//! declaration site in `lib.rs`, so release builds never include it
//! (release sets `debug_assertions = OFF`). The submodules expose
//! reusable harness types that integration tests would otherwise need
//! to copy-paste; routing them through one named module makes the
//! test-only entry points easy to grep and audit.

pub mod audit_tls_harness;
pub mod dispatch_bridges;

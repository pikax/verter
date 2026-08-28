//! Compile-fail driver for the `DisplaySignature` brand seal (P1-03 witness).
//!
//! Cargo's normal `[[test]]` target cannot express "this test passes when it
//! fails to compile"; the mechanical fix is the `trybuild` crate (standard
//! Rust idiom for compile-fail assertions).
//!
//! Every forge vector lives in its OWN fixture — co-located vectors mask each
//! other's diagnostics, so a seal widening could go unnoticed behind an
//! unrelated error in the same file.

// trybuild spawns a full `cargo build` of the fixture crate (linking
// `verter_type_runtime`), which dominates this test's runtime. Every fixture
// here is `#[ignore]`d unless the `compile-fail` feature is on, and that
// feature is NOT wired into the default gate (`node scripts/gate.mjs`) or any
// CI workflow — run it LOCALLY with
// `cargo nextest run -p verter_type_runtime --features compile-fail`. The
// underlying constraint is also enforced structurally by the normal build (a
// private tuple field, an absent `Deserialize` impl, and an absent `Deref`/
// `AsRef<str>` fail the ordinary compile at any violating call site), so this
// fixture is a belt-and-braces assertion, not the sole rail. The
// out-of-default-gate status is recorded in
// `docs/contributing/gate-integrity-ledger.md`.

/// Widening the seal — a `pub` inner field, a derived `Deserialize`, an added
/// `Deref`/`AsRef<str>`, or a forgeable witness — makes a fixture COMPILE and
/// this test FAIL.
#[test]
#[cfg_attr(
    not(feature = "compile-fail"),
    ignore = "run with --features compile-fail"
)]
fn display_signature_brand_is_sealed() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/display_signature_struct_literal.rs");
    t.compile_fail("tests/cases/compile-fail/display_signature_witness_forge.rs");
    t.compile_fail("tests/cases/compile-fail/display_signature_deserialize.rs");
    t.compile_fail("tests/cases/compile-fail/display_signature_not_a_str.rs");
}

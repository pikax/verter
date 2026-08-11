//! Compile-fail driver for the attribution no-semantic-authority seal.
//!
//! Cargo's normal `[[test]]` target cannot express "this test passes when
//! it fails to compile" — a normal integration test that fails to compile
//! breaks the entire test build. `trybuild` is the standard idiom.
//!
//! The in-crate `disabled_tests` module proves the recording macros are
//! free when the feature is off. It CANNOT prove the reader is absent: a
//! test able to observe the absence would itself be the reader. That half
//! is proven here, from outside the crate, against a default build.

// trybuild spawns a full `cargo build` of the fixture crate, so this is
// `#[ignore]`d unless the `compile-fail` feature is on. The `cfg_attr`
// applies the ignore only when the feature is OFF, so enabling it is
// enough: `cargo test -p verter_audit --features compile-fail`.
#[test]
#[cfg_attr(
    not(feature = "compile-fail"),
    ignore = "run with --features compile-fail"
)]
fn attribution_reader_path_is_absent_from_a_default_build() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/cases/compile-fail/attribution_reader_absent.rs");
}

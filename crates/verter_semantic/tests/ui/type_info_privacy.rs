//! C2 gateway privacy rails (charter home:
//! `crates/verter_semantic/tests/ui/type_info_privacy.rs`).
//!
//! The three blocking compile-fail rails of the sealed gateway:
//!
//! * `C2-GAP3-FOREIGN-IMPL` — a foreign crate cannot implement the
//!   sealed observation contract and inject its own semantic source.
//! * `C2-GAP3-PRIVATE-FIELDS` — kernel payload values cannot be minted
//!   or field-written outside `verter_semantic`.
//! * `C2-GAP3-ALTERNATE-ENTRY` — there is no entry into the gateway
//!   beside `TypeInfoCore::attempt`.
//!
//! `C2-GAP3-WILDCARD-DISPATCH` is structural (the kernel's dispatch and
//! proof-table matches carry no `_` arm; adding a variant is a compile
//! error) and is discriminated by the route-removal mutation recipes,
//! not by a fixture here.

#[test]
fn type_info_privacy_rails() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/fixtures/*.rs");
}

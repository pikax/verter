//! Resolver helper visibility is compiler-enforced from outside its owner.
//!
//! Each fixture import names a genuine `pub(crate)` helper at its defining
//! module path. The checked stderr pins the privacy error and source location;
//! an unrelated failure cannot satisfy this harness. `ModuleResolverCore`
//! remains a positive import in the fixture, so losing the public entry also
//! changes the expected diagnostics.

#[test]
fn resolver_core_helpers_are_private_outside_semantic() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/resolver_core_helpers_are_private.rs");
}

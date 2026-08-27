//! `ProjectMembership` has exactly the membership-module and crate-root public
//! paths. The compiler rejects the prohibited private-module path.

#[test]
fn project_membership_old_module_path_is_absent() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile-fail/project_membership_old_path_is_absent.rs");
}

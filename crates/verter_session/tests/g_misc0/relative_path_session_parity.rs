//! Session-side parity test for `id::resolve_external`.
//!
//! Tests both branches: the relative branch (delegated to
//! `verter_workspace::relative_path::join_relative`) AND the absolute /
//! bare branches (handled directly in `id::resolve_external`).

use verter_session::resolve_external;
use verter_workspace::relative_path::join_relative;

#[test]
fn resolve_external_branches_match_expected() {
    // Relative branch: must match join_relative byte-for-byte.
    let relatives = [("/src/Comp.vue", "./types"), ("/src/a/b.vue", "../c/d")];
    for (importer, specifier) in &relatives {
        assert_eq!(
            resolve_external(importer, specifier),
            join_relative(importer, specifier),
            "resolve_external relative branch must delegate to join_relative for ({importer}, {specifier})"
        );
    }

    // Absolute branch: not delegated; verifies session-only behaviour.
    assert_eq!(resolve_external("/anything", "/foo/bar"), "/foo/bar");

    // Bare branch: not delegated.
    assert_eq!(resolve_external("/anything", "vue"), "vue");
}

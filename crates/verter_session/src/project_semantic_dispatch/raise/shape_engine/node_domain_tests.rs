//! Source-shape guard for the root-kind delegation; reads only the sibling
//! module source, so it stays beside the shape engine.

/// DELEGATION GUARD: `project_node_root_kind` (the source the root-kind classifiers
/// read) delegates to the ROOT-ONLY projection (`node_domain::project_root_summary`),
/// NOT the full `fold_node` — so the short-circuit perf win is real, not silently
/// reverted to a whole-tree walk.
///
/// DISCRIMINATING: reverting the body to `fold_node(...).root_kind` makes it
/// reference `fold_node` and drop `project_root_summary`, failing BOTH asserts.
#[test]
fn project_node_root_kind_delegates_to_root_only_projection_not_full_fold() {
    const SRC: &str = include_str!("mod.rs");
    let start = SRC
        .find("fn project_node_root_kind")
        .expect("project_node_root_kind is defined in mod.rs");
    let after = &SRC[start..];
    let end = after.find("\n}").map(|e| e + 2).unwrap_or(after.len());
    let body = &after[..end];
    assert!(
        body.contains("project_root_summary"),
        "project_node_root_kind must delegate to the root-only projection \
         node_domain::project_root_summary; body:\n{body}"
    );
    assert!(
        !body.contains("fold_node"),
        "project_node_root_kind must NOT use the full fold_node (the root-only projection is the \
         short-circuit authority); body:\n{body}"
    );
}

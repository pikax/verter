use super::*;

#[test]
fn verter_session_public_surface_is_minimal() {
    let src = read_workspace_file("crates/verter_session/src/lib.rs");
    let live = extract_top_level_pub_items(&src);
    let snapshot = guard5_snapshot_pub_items();
    let (added, removed) = guard5_drift(&live, snapshot);
    // The guard is a snapshot — additions OR removals require
    // a deliberate edit to `VERTER_SESSION_PUB_SURFACE_SNAPSHOT`.
    // Subsequent bundles (B-C3 / §12.A9) shrink the snapshot
    // deliberately as `pub mod` items are demoted to
    // `pub(crate)` or removed.
    assert!(
        added.is_empty() && removed.is_empty(),
        "Guard 5 (`verter_session_public_surface_is_minimal`) drift:\n\
             added (live but not in snapshot): {added:?}\n\
             removed (in snapshot but not live): {removed:?}\n\n\
             Update `VERTER_SESSION_PUB_SURFACE_SNAPSHOT` deliberately when the surface changes.",
    );
}

#[test]
fn guard5_predicate_extracts_pub_items_correctly() {
    let src = "//! doc\n// pub mod commented_out;\npub mod foo;\npub mod bar;\npub use crate::foo::{A, B};\npub(crate) mod hidden;\nmod private;\npub fn not_a_module() {}\npub mod outer {\n    pub use crate::nested::Inner;\n}\n";
    let items = extract_top_level_pub_items(src);
    assert!(
        items.iter().any(|i| i == "pub mod foo"),
        "guard 5 predicate must extract `pub mod foo`",
    );
    assert!(
        items.iter().any(|i| i == "pub mod bar"),
        "guard 5 predicate must extract `pub mod bar`",
    );
    assert!(
        items.iter().any(|i| i == "pub use crate::foo::{A, B}"),
        "guard 5 predicate must extract `pub use ...` items",
    );
    assert!(
        items.iter().any(|i| i == "pub(crate) mod hidden"),
        "guard 5 predicate must extract `pub(crate) mod` items",
    );
    assert!(
        items.iter().any(|i| i == "pub mod outer"),
        "guard 5 predicate must capture block-form `pub mod outer {{ ... }}` as `pub mod outer`",
    );
    assert!(
        !items.iter().any(|i| i.contains("pub fn ")),
        "guard 5 predicate must NOT extract `pub fn` declarations",
    );
    assert!(
        !items.iter().any(|i| i.contains("commented_out")),
        "guard 5 predicate must NOT extract commented-out items",
    );
    assert!(
        !items.iter().any(|i| i.contains("nested::Inner")),
        "guard 5 predicate must NOT extract items nested inside a block",
    );
}

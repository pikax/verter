use super::*;
use crate::resolver::ProjectResolver;
use crate::workspace_snapshot::{SnapshotGeneration, WorkspaceSnapshot};

fn empty_snapshot(gen: u64) -> Arc<WorkspaceSnapshot> {
    Arc::new(WorkspaceSnapshot {
        owners_memo: Default::default(),
        projects: vec![],
        resolver: ProjectResolver::default(),
        generation: SnapshotGeneration(gen),
    })
}

// ── VFS-only publication ──

#[test]
fn new_vfs_only_has_no_ext() {
    let root = PublishedRoot::new_vfs_only(empty_snapshot(1));
    assert!(root.consumer_ext.is_none());
    assert!(root.ext::<String>().is_none());
    assert_eq!(root.snapshot.generation, SnapshotGeneration(1));
}

// ── Consumer extension ──

#[derive(Debug)]
struct MockLspViews {
    project_count: usize,
}

#[test]
fn with_ext_stores_and_downcasts() {
    let views = MockLspViews { project_count: 3 };
    let root = PublishedRoot::with_ext(empty_snapshot(2), Box::new(views));

    assert!(root.consumer_ext.is_some());
    let ext = root.ext::<MockLspViews>().unwrap();
    assert_eq!(ext.project_count, 3);
}

#[test]
fn ext_downcast_wrong_type_returns_none() {
    let views = MockLspViews { project_count: 1 };
    let root = PublishedRoot::with_ext(empty_snapshot(1), Box::new(views));

    // Wrong type
    assert!(root.ext::<String>().is_none());
}

// ── Arc reuse for view-only rebuilds ──

#[test]
fn arc_snapshot_reuse_for_view_rebuild() {
    let snapshot = empty_snapshot(5);
    let snap_ptr = Arc::as_ptr(&snapshot);

    // First publish: VFS-only
    let root1 = PublishedRoot::new_vfs_only(Arc::clone(&snapshot));

    // Second publish: same snapshot, new views
    let views = MockLspViews { project_count: 2 };
    let root2 = PublishedRoot::with_ext(Arc::clone(&snapshot), Box::new(views));

    // Both roots share the exact same snapshot Arc
    assert!(Arc::ptr_eq(&root1.snapshot, &root2.snapshot));
    assert_eq!(Arc::as_ptr(&root1.snapshot), snap_ptr);

    // But root2 has views and root1 doesn't
    assert!(root1.ext::<MockLspViews>().is_none());
    assert!(root2.ext::<MockLspViews>().is_some());
}

// ── Debug impl ──

#[test]
fn debug_shows_generation_and_has_ext() {
    let root = PublishedRoot::new_vfs_only(empty_snapshot(42));
    let debug = format!("{:?}", root);
    assert!(debug.contains("42"), "debug should show generation");
    assert!(debug.contains("false"), "has_ext should be false");
}

// ── Ownership readiness ──

#[test]
fn new_vfs_only_is_not_ownership_ready() {
    let root = PublishedRoot::new_vfs_only(empty_snapshot(1));
    assert!(
        !root.ownership_ready,
        "bootstrap VFS-only snapshot should not be ownership_ready"
    );
}

#[test]
fn with_ext_is_ownership_ready() {
    let views = MockLspViews { project_count: 1 };
    let root = PublishedRoot::with_ext(empty_snapshot(1), Box::new(views));
    assert!(
        root.ownership_ready,
        "snapshot published via with_ext should be ownership_ready"
    );
}

#[test]
fn debug_shows_ownership_ready() {
    let root = PublishedRoot::new_vfs_only(empty_snapshot(1));
    let debug = format!("{:?}", root);
    assert!(
        debug.contains("ownership_ready"),
        "debug output should include ownership_ready field"
    );
}

// ── Send + Sync ──

#[test]
fn published_root_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PublishedRoot>();
}

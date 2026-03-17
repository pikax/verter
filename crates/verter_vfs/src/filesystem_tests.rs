use super::*;
use crate::changes::WorkspaceChange;
use crate::traits::WorkspaceAccess;
use crate::types::{ExactResolution, FileKind, ResolveRequestKind};

// ── FilesystemWorkspace::read_file with disk fallback ──

#[test]
fn read_file_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.vue");
    std::fs::write(&file_path, "<template>disk content</template>").unwrap();

    let canonical = file_path.to_string_lossy().replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    let content = ws.read_file(&canonical);
    assert_eq!(
        content.as_deref(),
        Some("<template>disk content</template>")
    );
    // Negative: should not be None
    assert!(content.is_some());
}

#[test]
fn read_file_caches_in_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.vue");
    std::fs::write(&file_path, "initial content").unwrap();

    let canonical = file_path.to_string_lossy().replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    // First read — from disk
    let content1 = ws.read_file(&canonical);
    assert_eq!(content1.as_deref(), Some("initial content"));

    // Modify on disk — second read should return cached version
    std::fs::write(&file_path, "modified content").unwrap();
    let content2 = ws.read_file(&canonical);
    assert_eq!(
        content2.as_deref(),
        Some("initial content"),
        "second read should return cached snapshot, not re-read from disk"
    );
}

#[test]
fn read_file_three_layer_priority() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.vue");
    std::fs::write(&file_path, "disk content").unwrap();

    let canonical = file_path.to_string_lossy().replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    // Layer 3: Disk
    let content_disk = ws.read_file(&canonical);
    assert_eq!(content_disk.as_deref(), Some("disk content"));

    // Layer 2: Inject into snapshot (simulates explicit cache update)
    ws.inject_file(canonical.clone(), Arc::from("snapshot content"));
    let content_snap = ws.read_file(&canonical);
    assert_eq!(content_snap.as_deref(), Some("snapshot content"));
    assert_ne!(content_snap.as_deref(), Some("disk content"));

    // Layer 1: Overlay takes highest priority
    ws.apply_changes(vec![WorkspaceChange::OverlaySet {
        canonical_id: canonical.clone(),
        source: Arc::from("overlay content"),
    }]);
    let content_overlay = ws.read_file(&canonical);
    assert_eq!(content_overlay.as_deref(), Some("overlay content"));
    assert_ne!(content_overlay.as_deref(), Some("snapshot content"));
    assert_ne!(content_overlay.as_deref(), Some("disk content"));

    // Clear overlay — reverts to snapshot
    ws.apply_changes(vec![WorkspaceChange::OverlayClear {
        canonical_id: canonical.clone(),
    }]);
    let content_after_clear = ws.read_file(&canonical);
    assert_eq!(content_after_clear.as_deref(), Some("snapshot content"));
}

// ── FilesystemWorkspace::file_exists ──

#[test]
fn file_exists_disk_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("exists.vue");
    std::fs::write(&file_path, "content").unwrap();

    let canonical = file_path.to_string_lossy().replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    assert!(ws.file_exists(&canonical), "should find file on disk");
    assert!(
        !ws.file_exists("d:/nonexistent/file.vue"),
        "should not find nonexistent file"
    );
}

#[test]
fn file_exists_overlay_only() {
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    ws.apply_changes(vec![WorkspaceChange::OverlaySet {
        canonical_id: "d:/project/virtual.vue".to_string(),
        source: Arc::from("content"),
    }]);

    assert!(
        ws.file_exists("d:/project/virtual.vue"),
        "overlay-only file should exist"
    );
}

// ── FilesystemWorkspace::classify_file ──

#[test]
fn classify_file_types() {
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());
    assert_eq!(ws.classify_file("d:/project/app.vue"), FileKind::VueSfc);
    assert_eq!(ws.classify_file("d:/project/utils.ts"), FileKind::NonSfc);
    assert_ne!(
        ws.classify_file("d:/project/comp.vue"),
        FileKind::NonSfc,
        ".vue should not be NonSfc"
    );
}

// ── FilesystemWorkspace::realpath with disk ──

#[test]
fn realpath_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("real.txt");
    std::fs::write(&file_path, "content").unwrap();

    let canonical = file_path.to_string_lossy().replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    let rp = ws.realpath(&canonical);
    assert!(
        rp.is_some(),
        "realpath should return Some for existing file"
    );
}

#[test]
fn realpath_nonexistent() {
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());
    assert!(
        ws.realpath("d:/nonexistent/file.txt").is_none(),
        "realpath should return None for nonexistent file"
    );
}

// ── FilesystemWorkspace::apply_changes ──

#[test]
fn apply_changes_file_changed_no_source_invalidates_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("changing.vue");
    std::fs::write(&file_path, "original").unwrap();

    let canonical = file_path.to_string_lossy().replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    // Read to cache in snapshot
    let _ = ws.read_file(&canonical);

    // Modify on disk
    std::fs::write(&file_path, "modified on disk").unwrap();

    // FileChanged with source: None → invalidate snapshot, next read re-reads disk
    ws.apply_changes(vec![WorkspaceChange::FileChanged {
        canonical_id: canonical.clone(),
        source: None,
    }]);

    let content = ws.read_file(&canonical);
    assert_eq!(
        content.as_deref(),
        Some("modified on disk"),
        "after FileChanged(None), should re-read from disk"
    );
}

// ── FilesystemWorkspace::set_exact_resolutions ──

#[test]
fn set_exact_resolutions() {
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    let result = ws.set_exact_resolutions(
        "d:/project/src/app.vue",
        vec![ExactResolution {
            specifier: "./utils".to_string(),
            resolved_canonical_id: Some("d:/project/src/utils.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    assert!(result
        .newly_resolved
        .contains(&"d:/project/src/utils.ts".to_string()));

    // Exact resolution should be retrievable
    let resolve = ws.resolve_import(
        "d:/project/src/app.vue",
        "./utils",
        ResolveRequestKind::EsmImport,
    );
    assert!(resolve.is_some());
    assert_eq!(resolve.unwrap().source_id, "d:/project/src/utils.ts");
}

// ── notify_upsert / notify_close lifecycle ──

/// After notify_close, the VFS must NOT serve stale snapshot content
/// from a prior upsert. It should fall back to disk.
#[test]
fn notify_close_invalidates_snapshot_falls_back_to_disk() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("Comp.vue");
    std::fs::write(&file_path, "<template>disk v1</template>").unwrap();
    let canonical = file_path.to_string_lossy().replace('\\', "/");

    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    // 1. Read from disk — populates snapshot cache
    let content = ws.read_file(&canonical);
    assert_eq!(content.as_deref(), Some("<template>disk v1</template>"));

    // 2. Simulate editor open with edited content
    ws.notify_upsert(
        &canonical,
        std::sync::Arc::from("<template>edited</template>"),
    );
    let content = ws.read_file(&canonical);
    assert_eq!(
        content.as_deref(),
        Some("<template>edited</template>"),
        "overlay should take priority"
    );

    // 3. Update disk content (simulates save)
    std::fs::write(&file_path, "<template>disk v2</template>").unwrap();

    // 4. Close editor buffer — must clear overlay AND invalidate snapshot
    ws.notify_close(&canonical);

    // 5. Next read must see disk v2, NOT stale snapshot "disk v1"
    let content = ws.read_file(&canonical);
    assert_eq!(
        content.as_deref(),
        Some("<template>disk v2</template>"),
        "after close, must read from disk (not stale snapshot)"
    );
    assert_ne!(
        content.as_deref(),
        Some("<template>disk v1</template>"),
        "stale snapshot must not be served after close"
    );
}

/// After host.remove() → notify_delete(), the file must not be resolvable.
#[test]
fn notify_delete_removes_snapshot_and_edges() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("Gone.vue");
    std::fs::write(&file_path, "<template>exists</template>").unwrap();
    let canonical = file_path.to_string_lossy().replace('\\', "/");

    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    // 1. Load file (populates snapshot)
    ws.notify_upsert(
        &canonical,
        std::sync::Arc::from("<template>exists</template>"),
    );
    assert!(ws.file_exists(&canonical), "file should exist via overlay");

    // 2. Delete from disk
    std::fs::remove_file(&file_path).unwrap();

    // 3. Notify deletion
    ws.notify_delete(&canonical);

    // 4. File must no longer exist
    assert!(
        !ws.file_exists(&canonical),
        "deleted file must not exist via stale snapshot"
    );
    assert!(
        ws.read_file(&canonical).is_none(),
        "deleted file must not be readable"
    );
}

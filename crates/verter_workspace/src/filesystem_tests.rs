use super::*;
use crate::changes::WorkspaceChange;
use crate::traits::WorkspaceAccess;
use crate::types::{
    ExactResolution, FileKind, ResolutionContext, ResolvePhase, ResolveRequestKind,
};

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

#[test]
fn file_exists_does_not_treat_directory_entries_as_files() {
    let dir = tempfile::tempdir().unwrap();
    let child_dir = dir.path().join("nested.vue");
    std::fs::create_dir_all(&child_dir).unwrap();

    let canonical = child_dir.to_string_lossy().replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    assert!(
        !ws.file_exists(&canonical),
        "directory entries must not seed positive file-exists results"
    );
}

#[test]
fn delete_file_invalidates_parent_dir_index_for_removed_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("delete-me.vue");
    std::fs::write(&file_path, "content").unwrap();

    let canonical = file_path.to_string_lossy().replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    assert!(
        ws.file_exists(&canonical),
        "existing file should seed a positive dir-index entry"
    );

    ws.delete_file(&canonical).expect("delete should succeed");

    assert!(
        !ws.file_exists(&canonical),
        "delete_file must invalidate the parent dir index so the removed file is not reported as present"
    );
}

#[test]
fn copy_file_invalidates_parent_dir_index_for_new_destination() {
    let dir = tempfile::tempdir().unwrap();
    let src_path = dir.path().join("source.vue");
    let dst_path = dir.path().join("copied.vue");
    std::fs::write(&src_path, "content").unwrap();

    let src_canonical = src_path.to_string_lossy().replace('\\', "/");
    let dst_canonical = dst_path.to_string_lossy().replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    assert!(
        !ws.file_exists(&dst_canonical),
        "missing destination should seed a negative dir-index entry"
    );

    ws.copy_file(&src_canonical, &dst_canonical)
        .expect("copy should succeed");

    assert!(
        ws.file_exists(&dst_canonical),
        "copy_file must invalidate the destination parent dir index so the copied file becomes visible"
    );
}

#[test]
fn create_dir_all_invalidates_stale_missing_directory_listing() {
    let dir = tempfile::tempdir().unwrap();
    let child_dir = dir.path().join("nested");
    let child_file = child_dir.join("created.vue");

    let canonical = child_file.to_string_lossy().replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    assert!(
        !ws.file_exists(&canonical),
        "missing child path should seed a negative listing for the missing directory"
    );

    ws.create_dir_all(&child_dir.to_string_lossy().replace('\\', "/"))
        .expect("create_dir_all should succeed");
    std::fs::write(&child_file, "content").unwrap();

    assert!(
        ws.file_exists(&canonical),
        "create_dir_all must invalidate the cached empty listing for the newly created directory"
    );
}

#[test]
fn delete_dir_all_invalidates_cached_children_under_removed_directory() {
    let dir = tempfile::tempdir().unwrap();
    let child_dir = dir.path().join("nested");
    let child_file = child_dir.join("removed.vue");
    std::fs::create_dir_all(&child_dir).unwrap();
    std::fs::write(&child_file, "content").unwrap();

    let child_canonical = child_file.to_string_lossy().replace('\\', "/");
    let dir_canonical = child_dir.to_string_lossy().replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    assert!(
        ws.file_exists(&child_canonical),
        "existing child should seed a positive dir-index entry"
    );

    ws.delete_dir_all(&dir_canonical)
        .expect("delete_dir_all should succeed");

    assert!(
        !ws.file_exists(&child_canonical),
        "delete_dir_all must invalidate cached directory membership for removed children"
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
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
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
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
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

#[test]
fn file_exists_seeds_parent_dir_index_for_present_and_missing_siblings() {
    let dir = tempfile::tempdir().unwrap();
    let present_path = dir.path().join("present.vue");
    std::fs::write(&present_path, "<template>ok</template>").unwrap();

    let present = present_path.to_string_lossy().replace('\\', "/");
    let missing = dir
        .path()
        .join("missing.vue")
        .to_string_lossy()
        .replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    assert!(
        ws.file_exists(&present),
        "seed lookup should still report the real on-disk file as present"
    );
    assert!(
        !ws.file_exists(&missing),
        "missing sibling should still report false"
    );
    assert_eq!(
        ws.engine.dir_index.read().file_exists(&present),
        Some(true),
        "dir index should cache present siblings after the first lookup"
    );
    assert_eq!(
        ws.engine.dir_index.read().file_exists(&missing),
        Some(false),
        "dir index should cache missing siblings without probing the file path again"
    );
}

#[test]
fn directory_tree_dirty_forces_dir_index_rescan_on_next_access() {
    let dir = tempfile::tempdir().unwrap();
    let original_path = dir.path().join("original.vue");
    let late_path = dir.path().join("late.vue");
    std::fs::write(&original_path, "<template>original</template>").unwrap();

    let original = original_path.to_string_lossy().replace('\\', "/");
    let late = late_path.to_string_lossy().replace('\\', "/");
    let prefix = dir.path().to_string_lossy().replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    assert!(
        ws.file_exists(&original),
        "initial lookup should seed the directory index"
    );
    assert!(
        !ws.file_exists(&late),
        "late file should be absent before it exists on disk"
    );

    std::fs::write(&late_path, "<template>late</template>").unwrap();

    assert!(
        !ws.file_exists(&late),
        "clean directory index should keep returning the cached negative result until invalidated"
    );

    ws.apply_changes(vec![WorkspaceChange::DirectoryTreeDirty { prefix }]);

    assert!(
        ws.file_exists(&late),
        "directory dirty invalidation should force one rescan so new siblings become visible"
    );
}

#[test]
fn vfs_provenance_tracks_dir_index_hits_refreshes_and_dirty_rescans() {
    let dir = tempfile::tempdir().unwrap();
    let app_path = dir.path().join("App.vue");
    std::fs::write(&app_path, "<template>ok</template>").unwrap();

    let canonical = app_path.to_string_lossy().replace('\\', "/");
    let prefix = dir.path().to_string_lossy().replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    ws.reset_vfs_provenance();

    assert!(
        ws.file_exists(&canonical),
        "first lookup should seed the parent directory listing from disk"
    );
    let after_seed = ws.vfs_provenance_snapshot();
    assert_eq!(
        after_seed.dir_index_refresh_count, 1,
        "the initial directory lookup should refresh once"
    );
    assert_eq!(
        after_seed.native_fs_read_dir_count, 1,
        "refreshing the parent directory should use one read_dir call"
    );
    assert_eq!(
        after_seed.dir_index_hit_count, 0,
        "the first lookup should not be counted as a dir-index hit"
    );

    assert!(
        ws.file_exists(&canonical),
        "second lookup should reuse the indexed directory membership"
    );
    let after_hit = ws.vfs_provenance_snapshot();
    assert_eq!(
        after_hit.dir_index_hit_count, 1,
        "clean indexed directories should satisfy repeat lookups without another disk scan"
    );
    assert_eq!(
        after_hit.dir_index_refresh_count, 1,
        "dir-index hits should not trigger another refresh"
    );

    ws.apply_changes(vec![WorkspaceChange::DirectoryTreeDirty { prefix }]);
    assert!(
        ws.file_exists(&canonical),
        "a dirty directory should rescan and still report the existing file as present"
    );
    let after_dirty = ws.vfs_provenance_snapshot();
    assert_eq!(
        after_dirty.dir_index_dirty_rescan_count, 1,
        "the next lookup after DirectoryTreeDirty should be counted as a dirty rescan"
    );
    assert_eq!(
        after_dirty.dir_index_refresh_count, 2,
        "dirty rescans should refresh the directory exactly once"
    );
    assert_eq!(
        after_dirty.native_fs_read_dir_count, 2,
        "dirty rescans should issue one more read_dir call"
    );
}

#[test]
fn vfs_provenance_tracks_native_read_file_misses_after_stale_positive_index_hits() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("stale.vue");
    std::fs::write(&file_path, "<template>ok</template>").unwrap();

    let canonical = file_path.to_string_lossy().replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    ws.reset_vfs_provenance();
    assert!(
        ws.file_exists(&canonical),
        "the initial lookup should seed a positive dir-index entry"
    );

    std::fs::remove_file(&file_path).unwrap();

    assert!(
        ws.read_file(&canonical).is_none(),
        "read_file should return None after the backing file disappears"
    );
    let snapshot = ws.vfs_provenance_snapshot();
    assert_eq!(
        snapshot.native_fs_read_file_miss_count, 1,
        "stale positive dir-index entries should still count the resulting disk read miss"
    );
    assert_eq!(
        snapshot.dir_index_hit_count, 1,
        "read_file should reuse the stale positive dir-index entry before attempting disk"
    );
}

#[test]
fn dir_index_negative_avoids_disk_read_for_missing_sibling() {
    let dir = tempfile::tempdir().unwrap();
    let present_path = dir.path().join("App.vue");
    std::fs::write(&present_path, "<template>ok</template>").unwrap();

    let missing = dir
        .path()
        .join("package.json")
        .to_string_lossy()
        .replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    ws.reset_vfs_provenance();

    // First read of missing file — seeds the parent directory index
    let content = ws.read_file(&missing);
    assert!(content.is_none(), "missing file should return None");

    let after_seed = ws.vfs_provenance_snapshot();
    assert_eq!(
        after_seed.dir_index_refresh_count, 1,
        "first lookup should refresh the parent directory once"
    );
    assert_eq!(
        after_seed.native_fs_read_file_miss_count, 0,
        "dir-index negative should prevent any disk read attempt"
    );

    // Second read — should be a DirIndex hit (cached negative)
    let content = ws.read_file(&missing);
    assert!(content.is_none(), "missing file should still return None");

    let after_hit = ws.vfs_provenance_snapshot();
    assert_eq!(
        after_hit.dir_index_hit_count, 1,
        "subsequent lookup for the same missing file should be a dir-index hit"
    );
    assert_eq!(
        after_hit.native_fs_read_file_miss_count, 0,
        "dir-index cached negative should never attempt a disk read"
    );
}

#[test]
fn missing_read_file_trace_detail_distinguishes_dir_index_negative_from_generic_miss() {
    let path = "d:/project/package.json";

    assert_eq!(
        vfs_read_file_missing_result_detail(path, true),
        "path=d:/project/package.json layer=dir_index cache=negative bytes=0",
        "indexed negatives must be labeled as dir_index/cache=negative"
    );
    assert_eq!(
        vfs_read_file_missing_result_detail(path, false),
        "path=d:/project/package.json layer=missing cache=miss bytes=0",
        "true uncached misses must keep the generic missing/cache=miss label"
    );
}

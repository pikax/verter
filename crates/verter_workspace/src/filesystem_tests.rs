use super::*;
use crate::changes::WorkspaceChange;
use crate::traits::{WorkspaceAccess, WorkspaceRead};
use crate::types::{ExactResolution, ResolutionContext, ResolvePhase, ResolveRequestKind};

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

#[test]
fn read_file_trace_detail_tracks_actual_layer() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.vue");
    std::fs::write(&file_path, "disk content").unwrap();

    let canonical = file_path.to_string_lossy().replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    let content_disk = ws.read_file(&canonical);
    assert_eq!(content_disk.as_deref(), Some("disk content"));
    assert_eq!(
        ws.take_last_read_file_trace_detail(&canonical).as_deref(),
        Some("layer=disk cache=miss"),
        "cold reads should report a disk miss"
    );

    let content_snapshot = ws.read_file(&canonical);
    assert_eq!(content_snapshot.as_deref(), Some("disk content"));
    assert_eq!(
        ws.take_last_read_file_trace_detail(&canonical).as_deref(),
        Some("layer=snapshot cache=hit"),
        "warm reads should report a snapshot hit"
    );

    ws.apply_changes(vec![WorkspaceChange::OverlaySet {
        canonical_id: canonical.clone(),
        source: Arc::from("overlay content"),
    }]);
    let content_overlay = ws.read_file(&canonical);
    assert_eq!(content_overlay.as_deref(), Some("overlay content"));
    assert_eq!(
        ws.take_last_read_file_trace_detail(&canonical).as_deref(),
        Some("layer=overlay cache=hit"),
        "overlay reads should report overlay hits"
    );
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
    use verter_language::{FileLanguage, ScriptSourceType};
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());
    assert_eq!(ws.classify_file("d:/project/app.vue"), FileLanguage::vue());
    assert_eq!(
        ws.classify_file("d:/project/utils.ts"),
        FileLanguage::script(ScriptSourceType::Ts)
    );
    assert!(
        ws.classify_file("d:/project/comp.vue")
            .is_framework_carrier(),
        ".vue must classify as a framework carrier"
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

#[test]
fn apply_changes_file_deleted_evicts_realpath_memo() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("warm.txt");
    std::fs::write(&file_path, "x").unwrap();
    let canonical = file_path.to_string_lossy().replace('\\', "/");

    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    // Warm the realpath memo against disk.
    let first = ws.realpath(&canonical);
    assert!(first.is_some(), "realpath of an existing file resolves");

    // Remove the file: the memo keeps serving the cached value (proving the
    // cache is live and the deletion is not yet observed).
    std::fs::remove_file(&file_path).unwrap();
    assert_eq!(
        ws.realpath(&canonical),
        first,
        "memo serves the cached value before the change is applied"
    );

    // The authoritative external-change channel must evict the memo entry.
    ws.apply_changes(vec![WorkspaceChange::FileDeleted {
        canonical_id: canonical.clone(),
    }]);
    assert_eq!(
        ws.realpath(&canonical),
        None,
        "FileDeleted via apply_changes evicts the realpath memo so it reflects the deletion"
    );
}

#[test]
fn apply_changes_directory_tree_dirty_evicts_realpath_memo() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("under_dirty.txt");
    std::fs::write(&file_path, "x").unwrap();
    let canonical = file_path.to_string_lossy().replace('\\', "/");
    let parent = canonical.rsplit_once('/').unwrap().0.to_string();

    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    let first = ws.realpath(&canonical);
    assert!(first.is_some());

    std::fs::remove_file(&file_path).unwrap();
    assert_eq!(
        ws.realpath(&canonical),
        first,
        "memo serves the cached value before the change is applied"
    );

    ws.apply_changes(vec![WorkspaceChange::DirectoryTreeDirty { prefix: parent }]);
    assert_eq!(
        ws.realpath(&canonical),
        None,
        "DirectoryTreeDirty via apply_changes evicts the realpath memo for the subtree"
    );
}

/// A symlink-addressed memo entry (key = link path, value = real target) must
/// be evicted when the change arrives under the RESOLVED real path — exactly the
/// pnpm `node_modules/<pkg>` (symlink) vs `.pnpm/<pkg>` (real) split. Gated to
/// Unix: creating directory symlinks on Windows needs elevated privileges and is
/// unreliable on CI.
#[cfg(unix)]
#[test]
fn apply_changes_evicts_symlink_entry_by_resolved_value() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let real_dir = dir.path().join("real");
    std::fs::create_dir(&real_dir).unwrap();
    let real_file = real_dir.join("f.txt");
    std::fs::write(&real_file, "x").unwrap();
    let link_dir = dir.path().join("link");
    symlink(&real_dir, &link_dir).unwrap();

    let link_canonical = link_dir.join("f.txt").to_string_lossy().replace('\\', "/");

    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    // Warm: realpath through the symlink resolves to the real target.
    let resolved = ws
        .realpath(&link_canonical)
        .expect("symlinked path resolves");
    assert!(
        resolved.contains("/real/"),
        "realpath resolves through the symlink to the real target, got {resolved}"
    );
    assert!(
        !resolved.contains("/link/"),
        "the resolved value is the real target, not the link spelling, got {resolved}"
    );

    // Delete the real target and report the change under its REAL path.
    std::fs::remove_file(&real_file).unwrap();
    ws.apply_changes(vec![WorkspaceChange::FileDeleted {
        canonical_id: resolved.clone(),
    }]);

    // The entry keyed by the link path must be evicted because its resolved
    // VALUE fell under the changed prefix — otherwise it serves a stale path.
    assert_eq!(
        ws.realpath(&link_canonical),
        None,
        "an entry whose resolved value is under the changed prefix is evicted"
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

/// DISCRIMINATING regression (RouteDb stale-serve hole 4): a
/// `DirectoryTreeDirty` change is a file-set mutation produced by
/// watcher recovery. Route-surface edge currency
/// (`indexed_surface_is_current`) and known-miss staleness checks in
/// `verter_session` read the workspace `content_generation` epoch to
/// decide whether a cached route is still fresh. If
/// `DirectoryTreeDirty` clears the resolver lazy cache but leaves the
/// epoch un-advanced, those downstream freshness checks serve the
/// pre-recovery (stale) route surface.
///
/// FAILS pre-fix: the `DirectoryTreeDirty` arm did not set
/// `content_changed`, so the batch never bumped `content_generation`
/// (`after == before`). PASSES post-fix: the arm marks the batch as a
/// content mutation and the epoch advances exactly once.
#[test]
fn directory_tree_dirty_advances_content_generation() {
    let dir = tempfile::tempdir().unwrap();
    let prefix = dir.path().to_string_lossy().replace('\\', "/");
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());

    let before = ws.content_generation();
    ws.apply_changes(vec![WorkspaceChange::DirectoryTreeDirty { prefix }]);
    let after = ws.content_generation();

    assert_eq!(
        after,
        before + 1,
        "DirectoryTreeDirty (watcher recovery) is a file-set generation \
         mutation: it must advance content_generation exactly once for the \
         batch so route-surface edge-currency and known-miss staleness \
         checks do not serve stale results after recovery"
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

/// `FilesystemWorkspace::inject_file` is a per-canonical content
/// mutator and must record the canonical in the content-transition
/// ledger — a plain generation bump leaves artifact-only freshness
/// gates comparing against a stale (or 0) per-canonical entry, so
/// retained artifacts built before the injection keep serving as
/// fresh.
#[test]
fn inject_file_records_per_canonical_content_transition() {
    let ws = FilesystemWorkspace::new(FilesystemOptions::default());
    let canonical = "d:/project/src/injected.ts";
    assert_eq!(
        ws.last_content_transition_generation(canonical),
        0,
        "a never-transitioned canonical reports 0",
    );

    ws.inject_file(canonical.to_string(), Arc::from("export const i = 1;"));

    let recorded = ws.last_content_transition_generation(canonical);
    assert!(
        recorded > 0,
        "inject_file must record the canonical's content transition in \
         the ledger — direct snapshot injection is exactly the perimeter \
         the ledger exists to cover",
    );
    assert_eq!(
        recorded,
        ws.content_generation(),
        "the recorded transition is the post-bump generation",
    );
}

/// `FilesystemWorkspace::delete_dir_all` removes an UNKNOWN member set
/// (a recursive disk delete also removes files the snapshot cache never
/// saw), so it must record a SUBTREE transition: every member
/// canonical's `last_content_transition_generation` advances past its
/// pre-delete record. A delete→recreate of an artifact-only member must
/// not serve a retained pre-delete artifact as fresh.
#[test]
fn delete_dir_all_records_subtree_content_transition() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().replace('\\', "/");
    let pkg_dir = format!("{root}/pkg");
    let member = format!("{pkg_dir}/member.ts");
    let outside = format!("{root}/outside.ts");

    let ws = FilesystemWorkspace::new(FilesystemOptions::default());
    std::fs::create_dir_all(dir.path().join("pkg")).unwrap();
    ws.write_file(&member, "export const m = 1;")
        .expect("write member");
    ws.write_file(&outside, "export const o = 1;")
        .expect("write outside");
    let member_gen = ws.last_content_transition_generation(&member);
    let outside_gen = ws.last_content_transition_generation(&outside);
    assert!(
        member_gen > 0,
        "precondition: write_file recorded the member"
    );

    ws.delete_dir_all(&pkg_dir).expect("delete_dir_all");

    assert!(
        ws.last_content_transition_generation(&member) > member_gen,
        "delete_dir_all must advance every member canonical's transition \
         generation — the recursive delete transitions members the engine \
         cannot enumerate, so the subtree prefix is recorded and folded \
         into the per-canonical query",
    );
    assert_eq!(
        ws.last_content_transition_generation(&outside),
        outside_gen,
        "a canonical outside the deleted subtree is untouched",
    );
}

/// Scalar monorepo layout: package-level `paths: { "@/*": ["./src/*"] }` on
/// `packages/icons/tsconfig.json`. An importer under that package must resolve
/// `@/types` → `packages/icons/src/types.ts` via ProjectGraph discovery.
///
/// Regression: an exclude-only root tsconfig used to synthesize monorepo-wide
/// `include` that package leafs inherited, so the wrong package owned the file
/// and `@/*` mapped to the wrong `src/*`.
#[test]
fn monorepo_package_tsconfig_paths_resolve_at_types() {
    // crates/verter_workspace → repo root → sibling vize checkout
    let scalar = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("../vize/tests/_fixtures/_git/scalar");
    let scalar = match scalar.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("skip: scalar fixture not checked out beside the repo");
            return;
        }
    };
    let scalar_str = scalar.to_string_lossy().replace('\\', "/");
    let importer = format!("{scalar_str}/packages/icons/src/components/ScalarIconPersonSimple.vue");
    let expected = format!("{scalar_str}/packages/icons/src/types.ts");
    assert!(
        std::path::Path::new(&expected).is_file(),
        "precondition: {expected} must exist"
    );

    let ws = FilesystemWorkspace::new(FilesystemOptions {
        roots: vec![scalar_str.clone()],
        ..Default::default()
    });
    let graph = ProjectGraph::from_workspace_roots(
        &ws,
        std::slice::from_ref(&scalar_str),
        &crate::vite_config::ViteConfigOptions::default(),
    );
    ws.set_project_graph(graph.graph);

    // Sibling package that only extends the root for `paths` must not own icons.
    let ch_ts = format!("{scalar_str}/packages/code-highlight/tsconfig.json");
    let ch_mem = crate::snapshot_builder::configured_membership_from_raw(
        &format!("{scalar_str}/packages/code-highlight"),
        &crate::config::load_project_membership(&ws, &ch_ts),
        &Default::default(),
    );
    assert!(
        !ch_mem.contains(&crate::CanonicalPath::new(&importer)),
        "code-highlight must not claim icons sources after leaf-local default include"
    );

    let result = ws.resolve_import(
        &importer,
        "@/types",
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::TypeImport,
        },
    );
    let resolved = result.expect("@/types must resolve under package tsconfig paths");
    let got = resolved.source_id.replace('\\', "/");
    assert_eq!(
        got, expected,
        "package-level @/* paths must map @/types to icons/src/types.ts"
    );
}

#[test]
fn package_tsconfig_membership_does_not_claim_sibling_package_files() {
    use crate::resolver::{IdeProjectCompilerOptions, ProjectMembership};
    use crate::snapshot_builder::configured_membership_from_raw;
    use crate::CanonicalPath;

    let root = "/repo/packages/code-highlight";
    // MatchAll (no files/include) → defaults under THIS root only
    let mem = configured_membership_from_raw(
        root,
        &ProjectMembership::MatchAll,
        &IdeProjectCompilerOptions::default(),
    );
    let sibling = CanonicalPath::new("/repo/packages/icons/src/Icon.vue");
    let own = CanonicalPath::new("/repo/packages/code-highlight/src/x.ts");
    assert!(mem.contains(&own), "own package file must be a member");
    assert!(
        !mem.contains(&sibling),
        "sibling package file must NOT be claimed by code-highlight membership"
    );
}

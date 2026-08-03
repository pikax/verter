use super::*;
use crate::changes::WorkspaceChange;
use crate::traits::{WorkspaceAccess, WorkspaceRead};
use crate::types::{ExactResolution, ResolutionContext, ResolvePhase, ResolveRequestKind};

/// The temp directory's REAL path, for filesystem operations.
///
/// `tempfile::tempdir` honours `TMPDIR`, and the platform default is not
/// necessarily a real path: on macOS it is `/var/folders/...`, where `/var`
/// is a symlink to `/private/var`. The resolver reports real paths, so a
/// test that builds its expectation from the un-canonicalized handle passes
/// only on a machine whose `TMPDIR` happens to be canonical already, and
/// fails at its own precondition everywhere else — taking every assertion
/// after it out of the run. Canonicalizing here is also what the repository's
/// cross-platform rule means by "temp paths come from std abstractions".
fn canonical_temp_root(dir: &tempfile::TempDir) -> std::path::PathBuf {
    std::fs::canonicalize(dir.path()).expect("temp dir must canonicalize")
}

/// A filesystem path as the workspace's OWN canonical id.
///
/// `std::fs::canonicalize` resolves symlinks but returns a platform-native
/// spelling: on Windows that is the extended-length verbatim form with an
/// upper-case drive (`\\?\C:\Users\...`), which no resolver output ever
/// byte-matches — the workspace normalizes to `c:/users/...` through
/// `verter_span::path::canonicalize_path`. Resolving symlinks and
/// normalizing the spelling are two different jobs and BOTH are required:
/// a bare `to_string_lossy().replace('\\', "/")` keeps the verbatim prefix
/// and the drive case, and a bare `canonicalize_path` never resolves the
/// symlink. Every temp-derived id in this file goes through here.
fn temp_canonical_id(path: &std::path::Path) -> String {
    crate::resolver::normalize_canonical_id(&path.to_string_lossy())
}

/// Discrimination for [`temp_canonical_id`] on the platform whose spelling
/// the helper exists for. `std::fs::canonicalize` cannot produce a verbatim
/// path on a unix CI runner, so the normalization is exercised against the
/// shape it must survive rather than against whatever the host happens to
/// hand back — otherwise the Windows-only defect is untested everywhere it
/// can actually occur.
#[test]
fn temp_canonical_id_normalizes_the_windows_verbatim_spelling() {
    assert_eq!(
        temp_canonical_id(std::path::Path::new(r"\\?\C:\Users\dev\owner.ts")),
        "c:/Users/dev/owner.ts",
        "a verbatim, upper-case-drive path must reach the workspace's own \
         canonical spelling; leaving either the `\\\\?\\` prefix or the \
         drive case makes every expectation built from it fail to match \
         resolver output on Windows"
    );
    assert_eq!(
        temp_canonical_id(std::path::Path::new("/private/var/folders/t/dep.ts")),
        "/private/var/folders/t/dep.ts",
        "a unix real path must pass through unchanged"
    );
}

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
fn frozen_resolution_revalidation_bypasses_the_shared_file_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = canonical_temp_root(&dir).join("dep.ts");
    std::fs::write(&file_path, "export const value = 1").unwrap();
    let canonical = temp_canonical_id(&file_path);
    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());
    let published = workspace.load_published().expect("published root");

    let recorder = FilesystemResolutionRecorder::new(&workspace, published);
    assert_eq!(
        WorkspaceRead::read_file(&recorder, &canonical).as_deref(),
        Some("export const value = 1")
    );
    let frozen = recorder.freeze();

    std::fs::write(&file_path, "export const value = 2").unwrap();
    assert!(
        !frozen.revalidate(),
        "independent validation must see bytes newer than the shared snapshot cache"
    );
}

#[test]
fn frozen_resolution_revalidation_bypasses_probe_and_directory_caches() {
    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    let existing_path = root.join("existing.ts");
    let appearing_path = root.join("appearing.ts");
    std::fs::write(&existing_path, "export const existing = 1").unwrap();
    let existing = temp_canonical_id(&existing_path);
    let appearing = temp_canonical_id(&appearing_path);
    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());
    let published = workspace.load_published().expect("published root");

    let recorder = FilesystemResolutionRecorder::new(&workspace, published);
    assert_eq!(
        WorkspaceRead::probe_path(&recorder, &existing),
        crate::resolution_currency::PathProbe::File
    );
    assert_eq!(
        WorkspaceRead::probe_path(&recorder, &appearing),
        crate::resolution_currency::PathProbe::Absent
    );
    let frozen = recorder.freeze();

    std::fs::write(&appearing_path, "export const appeared = 1").unwrap();
    assert!(
        !frozen.revalidate(),
        "independent validation must reject a poisoned negative probe and changed directory membership"
    );
}

#[test]
fn frozen_resolution_revalidation_bypasses_the_package_manifest_cache() {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = canonical_temp_root(&dir).join("package.json");
    std::fs::write(&manifest_path, r#"{"name":"pkg","types":"./v1.d.ts"}"#).unwrap();
    let canonical = temp_canonical_id(&manifest_path);
    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());
    let published = workspace.load_published().expect("published root");

    let recorder = FilesystemResolutionRecorder::new(&workspace, published);
    let manifest =
        WorkspaceRead::read_package_manifest(&recorder, &canonical).expect("discovery manifest");
    assert_eq!(manifest.types.as_deref(), Some("./v1.d.ts"));
    let frozen = recorder.freeze();

    std::fs::write(&manifest_path, r#"{"name":"pkg","types":"./v2.d.ts"}"#).unwrap();
    assert!(
        !frozen.revalidate(),
        "independent validation must parse current bytes instead of reusing the package cache"
    );
}

#[cfg(unix)]
#[test]
fn frozen_resolution_revalidation_bypasses_the_realpath_memo() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    let first = root.join("first.ts");
    let second = root.join("second.ts");
    let link = root.join("link.ts");
    std::fs::write(&first, "export const first = 1").unwrap();
    std::fs::write(&second, "export const second = 1").unwrap();
    symlink(&first, &link).unwrap();
    let link_canonical = temp_canonical_id(&link);
    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());
    let published = workspace.load_published().expect("published root");

    let recorder = FilesystemResolutionRecorder::new(&workspace, published);
    let observed = WorkspaceRead::realpath(&recorder, &link_canonical).expect("discovery realpath");
    assert!(observed.ends_with("/first.ts"));
    let frozen = recorder.freeze();

    std::fs::remove_file(&link).unwrap();
    symlink(&second, &link).unwrap();
    assert!(
        !frozen.revalidate(),
        "independent validation must canonicalize live state instead of reusing the realpath memo"
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

/// Shared body for the monorepo package-`paths` regression: an importer
/// under a package carrying `paths: { "@/*": ["./src/*"] }` must resolve
/// `@/types` → that package's `src/types.ts` via ProjectGraph discovery,
/// and a sibling package that only extends the root must NOT claim the
/// importer as a member.
///
/// Regression: an exclude-only root tsconfig used to synthesize
/// monorepo-wide `include` that package leafs inherited, so the wrong
/// package owned the file and `@/*` mapped to the wrong `src/*`.
pub(super) fn assert_monorepo_package_paths_resolve(
    monorepo_root: &std::path::Path,
    importer_rel: &str,
    expected_rel: &str,
    sibling_pkg_rel: &str,
) {
    assert!(
        monorepo_root.is_dir(),
        "monorepo fixture root must exist: {monorepo_root:?}"
    );
    // Normalize through the workspace's own canonical form (drive-case +
    // separator handling) so expected/actual compare in one coordinate
    // system on every platform — std `canonicalize()` yields `\\?\`-prefixed
    // paths on Windows that never byte-match resolver output.
    let root_str =
        verter_span::path::canonicalize_path(&monorepo_root.to_string_lossy()).replace('\\', "/");
    let importer = format!("{root_str}/{importer_rel}");
    let expected = format!("{root_str}/{expected_rel}");
    assert!(
        std::path::Path::new(&expected).is_file(),
        "precondition: {expected} must exist"
    );

    let ws = FilesystemWorkspace::new(FilesystemOptions {
        roots: vec![root_str.clone()],
        ..Default::default()
    });
    let graph = ProjectGraph::from_workspace_roots(
        &ws,
        std::slice::from_ref(&root_str),
        &crate::vite_config::ViteConfigOptions::default(),
    );
    ws.set_project_graph(graph.graph);

    // Sibling package that only extends the root must not own the importer.
    let sibling_ts = format!("{root_str}/{sibling_pkg_rel}/tsconfig.json");
    let sibling_mem = crate::snapshot_builder::configured_membership_from_raw(
        &format!("{root_str}/{sibling_pkg_rel}"),
        &crate::config::load_project_membership(&ws, &sibling_ts),
        &Default::default(),
    );
    assert!(
        !sibling_mem.contains(&crate::CanonicalPath::new(&importer)),
        "sibling package must not claim another package's sources after leaf-local default include"
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
        "package-level @/* paths must map @/types to the owning package's src/types.ts"
    );
}

/// Hermetic monorepo package-`paths` regression over the vendored fixture
/// (`tests/fixtures/pkg-paths`): exclude-only root tsconfig, one package
/// with `paths: { "@/*": ["./src/*"] }`, one sibling package that only
/// extends the root.
#[test]
fn monorepo_package_tsconfig_paths_resolve_at_types() {
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("pkg-paths");
    assert_monorepo_package_paths_resolve(
        &fixture,
        "packages/icons/src/components/Icon.vue",
        "packages/icons/src/types.ts",
        "packages/hl",
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

/// A `paths` alias mapped DIRECTLY onto a carrier FILE makes the resolver
/// probe index candidates UNDER that file (`.../Child.vue/index.ts`), so the
/// filesystem evidence bridge enumerates the file as a directory and the OS
/// answers `NotADirectory`. That answer is a deterministic, stable
/// observation — the typed probe seam already classifies the same errno as
/// `Absent` — so the discovery evidence must stay consistent and the
/// resolution admissible. Classifying it as unstable I/O made every such
/// resolution permanently `ReturnOnly` (`ResolutionRetryExhausted`) on the
/// filesystem backend.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn alias_onto_file_directory_probe_is_stable_admissible_evidence() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp_canonical_id(&canonical_temp_root(&temp));
    std::fs::create_dir_all(temp.path().join("src")).expect("src dir");
    let child = format!("{root}/src/Child.vue");
    std::fs::write(
        temp.path().join("src/Child.vue"),
        "<script setup lang=\"ts\"></script>\n",
    )
    .expect("write child");

    let ws = FilesystemWorkspace::new(FilesystemOptions {
        roots: vec![root.clone()],
        eager_preload: false,
    });
    let mut config = crate::resolver::IdeProjectConfig::new(
        root.clone(),
        root.clone(),
        Some(format!("{root}/tsconfig.json")),
    );
    config.compiler_options = crate::resolver::IdeProjectCompilerOptions {
        base_url: Some(root.clone()),
        paths: vec![("@dep/child".to_string(), vec!["src/Child.vue".to_string()])],
        ..Default::default()
    };
    WorkspaceAccess::configure_resolver(&ws, vec![config]);
    let published = ws.load_published().expect("configured workspace publishes");

    let importer = format!("{root}/src/main.ts");
    let outcome = WorkspaceRead::resolve_import_at_published(
        &ws,
        &published,
        &importer,
        "@dep/child",
        ResolutionContext {
            phase: ResolvePhase::ProviderGraph,
            kind: ResolveRequestKind::EsmImport,
        },
    );

    assert_eq!(
        outcome
            .result()
            .map(|result| result.source_id.replace('\\', "/")),
        Some(child.replace('\\', "/")),
        "the alias must resolve to the carrier file"
    );
    assert!(
        outcome.is_cacheable(),
        "a directory enumeration answered NotADirectory is stable evidence, not unstable I/O; got {:?}",
        outcome.non_admission_reason()
    );
}

// ── The production backend's warm owner-edge reuse ──

/// **The production LSP backend must reuse a warm owner edge.**
///
/// Every other warm-reuse assertion in the suite runs on
/// `MemoryWorkspace`. `FilesystemWorkspace` resolves through the two-pass
/// evidence bridge (a discovery pass over live disk, then a replay
/// against independently re-read frozen evidence), and while BOTH of its
/// readers declared themselves request-local the shared candidate slot
/// was neither read nor written on this backend: every resolution was
/// cold, and every one of them paid two full resolver passes with real
/// `probe_path` / `realpath` syscalls. Nothing in the suite could see
/// that, which is why this test exists at all.
///
/// Pins three things, all on `FilesystemWorkspace`:
///
/// 1. a cold resolution ADMITS and publishes a candidate;
/// 2. the next identical demand REUSES it, with zero resolver misses;
/// 3. the resolved edge is registered in the shared edge store, so
///    reverse-dependency queries see it.
#[test]
fn filesystem_resolution_publishes_and_reuses_a_warm_owner_edge() {
    const CONTEXT: ResolutionContext = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    let owner_path = root.join("owner.ts");
    let dep_path = root.join("dep.ts");
    std::fs::write(&owner_path, "import { value } from './dep'\n").unwrap();
    std::fs::write(&dep_path, "export const value = 1\n").unwrap();
    let owner = temp_canonical_id(&owner_path);
    let dep = temp_canonical_id(&dep_path);

    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());

    let cold = workspace.resolve_import_outcome(&owner, "./dep", CONTEXT);
    assert_eq!(
        cold.result().map(|result| result.source_id.as_str()),
        Some(dep.as_str()),
        "precondition: the dependency must resolve on the filesystem backend"
    );
    assert!(
        cold.trace().published(),
        "a cold filesystem resolution must ADMIT and publish its candidate. \
         A request-local snapshot publishes nothing, so every later demand \
         re-runs both resolver passes with real filesystem syscalls — the \
         production LSP backend paying full cold cost on every import."
    );

    let before = workspace.vfs_provenance_snapshot();
    let warm = workspace.resolve_import_outcome(&owner, "./dep", CONTEXT);
    let after = workspace.vfs_provenance_snapshot();

    assert_eq!(
        warm.result().map(|result| result.source_id.as_str()),
        Some(dep.as_str()),
        "the warm demand must return the same target"
    );
    assert!(
        warm.trace().reused(),
        "the second identical demand must REUSE the published candidate"
    );
    assert_eq!(
        after.import_resolution_cache_miss_count - before.import_resolution_cache_miss_count,
        0,
        "a warm demand must drive the resolver ZERO times. Both passes of \
         the evidence-bridge protocol answer from the candidate slot."
    );

    assert!(
        workspace
            .reverse_deps_for(&dep)
            .iter()
            .any(|dependent| dependent == &owner),
        "an admitted resolution must register its edge in the shared edge \
         store — a request-local snapshot registers none, so lazily \
         resolved (bare-specifier) edges never become reverse-queryable on \
         this backend at all"
    );
}

/// Anti-vacuity control for the counter used above: an UNRELATED cold
/// demand on the same workspace does drive the resolver.
#[test]
fn filesystem_resolution_miss_counter_moves_on_a_cold_demand() {
    const CONTEXT: ResolutionContext = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    let owner_path = root.join("owner.ts");
    let dep_path = root.join("other.ts");
    std::fs::write(&owner_path, "import { value } from './other'\n").unwrap();
    std::fs::write(&dep_path, "export const value = 1\n").unwrap();
    let owner = temp_canonical_id(&owner_path);

    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());
    let before = workspace.vfs_provenance_snapshot();
    let _cold = workspace.resolve_import_outcome(&owner, "./other", CONTEXT);
    let after = workspace.vfs_provenance_snapshot();
    assert!(
        after.import_resolution_cache_miss_count > before.import_resolution_cache_miss_count,
        "the miss counter must move on a cold demand — otherwise the \
         zero-miss assertion above is vacuous"
    );
}

// ── Healing a stale known miss on the production backend ──

/// **A known miss must stop being served once the dependency exists.**
///
/// The failing user story: `import type { X } from "@verter/types"` resolves
/// to a miss, the user runs `npm install`, and the diagnostic never goes
/// away. Nothing invalidates it — `node_modules` is inside VS Code's default
/// `files.watcherExclude`, so no `workspace/didChangeWatchedFiles`
/// notification is produced, no `WorkspaceChange` is applied, and the miss
/// candidate's `Absent` probe fact keeps validating against the captured
/// resolution world for the lifetime of the process.
///
/// Restoring the memo on this backend is what exposed it: before that, every
/// resolution on `FilesystemWorkspace` was cold and re-probed disk, so the
/// miss healed by accident. Clearing the whole resolution memo on every
/// content generation — the mechanism this program removed — healed it by a
/// different accident, at O(workspace) cost.
///
/// The property pinned here is the narrow one: the candidate's OWN witness
/// canonicals are re-read live at the first resolution after a content
/// transition, and a value that moved advances its exact fact so the
/// candidate dies and the demand re-resolves.
#[test]
fn a_stale_known_miss_heals_after_the_next_content_transition() {
    const CONTEXT: ResolutionContext = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    let owner_path = root.join("owner.ts");
    let dep_path = root.join("late.ts");
    let unrelated_path = root.join("unrelated.ts");
    std::fs::write(&owner_path, "import { value } from './late'\n").unwrap();
    let owner = temp_canonical_id(&owner_path);
    let dep = temp_canonical_id(&dep_path);
    let unrelated = temp_canonical_id(&unrelated_path);

    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());

    let miss = workspace.resolve_import_outcome(&owner, "./late", CONTEXT);
    assert!(
        miss.result().is_none(),
        "precondition: './late' must be a known miss before the file exists"
    );
    assert!(
        miss.trace().published(),
        "precondition: the miss must PUBLISH a candidate — with nothing warm \
         there is no stale serve to heal and this case proves nothing"
    );

    // The dependency appears through the channel that produces no event at
    // all: a plain write on disk, exactly as a package manager performs it.
    std::fs::write(&dep_path, "export const value = 1\n").unwrap();

    // No content transition yet: the candidate is still the answer. This is
    // the deliberate bound of the design — evidence is re-read once per
    // content generation, not on every reuse — and stating it here keeps the
    // healing assertion below from silently becoming a per-reuse re-probe.
    let still_stale = workspace.resolve_import_outcome(&owner, "./late", CONTEXT);
    assert!(
        still_stale.result().is_none(),
        "without a content transition the warm miss is still served"
    );

    // The channel a real client DOES produce: the next keystroke in any
    // open document lands as an injected content mutation.
    workspace.inject_file(unrelated.clone(), Arc::from("export const other = 2\n"));

    let healed = workspace.resolve_import_outcome(&owner, "./late", CONTEXT);
    assert_eq!(
        healed.result().map(|result| result.source_id.as_str()),
        Some(dep.as_str()),
        "the first resolution after a content transition must re-read the \
         candidate's own witness canonicals live, observe that the recorded \
         `Absent` probe moved, advance that exact fact, and re-resolve. \
         Serving the miss here is the `npm install` regression: the \
         diagnostic survives until the server restarts."
    );
    assert!(
        !healed.trace().reused(),
        "the healed demand must NOT be a reuse — a reuse returning the new \
         target would mean the assertion above passed for the wrong reason"
    );
}

/// Anti-vacuity control for the case above: the healing path is scoped to a
/// backend that declares it needs re-observation, and it must not re-probe
/// evidence that no content transition touched.
///
/// Without this, `a_stale_known_miss_heals_after_the_next_content_transition`
/// would still pass if the reuse path were changed to re-resolve from
/// scratch on every demand — which would delete the memo the same story
/// depends on.
#[test]
fn steady_state_reuse_performs_no_evidence_reobservation() {
    const CONTEXT: ResolutionContext = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    let owner_path = root.join("owner.ts");
    let dep_path = root.join("dep.ts");
    std::fs::write(&owner_path, "import { value } from './dep'\n").unwrap();
    std::fs::write(&dep_path, "export const value = 1\n").unwrap();
    let owner = temp_canonical_id(&owner_path);

    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());
    let _cold = workspace.resolve_import_outcome(&owner, "./dep", CONTEXT);

    let before = workspace.vfs_provenance_snapshot();
    for _ in 0..4 {
        let warm = workspace.resolve_import_outcome(&owner, "./dep", CONTEXT);
        assert!(
            warm.trace().reused(),
            "every steady-state demand must reuse the published candidate"
        );
    }
    let after = workspace.vfs_provenance_snapshot();
    assert_eq!(
        after.import_resolution_cache_miss_count - before.import_resolution_cache_miss_count,
        0,
        "repeated demands inside one content generation must drive the \
         resolver ZERO times"
    );
    assert_eq!(
        after.dir_index_refresh_count - before.dir_index_refresh_count,
        0,
        "and must rescan ZERO directories: re-observation is scoped to the \
         first demand after a content transition, not to every reuse"
    );
}

/// The realpath half of the same healing rail.
///
/// A symlink can retarget with the typed probe UNCHANGED — `File` before,
/// `File` after — so a probe comparison alone leaves the `NativeFs`
/// realpath memo answering with the old target, and the warm candidate
/// keeps resolving to a file the specifier no longer names. Only
/// successful canonicalizations are memoized, so this costs one extra
/// syscall on present paths and nothing on absent ones.
#[cfg(unix)]
#[test]
fn a_retargeted_symlink_heals_after_the_next_content_transition() {
    use std::os::unix::fs::symlink;

    const CONTEXT: ResolutionContext = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    let owner_path = root.join("owner.ts");
    let first_path = root.join("first.ts");
    let second_path = root.join("second.ts");
    let link_path = root.join("link.ts");
    let unrelated_path = root.join("unrelated.ts");
    std::fs::write(&owner_path, "import { value } from './link'\n").unwrap();
    std::fs::write(&first_path, "export const value = 1\n").unwrap();
    std::fs::write(&second_path, "export const value = 2\n").unwrap();
    symlink(&first_path, &link_path).unwrap();
    let owner = temp_canonical_id(&owner_path);
    let first = temp_canonical_id(&first_path);
    let second = temp_canonical_id(&second_path);
    let unrelated = temp_canonical_id(&unrelated_path);

    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());
    let cold = workspace.resolve_import_outcome(&owner, "./link", CONTEXT);
    assert_eq!(
        cold.result().map(|result| result.source_id.as_str()),
        Some(first.as_str()),
        "precondition: the symlinked specifier must resolve through to its \
         current target"
    );

    // Retarget with no event of any kind, exactly as the miss case does.
    std::fs::remove_file(&link_path).unwrap();
    symlink(&second_path, &link_path).unwrap();

    workspace.inject_file(unrelated.clone(), Arc::from("export const other = 3\n"));

    let healed = workspace.resolve_import_outcome(&owner, "./link", CONTEXT);
    assert_eq!(
        healed.result().map(|result| result.source_id.as_str()),
        Some(second.as_str()),
        "the first resolution after a content transition must re-read the \
         realpath live. The typed probe is `File` on both sides of the \
         retarget, so a probe-only comparison keeps serving {first}"
    );
}

/// **The two evidence ledgers must never be held together.**
///
/// `pending_resolution_refresh` and `evidence_verified_generation` are
/// independent maps under independent `parking_lot` locks, which grant
/// neither reentrancy nor a global order. Selecting targets under
/// pending-then-verified while settling them under verified-then-pending is
/// an ABBA deadlock between two concurrent resolutions — and it presents as
/// the worst possible failure: the request wedges with no CPU burn, no
/// timeout, and no panic, so a test suite simply stops.
///
/// A hang is not a test result, so this drives concurrent resolutions
/// across repeated content transitions and joins them against a DEADLINE:
/// on the inverted order it fails with a named timeout instead of running
/// forever.
#[test]
fn concurrent_resolutions_across_content_transitions_do_not_deadlock() {
    const CONTEXT: ResolutionContext = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };
    const THREADS: usize = 8;
    const ROUNDS: usize = 40;
    const DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);

    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    for index in 0..THREADS {
        std::fs::write(
            root.join(format!("owner{index}.ts")),
            format!("import {{ v }} from './missing{index}'\n"),
        )
        .unwrap();
    }
    let root = temp_canonical_id(&root);

    let workspace = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    let (done_tx, done_rx) = std::sync::mpsc::channel::<usize>();

    let handles: Vec<_> = (0..THREADS)
        .map(|index| {
            let workspace = Arc::clone(&workspace);
            let root = root.clone();
            let done_tx = done_tx.clone();
            std::thread::spawn(move || {
                let owner = format!("{root}/owner{index}.ts");
                let specifier = format!("./missing{index}");
                for round in 0..ROUNDS {
                    // Every round advances the content generation, so every
                    // round takes the re-observation path rather than the
                    // stamped fast exit.
                    workspace.inject_file(
                        format!("{root}/churn{index}_{round}.ts"),
                        Arc::from(format!("export const c = {round};\n")),
                    );
                    let _ = workspace.resolve_import_outcome(&owner, &specifier, CONTEXT);
                }
                let _ = done_tx.send(index);
            })
        })
        .collect();
    drop(done_tx);

    let deadline = std::time::Instant::now() + DEADLINE;
    for completed in 0..THREADS {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(
            done_rx.recv_timeout(remaining).is_ok(),
            "only {completed} of {THREADS} concurrent resolution workers \
             finished within {DEADLINE:?}. The evidence ledgers are being \
             held together in inconsistent order: one site takes pending \
             then verified, another verified then pending, and two workers \
             wedge against each other with no CPU burn and no panic."
        );
    }
    for handle in handles {
        handle.join().expect("no worker may panic");
    }
}

// ── One live evidence rail: what the freeze bridge bypasses, reuse bypasses ──

/// Every PRODUCTION entry a resolution can arrive through.
///
/// The healing tests below are parameterised over this and NOT over the bare
/// workspace reader, because three consecutive review rounds landed an
/// evidence fix on one reader while production used another: the session path
/// (`VerterHost::resolve_for_persistent_state_with_overlay`) enters through
/// `resolve_import_outcome_with_overlay`, which composes an
/// `OverlaySnapshotReader` over the recorder. A test that exercises only
/// `resolve_import_outcome` stays green with that path completely broken.
#[derive(Clone, Copy, Debug)]
enum ResolveEntry {
    /// `WorkspaceRead::resolve_import_outcome`.
    Plain,
    /// `WorkspaceRead::resolve_import_outcome_with_overlay` — the production
    /// session path, through the composed overlay-snapshot reader.
    WithOverlay,
    /// `WorkspaceRead::resolve_import_at_published`.
    AtPublished,
}

impl ResolveEntry {
    const ALL: [Self; 3] = [Self::Plain, Self::WithOverlay, Self::AtPublished];

    /// Whether this entry READS the shared candidate slot.
    ///
    /// `resolve_import_outcome_with_overlay` composes a request-local reader,
    /// and a request-local reader is handed an EMPTY candidate set
    /// (`Engine::resolve_import_outcome_in_published`): its answers are
    /// overlay-effective while the cache key names the underlying population,
    /// so it may neither publish nor reuse. It therefore resolves cold every
    /// time, through the frozen replay's own independent re-reads.
    ///
    /// The healing assertions below hold for it ANYWAY, and that is the
    /// point: the evidence capability is stated by the backend at the Engine
    /// entry, not forwarded by the reader, so a later change that lets this
    /// entry reuse candidates inherits the healing instead of silently losing
    /// it.
    fn reads_candidates(self) -> bool {
        match self {
            Self::Plain | Self::AtPublished => true,
            Self::WithOverlay => false,
        }
    }

    fn resolve(
        self,
        workspace: &FilesystemWorkspace,
        importer_id: &str,
        specifier: &str,
        ctx: ResolutionContext,
    ) -> crate::resolution_currency::ResolutionOutcome {
        match self {
            Self::Plain => {
                WorkspaceRead::resolve_import_outcome(workspace, importer_id, specifier, ctx)
            }
            Self::WithOverlay => WorkspaceRead::resolve_import_outcome_with_overlay(
                workspace,
                &crate::resolution_currency::ResolutionOverlaySnapshot::default(),
                importer_id,
                specifier,
                ctx,
            ),
            Self::AtPublished => {
                let published = workspace
                    .load_published()
                    .expect("a configured workspace has a published root");
                WorkspaceRead::resolve_import_at_published(
                    workspace,
                    &published,
                    importer_id,
                    specifier,
                    ctx,
                )
            }
        }
    }
}

/// **A manifest rewritten on disk must retarget a warm candidate, through
/// every production entry.**
///
/// `frozen_resolution_revalidation_bypasses_the_package_manifest_cache` pins
/// this for the FREEZE half: the frozen replay parses current bytes instead
/// of reusing the package cache. Reuse is held to the same bar, and for the
/// same reason — a `package.json` under `node_modules` is rewritten by every
/// `npm install`, produces no watched-file event, and the parsed manifest
/// cache is refreshed only by an event.
///
/// What it pins, precisely: a candidate that is WARM (the reuse precondition
/// is asserted, not assumed) must, after the manifest's resolution-semantic
/// projection moves, resolve to the NEW target. Both failure directions are
/// defects and both fail this test — serving the pre-rewrite target is the
/// `npm install` staleness, and serving nothing is a total refusal.
///
/// Two independent defects had to be fixed for this to pass, which is why it
/// is one test and not two: the reuse-time re-observation read the manifest
/// through the ordinary cached accessor (so it could only ever confirm the
/// cache), and the admission fold recorded no manifest baseline at all (so
/// even a correct live read had nothing to disagree with, and a first
/// observation never advances a fact).
#[test]
fn a_rewritten_manifest_retargets_a_warm_candidate_through_every_entry() {
    for entry in ResolveEntry::ALL {
        assert_manifest_rewrite_retargets(entry);
    }
}

fn assert_manifest_rewrite_retargets(entry: ResolveEntry) {
    const CONTEXT: ResolutionContext = ResolutionContext {
        phase: ResolvePhase::ProviderGraph,
        kind: ResolveRequestKind::EsmImport,
    };

    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    let package = root.join("node_modules").join("pkg");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(root.join("owner.ts"), "import { value } from 'pkg'\n").unwrap();
    std::fs::write(package.join("v1.d.ts"), "export declare const value: 1\n").unwrap();
    std::fs::write(package.join("v2.d.ts"), "export declare const value: 2\n").unwrap();
    let manifest_path = package.join("package.json");
    std::fs::write(
        &manifest_path,
        r#"{"name":"pkg","typings":"./v1.d.ts","main":"./v1.d.ts"}"#,
    )
    .unwrap();

    let owner = temp_canonical_id(&root.join("owner.ts"));
    let v1 = temp_canonical_id(&package.join("v1.d.ts"));
    let v2 = temp_canonical_id(&package.join("v2.d.ts"));
    let unrelated = temp_canonical_id(&root.join("unrelated.ts"));

    let root_id = temp_canonical_id(&root);
    let workspace = FilesystemWorkspace::new(FilesystemOptions {
        roots: vec![root_id.clone()],
        eager_preload: false,
    });
    WorkspaceAccess::configure_resolver(
        &workspace,
        vec![crate::resolver::IdeProjectConfig::new(
            root_id.clone(),
            root_id.clone(),
            Some(format!("{root_id}/tsconfig.json")),
        )],
    );

    let cold = workspace.resolve_import_outcome(&owner, "pkg", CONTEXT);
    assert_eq!(
        cold.result().map(|result| result.source_id.as_str()),
        Some(v1.as_str()),
        "precondition ({entry:?}): the bare specifier must resolve through \
         the manifest's `types` field"
    );
    assert!(
        cold.trace().published(),
        "precondition ({entry:?}): the cold resolution must PUBLISH a \
         candidate — with nothing warm there is no stale serve to retarget"
    );
    // The candidate is genuinely WARM before the rewrite: without this the
    // post-rewrite assertion could be satisfied by a tree that simply never
    // caches anything, and the test would discriminate nothing a total
    // refusal would not also pass.
    let warm = entry.resolve(&workspace, &owner, "pkg", CONTEXT);
    assert_eq!(
        warm.trace().reused(),
        entry.reads_candidates(),
        "precondition ({entry:?}): a candidate-reading entry must REUSE the \
         published candidate — without a warm serve there is no stale-serve \
         defect and the healing assertion below is vacuous — and a \
         request-local entry must NOT, because it is handed an empty \
         candidate set by contract"
    );

    // The rewrite a package manager performs: new bytes, no event of any kind.
    std::fs::write(
        &manifest_path,
        r#"{"name":"pkg","typings":"./v2.d.ts","main":"./v2.d.ts"}"#,
    )
    .unwrap();
    workspace.inject_file(unrelated, Arc::from("export const other = 1\n"));

    let healed = entry.resolve(&workspace, &owner, "pkg", CONTEXT);
    assert_eq!(
        healed.result().map(|result| result.source_id.as_str()),
        Some(v2.as_str()),
        "({entry:?}) the first resolution after a content transition must \
         re-read the manifest LIVE and observe that its resolution-semantic \
         projection moved. Serving {v1} here is the `npm install` regression \
         with an extra step: the package is installed, the manifest changed, \
         and the editor keeps resolving to the old entry point. Serving \
         nothing is a different failure — a total refusal — and is equally a \
         defect: the import still resolves, just to the new target"
    );
}

/// **A snapshot-resident target deleted from disk must kill its candidate.**
///
/// The shared file snapshot is a read-through byte cache: `read_file` fills
/// it, and only an event empties it. An evidence read exists to detect the
/// changes the event stream missed, so consulting the snapshot inside one can
/// only ever confirm it — the re-observation used to early-return for any
/// snapshot-resident canonical and then answer its probe FROM the snapshot,
/// which reports `File` for a file that is no longer on disk.
///
/// The overlay is deliberately NOT covered by this: an open buffer's content
/// is authoritative state, not a copy of state, and the engine skips
/// overlay-shadowed canonicals before the evidence read is even reached.
#[test]
fn deleting_a_snapshot_resident_target_kills_its_candidate_through_every_entry() {
    for entry in ResolveEntry::ALL {
        assert_snapshot_resident_deletion_kills_candidate(entry);
    }
}

fn assert_snapshot_resident_deletion_kills_candidate(entry: ResolveEntry) {
    const CONTEXT: ResolutionContext = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    let owner_path = root.join("owner.ts");
    let dep_path = root.join("dep.ts");
    let unrelated_path = root.join("unrelated.ts");
    std::fs::write(&owner_path, "import { value } from './dep'\n").unwrap();
    std::fs::write(&dep_path, "export const value = 1\n").unwrap();
    let owner = temp_canonical_id(&owner_path);
    let dep = temp_canonical_id(&dep_path);
    let unrelated = temp_canonical_id(&unrelated_path);

    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());

    // Make the target snapshot-resident the way production does: read it.
    assert_eq!(
        workspace.read_file(&dep).as_deref(),
        Some("export const value = 1\n"),
        "precondition: the target must be readable"
    );
    let cold = workspace.resolve_import_outcome(&owner, "./dep", CONTEXT);
    assert_eq!(
        cold.result().map(|result| result.source_id.as_str()),
        Some(dep.as_str()),
        "precondition: the dependency must resolve"
    );
    assert!(
        cold.trace().published(),
        "precondition: the positive resolution must publish a candidate"
    );
    assert_eq!(
        entry
            .resolve(&workspace, &owner, "./dep", CONTEXT)
            .trace()
            .reused(),
        entry.reads_candidates(),
        "precondition ({entry:?}): a candidate-reading entry must REUSE the \
         candidate before the deletion — otherwise there is no stale serve \
         for the deletion to kill — and a request-local entry must not"
    );

    std::fs::remove_file(&dep_path).unwrap();
    assert_eq!(
        workspace.read_file(&dep).as_deref(),
        Some("export const value = 1\n"),
        "precondition: the deleted file is STILL snapshot-resident — that is \
         exactly the state whose probe must not be answered from the snapshot"
    );

    workspace.inject_file(unrelated, Arc::from("export const other = 1\n"));

    let after = entry.resolve(&workspace, &owner, "./dep", CONTEXT);
    assert_eq!(
        after.result().map(|result| result.source_id.as_str()),
        None,
        "({entry:?}) the first resolution after a content transition must \
         probe the target LIVE. Answering `File` out of the read-through \
         snapshot keeps serving a target that no longer exists — and \
         certifies it as freshly verified while doing so"
    );
}

/// **A first observation advances no fact and republishes no world root.**
///
/// The admitting attempt's own discovery is the FIRST time the world sees
/// most of the canonicals it probed. Recording those values contradicts
/// nothing — there was no recorded value for any witness's meaning to have
/// depended on — so nothing may advance and no captured root may be
/// superseded. Treating the fill as a change fences the request against
/// itself: the world identity moves, every concurrent attempt's capture stops
/// being current, and they all retry for a fill that moved no fact.
///
/// The filled baseline must nevertheless SURVIVE. Without it the family never
/// acquires the baseline that lets a later real change be detected as one, and
/// every generation refills it forever.
#[test]
fn a_first_observation_advances_no_fact_and_republishes_no_world_root() {
    const CONTEXT: ResolutionContext = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    let owner_path = root.join("owner.ts");
    let dep_path = root.join("dep.ts");
    std::fs::write(&owner_path, "import { value } from './dep'\n").unwrap();
    std::fs::write(&dep_path, "export const value = 1\n").unwrap();
    let owner = temp_canonical_id(&owner_path);
    let dep = temp_canonical_id(&dep_path);

    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());
    let base_world = || {
        workspace
            .engine
            .capture_published_resolution_world(
                crate::resolution_currency::ResolutionPopulation::Base,
            )
            .expect("a settled resolution world")
    };
    let facts_before = workspace.engine.current_resolution_fact_generation();
    let world_before = base_world().base.id;

    let cold = workspace.resolve_import_outcome(&owner, "./dep", CONTEXT);
    assert!(
        cold.trace().published(),
        "precondition: the attempt must reach admission and fold its observed \
         values — otherwise no first observation happened at all"
    );

    assert_eq!(
        workspace.engine.current_resolution_fact_generation(),
        facts_before,
        "a first observation must mint NO fact version: it contradicts \
         nothing, so no witness's meaning changed"
    );
    assert_eq!(
        base_world().base.id,
        world_before,
        "and must not republish the world identity: every in-flight attempt's \
         capture would stop being current and retry, for a fill that moved no \
         fact"
    );
    assert_eq!(
        base_world().base.path_probes.get(&dep),
        Some(&crate::resolution_currency::PathProbe::File),
        "the filled baseline must still be RECORDED. A fill that is discarded \
         leaves the family permanently unrecorded, and an unrecorded family \
         can never detect a change: every observation of it is a first \
         observation forever"
    );
}

/// **The cost model, measured.**
///
/// Warm reuse on a backend with no event coverage is not zero-syscall. It is
/// zero-syscall WITHIN a content generation, and O(distinct witness path
/// canonicals) live reads per generation — one live observation per
/// canonical, deduped engine-wide by the verified stamp, not one per reuse
/// and not five per canonical.
///
/// Both halves are asserted because they fail in opposite directions: the
/// per-generation bound catches a re-observation that starts reading through
/// the ordinary cached accessors again (cheap syscalls, wrong answers), and
/// the within-generation zero catches a re-observation that stops being
/// stamped (right answers, unbounded syscalls).
#[test]
fn warm_reuse_costs_one_live_read_per_witness_canonical_per_generation() {
    const CONTEXT: ResolutionContext = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    let owner_path = root.join("owner.ts");
    let dep_path = root.join("dep.ts");
    std::fs::write(&owner_path, "import { value } from './dep'\n").unwrap();
    std::fs::write(&dep_path, "export const value = 1\n").unwrap();
    let owner = temp_canonical_id(&owner_path);

    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());
    let cold = workspace.resolve_import_outcome(&owner, "./dep", CONTEXT);
    assert!(
        cold.trace().published(),
        "precondition: a candidate is warm"
    );

    let before = workspace.vfs_provenance_snapshot();
    for _ in 0..8 {
        let warm = workspace.resolve_import_outcome(&owner, "./dep", CONTEXT);
        assert!(warm.trace().reused(), "precondition: every demand reuses");
    }
    let after = workspace.vfs_provenance_snapshot();
    assert_eq!(
        after.resolution_evidence_live_read_count - before.resolution_evidence_live_read_count,
        0,
        "eight reuses inside ONE content generation must issue ZERO live \
         evidence reads — the stamp is what makes warm reuse warm"
    );

    // One content transition, then one demand: the whole per-generation cost.
    workspace.inject_file(
        temp_canonical_id(&root.join("unrelated.ts")),
        Arc::from("export const other = 1\n"),
    );
    let before = workspace.vfs_provenance_snapshot();
    let warm = workspace.resolve_import_outcome(&owner, "./dep", CONTEXT);
    assert!(
        warm.trace().reused(),
        "an unrelated transition must not cost the candidate"
    );
    let tick_reads = workspace
        .vfs_provenance_snapshot()
        .resolution_evidence_live_read_count
        - before.resolution_evidence_live_read_count;
    let witness_canonicals = 2_u64; // `./dep` probes `dep` and `dep.ts`.
    assert!(
        tick_reads <= witness_canonicals * 2,
        "one content transition must cost at most two live reads per witness \
         path canonical (one `metadata`, plus one `canonicalize` for a path \
         that exists); got {tick_reads} for {witness_canonicals} canonicals. \
         Re-probing through the ordinary accessors after the live read is \
         what made this ~5 per canonical"
    );
    assert!(
        tick_reads > 0,
        "and it must cost SOMETHING — a zero here means the re-observation \
         never ran and every assertion above about healing is vacuous"
    );

    // The second demand in the SAME generation is free again.
    let before = workspace.vfs_provenance_snapshot();
    let _ = workspace.resolve_import_outcome(&owner, "./dep", CONTEXT);
    assert_eq!(
        workspace
            .vfs_provenance_snapshot()
            .resolution_evidence_live_read_count
            - before.resolution_evidence_live_read_count,
        0,
        "the live read is stamped per canonical per generation, so the second \
         demand after the transition re-reads nothing"
    );
}

/// Restore a directory's mode on the way out, so the temp dir can be removed
/// even when an assertion unwinds through the middle of the test.
#[cfg(unix)]
struct RestoreMode(std::path::PathBuf);

#[cfg(unix)]
impl Drop for RestoreMode {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

/// Make `dir` unreadable and return the restore guard, or `None` when this
/// process cannot produce `EACCES` at all (running as root, where mode bits
/// are not enforced).
#[cfg(unix)]
fn deny_directory_access(dir: &std::path::Path, probe: &std::path::Path) -> Option<RestoreMode> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o000)).ok()?;
    let guard = RestoreMode(dir.to_path_buf());
    match std::fs::metadata(probe) {
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Some(guard),
        // root: mode bits are advisory, so no `EACCES` exists to observe on
        // this machine and there is nothing for the test to discriminate.
        _ => None,
    }
}

/// **An `Inaccessible` probe is an observed VALUE, and it kills the
/// candidate.**
///
/// `Inaccessible` is not "could not observe": it is a first-class outcome the
/// resolver already acts on — an observed `Inaccessible` forces
/// `NonAdmissionReason::ResolutionInaccessiblePath`. Dropping it from the
/// evidence read (returning "no observation") leaves the candidate's
/// `PathProbe` fact frozen at its last readable value, so its signature keeps
/// validating and the warm positive is served for the process's lifetime — a
/// target the process can no longer read at all.
///
/// Real triggers: revoked macOS Full Disk Access, a root-owned bind mount
/// under `node_modules`, and (as `Unknown`) `ELOOP`/`EIO` or a Windows sharing
/// violation.
#[cfg(unix)]
#[test]
fn an_inaccessible_target_kills_its_warm_candidate_through_every_entry() {
    for entry in ResolveEntry::ALL {
        assert_inaccessible_target_kills_candidate(entry);
    }
}

#[cfg(unix)]
fn assert_inaccessible_target_kills_candidate(entry: ResolveEntry) {
    const CONTEXT: ResolutionContext = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let owner_path = root.join("owner.ts");
    let dep_path = sub.join("dep.ts");
    std::fs::write(&owner_path, "import { value } from './sub/dep'\n").unwrap();
    std::fs::write(&dep_path, "export const value = 1\n").unwrap();
    let owner = temp_canonical_id(&owner_path);
    let dep = temp_canonical_id(&dep_path);
    let unrelated = temp_canonical_id(&root.join("unrelated.ts"));

    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());
    let cold = workspace.resolve_import_outcome(&owner, "./sub/dep", CONTEXT);
    assert_eq!(
        cold.result().map(|result| result.source_id.as_str()),
        Some(dep.as_str()),
        "precondition ({entry:?}): the dependency must resolve while readable"
    );
    assert!(
        cold.trace().published(),
        "precondition ({entry:?}): the positive resolution must publish a \
         candidate"
    );

    let Some(_restore) = deny_directory_access(&sub, &dep_path) else {
        // No `EACCES` is producible here, so there is no `Inaccessible` probe
        // and nothing this test could discriminate.
        return;
    };
    workspace.inject_file(unrelated, Arc::from("export const other = 1\n"));

    let after = entry.resolve(&workspace, &owner, "./sub/dep", CONTEXT);
    assert_eq!(
        after.result().map(|result| result.source_id.as_str()),
        None,
        "({entry:?}) a target the process can no longer read must not keep \
         being served. Treating the `Inaccessible` probe as 'no observation' \
         leaves the candidate's `PathProbe` fact at its last readable value, \
         so its signature validates forever and the resolver is never \
         re-entered"
    );
}

/// **The cost contract holds for an inaccessible path too: it is STAMPED and
/// DRAINED.**
///
/// The stamp certifies a live READ, not a live SUCCESS. An observation that is
/// dropped instead of folded also never stamps its canonical and never leaves
/// the pending ledger, so every subsequent demand in the same generation
/// re-issues the same failing syscalls — and the ledger's `is_empty()`
/// early-out is defeated process-wide for as long as the entry sits there.
///
/// This is asserted on the ledgers directly rather than on the syscall
/// counter, because the two halves of the fix interact: once the
/// `Inaccessible` probe advances the fact, the candidate correctly DIES, so
/// every later demand re-resolves cold through the freeze bridge and its
/// independent re-reads dominate the counter. The per-generation bound that
/// survives is exactly this one — one live evidence read per canonical, then
/// stamped and drained.
#[cfg(unix)]
#[test]
fn an_inaccessible_target_is_stamped_and_drained_once_per_generation() {
    const CONTEXT: ResolutionContext = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let owner_path = root.join("owner.ts");
    let dep_path = sub.join("dep.ts");
    std::fs::write(&owner_path, "import { value } from './sub/dep'\n").unwrap();
    std::fs::write(&dep_path, "export const value = 1\n").unwrap();
    let owner = temp_canonical_id(&owner_path);
    let dep = temp_canonical_id(&dep_path);

    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());
    assert!(
        workspace
            .resolve_import_outcome(&owner, "./sub/dep", CONTEXT)
            .trace()
            .published(),
        "precondition: a candidate is warm"
    );

    let Some(_restore) = deny_directory_access(&sub, &dep_path) else {
        return;
    };
    // Route the canonical through the pending ledger, which is the channel a
    // per-canonical content transition uses.
    let generation = workspace.engine.bump_content_generation_for(&dep);
    assert!(
        workspace.engine.pending_resolution_refresh_for_test(&dep),
        "precondition: the transition must enqueue the canonical for \
         re-observation"
    );

    let _ = workspace.resolve_import_outcome(&owner, "./sub/dep", CONTEXT);

    assert_eq!(
        workspace.engine.evidence_verified_generation_for_test(&dep),
        Some(generation),
        "an `Inaccessible` live read is a read: it must stamp its canonical \
         at the current generation. Dropping the observation leaves the \
         canonical unstamped, so every demand in this generation re-issues \
         the same failing syscalls"
    );
    assert!(
        !workspace.engine.pending_resolution_refresh_for_test(&dep),
        "and it must leave the pending ledger. An entry that never drains \
         defeats the ledger's `is_empty()` early-out for every resolution in \
         the process, not just this one"
    );
}

/// **A known miss must heal when the target appears — through every
/// production entry.**
///
/// The third healing scenario: `Absent` is a stable, witnessed outcome, so a
/// recorded miss keeps validating until its `PathProbe` fact moves. On a
/// backend with no event coverage nothing moves it but a live re-read, and
/// `node_modules` is exactly the tree no watcher covers.
#[test]
fn a_known_miss_heals_when_the_target_appears_through_every_entry() {
    for entry in ResolveEntry::ALL {
        assert_known_miss_appearance_heals(entry);
    }
}

fn assert_known_miss_appearance_heals(entry: ResolveEntry) {
    const CONTEXT: ResolutionContext = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };

    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    let owner_path = root.join("owner.ts");
    let dep_path = root.join("appears.ts");
    std::fs::write(&owner_path, "import { value } from './appears'\n").unwrap();
    let owner = temp_canonical_id(&owner_path);
    let dep = temp_canonical_id(&dep_path);

    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());
    let miss = workspace.resolve_import_outcome(&owner, "./appears", CONTEXT);
    assert_eq!(
        miss.result().map(|result| result.source_id.clone()),
        None,
        "precondition ({entry:?}): the target does not exist yet"
    );
    assert!(
        miss.trace().published(),
        "precondition ({entry:?}): the miss must be PUBLISHED as a candidate \
         — an unpublished miss is recomputed anyway and heals for free"
    );

    // The appearance a package install produces: new file, no event.
    std::fs::write(&dep_path, "export const value = 1\n").unwrap();
    workspace.inject_file(
        temp_canonical_id(&root.join("unrelated.ts")),
        Arc::from("export const other = 1\n"),
    );

    let healed = entry.resolve(&workspace, &owner, "./appears", CONTEXT);
    assert_eq!(
        healed.result().map(|result| result.source_id.as_str()),
        Some(dep.as_str()),
        "({entry:?}) the first resolution after a content transition must \
         re-probe the recorded `Absent` LIVE. Keeping the miss warm is the \
         `npm install` regression: the package is on disk and the editor \
         still reports an unresolved import"
    );
}

/// **Freeze and refresh must reach the same verdict, per family, on the same
/// input — and where they differ, freeze must be BROADER.**
///
/// They are not one read. They share the `independent_*` cache-bypassing rail
/// and the one `repair_resolution_memos` function; they differ in comparison
/// depth for manifests (freeze compares the whole `PackageManifest` including
/// `version`/`raw`, refresh compares the resolution-semantic fingerprint) and
/// in the non-present short-circuit. Divergence in the OTHER direction —
/// refresh detecting a change freeze misses, or either missing a
/// resolution-semantic change — is the drift class that produced four prior
/// defects, so it is pinned rather than described.
#[test]
fn freeze_and_refresh_agree_per_family_on_the_same_input() {
    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    let dep_path = root.join("dep.ts");
    let manifest_path = root.join("package.json");
    std::fs::write(&dep_path, "export const value = 1\n").unwrap();
    std::fs::write(
        &manifest_path,
        r#"{"name":"pkg","version":"1.0.0","types":"./a.d.ts"}"#,
    )
    .unwrap();
    let dep = temp_canonical_id(&dep_path);
    let manifest = temp_canonical_id(&manifest_path);

    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());

    // What each path believed before the mutation.
    let recorded_probe = workspace.independent_probe_path(&dep);
    let recorded_manifest_parsed = workspace.independent_manifest(&manifest).unwrap();
    let recorded_baseline =
        |probe, manifest_fingerprint| crate::resolution_currency::RecordedResolutionBaseline {
            probe: Some(probe),
            realpath: None,
            manifest: Some(manifest_fingerprint),
        };
    let recorded_manifest_fingerprint = recorded_manifest_parsed
        .as_ref()
        .map(crate::resolution_currency::manifest_fingerprint_of);

    // ── Family: PathProbe, unchanged ──
    let freeze_probe_moved = workspace.independent_probe_path(&dep) != recorded_probe;
    let refresh_probe_moved = recorded_baseline(recorded_probe, recorded_manifest_fingerprint)
        .disagreements(&workspace.observe_live_evidence(&dep).expect("a live read"))
        .probe;
    assert_eq!(
        (freeze_probe_moved, refresh_probe_moved),
        (false, false),
        "an unchanged path must move neither verdict"
    );

    // ── Family: PathProbe, deleted ──
    std::fs::remove_file(&dep_path).unwrap();
    let freeze_probe_moved = workspace.independent_probe_path(&dep) != recorded_probe;
    let refresh_probe_moved = recorded_baseline(recorded_probe, recorded_manifest_fingerprint)
        .disagreements(&workspace.observe_live_evidence(&dep).expect("a live read"))
        .probe;
    assert_eq!(
        (freeze_probe_moved, refresh_probe_moved),
        (true, true),
        "a deleted path must move BOTH verdicts; a `false` on either side is \
         a path that keeps serving a file that no longer exists"
    );

    // ── Family: Manifest, resolution-semantic change ──
    std::fs::write(
        &manifest_path,
        r#"{"name":"pkg","version":"1.0.0","types":"./b.d.ts"}"#,
    )
    .unwrap();
    let live_manifest = workspace.independent_manifest(&manifest).unwrap();
    let freeze_manifest_moved =
        !manifests_equal(live_manifest.as_ref(), recorded_manifest_parsed.as_ref());
    let refresh_manifest_moved = recorded_baseline(recorded_probe, recorded_manifest_fingerprint)
        .disagreements(
            &workspace
                .observe_live_evidence(&manifest)
                .expect("a live read"),
        )
        .manifest;
    assert_eq!(
        (freeze_manifest_moved, refresh_manifest_moved),
        (true, true),
        "a `types` rewrite is resolution-semantic and must move BOTH verdicts"
    );

    // ── Family: Manifest, version-only change — the one recorded divergence ──
    let semantic_baseline = live_manifest.clone();
    let semantic_fingerprint = semantic_baseline
        .as_ref()
        .map(crate::resolution_currency::manifest_fingerprint_of);
    std::fs::write(
        &manifest_path,
        r#"{"name":"pkg","version":"2.0.0","types":"./b.d.ts"}"#,
    )
    .unwrap();
    let live_manifest = workspace.independent_manifest(&manifest).unwrap();
    let freeze_manifest_moved =
        !manifests_equal(live_manifest.as_ref(), semantic_baseline.as_ref());
    let refresh_manifest_moved = recorded_baseline(recorded_probe, semantic_fingerprint)
        .disagreements(
            &workspace
                .observe_live_evidence(&manifest)
                .expect("a live read"),
        )
        .manifest;
    assert_eq!(
        (freeze_manifest_moved, refresh_manifest_moved),
        (true, false),
        "the documented divergence, and its DIRECTION: freeze compares the \
         whole manifest and re-runs its attempt for a `version` bump; refresh \
         compares the resolution-semantic projection and correctly moves no \
         fact. Freeze BROADER can only over-invalidate. The reverse — refresh \
         moving a fact freeze does not — would mean a warm candidate dying \
         for a change no resolution outcome depends on"
    );
}

/// The realpath family, on the platform that has symlinks.
#[cfg(unix)]
#[test]
fn freeze_and_refresh_agree_on_the_realpath_family() {
    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    std::fs::create_dir_all(root.join("v1")).unwrap();
    std::fs::create_dir_all(root.join("v2")).unwrap();
    std::fs::write(root.join("v1").join("dep.ts"), "export const v = 1\n").unwrap();
    std::fs::write(root.join("v2").join("dep.ts"), "export const v = 2\n").unwrap();
    let link = root.join("dep.ts");
    std::os::unix::fs::symlink(root.join("v1").join("dep.ts"), &link).unwrap();
    let link_id = temp_canonical_id(&link);

    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());
    let recorded_realpath = workspace.independent_realpath(&link_id).unwrap();
    let recorded_probe = workspace.independent_probe_path(&link_id);

    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(root.join("v2").join("dep.ts"), &link).unwrap();
    // The freeze bridge reads live; the refresh path must not be answered out
    // of the realpath memo the previous read populated.
    let freeze_realpath_moved =
        workspace.independent_realpath(&link_id).unwrap() != recorded_realpath;
    let refresh_realpath_moved = crate::resolution_currency::RecordedResolutionBaseline {
        probe: Some(recorded_probe),
        realpath: Some(
            recorded_realpath
                .as_deref()
                .map(crate::resolver::normalize_canonical_id),
        ),
        manifest: None,
    }
    .disagreements(
        &workspace
            .observe_live_evidence(&link_id)
            .expect("a live read"),
    )
    .realpath;
    assert_eq!(
        (freeze_realpath_moved, refresh_realpath_moved),
        (true, true),
        "a retargeted symlink keeps the typed probe at `File` in both \
         directions, so only the realpath family can detect it — and BOTH \
         paths must. A `false` on the refresh side is a candidate that keeps \
         resolving through the old store path"
    );
}

/// **Concurrent resolutions must not refuse each other for retry
/// exhaustion.**
///
/// A failed world CAPTURE means "somebody is publishing right now" —
/// transient contention with no mixed-world hazard. Charging it to the same
/// eight-attempt budget that exists for "my captured world was superseded"
/// starves the resolution under concurrency: every attempt lands in an
/// odd-epoch window and the request is refused as `ResolutionRetryExhausted`.
///
/// That refusal is not cosmetic. The LSP's carrier-import closure treats a
/// refused resolution as "the frontier is not live", so a rename issued while
/// other resolutions are in flight silently returns no edits.
#[test]
fn concurrent_resolutions_are_not_refused_for_retry_exhaustion() {
    const CONTEXT: ResolutionContext = ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    };
    const THREADS: usize = 16;
    const ROUNDS: usize = 24;

    let dir = tempfile::tempdir().unwrap();
    let root = canonical_temp_root(&dir);
    for index in 0..THREADS {
        std::fs::write(
            root.join(format!("owner{index}.ts")),
            format!("import {{ value }} from './dep{index}'\n"),
        )
        .unwrap();
        std::fs::write(
            root.join(format!("dep{index}.ts")),
            "export const value = 1\n",
        )
        .unwrap();
    }
    let root_id = temp_canonical_id(&root);
    let workspace = FilesystemWorkspace::new(FilesystemOptions::default());

    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..THREADS)
            .map(|index| {
                let workspace = &workspace;
                let root_id = root_id.as_str();
                scope.spawn(move || {
                    let owner = format!("{root_id}/owner{index}.ts");
                    let specifier = format!("./dep{index}");
                    let mut exhausted = 0_usize;
                    // ONE content transition per worker, then a burst of warm
                    // demands. The transition is what makes the first demand
                    // re-observe its candidate's evidence; the burst is what
                    // makes eight workers do it at once. A publication STORM
                    // is deliberately NOT modelled: a world genuinely
                    // superseded eight times mid-attempt exhausts the
                    // coherence budget by design, which is a different
                    // (correct) behaviour from a resolution losing its budget
                    // to windows the resolver opened for itself.
                    workspace.inject_file(
                        format!("{root_id}/churn{index}.ts"),
                        Arc::from("export const c = 1;\n"),
                    );
                    for _ in 0..ROUNDS {
                        let outcome = workspace.resolve_import_outcome(&owner, &specifier, CONTEXT);
                        if outcome.non_admission_reason()
                            == Some(verter_audit::NonAdmissionReason::ResolutionRetryExhausted)
                        {
                            exhausted += 1;
                        }
                    }
                    exhausted
                })
            })
            .collect();
        let exhausted: usize = handles
            .into_iter()
            .map(|handle| handle.join().expect("no worker may panic"))
            .sum();
        assert_eq!(
            exhausted,
            0,
            "{exhausted} of {} concurrent resolutions were refused for retry \
             exhaustion. Nothing was wrong with any of them: they lost their \
             attempt budget to other workers' publication windows",
            THREADS * ROUNDS
        );
    });
}

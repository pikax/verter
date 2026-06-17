use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;
use rustc_hash::FxHashMap;

use crate::error::{DirEntry, VfsError};
use crate::path_matches_prefix;

/// Native filesystem wrapper — the sole disk-touch boundary.
///
/// ALL `std::fs` calls in `verter_workspace` go through this struct.
/// Gated behind `#[cfg(not(target_arch = "wasm32"))]` — not available in WASM.
/// Used as the disk fallback layer in `FilesystemWorkspace`.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default)]
pub struct NativeFs {
    /// Memoized `realpath` results keyed by the requested canonical id.
    ///
    /// `std::fs::canonicalize` is a per-call disk syscall; the same path is
    /// canonicalized repeatedly during resolution (`is_workspace_owned` /
    /// `is_package_backed` probe it for every dependency). Entries are evicted
    /// through [`NativeFs::invalidate_realpath_under`], driven by the same
    /// dir-index dirty/refresh signal that owns file-change invalidation — so a
    /// changed or removed path never serves a stale canonicalization.
    ///
    /// Both keys and stored values are normalized through the single
    /// canonical-path owner, so drive-case (`D:` vs `d:`) and `\\?\` spellings
    /// cannot split or miss entries on lookup, insert, or invalidation.
    realpath_memo: RwLock<FxHashMap<String, String>>,
    /// Invalidation generation. Bumped (under the memo write lock) by every
    /// [`NativeFs::invalidate_realpath_under`]. A `realpath` miss snapshots this
    /// before canonicalizing without holding a lock, then commits its result
    /// only if the generation is unchanged — so an invalidation that lands
    /// during the lock-free canonicalize window is never clobbered by the
    /// in-flight (now potentially stale) computation.
    realpath_epoch: AtomicU64,
}

#[cfg(not(target_arch = "wasm32"))]
impl NativeFs {
    pub fn new() -> Self {
        Self::default()
    }

    // ── Read operations ──

    /// Read a file from disk. Returns `None` if the file doesn't exist or
    /// can't be read.
    pub fn read_file(&self, path: &str) -> Option<Arc<str>> {
        let os_path = to_os_path(path);
        std::fs::read_to_string(&os_path).ok().map(Arc::from)
    }

    /// Check if a file exists on disk.
    pub fn file_exists(&self, path: &str) -> bool {
        let os_path = to_os_path(path);
        std::path::Path::new(&os_path).exists()
    }

    /// Resolve symlinks to real path.
    ///
    /// Memoized: a hit returns the cached canonicalization without touching
    /// disk; a miss canonicalizes once and records the result. Only successful
    /// canonicalizations are cached — a path that fails to resolve is retried
    /// on the next call, exactly as a bare `std::fs::canonicalize` would.
    /// Stale entries are dropped via [`NativeFs::invalidate_realpath_under`].
    ///
    /// The lookup key is the normalized canonical id (same normalization the
    /// stored value and invalidation prefix use), so equivalent spellings of
    /// one path share a single entry. The disk syscall runs with NO lock held;
    /// the result is committed only if no invalidation landed meanwhile (see
    /// `realpath_epoch`).
    pub fn realpath(&self, path: &str) -> Option<String> {
        let key = normalize_path_str(path);
        if let Some(resolved) = self.realpath_memo.read().get(&key) {
            return Some(resolved.clone());
        }
        // `Relaxed` is sound here: this is only a snapshot of the epoch to
        // compare later. The authoritative recheck happens in `commit_realpath`
        // UNDER the memo write lock — that lock is the real synchronization rail,
        // so this read carries no ordering obligation. Do not move the decisive
        // epoch comparison out from under that write lock.
        let epoch_before = self.realpath_epoch.load(Ordering::Relaxed);
        let os_path = to_os_path(&key);
        let resolved = std::fs::canonicalize(&os_path)
            .ok()
            .map(|p| normalize_path_str(&p.to_string_lossy()))?;
        self.commit_realpath(key, &resolved, epoch_before);
        Some(resolved)
    }

    /// Commit a freshly-canonicalized `resolved` for `key` only if no
    /// invalidation has advanced the epoch since `epoch_before` was snapshotted.
    ///
    /// Both the epoch comparison and the insert happen under the memo write
    /// lock, which serializes against [`NativeFs::invalidate_realpath_under`]'s
    /// bump+evict; a stale computation whose epoch moved is silently dropped
    /// rather than poisoning the memo.
    fn commit_realpath(&self, key: String, resolved: &str, epoch_before: u64) {
        let mut memo = self.realpath_memo.write();
        // `Relaxed` suffices: the held memo write lock — not atomic ordering — is
        // the synchronization rail. It serializes this compare+insert against the
        // bump+evict in `invalidate_realpath_under` (which advances the epoch and
        // retains under the SAME lock), so an epoch move is always observed here.
        if self.realpath_epoch.load(Ordering::Relaxed) == epoch_before {
            memo.insert(key, resolved.to_string());
        }
    }

    /// Evict memoized `realpath` results for `prefix` and every descendant path.
    ///
    /// Wired to the dir-index dirty/refresh signal: when a structural change
    /// marks a directory dirty, the cached canonicalizations for that subtree
    /// are dropped so the next `realpath` re-resolves against the live disk.
    /// VFS / the dir-index remains the authority for change invalidation; this
    /// is purely the realpath memo's view of that same signal.
    ///
    /// An entry is dropped when EITHER its requested key OR its resolved value
    /// falls under `prefix`: a symlink-addressed entry (`/proj/link/f` ->
    /// `/real/dir/f`) must be evicted whether the change arrives under the link
    /// name or under the real target. The prefix is normalized to match the
    /// memo's normalized keys/values. The epoch bump and the eviction happen
    /// together under the write lock so an in-flight `realpath` cannot commit a
    /// stale result past this point.
    pub fn invalidate_realpath_under(&self, prefix: &str) {
        let prefix = normalize_path_str(prefix);
        let mut memo = self.realpath_memo.write();
        // `Relaxed` suffices: the held memo write lock is the synchronization
        // rail. The bump and the retain below run together under this lock, and
        // `commit_realpath` rechecks the epoch under the SAME lock, so an
        // in-flight realpath cannot commit a stale result past this point.
        self.realpath_epoch.fetch_add(1, Ordering::Relaxed);
        memo.retain(|key, value| {
            !path_matches_prefix(key, &prefix) && !path_matches_prefix(value, &prefix)
        });
    }

    /// Test-only seam: run the `realpath` miss path but invoke `hook` AFTER the
    /// lock-free canonicalize and BEFORE the commit, so a test can deterministically
    /// land an invalidation inside the race window and assert the stale result is
    /// refused admission. Shares the real `commit_realpath` gate.
    #[cfg(test)]
    pub(crate) fn realpath_committing_after(
        &self,
        path: &str,
        hook: impl FnOnce(&Self),
    ) -> Option<String> {
        let key = normalize_path_str(path);
        let epoch_before = self.realpath_epoch.load(Ordering::Relaxed);
        let os_path = to_os_path(&key);
        let resolved = std::fs::canonicalize(&os_path)
            .ok()
            .map(|p| normalize_path_str(&p.to_string_lossy()))?;
        hook(self);
        self.commit_realpath(key, &resolved, epoch_before);
        Some(resolved)
    }

    /// Check whether a path is a directory.
    pub fn is_dir(&self, path: &str) -> bool {
        let os_path = to_os_path(path);
        std::path::Path::new(&os_path).is_dir()
    }

    // ── Directory listing ──

    /// List entries in a directory.
    pub fn read_dir(&self, dir: &str) -> Result<Vec<DirEntry>, VfsError> {
        let os_path = to_os_path(dir);
        let os_path = normalize_read_dir_path(&os_path);
        let entries = std::fs::read_dir(os_path.as_ref()).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                VfsError::NotFound(dir.to_string())
            } else {
                VfsError::Io(e)
            }
        })?;

        let mut result = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = normalize_path_str(&entry.path().to_string_lossy());
            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            result.push(DirEntry { path, is_dir });
        }
        Ok(result)
    }

    /// Recursively walk a directory tree, filtering directories and files.
    /// Returns canonical paths of matching files.
    pub fn walk(
        &self,
        root: &str,
        filter_dir: &dyn Fn(&str) -> bool,
        filter_file: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<String>, VfsError> {
        let os_path = to_os_path(root);
        if !std::path::Path::new(&os_path).is_dir() {
            return Err(VfsError::NotFound(root.to_string()));
        }

        let mut result = Vec::new();
        let walker = walkdir::WalkDir::new(&os_path)
            .follow_links(false)
            .into_iter();

        for entry in walker.filter_entry(|e| {
            if e.file_type().is_dir() {
                let path = normalize_path_str(&e.path().to_string_lossy());
                filter_dir(&path)
            } else {
                true
            }
        }) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue, // Skip permission errors, etc.
            };
            if entry.file_type().is_file() {
                let path = normalize_path_str(&entry.path().to_string_lossy());
                if filter_file(&path) {
                    result.push(path);
                }
            }
        }
        Ok(result)
    }

    // ── Write operations ──

    /// Write content to a file, creating parent directories as needed.
    pub fn write_file(&self, path: &str, content: &str) -> Result<(), VfsError> {
        let os_path = to_os_path(path);
        if let Some(parent) = std::path::Path::new(&os_path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&os_path, content)?;
        // A write can replace a symlink at this path with a regular file,
        // changing how it (and anything resolving through it) canonicalizes, so
        // any memo entry keyed by or resolving under it is now stale.
        self.invalidate_realpath_under(path);
        Ok(())
    }

    /// Create a directory and all parent directories.
    pub fn create_dir_all(&self, path: &str) -> Result<(), VfsError> {
        let os_path = to_os_path(path);
        std::fs::create_dir_all(&os_path)?;
        // Creating a directory can supersede a symlink that previously occupied
        // this path, so cached canonicalizations under it are no longer valid.
        self.invalidate_realpath_under(path);
        Ok(())
    }

    /// Delete a file.
    pub fn delete_file(&self, path: &str) -> Result<(), VfsError> {
        let os_path = to_os_path(path);
        std::fs::remove_file(&os_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                VfsError::NotFound(path.to_string())
            } else {
                VfsError::Io(e)
            }
        })?;
        // A removed path must not keep serving a cached canonicalization; this
        // also drops any symlink-keyed entry whose resolved value was this path.
        self.invalidate_realpath_under(path);
        Ok(())
    }

    /// Delete a directory and all its contents.
    pub fn delete_dir_all(&self, path: &str) -> Result<(), VfsError> {
        let os_path = to_os_path(path);
        std::fs::remove_dir_all(&os_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                VfsError::NotFound(path.to_string())
            } else {
                VfsError::Io(e)
            }
        })?;
        // The whole subtree is gone; evict every memo entry under it (by key or
        // by resolved value, covering symlinks that pointed into the subtree).
        self.invalidate_realpath_under(path);
        Ok(())
    }

    /// Copy a file from `src` to `dst`.
    pub fn copy_file(&self, src: &str, dst: &str) -> Result<(), VfsError> {
        let src_os = to_os_path(src);
        let dst_os = to_os_path(dst);
        if let Some(parent) = std::path::Path::new(&dst_os).parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&src_os, &dst_os)?;
        // The copy can replace a symlink at `dst` with a regular file, changing
        // how `dst` canonicalizes; evict any memo entry under it.
        self.invalidate_realpath_under(dst);
        Ok(())
    }
}

/// Convert a canonical ID (forward slashes) to an OS path.
#[cfg(not(target_arch = "wasm32"))]
fn to_os_path(canonical_id: &str) -> String {
    if cfg!(windows) {
        canonical_id.replace('/', "\\")
    } else {
        canonical_id.to_string()
    }
}

/// Normalize an OS path for `read_dir` so bare Windows drive letters read the drive root.
///
/// On Windows, `std::fs::read_dir("D:")` resolves relative to the drive's current working
/// directory, not the drive root. This appends `\` to produce `D:\` which always means
/// the root of drive D.
///
/// On non-Windows platforms this is a no-op.
#[cfg(not(target_arch = "wasm32"))]
fn normalize_read_dir_path(os_path: &str) -> std::borrow::Cow<'_, str> {
    if cfg!(windows)
        && os_path.len() == 2
        && os_path.as_bytes()[0].is_ascii_alphabetic()
        && os_path.as_bytes()[1] == b':'
    {
        std::borrow::Cow::Owned(format!("{os_path}\\"))
    } else {
        std::borrow::Cow::Borrowed(os_path)
    }
}

/// Normalize an OS path string to canonical form.
///
/// Delegates to the single canonical-path owner (`verter_span::path`) so VFS-key
/// ingestion produces exactly the same canonical ID as every other consumer —
/// no divergent second normalizer.
#[cfg(not(target_arch = "wasm32"))]
fn normalize_path_str(path: &str) -> String {
    verter_span::path::canonicalize_path(path)
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    #[test]
    fn normalize_path_str_delegates_to_canonical_owner() {
        // FIX 3 regression: the owner strips `//?/UNC/` BEFORE `//?/`, so a
        // Windows UNC file canonicalizes to `//server/share/...`, NOT
        // `UNC/server/share/...` (the old generic `//?/` strip).
        assert_eq!(
            normalize_path_str("//?/UNC/server/share/f"),
            "//server/share/f"
        );
        // Owner strips a strippable trailing slash; the old impl did not.
        assert_eq!(normalize_path_str("c:/x/y/"), "c:/x/y");
        // Plain-path passthrough is unchanged.
        assert_eq!(normalize_path_str("/a/b/c.ts"), "/a/b/c.ts");
        // Backslash + drive lowering still applied.
        assert_eq!(normalize_path_str("D:\\x\\y"), "d:/x/y");
    }

    #[test]
    fn read_nonexistent_file() {
        let fs = NativeFs::new();
        assert!(fs.read_file("d:/nonexistent/path/to/file.txt").is_none());
    }

    #[test]
    fn file_exists_nonexistent() {
        let fs = NativeFs::new();
        assert!(!fs.file_exists("d:/nonexistent/path/to/file.txt"));
    }

    #[test]
    fn read_existing_file_with_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let canonical = file_path.to_string_lossy().replace('\\', "/");
        let fs = NativeFs::new();

        let content = fs.read_file(&canonical);
        assert_eq!(content.as_deref(), Some("hello world"));
        assert!(fs.file_exists(&canonical));
    }

    #[test]
    fn realpath_nonexistent() {
        let fs = NativeFs::new();
        assert!(fs.realpath("d:/nonexistent/path/to/file.txt").is_none());
    }

    #[test]
    fn realpath_caches_until_invalidated() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("cached.txt");
        std::fs::write(&file_path, "x").unwrap();
        let canonical = file_path.to_string_lossy().replace('\\', "/");

        let fs = NativeFs::new();

        // First call canonicalizes against disk and memoizes the result.
        let first = fs.realpath(&canonical);
        assert!(first.is_some(), "realpath of an existing file resolves");

        // Remove the underlying file. A memoized `realpath` keeps returning the
        // cached canonical path — proving the second call did NOT re-canonicalize
        // (an uncached impl would re-resolve and yield `None`).
        std::fs::remove_file(&file_path).unwrap();
        let cached = fs.realpath(&canonical);
        assert_eq!(
            cached, first,
            "second realpath serves the memoized result, not a fresh canonicalize"
        );

        // Fire the dir-index dirty/refresh invalidation signal for the parent dir.
        let parent = canonical.rsplit_once('/').unwrap().0;
        fs.invalidate_realpath_under(parent);

        // After invalidation the next call re-canonicalizes and reflects the
        // deletion, exactly as `std::fs::canonicalize` would.
        let after = fs.realpath(&canonical);
        assert_eq!(
            after, None,
            "after invalidation realpath re-resolves and reflects the removed file"
        );
    }

    #[test]
    fn realpath_invalidation_is_prefix_scoped() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        let file_a = dir_a.path().join("a.txt");
        let file_b = dir_b.path().join("b.txt");
        std::fs::write(&file_a, "a").unwrap();
        std::fs::write(&file_b, "b").unwrap();
        let canonical_a = file_a.to_string_lossy().replace('\\', "/");
        let canonical_b = file_b.to_string_lossy().replace('\\', "/");

        let fs = NativeFs::new();
        let resolved_a = fs.realpath(&canonical_a);
        let resolved_b = fs.realpath(&canonical_b);
        assert!(resolved_a.is_some() && resolved_b.is_some());

        // Remove both files so a re-canonicalize would yield `None`.
        std::fs::remove_file(&file_a).unwrap();
        std::fs::remove_file(&file_b).unwrap();

        // Invalidate only `dir_a`'s subtree.
        let parent_a = canonical_a.rsplit_once('/').unwrap().0;
        fs.invalidate_realpath_under(parent_a);

        // `dir_a`'s entry was evicted → re-canonicalizes → reflects the deletion.
        assert_eq!(
            fs.realpath(&canonical_a),
            None,
            "the invalidated entry re-resolves against disk"
        );
        // `dir_b`'s entry is untouched → still served from the memo, proving
        // per-prefix precision (an unrelated path is not evicted).
        assert_eq!(
            fs.realpath(&canonical_b),
            resolved_b,
            "an unrelated path's memo entry survives a different prefix's invalidation"
        );
    }

    #[test]
    fn realpath_does_not_commit_stale_result_after_midflight_invalidation() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("racy.txt");
        std::fs::write(&file_path, "x").unwrap();
        let canonical = file_path.to_string_lossy().replace('\\', "/");
        let key = normalize_path_str(&canonical);
        let parent = key.rsplit_once('/').unwrap().0.to_string();

        let fs = NativeFs::new();

        // Drive the miss path with an invalidation landing in the race window:
        // the call snapshots the epoch, canonicalizes, THEN (via the hook) an
        // invalidation bumps the epoch before the commit gate runs.
        let result = fs.realpath_committing_after(&canonical, |fs| {
            fs.invalidate_realpath_under(&parent);
        });
        assert!(
            result.is_some(),
            "the in-flight call still returns its freshly computed value"
        );

        // The epoch moved mid-flight, so the (now potentially stale) result must
        // NOT have been committed — the memo stays empty for this key. Without
        // the epoch gate the insert would land unconditionally and this fails.
        assert!(
            fs.realpath_memo.read().get(&key).is_none(),
            "a result computed before a mid-flight invalidation must not be committed",
        );
    }

    #[test]
    fn delete_file_invalidates_realpath_memo() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("warm.txt");
        std::fs::write(&file_path, "x").unwrap();
        let canonical = file_path.to_string_lossy().replace('\\', "/");

        let fs = NativeFs::new();
        let first = fs.realpath(&canonical);
        assert!(first.is_some(), "realpath of an existing file resolves");

        // Mutate through NativeFs's own API: the memo NativeFs owns must be
        // evicted, otherwise a direct caller reads a stale canonicalization.
        fs.delete_file(&canonical).unwrap();
        assert_eq!(
            fs.realpath(&canonical),
            None,
            "delete_file evicts the realpath memo so it reflects the removal"
        );
    }

    #[test]
    fn delete_dir_all_invalidates_realpath_memo() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let file_path = sub.join("warm.txt");
        std::fs::write(&file_path, "x").unwrap();
        let canonical = file_path.to_string_lossy().replace('\\', "/");
        let sub_canonical = sub.to_string_lossy().replace('\\', "/");

        let fs = NativeFs::new();
        assert!(fs.realpath(&canonical).is_some());

        // Removing the whole subtree must evict the entry keyed under it.
        fs.delete_dir_all(&sub_canonical).unwrap();
        assert_eq!(
            fs.realpath(&canonical),
            None,
            "delete_dir_all evicts memo entries under the removed subtree"
        );
    }

    /// `write_file` can replace a symlink with a regular file; the memo entry
    /// keyed by the link path must be evicted so it stops resolving to the old
    /// target. Unix-gated: directory/file symlinks need elevation on Windows.
    #[cfg(unix)]
    #[test]
    fn write_file_invalidates_realpath_memo_on_symlink_replacement() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        // Canonicalize the tempdir ROOT so the expected values match the memo's
        // `std::fs::canonicalize` output on hosts where the temp root is itself
        // under a symlink (macOS: `/var` -> `/private/var`, `/tmp` -> `/private/tmp`).
        // On Linux this is a no-op (the temp root is already canonical). The leaf
        // paths the test files actually live at are joined onto this canonical
        // root, so every expected path is symlink-agnostic.
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let target = root.join("target.txt");
        std::fs::write(&target, "real").unwrap();
        let link = root.join("link.txt");
        symlink(&target, &link).unwrap();
        let link_canonical = link.to_string_lossy().replace('\\', "/");
        let target_norm = normalize_path_str(&target.to_string_lossy());

        let fs = NativeFs::new();
        // Warm: realpath through the symlink resolves to the real target.
        assert_eq!(
            fs.realpath(&link_canonical).as_deref(),
            Some(target_norm.as_str())
        );

        // Replace the symlink with a regular file at the same path.
        std::fs::remove_file(&link).unwrap();
        fs.write_file(&link_canonical, "now-a-file").unwrap();

        // The entry must have been evicted: realpath now resolves to the path
        // itself, not the stale target. Pre-fix (no invalidation) returns the
        // cached target and this fails.
        assert_eq!(
            fs.realpath(&link_canonical).as_deref(),
            Some(normalize_path_str(&link_canonical).as_str()),
            "write_file evicts the stale symlink-keyed memo entry"
        );
    }

    /// `copy_file` can replace a symlink at the destination with a regular file;
    /// the destination's memo entry must be evicted. Unix-gated for symlinks.
    #[cfg(unix)]
    #[test]
    fn copy_file_invalidates_dst_realpath_memo_on_symlink_replacement() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        // Canonicalize the tempdir ROOT so the expected values match the memo's
        // `std::fs::canonicalize` output on hosts where the temp root is itself
        // under a symlink (macOS: `/var` -> `/private/var`, `/tmp` -> `/private/tmp`).
        // On Linux this is a no-op. Every test path is then joined onto the
        // canonical root, keeping all expected paths symlink-agnostic.
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let target = root.join("target.txt");
        std::fs::write(&target, "real").unwrap();
        let link = root.join("link.txt");
        symlink(&target, &link).unwrap();
        let src = root.join("src.txt");
        std::fs::write(&src, "copied").unwrap();
        let link_canonical = link.to_string_lossy().replace('\\', "/");
        let src_canonical = src.to_string_lossy().replace('\\', "/");
        let target_norm = normalize_path_str(&target.to_string_lossy());

        let fs = NativeFs::new();
        assert_eq!(
            fs.realpath(&link_canonical).as_deref(),
            Some(target_norm.as_str())
        );

        // Replace the symlink destination with a regular file via copy.
        std::fs::remove_file(&link).unwrap();
        fs.copy_file(&src_canonical, &link_canonical).unwrap();

        assert_eq!(
            fs.realpath(&link_canonical).as_deref(),
            Some(normalize_path_str(&link_canonical).as_str()),
            "copy_file evicts the stale destination memo entry"
        );
    }

    #[test]
    fn is_dir_on_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().to_string_lossy().replace('\\', "/");
        let fs = NativeFs::new();
        assert!(fs.is_dir(&canonical));
        assert!(!fs.is_dir(&format!("{canonical}/nonexistent")));
    }

    #[test]
    fn write_and_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("sub").join("test.txt");
        let canonical = file_path.to_string_lossy().replace('\\', "/");
        let fs = NativeFs::new();

        fs.write_file(&canonical, "round trip content").unwrap();
        let content = fs.read_file(&canonical);
        assert_eq!(content.as_deref(), Some("round trip content"));
    }

    #[test]
    fn read_dir_lists_entries() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();

        let canonical = dir.path().to_string_lossy().replace('\\', "/");
        let fs = NativeFs::new();

        let entries = fs.read_dir(&canonical).unwrap();
        assert_eq!(entries.len(), 2);
        let names: Vec<&str> = entries
            .iter()
            .map(|e| e.path.rsplit('/').next().unwrap())
            .collect();
        assert!(names.contains(&"a.txt"));
        assert!(names.contains(&"subdir"));
    }

    #[test]
    fn walk_filters_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.ts"), "a").unwrap();
        std::fs::write(dir.path().join("b.js"), "b").unwrap();
        std::fs::create_dir(dir.path().join("node_modules")).unwrap();
        std::fs::write(dir.path().join("node_modules").join("c.ts"), "c").unwrap();

        let canonical = dir.path().to_string_lossy().replace('\\', "/");
        let fs = NativeFs::new();

        let files = fs
            .walk(
                &canonical,
                &|path| !path.contains("node_modules"),
                &|path| path.ends_with(".ts"),
            )
            .unwrap();

        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("a.ts"));
    }

    #[test]
    fn delete_file_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("del.txt");
        std::fs::write(&file_path, "delete me").unwrap();
        let canonical = file_path.to_string_lossy().replace('\\', "/");
        let fs = NativeFs::new();

        assert!(fs.file_exists(&canonical));
        fs.delete_file(&canonical).unwrap();
        assert!(!fs.file_exists(&canonical));
    }

    // ── normalize_read_dir_path tests ──

    #[test]
    fn normalize_read_dir_path_bare_drive_letter() {
        let result = normalize_read_dir_path("D:");
        if cfg!(windows) {
            assert_eq!(
                result.as_ref(),
                "D:\\",
                "bare drive letter must get trailing backslash on Windows"
            );
        } else {
            assert_eq!(
                result.as_ref(),
                "D:",
                "bare drive letter is a no-op on non-Windows"
            );
        }
    }

    #[test]
    fn normalize_read_dir_path_lowercase_drive() {
        let result = normalize_read_dir_path("d:");
        if cfg!(windows) {
            assert_eq!(
                result.as_ref(),
                "d:\\",
                "lowercase drive letter must also normalize"
            );
        } else {
            assert_eq!(result.as_ref(), "d:");
        }
    }

    #[test]
    fn normalize_read_dir_path_drive_with_trailing_backslash() {
        // Already has a path separator — must be left alone
        let result = normalize_read_dir_path("D:\\");
        assert_eq!(result.as_ref(), "D:\\");
    }

    #[test]
    fn normalize_read_dir_path_regular_path() {
        let result = normalize_read_dir_path("D:\\projects\\verter");
        assert_eq!(result.as_ref(), "D:\\projects\\verter");
    }

    #[test]
    fn normalize_read_dir_path_unix_path_noop() {
        let result = normalize_read_dir_path("/usr/local");
        assert_eq!(result.as_ref(), "/usr/local");
    }

    #[test]
    fn normalize_read_dir_path_empty_noop() {
        let result = normalize_read_dir_path("");
        assert_eq!(result.as_ref(), "");
    }

    #[test]
    fn normalize_read_dir_path_single_char_noop() {
        let result = normalize_read_dir_path("D");
        assert_eq!(result.as_ref(), "D");
    }

    #[test]
    fn copy_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.txt");
        std::fs::write(&src, "copy me").unwrap();
        let src_canonical = src.to_string_lossy().replace('\\', "/");
        let dst_canonical = dir
            .path()
            .join("dst.txt")
            .to_string_lossy()
            .replace('\\', "/");
        let fs = NativeFs::new();

        fs.copy_file(&src_canonical, &dst_canonical).unwrap();
        let content = fs.read_file(&dst_canonical);
        assert_eq!(content.as_deref(), Some("copy me"));
    }
}

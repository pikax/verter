//! Immutable overlay snapshot + synchronous host-callback servicing.
//!
//! The tsgo engine asks the host to answer filesystem callbacks
//! (`readFile`/`fileExists`/`directoryExists`/`getAccessibleEntries`/`realpath`).
//! Those callbacks MUST be answered synchronously and must NEVER call back into
//! the async client or await provider state (the deadlock hazard: the engine is
//! blocked waiting for the callback reply while servicing an in-flight request).
//!
//! This module owns the snapshot type and the pure, synchronous servicing logic.
//! It deliberately does NOT pull in `verter_session` or reinvent a filesystem
//! (Directive 3): the snapshot is a thin VIEW fed FROM the consumer's VFS. The
//! "virtual" (overlay) entries are plain data; the "real" directory entries come
//! from a [`RealDirSource`] the consumer supplies (in S1, an in-memory test
//! source; later, a VFS-backed one). The snapshot is published via an
//! `ArcSwap<OverlaySnapshot>` on the transport side so the callback path reads it
//! lock-free.
//!
//! Wire semantics mirrored from `dist/api/sync/client.js` + `dist/api/fs.js`:
//! - `readFile`: a found file returns its content; a definitively-absent file
//!   returns "not found"; an unknown path falls through to the real FS
//!   (client.js:36-42 maps `undefined → ""` = fall through, `null → {content:null}`
//!   = not found, string → `{content:"…"}`).
//! - `getAccessibleEntries` returns `{ files, directories }` of BASENAMES and
//!   MUST MERGE the overlay entries with the real directory entries
//!   (returning overlay-only would hide real files → false module-resolution
//!   failures). Mirrors fs.js:85-101.

use std::collections::BTreeMap;
use std::sync::Arc;

/// The accessible entries of a directory: basenames split into files and
/// directories. Mirrors the JS `FileSystemEntries` shape (fs.d.ts:1-4,
/// fs.js:100). Entry names are basenames, not full paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessibleEntries {
    /// File basenames in the directory.
    pub files: Vec<String>,
    /// Subdirectory basenames in the directory.
    pub directories: Vec<String>,
}

/// The three-state result of a `readFile` callback, mirroring the wire's
/// `undefined`/`null`/string trichotomy (client.js:36-42).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadFileResult {
    /// The file exists with this content (maps to `{content:"…"}`).
    Found(String),
    /// The file is definitively absent; do NOT fall back (maps to `{content:null}`).
    NotFound,
    /// The path is not part of the overlay; fall through to the real filesystem
    /// (maps to the empty-string sentinel `""`).
    FallThrough,
}

/// A source of REAL directory entries, supplied by the consumer. S1 keeps the
/// crate decoupled from `verter_session` by taking this as a trait: the consumer
/// (S3) backs it with Verter's VFS; tests back it with an in-memory map. The
/// snapshot never reads the real filesystem itself.
pub trait RealDirSource: Send + Sync + std::fmt::Debug {
    /// Return the real directory entries for `dir` (a forward-slash-normalized
    /// absolute path), or `None` if the directory is unknown / does not exist
    /// in the real view.
    fn real_entries(&self, dir: &str) -> Option<AccessibleEntries>;
}

/// A [`RealDirSource`] that reports no real entries. Used when the engine is
/// driven purely from the overlay (the snapshot then answers `getAccessibleEntries`
/// with overlay entries only).
#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyRealDirSource;

impl RealDirSource for EmptyRealDirSource {
    fn real_entries(&self, _dir: &str) -> Option<AccessibleEntries> {
        None
    }
}

/// Normalize a path for overlay comparison: backslashes to forward slashes.
/// The tsgo wire and the gate harness both compare on forward-slash paths
/// (harness.mjs `norm`), so the overlay keys are stored normalized and lookups
/// normalize the incoming path the same way. This is portable across OSes.
fn norm_path(p: &str) -> String {
    p.replace('\\', "/")
}

/// Split a normalized path into (parent-dir, basename).
fn split_parent_basename(p: &str) -> (String, String) {
    let normalized = norm_path(p);
    match normalized.rfind('/') {
        Some(idx) => (
            normalized[..idx].to_string(),
            normalized[idx + 1..].to_string(),
        ),
        None => (String::new(), normalized),
    }
}

/// An immutable overlay snapshot: the set of off-disk (virtual) files and
/// directories the engine should see in addition to the real filesystem.
///
/// Construct via [`OverlaySnapshotBuilder`]. The snapshot is cheap to share
/// (`Arc`-friendly) and is published to the callback path via `ArcSwap` on the
/// transport side. All servicing methods are synchronous and allocation-light.
#[derive(Debug, Clone)]
pub struct OverlaySnapshot {
    /// Overlay file content keyed by normalized absolute path. A present key
    /// with `Some` is a found file; a present key with `None` is a
    /// definitively-absent file (`readFile` → NotFound).
    files: BTreeMap<String, Option<Arc<str>>>,
    /// Overlay directories known to exist (normalized absolute paths).
    directories: std::collections::BTreeSet<String>,
    /// The real directory entry source (consumer-supplied; never the real FS
    /// directly inside this crate).
    real: Arc<dyn RealDirSource>,
}

impl OverlaySnapshot {
    /// Start building an overlay snapshot with no real-dir source (overlay only).
    pub fn builder() -> OverlaySnapshotBuilder {
        OverlaySnapshotBuilder::new()
    }

    /// Service a `readFile` callback. Mirrors client.js:36-42.
    pub fn read_file(&self, path: &str) -> ReadFileResult {
        match self.files.get(&norm_path(path)) {
            Some(Some(content)) => ReadFileResult::Found(content.to_string()),
            Some(None) => ReadFileResult::NotFound,
            None => ReadFileResult::FallThrough,
        }
    }

    /// Service a `fileExists` callback. Returns `Some(true)` for an overlay file
    /// present with content, `Some(false)` for an overlay-known-absent file, and
    /// `None` to fall through to the real FS (fs.js:82-84 + the wrap semantics).
    pub fn file_exists(&self, path: &str) -> Option<bool> {
        match self.files.get(&norm_path(path)) {
            Some(Some(_)) => Some(true),
            Some(None) => Some(false),
            None => None,
        }
    }

    /// Service a `directoryExists` callback. Returns `Some(true)` for an overlay
    /// directory (or the parent of any overlay file), else `None` (fall through).
    pub fn directory_exists(&self, path: &str) -> Option<bool> {
        let n = norm_path(path);
        if self.directories.contains(&n) {
            return Some(true);
        }
        // A directory implicitly exists if it is the parent of an overlay file.
        if self
            .files
            .keys()
            .any(|f| f.rsplit_once('/').map(|(d, _)| d) == Some(n.as_str()))
        {
            return Some(true);
        }
        None
    }

    /// Service a `realpath` callback. The overlay does not remap paths, so an
    /// overlay-known path resolves to itself (identity, mirroring the vendored
    /// vfs `realpath: path => path`, fs.js:19); an unknown path falls through.
    pub fn realpath(&self, path: &str) -> Option<String> {
        let n = norm_path(path);
        if self.files.contains_key(&n) || self.directories.contains(&n) {
            Some(n)
        } else {
            None
        }
    }

    /// Service a `getAccessibleEntries` callback. MERGES the overlay entries for
    /// `dir` with the real directory entries (fs.js:85-101 + the gate harness's
    /// merge at harness.mjs:180-199). Overlay basenames are unioned into the real
    /// listing so real files stay visible (overlay-only would hide them).
    ///
    /// Returns `None` only when neither the overlay nor the real source knows the
    /// directory (so the engine falls through to its own real-FS enumeration).
    pub fn get_accessible_entries(&self, dir: &str) -> Option<AccessibleEntries> {
        let n = norm_path(dir);

        // Overlay contributions: files whose parent is `dir`, plus overlay dirs
        // whose parent is `dir`, plus immediate subdirs implied by file paths.
        let mut overlay_files: Vec<String> = Vec::new();
        for (path, content) in &self.files {
            if content.is_none() {
                continue; // a definitively-absent file is not an entry
            }
            let (parent, base) = split_parent_basename(path);
            if parent == n {
                overlay_files.push(base);
            }
        }
        let mut overlay_dirs: Vec<String> = Vec::new();
        for d in &self.directories {
            let (parent, base) = split_parent_basename(d);
            if parent == n {
                overlay_dirs.push(base);
            }
        }

        let real = self.real.real_entries(&n);

        // If nothing knows this dir, fall through.
        if overlay_files.is_empty() && overlay_dirs.is_empty() && real.is_none() {
            return None;
        }

        let mut files = Vec::new();
        let mut directories = Vec::new();
        if let Some(r) = real {
            files.extend(r.files);
            directories.extend(r.directories);
        }
        // Union overlay basenames in (dedup against the real listing).
        for f in overlay_files {
            if !files.contains(&f) {
                files.push(f);
            }
        }
        for d in overlay_dirs {
            if !directories.contains(&d) {
                directories.push(d);
            }
        }
        Some(AccessibleEntries { files, directories })
    }
}

/// Builder for an [`OverlaySnapshot`].
#[derive(Debug, Default)]
pub struct OverlaySnapshotBuilder {
    files: BTreeMap<String, Option<Arc<str>>>,
    directories: std::collections::BTreeSet<String>,
    real: Option<Arc<dyn RealDirSource>>,
}

impl OverlaySnapshotBuilder {
    /// Create an empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an overlay file with `content` at `path`.
    pub fn file(mut self, path: impl AsRef<str>, content: impl AsRef<str>) -> Self {
        self.files
            .insert(norm_path(path.as_ref()), Some(Arc::from(content.as_ref())));
        self
    }

    /// Mark `path` as a definitively-absent overlay file (`readFile` → NotFound,
    /// `fileExists` → Some(false)). Models a deleted overlay member that must not
    /// fall through to a stale on-disk copy.
    pub fn absent_file(mut self, path: impl AsRef<str>) -> Self {
        self.files.insert(norm_path(path.as_ref()), None);
        self
    }

    /// Add an overlay directory known to exist.
    pub fn directory(mut self, path: impl AsRef<str>) -> Self {
        self.directories.insert(norm_path(path.as_ref()));
        self
    }

    /// Set the real-directory entry source (consumer-supplied).
    pub fn real_dir_source(mut self, source: Arc<dyn RealDirSource>) -> Self {
        self.real = Some(source);
        self
    }

    /// Finalize the immutable snapshot.
    pub fn build(self) -> OverlaySnapshot {
        OverlaySnapshot {
            files: self.files,
            directories: self.directories,
            real: self
                .real
                .unwrap_or_else(|| Arc::new(EmptyRealDirSource) as Arc<dyn RealDirSource>),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct MapRealDirSource(BTreeMap<String, AccessibleEntries>);
    impl RealDirSource for MapRealDirSource {
        fn real_entries(&self, dir: &str) -> Option<AccessibleEntries> {
            self.0.get(dir).cloned()
        }
    }

    #[test]
    fn read_file_three_states() {
        let snap = OverlaySnapshot::builder()
            .file("/repo/src/a.ts", "export const a = 1;")
            .absent_file("/repo/src/gone.ts")
            .build();
        assert_eq!(
            snap.read_file("/repo/src/a.ts"),
            ReadFileResult::Found("export const a = 1;".to_string())
        );
        assert_eq!(
            snap.read_file("/repo/src/gone.ts"),
            ReadFileResult::NotFound
        );
        assert_eq!(
            snap.read_file("/repo/src/unknown.ts"),
            ReadFileResult::FallThrough
        );
    }

    #[test]
    fn read_file_normalizes_backslash_paths() {
        let snap = OverlaySnapshot::builder()
            .file("/repo/src/a.ts", "x")
            .build();
        // A Windows-style path with backslashes must hit the same overlay entry.
        assert_eq!(
            snap.read_file("\\repo\\src\\a.ts"),
            ReadFileResult::Found("x".to_string())
        );
    }

    #[test]
    fn file_exists_distinguishes_absent_from_unknown() {
        let snap = OverlaySnapshot::builder()
            .file("/a.ts", "x")
            .absent_file("/b.ts")
            .build();
        assert_eq!(snap.file_exists("/a.ts"), Some(true));
        assert_eq!(snap.file_exists("/b.ts"), Some(false));
        assert_eq!(
            snap.file_exists("/c.ts"),
            None,
            "unknown path falls through"
        );
    }

    #[test]
    fn directory_exists_for_explicit_and_implied_dirs() {
        let snap = OverlaySnapshot::builder()
            .directory("/repo/types")
            .file("/repo/src/a.ts", "x")
            .build();
        assert_eq!(snap.directory_exists("/repo/types"), Some(true));
        assert_eq!(
            snap.directory_exists("/repo/src"),
            Some(true),
            "parent of an overlay file implicitly exists"
        );
        assert_eq!(snap.directory_exists("/repo/nope"), None);
    }

    #[test]
    fn realpath_is_identity_for_known_paths_else_fallthrough() {
        let snap = OverlaySnapshot::builder().file("/a.ts", "x").build();
        assert_eq!(snap.realpath("/a.ts"), Some("/a.ts".to_string()));
        assert_eq!(snap.realpath("\\a.ts"), Some("/a.ts".to_string()));
        assert_eq!(snap.realpath("/other.ts"), None);
    }

    // ── THE MERGE REQUIREMENT: virtual + real entries are unioned ───────────
    #[test]
    fn get_accessible_entries_merges_virtual_and_real() {
        let mut real = BTreeMap::new();
        real.insert(
            "/repo/src".to_string(),
            AccessibleEntries {
                files: vec!["real.ts".to_string(), "shared.ts".to_string()],
                directories: vec!["utils".to_string()],
            },
        );
        let snap = OverlaySnapshot::builder()
            .file("/repo/src/Widget.carrier.tsx", "x")
            // An overlay file whose basename ALSO exists on disk must not duplicate.
            .file("/repo/src/shared.ts", "y")
            .directory("/repo/src/generated")
            .real_dir_source(Arc::new(MapRealDirSource(real)))
            .build();

        let entries = snap.get_accessible_entries("/repo/src").unwrap();

        // Real files are still present (overlay did NOT hide them).
        assert!(
            entries.files.contains(&"real.ts".to_string()),
            "{entries:?}"
        );
        // The overlay-only file is added.
        assert!(entries.files.contains(&"Widget.carrier.tsx".to_string()));
        // The basename present in both appears exactly once (no duplicate).
        assert_eq!(
            entries.files.iter().filter(|f| *f == "shared.ts").count(),
            1,
            "overlapping basename must not be duplicated"
        );
        // Real subdir + overlay subdir both present.
        assert!(entries.directories.contains(&"utils".to_string()));
        assert!(entries.directories.contains(&"generated".to_string()));
    }

    #[test]
    fn get_accessible_entries_overlay_only_when_no_real_source() {
        let snap = OverlaySnapshot::builder()
            .file("/repo/src/Only.tsx", "x")
            .build();
        let entries = snap.get_accessible_entries("/repo/src").unwrap();
        assert_eq!(entries.files, vec!["Only.tsx".to_string()]);
        assert!(entries.directories.is_empty());
    }

    #[test]
    fn get_accessible_entries_falls_through_for_unknown_dir() {
        let snap = OverlaySnapshot::builder()
            .file("/repo/src/a.ts", "x")
            .build();
        assert_eq!(
            snap.get_accessible_entries("/some/other/dir"),
            None,
            "a dir unknown to both overlay and real source falls through"
        );
    }

    // ── NEGATIVE: a definitively-absent overlay file is NOT an entry ────────
    #[test]
    fn absent_overlay_file_is_not_enumerated() {
        let snap = OverlaySnapshot::builder()
            .file("/repo/src/a.ts", "x")
            .absent_file("/repo/src/deleted.ts")
            .build();
        let entries = snap.get_accessible_entries("/repo/src").unwrap();
        assert!(entries.files.contains(&"a.ts".to_string()));
        assert!(
            !entries.files.contains(&"deleted.ts".to_string()),
            "a definitively-absent overlay file must not appear in the listing"
        );
    }

    #[test]
    fn snapshot_is_cloneable_and_shareable() {
        // The snapshot must be cheap to share for ArcSwap publication.
        let snap = OverlaySnapshot::builder().file("/a.ts", "x").build();
        let arc = Arc::new(snap);
        let clone = Arc::clone(&arc);
        assert_eq!(
            clone.read_file("/a.ts"),
            ReadFileResult::Found("x".to_string())
        );
    }
}

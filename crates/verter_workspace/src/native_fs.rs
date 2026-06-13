use std::sync::Arc;

use crate::error::{DirEntry, VfsError};

/// Native filesystem wrapper — the sole disk-touch boundary.
///
/// ALL `std::fs` calls in `verter_workspace` go through this struct.
/// Gated behind `#[cfg(not(target_arch = "wasm32"))]` — not available in WASM.
/// Used as the disk fallback layer in `FilesystemWorkspace`.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub struct NativeFs;

#[cfg(not(target_arch = "wasm32"))]
impl NativeFs {
    pub fn new() -> Self {
        Self
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
    pub fn realpath(&self, path: &str) -> Option<String> {
        let os_path = to_os_path(path);
        std::fs::canonicalize(&os_path)
            .ok()
            .map(|p| normalize_path_str(&p.to_string_lossy()))
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
        Ok(())
    }

    /// Create a directory and all parent directories.
    pub fn create_dir_all(&self, path: &str) -> Result<(), VfsError> {
        let os_path = to_os_path(path);
        std::fs::create_dir_all(&os_path)?;
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
        })
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
        })
    }

    /// Copy a file from `src` to `dst`.
    pub fn copy_file(&self, src: &str, dst: &str) -> Result<(), VfsError> {
        let src_os = to_os_path(src);
        let dst_os = to_os_path(dst);
        if let Some(parent) = std::path::Path::new(&dst_os).parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&src_os, &dst_os)?;
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for NativeFs {
    fn default() -> Self {
        Self::new()
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

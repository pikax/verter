use std::sync::Arc;

/// Native filesystem wrapper for reading files from disk.
///
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
            .map(|p| p.to_string_lossy().replace('\\', "/"))
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

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

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
}

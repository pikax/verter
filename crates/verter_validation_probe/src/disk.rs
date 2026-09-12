//! The crate's ONE disk boundary.
//!
//! This lane is CI tooling: it reads a pinned third-party corpus checkout, its
//! own committed manifests, and its own published artifact. None of that is
//! workspace, semantic, overlay or VFS state, so none of it belongs on the
//! host's disk boundary — but it is still real filesystem access, and it is
//! confined HERE rather than spread across the corpus adapter, the lane and
//! the summary binary. One module to audit, one module to allowlist, and no
//! second place for a future reader to appear.
//!
//! Nothing here interprets what it read. Classification, validation and
//! expectation live in their own modules; this one only moves bytes.

use std::path::{Path, PathBuf};

/// Why a disk operation failed, with the path it was attempted on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskError {
    /// What was being read or written.
    pub path: PathBuf,
    /// The operating system's message.
    pub message: String,
}

impl std::fmt::Display for DiskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for DiskError {}

fn at(path: &Path, error: std::io::Error) -> DiskError {
    DiskError {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

/// Read a UTF-8 text file.
pub fn read_text(path: &Path) -> Result<String, DiskError> {
    std::fs::read_to_string(path).map_err(|error| at(path, error))
}

/// Write a UTF-8 text file, creating its parent directory.
pub fn write_text(path: &Path, text: &str) -> Result<(), DiskError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| at(parent, error))?;
    }
    std::fs::write(path, text).map_err(|error| at(path, error))
}

/// Whether `path` is an existing directory.
pub fn is_directory(path: &Path) -> bool {
    path.is_dir()
}

/// Whether `path` is an existing file.
pub fn is_file(path: &Path) -> bool {
    path.is_file()
}

/// One directory's immediate children, sorted, so a walk over them is
/// deterministic on every platform.
pub fn sorted_children(dir: &Path) -> Result<Vec<PathBuf>, DiskError> {
    let read = std::fs::read_dir(dir).map_err(|error| at(dir, error))?;
    let mut children = Vec::new();
    for entry in read {
        children.push(entry.map_err(|error| at(dir, error))?.path());
    }
    children.sort();
    Ok(children)
}

/// VFS error type for workspace operations.
#[derive(Debug)]
pub enum VfsError {
    /// The requested path was not found.
    NotFound(String),
    /// The path is outside all workspace roots.
    OutsideWorkspace(String),
    /// The operation is not supported by this workspace type
    /// (e.g., disk writes on a `MemoryWorkspace`).
    UnsupportedOperation(&'static str),
    /// An underlying I/O error.
    Io(std::io::Error),
}

impl std::fmt::Display for VfsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VfsError::NotFound(path) => write!(f, "not found: {path}"),
            VfsError::OutsideWorkspace(path) => write!(f, "outside workspace: {path}"),
            VfsError::UnsupportedOperation(op) => write!(f, "unsupported operation: {op}"),
            VfsError::Io(err) => write!(f, "I/O error: {err}"),
        }
    }
}

impl std::error::Error for VfsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            VfsError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for VfsError {
    fn from(err: std::io::Error) -> Self {
        VfsError::Io(err)
    }
}

/// Directory entry returned by `read_dir`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DirEntry {
    /// Canonical path of the entry (forward slashes, lowercase drive on Windows).
    pub path: String,
    /// Whether the entry is a directory.
    pub is_dir: bool,
}

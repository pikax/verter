//! The crate's ONE disk boundary.
//!
//! The supervisor is test and benchmark tooling: it touches only its own
//! result document and the child's log files, the cgroup and `/proc`
//! pseudo-files of the workload it contains, and temporary paths its tests
//! and fixture use. None of that is workspace, semantic, overlay or VFS
//! state, so none of it belongs on the host's disk boundary, but it is still
//! real filesystem access, and it is confined HERE rather than spread across
//! the backends, the result writer and the fixture binary. One module to
//! audit, one module to allowlist.
//!
//! Nothing here interprets what it read; this module only moves bytes.

use std::io;
use std::path::Path;

/// An open file (the child's standard streams, a cgroup control file).
pub type File = std::fs::File;
/// How a file is opened.
pub type OpenOptions = std::fs::OpenOptions;

/// Read a whole file as UTF-8 text.
pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    std::fs::read_to_string(path)
}

/// Write `contents` to a file, creating or truncating it.
pub fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    std::fs::write(path, contents)
}

/// Rename `from` to `to`, replacing `to`.
pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// Create one directory.
pub fn create_dir(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::create_dir(path)
}

/// Create a directory and every missing parent.
pub fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Remove an empty directory (an emptied cgroup).
pub fn remove_dir(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::remove_dir(path)
}

/// Remove a directory tree.
pub fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}

/// Create or truncate a file for writing.
pub fn create(path: impl AsRef<Path>) -> io::Result<File> {
    File::create(path)
}

/// Open a file for reading.
pub fn open(path: impl AsRef<Path>) -> io::Result<File> {
    File::open(path)
}

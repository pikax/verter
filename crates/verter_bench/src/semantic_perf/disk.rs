//! The semantic-perf harness's only real-disk boundary: its job file, the
//! scenario inputs it reads, the record and phase marker it writes, and the
//! `/proc` pseudo-files it samples on Linux. Benchmark tooling, never
//! workspace, semantic, overlay or VFS state.

use std::path::Path;

/// The whole file at `path`, as text.
pub fn read_to_string(path: impl AsRef<Path>) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

/// Write `text` to `path`, replacing it.
pub fn write(path: impl AsRef<Path>, text: &str) -> std::io::Result<()> {
    std::fs::write(path, text)
}

/// Rename `from` to `to`, replacing `to`.
pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

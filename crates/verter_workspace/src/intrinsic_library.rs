//! Ambient TypeScript SDK library access.
//!
//! [`IntrinsicLibraryAccess`] is the workspace-level abstraction for reading
//! TypeScript SDK `lib*.d.ts` declarations and discovering the active SDK's
//! `lib` directory. It is intentionally **separate** from
//! [`WorkspaceAccess`](crate::WorkspaceAccess) because mixing SDK reads
//! with workspace source/config reads weakens source-overlay semantics:
//! ambient SDK content is owned by the installed TypeScript package, not
//! by the user's workspace, and must not flow through the user-content
//! overlay.
//!
//! Two concrete implementations live next to the trait:
//!
//! - [`NativeIntrinsicLibrary`] — production discovery + read backed by
//!   the installed `typescript` package on disk. Locates the active SDK
//!   via the workspace's `node_modules` (hoisted or pnpm virtual store).
//! - [`InMemoryIntrinsicLibrary`] — test fixture with a pre-populated
//!   `name -> source` map. Useful for tests that want to exercise the
//!   audit scanner without an installed `typescript` package.
//!
//! The architecture guard `no_std_fs_in_semantic_session_paths`
//! allowlists `intrinsic_library.rs` so the production impl can route
//! disk reads through `std::fs` here, while flagging any new direct
//! `std::fs::` callsite that appears in `verter_session::intrinsic_registry`.

use std::io;

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

/// Trait for reading TypeScript SDK ambient libraries.
///
/// Implementations must be `Send + Sync` so they can be shared across
/// threads via `Arc<dyn IntrinsicLibraryAccess>`.
pub trait IntrinsicLibraryAccess: Send + Sync {
    /// List the names of every `lib*.d.ts` file in the active SDK, in
    /// lexicographic order. Returns an empty `Vec` when no SDK is
    /// available.
    fn list_intrinsic_libs(&self) -> Vec<String>;

    /// Read a single `lib*.d.ts` file by `name` (e.g. `"lib.es5.d.ts"`).
    ///
    /// Returns `Err(io::ErrorKind::NotFound)` when the entry is unknown
    /// or when no SDK is available.
    fn read_intrinsic_lib(&self, name: &str) -> io::Result<String>;
}

/// Production-grade [`IntrinsicLibraryAccess`] backed by an installed
/// `typescript` package on disk.
#[cfg(not(target_arch = "wasm32"))]
pub struct NativeIntrinsicLibrary {
    /// Resolved `<typescript>/lib` directory, if discovery succeeded.
    lib_dir: Option<PathBuf>,
}

#[cfg(not(target_arch = "wasm32"))]
impl NativeIntrinsicLibrary {
    /// Construct an instance by discovering the active SDK rooted at
    /// `workspace_root` (the directory containing `node_modules`).
    ///
    /// Discovery checks, in order:
    /// 1. Hoisted install at `<workspace_root>/node_modules/typescript/lib`.
    /// 2. pnpm virtual store: any `typescript@*` entry under
    ///    `<workspace_root>/node_modules/.pnpm` (lexicographically newest
    ///    wins for determinism).
    pub fn discover(workspace_root: &std::path::Path) -> Self {
        let lib_dir = discover_active_lib_dir(workspace_root);
        Self { lib_dir }
    }

    /// Return the resolved `<typescript>/lib` path, if any. Visible for
    /// tests that want to assert which SDK was selected.
    pub fn lib_dir(&self) -> Option<&std::path::Path> {
        self.lib_dir.as_deref()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl IntrinsicLibraryAccess for NativeIntrinsicLibrary {
    fn list_intrinsic_libs(&self) -> Vec<String> {
        let Some(lib_dir) = &self.lib_dir else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(lib_dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_string();
                if name.starts_with("lib.") && name.ends_with(".d.ts") {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();
        names.sort();
        names
    }

    fn read_intrinsic_lib(&self, name: &str) -> io::Result<String> {
        let Some(lib_dir) = &self.lib_dir else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no active typescript SDK was discovered",
            ));
        };
        let path = lib_dir.join(name);
        std::fs::read_to_string(path)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn discover_active_lib_dir(workspace_root: &std::path::Path) -> Option<PathBuf> {
    // 1. Hoisted install
    let hoisted = workspace_root.join("node_modules/typescript/lib");
    if hoisted.is_dir() {
        return Some(hoisted);
    }

    // 2. pnpm virtual store — any `typescript@*` entry is fine.
    let pnpm_dir = workspace_root.join("node_modules/.pnpm");
    let entries = std::fs::read_dir(&pnpm_dir).ok()?;
    let mut candidates: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if name_str.starts_with("typescript@") && !name_str.contains('+') {
            let lib = entry.path().join("node_modules/typescript/lib");
            if lib.is_dir() {
                candidates.push(lib);
            }
        }
    }
    candidates.sort();
    candidates.pop()
}

/// In-memory [`IntrinsicLibraryAccess`] for tests. Stores a
/// pre-populated `name -> source` map.
pub struct InMemoryIntrinsicLibrary {
    entries: std::collections::BTreeMap<String, String>,
}

impl InMemoryIntrinsicLibrary {
    /// Create an empty in-memory library.
    pub fn new() -> Self {
        Self {
            entries: std::collections::BTreeMap::new(),
        }
    }

    /// Insert a `(name, source)` pair (e.g. `"lib.es5.d.ts"`).
    pub fn insert(&mut self, name: impl Into<String>, source: impl Into<String>) {
        self.entries.insert(name.into(), source.into());
    }
}

impl Default for InMemoryIntrinsicLibrary {
    fn default() -> Self {
        Self::new()
    }
}

impl IntrinsicLibraryAccess for InMemoryIntrinsicLibrary {
    fn list_intrinsic_libs(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    fn read_intrinsic_lib(&self, name: &str) -> io::Result<String> {
        self.entries.get(name).cloned().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no in-memory entry for `{name}`"),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_library_round_trips_name_and_source() {
        let mut lib = InMemoryIntrinsicLibrary::new();
        lib.insert("lib.es5.d.ts", "type Awaited<T> = intrinsic;");
        lib.insert("lib.es2015.d.ts", "// es2015");

        let names = lib.list_intrinsic_libs();
        assert_eq!(names, vec!["lib.es2015.d.ts", "lib.es5.d.ts"]);

        let src = lib.read_intrinsic_lib("lib.es5.d.ts").unwrap();
        assert!(src.contains("intrinsic"));
    }

    #[test]
    fn in_memory_library_returns_not_found_for_missing_entry() {
        let lib = InMemoryIntrinsicLibrary::new();
        let err = lib.read_intrinsic_lib("lib.es5.d.ts").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn empty_in_memory_library_has_no_entries() {
        let lib = InMemoryIntrinsicLibrary::new();
        assert!(lib.list_intrinsic_libs().is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_library_returns_empty_when_no_sdk_present() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = NativeIntrinsicLibrary::discover(tmp.path());
        assert!(lib.list_intrinsic_libs().is_empty());
        let err = lib.read_intrinsic_lib("lib.es5.d.ts").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}

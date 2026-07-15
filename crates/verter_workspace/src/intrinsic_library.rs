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
    /// `true` when an rc per-platform package
    /// (`@typescript/typescript-<platform>-<arch>`) carrying real declarations
    /// was discoverable in the workspace — i.e. the active engine is TS>=7.
    rc_platform_available: bool,
    /// Whether the workspace's ACTIVE TypeScript engine uses the tsgo/native-preview
    /// (TypeScript >= 7) toolchain, decided by [`workspace_active_typescript_is_tsgo`]
    /// — a bounded parent walk (mirroring `find_tsserver`) that, nearest-first,
    /// prefers a readable hoisted `node_modules/typescript` major and falls back to
    /// rc-per-platform-package presence AT THE SAME level. Both signals resolve over
    /// the SAME walk, so a nested member open under a hoisted (possibly launcher-only,
    /// unreadable-version) install is classified consistently.
    active_typescript_is_tsgo: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl NativeIntrinsicLibrary {
    /// Construct an instance by discovering the active SDK rooted at
    /// `workspace_root` (the directory containing `node_modules`).
    ///
    /// Discovery checks, in order:
    /// 1. Hoisted install at `<workspace_root>/node_modules/typescript/lib`.
    /// 2. The rc per-platform package and pnpm virtual store, selecting by
    ///    ACTIVE ENGINE VERSION (see [`discover_active_lib_dir_with_rc_flag`]).
    pub fn discover(workspace_root: &std::path::Path) -> Self {
        let (lib_dir, rc_platform_available) = discover_active_lib_dir_with_rc_flag(workspace_root);
        let active_typescript_is_tsgo = workspace_active_typescript_is_tsgo(workspace_root);
        Self {
            lib_dir,
            rc_platform_available,
            active_typescript_is_tsgo,
        }
    }

    /// Return the resolved `<typescript>/lib` path, if any. Visible for
    /// tests that want to assert which SDK was selected.
    pub fn lib_dir(&self) -> Option<&std::path::Path> {
        self.lib_dir.as_deref()
    }

    /// `true` when an rc per-platform TypeScript package
    /// (`@typescript/typescript-<platform>-<arch>`) carrying real declarations
    /// was discoverable in the workspace, i.e. the active engine is TS>=7. When
    /// this holds, the selected lib dir MUST be the rc platform package
    /// ([`Self::selected_lib_is_rc_platform`]) rather than a legacy
    /// `typescript@<ver>` lib dir. Exposed so consumers/tests can assert
    /// version-correct selection without performing their own `std::fs`.
    pub fn rc_platform_package_available(&self) -> bool {
        self.rc_platform_available
    }

    /// Is the selected lib dir the rc per-platform package's `lib` directory?
    ///
    /// Structural, OS-agnostic: a path component equals the `@typescript` scope
    /// dir AND is immediately followed by a `typescript-<platform>` package
    /// component (does NOT hardcode `win32-x64`/`linux-x64`/`darwin-arm64`). A
    /// legacy `typescript@<ver>/.../typescript/lib` has no `@typescript` scope
    /// component, so it returns `false`.
    pub fn selected_lib_is_rc_platform(&self) -> bool {
        self.lib_dir
            .as_deref()
            .is_some_and(lib_dir_is_rc_platform_package)
    }

    /// Does the workspace's ACTIVE TypeScript engine use the tsgo/native-preview
    /// (TypeScript >= 7) toolchain? Auto-mode provider selection uses this to route a
    /// tsgo workspace to the tsgo external engine instead of a lower bundled/global
    /// tsserver. Resolved by [`workspace_active_typescript_is_tsgo`] — a bounded parent
    /// walk (mirroring `find_tsserver`) that, nearest-first, prefers a readable hoisted
    /// `typescript` major and falls back to rc-per-platform-package presence at the same
    /// level, so a nested member open under a hoisted install is classified consistently.
    pub fn active_typescript_is_tsgo(&self) -> bool {
        self.active_typescript_is_tsgo
    }
}

/// Is `lib_dir` the rc per-platform package's `lib` directory? See
/// [`NativeIntrinsicLibrary::selected_lib_is_rc_platform`].
#[cfg(not(target_arch = "wasm32"))]
fn lib_dir_is_rc_platform_package(lib_dir: &std::path::Path) -> bool {
    let comps: Vec<&str> = lib_dir
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    comps
        .windows(2)
        .any(|w| w[0] == "@typescript" && w[1].starts_with("typescript-"))
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

/// Does `dir` contain at least one `lib.*.d.ts` intrinsic declaration file?
///
/// This is the authoritative "real SDK lib dir" check. The `typescript@>=7`
/// (rc) JS package's own `lib/` holds only launcher files (`getExePath.*`,
/// `tsc.js`) — the `lib.*.d.ts` declarations moved to the per-platform
/// `@typescript/typescript-<platform>-<arch>` package's `lib/`. Probing for an
/// actual `lib.*.d.ts` file lets discovery skip the launcher-only directory and
/// follow to the platform package, regardless of OS.
#[cfg(not(target_arch = "wasm32"))]
fn dir_has_intrinsic_libs(dir: &std::path::Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        e.file_name()
            .to_str()
            .is_some_and(|n| n.starts_with("lib.") && n.ends_with(".d.ts"))
    })
}

/// A discovered candidate `lib` directory plus the identity of its owning
/// TypeScript package, used to select the one that matches the ACTIVE engine.
#[cfg(not(target_arch = "wasm32"))]
struct CandidateLib {
    /// The `lib` directory carrying the `lib.*.d.ts` declarations.
    lib_dir: PathBuf,
    /// The owning package's `version` string (from the sibling `package.json`),
    /// if readable.
    version: Option<String>,
    /// `true` when this is an rc per-platform package
    /// (`@typescript/typescript-<platform>-<arch>`), i.e. the libs that pair
    /// with the typescript-go engine; `false` for a legacy `typescript@<ver>`.
    is_rc_platform: bool,
}

/// Extract the `version` field from a package directory's `package.json`.
///
/// Sibling of [`detect_ts_major_version`](crate)-style scanning, kept local and
/// dependency-free: a small string scan for `"version": "X.Y.Z"`. Returns the
/// full version string (e.g. `"7.0.1-rc"`) or `None` when unreadable.
#[cfg(not(target_arch = "wasm32"))]
fn read_package_version(pkg_dir: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(pkg_dir.join("package.json")).ok()?;
    let version_key = content.find("\"version\"")?;
    let after = &content[version_key..];
    let colon = after.find(':')?;
    let after_colon = after[colon + 1..].trim_start();
    let quote_start = after_colon.find('"')? + 1;
    let rest = &after_colon[quote_start..];
    let quote_end = rest.find('"')?;
    Some(rest[..quote_end].to_string())
}

/// Does the workspace's ACTIVE TypeScript engine (found by walking `workspace_root`
/// up to the monorepo root, bounded, mirroring `find_tsserver`'s parent walk) use
/// the tsgo/native-preview (TypeScript >= 7) toolchain? At each level, nearest-first:
/// a readable hoisted `node_modules/typescript` version is authoritative
/// (major >= 7 ⇒ tsgo); otherwise, when that version is unreadable but the rc
/// per-platform package carrying real libs is present at that level (launcher-only
/// rc layout), the workspace is tsgo. Both signals resolve over the SAME walk, so a
/// nested member open under a hoisted install is classified consistently.
#[cfg(not(target_arch = "wasm32"))]
fn workspace_active_typescript_is_tsgo(workspace_root: &std::path::Path) -> bool {
    let mut dir = workspace_root;
    for _ in 0..10 {
        let node_modules = dir.join("node_modules");
        if let Some(version) = read_package_version(&node_modules.join("typescript")) {
            return version
                .split('.')
                .next()
                .and_then(|m| m.parse::<u32>().ok())
                .is_some_and(|major| major >= 7);
        }
        // Launcher-only rc layout: no readable `typescript` version at this level,
        // but the rc per-platform package (real libs) is present ⇒ tsgo engine.
        if collect_lib_candidates(&node_modules)
            .iter()
            .any(|c| c.is_rc_platform)
        {
            return true;
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent,
            _ => break,
        }
    }
    false
}

/// Compare two TypeScript version strings (`major.minor.patch[-prerelease]`) by
/// SEMVER ordering: numeric component compare, then a release (no prerelease)
/// ranks ABOVE an otherwise-equal prerelease (`7.0.1` > `7.0.1-rc`). Returns
/// `Ordering::Equal` for equal cores + equal prerelease tags. Dependency-free —
/// the precision needed here (pick the newest installed TS) does not warrant a
/// full semver crate.
#[cfg(not(target_arch = "wasm32"))]
fn compare_semver(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    fn split(v: &str) -> (Vec<u64>, Option<String>) {
        let (core, pre) = match v.split_once('-') {
            Some((core, pre)) => (core, Some(pre.to_string())),
            None => (v, None),
        };
        let nums = core
            .split('.')
            .map(|n| n.parse::<u64>().unwrap_or(0))
            .collect();
        (nums, pre)
    }

    let (a_nums, a_pre) = split(a);
    let (b_nums, b_pre) = split(b);

    // Compare numeric cores component-by-component (missing components = 0).
    let max = a_nums.len().max(b_nums.len());
    for i in 0..max {
        let an = a_nums.get(i).copied().unwrap_or(0);
        let bn = b_nums.get(i).copied().unwrap_or(0);
        match an.cmp(&bn) {
            Ordering::Equal => continue,
            other => return other,
        }
    }

    // Equal cores: a release outranks a prerelease; otherwise compare tags
    // lexicographically for determinism.
    match (a_pre, b_pre) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a_pre), Some(b_pre)) => a_pre.cmp(&b_pre),
    }
}

/// Discover the active TypeScript SDK `lib` directory holding the ambient
/// `lib.*.d.ts` intrinsic declarations.
///
/// The layout differs across TS versions:
///
/// - Pre-`7` (`typescript@5.x`/`6.x`): the `lib.*.d.ts` files live directly in
///   `typescript/lib` (hoisted or under the pnpm virtual store).
/// - `7.x` (rc, typescript-go engine): `typescript/lib` holds only launcher
///   files; the declarations live in the per-platform package
///   `@typescript/typescript-<platform>-<arch>/lib` (e.g.
///   `typescript-win32-x64`, `typescript-linux-x64`, `typescript-darwin-arm64`).
///
/// Discovery is OS-agnostic: it probes each candidate directory for a real
/// `lib.*.d.ts` file ([`dir_has_intrinsic_libs`]) rather than constructing a
/// platform-specific package name, and ENUMERATES the platform packages on disk
/// instead of hardcoding the current platform.
///
/// Selection is by ACTIVE ENGINE VERSION, not lexicographic order. The active
/// `typescript` version is read from the hoisted `node_modules/typescript`
/// package (which, in the rc layout, is the launcher-only package that still
/// carries the pinned version and drives the typescript-go engine). The selected
/// candidate is:
///
/// 1. the candidate whose owning package `version` matches the active version,
///    preferring the rc per-platform package among equal-version ties (its libs
///    are the ones the engine actually uses); else
/// 2. the highest-SEMVER rc per-platform candidate; else
/// 3. the highest-SEMVER legacy candidate.
///
/// This avoids the lexicographic trap where a legacy `typescript@6.x` store dir
/// sorts AFTER the `@`-prefixed rc platform store dir and would otherwise be
/// chosen against a TS7-rc engine.
///
/// The returned flag reports whether an rc per-platform package carrying real
/// declarations was discoverable (i.e. the active engine is TS>=7), so consumers
/// can assert version-correct selection without a second `std::fs` scan.
#[cfg(not(target_arch = "wasm32"))]
fn discover_active_lib_dir_with_rc_flag(
    workspace_root: &std::path::Path,
) -> (Option<PathBuf>, bool) {
    let node_modules = workspace_root.join("node_modules");

    // 1. Hoisted `typescript/lib` — accept only when it actually carries the
    //    `lib.*.d.ts` declarations (the rc package's `lib/` is launcher-only).
    //    A legacy hoisted layout (TS5/6) means no rc platform package is in play.
    let hoisted = node_modules.join("typescript").join("lib");
    if dir_has_intrinsic_libs(&hoisted) {
        return (Some(hoisted), false);
    }

    // The ACTIVE engine version: the hoisted `typescript` package's pinned
    // `version` (present even when its `lib/` is launcher-only). Used to match
    // the candidate libs that pair with the engine the tsgo path runs.
    let active_version = read_package_version(&node_modules.join("typescript"));

    let candidates = collect_lib_candidates(&node_modules);
    let rc_platform_available = candidates.iter().any(|c| c.is_rc_platform);

    (
        select_active_candidate(candidates, active_version.as_deref()),
        rc_platform_available,
    )
}

/// Enumerate every candidate `lib.*.d.ts`-bearing directory under `node_modules`
/// (the rc per-platform `@typescript/typescript-*` packages and the pnpm virtual
/// store), each tagged with its owning package version + whether it is an rc
/// platform package. OS-agnostic: probes for real declarations and enumerates
/// platform packages rather than hardcoding the current platform.
#[cfg(not(target_arch = "wasm32"))]
fn collect_lib_candidates(node_modules: &std::path::Path) -> Vec<CandidateLib> {
    let mut candidates: Vec<CandidateLib> = Vec::new();

    // rc per-platform packages under the hoisted `@typescript` scope, e.g.
    // `node_modules/@typescript/typescript-<platform>-<arch>/lib`. Enumerate
    // every `typescript-*` sibling (do not hardcode the platform) and keep the
    // ones that carry real declarations.
    let scope_root = node_modules.join("@typescript");
    if let Ok(siblings) = std::fs::read_dir(&scope_root) {
        for sibling in siblings.flatten() {
            let name = sibling.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name.starts_with("typescript-") {
                continue;
            }
            let pkg = sibling.path();
            let lib = pkg.join("lib");
            if dir_has_intrinsic_libs(&lib) {
                candidates.push(CandidateLib {
                    version: read_package_version(&pkg),
                    lib_dir: lib,
                    is_rc_platform: true,
                });
            }
        }
    }

    // pnpm virtual store. Two shapes coexist:
    // - rc per-platform package:
    //   `.pnpm/@typescript+typescript-<platform>-<arch>@<ver>/node_modules/@typescript/typescript-<platform>-<arch>/lib`
    // - legacy `typescript@<ver>` package:
    //   `.pnpm/typescript@<ver>/node_modules/typescript/lib`
    // Probe both for a real `lib.*.d.ts` file; the rc platform package is
    // `+`-named (the legacy filter that excluded `+` would skip it), so the
    // discovery here keys on the presence of declarations, not the name.
    let pnpm_dir = node_modules.join(".pnpm");
    if let Ok(entries) = std::fs::read_dir(&pnpm_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let inner = entry.path().join("node_modules");
            if name.starts_with("@typescript+typescript-") {
                // rc per-platform store entry: descend into the `@typescript`
                // scope and probe each platform package's `lib`.
                let scope = inner.join("@typescript");
                if let Ok(pkgs) = std::fs::read_dir(&scope) {
                    for pkg in pkgs.flatten() {
                        let pkg = pkg.path();
                        let lib = pkg.join("lib");
                        if dir_has_intrinsic_libs(&lib) {
                            candidates.push(CandidateLib {
                                version: read_package_version(&pkg),
                                lib_dir: lib,
                                is_rc_platform: true,
                            });
                        }
                    }
                }
            } else if name.starts_with("typescript@") {
                // Legacy store entry: `lib.*.d.ts` live in `typescript/lib`.
                let pkg = inner.join("typescript");
                let lib = pkg.join("lib");
                if dir_has_intrinsic_libs(&lib) {
                    candidates.push(CandidateLib {
                        version: read_package_version(&pkg),
                        lib_dir: lib,
                        is_rc_platform: false,
                    });
                }
            }
        }
    }

    candidates
}

/// Select the candidate matching the active engine: prefer an exact active-
/// version match (rc platform package winning equal-version ties), else the
/// highest-SEMVER rc platform candidate, else the highest-SEMVER candidate.
#[cfg(not(target_arch = "wasm32"))]
fn select_active_candidate(
    candidates: Vec<CandidateLib>,
    active_version: Option<&str>,
) -> Option<PathBuf> {
    if candidates.is_empty() {
        return None;
    }

    // 1. Exact active-version match. Among equal-version candidates, prefer the
    //    rc per-platform package (its libs are the ones the engine uses).
    if let Some(active) = active_version {
        let matching = candidates
            .iter()
            .filter(|c| c.version.as_deref() == Some(active))
            .max_by_key(|c| c.is_rc_platform);
        if let Some(c) = matching {
            return Some(c.lib_dir.clone());
        }
    }

    // 2/3. No exact match (or no active version known): rank by (rc-platform,
    //      semver). The rc platform package outranks a legacy one; ties break on
    //      the highest installed SEMVER version (NOT lexicographic).
    candidates
        .into_iter()
        .max_by(|a, b| {
            a.is_rc_platform.cmp(&b.is_rc_platform).then_with(|| {
                match (a.version.as_deref(), b.version.as_deref()) {
                    (Some(av), Some(bv)) => compare_semver(av, bv),
                    (Some(_), None) => std::cmp::Ordering::Greater,
                    (None, Some(_)) => std::cmp::Ordering::Less,
                    (None, None) => std::cmp::Ordering::Equal,
                }
            })
        })
        .map(|c| c.lib_dir)
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

    #[cfg(not(target_arch = "wasm32"))]
    fn write_lib(dir: &std::path::Path, name: &str, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), body).unwrap();
    }

    // ── DISCRIMINATING: the rc (`typescript@>=7`) layout. The JS package's own
    //    `lib/` is launcher-only (`getExePath.js`, `tsc.js`) and the real
    //    `lib.*.d.ts` declarations live in the per-platform
    //    `@typescript/typescript-<platform>-<arch>` package. The PRE-rc
    //    discovery accepted the launcher-only `typescript/lib` (`is_dir()`) and
    //    skipped the `+`-named platform store entry — yielding an empty scan.
    //    This fixture fails against that discovery and passes against the
    //    declaration-probing one. ──────────────────────────────────────────────
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn discovers_rc_platform_package_lib_when_hoisted_lib_is_launcher_only() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // The rc JS package: launcher-only `lib/`, no `lib.*.d.ts`.
        let js_lib = root.join("node_modules/typescript/lib");
        write_lib(&js_lib, "tsc.js", "// launcher");
        write_lib(&js_lib, "getExePath.js", "// launcher");

        // The rc per-platform package under the pnpm store (`+`-named entry).
        // The platform segment is arbitrary here on purpose — discovery must
        // not hardcode `win32-x64`; it enumerates and probes for declarations.
        let plat_lib = root.join(
            "node_modules/.pnpm/@typescript+typescript-some-plat@7.0.1-rc/node_modules/@typescript/typescript-some-plat/lib",
        );
        write_lib(&plat_lib, "lib.es5.d.ts", "type Awaited<T> = intrinsic;");
        write_lib(&plat_lib, "lib.dom.d.ts", "// dom");

        let resolved = discover_active_lib_dir_with_rc_flag(root)
            .0
            .expect("rc platform lib must be discovered");
        assert_eq!(
            resolved, plat_lib,
            "discovery must follow to the rc per-platform package's lib"
        );

        let lib = NativeIntrinsicLibrary::discover(root);
        let names = lib.list_intrinsic_libs();
        assert!(
            !names.is_empty(),
            "the rc scan must be non-empty: {names:?}"
        );
        assert!(names.iter().any(|n| n == "lib.es5.d.ts"));
        assert!(lib
            .read_intrinsic_lib("lib.es5.d.ts")
            .unwrap()
            .contains("intrinsic"));
    }

    // ── A launcher-only `typescript/lib` with NO platform package anywhere must
    //    NOT be accepted as the lib dir (it carries zero declarations). The
    //    pre-rc `is_dir()` check would have returned it. ─────────────────────────
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn launcher_only_typescript_lib_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let js_lib = root.join("node_modules/typescript/lib");
        write_lib(&js_lib, "tsc.js", "// launcher");
        write_lib(&js_lib, "getExePath.js", "// launcher");

        assert!(
            discover_active_lib_dir_with_rc_flag(root).0.is_none(),
            "a launcher-only typescript/lib (no lib.*.d.ts) must not satisfy discovery"
        );
        assert!(NativeIntrinsicLibrary::discover(root)
            .list_intrinsic_libs()
            .is_empty());
    }

    // ── The legacy hoisted layout (`typescript@5.x`/`6.x`): `lib.*.d.ts` live
    //    directly in `node_modules/typescript/lib` and must be accepted. ─────────
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn discovers_legacy_hoisted_typescript_lib_with_declarations() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let hoisted = root.join("node_modules/typescript/lib");
        write_lib(&hoisted, "lib.es5.d.ts", "type Awaited<T> = intrinsic;");

        let resolved = discover_active_lib_dir_with_rc_flag(root)
            .0
            .expect("legacy hoisted lib must be found");
        assert_eq!(resolved, hoisted);
    }

    /// Write a minimal `package.json` carrying `name` + `version` for a package
    /// dir (the parent of a `lib/` directory), so discovery can read the owning
    /// package's version.
    #[cfg(not(target_arch = "wasm32"))]
    fn write_pkg(pkg_dir: &std::path::Path, name: &str, version: &str) {
        std::fs::create_dir_all(pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            format!("{{ \"name\": \"{name}\", \"version\": \"{version}\" }}"),
        )
        .unwrap();
    }

    // ── DISCRIMINATING (B5): with BOTH the rc per-platform package
    //    (`@typescript+typescript-*@7.0.1-rc`, the libs that pair with the active
    //    `typescript@7.0.1-rc` engine) AND a higher-LEXICOGRAPHIC legacy
    //    `typescript@6.x` lib dir present, discovery must select the rc libs that
    //    match the ACTIVE engine version — NOT the lexicographically-last path.
    //
    //    The pnpm store dir names sort `@typescript+typescript-…@7.0.1-rc` <
    //    `typescript@6.0.3` (because `@`=0x40 < `t`=0x74), so the old
    //    `candidates.sort(); candidates.pop()` selected the TS6 libs against a
    //    TS7-rc engine. This fixture FAILS on that logic and PASSES on
    //    active-version selection. The platform segment is arbitrary on purpose —
    //    selection must not hardcode `win32-x64`. ────────────────────────────────
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn selects_rc_platform_libs_over_higher_lexicographic_legacy_against_active_engine() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Active engine: the hoisted `typescript` is the launcher-only rc package
        // (its `lib/` carries no declarations) pinned at `7.0.1-rc`.
        let js_pkg = root.join("node_modules/typescript");
        write_pkg(&js_pkg, "typescript", "7.0.1-rc");
        let js_lib = js_pkg.join("lib");
        write_lib(&js_lib, "tsc.js", "// launcher");
        write_lib(&js_lib, "getExePath.js", "// launcher");

        // rc per-platform package (the CORRECT libs): `@`-prefixed pnpm store dir
        // sorts BEFORE `typescript@…`.
        let rc_store =
            root.join("node_modules/.pnpm/@typescript+typescript-some-plat@7.0.1-rc/node_modules");
        let rc_pkg = rc_store.join("@typescript/typescript-some-plat");
        write_pkg(&rc_pkg, "@typescript/typescript-some-plat", "7.0.1-rc");
        let rc_lib = rc_pkg.join("lib");
        write_lib(&rc_lib, "lib.es5.d.ts", "type Awaited<T> = intrinsic;");
        write_lib(&rc_lib, "lib.dom.d.ts", "// dom");

        // Legacy `typescript@6.0.3` libs: sorts AFTER the rc store dir, so the old
        // lexicographic-last logic wrongly selected THIS one.
        let legacy_store = root.join("node_modules/.pnpm/typescript@6.0.3/node_modules");
        let legacy_pkg = legacy_store.join("typescript");
        write_pkg(&legacy_pkg, "typescript", "6.0.3");
        let legacy_lib = legacy_pkg.join("lib");
        write_lib(&legacy_lib, "lib.es5.d.ts", "type Awaited<T> = intrinsic;");
        write_lib(&legacy_lib, "lib.dom.d.ts", "// dom");

        let resolved = discover_active_lib_dir_with_rc_flag(root)
            .0
            .expect("an rc-matching lib dir must be discovered");
        assert_eq!(
            resolved, rc_lib,
            "discovery must select the rc libs matching the active `typescript@7.0.1-rc` engine, \
             not the lexicographically-last legacy `typescript@6.0.3` libs"
        );
        // Negative: the legacy TS6 dir must NOT be selected.
        assert_ne!(
            resolved, legacy_lib,
            "the legacy `typescript@6.0.3` libs must not be selected against a TS7-rc engine"
        );
    }

    // ── B5 fallback: a LEGACY-only workspace (no rc platform package, no active
    //    version readable from the hoisted package) must select the HIGHEST-SEMVER
    //    legacy `typescript@X` libs — NOT the lexicographically-last. `5.8.2`
    //    sorts lexicographically AFTER `5.10.0` (because `8` > `1`), so the old
    //    logic would wrongly pick `5.8.2`; semver ordering picks `5.10.0`. ────────
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn legacy_only_workspace_selects_highest_semver_not_lexicographic() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // No hoisted typescript, no rc platform package — only legacy store
        // entries. Two versions whose semver order differs from lexicographic.
        let older_store = root.join("node_modules/.pnpm/typescript@5.8.2/node_modules");
        let older_pkg = older_store.join("typescript");
        write_pkg(&older_pkg, "typescript", "5.8.2");
        let older_lib = older_pkg.join("lib");
        write_lib(&older_lib, "lib.es5.d.ts", "type Awaited<T> = intrinsic;");

        let newer_store = root.join("node_modules/.pnpm/typescript@5.10.0/node_modules");
        let newer_pkg = newer_store.join("typescript");
        write_pkg(&newer_pkg, "typescript", "5.10.0");
        let newer_lib = newer_pkg.join("lib");
        write_lib(&newer_lib, "lib.es5.d.ts", "type Awaited<T> = intrinsic;");

        let resolved = discover_active_lib_dir_with_rc_flag(root)
            .0
            .expect("legacy-only libs must still be discovered");
        assert_eq!(
            resolved, newer_lib,
            "legacy-only fallback must select the highest-SEMVER libs (5.10.0), not the \
             lexicographically-last (5.8.2)"
        );
    }

    // ── The `rc_platform_package_available` + `selected_lib_is_rc_platform`
    //    queries (the version-discriminating audit's facts) — computed here in
    //    the disk-reading layer so consumers need no `std::fs` of their own. ────
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rc_workspace_reports_rc_available_and_selects_rc_platform() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Launcher-only hoisted rc package + an rc platform package + a legacy
        // TS6 package (the real-world shape that tripped the lexicographic bug).
        let js_pkg = root.join("node_modules/typescript");
        write_pkg(&js_pkg, "typescript", "7.0.1-rc");
        write_lib(&js_pkg.join("lib"), "tsc.js", "// launcher");

        let rc_pkg = root
            .join("node_modules/.pnpm/@typescript+typescript-some-plat@7.0.1-rc/node_modules/@typescript/typescript-some-plat");
        write_pkg(&rc_pkg, "@typescript/typescript-some-plat", "7.0.1-rc");
        write_lib(&rc_pkg.join("lib"), "lib.es5.d.ts", "// rc");

        let legacy_pkg = root.join("node_modules/.pnpm/typescript@6.0.3/node_modules/typescript");
        write_pkg(&legacy_pkg, "typescript", "6.0.3");
        write_lib(&legacy_pkg.join("lib"), "lib.es5.d.ts", "// legacy");

        let lib = NativeIntrinsicLibrary::discover(root);
        assert!(
            lib.rc_platform_package_available(),
            "an rc platform package is present, so it must be reported available"
        );
        assert!(
            lib.selected_lib_is_rc_platform(),
            "the selected lib must be the rc per-platform package, not the legacy TS6 dir: {:?}",
            lib.lib_dir()
        );
    }

    // ── A LEGACY-only hoisted workspace reports rc-UNavailable, and its selected
    //    lib is NOT an rc platform package (the discriminating negative). ────────
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn legacy_hoisted_workspace_reports_rc_unavailable() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let hoisted = root.join("node_modules/typescript/lib");
        write_lib(&hoisted, "lib.es5.d.ts", "// legacy hoisted");

        let lib = NativeIntrinsicLibrary::discover(root);
        assert!(
            !lib.rc_platform_package_available(),
            "a legacy hoisted workspace has no rc platform package"
        );
        assert!(
            !lib.selected_lib_is_rc_platform(),
            "the legacy hoisted lib dir is not an rc per-platform package"
        );
        assert_eq!(lib.lib_dir(), Some(hoisted.as_path()));
    }

    // ── active_typescript_is_tsgo: auto-mode provider selection keys on the
    //    WORKSPACE'S active TypeScript engine (the hoisted `node_modules/typescript`
    //    major version), NOT on whatever bundled/global tsserver the editor
    //    supplies. A hoisted TS7-rc install is a tsgo engine. ─────────────────────
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn active_typescript_is_tsgo_true_for_hoisted_ts7_rc() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(
            &root.join("node_modules/typescript"),
            "typescript",
            "7.0.1-rc",
        );
        assert!(
            NativeIntrinsicLibrary::discover(root).active_typescript_is_tsgo(),
            "a hoisted typescript@7.0.1-rc workspace is a tsgo (TS>=7) engine"
        );
    }

    // ── A hoisted TS6 workspace is NOT tsgo (the major arm returns false). ───────
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn active_typescript_is_tsgo_false_for_hoisted_ts6() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(&root.join("node_modules/typescript"), "typescript", "6.0.3");
        assert!(
            !NativeIntrinsicLibrary::discover(root).active_typescript_is_tsgo(),
            "a hoisted typescript@6.0.3 workspace is NOT a tsgo engine"
        );
    }

    // ── Fallback: when the hoisted `typescript` version is unreadable (launcher-
    //    only rc layout with no readable `package.json`), the decision falls back
    //    to whether the SELECTED active lib is the rc per-platform package. ───────
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn active_typescript_is_tsgo_falls_back_to_rc_platform_when_hoisted_unreadable() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Launcher-only hoisted typescript WITHOUT a package.json (version
        // unreadable) + an rc per-platform package carrying the real libs.
        let js_lib = root.join("node_modules/typescript/lib");
        write_lib(&js_lib, "tsc.js", "// launcher");
        let rc_pkg = root.join(
            "node_modules/.pnpm/@typescript+typescript-some-plat@7.0.1-rc/node_modules/@typescript/typescript-some-plat",
        );
        write_lib(&rc_pkg.join("lib"), "lib.es5.d.ts", "// rc");

        let lib = NativeIntrinsicLibrary::discover(root);
        assert!(
            lib.active_typescript_is_tsgo(),
            "unreadable hoisted version must fall back to selected_lib_is_rc_platform \
             (true here): {:?}",
            lib.lib_dir()
        );
    }

    // ── A legacy-only workspace (no rc platform package, no readable hoisted
    //    version) is NOT tsgo — the rc-platform fallback returns false. ───────────
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn active_typescript_is_tsgo_false_for_legacy_only() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Legacy pnpm store only; no hoisted typescript/package.json, no rc package.
        let legacy_pkg = root.join("node_modules/.pnpm/typescript@5.8.2/node_modules/typescript");
        write_pkg(&legacy_pkg, "typescript", "5.8.2");
        write_lib(&legacy_pkg.join("lib"), "lib.es5.d.ts", "// legacy");

        let lib = NativeIntrinsicLibrary::discover(root);
        assert!(
            !lib.active_typescript_is_tsgo(),
            "a legacy-only (TS5) workspace with no rc platform package is not tsgo"
        );
    }

    // ── Nested-workspace open: a TS7-rc `typescript` hoisted at the MONOREPO ROOT
    //    must be found by walking up from a nested member root, mirroring
    //    find_tsserver's parent walk — so a nested subpackage open is not dragged to
    //    a bundled/global lower tsserver. ──────────────────────────────────────────
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn active_typescript_is_tsgo_walks_up_to_hoisted_ts7_at_monorepo_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // TS7-rc hoisted at the monorepo root; the nested member has NO own typescript.
        write_pkg(
            &root.join("node_modules/typescript"),
            "typescript",
            "7.0.1-rc",
        );
        let member = root.join("packages/app");
        std::fs::create_dir_all(&member).unwrap();
        assert!(
            NativeIntrinsicLibrary::discover(&member).active_typescript_is_tsgo(),
            "a nested member open must walk up to the monorepo-root hoisted typescript@7.0.1-rc \
             and classify the workspace as tsgo"
        );
    }
    // ── Negative: nested member open under a TS6-hoisted monorepo root stays non-tsgo
    //    (the walk must not false-positive). ────────────────────────────────────────
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn active_typescript_is_tsgo_walks_up_but_stays_false_for_hoisted_ts6_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(&root.join("node_modules/typescript"), "typescript", "6.0.3");
        let member = root.join("packages/app");
        std::fs::create_dir_all(&member).unwrap();
        assert!(
            !NativeIntrinsicLibrary::discover(&member).active_typescript_is_tsgo(),
            "a nested member under a TS6-hoisted root is not tsgo"
        );
    }
    // ── Nearest-first: a member with its OWN TS7 typescript wins even under a TS6 root. ──
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn active_typescript_is_tsgo_prefers_nearest_member_typescript() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_pkg(&root.join("node_modules/typescript"), "typescript", "6.0.3");
        let member = root.join("packages/app");
        write_pkg(
            &member.join("node_modules/typescript"),
            "typescript",
            "7.0.1-rc",
        );
        assert!(
            NativeIntrinsicLibrary::discover(&member).active_typescript_is_tsgo(),
            "the nearest (member) typescript@7 wins over the TS6 monorepo root"
        );
    }

    // ── Nested launcher-only rc: a nested member open where the monorepo ROOT
    //    node_modules has the rc per-platform package (real libs) but the hoisted
    //    `typescript` has NO readable package.json must still classify tsgo — the
    //    rc-platform signal resolves over the SAME parent walk as the version read. ──
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn active_typescript_is_tsgo_walks_up_to_rc_platform_when_root_typescript_unreadable() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Root hoisted `typescript` is launcher-only WITHOUT a package.json (version
        // unreadable), and the rc per-platform package carries the real libs.
        write_lib(
            &root.join("node_modules/typescript/lib"),
            "tsc.js",
            "// launcher",
        );
        let rc_pkg = root.join(
            "node_modules/.pnpm/@typescript+typescript-some-plat@7.0.1-rc/node_modules/@typescript/typescript-some-plat",
        );
        write_lib(&rc_pkg.join("lib"), "lib.es5.d.ts", "// rc");
        // Open a NESTED member with no own typescript.
        let member = root.join("packages/app");
        std::fs::create_dir_all(&member).unwrap();
        assert!(
            NativeIntrinsicLibrary::discover(&member).active_typescript_is_tsgo(),
            "a nested member open must walk up and detect the root rc per-platform package \
             even when the hoisted typescript version is unreadable"
        );
    }
}

//! Portable tsgo binary discovery and `--api` spawn-argument construction.
//!
//! Mirrors the JS sync client's spawn (`dist/api/sync/client.js:12-29`): the
//! base args are `["--api", "--cwd", <cwd>]`, followed by
//! `--callbacks=<comma-joined enabled callbacks>` when the host services FS
//! callbacks. On Windows the pipe layer additionally appends `--pipe <path>`
//! (see [`crate::transport`]); that flag is added there, not here, because the
//! pipe path is only known once the pipe is created.
//!
//! Binary discovery mirrors `tools/tsgo-api-gate/run-gate.mjs::discoverTsgo`:
//! the engine is the `typescript@>=7` (rc) distribution's platform binary `tsc`,
//! an optional dependency found under the pnpm `.pnpm` layout or the classic
//! sibling layout. We build every path with [`std::path::PathBuf`] (never string
//! concatenation) and select the binary name platform-aware (`.exe` on Windows)
//! so it stays portable across macOS / Windows / Linux.

use std::path::{Path, PathBuf};

use crate::error::{TsgoApiError, TsgoApiResult};

/// The host-callback names enabled on the wire, in the exact order the JS client
/// emits them (`fs.js:3`). The Verter overlay snapshot services all five, so the
/// enabled set is the full list.
pub const ENABLED_CALLBACKS: &[&str] = &[
    "readFile",
    "fileExists",
    "directoryExists",
    "getAccessibleEntries",
    "realpath",
];

/// Build the base `--api` (sync MessagePack mode) spawn arguments.
///
/// Mirrors `sync/client.js:12-29`. `cwd` is the project working directory the
/// engine resolves relative paths against. When `with_callbacks` is true the
/// `--callbacks=…` flag is appended (the host will service FS callbacks).
///
/// The Windows `--pipe <path>` flag is NOT added here — the pipe layer appends
/// it once the named pipe is created.
pub fn build_sync_api_args(cwd: &str, with_callbacks: bool) -> Vec<String> {
    let mut args = vec!["--api".to_string(), "--cwd".to_string(), cwd.to_string()];
    if with_callbacks {
        args.push(format!("--callbacks={}", ENABLED_CALLBACKS.join(",")));
    }
    args
}

/// One TS≥7 tsgo-engine binary source: the package family + its binary stem.
struct EngineSource {
    /// pnpm `.pnpm` store-entry prefix, e.g. `@typescript+typescript-`.
    pnpm_prefix: &'static str,
    /// classic-sibling `@typescript/<name>` prefix, e.g. `typescript-`.
    sibling_prefix: &'static str,
    /// the binary file stem (no extension): `tsc` for the TS≥7 engine.
    binary_stem: &'static str,
}

/// The TS≥7 tsgo-engine binary source — the installed `typescript@>=7`
/// package's platform binary. Mirrors the gate (`run-gate.mjs::discoverTsgo`):
/// the published `typescript@7.x` (e.g. `7.0.2`) ships the typescript-go
/// engine as `tsc` (renamed from `tsgo`) in `@typescript/typescript-<plat>-<arch>`.
/// This is the SOLE engine source — there is no second channel.
const ENGINE_SOURCES: &[EngineSource] = &[EngineSource {
    pnpm_prefix: "@typescript+typescript-",
    sibling_prefix: "typescript-",
    binary_stem: "tsc",
}];

/// Discover the rc tsgo engine binary under `node_modules` rooted at `workspace_root`.
///
/// Mirrors `run-gate.mjs::discoverTsgo`: searches the pnpm `.pnpm` layout
/// (`node_modules/.pnpm/<scope+name>-<plat>-<arch>@<ver>/node_modules/<scope>/<name>/{lib,bin}/<bin>[.exe]`)
/// and the classic sibling layout (`node_modules/@typescript/<name>-<plat>-<arch>/{lib,bin}/<bin>[.exe]`)
/// for the rc `typescript@>=7` package's `tsc` binary — the sole engine source.
/// Returns the first existing binary.
///
/// This is the production discovery path; callers may also accept an explicit
/// override path (the JS client's `tsserverPath`).
pub fn discover_tsgo(workspace_root: &Path) -> TsgoApiResult<PathBuf> {
    let node_modules = workspace_root.join("node_modules");
    let ext = if cfg!(windows) { ".exe" } else { "" };

    // The sole engine source: the rc `typescript` package's `tsc` binary.
    for source in ENGINE_SOURCES {
        let bin = format!("{}{}", source.binary_stem, ext);
        let mut candidates: Vec<PathBuf> = Vec::new();

        // pnpm `.pnpm` layout (this source's store entries only).
        let pnpm_dir = node_modules.join(".pnpm");
        if let Ok(entries) = std::fs::read_dir(&pnpm_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if !name.starts_with(source.pnpm_prefix) {
                    continue;
                }
                let inner = entry.path().join("node_modules");
                let Ok(scopes) = std::fs::read_dir(&inner) else {
                    continue;
                };
                for scope in scopes.flatten() {
                    let Ok(pkgs) = std::fs::read_dir(scope.path()) else {
                        continue;
                    };
                    for pkg in pkgs.flatten() {
                        candidates.push(pkg.path().join("lib").join(&bin));
                        candidates.push(pkg.path().join("bin").join(&bin));
                    }
                }
            }
        }

        // Classic sibling layout under node_modules/@typescript/ (this source).
        let scope_root = node_modules.join("@typescript");
        if let Ok(siblings) = std::fs::read_dir(&scope_root) {
            for sibling in siblings.flatten() {
                let name = sibling.file_name();
                let name = name.to_string_lossy();
                if !name.starts_with(source.sibling_prefix) {
                    continue;
                }
                candidates.push(sibling.path().join("lib").join(&bin));
                candidates.push(sibling.path().join("bin").join(&bin));
            }
        }

        if let Some(hit) = candidates.into_iter().find(|c| c.is_file()) {
            return Ok(hit);
        }
    }

    Err(TsgoApiError::Spawn(format!(
        "could not discover the rc tsgo engine binary (`tsc`) under {}; \
             install typescript@>=7, or supply an explicit path",
        node_modules.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_api_args_without_callbacks() {
        let args = build_sync_api_args("/repo", false);
        assert_eq!(args, vec!["--api", "--cwd", "/repo"]);
    }

    #[test]
    fn sync_api_args_with_callbacks_lists_all_five_in_order() {
        let args = build_sync_api_args("/repo", true);
        assert_eq!(
            args,
            vec![
                "--api",
                "--cwd",
                "/repo",
                "--callbacks=readFile,fileExists,directoryExists,getAccessibleEntries,realpath",
            ]
        );
    }

    #[test]
    fn callback_order_matches_fs_js() {
        // The wire order is fixed by fs.js:3; guard it against accidental edits.
        assert_eq!(
            ENABLED_CALLBACKS,
            &[
                "readFile",
                "fileExists",
                "directoryExists",
                "getAccessibleEntries",
                "realpath"
            ]
        );
    }

    #[test]
    fn discover_returns_typed_error_when_absent() {
        // A directory with no node_modules yields a typed Spawn error, not a panic.
        let tmp = std::env::temp_dir().join("verter_tsgo_api_discover_absent_test");
        let _ = std::fs::create_dir_all(&tmp);
        let err = discover_tsgo(&tmp).expect_err("no binary under an empty dir");
        assert!(matches!(err, TsgoApiError::Spawn(_)), "got {err:?}");
    }

    // ── NON-VACUOUS when the engine is present in this worktree ─────────────
    #[test]
    fn discover_finds_real_binary_in_workspace() {
        // The worktree's node_modules contains the rc `typescript` win32 binary.
        // Walk up from CARGO_MANIFEST_DIR to the workspace root (where
        // node_modules lives) and assert discovery succeeds when present.
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest
            .parent() // crates/
            .and_then(|p| p.parent()) // repo root
            .expect("workspace root above crates/")
            .to_path_buf();
        if !workspace_root.join("node_modules").exists() {
            // No node_modules in this checkout (e.g. a source-only CI lane):
            // discovery legitimately cannot find a binary. Assert the typed
            // error rather than vacuously passing.
            assert!(discover_tsgo(&workspace_root).is_err());
            return;
        }
        match discover_tsgo(&workspace_root) {
            Ok(path) => {
                assert!(
                    path.is_file(),
                    "discovered path must be a real file: {path:?}"
                );
                let stem = path.file_name().unwrap().to_string_lossy();
                assert!(
                    stem.starts_with("tsc"),
                    "discovered the rc `tsc` engine binary: {path:?}"
                );
                assert!(
                    !stem.starts_with("tsgo"),
                    "rc-only discovery must NOT resolve a native-preview `tsgo` binary: {path:?}"
                );
            }
            Err(e) => panic!("node_modules present but discovery failed: {e}"),
        }
    }

    // ── rc-only discovery: the installed typescript@>=7 `tsc` binary is the sole
    // engine. The published typescript@7.x ships the typescript-go binary as
    // `tsc` in `@typescript/typescript-<plat>-<arch>`.

    /// Build a flat-npm `@typescript/<pkg>/lib/<bin><ext>` binary under a fake
    /// workspace root and return the workspace root + the binary path.
    fn materialize_flat(
        tag: &str,
        scope_pkg: &str,
        bin_stem: &str,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "verter_tsgo_api_rc_{}_{}_{}",
            tag,
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let ext = if cfg!(windows) { ".exe" } else { "" };
        let bin = root
            .join("node_modules")
            .join("@typescript")
            .join(scope_pkg)
            .join("lib")
            .join(format!("{bin_stem}{ext}"));
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::write(&bin, bin_stem).unwrap();
        (root, bin)
    }

    // ── DISCRIMINATING: discovery is rc-ONLY — there is NO native-preview path.
    //    `ENGINE_SOURCES` holds exactly the single rc `tsc` source, so a
    //    native-preview `tsgo` install is NOT discovered. This fails while a
    //    native-preview `EngineSource` is present and passes once it is gone. ───
    #[test]
    fn engine_sources_is_rc_only_no_native_preview() {
        assert_eq!(
            ENGINE_SOURCES.len(),
            1,
            "discovery is rc-only: exactly one engine source"
        );
        assert_eq!(
            ENGINE_SOURCES[0].binary_stem, "tsc",
            "the sole engine source is the rc `typescript` package's `tsc` binary"
        );
        // NEGATIVE: no source resolves a `tsgo` (native-preview) binary.
        assert!(
            !ENGINE_SOURCES.iter().any(|s| s.binary_stem == "tsgo"),
            "no native-preview `tsgo` source may remain"
        );
        assert!(
            !ENGINE_SOURCES
                .iter()
                .any(|s| s.pnpm_prefix.contains("native-preview")
                    || s.sibling_prefix.contains("native-preview")),
            "no native-preview package source may remain"
        );
    }

    #[test]
    fn discover_finds_rc_tsc_when_only_typescript_installed() {
        let (root, bin) = materialize_flat("rc_only", "typescript-test-plat", "tsc");
        let found = discover_tsgo(&root).expect("rc tsc must be discovered");
        assert_eq!(found, bin, "the rc `tsc` binary must be discoverable alone");
        let _ = std::fs::remove_dir_all(&root);
    }

    // ── DISCRIMINATING (negative): a native-preview `tsgo` install is NO LONGER
    //    discovered — rc-only discovery ignores it entirely. ──────────────────
    #[test]
    fn discover_ignores_native_preview_tsgo() {
        let (root, _bin) = materialize_flat("np_only", "native-preview-test-plat", "tsgo");
        assert!(
            discover_tsgo(&root).is_err(),
            "a native-preview `tsgo` install must NOT be discovered (rc-only)"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

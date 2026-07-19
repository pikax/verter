//! The bundled offline fallback: sidecar LOCATION + fallback CONTRACT.
//!
//! Phase A defines the contract; Phase B ships the actual sidecar binary.
//! The layout is derived SOLELY from the running executable's directory —
//! never from the process CWD, npm package names, or string-concatenated
//! separators — and is identical for `verter-tsc`, the LSP binary, and future
//! MCP distributions:
//!
//! ```text
//! <host-exe-dir>/tsgo/lib/tsc[.exe]            — the engine sidecar
//! <host-exe-dir>/tsgo/verter-tsgo-bundle.json  — the integrity manifest (Phase B)
//! ```
//!
//! The contract: a bundled binary that EXISTS but fails validation is a
//! PRODUCT-INTEGRITY failure (the installed product is corrupt), not a "no
//! provider" outcome. An absent sidecar simply means the offline floor was
//! not shipped (e.g. a dev checkout) and the resolver reports the tiers it
//! searched.

use std::path::{Path, PathBuf};

use super::platform::{host_platform, TsgoPlatform};
use super::policy::{TsgoVersion, BUNDLED_TSGO_VERSION};

/// The integrity manifest file name inside the `tsgo/` sidecar directory.
/// Phase B writes it (digest + provenance for the shipped binary); Phase A
/// fixes its location so packager and validator cannot drift.
pub const BUNDLE_INTEGRITY_MANIFEST_FILE: &str = "verter-tsgo-bundle.json";

/// The bundled tsgo version: the pinned supported floor the sidecar ships.
pub fn bundled_version() -> TsgoVersion {
    BUNDLED_TSGO_VERSION.clone()
}

/// The bundled sidecar path for the host platform, derived SOLELY from the
/// running executable's location: `<host-exe-dir>/tsgo/lib/tsc[.exe]`.
///
/// `host_exe` is the current executable (`std::env::current_exe` at the call
/// site — injected so tests never depend on the real binary). Returns `None`
/// on an unsupported host target (never a guess).
pub fn bundled_tsgo_path(host_exe: &Path) -> Option<PathBuf> {
    host_platform().map(|platform| bundled_tsgo_path_for(host_exe, platform))
}

/// The bundled sidecar path for an explicit platform — the testable core of
/// [`bundled_tsgo_path`].
pub fn bundled_tsgo_path_for(host_exe: &Path, platform: &TsgoPlatform) -> PathBuf {
    host_exe
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(platform.bundled_executable_rel_path())
}

/// The Phase B integrity manifest path, alongside the sidecar:
/// `<host-exe-dir>/tsgo/verter-tsgo-bundle.json`.
pub fn bundle_integrity_manifest_path(host_exe: &Path) -> Option<PathBuf> {
    host_exe
        .parent()
        .map(|dir| dir.join("tsgo").join(BUNDLE_INTEGRITY_MANIFEST_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolchain::platform::host_platform;
    use crate::toolchain::policy::VersionPolicy;
    use std::path::{Path, PathBuf};

    // ── DISCRIMINATING: the sidecar path derives ONLY from the host exe's
    //    directory — any exe name, any dir — via Path components. ────────────
    #[test]
    fn bundled_path_is_derived_from_the_host_exe_dir_only() {
        let host = host_platform().unwrap();
        for exe in [
            PathBuf::from("/dist/bin").join("verter-tsc"),
            PathBuf::from("/opt/verter").join("verter-lsp"),
            PathBuf::from("relative/bin").join("verter-mcp"),
        ] {
            let expected = exe
                .parent()
                .unwrap()
                .join("tsgo")
                .join("lib")
                .join(host.executable);
            assert_eq!(bundled_tsgo_path_for(&exe, host), expected);
        }
    }

    // ── DISCRIMINATING: the path is built from Path COMPONENTS — the
    //    sidecar-relative part is exactly [tsgo, lib, <exe>] with no embedded
    //    separators baked into a single component. ────────────────────────────
    #[test]
    fn bundled_path_uses_path_components_not_concatenated_separators() {
        let host = host_platform().unwrap();
        let exe = PathBuf::from("/x/bin").join("verter-tsc");
        let path = bundled_tsgo_path_for(&exe, host);
        let tail: Vec<_> = path
            .components()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|c| c.as_os_str().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            tail,
            vec![
                std::ffi::OsStr::new("tsgo"),
                std::ffi::OsStr::new("lib"),
                std::ffi::OsStr::new(host.executable)
            ]
        );
    }

    // ── DISCRIMINATING: the bundled version IS the pinned policy floor, and
    //    the production policy accepts it — the shipped offline floor can
    //    never be self-refusing. ──────────────────────────────────────────────
    #[test]
    fn bundled_version_is_the_supported_floor() {
        use crate::toolchain::policy::BUNDLED_TSGO_VERSION;
        assert_eq!(bundled_version(), BUNDLED_TSGO_VERSION);
        VersionPolicy::production()
            .check(&bundled_version())
            .expect("the bundled floor must satisfy the production policy");
    }

    // ── the host-derived convenience entry point ────────────────────────────
    #[test]
    fn bundled_tsgo_path_resolves_for_the_host_target() {
        let exe = PathBuf::from("/x/bin").join("verter-tsc");
        let derived = bundled_tsgo_path(&exe).expect("host target is supported");
        assert_eq!(
            derived,
            bundled_tsgo_path_for(&exe, host_platform().unwrap())
        );
    }

    // ── the Phase B integrity manifest location is fixed relative to the
    //    same exe dir (the packager and the validator cannot drift). ─────────
    #[test]
    fn integrity_manifest_path_sits_alongside_the_sidecar() {
        let exe = PathBuf::from("/x/bin").join("verter-tsc");
        let manifest = bundle_integrity_manifest_path(&exe).unwrap();
        assert_eq!(
            manifest,
            Path::new("/x/bin")
                .join("tsgo")
                .join(BUNDLE_INTEGRITY_MANIFEST_FILE)
        );
        assert!(BUNDLE_INTEGRITY_MANIFEST_FILE.ends_with(".json"));
    }
}

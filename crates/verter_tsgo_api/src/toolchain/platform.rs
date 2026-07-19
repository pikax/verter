//! The per-platform tsgo binary mapping — ONE manifest, zero drift.
//!
//! Every consumer of platform knowledge (tier-2 local discovery, the temp
//! cache layout, the bundled sidecar path, the Phase B packager) derives from
//! [`PLATFORM_MANIFEST`] below. No other code may hardcode a per-OS binary
//! name, package suffix, or target triple.

use std::path::PathBuf;

/// One supported desktop target: the npm package tokens, the engine binary
/// name, and the Rust target triple — the single source every path derives
/// from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TsgoPlatform {
    /// The npm `os` token (`darwin` / `linux` / `win32`).
    pub npm_os: &'static str,
    /// The npm `cpu` token (`x64` / `arm64`).
    pub npm_arch: &'static str,
    /// The scoped platform package (`@typescript/typescript-<os>-<arch>`).
    pub npm_package: &'static str,
    /// The pnpm store entry prefix (`@typescript+typescript-<os>-<arch>`).
    pub pnpm_store_entry: &'static str,
    /// The engine binary inside the package's `lib/` (`tsc` / `tsc.exe`).
    pub executable: &'static str,
    /// The `node_modules/.bin` shim name (`tsc` / `tsc.cmd`).
    pub bin_shim: &'static str,
    /// The Rust target triple (the temp-cache directory dimension).
    pub target_triple: &'static str,
}

impl TsgoPlatform {
    /// `lib/tsc[.exe]` — the engine binary relative to its package root.
    pub fn lib_executable_rel_path(&self) -> PathBuf {
        PathBuf::from("lib").join(self.executable)
    }

    /// `@typescript/typescript-<os>-<arch>` as path components — the package
    /// root relative to a `node_modules/` directory.
    pub fn package_rel_path(&self) -> PathBuf {
        PathBuf::from("@typescript").join(format!("typescript-{}-{}", self.npm_os, self.npm_arch))
    }

    /// `tsgo/lib/tsc[.exe]` — the bundled sidecar relative to the host
    /// executable's directory (the Phase B offline floor).
    pub fn bundled_executable_rel_path(&self) -> PathBuf {
        PathBuf::from("tsgo").join("lib").join(self.executable)
    }
}

/// The ONE manifest: every supported desktop target. Generated consumers
/// (resolver, bundle, cache, packaging) MUST derive from this table. A
/// `static` (not `const`) so there is exactly ONE materialization — entry
/// identity (`ptr::eq`) is meaningful.
pub static PLATFORM_MANIFEST: &[TsgoPlatform] = &[
    TsgoPlatform {
        npm_os: "darwin",
        npm_arch: "arm64",
        npm_package: "@typescript/typescript-darwin-arm64",
        pnpm_store_entry: "@typescript+typescript-darwin-arm64",
        executable: "tsc",
        bin_shim: "tsc",
        target_triple: "aarch64-apple-darwin",
    },
    TsgoPlatform {
        npm_os: "darwin",
        npm_arch: "x64",
        npm_package: "@typescript/typescript-darwin-x64",
        pnpm_store_entry: "@typescript+typescript-darwin-x64",
        executable: "tsc",
        bin_shim: "tsc",
        target_triple: "x86_64-apple-darwin",
    },
    TsgoPlatform {
        npm_os: "linux",
        npm_arch: "x64",
        npm_package: "@typescript/typescript-linux-x64",
        pnpm_store_entry: "@typescript+typescript-linux-x64",
        executable: "tsc",
        bin_shim: "tsc",
        target_triple: "x86_64-unknown-linux-gnu",
    },
    TsgoPlatform {
        npm_os: "linux",
        npm_arch: "arm64",
        npm_package: "@typescript/typescript-linux-arm64",
        pnpm_store_entry: "@typescript+typescript-linux-arm64",
        executable: "tsc",
        bin_shim: "tsc",
        target_triple: "aarch64-unknown-linux-gnu",
    },
    TsgoPlatform {
        npm_os: "win32",
        npm_arch: "x64",
        npm_package: "@typescript/typescript-win32-x64",
        pnpm_store_entry: "@typescript+typescript-win32-x64",
        executable: "tsc.exe",
        bin_shim: "tsc.cmd",
        target_triple: "x86_64-pc-windows-msvc",
    },
    TsgoPlatform {
        npm_os: "win32",
        npm_arch: "arm64",
        npm_package: "@typescript/typescript-win32-arm64",
        pnpm_store_entry: "@typescript+typescript-win32-arm64",
        executable: "tsc.exe",
        bin_shim: "tsc.cmd",
        target_triple: "aarch64-pc-windows-msvc",
    },
];

/// The manifest entry for a concrete npm os/arch token pair.
pub fn platform_for(npm_os: &str, npm_arch: &str) -> Option<&'static TsgoPlatform> {
    PLATFORM_MANIFEST
        .iter()
        .find(|p| p.npm_os == npm_os && p.npm_arch == npm_arch)
}

/// The manifest entry for the host this binary was compiled for, or `None` on
/// an unsupported target (callers record a diagnostic rather than guess).
pub fn host_platform() -> Option<&'static TsgoPlatform> {
    let npm_os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "win32"
    } else {
        return None;
    };
    let npm_arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        return None;
    };
    platform_for(npm_os, npm_arch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── DISCRIMINATING: the manifest covers exactly the six supported
    //    desktop targets — a dropped target fails the count, an added one
    //    fails it too. ───────────────────────────────────────────────────────
    #[test]
    fn manifest_covers_exactly_the_six_supported_targets() {
        assert_eq!(PLATFORM_MANIFEST.len(), 6);
        let mut pairs: Vec<(&str, &str)> = PLATFORM_MANIFEST
            .iter()
            .map(|p| (p.npm_os, p.npm_arch))
            .collect();
        pairs.sort_unstable();
        assert_eq!(
            pairs,
            vec![
                ("darwin", "arm64"),
                ("darwin", "x64"),
                ("linux", "arm64"),
                ("linux", "x64"),
                ("win32", "arm64"),
                ("win32", "x64"),
            ]
        );
    }

    // ── DISCRIMINATING: the host lookup resolves against cfg!(target_os) /
    //    cfg!(target_arch) of the running test binary, and round-trips through
    //    platform_for. ────────────────────────────────────────────────────────
    #[test]
    fn host_platform_matches_the_running_target() {
        let host = host_platform().expect("the test target must be a supported platform");
        let expected_os = if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "windows") {
            "win32"
        } else {
            panic!("unsupported test target os")
        };
        let expected_arch = if cfg!(target_arch = "x86_64") {
            "x64"
        } else if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            panic!("unsupported test target arch")
        };
        assert_eq!(host.npm_os, expected_os);
        assert_eq!(host.npm_arch, expected_arch);
        assert!(
            std::ptr::eq(host, platform_for(expected_os, expected_arch).unwrap()),
            "platform_for must return the same manifest entry"
        );
        assert!(platform_for("plan9", "mips").is_none());
    }

    // ── DISCRIMINATING: every derived string comes from the ONE manifest row —
    //    package name, pnpm store prefix, executable name, bin shim, target
    //    triple. A row edited inconsistently fails here. ──────────────────────
    #[test]
    fn manifest_rows_are_internally_consistent() {
        for row in PLATFORM_MANIFEST {
            let suffix = format!("{}-{}", row.npm_os, row.npm_arch);
            assert_eq!(
                row.npm_package,
                format!("@typescript/typescript-{suffix}"),
                "npm package name drift for {suffix}"
            );
            assert_eq!(
                row.pnpm_store_entry,
                format!("@typescript+typescript-{suffix}"),
                "pnpm store entry drift for {suffix}"
            );
            assert_eq!(
                row.executable,
                if row.npm_os == "win32" {
                    "tsc.exe"
                } else {
                    "tsc"
                },
                "executable name drift for {suffix}"
            );
            assert_eq!(
                row.bin_shim,
                if row.npm_os == "win32" {
                    "tsc.cmd"
                } else {
                    "tsc"
                },
                ".bin shim name drift for {suffix}"
            );
            // The target triple names the same platform the npm tokens name.
            let triple = row.target_triple;
            match row.npm_os {
                "darwin" => assert!(triple.contains("apple-darwin"), "{triple}"),
                "linux" => assert!(triple.contains("unknown-linux-gnu"), "{triple}"),
                "win32" => assert!(triple.contains("pc-windows-msvc"), "{triple}"),
                other => panic!("unexpected npm_os {other}"),
            }
            match row.npm_arch {
                "x64" => assert!(triple.starts_with("x86_64-"), "{triple}"),
                "arm64" => assert!(triple.starts_with("aarch64-"), "{triple}"),
                other => panic!("unexpected npm_arch {other}"),
            }
        }
    }

    // ── DISCRIMINATING: relative paths are built from Path components (never
    //    string-concatenated separators) and use the manifest executable. ─────
    #[test]
    fn derived_relative_paths_are_path_built() {
        let host = host_platform().unwrap();

        let lib_rel = host.lib_executable_rel_path();
        let components: Vec<_> = lib_rel.components().collect();
        assert_eq!(components.len(), 2, "lib/<exe>: {lib_rel:?}");
        assert_eq!(components[0].as_os_str(), "lib");
        assert_eq!(components[1].as_os_str(), host.executable);
        assert_eq!(lib_rel, Path::new("lib").join(host.executable));

        let pkg_rel = host.package_rel_path();
        assert_eq!(
            pkg_rel,
            Path::new("@typescript").join(format!("typescript-{}-{}", host.npm_os, host.npm_arch))
        );

        let bundle_rel = host.bundled_executable_rel_path();
        assert_eq!(
            bundle_rel,
            Path::new("tsgo").join("lib").join(host.executable),
            "the bundled sidecar lives at tsgo/lib/<exe> relative to the host exe dir"
        );
    }

    // ── DISCRIMINATING: the Windows row spells the executable/shim with the
    //    Windows extensions; no other row does. ───────────────────────────────
    #[test]
    fn windows_rows_carry_exe_and_cmd_extensions() {
        for row in PLATFORM_MANIFEST {
            if row.npm_os == "win32" {
                assert!(row.executable.ends_with(".exe"), "{row:?}");
                assert!(row.bin_shim.ends_with(".cmd"), "{row:?}");
            } else {
                assert!(!row.executable.ends_with(".exe"), "{row:?}");
                assert!(!row.bin_shim.ends_with(".cmd"), "{row:?}");
            }
        }
    }
}

//! Pure platform matrix: neutral `(Os, Arch)` → Rust target triple, the
//! `verter-lsp` binary file name, and the GitHub release asset name.
//!
//! The enums here are intentionally NEUTRAL: the host editor maps its own
//! platform signal (`zed::current_platform()`, or Lapce's `VOLT_OS` / `VOLT_ARCH`
//! env vars) onto them. This module imports no host types and performs no IO.
//!
//! Unknown tuples fail loud ([`UnsupportedPlatform`]) rather than guessing — a
//! wasm sandbox gives no reliable libc signal, so Linux is pinned to the static
//! `musl` triples instead of guessing `gnu` vs `musl`.

use std::fmt;

/// Neutral operating-system selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Os {
    /// macOS (`*-apple-darwin`).
    Mac,
    /// Linux (`*-unknown-linux-musl`).
    Linux,
    /// Windows (`*-pc-windows-msvc`).
    Windows,
}

/// Neutral CPU-architecture selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arch {
    /// 64-bit x86 (`x86_64-*`).
    X86_64,
    /// 64-bit ARM (`aarch64-*`).
    Aarch64,
}

/// A resolved Rust target triple for a supported `(Os, Arch)` tuple.
///
/// Wraps a `&'static str` so callers cannot fabricate an arbitrary triple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TargetTriple(&'static str);

impl TargetTriple {
    /// The triple as a string slice (e.g. `"x86_64-pc-windows-msvc"`).
    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

impl fmt::Display for TargetTriple {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Error returned when a platform tuple / host string is not supported, or when
/// a caller-supplied release version is not safe to interpolate into an asset
/// name. Carries the offending inputs so the host can surface a precise message
/// instead of guessing a binary or producing an unsafe on-disk name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnsupportedPlatform {
    /// The requested `(os, arch)` tuple (or host string pair) is not supported.
    UnsupportedTuple {
        /// The OS token (a neutral-enum debug name, or the raw host string).
        os: String,
        /// The arch token (a neutral-enum debug name, or the raw host string).
        arch: String,
    },
    /// The release `version` is empty, contains an NTFS-illegal character
    /// (`< > : " | ? * \`, `/`, or a control char), or ends with a dot or space
    /// — any of which would yield an ambiguous/unsafe on-disk asset name.
    InvalidVersion {
        /// The offending version string the caller supplied.
        version: String,
    },
}

impl fmt::Display for UnsupportedPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnsupportedPlatform::UnsupportedTuple { os, arch } => {
                write!(f, "unsupported platform: os={os:?}, arch={arch:?}")
            }
            UnsupportedPlatform::InvalidVersion { version } => {
                write!(
                    f,
                    "invalid release version {version:?}: empty, contains an \
                     NTFS-illegal character, or ends with a dot/space"
                )
            }
        }
    }
}

impl std::error::Error for UnsupportedPlatform {}

/// Whether `version` is safe to interpolate into an on-disk asset name.
///
/// Rejects: an empty string; any NTFS-illegal character (`< > : " | ? * \`, plus
/// `/` and ASCII control chars); and a trailing dot or space (both are stripped
/// or rejected by Windows path handling, producing an ambiguous name).
fn version_is_safe(version: &str) -> bool {
    if version.is_empty() {
        return false;
    }
    if version.ends_with('.') || version.ends_with(' ') {
        return false;
    }
    const ILLEGAL: [char; 9] = ['<', '>', ':', '"', '|', '?', '*', '\\', '/'];
    !version
        .chars()
        .any(|c| c.is_control() || ILLEGAL.contains(&c))
}

/// Parse a host-supplied `(os, arch)` string pair into the neutral enums.
///
/// Accepts the common spellings emitted by Zed (`zed::current_platform()` →
/// `"mac" | "linux" | "windows"`, `"x8664" | "x86_64" | "aarch64"`) and Lapce
/// (`VOLT_OS` / `VOLT_ARCH` → `"macos" | "linux" | "windows"`, `"x86_64" | "arm64"`).
/// Matching is case-insensitive. An unrecognised token fails loud with
/// [`UnsupportedPlatform`] — never a guess.
pub fn from_host(os: &str, arch: &str) -> Result<(Os, Arch), UnsupportedPlatform> {
    let parsed_os = match os.to_ascii_lowercase().as_str() {
        "mac" | "macos" | "darwin" => Some(Os::Mac),
        "linux" => Some(Os::Linux),
        "windows" | "win" => Some(Os::Windows),
        _ => None,
    };
    let parsed_arch = match arch.to_ascii_lowercase().as_str() {
        "x86_64" | "x8664" | "x64" | "amd64" => Some(Arch::X86_64),
        "aarch64" | "arm64" => Some(Arch::Aarch64),
        _ => None,
    };
    match (parsed_os, parsed_arch) {
        (Some(os), Some(arch)) => Ok((os, arch)),
        _ => Err(UnsupportedPlatform::UnsupportedTuple {
            os: os.to_string(),
            arch: arch.to_string(),
        }),
    }
}

/// Map a supported `(Os, Arch)` tuple to its Rust target triple.
///
/// The matrix is total over the neutral enums; it is a `Result` so that adding a
/// future enum variant without a matrix row fails loud rather than guessing.
pub fn target_triple(os: Os, arch: Arch) -> Result<TargetTriple, UnsupportedPlatform> {
    // Linux is pinned to static `musl` triples: the wasm sandbox gives no
    // reliable libc signal, so we never guess `gnu` vs `musl`.
    let triple = match (os, arch) {
        (Os::Mac, Arch::X86_64) => "x86_64-apple-darwin",
        (Os::Mac, Arch::Aarch64) => "aarch64-apple-darwin",
        (Os::Windows, Arch::X86_64) => "x86_64-pc-windows-msvc",
        (Os::Windows, Arch::Aarch64) => "aarch64-pc-windows-msvc",
        (Os::Linux, Arch::X86_64) => "x86_64-unknown-linux-musl",
        (Os::Linux, Arch::Aarch64) => "aarch64-unknown-linux-musl",
    };
    Ok(TargetTriple(triple))
}

/// The `verter-lsp` executable file name for the given OS.
///
/// `"verter-lsp.exe"` on Windows, `"verter-lsp"` elsewhere.
pub fn binary_file_name(os: Os) -> &'static str {
    match os {
        Os::Windows => "verter-lsp.exe",
        Os::Mac | Os::Linux => "verter-lsp",
    }
}

/// The archive extension for a release asset on the given OS.
///
/// `zip` on Windows, `tar.gz` elsewhere. Both are NTFS-safe.
fn archive_extension(os: Os) -> &'static str {
    match os {
        Os::Windows => "zip",
        Os::Mac | Os::Linux => "tar.gz",
    }
}

/// Build the GitHub release asset name for a platform + version.
///
/// Format: `verter-lsp-v<version>-<triple>.<ext>` where `<ext>` is `zip` on
/// Windows and `tar.gz` elsewhere. The result is NTFS-safe (no `: < > " | ? *`).
///
/// `version` is caller-supplied, so it is validated before interpolation: an
/// empty version, one containing an NTFS-illegal character, or one ending in a
/// dot/space is rejected with [`UnsupportedPlatform::InvalidVersion`] rather than
/// producing an unsafe/ambiguous on-disk name. The triple and extension are
/// fixed identifiers and are always safe.
pub fn asset_name(os: Os, arch: Arch, version: &str) -> Result<String, UnsupportedPlatform> {
    if !version_is_safe(version) {
        return Err(UnsupportedPlatform::InvalidVersion {
            version: version.to_string(),
        });
    }
    let triple = target_triple(os, arch)?;
    let ext = archive_extension(os);
    // version validated above; triple is a fixed identifier; extension is
    // `zip`/`tar.gz` — the result is NTFS-safe.
    Ok(format!("verter-lsp-v{version}-{}.{ext}", triple.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_known_tuples_map_to_correct_triples() {
        let cases = [
            (Os::Mac, Arch::X86_64, "x86_64-apple-darwin"),
            (Os::Mac, Arch::Aarch64, "aarch64-apple-darwin"),
            (Os::Windows, Arch::X86_64, "x86_64-pc-windows-msvc"),
            (Os::Windows, Arch::Aarch64, "aarch64-pc-windows-msvc"),
            (Os::Linux, Arch::X86_64, "x86_64-unknown-linux-musl"),
            (Os::Linux, Arch::Aarch64, "aarch64-unknown-linux-musl"),
        ];
        for (os, arch, expected) in cases {
            let triple = target_triple(os, arch).expect("known tuple resolves");
            assert_eq!(triple.as_str(), expected, "for {os:?}/{arch:?}");
        }
    }

    #[test]
    fn binary_file_name_is_exe_only_on_windows() {
        assert!(binary_file_name(Os::Windows).ends_with(".exe"));
        assert_eq!(binary_file_name(Os::Windows), "verter-lsp.exe");
        for os in [Os::Mac, Os::Linux] {
            let name = binary_file_name(os);
            assert!(
                !name.ends_with(".exe"),
                "{os:?} name must have no .exe: {name}"
            );
            assert_eq!(name, "verter-lsp");
        }
    }

    #[test]
    fn asset_names_are_ntfs_safe_and_carry_version_and_triple() {
        let version = "1.2.3";
        let all = [
            (Os::Mac, Arch::X86_64),
            (Os::Mac, Arch::Aarch64),
            (Os::Windows, Arch::X86_64),
            (Os::Windows, Arch::Aarch64),
            (Os::Linux, Arch::X86_64),
            (Os::Linux, Arch::Aarch64),
        ];
        for (os, arch) in all {
            let name = asset_name(os, arch, version).expect("known tuple");
            // NTFS-illegal characters must be absent.
            for ch in ['<', '>', ':', '"', '|', '?', '*', '\\'] {
                assert!(
                    !name.contains(ch),
                    "asset name {name:?} contains NTFS-illegal {ch:?}"
                );
            }
            // No trailing dot/space.
            assert!(
                !name.ends_with('.') && !name.ends_with(' '),
                "bad tail: {name:?}"
            );
            // Carries version + triple, never `latest`.
            assert!(name.contains(version), "{name:?} missing version");
            let triple = target_triple(os, arch).unwrap();
            assert!(name.contains(triple.as_str()), "{name:?} missing triple");
            assert!(
                !name.contains("latest"),
                "{name:?} must not contain `latest`"
            );
            // Correct extension per OS.
            if os == Os::Windows {
                assert!(name.ends_with(".zip"), "windows asset must be .zip: {name}");
            } else {
                assert!(
                    name.ends_with(".tar.gz"),
                    "non-windows asset must be .tar.gz: {name}"
                );
            }
        }
    }

    #[test]
    fn asset_name_rejects_unsafe_versions() {
        // F8: a caller-supplied `version` is interpolated into an on-disk asset
        // name, so an NTFS-unsafe/ambiguous version must be REJECTED, not produce
        // a malformed name. Every supported tuple must reject identically.
        let unsafe_versions = [
            "",       // empty
            "1:2",    // NTFS-illegal ':'
            "1*2",    // NTFS-illegal '*'
            "1|2",    // NTFS-illegal '|'
            "1?2",    // NTFS-illegal '?'
            "1\"2",   // NTFS-illegal '"'
            "1<2",    // NTFS-illegal '<'
            "1>2",    // NTFS-illegal '>'
            "1\\2",   // NTFS-illegal '\'
            "../x",   // path traversal via '/'
            "1.2.3 ", // trailing space
            "1.2.3.", // trailing dot
            "1.2\n3", // control char
        ];
        for version in unsafe_versions {
            let err = asset_name(Os::Linux, Arch::X86_64, version).unwrap_err();
            assert_eq!(
                err,
                UnsupportedPlatform::InvalidVersion {
                    version: version.to_string(),
                },
                "version {version:?} must be rejected as InvalidVersion"
            );
        }
    }

    #[test]
    fn asset_name_accepts_valid_version() {
        // A plain semver passes and yields an NTFS-safe name (the matrix test
        // below proves safety across all tuples; this pins the Ok path for F8).
        let name = asset_name(Os::Windows, Arch::X86_64, "1.2.3").expect("valid version");
        assert!(name.contains("1.2.3"));
        assert!(name.ends_with(".zip"));
    }

    #[test]
    fn from_host_parses_known_spellings() {
        // Zed-style
        assert_eq!(
            from_host("mac", "aarch64").unwrap(),
            (Os::Mac, Arch::Aarch64)
        );
        assert_eq!(
            from_host("linux", "x86_64").unwrap(),
            (Os::Linux, Arch::X86_64)
        );
        assert_eq!(
            from_host("windows", "x8664").unwrap(),
            (Os::Windows, Arch::X86_64)
        );
        // Lapce-style
        assert_eq!(
            from_host("macos", "arm64").unwrap(),
            (Os::Mac, Arch::Aarch64)
        );
        assert_eq!(
            from_host("windows", "x86_64").unwrap(),
            (Os::Windows, Arch::X86_64)
        );
        // case-insensitive
        assert_eq!(
            from_host("WINDOWS", "AArch64").unwrap(),
            (Os::Windows, Arch::Aarch64)
        );
    }

    #[test]
    fn from_host_unknown_fails_loud() {
        // Unknown OS, unknown arch, and a 32-bit arch we deliberately do not support
        // must all error — never silently guess a binary.
        assert!(from_host("solaris", "x86_64").is_err());
        assert!(from_host("linux", "riscv64").is_err());
        assert!(from_host("linux", "x86").is_err());
        assert!(from_host("", "").is_err());
        // The error carries the offending tokens.
        let err = from_host("plan9", "sparc").unwrap_err();
        match err {
            UnsupportedPlatform::UnsupportedTuple { os, arch } => {
                assert!(os.contains("plan9") || arch.contains("sparc"));
            }
            other => panic!("expected UnsupportedTuple, got {other:?}"),
        }
    }
}

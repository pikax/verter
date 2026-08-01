//! Node.js and TypeScript binary discovery helpers.
//!
//! Moved from `verter_lsp::tsserver::mod` to be shared between LSP and
//! component-meta consumers.

use std::fmt;
use std::path::{Path, PathBuf};

/// Detect the major TypeScript version from the workspace.
///
/// Reads `<tsserver_path>/../../package.json` to extract the `version` field.
/// Returns `Some(major)` (e.g., `5` for TypeScript 5.x) or `None` if unreadable.
pub fn detect_ts_major_version(tsserver_path: &Path) -> Option<u32> {
    detect_ts_version(tsserver_path).map(|(major, _)| major)
}

/// Detect the `(major, minor)` TypeScript version of the install a
/// `tsserver.js` belongs to (e.g. `(5, 9)` for TypeScript 5.9.3), or `None`
/// if the install's `package.json` is unreadable or unparseable.
pub fn detect_ts_version(tsserver_path: &Path) -> Option<(u32, u32)> {
    // tsserver.js lives in typescript/lib/ — go up twice to get typescript/
    let ts_root = tsserver_path.parent()?.parent()?;
    let pkg_json = ts_root.join("package.json");
    let content = std::fs::read_to_string(pkg_json).ok()?;
    // Simple extraction: find `"version": "X.Y.Z"` — no serde needed
    let version_key = content.find("\"version\"")?;
    let after = &content[version_key..];
    let colon = after.find(':')?;
    let after_colon = after[colon + 1..].trim_start();
    let quote_start = after_colon.find('"')? + 1;
    let version_str = &after_colon[quote_start..];
    let quote_end = version_str.find('"')?;
    let version = &version_str[..quote_end];
    let mut components = version.split('.');
    let major = components.next()?.parse::<u32>().ok()?;
    let minor = components.next()?.parse::<u32>().ok()?;
    Some((major, minor))
}

/// TypeScript >= 7 is the native (tsgo) engine family. A "tsserver" launcher
/// belonging to a 7+ install must classify as the tsgo family for
/// recommendation and serving-order purposes — it is never served over the
/// Node tsserver protocol.
pub fn ts_major_is_native_family(major: u32) -> bool {
    major >= 7
}

// ── The tsserver serving tiers (the ONE place the version floors live) ──────
//
// Verter SERVES every tsserver-family install it can spawn; the tiers below
// differ only in the user-facing advisory, so the behaviour and the message
// cannot drift apart:
//
// - tsgo >=7.0.2 <7.1.0  → the native engine (best path; owned by
//   `verter_tsgo_api::toolchain::policy`, never routed here).
// - TypeScript >= 6.x    → [`TsserverServingTier::Current`]: served, no
//   upgrade advisory.
// - TypeScript >= 5.8 <6 → [`TsserverServingTier::Legacy`]: served, WITH an
//   advisory recommending TypeScript 6 or 7 (7 enables the native tsgo
//   engine).
// - below 5.8            → [`TsserverServingTier::BelowFloor`]: nothing in the
//   tsserver stack hard-requires 5.8 (the plugin speaks the stable
//   language-service plugin API), so Verter still SERVES — but below the
//   supported floor the serving is best-effort and the advisory says so
//   plainly with the upgrade path, rather than silently claiming support.

/// The lowest TypeScript version Verter SUPPORTS for tsserver serving:
/// `>=5.8, <6` is the legacy-supported tier; below it serving is best-effort.
pub const TSSERVER_SUPPORTED_FLOOR: (u32, u32) = (5, 8);

/// The first current-generation tsserver major: TypeScript `>=6` (and not the
/// TS7+ native family) is served with NO upgrade advisory.
pub const TSSERVER_CURRENT_MAJOR: u32 = 6;

/// How a tsserver-family install is served, by its `(major, minor)` version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsserverServingTier {
    /// TypeScript `>=6` (below the TS7 native family): served, no advisory.
    Current,
    /// TypeScript `>=5.8, <6`: served, with an upgrade advisory.
    Legacy,
    /// TypeScript `<5.8`: served best-effort, with a plain this-is-unsupported
    /// advisory and the upgrade path.
    BelowFloor,
}

/// Classify a tsserver-family install into its serving tier.
///
/// `version` is `None` when the install's version is unreadable: fail open to
/// [`TsserverServingTier::Current`] — exactly like
/// [`tsserver_native_family_major`], classification requires positive
/// evidence, and an unreadable version must never fabricate a warning.
pub fn tsserver_serving_tier(version: Option<(u32, u32)>) -> TsserverServingTier {
    let Some((major, minor)) = version else {
        return TsserverServingTier::Current;
    };
    if major >= TSSERVER_CURRENT_MAJOR {
        return TsserverServingTier::Current;
    }
    if (major, minor) >= TSSERVER_SUPPORTED_FLOOR {
        return TsserverServingTier::Legacy;
    }
    TsserverServingTier::BelowFloor
}

/// The user-facing upgrade advisory for a serving tier, or `None` when the
/// tier is served silently ([`TsserverServingTier::Current`]). The text names
/// the tiers and the upgrade path; it is built HERE, next to the floors, so
/// the behaviour and the message cannot drift apart.
pub fn tsserver_serving_advisory(version: (u32, u32), tier: TsserverServingTier) -> Option<String> {
    let (major, minor) = version;
    match tier {
        TsserverServingTier::Current => None,
        TsserverServingTier::Legacy => Some(format!(
            "Verter: this workspace is served by tsserver from TypeScript {major}.{minor}. \
             TypeScript 5.x is supported, but upgrading to TypeScript 6 or 7 is recommended \
             (7 enables Verter's native tsgo engine). Hover, completions, and \
             go-to-definition are fully served in the meantime."
        )),
        TsserverServingTier::BelowFloor => Some(format!(
            "Verter: this workspace's TypeScript {major}.{minor} is below the supported \
             tsserver floor ({}.{}) — serving is best-effort. Upgrade to TypeScript 6 or 7 \
             (7 enables Verter's native tsgo engine).",
            TSSERVER_SUPPORTED_FLOOR.0, TSSERVER_SUPPORTED_FLOOR.1
        )),
    }
}

/// Classify a resolved tsserver candidate: `Some(major)` when the install it
/// belongs to is the TS7+ native (tsgo) family, `None` when it is a servable
/// 5.x/6.x tsserver or its version is unreadable (fail-open: classification
/// requires positive evidence of the native family).
pub fn tsserver_native_family_major(tsserver_path: &Path) -> Option<u32> {
    detect_ts_major_version(tsserver_path).filter(|major| ts_major_is_native_family(*major))
}

/// The tier that supplied a usable tsserver install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsserverSource {
    /// The owning configured project's nearest ancestor `node_modules/typescript`.
    ProjectLocal,
    /// The user-configured `typescript.tsdk` directory.
    ConfiguredTsdk,
    /// The TypeScript package below `npm root -g`.
    Global,
}

/// A validated TypeScript install suitable for tsserver serving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTsserver {
    /// Canonical `tsserver.js` path. Canonicalization is load-bearing for pnpm:
    /// tsserver's script-relative default-lib lookup must run beside the real
    /// `.pnpm/typescript@...` package, not through a package-level symlink.
    pub path: PathBuf,
    /// The tier that supplied the install.
    pub source: TsserverSource,
    /// Number of sibling `lib.*.d.ts` default-library files observed.
    pub default_lib_count: usize,
    /// Installs a NEARER tier offered and this resolution refused, in search
    /// order — empty on the common path where the first candidate served.
    ///
    /// Retained because a successful fall-through SWAPS THE TOOLCHAIN: a
    /// refused project-local TypeScript followed by a servable global one
    /// silently replaces the project-pinned engine, and the user then type-checks
    /// against a version their project never selected. That is a
    /// reproducibility defect, so the skipped tier and the reason travel WITH the
    /// result and are surfaced by [`Self::skipped_tier_advisory`] rather than
    /// being dropped on success.
    pub skipped: Vec<TsserverCandidateRejection>,
}

impl ResolvedTsserver {
    /// A user-facing advisory naming the nearer install(s) this resolution
    /// skipped and why, or `None` when nothing was skipped.
    ///
    /// Bounded like [`TsserverDiscoveryError`]'s report: an ancestor walk in a
    /// deep monorepo can refuse several tiers, and the first few are what the
    /// user acts on.
    #[must_use]
    pub fn skipped_tier_advisory(&self) -> Option<String> {
        const SHOWN: usize = 3;
        if self.skipped.is_empty() {
            return None;
        }
        let shown = self
            .skipped
            .iter()
            .take(SHOWN)
            .map(|rejection| format!("{}: {}", rejection.path.display(), rejection.reason))
            .collect::<Vec<_>>()
            .join("; ");
        let rest = match self.skipped.len().checked_sub(SHOWN) {
            Some(rest) if rest > 0 => format!(" (and {rest} more)"),
            _ => String::new(),
        };
        Some(format!(
            "Verter: TypeScript is being served from {} instead of a nearer install that was \
             refused \u{2014} {shown}{rest}. Type-checking therefore uses a TypeScript this project did \
             not pin; fix the refused install or set the VS Code `typescript.tsdk` setting to the \
             one you want.",
            self.path.display()
        ))
    }
}

/// One rejected tsserver candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsserverCandidateRejection {
    /// Candidate path before canonicalization.
    pub path: PathBuf,
    /// Clear refusal reason.
    pub reason: String,
}

/// No searched TypeScript install was safe to serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsserverDiscoveryError {
    /// Candidate-specific refusals, including library-less installs.
    pub rejections: Vec<TsserverCandidateRejection>,
}

impl fmt::Display for TsserverDiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "no usable TypeScript installation was found for this configured project"
        )?;
        for rejection in &self.rejections {
            writeln!(f, "  - {}: {}", rejection.path.display(), rejection.reason)?;
        }
        write!(
            f,
            "install it in the owning package: npm install -D typescript; \
             for a non-standard location, set the VS Code `typescript.tsdk` setting. \
             Verter's native analysis remains available without TypeScript."
        )
    }
}

impl std::error::Error for TsserverDiscoveryError {}

/// Resolve and validate the tsserver for one configured project.
///
/// Search order:
/// 1. `<owning-project-dir>/node_modules/typescript/lib/tsserver.js`, walking
///    through every ancestor.
/// 2. `<tsdk>/tsserver.js` from the user-configured `typescript.tsdk` setting.
/// 3. Global TypeScript via `npm root -g`.
///
/// Every candidate is canonicalized through package-manager symlinks and must
/// carry at least one sibling `lib.*.d.ts`. A library-less candidate is refused
/// and the next explicit tier is considered; if none is usable the returned
/// error preserves every refusal and gives the install/configuration action.
pub fn resolve_tsserver(
    tsdk: Option<&str>,
    owning_project_dir: Option<&str>,
) -> Result<ResolvedTsserver, TsserverDiscoveryError> {
    resolve_tsserver_with_global(
        tsdk,
        owning_project_dir,
        global_tsserver_candidate,
        |path| path.canonicalize(),
    )
}

/// The injectable core of [`resolve_tsserver`].
///
/// `canonicalize` is a parameter for the same reason `global_candidate` is: the
/// exec-representability refusal below triggers only on a Windows
/// extended-length path, which `Path::canonicalize()` produces on Windows ONLY.
/// Injecting it lets the whole discovery flow — candidate, validation, refusal,
/// next tier, rendered error — run and DISCRIMINATE on macOS and Linux, instead
/// of being a branch no gate can reach.
fn resolve_tsserver_with_global(
    tsdk: Option<&str>,
    owning_project_dir: Option<&str>,
    global_candidate: impl FnOnce() -> Option<PathBuf>,
    canonicalize: impl Fn(&Path) -> std::io::Result<PathBuf> + Copy,
) -> Result<ResolvedTsserver, TsserverDiscoveryError> {
    let mut candidates = Vec::new();

    if let Some(root) = owning_project_dir.filter(|root| !root.is_empty()) {
        let mut dir = Path::new(root);
        loop {
            candidates.push((
                dir.join("node_modules/typescript/lib/tsserver.js"),
                TsserverSource::ProjectLocal,
            ));
            match dir.parent() {
                Some(parent) if parent != dir => dir = parent,
                _ => break,
            }
        }
    }

    if let Some(tsdk) = tsdk.filter(|tsdk| !tsdk.is_empty()) {
        candidates.push((
            Path::new(tsdk).join("tsserver.js"),
            TsserverSource::ConfiguredTsdk,
        ));
    }

    // Rejections accumulate ACROSS tiers and travel with whichever tier finally
    // serves: a later tier winning does not make an earlier refusal irrelevant,
    // it makes it the reason the engine is not the one the project pinned.
    let mut rejections = Vec::new();
    for (candidate, source) in candidates {
        if !candidate.exists() {
            continue;
        }
        match validate_tsserver_candidate(&candidate, source, canonicalize) {
            Ok(resolved) => return Ok(resolved.with_skipped(rejections)),
            Err(rejection) => rejections.push(rejection),
        }
    }
    if let Some(candidate) = global_candidate() {
        if candidate.exists() {
            match validate_tsserver_candidate(&candidate, TsserverSource::Global, canonicalize) {
                Ok(resolved) => return Ok(resolved.with_skipped(rejections)),
                Err(rejection) => rejections.push(rejection),
            }
        }
    }

    Err(TsserverDiscoveryError { rejections })
}

fn validate_tsserver_candidate(
    candidate: &Path,
    source: TsserverSource,
    canonicalize: impl Fn(&Path) -> std::io::Result<PathBuf>,
) -> Result<ResolvedTsserver, TsserverCandidateRejection> {
    let path = canonicalize(candidate).map_err(|error| TsserverCandidateRejection {
        path: candidate.to_path_buf(),
        reason: format!("could not resolve the real install path: {error}"),
    })?;
    // An install whose canonical path has no normal Win32 spelling cannot be
    // EXECUTED even though it exists: node parses argv[1] with its own
    // `\\?\`-unaware path handling and dies before tsserver initialises. Refusing
    // it HERE, as a candidate rejection, is what makes the failure visible and
    // recoverable — discovery moves on to the `typescript.tsdk` and global tiers
    // (either may be nameable), and if nothing is usable the accumulated
    // rejections carry the path, the reason, and the action all the way to the
    // startup warning through `TsserverDiscoveryError`. A refusal discovered
    // later, at spawn time, is swallowed by the per-feature error paths and the
    // user just sees TypeScript go dark.
    if let Some(refusal) = path.to_str().and_then(verter_span::path::verbatim_refusal) {
        return Err(TsserverCandidateRejection {
            path,
            reason: format!(
                "this install resolves to an extended-length Windows path that node cannot \
                 execute: {refusal}"
            ),
        });
    }
    let Some(lib_dir) = path.parent() else {
        return Err(TsserverCandidateRejection {
            path: candidate.to_path_buf(),
            reason: "tsserver.js has no parent library directory".to_string(),
        });
    };
    let default_lib_count = std::fs::read_dir(lib_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("lib.")
                && name.ends_with(".d.ts")
                && name.len() > "lib..d.ts".len()
                && entry.file_type().is_ok_and(|kind| kind.is_file())
        })
        .count();
    if default_lib_count == 0 {
        return Err(TsserverCandidateRejection {
            path: path.clone(),
            reason: format!(
                "refusing library-less TypeScript install: {} contains no sibling lib.*.d.ts \
                 default libraries",
                lib_dir.display()
            ),
        });
    }
    Ok(ResolvedTsserver {
        path,
        source,
        default_lib_count,
        skipped: Vec::new(),
    })
}

impl ResolvedTsserver {
    /// Attach the tiers this resolution passed over on its way to serving.
    fn with_skipped(mut self, skipped: Vec<TsserverCandidateRejection>) -> Self {
        self.skipped = skipped;
        self
    }
}

/// The global tier's candidate, resolved AT MOST ONCE per process.
///
/// The last tier shells out to `npm root -g`, and it is reached by every
/// resolution that misses every explicit tier. Per-project resolution asks that
/// question once per configured project, so a monorepo where many packages have
/// no TypeScript would otherwise spawn npm once per package — tens of process
/// spawns on the startup path. `npm root -g` is a stable per-process fact, so it
/// is memoized (including the "npm is unavailable" answer).
fn global_tsserver_candidate() -> Option<PathBuf> {
    static GLOBAL_CANDIDATE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    GLOBAL_CANDIDATE
        .get_or_init(|| {
            let output = std::process::Command::new("npm")
                .args(["root", "-g"])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let global_root = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Some(Path::new(&global_root).join("typescript/lib/tsserver.js"))
        })
        .clone()
}

/// Compatibility helper returning only the validated canonical path.
///
/// New callers that need a user-facing refusal reason should use
/// [`resolve_tsserver`].
pub fn find_tsserver(tsdk: Option<&str>, owning_project_dir: Option<&str>) -> Option<PathBuf> {
    resolve_tsserver(tsdk, owning_project_dir)
        .ok()
        .map(|resolved| resolved.path)
}

/// Find the `node` executable on PATH, with platform-specific fallbacks.
///
/// Search order:
/// 1. `PATH` environment variable
/// 2. Platform-specific well-known locations (macOS: Homebrew; macOS+Linux: Volta, nvm, fnm)
/// 3. (macOS/Linux only) Login shell PATH detection as last resort
pub fn find_node() -> Option<String> {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let name = format!("node{ext}");

    // 1. Check PATH
    if let Some(result) = find_node_in_path(&name) {
        return Some(result);
    }

    // 2. Platform-specific well-known locations
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if let Some(result) = find_node_platform_fallbacks(&name) {
        return Some(result);
    }

    // 3. Last resort: detect full PATH from login shell (macOS/Linux only)
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if let Some(shell_path) = detect_shell_path() {
        let separator = ':';
        for dir in shell_path.split(separator) {
            let full = Path::new(dir).join(&name);
            if full.exists() {
                return Some(full.to_string_lossy().to_string());
            }
        }
    }

    None
}

/// Search for `node` in the PATH environment variable.
fn find_node_in_path(name: &str) -> Option<String> {
    let path_var = std::env::var("PATH").ok()?;
    let separator = if cfg!(windows) { ';' } else { ':' };
    for dir in path_var.split(separator) {
        let full = Path::new(dir).join(name);
        if full.exists() {
            return Some(full.to_string_lossy().to_string());
        }
    }
    None
}

/// Platform-specific well-known Node.js locations.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn find_node_platform_fallbacks(name: &str) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let static_paths = [
            "/opt/homebrew/bin", // Apple Silicon Homebrew
            "/usr/local/bin",    // Intel Homebrew / official installer
        ];
        for dir in &static_paths {
            let full = Path::new(dir).join(name);
            if full.exists() {
                return Some(full.to_string_lossy().to_string());
            }
        }
    }

    let home = std::env::var("HOME").ok()?;
    let home = Path::new(&home);

    // Volta
    let volta_path = home.join(".volta/bin").join(name);
    if volta_path.exists() {
        return Some(volta_path.to_string_lossy().to_string());
    }

    // nvm — pick highest installed version
    if let Some(result) = find_highest_version_node(&home.join(".nvm/versions/node"), "bin", name) {
        return Some(result);
    }

    // fnm
    if let Some(result) = find_highest_version_node(
        &home.join(".local/share/fnm/node-versions"),
        "installation/bin",
        name,
    ) {
        return Some(result);
    }

    None
}

/// Find the highest-versioned Node.js binary in a version-manager directory.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn find_highest_version_node(base_dir: &Path, bin_subpath: &str, name: &str) -> Option<String> {
    let entries = std::fs::read_dir(base_dir).ok()?;
    let mut versions: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    versions.sort_unstable_by(|a, b| b.cmp(a));

    for version in versions {
        let full = base_dir.join(&version).join(bin_subpath).join(name);
        if full.exists() {
            return Some(full.to_string_lossy().to_string());
        }
    }
    None
}

/// Detect the user's full PATH by spawning their login shell.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn detect_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let output = std::process::Command::new(&shell)
        .args(["-l", "-c", "echo $PATH"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(path);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_ts_major_version_parses_5() {
        let tmp = std::env::temp_dir().join("verter_runtime_test_ts_version");
        let lib_dir = tmp.join("lib");
        std::fs::create_dir_all(&lib_dir).unwrap();

        let tsserver_path = lib_dir.join("tsserver.js");
        std::fs::write(&tsserver_path, "// tsserver").unwrap();
        std::fs::write(
            tmp.join("package.json"),
            r#"{ "name": "typescript", "version": "5.7.2" }"#,
        )
        .unwrap();

        let result = detect_ts_major_version(&tsserver_path);
        assert_eq!(result, Some(5));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_detect_ts_major_version_parses_6() {
        let tmp = std::env::temp_dir().join("verter_runtime_test_ts_version_6");
        let lib_dir = tmp.join("lib");
        std::fs::create_dir_all(&lib_dir).unwrap();

        let tsserver_path = lib_dir.join("tsserver.js");
        std::fs::write(&tsserver_path, "// tsserver").unwrap();
        std::fs::write(
            tmp.join("package.json"),
            r#"{ "name": "typescript", "version": "6.0.0-beta.1" }"#,
        )
        .unwrap();

        let result = detect_ts_major_version(&tsserver_path);
        assert_eq!(result, Some(6));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_detect_ts_major_version_parses_7_rc() {
        // TypeScript 7 native-preview / release-candidate installs report a
        // `7.0.1-rc`-style version; the major must parse to Some(7) so auto-mode
        // provider selection routes them to the tsgo external engine. Uses an
        // isolated temp dir so parallel test runs never collide on a fixed path.
        let tmp = tempfile::tempdir().unwrap();
        let lib_dir = tmp.path().join("lib");
        std::fs::create_dir_all(&lib_dir).unwrap();

        let tsserver_path = lib_dir.join("tsserver.js");
        std::fs::write(&tsserver_path, "// tsserver").unwrap();
        std::fs::write(
            tmp.path().join("package.json"),
            r#"{ "name": "typescript", "version": "7.0.1-rc" }"#,
        )
        .unwrap();

        let result = detect_ts_major_version(&tsserver_path);
        assert_eq!(result, Some(7));
    }

    #[test]
    fn test_detect_ts_major_version_returns_none_for_missing() {
        let result = detect_ts_major_version(Path::new("/nonexistent/lib/tsserver.js"));
        assert_eq!(result, None);
    }

    /// Write a `typescript/` package layout with the given version and return
    /// the tsserver.js path inside it (kept alive by returning the tempdir).
    fn fake_typescript_install(version: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let lib_dir = tmp.path().join("lib");
        std::fs::create_dir_all(&lib_dir).unwrap();
        let tsserver_path = lib_dir.join("tsserver.js");
        std::fs::write(&tsserver_path, "// tsserver").unwrap();
        std::fs::write(
            tmp.path().join("package.json"),
            format!(r#"{{ "name": "typescript", "version": "{version}" }}"#),
        )
        .unwrap();
        (tmp, tsserver_path)
    }

    /// TS 7.x-family version-string matrix: every 7+ install (stable, rc,
    /// beta, and beyond) classifies as the native (tsgo) engine family for
    /// serving-order purposes; 5.x/6.x installs remain servable as tsserver.
    #[test]
    fn ts_version_matrix_classifies_native_family() {
        let matrix: &[(&str, Option<u32>)] = &[
            ("5.9.2", None),
            ("6.0.0-beta.1", None),
            ("7.0.0", Some(7)),
            ("7.0.1-rc", Some(7)),
            ("7.1.0-beta", Some(7)),
            ("8.0.0", Some(8)),
        ];
        for (version, expected) in matrix {
            let (_tmp, tsserver_path) = fake_typescript_install(version);
            assert_eq!(
                tsserver_native_family_major(&tsserver_path),
                *expected,
                "version {version} misclassified"
            );
        }
    }

    /// Fail-open: an unreadable/absent version never blocks the tsserver
    /// route (classification requires positive evidence of the native family).
    #[test]
    fn unreadable_version_is_not_native_family() {
        assert_eq!(
            tsserver_native_family_major(Path::new("/nonexistent/lib/tsserver.js")),
            None
        );
    }

    // ── DISCRIMINATING (the owner serving-tier policy): every tsserver-family
    //    install is SERVED; only the advisory differs. The tiers and their
    //    floors are pinned here so the behaviour and the message cannot drift
    //    apart: >=6 silent, >=5.8 <6 served WITH an upgrade advisory, <5.8
    //    served best-effort with a plain below-floor advisory. ───────────────
    #[test]
    fn tsserver_serving_tiers_follow_the_owner_floors() {
        let cases: &[(Option<(u32, u32)>, TsserverServingTier)] = &[
            (Some((6, 0)), TsserverServingTier::Current),
            (Some((6, 9)), TsserverServingTier::Current),
            // A TS7+ install never reaches this classifier (the native-family
            // gate routes it to tsgo), but if it did it is not a legacy tier.
            (Some((7, 0)), TsserverServingTier::Current),
            // An unreadable version fails open — never a fabricated warning.
            (None, TsserverServingTier::Current),
            (Some((5, 8)), TsserverServingTier::Legacy),
            (Some((5, 9)), TsserverServingTier::Legacy),
            (Some((5, 7)), TsserverServingTier::BelowFloor),
            (Some((5, 0)), TsserverServingTier::BelowFloor),
            (Some((4, 9)), TsserverServingTier::BelowFloor),
        ];
        for (version, expected) in cases {
            assert_eq!(
                tsserver_serving_tier(*version),
                *expected,
                "version {version:?} mis-tiered"
            );
        }
    }

    // ── DISCRIMINATING: the advisory exists exactly for the advisory tiers,
    //    names the version, and carries the 6-or-7 upgrade path. ─────────────
    #[test]
    fn tsserver_serving_advisory_matches_its_tier() {
        assert!(tsserver_serving_advisory((6, 0), TsserverServingTier::Current).is_none());

        let legacy = tsserver_serving_advisory((5, 9), TsserverServingTier::Legacy)
            .expect("the legacy tier advises");
        assert!(
            legacy.contains("5.9"),
            "names the serving version: {legacy}"
        );
        assert!(
            legacy.contains("TypeScript 6 or 7"),
            "carries the upgrade path: {legacy}"
        );
        assert!(legacy.contains("tsgo"), "names the native engine: {legacy}");

        let below = tsserver_serving_advisory((5, 4), TsserverServingTier::BelowFloor)
            .expect("the below-floor tier advises");
        assert!(below.contains("5.4"), "names the serving version: {below}");
        assert!(below.contains("5.8"), "names the supported floor: {below}");
        assert!(
            below.contains("best-effort"),
            "says plainly the serving is best-effort: {below}"
        );
    }

    #[test]
    fn detect_ts_version_reads_major_and_minor() {
        let (_tmp, tsserver_path) = fake_typescript_install("5.9.3");
        assert_eq!(detect_ts_version(&tsserver_path), Some((5, 9)));
        let (_tmp, tsserver_path) = fake_typescript_install("6.0.0-beta.1");
        assert_eq!(detect_ts_version(&tsserver_path), Some((6, 0)));
        assert_eq!(
            detect_ts_version(Path::new("/nonexistent/lib/tsserver.js")),
            None
        );
    }

    #[test]
    fn test_find_node_returns_some_on_this_machine() {
        let result = find_node();
        assert!(
            result.is_some(),
            "find_node() should find node on this machine"
        );
    }

    #[test]
    fn test_find_node_in_path_finds_existing() {
        let name = if cfg!(windows) { "node.exe" } else { "node" };
        let result = find_node_in_path(name);
        assert!(
            result.is_some(),
            "find_node_in_path should find node via PATH"
        );
    }

    fn write_complete_typescript_install(root: &Path, version: &str) -> PathBuf {
        let lib = root.join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("tsserver.js"), "// tsserver").unwrap();
        std::fs::write(lib.join("lib.es5.d.ts"), "interface Array<T> {}").unwrap();
        std::fs::write(
            root.join("package.json"),
            format!(r#"{{ "name": "typescript", "version": "{version}" }}"#),
        )
        .unwrap();
        lib.join("tsserver.js")
    }

    /// @ai-generated - Pins project-local pnpm resolution and realpath identity.
    #[cfg(unix)]
    #[test]
    fn project_local_pnpm_typescript_resolves_to_the_real_install() {
        use std::os::unix::fs::symlink;

        let workspace = tempfile::tempdir().unwrap();
        let project = workspace.path().join("packages/ui");
        let store_install = workspace
            .path()
            .join("node_modules/.pnpm/typescript@6.0.2/node_modules/typescript");
        let expected = write_complete_typescript_install(&store_install, "6.0.2");
        std::fs::create_dir_all(project.join("node_modules")).unwrap();
        symlink(
            Path::new("../../../node_modules/.pnpm/typescript@6.0.2/node_modules/typescript"),
            project.join("node_modules/typescript"),
        )
        .unwrap();

        let resolved = find_tsserver(None, project.to_str());

        assert_eq!(resolved, Some(expected.canonicalize().unwrap()));
    }

    /// @ai-generated - Pins fail-closed rejection of a library-less local install.
    #[test]
    fn library_less_project_install_falls_through_to_configured_tsdk() {
        let workspace = tempfile::tempdir().unwrap();
        let project = workspace.path().join("packages/ui");
        let broken_lib = project.join("node_modules/typescript/lib");
        std::fs::create_dir_all(&broken_lib).unwrap();
        std::fs::write(broken_lib.join("tsserver.js"), "// no default libraries").unwrap();

        let tsdk_install = workspace.path().join("configured-typescript");
        let expected = write_complete_typescript_install(&tsdk_install, "6.0.2");

        let resolved = find_tsserver(tsdk_install.join("lib").to_str(), project.to_str());

        assert_eq!(resolved, Some(expected.canonicalize().unwrap()));
    }

    // ── An install node cannot EXECUTE is refused as a candidate, not at spawn ──
    //
    // `Path::canonicalize()` only returns the Windows extended-length form on
    // Windows, so these inject the canonicalizer (the same shape
    // `resolve_tsserver_with_global` already uses for the global tier) and drive
    // the REAL discovery flow with a verbatim result. What is covered on every
    // host: the refusal, the tier fallthrough, and the rendered user-facing
    // string. What only Windows can show: that canonicalize emits this form.

    /// A canonicalizer that answers with an extended-length path Win32 cannot
    /// name — the `\\?\` shape a Windows `canonicalize()` would return for an
    /// install under a device-named directory.
    fn canonicalize_to_unnameable(path: &Path) -> std::io::Result<PathBuf> {
        let real = path.canonicalize()?;
        let _ = real;
        Ok(PathBuf::from(
            r"\\?\D:\ws\NUL\node_modules\typescript\lib\tsserver.js",
        ))
    }

    #[test]
    fn an_install_node_cannot_execute_is_refused_with_an_actionable_user_message() {
        let workspace = tempfile::tempdir().unwrap();
        let project = workspace.path().join("packages/ui");
        write_complete_typescript_install(&project.join("node_modules/typescript"), "6.0.2");

        let error = resolve_tsserver_with_global(
            None,
            project.to_str(),
            || None,
            canonicalize_to_unnameable,
        )
        .expect_err("an install node cannot execute must not be served");

        // This rendered string is what reaches the user: discovery error ->
        // `probe.refusals` -> `WorkspaceTsserverProbe::refusal_summary()` ->
        // `TsserverSpawnError::Unavailable` -> `tsserver_error_message` ->
        // `client.show_message(WARNING, ...)`.
        let message = error.to_string();
        assert!(
            message.contains(r"\\?\D:\ws\NUL\node_modules\typescript\lib\tsserver.js"),
            "the user message names the offending path: {message}"
        );
        assert!(
            message.contains("reserved Windows device name"),
            "the user message names WHY it was refused: {message}"
        );
        assert!(
            message.contains("typescript.tsdk"),
            "the user message names the action: {message}"
        );
        // NEGATIVE: this is a refusal with a reason, not the generic
        // library-less rejection.
        assert!(
            !message.contains("contains no sibling lib.*.d.ts"),
            "an unnameable install is not a library-less one: {message}"
        );
    }

    #[test]
    fn a_skipped_nearer_install_is_retained_and_surfaced_on_the_successful_result() {
        // A successful fall-through SWAPS THE TOOLCHAIN: the project-pinned
        // install was refused and a different one now type-checks the code. That
        // must never be silent, so the refusal travels WITH the result.
        let workspace = tempfile::tempdir().unwrap();
        let project = workspace.path().join("packages/ui");
        write_complete_typescript_install(&project.join("node_modules/typescript"), "6.0.2");
        let tsdk_install = workspace.path().join("configured-typescript");
        write_complete_typescript_install(&tsdk_install, "6.0.2");

        let seen = std::cell::Cell::new(0usize);
        let canonicalize = |path: &Path| -> std::io::Result<PathBuf> {
            let real = path.canonicalize()?;
            if seen.get() == 0 {
                seen.set(1);
                return Ok(PathBuf::from(
                    r"\\?\D:\ws\NUL\node_modules\typescript\lib\tsserver.js",
                ));
            }
            Ok(real)
        };

        let resolved = resolve_tsserver_with_global(
            tsdk_install.join("lib").to_str(),
            project.to_str(),
            || None,
            canonicalize,
        )
        .expect("a nameable tsdk install still serves");

        assert_eq!(
            resolved.skipped.len(),
            1,
            "the refused nearer tier is retained"
        );
        let advisory = resolved
            .skipped_tier_advisory()
            .expect("a skipped tier produces a user-facing advisory");
        assert!(
            advisory.contains(r"\\?\D:\ws\NUL\node_modules\typescript\lib\tsserver.js"),
            "the advisory names the install that was skipped: {advisory}"
        );
        assert!(
            advisory.contains("reserved Windows device name"),
            "the advisory names WHY it was skipped: {advisory}"
        );
        assert!(
            advisory.contains("did not pin"),
            "the advisory names the CONSEQUENCE \u{2014} a different TypeScript: {advisory}"
        );
        assert!(
            advisory.contains("typescript.tsdk"),
            "the advisory names the action: {advisory}"
        );
    }

    #[test]
    fn a_first_tier_hit_carries_no_skipped_advisory() {
        // NEGATIVE CONTROL: the common path must stay silent, or every workspace
        // gets a spurious toolchain-swap warning.
        let workspace = tempfile::tempdir().unwrap();
        let project = workspace.path().join("packages/ui");
        write_complete_typescript_install(&project.join("node_modules/typescript"), "6.0.2");

        let resolved =
            resolve_tsserver_with_global(None, project.to_str(), || None, |p| p.canonicalize())
                .expect("an ordinary install must serve");
        assert!(resolved.skipped.is_empty());
        assert_eq!(resolved.skipped_tier_advisory(), None);
    }

    #[test]
    fn an_unnameable_project_install_falls_through_to_a_nameable_tsdk() {
        // The refusal is a candidate rejection, so the NEXT tier still gets a
        // chance — a `typescript.tsdk` install at a nameable path serves.
        let workspace = tempfile::tempdir().unwrap();
        let project = workspace.path().join("packages/ui");
        write_complete_typescript_install(&project.join("node_modules/typescript"), "6.0.2");
        let tsdk_install = workspace.path().join("configured-typescript");
        let tsdk_entry = write_complete_typescript_install(&tsdk_install, "6.0.2");

        let seen = std::cell::Cell::new(0usize);
        let canonicalize = |path: &Path| -> std::io::Result<PathBuf> {
            let real = path.canonicalize()?;
            // Only the FIRST (project-local) candidate is unnameable.
            if seen.get() == 0 {
                seen.set(1);
                return Ok(PathBuf::from(
                    r"\\?\D:\ws\NUL\node_modules\typescript\lib\tsserver.js",
                ));
            }
            Ok(real)
        };

        let resolved = resolve_tsserver_with_global(
            tsdk_install.join("lib").to_str(),
            project.to_str(),
            || None,
            canonicalize,
        )
        .expect("a nameable tsdk install still serves");
        assert_eq!(resolved.path, tsdk_entry.canonicalize().unwrap());
        assert_eq!(resolved.source, TsserverSource::ConfiguredTsdk);
    }

    #[test]
    fn a_nameable_install_is_never_refused_by_the_exec_representability_check() {
        // NEGATIVE CONTROL: the check must reject ONLY what node cannot execute.
        let workspace = tempfile::tempdir().unwrap();
        let project = workspace.path().join("packages/ui");
        let expected =
            write_complete_typescript_install(&project.join("node_modules/typescript"), "6.0.2");

        let resolved =
            resolve_tsserver_with_global(None, project.to_str(), || None, |p| p.canonicalize())
                .expect("an ordinary install must serve");
        assert_eq!(resolved.path, expected.canonicalize().unwrap());
    }
}

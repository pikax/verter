//! Node.js and TypeScript binary discovery helpers.
//!
//! Moved from `verter_lsp::tsserver::mod` to be shared between LSP and
//! component-meta consumers.

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

/// Find the tsserver.js binary path.
///
/// Search order (project TypeScript preferred over bundled/global):
/// 1. `<workspace>/node_modules/typescript/lib/tsserver.js` (+ parent directories)
/// 2. `<tsdk>/tsserver.js` (from VS Code setting or extension's bundled TypeScript)
/// 3. Global TypeScript via `npm root -g`
pub fn find_tsserver(tsdk: Option<&str>, workspace_root: Option<&str>) -> Option<PathBuf> {
    // 1. Workspace node_modules — walk up parent directories
    if let Some(root) = workspace_root {
        let mut dir = Path::new(root);
        for _ in 0..10 {
            let path = dir.join("node_modules/typescript/lib/tsserver.js");
            if path.exists() {
                return Some(path);
            }
            match dir.parent() {
                Some(parent) if parent != dir => dir = parent,
                _ => break,
            }
        }
    }

    // 2. From tsdk setting
    if let Some(tsdk) = tsdk {
        if !tsdk.is_empty() {
            let path = Path::new(tsdk).join("tsserver.js");
            if path.exists() {
                return Some(path);
            }
        }
    }

    // 3. Global TypeScript
    if let Ok(output) = std::process::Command::new("npm")
        .args(["root", "-g"])
        .output()
    {
        if output.status.success() {
            let global_root = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let path = Path::new(&global_root).join("typescript/lib/tsserver.js");
            if path.exists() {
                return Some(path);
            }
        }
    }

    None
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
}

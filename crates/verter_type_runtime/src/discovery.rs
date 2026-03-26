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
    let major = version.split('.').next()?;
    major.parse::<u32>().ok()
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
    fn test_detect_ts_major_version_returns_none_for_missing() {
        let result = detect_ts_major_version(Path::new("/nonexistent/lib/tsserver.js"));
        assert_eq!(result, None);
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

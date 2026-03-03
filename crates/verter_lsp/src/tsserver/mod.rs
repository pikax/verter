//! TypeScript language service provider via tsserver.
//!
//! Uses the standard `tsserver.js` protocol (newline-delimited JSON over stdio)
//! with the `@verter/typescript-plugin` for `.vue` file resolution.
//!
//! This is an alternative to TSGO for users who don't have the Go-based
//! TypeScript server available. It uses the workspace TypeScript version.

pub mod ipc;
pub mod resilient;

use std::path::{Path, PathBuf};

/// Find the tsserver.js binary path.
///
/// Search order:
/// 1. `<tsdk>/tsserver.js` (from VS Code setting)
/// 2. `<workspace>/node_modules/typescript/lib/tsserver.js`
/// 3. Global TypeScript via `npm root -g`
pub fn find_tsserver(tsdk: Option<&str>, workspace_root: Option<&str>) -> Option<PathBuf> {
    // 1. From tsdk setting
    if let Some(tsdk) = tsdk {
        if !tsdk.is_empty() {
            let path = Path::new(tsdk).join("tsserver.js");
            if path.exists() {
                return Some(path);
            }
        }
    }

    // 2. Workspace node_modules
    if let Some(root) = workspace_root {
        let path = Path::new(root).join("node_modules/typescript/lib/tsserver.js");
        if path.exists() {
            return Some(path);
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

/// Find the `node` executable on PATH.
pub fn find_node() -> Option<String> {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let name = format!("node{ext}");

    // Check PATH
    if let Ok(path_var) = std::env::var("PATH") {
        let separator = if cfg!(windows) { ';' } else { ':' };
        for dir in path_var.split(separator) {
            let full = Path::new(dir).join(&name);
            if full.exists() {
                return Some(full.to_string_lossy().to_string());
            }
        }
    }

    None
}

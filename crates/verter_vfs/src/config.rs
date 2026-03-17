//! Tsconfig discovery and parsing for project configuration.
//!
//! Discovers `tsconfig.json` files under workspace roots, parses them with
//! `extends` resolution, and extracts membership filters, compiler options,
//! and project references. Only the DISCOVERY and PARSING parts are ported
//! here — lint config stays in `verter_diagnostics`.

use std::path::{Path, PathBuf};

use crate::resolver::{IdeProjectCompilerOptions, ProjectMembership};

/// Maximum depth for tsconfig `extends` chain resolution.
/// Prevents infinite loops from circular extends.
const MAX_TSCONFIG_EXTENDS_DEPTH: u8 = 8;

// ═══════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════

/// A discovered tsconfig.json and its containing directory.
#[derive(Debug, Clone)]
pub struct TsConfigEntry {
    /// Canonical path to the tsconfig.json file (forward slashes).
    pub path: String,
    /// Directory containing the tsconfig (forward slashes).
    pub root: String,
}

/// Parsed contents of a single tsconfig.json.
#[derive(Debug, Clone)]
pub struct ParsedTsConfig {
    pub compiler_options: IdeProjectCompilerOptions,
    pub membership: ProjectMembership,
    pub references: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════════════════
// JSON Comment Stripping
// ═══════════════════════════════════════════════════════════════════════════

/// Strip `//` and `/* */` comments from JSON (tsconfig supports them).
/// Also strips trailing commas before `}` or `]`.
pub fn strip_json_comments(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'"' {
            // Inside a string literal — find the closing quote
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2; // skip escaped char
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            result.push_str(&input[start..i]);
        } else if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            // Single-line comment — skip to end of line
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
        } else if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Multi-line comment — skip to */
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2; // skip */
            }
        } else {
            // Non-string, non-comment content — find next boundary and copy slice.
            let start = i;
            i += 1;
            while i < len
                && bytes[i] != b'"'
                && !(i + 1 < len
                    && bytes[i] == b'/'
                    && (bytes[i + 1] == b'/' || bytes[i + 1] == b'*'))
            {
                i += 1;
            }
            result.push_str(&input[start..i]);
        }
    }

    strip_trailing_commas(&result)
}

/// Remove trailing commas before `}` or `]` in JSON.
fn strip_trailing_commas(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'"' {
            // Copy string literals unchanged
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            result.push_str(&input[start..i]);
        } else if bytes[i] == b',' {
            // Check if this comma is followed only by whitespace/newlines
            // and then a closing bracket
            let mut j = i + 1;
            while j < len
                && (bytes[j] == b' ' || bytes[j] == b'\n' || bytes[j] == b'\r' || bytes[j] == b'\t')
            {
                j += 1;
            }
            if j < len && (bytes[j] == b'}' || bytes[j] == b']') {
                // Skip the trailing comma
                i += 1;
            } else {
                result.push(bytes[i] as char);
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
}

// ═══════════════════════════════════════════════════════════════════════════
// Tsconfig Discovery
// ═══════════════════════════════════════════════════════════════════════════

/// Discover all tsconfig.json files under a workspace root.
///
/// Finds both `tsconfig.json` and variant files like `tsconfig.app.json`,
/// `tsconfig.node.json`, etc. Excludes `node_modules` and dot-directories.
pub fn discover_tsconfigs(root: &Path) -> Vec<TsConfigEntry> {
    let mut entries = Vec::new();
    let root_str = root.to_string_lossy().replace('\\', "/");
    let root_component_count = root.components().count();

    for glob_pattern in &[
        format!("{root_str}/**/tsconfig.json"),
        format!("{root_str}/**/tsconfig.*.json"),
    ] {
        match glob::glob(glob_pattern) {
            Ok(paths) => {
                for entry in paths.flatten() {
                    // Only check components below the root (skip the root prefix
                    // itself — on Windows tempdir paths may start with `.tmp...`).
                    let relative_components: Vec<_> =
                        entry.components().skip(root_component_count).collect();

                    // Skip node_modules
                    if relative_components
                        .iter()
                        .any(|c| c.as_os_str() == "node_modules")
                    {
                        continue;
                    }
                    // Skip dot-directories (only in the relative part)
                    if relative_components.iter().any(|c| {
                        let name = c.as_os_str().to_string_lossy();
                        name.starts_with('.') && name != "."
                    }) {
                        continue;
                    }

                    let entry_str = entry.to_string_lossy().replace('\\', "/");
                    // Skip duplicates (tsconfig.json matches both patterns)
                    if entries.iter().any(|e: &TsConfigEntry| e.path == entry_str) {
                        continue;
                    }

                    if let Some(dir) = entry.parent() {
                        entries.push(TsConfigEntry {
                            path: entry_str,
                            root: dir.to_string_lossy().replace('\\', "/"),
                        });
                    }
                }
            }
            Err(e) => {
                tracing::warn!("failed to glob for tsconfig files: {}", e);
            }
        }
    }

    entries
}

// ═══════════════════════════════════════════════════════════════════════════
// Tsconfig Parsing
// ═══════════════════════════════════════════════════════════════════════════

/// Parse a tsconfig.json and extract compiler options, membership, and references.
/// Follows `extends` chains.
pub fn parse_tsconfig_json(path: &Path) -> Option<ParsedTsConfig> {
    let compiler_options = load_compiler_options(path);
    let membership = load_project_membership(path);
    let references = load_project_references(path);
    Some(ParsedTsConfig {
        compiler_options,
        membership,
        references,
    })
}

/// Load compiler options from a tsconfig.json, following `extends`.
pub fn load_compiler_options(tsconfig_path: &Path) -> IdeProjectCompilerOptions {
    load_compiler_options_inner(tsconfig_path, 0).unwrap_or_default()
}

fn load_compiler_options_inner(
    tsconfig_path: &Path,
    depth: u8,
) -> Option<IdeProjectCompilerOptions> {
    if depth > MAX_TSCONFIG_EXTENDS_DEPTH {
        return None;
    }

    let tsconfig_dir = tsconfig_path.parent()?;
    let content = std::fs::read_to_string(tsconfig_path).ok()?;
    let cleaned = strip_json_comments(&content);
    let json: serde_json::Value = serde_json::from_str(&cleaned).ok()?;

    let inherited = json
        .get("extends")
        .and_then(|value| value.as_str())
        .and_then(|extends| resolve_tsconfig_extends(tsconfig_dir, extends))
        .and_then(|base_path| load_compiler_options_inner(&base_path, depth + 1))
        .unwrap_or_default();

    let mut compiler_options = inherited;
    let Some(raw_compiler_options) = json.get("compilerOptions") else {
        return Some(compiler_options);
    };

    if let Some(base_url) = raw_compiler_options
        .get("baseUrl")
        .and_then(|value| value.as_str())
    {
        compiler_options.base_url = Some(resolve_path_value(tsconfig_dir, base_url));
    }

    if let Some(paths) = raw_compiler_options
        .get("paths")
        .and_then(|value| value.as_object())
    {
        let base_url = compiler_options
            .base_url
            .clone()
            .unwrap_or_else(|| tsconfig_dir.to_string_lossy().replace('\\', "/"));
        compiler_options.paths = paths
            .iter()
            .map(|(pattern, targets)| {
                let targets = targets
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|value| value.as_str())
                    .map(|value| resolve_path_target(&base_url, value))
                    .collect::<Vec<_>>();
                (pattern.clone(), targets)
            })
            .collect();
    }

    Some(compiler_options)
}

/// Load project membership (files/include/exclude) from a tsconfig.json.
pub fn load_project_membership(tsconfig_path: &Path) -> ProjectMembership {
    load_project_membership_inner(tsconfig_path, 0).unwrap_or(ProjectMembership::MatchAll)
}

fn load_project_membership_inner(tsconfig_path: &Path, depth: u8) -> Option<ProjectMembership> {
    if depth > MAX_TSCONFIG_EXTENDS_DEPTH {
        return None;
    }

    let tsconfig_dir = tsconfig_path.parent()?;
    let content = std::fs::read_to_string(tsconfig_path).ok()?;
    let cleaned = strip_json_comments(&content);
    let json: serde_json::Value = serde_json::from_str(&cleaned).ok()?;

    let inherited = json
        .get("extends")
        .and_then(|value| value.as_str())
        .and_then(|extends| resolve_tsconfig_extends(tsconfig_dir, extends))
        .and_then(|base_path| load_project_membership_inner(&base_path, depth + 1))
        .unwrap_or(ProjectMembership::MatchAll);

    let has_files = json.get("files").is_some();
    let has_include = json.get("include").is_some();
    let has_exclude = json.get("exclude").is_some();

    if !has_files && !has_include && !has_exclude {
        return Some(inherited);
    }

    let (mut files, mut include, mut exclude) = match inherited {
        ProjectMembership::MatchAll => (Vec::new(), Vec::new(), Vec::new()),
        ProjectMembership::IncludeExclude {
            files,
            include,
            exclude,
        } => (files, include, exclude),
    };

    if has_files {
        files = json_string_array(&json, "files")
            .into_iter()
            .map(|value| resolve_membership_path(tsconfig_dir, &value, false))
            .collect();
    }

    if has_include {
        include = json_string_array(&json, "include")
            .into_iter()
            .map(|value| resolve_membership_path(tsconfig_dir, &value, true))
            .collect();
    }

    if has_exclude {
        exclude = json_string_array(&json, "exclude")
            .into_iter()
            .map(|value| resolve_membership_path(tsconfig_dir, &value, true))
            .collect();
    }

    Some(ProjectMembership::IncludeExclude {
        files,
        include,
        exclude,
    })
}

/// Load project references from a tsconfig.json.
pub fn load_project_references(tsconfig_path: &Path) -> Vec<String> {
    let Some(tsconfig_dir) = tsconfig_path.parent() else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(tsconfig_path) else {
        return Vec::new();
    };
    let cleaned = strip_json_comments(&content);
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&cleaned) else {
        return Vec::new();
    };

    json.get("references")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("path").and_then(|value| value.as_str()))
        .filter_map(|reference| resolve_tsconfig_reference(tsconfig_dir, reference))
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect()
}

/// Check if a workspace has any solution-style tsconfig.json (non-empty references).
pub fn has_solution_style_tsconfig(workspace_root: &Path) -> bool {
    if is_solution_style_tsconfig(&workspace_root.join("tsconfig.json")) {
        return true;
    }

    // Check subdirectories up to 2 levels deep (monorepo packages)
    for depth1 in read_subdirs(workspace_root) {
        if is_solution_style_tsconfig(&depth1.join("tsconfig.json")) {
            return true;
        }
        for depth2 in read_subdirs(&depth1) {
            if is_solution_style_tsconfig(&depth2.join("tsconfig.json")) {
                return true;
            }
        }
    }

    false
}

// ═══════════════════════════════════════════════════════════════════════════
// Raw Paths Extraction (for tsserver configure_paths)
// ═══════════════════════════════════════════════════════════════════════════

/// Extract the raw `baseUrl` and `paths` JSON from a tsconfig for passing to tsserver.
///
/// Follows `extends` and `references` to find the effective paths.
/// Returns `(baseUrl, paths)` as raw JSON values, or `None` if no paths found.
pub fn raw_paths_json(tsconfig_path: &Path) -> Option<(String, serde_json::Value)> {
    raw_paths_json_inner(tsconfig_path, 0)
}

fn raw_paths_json_inner(tsconfig_path: &Path, depth: u8) -> Option<(String, serde_json::Value)> {
    if depth > 5 {
        return None;
    }

    let tsconfig_dir = tsconfig_path.parent()?;
    let content = std::fs::read_to_string(tsconfig_path).ok()?;
    let cleaned = strip_json_comments(&content);
    let json: serde_json::Value = serde_json::from_str(&cleaned).ok()?;

    // Check extends first
    if let Some(extends) = json.get("extends").and_then(|v| v.as_str()) {
        if let Some(base_path) = resolve_tsconfig_extends(tsconfig_dir, extends) {
            if let Some(result) = raw_paths_json_inner(&base_path, depth + 1) {
                // If base has paths, use them (current config may override)
                let base_result = Some(result);
                // Check if current config overrides
                if let Some(co) = json.get("compilerOptions") {
                    if co.get("paths").is_some() {
                        // Current config overrides base — fall through
                    } else {
                        return base_result;
                    }
                } else {
                    return base_result;
                }
            }
        }
    }

    // Check this config's own compilerOptions.paths
    if let Some(co) = json.get("compilerOptions") {
        if let Some(paths) = co.get("paths") {
            let base_url = co
                .get("baseUrl")
                .and_then(|v| v.as_str())
                .map(|b| normalize_path_buf(&tsconfig_dir.join(b)))
                .unwrap_or_else(|| normalize_path_buf(tsconfig_dir));
            return Some((base_url, paths.clone()));
        }
    }

    // Follow references
    if let Some(refs) = json.get("references").and_then(|v| v.as_array()) {
        for ref_entry in refs {
            if let Some(ref_path) = ref_entry.get("path").and_then(|v| v.as_str()) {
                if let Some(ref_tsconfig) = resolve_tsconfig_reference(tsconfig_dir, ref_path) {
                    if let Some(result) = raw_paths_json_inner(&ref_tsconfig, depth + 1) {
                        return Some(result);
                    }
                }
            }
        }
    }

    None
}

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Resolve `extends` field from tsconfig.json to an absolute path.
pub fn resolve_tsconfig_extends(tsconfig_dir: &Path, extends: &str) -> Option<PathBuf> {
    if extends.starts_with('.') {
        // Relative path
        let resolved = tsconfig_dir.join(extends);
        if resolved.exists() {
            return Some(resolved);
        }
        let with_json = resolved.with_extension("json");
        if with_json.exists() {
            return Some(with_json);
        }
    } else {
        // Package name — try node_modules resolution
        let mut dir = tsconfig_dir;
        loop {
            let candidate = dir.join("node_modules").join(extends);
            if candidate.exists() {
                return Some(candidate);
            }
            let with_json = candidate.with_extension("json");
            if with_json.exists() {
                return Some(with_json);
            }
            // Try as directory with tsconfig.json
            let as_dir = dir.join("node_modules").join(extends).join("tsconfig.json");
            if as_dir.exists() {
                return Some(as_dir);
            }
            match dir.parent() {
                Some(parent) if parent != dir => dir = parent,
                _ => break,
            }
        }
    }
    None
}

/// Resolve a `references[].path` entry to the actual tsconfig file path.
fn resolve_tsconfig_reference(tsconfig_dir: &Path, ref_path: &str) -> Option<PathBuf> {
    let resolved = tsconfig_dir.join(ref_path);

    // Direct file reference
    if resolved.is_file() {
        return Some(resolved);
    }

    // Directory reference -> look for tsconfig.json inside
    if resolved.is_dir() {
        let tsconfig = resolved.join("tsconfig.json");
        if tsconfig.exists() {
            return Some(tsconfig);
        }
    }

    // Try with .json extension
    let with_json = if resolved.extension().is_none() {
        resolved.with_extension("json")
    } else {
        return None;
    };
    if with_json.exists() {
        return Some(with_json);
    }

    None
}

fn is_solution_style_tsconfig(tsconfig_path: &Path) -> bool {
    let content = match std::fs::read_to_string(tsconfig_path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let cleaned = strip_json_comments(&content);
    let json: serde_json::Value = match serde_json::from_str(&cleaned) {
        Ok(v) => v,
        Err(_) => return false,
    };
    json.get("references")
        .and_then(|v| v.as_array())
        .is_some_and(|refs| !refs.is_empty())
}

fn read_subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_ok_and(|ft| ft.is_dir()) && {
                let name = e.file_name();
                let name = name.to_string_lossy();
                !name.starts_with('.') && name != "node_modules" && name != "dist"
            }
        })
        .map(|e| e.path())
        .collect()
}

fn json_string_array(json: &serde_json::Value, key: &str) -> Vec<String> {
    json.get(key)
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect()
}

fn resolve_membership_path(tsconfig_dir: &Path, value: &str, allow_directory_glob: bool) -> String {
    let resolved = if Path::new(value).is_absolute() {
        PathBuf::from(value)
    } else {
        tsconfig_dir.join(value)
    };

    let normalized = resolved.to_string_lossy().replace('\\', "/");
    if !allow_directory_glob {
        return normalized;
    }

    if normalized.contains('*') || normalized.contains('?') || normalized.contains('[') {
        return normalized;
    }

    if Path::new(&resolved)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some()
    {
        return normalized;
    }

    format!("{normalized}/**/*")
}

fn resolve_path_value(tsconfig_dir: &Path, value: &str) -> String {
    if Path::new(value).is_absolute() {
        normalize_path_buf(&PathBuf::from(value))
    } else {
        normalize_path_buf(&tsconfig_dir.join(value))
    }
}

fn resolve_path_target(base_url: &str, value: &str) -> String {
    if Path::new(value).is_absolute() {
        normalize_path_buf(&PathBuf::from(value))
    } else {
        normalize_path_buf(&PathBuf::from(base_url).join(value))
    }
}

/// Normalize a path buffer by collapsing `.` and `..` segments.
pub fn normalize_path_buf(path: &Path) -> String {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

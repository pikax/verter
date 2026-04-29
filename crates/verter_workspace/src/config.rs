//! Tsconfig discovery and parsing for project configuration.
//!
//! All filesystem access goes through `&dyn WorkspaceRead` (Phase 6b
//! sub-plan §6b.D2b — these helpers are read-only consumers).

use std::path::{Path, PathBuf};

use crate::resolver::{
    join_paths, normalize_canonical_id, parent_dir, IdeProjectCompilerOptions, ProjectMembership,
};
use crate::traits::WorkspaceRead;

/// Maximum depth for tsconfig `extends` chain resolution.
const MAX_TSCONFIG_EXTENDS_DEPTH: u8 = 8;

// ═══════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct TsConfigEntry {
    pub path: String,
    pub root: String,
}

#[derive(Debug, Clone)]
pub struct ParsedTsConfig {
    pub compiler_options: IdeProjectCompilerOptions,
    pub membership: ProjectMembership,
    pub references: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════════════════
// JSON Comment Stripping
// ═══════════════════════════════════════════════════════════════════════════

pub fn strip_json_comments(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'"' {
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
        } else if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
        } else if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2;
            }
        } else {
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

fn strip_trailing_commas(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'"' {
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
            let mut j = i + 1;
            while j < len
                && (bytes[j] == b' ' || bytes[j] == b'\n' || bytes[j] == b'\r' || bytes[j] == b'\t')
            {
                j += 1;
            }
            if j < len && (bytes[j] == b'}' || bytes[j] == b']') {
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
// Tsconfig Discovery — disk-walking, prunes node_modules / dot-dirs at descent
// ═══════════════════════════════════════════════════════════════════════════

/// Discover all `tsconfig.json` (and `tsconfig.*.json`) files under a
/// workspace root.
///
/// This is the one function in `verter_workspace::config` that still
/// touches disk directly; all sibling helpers take a
/// [`WorkspaceAccess`] and read through it.
///
/// Uses [`walkdir::WalkDir`] with [`follow_links(false)`][walkdir-follow]
/// and a `filter_entry` that prunes descent into:
///
///   * `node_modules` — package-manager output, never authored project
///     code. PNPM-managed `node_modules` is a symlink farm where each
///     `.pnpm/<pkg>/node_modules/<pkg>` directory contains nested
///     `node_modules` symlinks pointing back into `.pnpm/`. Recursive
///     globbing into that graph fans out exponentially and never
///     terminates in practice — it was the dominant source of
///     `ProjectGraph::from_workspace_roots` hangs against real Vue/Nuxt
///     projects.
///   * Directories whose name starts with `.` — `.git`, `.nuxt`,
///     `.pnpm`, `.output`, etc. They are never user source. Tsconfigs
///     inside them (e.g. `.nuxt/tsconfig.json`) are still reachable
///     through `extends` resolution in [`resolve_tsconfig_extends`],
///     which uses [`WorkspaceAccess::read_file`] directly.
///
/// [walkdir-follow]: https://docs.rs/walkdir/latest/walkdir/struct.WalkDir.html#method.follow_links
pub fn discover_tsconfigs(root: &Path) -> Vec<TsConfigEntry> {
    let mut entries = Vec::new();
    let mut seen = rustc_hash::FxHashSet::<String>::default();

    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            // Always include the root itself.
            if entry.depth() == 0 {
                return true;
            }
            // The filter fires on every entry (file or directory), but
            // it's only load-bearing for directories — pruning a file
            // just hides one entry, while pruning a directory skips
            // its entire subtree.
            if !entry.file_type().is_dir() {
                return true;
            }
            let Some(name) = entry.file_name().to_str() else {
                return true;
            };
            if name == "node_modules" {
                return false;
            }
            if name.starts_with('.') {
                return false;
            }
            true
        });

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!("walkdir: {}", err);
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str() else {
            continue;
        };
        let matches =
            name == "tsconfig.json" || (name.starts_with("tsconfig.") && name.ends_with(".json"));
        if !matches {
            continue;
        }
        let path = entry.path();
        let path_str = path.to_string_lossy().replace('\\', "/");
        if !seen.insert(path_str.clone()) {
            continue;
        }
        if let Some(dir) = path.parent() {
            entries.push(TsConfigEntry {
                path: path_str,
                root: dir.to_string_lossy().replace('\\', "/"),
            });
        }
    }

    entries
}

// ═══════════════════════════════════════════════════════════════════════════
// Tsconfig Parsing — all workspace-backed
// ═══════════════════════════════════════════════════════════════════════════

/// Parse a tsconfig.json. All file reads go through `ws`.
pub fn parse_tsconfig_json(ws: &dyn WorkspaceRead, tsconfig_path: &str) -> Option<ParsedTsConfig> {
    let compiler_options = load_compiler_options(ws, tsconfig_path);
    let membership = load_project_membership(ws, tsconfig_path);
    let references = load_project_references(ws, tsconfig_path);
    Some(ParsedTsConfig {
        compiler_options,
        membership,
        references,
    })
}

/// Load compiler options from a tsconfig.json, following `extends`.
pub fn load_compiler_options(
    ws: &dyn WorkspaceRead,
    tsconfig_path: &str,
) -> IdeProjectCompilerOptions {
    load_compiler_options_inner(ws, tsconfig_path, 0).unwrap_or_default()
}

fn load_compiler_options_inner(
    ws: &dyn WorkspaceRead,
    tsconfig_path: &str,
    depth: u8,
) -> Option<IdeProjectCompilerOptions> {
    if depth > MAX_TSCONFIG_EXTENDS_DEPTH {
        return None;
    }

    let tsconfig_dir = parent_dir(tsconfig_path);
    let content = ws.read_file(tsconfig_path)?;
    let cleaned = strip_json_comments(&content);
    let json: serde_json::Value = serde_json::from_str(&cleaned).ok()?;

    let inherited = json
        .get("extends")
        .and_then(|value| value.as_str())
        .and_then(|extends| resolve_tsconfig_extends(ws, &tsconfig_dir, extends))
        .and_then(|base_path| load_compiler_options_inner(ws, &base_path, depth + 1))
        .unwrap_or_default();

    let mut compiler_options = inherited;
    let Some(raw_compiler_options) = json.get("compilerOptions") else {
        return Some(compiler_options);
    };

    if let Some(base_url) = raw_compiler_options
        .get("baseUrl")
        .and_then(|value| value.as_str())
    {
        compiler_options.base_url = Some(resolve_path_value(&tsconfig_dir, base_url));
    }

    if let Some(paths) = raw_compiler_options
        .get("paths")
        .and_then(|value| value.as_object())
    {
        let base_url = compiler_options
            .base_url
            .clone()
            .unwrap_or(tsconfig_dir.clone());
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
pub fn load_project_membership(ws: &dyn WorkspaceRead, tsconfig_path: &str) -> ProjectMembership {
    load_project_membership_inner(ws, tsconfig_path, 0).unwrap_or(ProjectMembership::MatchAll)
}

fn load_project_membership_inner(
    ws: &dyn WorkspaceRead,
    tsconfig_path: &str,
    depth: u8,
) -> Option<ProjectMembership> {
    if depth > MAX_TSCONFIG_EXTENDS_DEPTH {
        return None;
    }

    let tsconfig_dir = parent_dir(tsconfig_path);
    let content = ws.read_file(tsconfig_path)?;
    let cleaned = strip_json_comments(&content);
    let json: serde_json::Value = serde_json::from_str(&cleaned).ok()?;

    let inherited = json
        .get("extends")
        .and_then(|value| value.as_str())
        .and_then(|extends| resolve_tsconfig_extends(ws, &tsconfig_dir, extends))
        .and_then(|base_path| load_project_membership_inner(ws, &base_path, depth + 1))
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
            .map(|value| resolve_membership_path(&tsconfig_dir, &value, false))
            .collect();
    }

    if has_include {
        include = json_string_array(&json, "include")
            .into_iter()
            .map(|value| resolve_membership_path(&tsconfig_dir, &value, true))
            .collect();
    }

    if has_exclude {
        exclude = json_string_array(&json, "exclude")
            .into_iter()
            .map(|value| resolve_membership_path(&tsconfig_dir, &value, true))
            .collect();
    }

    Some(ProjectMembership::IncludeExclude {
        files,
        include,
        exclude,
    })
}

/// Load project references from a tsconfig.json.
pub fn load_project_references(ws: &dyn WorkspaceRead, tsconfig_path: &str) -> Vec<String> {
    let tsconfig_dir = parent_dir(tsconfig_path);
    let Some(content) = ws.read_file(tsconfig_path) else {
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
        .filter_map(|reference| resolve_tsconfig_reference(ws, &tsconfig_dir, reference))
        .collect()
}

/// Check if a workspace has any solution-style tsconfig.json.
pub fn has_solution_style_tsconfig(ws: &dyn WorkspaceRead, workspace_root: &str) -> bool {
    let tsconfig = join_paths(workspace_root, "tsconfig.json");
    if is_solution_style_tsconfig(ws, &tsconfig) {
        return true;
    }

    let Ok(depth1_entries) = ws.read_dir(workspace_root) else {
        return false;
    };
    for d1 in &depth1_entries {
        if !d1.is_dir {
            continue;
        }
        let name = d1.path.rsplit('/').next().unwrap_or(&d1.path);
        if name.starts_with('.') || name == "node_modules" || name == "dist" {
            continue;
        }
        if is_solution_style_tsconfig(ws, &join_paths(&d1.path, "tsconfig.json")) {
            return true;
        }
        let Ok(depth2_entries) = ws.read_dir(&d1.path) else {
            continue;
        };
        for d2 in &depth2_entries {
            if !d2.is_dir {
                continue;
            }
            let name2 = d2.path.rsplit('/').next().unwrap_or(&d2.path);
            if name2.starts_with('.') || name2 == "node_modules" || name2 == "dist" {
                continue;
            }
            if is_solution_style_tsconfig(ws, &join_paths(&d2.path, "tsconfig.json")) {
                return true;
            }
        }
    }

    false
}

fn is_solution_style_tsconfig(ws: &dyn WorkspaceRead, tsconfig_path: &str) -> bool {
    let Some(content) = ws.read_file(tsconfig_path) else {
        return false;
    };
    let cleaned = strip_json_comments(&content);
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&cleaned) else {
        return false;
    };
    json.get("references")
        .and_then(|v| v.as_array())
        .is_some_and(|refs| !refs.is_empty())
}

// ═══════════════════════════════════════════════════════════════════════════
// Raw Paths Extraction (for tsserver configure_paths)
// ═══════════════════════════════════════════════════════════════════════════

/// Extract the raw `baseUrl` and `paths` JSON from a tsconfig.
pub fn raw_paths_json(
    ws: &dyn WorkspaceRead,
    tsconfig_path: &str,
) -> Option<(String, serde_json::Value)> {
    raw_paths_json_inner(ws, tsconfig_path, 0)
}

fn raw_paths_json_inner(
    ws: &dyn WorkspaceRead,
    tsconfig_path: &str,
    depth: u8,
) -> Option<(String, serde_json::Value)> {
    if depth > 5 {
        return None;
    }

    let tsconfig_dir = parent_dir(tsconfig_path);
    let content = ws.read_file(tsconfig_path)?;
    let cleaned = strip_json_comments(&content);
    let json: serde_json::Value = serde_json::from_str(&cleaned).ok()?;

    let inherited = json
        .get("extends")
        .and_then(|v| v.as_str())
        .and_then(|extends| resolve_tsconfig_extends(ws, &tsconfig_dir, extends))
        .and_then(|base_path| raw_paths_json_inner(ws, &base_path, depth + 1));

    let co = json.get("compilerOptions");
    let own_base_url = co
        .and_then(|c| c.get("baseUrl"))
        .and_then(|v| v.as_str())
        .map(|b| resolve_path_value(&tsconfig_dir, b));
    let own_paths = co.and_then(|c| c.get("paths")).cloned();

    match (own_paths, own_base_url, inherited) {
        (Some(paths), Some(base_url), _) => Some((base_url, paths)),
        (Some(paths), None, Some((inherited_base_url, _))) => Some((inherited_base_url, paths)),
        (Some(paths), None, None) => Some((tsconfig_dir, paths)),
        (None, Some(base_url), Some((_, inherited_paths))) => Some((base_url, inherited_paths)),
        (None, _, Some(inherited)) => Some(inherited),
        (None, _, None) => {
            if let Some(refs) = json.get("references").and_then(|v| v.as_array()) {
                for ref_entry in refs {
                    if let Some(ref_path) = ref_entry.get("path").and_then(|v| v.as_str()) {
                        if let Some(ref_tsconfig) =
                            resolve_tsconfig_reference(ws, &tsconfig_dir, ref_path)
                        {
                            if let Some(result) = raw_paths_json_inner(ws, &ref_tsconfig, depth + 1)
                            {
                                return Some(result);
                            }
                        }
                    }
                }
            }
            None
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Resolve `extends` field from tsconfig.json.
pub fn resolve_tsconfig_extends(
    ws: &dyn WorkspaceRead,
    tsconfig_dir: &str,
    extends: &str,
) -> Option<String> {
    if extends.starts_with('.') {
        let resolved = join_paths(tsconfig_dir, extends);
        if ws.file_exists(&resolved) {
            return Some(resolved);
        }
        let with_json = format!("{resolved}.json");
        if ws.file_exists(&with_json) {
            return Some(with_json);
        }
    } else {
        let mut dir = tsconfig_dir.to_string();
        loop {
            let nm = join_paths(&dir, "node_modules");
            let candidate = join_paths(&nm, extends);
            if ws.file_exists(&candidate) {
                return Some(candidate);
            }
            let with_json = format!("{candidate}.json");
            if ws.file_exists(&with_json) {
                return Some(with_json);
            }
            let as_dir = join_paths(&candidate, "tsconfig.json");
            if ws.file_exists(&as_dir) {
                return Some(as_dir);
            }
            let next = parent_dir(&dir);
            if next == dir || next.is_empty() {
                break;
            }
            dir = next;
        }
    }
    None
}

fn resolve_tsconfig_reference(
    ws: &dyn WorkspaceRead,
    tsconfig_dir: &str,
    ref_path: &str,
) -> Option<String> {
    let resolved = join_paths(tsconfig_dir, ref_path);

    if ws.file_exists(&resolved) && !ws.is_dir(&resolved) {
        return Some(resolved);
    }

    if ws.is_dir(&resolved) {
        let tsconfig = join_paths(&resolved, "tsconfig.json");
        if ws.file_exists(&tsconfig) {
            return Some(tsconfig);
        }
    }

    if !resolved.contains('.') || resolved.ends_with('/') {
        let with_json = format!("{resolved}.json");
        if ws.file_exists(&with_json) {
            return Some(with_json);
        }
    }

    None
}

fn json_string_array(json: &serde_json::Value, key: &str) -> Vec<String> {
    json.get(key)
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect()
}

fn resolve_membership_path(tsconfig_dir: &str, value: &str, allow_directory_glob: bool) -> String {
    let normalized = if crate::resolver::is_absolute_specifier(value) {
        normalize_canonical_id(value)
    } else {
        join_paths(tsconfig_dir, value)
    };

    if !allow_directory_glob {
        return normalized;
    }

    if normalized.contains('*') || normalized.contains('?') || normalized.contains('[') {
        return normalized;
    }

    if let Some(last_segment) = normalized.rsplit('/').next() {
        if last_segment.contains('.') {
            return normalized;
        }
    }

    format!("{normalized}/**/*")
}

fn resolve_path_value(tsconfig_dir: &str, value: &str) -> String {
    if crate::resolver::is_absolute_specifier(value) {
        normalize_canonical_id(value)
    } else {
        join_paths(tsconfig_dir, value)
    }
}

fn resolve_path_target(base_url: &str, value: &str) -> String {
    if crate::resolver::is_absolute_specifier(value) {
        normalize_canonical_id(value)
    } else {
        join_paths(base_url, value)
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

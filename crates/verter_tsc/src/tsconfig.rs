//! tsconfig.json reading and Vue/TS file discovery.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use walkdir::WalkDir;

/// Strip the Windows `\\?\` extended-length path prefix.
///
/// `std::fs::canonicalize()` on Windows returns paths prefixed with `\\?\`,
/// which breaks external tools (cmd.exe, tsc) and causes path comparison issues.
/// Return the parent directory of `path`, falling back to `"."` when
/// `Path::parent()` returns an empty path (which happens for bare filenames
/// like `"tsconfig.json"` — `parent()` yields `Some("")`, not `None`).
fn safe_parent(path: &Path) -> &Path {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    }
}

pub(crate) fn strip_unc_prefix(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        p.to_path_buf()
    }
}

/// Loaded tsconfig with resolved file lists.
pub struct TsConfig {
    /// Root directory (directory containing the tsconfig.json).
    pub root_dir: PathBuf,
    /// All `.vue` files matching the tsconfig include/files patterns.
    pub vue_files: Vec<PathBuf>,
    /// All `.ts`/`.tsx` files matching the tsconfig include/files patterns.
    pub ts_files: Vec<PathBuf>,
}

/// Raw tsconfig.json structure (minimal subset we need).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTsConfig {
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(rename = "extends")]
    #[allow(dead_code)] // reserved for future extends-chain resolution
    extends: Option<serde_json::Value>,
    #[serde(default)]
    references: Vec<RawReference>,
}

#[derive(Debug, Deserialize)]
struct RawReference {
    path: String,
}

/// Load and parse a `tsconfig.json`, returning the resolved file lists.
pub fn load_tsconfig(tsconfig_path: &Path) -> Result<TsConfig, String> {
    let mut vue_files: Vec<PathBuf> = Vec::new();
    let mut ts_files: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    load_tsconfig_recursive(tsconfig_path, &mut vue_files, &mut ts_files, &mut seen, 0)?;

    let root_dir = strip_unc_prefix(
        &safe_parent(tsconfig_path)
            .canonicalize()
            .map_err(|e| format!("cannot resolve tsconfig directory: {e}"))?,
    );

    Ok(TsConfig {
        root_dir,
        vue_files,
        ts_files,
    })
}

fn load_tsconfig_recursive(
    tsconfig_path: &Path,
    vue_files: &mut Vec<PathBuf>,
    ts_files: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    depth: usize,
) -> Result<(), String> {
    if depth > 10 {
        return Ok(()); // guard against circular references
    }

    let root_dir = strip_unc_prefix(
        &safe_parent(tsconfig_path)
            .canonicalize()
            .map_err(|e| format!("cannot resolve tsconfig directory: {e}"))?,
    );

    let raw = match load_raw_tsconfig(tsconfig_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "verter-tsc: skipping tsconfig {}: {e}",
                tsconfig_path.display()
            );
            return Ok(());
        }
    };

    // If files:[] + references — follow the references instead of scanning locally.
    if raw.files.is_empty() && raw.include.is_empty() && !raw.references.is_empty() {
        for r in &raw.references {
            let ref_path = root_dir.join(&r.path);
            // If the path points to a directory, append tsconfig.json.
            let ref_tsconfig = if ref_path.is_dir() {
                ref_path.join("tsconfig.json")
            } else if ref_path.extension().is_none() {
                ref_path.with_extension("json")
            } else {
                ref_path
            };
            if ref_tsconfig.exists() {
                load_tsconfig_recursive(&ref_tsconfig, vue_files, ts_files, seen, depth + 1)?;
            }
        }
        return Ok(());
    }

    // Collect explicit file list (if any).
    for file in &raw.files {
        let p = root_dir.join(file);
        if p.exists() {
            let canon = strip_unc_prefix(&p.canonicalize().unwrap_or(p));
            if seen.insert(canon.clone()) {
                classify_file(&canon, vue_files, ts_files);
            }
        }
    }

    // Build exclude rules from patterns.
    // Always exclude node_modules (matching tsc behavior), even when explicit
    // exclude patterns are provided.
    let mut exclude_rules: Vec<ExcludeRule> =
        vec![ExcludeRule::AnyComponent("node_modules".to_string())];
    if raw.exclude.is_empty() {
        exclude_rules.push(ExcludeRule::Prefix(root_dir.join("bower_components")));
        exclude_rules.push(ExcludeRule::Prefix(root_dir.join("jspm_packages")));
    } else {
        for p in &raw.exclude {
            exclude_rules.push(ExcludeRule::from_pattern(p, &root_dir));
        }
    }

    // Determine include patterns — default to root if none specified.
    let effective_include: Vec<String> = if raw.include.is_empty() && raw.files.is_empty() {
        // TypeScript default: include everything under root.
        vec![String::from(".")]
    } else {
        raw.include.clone()
    };

    for pattern in &effective_include {
        let prefix = glob_dir_prefix(pattern);
        let recursive = pattern.contains("**");
        let is_non_recursive_glob = !recursive && pattern.contains('*');

        // Determine extension filter from the tail of the pattern.
        let ext_filter = infer_ext_filter(pattern);

        let scan_dir = root_dir.join(prefix);
        if !scan_dir.exists() {
            continue;
        }

        // For non-recursive globs (e.g. "./*.ts"), only scan one level.
        let max_depth = if recursive {
            usize::MAX
        } else if is_non_recursive_glob {
            1
        } else {
            usize::MAX // bare directory — recurse fully
        };

        for entry in WalkDir::new(&scan_dir)
            .follow_links(true)
            .max_depth(max_depth)
            .into_iter()
            // CRITICAL: filter_entry() prunes the walk — prevents descending into
            // excluded directories (node_modules, dist) and hidden directories (.git).
            // Without this, WalkDir enters node_modules and yields every file inside,
            // causing 19s+ overhead on projects with large dependency trees.
            .filter_entry(|e| {
                if e.file_type().is_dir() {
                    let name = e.file_name().to_string_lossy();
                    // Skip hidden directories
                    if name.starts_with('.') && name.len() > 1 {
                        return false;
                    }
                    // Skip excluded directories
                    !is_excluded(e.path(), &exclude_rules)
                } else {
                    true
                }
            })
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            if path.is_file() {
                // Check file-level exclude rules (e.g. *.spec.ts, *.stories.ts).
                if is_excluded(path, &exclude_rules) {
                    continue;
                }
                let ext = path.extension().and_then(|e| e.to_str());
                let include = match ext_filter {
                    Some("vue") => ext == Some("vue"),
                    Some("ts") => {
                        matches!(ext, Some("ts") | Some("tsx") | Some("mts") | Some("cts"))
                    }
                    Some("js") => {
                        matches!(ext, Some("js") | Some("jsx") | Some("mjs") | Some("cjs"))
                    }
                    Some(_) | None => true, // No extension filter — include all classifiable files.
                };
                if include {
                    let canon = strip_unc_prefix(
                        &path.canonicalize().unwrap_or_else(|_| path.to_path_buf()),
                    );
                    if seen.insert(canon.clone()) {
                        classify_file(&canon, vue_files, ts_files);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Extract the non-glob directory prefix from a glob pattern.
///
/// Examples:
///   `"./packages/**/*.vue"` → `"./packages"`
///   `"./*.ts"`              → `"."`
///   `"src"`                 → `"src"`
///   `"typings/env.d.ts"`    → `"typings/env.d.ts"` (no glob)
fn glob_dir_prefix(pattern: &str) -> &str {
    match pattern.find(['*', '?']) {
        None => pattern, // No glob — literal path or directory.
        Some(glob_pos) => {
            let before = &pattern[..glob_pos];
            // Find the last slash before the first glob char.
            match before.rfind('/') {
                Some(slash) => &pattern[..slash],
                None => ".", // Glob starts at root (e.g. "*.ts").
            }
        }
    }
}

/// Infer the extension filter from an include pattern's tail.
fn infer_ext_filter(pattern: &str) -> Option<&'static str> {
    let lower = pattern.to_ascii_lowercase();
    if lower.ends_with(".vue") {
        Some("vue")
    } else if lower.ends_with(".ts")
        || lower.ends_with(".tsx")
        || lower.ends_with(".mts")
        || lower.ends_with(".cts")
    {
        Some("ts")
    } else if lower.ends_with(".js")
        || lower.ends_with(".jsx")
        || lower.ends_with(".mjs")
        || lower.ends_with(".cjs")
    {
        Some("js")
    } else {
        None // No extension filter — accept all classifiable files.
    }
}

fn load_raw_tsconfig(path: &Path) -> Result<RawTsConfig, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    // Strip JSON comments (tsconfig allows // and /* */ comments).
    let stripped = strip_json_comments(&content);
    serde_json::from_str::<RawTsConfig>(&stripped)
        .map_err(|e| format!("invalid tsconfig.json at {}: {e}", path.display()))
}

fn classify_file(path: &Path, vue_files: &mut Vec<PathBuf>, ts_files: &mut Vec<PathBuf>) {
    match path.extension().and_then(|e| e.to_str()) {
        Some("vue") => vue_files.push(path.to_path_buf()),
        Some("ts") | Some("tsx") | Some("mts") | Some("cts") => ts_files.push(path.to_path_buf()),
        _ => {}
    }
}

/// An exclude rule derived from a tsconfig `exclude` pattern.
enum ExcludeRule {
    /// Exclude any path that starts with this prefix (e.g. `node_modules`).
    Prefix(PathBuf),
    /// Exclude any path that contains this component name (e.g. `dist` from `**/dist`).
    AnyComponent(String),
    /// Exclude files matching a glob suffix (e.g. `*.spec.ts` from `src/**/*.spec.ts`).
    /// Only matches files, never prunes directories.
    FileSuffix(String),
}

impl ExcludeRule {
    fn from_pattern(pattern: &str, root_dir: &Path) -> Self {
        if let Some(tail) = pattern.strip_prefix("**/") {
            // `**/name` or `**/name/**` → exclude any path component named `name`.
            let name = tail
                .trim_end_matches("/**")
                .trim_end_matches("/*")
                .trim_end_matches('/')
                .to_string();
            if name.contains('*') {
                // e.g. `**/*.spec.ts` → file suffix match
                ExcludeRule::FileSuffix(name)
            } else {
                ExcludeRule::AnyComponent(name)
            }
        } else if pattern.contains("/**/") {
            // `src/**/__tests__/*` → extract the directory name after `**/` as an
            // AnyComponent rule. Without this, glob_dir_prefix("src/**/__tests__/*")
            // returns "src" and incorrectly excludes the entire src directory.
            if let Some(pos) = pattern.find("/**/") {
                let after = &pattern[pos + 4..]; // skip "/**/"
                let name = after
                    .split('/')
                    .next()
                    .unwrap_or(after)
                    .trim_end_matches("/*")
                    .trim_end_matches('*');
                if !name.is_empty() && !name.contains('*') {
                    return ExcludeRule::AnyComponent(name.to_string());
                }
            }
            // Patterns like `src/**/*.spec.ts` — extract the file glob part.
            if let Some(last_slash) = pattern.rfind('/') {
                let file_glob = &pattern[last_slash + 1..];
                if file_glob.contains('*') {
                    return ExcludeRule::FileSuffix(file_glob.to_string());
                }
            }
            // Fallback: prefix-based
            let prefix = glob_dir_prefix(pattern);
            ExcludeRule::Prefix(root_dir.join(prefix))
        } else {
            let prefix = glob_dir_prefix(pattern);
            ExcludeRule::Prefix(root_dir.join(prefix))
        }
    }

    fn matches(&self, path: &Path) -> bool {
        match self {
            ExcludeRule::Prefix(prefix) => path.starts_with(prefix),
            ExcludeRule::AnyComponent(name) => path
                .components()
                .any(|c| c.as_os_str().to_string_lossy() == name.as_str()),
            ExcludeRule::FileSuffix(glob) => {
                // Only match files, never directories.
                // `glob` is e.g. "*.spec.ts" — match against the file name.
                let file_name = match path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n,
                    None => return false,
                };
                // Simple glob matching: "*.ext" matches any file ending with ".ext".
                if let Some(suffix) = glob.strip_prefix('*') {
                    file_name.ends_with(suffix)
                } else {
                    file_name == glob
                }
            }
        }
    }
}

fn is_excluded(path: &Path, rules: &[ExcludeRule]) -> bool {
    rules.iter().any(|r| r.matches(path))
}

/// Strip single-line (`//`) and block (`/* */`) comments from JSON-like content.
/// TypeScript tsconfig files allow JS-style comments.
fn strip_json_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let mut in_string = false;
    let mut prev_backslash = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' && !prev_backslash {
                prev_backslash = true;
            } else {
                if c == '"' && !prev_backslash {
                    in_string = false;
                }
                prev_backslash = false;
            }
        } else {
            match c {
                '"' => {
                    in_string = true;
                    out.push(c);
                }
                '/' => match chars.peek() {
                    Some('/') => {
                        // Single-line comment — consume until newline.
                        chars.next();
                        while let Some(&nc) = chars.peek() {
                            chars.next();
                            if nc == '\n' {
                                out.push('\n');
                                break;
                            }
                        }
                    }
                    Some('*') => {
                        // Block comment — consume until `*/`.
                        chars.next();
                        while let Some(nc) = chars.next() {
                            if nc == '*' && chars.peek() == Some(&'/') {
                                chars.next();
                                break;
                            }
                        }
                    }
                    _ => out.push(c),
                },
                _ => out.push(c),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── safe_parent ────────────────────────────────────────────────

    #[test]
    fn safe_parent_bare_filename() {
        // Path::parent("tsconfig.json") returns Some("") — safe_parent must yield "."
        assert_eq!(safe_parent(Path::new("tsconfig.json")), Path::new("."));
    }

    #[test]
    fn safe_parent_with_directory() {
        assert_eq!(
            safe_parent(Path::new("some/dir/tsconfig.json")),
            Path::new("some/dir")
        );
    }

    #[test]
    fn safe_parent_dot_canonicalizes() {
        // "." must be canonicalizable (the current directory always exists)
        let parent = safe_parent(Path::new("tsconfig.json"));
        assert!(
            parent.canonicalize().is_ok(),
            "safe_parent(\"tsconfig.json\") = {:?} must be canonicalizable",
            parent
        );
    }

    // ── strip_unc_prefix ───────────────────────────────────────────

    #[test]
    fn strip_unc_prefix_windows_path() {
        let p = Path::new(r"\\?\D:\dev\project");
        assert_eq!(strip_unc_prefix(p), PathBuf::from(r"D:\dev\project"));
    }

    #[test]
    fn strip_unc_prefix_noop_on_normal_path() {
        let p = Path::new("/home/user/project");
        assert_eq!(strip_unc_prefix(p), PathBuf::from("/home/user/project"));
    }

    // ── glob_dir_prefix ────────────────────────────────────────────

    #[test]
    fn glob_dir_prefix_extracts_directory() {
        assert_eq!(glob_dir_prefix("./packages/**/*.vue"), "./packages");
        assert_eq!(glob_dir_prefix("./*.ts"), ".");
        assert_eq!(glob_dir_prefix("src"), "src");
        assert_eq!(glob_dir_prefix("typings/env.d.ts"), "typings/env.d.ts");
    }

    // ── strip_json_comments ────────────────────────────────────────

    #[test]
    fn strip_json_comments_removes_line_comment() {
        let input = r#"{ "a": 1 // comment
}"#;
        let out = strip_json_comments(input);
        assert!(!out.contains("comment"));
        assert!(out.contains("\"a\": 1"));
    }

    #[test]
    fn strip_json_comments_removes_block_comment() {
        let input = r#"{ "a": /* block */ 1 }"#;
        let out = strip_json_comments(input);
        assert!(!out.contains("block"));
        assert!(out.contains("\"a\":"));
    }
}

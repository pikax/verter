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
    /// Resolved `compilerOptions.declarationDir` (absolute path).
    /// Inherited through the `extends` chain; child overrides parent.
    pub declaration_dir: Option<PathBuf>,
    /// Resolved `compilerOptions.outDir` (absolute path).
    /// Inherited through the `extends` chain; child overrides parent.
    pub out_dir: Option<PathBuf>,
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
    extends: Option<serde_json::Value>,
    #[serde(default)]
    references: Vec<RawReference>,
    #[serde(default)]
    compiler_options: Option<RawCompilerOptions>,
}

/// Minimal compiler options we care about for output dir resolution.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCompilerOptions {
    declaration_dir: Option<String>,
    out_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawReference {
    path: String,
}

/// Resolved output directories from the extends chain.
struct ResolvedOutputDirs {
    declaration_dir: Option<PathBuf>,
    out_dir: Option<PathBuf>,
}

/// Resolve `compilerOptions.declarationDir` and `compilerOptions.outDir` through
/// the `extends` chain. Child values override parent. Relative paths are resolved
/// against the directory of the tsconfig file that declares them.
fn resolve_output_dirs(tsconfig_path: &Path, depth: usize) -> ResolvedOutputDirs {
    if depth > 10 {
        return ResolvedOutputDirs {
            declaration_dir: None,
            out_dir: None,
        };
    }

    let raw = match load_raw_tsconfig(tsconfig_path) {
        Ok(r) => r,
        Err(_) => {
            return ResolvedOutputDirs {
                declaration_dir: None,
                out_dir: None,
            }
        }
    };

    let config_dir = safe_parent(tsconfig_path)
        .canonicalize()
        .map(|p| strip_unc_prefix(&p))
        .unwrap_or_else(|_| safe_parent(tsconfig_path).to_path_buf());

    // Start with values inherited from parent (if extends is set).
    let mut inherited = if let Some(serde_json::Value::String(extends_path)) = &raw.extends {
        let parent_path = resolve_extends_path(extends_path, &config_dir);
        if parent_path.exists() {
            resolve_output_dirs(&parent_path, depth + 1)
        } else {
            ResolvedOutputDirs {
                declaration_dir: None,
                out_dir: None,
            }
        }
    } else {
        ResolvedOutputDirs {
            declaration_dir: None,
            out_dir: None,
        }
    };

    // Override with values from this config (child overrides parent).
    if let Some(ref opts) = raw.compiler_options {
        if let Some(ref decl_dir) = opts.declaration_dir {
            inherited.declaration_dir = Some(config_dir.join(decl_dir));
        }
        if let Some(ref out_dir) = opts.out_dir {
            inherited.out_dir = Some(config_dir.join(out_dir));
        }
    }

    inherited
}

/// Resolve an `extends` path to an absolute tsconfig path.
/// Handles relative paths (resolved against config_dir) and bare package names.
fn resolve_extends_path(extends: &str, config_dir: &Path) -> PathBuf {
    if extends.starts_with('.') {
        // Relative path
        let resolved = config_dir.join(extends);
        // If it doesn't have a .json extension, try appending it
        if resolved.extension().is_none() {
            let with_json = resolved.with_extension("json");
            if with_json.exists() {
                return with_json;
            }
        }
        resolved
    } else {
        // Bare package name — try node_modules resolution (best effort)
        let node_modules = config_dir.join("node_modules").join(extends);
        if node_modules.exists() {
            return node_modules;
        }
        let with_json = node_modules.with_extension("json");
        if with_json.exists() {
            return with_json;
        }
        // Fall back to treating it as relative (won't exist but avoids panic)
        config_dir.join(extends)
    }
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

    // Resolve output directories through the extends chain.
    let output_dirs = resolve_output_dirs(tsconfig_path, 0);

    Ok(TsConfig {
        root_dir,
        vue_files,
        ts_files,
        declaration_dir: output_dirs.declaration_dir,
        out_dir: output_dirs.out_dir,
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

    // Strip trailing commas (JSONC feature used by tsconfig.json).
    // Replaces `,` followed only by whitespace then `]` or `}`.
    strip_trailing_commas(&out)
}

/// Remove trailing commas before `]` and `}` to make JSONC valid JSON.
fn strip_trailing_commas(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut in_str = false;
    let mut prev_bs = false;

    while i < len {
        let c = bytes[i];
        if in_str {
            out.push(c as char);
            if c == b'\\' && !prev_bs {
                prev_bs = true;
            } else {
                if c == b'"' && !prev_bs {
                    in_str = false;
                }
                prev_bs = false;
            }
            i += 1;
        } else if c == b'"' {
            in_str = true;
            out.push('"');
            i += 1;
        } else if c == b',' {
            // Check if only whitespace follows before `]` or `}`
            let mut j = i + 1;
            while j < len && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < len && (bytes[j] == b']' || bytes[j] == b'}') {
                // Skip the trailing comma
                i += 1;
            } else {
                out.push(',');
                i += 1;
            }
        } else {
            out.push(c as char);
            i += 1;
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

    #[test]
    fn strip_json_comments_removes_trailing_commas() {
        let input = r#"{
  "compilerOptions": {
    "target": "es2020",
    "module": "esnext",
  },
  "include": ["src/**/*.ts", "src/**/*.vue",],
}"#;
        let out = strip_json_comments(input);
        // Should parse as valid JSON after stripping
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&out);
        assert!(
            parsed.is_ok(),
            "trailing commas should be stripped for valid JSON. Got: {}",
            out
        );
        // Positive: content preserved
        assert!(out.contains("\"target\""));
        assert!(out.contains("\"esnext\""));
        // Negative: no trailing commas before } or ]
        assert!(
            !out.contains(",\n}"),
            "trailing comma before }} should be removed: {out}"
        );
    }

    #[test]
    fn strip_json_comments_preserves_commas_in_strings() {
        let input = r#"{ "a": "hello, world", "b": 1 }"#;
        let out = strip_json_comments(input);
        assert!(
            out.contains("hello, world"),
            "commas inside strings should be preserved: {out}"
        );
    }

    // ── output dir resolution ─────────────────────────────────────

    #[test]
    fn parses_leaf_declaration_dir() {
        let temp = tempfile::TempDir::new().unwrap();
        let tsconfig = temp.path().join("tsconfig.json");
        std::fs::write(
            &tsconfig,
            r#"{
                "compilerOptions": { "declarationDir": "dist/types" },
                "include": ["src"]
            }"#,
        )
        .unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();

        let config = load_tsconfig(&tsconfig).unwrap();
        let expected = strip_unc_prefix(&temp.path().canonicalize().unwrap()).join("dist/types");
        assert_eq!(
            config.declaration_dir.as_deref(),
            Some(expected.as_path()),
            "declarationDir should be resolved to absolute path"
        );
        // Negative: outDir should not be set
        assert!(
            config.out_dir.is_none(),
            "outDir should be None when only declarationDir is set"
        );
    }

    #[test]
    fn parses_leaf_out_dir() {
        let temp = tempfile::TempDir::new().unwrap();
        let tsconfig = temp.path().join("tsconfig.json");
        std::fs::write(
            &tsconfig,
            r#"{
                "compilerOptions": { "outDir": "dist" },
                "include": ["src"]
            }"#,
        )
        .unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();

        let config = load_tsconfig(&tsconfig).unwrap();
        let expected = strip_unc_prefix(&temp.path().canonicalize().unwrap()).join("dist");
        assert_eq!(
            config.out_dir.as_deref(),
            Some(expected.as_path()),
            "outDir should be resolved to absolute path"
        );
        assert!(
            config.declaration_dir.is_none(),
            "declarationDir should be None when only outDir is set"
        );
    }

    #[test]
    fn resolves_relative_paths_against_declaring_tsconfig_dir() {
        let temp = tempfile::TempDir::new().unwrap();
        let sub = temp.path().join("packages").join("app");
        std::fs::create_dir_all(sub.join("src")).unwrap();
        let tsconfig = sub.join("tsconfig.json");
        std::fs::write(
            &tsconfig,
            r#"{
                "compilerOptions": { "declarationDir": "./types" },
                "include": ["src"]
            }"#,
        )
        .unwrap();

        let config = load_tsconfig(&tsconfig).unwrap();
        let expected = strip_unc_prefix(&sub.canonicalize().unwrap()).join("types");
        assert_eq!(
            config.declaration_dir.as_deref(),
            Some(expected.as_path()),
            "declarationDir should resolve relative to tsconfig dir, not cwd"
        );
    }

    #[test]
    fn inherits_declaration_dir_through_extends() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();

        // Base config declares declarationDir
        std::fs::write(
            root.join("tsconfig.base.json"),
            r#"{
                "compilerOptions": { "declarationDir": "dist/types" }
            }"#,
        )
        .unwrap();

        // Child extends base but doesn't override
        std::fs::write(
            root.join("tsconfig.json"),
            r#"{
                "extends": "./tsconfig.base.json",
                "include": ["src"]
            }"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();

        let config = load_tsconfig(&root.join("tsconfig.json")).unwrap();
        // Should inherit from base — resolved relative to base's directory
        let expected = strip_unc_prefix(&root.canonicalize().unwrap()).join("dist/types");
        assert_eq!(
            config.declaration_dir.as_deref(),
            Some(expected.as_path()),
            "declarationDir should be inherited from base tsconfig"
        );
    }

    #[test]
    fn child_overrides_inherited_declaration_dir() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();

        std::fs::write(
            root.join("tsconfig.base.json"),
            r#"{
                "compilerOptions": { "declarationDir": "dist/base-types" }
            }"#,
        )
        .unwrap();

        std::fs::write(
            root.join("tsconfig.json"),
            r#"{
                "extends": "./tsconfig.base.json",
                "compilerOptions": { "declarationDir": "dist/child-types" },
                "include": ["src"]
            }"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();

        let config = load_tsconfig(&root.join("tsconfig.json")).unwrap();
        let expected = strip_unc_prefix(&root.canonicalize().unwrap()).join("dist/child-types");
        assert_eq!(
            config.declaration_dir.as_deref(),
            Some(expected.as_path()),
            "child declarationDir should override inherited value"
        );
        // Negative: should not have the base's path
        let base_path = strip_unc_prefix(&root.canonicalize().unwrap()).join("dist/base-types");
        assert_ne!(
            config.declaration_dir.as_deref(),
            Some(base_path.as_path()),
            "base declarationDir should be overridden by child"
        );
    }

    #[test]
    fn missing_compiler_options_leaves_both_none() {
        let temp = tempfile::TempDir::new().unwrap();
        let tsconfig = temp.path().join("tsconfig.json");
        std::fs::write(&tsconfig, r#"{ "include": ["src"] }"#).unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();

        let config = load_tsconfig(&tsconfig).unwrap();
        assert!(
            config.declaration_dir.is_none(),
            "declarationDir should be None without compilerOptions"
        );
        assert!(
            config.out_dir.is_none(),
            "outDir should be None without compilerOptions"
        );
    }

    #[test]
    fn inherits_out_dir_through_extends() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();

        std::fs::write(
            root.join("tsconfig.base.json"),
            r#"{
                "compilerOptions": { "outDir": "dist" }
            }"#,
        )
        .unwrap();

        std::fs::write(
            root.join("tsconfig.json"),
            r#"{
                "extends": "./tsconfig.base.json",
                "include": ["src"]
            }"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();

        let config = load_tsconfig(&root.join("tsconfig.json")).unwrap();
        let expected = strip_unc_prefix(&root.canonicalize().unwrap()).join("dist");
        assert_eq!(
            config.out_dir.as_deref(),
            Some(expected.as_path()),
            "outDir should be inherited from base tsconfig"
        );
    }
}

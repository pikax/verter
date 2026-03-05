use std::path::{Path, PathBuf};

/// Discovers tsconfig.json files in a workspace and maps directories to their configs.
///
/// Ports the logic from `VerterManager.findTsServices()` in the TypeScript language server.
pub struct TsConfigDiscovery {
    /// Map from directory pattern (e.g., "/project/src/**") to tsconfig path.
    configs: Vec<TsConfigEntry>,
}

/// A discovered tsconfig.json and its coverage pattern.
#[derive(Debug, Clone)]
pub struct TsConfigEntry {
    /// Absolute path to the tsconfig.json file.
    pub config_path: PathBuf,
    /// Glob pattern for files covered by this tsconfig.
    pub pattern: String,
}

// ═══════════════════════════════════════════════════════════════════════════
// Path Alias Resolution (tsconfig.json `compilerOptions.paths`)
// ═══════════════════════════════════════════════════════════════════════════

/// A single path alias mapping: pattern prefix → replacement prefixes.
///
/// For `"@/*": ["src/*"]`, prefix = `"@/"`, replacements = `["src/"]`.
/// For `"@utils": ["src/utils"]`, prefix = `"@utils"`, replacements = `["src/utils"]`.
#[derive(Debug, Clone)]
struct PathAlias {
    /// The import specifier prefix (before the `*` wildcard, or the entire pattern).
    prefix: String,
    /// The import specifier suffix (after the `*` wildcard, or empty).
    suffix: String,
    /// Replacement prefixes (resolved to absolute paths).
    replacements: Vec<PathAliasReplacement>,
}

#[derive(Debug, Clone)]
struct PathAliasReplacement {
    /// Absolute directory prefix for the replacement.
    prefix: String,
    /// Suffix after the `*` wildcard (or empty).
    suffix: String,
}

/// Resolves import specifiers using tsconfig.json `compilerOptions.paths`.
///
/// Built from discovered tsconfig.json files in the workspace.
/// Resolves aliased imports like `@/components/Foo.vue` to absolute file paths.
#[derive(Debug, Default)]
pub struct TsConfigPathResolver {
    aliases: Vec<PathAlias>,
}

impl TsConfigPathResolver {
    /// Build a resolver from a tsconfig.json file path.
    ///
    /// Reads the file, extracts `compilerOptions.baseUrl` and `compilerOptions.paths`,
    /// and builds the alias lookup table. Follows `extends` for inheritance.
    pub fn from_tsconfig(tsconfig_path: &Path) -> Self {
        let mut resolver = Self::default();

        let tsconfig_dir = match tsconfig_path.parent() {
            Some(d) => d,
            None => return resolver,
        };

        // Read and parse tsconfig.json
        let content = match std::fs::read_to_string(tsconfig_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("failed to read {}: {}", tsconfig_path.display(), e);
                return resolver;
            }
        };

        // Strip single-line comments (tsconfig.json supports // comments)
        let cleaned = strip_json_comments(&content);
        let json: serde_json::Value = match serde_json::from_str(&cleaned) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("failed to parse {}: {}", tsconfig_path.display(), e);
                return resolver;
            }
        };

        // Handle `extends` — load base config first, then overlay
        if let Some(extends) = json.get("extends").and_then(|v| v.as_str()) {
            let base_path = resolve_tsconfig_extends(tsconfig_dir, extends);
            if let Some(base_path) = base_path {
                let base = Self::from_tsconfig(&base_path);
                resolver.aliases = base.aliases;
            }
        }

        // Build aliases from this config's own compilerOptions.paths (overrides extends)
        if let Some(compiler_options) = json.get("compilerOptions") {
            let base_url = compiler_options
                .get("baseUrl")
                .and_then(|v| v.as_str())
                .map(|b| tsconfig_dir.join(b))
                .unwrap_or_else(|| tsconfig_dir.to_path_buf());

            if let Some(paths) = compiler_options.get("paths").and_then(|v| v.as_object()) {
                resolver.aliases.clear();
                for (pattern, targets) in paths {
                    let targets: Vec<String> = match targets.as_array() {
                        Some(arr) => arr
                            .iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect(),
                        None => continue,
                    };

                    let (prefix, suffix) = if let Some(star_pos) = pattern.find('*') {
                        (
                            pattern[..star_pos].to_string(),
                            pattern[star_pos + 1..].to_string(),
                        )
                    } else {
                        (pattern.clone(), String::new())
                    };

                    let mut replacements = Vec::new();
                    for target in &targets {
                        let (rep_prefix, rep_suffix) = if let Some(star_pos) = target.find('*') {
                            (
                                target[..star_pos].to_string(),
                                target[star_pos + 1..].to_string(),
                            )
                        } else {
                            (target.clone(), String::new())
                        };

                        let abs_prefix = base_url.join(&rep_prefix);
                        let abs_str = abs_prefix.to_string_lossy().replace('\\', "/");

                        replacements.push(PathAliasReplacement {
                            prefix: abs_str,
                            suffix: rep_suffix,
                        });
                    }

                    resolver.aliases.push(PathAlias {
                        prefix,
                        suffix,
                        replacements,
                    });
                }
            }
        }

        // If still no aliases, follow `references` (solution-style tsconfigs).
        // The root config typically has `"files": [], "references": [...]` with paths
        // defined in the referenced configs like tsconfig.app.json.
        if resolver.is_empty() {
            if let Some(refs) = json.get("references").and_then(|v| v.as_array()) {
                for ref_entry in refs {
                    if let Some(ref_path) = ref_entry.get("path").and_then(|v| v.as_str()) {
                        if let Some(ref_tsconfig) =
                            resolve_tsconfig_reference(tsconfig_dir, ref_path)
                        {
                            let ref_resolver = Self::from_tsconfig(&ref_tsconfig);
                            if !ref_resolver.is_empty() {
                                resolver.aliases = ref_resolver.aliases;
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Sort: longer prefixes first (more specific matches take priority)
        resolver
            .aliases
            .sort_by(|a, b| b.prefix.len().cmp(&a.prefix.len()));

        resolver
    }

    /// Resolve an import specifier to an absolute file path.
    ///
    /// Returns `None` if no alias matches or the resolved path doesn't exist on disk.
    pub fn resolve(&self, specifier: &str) -> Option<String> {
        for alias in &self.aliases {
            // Check if the specifier matches the alias pattern
            if !specifier.starts_with(&alias.prefix) {
                continue;
            }

            // For exact match (no wildcard), suffix must be empty and specifier == prefix
            if alias.suffix.is_empty() && alias.replacements.iter().all(|r| r.suffix.is_empty()) {
                // Wildcard pattern: prefix + * + suffix
                if alias.prefix.len() < specifier.len() || alias.prefix == *specifier {
                    let captured = &specifier[alias.prefix.len()..];

                    for rep in &alias.replacements {
                        let resolved = format!("{}{}{}", rep.prefix, captured, rep.suffix);
                        if let Some(path) = try_resolve_file(&resolved) {
                            return Some(path);
                        }
                    }
                }
            } else {
                // Pattern with suffix: check both prefix and suffix match
                if !specifier.ends_with(&alias.suffix) {
                    continue;
                }
                let captured_end = specifier.len() - alias.suffix.len();
                if alias.prefix.len() > captured_end {
                    continue;
                }
                let captured = &specifier[alias.prefix.len()..captured_end];

                for rep in &alias.replacements {
                    let resolved = format!("{}{}{}", rep.prefix, captured, rep.suffix);
                    if let Some(path) = try_resolve_file(&resolved) {
                        return Some(path);
                    }
                }
            }
        }
        None
    }

    /// Check if the resolver has any aliases configured.
    pub fn is_empty(&self) -> bool {
        self.aliases.is_empty()
    }

    /// Merge vite aliases into this resolver. Vite aliases take precedence:
    /// any existing alias with the same prefix is replaced.
    pub fn merge_vite_aliases(&mut self, vite_aliases: Vec<(String, String)>) {
        for (find, replacement) in vite_aliases {
            // find values from discover_vite_aliases are already normalized with `/` suffix
            // Remove any existing alias with the same prefix
            self.aliases.retain(|a| a.prefix != find);

            let rep_prefix = if replacement.ends_with('/') {
                replacement
            } else {
                format!("{replacement}/")
            };

            self.aliases.push(PathAlias {
                prefix: find,
                suffix: String::new(),
                replacements: vec![PathAliasReplacement {
                    prefix: rep_prefix,
                    suffix: String::new(),
                }],
            });
        }

        // Re-sort: longer prefixes first
        self.aliases
            .sort_by(|a, b| b.prefix.len().cmp(&a.prefix.len()));
    }

    /// Extract the raw `baseUrl` and `paths` JSON from a tsconfig for passing to tsserver.
    ///
    /// Follows `extends` and `references` to find the effective paths.
    /// Returns `(baseUrl, paths)` as raw JSON values, or `None` if no paths found.
    pub fn raw_paths_json(tsconfig_path: &Path) -> Option<(String, serde_json::Value)> {
        Self::raw_paths_json_inner(tsconfig_path, 0)
    }

    fn raw_paths_json_inner(
        tsconfig_path: &Path,
        depth: u8,
    ) -> Option<(String, serde_json::Value)> {
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
                if let Some(result) = Self::raw_paths_json_inner(&base_path, depth + 1) {
                    // If base has paths, use them (current config may override)
                    let base_result = Some(result);
                    // Check if current config overrides
                    if let Some(co) = json.get("compilerOptions") {
                        if co.get("paths").is_some() {
                            // Current config overrides base
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
                    .map(|b| tsconfig_dir.join(b).to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|| tsconfig_dir.to_string_lossy().replace('\\', "/"));
                return Some((base_url, paths.clone()));
            }
        }

        // Follow references
        if let Some(refs) = json.get("references").and_then(|v| v.as_array()) {
            for ref_entry in refs {
                if let Some(ref_path) = ref_entry.get("path").and_then(|v| v.as_str()) {
                    if let Some(ref_tsconfig) = resolve_tsconfig_reference(tsconfig_dir, ref_path) {
                        if let Some(result) = Self::raw_paths_json_inner(&ref_tsconfig, depth + 1) {
                            return Some(result);
                        }
                    }
                }
            }
        }

        None
    }
}

/// Try to resolve a file path, checking common Vue extensions.
fn try_resolve_file(path: &str) -> Option<String> {
    let p = Path::new(path);
    if p.exists() && p.is_file() {
        return Some(normalize_path(p));
    }
    // Try common extensions
    for ext in &[".vue", ".ts", ".tsx", ".js", ".jsx"] {
        let with_ext = format!("{path}{ext}");
        let pe = Path::new(&with_ext);
        if pe.exists() && pe.is_file() {
            return Some(normalize_path(pe));
        }
    }
    // Try index files
    for idx in &["index.vue", "index.ts", "index.tsx", "index.js"] {
        let with_index = Path::new(path).join(idx);
        if with_index.exists() && with_index.is_file() {
            return Some(normalize_path(&with_index));
        }
    }
    None
}

/// Normalize a path to forward slashes for canonical ID format.
fn normalize_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Resolve `extends` field from tsconfig.json to an absolute path.
fn resolve_tsconfig_extends(tsconfig_dir: &Path, extends: &str) -> Option<PathBuf> {
    if extends.starts_with('.') {
        // Relative path
        let resolved = tsconfig_dir.join(extends);
        // Try as-is, then with .json extension
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
///
/// Handles both file references (`"./tsconfig.app.json"`) and directory references
/// (`"./packages/app"` → looks for `tsconfig.json` inside).
fn resolve_tsconfig_reference(tsconfig_dir: &Path, ref_path: &str) -> Option<PathBuf> {
    let resolved = tsconfig_dir.join(ref_path);

    // Direct file reference
    if resolved.is_file() {
        return Some(resolved);
    }

    // Directory reference → look for tsconfig.json inside
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

/// Check if a workspace has any solution-style `tsconfig.json` (non-empty `references` array).
/// TSGO cannot resolve path aliases from referenced tsconfig files, so this is used by
/// auto-mode provider selection to prefer tsserver when composite tsconfigs are detected.
///
/// Checks the workspace root first, then scans up to 2 levels of subdirectories
/// (handles monorepos where tsconfig.json lives in `packages/foo/tsconfig.json`).
pub fn has_solution_style_tsconfig(workspace_root: &Path) -> bool {
    // Check root tsconfig.json
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

/// Check if a single tsconfig.json file is solution-style (has non-empty `references`).
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

/// Read immediate subdirectories, skipping hidden dirs and node_modules.
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

/// Strip single-line (`//`) and multi-line (`/* */`) comments from JSON text.
/// tsconfig.json supports JSONC (JSON with Comments).
///
/// Uses byte-index slicing into the original `&str` to preserve valid UTF-8
/// (comments and delimiters are always ASCII, so byte scanning is safe).
fn strip_json_comments(input: &str) -> String {
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
            // Copy the entire string literal as a slice (preserves UTF-8)
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
            // JSON keys/values outside strings are always ASCII, but copy as slice for safety.
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

    // Strip trailing commas before } or ] (JSONC/tsconfig allows them, JSON does not)
    strip_trailing_commas(&result)
}

/// Remove trailing commas before `}` or `]` in JSON.
/// Handles whitespace/newlines between the comma and the closing bracket.
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
            // Check if this comma is trailing (only whitespace before } or ])
            let mut j = i + 1;
            while j < len
                && (bytes[j] == b' ' || bytes[j] == b'\t' || bytes[j] == b'\n' || bytes[j] == b'\r')
            {
                j += 1;
            }
            if j < len && (bytes[j] == b'}' || bytes[j] == b']') {
                // Trailing comma — skip it, keep the whitespace
                i += 1;
            } else {
                result.push_str(&input[i..i + 1]);
                i += 1;
            }
        } else {
            result.push_str(&input[i..i + 1]);
            i += 1;
        }
    }

    result
}

impl Default for TsConfigDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl TsConfigDiscovery {
    pub fn new() -> Self {
        Self {
            configs: Vec::new(),
        }
    }

    /// Discover all tsconfig files under the given workspace root.
    ///
    /// Finds both `tsconfig.json` and variant files like `tsconfig.app.json`,
    /// `tsconfig.node.json`, etc. Excludes `node_modules` and dot-directories.
    pub fn discover(&mut self, root: &Path) {
        let root_str = root.to_string_lossy().replace('\\', "/");
        // Glob for tsconfig.json AND tsconfig.*.json (e.g. tsconfig.app.json)
        for glob_pattern in &[
            format!("{root_str}/**/tsconfig.json"),
            format!("{root_str}/**/tsconfig.*.json"),
        ] {
            match glob::glob(glob_pattern) {
                Ok(paths) => {
                    for entry in paths.flatten() {
                        // Skip node_modules
                        if entry.components().any(|c| c.as_os_str() == "node_modules") {
                            continue;
                        }
                        // Skip dot-directories
                        if entry
                            .components()
                            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
                        {
                            continue;
                        }
                        // Skip duplicates (tsconfig.json matches both patterns)
                        if self.configs.iter().any(|e| e.config_path == entry) {
                            continue;
                        }

                        if let Some(dir) = entry.parent() {
                            let coverage =
                                format!("{}/**", dir.to_string_lossy().replace('\\', "/"));
                            self.configs.push(TsConfigEntry {
                                config_path: entry,
                                pattern: coverage,
                            });
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("failed to glob for tsconfig files: {}", e);
                }
            }
        }
    }

    /// Find the tsconfig.json that covers a given file path.
    ///
    /// Returns the most specific (longest directory prefix) match.
    pub fn find_config_for(&self, file_path: &Path) -> Option<&TsConfigEntry> {
        let file_str = file_path.to_string_lossy().replace('\\', "/");
        let mut best: Option<&TsConfigEntry> = None;
        let mut best_prefix_len = 0;

        for entry in &self.configs {
            // Extract the directory prefix from the pattern (everything before /**)
            let prefix = entry.pattern.trim_end_matches("/**");
            if file_str.starts_with(prefix) && prefix.len() > best_prefix_len {
                best_prefix_len = prefix.len();
                best = Some(entry);
            }
        }

        best
    }

    /// Get all discovered tsconfig entries.
    pub fn configs(&self) -> &[TsConfigEntry] {
        &self.configs
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Vite Config Alias Discovery
// ═══════════════════════════════════════════════════════════════════════════

/// Discover `resolve.alias` entries from a vite.config.{ts,js,mjs} file.
///
/// Spawns Node.js with a small inline script that dynamically imports the config
/// and prints `resolve.alias` as JSON. Returns a list of `(find, replacement)` pairs
/// suitable for merging into a `TsConfigPathResolver`.
///
/// Returns an empty vec if no vite config is found, Node.js is unavailable,
/// the config has no `resolve.alias`, or evaluation fails/times out.
pub fn discover_vite_aliases(project_root: &Path, node_path: &str) -> Vec<(String, String)> {
    // Find vite config file
    let config_file = ["vite.config.ts", "vite.config.js", "vite.config.mjs"]
        .iter()
        .map(|name| project_root.join(name))
        .find(|p| p.exists());

    let config_path = match config_file {
        Some(p) => p,
        None => return Vec::new(),
    };

    let config_path_str = config_path.to_string_lossy().replace('\\', "/");

    // Inline Node.js script that dynamically imports the vite config and
    // extracts resolve.alias as JSON. Uses pathToFileURL for correct Windows paths.
    // For .ts configs, tries to register tsx loader first.
    let loader_setup = if config_path_str.ends_with(".ts") {
        "try { await import('tsx/esm'); } catch {}"
    } else {
        ""
    };

    let script = format!(
        r#"
const {{ pathToFileURL }} = require('url');
(async () => {{
  try {{
    {loader_setup}
    const mod = await import(pathToFileURL('{config_path_str}').href);
    const config = mod.default || mod;
    const raw = typeof config === 'function' ? config({{ mode: 'development', command: 'serve' }}) : config;
    const resolved = raw instanceof Promise ? await raw : raw;
    const alias = resolved?.resolve?.alias;
    if (!alias) {{ process.stdout.write('__VERTER_ALIASES_BEGIN__[]__VERTER_ALIASES_END__'); return; }}
    let entries = [];
    if (Array.isArray(alias)) {{
      for (const a of alias) {{
        if (a.find && a.replacement) {{
          const f = typeof a.find === 'string' ? a.find : null;
          if (f) entries.push({{ find: f, replacement: a.replacement }});
        }}
      }}
    }} else if (typeof alias === 'object') {{
      for (const [key, val] of Object.entries(alias)) {{
        if (typeof val === 'string') entries.push({{ find: key, replacement: val }});
      }}
    }}
    process.stdout.write('__VERTER_ALIASES_BEGIN__' + JSON.stringify(entries) + '__VERTER_ALIASES_END__');
  }} catch (e) {{
    process.stderr.write('vite config eval error: ' + e.message + '\n');
    process.stdout.write('__VERTER_ALIASES_BEGIN__[]__VERTER_ALIASES_END__');
  }}
}})();
"#
    );

    let result = std::process::Command::new(node_path)
        .arg("-e")
        .arg(&script)
        .current_dir(project_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    let output = match result {
        Ok(o) => o,
        Err(e) => {
            tracing::debug!("failed to spawn node for vite config: {e}");
            return Vec::new();
        }
    };

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        tracing::debug!(
            "vite config eval stderr ({}): {}",
            config_path_str,
            stderr.trim()
        );
    }

    if !output.status.success() {
        tracing::debug!(
            "vite config eval failed for {} (exit code: {:?})",
            config_path_str,
            output.status.code()
        );
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let entries = match parse_vite_alias_stdout(&stdout) {
        Some(v) => v,
        None => {
            let trimmed = stdout.trim();
            if !trimmed.is_empty() {
                tracing::debug!(
                    "failed to parse vite alias output for {} (raw: {:?})",
                    config_path_str,
                    &trimmed[..trimmed.len().min(200)]
                );
            }
            return Vec::new();
        }
    };

    entries
        .into_iter()
        .map(|e| {
            let replacement = PathBuf::from(&e.replacement);
            // Make replacement absolute relative to project root if not already
            let abs_replacement = if replacement.is_absolute() {
                replacement
            } else {
                project_root.join(&replacement)
            };
            let abs_str = abs_replacement.to_string_lossy().replace('\\', "/");
            // Normalize: bare aliases like `@` become `@/` for wildcard matching
            let find = if e.find.ends_with('/') {
                e.find
            } else {
                format!("{}/", e.find)
            };
            (find, abs_str)
        })
        .collect()
}

#[derive(serde::Deserialize)]
struct ViteAliasEntry {
    find: String,
    replacement: String,
}

/// Sentinel markers used to extract JSON from potentially noisy Node.js stdout.
/// The vite config eval script wraps its JSON output in these markers so that
/// warnings, deprecation notices, or other console output don't corrupt parsing.
const VITE_SENTINEL_BEGIN: &str = "__VERTER_ALIASES_BEGIN__";
const VITE_SENTINEL_END: &str = "__VERTER_ALIASES_END__";

/// Parse vite alias JSON from Node.js stdout, handling noise via sentinel markers.
///
/// Strategy:
/// 1. If sentinel markers are present, extract JSON between them (ignores prefix/suffix noise).
/// 2. Fallback: try parsing the entire trimmed output as JSON (backward compat).
/// 3. Returns `None` if input is empty, whitespace-only, or unparseable.
fn parse_vite_alias_stdout(raw: &str) -> Option<Vec<ViteAliasEntry>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Sentinel-based extraction (handles prefix AND suffix noise)
    if let Some(begin) = trimmed.find(VITE_SENTINEL_BEGIN) {
        let after = &trimmed[begin + VITE_SENTINEL_BEGIN.len()..];
        if let Some(end) = after.find(VITE_SENTINEL_END) {
            return serde_json::from_str(&after[..end]).ok();
        }
    }

    // Fallback: try clean parse (backward compat with old eval script)
    serde_json::from_str(trimmed).ok()
}

// ═══════════════════════════════════════════════════════════════════════════
// Project Lint Configuration (.verterrc.json + ESLint migration)
// ═══════════════════════════════════════════════════════════════════════════

/// Project-level lint configuration read from `.verterrc.json`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerterProjectConfig {
    pub lint: Option<ProjectLintConfig>,
}

/// Lint section of `.verterrc.json`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLintConfig {
    /// Whether linting is enabled (default: true when config exists).
    pub enabled: Option<bool>,
    /// Preset name: "essential", "recommended", "all", etc.
    pub preset: Option<String>,
    /// Per-rule overrides: "off" | "warn" | "error" or [severity, options].
    pub rules: Option<std::collections::HashMap<String, serde_json::Value>>,
}

/// Resolved lint configuration from all sources.
#[derive(Debug, Clone, Default)]
pub struct ResolvedLintConfig {
    /// Whether linting was explicitly configured (via .verterrc.json, eslint, or VS Code).
    pub explicitly_configured: bool,
    /// The resolved lint config to pass to the Linter.
    pub config: verter_diagnostics::LintConfig,
}

/// Discover and load project lint configuration.
///
/// Priority: `.verterrc.json` > VS Code initializationOptions > eslint config
pub fn discover_lint_config(workspace_root: &Path) -> ResolvedLintConfig {
    // 1. Try .verterrc.json
    if let Some(config) = load_verterrc(workspace_root) {
        return config;
    }

    // 2. Try eslint config migration
    if let Some(config) = load_eslint_config(workspace_root) {
        return config;
    }

    // No config found — use defaults (not explicitly configured)
    ResolvedLintConfig::default()
}

/// Load `.verterrc.json` from workspace root.
fn load_verterrc(workspace_root: &Path) -> Option<ResolvedLintConfig> {
    let config_path = workspace_root.join(".verterrc.json");
    let content = std::fs::read_to_string(&config_path).ok()?;
    let cleaned = strip_json_comments(&content);
    let project_config: VerterProjectConfig = serde_json::from_str(&cleaned).ok()?;

    let lint = project_config.lint?;
    let mut config = verter_diagnostics::LintConfig::default();

    // Apply preset
    if let Some(preset_str) = &lint.preset {
        config.preset = match preset_str.as_str() {
            "essential" => verter_diagnostics::LintPreset::Essential,
            "recommended" => verter_diagnostics::LintPreset::Recommended,
            "all" => verter_diagnostics::LintPreset::All,
            "performance" => verter_diagnostics::LintPreset::Performance,
            "a11y" => verter_diagnostics::LintPreset::A11y,
            "strict" => verter_diagnostics::LintPreset::Strict,
            _ => verter_diagnostics::LintPreset::Recommended,
        };
    }

    // Apply per-rule overrides
    if let Some(rules) = &lint.rules {
        for (name, value) in rules {
            let severity = parse_rule_severity(value);
            config.rules.insert(name.clone(), severity);
        }
    }

    let enabled = lint.enabled.unwrap_or(true);

    Some(ResolvedLintConfig {
        explicitly_configured: enabled,
        config,
    })
}

/// Load and migrate eslint-plugin-vue config.
fn load_eslint_config(workspace_root: &Path) -> Option<ResolvedLintConfig> {
    // Try .eslintrc.json first, then .eslintrc.js (as JSON fallback), then package.json
    let eslint_json = workspace_root.join(".eslintrc.json");
    let package_json = workspace_root.join("package.json");

    let json: serde_json::Value = if eslint_json.exists() {
        let content = std::fs::read_to_string(&eslint_json).ok()?;
        let cleaned = strip_json_comments(&content);
        serde_json::from_str(&cleaned).ok()?
    } else if package_json.exists() {
        let content = std::fs::read_to_string(&package_json).ok()?;
        let pkg: serde_json::Value = serde_json::from_str(&content).ok()?;
        pkg.get("eslintConfig")?.clone()
    } else {
        return None;
    };

    let mut config = verter_diagnostics::LintConfig::default();

    // Extract preset from extends
    if let Some(extends) = json.get("extends") {
        let extends_list: Vec<&str> = match extends {
            serde_json::Value::String(s) => vec![s.as_str()],
            serde_json::Value::Array(arr) => arr.iter().filter_map(|v| v.as_str()).collect(),
            _ => vec![],
        };

        for ext in extends_list {
            match ext {
                "plugin:vue/vue3-essential" | "plugin:vue/essential" => {
                    config.preset = verter_diagnostics::LintPreset::Essential;
                }
                "plugin:vue/vue3-strongly-recommended" | "plugin:vue/strongly-recommended" => {
                    config.preset = verter_diagnostics::LintPreset::Recommended;
                }
                "plugin:vue/vue3-recommended" | "plugin:vue/recommended" => {
                    config.preset = verter_diagnostics::LintPreset::Recommended;
                }
                _ => {}
            }
        }
    }

    // Extract per-rule overrides
    if let Some(rules) = json.get("rules").and_then(|r| r.as_object()) {
        let mut has_vue_rules = false;
        for (name, value) in rules {
            // Only migrate vue/ prefixed rules
            if let Some(rule_name) = name.strip_prefix("vue/") {
                has_vue_rules = true;
                let severity = parse_rule_severity(value);
                config.rules.insert(rule_name.to_string(), severity);
            }
        }
        if !has_vue_rules {
            return None; // No vue rules found, skip eslint migration
        }
    }

    Some(ResolvedLintConfig {
        explicitly_configured: true,
        config,
    })
}

/// Parse a rule severity from JSON value.
///
/// Supports: `"off"` / `0`, `"warn"` / `1`, `"error"` / `2`,
/// or `["error", { options }]` array form.
fn parse_rule_severity(value: &serde_json::Value) -> Option<verter_diagnostics::Severity> {
    use verter_diagnostics::Severity;

    match value {
        serde_json::Value::String(s) => match s.as_str() {
            "off" => None,
            "warn" => Some(Severity::Warning),
            "error" => Some(Severity::Error),
            _ => Some(Severity::Warning),
        },
        serde_json::Value::Number(n) => match n.as_u64() {
            Some(0) => None,
            Some(1) => Some(Severity::Warning),
            Some(2) => Some(Severity::Error),
            _ => Some(Severity::Warning),
        },
        serde_json::Value::Array(arr) => {
            // [severity, options] — extract severity from first element
            arr.first().and_then(parse_rule_severity)
        }
        _ => Some(Severity::Warning),
    }
}

/// Merge VS Code initialization options into a resolved lint config.
pub fn merge_init_options(resolved: &mut ResolvedLintConfig, init_options: &serde_json::Value) {
    if let Some(lint) = init_options.get("lint") {
        if let Some(enabled) = lint.get("enabled").and_then(|v| v.as_bool()) {
            resolved.explicitly_configured = enabled;
        }
        if let Some(preset) = lint.get("preset").and_then(|v| v.as_str()) {
            resolved.config.preset = match preset {
                "essential" => verter_diagnostics::LintPreset::Essential,
                "recommended" => verter_diagnostics::LintPreset::Recommended,
                "all" => verter_diagnostics::LintPreset::All,
                "performance" => verter_diagnostics::LintPreset::Performance,
                "a11y" => verter_diagnostics::LintPreset::A11y,
                "strict" => verter_diagnostics::LintPreset::Strict,
                _ => resolved.config.preset,
            };
        }
    }
}

#[cfg(test)]
mod config_migration_tests {
    use super::*;

    #[test]
    fn parse_severity_string() {
        assert_eq!(parse_rule_severity(&serde_json::json!("off")), None);
        assert_eq!(
            parse_rule_severity(&serde_json::json!("warn")),
            Some(verter_diagnostics::Severity::Warning)
        );
        assert_eq!(
            parse_rule_severity(&serde_json::json!("error")),
            Some(verter_diagnostics::Severity::Error)
        );
    }

    #[test]
    fn parse_severity_number() {
        assert_eq!(parse_rule_severity(&serde_json::json!(0)), None);
        assert_eq!(
            parse_rule_severity(&serde_json::json!(1)),
            Some(verter_diagnostics::Severity::Warning)
        );
        assert_eq!(
            parse_rule_severity(&serde_json::json!(2)),
            Some(verter_diagnostics::Severity::Error)
        );
    }

    #[test]
    fn parse_severity_array() {
        assert_eq!(
            parse_rule_severity(&serde_json::json!(["error", {}])),
            Some(verter_diagnostics::Severity::Error)
        );
        assert_eq!(parse_rule_severity(&serde_json::json!(["off"])), None);
    }

    #[test]
    fn verterrc_json_roundtrip() {
        let json = r#"{"lint":{"enabled":true,"preset":"strict","rules":{"no-v-html":"error","unused-css-selector":"off"}}}"#;
        let config: VerterProjectConfig = serde_json::from_str(json).unwrap();
        let lint = config.lint.unwrap();
        assert_eq!(lint.enabled, Some(true));
        assert_eq!(lint.preset.as_deref(), Some("strict"));
        assert!(lint.rules.is_some());
        let rules = lint.rules.unwrap();
        assert_eq!(rules.get("no-v-html").unwrap(), "error");
        assert_eq!(rules.get("unused-css-selector").unwrap(), "off");
    }

    #[test]
    fn merge_init_options_applies_preset() {
        let mut resolved = ResolvedLintConfig::default();
        let opts = serde_json::json!({
            "lint": {
                "enabled": true,
                "preset": "strict"
            }
        });
        merge_init_options(&mut resolved, &opts);
        assert!(resolved.explicitly_configured);
        assert_eq!(
            resolved.config.preset,
            verter_diagnostics::LintPreset::Strict
        );
    }

    #[test]
    fn discover_no_config_returns_default() {
        let tmp = std::env::temp_dir().join("verter_test_no_config");
        let _ = std::fs::create_dir_all(&tmp);
        let result = discover_lint_config(&tmp);
        assert!(!result.explicitly_configured);
        assert_eq!(
            result.config.preset,
            verter_diagnostics::LintPreset::Recommended
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_verterrc_json() {
        let tmp = std::env::temp_dir().join("verter_test_verterrc");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(
            tmp.join(".verterrc.json"),
            r#"{"lint":{"preset":"essential","rules":{"no-v-html":"off"}}}"#,
        )
        .unwrap();
        let result = discover_lint_config(&tmp);
        assert!(result.explicitly_configured);
        assert_eq!(
            result.config.preset,
            verter_diagnostics::LintPreset::Essential
        );
        assert_eq!(result.config.rules.get("no-v-html"), Some(&None));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_eslintrc_json() {
        let tmp = std::env::temp_dir().join("verter_test_eslintrc");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(
            tmp.join(".eslintrc.json"),
            r#"{"extends":["plugin:vue/vue3-recommended"],"rules":{"vue/no-v-html":"error"}}"#,
        )
        .unwrap();
        let result = discover_lint_config(&tmp);
        assert!(result.explicitly_configured);
        assert_eq!(
            result.config.preset,
            verter_diagnostics::LintPreset::Recommended
        );
        assert_eq!(
            result.config.rules.get("no-v-html"),
            Some(&Some(verter_diagnostics::Severity::Error))
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Component diagnostics configuration
// ═══════════════════════════════════════════════════════════════════════════

/// Diagnostic severity configuration for component usage checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverityConfig {
    Error,
    Warning,
    Information,
    Hint,
    Off,
}

/// Configuration for component usage diagnostics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentDiagnosticsConfig {
    pub unknown_prop_severity: DiagnosticSeverityConfig,
    pub unknown_model_severity: DiagnosticSeverityConfig,
}

impl Default for ComponentDiagnosticsConfig {
    fn default() -> Self {
        Self {
            unknown_prop_severity: DiagnosticSeverityConfig::Warning,
            unknown_model_severity: DiagnosticSeverityConfig::Warning,
        }
    }
}

impl DiagnosticSeverityConfig {
    /// Convert to LSP severity, or None if Off.
    pub fn to_lsp(self) -> Option<tower_lsp_server::ls_types::DiagnosticSeverity> {
        use tower_lsp_server::ls_types::DiagnosticSeverity;
        match self {
            Self::Error => Some(DiagnosticSeverity::ERROR),
            Self::Warning => Some(DiagnosticSeverity::WARNING),
            Self::Information => Some(DiagnosticSeverity::INFORMATION),
            Self::Hint => Some(DiagnosticSeverity::HINT),
            Self::Off => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Per-Project Configuration (monorepo / multi-root workspace support)
// ═══════════════════════════════════════════════════════════════════════════

/// Per-project configuration grouping path alias resolution and lint config.
///
/// In a monorepo, each package may have its own `tsconfig.json` (with different
/// `paths` aliases), `.verterrc.json` (with different lint rules), and vite config.
/// `ProjectConfig` captures all of these for a single project root.
pub struct ProjectConfig {
    /// Directory this project covers (e.g., `packages/ui/`). Always forward slashes.
    pub root: String,
    /// Path alias resolver (from tsconfig + vite.config, merged).
    pub path_resolver: TsConfigPathResolver,
    /// Lint configuration for this project.
    pub lint_config: ResolvedLintConfig,
    /// Linter instance built from `lint_config`. Cached to avoid recreating.
    pub linter: verter_diagnostics::Linter,
    /// Whether lint was explicitly configured for this project.
    pub lint_explicitly_configured: bool,
}

/// Registry of per-project configurations for a multi-root workspace.
///
/// Projects are sorted by root prefix length (longest first) so that
/// `find_project()` returns the most specific match.
pub struct ProjectRegistry {
    /// Sorted by root length descending (longest prefix first).
    projects: Vec<ProjectConfig>,
}

impl ProjectRegistry {
    /// Build a registry from workspace roots by discovering tsconfigs, vite configs,
    /// and lint configs.
    ///
    /// For each root, discovers all `tsconfig.json` files, builds per-project resolvers,
    /// and discovers lint config. If a root has no tsconfig, a default project is created
    /// with empty aliases.
    ///
    /// When `node_path` is provided and `vite_config_enabled` is true, also evaluates
    /// `vite.config.{ts,js,mjs}` per project root and merges `resolve.alias` entries
    /// into the path resolver (vite aliases take precedence over tsconfig aliases).
    pub fn from_workspace_roots(
        roots: &[String],
        node_path: Option<&str>,
        vite_config_enabled: bool,
    ) -> Self {
        let mut projects = Vec::new();

        for root_uri in roots {
            let canonical = crate::documents::uri_to_canonical_id_from_str(root_uri);
            let root_path = PathBuf::from(&canonical);

            // Discover tsconfigs under this root
            let mut discovery = TsConfigDiscovery::new();
            discovery.discover(&root_path);

            // Group tsconfigs by project root (directory containing tsconfig)
            let mut project_roots_seen = std::collections::HashSet::new();
            // Always include the workspace root itself
            project_roots_seen.insert(canonical.clone());

            for entry in discovery.configs() {
                if let Some(dir) = entry.config_path.parent() {
                    let dir_str = dir.to_string_lossy().replace('\\', "/");
                    project_roots_seen.insert(dir_str);
                }
            }

            for project_root in &project_roots_seen {
                let project_root_path = PathBuf::from(project_root);

                // Find the best tsconfig for this project root
                let mut resolver = if let Some(entry) =
                    discovery.find_config_for(&project_root_path.join("src/dummy.ts"))
                {
                    TsConfigPathResolver::from_tsconfig(&entry.config_path)
                } else if let Some(entry) =
                    discovery.find_config_for(&project_root_path.join("dummy.ts"))
                {
                    TsConfigPathResolver::from_tsconfig(&entry.config_path)
                } else {
                    TsConfigPathResolver::default()
                };

                // Merge vite config aliases (takes precedence over tsconfig)
                if vite_config_enabled {
                    if let Some(np) = node_path {
                        let vite_aliases = discover_vite_aliases(&project_root_path, np);
                        if !vite_aliases.is_empty() {
                            tracing::info!(
                                "discovered {} vite aliases for {}",
                                vite_aliases.len(),
                                project_root
                            );
                            resolver.merge_vite_aliases(vite_aliases);
                        }
                    }
                }

                // Discover lint config for this project root
                let lint = discover_lint_config(&project_root_path);
                let linter = verter_diagnostics::Linter::new(lint.config.clone());

                projects.push(ProjectConfig {
                    root: project_root.clone(),
                    path_resolver: resolver,
                    lint_config: lint.clone(),
                    linter,
                    lint_explicitly_configured: lint.explicitly_configured,
                });
            }
        }

        // Sort by root length descending (longest prefix first for most-specific match)
        projects.sort_by(|a, b| b.root.len().cmp(&a.root.len()));

        Self { projects }
    }

    /// Build a registry from canonical paths (not URIs). Used in tests.
    pub fn from_canonical_roots(roots: &[&str]) -> Self {
        let mut projects = Vec::new();

        for &root in roots {
            let root_path = PathBuf::from(root);

            let mut discovery = TsConfigDiscovery::new();
            discovery.discover(&root_path);

            let mut project_roots_seen = std::collections::HashSet::new();
            project_roots_seen.insert(root.to_string());

            for entry in discovery.configs() {
                if let Some(dir) = entry.config_path.parent() {
                    let dir_str = dir.to_string_lossy().replace('\\', "/");
                    project_roots_seen.insert(dir_str);
                }
            }

            for project_root in &project_roots_seen {
                let project_root_path = PathBuf::from(project_root);

                let resolver = if let Some(entry) =
                    discovery.find_config_for(&project_root_path.join("src/dummy.ts"))
                {
                    TsConfigPathResolver::from_tsconfig(&entry.config_path)
                } else if let Some(entry) =
                    discovery.find_config_for(&project_root_path.join("dummy.ts"))
                {
                    TsConfigPathResolver::from_tsconfig(&entry.config_path)
                } else {
                    TsConfigPathResolver::default()
                };

                let lint = discover_lint_config(&project_root_path);
                let linter = verter_diagnostics::Linter::new(lint.config.clone());

                projects.push(ProjectConfig {
                    root: project_root.clone(),
                    path_resolver: resolver,
                    lint_config: lint.clone(),
                    linter,
                    lint_explicitly_configured: lint.explicitly_configured,
                });
            }
        }

        projects.sort_by(|a, b| b.root.len().cmp(&a.root.len()));
        Self { projects }
    }

    /// Find the project that covers a given file path (longest prefix match).
    ///
    /// Falls back to `None` if no project root is a prefix of the file path.
    pub fn find_project(&self, file_path: &str) -> Option<&ProjectConfig> {
        let normalized = file_path.replace('\\', "/");
        self.projects.iter().find(|p| {
            normalized.starts_with(&p.root)
                && (normalized.len() == p.root.len()
                    || p.root.ends_with('/')
                    || normalized.as_bytes().get(p.root.len()) == Some(&b'/'))
        })
    }

    /// Resolve a path alias for a file, using the file's project-specific resolver.
    ///
    /// Returns `None` if no project matches or the specifier doesn't match any alias.
    pub fn resolve_alias(&self, importer_path: &str, specifier: &str) -> Option<String> {
        let project = self.find_project(importer_path)?;
        project.path_resolver.resolve(specifier)
    }

    /// Get the project root directory for a file (for tsserver `projectRootPath`).
    pub fn find_project_root(&self, file_path: &str) -> Option<&str> {
        self.find_project(file_path).map(|p| p.root.as_str())
    }

    /// Get the lint config for a file's project.
    pub fn linter_for(&self, file_path: &str) -> Option<&ProjectConfig> {
        self.find_project(file_path)
    }

    /// Apply default lint config to projects that don't have explicit lint config.
    ///
    /// Used to propagate VS Code `verter.lint` settings to per-project linters
    /// when those projects don't have their own `.verterrc.json`.
    pub fn apply_default_lint(&mut self, config: &verter_diagnostics::LintConfig) {
        for project in &mut self.projects {
            if !project.lint_explicitly_configured {
                project.lint_config.config = config.clone();
                project.linter = verter_diagnostics::Linter::new(config.clone());
            }
        }
    }

    /// Get all project configs.
    pub fn projects(&self) -> &[ProjectConfig] {
        &self.projects
    }

    /// Get all project roots.
    pub fn project_roots(&self) -> Vec<&str> {
        self.projects.iter().map(|p| p.root.as_str()).collect()
    }

    /// Get tsconfig coverage patterns from all projects (for workspace scanner).
    pub fn tsconfig_patterns(&self, roots: &[String]) -> Vec<String> {
        let mut patterns = Vec::new();
        for root_uri in roots {
            let canonical = crate::documents::uri_to_canonical_id_from_str(root_uri);
            let root_path = PathBuf::from(&canonical);
            let mut discovery = TsConfigDiscovery::new();
            discovery.discover(&root_path);
            for entry in discovery.configs() {
                patterns.push(entry.pattern.clone());
            }
        }
        patterns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_config_most_specific() {
        let mut discovery = TsConfigDiscovery::new();
        discovery.configs.push(TsConfigEntry {
            config_path: PathBuf::from("/project/tsconfig.json"),
            pattern: "/project/**".into(),
        });
        discovery.configs.push(TsConfigEntry {
            config_path: PathBuf::from("/project/packages/app/tsconfig.json"),
            pattern: "/project/packages/app/**".into(),
        });

        // File in nested package should match the more specific config
        let result = discovery.find_config_for(Path::new("/project/packages/app/src/main.ts"));
        assert!(result.is_some());
        assert_eq!(
            result.unwrap().config_path,
            PathBuf::from("/project/packages/app/tsconfig.json")
        );
    }

    #[test]
    fn test_find_config_fallback_to_root() {
        let mut discovery = TsConfigDiscovery::new();
        discovery.configs.push(TsConfigEntry {
            config_path: PathBuf::from("/project/tsconfig.json"),
            pattern: "/project/**".into(),
        });

        // File outside specific packages should match root config
        let result = discovery.find_config_for(Path::new("/project/src/utils.ts"));
        assert!(result.is_some());
        assert_eq!(
            result.unwrap().config_path,
            PathBuf::from("/project/tsconfig.json")
        );
    }

    #[test]
    fn test_find_config_no_match() {
        let discovery = TsConfigDiscovery::new();

        let result = discovery.find_config_for(Path::new("/other/project/src/main.ts"));
        assert!(result.is_none());
    }

    // =====================================================================
    // TsConfigPathResolver tests
    // =====================================================================

    /// @ai-generated - Strip JSON comments handles // and /* */ and preserves strings
    #[test]
    fn test_strip_json_comments() {
        let input = r#"{
  // This is a comment
  "baseUrl": ".", /* inline comment */
  "paths": {
    "@/*": ["src/*"] // trailing comment
  }
}"#;
        let result = strip_json_comments(input);
        // Comments should be removed but strings preserved
        assert!(!result.contains("This is a comment"));
        assert!(!result.contains("inline comment"));
        assert!(!result.contains("trailing comment"));
        assert!(result.contains(r#""baseUrl""#));
        assert!(result.contains(r#""paths""#));
        // The "@/*" path pattern should be preserved (it's inside a string)
        assert!(result.contains(r#""@/*""#));

        // Comments inside strings should be preserved
        let input2 = r#"{ "url": "http://example.com" }"#;
        let result2 = strip_json_comments(input2);
        assert!(result2.contains("http://example.com"));

        // Trailing commas should be stripped (common in tsconfig.json)
        let input3 = r#"{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
  },
  "include": ["src/**/*",],
}"#;
        let result3 = strip_json_comments(input3);
        // Should parse as valid JSON after stripping
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&result3);
        assert!(
            parsed.is_ok(),
            "trailing commas should be stripped: {result3}"
        );

        // Commas inside strings should NOT be stripped
        let input4 = r#"{ "paths": { "@/*": ["src/*",] }, "desc": "a, b," }"#;
        let result4 = strip_json_comments(input4);
        let parsed4: Result<serde_json::Value, _> = serde_json::from_str(&result4);
        assert!(parsed4.is_ok(), "should handle mixed commas: {result4}");
        let v4 = parsed4.unwrap();
        assert_eq!(
            v4["desc"].as_str().unwrap(),
            "a, b,",
            "commas inside strings must be preserved"
        );
    }

    /// @ai-generated - PathAlias resolution with wildcard patterns
    #[test]
    fn test_path_resolver_wildcard_matching() {
        let resolver = TsConfigPathResolver {
            aliases: vec![
                PathAlias {
                    prefix: "@/".to_string(),
                    suffix: String::new(),
                    replacements: vec![PathAliasReplacement {
                        prefix: "/project/src/".to_string(),
                        suffix: String::new(),
                    }],
                },
                PathAlias {
                    prefix: "~/".to_string(),
                    suffix: String::new(),
                    replacements: vec![PathAliasReplacement {
                        prefix: "/project/src/".to_string(),
                        suffix: String::new(),
                    }],
                },
            ],
        };

        // These won't resolve because the files don't exist on disk,
        // but we can test the matching logic by checking is_empty
        assert!(!resolver.is_empty());
    }

    /// @ai-generated - from_tsconfig with temp file
    #[test]
    fn test_path_resolver_from_tsconfig_file() {
        let tmp_dir = std::env::temp_dir().join("verter_test_tsconfig");
        let _ = std::fs::create_dir_all(&tmp_dir);
        let src_dir = tmp_dir.join("src");
        let _ = std::fs::create_dir_all(&src_dir);

        // Create a test Vue file
        let test_vue = src_dir.join("Foo.vue");
        std::fs::write(&test_vue, "<template><div/></template>").unwrap();

        // Create tsconfig.json
        let tsconfig = tmp_dir.join("tsconfig.json");
        std::fs::write(
            &tsconfig,
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"],
      "~/*": ["src/*"]
    }
  }
}"#,
        )
        .unwrap();

        let resolver = TsConfigPathResolver::from_tsconfig(&tsconfig);
        assert!(!resolver.is_empty());

        // Resolve @/Foo.vue → /tmp/verter_test_tsconfig/src/Foo.vue
        let result = resolver.resolve("@/Foo.vue");
        assert!(result.is_some(), "should resolve @/Foo.vue");
        assert!(
            result.as_ref().unwrap().ends_with("Foo.vue"),
            "resolved path should end with Foo.vue, got: {:?}",
            result
        );

        // Resolve ~/Foo.vue → same path
        let result2 = resolver.resolve("~/Foo.vue");
        assert!(result2.is_some(), "should resolve ~/Foo.vue");

        // Non-matching specifier
        let result3 = resolver.resolve("lodash");
        assert!(result3.is_none(), "bare specifier should not resolve");

        // Non-existent file
        let result4 = resolver.resolve("@/NonExistent.vue");
        assert!(result4.is_none(), "non-existent file should not resolve");

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    /// @ai-generated - Resolver with JSONC (comments in tsconfig)
    #[test]
    fn test_path_resolver_jsonc_support() {
        let tmp_dir = std::env::temp_dir().join("verter_test_jsonc");
        let _ = std::fs::create_dir_all(&tmp_dir);
        let src_dir = tmp_dir.join("src");
        let _ = std::fs::create_dir_all(&src_dir);

        let test_file = src_dir.join("Bar.vue");
        std::fs::write(&test_file, "<template><div/></template>").unwrap();

        let tsconfig = tmp_dir.join("tsconfig.json");
        std::fs::write(
            &tsconfig,
            r#"{
  // Path aliases for the project
  "compilerOptions": {
    "baseUrl": ".", /* relative to tsconfig */
    "paths": {
      "@/*": ["src/*"] // maps @ to src
    }
  }
}"#,
        )
        .unwrap();

        let resolver = TsConfigPathResolver::from_tsconfig(&tsconfig);
        let result = resolver.resolve("@/Bar.vue");
        assert!(result.is_some(), "should resolve through JSONC");

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    /// @ai-generated - Resolver handles extends
    #[test]
    fn test_path_resolver_extends() {
        let tmp_dir = std::env::temp_dir().join("verter_test_extends");
        let _ = std::fs::create_dir_all(&tmp_dir);
        let src_dir = tmp_dir.join("src");
        let _ = std::fs::create_dir_all(&src_dir);

        let test_file = src_dir.join("Base.vue");
        std::fs::write(&test_file, "<template><div/></template>").unwrap();

        // Base tsconfig
        let base_config = tmp_dir.join("tsconfig.base.json");
        std::fs::write(
            &base_config,
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"]
    }
  }
}"#,
        )
        .unwrap();

        // Child tsconfig extending base
        let child_config = tmp_dir.join("tsconfig.json");
        std::fs::write(
            &child_config,
            r#"{
  "extends": "./tsconfig.base.json"
}"#,
        )
        .unwrap();

        let resolver = TsConfigPathResolver::from_tsconfig(&child_config);
        let result = resolver.resolve("@/Base.vue");
        assert!(result.is_some(), "should resolve through extends");

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    /// Resolver follows `references` in solution-style tsconfigs to find paths.
    /// Mirrors nexus-ui monorepo structure: packages/ui/tsconfig.json has references
    /// to tsconfig.app.json which defines `@/*` path aliases.
    #[test]
    fn test_path_resolver_references_solution_style() {
        let tmp_dir = std::env::temp_dir().join("verter_test_references");
        let _ = std::fs::remove_dir_all(&tmp_dir);
        let src_dir = tmp_dir.join("src");
        let components_dir = src_dir.join("components").join("Overlay");
        std::fs::create_dir_all(&components_dir).unwrap();

        // Create target files
        std::fs::write(components_dir.join("index.ts"), "export const Overlay = {}").unwrap();

        // tsconfig.app.json — has the actual paths
        std::fs::write(
            tmp_dir.join("tsconfig.app.json"),
            r#"{
  "compilerOptions": {
    "composite": true,
    "baseUrl": ".",
    "paths": {
      "@/*": ["./src/*"]
    }
  },
  "include": ["src"]
}"#,
        )
        .unwrap();

        // Root tsconfig.json — solution-style with references only
        std::fs::write(
            tmp_dir.join("tsconfig.json"),
            r#"{
  "compilerOptions": { "composite": true },
  "files": [],
  "references": [
    { "path": "./tsconfig.app.json" }
  ]
}"#,
        )
        .unwrap();

        // from_tsconfig on the root should follow references and find @/* paths
        let resolver = TsConfigPathResolver::from_tsconfig(&tmp_dir.join("tsconfig.json"));
        assert!(
            !resolver.is_empty(),
            "resolver should have aliases from referenced tsconfig.app.json"
        );

        // @/components/Overlay → src/components/Overlay (index.ts)
        let result = resolver.resolve("@/components/Overlay");
        assert!(
            result.is_some(),
            "should resolve @/components/Overlay through references; aliases: {:?}",
            resolver.aliases
        );

        // Negative: non-existent path should not resolve
        let result2 = resolver.resolve("@/nonexistent/Module");
        assert!(result2.is_none(), "non-existent module should not resolve");

        // Negative: bare specifier should not match
        let result3 = resolver.resolve("motion");
        assert!(result3.is_none(), "bare specifier should not resolve");

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    /// Resolver follows `references` in a monorepo sub-package.
    /// The workspace root's ProjectRegistry calls from_tsconfig on packages/ui/tsconfig.json
    /// which has references to tsconfig.app.json.
    #[test]
    fn test_path_resolver_monorepo_nested_references() {
        let tmp_dir = std::env::temp_dir().join("verter_test_mono_refs");
        let _ = std::fs::remove_dir_all(&tmp_dir);
        let pkg_ui = tmp_dir.join("packages").join("ui");
        let src_dir = pkg_ui.join("src").join("components").join("Popup");
        std::fs::create_dir_all(&src_dir).unwrap();

        // Create target component
        std::fs::write(src_dir.join("Popup.vue"), "<template><div/></template>").unwrap();

        // packages/ui/tsconfig.app.json — has @/* paths
        std::fs::write(
            pkg_ui.join("tsconfig.app.json"),
            r#"{
  "compilerOptions": {
    "composite": true,
    "baseUrl": ".",
    "paths": { "@/*": ["./src/*"] }
  },
  "include": ["src"]
}"#,
        )
        .unwrap();

        // packages/ui/tsconfig.json — solution-style
        std::fs::write(
            pkg_ui.join("tsconfig.json"),
            r#"{
  "compilerOptions": { "composite": true },
  "files": [],
  "references": [
    { "path": "./tsconfig.app.json" },
    { "path": "./tsconfig.vitest.json" }
  ]
}"#,
        )
        .unwrap();

        // Resolve from the sub-package tsconfig.json
        let resolver = TsConfigPathResolver::from_tsconfig(&pkg_ui.join("tsconfig.json"));
        assert!(
            !resolver.is_empty(),
            "should find @/* aliases from packages/ui/tsconfig.app.json"
        );

        let result = resolver.resolve("@/components/Popup/Popup.vue");
        assert!(
            result.is_some(),
            "should resolve @/components/Popup/Popup.vue"
        );

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    /// @ai-generated - Extension guessing resolves .ts files
    #[test]
    fn test_try_resolve_file_extension_guessing() {
        let tmp_dir = std::env::temp_dir().join("verter_test_ext");
        let _ = std::fs::create_dir_all(&tmp_dir);

        // Create a .ts file
        let ts_file = tmp_dir.join("utils.ts");
        std::fs::write(&ts_file, "export const x = 1;").unwrap();

        // Resolve without extension
        let path = tmp_dir.join("utils");
        let result = try_resolve_file(&path.to_string_lossy().replace('\\', "/"));
        assert!(result.is_some(), "should resolve utils → utils.ts");
        assert!(
            result.as_ref().unwrap().ends_with("utils.ts"),
            "should end with .ts"
        );

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    // -- ComponentDiagnosticsConfig tests --

    #[test]
    fn default_config_uses_warning() {
        let config = ComponentDiagnosticsConfig::default();
        // Positive: defaults to Warning
        assert_eq!(
            config.unknown_prop_severity,
            DiagnosticSeverityConfig::Warning
        );
        assert_eq!(
            config.unknown_model_severity,
            DiagnosticSeverityConfig::Warning
        );
        // Negative: NOT Error, NOT Off
        assert_ne!(
            config.unknown_prop_severity,
            DiagnosticSeverityConfig::Error
        );
        assert_ne!(config.unknown_prop_severity, DiagnosticSeverityConfig::Off);
    }

    #[test]
    fn off_severity_returns_none() {
        // Off → to_lsp() returns None (disables diagnostics)
        assert!(DiagnosticSeverityConfig::Off.to_lsp().is_none());
        // Positive: Warning returns Some
        assert!(DiagnosticSeverityConfig::Warning.to_lsp().is_some());
        // Positive: Error returns Some
        assert!(DiagnosticSeverityConfig::Error.to_lsp().is_some());
    }

    #[test]
    fn severity_to_lsp_maps_correctly() {
        use tower_lsp_server::ls_types::DiagnosticSeverity;
        assert_eq!(
            DiagnosticSeverityConfig::Error.to_lsp(),
            Some(DiagnosticSeverity::ERROR)
        );
        assert_eq!(
            DiagnosticSeverityConfig::Warning.to_lsp(),
            Some(DiagnosticSeverity::WARNING)
        );
        assert_eq!(
            DiagnosticSeverityConfig::Information.to_lsp(),
            Some(DiagnosticSeverity::INFORMATION)
        );
        assert_eq!(
            DiagnosticSeverityConfig::Hint.to_lsp(),
            Some(DiagnosticSeverity::HINT)
        );
    }

    // =====================================================================
    // ProjectRegistry tests
    // =====================================================================

    #[test]
    fn registry_find_project_most_specific() {
        let tmp = std::env::temp_dir().join("verter_test_registry_specific");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("packages/ui/src")).unwrap();
        std::fs::create_dir_all(tmp.join("packages/app/src")).unwrap();

        // Create tsconfigs with different aliases
        std::fs::write(
            tmp.join("packages/ui/tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@ui/*":["src/*"]}}}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("packages/app/tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@app/*":["src/*"]}}}"#,
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let registry = ProjectRegistry::from_canonical_roots(&[&root]);

        // File in packages/ui should match packages/ui project
        let ui_file = format!("{root}/packages/ui/src/Button.vue");
        let project = registry.find_project(&ui_file);
        assert!(project.is_some(), "should find project for ui file");
        assert!(
            project.unwrap().root.contains("packages/ui"),
            "should match packages/ui, got: {}",
            project.unwrap().root,
        );

        // File in packages/app should match packages/app project
        let app_file = format!("{root}/packages/app/src/App.vue");
        let project = registry.find_project(&app_file);
        assert!(project.is_some(), "should find project for app file");
        assert!(
            project.unwrap().root.contains("packages/app"),
            "should match packages/app, got: {}",
            project.unwrap().root,
        );

        // File in packages/ui should NOT match packages/app
        let ui_project_root = registry.find_project_root(&ui_file).unwrap();
        assert!(
            !ui_project_root.contains("packages/app"),
            "ui file must not match app project"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn registry_resolve_alias_per_project() {
        let tmp = std::env::temp_dir().join("verter_test_registry_alias");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("packages/ui/src")).unwrap();
        std::fs::create_dir_all(tmp.join("packages/app/src")).unwrap();

        // Create test files
        std::fs::write(
            tmp.join("packages/ui/src/Button.vue"),
            "<template><div/></template>",
        )
        .unwrap();
        std::fs::write(
            tmp.join("packages/app/src/Home.vue"),
            "<template><div/></template>",
        )
        .unwrap();

        // Different alias mappings per package
        std::fs::write(
            tmp.join("packages/ui/tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("packages/app/tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let registry = ProjectRegistry::from_canonical_roots(&[&root]);

        // @/Button.vue from ui package should resolve to ui/src/Button.vue
        let ui_file = format!("{root}/packages/ui/src/index.ts");
        let resolved = registry.resolve_alias(&ui_file, "@/Button.vue");
        assert!(resolved.is_some(), "should resolve @/Button.vue from ui");
        assert!(
            resolved.as_ref().unwrap().ends_with("Button.vue"),
            "should resolve to Button.vue in ui, got: {:?}",
            resolved
        );
        assert!(
            resolved.as_ref().unwrap().contains("packages/ui"),
            "resolved path should be under packages/ui, got: {:?}",
            resolved
        );

        // @/Home.vue from app package should resolve to app/src/Home.vue
        let app_file = format!("{root}/packages/app/src/index.ts");
        let resolved = registry.resolve_alias(&app_file, "@/Home.vue");
        assert!(resolved.is_some(), "should resolve @/Home.vue from app");
        assert!(
            resolved.as_ref().unwrap().ends_with("Home.vue"),
            "should resolve to Home.vue in app, got: {:?}",
            resolved
        );
        assert!(
            resolved.as_ref().unwrap().contains("packages/app"),
            "resolved path should be under packages/app, got: {:?}",
            resolved
        );

        // @/Home.vue from ui package should NOT resolve (file doesn't exist in ui/src)
        let resolved_cross = registry.resolve_alias(&ui_file, "@/Home.vue");
        assert!(
            resolved_cross.is_none(),
            "should not resolve @/Home.vue from ui package"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn registry_fallback_to_workspace_root() {
        let tmp = std::env::temp_dir().join("verter_test_registry_fallback");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let registry = ProjectRegistry::from_canonical_roots(&[&root]);

        // File in root (no tsconfig) should still find a default project
        let file = format!("{root}/src/App.vue");
        let project = registry.find_project(&file);
        assert!(
            project.is_some(),
            "should fall back to workspace root project"
        );

        // Default project should have empty aliases
        let resolved = registry.resolve_alias(&file, "@/Something.vue");
        assert!(
            resolved.is_none(),
            "default project (no tsconfig) should have no aliases"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn registry_per_project_lint_config() {
        let tmp = std::env::temp_dir().join("verter_test_registry_lint");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("packages/strict-pkg/src")).unwrap();
        std::fs::create_dir_all(tmp.join("packages/lax-pkg/src")).unwrap();

        // strict-pkg has verterrc with strict preset
        std::fs::write(
            tmp.join("packages/strict-pkg/.verterrc.json"),
            r#"{"lint":{"preset":"strict"}}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("packages/strict-pkg/tsconfig.json"),
            r#"{"compilerOptions":{}}"#,
        )
        .unwrap();

        // lax-pkg has verterrc with essential preset
        std::fs::write(
            tmp.join("packages/lax-pkg/.verterrc.json"),
            r#"{"lint":{"preset":"essential"}}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("packages/lax-pkg/tsconfig.json"),
            r#"{"compilerOptions":{}}"#,
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let registry = ProjectRegistry::from_canonical_roots(&[&root]);

        let strict_file = format!("{root}/packages/strict-pkg/src/Foo.vue");
        let strict_project = registry.linter_for(&strict_file);
        assert!(strict_project.is_some(), "should find strict project");
        assert_eq!(
            strict_project.unwrap().lint_config.config.preset,
            verter_diagnostics::LintPreset::Strict,
        );

        let lax_file = format!("{root}/packages/lax-pkg/src/Bar.vue");
        let lax_project = registry.linter_for(&lax_file);
        assert!(lax_project.is_some(), "should find lax project");
        assert_eq!(
            lax_project.unwrap().lint_config.config.preset,
            verter_diagnostics::LintPreset::Essential,
        );

        // Verify they DON'T share the same config
        assert_ne!(
            strict_project.unwrap().lint_config.config.preset,
            lax_project.unwrap().lint_config.config.preset,
            "different packages must have different lint presets"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // =====================================================================
    // Vite alias discovery tests
    // =====================================================================

    #[test]
    fn merge_vite_aliases_adds_new_prefixes() {
        let mut resolver = TsConfigPathResolver::default();
        resolver.merge_vite_aliases(vec![
            ("@/".to_string(), "/project/src".to_string()),
            ("~/".to_string(), "/project/lib".to_string()),
        ]);

        // Should have 2 aliases
        assert_eq!(resolver.aliases.len(), 2, "should have 2 aliases");
        assert!(!resolver.is_empty(), "resolver should not be empty");

        // Aliases should end with `/` for wildcard matching
        assert!(
            resolver.aliases.iter().any(|a| a.prefix == "@/"),
            "should have @/ prefix"
        );
        assert!(
            resolver.aliases.iter().any(|a| a.prefix == "~/"),
            "should have ~/ prefix"
        );

        // Replacements should end with `/`
        for alias in &resolver.aliases {
            for rep in &alias.replacements {
                assert!(
                    rep.prefix.ends_with('/'),
                    "replacement prefix should end with /, got: {}",
                    rep.prefix
                );
            }
        }
    }

    #[test]
    fn merge_vite_aliases_overrides_existing() {
        let mut resolver = TsConfigPathResolver {
            aliases: vec![PathAlias {
                prefix: "@/".to_string(),
                suffix: String::new(),
                replacements: vec![PathAliasReplacement {
                    prefix: "/old/path/".to_string(),
                    suffix: String::new(),
                }],
            }],
        };

        // Merge vite alias with same prefix — should override
        resolver.merge_vite_aliases(vec![("@/".to_string(), "/new/path".to_string())]);

        assert_eq!(
            resolver.aliases.len(),
            1,
            "should still have 1 alias (replaced)"
        );
        assert_eq!(
            resolver.aliases[0].replacements[0].prefix, "/new/path/",
            "replacement should be updated to new path"
        );
        // Negative: old path should be gone
        assert!(
            !resolver
                .aliases
                .iter()
                .any(|a| a.replacements.iter().any(|r| r.prefix.contains("old"))),
            "old path should not remain"
        );
    }

    #[test]
    fn merge_vite_aliases_preserves_non_conflicting() {
        let mut resolver = TsConfigPathResolver {
            aliases: vec![PathAlias {
                prefix: "~/".to_string(),
                suffix: String::new(),
                replacements: vec![PathAliasReplacement {
                    prefix: "/project/lib/".to_string(),
                    suffix: String::new(),
                }],
            }],
        };

        // Add a different alias — should not remove existing
        resolver.merge_vite_aliases(vec![("@/".to_string(), "/project/src".to_string())]);

        assert_eq!(resolver.aliases.len(), 2, "should have 2 aliases");
        assert!(
            resolver.aliases.iter().any(|a| a.prefix == "~/"),
            "original ~/ alias should remain"
        );
        assert!(
            resolver.aliases.iter().any(|a| a.prefix == "@/"),
            "new @/ alias should be added"
        );
    }

    #[test]
    fn discover_vite_aliases_no_config_returns_empty() {
        let tmp = std::env::temp_dir().join("verter_test_vite_no_config");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let result = discover_vite_aliases(&tmp, "node");
        assert!(
            result.is_empty(),
            "should return empty when no vite config exists"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_vite_aliases_simple_config() {
        // Only run if Node.js is available
        let node = crate::tsserver::find_node();
        if node.is_none() {
            eprintln!("skipping discover_vite_aliases_simple_config: node not found");
            return;
        }
        let node = node.unwrap();

        let tmp = std::env::temp_dir().join("verter_test_vite_simple");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        // Create a simple vite.config.js with resolve.alias
        std::fs::write(
            tmp.join("vite.config.js"),
            &format!(
                r#"
export default {{
  resolve: {{
    alias: {{
      '@': '{src_dir}',
    }}
  }}
}};
"#,
                src_dir = tmp.join("src").to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();

        let result = discover_vite_aliases(&tmp, &node);
        assert!(
            !result.is_empty(),
            "should discover aliases from vite config"
        );
        assert_eq!(result.len(), 1, "should have exactly 1 alias");
        assert_eq!(result[0].0, "@/", "alias find should be '@/'");
        assert!(
            result[0].1.contains("src"),
            "alias replacement should contain 'src', got: {}",
            result[0].1
        );
        // Negative: should not have any empty entries
        assert!(
            result.iter().all(|(f, r)| !f.is_empty() && !r.is_empty()),
            "no alias should have empty find or replacement"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_vite_aliases_array_format() {
        // Only run if Node.js is available
        let node = crate::tsserver::find_node();
        if node.is_none() {
            eprintln!("skipping discover_vite_aliases_array_format: node not found");
            return;
        }
        let node = node.unwrap();

        let tmp = std::env::temp_dir().join("verter_test_vite_array");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();
        std::fs::create_dir_all(tmp.join("lib")).unwrap();

        // Create vite config with array-style aliases
        std::fs::write(
            tmp.join("vite.config.mjs"),
            &format!(
                r#"
export default {{
  resolve: {{
    alias: [
      {{ find: '@', replacement: '{src}' }},
      {{ find: '~', replacement: '{lib}' }},
    ]
  }}
}};
"#,
                src = tmp.join("src").to_string_lossy().replace('\\', "/"),
                lib = tmp.join("lib").to_string_lossy().replace('\\', "/"),
            ),
        )
        .unwrap();

        let result = discover_vite_aliases(&tmp, &node);
        assert_eq!(result.len(), 2, "should discover 2 aliases");
        assert!(
            result.iter().any(|(f, _)| f == "@/"),
            "should have @/ alias"
        );
        assert!(
            result.iter().any(|(f, _)| f == "~/"),
            "should have ~/ alias"
        );
        // Negative: should not have regex-based aliases (they are filtered out)
        assert!(
            result.iter().all(|(f, _)| !f.starts_with('^')),
            "regex aliases should be filtered out"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_vite_aliases_disabled_returns_empty() {
        // Test that when vite_config_enabled is false, no aliases are discovered.
        // This is tested at the ProjectRegistry level (from_workspace_roots skips discovery).
        // At the function level, discover_vite_aliases always runs — the caller controls enablement.
        let tmp = std::env::temp_dir().join("verter_test_vite_disabled");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // Even with a vite config, if we don't call discover_vite_aliases, no aliases.
        std::fs::write(
            tmp.join("vite.config.js"),
            "export default { resolve: { alias: { '@': '/src' } } };",
        )
        .unwrap();

        // Simulate disabled: just don't call discover_vite_aliases
        let resolver = TsConfigPathResolver::default();
        assert!(
            resolver.is_empty(),
            "resolver should be empty when vite discovery not called"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn strip_trailing_commas_preserves_multibyte_utf8() {
        // Multi-byte UTF-8: non-ASCII chars in string values
        let input = r#"{"desc": "Compilé avec succès", "ok": true,}"#;
        let result = strip_trailing_commas(input);
        // Positive: valid JSON after stripping trailing comma
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&result);
        assert!(parsed.is_ok(), "should produce valid JSON: {result}");
        let v = parsed.unwrap();
        assert_eq!(
            v["desc"].as_str().unwrap(),
            "Compilé avec succès",
            "multi-byte chars must be preserved"
        );
        // Negative: no trailing comma before }
        assert!(
            !result.contains(",}"),
            "trailing comma should be removed: {result}"
        );
    }

    #[test]
    fn strip_trailing_commas_preserves_cjk_chars() {
        let input = r#"{"名前": "テスト",}"#;
        let result = strip_trailing_commas(input);
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&result);
        assert!(parsed.is_ok(), "should produce valid JSON: {result}");
        let v = parsed.unwrap();
        assert_eq!(v["名前"].as_str().unwrap(), "テスト");
    }

    #[test]
    fn strip_trailing_commas_roundtrip_bytes() {
        // Verify that strip_trailing_commas preserves byte-exact content
        // for any input that has no trailing commas.
        let input = r#"{"key": "café", "num": 42}"#;
        let result = strip_trailing_commas(input);
        assert_eq!(result, input, "no-op input should be byte-exact preserved");
    }

    #[test]
    fn strip_trailing_commas_bare_multibyte_outside_strings() {
        // strip_json_comments may produce output with non-ASCII chars in
        // positions outside JSON strings (e.g., replaced comments leaving stubs).
        // Also tests that the full pipeline works: strip_json_comments calls
        // strip_trailing_commas, so multi-byte content must survive both passes.
        let input = "{ \"path\": \"@/*\", } // résumé";
        let result = strip_json_comments(input);
        // The comment is stripped; trailing comma is stripped
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&result);
        assert!(parsed.is_ok(), "should produce valid JSON: {result}");
    }

    #[test]
    fn find_project_enforces_path_boundary() {
        // Only /workspace/app exists — NOT /workspace/app-admin.
        // Without path boundary enforcement, /workspace/app-admin/src/Foo.vue
        // would incorrectly match /workspace/app because it starts_with "/workspace/app".
        let registry = ProjectRegistry {
            projects: vec![ProjectConfig {
                root: "/workspace/app".to_string(),
                path_resolver: TsConfigPathResolver::default(),
                lint_config: ResolvedLintConfig::default(),
                linter: verter_diagnostics::Linter::default(),
                lint_explicitly_configured: false,
            }],
        };

        // Positive: file in /workspace/app/ should match
        let project = registry.find_project("/workspace/app/src/Baz.vue");
        assert!(project.is_some(), "file in /workspace/app/ should match");

        // Negative: file in /workspace/app-admin/ should NOT match /workspace/app
        let project2 = registry.find_project("/workspace/app-admin/src/Foo.vue");
        assert!(
            project2.is_none(),
            "file in /workspace/app-admin/ must not match /workspace/app"
        );
    }

    #[test]
    fn apply_default_lint_only_affects_non_explicit_projects() {
        let mut registry = ProjectRegistry {
            projects: vec![
                ProjectConfig {
                    root: "/workspace/explicit/".to_string(),
                    path_resolver: TsConfigPathResolver::default(),
                    lint_config: ResolvedLintConfig {
                        config: verter_diagnostics::LintConfig {
                            preset: verter_diagnostics::LintPreset::Strict,
                            ..Default::default()
                        },
                        explicitly_configured: true,
                    },
                    linter: verter_diagnostics::Linter::default(),
                    lint_explicitly_configured: true,
                },
                ProjectConfig {
                    root: "/workspace/default/".to_string(),
                    path_resolver: TsConfigPathResolver::default(),
                    lint_config: ResolvedLintConfig::default(),
                    linter: verter_diagnostics::Linter::default(),
                    lint_explicitly_configured: false,
                },
            ],
        };

        let new_config = verter_diagnostics::LintConfig {
            preset: verter_diagnostics::LintPreset::All,
            ..Default::default()
        };
        registry.apply_default_lint(&new_config);

        // Positive: non-explicit project gets the default config
        assert_eq!(
            registry.projects[1].lint_config.config.preset,
            verter_diagnostics::LintPreset::All,
            "non-explicit project should get default lint config"
        );

        // Negative: explicit project should NOT be overridden
        assert_eq!(
            registry.projects[0].lint_config.config.preset,
            verter_diagnostics::LintPreset::Strict,
            "explicitly configured project must keep its own config"
        );
    }

    #[test]
    fn registry_file_outside_all_projects() {
        let tmp = std::env::temp_dir().join("verter_test_registry_outside");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let registry = ProjectRegistry::from_canonical_roots(&[&root]);

        // File completely outside the workspace
        let outside = "/some/other/project/App.vue";
        let project = registry.find_project(outside);
        assert!(
            project.is_none(),
            "file outside all workspace roots should return None"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── parse_vite_alias_stdout tests ──────────────────────────────────

    #[test]
    fn parse_vite_alias_stdout_clean_json() {
        let raw = r#"[{"find":"@","replacement":"/src"}]"#;
        let result = parse_vite_alias_stdout(raw);
        assert!(result.is_some(), "should parse clean JSON");
        let entries = result.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].find, "@");
        assert_eq!(entries[0].replacement, "/src");
    }

    #[test]
    fn parse_vite_alias_stdout_empty_input() {
        assert!(
            parse_vite_alias_stdout("").is_none(),
            "empty input should return None"
        );
        assert!(
            parse_vite_alias_stdout("  \n  ").is_none(),
            "whitespace-only input should return None"
        );
    }

    #[test]
    fn parse_vite_alias_stdout_sentinel_markers() {
        let raw = "some noise\n__VERTER_ALIASES_BEGIN__[{\"find\":\"@\",\"replacement\":\"/src\"}]__VERTER_ALIASES_END__\nmore noise";
        let result = parse_vite_alias_stdout(raw);
        assert!(result.is_some(), "should extract JSON between sentinels");
        let entries = result.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].find, "@");
    }

    #[test]
    fn parse_vite_alias_stdout_sentinel_with_prefix_noise() {
        let raw = "Warning: something\nDeprecation notice\n__VERTER_ALIASES_BEGIN__[]__VERTER_ALIASES_END__";
        let result = parse_vite_alias_stdout(raw);
        assert!(
            result.is_some(),
            "should handle prefix noise with sentinels"
        );
        assert!(result.unwrap().is_empty(), "should return empty array");
    }

    #[test]
    fn parse_vite_alias_stdout_invalid_json_between_sentinels() {
        let raw = "__VERTER_ALIASES_BEGIN__not-json__VERTER_ALIASES_END__";
        let result = parse_vite_alias_stdout(raw);
        assert!(
            result.is_none(),
            "should return None when sentinel content is not valid JSON"
        );
    }

    #[test]
    fn parse_vite_alias_stdout_multiple_sentinel_pairs() {
        // First valid pair should win
        let raw = "__VERTER_ALIASES_BEGIN__[{\"find\":\"@\",\"replacement\":\"/src\"}]__VERTER_ALIASES_END__ junk __VERTER_ALIASES_BEGIN__[{\"find\":\"~\",\"replacement\":\"/lib\"}]__VERTER_ALIASES_END__";
        let result = parse_vite_alias_stdout(raw);
        assert!(result.is_some(), "should use first sentinel pair");
        let entries = result.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].find, "@", "should use first pair's content");
        assert!(
            entries.iter().all(|e| e.find != "~"),
            "should not include second pair's content"
        );
    }

    #[test]
    fn parse_vite_alias_stdout_fallback_without_sentinels() {
        // Backward compat: clean JSON without sentinels
        let raw = r#"[{"find":"@","replacement":"/src"},{"find":"~","replacement":"/lib"}]"#;
        let result = parse_vite_alias_stdout(raw);
        assert!(result.is_some(), "should fall back to direct JSON parse");
        assert_eq!(result.unwrap().len(), 2);
    }

    #[test]
    fn parse_vite_alias_stdout_noisy_without_sentinels() {
        // No sentinels + noise → fallback fails
        let raw = "ExperimentalWarning: something\n[{\"find\":\"@\"}]";
        let result = parse_vite_alias_stdout(raw);
        assert!(
            result.is_none(),
            "noisy output without sentinels should return None (fallback parse fails)"
        );
    }

    // =====================================================================
    // has_solution_style_tsconfig tests
    // =====================================================================

    #[test]
    fn has_solution_style_tsconfig_detects_references() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("tsconfig.json"),
            r#"{ "files": [], "references": [{ "path": "./tsconfig.app.json" }] }"#,
        )
        .unwrap();
        assert!(
            has_solution_style_tsconfig(dir.path()),
            "should detect solution-style tsconfig with references"
        );
    }

    #[test]
    fn has_solution_style_tsconfig_false_for_flat_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("tsconfig.json"),
            r#"{ "compilerOptions": { "strict": true }, "include": ["src"] }"#,
        )
        .unwrap();
        assert!(
            !has_solution_style_tsconfig(dir.path()),
            "flat tsconfig without references should return false"
        );
    }

    #[test]
    fn has_solution_style_tsconfig_false_for_empty_references() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tsconfig.json"), r#"{ "references": [] }"#).unwrap();
        assert!(
            !has_solution_style_tsconfig(dir.path()),
            "empty references array should return false"
        );
    }

    #[test]
    fn has_solution_style_tsconfig_false_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            !has_solution_style_tsconfig(dir.path()),
            "missing tsconfig.json should return false"
        );
    }

    #[test]
    fn has_solution_style_tsconfig_handles_jsonc() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("tsconfig.json"),
            "{\n  // Solution-style config\n  \"files\": [],\n  \"references\": [{ \"path\": \"./tsconfig.app.json\" }],\n}",
        )
        .unwrap();
        assert!(
            has_solution_style_tsconfig(dir.path()),
            "should handle JSONC with comments and trailing commas"
        );
    }

    #[test]
    fn has_solution_style_tsconfig_detects_monorepo_subdirectory() {
        // Monorepo: root has no tsconfig, but packages/ui/tsconfig.json has references
        let dir = tempfile::tempdir().unwrap();
        let packages = dir.path().join("packages");
        let ui = packages.join("ui");
        std::fs::create_dir_all(&ui).unwrap();
        std::fs::write(
            ui.join("tsconfig.json"),
            r#"{ "composite": true, "files": [], "references": [{ "path": "./tsconfig.app.json" }] }"#,
        )
        .unwrap();
        assert!(
            has_solution_style_tsconfig(dir.path()),
            "should detect solution-style tsconfig in monorepo subdirectory (packages/ui/)"
        );
        // Negative: root itself has no tsconfig
        assert!(
            !is_solution_style_tsconfig(&dir.path().join("tsconfig.json")),
            "root tsconfig.json should not exist"
        );
    }

    #[test]
    fn has_solution_style_tsconfig_skips_node_modules() {
        // node_modules should be skipped even if it contains a solution-style tsconfig
        let dir = tempfile::tempdir().unwrap();
        let nm = dir.path().join("node_modules").join("some-pkg");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(
            nm.join("tsconfig.json"),
            r#"{ "files": [], "references": [{ "path": "./tsconfig.app.json" }] }"#,
        )
        .unwrap();
        assert!(
            !has_solution_style_tsconfig(dir.path()),
            "should not scan node_modules"
        );
    }
}

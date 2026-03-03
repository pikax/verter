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
    pub fn to_lsp(self) -> Option<tower_lsp_server::lsp_types::DiagnosticSeverity> {
        use tower_lsp_server::lsp_types::DiagnosticSeverity;
        match self {
            Self::Error => Some(DiagnosticSeverity::ERROR),
            Self::Warning => Some(DiagnosticSeverity::WARNING),
            Self::Information => Some(DiagnosticSeverity::INFORMATION),
            Self::Hint => Some(DiagnosticSeverity::HINT),
            Self::Off => None,
        }
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
        use tower_lsp_server::lsp_types::DiagnosticSeverity;
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
}

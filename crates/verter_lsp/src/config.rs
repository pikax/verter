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
/// This is the current alias-only resolver. It is intentionally narrower than
/// the native project resolver, which needs to cover tsconfig,
/// Node/package resolution, and provider-target mapping without assuming direct
/// provider disk access.
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
            // find values are already normalized with `/` suffix (from vite_config module)
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

pub use verter_diagnostics::{
    discover_lint_config, parse_rule_severity, strip_json_comments, strip_trailing_commas,
    ResolvedLintConfig, VerterProjectConfig,
};

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
    /// Workspace folder that discovered this project.
    pub workspace_root: String,
    /// Canonical tsconfig path when this project is backed by a discovered config.
    pub tsconfig_path: Option<String>,
    /// Resolved tsconfig file membership for owner selection.
    pub membership: crate::project_resolver::ProjectMembership,
    /// Existing IDE alias sources (currently Vite aliases) injected ahead of tsconfig paths.
    pub workspace_aliases: Vec<crate::project_resolver::WorkspaceAlias>,
    /// Preserved tsconfig compiler options for the native resolver.
    pub compiler_options: crate::project_resolver::IdeProjectCompilerOptions,
    /// Resolved project-reference edges for the native resolver.
    pub references: Vec<String>,
    /// Path alias resolver (from tsconfig paths, or vite aliases on fallback projects).
    pub path_resolver: TsConfigPathResolver,
    /// Lint configuration for this project.
    pub lint_config: ResolvedLintConfig,
    /// Linter instance built from `lint_config`. Cached to avoid recreating.
    pub linter: verter_diagnostics::Linter,
    /// Whether lint was explicitly configured for this project.
    pub lint_explicitly_configured: bool,
    /// Path to the analyzed vite config file (only on fallback projects).
    pub vite_config_path: Option<String>,
    /// Config file + helper deps for invalidation (canonical absolute paths).
    /// Always includes the config file itself. Only populated on fallback projects.
    pub vite_config_deps: Vec<String>,
    /// Whether this project uses SSR (detected from Nuxt, `.verterrc.json`, or init options).
    pub ssr_enabled: bool,
}

/// Result of building a project registry, including trust-required entries.
pub struct RegistryBuildResult {
    pub registry: ProjectRegistry,
    /// Configs that need user trust before their aliases can be used.
    pub trust_required: Vec<crate::vite_config::ViteConfigTrustInfo>,
}

/// Detect whether a project root is an SSR project.
///
/// Returns `true` if:
/// - `nuxt.config.{ts,js,mjs,mts}` exists (Nuxt project)
/// - `.nuxt/` directory exists
/// - `.verterrc.json` has `"ssr": { "enabled": true }`
fn detect_ssr_project(root: &std::path::Path, lint_config: &ResolvedLintConfig) -> bool {
    // Check if the lint config already has ssr_mode set (from .verterrc.json parsing)
    if lint_config.config.ssr_mode {
        return true;
    }

    // Detect Nuxt: nuxt.config.{ts,js,mjs,mts}
    for ext in &["ts", "js", "mjs", "mts"] {
        if root.join(format!("nuxt.config.{ext}")).exists() {
            return true;
        }
    }

    // Detect Nuxt: .nuxt/ directory
    if root.join(".nuxt").is_dir() {
        return true;
    }

    false
}

/// Check if a file path indicates SSR context (e.g., `*.server.vue`).
pub fn is_ssr_file(path: &str) -> bool {
    path.ends_with(".server.vue")
}

/// Check if a file path indicates client-only context (e.g., `*.client.vue`).
pub fn is_client_only_file(path: &str) -> bool {
    path.ends_with(".client.vue")
}

impl ProjectConfig {
    pub fn to_ide_project_config(&self) -> crate::project_resolver::IdeProjectConfig {
        let mut project = crate::project_resolver::IdeProjectConfig::new(
            self.root.clone(),
            self.workspace_root.clone(),
            self.tsconfig_path.clone(),
        );
        project.workspace_aliases = self.workspace_aliases.clone();
        project.compiler_options = self.compiler_options.clone();
        project.references = self.references.clone();
        project.membership = self.membership.clone();
        project
    }
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
    /// Build a registry from workspace roots. Tsconfig-backed projects use tsconfig
    /// paths exclusively; fallback (no-tsconfig) projects get vite aliases via static
    /// analysis or trusted execution.
    pub fn from_workspace_roots(
        roots: &[String],
        vite_opts: &crate::vite_config::ViteConfigOptions,
    ) -> RegistryBuildResult {
        let mut projects = Vec::new();
        let mut trust_required = Vec::new();

        for root_uri in roots {
            let canonical = crate::documents::uri_to_canonical_id_from_str(root_uri);
            let root_path = PathBuf::from(&canonical);

            // Discover tsconfigs under this root
            let mut discovery = TsConfigDiscovery::new();
            discovery.discover(&root_path);

            for entry in discovery.configs() {
                let Some(dir) = entry.config_path.parent() else {
                    continue;
                };
                let project_root = dir.to_string_lossy().replace('\\', "/");
                let project_root_path = PathBuf::from(&project_root);
                let resolver = TsConfigPathResolver::from_tsconfig(&entry.config_path);
                let membership = load_project_membership(&entry.config_path);
                let compiler_options = load_compiler_options(&entry.config_path);
                let references = load_project_references(&entry.config_path);
                // Tsconfig-backed projects use tsconfig paths as the sole alias source.
                // Vite aliases are only applied to fallback (no-tsconfig) projects.
                let workspace_aliases = Vec::new();

                let lint = discover_lint_config(&project_root_path);
                let ssr_enabled = detect_ssr_project(&project_root_path, &lint);
                let linter = verter_diagnostics::Linter::new(lint.config.clone());

                projects.push(ProjectConfig {
                    root: project_root,
                    workspace_root: canonical.clone(),
                    tsconfig_path: Some(entry.config_path.to_string_lossy().replace('\\', "/")),
                    membership,
                    workspace_aliases,
                    compiler_options,
                    references,
                    path_resolver: resolver,
                    lint_config: lint.clone(),
                    linter,
                    lint_explicitly_configured: lint.explicitly_configured,
                    vite_config_path: None,
                    vite_config_deps: Vec::new(),
                    ssr_enabled,
                });
            }

            // Fallback project (no tsconfig) — apply vite aliases if enabled.
            // Skip vite analysis when tsconfigs were found for this root: those projects
            // already own alias resolution and the fallback is only a catch-all.
            let has_tsconfigs = !discovery.configs().is_empty();
            let lint = discover_lint_config(&root_path);
            let linter = verter_diagnostics::Linter::new(lint.config.clone());
            let mut fallback_resolver = TsConfigPathResolver::default();
            let mut fallback_workspace_aliases = Vec::new();
            let mut fallback_vite_config_path = None;
            let mut fallback_vite_config_deps = Vec::new();

            if vite_opts.enabled && !has_tsconfigs {
                use crate::vite_config::{analyze_vite_config, ViteConfigAnalysis};
                match analyze_vite_config(&root_path) {
                    ViteConfigAnalysis::Resolved {
                        config_path,
                        aliases,
                        dependency_files,
                    } => {
                        if !aliases.is_empty() {
                            tracing::debug!(
                                "statically resolved {} vite aliases for {}",
                                aliases.len(),
                                canonical
                            );
                            fallback_workspace_aliases = aliases
                                .iter()
                                .map(|(find, replacement)| {
                                    crate::project_resolver::WorkspaceAlias {
                                        find: find.clone(),
                                        replacement: replacement.clone(),
                                    }
                                })
                                .collect();
                            fallback_resolver.merge_vite_aliases(aliases);
                        }
                        fallback_vite_config_path = Some(config_path);
                        fallback_vite_config_deps = dependency_files;
                    }
                    ViteConfigAnalysis::Complex {
                        config_path,
                        reason,
                    } => {
                        // Check if file is trusted
                        let is_trusted = vite_opts.trusted_files.iter().any(|tf| {
                            let tf_normalized = tf.replace('\\', "/");
                            tf_normalized == config_path
                        });

                        if is_trusted {
                            if let Some(np) = &vite_opts.node_path {
                                let config_path_buf = PathBuf::from(&config_path);
                                if let Some(result) =
                                    crate::vite_config::execute_trusted_vite_config(
                                        &config_path_buf,
                                        &root_path,
                                        np,
                                    )
                                {
                                    if !result.aliases.is_empty() {
                                        tracing::debug!(
                                            "trusted execution: {} vite aliases for {}",
                                            result.aliases.len(),
                                            canonical
                                        );
                                        fallback_workspace_aliases = result
                                            .aliases
                                            .iter()
                                            .map(|(find, replacement)| {
                                                crate::project_resolver::WorkspaceAlias {
                                                    find: find.clone(),
                                                    replacement: replacement.clone(),
                                                }
                                            })
                                            .collect();
                                        fallback_resolver.merge_vite_aliases(result.aliases);
                                    }
                                    fallback_vite_config_deps = result.dependency_files;
                                } else {
                                    // Execution failed, try LKG
                                    let lkg = crate::vite_config::get_lkg_or_empty(&config_path);
                                    if !lkg.is_empty() {
                                        fallback_workspace_aliases = lkg
                                            .iter()
                                            .map(|(find, replacement)| {
                                                crate::project_resolver::WorkspaceAlias {
                                                    find: find.clone(),
                                                    replacement: replacement.clone(),
                                                }
                                            })
                                            .collect();
                                        fallback_resolver.merge_vite_aliases(lkg);
                                    }
                                }
                                fallback_vite_config_path = Some(config_path);
                            }
                        } else {
                            // Not trusted → add to trust_required
                            trust_required.push(crate::vite_config::ViteConfigTrustInfo {
                                config_path: config_path.clone(),
                                workspace_root: canonical.clone(),
                                reason,
                            });
                            fallback_vite_config_path = Some(config_path);
                        }
                    }
                    ViteConfigAnalysis::NotFound => {}
                }
            }

            let ssr_enabled = detect_ssr_project(&root_path, &lint);
            projects.push(ProjectConfig {
                root: canonical,
                workspace_root: crate::documents::uri_to_canonical_id_from_str(root_uri),
                tsconfig_path: None,
                membership: crate::project_resolver::ProjectMembership::MatchAll,
                workspace_aliases: fallback_workspace_aliases,
                compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
                references: Vec::new(),
                path_resolver: fallback_resolver,
                lint_config: lint.clone(),
                linter,
                lint_explicitly_configured: lint.explicitly_configured,
                vite_config_path: fallback_vite_config_path,
                vite_config_deps: fallback_vite_config_deps,
                ssr_enabled,
            });
        }

        sort_projects(&mut projects);

        RegistryBuildResult {
            registry: Self { projects },
            trust_required,
        }
    }

    /// Build a registry from canonical paths (not URIs). Used in tests.
    pub fn from_canonical_roots(roots: &[&str]) -> Self {
        let mut projects = Vec::new();

        for &root in roots {
            let root_path = PathBuf::from(root);

            let mut discovery = TsConfigDiscovery::new();
            discovery.discover(&root_path);

            for entry in discovery.configs() {
                let Some(dir) = entry.config_path.parent() else {
                    continue;
                };
                let project_root = dir.to_string_lossy().replace('\\', "/");
                let project_root_path = PathBuf::from(&project_root);
                let resolver = TsConfigPathResolver::from_tsconfig(&entry.config_path);
                let membership = load_project_membership(&entry.config_path);
                let compiler_options = load_compiler_options(&entry.config_path);
                let references = load_project_references(&entry.config_path);
                let lint = discover_lint_config(&project_root_path);
                let ssr_enabled = detect_ssr_project(&project_root_path, &lint);
                let linter = verter_diagnostics::Linter::new(lint.config.clone());

                projects.push(ProjectConfig {
                    root: project_root,
                    workspace_root: root.to_string(),
                    tsconfig_path: Some(entry.config_path.to_string_lossy().replace('\\', "/")),
                    membership,
                    workspace_aliases: Vec::new(),
                    compiler_options,
                    references,
                    path_resolver: resolver,
                    lint_config: lint.clone(),
                    linter,
                    lint_explicitly_configured: lint.explicitly_configured,
                    vite_config_path: None,
                    vite_config_deps: Vec::new(),
                    ssr_enabled,
                });
            }

            let lint = discover_lint_config(&root_path);
            let ssr_enabled = detect_ssr_project(&root_path, &lint);
            let linter = verter_diagnostics::Linter::new(lint.config.clone());
            projects.push(ProjectConfig {
                root: root.to_string(),
                workspace_root: root.to_string(),
                tsconfig_path: None,
                membership: crate::project_resolver::ProjectMembership::MatchAll,
                workspace_aliases: Vec::new(),
                compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
                references: Vec::new(),
                path_resolver: TsConfigPathResolver::default(),
                lint_config: lint.clone(),
                linter,
                lint_explicitly_configured: lint.explicitly_configured,
                vite_config_path: None,
                vite_config_deps: Vec::new(),
                ssr_enabled,
            });
        }

        sort_projects(&mut projects);
        Self { projects }
    }

    /// Find the project that covers a given file path (longest prefix match).
    ///
    /// Falls back to `None` if no project root is a prefix of the file path.
    pub fn find_project(&self, file_path: &str) -> Option<&ProjectConfig> {
        let normalized = file_path.replace('\\', "/");
        self.projects
            .iter()
            .find(|project| project_matches_file(project, &normalized))
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

    /// Check whether a file is in an SSR context.
    ///
    /// Returns `true` if:
    /// - The file is `*.server.vue` (always SSR regardless of project config)
    /// - The project has `ssr_enabled: true` AND the file is NOT `*.client.vue`
    pub fn is_ssr_context(&self, file_path: &str) -> bool {
        // *.server.vue files are always SSR
        if is_ssr_file(file_path) {
            return true;
        }
        // *.client.vue files are never SSR
        if is_client_only_file(file_path) {
            return false;
        }
        // Otherwise, inherit from project config
        self.find_project(file_path).is_some_and(|p| p.ssr_enabled)
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

    /// Set `conditional_root_narrowing` on all project lint configs and rebuild linters.
    pub fn set_conditional_root_narrowing(&mut self, enabled: bool) {
        for project in &mut self.projects {
            project.lint_config.config.conditional_root_narrowing = enabled;
            project.linter = verter_diagnostics::Linter::new(project.lint_config.config.clone());
        }
    }

    /// Get all project configs.
    pub fn projects(&self) -> &[ProjectConfig] {
        &self.projects
    }

    pub fn to_native_project_resolver(&self) -> crate::project_resolver::NativeProjectResolver {
        crate::project_resolver::NativeProjectResolver::new(
            self.projects
                .iter()
                .map(ProjectConfig::to_ide_project_config)
                .collect(),
        )
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

fn sort_projects(projects: &mut [ProjectConfig]) {
    projects.sort_by(|a, b| {
        b.root
            .len()
            .cmp(&a.root.len())
            .then_with(|| project_rank(a).cmp(&project_rank(b)))
            .then_with(|| a.tsconfig_path.cmp(&b.tsconfig_path))
            .then_with(|| a.root.cmp(&b.root))
    });
}

fn project_rank(project: &ProjectConfig) -> u8 {
    match project.tsconfig_path.as_deref() {
        Some(path) if path.ends_with("/tsconfig.json") => 0,
        Some(_) => 1,
        None => 2,
    }
}

fn project_matches_file(project: &ProjectConfig, file_path: &str) -> bool {
    if !path_has_prefix(file_path, &project.root) {
        return false;
    }

    match &project.membership {
        crate::project_resolver::ProjectMembership::MatchAll => true,
        crate::project_resolver::ProjectMembership::IncludeExclude {
            files,
            include,
            exclude,
        } => {
            if matches_any_pattern(file_path, exclude) {
                return false;
            }

            if files.iter().any(|candidate| candidate == file_path) {
                return true;
            }

            if !include.is_empty() {
                return matches_any_pattern(file_path, include);
            }

            !exclude.is_empty()
        }
    }
}

fn path_has_prefix(path: &str, prefix: &str) -> bool {
    path.starts_with(prefix)
        && (path.len() == prefix.len()
            || prefix.ends_with('/')
            || path.as_bytes().get(prefix.len()) == Some(&b'/'))
}

fn matches_any_pattern(path: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .filter_map(|pattern| glob::Pattern::new(pattern).ok())
        .any(|pattern| pattern.matches(path))
}

fn load_project_membership(tsconfig_path: &Path) -> crate::project_resolver::ProjectMembership {
    load_project_membership_inner(tsconfig_path, 0)
        .unwrap_or(crate::project_resolver::ProjectMembership::MatchAll)
}

fn load_compiler_options(
    tsconfig_path: &Path,
) -> crate::project_resolver::IdeProjectCompilerOptions {
    load_compiler_options_inner(tsconfig_path, 0).unwrap_or_default()
}

fn load_compiler_options_inner(
    tsconfig_path: &Path,
    depth: u8,
) -> Option<crate::project_resolver::IdeProjectCompilerOptions> {
    if depth > 8 {
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

fn load_project_references(tsconfig_path: &Path) -> Vec<String> {
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

fn load_project_membership_inner(
    tsconfig_path: &Path,
    depth: u8,
) -> Option<crate::project_resolver::ProjectMembership> {
    if depth > 8 {
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
        .unwrap_or(crate::project_resolver::ProjectMembership::MatchAll);

    let has_files = json.get("files").is_some();
    let has_include = json.get("include").is_some();
    let has_exclude = json.get("exclude").is_some();

    if !has_files && !has_include && !has_exclude {
        return Some(inherited);
    }

    let (mut files, mut include, mut exclude) = match inherited {
        crate::project_resolver::ProjectMembership::MatchAll => {
            (Vec::new(), Vec::new(), Vec::new())
        }
        crate::project_resolver::ProjectMembership::IncludeExclude {
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

    Some(crate::project_resolver::ProjectMembership::IncludeExclude {
        files,
        include,
        exclude,
    })
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

fn normalize_path_buf(path: &Path) -> String {
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

    // =====================================================================
    // Tsconfig-first policy tests (Phase 1)
    // =====================================================================

    #[test]
    fn tsconfig_backed_project_no_vite_aliases() {
        // Tsconfig-backed projects must NOT get vite aliases merged, even when
        // vite.config.ts exists alongside and vite_config_enabled is true.
        let tmp = std::env::temp_dir().join("verter_test_tsconfig_first_no_vite");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        // Create tsconfig with path aliases
        std::fs::write(
            tmp.join("tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
        )
        .unwrap();

        // Create vite config with different alias
        std::fs::write(
            tmp.join("vite.config.js"),
            "export default { resolve: { alias: { '~': './lib' } } };",
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let registry = ProjectRegistry::from_workspace_roots(
            &[root.clone()],
            &crate::vite_config::ViteConfigOptions {
                enabled: true,
                trusted_files: Vec::new(),
                node_path: Some("node".to_string()),
            },
        )
        .registry;

        // Find tsconfig-backed project
        let file = format!("{root}/src/App.vue");
        let project = registry.find_project(&file);
        assert!(project.is_some(), "should find tsconfig-backed project");
        let project = project.unwrap();
        assert!(
            project.tsconfig_path.is_some(),
            "project should have tsconfig_path"
        );

        // Positive: tsconfig aliases present
        assert!(
            !project.path_resolver.is_empty(),
            "tsconfig aliases should be present"
        );

        // Negative: no vite aliases in workspace_aliases
        assert!(
            project.workspace_aliases.is_empty(),
            "tsconfig-backed project must have empty workspace_aliases, got {} entries",
            project.workspace_aliases.len()
        );

        // Negative: path resolver should NOT contain vite alias prefix "~/"
        // (Only tsconfig paths: "@/*")
        let has_tilde = project.path_resolver.resolve("~/foo").is_some();
        assert!(
            !has_tilde,
            "tsconfig-backed project must not have vite alias ~/",
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn tsconfig_backed_project_resolver_only_tsconfig_paths() {
        // Verify that a tsconfig-backed project's resolver contains ONLY tsconfig
        // paths and no vite alias prefixes.
        let tmp = std::env::temp_dir().join("verter_test_tsconfig_only_paths");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();
        std::fs::create_dir_all(tmp.join("lib")).unwrap();

        std::fs::write(
            tmp.join("tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
        )
        .unwrap();

        // Vite config with both @ and ~ aliases
        std::fs::write(
            tmp.join("vite.config.js"),
            &format!(
                "export default {{ resolve: {{ alias: {{ '@': '{src}', '~': '{lib}' }} }} }};",
                src = tmp.join("src").to_string_lossy().replace('\\', "/"),
                lib = tmp.join("lib").to_string_lossy().replace('\\', "/"),
            ),
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let registry = ProjectRegistry::from_workspace_roots(
            &[root.clone()],
            &crate::vite_config::ViteConfigOptions {
                enabled: true,
                trusted_files: Vec::new(),
                node_path: Some("node".to_string()),
            },
        )
        .registry;

        let file = format!("{root}/src/App.vue");
        let project = registry.find_project(&file).unwrap();

        // tsconfig has @/* — that should exist
        assert!(
            !project.path_resolver.is_empty(),
            "path resolver should have tsconfig aliases"
        );

        // Negative: vite's ~ alias must NOT be in the resolver
        // (even though vite config defines it)
        let has_tilde = project.path_resolver.resolve("~/something").is_some();
        assert!(
            !has_tilde,
            "vite ~ alias must not leak into tsconfig-backed resolver",
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // =====================================================================
    // Phase 3: Fallback project vite alias wiring tests
    // =====================================================================

    #[test]
    fn fallback_project_static_vite_aliases() {
        // Fallback (no tsconfig) project with static-analyzable vite config
        // should get aliases populated.
        let tmp = std::env::temp_dir().join("verter_test_fallback_static_vite");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        // No tsconfig.json — this will be a fallback project
        std::fs::write(
            tmp.join("vite.config.js"),
            "export default { resolve: { alias: { '@': './src' } } };",
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let build_result = ProjectRegistry::from_workspace_roots(
            &[root.clone()],
            &crate::vite_config::ViteConfigOptions {
                enabled: true,
                trusted_files: Vec::new(),
                node_path: None,
            },
        );

        let file = format!("{root}/src/App.vue");
        let project = build_result.registry.find_project(&file);
        assert!(project.is_some(), "should find fallback project");
        let project = project.unwrap();

        // Positive: fallback project has no tsconfig
        assert!(
            project.tsconfig_path.is_none(),
            "should be a fallback project"
        );

        // Positive: should have vite aliases
        assert!(
            !project.workspace_aliases.is_empty(),
            "fallback project should have vite aliases"
        );

        // Positive: vite_config_path and vite_config_deps should be set
        assert!(
            project.vite_config_path.is_some(),
            "vite_config_path should be set"
        );
        assert!(
            !project.vite_config_deps.is_empty(),
            "vite_config_deps should include the config file"
        );

        // Negative: no trust_required entries
        assert!(
            build_result.trust_required.is_empty(),
            "static analysis should not require trust"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fallback_project_complex_config_not_trusted() {
        // Fallback project with complex (function export) vite config and no trust
        // should have empty aliases and generate trust_required entry.
        let tmp = std::env::temp_dir().join("verter_test_fallback_complex_notrusted");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        std::fs::write(
            tmp.join("vite.config.ts"),
            r#"import { defineConfig } from 'vite'
export default defineConfig(({ mode }) => ({
  resolve: { alias: { '@': './src' } }
}))"#,
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let build_result = ProjectRegistry::from_workspace_roots(
            &[root.clone()],
            &crate::vite_config::ViteConfigOptions {
                enabled: true,
                trusted_files: Vec::new(),
                node_path: None,
            },
        );

        let file = format!("{root}/src/App.vue");
        let project = build_result.registry.find_project(&file).unwrap();

        // Negative: no aliases when not trusted
        assert!(
            project.workspace_aliases.is_empty(),
            "untrusted complex config should have empty aliases"
        );

        // Positive: trust_required should have an entry
        assert_eq!(
            build_result.trust_required.len(),
            1,
            "should have 1 trust_required entry"
        );
        assert!(
            build_result.trust_required[0].reason.contains("function")
                || build_result.trust_required[0].reason.contains("arrow"),
            "reason should mention function/arrow: {}",
            build_result.trust_required[0].reason
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn tsconfig_project_no_vite_config_path() {
        // Tsconfig-backed projects should never have vite_config_path set.
        let tmp = std::env::temp_dir().join("verter_test_tsconfig_no_vite_path");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        std::fs::write(
            tmp.join("tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("vite.config.js"),
            "export default { resolve: { alias: { '@': './src' } } };",
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let build_result = ProjectRegistry::from_workspace_roots(
            &[root.clone()],
            &crate::vite_config::ViteConfigOptions {
                enabled: true,
                trusted_files: Vec::new(),
                node_path: None,
            },
        );

        let file = format!("{root}/src/App.vue");
        let project = build_result.registry.find_project(&file).unwrap();

        // Positive: tsconfig project
        assert!(project.tsconfig_path.is_some());

        // Negative: no vite_config_path on tsconfig project
        assert!(
            project.vite_config_path.is_none(),
            "tsconfig-backed project must not have vite_config_path"
        );
        assert!(
            project.vite_config_deps.is_empty(),
            "tsconfig-backed project must not have vite_config_deps"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn disabled_vite_fallback_has_no_aliases() {
        // When vite is disabled, fallback projects should have empty aliases.
        let tmp = std::env::temp_dir().join("verter_test_disabled_vite");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        std::fs::write(
            tmp.join("vite.config.js"),
            "export default { resolve: { alias: { '@': './src' } } };",
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let build_result = ProjectRegistry::from_workspace_roots(
            &[root.clone()],
            &crate::vite_config::ViteConfigOptions {
                enabled: false,
                trusted_files: Vec::new(),
                node_path: None,
            },
        );

        let file = format!("{root}/src/App.vue");
        let project = build_result.registry.find_project(&file).unwrap();

        assert!(
            project.workspace_aliases.is_empty(),
            "disabled vite should mean empty aliases"
        );
        assert!(
            project.vite_config_path.is_none(),
            "disabled vite should not set vite_config_path"
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
                workspace_root: "/workspace".to_string(),
                tsconfig_path: None,
                membership: crate::project_resolver::ProjectMembership::MatchAll,
                workspace_aliases: Vec::new(),
                compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
                references: Vec::new(),
                path_resolver: TsConfigPathResolver::default(),
                lint_config: ResolvedLintConfig::default(),
                linter: verter_diagnostics::Linter::default(),
                lint_explicitly_configured: false,
                vite_config_path: None,
                vite_config_deps: Vec::new(),
                ssr_enabled: false,
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
                    workspace_root: "/workspace".to_string(),
                    tsconfig_path: None,
                    membership: crate::project_resolver::ProjectMembership::MatchAll,
                    workspace_aliases: Vec::new(),
                    compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
                    references: Vec::new(),
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
                    vite_config_path: None,
                    vite_config_deps: Vec::new(),
                    ssr_enabled: false,
                },
                ProjectConfig {
                    root: "/workspace/default/".to_string(),
                    workspace_root: "/workspace".to_string(),
                    tsconfig_path: None,
                    membership: crate::project_resolver::ProjectMembership::MatchAll,
                    workspace_aliases: Vec::new(),
                    compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
                    references: Vec::new(),
                    path_resolver: TsConfigPathResolver::default(),
                    lint_config: ResolvedLintConfig::default(),
                    linter: verter_diagnostics::Linter::default(),
                    lint_explicitly_configured: false,
                    vite_config_path: None,
                    vite_config_deps: Vec::new(),
                    ssr_enabled: false,
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

    #[test]
    fn registry_solution_style_root_does_not_own_member_files() {
        let tmp = std::env::temp_dir().join("verter_test_registry_solution_owner");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();
        std::fs::create_dir_all(tmp.join("tests")).unwrap();

        std::fs::write(
            tmp.join("tsconfig.json"),
            r#"{
  "files": [],
  "references": [
    { "path": "./tsconfig.app.json" },
    { "path": "./tsconfig.vitest.json" }
  ]
}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("tsconfig.app.json"),
            r#"{
  "include": ["src/**/*"],
  "exclude": ["tests/**/*"]
}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("tsconfig.vitest.json"),
            r#"{
  "include": ["tests/**/*"]
}"#,
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let registry = ProjectRegistry::from_canonical_roots(&[&root]);
        let source_file = format!("{root}/src/App.vue");
        let expected_app = format!("{root}/tsconfig.app.json");
        let solution_root = format!("{root}/tsconfig.json");

        let project = registry
            .find_project(&source_file)
            .expect("solution-style workspace should still find an owner");

        assert_eq!(
            project.tsconfig_path.as_deref(),
            Some(expected_app.as_str()),
            "src/App.vue should be owned by tsconfig.app.json, not the solution tsconfig"
        );
        assert_ne!(
            project.tsconfig_path.as_deref(),
            Some(solution_root.as_str()),
            "solution-style tsconfig.json must not claim files outside its membership"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn registry_unmatched_root_file_uses_synthetic_workspace_project() {
        let tmp = std::env::temp_dir().join("verter_test_registry_synth_workspace");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();
        std::fs::create_dir_all(tmp.join("scripts")).unwrap();

        std::fs::write(
            tmp.join("tsconfig.app.json"),
            r#"{
  "include": ["src/**/*"]
}"#,
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let registry = ProjectRegistry::from_canonical_roots(&[&root]);
        let unmatched = format!("{root}/scripts/tool.ts");

        let project = registry
            .find_project(&unmatched)
            .expect("unmatched file should fall back to a synthetic workspace project");

        assert_eq!(
            project.tsconfig_path, None,
            "scripts/tool.ts should not be assigned to tsconfig.app.json"
        );
        assert_eq!(
            project.root, root,
            "synthetic workspace project should use the workspace root"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn registry_projects_preserve_compiler_options_through_extends() {
        let tmp = std::env::temp_dir().join("verter_test_registry_compiler_options");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        std::fs::write(
            tmp.join("tsconfig.base.json"),
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
        std::fs::write(
            tmp.join("tsconfig.app.json"),
            r#"{
  "extends": "./tsconfig.base.json",
  "include": ["src/**/*"]
}"#,
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let registry = ProjectRegistry::from_canonical_roots(&[&root]);
        let app_file = format!("{root}/src/App.ts");
        let project = registry
            .find_project(&app_file)
            .expect("extended tsconfig project should own src/App.ts");

        assert_eq!(
            project.compiler_options.base_url.as_deref(),
            Some(root.as_str()),
            "baseUrl should be resolved to an absolute canonical path"
        );
        assert_eq!(
            project.compiler_options.paths,
            vec![("@/*".to_string(), vec![format!("{root}/src/*")])],
            "tsconfig paths should be preserved on the project for native resolution"
        );

        let _ = std::fs::remove_dir_all(&tmp);
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

    // ── SSR Detection Tests ──────────────────────────────────────────────

    #[test]
    fn detect_ssr_nuxt_config_ts() {
        let tmp = std::env::temp_dir().join("verter_test_ssr_nuxt");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("nuxt.config.ts"), "export default {}").unwrap();
        let lint = ResolvedLintConfig::default();
        assert!(
            detect_ssr_project(&tmp, &lint),
            "should detect nuxt.config.ts"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_ssr_nuxt_config_js() {
        let tmp = std::env::temp_dir().join("verter_test_ssr_nuxt_js");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("nuxt.config.js"), "export default {}").unwrap();
        let lint = ResolvedLintConfig::default();
        assert!(
            detect_ssr_project(&tmp, &lint),
            "should detect nuxt.config.js"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_ssr_nuxt_dir() {
        let tmp = std::env::temp_dir().join("verter_test_ssr_nuxt_dir");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::create_dir_all(tmp.join(".nuxt")).unwrap();
        let lint = ResolvedLintConfig::default();
        assert!(detect_ssr_project(&tmp, &lint), "should detect .nuxt dir");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_ssr_from_lint_config() {
        let tmp = std::env::temp_dir().join("verter_test_ssr_lint");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let lint = ResolvedLintConfig {
            config: verter_diagnostics::LintConfig {
                ssr_mode: true,
                ..Default::default()
            },
            explicitly_configured: true,
        };
        assert!(
            detect_ssr_project(&tmp, &lint),
            "should detect ssr_mode from lint config"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn no_ssr_for_plain_vite_project() {
        let tmp = std::env::temp_dir().join("verter_test_no_ssr");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("vite.config.ts"), "export default {}").unwrap();
        let lint = ResolvedLintConfig::default();
        assert!(
            !detect_ssr_project(&tmp, &lint),
            "plain vite project should not be SSR"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn is_ssr_file_detection() {
        assert!(is_ssr_file("MyComp.server.vue"));
        assert!(is_ssr_file("/path/to/MyComp.server.vue"));
        assert!(!is_ssr_file("MyComp.vue"));
        assert!(!is_ssr_file("MyComp.client.vue"));
    }

    #[test]
    fn is_client_only_file_detection() {
        assert!(is_client_only_file("MyComp.client.vue"));
        assert!(is_client_only_file("/path/to/MyComp.client.vue"));
        assert!(!is_client_only_file("MyComp.vue"));
        assert!(!is_client_only_file("MyComp.server.vue"));
    }

    #[test]
    fn registry_is_ssr_context() {
        let registry = ProjectRegistry {
            projects: vec![ProjectConfig {
                root: "/workspace/app".to_string(),
                workspace_root: "/workspace".to_string(),
                tsconfig_path: None,
                membership: crate::project_resolver::ProjectMembership::MatchAll,
                workspace_aliases: Vec::new(),
                compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
                references: Vec::new(),
                path_resolver: TsConfigPathResolver::default(),
                lint_config: ResolvedLintConfig::default(),
                linter: verter_diagnostics::Linter::default(),
                lint_explicitly_configured: false,
                vite_config_path: None,
                vite_config_deps: Vec::new(),
                ssr_enabled: true,
            }],
        };

        // Regular .vue in SSR project → SSR context
        assert!(registry.is_ssr_context("/workspace/app/src/Comp.vue"));
        // *.server.vue → always SSR
        assert!(registry.is_ssr_context("/workspace/app/src/Comp.server.vue"));
        // *.client.vue → never SSR even in SSR project
        assert!(!registry.is_ssr_context("/workspace/app/src/Comp.client.vue"));
        // File outside project → not SSR
        assert!(!registry.is_ssr_context("/other/Comp.vue"));
    }

    /// Resolver follows Nuxt's `extends: "./.nuxt/tsconfig.json"` chain
    /// and picks up #-prefixed path aliases from the generated tsconfig.
    #[test]
    fn test_path_resolver_nuxt_extends_hash_aliases() {
        let tmp_dir = std::env::temp_dir().join("verter_test_nuxt_extends");
        let _ = std::fs::remove_dir_all(&tmp_dir);

        // Create .nuxt directory with types subdirectory
        let nuxt_dir = tmp_dir.join(".nuxt");
        std::fs::create_dir_all(&nuxt_dir).unwrap();

        // Create target files
        let types_dir = nuxt_dir.join("types");
        std::fs::create_dir_all(&types_dir).unwrap();
        std::fs::write(types_dir.join("imports.d.ts"), "export {}").unwrap();

        let ui_dir = tmp_dir
            .join("node_modules")
            .join("@nuxt")
            .join("ui")
            .join("runtime");
        std::fs::create_dir_all(&ui_dir).unwrap();
        let ui_components = ui_dir.join("components");
        std::fs::create_dir_all(&ui_components).unwrap();
        std::fs::write(
            ui_components.join("Button.vue"),
            "<template><button/></template>",
        )
        .unwrap();

        let shared_dir = tmp_dir.join("shared");
        std::fs::create_dir_all(&shared_dir).unwrap();
        std::fs::write(shared_dir.join("utils.ts"), "export const x = 1;").unwrap();

        // .nuxt/tsconfig.json with Nuxt-generated #-prefixed aliases
        let ui_abs = tmp_dir
            .join("node_modules/@nuxt/ui")
            .to_string_lossy()
            .replace('\\', "/");
        let nuxt_tsconfig = format!(
            "{{\n  \"compilerOptions\": {{\n    \"baseUrl\": \"..\",\n    \"paths\": {{\n      \"#imports\": [\"./.nuxt/types/imports.d.ts\"],\n      \"#ui/{star}\": [\"{ui_abs}/runtime/{star}\"],\n      \"#shared/{star}\": [\"./shared/{star}\"]\n    }}\n  }}\n}}",
            star = "*",
            ui_abs = ui_abs,
        );
        std::fs::write(nuxt_dir.join("tsconfig.json"), &nuxt_tsconfig).unwrap();

        // Root tsconfig.json extending .nuxt/tsconfig.json
        std::fs::write(
            tmp_dir.join("tsconfig.json"),
            r#"{ "extends": "./.nuxt/tsconfig.json" }"#,
        )
        .unwrap();

        let resolver = TsConfigPathResolver::from_tsconfig(&tmp_dir.join("tsconfig.json"));
        assert!(
            !resolver.is_empty(),
            "resolver should have aliases from .nuxt/tsconfig.json"
        );

        // Exact match: #imports
        let result = resolver.resolve("#imports");
        assert!(
            result.is_some(),
            "should resolve #imports; aliases: {:?}",
            resolver.aliases
        );

        // Wildcard match: #ui/components/Button.vue
        let result2 = resolver.resolve("#ui/components/Button.vue");
        assert!(
            result2.is_some(),
            "should resolve #ui/components/Button.vue; aliases: {:?}",
            resolver.aliases
        );

        // Wildcard match: #shared/utils
        let result3 = resolver.resolve("#shared/utils");
        assert!(
            result3.is_some(),
            "should resolve #shared/utils; aliases: {:?}",
            resolver.aliases
        );

        // Negative: unknown alias
        let result4 = resolver.resolve("#nonexistent");
        assert!(result4.is_none(), "#nonexistent should not resolve");

        // Negative: unknown sub-path
        let result5 = resolver.resolve("#ui/nonexistent/Nope");
        assert!(result5.is_none(), "#ui/nonexistent/Nope should not resolve");

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn verterrc_ssr_config_roundtrip() {
        let json = r#"{"lint":{"enabled":true},"ssr":{"enabled":true}}"#;
        let config: verter_diagnostics::VerterProjectConfig = serde_json::from_str(json).unwrap();
        assert!(config.ssr.unwrap().enabled.unwrap());
    }
}

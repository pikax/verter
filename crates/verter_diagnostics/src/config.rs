//! Lint configuration: presets, per-rule severity overrides.

use rustc_hash::FxHashMap;

use crate::diagnostic::Severity;

/// Lint configuration controlling which rules are active and at what severity.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LintConfig {
    /// Active preset.
    #[serde(default)]
    pub preset: LintPreset,
    /// Per-rule severity overrides. Keys are rule names (e.g., `"no-v-html"`).
    /// `None` disables the rule.
    #[serde(default)]
    pub rules: FxHashMap<String, Option<Severity>>,
    /// Vapor mode: enables Vapor-specific rules, adjusts others.
    #[serde(default)]
    pub vapor_mode: bool,
    /// SSR mode: enables SSR-specific rules.
    #[serde(default)]
    pub ssr_mode: bool,
    /// File patterns to ignore (glob syntax). Populated from `.verterrc.json` `ignore` field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_patterns: Vec<String>,
    /// Experimental: conditional root narrowing enabled.
    /// When true, enables the `conditional-root-complex` diagnostic.
    #[serde(default)]
    pub conditional_root_narrowing: bool,
}

impl Default for LintConfig {
    fn default() -> Self {
        Self {
            preset: LintPreset::Recommended,
            rules: FxHashMap::default(),
            vapor_mode: false,
            ssr_mode: false,
            conditional_root_narrowing: false,
            ignore_patterns: Vec::new(),
        }
    }
}

/// Lint preset controlling which rule sets are active.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum LintPreset {
    /// Error prevention only (Vue essential rules).
    Essential,
    /// Essential + readability (Vue recommended rules).
    #[default]
    Recommended,
    /// Everything enabled.
    All,
    /// Essential + performance rules.
    Performance,
    /// Essential + accessibility rules.
    A11y,
    /// All rules at error severity.
    Strict,
}

impl LintConfig {
    /// Returns the effective severity for a rule, considering the preset and overrides.
    /// Returns `None` if the rule is disabled.
    ///
    /// `default` is the rule's `default_severity()`: `Some(severity)` for rules
    /// that are on by default, `None` for opt-in rules that require explicit config.
    pub fn effective_severity(
        &self,
        rule_name: &str,
        default: Option<Severity>,
    ) -> Option<Severity> {
        // Check per-rule overrides first
        if let Some(override_sev) = self.rules.get(rule_name) {
            return *override_sev;
        }
        // Strict preset promotes everything to Error (including opt-in rules)
        if self.preset == LintPreset::Strict {
            return Some(Severity::Error);
        }
        // None default = opt-in rule, disabled unless explicitly enabled above
        default
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// JSON Comment / Trailing Comma Stripping
// ═══════════════════════════════════════════════════════════════════════════

/// Strip single-line (`//`) and multi-line (`/* */`) comments from JSON text.
/// tsconfig.json supports JSONC (JSON with Comments).
///
/// Uses byte-index slicing into the original `&str` to preserve valid UTF-8
/// (comments and delimiters are always ASCII, so byte scanning is safe).
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
pub fn strip_trailing_commas(input: &str) -> String {
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

// ═══════════════════════════════════════════════════════════════════════════
// Project Lint Configuration (.verterrc.json + ESLint migration)
// ═══════════════════════════════════════════════════════════════════════════

/// Project-level lint configuration read from `.verterrc.json`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerterProjectConfig {
    pub lint: Option<ProjectLintConfig>,
    /// File patterns to ignore from linting.
    pub ignore: Option<Vec<String>>,
    /// SSR configuration.
    pub ssr: Option<ProjectSsrConfig>,
}

/// SSR section of `.verterrc.json`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSsrConfig {
    /// Whether SSR mode is enabled. When `true`, SSR-specific lint rules fire
    /// and the LSP shows SSR-aware warnings/completions.
    pub enabled: Option<bool>,
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
    pub config: LintConfig,
}

/// Discover and load project lint configuration.
///
/// Priority: `.verterrc.json` > eslint config
pub fn discover_lint_config(workspace_root: &std::path::Path) -> ResolvedLintConfig {
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
fn load_verterrc(workspace_root: &std::path::Path) -> Option<ResolvedLintConfig> {
    let config_path = workspace_root.join(".verterrc.json");
    let content = std::fs::read_to_string(&config_path).ok()?;
    let cleaned = strip_json_comments(&content);
    let project_config: VerterProjectConfig = serde_json::from_str(&cleaned).ok()?;

    let lint = project_config.lint?;
    let mut config = LintConfig::default();

    // Apply preset
    if let Some(preset_str) = &lint.preset {
        config.preset = match preset_str.as_str() {
            "essential" => LintPreset::Essential,
            "recommended" => LintPreset::Recommended,
            "all" => LintPreset::All,
            "performance" => LintPreset::Performance,
            "a11y" => LintPreset::A11y,
            "strict" => LintPreset::Strict,
            _ => LintPreset::Recommended,
        };
    }

    // Apply per-rule overrides
    if let Some(rules) = &lint.rules {
        for (name, value) in rules {
            let severity = parse_rule_severity(value);
            config.rules.insert(name.clone(), severity);
        }
    }

    // Apply ignore patterns
    if let Some(ignore) = project_config.ignore {
        config.ignore_patterns = ignore;
    }

    // Apply SSR mode from config
    if let Some(ssr) = &project_config.ssr {
        if ssr.enabled.unwrap_or(false) {
            config.ssr_mode = true;
        }
    }

    let enabled = lint.enabled.unwrap_or(true);

    Some(ResolvedLintConfig {
        explicitly_configured: enabled,
        config,
    })
}

/// Load and migrate eslint-plugin-vue config.
fn load_eslint_config(workspace_root: &std::path::Path) -> Option<ResolvedLintConfig> {
    // Try .eslintrc.json first, then package.json
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

    let mut config = LintConfig::default();

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
                    config.preset = LintPreset::Essential;
                }
                "plugin:vue/vue3-strongly-recommended" | "plugin:vue/strongly-recommended" => {
                    config.preset = LintPreset::Recommended;
                }
                "plugin:vue/vue3-recommended" | "plugin:vue/recommended" => {
                    config.preset = LintPreset::Recommended;
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
pub fn parse_rule_severity(value: &serde_json::Value) -> Option<Severity> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_recommended() {
        let config = LintConfig::default();
        assert_eq!(config.preset, LintPreset::Recommended);
        assert!(!config.vapor_mode);
        assert!(!config.ssr_mode);
    }

    #[test]
    fn override_disables_rule() {
        let mut config = LintConfig::default();
        config.rules.insert("no-v-html".to_string(), None);
        assert_eq!(
            config.effective_severity("no-v-html", Some(Severity::Warning)),
            None
        );
    }

    #[test]
    fn override_changes_severity() {
        let mut config = LintConfig::default();
        config
            .rules
            .insert("no-v-html".to_string(), Some(Severity::Error));
        assert_eq!(
            config.effective_severity("no-v-html", Some(Severity::Warning)),
            Some(Severity::Error)
        );
    }

    #[test]
    fn strict_preset_promotes_to_error() {
        let config = LintConfig {
            preset: LintPreset::Strict,
            ..Default::default()
        };
        assert_eq!(
            config.effective_severity("any-rule", Some(Severity::Warning)),
            Some(Severity::Error)
        );
    }

    #[test]
    fn opt_in_rule_is_disabled_without_override() {
        let config = LintConfig::default();
        assert_eq!(
            config.effective_severity("require-typed-ref", None),
            None,
            "opt-in rules (None default) should be disabled without explicit config"
        );
    }

    #[test]
    fn opt_in_rule_can_be_enabled_via_override() {
        let mut config = LintConfig::default();
        config
            .rules
            .insert("require-typed-ref".to_string(), Some(Severity::Warning));
        assert_eq!(
            config.effective_severity("require-typed-ref", None),
            Some(Severity::Warning),
            "opt-in rules should be enabled when explicitly configured"
        );
    }

    #[test]
    fn opt_in_rule_enabled_by_strict_preset() {
        let config = LintConfig {
            preset: LintPreset::Strict,
            ..Default::default()
        };
        assert_eq!(
            config.effective_severity("require-typed-ref", None),
            Some(Severity::Error),
            "Strict preset should enable opt-in rules at Error severity"
        );
    }

    #[test]
    fn config_serde_roundtrip() {
        let config = LintConfig {
            preset: LintPreset::A11y,
            vapor_mode: true,
            ssr_mode: false,
            conditional_root_narrowing: false,
            ignore_patterns: vec![],
            rules: {
                let mut m = FxHashMap::default();
                m.insert("no-v-html".to_string(), Some(Severity::Error));
                m.insert("no-inline-style".to_string(), None);
                m
            },
        };

        let json = serde_json::to_string(&config).expect("serialize");
        let roundtrip: LintConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(roundtrip.preset, LintPreset::A11y);
        assert!(roundtrip.vapor_mode);
        assert_eq!(roundtrip.rules.len(), 2);
        assert!(roundtrip.ignore_patterns.is_empty());
    }

    #[test]
    fn ignore_patterns_serde_roundtrip() {
        let config = LintConfig {
            ignore_patterns: vec!["src/generated/**".to_string(), "*.test.vue".to_string()],
            ..Default::default()
        };
        let json = serde_json::to_string(&config).expect("serialize");
        assert!(json.contains("ignorePatterns"));
        let roundtrip: LintConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(roundtrip.ignore_patterns.len(), 2);
        assert_eq!(roundtrip.ignore_patterns[0], "src/generated/**");
    }

    #[test]
    fn ignore_patterns_omitted_when_empty() {
        let config = LintConfig::default();
        let json = serde_json::to_string(&config).expect("serialize");
        assert!(
            !json.contains("ignorePatterns"),
            "empty ignore_patterns should be omitted"
        );
    }

    // ── Config discovery tests ──────────────────────────────────────────

    #[test]
    fn strip_json_comments_removes_line_comments() {
        let input = r#"{ "key": "value" // comment
}"#;
        let result = strip_json_comments(input);
        assert!(result.contains(r#""key": "value""#));
        assert!(
            !result.contains("// comment"),
            "line comment must be stripped"
        );
    }

    #[test]
    fn strip_json_comments_removes_block_comments() {
        let input = r#"{ /* block */ "key": "value" }"#;
        let result = strip_json_comments(input);
        assert!(result.contains(r#""key": "value""#));
        assert!(
            !result.contains("/* block */"),
            "block comment must be stripped"
        );
    }

    #[test]
    fn strip_json_comments_preserves_strings() {
        let input = r#"{ "key": "value // not a comment" }"#;
        let result = strip_json_comments(input);
        assert!(
            result.contains("value // not a comment"),
            "comment-like content in strings must be preserved"
        );
    }

    #[test]
    fn strip_trailing_commas_removes_trailing() {
        let input = r#"{ "a": 1, "b": 2, }"#;
        let result = strip_trailing_commas(input);
        assert!(!result.ends_with(", }"), "trailing comma must be removed");
        // Should be parseable JSON now
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["a"], 1);
        assert_eq!(parsed["b"], 2);
    }

    #[test]
    fn parse_rule_severity_strings() {
        assert_eq!(parse_rule_severity(&serde_json::json!("off")), None);
        assert_eq!(
            parse_rule_severity(&serde_json::json!("warn")),
            Some(Severity::Warning)
        );
        assert_eq!(
            parse_rule_severity(&serde_json::json!("error")),
            Some(Severity::Error)
        );
    }

    #[test]
    fn parse_rule_severity_numbers() {
        assert_eq!(parse_rule_severity(&serde_json::json!(0)), None);
        assert_eq!(
            parse_rule_severity(&serde_json::json!(1)),
            Some(Severity::Warning)
        );
        assert_eq!(
            parse_rule_severity(&serde_json::json!(2)),
            Some(Severity::Error)
        );
    }

    #[test]
    fn parse_rule_severity_array_form() {
        assert_eq!(
            parse_rule_severity(&serde_json::json!(["error", {}])),
            Some(Severity::Error)
        );
        assert_eq!(parse_rule_severity(&serde_json::json!(["off"])), None);
    }

    #[test]
    fn discover_lint_config_no_config_returns_default() {
        let tmp = std::env::temp_dir().join("verter_diag_test_no_config");
        let _ = std::fs::create_dir_all(&tmp);
        let result = discover_lint_config(&tmp);
        assert!(
            !result.explicitly_configured,
            "should not be explicitly configured"
        );
        assert_eq!(result.config.preset, LintPreset::Recommended);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_lint_config_verterrc() {
        let tmp = std::env::temp_dir().join("verter_diag_test_verterrc");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(
            tmp.join(".verterrc.json"),
            r#"{"lint":{"preset":"essential","rules":{"no-v-html":"off"}}}"#,
        )
        .unwrap();
        let result = discover_lint_config(&tmp);
        assert!(result.explicitly_configured);
        assert_eq!(result.config.preset, LintPreset::Essential);
        assert_eq!(result.config.rules.get("no-v-html"), Some(&None));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_lint_config_eslintrc() {
        let tmp = std::env::temp_dir().join("verter_diag_test_eslintrc");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(
            tmp.join(".eslintrc.json"),
            r#"{"extends":["plugin:vue/vue3-essential"],"rules":{"vue/no-v-html":"error"}}"#,
        )
        .unwrap();
        let result = discover_lint_config(&tmp);
        assert!(result.explicitly_configured);
        assert_eq!(result.config.preset, LintPreset::Essential);
        assert_eq!(
            result.config.rules.get("no-v-html"),
            Some(&Some(Severity::Error))
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_lint_config_ignore_patterns_from_verterrc() {
        let tmp = std::env::temp_dir().join("verter_diag_test_ignore");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(
            tmp.join(".verterrc.json"),
            r#"{"lint":{"preset":"recommended"},"ignore":["src/generated/**","*.test.vue"]}"#,
        )
        .unwrap();
        let result = discover_lint_config(&tmp);
        assert!(result.explicitly_configured);
        assert_eq!(result.config.ignore_patterns.len(), 2);
        assert_eq!(result.config.ignore_patterns[0], "src/generated/**");
        assert_eq!(result.config.ignore_patterns[1], "*.test.vue");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

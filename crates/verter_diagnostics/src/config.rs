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
    pub fn effective_severity(&self, rule_name: &str, default: Severity) -> Option<Severity> {
        // Check per-rule overrides first
        if let Some(override_sev) = self.rules.get(rule_name) {
            return *override_sev;
        }
        // Strict preset promotes everything to Error
        if self.preset == LintPreset::Strict {
            return Some(Severity::Error);
        }
        Some(default)
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
            config.effective_severity("no-v-html", Severity::Warning),
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
            config.effective_severity("no-v-html", Severity::Warning),
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
            config.effective_severity("any-rule", Severity::Warning),
            Some(Severity::Error)
        );
    }

    #[test]
    fn config_serde_roundtrip() {
        let config = LintConfig {
            preset: LintPreset::A11y,
            vapor_mode: true,
            ssr_mode: false,
            conditional_root_narrowing: false,
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
    }
}

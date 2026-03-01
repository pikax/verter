//! Diagnostic tool helpers.

use verter_diagnostics::{LintConfig, LintPreset};

/// Create a LintConfig from a preset name string.
pub fn make_lint_config(preset: &str) -> LintConfig {
    let preset = match preset.to_lowercase().as_str() {
        "essential" => LintPreset::Essential,
        "recommended" => LintPreset::Recommended,
        "all" => LintPreset::All,
        "performance" => LintPreset::Performance,
        "a11y" => LintPreset::A11y,
        "strict" => LintPreset::Strict,
        _ => LintPreset::Recommended,
    };
    LintConfig {
        preset,
        ..Default::default()
    }
}

//! # no-duplicate-vue
//!
//! Error when multiple Vue installations are detected in `node_modules`.
//! Duplicate Vue packages cause broken reactivity, failed `instanceof` checks,
//! and subtle runtime bugs that are extremely hard to diagnose.
//!
//! ## Example diagnostic
//! ```text
//! Multiple Vue installations detected (2 copies). This causes broken
//! reactivity and failed instanceof checks.
//!   - vue@3.4.21 at node_modules/vue
//!   - vue@3.3.4 at node_modules/some-lib/node_modules/vue
//! Run `npm ls vue` to investigate.
//! ```
//!
//! ## Data source
//! The `duplicate_vue_versions` field on [`CrossFileSnapshot`] is populated by
//! the caller (host, LSP, or build tool) via filesystem scanning of
//! `node_modules`. The linter rule only consumes the pre-computed data.

use crate::context::LintContext;
use crate::cross_file::CrossFileSnapshot;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};

/// Lint rule: error when multiple Vue installations are detected.
pub struct NoDuplicateVue;

impl LintRule for NoDuplicateVue {
    fn name(&self) -> &'static str {
        "no-duplicate-vue"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::CrossFile
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_cross_file(&self, snapshot: &CrossFileSnapshot, ctx: &mut LintContext) {
        let versions = &snapshot.duplicate_vue_versions;

        // Only report when there are 2+ installations
        if versions.len() < 2 {
            return;
        }

        let details: Vec<String> = versions
            .iter()
            .map(|v| format!("  - vue@{} at {}", v.version, v.path))
            .collect();

        ctx.report_with_severity(
            self.name(),
            self.category().as_str(),
            format!(
                "Multiple Vue installations detected ({} copies). This causes broken \
                 reactivity and failed `instanceof` checks.\n{}\n\
                 Run `npm ls vue` to investigate.",
                versions.len(),
                details.join("\n"),
            ),
            0,
            0,
            self.default_severity(),
            DiagnosticSpanKind::FileLevel,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::cross_file::DuplicateVueEntry;

    fn run_rule(snapshot: &CrossFileSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_cross_file_rule(NoDuplicateVue, snapshot)
    }

    #[test]
    fn no_duplicates_no_diagnostic() {
        let snapshot = CrossFileSnapshot::default();
        assert!(run_rule(&snapshot).is_empty());
    }

    #[test]
    fn single_installation_no_diagnostic() {
        let snapshot = CrossFileSnapshot {
            duplicate_vue_versions: vec![DuplicateVueEntry {
                path: "node_modules/vue".to_string(),
                version: "3.4.21".to_string(),
            }],
            ..Default::default()
        };
        assert!(run_rule(&snapshot).is_empty());
    }

    #[test]
    fn two_installations_reports_error() {
        let snapshot = CrossFileSnapshot {
            duplicate_vue_versions: vec![
                DuplicateVueEntry {
                    path: "node_modules/vue".to_string(),
                    version: "3.4.21".to_string(),
                },
                DuplicateVueEntry {
                    path: "node_modules/some-lib/node_modules/vue".to_string(),
                    version: "3.3.4".to_string(),
                },
            ],
            ..Default::default()
        };
        let diags = run_rule(&snapshot);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "no-duplicate-vue");
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(diags[0].message.contains("2 copies"));
        assert!(diags[0].message.contains("vue@3.4.21"));
        assert!(diags[0].message.contains("vue@3.3.4"));
        assert!(diags[0].message.contains("npm ls vue"));
    }

    #[test]
    fn three_installations_reports_all() {
        let snapshot = CrossFileSnapshot {
            duplicate_vue_versions: vec![
                DuplicateVueEntry {
                    path: "node_modules/vue".to_string(),
                    version: "3.4.21".to_string(),
                },
                DuplicateVueEntry {
                    path: "node_modules/a/node_modules/vue".to_string(),
                    version: "3.3.4".to_string(),
                },
                DuplicateVueEntry {
                    path: "node_modules/b/node_modules/vue".to_string(),
                    version: "3.2.0".to_string(),
                },
            ],
            ..Default::default()
        };
        let diags = run_rule(&snapshot);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("3 copies"));
    }
}

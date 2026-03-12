//! # provide-inject-validation
//!
//! Cross-file validation of Vue's provide/inject dependency injection:
//! - **Warning**: `inject()` with no matching `provide()` in the project
//! - **Info**: `provide()` keys that are never injected
//!
//! ## Bad
//! ```vue
//! <!-- Consumer.vue -->
//! <script setup>
//! const config = inject('appConfig') // no file provides 'appConfig'!
//! </script>
//! ```

use crate::context::LintContext;
use crate::cross_file::CrossFileSnapshot;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};

/// Lint rule: validate provide/inject consistency across files.
pub struct ProvideInjectValidation;

impl LintRule for ProvideInjectValidation {
    fn name(&self) -> &'static str {
        "provide-inject-validation"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::CrossFile
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_cross_file(&self, snapshot: &CrossFileSnapshot, ctx: &mut LintContext) {
        // Missing providers: inject() with no matching provide()
        for entry in &snapshot.missing_providers {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "No `provide('{}')` found in the project. This `inject()` \
                     will return `undefined` at runtime.",
                    entry.key
                ),
                entry.span.start,
                entry.span.end,
                Some(Severity::Warning),
                DiagnosticSpanKind::CrossFileEntry,
            );
        }

        // Unused provides: provide() key never injected
        for entry in &snapshot.unused_provides {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "`provide('{}')` is never injected by any file in the project.",
                    entry.key
                ),
                entry.span.start,
                entry.span.end,
                Some(Severity::Info),
                DiagnosticSpanKind::CrossFileEntry,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::cross_file::{MissingProviderEntry, UnusedProvideEntry};

    use std::path::PathBuf;

    fn run_rule(snapshot: &CrossFileSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_cross_file_rule(ProvideInjectValidation, snapshot)
    }

    #[test]
    fn empty_snapshot_no_diagnostics() {
        let snapshot = CrossFileSnapshot::default();
        assert!(run_rule(&snapshot).is_empty());
    }

    #[test]
    fn missing_provider_reports_warning() {
        let snapshot = CrossFileSnapshot {
            missing_providers: vec![MissingProviderEntry {
                key: "appConfig".to_string(),
                file: PathBuf::from("/src/Consumer.vue"),
                span: verter_span::Span::new(10, 30),
            }],
            ..Default::default()
        };
        let diags = run_rule(&snapshot);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "provide-inject-validation");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("appConfig"));
    }

    #[test]
    fn unused_provide_reports_info() {
        let snapshot = CrossFileSnapshot {
            unused_provides: vec![UnusedProvideEntry {
                key: "theme".to_string(),
                file: PathBuf::from("/src/Provider.vue"),
                span: verter_span::Span::new(5, 25),
            }],
            ..Default::default()
        };
        let diags = run_rule(&snapshot);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Info);
        assert!(diags[0].message.contains("theme"));
        assert!(diags[0].message.contains("never injected"));
    }

    #[test]
    fn both_missing_and_unused() {
        let snapshot = CrossFileSnapshot {
            missing_providers: vec![MissingProviderEntry {
                key: "config".to_string(),
                file: PathBuf::from("/src/A.vue"),
                span: verter_span::Span::new(10, 30),
            }],
            unused_provides: vec![UnusedProvideEntry {
                key: "orphan".to_string(),
                file: PathBuf::from("/src/B.vue"),
                span: verter_span::Span::new(40, 60),
            }],
            ..Default::default()
        };
        let diags = run_rule(&snapshot);
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[1].severity, Severity::Info);
    }
}

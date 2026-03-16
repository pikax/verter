//! # no-store-outside-setup
//!
//! Errors when a store composable is called outside `<script setup>` or a
//! setup function. Pinia/Vuex stores rely on Vue's runtime injection context
//! which is only available during setup.
//!
//! ## Bad
//! ```js
//! // utils.ts
//! import { useUserStore } from '@/stores/user';
//! const store = useUserStore(); // called at module scope!
//! ```
//!
//! ## Good
//! ```vue
//! <script setup>
//! import { useUserStore } from '@/stores/user';
//! const store = useUserStore(); // called inside setup
//! </script>
//! ```

use crate::context::LintContext;
use crate::cross_file::CrossFileSnapshot;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};

pub struct NoStoreOutsideSetup;

impl LintRule for NoStoreOutsideSetup {
    fn name(&self) -> &'static str {
        "no-store-outside-setup"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::CrossFile
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_cross_file(&self, snapshot: &CrossFileSnapshot, ctx: &mut LintContext) {
        for entry in &snapshot.store_outside_setup {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "Store composable `{}()` called outside `<script setup>` or setup function. \
                     Store composables must be called within the Vue setup context.",
                    entry.callee
                ),
                entry.span.start,
                entry.span.end,
                self.default_severity(),
                DiagnosticSpanKind::CrossFileEntry,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cross_file::StoreOutsideSetupEntry;
    use std::path::PathBuf;

    fn run_rule(snapshot: &CrossFileSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_cross_file_rule(NoStoreOutsideSetup, snapshot)
    }

    #[test]
    fn empty_snapshot_no_diagnostics() {
        let snapshot = CrossFileSnapshot::default();
        assert!(run_rule(&snapshot).is_empty());
    }

    #[test]
    fn store_outside_setup_reports_error() {
        let snapshot = CrossFileSnapshot {
            store_outside_setup: vec![StoreOutsideSetupEntry {
                callee: "useUserStore".to_string(),
                file: PathBuf::from("/src/utils.ts"),
                span: verter_span::Span::new(100, 125),
            }],
            ..Default::default()
        };
        let diags = run_rule(&snapshot);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("useUserStore"));
        assert!(diags[0].message.contains("outside"));
        assert_eq!(diags[0].severity, Severity::Error);
    }

    #[test]
    fn no_false_positive_when_empty() {
        let snapshot = CrossFileSnapshot {
            store_outside_setup: vec![],
            ..Default::default()
        };
        let diags = run_rule(&snapshot);
        assert!(diags.is_empty(), "should not report when list is empty");
    }
}

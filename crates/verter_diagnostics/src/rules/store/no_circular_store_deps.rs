//! # no-circular-store-deps
//!
//! Warns when circular dependencies are detected between stores.
//! Circular store dependencies can cause initialization order issues and
//! are a code smell indicating overly coupled state management.
//!
//! ## Bad
//! ```js
//! // stores/user.ts
//! import { useCartStore } from './cart';
//! export const useUserStore = defineStore('user', () => {
//!   const cart = useCartStore(); // user → cart
//! });
//!
//! // stores/cart.ts
//! import { useUserStore } from './user';
//! export const useCartStore = defineStore('cart', () => {
//!   const user = useUserStore(); // cart → user → CYCLE!
//! });
//! ```

use crate::context::LintContext;
use crate::cross_file::CrossFileSnapshot;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};

pub struct NoCircularStoreDeps;

impl LintRule for NoCircularStoreDeps {
    fn name(&self) -> &'static str {
        "no-circular-store-deps"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::CrossFile
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_cross_file(&self, snapshot: &CrossFileSnapshot, ctx: &mut LintContext) {
        for entry in &snapshot.circular_store_deps {
            let cycle_str = entry.cycle.join(" → ");
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "Circular store dependency detected: {cycle_str}. \
                     Circular dependencies between stores can cause initialization issues."
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
    use crate::cross_file::CircularStoreDepsEntry;

    fn run_rule(snapshot: &CrossFileSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_cross_file_rule(NoCircularStoreDeps, snapshot)
    }

    #[test]
    fn empty_snapshot_no_diagnostics() {
        let snapshot = CrossFileSnapshot::default();
        assert!(run_rule(&snapshot).is_empty());
    }

    #[test]
    fn circular_dep_reports_warning() {
        let snapshot = CrossFileSnapshot {
            circular_store_deps: vec![CircularStoreDepsEntry {
                cycle: vec!["user".to_string(), "cart".to_string(), "user".to_string()],
                span: verter_span::Span::new(50, 100),
            }],
            ..Default::default()
        };
        let diags = run_rule(&snapshot);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("user"));
        assert!(diags[0].message.contains("cart"));
        assert!(diags[0].message.contains("Circular"));
        assert_eq!(diags[0].severity, Severity::Warning);
    }

    #[test]
    fn no_false_positive_when_empty() {
        let snapshot = CrossFileSnapshot {
            circular_store_deps: vec![],
            ..Default::default()
        };
        let diags = run_rule(&snapshot);
        assert!(diags.is_empty(), "should not report when list is empty");
    }
}

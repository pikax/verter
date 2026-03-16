//! # prefer-store-to-refs
//!
//! Warns when a Pinia store is destructured without `storeToRefs()`, which
//! causes reactive state/getters to lose reactivity.
//!
//! ## Bad
//! ```vue
//! <script setup>
//! import { useUserStore } from '@/stores/user';
//! const { name, email } = useUserStore(); // loses reactivity!
//! </script>
//! ```
//!
//! ## Good
//! ```vue
//! <script setup>
//! import { storeToRefs } from 'pinia';
//! import { useUserStore } from '@/stores/user';
//! const store = useUserStore();
//! const { name, email } = storeToRefs(store);
//! </script>
//! ```

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::ScriptAnalysisSnapshot;

pub struct PreferStoreToRefs;

impl LintRule for PreferStoreToRefs {
    fn name(&self) -> &'static str {
        "prefer-store-to-refs"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Reactivity
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for usage in &script.store_usages {
            if usage.destructured_without_store_to_refs {
                let props = if usage.destructured_props.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", usage.destructured_props.join(", "))
                };
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Destructuring `{}()` without `storeToRefs()` loses reactivity{props}. \
                         Wrap with `storeToRefs()` to preserve reactivity.",
                        usage.callee,
                    ),
                    usage.span.start,
                    usage.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::ScriptCallSite,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_analysis::types::{StoreApiClassification, StoreUsage};
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_script_rule(PreferStoreToRefs, script)
    }

    #[test]
    fn empty_script_no_diagnostics() {
        let script = ScriptAnalysisSnapshot::default();
        assert!(run_rule(&script).is_empty());
    }

    #[test]
    fn destructured_without_store_to_refs_warns() {
        let script = ScriptAnalysisSnapshot {
            store_usages: vec![StoreUsage {
                binding_name: "store".to_string(),
                callee: "useUserStore".to_string(),
                import_source: "@/stores/user".to_string(),
                store_api: StoreApiClassification::StoreComposable,
                span: Span::new(50, 75),
                has_store_to_refs: false,
                destructured_props: vec!["name".to_string(), "email".to_string()],
                destructured_without_store_to_refs: true,
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("useUserStore"));
        assert!(diags[0].message.contains("storeToRefs"));
        assert!(diags[0].message.contains("name, email"));
        assert_eq!(diags[0].severity, Severity::Warning);
    }

    #[test]
    fn non_destructured_store_no_warning() {
        let script = ScriptAnalysisSnapshot {
            store_usages: vec![StoreUsage {
                binding_name: "store".to_string(),
                callee: "useUserStore".to_string(),
                import_source: "@/stores/user".to_string(),
                store_api: StoreApiClassification::StoreComposable,
                span: Span::new(50, 75),
                has_store_to_refs: false,
                destructured_props: Vec::new(),
                destructured_without_store_to_refs: false,
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "non-destructured store should not trigger warning"
        );
    }

    #[test]
    fn store_to_refs_usage_no_warning() {
        let script = ScriptAnalysisSnapshot {
            store_usages: vec![StoreUsage {
                binding_name: "refs".to_string(),
                callee: "storeToRefs".to_string(),
                import_source: "pinia".to_string(),
                store_api: StoreApiClassification::PiniaStoreToRefs,
                span: Span::new(50, 75),
                has_store_to_refs: true,
                destructured_props: vec!["name".to_string()],
                destructured_without_store_to_refs: false,
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "storeToRefs usage should not trigger warning"
        );
    }
}

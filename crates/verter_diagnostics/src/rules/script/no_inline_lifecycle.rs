//! # no-inline-lifecycle
//!
//! Suggests extracting lifecycle hooks into composables for better reusability.
//! This is an informational rule — it does not flag errors, just nudges toward
//! the Vue composition pattern of encapsulating side effects.
//!
//! ## Flagged
//! ```vue
//! <script setup>
//! onMounted(() => { /* ... */ })
//! </script>
//! ```
//!
//! ## Preferred
//! ```vue
//! <script setup>
//! useMySetup() // lifecycle hooks inside composable
//! </script>
//! ```

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::types::ScriptAnalysisSnapshot;

/// Lint rule: suggest extracting lifecycle hooks into composables.
pub struct NoInlineLifecycle;

impl LintRule for NoInlineLifecycle {
    fn name(&self) -> &'static str {
        "no-inline-lifecycle"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Info)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for call in &script.vue_api_calls {
            if !call.api.is_lifecycle() {
                continue;
            }

            let api_name = call.api.display_name();
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "Consider extracting `{api_name}()` into a composable for better reusability."
                ),
                call.span.start,
                call.span.end,
                self.default_severity(),
                DiagnosticSpanKind::ScriptCallSite,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_semantic::analysis::types::{VueApiCallSite, VueApiClassification};
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_script_rule(NoInlineLifecycle, script)
    }

    #[test]
    fn no_lifecycle_no_diagnostics() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::Ref,
                span: Span::new(10, 30),
                arg_value: None,
                has_type_params: false,
                is_async_callback: false,
                callback_params: vec![],
            }],
            ..Default::default()
        };
        assert!(run_rule(&script).is_empty());
    }

    #[test]
    fn lifecycle_reports_info() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::OnMounted,
                span: Span::new(10, 30),
                arg_value: None,
                has_type_params: false,
                is_async_callback: false,
                callback_params: vec![],
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "no-inline-lifecycle");
        assert_eq!(diags[0].severity, Severity::Info);
        assert!(diags[0].message.contains("onMounted()"));
        assert!(diags[0].message.contains("composable"));
    }

    #[test]
    fn watcher_not_reported() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::Watch,
                span: Span::new(10, 30),
                arg_value: None,
                has_type_params: false,
                is_async_callback: false,
                callback_params: vec![],
            }],
            ..Default::default()
        };
        // watch() is not a lifecycle hook
        assert!(run_rule(&script).is_empty());
    }

    #[test]
    fn multiple_lifecycles() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![
                VueApiCallSite {
                    api: VueApiClassification::OnMounted,
                    span: Span::new(10, 30),
                    arg_value: None,
                    has_type_params: false,
                    is_async_callback: false,
                    callback_params: vec![],
                },
                VueApiCallSite {
                    api: VueApiClassification::OnUnmounted,
                    span: Span::new(40, 60),
                    arg_value: None,
                    has_type_params: false,
                    is_async_callback: false,
                    callback_params: vec![],
                },
            ],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(diags.len(), 2);
    }
}

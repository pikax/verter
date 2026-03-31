//! Rule: define-macros-in-root
//!
//! Warns when Vue compiler macros (`defineProps`, `defineEmits`, `defineModel`, etc.)
//! are called inside nested scopes (functions, conditionals, loops) rather than at the
//! root level of `<script setup>`.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::types::ScriptAnalysisSnapshot;

/// Require compiler macros to be at the root level of `<script setup>`.
pub struct DefineMacrosInRoot;

impl LintRule for DefineMacrosInRoot {
    fn name(&self) -> &'static str {
        "define-macros-in-root"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for nested in &script.nested_macro_calls {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "`{}()` must be called at the root level of `<script setup>`, not inside a nested scope.",
                    nested.name
                ),
                nested.span.start,
                nested.span.end,
                self.default_severity(),
                DiagnosticSpanKind::ScriptCallSite,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_semantic::analysis::types::{NestedMacroCall, ScriptAnalysisSnapshot};
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_script_rule(DefineMacrosInRoot, script)
    }

    #[test]
    fn reports_nested_define_props() {
        let script = ScriptAnalysisSnapshot {
            nested_macro_calls: vec![NestedMacroCall {
                name: "defineProps".to_string(),
                span: Span::new(50, 80),
            }],
            ..Default::default()
        };

        let diags = run_rule(&script);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "define-macros-in-root");
        assert!(diags[0].message.contains("defineProps"));
        assert!(diags[0].message.contains("root level"));
    }

    #[test]
    fn reports_multiple_nested_macros() {
        let script = ScriptAnalysisSnapshot {
            nested_macro_calls: vec![
                NestedMacroCall {
                    name: "defineProps".to_string(),
                    span: Span::new(50, 80),
                },
                NestedMacroCall {
                    name: "defineEmits".to_string(),
                    span: Span::new(100, 130),
                },
            ],
            ..Default::default()
        };

        let diags = run_rule(&script);
        assert_eq!(diags.len(), 2);
        assert!(diags[0].message.contains("defineProps"));
        assert!(diags[1].message.contains("defineEmits"));
    }

    #[test]
    fn no_nested_macros_no_diags() {
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "no nested macros means no diagnostics");
    }
}

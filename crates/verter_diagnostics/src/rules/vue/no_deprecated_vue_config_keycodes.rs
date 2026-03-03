//! Rule: no-deprecated-vue-config-keycodes
//!
//! `Vue.config.keyCodes` was removed in Vue 3. KeyboardEvent modifiers
//! now use kebab-case key names directly (e.g., `@keyup.page-down`).
//! Detect bindings or import names containing "keyCodes".

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::ScriptAnalysisSnapshot;

pub struct NoDeprecatedVueConfigKeycodes;

impl LintRule for NoDeprecatedVueConfigKeycodes {
    fn name(&self) -> &'static str {
        "no-deprecated-vue-config-keycodes"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        // Check bindings for references containing "keyCodes"
        for binding in &script.bindings {
            if binding.name.contains("keyCodes") {
                ctx.report_with_tags(
                    self.name(),
                    self.category().as_str(),
                    "'Vue.config.keyCodes' has been removed in Vue 3. Use kebab-case key names directly (e.g., '@keyup.page-down').".to_string(),
                    binding.span.start,
                    binding.span.end,
                    self.default_severity(),
                    vec![DiagnosticTag::Deprecated],
                    DiagnosticSpanKind::ScriptCallSite,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDeprecatedVueConfigKeycodes)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_script(script, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn keycodes_binding_reports() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![AnalyzedBinding {
                name: "keyCodes".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: false,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span: Span::new(10, 18),
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(!diags.is_empty(), "keyCodes binding should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-deprecated-vue-config-keycodes"));
        assert!(
            diags[0].tags.contains(&DiagnosticTag::Deprecated),
            "should have Deprecated tag"
        );
        assert!(
            diags[0].message.contains("kebab-case"),
            "message should mention kebab-case key names"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn normal_binding_passes() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![AnalyzedBinding {
                name: "handleKeyPress".to_string(),
                kind: AnalyzedBindingKind::Function,
                is_reactive: false,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span: Span::new(10, 24),
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "normal binding without keyCodes should pass"
        );
    }
}

//! Rule: no-deprecated-model-definition
//!
//! The `model` component option (used in Vue 2 to customize `v-model` prop/event)
//! was removed in Vue 3. Use `v-model:propName` argument syntax instead.
//! Detect bindings named `model` with non-function kind.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{AnalyzedBindingKind, ScriptAnalysisSnapshot};

pub struct NoDeprecatedModelDefinition;

impl LintRule for NoDeprecatedModelDefinition {
    fn name(&self) -> &'static str {
        "no-deprecated-model-definition"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for binding in &script.bindings {
            if binding.name != "model" {
                continue;
            }
            // In Options API, `model` is typically a const object like `model: { prop: 'value', event: 'input' }`
            if matches!(
                binding.kind,
                AnalyzedBindingKind::Const | AnalyzedBindingKind::Let | AnalyzedBindingKind::Var
            ) {
                ctx.report_with_tags(
                    self.name(),
                    self.category().as_str(),
                    "The 'model' component option is deprecated in Vue 3. Use 'v-model:propName' argument syntax instead.".to_string(),
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDeprecatedModelDefinition)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_script(script, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn model_const_binding_reports() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![AnalyzedBinding {
                name: "model".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: false,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span: Span::new(10, 15),
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            !diags.is_empty(),
            "model as Const should trigger deprecation"
        );
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-deprecated-model-definition"));
        assert!(
            diags[0].tags.contains(&DiagnosticTag::Deprecated),
            "should have Deprecated tag"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn non_model_binding_passes() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![AnalyzedBinding {
                name: "setup".to_string(),
                kind: AnalyzedBindingKind::Function,
                is_reactive: false,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span: Span::new(10, 15),
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "non-model binding should pass");
    }
}

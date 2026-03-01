//! Rule: no-deprecated-v-bind-sync
//!
//! The `.sync` modifier on `v-bind` was removed in Vue 3.
//! Use `v-model:propName` instead of `:propName.sync`.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct NoDeprecatedVBindSync;

impl LintRule for NoDeprecatedVBindSync {
    fn name(&self) -> &'static str {
        "no-deprecated-v-bind-sync"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_directive(
        &self,
        dir: &TemplateDirective,
        _el: &TemplateElement,
        ctx: &mut LintContext,
    ) {
        if dir.name != "bind" {
            return;
        }
        if !dir.modifiers.iter().any(|m| m == "sync") {
            return;
        }
        let prop = dir.argument.as_deref().unwrap_or("prop");
        ctx.report_with_severity(
            self.name(),
            self.category().as_str(),
            format!(
                "The '.sync' modifier is not supported in Vue 3. \
                 Use 'v-model:{prop}' instead of ':{prop}.sync'.",
            ),
            dir.span.start,
            dir.span.end,
            self.default_severity(),
            DiagnosticSpanKind::Directive,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDeprecatedVBindSync)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn sync_modifier_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "MyComp".to_string(),
                is_component: true,
                directives: vec![TemplateDirective {
                    name: "bind".to_string(),
                    raw_name: ":title.sync".to_string(),
                    argument: Some("title".to_string()),
                    modifiers: vec!["sync".to_string()],
                    expression: Some("value".to_string()),
                    span: Span::new(8, 25),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), ".sync modifier should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-deprecated-v-bind-sync"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn v_model_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "MyComp".to_string(),
                is_component: true,
                directives: vec![TemplateDirective {
                    name: "model".to_string(),
                    raw_name: "v-model:title".to_string(),
                    argument: Some("title".to_string()),
                    modifiers: vec![],
                    expression: Some("value".to_string()),
                    span: Span::new(8, 25),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "v-model should pass");
    }
}

//! Rule: valid-v-cloak
//!
//! `v-cloak` should not have an expression, argument, or modifiers.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct ValidVCloak;

impl LintRule for ValidVCloak {
    fn name(&self) -> &'static str {
        "valid-v-cloak"
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
        if dir.name != "cloak" {
            return;
        }
        if dir.argument.is_some() {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'v-cloak' does not support arguments.".to_string(),
                dir.span.start,
                dir.span.end,
                self.default_severity(),
                DiagnosticSpanKind::Directive,
            );
        }
        if !dir.modifiers.is_empty() {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'v-cloak' does not support modifiers.".to_string(),
                dir.span.start,
                dir.span.end,
                self.default_severity(),
                DiagnosticSpanKind::Directive,
            );
        }
        if dir.expression.is_some() {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'v-cloak' does not expect a value.".to_string(),
                dir.span.start,
                dir.span.end,
                self.default_severity(),
                DiagnosticSpanKind::Directive,
            );
        }
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(ValidVCloak)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn v_cloak_with_expression_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "cloak".to_string(),
                    raw_name: "v-cloak".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: Some("x".to_string()),
                    span: Span::new(5, 17),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "v-cloak with expression should trigger");
        assert!(diags.iter().any(|d| d.rule == "valid-v-cloak"));
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-once"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn v_cloak_valid_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "cloak".to_string(),
                    raw_name: "v-cloak".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: None,
                    span: Span::new(5, 12),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "valid v-cloak should pass");
    }
}

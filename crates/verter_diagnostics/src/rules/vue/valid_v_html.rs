//! Rule: valid-v-html
//!
//! `v-html` must have an expression and must not have arguments or modifiers.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct ValidVHtml;

impl LintRule for ValidVHtml {
    fn name(&self) -> &'static str {
        "valid-v-html"
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
        if dir.name != "html" {
            return;
        }
        if dir.argument.is_some() {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'v-html' does not support arguments.".to_string(),
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
                "'v-html' does not support modifiers.".to_string(),
                dir.span.start,
                dir.span.end,
                self.default_severity(),
                DiagnosticSpanKind::Directive,
            );
        }
        if dir.expression.is_none() {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'v-html' requires an expression.".to_string(),
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(ValidVHtml)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn v_html_without_expression_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "html".to_string(),
                    raw_name: "v-html".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: None,
                    span: Span::new(5, 11),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "v-html without expression should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "valid-v-html"));
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-once"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn v_html_with_argument_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "html".to_string(),
                    raw_name: "v-html:foo".to_string(),
                    argument: Some("foo".to_string()),
                    modifiers: vec![],
                    expression: Some("content".to_string()),
                    span: Span::new(5, 25),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "v-html with argument should trigger");
    }

    #[test]
    fn v_html_valid_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "html".to_string(),
                    raw_name: "v-html".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: Some("content".to_string()),
                    span: Span::new(5, 25),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "valid v-html should pass");
    }
}

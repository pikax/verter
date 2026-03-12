//! Rule: valid-v-pre
//!
//! `v-pre` should not have an expression, argument, or modifiers.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct ValidVPre;

impl LintRule for ValidVPre {
    fn name(&self) -> &'static str {
        "valid-v-pre"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_directive(
        &self,
        dir: &TemplateDirective,
        _el: &TemplateElement,
        ctx: &mut LintContext,
    ) {
        if dir.name != "pre" {
            return;
        }
        if dir.argument.is_some() {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'v-pre' does not support arguments.".to_string(),
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
                "'v-pre' does not support modifiers.".to_string(),
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
                "'v-pre' does not expect a value.".to_string(),
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

    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(ValidVPre, template)
    }

    #[test]
    fn v_pre_with_expression_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "span".to_string(),
                directives: vec![TemplateDirective {
                    name: "pre".to_string(),
                    raw_name: "v-pre".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: Some("value".to_string()),
                    span: Span::new(6, 18),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "v-pre with expression should trigger");
        assert!(diags.iter().any(|d| d.rule == "valid-v-pre"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn v_pre_no_expression_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "span".to_string(),
                directives: vec![TemplateDirective {
                    name: "pre".to_string(),
                    raw_name: "v-pre".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: None,
                    span: Span::new(6, 11),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "v-pre without expression should pass");
    }
}

//! Rule: valid-v-once
//!
//! `v-once` should not have an expression — it has no value, like `v-pre`.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::{TemplateDirective, TemplateElement};

pub struct ValidVOnce;

impl LintRule for ValidVOnce {
    fn name(&self) -> &'static str {
        "valid-v-once"
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
        if dir.name != "once" {
            return;
        }
        if dir.argument.is_some() {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'v-once' does not support arguments.".to_string(),
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
                "'v-once' does not support modifiers.".to_string(),
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
                "'v-once' does not expect a value.".to_string(),
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

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(ValidVOnce, template)
    }

    #[test]
    fn v_once_with_expression_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "once".to_string(),
                    raw_name: "v-once".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: Some("value".to_string()),
                    span: Span::new(5, 17),
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
        assert!(!diags.is_empty(), "v-once with expression should trigger");
        assert!(diags.iter().any(|d| d.rule == "valid-v-once"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn v_once_no_expression_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "once".to_string(),
                    raw_name: "v-once".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: None,
                    span: Span::new(5, 11),
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
        assert!(diags.is_empty(), "v-once without expression should pass");
    }
}

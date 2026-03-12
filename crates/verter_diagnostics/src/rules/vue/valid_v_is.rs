//! Rule: valid-v-is
//!
//! `v-is` must have an expression.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct ValidVIs;

impl LintRule for ValidVIs {
    fn name(&self) -> &'static str {
        "valid-v-is"
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
        if dir.name != "is" {
            return;
        }
        if dir.expression.is_none() {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'v-is' requires an expression.".to_string(),
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
        crate::test_support::run_template_rule(ValidVIs, template)
    }

    #[test]
    fn v_is_without_expression_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "component".to_string(),
                directives: vec![TemplateDirective {
                    name: "is".to_string(),
                    raw_name: "v-is".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: None,
                    span: Span::new(11, 15),
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
        assert!(!diags.is_empty(), "v-is without expression should trigger");
        assert!(diags.iter().any(|d| d.rule == "valid-v-is"));
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-once"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn v_is_with_expression_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "component".to_string(),
                directives: vec![TemplateDirective {
                    name: "is".to_string(),
                    raw_name: "v-is".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: Some("'vue:my-component'".to_string()),
                    span: Span::new(11, 40),
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
        assert!(diags.is_empty(), "v-is with expression should pass");
    }
}

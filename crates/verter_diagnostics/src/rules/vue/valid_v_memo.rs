//! Rule: valid-v-memo
//!
//! `v-memo` must have an expression (array), must not have arguments.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct ValidVMemo;

impl LintRule for ValidVMemo {
    fn name(&self) -> &'static str {
        "valid-v-memo"
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
        if dir.name != "memo" {
            return;
        }
        if dir.argument.is_some() {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'v-memo' does not support arguments.".to_string(),
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
                "'v-memo' requires an array expression.".to_string(),
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(ValidVMemo)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn v_memo_without_expression_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "memo".to_string(),
                    raw_name: "v-memo".to_string(),
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
                text_children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "v-memo without expression should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "valid-v-memo"));
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-once"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn v_memo_with_expression_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "memo".to_string(),
                    raw_name: "v-memo".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: Some("[a, b]".to_string()),
                    span: Span::new(5, 25),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                content_end: 0,
                text_children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "v-memo with expression should pass");
    }
}

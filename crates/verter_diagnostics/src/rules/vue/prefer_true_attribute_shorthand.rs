//! Rule: prefer-true-attribute-shorthand
//!
//! Using `:prop="true"` is unnecessary. Write the boolean attribute without
//! a binding instead: just `prop` (or omit for `false`).

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct PreferTrueAttributeShorthand;

impl LintRule for PreferTrueAttributeShorthand {
    fn name(&self) -> &'static str {
        "prefer-true-attribute-shorthand"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_directive(
        &self,
        dir: &TemplateDirective,
        el: &TemplateElement,
        ctx: &mut LintContext,
    ) {
        if dir.name != "bind" {
            return;
        }
        let Some(expr) = &dir.expression else {
            return;
        };
        let trimmed = expr.trim();
        if trimmed != "true" {
            return;
        }
        let Some(prop) = &dir.argument else {
            return;
        };
        // Only apply to HTML elements (native boolean attributes) or component props
        if el.is_component {
            // On components, suggest shorthand only for well-known boolean attrs
            // For simplicity, report on all components
        }
        ctx.report_with_severity(
            self.name(),
            self.category().as_str(),
            format!("Redundant ':{}=\"true\"'. Use just '{prop}' instead.", prop),
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

    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(PreferTrueAttributeShorthand, template)
    }

    #[test]
    fn bound_true_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "input".to_string(),
                directives: vec![TemplateDirective {
                    name: "bind".to_string(),
                    raw_name: ":disabled".to_string(),
                    argument: Some("disabled".to_string()),
                    modifiers: vec![],
                    expression: Some("true".to_string()),
                    span: Span::new(7, 22),
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
        assert!(!diags.is_empty(), ":disabled=\"true\" should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "prefer-true-attribute-shorthand"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn bound_false_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "input".to_string(),
                directives: vec![TemplateDirective {
                    name: "bind".to_string(),
                    raw_name: ":disabled".to_string(),
                    argument: Some("disabled".to_string()),
                    modifiers: vec![],
                    expression: Some("false".to_string()),
                    span: Span::new(7, 23),
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
        assert!(
            diags.is_empty(),
            ":disabled=\"false\" should pass (can't shorten)"
        );
    }

    #[test]
    fn dynamic_expression_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "input".to_string(),
                directives: vec![TemplateDirective {
                    name: "bind".to_string(),
                    raw_name: ":disabled".to_string(),
                    argument: Some("disabled".to_string()),
                    modifiers: vec![],
                    expression: Some("isDisabled".to_string()),
                    span: Span::new(7, 28),
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
        assert!(diags.is_empty(), "dynamic expression should pass");
    }
}

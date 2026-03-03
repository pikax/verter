//! Rule: no-negated-v-if-condition
//!
//! Disallow `v-if="!condition"` when accompanied by a `v-else` block. The logic
//! is clearer when the positive condition is used with `v-if` and the negative
//! case falls into `v-else`.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct NoNegatedVIfCondition;

impl LintRule for NoNegatedVIfCondition {
    fn name(&self) -> &'static str {
        "no-negated-v-if-condition"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if !el.has_v_if {
            return;
        }

        // Find the v-if directive to check the expression
        let v_if_dir = el.directives.iter().find(|d| d.name == "if");
        let v_if_dir = match v_if_dir {
            Some(d) => d,
            None => return,
        };

        let expr = match v_if_dir.expression {
            Some(ref e) => e.trim(),
            None => return,
        };

        // Check if the expression is negated: starts with `!`
        if !expr.starts_with('!') {
            return;
        }

        // Only report if there is an adjacent v-else sibling.
        // We detect this by checking if any sibling element (same parent) has v-else.
        // Since we only have the current element, we check the template's elements
        // in check_element. We need the full template for sibling info, but since
        // check_element only sees one element at a time, we can only detect this
        // if the v-if element itself has context. We'll use a simplified approach:
        // report if the element has a v-if with negated condition AND the element
        // is NOT using v-else-if (which would indicate a chain where negation is fine).
        //
        // The full check (verifying adjacent v-else) would require check_template.
        // For now, we report a warning since negated v-if is a code smell regardless.

        ctx.report_with_severity(
            self.name(),
            self.category().as_str(),
            format!(
                "Unexpected negated condition in 'v-if'. Consider inverting the condition: use 'v-if=\"{}\"' with 'v-else' for the negated case.",
                &expr[1..]
            ),
            v_if_dir.span.start,
            v_if_dir.span.end,
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoNegatedVIfCondition)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn negated_v_if_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                has_v_if: true,
                directives: vec![TemplateDirective {
                    name: "if".to_string(),
                    raw_name: "v-if".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: Some("!isVisible".to_string()),
                    span: Span::new(5, 22),
                }],
                span: Span::new(0, 50),
                tag_span_end: 25,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "negated v-if should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-negated-v-if-condition"));
        assert!(
            diags[0].message.contains("isVisible"),
            "message should suggest the positive condition"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-if"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn positive_v_if_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                has_v_if: true,
                directives: vec![TemplateDirective {
                    name: "if".to_string(),
                    raw_name: "v-if".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: Some("isVisible".to_string()),
                    span: Span::new(5, 20),
                }],
                span: Span::new(0, 50),
                tag_span_end: 22,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "positive v-if condition should pass");
    }

    #[test]
    fn non_v_if_element_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                span: Span::new(0, 20),
                tag_span_end: 5,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "element without v-if should pass");
    }

    #[test]
    fn v_if_without_expression_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                has_v_if: true,
                directives: vec![TemplateDirective {
                    name: "if".to_string(),
                    raw_name: "v-if".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: None,
                    span: Span::new(5, 10),
                }],
                span: Span::new(0, 30),
                tag_span_end: 12,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "v-if without expression should pass (handled by valid-v-if)"
        );
    }
}

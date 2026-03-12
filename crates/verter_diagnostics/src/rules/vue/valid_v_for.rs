//! Rule: valid-v-for
//!
//! Enforce valid v-for directives.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateElement, VForDirective};

/// Enforce valid v-for directives (variable and iterable must be present).
pub struct ValidVFor;

impl LintRule for ValidVFor {
    fn name(&self) -> &'static str {
        "valid-v-for"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_v_for(&self, vfor: &VForDirective, _el: &TemplateElement, ctx: &mut LintContext) {
        if vfor.variable.is_empty() {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Expected an iteration variable in 'v-for'.".to_string(),
                vfor.span.start,
                vfor.span.end,
                self.default_severity(),
                DiagnosticSpanKind::Directive,
            );
        }
        if vfor.iterable.is_empty() {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Expected an iterable expression in 'v-for'.".to_string(),
                vfor.span.start,
                vfor.span.end,
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

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(ValidVFor, template)
    }

    fn make_v_for(variable: &str, iterable: &str) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![],
            v_for: Some(VForDirective {
                variable: variable.to_string(),
                index: None,
                iterable: iterable.to_string(),
                has_key: true,
                key_expression: None,
                key_uses_index: false,
                span: verter_span::Span::new(0, 30),
            }),
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_bare_text: false,
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: verter_span::Span::new(0, 50),
            tag_span_end: 50,
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn valid_v_for_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_v_for("item", "items")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }

    #[test]
    fn empty_variable_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_v_for("", "items")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("variable"));
    }

    #[test]
    fn empty_iterable_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_v_for("item", "")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("iterable"));
    }

    /// @ai-generated - Literal array iterable should NOT produce a diagnostic.
    #[test]
    fn literal_array_iterable_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_v_for(
                "route",
                "['dashboard', 'settings', 'profile'] as const",
            )],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "literal array iterable should not trigger diagnostic"
        );
    }

    /// @ai-generated - Numeric array literal iterable should NOT produce a diagnostic.
    #[test]
    fn numeric_array_iterable_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_v_for("item", "[1, 2, 3]")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "numeric array literal should not trigger diagnostic"
        );
    }
}

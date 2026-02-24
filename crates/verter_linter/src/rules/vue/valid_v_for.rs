//! Rule: valid-v-for
//!
//! Enforce valid v-for directives.

use crate::context::LintContext;
use crate::diagnostic::Severity;
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

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_v_for(&self, vfor: &VForDirective, _el: &TemplateElement, ctx: &mut LintContext) {
        if vfor.variable.is_empty() {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Expected an iteration variable in 'v-for'.".to_string(),
                vfor.span_start,
                vfor.span_end,
                self.default_severity(),
            );
        }
        if vfor.iterable.is_empty() {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Expected an iterable expression in 'v-for'.".to_string(),
                vfor.span_start,
                vfor.span_end,
                self.default_severity(),
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

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(ValidVFor)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
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
                span_start: 0,
                span_end: 30,
            }),
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            nesting_depth: 0,
            parent_tag: None,
            span_start: 0,
            span_end: 50,
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
}

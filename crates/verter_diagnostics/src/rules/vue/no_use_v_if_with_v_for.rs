//! Rule: no-use-v-if-with-v-for
//!
//! Disallow use of v-if on the same element as v-for.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateAnalysisSnapshot, TemplateElement};

/// Disallow v-if on the same element as v-for.
pub struct NoUseVIfWithVFor;

impl LintRule for NoUseVIfWithVFor {
    fn name(&self) -> &'static str {
        "no-use-v-if-with-v-for"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if el.has_v_if && el.v_for.is_some() {
            // Try to find the v-if directive for precise span
            let (start, end) = el
                .directives
                .iter()
                .find(|d| d.name == "if")
                .map(|d| (d.span.start, d.span.end))
                .unwrap_or((el.span.start, el.tag_span_end));
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Do not use 'v-if' on the same element as 'v-for'.".to_string(),
                start,
                end,
                self.default_severity(),
                DiagnosticSpanKind::Directive,
            );
        }
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        // Also check the pre-computed conflicts list
        for (start, end) in &tpl.v_if_v_for_conflicts {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Do not use 'v-if' on the same element as 'v-for'.".to_string(),
                *start,
                *end,
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

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoUseVIfWithVFor)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_element(has_v_if: bool, has_v_for: bool) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![],
            v_for: if has_v_for {
                Some(VForDirective {
                    variable: "item".to_string(),
                    index: None,
                    iterable: "items".to_string(),
                    has_key: true,
                    key_expression: Some("item.id".to_string()),
                    key_uses_index: false,
                    span: verter_span::Span::new(5, 30),
                })
            } else {
                None
            },
            v_model: None,
            has_v_if,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: verter_span::Span::new(0, 50),
            tag_span_end: 50,
        }
    }

    #[test]
    fn v_if_with_v_for_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element(true, true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty());
        assert_eq!(diags[0].rule, "no-use-v-if-with-v-for");
    }

    #[test]
    fn v_if_without_v_for_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element(true, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }

    #[test]
    fn v_for_without_v_if_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element(false, true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }
}

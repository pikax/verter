//! Rule: anchor-has-content
//!
//! Enforce that anchors have content.

use crate::context::LintContext;
use crate::diagnostic::Severity;
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct AnchorHasContent;

impl LintRule for AnchorHasContent {
    fn name(&self) -> &'static str {
        "anchor-has-content"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Accessibility
    }
    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if el.tag != "a" || el.is_component {
            return;
        }
        // Check for aria-label or aria-labelledby as alternatives
        let has_label = el
            .attributes
            .iter()
            .any(|a| a.name == "aria-label" || a.name == "aria-labelledby");
        // If self-closing and no aria-label, it has no content
        if el.is_self_closing && !has_label {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Anchors must have text content or an aria-label.".to_string(),
                el.span_start,
                el.span_end,
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

    fn run(elements: Vec<TemplateElement>) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(AnchorHasContent)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(
            &TemplateAnalysisSnapshot {
                elements,
                ..Default::default()
            },
            &mut ctx,
        );
        ctx.into_diagnostics()
    }

    fn anchor(self_closing: bool, attrs: Vec<(&str, &str)>) -> TemplateElement {
        TemplateElement {
            tag: "a".to_string(),
            is_component: false,
            is_self_closing: self_closing,
            namespace: ElementNamespace::Html,
            attributes: attrs
                .into_iter()
                .map(|(n, v)| TemplateAttribute {
                    name: n.to_string(),
                    value: Some(v.to_string()),
                    is_dynamic: false,
                    span_start: 0,
                    span_end: 10,
                })
                .collect(),
            directives: vec![],
            v_for: None,
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
            span_end: 30,
        }
    }

    #[test]
    fn self_closing_anchor_reports() {
        assert_eq!(run(vec![anchor(true, vec![("href", "/")])]).len(), 1);
    }

    #[test]
    fn anchor_with_aria_label_passes() {
        assert!(run(vec![anchor(
            true,
            vec![("href", "/"), ("aria-label", "Home")]
        )])
        .is_empty());
    }

    #[test]
    fn non_self_closing_anchor_passes() {
        assert!(run(vec![anchor(false, vec![("href", "/")])]).is_empty());
    }
}

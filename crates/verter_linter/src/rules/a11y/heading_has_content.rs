//! Rule: heading-has-content
//!
//! Heading elements (`<h1>` through `<h6>`) must have content. A self-closing
//! heading with no `aria-label` is reported as having no accessible content.

use crate::context::LintContext;
use crate::diagnostic::Severity;
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct HeadingHasContent;

impl LintRule for HeadingHasContent {
    fn name(&self) -> &'static str {
        "heading-has-content"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Accessibility
    }
    fn default_severity(&self) -> Severity {
        Severity::Warning
    }
    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if el.is_component {
            return;
        }
        let is_heading = matches!(el.tag.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
        if !is_heading {
            return;
        }
        if !el.is_self_closing {
            return;
        }
        let has_aria_label = el.attributes.iter().any(|a| a.name == "aria-label");
        if !has_aria_label {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!("<{}> elements must have content.", el.tag),
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(HeadingHasContent)];
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

    fn heading(tag: &str, self_closing: bool, attrs: Vec<(&str, &str)>) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
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
    fn self_closing_heading_without_aria_label_reports() {
        let diags = run(vec![heading("h1", true, vec![])]);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("<h1>"));
    }

    #[test]
    fn self_closing_heading_with_aria_label_passes() {
        assert!(run(vec![heading(
            "h2",
            true,
            vec![("aria-label", "Section title")]
        )])
        .is_empty());
    }

    #[test]
    fn non_self_closing_heading_passes() {
        assert!(run(vec![heading("h3", false, vec![])]).is_empty());
    }

    #[test]
    fn non_heading_element_passes() {
        assert!(run(vec![heading("div", true, vec![])]).is_empty());
    }
}

//! Rule: iframe-has-title
//!
//! `<iframe>` elements must have a `title` attribute to provide an accessible
//! description for screen readers.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct IframeHasTitle;

impl LintRule for IframeHasTitle {
    fn name(&self) -> &'static str {
        "iframe-has-title"
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
        if el.tag != "iframe" {
            return;
        }
        let has_title = el.attributes.iter().any(|a| a.name == "title");
        if !has_title {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "<iframe> elements must have a 'title' attribute.".to_string(),
                el.span.start,
                el.tag_span_end,
                self.default_severity(),
                DiagnosticSpanKind::ElementOpenTag,
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

    fn run(elements: Vec<TemplateElement>) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(IframeHasTitle)];
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

    fn el(tag: &str, attrs: Vec<(&str, Option<&str>)>) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: attrs
                .into_iter()
                .map(|(n, v)| TemplateAttribute {
                    name: n.to_string(),
                    value: v.map(|s| s.to_string()),
                    is_dynamic: false,
                    span: Span::new(0, 10),
                    name_end: 0,
                    value_span: None,
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
            has_text_content: false,
            has_bare_text: false,

            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 30),
            tag_span_end: 30,
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn iframe_without_title_reports() {
        let diags = run(vec![el(
            "iframe",
            vec![("src", Some("https://example.com"))],
        )]);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("<iframe>"));
    }

    #[test]
    fn iframe_with_title_passes() {
        assert!(run(vec![el(
            "iframe",
            vec![
                ("src", Some("https://example.com")),
                ("title", Some("Example"))
            ]
        )])
        .is_empty());
    }

    #[test]
    fn non_iframe_element_passes() {
        assert!(run(vec![el("div", vec![])]).is_empty());
    }
}

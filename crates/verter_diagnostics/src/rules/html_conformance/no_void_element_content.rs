//! Rule: no-void-element-content
//!
//! Void elements (br, hr, img, input, link, meta, etc.) cannot have children
//! per the HTML specification. Any content inside them is invalid.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

/// HTML void elements that cannot have children.
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

pub struct NoVoidElementContent;

impl LintRule for NoVoidElementContent {
    fn name(&self) -> &'static str {
        "no-void-element-content"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::HtmlConformance
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if el.is_component {
            return;
        }
        let tag = el.tag.to_ascii_lowercase();
        if !VOID_ELEMENTS.contains(&tag.as_str()) {
            return;
        }
        if !el.has_text_content && !el.has_element_children {
            return;
        }
        ctx.report_with_severity(
            self.name(),
            self.category().as_str(),
            format!(
                "Void element '<{}>' cannot have content. \
                 Remove any children or closing tag.",
                el.tag
            ),
            el.span.start,
            el.tag_span_end,
            self.default_severity(),
            DiagnosticSpanKind::ElementOpenTag,
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoVoidElementContent)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn br_with_text_content_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "br".to_string(),
                has_text_content: true,
                span: Span::new(0, 10),
                tag_span_end: 4,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "br with text content should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-void-element-content"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn img_without_children_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "img".to_string(),
                is_self_closing: true,
                span: Span::new(0, 20),
                tag_span_end: 20,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "self-closing img should pass");
    }

    #[test]
    fn div_with_children_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                has_text_content: true,
                span: Span::new(0, 20),
                tag_span_end: 5,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "non-void elements may have children");
    }
}

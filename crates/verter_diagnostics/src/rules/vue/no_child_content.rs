//! Rule: no-child-content
//!
//! Disallows child content (element children or text) alongside `v-html` or `v-text`
//! directives, since those directives completely replace the element's content.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateElement;

pub struct NoChildContent;

impl LintRule for NoChildContent {
    fn name(&self) -> &'static str {
        "no-child-content"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if el.has_v_html && (el.has_element_children || el.has_text_content) {
            let (start, end) = el
                .directives
                .iter()
                .find(|d| d.name == "html")
                .map(|d| (d.span.start, d.span.end))
                .unwrap_or((el.span.start, el.tag_span_end));
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Child content of elements using 'v-html' will be overwritten. Use the directive or children, not both.".to_string(),
                start,
                end,
                self.default_severity(),
                DiagnosticSpanKind::Directive,
            );
        }

        if el.has_v_text && (el.has_element_children || el.has_text_content) {
            let (start, end) = el
                .directives
                .iter()
                .find(|d| d.name == "text")
                .map(|d| (d.span.start, d.span.end))
                .unwrap_or((el.span.start, el.tag_span_end));
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Child content of elements using 'v-text' will be overwritten. Use the directive or children, not both.".to_string(),
                start,
                end,
                self.default_severity(),
                DiagnosticSpanKind::Directive,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoChildContent, template)
    }

    fn make_element(
        has_v_html: bool,
        has_v_text: bool,
        has_element_children: bool,
        has_text_content: bool,
    ) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![],
            v_for: None,
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html,
            has_v_text,
            has_text_content,
            has_bare_text: false,
            has_element_children,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 30,
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn v_html_with_element_children_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element(true, false, true, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "v-html + element children should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "no-child-content"));
        assert!(
            diags[0].message.contains("v-html"),
            "message should mention v-html"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger no-v-html"
        );
    }

    #[test]
    fn v_html_with_text_content_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element(true, false, false, true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "v-html + text content should trigger");
    }

    #[test]
    fn v_text_with_element_children_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element(false, true, true, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "v-text + element children should trigger"
        );
        assert!(
            diags[0].message.contains("v-text"),
            "message should mention v-text"
        );
    }

    #[test]
    fn v_html_without_children_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element(true, false, false, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "v-html without children should pass");
    }

    #[test]
    fn element_with_children_no_directives_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element(false, false, true, true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "element with children but no v-html/v-text should pass"
        );
    }
}

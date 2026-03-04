//! Rule: html-self-closing
//!
//! Requires void HTML elements (br, hr, img, input, etc.) to be self-closing.
//! E.g., `<br />` instead of `<br>` or `<br></br>`.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

/// HTML void elements that must be self-closing.
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

pub struct HtmlSelfClosing;

impl LintRule for HtmlSelfClosing {
    fn name(&self) -> &'static str {
        "html-self-closing"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if el.is_component {
            return;
        }

        let tag_lower = el.tag.to_ascii_lowercase();
        if VOID_ELEMENTS.contains(&tag_lower.as_str()) && !el.is_self_closing {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "Void element '<{}>' should be self-closing. Use '<{} />'.",
                    el.tag, el.tag
                ),
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

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(HtmlSelfClosing)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_el(tag: &str, is_self_closing: bool, is_component: bool) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            is_component,
            is_self_closing,
            namespace: ElementNamespace::Html,
            attributes: vec![],
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
            span: Span::new(0, 10),
            tag_span_end: 10,
        }
    }

    #[test]
    fn non_self_closing_br_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("br", false, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "<br> (non-self-closing) should trigger");
        assert!(diags.iter().any(|d| d.rule == "html-self-closing"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger no-v-html"
        );
    }

    #[test]
    fn self_closing_br_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("br", true, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "<br /> should pass");
    }

    #[test]
    fn self_closing_img_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("img", true, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "<img /> should pass");
    }

    #[test]
    fn non_self_closing_input_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("input", false, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "<input> without self-closing should trigger"
        );
    }

    #[test]
    fn non_void_element_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("div", false, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "<div> is not a void element, must not trigger"
        );
    }

    #[test]
    fn void_element_used_as_component_passes() {
        // If somehow a component is named "br" (unlikely but possible)
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("br", false, true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "component named 'br' should not trigger html-self-closing"
        );
    }
}

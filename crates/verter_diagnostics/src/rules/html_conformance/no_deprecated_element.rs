//! Rule: no-deprecated-element
//!
//! Disallows use of deprecated HTML elements that have been removed or
//! deprecated in the HTML living standard.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateElement;

/// Deprecated HTML elements and their suggested replacements.
const DEPRECATED: &[(&str, &str)] = &[
    ("marquee", "CSS animations"),
    ("blink", "CSS animations"),
    ("center", "<div style=\"text-align:center\">"),
    ("font", "CSS font properties"),
    ("frame", "iframe or CSS layout"),
    ("frameset", "CSS layout"),
    ("big", "CSS font-size"),
    ("strike", "<s> or <del>"),
    ("tt", "<code> or CSS"),
    ("acronym", "<abbr>"),
    ("applet", "<object> or <embed>"),
    ("basefont", "CSS"),
    ("dir", "<ul>"),
    ("isindex", "<input>"),
    ("noframes", "CSS layout"),
];

pub struct NoDeprecatedElement;

impl LintRule for NoDeprecatedElement {
    fn name(&self) -> &'static str {
        "no-deprecated-element"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::HtmlConformance
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if el.is_component {
            return;
        }

        let tag_lower = el.tag.to_ascii_lowercase();
        if let Some((_, replacement)) = DEPRECATED
            .iter()
            .find(|(tag, _)| *tag == tag_lower.as_str())
        {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "HTML element '<{}>' is deprecated. Consider using {} instead.",
                    el.tag, replacement
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

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoDeprecatedElement, template)
    }

    fn make_el(tag: &str, is_component: bool) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            is_component,
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
            tag_span_end: 10,
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn marquee_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("marquee", false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "<marquee> should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-deprecated-element"));
        assert!(
            diags[0].message.contains("deprecated"),
            "message should say deprecated"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger no-v-html"
        );
    }

    #[test]
    fn center_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("center", false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "<center> should trigger");
    }

    #[test]
    fn div_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("div", false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "<div> is not deprecated");
    }

    #[test]
    fn span_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("span", false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "<span> is not deprecated");
    }

    #[test]
    fn component_named_center_not_flagged() {
        // A component named "Center" (capitalized) or "center" — is_component=true
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el("center", true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "component named 'center' must not trigger no-deprecated-element"
        );
    }
}

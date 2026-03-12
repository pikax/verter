//! Rule: no-autofocus
//!
//! Disallow the `autofocus` attribute. Autofocus can cause accessibility issues
//! by unexpectedly moving focus, disorienting screen reader users and users with
//! motor impairments.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct NoAutofocus;

impl LintRule for NoAutofocus {
    fn name(&self) -> &'static str {
        "no-autofocus"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Accessibility
    }
    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }
    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if let Some(attr) = el.attributes.iter().find(|a| a.name == "autofocus") {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "The 'autofocus' attribute should not be used. It can cause accessibility issues by unexpectedly moving focus.".to_string(),
                attr.span.start,
                attr.span.end,
                self.default_severity(),
                DiagnosticSpanKind::Attribute,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(elements: Vec<TemplateElement>) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_elements_rule(NoAutofocus, elements)
    }

    fn el(tag: &str, attrs: Vec<(&str, Option<&str>)>) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            is_component: false,
            is_self_closing: tag == "input",
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
    fn element_with_autofocus_reports() {
        let diags = run(vec![el("input", vec![("autofocus", None)])]);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("autofocus"));
    }

    #[test]
    fn element_without_autofocus_passes() {
        assert!(run(vec![el("input", vec![("type", Some("text"))])]).is_empty());
    }

    #[test]
    fn div_with_autofocus_also_reports() {
        assert_eq!(run(vec![el("div", vec![("autofocus", None)])]).len(), 1);
    }
}

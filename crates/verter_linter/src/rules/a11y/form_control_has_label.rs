//! Rule: form-control-has-label
//!
//! Form controls (`<input>`, `<select>`, `<textarea>`) should have an associated
//! label. Checks for `aria-label` or `aria-labelledby` attributes on the element.

use crate::context::LintContext;
use crate::diagnostic::Severity;
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct FormControlHasLabel;

impl LintRule for FormControlHasLabel {
    fn name(&self) -> &'static str {
        "form-control-has-label"
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
        let is_form_control = matches!(el.tag.as_str(), "input" | "select" | "textarea");
        if !is_form_control {
            return;
        }
        // Hidden inputs don't need labels
        if el.tag == "input"
            && el
                .attributes
                .iter()
                .any(|a| a.name == "type" && a.value.as_deref() == Some("hidden"))
        {
            return;
        }
        let has_label = el
            .attributes
            .iter()
            .any(|a| a.name == "aria-label" || a.name == "aria-labelledby");
        if !has_label {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "<{}> elements must have an associated label (aria-label or aria-labelledby).",
                    el.tag
                ),
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(FormControlHasLabel)];
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
            is_self_closing: tag == "input",
            namespace: ElementNamespace::Html,
            attributes: attrs
                .into_iter()
                .map(|(n, v)| TemplateAttribute {
                    name: n.to_string(),
                    value: v.map(|s| s.to_string()),
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
    fn input_without_label_reports() {
        let diags = run(vec![el("input", vec![("type", Some("text"))])]);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("<input>"));
    }

    #[test]
    fn input_with_aria_label_passes() {
        assert!(run(vec![el(
            "input",
            vec![("type", Some("text")), ("aria-label", Some("Name"))]
        )])
        .is_empty());
    }

    #[test]
    fn select_without_label_reports() {
        assert_eq!(run(vec![el("select", vec![])]).len(), 1);
    }

    #[test]
    fn textarea_with_aria_labelledby_passes() {
        assert!(run(vec![el(
            "textarea",
            vec![("aria-labelledby", Some("label-id"))]
        )])
        .is_empty());
    }

    #[test]
    fn hidden_input_passes() {
        assert!(run(vec![el("input", vec![("type", Some("hidden"))])]).is_empty());
    }

    #[test]
    fn div_without_label_passes() {
        assert!(run(vec![el("div", vec![])]).is_empty());
    }
}

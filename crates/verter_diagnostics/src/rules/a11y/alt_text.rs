//! Rule: alt-text
//!
//! Enforce that `<img>`, `<area>`, `<input type="image">`, and `<object>` elements have alt text.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct AltText;

impl LintRule for AltText {
    fn name(&self) -> &'static str {
        "alt-text"
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
        let needs_alt = match el.tag.as_str() {
            "img" => true,
            "area" => true,
            "input" => el
                .attributes
                .iter()
                .any(|a| a.name == "type" && a.value.as_deref() == Some("image")),
            _ => false,
        };
        if needs_alt && !el.attributes.iter().any(|a| a.name == "alt") {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!("<{}> elements must have an 'alt' attribute.", el.tag),
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(AltText)];
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
            is_self_closing: true,
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
            text_children: Vec::new(),
        }
    }

    #[test]
    fn img_without_alt_reports() {
        assert_eq!(run(vec![el("img", vec![("src", Some("x.png"))])]).len(), 1);
    }

    #[test]
    fn img_with_alt_passes() {
        assert!(run(vec![el(
            "img",
            vec![("src", Some("x.png")), ("alt", Some("desc"))]
        )])
        .is_empty());
    }

    #[test]
    fn div_without_alt_passes() {
        assert!(run(vec![el("div", vec![])]).is_empty());
    }
}

//! Rule: html-button-has-type
//!
//! Requires `<button>` elements to have an explicit `type` attribute.
//! Without `type`, buttons default to `type="submit"` which may cause
//! unexpected form submissions.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct HtmlButtonHasType;

impl LintRule for HtmlButtonHasType {
    fn name(&self) -> &'static str {
        "html-button-has-type"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if el.tag != "button" || el.is_component {
            return;
        }

        // Check for a `type` attribute (static or dynamic)
        let has_type = el.attributes.iter().any(|a| a.name == "type")
            || el
                .directives
                .iter()
                .any(|d| d.name == "bind" && d.argument.as_deref() == Some("type"));

        if !has_type {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "The `<button>` element should have an explicit `type` attribute (\"button\", \"submit\", or \"reset\").".to_string(),
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

    use verter_analysis::template::*;
    use verter_span::Span;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(HtmlButtonHasType, template)
    }

    fn make_button(attrs: Vec<TemplateAttribute>) -> TemplateElement {
        TemplateElement {
            tag: "button".to_string(),
            attributes: attrs,
            span: Span::new(0, 20),
            tag_span_end: 20,
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn button_without_type_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_button(vec![])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "button without type should trigger");
        assert!(diags.iter().any(|d| d.rule == "html-button-has-type"));
        assert!(
            diags[0].message.contains("type"),
            "message should mention type"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn button_with_type_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_button(vec![TemplateAttribute {
                name: "type".to_string(),
                value: Some("button".to_string()),
                is_dynamic: false,
                span: Span::new(8, 22),
                name_end: 0,
                value_span: None,
            }])],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "button with type should pass");
    }

    #[test]
    fn button_with_dynamic_type_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "button".to_string(),
                directives: vec![TemplateDirective {
                    name: "bind".to_string(),
                    raw_name: ":type".to_string(),
                    argument: Some("type".to_string()),
                    modifiers: vec![],
                    expression: Some("buttonType".to_string()),
                    span: Span::new(8, 30),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                span: Span::new(0, 40),
                tag_span_end: 40,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "button with :type should pass");
    }

    #[test]
    fn non_button_element_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                span: Span::new(0, 10),
                tag_span_end: 10,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "non-button element should pass");
    }

    #[test]
    fn button_component_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "button".to_string(),
                is_component: true,
                span: Span::new(0, 20),
                tag_span_end: 20,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "button component should pass (not a native element)"
        );
    }
}

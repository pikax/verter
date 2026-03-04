//! Rule: no-deprecated-html-element-is
//!
//! In Vue 3, the `is` attribute on native HTML elements must use the `vue:` prefix
//! (e.g., `<tr is="vue:my-row">`). Plain `is` on native elements is no longer
//! treated as a Vue dynamic component. Detect `is` attributes on non-component
//! elements without the `vue:` prefix.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct NoDeprecatedHtmlElementIs;

impl LintRule for NoDeprecatedHtmlElementIs {
    fn name(&self) -> &'static str {
        "no-deprecated-html-element-is"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        // Only applies to native HTML elements, not components
        if el.is_component {
            return;
        }

        for attr in &el.attributes {
            if attr.name != "is" {
                continue;
            }
            // Check if the value starts with "vue:" — that's the Vue 3 way
            if let Some(ref value) = attr.value {
                if value.starts_with("vue:") {
                    continue;
                }
            }
            ctx.report_with_tags(
                self.name(),
                self.category().as_str(),
                format!(
                    "The 'is' attribute on '<{}>' is no longer treated as a Vue dynamic component in Vue 3. \
                     Use 'is=\"vue:component-name\"' prefix instead.",
                    el.tag
                ),
                attr.span.start,
                attr.span.end,
                self.default_severity(),
                vec![DiagnosticTag::Deprecated],
                DiagnosticSpanKind::Attribute,
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

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDeprecatedHtmlElementIs)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn plain_is_on_html_element_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "tr".to_string(),
                is_component: false,
                attributes: vec![TemplateAttribute {
                    name: "is".to_string(),
                    value: Some("my-row".to_string()),
                    is_dynamic: false,
                    span: Span::new(4, 17),
                    name_end: 0,
                    value_span: None,
                }],
                content_end: 0,
                text_children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "is=\"my-row\" on <tr> should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-deprecated-html-element-is"));
        assert!(
            diags[0].tags.contains(&DiagnosticTag::Deprecated),
            "should have Deprecated tag"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn vue_prefix_is_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "tr".to_string(),
                is_component: false,
                attributes: vec![TemplateAttribute {
                    name: "is".to_string(),
                    value: Some("vue:my-row".to_string()),
                    is_dynamic: false,
                    span: Span::new(4, 21),
                    name_end: 0,
                    value_span: None,
                }],
                content_end: 0,
                text_children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "is=\"vue:my-row\" should pass (Vue 3 style)"
        );
    }

    #[test]
    fn is_on_component_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "component".to_string(),
                is_component: true,
                attributes: vec![TemplateAttribute {
                    name: "is".to_string(),
                    value: Some("my-comp".to_string()),
                    is_dynamic: false,
                    span: Span::new(12, 25),
                    name_end: 0,
                    value_span: None,
                }],
                content_end: 0,
                text_children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "is on <component> should pass (it's a component)"
        );
    }
}

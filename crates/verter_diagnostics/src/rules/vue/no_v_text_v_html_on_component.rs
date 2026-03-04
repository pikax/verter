//! Rule: no-v-text-v-html-on-component
//!
//! Disallows `v-html` and `v-text` on components.
//! These directives work as slot content on native elements but are ignored on
//! Vue components — use default slots instead.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct NoVTextVHtmlOnComponent;

impl LintRule for NoVTextVHtmlOnComponent {
    fn name(&self) -> &'static str {
        "no-v-text-v-html-on-component"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if !el.is_component {
            return;
        }

        if el.has_v_html {
            let (start, end) = el
                .directives
                .iter()
                .find(|d| d.name == "html")
                .map(|d| (d.span.start, d.span.end))
                .unwrap_or((el.span.start, el.tag_span_end));
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "'v-html' cannot be used on component '<{}>'. Use a default slot instead.",
                    el.tag
                ),
                start,
                end,
                self.default_severity(),
                DiagnosticSpanKind::Directive,
            );
        }

        if el.has_v_text {
            let (start, end) = el
                .directives
                .iter()
                .find(|d| d.name == "text")
                .map(|d| (d.span.start, d.span.end))
                .unwrap_or((el.span.start, el.tag_span_end));
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "'v-text' cannot be used on component '<{}>'. Use a default slot instead.",
                    el.tag
                ),
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
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoVTextVHtmlOnComponent)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_component(tag: &str, has_v_html: bool, has_v_text: bool) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            is_component: true,
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
            has_text_content: false,
            has_bare_text: false,
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 50,
        }
    }

    #[test]
    fn v_html_on_component_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component("MyComp", true, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "v-html on component should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-v-text-v-html-on-component"));
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
    fn v_text_on_component_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component("MyComp", false, true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "v-text on component should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-v-text-v-html-on-component"));
        assert!(
            diags[0].message.contains("v-text"),
            "message should mention v-text"
        );
    }

    #[test]
    fn v_html_on_native_div_passes() {
        // <div v-html="x"> — native element, not a component
        let el = TemplateElement {
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
            has_v_html: true,
            has_v_text: false,
            has_text_content: false,
            has_bare_text: false,
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 50,
        };
        let template = TemplateAnalysisSnapshot {
            elements: vec![el],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "v-html on native element must not trigger no-v-text-v-html-on-component"
        );
    }

    #[test]
    fn clean_component_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component("MyComp", false, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "component without v-html/v-text should pass"
        );
    }
}

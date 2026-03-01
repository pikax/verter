//! Rule: vapor/no-inline-template
//!
//! Disallows `inline-template` attribute on components in Vapor mode.
//! Inline templates are not supported in Vapor.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct NoInlineTemplate;

impl LintRule for NoInlineTemplate {
    fn name(&self) -> &'static str {
        "vapor/no-inline-template"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Vapor
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if !ctx.config().vapor_mode {
            return;
        }

        if !el.is_component {
            return;
        }

        if let Some(attr) = el.attributes.iter().find(|a| a.name == "inline-template") {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'inline-template' is not supported in Vapor mode.".to_string(),
                attr.span.start,
                attr.span.end,
                self.default_severity(),
                DiagnosticSpanKind::Directive,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LintConfig, LintPreset};
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run_rule(
        template: &TemplateAnalysisSnapshot,
        vapor: bool,
    ) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoInlineTemplate)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig {
            vapor_mode: vapor,
            preset: LintPreset::Recommended,
            ..Default::default()
        };
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_component(has_inline_template: bool) -> TemplateElement {
        let attrs = if has_inline_template {
            vec![TemplateAttribute {
                name: "inline-template".to_string(),
                value: None,
                is_dynamic: false,
                span: Span::new(10, 27),
            }]
        } else {
            vec![]
        };
        TemplateElement {
            tag: "MyComp".to_string(),
            is_component: true,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: attrs,
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
    fn inline_template_in_vapor_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component(true)],
            ..Default::default()
        };
        let diags = run_rule(&template, true);
        assert!(!diags.is_empty(), "inline-template in vapor should trigger");
        assert!(diags.iter().any(|d| d.rule == "vapor/no-inline-template"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger no-v-html"
        );
    }

    #[test]
    fn inline_template_in_vdom_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component(true)],
            ..Default::default()
        };
        let diags = run_rule(&template, false);
        assert!(
            diags.is_empty(),
            "inline-template in VDOM should not trigger vapor rule"
        );
    }

    #[test]
    fn component_without_inline_template_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component(false)],
            ..Default::default()
        };
        let diags = run_rule(&template, true);
        assert!(
            diags.is_empty(),
            "component without inline-template should pass"
        );
    }
}

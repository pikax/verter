//! Rule: require-component-is
//!
//! Requires that `<component>` elements have an `:is` binding to specify
//! which component to render dynamically.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct RequireComponentIs;

impl LintRule for RequireComponentIs {
    fn name(&self) -> &'static str {
        "require-component-is"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if el.tag != "component" {
            return;
        }

        // Must have a dynamic `:is` attribute
        let has_is = el.attributes.iter().any(|a| a.name == "is" && a.is_dynamic);
        if !has_is {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'<component>' elements require a ':is' binding to specify the component."
                    .to_string(),
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(RequireComponentIs)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_component_el(has_is: bool, is_dynamic: bool) -> TemplateElement {
        let mut attrs = vec![];
        if has_is {
            attrs.push(TemplateAttribute {
                name: "is".to_string(),
                value: Some("MyComp".to_string()),
                is_dynamic,
                span: Span::new(12, 25),
            });
        }
        TemplateElement {
            tag: "component".to_string(),
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
            has_bare_text: false,
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 30,
        }
    }

    #[test]
    fn component_without_is_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component_el(false, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "<component> without :is should trigger");
        assert!(diags.iter().any(|d| d.rule == "require-component-is"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger no-v-html"
        );
    }

    #[test]
    fn component_with_static_is_reports() {
        // <component is="MyComp"> — static is, not dynamic
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component_el(true, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "<component> with static is should trigger (needs :is)"
        );
    }

    #[test]
    fn component_with_dynamic_is_passes() {
        // <component :is="comp">
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_component_el(true, true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "<component :is=\"comp\"> should pass");
    }

    #[test]
    fn non_component_tag_passes() {
        let element = TemplateElement {
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
            has_v_html: false,
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
            elements: vec![element],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "regular div must not trigger require-component-is"
        );
    }
}

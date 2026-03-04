//! Rule: attribute-order
//!
//! Enforces recommended attribute order in Vue templates.
//! The most important check: `v-if` should come before `v-for` on the same element,
//! but `v-for` elements should typically come before `v-if` sibling elements.
//!
//! Simplified check: warns when both `v-for` and `v-if` directives are on the same
//! element without `v-for` being handled at a parent level first.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct AttributeOrder;

impl LintRule for AttributeOrder {
    fn name(&self) -> &'static str {
        "attribute-order"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        // Detect v-for + v-if on same element — v-if should be on wrapper, not alongside v-for
        // This is the most common attribute ordering issue
        if el.v_for.is_some() && el.has_v_if {
            // Find the v-if directive to get its span
            let v_if_dir = el.directives.iter().find(|d| d.name == "if");
            let (start, end) = v_if_dir
                .map(|d| (d.span.start, d.span.end))
                .unwrap_or((el.span.start, el.tag_span_end));

            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Avoid using 'v-if' and 'v-for' on the same element. Move 'v-if' to a wrapper element to avoid iterating over filtered items.".to_string(),
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(AttributeOrder)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_el_with_for_and_if(has_v_for: bool, has_v_if: bool) -> TemplateElement {
        let v_for = if has_v_for {
            Some(VForDirective {
                variable: "item".to_string(),
                index: None,
                iterable: "items".to_string(),
                has_key: true,
                key_expression: Some("item.id".to_string()),
                key_uses_index: false,
                span: Span::new(5, 30),
            })
        } else {
            None
        };
        let directives = if has_v_if {
            vec![TemplateDirective {
                name: "if".to_string(),
                raw_name: "v-if".to_string(),
                argument: None,
                modifiers: vec![],
                expression: Some("cond".to_string()),
                span: Span::new(31, 45),
                name_end: 0,
                arg_span: None,
                expression_span: None,
                modifier_spans: Vec::new(),
            }]
        } else {
            vec![]
        };
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives,
            v_for,
            v_model: None,
            has_v_if,
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
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn v_for_and_v_if_on_same_element_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_for_and_if(true, true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "v-for + v-if on same element should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "attribute-order"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger no-v-html"
        );
    }

    #[test]
    fn only_v_for_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_for_and_if(true, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "v-for without v-if should pass");
    }

    #[test]
    fn only_v_if_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_for_and_if(false, true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "v-if without v-for should pass");
    }
}

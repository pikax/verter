//! Rule: require-v-for-key
//!
//! Requires `v-bind:key` with `v-for` directives.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateElement, VForDirective};

/// Elements in iteration expect to have `v-bind:key` directives.
pub struct RequireVForKey;

impl LintRule for RequireVForKey {
    fn name(&self) -> &'static str {
        "require-v-for-key"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_v_for(&self, vfor: &VForDirective, _el: &TemplateElement, ctx: &mut LintContext) {
        if !vfor.has_key {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Elements in iteration expect to have 'v-bind:key' directives.".to_string(),
                vfor.span.start,
                vfor.span.end,
                self.default_severity(),
                DiagnosticSpanKind::Directive,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_analysis::template::*;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(RequireVForKey, template)
    }

    fn make_element_with_v_for(has_key: bool) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![],
            v_for: Some(VForDirective {
                variable: "item".to_string(),
                index: None,
                iterable: "items".to_string(),
                has_key,
                key_expression: if has_key {
                    Some("item.id".to_string())
                } else {
                    None
                },
                key_uses_index: false,
                span: verter_span::Span::new(5, 30),
            }),
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
            span: verter_span::Span::new(0, 50),
            tag_span_end: 50,
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn v_for_without_key_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_for(false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "require-v-for-key");
        assert_eq!(diags[0].severity, Severity::Error);
    }

    #[test]
    fn v_for_with_key_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_for(true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }
}

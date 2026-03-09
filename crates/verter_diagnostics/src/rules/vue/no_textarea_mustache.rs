//! Rule: no-textarea-mustache
//!
//! Disallow mustaches in `<textarea>`.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{BindingUsageKind, TemplateAnalysisSnapshot};

/// Disallow mustaches in `<textarea>`. Use v-model instead.
pub struct NoTextareaMustache;

impl LintRule for NoTextareaMustache {
    fn name(&self) -> &'static str {
        "no-textarea-mustache"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        // Check if any interpolation binding occurs inside a textarea element
        for element in &tpl.elements {
            if element.tag != "textarea" {
                continue;
            }
            // Check binding occurrences that fall within this textarea's span
            for occ in &tpl.binding_occurrences {
                if occ.usage_kind == BindingUsageKind::Interpolation
                    && occ.span.start >= element.span.start
                    && occ.span.end <= element.span.end
                {
                    ctx.report_with_severity(
                        self.name(),
                        self.category().as_str(),
                        "Unexpected mustache in '<textarea>'. Use 'v-model' instead.".to_string(),
                        occ.span.start,
                        occ.span.end,
                        self.default_severity(),
                        DiagnosticSpanKind::Interpolation,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_analysis::template::*;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoTextareaMustache, template)
    }

    #[test]
    fn mustache_in_textarea_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "textarea".to_string(),
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
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                dynamic_classes: vec![],
                has_element_children: false,
                span: verter_span::Span::new(0, 50),
                tag_span_end: 50,
                content_end: 0,
                ..Default::default()
            }],
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "message".to_string(),
                span: verter_span::Span::new(10, 20),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("textarea"));
    }

    #[test]
    fn mustache_in_div_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
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
                nesting_depth: 0,
                parent_tag: None,
                parent_index: None,
                dynamic_classes: vec![],
                has_element_children: false,
                span: verter_span::Span::new(0, 50),
                tag_span_end: 50,
                content_end: 0,
                ..Default::default()
            }],
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "message".to_string(),
                span: verter_span::Span::new(10, 20),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }
}

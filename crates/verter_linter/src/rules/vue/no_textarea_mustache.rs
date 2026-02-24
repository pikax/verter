//! Rule: no-textarea-mustache
//!
//! Disallow mustaches in `<textarea>`.

use crate::context::LintContext;
use crate::diagnostic::Severity;
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
                    && occ.span_start >= element.span_start
                    && occ.span_end <= element.span_end
                {
                    ctx.report_with_severity(
                        self.name(),
                        self.category().as_str(),
                        "Unexpected mustache in '<textarea>'. Use 'v-model' instead.".to_string(),
                        occ.span_start,
                        occ.span_end,
                        self.default_severity(),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoTextareaMustache)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
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
                nesting_depth: 0,
                parent_tag: None,
                span_start: 0,
                span_end: 50,
            }],
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "message".to_string(),
                span_start: 10,
                span_end: 20,
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
                nesting_depth: 0,
                parent_tag: None,
                span_start: 0,
                span_end: 50,
            }],
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "message".to_string(),
                span_start: 10,
                span_end: 20,
                usage_kind: BindingUsageKind::Interpolation,
            }],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }
}

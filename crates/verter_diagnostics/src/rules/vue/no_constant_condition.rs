//! Rule: no-constant-condition
//!
//! Warns when `v-if` or `v-show` is given a constant expression like `true` or `false`.
//! Such conditions never change and suggest the directive should be removed.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::{TemplateDirective, TemplateElement};

pub struct NoConstantCondition;

impl LintRule for NoConstantCondition {
    fn name(&self) -> &'static str {
        "no-constant-condition"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Performance
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_directive(
        &self,
        dir: &TemplateDirective,
        _el: &TemplateElement,
        ctx: &mut LintContext,
    ) {
        if dir.name != "if" && dir.name != "show" {
            return;
        }
        let Some(expr) = &dir.expression else {
            return;
        };
        let trimmed = expr.trim();
        let is_constant = matches!(
            trimmed,
            "true" | "false" | "1" | "0" | "null" | "undefined" | "''" | "\"\"" | "``"
        );
        if !is_constant {
            return;
        }
        ctx.report_with_severity(
            self.name(),
            self.category().as_str(),
            format!(
                "'v-{}' has a constant expression '{trimmed}'. \
                 The condition never changes — remove the directive or use a dynamic value.",
                dir.name
            ),
            dir.span.start,
            dir.span.end,
            self.default_severity(),
            DiagnosticSpanKind::Directive,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoConstantCondition, template)
    }

    fn make_el_with_if(expr: &str) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            has_v_if: true,
            directives: vec![TemplateDirective {
                name: "if".to_string(),
                raw_name: "v-if".to_string(),
                argument: None,
                modifiers: vec![],
                expression: Some(expr.to_string()),
                span: Span::new(5, 15),
                name_end: 0,
                arg_span: None,
                expression_span: None,
                modifier_spans: Vec::new(),
            }],
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn v_if_true_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_if("true")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "v-if=\"true\" should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-constant-condition"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn v_if_false_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_if("false")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "v-if=\"false\" should trigger");
    }

    #[test]
    fn v_if_dynamic_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_if("isVisible")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "dynamic condition should pass");
    }
}

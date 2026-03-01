//! Rule: valid-v-show
//!
//! Ensures `v-show` directives have a valid non-empty expression.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct ValidVShow;

impl LintRule for ValidVShow {
    fn name(&self) -> &'static str {
        "valid-v-show"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_directive(
        &self,
        dir: &TemplateDirective,
        _el: &TemplateElement,
        ctx: &mut LintContext,
    ) {
        if dir.name != "show" {
            return;
        }

        match dir.expression.as_deref() {
            None | Some("") => {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "'v-show' directives require an expression.".to_string(),
                    dir.span.start,
                    dir.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::Directive,
                );
            }
            _ => {}
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(ValidVShow)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_element_with_v_show(expression: Option<&str>) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![TemplateDirective {
                name: "show".to_string(),
                raw_name: "v-show".to_string(),
                argument: None,
                modifiers: vec![],
                expression: expression.map(|s| s.to_string()),
                span: Span::new(5, 20),
            }],
            v_for: None,
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: true,
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
    fn v_show_without_expression_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_show(None)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "v-show without expression should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "valid-v-show"));
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-if"),
            "must not trigger valid-v-if"
        );
    }

    #[test]
    fn v_show_with_empty_expression_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_show(Some(""))],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "v-show with empty expression should trigger"
        );
    }

    #[test]
    fn v_show_with_expression_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_show(Some("visible"))],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "valid v-show should produce no diagnostics"
        );
    }
}

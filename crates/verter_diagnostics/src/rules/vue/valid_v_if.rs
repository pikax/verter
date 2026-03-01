//! Rule: valid-v-if
//!
//! Ensures `v-if` directives have a valid expression and are not mixed with
//! `v-else`/`v-else-if` on the same element.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct ValidVIf;

impl LintRule for ValidVIf {
    fn name(&self) -> &'static str {
        "valid-v-if"
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
        el: &TemplateElement,
        ctx: &mut LintContext,
    ) {
        if dir.name != "if" {
            return;
        }

        // v-if must have a non-empty expression
        match dir.expression.as_deref() {
            None | Some("") => {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "'v-if' directives require an expression.".to_string(),
                    dir.span.start,
                    dir.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::Directive,
                );
            }
            _ => {}
        }

        // v-if and v-else/v-else-if cannot be on the same element
        if el.has_v_else || el.has_v_else_if {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "'v-if' and 'v-else'/'v-else-if' cannot be used on the same element.".to_string(),
                dir.span.start,
                dir.span.end,
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(ValidVIf)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_element_with_v_if(expression: Option<&str>, has_v_else: bool) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives: vec![TemplateDirective {
                name: "if".to_string(),
                raw_name: "v-if".to_string(),
                argument: None,
                modifiers: vec![],
                expression: expression.map(|s| s.to_string()),
                span: Span::new(5, 20),
            }],
            v_for: None,
            v_model: None,
            has_v_if: true,
            has_v_else,
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
    fn v_if_without_expression_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_if(None, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "v-if without expression should trigger");
        assert!(
            diags.iter().any(|d| d.rule == "valid-v-if"),
            "should be valid-v-if"
        );
        assert!(
            diags[0].message.contains("expression"),
            "message should mention expression"
        );
    }

    #[test]
    fn v_if_with_empty_expression_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_if(Some(""), false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "v-if with empty expression should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "valid-v-if"));
    }

    #[test]
    fn v_if_with_v_else_on_same_element_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_if(Some("show"), true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        // v-if+v-else on same element triggers the conflict error
        assert!(
            diags.iter().any(|d| d.message.contains("v-else")),
            "should warn about v-if + v-else on same element"
        );
    }

    #[test]
    fn v_if_with_valid_expression_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element_with_v_if(Some("show"), false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "valid v-if should produce no diagnostics");
    }

    #[test]
    fn non_v_if_directives_not_triggered() {
        let element = TemplateElement {
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
                expression: None,
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
        };
        let template = TemplateAnalysisSnapshot {
            elements: vec![element],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "valid-v-if must not trigger for v-show directives"
        );
    }
}

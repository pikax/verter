//! Rule: no-v-text
//!
//! Disallows the `v-text` directive. Prefer using mustache interpolation
//! (`{{ expr }}`) instead of `v-text="expr"`.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct NoVTextDirective;

impl LintRule for NoVTextDirective {
    fn name(&self) -> &'static str {
        "no-v-text"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_directive(
        &self,
        dir: &TemplateDirective,
        _el: &TemplateElement,
        ctx: &mut LintContext,
    ) {
        if dir.name == "text" {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Prefer mustache interpolation (`{{ expr }}`) over `v-text`.".to_string(),
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

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoVTextDirective)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn v_text_directive_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "text".to_string(),
                    raw_name: "v-text".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: Some("msg".to_string()),
                    span: Span::new(5, 19),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "v-text directive should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-v-text"));
        assert!(
            diags[0].message.contains("interpolation"),
            "message should mention interpolation"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-text"),
            "must not trigger valid-v-text"
        );
    }

    #[test]
    fn v_html_directive_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "html".to_string(),
                    raw_name: "v-html".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: Some("content".to_string()),
                    span: Span::new(5, 25),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "v-html should not trigger no-v-text");
    }

    #[test]
    fn no_directives_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "no directives should pass");
    }
}

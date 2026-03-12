//! Rule: prefer-separate-static-class
//!
//! Prefer using static `class="..."` instead of `:class="'static-class'"`.
//! Dynamic class bindings with string literals should be static attributes.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct PreferSeparateStaticClass;

impl LintRule for PreferSeparateStaticClass {
    fn name(&self) -> &'static str {
        "prefer-separate-static-class"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
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
        if dir.name != "bind" {
            return;
        }
        if dir.argument.as_deref() != Some("class") {
            return;
        }

        let expr = match &dir.expression {
            Some(e) => e.trim(),
            None => return,
        };

        // Check if the expression is a simple string literal
        let is_static_string = (expr.starts_with('\'') && expr.ends_with('\''))
            || (expr.starts_with('"') && expr.ends_with('"'));

        if is_static_string {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Static class binding detected. Use `class=\"...\"` instead of `:class=\"'...'\"` \
                 for plain string values."
                    .to_string(),
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

    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(PreferSeparateStaticClass, template)
    }

    fn make_class_dir(expression: &str) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            directives: vec![TemplateDirective {
                name: "bind".to_string(),
                raw_name: ":class".to_string(),
                argument: Some("class".to_string()),
                modifiers: vec![],
                expression: Some(expression.to_string()),
                span: Span::new(5, 35),
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
    fn static_string_single_quotes_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_class_dir("'my-class'")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "static string in :class should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "prefer-separate-static-class"));
        assert!(
            diags[0].message.contains("class="),
            "message should suggest static class"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn static_string_double_quotes_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_class_dir("\"my-class\"")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "static string in :class with double quotes should trigger"
        );
    }

    #[test]
    fn dynamic_expression_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_class_dir("{ active: isActive }")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "dynamic object expression should pass");
    }

    #[test]
    fn variable_reference_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_class_dir("myClass")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "variable reference should pass");
    }
}

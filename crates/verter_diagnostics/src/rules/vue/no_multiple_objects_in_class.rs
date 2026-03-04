//! Rule: no-multiple-objects-in-class
//!
//! Detect multiple object literals in a `:class` binding. Merging them into a
//! single object is cleaner: `:class="{ a: x, b: y }"` instead of
//! `:class="[{ a: x }, { b: y }]"` with multiple `{`.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct NoMultipleObjectsInClass;

impl LintRule for NoMultipleObjectsInClass {
    fn name(&self) -> &'static str {
        "no-multiple-objects-in-class"
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
        if dir.name != "bind" {
            return;
        }
        if dir.argument.as_deref() != Some("class") {
            return;
        }

        let expr = match &dir.expression {
            Some(e) => e,
            None => return,
        };

        // Count occurrences of `{` in the expression to detect multiple objects
        let brace_count = expr.chars().filter(|&c| c == '{').count();
        if brace_count >= 2 {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "Multiple object literals in `:class` binding. \
                 Consider merging them into a single object for clarity."
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
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoMultipleObjectsInClass)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_class_directive(expression: &str) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            directives: vec![TemplateDirective {
                name: "bind".to_string(),
                raw_name: ":class".to_string(),
                argument: Some("class".to_string()),
                modifiers: vec![],
                expression: Some(expression.to_string()),
                span: Span::new(5, 40),
                name_end: 0,
                arg_span: None,
                expression_span: None,
                modifier_spans: Vec::new(),
            }],
            content_end: 0,
            text_children: Vec::new(),
            ..Default::default()
        }
    }

    #[test]
    fn multiple_objects_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_class_directive("[{ a: x }, { b: y }]")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "multiple objects in :class should trigger"
        );
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-multiple-objects-in-class"));
        assert!(
            diags[0].message.contains("merging"),
            "message should suggest merging"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn single_object_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_class_directive("{ a: x, b: y }")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "single object in :class should pass");
    }

    #[test]
    fn non_class_binding_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                directives: vec![TemplateDirective {
                    name: "bind".to_string(),
                    raw_name: ":style".to_string(),
                    argument: Some("style".to_string()),
                    modifiers: vec![],
                    expression: Some("[{ a: 1 }, { b: 2 }]".to_string()),
                    span: Span::new(5, 40),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                content_end: 0,
                text_children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            ":style with multiple objects should not trigger this rule"
        );
    }
}

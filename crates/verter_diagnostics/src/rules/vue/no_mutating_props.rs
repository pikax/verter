//! Rule: no-mutating-props
//!
//! Disallow mutating component props in template expressions. Props are one-way
//! data flow and should not be assigned to directly.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct NoMutatingProps;

/// Check if an expression mutates a given prop name via assignment or increment/decrement.
fn expression_mutates_prop(expr: &str, prop_name: &str) -> bool {
    // Check for direct assignment: `prop = ...`, `prop += ...`, `prop -= ...`, etc.
    let trimmed = expr.trim();

    // Check for `propName =`, `propName +=`, `propName -=`, etc.
    if let Some(rest) = trimmed.strip_prefix(prop_name) {
        let rest = rest.trim_start();
        if rest.starts_with('=')
            || rest.starts_with("+=")
            || rest.starts_with("-=")
            || rest.starts_with("*=")
            || rest.starts_with("/=")
        {
            // Make sure it's not `==` or `===`
            if rest.starts_with("==") {
                return false;
            }
            return true;
        }
        // Check for `prop++` or `prop--`
        if rest.starts_with("++") || rest.starts_with("--") {
            return true;
        }
    }

    // Check for `++prop` or `--prop`
    if let Some(rest) = trimmed
        .strip_prefix("++")
        .or_else(|| trimmed.strip_prefix("--"))
    {
        if rest.trim_start() == prop_name {
            return true;
        }
    }

    // Check for mutation of nested prop: `prop.x = ...`
    let dot_prefix = format!("{prop_name}.");
    if let Some(rest) = trimmed.strip_prefix(&dot_prefix) {
        // Find the property path, then check for assignment
        let after_path: &str = rest.split([' ', '=', '+', '-']).next().unwrap_or("");
        let rest_after = &rest[after_path.len()..].trim_start();
        if rest_after.starts_with('=') && !rest_after.starts_with("==") {
            return true;
        }
        if rest_after.starts_with("+=")
            || rest_after.starts_with("-=")
            || rest_after.starts_with("*=")
            || rest_after.starts_with("/=")
        {
            return true;
        }
    }

    false
}

impl LintRule for NoMutatingProps {
    fn name(&self) -> &'static str {
        "no-mutating-props"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        if tpl.prop_definitions.is_empty() {
            return;
        }
        let prop_names: Vec<&str> = tpl
            .prop_definitions
            .iter()
            .map(|p| p.name.as_str())
            .collect();

        // Check directive expressions on elements
        for el in &tpl.elements {
            for dir in &el.directives {
                if let Some(ref expr) = dir.expression {
                    for prop_name in &prop_names {
                        if expression_mutates_prop(expr, prop_name) {
                            ctx.report_with_severity(
                                self.name(),
                                self.category().as_str(),
                                format!(
                                    "Unexpected mutation of prop '{prop_name}'. Props are read-only."
                                ),
                                dir.span.start,
                                dir.span.end,
                                self.default_severity(),
                                DiagnosticSpanKind::Directive,
                            );
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoMutatingProps, template)
    }

    fn make_template_with_prop_and_expr(prop_name: &str, expr: &str) -> TemplateAnalysisSnapshot {
        TemplateAnalysisSnapshot {
            prop_definitions: vec![AnalyzedPropDefinition {
                name: prop_name.to_string(),
                type_annotation: None,
                has_default: false,
                is_required: true,
                is_boolean: false,
                used_in_template: true,
                used_in_script: false,
                span: Span::new(0, 10),
            }],
            elements: vec![TemplateElement {
                tag: "button".to_string(),
                directives: vec![TemplateDirective {
                    name: "on".to_string(),
                    raw_name: "@click".to_string(),
                    argument: Some("click".to_string()),
                    modifiers: vec![],
                    expression: Some(expr.to_string()),
                    span: Span::new(20, 50),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                span: Span::new(15, 60),
                tag_span_end: 55,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn prop_assignment_reports() {
        let template = make_template_with_prop_and_expr("count", "count = 5");
        let diags = run(&template);
        assert!(!diags.is_empty(), "prop assignment should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-mutating-props"));
        assert!(
            diags[0].message.contains("count"),
            "message should mention the prop"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn prop_increment_reports() {
        let template = make_template_with_prop_and_expr("count", "count++");
        let diags = run(&template);
        assert!(!diags.is_empty(), "prop++ should trigger");
    }

    #[test]
    fn prop_nested_mutation_reports() {
        let template = make_template_with_prop_and_expr("obj", "obj.x = 5");
        let diags = run(&template);
        assert!(!diags.is_empty(), "prop.x = 5 should trigger");
    }

    #[test]
    fn prop_read_passes() {
        let template = make_template_with_prop_and_expr("count", "console.log(count)");
        let diags = run(&template);
        assert!(diags.is_empty(), "reading prop should pass");
    }

    #[test]
    fn prop_equality_check_passes() {
        let template = make_template_with_prop_and_expr("count", "count === 5");
        let diags = run(&template);
        assert!(diags.is_empty(), "prop === 5 should pass");
    }

    #[test]
    fn non_prop_assignment_passes() {
        let template = TemplateAnalysisSnapshot {
            prop_definitions: vec![AnalyzedPropDefinition {
                name: "count".to_string(),
                type_annotation: None,
                has_default: false,
                is_required: true,
                is_boolean: false,
                used_in_template: true,
                used_in_script: false,
                span: Span::new(0, 10),
            }],
            elements: vec![TemplateElement {
                tag: "button".to_string(),
                directives: vec![TemplateDirective {
                    name: "on".to_string(),
                    raw_name: "@click".to_string(),
                    argument: Some("click".to_string()),
                    modifiers: vec![],
                    expression: Some("localVar = 5".to_string()),
                    span: Span::new(20, 45),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                span: Span::new(15, 55),
                tag_span_end: 50,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "assigning non-prop should pass");
    }
}

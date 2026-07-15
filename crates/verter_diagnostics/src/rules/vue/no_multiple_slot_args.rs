//! Rule: no-multiple-slot-args
//!
//! Disallow `v-slot` with compound (destructured or multiple) arguments on
//! the same element, which is a syntax error in Vue.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::{TemplateDirective, TemplateElement};

pub struct NoMultipleSlotArgs;

impl LintRule for NoMultipleSlotArgs {
    fn name(&self) -> &'static str {
        "no-multiple-slot-args"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        // Count v-slot directives on this element
        let slot_directives: Vec<_> = el.directives.iter().filter(|d| d.name == "slot").collect();

        if slot_directives.len() > 1 {
            for dir in &slot_directives[1..] {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "An element can only have one 'v-slot' directive.".to_string(),
                    dir.span.start,
                    dir.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::Directive,
                );
            }
        }
    }

    fn check_directive(
        &self,
        dir: &TemplateDirective,
        _el: &TemplateElement,
        ctx: &mut LintContext,
    ) {
        if dir.name != "slot" {
            return;
        }
        // Check for compound args with commas outside destructuring
        if let Some(ref expr) = dir.expression {
            let trimmed = expr.trim();
            // A top-level comma (not inside braces/brackets) suggests multiple args
            let mut depth = 0i32;
            for byte in trimmed.bytes() {
                match byte {
                    b'{' | b'[' | b'(' => depth += 1,
                    b'}' | b']' | b')' => depth -= 1,
                    b',' if depth == 0 => {
                        ctx.report_with_severity(
                            self.name(),
                            self.category().as_str(),
                            "'v-slot' binding should not have multiple arguments at the top level."
                                .to_string(),
                            dir.span.start,
                            dir.span.end,
                            self.default_severity(),
                            DiagnosticSpanKind::Directive,
                        );
                        return;
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoMultipleSlotArgs, template)
    }

    #[test]
    fn multiple_v_slot_on_element_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "template".to_string(),
                directives: vec![
                    TemplateDirective {
                        name: "slot".to_string(),
                        raw_name: "#default".to_string(),
                        argument: Some("default".to_string()),
                        modifiers: vec![],
                        expression: Some("{ item }".to_string()),
                        span: Span::new(10, 30),
                        name_end: 0,
                        arg_span: None,
                        expression_span: None,
                        modifier_spans: Vec::new(),
                    },
                    TemplateDirective {
                        name: "slot".to_string(),
                        raw_name: "#header".to_string(),
                        argument: Some("header".to_string()),
                        modifiers: vec![],
                        expression: None,
                        span: Span::new(31, 45),
                        name_end: 0,
                        arg_span: None,
                        expression_span: None,
                        modifier_spans: Vec::new(),
                    },
                ],
                parent_index: Some(0),
                span: Span::new(5, 60),
                tag_span_end: 50,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "multiple v-slot on one element should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "no-multiple-slot-args"));
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-slot"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn single_v_slot_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "template".to_string(),
                directives: vec![TemplateDirective {
                    name: "slot".to_string(),
                    raw_name: "#default".to_string(),
                    argument: Some("default".to_string()),
                    modifiers: vec![],
                    expression: Some("{ item }".to_string()),
                    span: Span::new(10, 30),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                parent_index: Some(0),
                span: Span::new(5, 50),
                tag_span_end: 35,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "single v-slot should pass");
    }

    #[test]
    fn compound_args_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "template".to_string(),
                directives: vec![TemplateDirective {
                    name: "slot".to_string(),
                    raw_name: "#default".to_string(),
                    argument: Some("default".to_string()),
                    modifiers: vec![],
                    expression: Some("item, index".to_string()),
                    span: Span::new(10, 40),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                parent_index: Some(0),
                span: Span::new(5, 60),
                tag_span_end: 45,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "v-slot with top-level comma args should trigger"
        );
    }

    #[test]
    fn destructured_args_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "template".to_string(),
                directives: vec![TemplateDirective {
                    name: "slot".to_string(),
                    raw_name: "#default".to_string(),
                    argument: Some("default".to_string()),
                    modifiers: vec![],
                    expression: Some("{ a, b }".to_string()),
                    span: Span::new(10, 35),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                parent_index: Some(0),
                span: Span::new(5, 50),
                tag_span_end: 40,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "destructured args in v-slot should pass");
    }
}

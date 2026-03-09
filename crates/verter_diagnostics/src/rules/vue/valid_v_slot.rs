//! Rule: valid-v-slot
//!
//! `v-slot` must only be used on components or `<template>` elements.
//! No duplicate `v-slot` on the same element.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct ValidVSlot;

impl LintRule for ValidVSlot {
    fn name(&self) -> &'static str {
        "valid-v-slot"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        let slot_directives: Vec<_> = el.directives.iter().filter(|d| d.name == "slot").collect();

        if slot_directives.is_empty() {
            return;
        }

        // v-slot is only valid on components or <template>
        if !el.is_component && el.tag != "template" {
            for dir in &slot_directives {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "'v-slot' can only be used on components or '<template>' elements, not '<{}>'.",
                        el.tag
                    ),
                    dir.span.start,
                    dir.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::Directive,
                );
            }
        }

        // Duplicate v-slot directives
        if slot_directives.len() > 1 {
            for dir in &slot_directives[1..] {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "Duplicate 'v-slot' directive on the same element.".to_string(),
                    dir.span.start,
                    dir.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::Directive,
                );
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
        crate::test_support::run_template_rule(ValidVSlot, template)
    }

    #[test]
    fn v_slot_on_html_element_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                is_component: false,
                directives: vec![TemplateDirective {
                    name: "slot".to_string(),
                    raw_name: "v-slot".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: None,
                    span: Span::new(5, 11),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "v-slot on div should trigger");
        assert!(diags.iter().any(|d| d.rule == "valid-v-slot"));
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-once"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn v_slot_on_component_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "MyComponent".to_string(),
                is_component: true,
                directives: vec![TemplateDirective {
                    name: "slot".to_string(),
                    raw_name: "v-slot".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: None,
                    span: Span::new(5, 11),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "v-slot on component should pass");
    }

    #[test]
    fn v_slot_on_template_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "template".to_string(),
                is_component: false,
                directives: vec![TemplateDirective {
                    name: "slot".to_string(),
                    raw_name: "#default".to_string(),
                    argument: Some("default".to_string()),
                    modifiers: vec![],
                    expression: None,
                    span: Span::new(10, 18),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "v-slot on template should pass");
    }

    #[test]
    fn duplicate_v_slot_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "MyComponent".to_string(),
                is_component: true,
                directives: vec![
                    TemplateDirective {
                        name: "slot".to_string(),
                        raw_name: "#default".to_string(),
                        argument: Some("default".to_string()),
                        modifiers: vec![],
                        expression: None,
                        span: Span::new(5, 13),
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
                        span: Span::new(14, 21),
                        name_end: 0,
                        arg_span: None,
                        expression_span: None,
                        modifier_spans: Vec::new(),
                    },
                ],
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "duplicate v-slot should trigger");
        assert!(diags.iter().any(|d| d.message.contains("Duplicate")));
    }
}

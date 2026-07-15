//! Rule: no-lone-template
//!
//! A `<template>` element without `v-if`, `v-for`, or `v-slot` is useless —
//! it renders nothing on its own and adds unnecessary nesting.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateElement;

pub struct NoLoneTemplate;

impl LintRule for NoLoneTemplate {
    fn name(&self) -> &'static str {
        "no-lone-template"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if el.tag != "template" {
            return;
        }
        // Root <template> is the SFC wrapper, skip it (parent_index is None)
        if el.parent_index.is_none() {
            return;
        }
        // If it has v-if, v-else-if, v-else, v-for, or v-slot, it's useful
        if el.has_v_if || el.has_v_else_if || el.has_v_else {
            return;
        }
        if el.v_for.is_some() {
            return;
        }
        // Check for v-slot directive
        let has_slot = el.directives.iter().any(|d| d.name == "slot");
        if has_slot {
            return;
        }
        ctx.report_with_severity(
            self.name(),
            self.category().as_str(),
            "'<template>' without 'v-if', 'v-for', or 'v-slot' is useless.".to_string(),
            el.span.start,
            el.tag_span_end,
            self.default_severity(),
            DiagnosticSpanKind::ElementOpenTag,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoLoneTemplate, template)
    }

    #[test]
    fn lone_template_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "template".to_string(),
                parent_index: Some(0),
                parent_tag: Some("div".to_string()),
                span: Span::new(5, 40),
                tag_span_end: 15,
                content_end: 0,
                text_children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "lone <template> should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-lone-template"));
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-once"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn template_with_v_if_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "template".to_string(),
                has_v_if: true,
                parent_index: Some(0),
                parent_tag: Some("div".to_string()),
                span: Span::new(5, 40),
                tag_span_end: 15,
                content_end: 0,
                text_children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "<template v-if> should pass");
    }

    #[test]
    fn template_with_v_for_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "template".to_string(),
                v_for: Some(VForDirective {
                    variable: "item".to_string(),
                    index: None,
                    iterable: "items".to_string(),
                    has_key: false,
                    key_expression: None,
                    key_uses_index: false,
                    span: Span::new(15, 35),
                }),
                parent_index: Some(0),
                parent_tag: Some("div".to_string()),
                span: Span::new(5, 40),
                tag_span_end: 35,
                content_end: 0,
                text_children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "<template v-for> should pass");
    }

    #[test]
    fn template_with_v_slot_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "template".to_string(),
                directives: vec![TemplateDirective {
                    name: "slot".to_string(),
                    raw_name: "#default".to_string(),
                    argument: Some("default".to_string()),
                    modifiers: vec![],
                    expression: None,
                    span: Span::new(15, 25),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                parent_index: Some(0),
                parent_tag: Some("MyComp".to_string()),
                span: Span::new(5, 50),
                tag_span_end: 30,
                content_end: 0,
                text_children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "<template #default> should pass");
    }

    #[test]
    fn root_template_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "template".to_string(),
                parent_index: None,
                span: Span::new(0, 50),
                tag_span_end: 10,
                content_end: 0,
                text_children: Vec::new(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "root <template> should pass");
    }
}

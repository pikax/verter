//! Rule: no-useless-template-attributes
//!
//! Disallows static attributes (non-structural) on `<template>` elements.
//! Structural directives (v-if, v-else, v-for, v-slot, #slot) are allowed;
//! everything else (e.g., `class="foo"`) is useless on `<template>`.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateElement;

/// Directive names that are structural and allowed on `<template>`.
const ALLOWED_DIRECTIVE_NAMES: &[&str] = &["if", "else", "else-if", "for", "slot", "once", "key"];

pub struct NoUselessTemplateAttrs;

impl LintRule for NoUselessTemplateAttrs {
    fn name(&self) -> &'static str {
        "no-useless-template-attributes"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if el.tag != "template" {
            return;
        }

        // Static attributes on <template> are useless
        for attr in &el.attributes {
            // `key` as an attribute is fine (v-for key)
            if attr.name == "key" {
                continue;
            }
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "Attribute '{}' on '<template>' is useless and has no effect.",
                    attr.name
                ),
                attr.span.start,
                attr.span.end,
                self.default_severity(),
                DiagnosticSpanKind::Directive,
            );
        }

        // Directives that are not structural
        for dir in &el.directives {
            if ALLOWED_DIRECTIVE_NAMES.contains(&dir.name.as_str()) {
                continue;
            }
            // v-bind (:class, etc.) and v-on (@event) are not structural on <template>
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "Directive '{}' on '<template>' is useless and has no effect.",
                    dir.raw_name
                ),
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

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoUselessTemplateAttrs, template)
    }

    fn make_template_el(
        attrs: Vec<TemplateAttribute>,
        dirs: Vec<TemplateDirective>,
    ) -> TemplateElement {
        TemplateElement {
            tag: "template".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: attrs,
            directives: dirs,
            v_for: None,
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_bare_text: false,
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 50,
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn class_attr_on_template_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_template_el(
                vec![TemplateAttribute {
                    name: "class".to_string(),
                    value: Some("foo".to_string()),
                    is_dynamic: false,
                    span: Span::new(10, 22),
                    name_end: 0,
                    value_span: None,
                }],
                vec![],
            )],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "class on <template> should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-useless-template-attributes"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger no-v-html"
        );
    }

    #[test]
    fn v_if_on_template_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_template_el(
                vec![],
                vec![TemplateDirective {
                    name: "if".to_string(),
                    raw_name: "v-if".to_string(),
                    argument: None,
                    modifiers: vec![],
                    expression: Some("show".to_string()),
                    span: Span::new(10, 22),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
            )],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "v-if on <template> should pass");
    }

    #[test]
    fn v_slot_on_template_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_template_el(
                vec![],
                vec![TemplateDirective {
                    name: "slot".to_string(),
                    raw_name: "v-slot:header".to_string(),
                    argument: Some("header".to_string()),
                    modifiers: vec![],
                    expression: None,
                    span: Span::new(10, 30),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
            )],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "v-slot on <template> should pass");
    }

    #[test]
    fn v_bind_class_on_template_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_template_el(
                vec![],
                vec![TemplateDirective {
                    name: "bind".to_string(),
                    raw_name: ":class".to_string(),
                    argument: Some("class".to_string()),
                    modifiers: vec![],
                    expression: Some("myClass".to_string()),
                    span: Span::new(10, 25),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
            )],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), ":class on <template> should trigger");
    }

    #[test]
    fn non_template_div_passes() {
        let element = TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![TemplateAttribute {
                name: "class".to_string(),
                value: Some("foo".to_string()),
                is_dynamic: false,
                span: Span::new(5, 17),
                name_end: 0,
                value_span: None,
            }],
            directives: vec![],
            v_for: None,
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_bare_text: false,
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 50,
            content_end: 0,
            ..Default::default()
        };
        let template = TemplateAnalysisSnapshot {
            elements: vec![element],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "class on <div> must not trigger this rule"
        );
    }
}

//! Rule: no-deprecated-scope-attribute
//!
//! The `scope` attribute on `<template>` was deprecated in Vue 2.5 and removed in Vue 3.
//! Use `v-slot` with destructuring instead.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateElement;

pub struct NoDeprecatedScopeAttribute;

impl LintRule for NoDeprecatedScopeAttribute {
    fn name(&self) -> &'static str {
        "no-deprecated-scope-attribute"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        for attr in &el.attributes {
            if attr.is_dynamic {
                continue;
            }
            if attr.name != "scope" {
                continue;
            }
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "The 'scope' attribute is deprecated. Use 'v-slot' with destructuring instead."
                    .to_string(),
                attr.span.start,
                attr.span.end,
                self.default_severity(),
                DiagnosticSpanKind::Attribute,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoDeprecatedScopeAttribute, template)
    }

    fn make_el_with_attr(name: &str) -> TemplateElement {
        TemplateElement {
            tag: "template".to_string(),
            attributes: vec![TemplateAttribute {
                name: name.to_string(),
                value: Some("{ item }".to_string()),
                is_dynamic: false,
                span: Span::new(10, 30),
                name_end: 0,
                value_span: None,
            }],
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn scope_attribute_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el_with_attr("scope")],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "scope attribute should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-deprecated-scope-attribute"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn v_slot_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "template".to_string(),
                attributes: vec![],
                directives: vec![TemplateDirective {
                    name: "slot".to_string(),
                    raw_name: "v-slot:default".to_string(),
                    argument: Some("default".to_string()),
                    modifiers: vec![],
                    expression: Some("{ item }".to_string()),
                    span: Span::new(10, 30),
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
        assert!(diags.is_empty(), "v-slot should pass");
    }
}

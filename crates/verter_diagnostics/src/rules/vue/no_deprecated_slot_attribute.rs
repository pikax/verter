//! Rule: no-deprecated-slot-attribute
//!
//! The static `slot` attribute (e.g., `<div slot="header">`) was deprecated in Vue 2.6
//! and removed in Vue 3. Use `v-slot` on `<template>` instead.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct NoDeprecatedSlotAttribute;

impl LintRule for NoDeprecatedSlotAttribute {
    fn name(&self) -> &'static str {
        "no-deprecated-slot-attribute"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        for attr in &el.attributes {
            if attr.is_dynamic {
                continue;
            }
            if attr.name != "slot" {
                continue;
            }
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "The 'slot' attribute is deprecated. Use 'v-slot' on <template> instead."
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
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDeprecatedSlotAttribute)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn slot_attribute_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                attributes: vec![TemplateAttribute {
                    name: "slot".to_string(),
                    value: Some("header".to_string()),
                    is_dynamic: false,
                    span: Span::new(5, 18),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "static slot attribute should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-deprecated-slot-attribute"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger no-v-html"
        );
    }

    #[test]
    fn v_slot_directive_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "template".to_string(),
                directives: vec![TemplateDirective {
                    name: "slot".to_string(),
                    raw_name: "#header".to_string(),
                    argument: Some("header".to_string()),
                    modifiers: vec![],
                    expression: None,
                    span: Span::new(10, 20),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "v-slot directive should pass");
    }
}

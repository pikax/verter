//! Rule: no-template-key
//!
//! Disallow `key` attribute on `<template>`.

use crate::context::LintContext;
use crate::diagnostic::Severity;
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

/// Disallow `key` attribute on `<template>`.
pub struct NoTemplateKey;

impl LintRule for NoTemplateKey {
    fn name(&self) -> &'static str {
        "no-template-key"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        if el.tag == "template" && el.v_for.is_none() {
            for attr in &el.attributes {
                if attr.name == "key" {
                    ctx.report_with_severity(
                        self.name(),
                        self.category().as_str(),
                        "'<template>' cannot be keyed. Place the key on real elements instead."
                            .to_string(),
                        attr.span_start,
                        attr.span_end,
                        self.default_severity(),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoTemplateKey)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_element(tag: &str, has_key: bool, has_v_for: bool) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: if has_key {
                vec![TemplateAttribute {
                    name: "key".to_string(),
                    value: Some("k".to_string()),
                    is_dynamic: true,
                    span_start: 10,
                    span_end: 20,
                }]
            } else {
                vec![]
            },
            directives: vec![],
            v_for: if has_v_for {
                Some(VForDirective {
                    variable: "item".to_string(),
                    index: None,
                    iterable: "items".to_string(),
                    has_key: true,
                    key_expression: Some("item.id".to_string()),
                    key_uses_index: false,
                    span_start: 5,
                    span_end: 30,
                })
            } else {
                None
            },
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            nesting_depth: 0,
            parent_tag: None,
            span_start: 0,
            span_end: 50,
        }
    }

    #[test]
    fn template_with_key_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element("template", true, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "no-template-key");
    }

    #[test]
    fn template_with_v_for_and_key_passes() {
        // template + v-for + key is valid
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element("template", true, true)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }

    #[test]
    fn div_with_key_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element("div", true, false)],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty());
    }
}

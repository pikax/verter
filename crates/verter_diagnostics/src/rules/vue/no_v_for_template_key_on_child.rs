//! Rule: no-v-for-template-key-on-child
//!
//! When using `<template v-for>`, the `:key` should be on the `<template>` element
//! itself, not on a child element. Placing it on a child is a Vue 2 pattern
//! that is unnecessary in Vue 3.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateElement;

pub struct NoVForTemplateKeyOnChild;

impl LintRule for NoVForTemplateKeyOnChild {
    fn name(&self) -> &'static str {
        "no-v-for-template-key-on-child"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        // Check if parent is <template> with v-for
        let parent_is_template_v_for = el
            .parent_tag
            .as_deref()
            .map(|t| t == "template")
            .unwrap_or(false);

        if !parent_is_template_v_for {
            return;
        }

        // Check if this element has a :key directive (v-bind with argument "key")
        let has_key_directive = el
            .directives
            .iter()
            .any(|d| d.name == "bind" && d.argument.as_deref() == Some("key"));

        // Also check for static key attribute
        let has_key_attr = el.attributes.iter().any(|a| a.name == "key");

        if has_key_directive || has_key_attr {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                "':key' should be placed on the '<template>' element with 'v-for', not on a child element."
                    .to_string(),
                el.span.start,
                el.tag_span_end,
                self.default_severity(),
                DiagnosticSpanKind::ElementOpenTag,
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
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoVForTemplateKeyOnChild)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn child_key_inside_template_v_for_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![
                TemplateElement {
                    tag: "template".to_string(),
                    v_for: Some(VForDirective {
                        variable: "item".to_string(),
                        index: None,
                        iterable: "items".to_string(),
                        has_key: false,
                        key_expression: None,
                        key_uses_index: false,
                        span: Span::new(10, 35),
                    }),
                    span: Span::new(0, 80),
                    tag_span_end: 40,
                    content_end: 0,
                    ..Default::default()
                },
                TemplateElement {
                    tag: "div".to_string(),
                    parent_tag: Some("template".to_string()),
                    parent_index: Some(0),
                    directives: vec![TemplateDirective {
                        name: "bind".to_string(),
                        raw_name: ":key".to_string(),
                        argument: Some("key".to_string()),
                        modifiers: vec![],
                        expression: Some("item.id".to_string()),
                        span: Span::new(45, 60),
                        name_end: 0,
                        arg_span: None,
                        expression_span: None,
                        modifier_spans: Vec::new(),
                    }],
                    span: Span::new(41, 70),
                    tag_span_end: 62,
                    content_end: 0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            ":key on child of <template v-for> should trigger"
        );
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-v-for-template-key-on-child"));
        assert!(
            !diags.iter().any(|d| d.rule == "require-v-for-key"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn key_on_non_template_parent_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                parent_tag: Some("ul".to_string()),
                parent_index: Some(0),
                directives: vec![TemplateDirective {
                    name: "bind".to_string(),
                    raw_name: ":key".to_string(),
                    argument: Some("key".to_string()),
                    modifiers: vec![],
                    expression: Some("item.id".to_string()),
                    span: Span::new(10, 25),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                span: Span::new(5, 40),
                tag_span_end: 30,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            ":key on child of non-template parent should pass"
        );
    }

    #[test]
    fn child_without_key_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                parent_tag: Some("template".to_string()),
                parent_index: Some(0),
                span: Span::new(41, 70),
                tag_span_end: 46,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "child without :key should pass");
    }
}

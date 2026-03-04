//! Rule: v-on-event-hyphenation
//!
//! Enforces kebab-case for event names in `v-on` directives on component elements.
//! For example, `@myEvent="handler"` should be `@my-event="handler"`.
//! Only applies to component elements, not native HTML elements.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct VOnEventHyphenation;

/// Returns true if the string contains any ASCII uppercase letter.
fn has_uppercase(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_uppercase())
}

/// Converts a camelCase string to kebab-case.
fn to_kebab_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                result.push('-');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

impl LintRule for VOnEventHyphenation {
    fn name(&self) -> &'static str {
        "v-on-event-hyphenation"
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
        el: &TemplateElement,
        ctx: &mut LintContext,
    ) {
        // Only check v-on / @ directives
        if dir.name != "on" {
            return;
        }
        // Only check on component elements
        if !el.is_component {
            return;
        }
        // Check the event argument for uppercase
        if let Some(ref arg) = dir.argument {
            if has_uppercase(arg) {
                let kebab = to_kebab_case(arg);
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Event '@{arg}' should be '@{kebab}'. Use kebab-case for v-on events on components.",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(VOnEventHyphenation)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_comp_with_event(dir: TemplateDirective) -> TemplateElement {
        TemplateElement {
            tag: "MyComp".to_string(),
            is_component: true,
            directives: vec![dir],
            span: Span::new(0, 50),
            tag_span_end: 45,
            content_end: 0,
            ..Default::default()
        }
    }

    #[test]
    fn camel_case_event_on_component_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_comp_with_event(TemplateDirective {
                name: "on".to_string(),
                raw_name: "@myEvent".to_string(),
                argument: Some("myEvent".to_string()),
                modifiers: vec![],
                expression: Some("handler".to_string()),
                span: Span::new(10, 28),
                name_end: 0,
                arg_span: None,
                expression_span: None,
                modifier_spans: Vec::new(),
            })],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "camelCase event on component should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "v-on-event-hyphenation"));
        assert!(
            diags[0].message.contains("my-event"),
            "message should suggest kebab-case"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn kebab_case_event_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_comp_with_event(TemplateDirective {
                name: "on".to_string(),
                raw_name: "@my-event".to_string(),
                argument: Some("my-event".to_string()),
                modifiers: vec![],
                expression: Some("handler".to_string()),
                span: Span::new(10, 30),
                name_end: 0,
                arg_span: None,
                expression_span: None,
                modifier_spans: Vec::new(),
            })],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "kebab-case event should pass");
    }

    #[test]
    fn camel_case_event_on_native_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "div".to_string(),
                is_component: false,
                directives: vec![TemplateDirective {
                    name: "on".to_string(),
                    raw_name: "@myEvent".to_string(),
                    argument: Some("myEvent".to_string()),
                    modifiers: vec![],
                    expression: Some("handler".to_string()),
                    span: Span::new(5, 25),
                    name_end: 0,
                    arg_span: None,
                    expression_span: None,
                    modifier_spans: Vec::new(),
                }],
                span: Span::new(0, 40),
                tag_span_end: 35,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "camelCase event on native element should not trigger"
        );
    }

    #[test]
    fn non_on_directive_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_comp_with_event(TemplateDirective {
                name: "bind".to_string(),
                raw_name: ":myProp".to_string(),
                argument: Some("myProp".to_string()),
                modifiers: vec![],
                expression: Some("val".to_string()),
                span: Span::new(10, 25),
                name_end: 0,
                arg_span: None,
                expression_span: None,
                modifier_spans: Vec::new(),
            })],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "v-bind directive should not trigger v-on-event-hyphenation"
        );
    }
}

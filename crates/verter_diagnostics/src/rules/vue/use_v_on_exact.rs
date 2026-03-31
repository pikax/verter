//! Rule: use-v-on-exact
//!
//! When an element has multiple `v-on` handlers for the same base event
//! (e.g., `@click` and `@click.ctrl`), the handlers without a system modifier
//! should use `.exact` to prevent unintended triggering.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateElement;

pub struct UseVOnExact;

/// System modifier keys that narrow event scope.
const SYSTEM_MODIFIERS: &[&str] = &["ctrl", "shift", "alt", "meta"];

impl LintRule for UseVOnExact {
    fn name(&self) -> &'static str {
        "use-v-on-exact"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_element(&self, el: &TemplateElement, ctx: &mut LintContext) {
        // Group v-on directives by their argument (event name)
        let on_directives: Vec<_> = el
            .directives
            .iter()
            .filter(|d| d.name == "on" && d.argument.is_some())
            .collect();

        // For each event name, check if there are multiple handlers
        // with different system modifier sets
        let mut seen_events: std::collections::HashMap<&str, Vec<usize>> =
            std::collections::HashMap::new();
        for (i, dir) in on_directives.iter().enumerate() {
            if let Some(ref arg) = dir.argument {
                seen_events.entry(arg.as_str()).or_default().push(i);
            }
        }

        for indices in seen_events.values() {
            if indices.len() < 2 {
                continue;
            }
            // Check if any handler has a system modifier
            let has_system_modified = indices.iter().any(|&i| {
                on_directives[i]
                    .modifiers
                    .iter()
                    .any(|m| SYSTEM_MODIFIERS.contains(&m.as_str()))
            });
            if !has_system_modified {
                continue;
            }
            // Flag handlers without system modifiers and without .exact
            for &i in indices {
                let dir = on_directives[i];
                let has_system = dir
                    .modifiers
                    .iter()
                    .any(|m| SYSTEM_MODIFIERS.contains(&m.as_str()));
                let has_exact = dir.modifiers.iter().any(|m| m == "exact");
                if !has_system && !has_exact {
                    ctx.report_with_severity(
                        self.name(),
                        self.category().as_str(),
                        "Use '.exact' modifier when there are multiple handlers for the same event with system modifiers."
                            .to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(UseVOnExact, template)
    }

    #[test]
    fn click_and_click_ctrl_without_exact_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "button".to_string(),
                directives: vec![
                    TemplateDirective {
                        name: "on".to_string(),
                        raw_name: "@click".to_string(),
                        argument: Some("click".to_string()),
                        modifiers: vec![],
                        expression: Some("handleClick".to_string()),
                        span: Span::new(8, 30),
                        name_end: 0,
                        arg_span: None,
                        expression_span: None,
                        modifier_spans: Vec::new(),
                    },
                    TemplateDirective {
                        name: "on".to_string(),
                        raw_name: "@click.ctrl".to_string(),
                        argument: Some("click".to_string()),
                        modifiers: vec!["ctrl".to_string()],
                        expression: Some("handleCtrlClick".to_string()),
                        span: Span::new(31, 60),
                        name_end: 0,
                        arg_span: None,
                        expression_span: None,
                        modifier_spans: Vec::new(),
                    },
                ],
                span: Span::new(0, 70),
                tag_span_end: 65,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "@click without .exact alongside @click.ctrl should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "use-v-on-exact"));
        assert!(
            !diags.iter().any(|d| d.rule == "valid-v-on"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn click_exact_and_click_ctrl_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "button".to_string(),
                directives: vec![
                    TemplateDirective {
                        name: "on".to_string(),
                        raw_name: "@click.exact".to_string(),
                        argument: Some("click".to_string()),
                        modifiers: vec!["exact".to_string()],
                        expression: Some("handleClick".to_string()),
                        span: Span::new(8, 35),
                        name_end: 0,
                        arg_span: None,
                        expression_span: None,
                        modifier_spans: Vec::new(),
                    },
                    TemplateDirective {
                        name: "on".to_string(),
                        raw_name: "@click.ctrl".to_string(),
                        argument: Some("click".to_string()),
                        modifiers: vec!["ctrl".to_string()],
                        expression: Some("handleCtrlClick".to_string()),
                        span: Span::new(36, 65),
                        name_end: 0,
                        arg_span: None,
                        expression_span: None,
                        modifier_spans: Vec::new(),
                    },
                ],
                span: Span::new(0, 75),
                tag_span_end: 70,
                content_end: 0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "@click.exact + @click.ctrl should pass");
    }

    #[test]
    fn single_handler_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "button".to_string(),
                directives: vec![TemplateDirective {
                    name: "on".to_string(),
                    raw_name: "@click".to_string(),
                    argument: Some("click".to_string()),
                    modifiers: vec![],
                    expression: Some("handleClick".to_string()),
                    span: Span::new(8, 30),
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
        assert!(diags.is_empty(), "single event handler should pass");
    }
}

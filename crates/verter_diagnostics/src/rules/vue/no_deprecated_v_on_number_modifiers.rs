//! Rule: no-deprecated-v-on-number-modifiers
//!
//! Using number key codes (e.g., `@keydown.13`) as `v-on` modifiers is deprecated in Vue 3.
//! Use named key modifiers instead: `@keydown.enter`.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::{TemplateDirective, TemplateElement};

pub struct NoDeprecatedVOnNumberModifiers;

impl LintRule for NoDeprecatedVOnNumberModifiers {
    fn name(&self) -> &'static str {
        "no-deprecated-v-on-number-modifiers"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_directive(
        &self,
        dir: &TemplateDirective,
        _el: &TemplateElement,
        ctx: &mut LintContext,
    ) {
        if dir.name != "on" {
            return;
        }
        for modifier in &dir.modifiers {
            // A modifier is numeric if it parses as a non-negative integer
            if modifier.chars().all(|c| c.is_ascii_digit()) && !modifier.is_empty() {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Numeric key code modifier '.{modifier}' is deprecated in Vue 3. \
                         Use named key modifiers instead (e.g., '.enter', '.space').",
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

    use verter_analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(NoDeprecatedVOnNumberModifiers, template)
    }

    #[test]
    fn numeric_modifier_reports() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "input".to_string(),
                directives: vec![TemplateDirective {
                    name: "on".to_string(),
                    raw_name: "@keydown.13".to_string(),
                    argument: Some("keydown".to_string()),
                    modifiers: vec!["13".to_string()],
                    expression: Some("handler".to_string()),
                    span: Span::new(7, 20),
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
        assert!(
            !diags.is_empty(),
            "numeric key code modifier should trigger"
        );
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-deprecated-v-on-number-modifiers"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn named_modifier_passes() {
        let template = TemplateAnalysisSnapshot {
            elements: vec![TemplateElement {
                tag: "input".to_string(),
                directives: vec![TemplateDirective {
                    name: "on".to_string(),
                    raw_name: "@keydown.enter".to_string(),
                    argument: Some("keydown".to_string()),
                    modifiers: vec!["enter".to_string()],
                    expression: Some("handler".to_string()),
                    span: Span::new(7, 23),
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
        assert!(diags.is_empty(), "named key modifier should pass");
    }
}

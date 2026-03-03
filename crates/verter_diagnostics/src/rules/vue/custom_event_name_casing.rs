//! Rule: custom-event-name-casing
//!
//! Enforces camelCase for custom event names. Event handler names containing
//! hyphens (kebab-case) should be written in camelCase instead.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct CustomEventNameCasing;

impl LintRule for CustomEventNameCasing {
    fn name(&self) -> &'static str {
        "custom-event-name-casing"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for handler in &tpl.event_handlers {
            // Skip native DOM events (they never have hyphens anyway)
            // Only check custom events which may use kebab-case
            if handler.event_name.contains('-') {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Custom event '{}' should use camelCase instead of kebab-case.",
                        handler.event_name
                    ),
                    handler.span.start,
                    handler.span.end,
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

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(CustomEventNameCasing)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_handler(event_name: &str) -> TemplateEventHandler {
        TemplateEventHandler {
            event_name: event_name.to_string(),
            handler_binding: Some("handler".to_string()),
            is_inline: false,
            target_tag: "MyComponent".to_string(),
            span: Span::new(10, 30),
        }
    }

    #[test]
    fn kebab_case_event_reports() {
        let template = TemplateAnalysisSnapshot {
            event_handlers: vec![make_handler("my-event")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(!diags.is_empty(), "kebab-case event should trigger");
        assert!(diags.iter().any(|d| d.rule == "custom-event-name-casing"));
        assert!(
            diags[0].message.contains("camelCase"),
            "message should mention camelCase"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn camel_case_event_passes() {
        let template = TemplateAnalysisSnapshot {
            event_handlers: vec![make_handler("myEvent")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "camelCase event should pass");
    }

    #[test]
    fn simple_event_passes() {
        let template = TemplateAnalysisSnapshot {
            event_handlers: vec![make_handler("click")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "simple event name should pass");
    }

    #[test]
    fn no_events_passes() {
        let template = TemplateAnalysisSnapshot::default();
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "no events should pass");
    }
}

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

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        // Check emit DEFINITIONS (events this component declares it can emit),
        // not event HANDLERS (listeners on child components). Kebab-case in
        // template @event-name listeners is standard Vue convention.
        for emit_def in &tpl.emit_definitions {
            if emit_def.event_name.contains('-') {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Custom event '{}' should use camelCase instead of kebab-case.",
                        emit_def.event_name
                    ),
                    emit_def.span.start,
                    emit_def.span.end,
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

    fn run_rule(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(CustomEventNameCasing, template)
    }

    fn make_emit_def(event_name: &str) -> AnalyzedEmitDefinition {
        AnalyzedEmitDefinition {
            event_name: event_name.to_string(),
            has_validator: false,
            is_declared: true,
            emit_locations: vec![],
            span: Span::new(10, 30),
        }
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
    fn kebab_case_emit_def_reports() {
        let template = TemplateAnalysisSnapshot {
            emit_definitions: vec![make_emit_def("my-event")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            !diags.is_empty(),
            "kebab-case emit definition should trigger"
        );
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
    fn camel_case_emit_def_passes() {
        let template = TemplateAnalysisSnapshot {
            emit_definitions: vec![make_emit_def("myEvent")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "camelCase emit definition should pass");
    }

    #[test]
    fn simple_emit_def_passes() {
        let template = TemplateAnalysisSnapshot {
            emit_definitions: vec![make_emit_def("click")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "simple event name should pass");
    }

    #[test]
    fn kebab_case_listener_on_child_component_passes() {
        // FP5: @click-overlay on a child component is a LISTENER, not an emit
        // declaration. Kebab-case is the correct Vue convention in templates.
        // Only emit DECLARATIONS should be checked for casing.
        let template = TemplateAnalysisSnapshot {
            event_handlers: vec![make_handler("click-overlay")],
            ..Default::default()
        };
        let diags = run_rule(&template);
        assert!(
            diags.is_empty(),
            "kebab-case event listener on child component should NOT trigger"
        );
    }

    #[test]
    fn no_events_passes() {
        let template = TemplateAnalysisSnapshot::default();
        let diags = run_rule(&template);
        assert!(diags.is_empty(), "no events should pass");
    }
}

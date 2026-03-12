//! Rule: no-unused-emit-declarations
//!
//! Warns when an event is declared in `defineEmits()` but never actually emitted
//! with `emit('event-name')` anywhere in the component.
//!
//! Unused emit declarations are dead code that bloat the component API surface
//! and can confuse consumers about which events the component actually emits.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;

pub struct NoUnusedEmitDeclarations;

impl LintRule for NoUnusedEmitDeclarations {
    fn name(&self) -> &'static str {
        "no-unused-emit-declarations"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for emit in &tpl.emit_definitions {
            // Only check explicitly declared events (from defineEmits)
            if !emit.is_declared {
                continue;
            }
            // If there are no emit locations, the event is never emitted
            if emit.emit_locations.is_empty() {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Emit '{}' is declared but never emitted. \
                         Remove the declaration or add a call to 'emit(\"{}\", ...)'.",
                        emit.event_name, emit.event_name
                    ),
                    emit.span.start,
                    emit.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::ScriptCallSite,
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
        crate::test_support::run_template_rule(NoUnusedEmitDeclarations, template)
    }

    fn make_emit(event_name: &str, is_declared: bool, emit_count: usize) -> AnalyzedEmitDefinition {
        AnalyzedEmitDefinition {
            event_name: event_name.to_string(),
            has_validator: false,
            is_declared,
            emit_locations: (0..emit_count)
                .map(|i| (i as u32 * 10, i as u32 * 10 + 5))
                .collect(),
            span: Span::new(10, 30),
        }
    }

    #[test]
    fn declared_emit_not_emitted_reports() {
        let template = TemplateAnalysisSnapshot {
            emit_definitions: vec![make_emit("update:modelValue", true, 0)],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            !diags.is_empty(),
            "declared emit with no locations should trigger"
        );
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-unused-emit-declarations"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn declared_emit_that_is_emitted_passes() {
        let template = TemplateAnalysisSnapshot {
            emit_definitions: vec![make_emit("click", true, 1)],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "declared emit with emit location should pass"
        );
    }

    #[test]
    fn undeclared_emit_passes() {
        // Ad-hoc emit() call without defineEmits declaration
        let template = TemplateAnalysisSnapshot {
            emit_definitions: vec![make_emit("click", false, 1)],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(
            diags.is_empty(),
            "undeclared emit should not trigger this rule"
        );
    }

    #[test]
    fn multiple_emits_only_unused_reported() {
        let template = TemplateAnalysisSnapshot {
            emit_definitions: vec![
                make_emit("change", true, 1), // used
                make_emit("close", true, 0),  // unused
                make_emit("open", true, 2),   // used
            ],
            ..Default::default()
        };
        let diags = run(&template);
        assert_eq!(diags.len(), 1, "only unused emit should be reported");
        assert!(diags[0].message.contains("close"));
    }
}

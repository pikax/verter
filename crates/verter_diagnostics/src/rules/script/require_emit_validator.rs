//! Rule: require-emit-validator
//!
//! Requires validator functions on emit declarations.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::template::TemplateAnalysisSnapshot;

pub struct RequireEmitValidator;

impl LintRule for RequireEmitValidator {
    fn name(&self) -> &'static str {
        "require-emit-validator"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for emit in &tpl.emit_definitions {
            if emit.is_declared && !emit.has_validator {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Emit '{}' should have a validator function.",
                        emit.event_name
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

    use verter_semantic::analysis::template::*;
    use verter_span::Span;

    fn run(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_template_rule(RequireEmitValidator, template)
    }

    #[test]
    fn emit_without_validator_reports() {
        let template = TemplateAnalysisSnapshot {
            emit_definitions: vec![AnalyzedEmitDefinition {
                event_name: "update".to_string(),
                has_validator: false,
                is_declared: true,
                emit_locations: vec![],
                span: Span::new(10, 20),
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(!diags.is_empty(), "emit without validator should trigger");
        assert!(diags[0].rule == "require-emit-validator");
    }

    #[test]
    fn emit_with_validator_passes() {
        let template = TemplateAnalysisSnapshot {
            emit_definitions: vec![AnalyzedEmitDefinition {
                event_name: "update".to_string(),
                has_validator: true,
                is_declared: true,
                emit_locations: vec![],
                span: Span::new(10, 20),
            }],
            ..Default::default()
        };
        let diags = run(&template);
        assert!(diags.is_empty(), "emit with validator should pass");
    }
}

//! Rule: no-deprecated-events-api
//!
//! The instance event methods `$on`, `$off`, and `$once` were removed in Vue 3.
//! Use an external event library (e.g., mitt, tiny-emitter) instead.
//! Detect these names in template binding occurrences and script bindings.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::template::TemplateAnalysisSnapshot;
use verter_analysis::types::ScriptAnalysisSnapshot;

pub struct NoDeprecatedEventsApi;

const DEPRECATED_EVENT_APIS: &[&str] = &["$on", "$off", "$once"];

impl LintRule for NoDeprecatedEventsApi {
    fn name(&self) -> &'static str {
        "no-deprecated-events-api"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_template(&self, tpl: &TemplateAnalysisSnapshot, ctx: &mut LintContext) {
        for occ in &tpl.binding_occurrences {
            if DEPRECATED_EVENT_APIS.contains(&occ.name.as_str()) {
                ctx.report_with_tags(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "'{}' has been removed in Vue 3. Use an external event library (e.g., mitt) instead.",
                        occ.name
                    ),
                    occ.span.start,
                    occ.span.end,
                    self.default_severity(),
                    vec![DiagnosticTag::Deprecated],
                    DiagnosticSpanKind::Interpolation,
                );
            }
        }
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for binding in &script.bindings {
            if DEPRECATED_EVENT_APIS.contains(&binding.name.as_str()) {
                ctx.report_with_tags(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "'{}' has been removed in Vue 3. Use an external event library (e.g., mitt) instead.",
                        binding.name
                    ),
                    binding.span.start,
                    binding.span.end,
                    self.default_severity(),
                    vec![DiagnosticTag::Deprecated],
                    DiagnosticSpanKind::ScriptCallSite,
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
    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_template(template: &TemplateAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDeprecatedEventsApi)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_template(template, &mut ctx);
        ctx.into_diagnostics()
    }

    fn run_script(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDeprecatedEventsApi)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_script(script, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn dollar_on_in_template_reports() {
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "$on".to_string(),
                span: Span::new(10, 13),
                usage_kind: BindingUsageKind::DirectiveValue,
            }],
            ..Default::default()
        };
        let diags = run_template(&template);
        assert!(!diags.is_empty(), "$on should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-deprecated-events-api"));
        assert!(
            diags[0].tags.contains(&DiagnosticTag::Deprecated),
            "should have Deprecated tag"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn dollar_off_in_script_reports() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![AnalyzedBinding {
                name: "$off".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: false,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span: Span::new(10, 14),
                used_in_script: false,
                used_in_style: false,
            }],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(!diags.is_empty(), "$off should trigger in script");
    }

    #[test]
    fn dollar_emit_passes() {
        let template = TemplateAnalysisSnapshot {
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "$emit".to_string(),
                span: Span::new(10, 15),
                usage_kind: BindingUsageKind::DirectiveValue,
            }],
            ..Default::default()
        };
        let diags = run_template(&template);
        assert!(diags.is_empty(), "$emit should pass");
    }
}

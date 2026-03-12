use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{ScriptAnalysisSnapshot, VueApiClassification};

/// Warns about `useTemplateRef` at setup scope during SSR. Template refs are
/// `null` during SSR because no DOM exists. Accessing `.value` will fail.
pub struct NoTemplateRefInSetup;

impl LintRule for NoTemplateRefInSetup {
    fn name(&self) -> &'static str {
        "no-template-ref-in-setup"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Ssr
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        if !ctx.config().ssr_mode {
            return;
        }

        for call in &script.vue_api_calls {
            if call.api == VueApiClassification::UseTemplateRef {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "Template refs are `null` during SSR. Avoid accessing `.value` in setup scope — use `onMounted()` instead.".to_string(),
                    call.span.start,
                    call.span.end,
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
    use crate::test_support::{run_script_rule, run_script_rule_ssr};
    use verter_analysis::types::VueApiCallSite;
    use verter_span::Span;

    fn call(api: VueApiClassification) -> VueApiCallSite {
        VueApiCallSite {
            api,
            span: Span::new(10, 40),
            arg_value: Some("myRef".to_string()),
            has_type_params: false,
            is_async_callback: false,
            callback_params: vec![],
        }
    }

    #[test]
    fn no_report_without_ssr_mode() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![call(VueApiClassification::UseTemplateRef)],
            ..Default::default()
        };
        let diags = run_script_rule(NoTemplateRefInSetup, &script);
        assert!(diags.is_empty());
    }

    #[test]
    fn reports_use_template_ref_in_ssr() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![call(VueApiClassification::UseTemplateRef)],
            ..Default::default()
        };
        let diags = run_script_rule_ssr(NoTemplateRefInSetup, &script);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("null"));
        assert!(diags[0].message.contains("onMounted"));
    }

    #[test]
    fn ignores_non_template_ref_apis() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![
                call(VueApiClassification::Ref),
                call(VueApiClassification::Computed),
            ],
            ..Default::default()
        };
        let diags = run_script_rule_ssr(NoTemplateRefInSetup, &script);
        assert!(diags.is_empty());
    }
}

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{ScriptAnalysisSnapshot, VueApiClassification};

/// Detects client-only lifecycle hooks (`onMounted`, `onUpdated`, `onActivated`, etc.)
/// at top-level setup scope. These never fire during SSR.
pub struct NoClientOnlyLifecycleInSetup;

impl NoClientOnlyLifecycleInSetup {
    fn is_client_only_lifecycle(api: &VueApiClassification) -> bool {
        matches!(
            api,
            VueApiClassification::OnMounted
                | VueApiClassification::OnUpdated
                | VueApiClassification::OnBeforeUpdate
                | VueApiClassification::OnActivated
                | VueApiClassification::OnDeactivated
                | VueApiClassification::OnRenderTracked
                | VueApiClassification::OnRenderTriggered
        )
    }
}

impl LintRule for NoClientOnlyLifecycleInSetup {
    fn name(&self) -> &'static str {
        "no-client-only-lifecycle-in-setup"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Ssr
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        if !ctx.config().ssr_mode {
            return;
        }

        for call in &script.vue_api_calls {
            if Self::is_client_only_lifecycle(&call.api) {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "`{}` never fires during SSR. Guard with `onMounted()` or use `<ClientOnly>`.",
                        call.api.display_name()
                    ),
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

    fn call(api: VueApiClassification, start: u32, end: u32) -> VueApiCallSite {
        VueApiCallSite {
            api,
            span: Span::new(start, end),
            arg_value: None,
            has_type_params: false,
            is_async_callback: false,
            callback_params: vec![],
        }
    }

    #[test]
    fn no_report_without_ssr_mode() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![call(VueApiClassification::OnMounted, 10, 30)],
            ..Default::default()
        };
        let diags = run_script_rule(NoClientOnlyLifecycleInSetup, &script);
        assert!(diags.is_empty(), "should not report without ssr_mode");
    }

    #[test]
    fn reports_on_mounted_in_ssr_mode() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![call(VueApiClassification::OnMounted, 10, 30)],
            ..Default::default()
        };
        let diags = run_script_rule_ssr(NoClientOnlyLifecycleInSetup, &script);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("onMounted"));
    }

    #[test]
    fn reports_on_activated_in_ssr_mode() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![call(VueApiClassification::OnActivated, 10, 30)],
            ..Default::default()
        };
        let diags = run_script_rule_ssr(NoClientOnlyLifecycleInSetup, &script);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("onActivated"));
    }

    #[test]
    fn ignores_on_server_prefetch() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![call(VueApiClassification::OnServerPrefetch, 10, 30)],
            ..Default::default()
        };
        let diags = run_script_rule_ssr(NoClientOnlyLifecycleInSetup, &script);
        assert!(diags.is_empty(), "onServerPrefetch is SSR-safe");
    }

    #[test]
    fn ignores_on_error_captured() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![call(VueApiClassification::OnErrorCaptured, 10, 30)],
            ..Default::default()
        };
        let diags = run_script_rule_ssr(NoClientOnlyLifecycleInSetup, &script);
        assert!(diags.is_empty(), "onErrorCaptured runs during SSR");
    }

    #[test]
    fn multiple_client_only_hooks() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![
                call(VueApiClassification::OnMounted, 10, 30),
                call(VueApiClassification::OnUpdated, 40, 60),
                call(VueApiClassification::OnRenderTracked, 70, 90),
            ],
            ..Default::default()
        };
        let diags = run_script_rule_ssr(NoClientOnlyLifecycleInSetup, &script);
        assert_eq!(diags.len(), 3);
    }
}

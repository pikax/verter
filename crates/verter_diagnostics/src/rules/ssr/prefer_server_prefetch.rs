use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{ScriptAnalysisSnapshot, VueApiClassification};

/// Suggests adding `onServerPrefetch` when `onMounted` contains an async callback
/// (likely data fetching) but no `onServerPrefetch` companion is present.
pub struct PreferServerPrefetch;

impl LintRule for PreferServerPrefetch {
    fn name(&self) -> &'static str {
        "prefer-server-prefetch"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Ssr
    }

    fn default_severity(&self) -> Severity {
        Severity::Hint
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        if !ctx.config().ssr_mode {
            return;
        }

        let has_server_prefetch = script
            .vue_api_calls
            .iter()
            .any(|c| c.api == VueApiClassification::OnServerPrefetch);

        if has_server_prefetch {
            return;
        }

        for call in &script.vue_api_calls {
            if call.api == VueApiClassification::OnMounted && call.is_async_callback {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "Async `onMounted` detected without `onServerPrefetch`. Add `onServerPrefetch` for server-side data fetching.".to_string(),
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

    fn call(api: VueApiClassification, is_async: bool) -> VueApiCallSite {
        VueApiCallSite {
            api,
            span: Span::new(10, 30),
            arg_value: None,
            has_type_params: false,
            is_async_callback: is_async,
            callback_params: vec![],
        }
    }

    #[test]
    fn no_report_without_ssr_mode() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![call(VueApiClassification::OnMounted, true)],
            ..Default::default()
        };
        let diags = run_script_rule(PreferServerPrefetch, &script);
        assert!(diags.is_empty());
    }

    #[test]
    fn reports_async_on_mounted_without_server_prefetch() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![call(VueApiClassification::OnMounted, true)],
            ..Default::default()
        };
        let diags = run_script_rule_ssr(PreferServerPrefetch, &script);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("onServerPrefetch"));
    }

    #[test]
    fn no_report_when_server_prefetch_present() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![
                call(VueApiClassification::OnMounted, true),
                call(VueApiClassification::OnServerPrefetch, false),
            ],
            ..Default::default()
        };
        let diags = run_script_rule_ssr(PreferServerPrefetch, &script);
        assert!(
            diags.is_empty(),
            "should not report when onServerPrefetch exists"
        );
    }

    #[test]
    fn no_report_for_sync_on_mounted() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![call(VueApiClassification::OnMounted, false)],
            ..Default::default()
        };
        let diags = run_script_rule_ssr(PreferServerPrefetch, &script);
        assert!(diags.is_empty(), "sync onMounted is not data fetching");
    }
}

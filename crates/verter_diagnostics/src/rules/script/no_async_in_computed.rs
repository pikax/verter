//! Rule: no-async-in-computed
//!
//! Disallows async callbacks inside `computed()`. Computed properties must be
//! synchronous — an async computed never resolves in Vue's reactivity system.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{ScriptAnalysisSnapshot, VueApiClassification};

pub struct NoAsyncInComputed;

impl LintRule for NoAsyncInComputed {
    fn name(&self) -> &'static str {
        "no-async-in-computed"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for call in &script.vue_api_calls {
            if call.api == VueApiClassification::Computed && call.is_async_callback {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "computed() cannot use an async callback. Use watchEffect() or composables with async data fetching instead.".to_string(),
                    call.span.start,
                    call.span.end,
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

    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_script_rule(NoAsyncInComputed, script)
    }

    fn make_call_site(api: VueApiClassification, is_async_callback: bool) -> VueApiCallSite {
        VueApiCallSite {
            api,
            span: Span::new(10, 50),
            arg_value: None,
            is_async_callback,
            callback_params: vec![],
        }
    }

    #[test]
    fn async_computed_reports() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_call_site(VueApiClassification::Computed, true)],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            !diags.is_empty(),
            "computed with async callback should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "no-async-in-computed"));
        assert!(
            diags[0].message.contains("async"),
            "message should mention async"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger no-v-html"
        );
    }

    #[test]
    fn sync_computed_passes() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_call_site(VueApiClassification::Computed, false)],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "sync computed should pass");
    }

    #[test]
    fn async_watch_effect_passes() {
        // watchEffect with async is fine
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_call_site(VueApiClassification::WatchEffect, true)],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "async watchEffect must not trigger no-async-in-computed"
        );
    }
}

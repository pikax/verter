//! Rule: no-side-effects-in-computed
//!
//! Computed properties must be pure — no side effects. Async callbacks in
//! `computed()` are a proxy indicator for side effects (network requests,
//! DOM mutations, etc.) and should use `watchEffect()` or composables instead.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::types::{ScriptAnalysisSnapshot, VueApiClassification};

pub struct NoSideEffectsInComputed;

impl LintRule for NoSideEffectsInComputed {
    fn name(&self) -> &'static str {
        "no-side-effects-in-computed"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for call in &script.vue_api_calls {
            if call.api == VueApiClassification::Computed && call.is_async_callback {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "Computed properties should not have side effects. \
                     An async callback in `computed()` indicates side effects. \
                     Use `watchEffect()` or a composable instead."
                        .to_string(),
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
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_semantic::analysis::types::*;
    use verter_span::Span;

    fn run_script(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoSideEffectsInComputed)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_script(script, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_call(api: VueApiClassification, is_async: bool) -> VueApiCallSite {
        VueApiCallSite {
            api,
            span: Span::new(10, 50),
            arg_value: None,
            has_type_params: false,
            is_async_callback: is_async,
            callback_params: vec![],
        }
    }

    #[test]
    fn async_computed_reports() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_call(VueApiClassification::Computed, true)],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(
            !diags.is_empty(),
            "async computed should trigger side-effects rule"
        );
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-side-effects-in-computed"));
        assert!(
            diags[0].message.contains("side effects"),
            "message should mention side effects"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn sync_computed_passes() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_call(VueApiClassification::Computed, false)],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(diags.is_empty(), "sync computed should pass");
    }

    #[test]
    fn async_watch_effect_passes() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_call(VueApiClassification::WatchEffect, true)],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(
            diags.is_empty(),
            "async watchEffect should not trigger no-side-effects-in-computed"
        );
    }

    #[test]
    fn no_api_calls_passes() {
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_script(&script);
        assert!(diags.is_empty(), "no API calls should pass");
    }
}

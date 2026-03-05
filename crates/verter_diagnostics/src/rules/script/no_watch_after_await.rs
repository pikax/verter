//! Rule: no-watch-after-await
//!
//! `watch()` and `watchEffect()` must be called synchronously at the top of
//! `<script setup>`. Calling them after an `await` expression means they won't
//! be associated with the current component instance.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::ScriptAnalysisSnapshot;

pub struct NoWatchAfterAwait;

impl LintRule for NoWatchAfterAwait {
    fn name(&self) -> &'static str {
        "no-watch-after-await"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        let Some(first_await) = script.first_await_offset else {
            return;
        };

        for call in &script.vue_api_calls {
            if !call.api.is_watcher() {
                continue;
            }
            if call.span.start <= first_await {
                continue;
            }
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "'{}()' should not be called after 'await'. \
                     Watchers must be set up synchronously to be associated with the component instance.",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::template::TemplateAnalysisSnapshot;
    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_script(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoWatchAfterAwait)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        let template = TemplateAnalysisSnapshot::default();
        visitor.visit_script(script, &mut ctx);
        drop(template);
        ctx.into_diagnostics()
    }

    #[test]
    fn watch_after_await_reports() {
        let script = ScriptAnalysisSnapshot {
            first_await_offset: Some(50),
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::Watch,
                span: Span::new(80, 100),
                arg_value: None,
                is_async_callback: false,
                callback_params: vec![],
            }],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(!diags.is_empty(), "watch() after await should trigger");
        assert!(diags.iter().any(|d| d.rule == "no-watch-after-await"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn watch_before_await_passes() {
        let script = ScriptAnalysisSnapshot {
            first_await_offset: Some(100),
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::Watch,
                span: Span::new(20, 50),
                arg_value: None,
                is_async_callback: false,
                callback_params: vec![],
            }],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(diags.is_empty(), "watch() before await should pass");
    }

    #[test]
    fn no_await_passes() {
        let script = ScriptAnalysisSnapshot {
            first_await_offset: None,
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::WatchEffect,
                span: Span::new(20, 50),
                arg_value: None,
                is_async_callback: false,
                callback_params: vec![],
            }],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(diags.is_empty(), "no await = no issue");
    }
}

//! # no-lifecycle-after-await
//!
//! Disallow calling lifecycle hooks, watchers, provide, or inject after `await`
//! in `<script setup>`. After an `await`, the component instance context is lost,
//! so these APIs may bind to the wrong (or no) instance.
//!
//! ## Bad
//! ```vue
//! <script setup>
//! const data = await fetchData()
//! onMounted(() => { /* lost context! */ })
//! </script>
//! ```
//!
//! ## Good
//! ```vue
//! <script setup>
//! onMounted(() => { /* before await, context is fine */ })
//! const data = await fetchData()
//! </script>
//! ```

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::ScriptAnalysisSnapshot;

/// Lint rule: no lifecycle hooks / watchers / provide / inject after `await`.
pub struct NoLifecycleAfterAwait;

impl LintRule for NoLifecycleAfterAwait {
    fn name(&self) -> &'static str {
        "no-lifecycle-after-await"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Reactivity
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        // Only relevant for async setup
        let first_await = match script.first_await_offset {
            Some(pos) => pos,
            None => return,
        };

        for call in &script.vue_api_calls {
            // Only check APIs that require sync context
            if !call.api.requires_sync_context() {
                continue;
            }

            // Skip calls that happen before the first await
            if call.span.start <= first_await {
                continue;
            }

            let api_name = call.api.display_name();
            let kind_desc = if call.api.is_lifecycle() {
                "Lifecycle hook"
            } else if call.api.is_watcher() {
                "Watcher"
            } else {
                "API call"
            };

            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "{kind_desc} `{api_name}()` is called after `await`. \
                     Move it before the first `await` to ensure it binds to the correct \
                     component instance."
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

    use verter_analysis::types::{AnalysisFlags, VueApiCallSite, VueApiClassification};
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_script_rule(NoLifecycleAfterAwait, script)
    }

    #[test]
    fn no_await_no_diagnostics() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::OnMounted,
                span: Span::new(10, 30),
                arg_value: None,
                has_type_params: false,
                is_async_callback: false,
                callback_params: vec![],
            }],
            first_await_offset: None,
            ..Default::default()
        };
        assert!(run_rule(&script).is_empty());
    }

    #[test]
    fn lifecycle_before_await_ok() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::OnMounted,
                span: Span::new(10, 30),
                arg_value: None,
                has_type_params: false,
                is_async_callback: false,
                callback_params: vec![],
            }],
            first_await_offset: Some(50),
            flags: AnalysisFlags::ASYNC_SETUP,
            ..Default::default()
        };
        assert!(run_rule(&script).is_empty());
    }

    #[test]
    fn lifecycle_after_await_reports() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::OnMounted,
                span: Span::new(100, 120),
                arg_value: None,
                has_type_params: false,
                is_async_callback: false,
                callback_params: vec![],
            }],
            first_await_offset: Some(50),
            flags: AnalysisFlags::ASYNC_SETUP,
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].rule, "no-lifecycle-after-await");
        assert!(diags[0].message.contains("onMounted()"));
        assert!(diags[0].message.contains("after `await`"));
    }

    #[test]
    fn watcher_after_await_reports() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::Watch,
                span: Span::new(80, 100),
                arg_value: None,
                has_type_params: false,
                is_async_callback: false,
                callback_params: vec![],
            }],
            first_await_offset: Some(50),
            flags: AnalysisFlags::ASYNC_SETUP,
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("watch()"));
    }

    #[test]
    fn provide_after_await_reports() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::Provide,
                span: Span::new(80, 100),
                arg_value: None,
                has_type_params: false,
                is_async_callback: false,
                callback_params: vec![],
            }],
            first_await_offset: Some(50),
            flags: AnalysisFlags::ASYNC_SETUP,
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("provide()"));
    }

    #[test]
    fn non_sync_api_after_await_ok() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::Ref,
                span: Span::new(80, 100),
                arg_value: None,
                has_type_params: false,
                is_async_callback: false,
                callback_params: vec![],
            }],
            first_await_offset: Some(50),
            flags: AnalysisFlags::ASYNC_SETUP,
            ..Default::default()
        };
        // ref() doesn't require sync context
        assert!(run_rule(&script).is_empty());
    }

    #[test]
    fn multiple_calls_mixed() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![
                VueApiCallSite {
                    api: VueApiClassification::OnMounted,
                    span: Span::new(10, 30), // before await
                    arg_value: None,
                    has_type_params: false,
                    is_async_callback: false,
                    callback_params: vec![],
                },
                VueApiCallSite {
                    api: VueApiClassification::OnUnmounted,
                    span: Span::new(100, 120), // after await
                    arg_value: None,
                    has_type_params: false,
                    is_async_callback: false,
                    callback_params: vec![],
                },
                VueApiCallSite {
                    api: VueApiClassification::WatchEffect,
                    span: Span::new(130, 150), // after await
                    arg_value: None,
                    has_type_params: false,
                    is_async_callback: false,
                    callback_params: vec![],
                },
            ],
            first_await_offset: Some(50),
            flags: AnalysisFlags::ASYNC_SETUP,
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(diags.len(), 2);
        assert!(diags[0].message.contains("onUnmounted()"));
        assert!(diags[1].message.contains("watchEffect()"));
    }
}

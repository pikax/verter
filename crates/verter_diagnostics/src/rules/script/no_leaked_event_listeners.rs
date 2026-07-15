//! # no-leaked-event-listeners
//!
//! Warns when a component has `onMounted` with DOM queries but no
//! `onUnmounted` or `onBeforeUnmount` cleanup hook. This pattern often
//! indicates missing `removeEventListener` calls, leading to memory leaks.
//!
//! ## Bad
//! ```vue
//! <script setup>
//! import { onMounted } from 'vue'
//! onMounted(() => {
//!   document.querySelector('.btn').addEventListener('click', handler)
//! })
//! </script>
//! ```
//!
//! ## Good
//! ```vue
//! <script setup>
//! import { onMounted, onUnmounted } from 'vue'
//! onMounted(() => {
//!   document.querySelector('.btn').addEventListener('click', handler)
//! })
//! onUnmounted(() => {
//!   document.querySelector('.btn').removeEventListener('click', handler)
//! })
//! </script>
//! ```

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::types::ScriptAnalysisSnapshot;

/// Lint rule: warn when `onMounted` + DOM queries exist without a cleanup lifecycle hook.
pub struct NoLeakedEventListeners;

impl LintRule for NoLeakedEventListeners {
    fn name(&self) -> &'static str {
        "no-leaked-event-listeners"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Reactivity
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        use verter_semantic::analysis::types::VueApiClassification;

        // Must have DOM query calls — otherwise nothing to warn about
        if script.dom_query_calls.is_empty() {
            return;
        }

        // Check for onMounted
        let has_on_mounted = script
            .vue_api_calls
            .iter()
            .any(|c| c.api == VueApiClassification::OnMounted);

        if !has_on_mounted {
            return;
        }

        // Check for cleanup hooks
        let has_cleanup = script.vue_api_calls.iter().any(|c| {
            matches!(
                c.api,
                VueApiClassification::OnUnmounted | VueApiClassification::OnBeforeUnmount
            )
        });

        if has_cleanup {
            return;
        }

        // Find the onMounted call to report on its span
        let on_mounted_call = script
            .vue_api_calls
            .iter()
            .find(|c| c.api == VueApiClassification::OnMounted)
            .unwrap();

        ctx.report_with_severity(
            self.name(),
            self.category().as_str(),
            "Component has `onMounted` with DOM queries but no `onUnmounted`/`onBeforeUnmount` \
             cleanup hook. Consider adding cleanup to prevent memory leaks."
                .to_string(),
            on_mounted_call.span.start,
            on_mounted_call.span.end,
            self.default_severity(),
            DiagnosticSpanKind::ScriptCallSite,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use verter_semantic::analysis::types::{
        DomQueryCallSite, DomQueryKind, VueApiCallSite, VueApiClassification,
    };
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_script_rule(NoLeakedEventListeners, script)
    }

    fn make_dom_query() -> DomQueryCallSite {
        DomQueryCallSite {
            kind: DomQueryKind::QuerySelector,
            selector_text: ".btn".to_string(),
            parsed: None,
            span: Span::new(50, 80),
            arg_span: Span::new(65, 70),
        }
    }

    fn make_api_call(api: VueApiClassification, start: u32, end: u32) -> VueApiCallSite {
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
    fn on_mounted_with_dom_query_no_cleanup_reports() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_api_call(VueApiClassification::OnMounted, 10, 30)],
            dom_query_calls: vec![make_dom_query()],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(diags.len(), 1, "should report exactly one diagnostic");
        assert_eq!(diags[0].rule, "no-leaked-event-listeners");
        assert!(
            diags[0].message.contains("onMounted"),
            "message should mention onMounted"
        );
        assert!(
            diags[0].message.contains("cleanup"),
            "message should mention cleanup"
        );
        // Negative: should not contain unrelated rule names
        assert!(
            !diags.iter().any(|d| d.rule != "no-leaked-event-listeners"),
            "should not trigger any other rule"
        );
    }

    #[test]
    fn on_mounted_with_dom_query_and_on_unmounted_ok() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![
                make_api_call(VueApiClassification::OnMounted, 10, 30),
                make_api_call(VueApiClassification::OnUnmounted, 100, 120),
            ],
            dom_query_calls: vec![make_dom_query()],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "onUnmounted present = no warning");
    }

    #[test]
    fn on_mounted_with_dom_query_and_on_before_unmount_ok() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![
                make_api_call(VueApiClassification::OnMounted, 10, 30),
                make_api_call(VueApiClassification::OnBeforeUnmount, 100, 120),
            ],
            dom_query_calls: vec![make_dom_query()],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "onBeforeUnmount present = no warning");
    }

    #[test]
    fn no_dom_queries_no_diagnostic() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_api_call(VueApiClassification::OnMounted, 10, 30)],
            dom_query_calls: vec![],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "no DOM queries = no warning even without cleanup hook"
        );
    }

    #[test]
    fn no_on_mounted_no_diagnostic() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_api_call(VueApiClassification::OnUpdated, 10, 30)],
            dom_query_calls: vec![make_dom_query()],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "no onMounted = no warning even with DOM queries"
        );
    }

    #[test]
    fn empty_script_no_diagnostic() {
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "empty script = no diagnostic");
    }

    #[test]
    fn multiple_dom_queries_single_diagnostic() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_api_call(VueApiClassification::OnMounted, 10, 30)],
            dom_query_calls: vec![
                make_dom_query(),
                DomQueryCallSite {
                    kind: DomQueryKind::GetElementById,
                    selector_text: "app".to_string(),
                    parsed: None,
                    span: Span::new(90, 120),
                    arg_span: Span::new(105, 110),
                },
            ],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(
            diags.len(),
            1,
            "multiple DOM queries should still produce only one diagnostic"
        );
    }

    #[test]
    fn diagnostic_span_points_to_on_mounted() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_api_call(VueApiClassification::OnMounted, 42, 60)],
            dom_query_calls: vec![make_dom_query()],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].span.start, 42,
            "span should start at onMounted call"
        );
        assert_eq!(diags[0].span.end, 60, "span should end at onMounted call");
    }
}

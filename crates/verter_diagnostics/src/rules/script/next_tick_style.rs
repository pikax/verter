//! Rule: next-tick-style
//!
//! Prefer `await nextTick()` over callback style `nextTick(() => { ... })`.
//! The await style is cleaner and avoids callback nesting.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{ScriptAnalysisSnapshot, VueApiClassification};

pub struct NextTickStyle;

impl LintRule for NextTickStyle {
    fn name(&self) -> &'static str {
        "next-tick-style"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Severity {
        Severity::Hint
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for call in &script.vue_api_calls {
            if call.api == VueApiClassification::NextTick && call.is_async_callback {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "Prefer `await nextTick()` over callback style. \
                     The await pattern is cleaner and avoids unnecessary nesting."
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

    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_script_rule(NextTickStyle, script)
    }

    fn make_next_tick(is_async_callback: bool) -> VueApiCallSite {
        VueApiCallSite {
            api: VueApiClassification::NextTick,
            span: Span::new(10, 40),
            arg_value: None,
            is_async_callback,
            callback_params: vec![],
        }
    }

    #[test]
    fn callback_style_reports() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_next_tick(true)],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(!diags.is_empty(), "callback style nextTick should trigger");
        assert!(diags.iter().any(|d| d.rule == "next-tick-style"));
        assert!(
            diags[0].message.contains("await nextTick()"),
            "message should suggest await style"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn await_style_passes() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_next_tick(false)],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "await-style nextTick should not trigger");
    }

    #[test]
    fn other_api_with_callback_passes() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::WatchEffect,
                span: Span::new(10, 40),
                arg_value: None,
                is_async_callback: true,
                callback_params: vec![],
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "other APIs with callbacks should not trigger next-tick-style"
        );
    }
}

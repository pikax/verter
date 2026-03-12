//! Rule: require-typed-ref
//!
//! Requires `ref()` calls to have a type parameter. Untyped refs default to
//! `Ref<T | undefined>` which weakens type checking. Use `ref<Type>(...)` instead.
//!
//! This rule is **off by default** — users must opt in via `.verterrc.json`:
//! ```json
//! { "rules": { "require-typed-ref": "warn" } }
//! ```

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{ScriptAnalysisSnapshot, VueApiClassification};

pub struct RequireTypedRef;

impl LintRule for RequireTypedRef {
    fn name(&self) -> &'static str {
        "require-typed-ref"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn is_default_off(&self) -> bool {
        true
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for call in &script.vue_api_calls {
            if call.api == VueApiClassification::Ref && !call.has_type_params {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "ref() should have a type parameter. Use ref<Type>() for better type safety."
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
        crate::test_support::run_script_rule(RequireTypedRef, script)
    }

    fn make_ref_call(has_type_params: bool, arg_value: Option<&str>) -> VueApiCallSite {
        VueApiCallSite {
            api: VueApiClassification::Ref,
            span: Span::new(10, 30),
            arg_value: arg_value.map(|s| s.to_string()),
            has_type_params,
            is_async_callback: false,
            callback_params: vec![],
        }
    }

    #[test]
    fn ref_without_type_params_reports() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_ref_call(false, None)],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            !diags.is_empty(),
            "ref() without type params should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "require-typed-ref"));
        assert!(
            diags[0].message.contains("type parameter"),
            "message should mention type parameter"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn ref_with_type_params_passes() {
        // ref<string>() — typed, no arg value
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_ref_call(true, None)],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "ref<string>() with type params should pass"
        );
    }

    #[test]
    fn ref_with_arg_but_no_type_params_reports() {
        // ref(0) — has arg value but no type param
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_ref_call(false, Some("0"))],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            !diags.is_empty(),
            "ref(0) without type params should trigger"
        );
    }

    #[test]
    fn ref_with_type_params_and_arg_passes() {
        // ref<number>(0) — typed with arg value
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_ref_call(true, Some("0"))],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "ref<number>(0) with type params should pass"
        );
    }

    #[test]
    fn computed_without_type_params_passes() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::Computed,
                span: Span::new(10, 40),
                arg_value: None,
                has_type_params: false,
                is_async_callback: false,
                callback_params: vec![],
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "computed() should not trigger require-typed-ref"
        );
    }

    #[test]
    fn no_calls_passes() {
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "no calls should pass");
    }

    #[test]
    fn is_default_off() {
        assert!(
            RequireTypedRef.is_default_off(),
            "require-typed-ref should be default off"
        );
    }
}

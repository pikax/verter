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

    fn default_severity(&self) -> Option<Severity> {
        None // Opt-in: off by default
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        if !script.is_typescript {
            return;
        }
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

    use crate::config::LintConfig;
    use crate::context::LintContext;
    use crate::visitor::LintVisitor;
    use verter_analysis::types::*;
    use verter_span::Span;

    /// Run the rule with it explicitly enabled (since it's off by default).
    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rule = RequireTypedRef;
        let rules: Vec<Box<dyn crate::rules::LintRule>> = vec![Box::new(rule)];
        let visitor = LintVisitor::new(&rules);
        let mut config = LintConfig::default();
        config
            .rules
            .insert("require-typed-ref".to_string(), Some(Severity::Warning));
        let mut ctx = LintContext::new(&config);
        visitor.visit_script(script, &mut ctx);
        ctx.into_diagnostics()
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
            is_typescript: true,
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
            is_typescript: true,
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
    fn default_severity_is_none() {
        assert_eq!(
            RequireTypedRef.default_severity(),
            None,
            "require-typed-ref should be off by default (None severity)"
        );
    }

    #[test]
    fn js_script_does_not_report() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_ref_call(false, Some("0"))],
            is_typescript: false,
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "require-typed-ref must not fire for JS SFCs"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "require-typed-ref"),
            "must not produce require-typed-ref diagnostic for JS"
        );
    }
}

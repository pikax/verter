//! Rule: require-symbol-provide
//!
//! Recommends using `Symbol` keys with `provide()` for better encapsulation.
//! String literal keys in `provide()` are leakable between components.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{ScriptAnalysisSnapshot, VueApiClassification};

pub struct RequireSymbolProvide;

impl LintRule for RequireSymbolProvide {
    fn name(&self) -> &'static str {
        "require-symbol-provide"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for call in &script.vue_api_calls {
            if call.api != VueApiClassification::Provide {
                continue;
            }

            // arg_value is set when the first arg is a string literal
            if call.arg_value.is_some() {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "Use a Symbol key with provide() for better encapsulation and type safety. Consider: const key = Symbol('key'); provide(key, value)".to_string(),
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
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(RequireSymbolProvide)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_script(script, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_provide(arg_value: Option<&str>) -> VueApiCallSite {
        VueApiCallSite {
            api: VueApiClassification::Provide,
            span: Span::new(0, 30),
            arg_value: arg_value.map(|s| s.to_string()),
            is_async_callback: false,
        }
    }

    #[test]
    fn string_literal_provide_reports() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_provide(Some("myKey"))],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(!diags.is_empty(), "provide with string key should trigger");
        assert!(diags.iter().any(|d| d.rule == "require-symbol-provide"));
        assert!(
            diags[0].message.contains("Symbol"),
            "message should mention Symbol"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger no-v-html"
        );
    }

    #[test]
    fn symbol_key_provide_passes() {
        // provide(MY_KEY, val) — arg_value is None (not a string literal)
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![make_provide(None)],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "provide with non-literal key should pass");
    }

    #[test]
    fn inject_not_triggered() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::Inject,
                span: Span::new(0, 20),
                arg_value: Some("myKey".to_string()),
                is_async_callback: false,
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "inject must not trigger require-symbol-provide"
        );
    }
}

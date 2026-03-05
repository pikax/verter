//! Rule: no-arrow-functions-in-watch
//!
//! Disallows arrow functions as watch callbacks, because they cannot be
//! bound to the component instance.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{ScriptAnalysisSnapshot, VueApiClassification};

pub struct NoArrowFunctionsInWatch;

impl LintRule for NoArrowFunctionsInWatch {
    fn name(&self) -> &'static str {
        "no-arrow-functions-in-watch"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for call in &script.vue_api_calls {
            if call.api == VueApiClassification::Watch && call.is_async_callback {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "Avoid using async arrow functions as watch callbacks. They cannot access the component instance via `this`.".to_string(),
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
    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoArrowFunctionsInWatch)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_script(script, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn async_watch_callback_reports() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::Watch,
                span: Span::new(10, 50),
                arg_value: None,
                is_async_callback: true,
                callback_params: vec![],
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(!diags.is_empty(), "async watch should trigger");
        assert!(diags[0].rule == "no-arrow-functions-in-watch");
        assert!(
            !diags.iter().any(|d| d.rule == "no-async-in-computed"),
            "must not trigger computed rule"
        );
    }

    #[test]
    fn sync_watch_passes() {
        let script = ScriptAnalysisSnapshot {
            vue_api_calls: vec![VueApiCallSite {
                api: VueApiClassification::Watch,
                span: Span::new(10, 50),
                arg_value: None,
                is_async_callback: false,
                callback_params: vec![],
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "sync watch should pass");
    }
}

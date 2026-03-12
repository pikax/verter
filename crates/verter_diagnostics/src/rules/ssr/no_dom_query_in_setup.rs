use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::ScriptAnalysisSnapshot;

/// Detects DOM query calls (`querySelector`, `getElementById`, etc.) at setup scope.
/// There is no DOM on the server, so these always fail during SSR.
pub struct NoDomQueryInSetup;

impl LintRule for NoDomQueryInSetup {
    fn name(&self) -> &'static str {
        "no-dom-query-in-setup"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Ssr
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        if !ctx.config().ssr_mode {
            return;
        }

        for call in &script.dom_query_calls {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "DOM query `{}` has no effect during SSR (no DOM on server). Move to `onMounted()`.",
                    call.kind.display_name()
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
    use crate::test_support::{run_script_rule, run_script_rule_ssr};
    use verter_analysis::types::{DomQueryCallSite, DomQueryKind};
    use verter_span::Span;

    fn dom_call(kind: DomQueryKind) -> DomQueryCallSite {
        DomQueryCallSite {
            kind,
            selector_text: ".foo".to_string(),
            parsed: None,
            span: Span::new(10, 50),
            arg_span: Span::new(30, 35),
        }
    }

    #[test]
    fn no_report_without_ssr_mode() {
        let script = ScriptAnalysisSnapshot {
            dom_query_calls: vec![dom_call(DomQueryKind::QuerySelector)],
            ..Default::default()
        };
        let diags = run_script_rule(NoDomQueryInSetup, &script);
        assert!(diags.is_empty());
    }

    #[test]
    fn reports_query_selector_in_ssr_mode() {
        let script = ScriptAnalysisSnapshot {
            dom_query_calls: vec![dom_call(DomQueryKind::QuerySelector)],
            ..Default::default()
        };
        let diags = run_script_rule_ssr(NoDomQueryInSetup, &script);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("DOM query"));
    }

    #[test]
    fn reports_get_element_by_id() {
        let script = ScriptAnalysisSnapshot {
            dom_query_calls: vec![dom_call(DomQueryKind::GetElementById)],
            ..Default::default()
        };
        let diags = run_script_rule_ssr(NoDomQueryInSetup, &script);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("onMounted"));
    }
}

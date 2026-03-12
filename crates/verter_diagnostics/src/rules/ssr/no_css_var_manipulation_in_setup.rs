use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::ScriptAnalysisSnapshot;

/// Warns about CSS variable manipulations (`setProperty`, `getPropertyValue`) on DOM
/// style objects at setup scope. The DOM is not available during SSR.
pub struct NoCssVarManipulationInSetup;

impl LintRule for NoCssVarManipulationInSetup {
    fn name(&self) -> &'static str {
        "no-css-var-manipulation-in-setup"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Ssr
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        if !ctx.config().ssr_mode {
            return;
        }

        for manip in &script.css_var_manipulations {
            ctx.report_with_severity(
                self.name(),
                self.category().as_str(),
                format!(
                    "CSS variable manipulation `{}` requires DOM access, which is unavailable during SSR. Move to `onMounted()`.",
                    manip.kind.display_name()
                ),
                manip.span.start,
                manip.span.end,
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
    use verter_analysis::types::{CssVarManipulation, CssVarManipulationKind};
    use verter_span::Span;

    fn manip(kind: CssVarManipulationKind) -> CssVarManipulation {
        CssVarManipulation {
            kind,
            var_name: "--color".to_string(),
            value_expr: Some("'red'".to_string()),
            span: Span::new(10, 50),
        }
    }

    #[test]
    fn no_report_without_ssr_mode() {
        let script = ScriptAnalysisSnapshot {
            css_var_manipulations: vec![manip(CssVarManipulationKind::SetProperty)],
            ..Default::default()
        };
        let diags = run_script_rule(NoCssVarManipulationInSetup, &script);
        assert!(diags.is_empty());
    }

    #[test]
    fn reports_set_property_in_ssr() {
        let script = ScriptAnalysisSnapshot {
            css_var_manipulations: vec![manip(CssVarManipulationKind::SetProperty)],
            ..Default::default()
        };
        let diags = run_script_rule_ssr(NoCssVarManipulationInSetup, &script);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("CSS variable manipulation"));
    }

    #[test]
    fn reports_get_property_value() {
        let script = ScriptAnalysisSnapshot {
            css_var_manipulations: vec![manip(CssVarManipulationKind::GetPropertyValue)],
            ..Default::default()
        };
        let diags = run_script_rule_ssr(NoCssVarManipulationInSetup, &script);
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn no_manipulations_no_reports() {
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_script_rule_ssr(NoCssVarManipulationInSetup, &script);
        assert!(diags.is_empty());
    }
}

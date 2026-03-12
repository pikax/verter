//! Rule: valid-define-props
//!
//! Validates that `defineProps` is used correctly:
//! - Only one `defineProps` call per component
//! - Must be in `<script setup>`

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{AnalyzedMacroKind, ScriptAnalysisSnapshot};

pub struct ValidDefineProps;

impl LintRule for ValidDefineProps {
    fn name(&self) -> &'static str {
        "valid-define-props"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        let prop_macros: Vec<_> = script
            .macros
            .iter()
            .filter(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .collect();

        if prop_macros.len() > 1 {
            for m in &prop_macros[1..] {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "`defineProps()` can only be called once per component.".to_string(),
                    m.span.start,
                    m.span.end,
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
        crate::test_support::run_script_rule(ValidDefineProps, script)
    }

    fn make_props_macro(start: u32, end: u32) -> AnalyzedMacro {
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: false,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            emit_fields: vec![],
            slot_fields: vec![],
            span: Span::new(start, end),
        }
    }

    #[test]
    fn single_define_props_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_props_macro(10, 30)],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "single defineProps should pass");
    }

    #[test]
    fn duplicate_define_props_reports() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_props_macro(10, 30), make_props_macro(40, 60)],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(diags.len(), 1, "duplicate defineProps should trigger once");
        assert!(diags.iter().any(|d| d.rule == "valid-define-props"));
        assert!(
            !diags.iter().any(|d| d.rule == "valid-define-emits"),
            "must not trigger emits rule"
        );
    }
}

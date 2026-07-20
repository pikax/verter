//! Rule: valid-define-emits
//!
//! Validates that `defineEmits` is used correctly:
//! - Only one `defineEmits` call per component
//! - Must be in `<script setup>`

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::types::{AnalyzedMacroKind, ScriptAnalysisSnapshot};

pub struct ValidDefineEmits;

impl LintRule for ValidDefineEmits {
    fn name(&self) -> &'static str {
        "valid-define-emits"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        let emit_macros: Vec<_> = script
            .macros
            .iter()
            .filter(|m| m.kind == AnalyzedMacroKind::DefineEmits)
            .collect();

        if emit_macros.len() > 1 {
            for m in &emit_macros[1..] {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "`defineEmits()` can only be called once per component.".to_string(),
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

    use verter_semantic::analysis::types::*;
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_script_rule(ValidDefineEmits, script)
    }

    fn make_emit_macro(start: u32, end: u32) -> AnalyzedMacro {
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineEmits,
            owner: verter_type_expr::TopLevelOwnerId::instance(0),
            is_type_based: false,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            emit_fields: vec![],
            slot_fields: vec![],
            default_keys: vec![],
            expose_fields: vec![],
            default_values: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(start, end),
        }
    }

    #[test]
    fn single_define_emits_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_emit_macro(10, 30)],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "single defineEmits should pass");
    }

    #[test]
    fn duplicate_define_emits_reports() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_emit_macro(10, 30), make_emit_macro(40, 60)],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(diags.len(), 1, "duplicate defineEmits should trigger once");
        assert!(diags.iter().any(|d| d.rule == "valid-define-emits"));
        assert!(
            !diags.iter().any(|d| d.rule == "valid-define-props"),
            "must not trigger props rule"
        );
    }

    #[test]
    fn no_define_emits_passes() {
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "no defineEmits should pass");
    }
}

//! Rule: define-emits-declaration
//!
//! Enforces type-based `defineEmits` declarations over runtime declarations.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::types::{AnalyzedMacroKind, ScriptAnalysisSnapshot};

pub struct DefineEmitsDeclaration;

impl LintRule for DefineEmitsDeclaration {
    fn name(&self) -> &'static str {
        "define-emits-declaration"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        if !script.is_typescript {
            return;
        }
        for m in &script.macros {
            if m.kind == AnalyzedMacroKind::DefineEmits && !m.is_type_based {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "Use type-based `defineEmits` declaration (e.g., `defineEmits<{...}>()`) instead of runtime declaration.".to_string(),
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
        crate::test_support::run_script_rule(DefineEmitsDeclaration, script)
    }

    fn make_emit_macro(is_type_based: bool) -> AnalyzedMacro {
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineEmits,
            owner: verter_type_expr::TopLevelOwnerId::instance(0),
            is_type_based,
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
            span: Span::new(10, 40),
        }
    }

    #[test]
    fn runtime_define_emits_reports() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_emit_macro(false)],
            is_typescript: true,
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(!diags.is_empty(), "runtime defineEmits should trigger");
        assert!(diags[0].rule == "define-emits-declaration");
        assert!(
            !diags.iter().any(|d| d.rule == "define-props-declaration"),
            "must not trigger props rule"
        );
    }

    #[test]
    fn type_based_define_emits_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_emit_macro(true)],
            is_typescript: true,
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "type-based defineEmits should pass");
    }

    #[test]
    fn js_script_does_not_report() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_emit_macro(false)],
            is_typescript: false,
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "define-emits-declaration must not fire for JS SFCs"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "define-emits-declaration"),
            "must not produce define-emits-declaration diagnostic for JS"
        );
    }
}

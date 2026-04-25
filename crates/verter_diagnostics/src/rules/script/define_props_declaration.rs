//! Rule: define-props-declaration
//!
//! Enforces type-based `defineProps` declarations over runtime declarations.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_semantic::analysis::types::{AnalyzedMacroKind, ScriptAnalysisSnapshot};

pub struct DefinePropsDeclaration;

impl LintRule for DefinePropsDeclaration {
    fn name(&self) -> &'static str {
        "define-props-declaration"
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
            if m.kind == AnalyzedMacroKind::DefineProps && !m.is_type_based {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "Use type-based `defineProps` declaration (e.g., `defineProps<{...}>()`) instead of runtime declaration.".to_string(),
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
        crate::test_support::run_script_rule(DefinePropsDeclaration, script)
    }

    fn make_props_macro(is_type_based: bool) -> AnalyzedMacro {
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
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
            span: Span::new(10, 40),
        }
    }

    #[test]
    fn runtime_define_props_reports() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_props_macro(false)],
            is_typescript: true,
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(!diags.is_empty(), "runtime defineProps should trigger");
        assert!(diags[0].rule == "define-props-declaration");
    }

    #[test]
    fn type_based_define_props_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_props_macro(true)],
            is_typescript: true,
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "type-based defineProps should pass");
    }

    #[test]
    fn js_script_does_not_report() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_props_macro(false)],
            is_typescript: false,
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "define-props-declaration must not fire for JS SFCs"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "define-props-declaration"),
            "must not produce define-props-declaration diagnostic for JS"
        );
    }
}

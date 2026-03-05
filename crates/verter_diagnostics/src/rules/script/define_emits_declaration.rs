//! Rule: define-emits-declaration
//!
//! Enforces type-based `defineEmits` declarations over runtime declarations.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{AnalyzedMacroKind, ScriptAnalysisSnapshot};

pub struct DefineEmitsDeclaration;

impl LintRule for DefineEmitsDeclaration {
    fn name(&self) -> &'static str {
        "define-emits-declaration"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
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
    use crate::config::LintConfig;
    use crate::visitor::LintVisitor;
    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(DefineEmitsDeclaration)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_script(script, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_emit_macro(is_type_based: bool) -> AnalyzedMacro {
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineEmits,
            is_type_based,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            span: Span::new(10, 40),
        }
    }

    #[test]
    fn runtime_define_emits_reports() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_emit_macro(false)],
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
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "type-based defineEmits should pass");
    }
}

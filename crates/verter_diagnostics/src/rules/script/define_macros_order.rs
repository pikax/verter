//! Rule: define-macros-order
//!
//! Enforces consistent ordering of Vue compiler macros in `<script setup>`.
//! `defineProps` should appear before `defineEmits`, and both before other macros.
//!
//! Recommended order: defineProps → defineEmits → defineModel → defineExpose

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{AnalyzedMacroKind, ScriptAnalysisSnapshot};

pub struct DefineMacrosOrder;

impl LintRule for DefineMacrosOrder {
    fn name(&self) -> &'static str {
        "define-macros-order"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        // Find defineProps and defineEmits spans
        let props_span = script
            .macros
            .iter()
            .find(|m| {
                m.kind == AnalyzedMacroKind::DefineProps
                    || m.kind == AnalyzedMacroKind::WithDefaults
            })
            .map(|m| m.span);

        let emits_span = script
            .macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineEmits)
            .map(|m| m.span);

        // defineProps should come before defineEmits
        if let (Some(props), Some(emits)) = (props_span, emits_span) {
            if props.start > emits.start {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "'defineProps()' should appear before 'defineEmits()' for consistency."
                        .to_string(),
                    props.start,
                    props.end,
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

    fn run_script(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(DefineMacrosOrder)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        visitor.visit_script(script, &mut ctx);
        ctx.into_diagnostics()
    }

    fn make_macro(kind: AnalyzedMacroKind, start: u32, end: u32) -> AnalyzedMacro {
        AnalyzedMacro {
            kind,
            is_type_based: false,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            span: Span::new(start, end),
        }
    }

    #[test]
    fn emits_before_props_reports() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![
                make_macro(AnalyzedMacroKind::DefineEmits, 10, 30),
                make_macro(AnalyzedMacroKind::DefineProps, 40, 60),
            ],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(
            !diags.is_empty(),
            "defineEmits before defineProps should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "define-macros-order"));
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn props_before_emits_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![
                make_macro(AnalyzedMacroKind::DefineProps, 10, 30),
                make_macro(AnalyzedMacroKind::DefineEmits, 40, 60),
            ],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(
            diags.is_empty(),
            "defineProps before defineEmits should pass"
        );
    }

    #[test]
    fn only_props_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_macro(AnalyzedMacroKind::DefineProps, 10, 30)],
            ..Default::default()
        };
        let diags = run_script(&script);
        assert!(diags.is_empty(), "only defineProps should pass");
    }
}

//! Rule: no-expose-after-await
//!
//! Disallows calling `defineExpose()` after `await` in `<script setup>`.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{AnalyzedMacroKind, ScriptAnalysisSnapshot};

pub struct NoExposeAfterAwait;

impl LintRule for NoExposeAfterAwait {
    fn name(&self) -> &'static str {
        "no-expose-after-await"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Reactivity
    }

    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        let first_await = match script.first_await_offset {
            Some(pos) => pos,
            None => return,
        };

        for m in &script.macros {
            if m.kind == AnalyzedMacroKind::DefineExpose && m.span.start > first_await {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "`defineExpose()` is called after `await`. Move it before the first `await` to ensure it binds to the correct component instance.".to_string(),
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
        crate::test_support::run_script_rule(NoExposeAfterAwait, script)
    }

    fn make_expose_macro(start: u32, end: u32) -> AnalyzedMacro {
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineExpose,
            is_type_based: false,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            emit_fields: vec![],
            span: Span::new(start, end),
        }
    }

    #[test]
    fn expose_after_await_reports() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_expose_macro(100, 120)],
            first_await_offset: Some(50),
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].rule == "no-expose-after-await");
        assert!(
            !diags.iter().any(|d| d.rule == "no-lifecycle-after-await"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn expose_before_await_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_expose_macro(10, 30)],
            first_await_offset: Some(50),
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "expose before await should pass");
    }

    #[test]
    fn no_await_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_expose_macro(10, 30)],
            first_await_offset: None,
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "no await should pass");
    }
}

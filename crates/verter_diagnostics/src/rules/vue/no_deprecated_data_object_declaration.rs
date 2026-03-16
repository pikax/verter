//! Rule: no-deprecated-data-object-declaration
//!
//! In Vue 2, the `data` option could be a plain object. In Vue 3, `data` must
//! always be a function that returns an object. Detect bindings named `data`
//! with non-function kind (Const/Let) which indicates an object declaration.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{AnalyzedBindingKind, ScriptAnalysisSnapshot};

pub struct NoDeprecatedDataObjectDeclaration;

impl LintRule for NoDeprecatedDataObjectDeclaration {
    fn name(&self) -> &'static str {
        "no-deprecated-data-object-declaration"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for binding in &script.bindings {
            if binding.name != "data" {
                continue;
            }
            match binding.kind {
                AnalyzedBindingKind::Const
                | AnalyzedBindingKind::Let
                | AnalyzedBindingKind::Var => {
                    ctx.report_with_tags(
                        self.name(),
                        self.category().as_str(),
                        "The 'data' property must be a function in Vue 3. Object declarations are deprecated.".to_string(),
                        binding.span.start,
                        binding.span.end,
                        self.default_severity(),
                        vec![DiagnosticTag::Deprecated],
                        DiagnosticSpanKind::ScriptCallSite,
                    );
                }
                _ => {}
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
        crate::test_support::run_script_rule(NoDeprecatedDataObjectDeclaration, script)
    }

    #[test]
    fn data_const_reports() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![AnalyzedBinding {
                name: "data".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: false,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span: Span::new(10, 14),
                used_in_script: false,
                used_in_style: false,
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(!diags.is_empty(), "data as Const should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-deprecated-data-object-declaration"));
        assert!(
            diags[0].tags.contains(&DiagnosticTag::Deprecated),
            "should have Deprecated tag"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn data_function_passes() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![AnalyzedBinding {
                name: "data".to_string(),
                kind: AnalyzedBindingKind::Function,
                is_reactive: false,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span: Span::new(10, 14),
                used_in_script: false,
                used_in_style: false,
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "data as Function should pass");
    }

    #[test]
    fn non_data_binding_passes() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![AnalyzedBinding {
                name: "myVar".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: false,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span: Span::new(10, 15),
                used_in_script: false,
                used_in_style: false,
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "non-data const binding should not trigger"
        );
    }
}

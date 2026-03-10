//! Rule: define-model-type-required
//!
//! When `defineModel()` is called without a type parameter, report a warning.
//! Type parameters improve type safety for v-model bindings.
//!
//! ## Bad
//!
//! ```vue
//! <script setup lang="ts">
//! const model = defineModel()
//! </script>
//! ```
//!
//! ## Good
//!
//! ```vue
//! <script setup lang="ts">
//! const model = defineModel<string>()
//! </script>
//! ```

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{AnalyzedMacroKind, ScriptAnalysisSnapshot};

pub struct DefineModelTypeRequired;

impl LintRule for DefineModelTypeRequired {
    fn name(&self) -> &'static str {
        "define-model-type-required"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueRecommended
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for mac in &script.macros {
            if mac.kind != AnalyzedMacroKind::DefineModel {
                continue;
            }
            if !mac.is_type_based {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    "`defineModel()` should have a type parameter. \
                     Add `defineModel<Type>()` to ensure type safety."
                        .to_string(),
                    mac.span.start,
                    mac.span.end,
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
        crate::test_support::run_script_rule(DefineModelTypeRequired, script)
    }

    #[test]
    fn define_model_without_type_reports() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineModel,
                is_type_based: false,
                type_references: vec![],
                binding_name: Some("model".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: vec![],
                emit_fields: vec![],
                span: Span::new(20, 34),
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            !diags.is_empty(),
            "should report defineModel() without type param"
        );
        assert!(diags.iter().any(|d| d.rule == "define-model-type-required"));
        assert!(
            diags[0].message.contains("type parameter"),
            "message should mention type parameter"
        );
        // Negative assertion
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn define_model_with_type_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineModel,
                is_type_based: true,
                type_references: vec!["string".to_string()],
                binding_name: Some("model".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: vec![],
                emit_fields: vec![],
                span: Span::new(20, 42),
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "should pass when defineModel has type param"
        );
    }

    #[test]
    fn multiple_define_models_reports_only_untyped() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![
                AnalyzedMacro {
                    kind: AnalyzedMacroKind::DefineModel,
                    is_type_based: true,
                    type_references: vec!["string".to_string()],
                    binding_name: Some("model".to_string()),
                    model_name: None,
                    has_inherit_attrs_false: false,
                    prop_fields: vec![],
                    emit_fields: vec![],
                    span: Span::new(20, 42),
                },
                AnalyzedMacro {
                    kind: AnalyzedMacroKind::DefineModel,
                    is_type_based: false,
                    type_references: vec![],
                    binding_name: Some("title".to_string()),
                    model_name: Some("title".to_string()),
                    has_inherit_attrs_false: false,
                    prop_fields: vec![],
                    emit_fields: vec![],
                    span: Span::new(50, 72),
                },
            ],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(diags.len(), 1, "should report only the untyped defineModel");
        assert_eq!(
            diags[0].span.start, 50,
            "should report the second (untyped) macro"
        );
    }

    #[test]
    fn no_define_model_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec![],
                binding_name: Some("props".to_string()),
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: vec![],
                emit_fields: vec![],
                span: Span::new(20, 40),
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "should not report for defineProps");
    }

    #[test]
    fn empty_script_passes() {
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "should pass with no macros");
    }
}

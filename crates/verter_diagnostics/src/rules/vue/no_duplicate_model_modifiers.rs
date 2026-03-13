//! Rule: no-duplicate-model-modifiers
//!
//! Multiple `defineModel()` calls with the same model name are not allowed.
//! Each model name must be unique within a component.
//!
//! ## Bad
//!
//! ```vue
//! <script setup lang="ts">
//! const model = defineModel<string>()
//! const model2 = defineModel<number>()  // duplicate default model
//! </script>
//! ```
//!
//! ## Good
//!
//! ```vue
//! <script setup lang="ts">
//! const model = defineModel<string>()
//! const title = defineModel<string>('title')
//! </script>
//! ```

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{AnalyzedMacroKind, ScriptAnalysisSnapshot};

pub struct NoDuplicateModelModifiers;

impl LintRule for NoDuplicateModelModifiers {
    fn name(&self) -> &'static str {
        "no-duplicate-model-modifiers"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        let models: Vec<_> = script
            .macros
            .iter()
            .filter(|m| m.kind == AnalyzedMacroKind::DefineModel)
            .collect();

        if models.len() < 2 {
            return;
        }

        // Group by model_name (None = default "modelValue")
        let mut seen: std::collections::HashMap<
            Option<&str>,
            &verter_analysis::types::AnalyzedMacro,
        > = std::collections::HashMap::new();

        for mac in &models {
            let key = mac.model_name.as_deref();
            if let Some(first) = seen.get(&key) {
                let display_name = key.unwrap_or("modelValue");
                // Report on the duplicate (second+ occurrence)
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Duplicate `defineModel()` for model name '{}'. \
                         Each model name must be unique.",
                        display_name
                    ),
                    mac.span.start,
                    mac.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::ScriptCallSite,
                );
                // Also report the first occurrence if it hasn't been reported yet
                // (we only report on duplicates, the first one is the "original")
                let _ = first; // suppress unused warning
            } else {
                seen.insert(key, mac);
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
        crate::test_support::run_script_rule(NoDuplicateModelModifiers, script)
    }

    fn make_model(model_name: Option<&str>, span_start: u32, span_end: u32) -> AnalyzedMacro {
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineModel,
            is_type_based: true,
            type_references: vec![],
            binding_name: None,
            model_name: model_name.map(|s| s.to_string()),
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            emit_fields: vec![],
            slot_fields: vec![],
            default_keys: vec![],
            expose_fields: vec![],
            span: Span::new(span_start, span_end),
        }
    }

    #[test]
    fn duplicate_default_model_reports() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_model(None, 10, 30), make_model(None, 40, 60)],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(!diags.is_empty(), "should report duplicate default model");
        assert_eq!(
            diags.len(),
            1,
            "should report exactly one diagnostic for the duplicate"
        );
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-duplicate-model-modifiers"));
        assert!(
            diags[0].message.contains("modelValue"),
            "message should mention modelValue for default model"
        );
        assert_eq!(
            diags[0].span.start, 40,
            "should report on the second occurrence"
        );
        // Negative assertion
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn duplicate_named_model_reports() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![
                make_model(Some("title"), 10, 35),
                make_model(Some("title"), 40, 65),
            ],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(
            diags.len(),
            1,
            "should report one duplicate for named model"
        );
        assert!(
            diags[0].message.contains("title"),
            "message should mention the model name 'title'"
        );
        assert!(
            !diags[0].message.contains("modelValue"),
            "message should NOT mention modelValue for named model"
        );
    }

    #[test]
    fn three_same_models_reports_two() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![
                make_model(None, 10, 30),
                make_model(None, 40, 60),
                make_model(None, 70, 90),
            ],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(
            diags.len(),
            2,
            "should report two diagnostics for three duplicate default models"
        );
    }

    #[test]
    fn different_model_names_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![
                make_model(None, 10, 30),
                make_model(Some("title"), 40, 65),
                make_model(Some("count"), 70, 95),
            ],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "should pass when all model names are different"
        );
    }

    #[test]
    fn single_model_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![make_model(None, 10, 30)],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "should pass with single model");
    }

    #[test]
    fn no_models_passes() {
        let script = ScriptAnalysisSnapshot {
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec![],
                binding_name: None,
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: vec![],
                emit_fields: vec![],
                slot_fields: vec![],
                default_keys: vec![],
                expose_fields: vec![],
                span: Span::new(10, 30),
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "should pass when no defineModel macros");
    }

    #[test]
    fn empty_script_passes() {
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "should pass with empty script");
    }
}

//! Rule: no-import-compiler-macros
//!
//! Disallows importing Vue compiler macros (`defineProps`, `defineEmits`,
//! `defineExpose`, `defineModel`, `defineOptions`, `defineSlots`, `withDefaults`)
//! from `'vue'`. These are compiler macros that are automatically available in
//! `<script setup>` and do not need to be imported.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::{ScriptAnalysisSnapshot, VueApiClassification};

pub struct NoImportCompilerMacros;

/// Compiler macro APIs that should not be imported.
const COMPILER_MACROS: &[VueApiClassification] = &[
    VueApiClassification::DefineProps,
    VueApiClassification::DefineEmits,
    VueApiClassification::DefineModel,
    VueApiClassification::DefineExpose,
    VueApiClassification::DefineOptions,
    VueApiClassification::DefineSlots,
    VueApiClassification::WithDefaults,
];

impl LintRule for NoImportCompilerMacros {
    fn name(&self) -> &'static str {
        "no-import-compiler-macros"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Error)
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        for import in &script.imports {
            if import.source != "vue" {
                continue;
            }

            for binding in &import.bindings {
                if let Some(ref api) = binding.vue_api {
                    if COMPILER_MACROS.contains(api) {
                        ctx.report_with_severity(
                            self.name(),
                            self.category().as_str(),
                            format!(
                                "'{}' is a compiler macro and does not need to be imported from 'vue'.",
                                binding.name
                            ),
                            binding.span.start,
                            binding.span.end,
                            self.default_severity(),
                            DiagnosticSpanKind::ScriptCallSite,
                        );
                    }
                }
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
        crate::test_support::run_script_rule(NoImportCompilerMacros, script)
    }

    fn make_import(source: &str, bindings: Vec<AnalyzedImportBinding>) -> AnalyzedImport {
        AnalyzedImport {
            source: source.to_string(),
            is_type_only: false,
            bindings,
            span: Span::new(0, 50),
            resolved_canonical_id: None,
        }
    }

    fn make_binding(name: &str, api: Option<VueApiClassification>) -> AnalyzedImportBinding {
        AnalyzedImportBinding {
            name: name.to_string(),
            is_type_only: false,
            vue_api: api,
            span: Span::new(10, 30),
        }
    }

    #[test]
    fn import_define_props_from_vue_reports() {
        let script = ScriptAnalysisSnapshot {
            imports: vec![make_import(
                "vue",
                vec![make_binding(
                    "defineProps",
                    Some(VueApiClassification::DefineProps),
                )],
            )],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            !diags.is_empty(),
            "importing defineProps from vue should trigger"
        );
        assert!(diags.iter().any(|d| d.rule == "no-import-compiler-macros"));
        assert!(
            diags[0].message.contains("defineProps"),
            "message should mention defineProps"
        );
        assert!(
            diags[0].message.contains("compiler macro"),
            "message should mention compiler macro"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn import_define_emits_from_vue_reports() {
        let script = ScriptAnalysisSnapshot {
            imports: vec![make_import(
                "vue",
                vec![make_binding(
                    "defineEmits",
                    Some(VueApiClassification::DefineEmits),
                )],
            )],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            !diags.is_empty(),
            "importing defineEmits from vue should trigger"
        );
    }

    #[test]
    fn import_ref_from_vue_passes() {
        let script = ScriptAnalysisSnapshot {
            imports: vec![make_import(
                "vue",
                vec![make_binding("ref", Some(VueApiClassification::Ref))],
            )],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "importing ref from vue should pass");
    }

    #[test]
    fn import_define_props_from_other_source_passes() {
        let script = ScriptAnalysisSnapshot {
            imports: vec![make_import(
                "@vue/runtime-core",
                vec![make_binding(
                    "defineProps",
                    Some(VueApiClassification::DefineProps),
                )],
            )],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "importing from non-vue source should pass"
        );
    }

    #[test]
    fn no_imports_passes() {
        let script = ScriptAnalysisSnapshot::default();
        let diags = run_rule(&script);
        assert!(diags.is_empty(), "no imports should pass");
    }

    #[test]
    fn import_with_defaults_reports() {
        let script = ScriptAnalysisSnapshot {
            imports: vec![make_import(
                "vue",
                vec![make_binding(
                    "withDefaults",
                    Some(VueApiClassification::WithDefaults),
                )],
            )],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            !diags.is_empty(),
            "importing withDefaults from vue should trigger"
        );
    }
}

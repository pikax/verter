//! Rule: component-api-style
//!
//! Enforce Composition API style. Detects Options API patterns (bindings named
//! `data`, `computed`, `methods`, `watch`, etc.) when no Composition API macros
//! (defineProps, defineEmits, etc.) are present.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{LintRule, RuleCategory};
use verter_analysis::types::ScriptAnalysisSnapshot;

/// Options API binding names that indicate non-Composition usage.
const OPTIONS_API_KEYS: &[&str] = &[
    "data",
    "computed",
    "methods",
    "watch",
    "props",
    "emits",
    "components",
    "mixins",
    "extends",
    "setup",
    "render",
    "beforeCreate",
    "created",
    "beforeMount",
    "mounted",
    "beforeUpdate",
    "updated",
    "beforeUnmount",
    "unmounted",
];

pub struct ComponentApiStyle;

impl LintRule for ComponentApiStyle {
    fn name(&self) -> &'static str {
        "component-api-style"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::Script
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_script(&self, script: &ScriptAnalysisSnapshot, ctx: &mut LintContext) {
        // If Composition API macros are present, this is already Composition API
        if !script.macros.is_empty() {
            return;
        }

        for binding in &script.bindings {
            if OPTIONS_API_KEYS.contains(&binding.name.as_str()) {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Options API pattern detected: '{}'. Prefer Composition API with `<script setup>`.",
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

#[cfg(test)]
mod tests {
    use super::*;

    use verter_analysis::types::*;
    use verter_span::Span;

    fn run_rule(script: &ScriptAnalysisSnapshot) -> Vec<crate::diagnostic::LintDiagnostic> {
        crate::test_support::run_script_rule(ComponentApiStyle, script)
    }

    fn make_binding(name: &str) -> AnalyzedBinding {
        AnalyzedBinding {
            name: name.to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: false,
            reactivity_kind: ReactivityKind::None,
            type_annotation: None,
            initializer: None,
            span: Span::new(10, 30),
            used_in_script: false,
            used_in_style: false,
        }
    }

    #[test]
    fn options_api_detected() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![make_binding("data"), make_binding("computed")],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert_eq!(diags.len(), 2, "should report data + computed");
        assert!(diags.iter().all(|d| d.rule == "component-api-style"));
        assert!(
            diags[0].message.contains("data"),
            "message should mention the binding name"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn composition_api_with_macros_passes() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![make_binding("data")],
            macros: vec![AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: false,
                type_references: vec![],
                binding_name: None,
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: vec![],
                emit_fields: vec![],
                slot_fields: vec![],
                span: Span::new(0, 20),
            }],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "should pass when Composition API macros are present"
        );
    }

    #[test]
    fn regular_bindings_pass() {
        let script = ScriptAnalysisSnapshot {
            bindings: vec![make_binding("myVariable"), make_binding("count")],
            ..Default::default()
        };
        let diags = run_rule(&script);
        assert!(
            diags.is_empty(),
            "regular bindings should not trigger the rule"
        );
    }
}

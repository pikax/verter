//! Quick fix: convert a runtime `defineEmits(...)` to type-based `defineEmits<{...}>()`.
//!
//! Handles: `define-emits-declaration`
//!
//! Builds the TypeScript emit type from `AnalyzedMacro.emit_fields`:
//! - Each event gets `eventName: []` (empty tuple — payload types can't be inferred from
//!   runtime validators, so we use the safe conservative `[]` which allows any call)
//!
//! Example output:
//! ```ts
//! defineEmits<{ click: []; update: [] }>()
//! ```

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, AutofixSafety, CodeAction, FileEdit};
use verter_analysis::types::AnalyzedMacroKind;
use verter_diagnostics::LintDiagnostic;

pub struct ConvertToTypedEmits;

impl ActionProvider for ConvertToTypedEmits {
    fn name(&self) -> &str {
        "convert-to-typed-emits"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        if diag.rule != "define-emits-declaration" {
            return vec![];
        }

        let script = match ctx.script {
            Some(s) => s,
            None => return vec![],
        };

        // Find the matching macro by span
        let mac = match script
            .macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineEmits && m.span == diag.span)
        {
            Some(m) => m,
            None => return vec![],
        };

        if mac.emit_fields.is_empty() {
            return vec![];
        }

        // Build the TypeScript emit type: { eventName: [] }
        let type_parts: Vec<String> = mac
            .emit_fields
            .iter()
            .map(|field| format!("{}: []", field.name))
            .collect();

        let type_body = type_parts.join("; ");
        let replacement = format!("defineEmits<{{ {type_body} }}>()");

        vec![CodeAction {
            title: "Convert to type-based defineEmits".to_string(),
            kind: ActionKind::QuickFix,
            edits: vec![FileEdit {
                file_id: None,
                replacement,
                span: mac.span,
            }],
            is_preferred: true,
            diagnostic_rule: Some(diag.rule.clone()),
            safety: AutofixSafety::Safe,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ActionContext;
    use verter_analysis::types::{
        AnalyzedEmitField, AnalyzedMacro, AnalyzedMacroKind, ScriptAnalysisSnapshot,
    };
    use verter_diagnostics::{
        Certainty, DiagnosticSet, DiagnosticSpanKind, LintDiagnostic, Severity,
    };
    use verter_span::Span;

    fn make_diag(span: Span) -> LintDiagnostic {
        LintDiagnostic {
            rule: "define-emits-declaration".to_string(),
            category: "script".to_string(),
            severity: Severity::Warning,
            message: "Use type-based defineEmits".to_string(),
            span,
            tags: vec![],
            span_kind: DiagnosticSpanKind::ScriptCallSite,
            certainty: Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        }
    }

    fn make_emit(name: &str) -> AnalyzedEmitField {
        AnalyzedEmitField {
            name: name.to_string(),
            span: Span::new(0, 0),
            payload_type: None,
            description: None,
            tags: vec![],
        }
    }

    fn make_macro(emit_fields: Vec<AnalyzedEmitField>, span: Span) -> AnalyzedMacro {
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineEmits,
            is_type_based: false,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            emit_fields,
            slot_fields: vec![],
            default_keys: vec![],
            expose_fields: vec![],
            default_values: vec![],
            resolved_local_types: vec![],
            span,
        }
    }

    #[test]
    fn converts_array_form_emits() {
        let span = Span::new(0, 35);
        let mac = make_macro(vec![make_emit("click"), make_emit("update")], span);
        let script = ScriptAnalysisSnapshot {
            macros: vec![mac],
            is_typescript: true,
            ..Default::default()
        };
        let diag = make_diag(span);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source: "defineEmits(['click', 'update'])",
            file_id: "/src/Comp.vue",
            diagnostics: &set,
            template: None,
            script: Some(&script),
            styles: &[],
        };
        let actions = ConvertToTypedEmits.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce one fix");
        assert_eq!(
            actions[0].edits[0].replacement,
            "defineEmits<{ click: []; update: [] }>()"
        );
        assert_eq!(actions[0].edits[0].span, span, "should replace macro span");
        assert!(
            !actions[0].edits[0].replacement.contains("withDefaults"),
            "emits do not use withDefaults"
        );
    }

    #[test]
    fn converts_object_form_emits() {
        let span = Span::new(0, 50);
        let mac = make_macro(vec![make_emit("submit"), make_emit("cancel")], span);
        let script = ScriptAnalysisSnapshot {
            macros: vec![mac],
            is_typescript: true,
            ..Default::default()
        };
        let diag = make_diag(span);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source: "defineEmits({ submit: (p) => true, cancel: () => true })",
            file_id: "/src/Comp.vue",
            diagnostics: &set,
            template: None,
            script: Some(&script),
            styles: &[],
        };
        let actions = ConvertToTypedEmits.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].edits[0].replacement,
            "defineEmits<{ submit: []; cancel: [] }>()"
        );
        assert!(
            !actions[0].edits[0].replacement.contains("=>"),
            "should not include validator functions in output"
        );
    }

    #[test]
    fn no_fix_for_empty_emit_fields() {
        let span = Span::new(0, 15);
        let mac = make_macro(vec![], span);
        let script = ScriptAnalysisSnapshot {
            macros: vec![mac],
            is_typescript: true,
            ..Default::default()
        };
        let diag = make_diag(span);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source: "defineEmits()",
            file_id: "/src/Comp.vue",
            diagnostics: &set,
            template: None,
            script: Some(&script),
            styles: &[],
        };
        let actions = ConvertToTypedEmits.fixes_for_diagnostic(&diag, &ctx);
        assert!(actions.is_empty(), "should not offer fix for empty emits");
    }

    #[test]
    fn no_fix_when_span_does_not_match() {
        let mac_span = Span::new(0, 35);
        let diag_span = Span::new(50, 85);
        let mac = make_macro(vec![make_emit("click")], mac_span);
        let script = ScriptAnalysisSnapshot {
            macros: vec![mac],
            is_typescript: true,
            ..Default::default()
        };
        let diag = make_diag(diag_span);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source: "defineEmits(['click'])",
            file_id: "/src/Comp.vue",
            diagnostics: &set,
            template: None,
            script: Some(&script),
            styles: &[],
        };
        let actions = ConvertToTypedEmits.fixes_for_diagnostic(&diag, &ctx);
        assert!(actions.is_empty(), "should not fix when spans do not match");
    }

    #[test]
    fn ignores_unrelated_rule() {
        let span = Span::new(0, 35);
        let mac = make_macro(vec![make_emit("click")], span);
        let script = ScriptAnalysisSnapshot {
            macros: vec![mac],
            is_typescript: true,
            ..Default::default()
        };
        let mut diag = make_diag(span);
        diag.rule = "some-other-rule".to_string();
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source: "defineEmits(['click'])",
            file_id: "/src/Comp.vue",
            diagnostics: &set,
            template: None,
            script: Some(&script),
            styles: &[],
        };
        let actions = ConvertToTypedEmits.fixes_for_diagnostic(&diag, &ctx);
        assert!(actions.is_empty(), "should not handle unrelated rules");
    }
}

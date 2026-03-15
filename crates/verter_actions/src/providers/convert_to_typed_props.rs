//! Quick fix: convert a runtime `defineProps(...)` to type-based `defineProps<{...}>()`.
//!
//! Handles: `define-props-declaration`
//!
//! Builds the TypeScript interface type from `AnalyzedMacro.prop_fields`:
//! - Uses the type annotation already mapped from runtime constructors
//!   (`String → string`, `Number → number`, etc.)
//! - Falls back to `unknown` for unresolvable types
//! - Props with a default value (in `default_keys`) become optional (`?`)
//!
//! When any prop has a default value, wraps the result in `withDefaults(...)`:
//! ```ts
//! withDefaults(defineProps<{ count?: number; label: string }>(), { count: 0 })
//! ```

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, AutofixSafety, CodeAction, FileEdit};
use verter_analysis::types::AnalyzedMacroKind;
use verter_diagnostics::LintDiagnostic;

pub struct ConvertToTypedProps;

impl ActionProvider for ConvertToTypedProps {
    fn name(&self) -> &str {
        "convert-to-typed-props"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        if diag.rule != "define-props-declaration" {
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
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps && m.span == diag.span)
        {
            Some(m) => m,
            None => return vec![],
        };

        // Build the TypeScript type from prop_fields
        let type_parts: Vec<String> = mac
            .prop_fields
            .iter()
            .map(|field| {
                let optional = if mac.default_keys.contains(&field.name) {
                    "?"
                } else {
                    ""
                };
                let ts_type = field.type_annotation.as_deref().unwrap_or("unknown");
                format!("{}{}: {}", field.name, optional, ts_type)
            })
            .collect();

        if type_parts.is_empty() {
            // Nothing to generate (e.g. defineProps() with no args)
            return vec![];
        }

        let type_body = type_parts.join("; ");

        let replacement = if mac.default_values.is_empty() {
            // No defaults: simple typed call
            format!("defineProps<{{ {type_body} }}>()")
        } else {
            // Build defaults object from AnalyzedDefaultValue
            let defaults_parts: Vec<String> = mac
                .default_values
                .iter()
                .map(|dv| format!("{}: {}", dv.key, dv.value))
                .collect();
            let defaults_body = defaults_parts.join(", ");
            format!("withDefaults(defineProps<{{ {type_body} }}>(), {{ {defaults_body} }})")
        };

        vec![CodeAction {
            title: "Convert to type-based defineProps".to_string(),
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
        AnalyzedDefaultValue, AnalyzedMacro, AnalyzedMacroKind, AnalyzedPropField,
        ScriptAnalysisSnapshot, TypeResolutionSource,
    };
    use verter_diagnostics::{
        Certainty, DiagnosticSet, DiagnosticSpanKind, LintDiagnostic, Severity,
    };
    use verter_span::Span;

    fn make_diag(span: Span) -> LintDiagnostic {
        LintDiagnostic {
            rule: "define-props-declaration".to_string(),
            category: "script".to_string(),
            severity: Severity::Warning,
            message: "Use type-based defineProps".to_string(),
            span,
            tags: vec![],
            span_kind: DiagnosticSpanKind::ScriptCallSite,
            certainty: Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        }
    }

    fn make_prop(name: &str, ts_type: Option<&str>) -> AnalyzedPropField {
        AnalyzedPropField {
            name: name.to_string(),
            is_optional: false,
            span: Span::new(0, 0),
            type_annotation: ts_type.map(String::from),
            description: None,
            tags: vec![],
            resolution_source: TypeResolutionSource::Rust,
            resolution_error: None,
        }
    }

    fn make_macro(
        prop_fields: Vec<AnalyzedPropField>,
        default_keys: Vec<String>,
        default_values: Vec<AnalyzedDefaultValue>,
        span: Span,
    ) -> AnalyzedMacro {
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: false,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields,
            emit_fields: vec![],
            slot_fields: vec![],
            default_keys,
            expose_fields: vec![],
            default_values,
            resolved_local_types: vec![],
            span,
        }
    }

    #[test]
    fn converts_simple_object_props() {
        let span = Span::new(0, 40);
        let mac = make_macro(
            vec![
                make_prop("count", Some("number")),
                make_prop("label", Some("string")),
            ],
            vec![],
            vec![],
            span,
        );
        let script = ScriptAnalysisSnapshot {
            macros: vec![mac],
            is_typescript: true,
            ..Default::default()
        };
        let diag = make_diag(span);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source: "defineProps({ count: Number, label: String })",
            file_id: "/src/Comp.vue",
            diagnostics: &set,
            template: None,
            script: Some(&script),
            styles: &[],
        };
        let actions = ConvertToTypedProps.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce one fix");
        assert_eq!(
            actions[0].edits[0].replacement,
            "defineProps<{ count: number; label: string }>()"
        );
        assert!(
            !actions[0].edits[0].replacement.contains("withDefaults"),
            "should not wrap in withDefaults when no defaults"
        );
        assert_eq!(actions[0].edits[0].span, span, "should replace macro span");
    }

    #[test]
    fn wraps_with_defaults_when_defaults_present() {
        let span = Span::new(0, 60);
        let mac = make_macro(
            vec![
                make_prop("count", Some("number")),
                make_prop("label", Some("string")),
            ],
            vec!["count".to_string()],
            vec![AnalyzedDefaultValue {
                key: "count".to_string(),
                value: "0".to_string(),
                span: Span::new(30, 31),
            }],
            span,
        );
        let script = ScriptAnalysisSnapshot {
            macros: vec![mac],
            is_typescript: true,
            ..Default::default()
        };
        let diag = make_diag(span);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source: "defineProps({ count: { type: Number, default: 0 }, label: String })",
            file_id: "/src/Comp.vue",
            diagnostics: &set,
            template: None,
            script: Some(&script),
            styles: &[],
        };
        let actions = ConvertToTypedProps.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        let replacement = &actions[0].edits[0].replacement;
        assert!(
            replacement.starts_with("withDefaults("),
            "should wrap in withDefaults"
        );
        assert!(
            replacement.contains("count?: number"),
            "should mark count as optional (has default)"
        );
        assert!(
            replacement.contains("label: string"),
            "should keep label as required (no default)"
        );
        assert!(
            replacement.contains("{ count: 0 }"),
            "should include default value"
        );
        assert!(
            !replacement.contains("defineProps({ "),
            "should not contain original runtime syntax"
        );
    }

    #[test]
    fn array_form_uses_unknown_type() {
        let span = Span::new(0, 30);
        let mac = make_macro(
            vec![make_prop("title", None), make_prop("active", None)],
            vec![],
            vec![],
            span,
        );
        let script = ScriptAnalysisSnapshot {
            macros: vec![mac],
            is_typescript: true,
            ..Default::default()
        };
        let diag = make_diag(span);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source: "defineProps(['title', 'active'])",
            file_id: "/src/Comp.vue",
            diagnostics: &set,
            template: None,
            script: Some(&script),
            styles: &[],
        };
        let actions = ConvertToTypedProps.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        let replacement = &actions[0].edits[0].replacement;
        assert!(
            replacement.contains("title: unknown"),
            "should use unknown for untyped props"
        );
        assert!(
            replacement.contains("active: unknown"),
            "should use unknown for untyped props"
        );
        assert!(
            !replacement.contains("None"),
            "should not contain Rust None in output"
        );
    }

    #[test]
    fn no_fix_for_empty_prop_fields() {
        let span = Span::new(0, 15);
        let mac = make_macro(vec![], vec![], vec![], span);
        let script = ScriptAnalysisSnapshot {
            macros: vec![mac],
            is_typescript: true,
            ..Default::default()
        };
        let diag = make_diag(span);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source: "defineProps()",
            file_id: "/src/Comp.vue",
            diagnostics: &set,
            template: None,
            script: Some(&script),
            styles: &[],
        };
        let actions = ConvertToTypedProps.fixes_for_diagnostic(&diag, &ctx);
        assert!(actions.is_empty(), "should not offer fix for empty props");
    }

    #[test]
    fn no_fix_when_span_does_not_match() {
        let mac_span = Span::new(0, 40);
        let diag_span = Span::new(50, 90); // different span
        let mac = make_macro(
            vec![make_prop("count", Some("number"))],
            vec![],
            vec![],
            mac_span,
        );
        let script = ScriptAnalysisSnapshot {
            macros: vec![mac],
            is_typescript: true,
            ..Default::default()
        };
        let diag = make_diag(diag_span);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source: "defineProps({ count: Number })",
            file_id: "/src/Comp.vue",
            diagnostics: &set,
            template: None,
            script: Some(&script),
            styles: &[],
        };
        let actions = ConvertToTypedProps.fixes_for_diagnostic(&diag, &ctx);
        assert!(actions.is_empty(), "should not fix when spans do not match");
    }

    #[test]
    fn ignores_unrelated_rule() {
        let span = Span::new(0, 40);
        let mac = make_macro(
            vec![make_prop("count", Some("number"))],
            vec![],
            vec![],
            span,
        );
        let script = ScriptAnalysisSnapshot {
            macros: vec![mac],
            is_typescript: true,
            ..Default::default()
        };
        let mut diag = make_diag(span);
        diag.rule = "some-other-rule".to_string();
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source: "defineProps({ count: Number })",
            file_id: "/src/Comp.vue",
            diagnostics: &set,
            template: None,
            script: Some(&script),
            styles: &[],
        };
        let actions = ConvertToTypedProps.fixes_for_diagnostic(&diag, &ctx);
        assert!(actions.is_empty(), "should not handle unrelated rules");
    }
}

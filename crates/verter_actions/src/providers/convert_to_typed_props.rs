//! Quick fix: convert a runtime `defineProps(...)` to type-based `defineProps<{...}>()`.
//!
//! Handles: `define-props-declaration`
//!
//! Builds the TypeScript interface type from `AnalyzedMacro.prop_fields`:
//! - Uses the type annotation already mapped from runtime constructors
//!   (`String → string`, `Number → number`, etc.)
//! - Falls back to `unknown` for unresolvable types
//! - Uses `field.is_optional` to determine optionality: `false` only when `required: true` was set
//!   (Vue semantics: runtime props are optional by default)
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

        // Build the TypeScript type from prop_fields.
        // `is_optional` reflects Vue semantics: `true` by default, `false` only when
        // the runtime declaration had `required: true`.
        let type_parts: Vec<String> = mac
            .prop_fields
            .iter()
            .map(|field| {
                let optional = if field.is_optional { "?" } else { "" };
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
            safety: AutofixSafety::Caution,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ActionContext;
    use oxc_allocator::Allocator;
    use oxc_span::SourceType;
    use verter_analysis::build_script_analysis;
    use verter_analysis::types::{
        AnalyzedMacro, AnalyzedMacroKind, AnalyzedPropField, ScriptAnalysisSnapshot,
        TypeResolutionSource,
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

    /// Run real source analysis and return the `ScriptAnalysisSnapshot`.
    fn analyze(source: &str, alloc: &Allocator) -> ScriptAnalysisSnapshot {
        build_script_analysis(source, SourceType::ts(), alloc)
    }

    /// Find the `defineProps` macro span from a real analysis result.
    fn props_span(script: &ScriptAnalysisSnapshot) -> Span {
        script
            .macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .expect("no defineProps macro found")
            .span
    }

    // =========================================================================
    // Guardrail tests — hand-built structs (structural conditions, not output)
    // =========================================================================

    fn make_prop_required(name: &str, ts_type: Option<&str>) -> AnalyzedPropField {
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

    fn make_macro_struct(prop_fields: Vec<AnalyzedPropField>, span: Span) -> AnalyzedMacro {
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
            default_keys: vec![],
            expose_fields: vec![],
            default_values: vec![],
            resolved_local_types: vec![],
            span,
        }
    }

    #[test]
    fn no_fix_for_empty_prop_fields() {
        let span = Span::new(0, 15);
        let mac = make_macro_struct(vec![], span);
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
        let mac = make_macro_struct(vec![make_prop_required("count", Some("number"))], mac_span);
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
        let mac = make_macro_struct(vec![make_prop_required("count", Some("number"))], span);
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

    // =========================================================================
    // Behavioral tests — use real source analysis (TDD: fail first, then implement)
    // =========================================================================

    #[test]
    fn converts_simple_object_props() {
        // Vue semantics: props without required:true are optional by default
        let source = "defineProps({ count: Number, label: String })";
        let alloc = Allocator::new();
        let script = analyze(source, &alloc);
        let span = props_span(&script);
        let diag = make_diag(span);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/Comp.vue",
            diagnostics: &set,
            template: None,
            script: Some(&script),
            styles: &[],
        };
        let actions = ConvertToTypedProps.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce one fix");
        assert_eq!(
            actions[0].edits[0].replacement, "defineProps<{ count?: number; label?: string }>()",
            "props without required:true should be optional"
        );
        assert!(
            !actions[0].edits[0].replacement.contains("withDefaults"),
            "should not wrap in withDefaults when no defaults"
        );
        assert_eq!(actions[0].edits[0].span, span, "should replace macro span");
    }

    #[test]
    fn wraps_with_defaults_when_defaults_present() {
        // Props with defaults are optional; props without required:true are also optional
        let source = "defineProps({ count: { type: Number, default: 0 }, label: String })";
        let alloc = Allocator::new();
        let script = analyze(source, &alloc);
        let span = props_span(&script);
        let diag = make_diag(span);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
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
            "count should be optional (has default)"
        );
        assert!(
            replacement.contains("label?: string"),
            "label should be optional (no required:true)"
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
    fn required_true_prop_is_not_optional() {
        // `required: true` makes a prop required (no `?`)
        let source = "defineProps({ foo: { type: String, required: true }, bar: Number })";
        let alloc = Allocator::new();
        let script = analyze(source, &alloc);
        let span = props_span(&script);
        let diag = make_diag(span);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
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
            replacement.contains("foo: string"),
            "foo with required:true should not have ?"
        );
        assert!(
            !replacement.contains("foo?: string"),
            "foo must not be marked optional"
        );
        assert!(
            replacement.contains("bar?: number"),
            "bar without required:true should be optional"
        );
    }

    #[test]
    fn array_form_uses_unknown_type() {
        // Array form: no type info → unknown, all optional
        let source = "defineProps(['title', 'active'])";
        let alloc = Allocator::new();
        let script = analyze(source, &alloc);
        let span = props_span(&script);
        let diag = make_diag(span);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
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
            replacement.contains("title?: unknown"),
            "should use unknown for untyped props and mark optional"
        );
        assert!(
            replacement.contains("active?: unknown"),
            "should use unknown for untyped props and mark optional"
        );
        assert!(
            !replacement.contains("None"),
            "should not contain Rust None in output"
        );
    }

    #[test]
    fn mixed_fixture_with_ts_assertions() {
        // Full regression: PropType<T>, () => T, required:true, defaults
        let source = r#"defineProps({
  bar: Number,
  foo: { type: String, required: true },
  baz: { type: Object as () => typeof Card, default: () => { return Card } },
  foz: { type: Object as PropType<typeof Card>, default: () => { return Card } }
})"#;
        let alloc = Allocator::new();
        let script = analyze(source, &alloc);
        let span = props_span(&script);
        let diag = make_diag(span);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/Comp.vue",
            diagnostics: &set,
            template: None,
            script: Some(&script),
            styles: &[],
        };
        let actions = ConvertToTypedProps.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        let replacement = &actions[0].edits[0].replacement;

        // bar: Number → optional, number type
        assert!(
            replacement.contains("bar?: number"),
            "bar should be optional number"
        );
        // foo: required:true → required, string type
        assert!(
            replacement.contains("foo: string"),
            "foo should be required string"
        );
        assert!(!replacement.contains("foo?: string"), "foo must not have ?");
        // baz: Object as () => typeof Card → optional, typeof Card
        assert!(
            replacement.contains("baz?: typeof Card"),
            "baz should be optional typeof Card"
        );
        // foz: Object as PropType<typeof Card> → optional, typeof Card
        assert!(
            replacement.contains("foz?: typeof Card"),
            "foz should be optional typeof Card"
        );

        // Negative: asserted types must not degrade
        assert!(
            !replacement.contains(": object"),
            "no prop should degrade to 'object'"
        );
        assert!(
            !replacement.contains(": unknown"),
            "no prop should degrade to 'unknown'"
        );
        assert!(
            !replacement.contains(": Function"),
            "no prop should degrade to 'Function'"
        );

        // baz and foz have defaults → wraps in withDefaults
        assert!(
            replacement.starts_with("withDefaults("),
            "should wrap in withDefaults because baz/foz have defaults"
        );
    }
}

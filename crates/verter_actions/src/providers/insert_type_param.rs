//! Quick fix: insert a type parameter on `ref()` calls.
//!
//! Handles: `require-typed-ref`
//!
//! Infers the type from the literal argument:
//! - `'...'` / `"..."` → `string`
//! - digits / `-digits` → `number`
//! - `true` / `false` → `boolean`
//! - `null` → `null`
//! - `[]` → `unknown[]`
//! - `{}` → `Record<string, unknown>`
//! - anything else → no fix offered (conservative)

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, AutofixSafety, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct InsertTypeParam;

/// Try to infer a TypeScript type from a literal argument string.
fn infer_type_from_literal(arg: &str) -> Option<&'static str> {
    let trimmed = arg.trim();
    if trimmed.is_empty() {
        return None;
    }
    // String literals
    if (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('`') && trimmed.ends_with('`'))
    {
        return Some("string");
    }
    // Boolean
    if trimmed == "true" || trimmed == "false" {
        return Some("boolean");
    }
    // Null
    if trimmed == "null" {
        return Some("null");
    }
    // Empty array
    if trimmed == "[]" {
        return Some("unknown[]");
    }
    // Empty object
    if trimmed == "{}" {
        return Some("Record<string, unknown>");
    }
    // Number (integer or float, possibly negative)
    let numeric = if let Some(rest) = trimmed.strip_prefix('-') {
        rest
    } else {
        trimmed
    };
    if !numeric.is_empty() && numeric.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        // Must have at least one digit
        if numeric.bytes().any(|b| b.is_ascii_digit()) {
            return Some("number");
        }
    }
    None
}

impl ActionProvider for InsertTypeParam {
    fn name(&self) -> &str {
        "insert-type-param"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        if diag.rule != "require-typed-ref" {
            return vec![];
        }

        let source = ctx.source;
        let start = diag.span.start as usize;
        let end = diag.span.end as usize;

        if end > source.len() {
            return vec![];
        }

        let call_text = &source[start..end];

        // Find the opening paren position
        let paren_rel = match call_text.find('(') {
            Some(p) => p,
            None => return vec![],
        };

        // Extract argument text between parens
        let after_paren = &call_text[paren_rel + 1..];
        let close_paren = match after_paren.rfind(')') {
            Some(p) => p,
            None => return vec![],
        };
        let arg_text = &after_paren[..close_paren];

        // No arg → no fix (can't infer type from nothing)
        if arg_text.trim().is_empty() {
            return vec![];
        }

        // Try to infer type
        let inferred = match infer_type_from_literal(arg_text) {
            Some(t) => t,
            None => return vec![],
        };

        // Insert `<Type>` right before the `(`
        let insert_offset = (start + paren_rel) as u32;
        let insert_text = format!("<{inferred}>");

        vec![CodeAction {
            title: format!("Add type parameter <{inferred}>"),
            kind: ActionKind::QuickFix,
            edits: vec![FileEdit {
                file_id: None,
                replacement: insert_text,
                span: verter_span::Span::new(insert_offset, insert_offset),
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
    use verter_diagnostics::{DiagnosticSet, DiagnosticSpanKind, LintDiagnostic, Severity};

    fn make_diag(source: &str, rule: &str) -> LintDiagnostic {
        // Find the ref(...) call in source
        let start = source.find("ref(").unwrap_or(0);
        // Find matching close paren
        let after = &source[start..];
        let end = start + after.find(')').unwrap_or(after.len()) + 1;
        LintDiagnostic {
            rule: rule.to_string(),
            category: "script".to_string(),
            severity: Severity::Warning,
            message: "ref() should have a type parameter".to_string(),
            span: verter_span::Span::new(start as u32, end as u32),
            tags: vec![],
            span_kind: DiagnosticSpanKind::ScriptCallSite,
            certainty: verter_diagnostics::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        }
    }

    #[test]
    fn inserts_number_type() {
        let source = "const x = ref(0)";
        let diag = make_diag(source, "require-typed-ref");
        // Need to create ctx with proper lifetime
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = InsertTypeParam.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce one fix");
        assert_eq!(actions[0].edits[0].replacement, "<number>");
        assert!(
            actions[0].title.contains("number"),
            "title should mention number"
        );
        // Negative: insertion point should be zero-width
        assert_eq!(
            actions[0].edits[0].span.start, actions[0].edits[0].span.end,
            "should be zero-width insertion"
        );
        // Verify insert position is right before the paren
        let insert_pos = actions[0].edits[0].span.start as usize;
        assert_eq!(
            &source[insert_pos..insert_pos + 1],
            "(",
            "should insert before ("
        );
    }

    #[test]
    fn inserts_string_type() {
        let source = "const x = ref('hello')";
        let diag = make_diag(source, "require-typed-ref");
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = InsertTypeParam.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].edits[0].replacement, "<string>");
        assert!(
            !actions[0].edits[0].replacement.contains("number"),
            "must not be number"
        );
    }

    #[test]
    fn inserts_boolean_type() {
        let source = "const x = ref(true)";
        let diag = make_diag(source, "require-typed-ref");
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = InsertTypeParam.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].edits[0].replacement, "<boolean>");
    }

    #[test]
    fn inserts_record_type_for_empty_object() {
        let source = "const x = ref({})";
        let diag = make_diag(source, "require-typed-ref");
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = InsertTypeParam.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].edits[0].replacement, "<Record<string, unknown>>");
        assert!(
            !actions[0].edits[0].replacement.contains("number"),
            "must not be number"
        );
    }

    #[test]
    fn inserts_unknown_array_type() {
        let source = "const x = ref([])";
        let diag = make_diag(source, "require-typed-ref");
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = InsertTypeParam.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].edits[0].replacement, "<unknown[]>");
    }

    #[test]
    fn no_fix_for_variable_arg() {
        let source = "const x = ref(someVar)";
        let diag = make_diag(source, "require-typed-ref");
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = InsertTypeParam.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "should not offer fix for variable argument"
        );
    }

    #[test]
    fn no_fix_for_empty_call() {
        let source = "const x = ref()";
        let diag = make_diag(source, "require-typed-ref");
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = InsertTypeParam.fixes_for_diagnostic(&diag, &ctx);
        assert!(actions.is_empty(), "should not offer fix for empty ref()");
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = "const x = ref(0)";
        let mut diag = make_diag(source, "require-typed-ref");
        diag.rule = "some-other-rule".to_string();
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = InsertTypeParam.fixes_for_diagnostic(&diag, &ctx);
        assert!(actions.is_empty(), "should not handle unrelated rule");
    }

    #[test]
    fn inserts_null_type() {
        let source = "const x = ref(null)";
        let diag = make_diag(source, "require-typed-ref");
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = InsertTypeParam.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].edits[0].replacement, "<null>");
    }

    #[test]
    fn inserts_number_for_negative() {
        let source = "const x = ref(-42)";
        let diag = make_diag(source, "require-typed-ref");
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = InsertTypeParam.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].edits[0].replacement, "<number>");
    }

    #[test]
    fn inserts_number_for_float() {
        let source = "const x = ref(3.14)";
        let diag = make_diag(source, "require-typed-ref");
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = InsertTypeParam.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].edits[0].replacement, "<number>");
    }

    #[test]
    fn inserts_string_for_double_quotes() {
        let source = r#"const x = ref("hello")"#;
        let diag = make_diag(source, "require-typed-ref");
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = InsertTypeParam.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].edits[0].replacement, "<string>");
    }

    #[test]
    fn no_fix_for_function_call_arg() {
        let source = "const x = ref(getDefault())";
        let diag = make_diag(source, "require-typed-ref");
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = InsertTypeParam.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "should not offer fix for function call argument"
        );
    }
}

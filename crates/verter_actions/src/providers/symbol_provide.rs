//! Quick fix: wrap a string literal in `Symbol()` for `provide()` calls.
//!
//! Handles: `require-symbol-provide`
//!
//! Transforms: `provide('key', val)` → `provide(Symbol('key'), val)`

// @ai-generated

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct SymbolProvide;

impl ActionProvider for SymbolProvide {
    fn name(&self) -> &str {
        "symbol-provide"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        if diag.rule != "require-symbol-provide" {
            return vec![];
        }

        let source = ctx.source;
        let start = diag.span.start as usize;
        let end = diag.span.end as usize;

        if end > source.len() {
            return vec![];
        }

        let literal = &source[start..end];

        // The span covers the string literal (e.g., `'key'` or `"key"`).
        // We produce two edits: insert `Symbol(` before and `)` after.
        let insert_before_pos = start as u32;
        let insert_after_pos = end as u32;

        vec![CodeAction {
            title: format!("Wrap with Symbol({literal})"),
            kind: ActionKind::QuickFix,
            edits: vec![
                FileEdit {
                    file_id: None,
                    replacement: "Symbol(".to_string(),
                    span: verter_span::Span::new(insert_before_pos, insert_before_pos),
                },
                FileEdit {
                    file_id: None,
                    replacement: ")".to_string(),
                    span: verter_span::Span::new(insert_after_pos, insert_after_pos),
                },
            ],
            is_preferred: true,
            diagnostic_rule: Some(diag.rule.clone()),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ActionContext;
    use verter_diagnostics::{DiagnosticSet, DiagnosticSpanKind, LintDiagnostic, Severity};

    #[test]
    fn wraps_string_key_with_symbol() {
        let source = "provide('myKey', value)";
        let key_start = source.find("'myKey'").unwrap() as u32;
        let key_end = key_start + "'myKey'".len() as u32;

        let diag = LintDiagnostic {
            rule: "require-symbol-provide".to_string(),
            category: "script".to_string(),
            severity: Severity::Warning,
            message: "Use Symbol keys with provide()".to_string(),
            span: verter_span::Span::new(key_start, key_end),
            tags: vec![],
            span_kind: DiagnosticSpanKind::ScriptCallSite,
        };

        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };

        let actions = SymbolProvide.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert!(
            actions[0].title.contains("Symbol"),
            "title should mention Symbol"
        );
        assert_eq!(
            actions[0].edits.len(),
            2,
            "should have 2 edits (insert before + after)"
        );

        // First edit: insert `Symbol(` before the literal
        let before_edit = &actions[0].edits[0];
        assert_eq!(before_edit.replacement, "Symbol(");
        assert_eq!(
            before_edit.span.start, key_start,
            "insert before the key literal"
        );
        assert_eq!(before_edit.span.end, key_start, "zero-width insert");

        // Second edit: insert `)` after the literal
        let after_edit = &actions[0].edits[1];
        assert_eq!(after_edit.replacement, ")");
        assert_eq!(
            after_edit.span.start, key_end,
            "insert after the key literal"
        );
        assert_eq!(after_edit.span.end, key_end, "zero-width insert");
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = "provide('myKey', value)";
        let diag = LintDiagnostic {
            rule: "other-rule".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "other".to_string(),
            span: verter_span::Span::new(8, 15),
            tags: vec![],
            span_kind: DiagnosticSpanKind::ScriptCallSite,
        };
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = SymbolProvide.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "unrelated rule must not produce actions"
        );
    }
}

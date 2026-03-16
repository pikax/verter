//! Quick fix: add `:is=""` binding to a `<component>` element.
//!
//! Handles: `require-component-is`

// @ai-generated

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, AutofixSafety, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct AddComponentIs;

impl ActionProvider for AddComponentIs {
    fn name(&self) -> &str {
        "add-component-is"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        if diag.rule != "require-component-is" {
            return vec![];
        }

        let source = ctx.source;
        let start = diag.span.start as usize;

        if start >= source.len() {
            return vec![];
        }

        // The diagnostic span points to the <component> tag element.
        // We need to find the position right after the "component" tag name.
        // Look for "<component" starting at or near diag.span.start.
        let snippet = &source[start..];
        let tag_end_rel = snippet
            .find(|c: char| c == '>' || c == '/' || c.is_whitespace())
            .unwrap_or(snippet.len());

        let insert_pos = (start + tag_end_rel) as u32;

        vec![CodeAction {
            title: "Add `:is` binding".to_string(),
            kind: ActionKind::QuickFix,
            edits: vec![FileEdit {
                file_id: None,
                replacement: " :is=\"\"".to_string(),
                span: verter_span::Span::new(insert_pos, insert_pos),
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

    #[test]
    fn adds_is_binding() {
        let source = "<component></component>";
        // Diagnostic span covers the entire element or just the tag name area
        let start = source.find("component").unwrap() as u32;
        let end = start + "component".len() as u32;

        let diag = LintDiagnostic {
            rule: "require-component-is".to_string(),
            category: "vue-essential".to_string(),
            severity: Severity::Error,
            message: "<component> requires a :is binding".to_string(),
            span: verter_span::Span::new(start, end),
            tags: vec![],
            span_kind: DiagnosticSpanKind::FullElement,
            certainty: verter_diagnostics::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
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

        let actions = AddComponentIs.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert!(actions[0].title.contains(":is"), "title should mention :is");
        assert_eq!(actions[0].kind, ActionKind::QuickFix);
        assert!(
            actions[0].edits[0].replacement.contains(":is="),
            "replacement should insert :is binding"
        );
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = "<component></component>";
        let diag = LintDiagnostic {
            rule: "other-rule".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "other".to_string(),
            span: verter_span::Span::new(1, 10),
            tags: vec![],
            span_kind: DiagnosticSpanKind::FullElement,
            certainty: verter_diagnostics::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
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
        let actions = AddComponentIs.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "unrelated rule must not produce actions"
        );
    }
}

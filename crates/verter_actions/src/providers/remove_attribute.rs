//! Quick fix: remove an attribute from an element.
//!
//! Handles: `valid-v-html`, `valid-v-text`, `valid-v-cloak`,
//! `no-deprecated-functional-template`, `no-deprecated-inline-template`,
//! `no-deprecated-router-link-tag-prop`, `no-static-inline-styles`

// @ai-generated

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct RemoveAttribute;

impl ActionProvider for RemoveAttribute {
    fn name(&self) -> &str {
        "remove-attribute"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        let title = match diag.rule.as_str() {
            "valid-v-html" => "Remove invalid v-html argument/modifier",
            "valid-v-text" => "Remove invalid v-text argument/modifier",
            "valid-v-cloak" => "Remove invalid v-cloak argument/modifier/expression",
            "no-deprecated-functional-template" => "Remove 'functional' attribute",
            "no-deprecated-inline-template" => "Remove 'inline-template' attribute",
            "no-deprecated-router-link-tag-prop" => "Remove 'tag' attribute",
            "no-static-inline-styles" => "Remove inline style",
            _ => return vec![],
        };

        let source = ctx.source;
        let start = diag.span.start as usize;
        let end = diag.span.end as usize;

        if end > source.len() {
            return vec![];
        }

        // Expand backwards to include leading whitespace
        let remove_start = {
            let before = &source[..start];
            let ws = before
                .as_bytes()
                .iter()
                .rev()
                .take_while(|&&b| b == b' ' || b == b'\t')
                .count();
            start - ws
        };

        vec![CodeAction {
            title: title.to_string(),
            kind: ActionKind::QuickFix,
            edits: vec![FileEdit {
                file_id: None,
                replacement: String::new(),
                span: verter_span::Span::new(remove_start as u32, end as u32),
            }],
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
    fn removes_functional_attribute() {
        let source = "<template functional>";
        let start = source.find("functional").unwrap() as u32;
        let end = start + "functional".len() as u32;

        let diag = LintDiagnostic {
            rule: "no-deprecated-functional-template".to_string(),
            category: "vue".to_string(),
            severity: Severity::Error,
            message: "'functional' attribute is deprecated".to_string(),
            span: verter_span::Span::new(start, end),
            tags: vec![],
            span_kind: DiagnosticSpanKind::Directive,
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

        let provider = RemoveAttribute;
        let actions = provider.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert!(
            actions[0].title.contains("functional"),
            "title should mention 'functional'"
        );
        assert_eq!(actions[0].kind, ActionKind::QuickFix);
        assert!(
            actions[0].edits[0].replacement.is_empty(),
            "replacement should be empty (deletion)"
        );
        // The edit span should cover the leading space + "functional"
        let edit_span = &actions[0].edits[0].span;
        let removed = &source[edit_span.start as usize..edit_span.end as usize];
        assert!(
            removed.contains("functional"),
            "edit span should cover the 'functional' text"
        );
        assert!(
            !source[..edit_span.start as usize].contains("functional"),
            "'functional' should not appear before the edit span"
        );
    }

    #[test]
    fn removes_inline_style() {
        let source = r#"<div style="color: red">hello</div>"#;
        let start = source.find("style").unwrap() as u32;
        let end = start + r#"style="color: red""#.len() as u32;

        let diag = LintDiagnostic {
            rule: "no-static-inline-styles".to_string(),
            category: "vue".to_string(),
            severity: Severity::Warning,
            message: "Unexpected static inline style".to_string(),
            span: verter_span::Span::new(start, end),
            tags: vec![],
            span_kind: DiagnosticSpanKind::Directive,
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

        let provider = RemoveAttribute;
        let actions = provider.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert!(
            actions[0].edits[0].replacement.is_empty(),
            "replacement should be empty (deletion)"
        );
        assert!(
            actions[0].title.contains("inline style"),
            "title should mention inline style"
        );
        assert!(actions[0].is_preferred, "should be preferred action");
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = r#"<div class="foo">hello</div>"#;
        let diag = LintDiagnostic {
            rule: "some-other-rule".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "unrelated".to_string(),
            span: verter_span::Span::new(5, 16),
            tags: vec![],
            span_kind: DiagnosticSpanKind::Directive,
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

        let actions = RemoveAttribute.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "unrelated rule must not produce actions"
        );
    }
}

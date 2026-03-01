//! Quick fix: replace a `javascript:` URL attribute value with `"#"`.
//!
//! Handles: `no-unsafe-url`

// @ai-generated

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct RemoveUnsafeUrl;

impl ActionProvider for RemoveUnsafeUrl {
    fn name(&self) -> &str {
        "remove-unsafe-url"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        if diag.rule != "no-unsafe-url" {
            return vec![];
        }

        let source = ctx.source;
        let start = diag.span.start as usize;
        let end = diag.span.end as usize;

        if end > source.len() {
            return vec![];
        }

        // The diagnostic span covers the attribute value (the quoted `javascript:...` string).
        // We replace the entire quoted value with `"#"`.
        let current_value = &source[start..end];

        // Detect the quote character used (single or double)
        let replacement = if current_value.starts_with('\'') {
            "'#'".to_string()
        } else {
            "\"#\"".to_string()
        };

        vec![CodeAction {
            title: "Replace unsafe URL with \"#\"".to_string(),
            kind: ActionKind::QuickFix,
            edits: vec![FileEdit {
                file_id: None,
                replacement,
                span: verter_span::Span::new(start as u32, end as u32),
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
    fn replaces_javascript_url() {
        let source = r#"<a href="javascript:void(0)">click</a>"#;
        let val_start = source.find("\"javascript:void(0)\"").unwrap() as u32;
        let val_end = val_start + "\"javascript:void(0)\"".len() as u32;

        let diag = LintDiagnostic {
            rule: "no-unsafe-url".to_string(),
            category: "security".to_string(),
            severity: Severity::Error,
            message: "Unsafe javascript: URL in href".to_string(),
            span: verter_span::Span::new(val_start, val_end),
            tags: vec![],
            span_kind: DiagnosticSpanKind::Attribute,
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

        let actions = RemoveUnsafeUrl.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert!(
            actions[0].title.contains("unsafe"),
            "title should mention unsafe"
        );
        assert_eq!(actions[0].kind, ActionKind::QuickFix);
        assert_eq!(
            actions[0].edits[0].replacement, "\"#\"",
            "should replace with \"#\""
        );
        assert!(
            !actions[0].edits[0].replacement.contains("javascript"),
            "replacement must not contain javascript:"
        );
    }

    #[test]
    fn replaces_single_quoted_javascript_url() {
        let source = "<a href='javascript:alert(1)'>click</a>";
        let val_start = source.find("'javascript:alert(1)'").unwrap() as u32;
        let val_end = val_start + "'javascript:alert(1)'".len() as u32;

        let diag = LintDiagnostic {
            rule: "no-unsafe-url".to_string(),
            category: "security".to_string(),
            severity: Severity::Error,
            message: "Unsafe javascript: URL in href".to_string(),
            span: verter_span::Span::new(val_start, val_end),
            tags: vec![],
            span_kind: DiagnosticSpanKind::Attribute,
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

        let actions = RemoveUnsafeUrl.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].edits[0].replacement, "'#'",
            "should preserve single-quote style"
        );
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = r#"<a href="javascript:void(0)">click</a>"#;
        let diag = LintDiagnostic {
            rule: "other-rule".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "other".to_string(),
            span: verter_span::Span::new(9, 31),
            tags: vec![],
            span_kind: DiagnosticSpanKind::Attribute,
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
        let actions = RemoveUnsafeUrl.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "unrelated rule must not produce actions"
        );
    }
}

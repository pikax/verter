//! Quick fix: remove a directive from an element.
//!
//! Handles: `no-v-text-v-html-on-component`, `no-child-content`

// @ai-generated

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, AutofixSafety, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct RemoveDirective;

impl ActionProvider for RemoveDirective {
    fn name(&self) -> &str {
        "remove-directive"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        let title = match diag.rule.as_str() {
            "no-v-text-v-html-on-component" => {
                if diag.message.contains("v-html") {
                    "Remove v-html directive"
                } else {
                    "Remove v-text directive"
                }
            }
            "no-child-content" => {
                if diag.message.contains("v-html") {
                    "Remove v-html directive"
                } else {
                    "Remove v-text directive"
                }
            }
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
            if before.ends_with(' ') || before.ends_with('\t') {
                start - 1
            } else {
                start
            }
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
    fn removes_v_html_on_component() {
        let source = "<MyComp v-html=\"x\"></MyComp>";
        let start = source.find("v-html").unwrap() as u32;
        let end = start + "v-html=\"x\"".len() as u32;

        let diag = LintDiagnostic {
            rule: "no-v-text-v-html-on-component".to_string(),
            category: "vue-essential".to_string(),
            severity: Severity::Error,
            message: "'v-html' cannot be used on component".to_string(),
            span: verter_span::Span::new(start, end),
            tags: vec![],
            span_kind: DiagnosticSpanKind::Directive,
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
        let provider = RemoveDirective;
        let actions = provider.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert!(
            actions[0].title.contains("v-html"),
            "title should mention v-html"
        );
        assert_eq!(actions[0].kind, ActionKind::QuickFix);
        assert!(
            actions[0].edits[0].replacement.is_empty(),
            "replacement should be empty (deletion)"
        );
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = "<div v-html=\"x\"></div>";
        let diag = LintDiagnostic {
            rule: "other-rule".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "other".to_string(),
            span: verter_span::Span::new(5, 15),
            tags: vec![],
            span_kind: DiagnosticSpanKind::Directive,
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
        let actions = RemoveDirective.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "unrelated rule must not produce actions"
        );
    }
}

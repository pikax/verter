//! Quick fix: remove a stray v-else or v-else-if directive.
//!
//! Handles: `valid-v-else`

// @ai-generated

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, AutofixSafety, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct RemoveStrayDirective;

impl ActionProvider for RemoveStrayDirective {
    fn name(&self) -> &str {
        "remove-stray-directive"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        if diag.rule != "valid-v-else" {
            return vec![];
        }

        let source = ctx.source;
        let start = diag.span.start as usize;
        let end = diag.span.end as usize;

        if end > source.len() {
            return vec![];
        }

        let remove_start = crate::provider::expand_remove_start(source, start);

        // Determine if it's v-else or v-else-if for the title
        let directive_text = &source[start..end];
        let title = if directive_text.contains("v-else-if") || directive_text.contains("else-if") {
            "Remove stray v-else-if directive"
        } else {
            "Remove stray v-else directive"
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
    fn removes_stray_v_else() {
        let source = "<div v-else>content</div>";
        let start = source.find("v-else").unwrap() as u32;
        let end = start + "v-else".len() as u32;

        let diag = LintDiagnostic {
            rule: "valid-v-else".to_string(),
            category: "vue-essential".to_string(),
            severity: Severity::Error,
            message: "v-else used without preceding v-if".to_string(),
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

        let actions = RemoveStrayDirective.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert!(
            actions[0].title.contains("v-else"),
            "title should mention v-else"
        );
        assert_eq!(actions[0].kind, ActionKind::QuickFix);
        assert!(
            actions[0].edits[0].replacement.is_empty(),
            "replacement should be empty (deletion)"
        );
    }

    #[test]
    fn removes_stray_v_else_if() {
        let source = "<div v-else-if=\"x\">content</div>";
        let start = source.find("v-else-if").unwrap() as u32;
        let end = start + "v-else-if=\"x\"".len() as u32;

        let diag = LintDiagnostic {
            rule: "valid-v-else".to_string(),
            category: "vue-essential".to_string(),
            severity: Severity::Error,
            message: "v-else-if used without preceding v-if".to_string(),
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

        let actions = RemoveStrayDirective.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert!(
            actions[0].title.contains("v-else-if"),
            "title should mention v-else-if"
        );
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = "<div v-else>content</div>";
        let diag = LintDiagnostic {
            rule: "other-rule".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "other".to_string(),
            span: verter_span::Span::new(5, 11),
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
        let actions = RemoveStrayDirective.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "unrelated rule must not produce actions"
        );
    }
}

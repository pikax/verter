//! Quick fix: invert a negated v-if condition.
//!
//! Handles: `no-negated-v-if-condition`

// @ai-generated

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, AutofixSafety, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct ToggleNegation;

impl ActionProvider for ToggleNegation {
    fn name(&self) -> &str {
        "toggle-negation"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        if diag.rule != "no-negated-v-if-condition" {
            return vec![];
        }

        let source = ctx.source;
        let start = diag.span.start as usize;
        let end = diag.span.end as usize;

        if end > source.len() {
            return vec![];
        }

        let text = &source[start..end];

        // The span covers the negated expression like "!show" — remove the "!"
        let replacement = if let Some(rest) = text.strip_prefix('!') {
            rest.to_string()
        } else {
            return vec![];
        };

        vec![CodeAction {
            title: "Remove negation from v-if condition".to_string(),
            kind: ActionKind::QuickFix,
            edits: vec![FileEdit {
                file_id: None,
                replacement,
                span: verter_span::Span::new(start as u32, end as u32),
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
    fn removes_negation() {
        let source = r#"<div v-if="!show">a</div><div v-else>b</div>"#;
        let start = source.find("!show").unwrap() as u32;
        let end = start + "!show".len() as u32;
        let diag = LintDiagnostic {
            rule: "no-negated-v-if-condition".to_string(),
            category: "vue".to_string(),
            severity: Severity::Warning,
            message: "negated".to_string(),
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
        let actions = ToggleNegation.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].edits[0].replacement, "show");
        assert!(
            !actions[0].edits[0].replacement.contains('!'),
            "should not contain negation"
        );
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = r#"<div v-if="show">a</div>"#;
        let diag = LintDiagnostic {
            rule: "other-rule".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "other".to_string(),
            span: verter_span::Span::new(0, 5),
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
        assert!(ToggleNegation.fixes_for_diagnostic(&diag, &ctx).is_empty());
    }
}

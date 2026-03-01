//! Quick fix: remove a useless static attribute from a `<template>` element.
//!
//! Handles: `no-useless-template-attributes`

// @ai-generated

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct RemoveTemplateAttr;

impl ActionProvider for RemoveTemplateAttr {
    fn name(&self) -> &str {
        "remove-template-attr"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        if diag.rule != "no-useless-template-attributes" {
            return vec![];
        }

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
            title: "Remove useless attribute".to_string(),
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
    fn removes_useless_attr() {
        let source = r#"<template class="foo" v-if="x"></template>"#;
        let attr_start = source.find("class").unwrap() as u32;
        let attr_end = attr_start + "class=\"foo\"".len() as u32;

        let diag = LintDiagnostic {
            rule: "no-useless-template-attributes".to_string(),
            category: "vue-essential".to_string(),
            severity: Severity::Warning,
            message: "Attribute 'class' is useless on <template>".to_string(),
            span: verter_span::Span::new(attr_start, attr_end),
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

        let actions = RemoveTemplateAttr.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert!(
            actions[0].title.contains("useless"),
            "title should mention useless"
        );
        assert_eq!(actions[0].kind, ActionKind::QuickFix);
        assert!(
            actions[0].edits[0].replacement.is_empty(),
            "replacement should be empty (deletion)"
        );
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = "<template class=\"foo\"></template>";
        let diag = LintDiagnostic {
            rule: "other-rule".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "other".to_string(),
            span: verter_span::Span::new(10, 20),
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
        let actions = RemoveTemplateAttr.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "unrelated rule must not produce actions"
        );
    }
}

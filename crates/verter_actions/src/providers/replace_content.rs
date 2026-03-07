//! Quick fix: replace content in directives or expressions.
//!
//! Handles: `no-v-text`, `v-for-delimiter-style`, `no-boolean-default`,
//! `no-required-prop-with-default`, `prefer-import-from-vue`

// @ai-generated

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, AutofixSafety, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct ReplaceContent;

impl ActionProvider for ReplaceContent {
    fn name(&self) -> &str {
        "replace-content"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        let source = ctx.source;
        let start = diag.span.start as usize;
        let end = diag.span.end as usize;

        if end > source.len() {
            return vec![];
        }

        let text = &source[start..end];

        let (title, replacement) = match diag.rule.as_str() {
            "prefer-import-from-vue" => {
                // Replace @vue/runtime-core or @vue/reactivity etc. with vue
                let replacement = if text.contains("@vue/runtime-core") {
                    text.replace("@vue/runtime-core", "vue")
                } else if text.contains("@vue/reactivity") {
                    text.replace("@vue/reactivity", "vue")
                } else if text.contains("@vue/runtime-dom") {
                    text.replace("@vue/runtime-dom", "vue")
                } else if text.contains("@vue/shared") {
                    text.replace("@vue/shared", "vue")
                } else {
                    return vec![];
                };
                ("Import from 'vue' instead".to_string(), replacement)
            }
            "no-required-prop-with-default" => {
                // Remove "required: true" — the span covers it
                ("Remove 'required: true'".to_string(), String::new())
            }
            "no-boolean-default" => {
                // Remove the default value — the span covers it
                ("Remove boolean default value".to_string(), String::new())
            }
            "v-for-delimiter-style" => {
                // Replace "in" with "of" or vice versa
                let replacement = if text == "in" { "of" } else { "in" };
                (
                    format!("Use '{replacement}' delimiter"),
                    replacement.to_string(),
                )
            }
            "no-v-text" => {
                // The span covers `v-text="expr"`. Replace with text interpolation.
                // This is complex — just remove the directive for now.
                ("Remove v-text directive".to_string(), String::new())
            }
            _ => return vec![],
        };

        // For deletion rules, expand to include leading whitespace
        let actual_start = if replacement.is_empty() {
            let before = &source[..start];
            let ws = before
                .as_bytes()
                .iter()
                .rev()
                .take_while(|&&b| b == b' ' || b == b'\t' || b == b',')
                .count();
            start - ws
        } else {
            start
        };

        vec![CodeAction {
            title,
            kind: ActionKind::QuickFix,
            edits: vec![FileEdit {
                file_id: None,
                replacement,
                span: verter_span::Span::new(actual_start as u32, end as u32),
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
    use verter_diagnostics::{DiagnosticSet, DiagnosticSpanKind, LintDiagnostic, Severity};

    #[test]
    fn replaces_vue_import_source() {
        let source = "import { ref } from '@vue/reactivity'";
        let start = source.find("'@vue/reactivity'").unwrap() as u32;
        let end = start + "'@vue/reactivity'".len() as u32;
        let diag = LintDiagnostic {
            rule: "prefer-import-from-vue".to_string(),
            category: "script".to_string(),
            severity: Severity::Warning,
            message: "import from vue".to_string(),
            span: verter_span::Span::new(start, end),
            tags: vec![],
            span_kind: DiagnosticSpanKind::ScriptCallSite,
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
        let actions = ReplaceContent.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert!(
            actions[0].edits[0].replacement.contains("vue"),
            "should contain vue"
        );
        assert!(
            !actions[0].edits[0].replacement.contains("@vue/reactivity"),
            "should not contain @vue/reactivity"
        );
    }

    #[test]
    fn replaces_v_for_delimiter() {
        let source = "item in items";
        let start = source.find("in").unwrap() as u32;
        let end = start + 2;
        let diag = LintDiagnostic {
            rule: "v-for-delimiter-style".to_string(),
            category: "vue".to_string(),
            severity: Severity::Warning,
            message: "use of".to_string(),
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
        let actions = ReplaceContent.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].edits[0].replacement, "of");
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = "some code";
        let diag = LintDiagnostic {
            rule: "other".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "x".to_string(),
            span: verter_span::Span::new(0, 4),
            tags: vec![],
            span_kind: DiagnosticSpanKind::ScriptCallSite,
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
        assert!(ReplaceContent.fixes_for_diagnostic(&diag, &ctx).is_empty());
    }
}

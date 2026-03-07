//! Quick fix: convert a void element to self-closing form.
//!
//! Handles: `html-self-closing`

// @ai-generated

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, AutofixSafety, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct HtmlSelfClose;

impl ActionProvider for HtmlSelfClose {
    fn name(&self) -> &str {
        "html-self-close"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        if diag.rule != "html-self-closing" {
            return vec![];
        }

        let source = ctx.source;
        let start = diag.span.start as usize;

        if start >= source.len() {
            return vec![];
        }

        // The diagnostic span points to the element. We need to:
        // 1. Find the closing `>` of the opening tag.
        // 2. Find and remove the closing tag `</tagname>`.

        let snippet = &source[start..];

        // Find the opening tag's closing `>`
        let Some(open_tag_close_rel) = snippet.find('>') else {
            return vec![];
        };
        let open_tag_close_abs = start + open_tag_close_rel;

        // Check if it's already self-closing (shouldn't happen, but guard anyway)
        if open_tag_close_rel > 0 && snippet.as_bytes().get(open_tag_close_rel - 1) == Some(&b'/') {
            return vec![];
        }

        // Extract the tag name from the snippet (starts with `<`)
        let tag_name: &str = snippet
            .trim_start_matches('<')
            .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .next()
            .unwrap_or("");

        if tag_name.is_empty() {
            return vec![];
        }

        // Find the closing tag `</tagname>` after the opening tag
        let after_open = &source[open_tag_close_abs + 1..];
        let close_tag = format!("</{}>", tag_name);
        let close_tag_abs = after_open.find(&close_tag).map(|rel| {
            let abs = open_tag_close_abs + 1 + rel;
            (abs, abs + close_tag.len())
        });

        // Build the edit(s)
        // Edit 1: Replace `>` with `/>` at the end of the opening tag
        let mut edits = vec![FileEdit {
            file_id: None,
            replacement: "/>".to_string(),
            span: verter_span::Span::new(
                open_tag_close_abs as u32,
                (open_tag_close_abs + 1) as u32,
            ),
        }];

        // Edit 2 (if any): Remove the closing tag and any whitespace before it
        if let Some((close_start, close_end)) = close_tag_abs {
            // Include any whitespace between content and closing tag
            let between = &source[open_tag_close_abs + 1..close_start];
            let trim_start = between.len() - between.trim_start().len();
            // Only eat whitespace-only content between open and close
            let actual_close_start = if between.trim().is_empty() {
                open_tag_close_abs + 1 + trim_start
            } else {
                close_start
            };

            edits.push(FileEdit {
                file_id: None,
                replacement: String::new(),
                span: verter_span::Span::new(actual_close_start as u32, close_end as u32),
            });
        }

        vec![CodeAction {
            title: format!("Convert <{tag_name}> to self-closing"),
            kind: ActionKind::QuickFix,
            edits,
            is_preferred: true,
            diagnostic_rule: Some(diag.rule.clone()),
            safety: AutofixSafety::StyleOnly,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ActionContext;
    use verter_diagnostics::{DiagnosticSet, DiagnosticSpanKind, LintDiagnostic, Severity};

    fn make_diag(rule: &str, source: &str) -> LintDiagnostic {
        LintDiagnostic {
            rule: rule.to_string(),
            category: "vue-recommended".to_string(),
            severity: Severity::Warning,
            message: "Void element should be self-closing".to_string(),
            span: verter_span::Span::new(0, source.len() as u32),
            tags: vec![],
            span_kind: DiagnosticSpanKind::FullElement,
            certainty: verter_diagnostics::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        }
    }

    #[test]
    fn converts_br_to_self_closing() {
        let source = "<br></br>";
        let diag = make_diag("html-self-closing", source);

        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };

        let actions = HtmlSelfClose.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert!(actions[0].title.contains("br"), "title should mention br");
        assert_eq!(actions[0].kind, ActionKind::QuickFix);
        // Should have 2 edits: replace `>` with `/>` and remove `</br>`
        assert!(actions[0].edits.len() >= 1, "should have at least 1 edit");
        // First edit: `>` → `/>`
        let first_edit = &actions[0].edits[0];
        assert_eq!(first_edit.replacement, "/>", "should replace > with />");
    }

    #[test]
    fn converts_input_without_close_tag() {
        // Some void elements may appear as `<input>` without explicit closing tag
        let source = "<input type=\"text\">";
        let diag = make_diag("html-self-closing", source);

        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };

        let actions = HtmlSelfClose.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert!(
            actions[0].title.contains("input"),
            "title should mention input"
        );
        // First edit: `>` → `/>`
        assert_eq!(
            actions[0].edits[0].replacement, "/>",
            "should replace > with />"
        );
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = "<br></br>";
        let diag = make_diag("other-rule", source);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = HtmlSelfClose.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "unrelated rule must not produce actions"
        );
    }
}

//! Quick fix: insert a missing attribute on an element/block.
//!
//! Handles: `html-button-has-type`, `block-lang`, `enforce-style-attribute`,
//! `use-v-on-exact`

// @ai-generated

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, AutofixSafety, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct InsertAttribute;

impl ActionProvider for InsertAttribute {
    fn name(&self) -> &str {
        "insert-attribute"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        let source = ctx.source;
        let start = diag.span.start as usize;
        let end = diag.span.end as usize;

        if end > source.len() {
            return vec![];
        }

        let text = &source[start..end];

        let (title, insert_text, insert_offset) = match diag.rule.as_str() {
            "html-button-has-type" => {
                // The span is the <button> open tag. Insert type="button" before the >
                if let Some(gt_pos) = text.find('>') {
                    (
                        "Add type=\"button\"",
                        " type=\"button\"".to_string(),
                        start + gt_pos,
                    )
                } else {
                    return vec![];
                }
            }
            "block-lang" => {
                // The span is <script setup> or <script>. Insert lang="ts" before >
                if let Some(gt_pos) = text.find('>') {
                    (
                        "Add lang=\"ts\"",
                        " lang=\"ts\"".to_string(),
                        start + gt_pos,
                    )
                } else {
                    return vec![];
                }
            }
            "enforce-style-attribute" => {
                // The span is <style>. Insert scoped before >
                if let Some(gt_pos) = text.find('>') {
                    (
                        "Add 'scoped' attribute",
                        " scoped".to_string(),
                        start + gt_pos,
                    )
                } else {
                    return vec![];
                }
            }
            "use-v-on-exact" => {
                // The span is the event handler like @click="handler". Add .exact modifier.
                // Insert .exact before the = sign
                if let Some(eq_pos) = text.find('=') {
                    ("Add .exact modifier", ".exact".to_string(), start + eq_pos)
                } else {
                    return vec![];
                }
            }
            _ => return vec![],
        };

        vec![CodeAction {
            title: title.to_string(),
            kind: ActionKind::QuickFix,
            edits: vec![FileEdit {
                file_id: None,
                replacement: insert_text,
                // Zero-width span at insert point = insertion
                span: verter_span::Span::new(insert_offset as u32, insert_offset as u32),
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
    fn adds_type_button() {
        let source = "<button>Click</button>";
        let tag_start = source.find("<button>").unwrap() as u32;
        let tag_end = tag_start + "<button>".len() as u32;

        let diag = LintDiagnostic {
            rule: "html-button-has-type".to_string(),
            category: "html".to_string(),
            severity: Severity::Warning,
            message: "<button> should have an explicit type attribute".to_string(),
            span: verter_span::Span::new(tag_start, tag_end),
            tags: vec![],
            span_kind: DiagnosticSpanKind::ElementOpenTag,
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

        let actions = InsertAttribute.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert!(
            actions[0].title.contains("type="),
            "title should mention type attribute"
        );
        assert_eq!(actions[0].kind, ActionKind::QuickFix);
        assert_eq!(
            actions[0].edits[0].replacement, " type=\"button\"",
            "replacement should insert type=\"button\""
        );
        // Verify insertion point is a zero-width span (insertion, not replacement)
        assert_eq!(
            actions[0].edits[0].span.start, actions[0].edits[0].span.end,
            "span should be zero-width (insertion)"
        );
        // Verify the insertion point is right before the >
        let insert_pos = actions[0].edits[0].span.start as usize;
        assert_eq!(
            &source[insert_pos..insert_pos + 1],
            ">",
            "insertion should be right before the > character"
        );
        assert!(actions[0].is_preferred, "should be preferred action");
        assert_eq!(
            actions[0].diagnostic_rule.as_deref(),
            Some("html-button-has-type")
        );
    }

    #[test]
    fn adds_scoped_attribute() {
        let source = "<style>.foo { color: red; }</style>";
        let tag_start = source.find("<style>").unwrap() as u32;
        let tag_end = tag_start + "<style>".len() as u32;

        let diag = LintDiagnostic {
            rule: "enforce-style-attribute".to_string(),
            category: "vue-recommended".to_string(),
            severity: Severity::Warning,
            message: "<style> should have 'scoped' attribute".to_string(),
            span: verter_span::Span::new(tag_start, tag_end),
            tags: vec![],
            span_kind: DiagnosticSpanKind::ElementOpenTag,
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

        let actions = InsertAttribute.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert!(
            actions[0].title.contains("scoped"),
            "title should mention scoped"
        );
        assert_eq!(
            actions[0].edits[0].replacement, " scoped",
            "replacement should insert scoped"
        );
        // Verify insertion point is a zero-width span (insertion, not replacement)
        assert_eq!(
            actions[0].edits[0].span.start, actions[0].edits[0].span.end,
            "span should be zero-width (insertion)"
        );
        // Verify the insertion point is right before the >
        let insert_pos = actions[0].edits[0].span.start as usize;
        assert_eq!(
            &source[insert_pos..insert_pos + 1],
            ">",
            "insertion should be right before the > character"
        );
        // Negative: no stray text in replacement
        assert!(
            !actions[0].edits[0].replacement.contains('>'),
            "replacement must not contain >"
        );
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = "<button>Click</button>";
        let diag = LintDiagnostic {
            rule: "other-rule".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "other".to_string(),
            span: verter_span::Span::new(0, 8),
            tags: vec![],
            span_kind: DiagnosticSpanKind::ElementOpenTag,
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

        let actions = InsertAttribute.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "unrelated rule must not produce actions"
        );
    }
}

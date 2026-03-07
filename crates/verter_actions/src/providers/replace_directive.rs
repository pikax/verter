//! Quick fix: replace deprecated directives/APIs with Vue 3 equivalents.
//!
//! Handles: `no-deprecated-v-is`, `no-deprecated-html-element-is`,
//! `no-deprecated-dollar-scopedslots-api`, `no-deprecated-destroyed-lifecycle`

// @ai-generated

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, AutofixSafety, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct ReplaceDirective;

impl ActionProvider for ReplaceDirective {
    fn name(&self) -> &str {
        "replace-directive"
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
            "no-deprecated-v-is" => {
                // v-is="Component" → is="vue:Component"
                // Extract expression from v-is="..."
                if let Some(expr) = text
                    .strip_prefix("v-is=\"")
                    .and_then(|s| s.strip_suffix('"'))
                {
                    (
                        "Replace v-is with is=\"vue:...\"".to_string(),
                        format!("is=\"vue:{expr}\""),
                    )
                } else {
                    return vec![];
                }
            }
            "no-deprecated-html-element-is" => {
                // is="component" → v-is="'component'"
                if let Some(comp) = text.strip_prefix("is=\"").and_then(|s| s.strip_suffix('"')) {
                    (
                        "Replace is with v-is".to_string(),
                        format!("v-is=\"'{comp}'\""),
                    )
                } else {
                    return vec![];
                }
            }
            "no-deprecated-dollar-scopedslots-api" => {
                let replacement = text.replace("$scopedSlots", "$slots");
                ("Replace $scopedSlots with $slots".to_string(), replacement)
            }
            "no-deprecated-destroyed-lifecycle" => {
                let replacement = if text.contains("beforeDestroy") {
                    text.replace("beforeDestroy", "beforeUnmount")
                } else {
                    text.replace("destroyed", "unmounted")
                };
                let title = if text.contains("beforeDestroy") {
                    "Replace beforeDestroy with beforeUnmount"
                } else {
                    "Replace destroyed with unmounted"
                };
                (title.to_string(), replacement)
            }
            _ => return vec![],
        };

        vec![CodeAction {
            title,
            kind: ActionKind::QuickFix,
            edits: vec![FileEdit {
                file_id: None,
                replacement,
                span: verter_span::Span::new(start as u32, end as u32),
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

    fn make_diag(
        rule: &str,
        source: &str,
        fragment: &str,
        span_kind: DiagnosticSpanKind,
    ) -> LintDiagnostic {
        let start = source.find(fragment).unwrap() as u32;
        let end = start + fragment.len() as u32;
        LintDiagnostic {
            rule: rule.to_string(),
            category: "vue-essential".to_string(),
            severity: Severity::Warning,
            message: format!("Deprecated: {fragment}"),
            span: verter_span::Span::new(start, end),
            tags: vec![],
            span_kind,
            certainty: verter_diagnostics::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        }
    }

    #[test]
    fn replaces_scoped_slots_with_slots() {
        let source = r#"<script>this.$scopedSlots.default()</script>"#;
        let fragment = "$scopedSlots";
        let diag = make_diag(
            "no-deprecated-dollar-scopedslots-api",
            source,
            fragment,
            DiagnosticSpanKind::ScriptCallSite,
        );

        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };

        let actions = ReplaceDirective.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert!(
            actions[0].edits[0].replacement.contains("$slots"),
            "replacement should contain $slots"
        );
        assert!(
            !actions[0].edits[0].replacement.contains("$scopedSlots"),
            "replacement should not contain $scopedSlots"
        );
    }

    #[test]
    fn replaces_destroyed_with_unmounted() {
        let source = r#"<script>export default { destroyed() {} }</script>"#;
        let fragment = "destroyed";
        let diag = make_diag(
            "no-deprecated-destroyed-lifecycle",
            source,
            fragment,
            DiagnosticSpanKind::ScriptCallSite,
        );

        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };

        let actions = ReplaceDirective.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert_eq!(
            actions[0].edits[0].replacement, "unmounted",
            "should replace destroyed with unmounted"
        );
        assert!(
            !actions[0].edits[0].replacement.contains("destroyed"),
            "replacement should not contain destroyed"
        );
    }

    #[test]
    fn replaces_before_destroy_with_before_unmount() {
        let source = r#"<script>export default { beforeDestroy() {} }</script>"#;
        let fragment = "beforeDestroy";
        let diag = make_diag(
            "no-deprecated-destroyed-lifecycle",
            source,
            fragment,
            DiagnosticSpanKind::ScriptCallSite,
        );

        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };

        let actions = ReplaceDirective.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert_eq!(
            actions[0].edits[0].replacement, "beforeUnmount",
            "should replace beforeDestroy with beforeUnmount"
        );
        assert!(
            !actions[0].edits[0].replacement.contains("beforeDestroy"),
            "replacement should not contain beforeDestroy"
        );
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = r#"<div v-bind:class="foo"></div>"#;
        let diag = LintDiagnostic {
            rule: "other-rule".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "other".to_string(),
            span: verter_span::Span::new(5, 22),
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
        let actions = ReplaceDirective.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "unrelated rule must not produce actions"
        );
    }
}

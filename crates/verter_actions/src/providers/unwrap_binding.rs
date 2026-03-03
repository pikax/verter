//! Quick fix: unwrap or simplify binding expressions.
//!
//! Handles: `no-useless-v-bind`, `no-useless-mustaches`, `this-in-template`,
//! `prefer-separate-static-class`

// @ai-generated

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct UnwrapBinding;

impl ActionProvider for UnwrapBinding {
    fn name(&self) -> &str {
        "unwrap-binding"
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
            "no-useless-v-bind" | "prefer-separate-static-class" => {
                // :prop="'value'" → prop="value"
                // The span covers the full attribute including `:` prefix
                // Find the `:` or `v-bind:` prefix, the prop name, and the quoted value
                let attr_text = text;
                let prop_name = if let Some(rest) = attr_text.strip_prefix(':') {
                    rest.split('=').next().unwrap_or("")
                } else if let Some(rest) = attr_text.strip_prefix("v-bind:") {
                    rest.split('=').next().unwrap_or("")
                } else {
                    return vec![];
                };

                // Extract the inner value: ="'literal'" → literal
                if let Some(eq_pos) = attr_text.find('=') {
                    let value_part = &attr_text[eq_pos + 1..];
                    // value_part is like "'literal'" or "\"'literal'\""
                    let inner = value_part.trim_matches('"').trim_matches('\'');
                    (
                        "Use static attribute".to_string(),
                        format!("{prop_name}=\"{inner}\""),
                    )
                } else {
                    return vec![];
                }
            }
            "no-useless-mustaches" => {
                // {{ "literal" }} → literal
                let inner = text.trim_start_matches("{{").trim_end_matches("}}").trim();
                let unquoted = inner.trim_matches('"').trim_matches('\'');
                ("Replace with plain text".to_string(), unquoted.to_string())
            }
            "this-in-template" => {
                // this.foo → foo
                let replacement = if let Some(rest) = text.strip_prefix("this.") {
                    rest.to_string()
                } else {
                    text.replace("this.", "")
                };
                ("Remove 'this.' prefix".to_string(), replacement)
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
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ActionContext;
    use verter_diagnostics::{DiagnosticSet, DiagnosticSpanKind, LintDiagnostic, Severity};

    #[test]
    fn this_in_template_removes_prefix() {
        let source = r#"<div>{{ this.foo }}</div>"#;
        let this_start = source.find("this.foo").unwrap() as u32;
        let this_end = this_start + "this.foo".len() as u32;

        let diag = LintDiagnostic {
            rule: "this-in-template".to_string(),
            category: "vue-essential".to_string(),
            severity: Severity::Error,
            message: "Unexpected 'this' in template".to_string(),
            span: verter_span::Span::new(this_start, this_end),
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

        let actions = UnwrapBinding.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert_eq!(
            actions[0].edits[0].replacement, "foo",
            "replacement should be 'foo'"
        );
        assert!(
            !actions[0].edits[0].replacement.contains("this."),
            "replacement must not contain 'this.'"
        );
        assert!(actions[0].is_preferred, "should be preferred action");
        assert_eq!(actions[0].kind, ActionKind::QuickFix);
        assert_eq!(
            actions[0].diagnostic_rule.as_deref(),
            Some("this-in-template")
        );
    }

    #[test]
    fn no_useless_mustaches_unwraps() {
        let source = r#"<div>{{ "hello" }}</div>"#;
        let mustache_start = source.find("{{ \"hello\" }}").unwrap() as u32;
        let mustache_end = mustache_start + "{{ \"hello\" }}".len() as u32;

        let diag = LintDiagnostic {
            rule: "no-useless-mustaches".to_string(),
            category: "vue-recommended".to_string(),
            severity: Severity::Warning,
            message: "Useless mustaches".to_string(),
            span: verter_span::Span::new(mustache_start, mustache_end),
            tags: vec![],
            span_kind: DiagnosticSpanKind::ElementOpenTag,
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

        let actions = UnwrapBinding.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert_eq!(
            actions[0].edits[0].replacement, "hello",
            "replacement should be 'hello'"
        );
        assert!(
            !actions[0].edits[0].replacement.contains("{{"),
            "replacement must not contain '{{'"
        );
        assert!(
            !actions[0].edits[0].replacement.contains("}}"),
            "replacement must not contain '}}'"
        );
        assert!(
            !actions[0].edits[0].replacement.contains('"'),
            "replacement must not contain quotes"
        );
        assert_eq!(actions[0].kind, ActionKind::QuickFix);
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = r#"<div>{{ "hello" }}</div>"#;
        let diag = LintDiagnostic {
            rule: "other-rule".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "other".to_string(),
            span: verter_span::Span::new(5, 18),
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

        let actions = UnwrapBinding.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "unrelated rule must not produce actions"
        );
    }
}

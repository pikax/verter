//! Quick fix: replace verbose directive syntax with shorthand.
//!
//! Handles: `v-bind-style`, `v-on-style`, `v-slot-style`
//!
//! - `v-bind:foo="x"` → `:foo="x"`
//! - `v-on:click="fn"` → `@click="fn"`
//! - `v-slot:header` → `#header`

// @ai-generated

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct ShorthandDirective;

impl ActionProvider for ShorthandDirective {
    fn name(&self) -> &str {
        "shorthand-directive"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        let prefix_and_sigil: Option<(&str, &str)> = match diag.rule.as_str() {
            "v-bind-style" => Some(("v-bind:", ":")),
            "v-on-style" => Some(("v-on:", "@")),
            "v-slot-style" => Some(("v-slot:", "#")),
            _ => None,
        };

        let Some((prefix, sigil)) = prefix_and_sigil else {
            return vec![];
        };

        let source = ctx.source;
        let start = diag.span.start as usize;
        let end = diag.span.end as usize;

        if end > source.len() {
            return vec![];
        }

        let directive_text = &source[start..end];

        // The directive text should start with the prefix; replace it with the sigil
        if !directive_text.starts_with(prefix) {
            return vec![];
        }

        let replacement = format!("{}{}", sigil, &directive_text[prefix.len()..]);

        vec![CodeAction {
            title: format!("Use '{}' shorthand", sigil),
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

    fn make_diag(rule: &str, source: &str, directive: &str) -> LintDiagnostic {
        let start = source.find(directive).unwrap() as u32;
        let end = start + directive.len() as u32;
        LintDiagnostic {
            rule: rule.to_string(),
            category: "vue-recommended".to_string(),
            severity: Severity::Warning,
            message: format!("Use shorthand for {directive}"),
            span: verter_span::Span::new(start, end),
            tags: vec![],
            span_kind: DiagnosticSpanKind::Directive,
        }
    }

    #[test]
    fn converts_v_bind_to_colon() {
        let source = r#"<div v-bind:class="foo"></div>"#;
        let directive = "v-bind:class=\"foo\"";
        let diag = make_diag("v-bind-style", source, directive);

        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };

        let actions = ShorthandDirective.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert_eq!(
            actions[0].edits[0].replacement, ":class=\"foo\"",
            "should replace v-bind:class with :class"
        );
        assert!(
            !actions[0].edits[0].replacement.contains("v-bind"),
            "should not contain v-bind"
        );
    }

    #[test]
    fn converts_v_on_to_at() {
        let source = r#"<div v-on:click="handler"></div>"#;
        let directive = "v-on:click=\"handler\"";
        let diag = make_diag("v-on-style", source, directive);

        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };

        let actions = ShorthandDirective.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert_eq!(
            actions[0].edits[0].replacement, "@click=\"handler\"",
            "should replace v-on:click with @click"
        );
        assert!(
            !actions[0].edits[0].replacement.contains("v-on"),
            "should not contain v-on"
        );
    }

    #[test]
    fn converts_v_slot_to_hash() {
        let source = "<template v-slot:header></template>";
        let directive = "v-slot:header";
        let diag = make_diag("v-slot-style", source, directive);

        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };

        let actions = ShorthandDirective.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert_eq!(
            actions[0].edits[0].replacement, "#header",
            "should replace v-slot:header with #header"
        );
        assert!(
            !actions[0].edits[0].replacement.contains("v-slot"),
            "should not contain v-slot"
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
        let actions = ShorthandDirective.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "unrelated rule must not produce actions"
        );
    }
}

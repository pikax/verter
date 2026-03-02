//! Quick fix: convert `:foo="foo"` → `:foo` (same-name shorthand).
//! Refactoring: expand `:foo` → `:foo="foo"`.
//!
//! Handles `prefer-v-bind-shorthand` diagnostic fixes and position-based
//! expand actions for Vue 3.4+ same-name shorthand.

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct VBindShorthand;

/// Convert a kebab-case string to camelCase.
fn kebab_to_camel(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for ch in s.chars() {
        if ch == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

impl ActionProvider for VBindShorthand {
    fn name(&self) -> &str {
        "v-bind-shorthand"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        if diag.rule != "prefer-v-bind-shorthand" {
            return vec![];
        }

        let source = ctx.source;
        let start = diag.span.start as usize;
        let end = diag.span.end as usize;

        if end > source.len() {
            return vec![];
        }

        let directive_text = &source[start..end];

        // Find the `=` that separates argument from value.
        // The directive text looks like `:foo="foo"` or `:foo-bar="fooBar"`.
        let Some(eq_pos) = directive_text.find('=') else {
            return vec![];
        };

        // Remove everything from `=` to end (the `="value"` part)
        let remove_start = start + eq_pos;
        let remove_end = end;

        vec![CodeAction {
            title: "Use v-bind same-name shorthand".to_string(),
            kind: ActionKind::QuickFix,
            edits: vec![FileEdit {
                file_id: None,
                replacement: String::new(),
                span: verter_span::Span::new(remove_start as u32, remove_end as u32),
            }],
            is_preferred: true,
            diagnostic_rule: Some(diag.rule.clone()),
        }]
    }

    fn actions_at(&self, offset: u32, ctx: &ActionContext) -> Vec<CodeAction> {
        let Some(template) = ctx.template.as_ref() else {
            return vec![];
        };

        let off = offset as usize;

        for el in &template.elements {
            for dir in &el.directives {
                if dir.name != "bind" {
                    continue;
                }

                let ds = dir.span.start as usize;
                let de = dir.span.end as usize;

                // Cursor must be within this directive span
                if off < ds || off > de {
                    continue;
                }

                // Must have an argument
                let Some(arg) = dir.argument.as_deref() else {
                    continue;
                };

                // Dynamic arguments — skip
                if arg.starts_with('[') {
                    continue;
                }

                // Skip directives with modifiers (ambiguous expansion)
                if !dir.modifiers.is_empty() {
                    continue;
                }

                // Must be already-shorthand (no expression) to offer expand
                if dir.expression.is_some() {
                    continue;
                }

                let camel_value = kebab_to_camel(arg);
                let insert_text = format!("=\"{}\"", camel_value);

                return vec![CodeAction {
                    title: "Expand v-bind shorthand".to_string(),
                    kind: ActionKind::Refactor,
                    edits: vec![FileEdit {
                        file_id: None,
                        replacement: insert_text,
                        // Insert at end of directive span (after `:foo`)
                        span: verter_span::Span::new(de as u32, de as u32),
                    }],
                    is_preferred: false,
                    diagnostic_rule: None,
                }];
            }
        }

        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ActionContext;
    use verter_analysis::template::*;
    use verter_diagnostics::{DiagnosticSet, DiagnosticSpanKind, LintDiagnostic, Severity};
    use verter_span::Span;

    fn make_diag(source: &str, directive: &str) -> LintDiagnostic {
        let start = source.find(directive).unwrap() as u32;
        let end = start + directive.len() as u32;
        LintDiagnostic {
            rule: "prefer-v-bind-shorthand".to_string(),
            category: "vue-recommended".to_string(),
            severity: Severity::Warning,
            message: "Use same-name shorthand".to_string(),
            span: Span::new(start, end),
            tags: vec![],
            span_kind: DiagnosticSpanKind::Directive,
        }
    }

    fn make_ctx<'a>(
        source: &'a str,
        template: Option<&'a TemplateAnalysisSnapshot>,
    ) -> ActionContext<'a> {
        // Leak a DiagnosticSet for test lifetime convenience (tests only)
        let set = Box::leak(Box::new(DiagnosticSet::new()));
        ActionContext {
            source,
            file_id: "/src/App.vue",
            diagnostics: set,
            template,
            script: None,
            styles: &[],
        }
    }

    fn make_el(directives: Vec<TemplateDirective>) -> TemplateElement {
        TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![],
            directives,
            v_for: None,
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 50,
        }
    }

    // ── fixes_for_diagnostic: convert to shorthand ──

    #[test]
    fn converts_to_shorthand() {
        let source = r#"<div :foo="foo"></div>"#;
        let diag = make_diag(source, r#":foo="foo""#);
        let ctx = make_ctx(source, None);
        let actions = VBindShorthand.fixes_for_diagnostic(&diag, &ctx);

        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert_eq!(actions[0].title, "Use v-bind same-name shorthand");
        assert_eq!(actions[0].edits.len(), 1);
        // The edit should remove ="foo" (from = to end of directive)
        assert_eq!(actions[0].edits[0].replacement, "");
        // After applying: <div :foo></div>
        let start = actions[0].edits[0].span.start as usize;
        let end = actions[0].edits[0].span.end as usize;
        let result = format!("{}{}", &source[..start], &source[end..]);
        assert!(
            result.contains(":foo>"),
            "result should have :foo without value: {}",
            result
        );
        assert!(!result.contains("=\"foo\""), "value must be removed");
    }

    #[test]
    fn converts_kebab_to_shorthand() {
        let source = r#"<div :foo-bar="fooBar"></div>"#;
        let diag = make_diag(source, r#":foo-bar="fooBar""#);
        let ctx = make_ctx(source, None);
        let actions = VBindShorthand.fixes_for_diagnostic(&diag, &ctx);

        assert_eq!(actions.len(), 1);
        let start = actions[0].edits[0].span.start as usize;
        let end = actions[0].edits[0].span.end as usize;
        let result = format!("{}{}", &source[..start], &source[end..]);
        assert!(
            result.contains(":foo-bar>"),
            "should have :foo-bar without value: {}",
            result
        );
        assert!(!result.contains("=\"fooBar\""), "value must be removed");
    }

    #[test]
    fn converts_with_single_quotes() {
        let source = "<div :foo='foo'></div>";
        let diag = make_diag(source, ":foo='foo'");
        let ctx = make_ctx(source, None);
        let actions = VBindShorthand.fixes_for_diagnostic(&diag, &ctx);

        assert_eq!(actions.len(), 1);
        let start = actions[0].edits[0].span.start as usize;
        let end = actions[0].edits[0].span.end as usize;
        let result = format!("{}{}", &source[..start], &source[end..]);
        assert!(
            result.contains(":foo>"),
            "should work with single quotes: {}",
            result
        );
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = r#"<div :foo="foo"></div>"#;
        let diag = LintDiagnostic {
            rule: "v-bind-style".to_string(),
            category: "vue-recommended".to_string(),
            severity: Severity::Warning,
            message: "other".to_string(),
            span: Span::new(5, 15),
            tags: vec![],
            span_kind: DiagnosticSpanKind::Directive,
        };
        let ctx = make_ctx(source, None);
        let actions = VBindShorthand.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "v-bind-style must not trigger v-bind-shorthand provider"
        );
    }

    #[test]
    fn fix_span_is_correct() {
        let source = r#"<div :foo="foo" :bar="x"></div>"#;
        let diag = make_diag(source, r#":foo="foo""#);
        let ctx = make_ctx(source, None);
        let actions = VBindShorthand.fixes_for_diagnostic(&diag, &ctx);

        assert_eq!(actions.len(), 1);
        let edit = &actions[0].edits[0];
        // The edit span should cover exactly ="foo" (including quotes)
        let removed = &source[edit.span.start as usize..edit.span.end as usize];
        assert_eq!(removed, r#"="foo""#, "edit should cover exactly =\"foo\"");
    }

    #[test]
    fn preserves_surrounding_whitespace() {
        let source = r#"<div :foo="foo" :bar="x"></div>"#;
        let diag = make_diag(source, r#":foo="foo""#);
        let ctx = make_ctx(source, None);
        let actions = VBindShorthand.fixes_for_diagnostic(&diag, &ctx);

        let start = actions[0].edits[0].span.start as usize;
        let end = actions[0].edits[0].span.end as usize;
        let result = format!("{}{}", &source[..start], &source[end..]);
        assert!(
            result.contains(":foo :bar="),
            "should preserve space between attributes: {}",
            result
        );
    }

    // ── actions_at: expand shorthand ──

    #[test]
    fn expands_shorthand() {
        let source = r#"<div :foo></div>"#;
        // :foo starts at byte 5
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![TemplateDirective {
                name: "bind".to_string(),
                raw_name: ":foo".to_string(),
                argument: Some("foo".to_string()),
                modifiers: vec![],
                expression: None,
                span: Span::new(5, 9), // :foo
            }])],
            ..Default::default()
        };
        let ctx = make_ctx(source, Some(&template));
        let actions = VBindShorthand.actions_at(7, &ctx); // cursor inside :foo

        assert_eq!(actions.len(), 1, "should offer expand action");
        assert_eq!(actions[0].title, "Expand v-bind shorthand");
        assert_eq!(actions[0].kind, ActionKind::Refactor);
        assert_eq!(actions[0].edits[0].replacement, r#"="foo""#);
        // Insert at end of :foo (byte 9)
        assert_eq!(actions[0].edits[0].span.start, 9);
        assert_eq!(actions[0].edits[0].span.end, 9);
    }

    #[test]
    fn expands_kebab_shorthand() {
        let source = r#"<div :foo-bar></div>"#;
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![TemplateDirective {
                name: "bind".to_string(),
                raw_name: ":foo-bar".to_string(),
                argument: Some("foo-bar".to_string()),
                modifiers: vec![],
                expression: None,
                span: Span::new(5, 13), // :foo-bar
            }])],
            ..Default::default()
        };
        let ctx = make_ctx(source, Some(&template));
        let actions = VBindShorthand.actions_at(8, &ctx);

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].edits[0].replacement, r#"="fooBar""#);
    }

    #[test]
    fn no_expand_when_has_value() {
        let source = r#"<div :foo="bar"></div>"#;
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![TemplateDirective {
                name: "bind".to_string(),
                raw_name: ":foo".to_string(),
                argument: Some("foo".to_string()),
                modifiers: vec![],
                expression: Some("bar".to_string()),
                span: Span::new(5, 15),
            }])],
            ..Default::default()
        };
        let ctx = make_ctx(source, Some(&template));
        let actions = VBindShorthand.actions_at(8, &ctx);

        assert!(
            actions.is_empty(),
            "directive with value must not offer expand"
        );
    }

    #[test]
    fn no_expand_on_non_bind() {
        let source = r#"<div @click="handler"></div>"#;
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![TemplateDirective {
                name: "on".to_string(),
                raw_name: "@click".to_string(),
                argument: Some("click".to_string()),
                modifiers: vec![],
                expression: Some("handler".to_string()),
                span: Span::new(5, 21),
            }])],
            ..Default::default()
        };
        let ctx = make_ctx(source, Some(&template));
        let actions = VBindShorthand.actions_at(8, &ctx);

        assert!(actions.is_empty(), "v-on must not offer v-bind expand");
    }

    #[test]
    fn expand_correct_insert_position() {
        let source = r#"<div :foo class="bar"></div>"#;
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![TemplateDirective {
                name: "bind".to_string(),
                raw_name: ":foo".to_string(),
                argument: Some("foo".to_string()),
                modifiers: vec![],
                expression: None,
                span: Span::new(5, 9),
            }])],
            ..Default::default()
        };
        let ctx = make_ctx(source, Some(&template));
        let actions = VBindShorthand.actions_at(7, &ctx);

        assert_eq!(actions.len(), 1);
        let edit = &actions[0].edits[0];
        // Verify insert at exact end of :foo
        assert_eq!(edit.span.start, 9);
        assert_eq!(edit.span.end, 9);
        // Simulate the insertion
        let result = format!("{}{}{}", &source[..9], edit.replacement, &source[9..]);
        assert!(
            result.contains(r#":foo="foo""#),
            "should insert value after :foo: {}",
            result
        );
    }

    #[test]
    fn expand_preserves_modifiers_skips() {
        // :foo.sync → no action (modifiers present, ambiguous)
        let source = r#"<div :foo.sync></div>"#;
        let template = TemplateAnalysisSnapshot {
            elements: vec![make_el(vec![TemplateDirective {
                name: "bind".to_string(),
                raw_name: ":foo".to_string(),
                argument: Some("foo".to_string()),
                modifiers: vec!["sync".to_string()],
                expression: None,
                span: Span::new(5, 14),
            }])],
            ..Default::default()
        };
        let ctx = make_ctx(source, Some(&template));
        let actions = VBindShorthand.actions_at(8, &ctx);

        assert!(actions.is_empty(), "modifiers present should skip expand");
    }
}

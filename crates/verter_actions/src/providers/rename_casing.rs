//! Quick fix: rename identifiers between PascalCase/camelCase/kebab-case.
//!
//! Handles: `attribute-hyphenation`, `v-on-event-hyphenation`,
//! `component-name-in-template-casing`, `custom-event-name-casing`,
//! `slot-name-casing`, `prop-name-casing`, `component-definition-name-casing`

// @ai-generated

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

pub struct RenameCasing;

impl ActionProvider for RenameCasing {
    fn name(&self) -> &str {
        "rename-casing"
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
            "attribute-hyphenation" | "v-on-event-hyphenation" | "custom-event-name-casing" => {
                // camelCase -> kebab-case
                let kebab = to_kebab_case(text);
                if kebab == text {
                    return vec![];
                }
                (format!("Rename to '{kebab}'"), kebab)
            }
            "prop-name-casing" => {
                // PascalCase/kebab -> camelCase
                let camel = to_camel_case(text);
                if camel == text {
                    return vec![];
                }
                (format!("Rename to '{camel}'"), camel)
            }
            "component-name-in-template-casing" | "component-definition-name-casing" => {
                // kebab-case -> PascalCase (most common direction)
                let pascal = to_pascal_case(text);
                if pascal == text {
                    return vec![];
                }
                (format!("Rename to '{pascal}'"), pascal)
            }
            "slot-name-casing" => {
                // Slot names -> kebab-case
                let kebab = to_kebab_case(text);
                if kebab == text {
                    return vec![];
                }
                (format!("Rename to '{kebab}'"), kebab)
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

/// Convert camelCase or PascalCase to kebab-case.
fn to_kebab_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('-');
            }
            result.push(ch.to_lowercase().next().unwrap_or(ch));
        } else {
            result.push(ch);
        }
    }
    result
}

/// Convert kebab-case to camelCase.
fn to_camel_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for ch in s.chars() {
        if ch == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_uppercase().next().unwrap_or(ch));
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Convert kebab-case or camelCase to PascalCase.
fn to_pascal_case(s: &str) -> String {
    let camel = to_camel_case(s);
    let mut chars = camel.chars();
    match chars.next() {
        Some(first) => {
            let upper = first.to_uppercase().collect::<String>();
            format!("{upper}{}", chars.as_str())
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ActionContext;
    use verter_diagnostics::{DiagnosticSet, DiagnosticSpanKind, LintDiagnostic, Severity};

    #[test]
    fn camel_to_kebab() {
        assert_eq!(to_kebab_case("myProp"), "my-prop");
        assert_eq!(to_kebab_case("onClick"), "on-click");
        assert_eq!(to_kebab_case("MyComponent"), "my-component");
    }

    #[test]
    fn kebab_to_camel() {
        assert_eq!(to_camel_case("my-prop"), "myProp");
        assert_eq!(to_camel_case("on-click"), "onClick");
    }

    #[test]
    fn kebab_to_pascal() {
        assert_eq!(to_pascal_case("my-component"), "MyComponent");
        assert_eq!(to_pascal_case("base-button"), "BaseButton");
    }

    #[test]
    fn attribute_hyphenation_fix() {
        let source = r#"<MyComp myProp="val"></MyComp>"#;
        let start = source.find("myProp").unwrap() as u32;
        let end = start + "myProp".len() as u32;
        let diag = LintDiagnostic {
            rule: "attribute-hyphenation".to_string(),
            category: "vue-recommended".to_string(),
            severity: Severity::Warning,
            message: "use kebab-case".to_string(),
            span: verter_span::Span::new(start, end),
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
        let actions = RenameCasing.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].edits[0].replacement, "my-prop");
        assert!(
            !actions[0].edits[0].replacement.contains("myProp"),
            "should not contain camelCase"
        );
    }

    #[test]
    fn component_name_to_pascal() {
        let source = r#"<my-component></my-component>"#;
        let start = source.find("my-component").unwrap() as u32;
        let end = start + "my-component".len() as u32;
        let diag = LintDiagnostic {
            rule: "component-name-in-template-casing".to_string(),
            category: "vue-recommended".to_string(),
            severity: Severity::Warning,
            message: "use PascalCase".to_string(),
            span: verter_span::Span::new(start, end),
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
        let actions = RenameCasing.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].edits[0].replacement, "MyComponent");
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = "some code";
        let diag = LintDiagnostic {
            rule: "other-rule".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "x".to_string(),
            span: verter_span::Span::new(0, 4),
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
        assert!(RenameCasing.fixes_for_diagnostic(&diag, &ctx).is_empty());
    }
}

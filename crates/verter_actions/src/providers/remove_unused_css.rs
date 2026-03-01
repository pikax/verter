//! Quick fix: remove unused CSS selector rule.
//!
//! When a `unused-css-selector` diagnostic is triggered, this provider
//! offers to delete the entire CSS rule (selector + declaration block).

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;

/// Provider that removes unused CSS selector rules.
pub struct RemoveUnusedCss;

impl ActionProvider for RemoveUnusedCss {
    fn name(&self) -> &str {
        "remove-unused-css"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        if diag.rule != "unused-css-selector" {
            return vec![];
        }

        let source = ctx.source;
        let selector_end = diag.span.end as usize;

        if selector_end >= source.len() {
            return vec![];
        }

        // Find the opening brace of the rule block
        let rest = &source[selector_end..];
        let Some(open_rel) = rest.find('{') else {
            return vec![];
        };
        let open_abs = selector_end + open_rel;

        // Find the selector list area: scan backwards from the opening brace
        // to find the start (previous '}' or start of content).
        let selector_area = &source[..open_abs];

        // Check for commas in the selector area around the diagnostic span.
        // A comma before or after the unused selector means it's grouped.
        let has_comma_before = selector_area[..diag.span.start as usize]
            .rfind([',', '}', '{'])
            .map(|pos| source.as_bytes()[pos] == b',')
            .unwrap_or(false);
        let between_end_and_brace = &source[diag.span.end as usize..open_abs];
        let has_comma_after = between_end_and_brace.find(',').is_some();

        let selector_text = &source[diag.span.start as usize..diag.span.end as usize];

        if has_comma_before || has_comma_after {
            // Grouped selector: remove only this selector and its adjacent comma
            let (remove_start, remove_end) = if has_comma_before {
                // Remove the comma + whitespace before this selector + the selector
                // Find the comma before the selector
                let before = &source[..diag.span.start as usize];
                let comma_pos = before.rfind(',').unwrap();
                (comma_pos as u32, diag.span.end)
            } else {
                // Remove the selector + whitespace + comma after it
                let after = &source[diag.span.end as usize..open_abs];
                let comma_rel = after.find(',').unwrap();
                // Include whitespace after the comma
                let after_comma = &source[diag.span.end as usize + comma_rel + 1..open_abs];
                let trim_len = after_comma.len() - after_comma.trim_start().len();
                (
                    diag.span.start,
                    diag.span.end + comma_rel as u32 + 1 + trim_len as u32,
                )
            };

            return vec![CodeAction {
                title: format!("Remove unused selector `{}`", selector_text),
                kind: ActionKind::QuickFix,
                edits: vec![FileEdit {
                    file_id: None,
                    replacement: String::new(),
                    span: verter_span::Span::new(remove_start, remove_end),
                }],
                is_preferred: false,
                diagnostic_rule: Some("unused-css-selector".to_string()),
            }];
        }

        // Solo selector: remove the entire rule (selector + declaration block)
        let after_open = &source[open_abs + 1..];
        let mut depth = 1u32;
        for (i, ch) in after_open.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let rule_end = (open_abs + 1 + i + 1) as u32;

                        // Include leading whitespace/newline before selector
                        let mut rule_start = diag.span.start;
                        let before = &source[..diag.span.start as usize];
                        if let Some(last_nl) = before.rfind('\n') {
                            let between = &before[last_nl + 1..];
                            if between.trim().is_empty() {
                                rule_start = (last_nl + 1) as u32;
                            }
                        }

                        // Include trailing newline
                        let after_rule = &source[rule_end as usize..];
                        let rule_end_with_nl = if after_rule.starts_with('\n') {
                            rule_end + 1
                        } else if after_rule.starts_with("\r\n") {
                            rule_end + 2
                        } else {
                            rule_end
                        };

                        return vec![CodeAction {
                            title: format!(
                                "Remove unused CSS rule for selector `{}`",
                                selector_text
                            ),
                            kind: ActionKind::QuickFix,
                            edits: vec![FileEdit {
                                file_id: None,
                                replacement: String::new(),
                                span: verter_span::Span::new(rule_start, rule_end_with_nl),
                            }],
                            is_preferred: false,
                            diagnostic_rule: Some("unused-css-selector".to_string()),
                        }];
                    }
                }
                _ => {}
            }
        }

        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ActionContext;
    use verter_diagnostics::{DiagnosticSet, DiagnosticSpanKind, LintDiagnostic, Severity};

    #[test]
    fn removes_unused_css_rule() {
        let source = ".used { color: red; }\n.unused { color: blue; }\n";
        let diag = LintDiagnostic {
            rule: "unused-css-selector".to_string(),
            category: "css".to_string(),
            severity: Severity::Hint,
            message: "Unused CSS selector `.unused`".to_string(),
            span: verter_span::Span::new(
                source.find(".unused").unwrap() as u32,
                (source.find(".unused").unwrap() + ".unused".len()) as u32,
            ),
            tags: vec![],
            span_kind: DiagnosticSpanKind::CssSelector,
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

        let provider = RemoveUnusedCss;
        let actions = provider.fixes_for_diagnostic(&diag, &ctx);

        assert_eq!(actions.len(), 1, "should produce 1 action");
        assert!(actions[0].title.contains(".unused"));
        assert_eq!(actions[0].kind, ActionKind::QuickFix);
        assert_eq!(actions[0].edits.len(), 1);
        assert!(
            actions[0].edits[0].replacement.is_empty(),
            "replacement should be empty (deletion)"
        );
        assert_eq!(
            actions[0].diagnostic_rule.as_deref(),
            Some("unused-css-selector")
        );
    }

    #[test]
    fn grouped_selector_removes_only_unused() {
        let source = ".used, .unused { color: blue; }\n";
        let unused_start = source.find(".unused").unwrap();
        let diag = LintDiagnostic {
            rule: "unused-css-selector".to_string(),
            category: "css".to_string(),
            severity: Severity::Hint,
            message: "Unused CSS selector `.unused`".to_string(),
            span: verter_span::Span::new(
                unused_start as u32,
                (unused_start + ".unused".len()) as u32,
            ),
            tags: vec![],
            span_kind: DiagnosticSpanKind::CssSelector,
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

        let provider = RemoveUnusedCss;
        let actions = provider.fixes_for_diagnostic(&diag, &ctx);

        assert_eq!(actions.len(), 1);
        assert!(actions[0].title.contains(".unused"));
        let edit = &actions[0].edits[0];
        // Should only remove ", .unused" — not the entire rule
        let removed = &source[edit.span.start as usize..edit.span.end as usize];
        assert!(
            removed.contains(".unused"),
            "removed text should contain .unused, got: {:?}",
            removed
        );
        assert!(
            !removed.contains(".used"),
            "removed text should NOT contain .used, got: {:?}",
            removed
        );
        // Applying the edit should leave ".used { color: blue; }\n"
        let mut result = String::new();
        result.push_str(&source[..edit.span.start as usize]);
        result.push_str(&edit.replacement);
        result.push_str(&source[edit.span.end as usize..]);
        assert!(
            result.contains(".used"),
            "result should still contain .used: {:?}",
            result
        );
        assert!(
            result.contains("color: blue"),
            "result should still contain declaration: {:?}",
            result
        );
    }

    #[test]
    fn grouped_selector_first_is_unused() {
        let source = ".unused, .used { color: red; }\n";
        let unused_start = source.find(".unused").unwrap();
        let diag = LintDiagnostic {
            rule: "unused-css-selector".to_string(),
            category: "css".to_string(),
            severity: Severity::Hint,
            message: "Unused".to_string(),
            span: verter_span::Span::new(
                unused_start as u32,
                (unused_start + ".unused".len()) as u32,
            ),
            tags: vec![],
            span_kind: DiagnosticSpanKind::CssSelector,
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

        let provider = RemoveUnusedCss;
        let actions = provider.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);
        let edit = &actions[0].edits[0];

        // Applying the edit should leave ".used { color: red; }\n"
        let mut result = String::new();
        result.push_str(&source[..edit.span.start as usize]);
        result.push_str(&edit.replacement);
        result.push_str(&source[edit.span.end as usize..]);
        assert!(result.contains(".used"), "should keep .used: {:?}", result);
        assert!(
            !result.contains(".unused"),
            "should remove .unused: {:?}",
            result
        );
    }

    #[test]
    fn ignores_unrelated_diagnostics() {
        let source = ".foo { color: red; }";
        let diag = LintDiagnostic {
            rule: "other-rule".to_string(),
            category: "test".to_string(),
            severity: Severity::Warning,
            message: "other".to_string(),
            span: verter_span::Span::new(0, 4),
            tags: vec![],
            span_kind: DiagnosticSpanKind::CssSelector,
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

        let provider = RemoveUnusedCss;
        let actions = provider.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "should not produce actions for unrelated rules"
        );
    }
}

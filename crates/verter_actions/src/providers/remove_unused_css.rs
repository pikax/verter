//! Quick fix: remove unused CSS selector rule.
//!
//! When a `unused-css-selector` diagnostic is triggered, this provider
//! offers to delete the entire CSS rule (selector + declaration block).

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, AutofixSafety, CodeAction, FileEdit};
use verter_diagnostics::LintDiagnostic;
use verter_semantic::analysis::AnalyzedSelector;

/// Provider that removes unused CSS selector rules.
pub struct RemoveUnusedCss;

/// Find the `AnalyzedSelector` the diagnostic's span identifies, and the
/// full sibling list of the style block that owns it. The diagnostic's span
/// is produced from the exact same `CssAnalysis.selectors` entry it
/// identifies, so exact span equality is the join key — never a
/// re-derivation of selector/rule extent from raw bytes.
fn find_selector<'a>(
    ctx: &'a ActionContext,
    diag_span: verter_span::Span,
) -> Option<(&'a [AnalyzedSelector], usize)> {
    ctx.styles.iter().find_map(|block| {
        let css = block.css.as_ref()?;
        let index = css
            .selectors
            .iter()
            .position(|selector| selector.span == diag_span)?;
        Some((css.selectors.as_slice(), index))
    })
}

impl ActionProvider for RemoveUnusedCss {
    fn name(&self) -> &str {
        "remove-unused-css"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        if diag.rule != "unused-css-selector" {
            return vec![];
        }

        let source = ctx.source;
        if diag.span.end as usize > source.len() {
            return vec![];
        }

        let Some((selectors, index)) = find_selector(ctx, diag.span) else {
            return vec![];
        };
        let selector = &selectors[index];
        let Some(rule_body_span) = selector.rule_body_span else {
            return vec![];
        };
        let selector_text = &source[selector.span.start as usize..selector.span.end as usize];

        // Comma-grouped siblings: consecutive `selectors` entries sharing
        // the SAME enclosing rule's declaration-block span.
        let group_start = {
            let mut i = index;
            while i > 0 && selectors[i - 1].rule_body_span == Some(rule_body_span) {
                i -= 1;
            }
            i
        };
        let group_end = {
            let mut i = index;
            while i + 1 < selectors.len() && selectors[i + 1].rule_body_span == Some(rule_body_span)
            {
                i += 1;
            }
            i
        };

        if group_start != group_end {
            // Grouped selector: remove only this selector and its adjacent comma.
            let (remove_start, remove_end) = if index > group_start {
                // Not first in the group: remove from the end of the
                // previous sibling (the comma sits immediately after it)
                // through the end of this selector.
                (selectors[index - 1].span.end, selector.span.end)
            } else {
                // First in the group: remove from the start of this
                // selector through the start of the next sibling (which
                // covers the selector, its trailing comma, and the
                // whitespace before the next selector's real text).
                (selector.span.start, selectors[index + 1].span.start)
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
                safety: AutofixSafety::Safe,
            }];
        }

        // Solo selector: remove the entire rule (selector + declaration
        // block). `rule_body_span` covers the declaration block including
        // both braces, from the real structural parse — immune to a quoted
        // `}` inside a declaration value being mistaken for the block's own
        // closing brace.
        let rule_end = rule_body_span.end;

        // Include leading whitespace/newline before the selector.
        let mut rule_start = selector.span.start;
        let before = &source[..rule_start as usize];
        if let Some(last_nl) = before.rfind('\n') {
            let between = &before[last_nl + 1..];
            if between.trim().is_empty() {
                rule_start = (last_nl + 1) as u32;
            }
        }

        // Include trailing newline.
        let after_rule = &source[rule_end as usize..];
        let rule_end_with_nl = if after_rule.starts_with('\n') {
            rule_end + 1
        } else if after_rule.starts_with("\r\n") {
            rule_end + 2
        } else {
            rule_end
        };

        vec![CodeAction {
            title: format!("Remove unused CSS rule for selector `{}`", selector_text),
            kind: ActionKind::QuickFix,
            edits: vec![FileEdit {
                file_id: None,
                replacement: String::new(),
                span: verter_span::Span::new(rule_start, rule_end_with_nl),
            }],
            is_preferred: false,
            diagnostic_rule: Some("unused-css-selector".to_string()),
            safety: AutofixSafety::Safe,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ActionContext;
    use verter_diagnostics::{DiagnosticSet, DiagnosticSpanKind, LintDiagnostic, Severity};
    use verter_semantic::analysis::{build_css_style_analysis, StyleBlockAnalysis, VueStyleInput};

    /// Build a real `StyleBlockAnalysis` (selectors, specificity,
    /// `rule_body_span`, ...) from raw CSS text through the shared syntax
    /// authority — the same production path that populates `ActionContext.styles`
    /// in the real LSP flow, so these tests exercise the structural join
    /// `RemoveUnusedCss` performs, not a hand-faked span.
    fn analyze(source: &str) -> StyleBlockAnalysis {
        build_css_style_analysis(source, VueStyleInput::default(), false, false, None, 0)
    }

    #[test]
    fn removes_unused_css_rule() {
        let source = ".used { color: red; }\n.unused { color: blue; }\n";
        let style_block = analyze(source);
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
            styles: &[style_block],
            blocks: &[],
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
        let style_block = analyze(source);
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
            styles: &[style_block],
            blocks: &[],
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
        let style_block = analyze(source);
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
            styles: &[style_block],
            blocks: &[],
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

    // A17-style discriminating positive (A21, round-8 finding 6): a solo
    // (ungrouped) unused selector whose rule body contains a quoted `}` in
    // a declaration value. The old brace-depth counter walked raw
    // characters with no string awareness, so it mistook the quoted `}`
    // for the rule's real closing brace and left `"; color: red; }` behind
    // as broken garbage. `rule_body_span` comes from the real structural
    // parse, which tokenizes strings as opaque atomic tokens, and is
    // immune to this.
    #[test]
    fn solo_selector_with_quoted_brace_in_value_deletes_whole_rule() {
        let source = ".foo { content: \"}\"; color: red; }\n";
        let style_block = analyze(source);
        let foo_start = source.find(".foo").unwrap();
        let diag = LintDiagnostic {
            rule: "unused-css-selector".to_string(),
            category: "css".to_string(),
            severity: Severity::Hint,
            message: "Unused CSS selector `.foo`".to_string(),
            span: verter_span::Span::new(foo_start as u32, (foo_start + ".foo".len()) as u32),
            tags: vec![],
            span_kind: DiagnosticSpanKind::CssSelector,
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
            styles: &[style_block],
            blocks: &[],
        };

        let provider = RemoveUnusedCss;
        let actions = provider.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1, "should produce 1 action");
        let edit = &actions[0].edits[0];

        let mut result = String::new();
        result.push_str(&source[..edit.span.start as usize]);
        result.push_str(&edit.replacement);
        result.push_str(&source[edit.span.end as usize..]);
        assert_eq!(
            result, "",
            "should delete the entire rule, leaving no broken garbage behind: {:?}",
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
            blocks: &[],
        };

        let provider = RemoveUnusedCss;
        let actions = provider.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "should not produce actions for unrelated rules"
        );
    }
}

//! Code action: migrate `useAttrs<T>()` → `<script setup attrs="T">`.
//!
//! When the `prefer-script-attrs` lint rule fires, this provider offers a
//! quick fix that:
//! 1. Removes the type parameter from `useAttrs<T>()` → `useAttrs()`
//! 2. Adds `attrs="T"` to the `<script setup>` tag

use verter_diagnostics::LintDiagnostic;
use verter_span::Span;

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, AutofixSafety, CodeAction, FileEdit};

pub struct PreferScriptAttrs;

impl ActionProvider for PreferScriptAttrs {
    fn name(&self) -> &str {
        "prefer-script-attrs"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        if diag.rule != "prefer-script-attrs" {
            return vec![];
        }

        let source = ctx.source;
        let start = diag.span.start as usize;
        let end = diag.span.end as usize;
        if end > source.len() || start >= end {
            return vec![];
        }

        let call_text = &source[start..end];

        // Extract type parameter: find `<` after `useAttrs` and matching `>`
        let Some(type_param) = extract_type_parameter(call_text) else {
            return vec![];
        };

        // The parser-owned attribute-insertion anchor of the `<script setup>`
        // block (fail closed without inventory facts).
        let Some(insert_pos) = script_setup_insert_pos(ctx.blocks) else {
            return vec![];
        };

        // Build edits:
        // 1. Remove `<T>` from the call expression (replace `useAttrs<T>` with `useAttrs`)
        let type_param_start = start + type_param.lt_offset;
        let type_param_end = start + type_param.gt_offset + 1; // include `>`

        // 2. Insert ` attrs="T"` at the script tag insert position
        let attrs_value = &call_text[type_param.type_start..type_param.type_end];

        vec![CodeAction {
            title: "Move type to <script setup attrs=\"...\">".to_string(),
            kind: ActionKind::QuickFix,
            edits: vec![
                // Remove <T> from useAttrs<T>()
                FileEdit {
                    file_id: None,
                    replacement: String::new(),
                    span: Span::new(type_param_start as u32, type_param_end as u32),
                },
                // Insert attrs="T" in script tag
                FileEdit {
                    file_id: None,
                    replacement: format!(" attrs=\"{}\"", attrs_value),
                    span: Span::new(insert_pos, insert_pos),
                },
            ],
            is_preferred: true,
            diagnostic_rule: Some("prefer-script-attrs".to_string()),
            safety: AutofixSafety::Caution,
        }]
    }
}

struct TypeParamOffsets {
    /// Offset of `<` relative to call_text start
    lt_offset: usize,
    /// Offset of `>` relative to call_text start
    gt_offset: usize,
    /// Start of type text (after `<`, trimmed) relative to call_text
    type_start: usize,
    /// End of type text (before `>`, trimmed) relative to call_text
    type_end: usize,
}

/// Extract type parameter offsets from a call like `useAttrs<{ class?: string }>()`.
fn extract_type_parameter(call_text: &str) -> Option<TypeParamOffsets> {
    let needle = "useAttrs";
    let name_pos = call_text.find(needle)?;
    let after_name = name_pos + needle.len();

    // Find `<` after `useAttrs`
    let rest = &call_text[after_name..];
    let lt_rel = rest.find('<')?;
    let lt_offset = after_name + lt_rel;

    // Find matching `>` — handle nested angle brackets
    let after_lt = lt_offset + 1;
    let mut depth = 1u32;
    let mut gt_offset = None;
    for (i, b) in call_text[after_lt..].bytes().enumerate() {
        match b {
            b'<' => depth += 1,
            b'>' => {
                depth -= 1;
                if depth == 0 {
                    gt_offset = Some(after_lt + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let gt_offset = gt_offset?;

    // Extract type text (trim whitespace between < and >)
    let type_text = &call_text[after_lt..gt_offset];
    let trimmed = type_text.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Find the actual start/end of trimmed text within call_text
    let trim_start = after_lt + type_text.find(trimmed.as_bytes()[0] as char)?;
    let trim_end = trim_start + trimmed.len();

    Some(TypeParamOffsets {
        lt_offset,
        gt_offset,
        type_start: trim_start,
        type_end: trim_end,
    })
}

/// The parser-owned position to insert ` attrs="..."` in the `<script setup>`
/// opening tag: the selected script block's attribute-insertion anchor (just
/// before `>`). Raw source is never scanned for a `<script` delimiter, so a
/// decoy literal in a comment or string can never displace the anchor.
/// `None` when there is no `setup` script block or it already carries an
/// `attrs`/`attributes` attribute.
fn script_setup_insert_pos(blocks: &[verter_diagnostics::SfcBlockFact]) -> Option<u32> {
    let block = blocks.iter().find(|block| {
        block.role == verter_diagnostics::SfcBlockRole::Script
            && block
                .attributes
                .iter()
                .any(|attribute| attribute.name == "setup")
    })?;
    if block
        .attributes
        .iter()
        .any(|attribute| matches!(attribute.name.as_str(), "attrs" | "attributes"))
    {
        return None; // already has attrs
    }
    Some(block.attribute_insertion_anchor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ActionContext;
    use verter_diagnostics::{DiagnosticSet, DiagnosticSpanKind, LintDiagnostic, Severity};

    fn make_diag(rule: &str, start: u32, end: u32) -> LintDiagnostic {
        LintDiagnostic {
            rule: rule.to_string(),
            category: "script".to_string(),
            message: "test".to_string(),
            span: Span::new(start, end),
            severity: Severity::Warning,
            span_kind: DiagnosticSpanKind::ScriptCallSite,
            tags: vec![],
            certainty: verter_diagnostics::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        }
    }

    /// Test-fixture script facts: geometry a registered inventory would
    /// supply for these well-formed fixtures (offsets computed from the
    /// fixture text, as the expected-offset assertions already do).
    fn script_setup_facts(source: &str) -> Vec<verter_diagnostics::SfcBlockFact> {
        let Some(open) = source.find("<script") else {
            return vec![];
        };
        let Some(gt) = source[open..].find('>') else {
            return vec![];
        };
        let open_end = open + gt + 1;
        let close = source.rfind("</script").unwrap_or(source.len());
        let tag_text = &source[open..open_end - 1];
        let mut attributes = Vec::new();
        for name in ["setup", "lang", "attrs", "attributes"] {
            if let Some(at) = tag_text.find(name) {
                attributes.push(verter_diagnostics::SfcBlockAttribute {
                    name: name.to_string(),
                    value: None,
                    name_span: Span::new((open + at) as u32, (open + at + name.len()) as u32),
                });
            }
        }
        vec![verter_diagnostics::SfcBlockFact {
            role: verter_diagnostics::SfcBlockRole::Script,
            opening_span: Span::new(open as u32, open_end as u32),
            content_span: Span::new(open_end as u32, close as u32),
            attribute_insertion_anchor: (open_end - 1) as u32,
            attributes,
        }]
    }

    #[test]
    fn migrates_use_attrs_type_to_script_tag() {
        let source = r#"<script setup lang="ts">
import { useAttrs } from 'vue'
const attrs = useAttrs<{ class?: string }>()
</script>
<template><div /></template>"#;

        let call_start = source.find("useAttrs<").unwrap() as u32;
        let call_end = (source.find(">()\n").unwrap() + 3) as u32;

        let diag = make_diag("prefer-script-attrs", call_start, call_end);
        let set = DiagnosticSet::new();
        let blocks = script_setup_facts(source);
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
            blocks: &blocks,
        };
        let actions = PreferScriptAttrs.fixes_for_diagnostic(&diag, &ctx);

        // Positive: produces an action
        assert_eq!(actions.len(), 1, "should produce one action");
        assert_eq!(actions[0].edits.len(), 2, "should have 2 edits");

        // Edit 1: remove <T> from useAttrs<T>()
        let remove_edit = &actions[0].edits[0];
        assert!(
            remove_edit.replacement.is_empty(),
            "first edit should remove type param"
        );
        let removed = &source[remove_edit.span.start as usize..remove_edit.span.end as usize];
        assert!(
            removed.starts_with('<') && removed.ends_with('>'),
            "removed text should be <...>: got '{}'",
            removed
        );

        // Edit 2: insert attrs="T" in script tag
        let insert_edit = &actions[0].edits[1];
        assert!(
            insert_edit.replacement.contains("attrs="),
            "second edit should insert attrs attribute: got '{}'",
            insert_edit.replacement
        );
        assert!(
            insert_edit.replacement.contains("{ class?: string }"),
            "inserted attrs should contain the type: got '{}'",
            insert_edit.replacement
        );

        // Negative: no unexpected edits
        assert!(
            !insert_edit.replacement.contains("useAttrs"),
            "inserted attrs should not contain 'useAttrs'"
        );
    }

    #[test]
    fn ignores_unrelated_rule() {
        let source = "<script setup lang=\"ts\">\nconst x = 1\n</script>";
        let diag = make_diag("no-v-html", 24, 35);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
            blocks: &[],
        };
        let actions = PreferScriptAttrs.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "should not produce actions for unrelated rule"
        );
    }

    #[test]
    fn no_action_when_attrs_already_present() {
        let source = r#"<script setup lang="ts" attrs="{ role?: string }">
import { useAttrs } from 'vue'
const attrs = useAttrs<{ class?: string }>()
</script>"#;

        let call_start = source.find("useAttrs<").unwrap() as u32;
        let call_end = (source.find(">()\n").unwrap() + 3) as u32;

        let diag = make_diag("prefer-script-attrs", call_start, call_end);
        let set = DiagnosticSet::new();
        let blocks = script_setup_facts(source);
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
            blocks: &blocks,
        };
        let actions = PreferScriptAttrs.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "should not produce action when script tag already has attrs"
        );
    }

    #[test]
    fn extract_type_parameter_simple() {
        let result = extract_type_parameter("useAttrs<string>()");
        assert!(result.is_some());
        let tp = result.unwrap();
        assert_eq!(&"useAttrs<string>()"[tp.type_start..tp.type_end], "string");
    }

    #[test]
    fn extract_type_parameter_nested() {
        let result = extract_type_parameter("useAttrs<{ items: Array<string> }>()");
        assert!(result.is_some());
        let tp = result.unwrap();
        assert_eq!(
            &"useAttrs<{ items: Array<string> }>()"[tp.type_start..tp.type_end],
            "{ items: Array<string> }"
        );
    }

    #[test]
    fn extract_type_parameter_none_without_angle() {
        let result = extract_type_parameter("useAttrs()");
        assert!(result.is_none());
    }

    #[test]
    fn insert_pos_ignores_decoy_script_setup_in_root_comment() {
        // A `<script setup x>` literal inside a ROOT COMMENT is not a block;
        // the insertion anchor is the REAL script block's parser-owned
        // attribute-insertion anchor, never a raw `<script` byte scan (which
        // anchored on the comment decoy).
        let source =
            "<!-- <script setup x> -->\n<script setup lang=\"ts\">\nconst x = 1\n</script>";
        let real_open = source.rfind("<script setup lang").unwrap();
        let real_gt = source.rfind("lang=\"ts\">").unwrap() + "lang=\"ts\"".len();
        let facts = vec![verter_diagnostics::SfcBlockFact {
            role: verter_diagnostics::SfcBlockRole::Script,
            opening_span: Span::new(real_open as u32, (real_gt + 1) as u32),
            content_span: Span::new(
                (real_gt + 1) as u32,
                source.rfind("</script>").unwrap() as u32,
            ),
            attribute_insertion_anchor: real_gt as u32,
            attributes: vec![
                verter_diagnostics::SfcBlockAttribute {
                    name: "setup".to_string(),
                    value: None,
                    name_span: Span::new(0, 0),
                },
                verter_diagnostics::SfcBlockAttribute {
                    name: "lang".to_string(),
                    value: Some("ts".to_string()),
                    name_span: Span::new(0, 0),
                },
            ],
        }];
        let pos = script_setup_insert_pos(&facts);
        assert_eq!(
            pos,
            Some(real_gt as u32),
            "anchor must be the real script opening tag, not the comment decoy"
        );
    }

    #[test]
    fn insert_pos_from_facts_lands_before_the_gt() {
        let source = "<script setup lang=\"ts\">\nconst x = 1\n</script>";
        let facts = script_setup_facts(source);
        let pos = script_setup_insert_pos(&facts).expect("anchor");
        assert_eq!(source.as_bytes()[pos as usize], b'>');
    }

    #[test]
    fn insert_pos_none_when_attrs_already_present() {
        let source = "<script setup attrs=\"{ x: 1 }\">\nconst x = 1\n</script>";
        let facts = script_setup_facts(source);
        let pos = script_setup_insert_pos(&facts);
        assert!(
            pos.is_none(),
            "should return None when attrs already present"
        );
    }

    #[test]
    fn insert_pos_none_without_facts() {
        // Without inventory facts the provider fails closed — no raw-source
        // `<script` search recovers an anchor.
        assert!(script_setup_insert_pos(&[]).is_none());
    }
}

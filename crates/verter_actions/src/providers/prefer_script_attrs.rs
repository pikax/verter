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

        // Find the `<script setup` tag in the source to insert `attrs="T"` before `>`
        let Some(insert_pos) = find_script_setup_insert_pos(source) else {
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

/// Find the byte position to insert ` attrs="..."` in the `<script setup ...>` tag.
///
/// Returns the position just before `>` in the opening tag.
fn find_script_setup_insert_pos(source: &str) -> Option<u32> {
    let bytes = source.as_bytes();
    // Find `<script` tag that has `setup` attribute
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let tag_start = i;
            i += 1;
            // Skip whitespace
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            // Check for `script`
            if i + 6 <= bytes.len() && bytes[i..i + 6].eq_ignore_ascii_case(b"script") {
                let after_script = i + 6;
                // Check it's followed by whitespace or `>` (not `style` or other prefix)
                if after_script < bytes.len()
                    && (bytes[after_script].is_ascii_whitespace() || bytes[after_script] == b'>')
                {
                    // Find the `>` that closes this tag
                    let mut j = after_script;
                    let mut has_setup = false;
                    while j < bytes.len() && bytes[j] != b'>' {
                        // Check for `setup` attribute
                        if j + 5 <= bytes.len() && &bytes[j..j + 5] == b"setup" {
                            let after = j + 5;
                            if after >= bytes.len()
                                || bytes[after].is_ascii_whitespace()
                                || bytes[after] == b'>'
                                || bytes[after] == b'='
                            {
                                has_setup = true;
                            }
                        }
                        j += 1;
                    }
                    if has_setup && j < bytes.len() {
                        // j points to `>` — check we don't already have `attrs`
                        let tag_text = &source[tag_start..j];
                        if tag_text.contains("attrs=") || tag_text.contains("attributes=") {
                            return None; // already has attrs
                        }
                        return Some(j as u32); // insert position just before `>`
                    }
                }
            }
        }
        i += 1;
    }
    None
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
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
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
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
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
    fn find_insert_pos_basic() {
        let source = r#"<script setup lang="ts">
const x = 1
</script>"#;
        let pos = find_script_setup_insert_pos(source);
        assert!(pos.is_some());
        assert_eq!(source.as_bytes()[pos.unwrap() as usize], b'>');
    }

    #[test]
    fn find_insert_pos_already_has_attrs() {
        let source = r#"<script setup attrs="{ x: 1 }">
const x = 1
</script>"#;
        let pos = find_script_setup_insert_pos(source);
        assert!(
            pos.is_none(),
            "should return None when attrs already present"
        );
    }
}

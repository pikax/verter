//! Convert `verter_actions::CodeAction` to its FFI representation. Span byte
//! offsets are converted to UTF-16 for browser consumption.

use crate::types::*;

use super::offset::byte_offset_to_utf16;

pub fn code_action_to_ffi(action: &verter_actions::CodeAction, source: &str) -> FfiCodeAction {
    FfiCodeAction {
        title: action.title.clone(),
        kind: match action.kind {
            verter_actions::ActionKind::QuickFix => "quickfix".to_string(),
            verter_actions::ActionKind::Refactor => "refactor".to_string(),
            verter_actions::ActionKind::Source => "source".to_string(),
        },
        edits: action
            .edits
            .iter()
            .map(|edit| FfiTextEdit {
                span_start: byte_offset_to_utf16(source, edit.span.start),
                span_end: byte_offset_to_utf16(source, edit.span.end),
                new_text: edit.replacement.clone(),
            })
            .collect(),
        is_preferred: action.is_preferred,
        diagnostic_rule: action.diagnostic_rule.clone(),
    }
}

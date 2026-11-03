//! Convert `verter_actions::CodeAction` to its FFI representation. Span byte
//! offsets are converted to UTF-16 for browser consumption through one
//! [`OffsetIndex`] per source, shared by every action of a request.

use crate::types::*;

use verter_span::CoordError;

use super::offset::{OffsetEncoding, OffsetIndex};

///
/// An edit whose range cannot be converted exactly is refused for the whole
/// action — never clamped — so no partial edit set is ever returned.
pub fn code_action_to_ffi(
    action: &verter_actions::CodeAction,
    index: &OffsetIndex<'_>,
) -> Result<FfiCodeAction, CoordError> {
    Ok(FfiCodeAction {
        title: action.title.clone(),
        kind: match action.kind {
            verter_actions::ActionKind::QuickFix => "quickfix".to_string(),
            verter_actions::ActionKind::Refactor => "refactor".to_string(),
            verter_actions::ActionKind::Source => "source".to_string(),
        },
        edits: action
            .edits
            .iter()
            .map(|edit| {
                let (span_start, span_end) = index.convert_range(
                    super::offset::source_range(edit.span)?,
                    OffsetEncoding::Utf16,
                )?;
                Ok(FfiTextEdit {
                    span_start,
                    span_end,
                    span_encoding: SpanEncodingTag::Utf16,
                    new_text: edit.replacement.clone(),
                })
            })
            .collect::<Result<Vec<_>, CoordError>>()?,
        is_preferred: action.is_preferred,
        diagnostic_rule: action.diagnostic_rule.clone(),
    })
}

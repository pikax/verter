//! Lint diagnostics tagged wire-span projection and lint-rule metadata
//! projection for the FFI boundary.

use crate::types::*;

use super::wire_span::{DiagnosticWireSpan, SpanConverter};
use verter_protocol::types::SpanEncodingTag;

/// A lint diagnostic paired with its separately tagged wire span.
///
/// [`verter_diagnostics::LintDiagnostic::span`] stays UTF-8 bytes and is never
/// rewritten in place; the wire span is a distinct value in the encoding it
/// names.
pub struct WireLintDiagnostic {
    pub diagnostic: verter_diagnostics::LintDiagnostic,
    pub span: DiagnosticWireSpan,
}

// The wire form keeps the flat field names of the native serialization
// (`serde-wasm-bindgen` turns a map into a JS `Map`, so this must be a struct)
// and replaces only the span: it is the separately tagged wire span, never the
// native byte span rewritten in place.
impl serde::Serialize for WireLintDiagnostic {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let d = &self.diagnostic;
        let has_tags = !d.tags.is_empty();
        let has_evidence = !d.evidence.is_empty();
        let has_related = !d.related_files.is_empty();
        let len = 7
            + usize::from(self.span.start.is_some()) * 2
            + usize::from(self.span.error.is_some())
            + usize::from(has_tags)
            + usize::from(has_evidence)
            + usize::from(has_related);
        let mut state = serializer.serialize_struct("LintDiagnostic", len)?;
        state.serialize_field("rule", &d.rule)?;
        state.serialize_field("category", &d.category)?;
        state.serialize_field("severity", &d.severity)?;
        state.serialize_field("message", &d.message)?;
        if let (Some(start), Some(end)) = (self.span.start, self.span.end) {
            state.serialize_field("spanStart", &start)?;
            state.serialize_field("spanEnd", &end)?;
        }
        state.serialize_field("spanEncoding", &self.span.encoding)?;
        if let Some(error) = self.span.error {
            state.serialize_field("spanError", error)?;
        }
        if has_tags {
            state.serialize_field("tags", &d.tags)?;
        }
        state.serialize_field("spanKind", &d.span_kind)?;
        state.serialize_field("certainty", &d.certainty)?;
        if has_evidence {
            state.serialize_field("evidence", &d.evidence)?;
        }
        if has_related {
            state.serialize_field("relatedFiles", &d.related_files)?;
        }
        state.end()
    }
}

/// Tag each lint diagnostic with a UTF-16 wire span converted against
/// `source` (one shared offset index). A diagnostic whose span cannot be
/// converted exactly (no source snapshot, off-boundary, past-end, inverted)
/// is still reported, with the typed refusal and no span.
pub fn lint_diagnostics_to_wire_utf16(
    diagnostics: Vec<verter_diagnostics::LintDiagnostic>,
    source: Option<&str>,
) -> Vec<WireLintDiagnostic> {
    let spans = SpanConverter::new(source);
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            let span = spans.diagnostic_span(diagnostic.span.start, diagnostic.span.end);
            WireLintDiagnostic { diagnostic, span }
        })
        .collect()
}

/// Tag each lint diagnostic with its UTF-8 byte span (MCP wire): the native
/// span passes through under the `utf8.v1` tag after a validity check
/// against `source` when one is available.
pub fn lint_diagnostics_to_wire_utf8(
    diagnostics: Vec<verter_diagnostics::LintDiagnostic>,
) -> Vec<WireLintDiagnostic> {
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            let span = if diagnostic.span.start <= diagnostic.span.end {
                DiagnosticWireSpan {
                    start: Some(diagnostic.span.start),
                    end: Some(diagnostic.span.end),
                    encoding: SpanEncodingTag::Utf8,
                    error: None,
                }
            } else {
                DiagnosticWireSpan {
                    start: None,
                    end: None,
                    encoding: SpanEncodingTag::Utf8,
                    error: Some("InvalidBoundary"),
                }
            };
            WireLintDiagnostic { diagnostic, span }
        })
        .collect()
}

pub fn lint_rule_to_ffi_metadata(rule: &dyn verter_diagnostics::LintRule) -> FfiLintRuleMetadata {
    FfiLintRuleMetadata {
        name: rule.name().to_string(),
        category: rule.category().as_str().to_string(),
        default_severity: match rule.default_severity() {
            Some(verter_diagnostics::Severity::Error) => "error".to_string(),
            Some(verter_diagnostics::Severity::Warning) => "warning".to_string(),
            Some(verter_diagnostics::Severity::Info) => "info".to_string(),
            Some(verter_diagnostics::Severity::Hint) => "hint".to_string(),
            None => "off".to_string(),
        },
    }
}

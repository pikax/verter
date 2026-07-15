//! Lint diagnostics span conversion (UTF-8 → UTF-16) and lint-rule metadata
//! projection for the FFI boundary.

use crate::types::*;

use super::offset::byte_offset_to_utf16;

pub fn lint_diagnostics_to_utf16(
    mut diagnostics: Vec<verter_diagnostics::LintDiagnostic>,
    source: Option<&str>,
) -> Vec<verter_diagnostics::LintDiagnostic> {
    let Some(source) = source else {
        return diagnostics;
    };

    for d in &mut diagnostics {
        let start = byte_offset_to_utf16(source, d.span.start);
        let end = byte_offset_to_utf16(source, d.span.end);
        d.span = verter_span::Span::new(start, end);
    }

    diagnostics
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

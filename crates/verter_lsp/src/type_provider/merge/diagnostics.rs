//! Diagnostics merge: map TypeProvider diagnostics from TSX positions back to
//! carrier-source positions and combine with verter diagnostics.

use tower_lsp_server::ls_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::provider_projection::ProviderPositionMapper;
use crate::type_provider::protocol::{TypeDiagnostic, TypeDiagnosticSeverity};

use super::position::tsx_range_to_carrier_range;

/// Merge verter diagnostics with TypeProvider diagnostics.
///
/// Strategy:
/// - Verter diagnostics are already in Vue positions
/// - TypeProvider diagnostics are in TSX positions; map back to Vue
/// - Filter out diagnostics that map to unmapped regions (generated code)
pub fn merge_diagnostics(
    verter_diags: Vec<Diagnostic>,
    type_diags: Vec<TypeDiagnostic>,
    tsx_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
) -> Vec<Diagnostic> {
    let mut result = verter_diags;
    let mut dropped = 0u32;

    for diag in &type_diags {
        let range = tsx_range_to_carrier_range(
            diag.start,
            diag.end,
            tsx_line_index,
            mapper,
            carrier_line_index,
        );

        if let Some(range) = range {
            result.push(Diagnostic {
                range,
                severity: Some(convert_severity(diag.severity)),
                code: diag.code.clone().map(NumberOrString::String),
                source: Some("ts".to_string()),
                message: diag.message.clone(),
                ..Default::default()
            });
        } else {
            dropped += 1;
            tracing::debug!(
                "merge_diagnostics: dropped type provider diagnostic (unmapped range) — {:?} at offsets {}..{}",
                diag.message,
                diag.start,
                diag.end,
            );
        }
    }

    if dropped > 0 {
        tracing::debug!(
            "merge_diagnostics: {dropped}/{} type provider diagnostics dropped (unmapped ranges)",
            type_diags.len()
        );
    }

    result
}

fn convert_severity(sev: TypeDiagnosticSeverity) -> DiagnosticSeverity {
    match sev {
        TypeDiagnosticSeverity::Error => DiagnosticSeverity::ERROR,
        TypeDiagnosticSeverity::Warning => DiagnosticSeverity::WARNING,
        TypeDiagnosticSeverity::Info => DiagnosticSeverity::INFORMATION,
        TypeDiagnosticSeverity::Hint => DiagnosticSeverity::HINT,
    }
}

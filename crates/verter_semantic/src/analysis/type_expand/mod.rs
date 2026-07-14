//! Type expansion — now fully backed by the shared semantic dispatch layer.
//!
//! The standalone arena solver has been retired. All expansion goes through
//! `ProjectSemanticDispatch::execute`; the helpers here only convert solver
//! result metadata into the public expansion contract.

mod request;

pub use request::{
    ExpandedCallSignature, ExpandedComponentTypes, ExpandedField, ExpandedIndexSignature,
    ExpandedMacroExposed, ExpandedMacroObjectShape, ExpandedMacroProps, ExpandedNormalizedExpr,
    ExpandedObjectShape, ExpandedParameter, ExpandedProperty, ExpansionDiagnostic,
    ExpansionExactness, ExpansionExecutionStatus, ExpansionMetadata, ExpansionResult,
    ExpansionStopReason,
};

use crate::analysis::type_solver::result::{IncompleteReason, SolverDiagnostic, SolverResult};
use verter_type_expr::facts::SemanticTypeSource;

// ---------------------------------------------------------------------------
// Shared solver-result → expansion-result conversion
// ---------------------------------------------------------------------------

/// Convert a single `IncompleteReason` to an `ExpansionDiagnostic`.
fn incomplete_reason_to_expansion_diagnostic(reason: &IncompleteReason) -> ExpansionDiagnostic {
    let stop_reason = match reason {
        IncompleteReason::MissingSource { .. } => ExpansionStopReason::UnresolvedReference,
        IncompleteReason::UnsupportedSyntax { .. } => ExpansionStopReason::UnsupportedOperator,
        IncompleteReason::Cancelled => ExpansionStopReason::BudgetExceeded,
        IncompleteReason::RecursionPolicy { .. } => ExpansionStopReason::BudgetExceeded,
    };
    ExpansionDiagnostic {
        reason: stop_reason,
        context: reason.to_string(),
        property_name: None,
    }
}

/// Convert a single `SolverDiagnostic` to an `ExpansionDiagnostic`.
fn solver_diagnostic_to_expansion_diagnostic(diagnostic: &SolverDiagnostic) -> ExpansionDiagnostic {
    match diagnostic {
        SolverDiagnostic::ConditionalContextTruncated {
            available,
            captured,
        } => ExpansionDiagnostic {
            reason: ExpansionStopReason::ConditionalContextTruncated,
            context: format!(
                "conditional context truncated: {} available, {} captured",
                available, captured
            ),
            property_name: None,
        },
    }
}

/// Collect all expansion diagnostics from a `SolverResult` — both
/// `incomplete_reasons` and `diagnostics`.
pub fn solver_result_to_expansion_diagnostics<T>(
    result: &SolverResult<T>,
) -> Vec<ExpansionDiagnostic> {
    let mut out: Vec<ExpansionDiagnostic> = result
        .incomplete_reasons
        .iter()
        .map(incomplete_reason_to_expansion_diagnostic)
        .collect();
    out.extend(
        result
            .diagnostics
            .iter()
            .map(solver_diagnostic_to_expansion_diagnostic),
    );
    out
}

/// Convert a `SolverResult<SemanticTypeSource>` to an
/// `ExpansionResult<ExpandedNormalizedExpr>`, preserving all metadata including
/// solver diagnostics. The value is the resolved SOURCE carrier — materialized
/// shape production is a host concern, never a lower-crate walk.
pub fn solver_result_to_normalized_expansion(
    result: SolverResult<SemanticTypeSource>,
) -> ExpansionResult<ExpandedNormalizedExpr> {
    let diagnostics = solver_result_to_expansion_diagnostics(&result);
    ExpansionResult {
        value: ExpandedNormalizedExpr { expr: result.value },
        exactness: result.exactness,
        execution_status: result.execution_status,
        diagnostics,
    }
}

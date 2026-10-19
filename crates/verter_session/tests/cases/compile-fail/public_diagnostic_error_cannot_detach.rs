//! Compile-fail: a diagnostic borrowed from its read cannot be copied out,
//! and neither can the query failure it records.

use verter_type_engine::project_semantic_dispatch::walk::{DiagnosticError, ShallowDiagnostic};
use verter_type_engine::semantic_query::QueryError;

fn detach_diagnostic(diagnostic: &ShallowDiagnostic) -> ShallowDiagnostic {
    diagnostic.clone()
}

fn detach_error(error: &DiagnosticError) -> DiagnosticError {
    error.clone()
}

fn unwrap_error(error: &DiagnosticError) -> &QueryError {
    &error.0
}

fn forge_error(error: QueryError) -> DiagnosticError {
    DiagnosticError(error)
}

fn main() {}

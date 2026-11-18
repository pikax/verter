//! One carrier in, one overlay file out. Nothing here can fail a run.

use std::path::Path;
use std::sync::Arc;

use verter_session::sfc_projection_mapper::{
    ContentMapperProjectionAdapter, MapperDiagnostic, MapperDiagnosticCode, MapperProjectHandle,
    MapperTransformResult,
};
use verter_workspace::WorkspaceRead;

use crate::reporter::{Diagnostic, VerterCode};

/// The projection of one carrier for one project.
#[derive(Debug, Clone)]
pub struct CarrierProjection {
    pub carrier: Arc<str>,
    /// The exact workspace source used for this transform and its revisions.
    pub source: Arc<str>,
    pub result: Arc<MapperTransformResult>,
    /// True when the carrier could not be read. `result` then holds the read
    /// error as a diagnostic.
    pub unreadable: bool,
}

impl CarrierProjection {
    /// The registered code a mapper diagnostic of this carrier prints with.
    #[must_use]
    pub fn verter_code(&self, code: MapperDiagnosticCode) -> VerterCode {
        if self.unreadable {
            return VerterCode::CarrierUnreadable;
        }
        match code {
            MapperDiagnosticCode::Syntax => VerterCode::CarrierSyntax,
            MapperDiagnosticCode::ScriptLangConflict => VerterCode::ScriptLangMismatch,
            MapperDiagnosticCode::ProjectionRefused => VerterCode::CarrierProjectionRefused,
            MapperDiagnosticCode::ProjectionInternal => VerterCode::CarrierProjectionInternal,
        }
    }

    /// The carrier's own diagnostics, positioned in the source it was
    /// projected from.
    #[must_use]
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        let line_starts = crate::position::LineTable::line_starts(&self.source);
        let table = crate::position::LineTable::from_parts(&self.source, &line_starts);
        self.result
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let (line, col) = super::exit::engine_position(&table, diagnostic.start);
                Diagnostic::verter(
                    self.carrier.to_string(),
                    line,
                    col,
                    self.verter_code(diagnostic.code),
                    diagnostic.message.clone(),
                )
            })
            .collect()
    }
}

/// Read, parse, compose and lower one carrier. A read error, a parser panic,
/// a refusal and a composer panic each become a diagnostic on the carrier.
pub fn project_carrier(
    adapter: &ContentMapperProjectionAdapter,
    project: &MapperProjectHandle,
    carrier: &Path,
    workspace: &dyn WorkspaceRead,
) -> CarrierProjection {
    let carrier: Arc<str> = Arc::from(verter_span::path::canonicalize_path(
        &carrier.to_string_lossy(),
    ));
    let source = workspace.read_file(&carrier);
    let unreadable = source.is_none();
    let source = source.unwrap_or_else(|| Arc::from(""));
    // The adapter owns syntax recovery and the composition unwind boundary.
    let mut result = adapter
        .transform(project, &carrier, &source)
        .expect("the driver opens the mapper project before projecting its members");
    if unreadable {
        result.text = Arc::from("export {};");
        result.module = None;
        result.plane = None;
        result.mappings.clear();
        result.complete = false;
        result.diagnostics = vec![MapperDiagnostic {
            start: 0,
            end: 0,
            code: MapperDiagnosticCode::ProjectionRefused,
            message: format!("Cannot read carrier '{carrier}'"),
        }];
    }
    CarrierProjection {
        carrier,
        source,
        result: Arc::new(result),
        unreadable,
    }
}

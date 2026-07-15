// Diagnostics — parse errors, macro validation from verter_session.
// Enhanced with type errors, unused variables, strict null checks from TypeProvider.

use tower_lsp_server::ls_types::*;
use verter_session::{DiagnosticsSnapshot, HostDiagnostic, HostSeverity};

use crate::documents::line_index::LineIndex;

pub(crate) const TYPE_EXPANSION_BUDGET_CODE: &str = "verter/type-expansion-budget";
pub(crate) const TYPE_QUERY_DEPTH_LIMIT_CODE: &str = "verter/type-query-depth-limit";

/// Convert a `DiagnosticsSnapshot` from verter_session into LSP `Diagnostic` items.
pub fn map_diagnostics(snapshot: &DiagnosticsSnapshot, line_index: &LineIndex) -> Vec<Diagnostic> {
    snapshot
        .diagnostics
        .iter()
        .map(|d| map_single_diagnostic(d, line_index))
        .collect()
}

/// Map the two operational type-evaluation limits from component-meta's
/// typed expansion diagnostics into the editor diagnostic stream. Diagnostics
/// are deduplicated by `(macro_index, reason)`, which is the public root-demand
/// identity available at this boundary.
pub(crate) fn map_projection_limit_diagnostics(
    macro_spans: &[verter_span::Span],
    expansions: &[verter_semantic::analysis::component_meta::MacroExpansionDiagnostics],
    line_index: &LineIndex,
) -> Vec<Diagnostic> {
    use std::collections::HashSet;
    use verter_semantic::analysis::type_expand::ExpansionStopReason;

    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    enum OperationalLimit {
        ProjectionWork,
        ConnectedQueryDepth,
    }

    let mut seen = HashSet::new();
    let mut diagnostics = Vec::new();
    for envelope in expansions {
        for diagnostic in &envelope.diagnostics {
            let (kind, code, message) = match diagnostic.reason {
                ExpansionStopReason::ProjectionWorkLimit => (
                    OperationalLimit::ProjectionWork,
                    TYPE_EXPANSION_BUDGET_CODE,
                    "Type expansion exceeded Verter's safe evaluation budget.",
                ),
                ExpansionStopReason::ConnectedQueryDepthLimit => (
                    OperationalLimit::ConnectedQueryDepth,
                    TYPE_QUERY_DEPTH_LIMIT_CODE,
                    "Type evaluation exceeded Verter's safe connected-query depth limit.",
                ),
                _ => continue,
            };
            if !seen.insert((envelope.macro_index, kind)) {
                continue;
            }
            diagnostics.push(map_single_diagnostic(
                &HostDiagnostic {
                    severity: HostSeverity::Warning,
                    code: code.to_string(),
                    message: message.to_string(),
                    span: macro_spans.get(envelope.macro_index).copied(),
                },
                line_index,
            ));
        }
    }
    diagnostics
}

fn map_single_diagnostic(diag: &HostDiagnostic, line_index: &LineIndex) -> Diagnostic {
    let range = match diag.span {
        Some(span) => {
            let start_pos = line_index
                .offset_to_position(span.start)
                .unwrap_or(Position {
                    line: 0,
                    character: 0,
                });
            let end_pos = line_index.offset_to_position(span.end).unwrap_or(start_pos);
            Range {
                start: start_pos,
                end: end_pos,
            }
        }
        None => Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 0,
            },
        },
    };

    Diagnostic {
        range,
        severity: Some(map_severity(&diag.severity)),
        code: Some(NumberOrString::String(diag.code.clone())),
        source: Some("verter".to_string()),
        message: diag.message.clone(),
        ..Default::default()
    }
}

fn map_severity(severity: &HostSeverity) -> DiagnosticSeverity {
    match severity {
        HostSeverity::Error => DiagnosticSeverity::ERROR,
        HostSeverity::Warning => DiagnosticSeverity::WARNING,
        HostSeverity::Info => DiagnosticSeverity::INFORMATION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_line_index(source: &str) -> LineIndex {
        LineIndex::new_utf16(source)
    }

    fn make_diag(
        severity: HostSeverity,
        code: &str,
        message: &str,
        start: Option<u32>,
        end: Option<u32>,
    ) -> HostDiagnostic {
        HostDiagnostic {
            severity,
            code: code.to_string(),
            message: message.to_string(),
            span: match (start, end) {
                (Some(s), Some(e)) => Some(verter_span::Span::new(s, e)),
                _ => None,
            },
        }
    }

    fn make_snapshot(diagnostics: Vec<HostDiagnostic>) -> DiagnosticsSnapshot {
        let has_errors = diagnostics
            .iter()
            .any(|d| d.severity == HostSeverity::Error);
        DiagnosticsSnapshot {
            diagnostics,
            has_errors,
        }
    }

    #[test]
    fn test_empty_diagnostics() {
        let idx = make_line_index("hello");
        let snapshot = make_snapshot(vec![]);
        let result = map_diagnostics(&snapshot, &idx);
        assert!(result.is_empty());
    }

    #[test]
    fn test_error_with_span() {
        let source = "<template>\n  <div>\n</template>";
        let idx = make_line_index(source);
        let snapshot = make_snapshot(vec![make_diag(
            HostSeverity::Error,
            "PARSE_ERROR",
            "Unexpected end of template",
            Some(13), // inside <div>
            Some(18),
        )]);

        let result = map_diagnostics(&snapshot, &idx);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(result[0].source, Some("verter".to_string()));
        assert_eq!(result[0].message, "Unexpected end of template");
        assert_eq!(
            result[0].code,
            Some(NumberOrString::String("PARSE_ERROR".to_string()))
        );
        // Span 13..18 should be on line 1
        assert_eq!(result[0].range.start.line, 1);
    }

    #[test]
    fn test_warning_severity() {
        let idx = make_line_index("hello");
        let snapshot = make_snapshot(vec![make_diag(
            HostSeverity::Warning,
            "WARN_001",
            "A warning",
            Some(0),
            Some(5),
        )]);

        let result = map_diagnostics(&snapshot, &idx);
        assert_eq!(result[0].severity, Some(DiagnosticSeverity::WARNING));
    }

    #[test]
    fn test_info_severity() {
        let idx = make_line_index("hello");
        let snapshot = make_snapshot(vec![make_diag(
            HostSeverity::Info,
            "INFO_001",
            "An info",
            Some(0),
            Some(5),
        )]);

        let result = map_diagnostics(&snapshot, &idx);
        assert_eq!(result[0].severity, Some(DiagnosticSeverity::INFORMATION));
    }

    #[test]
    fn test_no_span_defaults_to_zero() {
        let idx = make_line_index("hello");
        let snapshot = make_snapshot(vec![make_diag(
            HostSeverity::Error,
            "ERR",
            "no span",
            None,
            None,
        )]);

        let result = map_diagnostics(&snapshot, &idx);
        assert_eq!(result[0].range.start.line, 0);
        assert_eq!(result[0].range.start.character, 0);
    }

    #[test]
    fn test_multiple_diagnostics() {
        let source = "line 1\nline 2\nline 3";
        let idx = make_line_index(source);
        let snapshot = make_snapshot(vec![
            make_diag(HostSeverity::Error, "E1", "err1", Some(0), Some(6)),
            make_diag(HostSeverity::Warning, "W1", "warn1", Some(7), Some(13)),
        ]);

        let result = map_diagnostics(&snapshot, &idx);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].range.start.line, 0);
        assert_eq!(result[1].range.start.line, 1);
    }

    #[test]
    fn operational_projection_limits_map_to_stable_deduplicated_editor_diagnostics() {
        use verter_semantic::analysis::component_meta::{
            MacroExpansionDiagnostics, MacroExpansionKind,
        };
        use verter_semantic::analysis::type_expand::{
            ExpansionDiagnostic, ExpansionExactness, ExpansionExecutionStatus, ExpansionStopReason,
        };

        let envelope = |reason| MacroExpansionDiagnostics {
            macro_kind: MacroExpansionKind::DefineProps,
            macro_index: 0,
            diagnostics: vec![ExpansionDiagnostic {
                reason,
                context: "typed-test".to_string(),
                property_name: None,
            }],
            exactness: ExpansionExactness::Incomplete,
            execution_status: ExpansionExecutionStatus::Interrupted,
        };
        let expansions = vec![
            envelope(ExpansionStopReason::ProjectionWorkLimit),
            envelope(ExpansionStopReason::ProjectionWorkLimit),
            envelope(ExpansionStopReason::ConnectedQueryDepthLimit),
            envelope(ExpansionStopReason::UnresolvedReference),
        ];
        let source = "head\ndefineProps<Props>()\ntail";
        let line_index = make_line_index(source);
        let diagnostics = map_projection_limit_diagnostics(
            &[verter_span::Span::new(5, 25)],
            &expansions,
            &line_index,
        );

        assert_eq!(diagnostics.len(), 2, "one diagnostic per root/reason");
        assert_eq!(
            diagnostics[0].code,
            Some(NumberOrString::String(
                TYPE_EXPANSION_BUDGET_CODE.to_string()
            ))
        );
        assert_eq!(
            diagnostics[0].message,
            "Type expansion exceeded Verter's safe evaluation budget."
        );
        assert_eq!(
            diagnostics[1].code,
            Some(NumberOrString::String(
                TYPE_QUERY_DEPTH_LIMIT_CODE.to_string()
            ))
        );
        assert_eq!(diagnostics[0].range.start.line, 1);
        assert_eq!(diagnostics[1].range, diagnostics[0].range);
        assert!(diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity == Some(DiagnosticSeverity::WARNING)));
    }
}

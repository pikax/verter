// Phase 2: Diagnostics — parse errors, macro validation from verter_host.
// Phase 3: Enhanced with type errors, unused variables, strict null checks from TypeProvider.

use tower_lsp_server::ls_types::*;
use verter_host::{DiagnosticsSnapshot, HostDiagnostic, HostSeverity};

use crate::documents::line_index::LineIndex;

/// Convert a `DiagnosticsSnapshot` from verter_host into LSP `Diagnostic` items.
pub fn map_diagnostics(snapshot: &DiagnosticsSnapshot, line_index: &LineIndex) -> Vec<Diagnostic> {
    snapshot
        .diagnostics
        .iter()
        .map(|d| map_single_diagnostic(d, line_index))
        .collect()
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
}

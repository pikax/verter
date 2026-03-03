//! Bridge: convert `verter_diagnostics` types into LSP protocol types.

use tower_lsp_server::ls_types::*;
use verter_analysis::types::{AnalysisFlags, ScriptAnalysisSnapshot};
use verter_diagnostics::{
    DiagnosticSet, DiagnosticTag as LintDiagnosticTag, LintDiagnostic, Severity,
};
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;

/// Run the linter on a `FileAnalysisSnapshot` and return LSP diagnostics.
///
/// Bridges the host's analysis format to the linter's input, runs all rules,
/// and converts the resulting `DiagnosticSet` to LSP `Diagnostic` items.
pub fn run_linter(
    linter: &verter_diagnostics::Linter,
    analysis: &FileAnalysisSnapshot,
    source: &str,
    line_index: &LineIndex,
) -> Vec<Diagnostic> {
    let script = script_from_host(analysis);
    let set = linter.lint_with_source(
        Some(&script),
        analysis.template.as_ref(),
        &analysis.styles,
        Some(source),
    );
    map_diagnostic_set(&set, line_index)
}

/// Construct a [`ScriptAnalysisSnapshot`] by borrowing from a host
/// [`FileAnalysisSnapshot`].
fn script_from_host(analysis: &FileAnalysisSnapshot) -> ScriptAnalysisSnapshot {
    ScriptAnalysisSnapshot {
        imports: analysis.imports.clone(),
        bindings: analysis.bindings.clone(),
        macros: analysis.macros.clone(),
        macro_type_deps: analysis.macro_type_deps.clone(),
        flags: AnalysisFlags::from_bits_truncate(analysis.script_flags),
        vue_api_calls: analysis.vue_api_calls.clone(),
        ..Default::default()
    }
}

/// Convert a [`DiagnosticSet`] from the diagnostics engine into LSP [`Diagnostic`] items.
pub fn map_diagnostic_set(set: &DiagnosticSet, line_index: &LineIndex) -> Vec<Diagnostic> {
    set.iter()
        .map(|d| map_lint_diagnostic(d, line_index))
        .collect()
}

fn map_lint_diagnostic(diag: &LintDiagnostic, line_index: &LineIndex) -> Diagnostic {
    let start_pos = line_index
        .offset_to_position(diag.span.start)
        .unwrap_or(Position {
            line: 0,
            character: 0,
        });
    let end_pos = line_index
        .offset_to_position(diag.span.end)
        .unwrap_or(start_pos);

    let tags: Vec<DiagnosticTag> = diag
        .tags
        .iter()
        .map(|t| match t {
            LintDiagnosticTag::Unnecessary => DiagnosticTag::UNNECESSARY,
            LintDiagnosticTag::Deprecated => DiagnosticTag::DEPRECATED,
        })
        .collect();

    Diagnostic {
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        severity: Some(map_lint_severity(&diag.severity)),
        code: Some(NumberOrString::String(format!("verter/{}", diag.rule))),
        source: Some("verter".to_string()),
        message: diag.message.clone(),
        tags: if tags.is_empty() { None } else { Some(tags) },
        ..Default::default()
    }
}

/// Get quick-fix code actions from the action engine for diagnostics in the
/// editor's code action context.
///
/// Re-runs the linter to reconstruct the `DiagnosticSet`, matches context
/// diagnostics by rule name + range, and converts action results to LSP types.
pub fn action_engine_fixes(
    engine: &verter_actions::ActionEngine,
    analysis: &FileAnalysisSnapshot,
    source: &str,
    line_index: &LineIndex,
    linter: &verter_diagnostics::Linter,
    context_diagnostics: &[Diagnostic],
    uri: &Uri,
) -> Vec<CodeActionOrCommand> {
    use verter_actions::ActionContext;

    let script = script_from_host(analysis);
    let lint_set = linter.lint_with_source(
        Some(&script),
        analysis.template.as_ref(),
        &analysis.styles,
        Some(source),
    );

    let file_id = uri.as_str();
    let ctx = ActionContext {
        source,
        file_id,
        diagnostics: &lint_set,
        template: analysis.template.as_ref(),
        script: Some(&script),
        styles: &analysis.styles,
    };

    let mut result = Vec::new();

    // Match context diagnostics to lint diagnostics and get fixes.
    for lsp_diag in context_diagnostics {
        let rule = match &lsp_diag.code {
            Some(NumberOrString::String(s)) => s.strip_prefix("verter/").unwrap_or(s),
            _ => continue,
        };

        for (_idx, lint_diag) in lint_set.find_by_rule(rule) {
            let start = line_index.offset_to_position(lint_diag.span.start);
            let end = line_index.offset_to_position(lint_diag.span.end);
            if let (Some(s), Some(e)) = (start, end) {
                if s == lsp_diag.range.start && e == lsp_diag.range.end {
                    let fixes = engine.fixes_for(lint_diag, &ctx);
                    for fix in fixes {
                        result.push(map_code_action_to_lsp(&fix, lsp_diag, line_index, uri));
                    }
                }
            }
        }
    }

    result
}

fn map_code_action_to_lsp(
    action: &verter_actions::CodeAction,
    lsp_diag: &Diagnostic,
    line_index: &LineIndex,
    uri: &Uri,
) -> CodeActionOrCommand {
    use verter_actions::ActionKind;

    let kind = match action.kind {
        ActionKind::QuickFix => CodeActionKind::QUICKFIX,
        ActionKind::Refactor => CodeActionKind::REFACTOR,
        ActionKind::Source => CodeActionKind::SOURCE,
    };

    let mut text_edits = Vec::new();
    for edit in &action.edits {
        if edit.file_id.is_some() {
            continue; // Cross-file edits require a different WorkspaceEdit shape
        }
        let start = line_index
            .offset_to_position(edit.span.start)
            .unwrap_or(Position {
                line: 0,
                character: 0,
            });
        let end = line_index
            .offset_to_position(edit.span.end)
            .unwrap_or(start);
        text_edits.push(TextEdit {
            range: Range { start, end },
            new_text: edit.replacement.clone(),
        });
    }

    #[allow(clippy::mutable_key_type)] // Uri has interior mutability but is used correctly here
    let mut changes = std::collections::HashMap::new();
    changes.insert(uri.clone(), text_edits);

    CodeActionOrCommand::CodeAction(tower_lsp_server::ls_types::CodeAction {
        title: action.title.clone(),
        kind: Some(kind),
        diagnostics: Some(vec![lsp_diag.clone()]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        is_preferred: Some(action.is_preferred),
        ..Default::default()
    })
}

/// Get position-based refactoring actions from the action engine at a byte
/// offset (cursor position).
///
/// This calls `actions_at()` on all registered providers and converts results
/// to LSP types.
pub fn action_engine_refactorings(
    engine: &verter_actions::ActionEngine,
    analysis: &FileAnalysisSnapshot,
    source: &str,
    line_index: &LineIndex,
    linter: &verter_diagnostics::Linter,
    offset: u32,
    uri: &Uri,
) -> Vec<CodeActionOrCommand> {
    use verter_actions::ActionContext;

    let script = script_from_host(analysis);
    let lint_set = linter.lint_with_source(
        Some(&script),
        analysis.template.as_ref(),
        &analysis.styles,
        Some(source),
    );

    let file_id = uri.as_str();
    let ctx = ActionContext {
        source,
        file_id,
        diagnostics: &lint_set,
        template: analysis.template.as_ref(),
        script: Some(&script),
        styles: &analysis.styles,
    };

    let actions = engine.actions_at(offset, &ctx);
    actions
        .iter()
        .map(|a| map_refactoring_action_to_lsp(a, line_index, uri))
        .collect()
}

fn map_refactoring_action_to_lsp(
    action: &verter_actions::CodeAction,
    line_index: &LineIndex,
    uri: &Uri,
) -> CodeActionOrCommand {
    use verter_actions::ActionKind;

    let kind = match action.kind {
        ActionKind::QuickFix => CodeActionKind::QUICKFIX,
        ActionKind::Refactor => CodeActionKind::REFACTOR,
        ActionKind::Source => CodeActionKind::SOURCE,
    };

    let mut text_edits = Vec::new();
    for edit in &action.edits {
        if edit.file_id.is_some() {
            continue;
        }
        let start = line_index
            .offset_to_position(edit.span.start)
            .unwrap_or(Position {
                line: 0,
                character: 0,
            });
        let end = line_index
            .offset_to_position(edit.span.end)
            .unwrap_or(start);
        text_edits.push(TextEdit {
            range: Range { start, end },
            new_text: edit.replacement.clone(),
        });
    }

    #[allow(clippy::mutable_key_type)]
    let mut changes = std::collections::HashMap::new();
    changes.insert(uri.clone(), text_edits);

    CodeActionOrCommand::CodeAction(tower_lsp_server::ls_types::CodeAction {
        title: action.title.clone(),
        kind: Some(kind),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        is_preferred: Some(action.is_preferred),
        ..Default::default()
    })
}

fn map_lint_severity(severity: &Severity) -> DiagnosticSeverity {
    match severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Info => DiagnosticSeverity::INFORMATION,
        Severity::Hint => DiagnosticSeverity::HINT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_diagnostics::{DiagnosticSet, DiagnosticSpanKind};

    fn make_line_index(source: &str) -> LineIndex {
        LineIndex::new(source, PositionEncodingKind::UTF16)
    }

    #[test]
    fn empty_set_produces_empty_diagnostics() {
        let set = DiagnosticSet::new();
        let li = make_line_index("hello\nworld");
        let result = map_diagnostic_set(&set, &li);
        assert!(result.is_empty());
    }

    #[test]
    fn maps_severity_and_code() {
        let mut set = DiagnosticSet::new();
        set.add(LintDiagnostic {
            rule: "unused-css-selector".to_string(),
            category: "css".to_string(),
            severity: Severity::Hint,
            message: "Selector `.foo` is unused".to_string(),
            span: verter_span::Span::new(0, 5),
            tags: vec![LintDiagnosticTag::Unnecessary],
            span_kind: DiagnosticSpanKind::CssSelector,
        });
        let li = make_line_index("hello\nworld");
        let result = map_diagnostic_set(&set, &li);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].severity, Some(DiagnosticSeverity::HINT));
        assert_eq!(
            result[0].code,
            Some(NumberOrString::String("verter/unused-css-selector".into()))
        );
        assert_eq!(result[0].source.as_deref(), Some("verter"));
        assert_eq!(result[0].tags, Some(vec![DiagnosticTag::UNNECESSARY]));
    }

    #[test]
    fn maps_all_severities() {
        let li = make_line_index("x");
        let make = |sev| {
            let mut set = DiagnosticSet::new();
            set.add(LintDiagnostic {
                rule: "r".into(),
                category: "c".into(),
                severity: sev,
                message: "m".into(),
                span: verter_span::Span::new(0, 1),
                tags: vec![],
                span_kind: DiagnosticSpanKind::ElementOpenTag,
            });
            map_diagnostic_set(&set, &li)
        };

        assert_eq!(
            make(Severity::Error)[0].severity,
            Some(DiagnosticSeverity::ERROR)
        );
        assert_eq!(
            make(Severity::Warning)[0].severity,
            Some(DiagnosticSeverity::WARNING)
        );
        assert_eq!(
            make(Severity::Info)[0].severity,
            Some(DiagnosticSeverity::INFORMATION)
        );
        assert_eq!(
            make(Severity::Hint)[0].severity,
            Some(DiagnosticSeverity::HINT)
        );
    }

    #[test]
    fn no_tags_produces_none() {
        let mut set = DiagnosticSet::new();
        set.add(LintDiagnostic {
            rule: "r".into(),
            category: "c".into(),
            severity: Severity::Warning,
            message: "m".into(),
            span: verter_span::Span::new(0, 1),
            tags: vec![],
            span_kind: DiagnosticSpanKind::ElementOpenTag,
        });
        let li = make_line_index("x");
        let result = map_diagnostic_set(&set, &li);
        assert_eq!(result[0].tags, None, "empty tags should map to None");
    }

    #[test]
    fn position_mapping_multiline() {
        let source = "<template>\n  <div class=\"foo\">text</div>\n</template>";
        let li = make_line_index(source);
        // "foo" starts at byte 23 (class="foo")
        let foo_start = source.find("foo").unwrap() as u32;
        let foo_end = foo_start + 3;

        let mut set = DiagnosticSet::new();
        set.add(LintDiagnostic {
            rule: "undefined-css-class".into(),
            category: "css".into(),
            severity: Severity::Hint,
            message: "Class `foo` is undefined".into(),
            span: verter_span::Span::new(foo_start, foo_end),
            tags: vec![LintDiagnosticTag::Unnecessary],
            span_kind: DiagnosticSpanKind::CssSelector,
        });

        let result = map_diagnostic_set(&set, &li);
        assert_eq!(result.len(), 1);
        // "foo" is on line 1 (0-indexed), character 14
        assert_eq!(result[0].range.start.line, 1);
        assert!(result[0].range.start.character > 0);
        assert_eq!(result[0].range.end.line, 1);
    }
}

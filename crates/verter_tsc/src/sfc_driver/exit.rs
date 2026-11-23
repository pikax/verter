//! Exit status and the printed outcome of a run.

use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

use verter_compiler::framework_common::vue_projection_backend::ObligationFamily;
use verter_session::sfc_projection_diagnostics::{
    DiagnosticAnchor, DiagnosticCategory, ProjectionDiagnosticAttribution,
};
use verter_session::sfc_projection_mapper::MapperDiagnosticCode;
use verter_span::path::InjectedPathKey;
use verter_workspace::native_fs::NativeFs;

use super::per_file::CarrierProjection;
use super::project_graph::ProjectGraphError;
use super::project_program::{missing_reference_diagnostics, ProjectCheck};
use crate::position::{ColumnEncoding, LineTable};
use crate::reporter::{Diagnostic, PositionRenderer, Severity, VerterCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitStatus {
    /// No diagnostics.
    Clean = 0,
    /// At least one type or syntax diagnostic and nothing worse.
    Diagnostics = 1,
    /// A projection-internal failure, an engine failure or a config problem.
    Failed = 2,
}

#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub projects: Vec<ProjectCheck>,
    pub graph_problems: Vec<ProjectGraphError>,
    pub carriers_checked: usize,
    pub emitted_files: Vec<std::path::PathBuf>,
}

/// `Failed` when any project has an engine failure, any carrier is
/// unreadable, any mapper diagnostic is `ProjectionInternal`, or the graph
/// has a problem. Otherwise `Diagnostics` when any error exists. Otherwise
/// `Clean`. Warnings, a carrier's own included, do not change the status.
#[must_use]
pub fn exit_status(outcome: &RunOutcome) -> ExitStatus {
    let failed = !outcome.graph_problems.is_empty()
        || outcome.projects.iter().any(|project| {
            project.engine_failure.is_some()
                || project.checked_carriers().any(|carrier| {
                    carrier.unreadable
                        || carrier
                            .result
                            .diagnostics
                            .iter()
                            .any(|d| d.code == MapperDiagnosticCode::ProjectionInternal)
                })
        });
    if failed {
        return ExitStatus::Failed;
    }
    let errors = outcome.projects.iter().any(|project| {
        project
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
            || project.checked_carriers().any(|carrier| {
                carrier
                    .result
                    .diagnostics
                    .iter()
                    .any(|diagnostic| !diagnostic.code.is_warning())
            })
            || project
                .attributed
                .iter()
                .any(|d| d.category == DiagnosticCategory::Error)
    });
    if errors {
        ExitStatus::Diagnostics
    } else {
        ExitStatus::Clean
    }
}

/// Every diagnostic of the run, in print order: graph problems, then each
/// project in graph order. A project's lines (its layout warnings, each
/// carrier's own diagnostics and partial-check warning, its engine
/// diagnostics) print in tsc's order: by file, then by position, so a
/// carrier's own lines sit among its engine lines. Its engine failure comes
/// last.
#[must_use]
pub fn outcome_diagnostics(outcome: &RunOutcome) -> Vec<Diagnostic> {
    let mut sources = AuthoredSources::new(outcome);
    let mut out = Vec::new();
    for problem in &outcome.graph_problems {
        out.extend(problem_diagnostics(problem, &mut sources));
    }
    for project in &outcome.projects {
        let mut lines = project.diagnostics.clone();
        for carrier in project.checked_carriers() {
            lines.extend(carrier.diagnostics());
            lines.extend(partial_check(carrier));
        }
        lines.extend(
            project
                .attributed
                .iter()
                .map(|diagnostic| sources.diagnostic(diagnostic)),
        );
        // tsc compares files by their path under the host's case policy, and
        // lines with no file come first. The sort is stable: lines at one
        // position keep the order they were reported in.
        lines.sort_by_cached_key(|line| {
            (
                InjectedPathKey::new(&line.file).as_str().to_string(),
                line.line,
                line.col,
            )
        });
        out.extend(lines);
        if let Some(failure) = &project.engine_failure {
            out.push(Diagnostic::verter(
                project.config.to_string(),
                0,
                0,
                VerterCode::ProjectCheckFailed,
                failure.clone(),
            ));
        }
    }
    out
}

/// The warning on a carrier whose module was composed with withheld
/// products: those checks did not run, so the carrier is never reported as
/// fully checked. Families are listed in product order.
fn partial_check(carrier: &CarrierProjection) -> Option<Diagnostic> {
    let module = carrier.result.module.as_ref()?;
    if module.withheld.is_empty() {
        return None;
    }
    let families = module
        .withheld
        .iter()
        .map(|product| family_name(product.family))
        .collect::<Vec<_>>()
        .join(", ");
    let mut diagnostic = Diagnostic::verter(
        carrier.carrier.to_string(),
        1,
        1,
        VerterCode::PartialCheck,
        format!("this file was only partly checked: {families} checks were not run"),
    );
    diagnostic.severity = Severity::Warning;
    Some(diagnostic)
}

/// The user's name for the checks of one obligation family.
fn family_name(family: ObligationFamily) -> &'static str {
    match family {
        ObligationFamily::Interpolation => "interpolation",
        ObligationFamily::Branch => "v-if branch",
        ObligationFamily::Iteration => "v-for",
        ObligationFamily::ComponentUse => "component",
        ObligationFamily::Listener => "event listener",
        ObligationFamily::Model => "v-model",
        ObligationFamily::ProvidedSlot => "slot content",
        ObligationFamily::SlotOutlet => "slot outlet",
        ObligationFamily::NativeElement => "element attribute",
        ObligationFamily::NativeModel => "element v-model",
        ObligationFamily::Directive => "directive",
        ObligationFamily::Builtin => "built-in component",
        ObligationFamily::Fallthrough => "fallthrough attribute",
        ObligationFamily::TemplateRef => "template ref",
        ObligationFamily::Scaffold => "template",
    }
}

/// A renderer that positions carrier diagnostics in the exact source each
/// carrier was checked from, not in what is on disk when printing.
#[must_use]
pub fn outcome_renderer(outcome: &RunOutcome, encoding: ColumnEncoding) -> PositionRenderer {
    let checked = outcome
        .projects
        .iter()
        .flat_map(|project| project.checked_carriers())
        // An unreadable carrier has no checked text to position in.
        .filter(|carrier| !carrier.unreadable)
        .map(|carrier| (carrier.carrier.to_string(), carrier.source.clone()))
        .collect::<HashMap<_, _>>();
    PositionRenderer::new(encoding).with_virtual_sources(&checked)
}

/// Print the run's diagnostics, one line each, in [`outcome_diagnostics`]
/// order.
pub fn render_outcome(
    outcome: &RunOutcome,
    renderer: &PositionRenderer,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    for diagnostic in outcome_diagnostics(outcome) {
        writeln!(out, "{}", renderer.render(&diagnostic))?;
    }
    Ok(())
}

/// The text of a graph problem that stops the load before any project is
/// checked.
pub(super) fn load_failure(problem: &ProjectGraphError) -> String {
    problem_diagnostics(problem, &mut AuthoredSources::default())
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// A graph problem as the diagnostics tsc reports for it.
fn problem_diagnostics(
    problem: &ProjectGraphError,
    sources: &mut AuthoredSources,
) -> Vec<Diagnostic> {
    let global = |ts_code, message| Diagnostic {
        file: String::new(),
        line: 0,
        col: 0,
        ts_code,
        verter_code: None,
        message,
        severity: Severity::Error,
    };
    match problem {
        ProjectGraphError::MissingReference { .. } => {
            missing_reference_diagnostics(std::slice::from_ref(problem))
                .iter()
                .map(|diagnostic| sources.diagnostic(diagnostic))
                .collect()
        }
        ProjectGraphError::Cycle { path } => vec![global(
            6202,
            format!(
                "Project references may not form a circular graph. Cycle detected: {}",
                path.iter()
                    .map(|config| config.to_string_lossy().replace('\\', "/"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        )],
        ProjectGraphError::UnreadableRoot { config, .. } => vec![global(
            5082,
            format!("Cannot read tsconfig file '{}'.", config.display()),
        )],
    }
}

/// The one-based line and UTF-16 column of a UTF-8 byte offset, the engine's
/// position convention. `(0, 0)` when the offset is not a position in the
/// text; such a diagnostic prints without a position.
pub(super) fn engine_position(table: &LineTable<'_>, byte: u32) -> (u32, u32) {
    table
        .line_col_of_offset(u64::from(byte), ColumnEncoding::Utf8)
        .and_then(|(line, col)| {
            table
                .convert_column(line, col, ColumnEncoding::Utf8, ColumnEncoding::Utf16)
                .map(|col| (line, col))
        })
        .unwrap_or((0, 0))
}

/// A text and its line starts, scanned once.
type CheckedText = (Arc<str>, Vec<usize>);

/// The text each attributed diagnostic is positioned in: a carrier's checked
/// source from the outcome, else the authored file read once.
#[derive(Default)]
struct AuthoredSources {
    texts: HashMap<InjectedPathKey, Option<CheckedText>>,
    fs: NativeFs,
}

impl AuthoredSources {
    fn new(outcome: &RunOutcome) -> Self {
        let mut texts = HashMap::new();
        for carrier in outcome.projects.iter().flat_map(|p| p.checked_carriers()) {
            texts
                .entry(InjectedPathKey::new(&carrier.carrier))
                .or_insert_with(|| {
                    Some((
                        carrier.source.clone(),
                        LineTable::line_starts(&carrier.source),
                    ))
                });
        }
        Self {
            texts,
            fs: NativeFs::new(),
        }
    }

    fn diagnostic(&mut self, attributed: &ProjectionDiagnosticAttribution) -> Diagnostic {
        let file = attributed.carrier.to_string();
        let (line, col, message) = match &attributed.anchor {
            DiagnosticAnchor::Authored { start, .. } => {
                let (line, col) = self.position(&file, *start);
                (line, col, attributed.message.clone())
            }
            DiagnosticAnchor::Generated { snippet } => {
                let snippet = snippet.split_whitespace().collect::<Vec<_>>().join(" ");
                let message = if snippet.is_empty() {
                    attributed.message.clone()
                } else {
                    format!("{} (in generated code `{snippet}`)", attributed.message)
                };
                (0, 0, message)
            }
        };
        Diagnostic {
            file,
            line,
            col,
            ts_code: attributed.code,
            verter_code: None,
            message,
            severity: match attributed.category {
                DiagnosticCategory::Error => Severity::Error,
                DiagnosticCategory::Warning => Severity::Warning,
            },
        }
    }

    fn position(&mut self, file: &str, byte: u32) -> (u32, u32) {
        if file.is_empty() {
            return (0, 0);
        }
        let fs = &self.fs;
        let Some((text, line_starts)) = self
            .texts
            .entry(InjectedPathKey::new(file))
            .or_insert_with(|| {
                fs.read_file(file).map(|text| {
                    let line_starts = LineTable::line_starts(&text);
                    (text, line_starts)
                })
            })
        else {
            return (0, 0);
        };
        engine_position(&LineTable::from_parts(text, line_starts), byte)
    }
}

#[cfg(test)]
#[path = "exit_tests.rs"]
mod tests;

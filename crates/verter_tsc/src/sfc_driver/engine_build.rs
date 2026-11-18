//! Projects that emit and hold no carrier, built by the engine itself.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use verter_session::sfc_projection_diagnostics::{
    attribute_diagnostics, DiagnosticCategory, EngineDiagnostic, TransformLookup,
};
use verter_span::path::{canonicalize_path, simplify_verbatim_path, CanonicalPath};
use verter_span::DiagnosticSourceCache;
use verter_workspace::workspace_snapshot::ProjectPayload;
use verter_workspace::WorkspaceAccess;

use crate::checker::{checker_base_command, invoke_command_bounded};
use crate::position::{ColumnEncoding, LineTable};
use crate::reporter::{parse_tsc_build_output, Severity, TscBuildEvent};

use super::project_graph::{ProjectGraph, ProjectGraphError};
use super::project_program::ProjectCheck;

#[cfg(test)]
#[path = "engine_build_tests.rs"]
mod tests;

/// The part of a graph the engine builds on its own: every project that
/// emits, has no carrier, and references only such projects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineBuild {
    /// Configs handed to the engine, in graph order.
    pub configs: Vec<PathBuf>,
    /// Projects that emit but reference a project with carriers. Each is
    /// reported and fails the run.
    pub unsupported: Vec<(PathBuf, PathBuf)>,
}

impl EngineBuild {
    #[must_use]
    pub fn partition(graph: &ProjectGraph) -> Self {
        let incomplete: std::collections::BTreeSet<_> = graph
            .problems
            .iter()
            .filter_map(|problem| match problem {
                ProjectGraphError::MissingReference { from, .. } => Some(from.as_path()),
                _ => None,
            })
            .collect();
        let by_config: BTreeMap<_, _> = graph
            .projects
            .iter()
            .enumerate()
            .map(|(index, project)| (project.config.as_path(), index))
            .collect();
        let mut build = Self {
            configs: vec![],
            unsupported: vec![],
        };
        for project in &graph.projects {
            if !project.emits || project.solution_only {
                continue;
            }
            let mut eligible =
                project.carriers.is_empty() && !incomplete.contains(project.config.as_path());
            let mut carrier = None;
            let mut state = vec![0u8; graph.projects.len()];
            let mut pending: Vec<_> = project
                .references
                .iter()
                .rev()
                .map(|path| (path.as_path(), false))
                .collect();
            while let Some((config, leaving)) = pending.pop() {
                let Some(&index) = by_config.get(config) else {
                    eligible = false;
                    continue;
                };
                if leaving {
                    state[index] = 2;
                    continue;
                }
                match state[index] {
                    1 => {
                        eligible = false;
                        continue;
                    }
                    2 => continue,
                    _ => {}
                }
                state[index] = 1;
                let dependency = &graph.projects[index];
                if incomplete.contains(dependency.config.as_path()) {
                    eligible = false;
                }
                if !dependency.emits {
                    eligible = false;
                }
                if !dependency.carriers.is_empty() {
                    eligible = false;
                    carrier.get_or_insert_with(|| dependency.config.clone());
                }
                pending.push((config, true));
                pending.extend(
                    dependency
                        .references
                        .iter()
                        .rev()
                        .map(|path| (path.as_path(), false)),
                );
            }
            if let Some(carrier) = carrier {
                build.unsupported.push((project.config.clone(), carrier));
            } else if eligible {
                build.configs.push(project.config.clone());
            }
        }
        build
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.configs.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct EngineBuildOutcome {
    /// One entry per built project, in graph order, also when the build
    /// failed and left them unchecked.
    pub projects: Vec<ProjectCheck>,
    /// The CLI build does not request an emitted-file listing. Emit belongs
    /// entirely to the engine; no filesystem walk reconstructs that inventory.
    pub emitted_files: Vec<PathBuf>,
    /// Set when invocation or diagnostic attribution failed, including a child
    /// that exited unsuccessfully without reporting an attributable error.
    pub failure: Option<String>,
}

impl EngineBuildOutcome {
    /// The built projects as run results, in graph order. A failure of the
    /// one engine child is reported once, on the first project of the build,
    /// and names every project the build covered.
    pub(super) fn into_checks(self) -> Vec<ProjectCheck> {
        let mut projects = self.projects;
        if let Some(failure) = self.failure {
            let configs = projects
                .iter()
                .map(|project| format!("'{}'", project.config))
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(first) = projects.first_mut() {
                first.engine_failure =
                    Some(format!("The engine build of {configs} failed: {failure}"));
            }
        }
        projects
    }
}

/// The check of a project that emits but whose references reach a project
/// with carriers. The engine's build cannot compile carriers, so the project
/// is refused rather than checked without its output.
pub(super) fn refused_check(project: &Path, carrier_project: &Path) -> ProjectCheck {
    let mut check = unchecked(project);
    check.engine_failure = Some(format!(
        "Cannot build project '{}': it emits, and it references '{}', whose component files only the projection driver can check.",
        project.display(),
        carrier_project.display()
    ));
    check
}

/// A project with no program yet: no carriers and no diagnostics.
fn unchecked(config: &Path) -> ProjectCheck {
    ProjectCheck {
        config: Arc::from(config.to_string_lossy().as_ref()),
        carriers: Vec::new(),
        imported_carriers: Vec::new(),
        unowned_carriers: Vec::new(),
        foreign_surfaces: Vec::new(),
        diagnostics: Vec::new(),
        binding: None,
        raw: vec![],
        attributed: vec![],
        engine_failure: None,
    }
}

/// Run the engine's build once over `build.configs`. Membership and source
/// positions come from the same retained workspace used to load `graph`.
pub fn run_engine_build(
    build: &EngineBuild,
    graph: &ProjectGraph,
    ws: &dyn WorkspaceAccess,
    engine: &Path,
    force: bool,
    no_emit: bool,
) -> EngineBuildOutcome {
    let mut outcome = EngineBuildOutcome {
        projects: vec![],
        emitted_files: vec![],
        failure: None,
    };
    if build.is_empty() {
        return outcome;
    }
    let unbuilt = || {
        build
            .configs
            .iter()
            .map(PathBuf::as_path)
            .map(unchecked)
            .collect::<Vec<_>>()
    };
    let invocation = match invoke_command_bounded(
        build_command(build, engine, force, no_emit),
        &engine.display().to_string(),
        std::time::Duration::from_secs(300),
    ) {
        Ok(invocation) => invocation,
        Err(failure) => {
            outcome.projects = unbuilt();
            outcome.failure = Some(failure);
            return outcome;
        }
    };
    match attribute_output(build, graph, ws, &invocation.output) {
        Ok(projects) => outcome.projects = projects,
        Err(failure) => {
            outcome.projects = unbuilt();
            outcome.failure = Some(format!("{failure}\n{}", invocation.output));
            return outcome;
        }
    }
    if !invocation.success
        && !outcome
            .projects
            .iter()
            .flat_map(|project| &project.raw)
            .any(|diagnostic| diagnostic.category == DiagnosticCategory::Error)
    {
        outcome.failure = Some(format!(
            "engine build exited unsuccessfully without an attributable error\n{}",
            invocation.output
        ));
    }
    outcome
}

fn build_command(
    build: &EngineBuild,
    engine: &Path,
    force: bool,
    no_emit: bool,
) -> std::process::Command {
    let mut command = checker_base_command(engine);
    command.arg("-b");
    for config in &build.configs {
        command.arg(simplify_verbatim_path(config).as_ref());
    }
    command.args(["--pretty", "false", "--verbose"]);
    if force {
        command.arg("--force");
    }
    if no_emit {
        command.arg("--noEmit");
    }
    command
}

struct NoTransforms;
impl TransformLookup for NoTransforms {
    fn result_for_virtual(
        &self,
        _: &str,
    ) -> Option<Arc<verter_session::sfc_projection_mapper::MapperTransformResult>> {
        None
    }
    fn is_authored(&self, _: &str) -> bool {
        true
    }
}

fn attribute_output(
    build: &EngineBuild,
    graph: &ProjectGraph,
    ws: &dyn WorkspaceAccess,
    output: &str,
) -> Result<Vec<ProjectCheck>, String> {
    let published = ws
        .published_root()
        .ok_or("engine build has no published workspace membership")?;
    let mut projects: Vec<_> = graph
        .projects
        .iter()
        .filter(|project| build.configs.contains(&project.config))
        .map(|project| unchecked(&project.config))
        .collect();
    if projects.len() != build.configs.len() {
        return Err("engine build configs are absent from the loaded graph".into());
    }
    let memberships: Vec<_> = projects
        .iter()
        .map(|project| {
            published
                .snapshot
                .projects
                .iter()
                .find_map(|owner| match &owner.payload {
                    ProjectPayload::Configured {
                        tsconfig_path,
                        membership,
                        ..
                    } if tsconfig_path.as_str() == project.config.as_ref() => Some(membership),
                    _ => None,
                })
                .ok_or_else(|| {
                    format!(
                        "engine build project has no published membership: {}",
                        project.config
                    )
                })
        })
        .collect::<Result<_, _>>()?;
    let sources = DiagnosticSourceCache::new(|path: &str| ws.read_file(path));
    let mut line_starts = HashMap::new();
    let cwd = std::env::current_dir()
        .map_err(|error| format!("cannot resolve engine build working directory: {error}"))?;
    let canonical_file = |reported: &str| {
        let reported = Path::new(reported);
        let absolute = if reported.is_absolute() {
            reported.to_path_buf()
        } else {
            cwd.join(reported)
        };
        let spelling = absolute.to_string_lossy();
        canonicalize_path(
            &ws.realpath(&spelling)
                .unwrap_or_else(|| spelling.into_owned()),
        )
    };
    let mut building = None;
    for event in parse_tsc_build_output(output) {
        let diagnostic = match event {
            TscBuildEvent::BuildingProject(config) => {
                let config = canonical_file(&config);
                building = Some(projects.iter().position(|project| project.config.as_ref() == config)
                    .ok_or_else(|| format!("engine is building a project outside the selected partition: {config}"))?);
                continue;
            }
            TscBuildEvent::Diagnostic(diagnostic) => diagnostic,
        };
        let category = match diagnostic.severity {
            Severity::Error => DiagnosticCategory::Error,
            Severity::Warning => DiagnosticCategory::Warning,
        };
        if diagnostic.file.is_empty() {
            let owner = building
                .or_else(|| (projects.len() == 1).then_some(0))
                .ok_or("engine unlocated diagnostic has no project build progress")?;
            projects[owner].raw.push(EngineDiagnostic {
                file: Arc::from(""),
                start: 0,
                length: 0,
                code: diagnostic.ts_code,
                category,
                message: diagnostic.message,
                related: vec![],
            });
            continue;
        }
        let file = canonical_file(&diagnostic.file);
        let canonical = CanonicalPath::new(&file);
        let candidates: Vec<_> = projects
            .iter()
            .zip(&memberships)
            .enumerate()
            .filter_map(|(index, (project, membership))| {
                (project.config.as_ref() == file || membership.contains(&canonical))
                    .then_some(index)
            })
            .collect();
        // The engine's current program also owns diagnostics in its imported
        // dependencies, which need not be configured root files. Membership
        // is only the fallback when the engine supplies no build context.
        let owner = match (building, candidates.as_slice()) {
            (Some(owner), _) => owner,
            (None, [owner]) => *owner,
            (None, []) => {
                return Err(format!(
                    "engine diagnostic is outside the built projects' membership: {file}"
                ))
            }
            (None, _) => {
                return Err(format!(
                    "cannot attribute engine diagnostic on {file}; candidate projects: {}",
                    candidates
                        .iter()
                        .map(|&index| projects[index].config.as_ref())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            }
        };
        let source = sources
            .source_file(&file)
            .ok_or_else(|| format!("cannot read engine diagnostic source: {file}"))?;
        let starts = line_starts
            .entry(file.clone())
            .or_insert_with(|| LineTable::line_starts(source.text()));
        let table = LineTable::from_parts(source.text(), starts);
        let column = table
            .convert_column(
                diagnostic.line,
                diagnostic.col,
                ColumnEncoding::Utf16,
                ColumnEncoding::Utf8,
            )
            .map_err(|error| format!("invalid engine diagnostic position in {file}: {error}"))?;
        let line = usize::try_from(diagnostic.line)
            .ok()
            .and_then(|line| line.checked_sub(1))
            .and_then(|line| starts.get(line))
            .ok_or_else(|| format!("invalid engine diagnostic line in {file}"))?;
        let start = line
            .checked_add(column as usize - 1)
            .and_then(|offset| u32::try_from(offset).ok())
            .ok_or_else(|| format!("engine diagnostic position overflows in {file}"))?;
        projects[owner].raw.push(EngineDiagnostic {
            file: Arc::from(file),
            start,
            length: 0,
            code: diagnostic.ts_code,
            category,
            message: diagnostic.message,
            related: vec![],
        });
    }
    for project in &mut projects {
        project.attributed = attribute_diagnostics(&project.raw, &NoTransforms);
    }
    Ok(projects)
}

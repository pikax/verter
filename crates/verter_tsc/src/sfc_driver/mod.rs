#![allow(dead_code, unused_variables, unused_imports)]
//! The SFC projection driver over the certified TypeScript engine.

mod declaration_mirror;
mod declaration_output;
mod engine_build;
#[cfg(test)]
mod engine_tests;
mod exit;
pub(crate) mod mapper_server;
mod per_file;
mod project_graph;
mod project_program;
mod watch;
mod watch_binding;

pub use declaration_output::AtomicDeclarationOutput;
pub use engine_build::{run_engine_build, EngineBuild, EngineBuildOutcome};
pub use exit::{
    exit_status, outcome_diagnostics, outcome_renderer, render_outcome, ExitStatus, RunOutcome,
};
pub use per_file::{project_carrier, CarrierProjection};
pub use project_graph::{BuildMode, DriverProject, ProjectGraph, ProjectGraphError};
pub use project_program::{
    ForeignSurface, ForeignSurfaceSource, ProjectCheck, ProjectProgramInputs,
};
pub use watch::{run_watch, ChangeSource, Debouncer, FileSystemChangeSource, WatchSession};
pub use watch_binding::{ProjectWatchProjectionBinding, Recheck, WatchedPaths};

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use verter_session::sfc_projection_mapper::ContentMapperProjectionAdapter;
use verter_session::sfc_projection_observations::ProjectionPublicationAdmission;
use verter_tsgo_api::toolchain::discovery::Resolution;
use verter_tsgo_api::toolchain::validation::Capability;
use verter_workspace::{FilesystemOptions, FilesystemWorkspace};

use crate::api_check::TypecheckError;
use project_graph::GraphLoad;

/// What the command line asked for.
#[derive(Debug, Clone)]
pub struct DriverRequest {
    pub mode: BuildMode,
    /// The `-p` target, or the `-b` roots.
    pub configs: Vec<PathBuf>,
    /// `--noEmit`: no route writes output. The engine build checks its
    /// projects without emitting, and declarations are written only when
    /// `declaration` is set and this is not.
    pub no_emit: bool,
    pub declaration: bool,
    pub declaration_dir: Option<PathBuf>,
    pub tsgo_bin: Option<PathBuf>,
    /// `--force`, passed to the engine's build for emitting projects.
    pub force: bool,
    /// `--watch`.
    pub watch: bool,
}

/// The driver's public face. Its adapter, cache and admission state are
/// private.
pub struct SfcTypeScriptDriverAdapter {
    mapper: ContentMapperProjectionAdapter,
    /// Shared with a watch loop's change reader, which restarts the projects
    /// a change makes stale while a check runs.
    admission: Arc<ProjectionPublicationAdmission>,
    workspace: Arc<FilesystemWorkspace>,
    load_ordinal: AtomicU64,
}

impl SfcTypeScriptDriverAdapter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            mapper: ContentMapperProjectionAdapter::new(),
            admission: Arc::new(ProjectionPublicationAdmission::new()),
            workspace: Arc::new(FilesystemWorkspace::new(FilesystemOptions::default())),
            load_ordinal: AtomicU64::new(0),
        }
    }
    /// Check every project of the request, in graph order. A failure inside
    /// one carrier or one project is recorded in the outcome. Only an
    /// unusable engine or an unreadable root config returns `Err`.
    ///
    /// In build mode, the projects the engine builds on its own (see
    /// [`EngineBuild`]) go through one engine build first, which finishes
    /// before the first driver program opens, so a driver project reads the
    /// declarations those projects emit. Every other project is one driver
    /// program. Each project is checked once, by one of the two. `surfaces`
    /// serves each project's carriers of another framework.
    pub fn run(
        &self,
        request: &DriverRequest,
        surfaces: Option<&dyn ForeignSurfaceSource>,
    ) -> Result<RunOutcome, TypecheckError> {
        let load = self.load_graph(request)?;
        let mut engine = None;
        let (mut routed, engine_route) =
            self.engine_route(request, &load, &mut engine, request.force)?;
        let projects = load
            .graph
            .projects
            .iter()
            .map(|project| {
                if let Some(check) = routed.remove(&project.config) {
                    return Ok(check);
                }
                resolve_engine_for(project, request, &mut engine)?;
                Ok(project_program::check_project(
                    self,
                    &load,
                    project,
                    engine.as_ref(),
                    declaration_output::publishes(request, project),
                    surfaces,
                ))
            })
            .collect::<Result<Vec<_>, TypecheckError>>()?;
        Ok(outcome(self, request, &load, projects, engine_route))
    }

    /// The engine's share of a check of `load`, before any driver program
    /// opens: in build mode, the projects the engine builds on its own (see
    /// [`EngineBuild`]) go through one engine build, and the projects that
    /// emit but reference a carrier project are refused, as is a built
    /// project that imports a carrier the build could not compile. Returns
    /// their checks by config, and what the outcome needs of the route.
    /// `force` is passed to the engine build. The engine is resolved here
    /// only when a build runs and none was resolved before.
    pub(crate) fn engine_route(
        &self,
        request: &DriverRequest,
        load: &GraphLoad,
        engine: &mut Option<Resolution>,
        force: bool,
    ) -> Result<(HashMap<PathBuf, ProjectCheck>, EngineRoute), TypecheckError> {
        let build = match request.mode {
            BuildMode::Build => EngineBuild::partition(&load.graph),
            BuildMode::Project => EngineBuild {
                configs: Vec::new(),
                unsupported: Vec::new(),
            },
        };
        let mut routed = HashMap::new();
        let mut engine_route = EngineRoute {
            configs: build
                .configs
                .iter()
                .chain(build.unsupported.iter().map(|(config, _)| config))
                .cloned()
                .collect(),
            emitted_files: Vec::new(),
        };
        if let Some(first) = load
            .graph
            .projects
            .iter()
            .find(|project| build.configs.contains(&project.config))
        {
            if engine.is_none() {
                *engine = Some(resolve_engine(&first.root, request)?);
            }
            let resolution = engine.as_ref().expect("the engine was resolved above");
            let mut outcome = run_engine_build(
                &build,
                &load.graph,
                self.workspace.as_ref(),
                &resolution.path,
                force,
                request.no_emit,
            );
            engine_route.emitted_files = std::mem::take(&mut outcome.emitted_files);
            routed.extend(
                engine_build::refuse_carrier_imports(
                    outcome.into_checks(),
                    load,
                    self.workspace.as_ref(),
                )
                .into_iter()
                .map(|check| (PathBuf::from(&*check.config), check)),
            );
        }
        for (config, carrier_project) in &build.unsupported {
            routed.insert(
                config.clone(),
                engine_build::refused_check(config, carrier_project),
            );
        }
        Ok((routed, engine_route))
    }

    /// Load and publish the request's graph under the next load ordinal.
    pub(crate) fn next_load(
        &self,
        request: &DriverRequest,
    ) -> Result<GraphLoad, ProjectGraphError> {
        let ordinal = self.load_ordinal.fetch_add(1, Ordering::Relaxed) + 1;
        ProjectGraph::load_published(&self.workspace, request.mode, &request.configs, ordinal)
    }

    /// [`Self::next_load`], with a graph that cannot load reported as the
    /// run's error.
    pub(crate) fn load_graph(&self, request: &DriverRequest) -> Result<GraphLoad, TypecheckError> {
        self.next_load(request)
            .map_err(|problem| TypecheckError::new(exit::load_failure(&problem)))
    }
}

/// What the engine build contributes to a run's outcome.
#[derive(Debug, Clone, Default)]
pub(crate) struct EngineRoute {
    /// The projects the driver did not check: the engine built them, or the
    /// run refused them. The driver publishes none of their declarations;
    /// the engine wrote a built project's output itself.
    pub(crate) configs: HashSet<PathBuf>,
    /// The files the engine build wrote.
    pub(crate) emitted_files: Vec<PathBuf>,
}

/// The engine for programs under `root`.
fn resolve_engine(root: &Path, request: &DriverRequest) -> Result<Resolution, TypecheckError> {
    crate::checker::resolve_tsgo_engine(root, Capability::Api, request.tsgo_bin.as_deref())
        .map_err(TypecheckError::new)
}

/// Resolve the engine when the first project that opens a program needs it,
/// so a solution-only request runs without one.
pub(crate) fn resolve_engine_for(
    project: &DriverProject,
    request: &DriverRequest,
    engine: &mut Option<Resolution>,
) -> Result<(), TypecheckError> {
    if !project.solution_only && engine.is_none() {
        *engine = Some(resolve_engine(&project.root, request)?);
    }
    Ok(())
}

/// The outcome of the checked projects, in graph order. Graph-owned
/// missing-reference diagnostics suppress the engine's copies, a native
/// warning on a file is reported by its first project only, and then every
/// publishing project the driver checked publishes its declarations. The
/// engine build's files are listed before the published declarations.
pub(crate) fn outcome(
    driver: &SfcTypeScriptDriverAdapter,
    request: &DriverRequest,
    load: &GraphLoad,
    mut projects: Vec<ProjectCheck>,
    engine: EngineRoute,
) -> RunOutcome {
    let graph_owned = project_program::missing_reference_diagnostics(&load.graph.problems)
        .into_iter()
        .map(|diagnostic| diagnostic.dedup)
        .collect::<HashSet<_>>();
    let mut warned = HashSet::new();
    for project in &mut projects {
        project
            .attributed
            .retain(|diagnostic| !graph_owned.contains(&diagnostic.dedup));
        project.diagnostics.retain(|diagnostic| {
            warned.insert((
                verter_span::path::InjectedPathKey::new(&diagnostic.file),
                diagnostic.verter_code,
            ))
        });
    }
    // After the per-file warning collapse: every declaration diagnostic
    // of a project is its own.
    let mut emitted_files = engine.emitted_files;
    emitted_files.extend(declaration_output::publish_declarations(
        &*driver.workspace,
        request,
        load,
        &mut projects,
        &engine.configs,
    ));
    let carriers_checked = projects.iter().map(|p| p.carriers.len()).sum();
    RunOutcome {
        projects,
        graph_problems: load.graph.problems.clone(),
        carriers_checked,
        emitted_files,
    }
}

impl Default for SfcTypeScriptDriverAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Entry used by `main`. `surfaces` serves carriers of another framework.
pub fn run(
    request: &DriverRequest,
    surfaces: Option<&dyn ForeignSurfaceSource>,
) -> Result<RunOutcome, TypecheckError> {
    SfcTypeScriptDriverAdapter::new().run(request, surfaces)
}

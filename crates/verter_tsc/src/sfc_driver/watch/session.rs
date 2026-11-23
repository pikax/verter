//! Engine sessions and projections kept between runs.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use verter_session::sfc_projection_mapper::ContentMapperProjectionAdapter;
use verter_session::sfc_projection_observations::ProjectionPublicationAdmission;
use verter_session_query::resolution::path_utils::path_is_carrier;
use verter_span::path::canonicalize_path;
use verter_tsgo_api::toolchain::discovery::Resolution;
use verter_workspace::{WorkspaceChange, WorkspaceRead};

use super::super::declaration_output;
use super::super::project_graph::GraphLoad;
use super::super::project_program::{check_live_project, LiveProgram, ProjectChanges};
use super::super::{
    outcome, resolve_engine_for, DriverRequest, EngineRoute, ProjectCheck,
    ProjectWatchProjectionBinding, Recheck, RunOutcome, SfcTypeScriptDriverAdapter, WatchedPaths,
};
use crate::api_check::TypecheckError;

/// The state of one watched request. Its outcome after any sequence of
/// rechecks equals a fresh run over the same files. The graph, engine
/// sessions, adapter and watch binding it holds are private.
pub struct WatchSession {
    driver: SfcTypeScriptDriverAdapter,
    request: DriverRequest,
    load: GraphLoad,
    binding: ProjectWatchProjectionBinding,
    engine: Option<Resolution>,
    /// The projects the engine build checks, as the last build left them.
    route: EngineRoute,
    /// One entry per project of `load`, in graph order.
    projects: Vec<WatchedProject>,
    /// Changes no check has consumed yet, because the graph reload they
    /// needed failed. The next recheck takes them with its own.
    pending: Recheck,
}

/// A project's live program and its last check, before the run's
/// cross-project deduplication, so a later outcome is assembled exactly as a
/// fresh run assembles it.
struct WatchedProject {
    live: Option<LiveProgram>,
    check: ProjectCheck,
}

impl WatchedProject {
    /// The host paths the project's engine session has read from disk since
    /// it opened: imported sources, failed-lookup probes and listed
    /// directories.
    ///
    /// The engine keeps what it read for the session's life and reads a path
    /// again only when told it changed, so the set grows while one session
    /// serves, bounded by what that session holds, and starts over with each
    /// new session. A path the program stopped importing stays until then:
    /// the engine still holds its old text, and serves it if the import
    /// returns, so it must still be told when the path changes.
    ///
    /// The carriers the program reached by import are read too: the engine
    /// reads their projections, so each carrier itself is what changes. So
    /// is the carrier of every checking module the engine looked for, found
    /// or not: a carrier created after its import is what the import then
    /// resolves to.
    fn program_reads(&self, mapper: &ContentMapperProjectionAdapter) -> Vec<PathBuf> {
        let host = self
            .live
            .as_ref()
            .map(LiveProgram::host_reads)
            .unwrap_or_default();
        let probed = host
            .iter()
            .filter_map(|read| mapper.carrier_of_checking_module(read))
            .collect::<Vec<_>>();
        host.into_iter()
            .chain(probed)
            .map(PathBuf::from)
            .chain(
                self.check
                    .imported_carriers
                    .iter()
                    .map(|carrier| PathBuf::from(&*carrier.carrier)),
            )
            .collect()
    }
}

impl WatchSession {
    /// Load the graph and run every project once, as a fresh run does: in
    /// build mode the engine builds its own projects before any program
    /// opens.
    pub fn open(request: &DriverRequest) -> Result<(Self, RunOutcome), TypecheckError> {
        let mut session = Self::load(request)?;
        let outcome = session.check_all()?;
        Ok((session, outcome))
    }

    /// Load and bind the graph without checking anything. What a change
    /// source must report for the graph is known from here on; what the
    /// engine programs read is added by the first check.
    pub(crate) fn load(request: &DriverRequest) -> Result<Self, TypecheckError> {
        let driver = SfcTypeScriptDriverAdapter::new();
        let load = driver.load_graph(request)?;
        Ok(Self {
            binding: ProjectWatchProjectionBinding::new(&load.graph),
            driver,
            request: request.clone(),
            load,
            engine: None,
            route: EngineRoute::default(),
            projects: Vec::new(),
            pending: Recheck::default(),
        })
    }

    /// Run every project of a loaded session once (see [`Self::open`]).
    pub(crate) fn check_all(&mut self) -> Result<RunOutcome, TypecheckError> {
        let (mut routed, route) = self.driver.engine_route(
            &self.request,
            &self.load,
            &mut self.engine,
            self.request.force,
        )?;
        self.route = route;
        for project in &self.load.graph.projects {
            if let Some(check) = routed.remove(&project.config) {
                self.projects.push(WatchedProject { live: None, check });
                continue;
            }
            resolve_engine_for(project, &self.request, &mut self.engine)?;
            let mut live = None;
            let check = check_live_project(
                &self.driver,
                &self.load,
                project,
                self.engine.as_ref(),
                declaration_output::publishes(&self.request, project),
                None,
                &mut live,
                None,
            );
            self.projects.push(WatchedProject { live, check });
        }
        self.record_program_reads();
        Ok(self.outcome())
    }

    /// Redo what `next` names, with the changes a failed graph reload left
    /// unconsumed, and return the whole outcome.
    pub fn recheck(&mut self, next: &Recheck) -> RunOutcome {
        let mut recheck = std::mem::take(&mut self.pending);
        merge(&mut recheck, next);
        // The retained workspace reads changed files again from here on.
        let changes = recheck
            .changed
            .iter()
            .filter_map(|path| path.to_str())
            .map(|path| WorkspaceChange::FileChanged {
                canonical_id: path.to_owned(),
                source: None,
            })
            .collect::<Vec<_>>();
        if !changes.is_empty() {
            self.driver.workspace.apply_changes(changes);
        }
        // Carriers are projected from the workspace; every other changed
        // path is one the engine reads itself.
        let mut paths = recheck
            .changed
            .iter()
            .filter_map(|path| path.to_str())
            .filter(|path| !path_is_carrier(path))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        // A deleted carrier leaves the graph's carrier inventory.
        let deleted = recheck.carriers.iter().any(|(_, carrier)| {
            carrier
                .to_str()
                .is_some_and(|carrier| !self.driver.workspace.file_exists(carrier))
        });
        let stale: BTreeSet<PathBuf> = if recheck.reload_graph || deleted {
            match self.driver.next_load(&self.request) {
                Ok(load) => self.reload(load),
                Err(problem) => {
                    recheck.reload_graph = true;
                    self.pending = recheck;
                    return RunOutcome {
                        projects: Vec::new(),
                        graph_problems: vec![problem],
                        carriers_checked: 0,
                        emitted_files: Vec::new(),
                    };
                }
            }
        } else {
            recheck.projects.iter().cloned().collect()
        };
        if self.rebuild(&stale, recheck.reload_graph || deleted) {
            // The build may have rewritten anything a program read from
            // disk, wherever a built project emits, and an engine reads a
            // path again only when told it changed.
            let reads = self
                .projects
                .iter()
                .flat_map(|watched| watched.program_reads(&self.driver.mapper))
                .filter_map(|read| read.to_str().map(str::to_owned))
                .collect::<BTreeSet<_>>();
            paths.extend(reads);
        }
        for (project, watched) in self.load.graph.projects.iter().zip(&mut self.projects) {
            // The engine build checked it.
            if self.route.configs.contains(&project.config) {
                continue;
            }
            // A project whose engine failed is checked again on every recheck.
            if !stale.contains(&project.config) && watched.check.engine_failure.is_none() {
                continue;
            }
            if let Err(error) = resolve_engine_for(project, &self.request, &mut self.engine) {
                watched.check.engine_failure = Some(error.message);
                continue;
            }
            let carriers = recheck
                .carriers
                .iter()
                .filter(|(config, _)| config == &project.config)
                .map(|(_, carrier)| carrier.clone())
                .collect::<Vec<_>>();
            watched.check = check_live_project(
                &self.driver,
                &self.load,
                project,
                self.engine.as_ref(),
                declaration_output::publishes(&self.request, project),
                None,
                &mut watched.live,
                Some(ProjectChanges {
                    previous: &watched.check,
                    carriers: &carriers,
                    paths: &paths,
                }),
            );
        }
        self.record_program_reads();
        self.outcome()
    }

    /// Run the engine build again, before any program is checked, when one
    /// of its projects is stale or failed, or the graph was reloaded. Its own
    /// build info makes an unchanged project a no-op, so the build is not
    /// forced. Projects that moved into the build close their programs.
    /// Returns whether the build ran.
    fn rebuild(&mut self, stale: &BTreeSet<PathBuf>, reloaded: bool) -> bool {
        let routed_stale = |config: &PathBuf, check: &ProjectCheck| {
            self.route.configs.contains(config)
                && (stale.contains(config) || check.engine_failure.is_some())
        };
        let due = reloaded
            || self
                .load
                .graph
                .projects
                .iter()
                .zip(&self.projects)
                .any(|(project, watched)| routed_stale(&project.config, &watched.check));
        if !due {
            return false;
        }
        match self
            .driver
            .engine_route(&self.request, &self.load, &mut self.engine, false)
        {
            Ok((mut routed, route)) => {
                for (project, watched) in self.load.graph.projects.iter().zip(&mut self.projects) {
                    if let Some(check) = routed.remove(&project.config) {
                        if let Some(live) = watched.live.take() {
                            live.close(&self.driver);
                        }
                        watched.check = check;
                    }
                }
                self.route = route;
                true
            }
            // The engine cannot be resolved: the build's stale projects fail,
            // and the next recheck tries again.
            Err(error) => {
                for (project, watched) in self.load.graph.projects.iter().zip(&mut self.projects) {
                    if self.route.configs.contains(&project.config)
                        && stale.contains(&project.config)
                    {
                        watched.check.engine_failure = Some(error.message.clone());
                    }
                }
                false
            }
        }
    }

    /// The binding the last check left: what a change makes stale.
    pub(crate) fn binding(&self) -> &ProjectWatchProjectionBinding {
        &self.binding
    }

    /// The admission every project's results pass before they are reported.
    pub(crate) fn admission(&self) -> Arc<ProjectionPublicationAdmission> {
        Arc::clone(&self.driver.admission)
    }

    /// Give the binding every project's program reads; a reloaded graph's
    /// binding starts without them.
    fn record_program_reads(&mut self) {
        for (project, watched) in self.load.graph.projects.iter().zip(&self.projects) {
            self.binding
                .record_program_reads(&project.config, &watched.program_reads(&self.driver.mapper));
        }
    }

    /// What a set of changed paths makes stale.
    #[must_use]
    pub fn recheck_for(&self, changed: &[PathBuf]) -> Recheck {
        self.binding.recheck_for(changed)
    }

    /// What the change source must report for the current graph, its
    /// projections and what its engine programs read from disk.
    #[must_use]
    pub fn watched_paths(&self) -> WatchedPaths {
        self.binding.watched_paths()
    }

    /// Adopt a reloaded graph. Programs of projects that stay are kept; the
    /// rest are closed. Every project of the new graph is checked again: its
    /// program advances in place when its input basis held and is reopened
    /// when it moved.
    fn reload(&mut self, load: GraphLoad) -> BTreeSet<PathBuf> {
        let mut previous = self
            .load
            .graph
            .projects
            .iter()
            .map(|project| project.config.clone())
            .zip(std::mem::take(&mut self.projects))
            .collect::<Vec<_>>();
        for project in &load.graph.projects {
            let kept = previous
                .iter()
                .position(|(config, _)| config == &project.config)
                .map(|index| previous.remove(index).1);
            self.projects.push(kept.unwrap_or_else(|| WatchedProject {
                live: None,
                check: ProjectCheck {
                    config: Arc::from(canonicalize_path(&project.config.to_string_lossy())),
                    carriers: Vec::new(),
                    imported_carriers: Vec::new(),
                    unowned_carriers: Vec::new(),
                    foreign_surfaces: Vec::new(),
                    diagnostics: Vec::new(),
                    binding: None,
                    raw: Vec::new(),
                    attributed: Vec::new(),
                    engine_failure: None,
                },
            }));
        }
        for (_, gone) in previous {
            if let Some(live) = gone.live {
                live.close(&self.driver);
            }
        }
        self.binding = ProjectWatchProjectionBinding::new(&load.graph);
        self.load = load;
        self.load
            .graph
            .projects
            .iter()
            .map(|project| project.config.clone())
            .collect()
    }

    fn outcome(&self) -> RunOutcome {
        outcome(
            &self.driver,
            &self.request,
            &self.load,
            self.projects
                .iter()
                .map(|project| project.check.clone())
                .collect(),
            self.route.clone(),
        )
    }
}

/// Add `next` to the unconsumed `pending` changes.
fn merge(pending: &mut Recheck, next: &Recheck) {
    pending.reload_graph |= next.reload_graph;
    for project in &next.projects {
        if !pending.projects.contains(project) {
            pending.projects.push(project.clone());
        }
    }
    for carrier in &next.carriers {
        if !pending.carriers.contains(carrier) {
            pending.carriers.push(carrier.clone());
        }
    }
    pending.changed.extend(next.changed.iter().cloned());
    pending.changed.sort();
    pending.changed.dedup();
}

impl Drop for WatchSession {
    fn drop(&mut self) {
        for project in self.projects.drain(..) {
            if let Some(live) = project.live {
                live.close(&self.driver);
            }
        }
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

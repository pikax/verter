//! Builds `verter_semantic::analysis::RouteAnalysisInputs` from a live
//! `WorkspaceRead` — the caller-side half of the snapshot conversion.
//!
//! `analysis/routes.rs`'s six route-analysis functions took `&dyn
//! verter_workspace::WorkspaceRead` before this conversion; they now take
//! `&RouteAnalysisInputs`, an immutable snapshot with no handle. Snapshot
//! construction belongs to the higher-level LSP/MCP orchestration callers,
//! so the WALK that builds that snapshot
//! lives HERE — `verter_session` is the shared host-backed loading
//! authority both `verter_lsp` and `verter_mcp` already depend on
//! (CLAUDE.md: "`verter_session` ... is the authority for host-backed
//! loading"), so this is the one place that walk needs to exist, not
//! duplicated per consumer crate.
//!
//! Mirrors exactly the directory/file set `build_route_analysis`'s own
//! internal branching will consult: `package.json` (framework detection),
//! the pages directory the DETECTED framework selects (recursively), the
//! `layouts/` directory (recursively, unconditionally — `discover_layouts`
//! runs regardless of framework), and the fixed router-config candidate
//! paths (`ROUTER_CONFIG_CANDIDATES`, probed for existence, read if
//! present). A directory/file never captured here answers `is_dir ==
//! false` / `read_file == None` on the `RouteAnalysisInputs` side, which
//! is the desired collapse: route analysis has no error-vs-absent
//! distinction to preserve (unlike module resolution's witnessed
//! `PathProbe`).

use std::sync::Arc;

use verter_semantic::analysis::{
    detect_routing_framework, RouteAnalysisInputs, RouteDirEntry, RoutingFramework,
    ROUTER_CONFIG_CANDIDATES,
};

use crate::input_basis::{
    DirectoryEntry, InputBasis, LoadWave, NegativeFact, Observation, RequestInputBinding,
};

/// Builds the complete `RouteAnalysisInputs` snapshot for `project_root`.
///
/// The workspace is read only while committing one [`InputBasis`]. When a
/// request context is installed, that basis is bound once and later calls
/// project the bound rows. Without a request, the capture is fenced locally
/// then projected. Snapshot rows are never a second live consumer read.
#[must_use]
pub fn build_route_analysis_inputs(
    workspace: &dyn verter_workspace::WorkspaceRead,
    project_root: &str,
) -> RouteAnalysisInputs {
    if let Some(ctx) = crate::request_context::current_request_context() {
        if let Some(bound) = ctx.committed_input() {
            return project_admitted_route_analysis_inputs(bound);
        }
        let basis = commit_route_analysis_basis(workspace, project_root);
        return bind_and_project_route_analysis_inputs(&ctx, basis);
    }
    project_admitted_route_analysis_inputs(&RequestInputBinding::from_basis(
        commit_route_analysis_basis(workspace, project_root),
    ))
}

/// Bind `basis` as the request's sole committed input and project it. A
/// request is shared across worker threads, so two captures can race to
/// bind: the loser's basis is discarded and the bound basis is projected —
/// it is the request's snapshot authority even when the workspace moved
/// between the two captures.
fn bind_and_project_route_analysis_inputs(
    ctx: &crate::request_context::RequestContext,
    basis: InputBasis,
) -> RouteAnalysisInputs {
    match ctx.bind_committed_input(basis) {
        Ok(()) => project_admitted_route_analysis_inputs(
            ctx.committed_input()
                .expect("request bind stored the route-analysis basis"),
        ),
        Err(rejected) => {
            drop(rejected);
            let bound = ctx
                .committed_input()
                .expect("failed bind implies a stored request basis");
            project_admitted_route_analysis_inputs(bound)
        }
    }
}

fn project_admitted_route_analysis_inputs(binding: &RequestInputBinding) -> RouteAnalysisInputs {
    binding
        .admit_publication(binding.basis())
        .expect("route-analysis projection requires the bound snapshot fence");
    project_route_analysis_inputs(binding.basis())
}

/// Commit one [`InputBasis`] covering the route-analysis load wave.
#[must_use]
pub fn commit_route_analysis_basis(
    workspace: &dyn verter_workspace::WorkspaceRead,
    project_root: &str,
) -> InputBasis {
    let mut capture = RouteCapture::default();
    let trimmed_root = project_root.trim_end_matches('/');

    let pkg_path = format!("{trimmed_root}/package.json");
    capture.probe_file(workspace, pkg_path);

    let framework = detect_routing_framework(&capture.detection_inputs(), project_root);

    match framework {
        RoutingFramework::NuxtPages => {
            capture.walk_dir(workspace, &format!("{trimmed_root}/pages"));
        }
        RoutingFramework::UnpluginVueRouter => {
            let src_pages = format!("{trimmed_root}/src/pages");
            if !capture.walk_dir(workspace, &src_pages) {
                capture.walk_dir(workspace, &format!("{trimmed_root}/pages"));
            }
        }
        RoutingFramework::VueRouter | RoutingFramework::Unknown => {}
    }

    capture.walk_dir(workspace, &format!("{trimmed_root}/layouts"));

    for candidate in ROUTER_CONFIG_CANDIDATES {
        capture.probe_file(workspace, format!("{trimmed_root}/{candidate}"));
    }

    capture.commit()
}

/// Project committed observations into `RouteAnalysisInputs`.
///
/// Does not re-read the workspace. Callers that need a frozen snapshot after
/// a later mutation must pass the original basis.
pub fn project_route_analysis_inputs(basis: &InputBasis) -> RouteAnalysisInputs {
    let mut inputs = RouteAnalysisInputs::new();
    project_route_analysis_inputs_into(basis, &mut inputs);
    inputs
}

fn project_route_analysis_inputs_into(basis: &InputBasis, inputs: &mut RouteAnalysisInputs) {
    for observation in basis.observations() {
        if let Some(content) = observation.file_content() {
            inputs.insert_file(observation.canonical(), Arc::from(content));
        } else if let Some(entries) = observation.directory_entries() {
            let projected: Vec<RouteDirEntry> = entries
                .iter()
                .map(|entry| RouteDirEntry {
                    path: entry.path().to_string(),
                    is_dir: entry.is_dir(),
                })
                .collect();
            inputs.insert_directory(observation.canonical(), projected);
        }
    }
}

#[derive(Default)]
struct RouteCapture {
    keys: Vec<String>,
    observations: Vec<Observation>,
    negatives: Vec<NegativeFact>,
    directory_read_failed: bool,
}

impl RouteCapture {
    fn detection_inputs(&self) -> RouteAnalysisInputs {
        let mut inputs = RouteAnalysisInputs::new();
        for observation in &self.observations {
            if let Some(content) = observation.file_content() {
                inputs.insert_file(observation.canonical(), Arc::from(content));
            }
        }
        inputs
    }

    fn probe_file(&mut self, workspace: &dyn verter_workspace::WorkspaceRead, path: String) {
        self.keys.push(path.clone());
        match workspace.read_file(&path) {
            Some(content) => self.observations.push(Observation::file(path, content)),
            None => self.negatives.push(NegativeFact::absent(path)),
        }
    }

    /// Walk `dir`. Returns whether a directory listing was recorded.
    /// A non-directory path is probed as a file: an existing regular file
    /// commits a file observation and only a missing path commits
    /// [`NegativeFact::absent`], so the two answer distinct bases. A
    /// `read_dir` error aborts capture rather than being classified as
    /// absence.
    fn walk_dir(&mut self, workspace: &dyn verter_workspace::WorkspaceRead, dir: &str) -> bool {
        if self.directory_read_failed {
            return false;
        }
        if !workspace.is_dir(dir) {
            self.probe_file(workspace, dir.to_string());
            return false;
        }
        let entries = match workspace.read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => {
                self.directory_read_failed = true;
                return false;
            }
        };
        let projected: Vec<DirectoryEntry> = entries
            .iter()
            .map(|entry| DirectoryEntry::new(entry.path.clone(), entry.is_dir))
            .collect();
        let observation = match Observation::directory(dir, projected) {
            Ok(observation) => observation,
            Err(_) => {
                self.directory_read_failed = true;
                return false;
            }
        };
        self.keys.push(dir.to_string());
        self.observations.push(observation);
        for entry in &entries {
            if entry.is_dir {
                self.walk_dir(workspace, &entry.path);
            } else {
                self.probe_file(workspace, entry.path.clone());
            }
        }
        true
    }

    fn commit(self) -> InputBasis {
        if self.directory_read_failed {
            panic!("route-analysis directory read failed; not classified as absent");
        }
        InputBasis::commit(
            LoadWave::from_keys(self.keys),
            self.observations,
            self.negatives,
        )
        .expect("route-analysis capture mixed a canonical during one producer walk")
    }
}

#[cfg(test)]
#[path = "route_analysis_inputs_tests.rs"]
mod route_analysis_inputs_tests;

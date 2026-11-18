#![allow(dead_code, unused_variables, unused_imports)]
//! Projects to check and the order to check them in.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use verter_session::semantic_capability::ServingLease;
use verter_session_query::resolution::path_utils::path_is_carrier;
use verter_session_query::resolution::ProjectId;
use verter_span::path::{canonicalize_path, CanonicalPath};
use verter_workspace::config::{
    load_project_reference_targets, parse_tsconfig_json_with_dependencies,
    resolve_tsconfig_reference, TsConfigDependencies,
};
use verter_workspace::snapshot_builder::build_selected_projects_snapshot;
use verter_workspace::workspace_snapshot::{ProjectPayload, SnapshotGeneration};
use verter_workspace::{FilesystemOptions, FilesystemWorkspace, PublishedRoot, WorkspaceRead};

/// `Project` is `-p`: one project. `Build` is `-b`: the roots and every
/// project they reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMode {
    Project,
    Build,
}

#[cfg(test)]
#[path = "project_graph_tests.rs"]
mod tests;

/// One project: one tsconfig, checked as one engine program with that
/// config's own options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverProject {
    /// Canonical tsconfig path.
    pub config: PathBuf,
    /// Directory of the tsconfig.
    pub root: PathBuf,
    /// Carriers selected by this config's own `files` / `include` /
    /// `exclude`, with `extends` followed. Sorted.
    pub carriers: Vec<PathBuf>,
    /// The published membership, including specs for files created after load.
    pub membership: verter_session_query::resolution::ConfiguredMembership,
    /// Realpath identities captured by the config owner's existing reads.
    pub config_dependencies: BTreeSet<PathBuf>,
    /// Lexical reference targets the config owner probed and could not find.
    pub missing_config_probes: BTreeSet<PathBuf>,
    /// Canonical tsconfig paths this project references, in authored order.
    pub references: Vec<PathBuf>,
    /// True for `files: []` with no `include`: nothing to check here.
    pub solution_only: bool,
    /// Custom-element prefixes read from the config, if any.
    pub custom_elements: Vec<String>,
    /// True when the project's own resolved options do not set `noEmit`.
    pub emits: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectGraphError {
    /// The root config cannot be read or parsed. Exit status 2.
    UnreadableRoot { config: PathBuf, detail: String },
    /// A referenced config is missing. Reported as TS6053 against the
    /// referencing config in either mode. The graph owns this diagnostic;
    /// consumers deduplicate an engine copy through DiagnosticDeduplicationKey.
    /// The run continues with the other projects.
    MissingReference { from: PathBuf, to: PathBuf },
    /// References form a cycle. Reported as TS6202. Each project of the
    /// cycle is still checked once, in authored order.
    Cycle { path: Vec<PathBuf> },
}

/// Projects in dependency order: a referenced project precedes its
/// referrers, ties keep authored order, each project appears once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectGraph {
    pub projects: Vec<DriverProject>,
    pub problems: Vec<ProjectGraphError>,
}

/// A graph and the workspace publication from that same load ordinal.
pub(crate) struct GraphLoad {
    pub(crate) graph: ProjectGraph,
    pub(crate) published: Arc<PublishedRoot>,
    pub(crate) lease: ServingLease,
}

impl GraphLoad {
    pub(crate) fn project_id(&self, config: &Path) -> Option<ProjectId> {
        let config = CanonicalPath::new(config.to_str()?);
        self.published
            .snapshot
            .projects
            .iter()
            .find_map(|project| match &project.payload {
                ProjectPayload::Configured { tsconfig_path, .. } if tsconfig_path == &config => {
                    Some(project.id)
                }
                _ => None,
            })
    }
}

impl ProjectGraph {
    /// Load the graph.
    /// - `Build`: `configs` are the roots; references are followed transitively.
    ///
    /// Targets may name a config file or a directory containing `tsconfig.json`.
    /// - `Project`: only `configs[0]` is loaded; its direct references are
    ///   validated without traversal. A solution-only config
    ///   yields that one project with nothing to check, as the TypeScript
    ///   compiler does.
    /// - A referenced solution-only config is followed to its own references.
    ///
    /// Config parsing goes through `verter_workspace::config`
    /// (`parse_tsconfig_json`, `load_project_membership`,
    /// `load_project_references`, `resolve_tsconfig_extends`), not through
    /// this crate's `tsconfig.rs`.
    pub fn load(mode: BuildMode, configs: &[PathBuf]) -> Result<Self, ProjectGraphError> {
        let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
        Ok(Self::load_published(&ws, mode, configs, 1)?.graph)
    }

    /// Load through the run's retained workspace and publish all selected
    /// projects atomically before projecting any driver-facing metadata.
    pub(crate) fn load_published(
        ws: &Arc<FilesystemWorkspace>,
        mode: BuildMode,
        configs: &[PathBuf],
        ordinal: u64,
    ) -> Result<GraphLoad, ProjectGraphError> {
        let mut roots = Vec::new();
        let mut config_dependencies = BTreeMap::new();
        for config in configs.iter().take(if mode == BuildMode::Project {
            1
        } else {
            configs.len()
        }) {
            let canonical = config
                .to_str()
                .and_then(|path| resolve_tsconfig_reference(ws.as_ref(), ".", path))
                .and_then(|path| ws.realpath(&path))
                .map(|path| canonicalize_path(&path))
                .ok_or_else(|| ProjectGraphError::UnreadableRoot {
                    config: config.clone(),
                    detail: "cannot resolve the root config".into(),
                })?;
            let (parsed, dependencies) =
                parse_tsconfig_json_with_dependencies(ws.as_ref(), &canonical);
            config_dependencies.insert(canonical.clone(), dependencies);
            if parsed.is_none() {
                return Err(ProjectGraphError::UnreadableRoot {
                    config: PathBuf::from(canonical),
                    detail: "cannot read or parse the root config".into(),
                });
            }
            roots.push(canonical);
        }
        let Some(first) = roots.first() else {
            return Err(ProjectGraphError::UnreadableRoot {
                config: PathBuf::new(),
                detail: "no root config was selected".into(),
            });
        };
        let mut workspace_root = Path::new(first)
            .parent()
            .unwrap_or(Path::new(first))
            .to_path_buf();
        for root in &roots[1..] {
            while !Path::new(root).starts_with(&workspace_root) {
                if !workspace_root.pop() {
                    return Err(ProjectGraphError::UnreadableRoot {
                        config: PathBuf::from(root),
                        detail: "root configs have no common workspace directory".into(),
                    });
                }
            }
        }

        let mut ordered = Vec::new();
        let mut stack = Vec::new();
        let mut problems = Vec::new();
        for root in roots {
            visit_config(
                ws,
                mode,
                root,
                &mut ordered,
                &mut stack,
                &mut problems,
                &mut config_dependencies,
            );
        }
        let snapshot = build_selected_projects_snapshot(
            ws.as_ref(),
            &ordered,
            workspace_root.to_str().expect("canonical root is UTF-8"),
            SnapshotGeneration(ordinal),
        );
        let mut publication = PublishedRoot::new_vfs_only(Arc::new(snapshot));
        publication.ownership_ready = true;
        ws.publish_snapshot(publication);
        let published = ws
            .load_published()
            .expect("workspace has a publication after publish_snapshot");

        let projects = ordered.iter().map(|config| {
            let project = published.snapshot.projects.iter().find(|project| matches!(
                &project.payload, ProjectPayload::Configured { tsconfig_path, .. } if tsconfig_path.as_str() == config
            )).expect("selected config has a configured snapshot project");
            let ProjectPayload::Configured { membership, compiler_options, references, .. } = &project.payload else {
                unreachable!("selected config is configured");
            };
            let mut carriers: Vec<PathBuf> = membership.materialized_files.iter()
                .filter(|file| path_is_carrier(file.as_str()))
                .map(|file| PathBuf::from(file.as_str())).collect();
            carriers.sort();
            DriverProject {
                config: PathBuf::from(config),
                root: PathBuf::from(project.root.as_str()),
                carriers,
                membership: membership.clone(),
                config_dependencies: config_dependencies.get(config).into_iter().flat_map(|deps| &deps.realpaths).map(PathBuf::from).collect(),
                missing_config_probes: config_dependencies.get(config).into_iter().flat_map(|deps| &deps.missing_probes).map(PathBuf::from).collect(),
                references: references.iter().map(|path| PathBuf::from(path.as_str())).collect(),
                solution_only: membership.spec.files.is_empty() && membership.spec.include.is_empty(),
                custom_elements: Vec::new(),
                emits: !compiler_options.emit.no_emit,
            }
        }).collect();
        Ok(GraphLoad {
            graph: Self { projects, problems },
            published,
            lease: ServingLease::new(ordinal),
        })
    }
    #[must_use]
    pub fn carrier_count(&self) -> usize {
        self.projects
            .iter()
            .map(|project| project.carriers.len())
            .sum()
    }
}

/// Depth-first post-order; both visitation and the cycle path preserve authored order.
fn visit_config(
    ws: &Arc<FilesystemWorkspace>,
    mode: BuildMode,
    config: String,
    ordered: &mut Vec<String>,
    stack: &mut Vec<String>,
    problems: &mut Vec<ProjectGraphError>,
    config_dependencies: &mut BTreeMap<String, TsConfigDependencies>,
) {
    if let Some(start) = stack.iter().position(|path| path == &config) {
        let mut path: Vec<PathBuf> = stack[start..].iter().map(PathBuf::from).collect();
        path.push(PathBuf::from(config));
        problems.push(ProjectGraphError::Cycle { path });
        return;
    }
    if ordered.contains(&config) {
        return;
    }
    stack.push(config.clone());
    for target in load_project_reference_targets(ws.as_ref(), &config) {
        let resolved = target.resolved.clone();
        let readable = resolved.as_ref().is_some_and(|path| {
            let (parsed, dependencies) = parse_tsconfig_json_with_dependencies(ws.as_ref(), path);
            config_dependencies.insert(path.clone(), dependencies);
            parsed.is_some()
        });
        match resolved {
            Some(path) if readable => {
                if mode == BuildMode::Build {
                    visit_config(
                        ws,
                        mode,
                        path,
                        ordered,
                        stack,
                        problems,
                        config_dependencies,
                    );
                }
            }
            _ => {
                let authored = Path::new(&config)
                    .parent()
                    .expect("config has a parent")
                    .join(&target.authored);
                let to = target.resolved.map(PathBuf::from).unwrap_or_else(|| {
                    PathBuf::from(canonicalize_path(&authored.to_string_lossy()))
                });
                problems.push(ProjectGraphError::MissingReference {
                    from: PathBuf::from(&config),
                    to,
                });
            }
        }
    }
    stack.pop();
    ordered.push(config);
}

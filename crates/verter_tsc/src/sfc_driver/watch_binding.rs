//! Which projects and carriers a changed path makes stale.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;
use std::path::{Path, PathBuf};
use verter_session_query::resolution::path_utils::path_is_carrier;
use verter_session_query::resolution::{ConfiguredMembership, StaticMembershipSpec};
use verter_span::path::{canonicalize_path, CanonicalPath};

use super::project_graph::{DriverProject, ProjectGraph};

/// What to redo after a set of paths changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recheck {
    /// Configs of the projects whose engine program must be rebuilt.
    pub projects: Vec<PathBuf>,
    /// Carriers to re-project, as (project config, carrier).
    pub carriers: Vec<(PathBuf, PathBuf)>,
    /// True when a config dependency or carrier membership changed and the
    /// graph must be reloaded.
    pub reload_graph: bool,
    /// The changed paths that made anything above stale, sorted. They are
    /// what the workspace and the engine programs are told changed.
    pub changed: Vec<PathBuf>,
}

/// The graph and the read sets recorded per carrier and per project program,
/// all private.
#[derive(Clone)]
pub struct ProjectWatchProjectionBinding {
    projects: Vec<DriverProject>,
    reads: BTreeMap<(PathBuf, PathBuf), BTreeSet<PathBuf>>,
    program_reads: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
}

impl ProjectWatchProjectionBinding {
    pub fn new(graph: &ProjectGraph) -> Self {
        Self {
            projects: graph.projects.clone(),
            reads: BTreeMap::new(),
            program_reads: BTreeMap::new(),
        }
    }
    /// Replace the host paths a project's engine program reads from disk:
    /// imported files, failed-lookup probes and listed directories, including
    /// ones outside the project's root and configured membership. A change to
    /// any of them makes the project stale.
    pub fn record_program_reads(&mut self, project: &Path, paths: &[PathBuf]) {
        let Some(project) = normalized_path(project) else {
            return;
        };
        self.program_reads.insert(
            project,
            paths
                .iter()
                .filter_map(|path| normalized_path(path))
                .collect(),
        );
    }
    /// Every host path a project program reads, as recorded.
    pub fn program_reads(&self) -> impl Iterator<Item = &PathBuf> {
        self.program_reads.values().flatten()
    }
    /// Replace the last successful projection's external read set.
    /// Paths carry their source's realpath identity; normalize transport
    /// spellings here without probing disk, so deletion events keep that identity.
    pub fn record_read(&mut self, project: &Path, carrier: &Path, external_sources: &[PathBuf]) {
        let (Some(project), Some(carrier)) = (normalized_path(project), normalized_path(carrier))
        else {
            return;
        };
        self.reads.insert(
            (project, carrier),
            external_sources
                .iter()
                .filter_map(|path| normalized_path(path))
                .collect(),
        );
    }
    /// What a change source must report for the current graph and read sets.
    #[must_use]
    pub fn watched_paths(&self) -> WatchedPaths {
        let mut watched = WatchedPaths::default();
        for project in &self.projects {
            watched.add_file(project.config.clone());
            for dependency in &project.config_dependencies {
                watched.add_file(dependency.clone());
            }
            for probe in &project.missing_config_probes {
                watched.probes.insert(probe.clone());
                watched.add_directory(probe.clone(), true);
            }
            for carrier in &project.carriers {
                watched.add_file(carrier.clone());
            }
            let spec = &project.membership.spec;
            for file in project
                .membership
                .materialized_files
                .iter()
                .chain(&spec.files)
            {
                watched.add_file(PathBuf::from(file.as_str()));
            }
            for root in spec.include.iter().filter_map(|glob| glob.walk_root()) {
                watched.add_directory(PathBuf::from(root.as_str()), true);
            }
            watched
                .memberships
                .push((project.config.clone(), project.membership.clone()));
        }
        for ((_, carrier), reads) in &self.reads {
            watched.add_file(carrier.clone());
            for read in reads {
                watched.add_file(read.clone());
            }
        }
        // Engine reads include probes of ancestor directories up to the
        // drive root; each is watched through its parent, never recursively.
        for read in self.program_reads() {
            watched.add_file(read.clone());
        }
        watched
    }
    #[must_use]
    pub fn recheck_for(&self, changed: &[PathBuf]) -> Recheck {
        let changed: BTreeSet<PathBuf> = changed
            .iter()
            .filter_map(|path| normalized_path(path))
            .collect();
        let mut stale = BTreeSet::new();
        let mut carriers = BTreeSet::new();
        let mut reload_graph = false;
        let mut relevant = BTreeSet::new();
        for project in &self.projects {
            for carrier in &project.carriers {
                if changed.contains(carrier) {
                    carriers.insert((project.config.clone(), carrier.clone()));
                    stale.insert(project.config.clone());
                    relevant.insert(carrier.clone());
                }
            }
            for ((config, carrier), reads) in &self.reads {
                if config != &project.config {
                    continue;
                }
                let read = std::iter::once(carrier)
                    .chain(reads)
                    .filter(|path| changed.contains(*path))
                    .cloned()
                    .collect::<Vec<_>>();
                if !read.is_empty() {
                    carriers.insert((config.clone(), carrier.clone()));
                    stale.insert(config.clone());
                    relevant.extend(read);
                }
            }
            if let Some(reads) = self.program_reads.get(&project.config) {
                let read = changed
                    .iter()
                    .filter(|path| reads.contains(*path))
                    .cloned()
                    .collect::<Vec<_>>();
                if !read.is_empty() {
                    stale.insert(project.config.clone());
                    relevant.extend(read);
                }
            }
            for path in &changed {
                let mut hit = false;
                if path == &project.config || project.config_dependencies.contains(path) {
                    reload_graph = true;
                    hit = true;
                }
                // A missing directory reference may first be reported as the
                // creation of its tsconfig, without a separate directory event.
                if project
                    .missing_config_probes
                    .iter()
                    .any(|probe| path.starts_with(probe))
                {
                    reload_graph = true;
                    stale.insert(project.config.clone());
                    hit = true;
                }
                let member = path
                    .to_str()
                    .is_some_and(|path| project.membership.contains(&CanonicalPath::new(path)));
                if member && path.to_str().is_some_and(path_is_carrier) {
                    carriers.insert((project.config.clone(), path.clone()));
                    // A new checking path changes the graph's carrier inventory
                    // and the owning engine program's virtual-config identity.
                    if project.carriers.binary_search(path).is_err() {
                        reload_graph = true;
                    }
                }
                if path.starts_with(&project.root) || member {
                    stale.insert(project.config.clone());
                    hit = true;
                }
                if hit {
                    relevant.insert(path.clone());
                }
            }
        }
        // A cycle need not be in topological order; compute the full closure.
        loop {
            let before = stale.len();
            for project in &self.projects {
                if project
                    .references
                    .iter()
                    .any(|reference| stale.contains(reference))
                {
                    stale.insert(project.config.clone());
                }
            }
            if stale.len() == before {
                break;
            }
        }
        Recheck {
            projects: self
                .projects
                .iter()
                .filter(|project| stale.contains(&project.config))
                .map(|project| project.config.clone())
                .collect(),
            carriers: self
                .projects
                .iter()
                .flat_map(|project| {
                    carriers
                        .iter()
                        .filter(|(config, _)| config == &project.config)
                        .cloned()
                })
                .collect(),
            reload_graph,
            changed: relevant.into_iter().collect(),
        }
    }
}

/// The paths a change source reports, taken from the graph and the recorded
/// read sets, program reads included. Membership decides which new files count, so a path under an
/// excluded directory is not reported unless a read set names it.
#[derive(Debug, Clone, Default)]
pub struct WatchedPaths {
    /// Configs, config dependencies, carriers, materialized members and
    /// recorded external reads.
    files: BTreeSet<PathBuf>,
    /// Missing reference probes: the probe itself and anything created below it.
    probes: BTreeSet<PathBuf>,
    /// Each project's config with its membership.
    memberships: Vec<(PathBuf, ConfiguredMembership)>,
    /// Directories whose events can carry a reported path, with whether their
    /// whole subtree is needed (true) or only their direct entries (false).
    directories: BTreeMap<PathBuf, bool>,
}

impl WatchedPaths {
    /// Whether a change to the canonical `path` is reported.
    #[must_use]
    pub fn reports(&self, path: &Path) -> bool {
        self.files.contains(path)
            || self.probes.iter().any(|probe| path.starts_with(probe))
            || path.to_str().is_some_and(|text| {
                let canonical = CanonicalPath::new(text);
                self.memberships
                    .iter()
                    .any(|(_, membership)| membership.contains(&canonical))
            })
    }

    /// Named paths strictly below the canonical directory `path`. Removing,
    /// renaming or moving in such a directory changes them, and platforms
    /// report only the directory.
    pub fn named_below<'a>(&'a self, path: &'a Path) -> impl Iterator<Item = &'a Path> + 'a {
        self.files
            .range::<Path, _>((Bound::Excluded(path), Bound::Unbounded))
            .take_while(move |file| file.starts_with(path))
            .map(PathBuf::as_path)
    }

    /// Configs of the projects whose member walk reaches the canonical
    /// directory `path`. A directory that arrives whole (moved in, or created
    /// with its files before a watch reached it) brings files no event names,
    /// so those projects' membership must be walked again: reporting the
    /// config makes the recheck reload the graph.
    pub fn configs_reaching<'a>(&'a self, path: &'a Path) -> impl Iterator<Item = &'a Path> + 'a {
        self.memberships
            .iter()
            .filter(move |(_, membership)| walk_reaches(&membership.spec, path))
            .map(|(config, _)| config.as_path())
    }

    /// Whether a directory the source must observe is the canonical `path`
    /// or lies below it, so a change to `path` can move a watch.
    #[must_use]
    pub fn needs_directory_within(&self, path: &Path) -> bool {
        self.directories
            .range::<Path, _>((Bound::Included(path), Bound::Unbounded))
            .next()
            .is_some_and(|(directory, _)| directory.starts_with(path))
    }

    /// Every named path and missing-reference probe: what a source reports
    /// when the platform lost events. The configs among them make the
    /// recheck reload the graph, which finds files created meanwhile.
    pub fn named_paths(&self) -> impl Iterator<Item = &Path> {
        self.files.iter().chain(&self.probes).map(PathBuf::as_path)
    }

    /// Directories a source must observe, in path order, with whether their
    /// whole subtree is needed. A directory that does not exist yet is listed
    /// as it is; the source watches its nearest existing ancestor instead.
    pub fn directories(&self) -> impl Iterator<Item = (&Path, bool)> {
        self.directories
            .iter()
            .map(|(directory, recursive)| (directory.as_path(), *recursive))
    }

    fn add_file(&mut self, file: PathBuf) {
        if let Some(parent) = file.parent() {
            self.add_directory(parent.to_path_buf(), false);
        }
        self.files.insert(file);
    }

    fn add_directory(&mut self, directory: PathBuf, recursive: bool) {
        *self.directories.entry(directory).or_insert(false) |= recursive;
    }
}

/// Whether the member walk of `spec` reaches directory `path`: it holds a
/// `files` entry or an include walk root the walk descends into, or it lies
/// below a walk root and the walk descends into it, the root and every
/// directory between them.
fn walk_reaches(spec: &StaticMembershipSpec, path: &Path) -> bool {
    let descends = |directory: &Path| {
        directory
            .to_str()
            .is_some_and(|text| spec.descends_into(&CanonicalPath::new(text)))
    };
    spec.files
        .iter()
        .any(|file| Path::new(file.as_str()).starts_with(path))
        || spec
            .include
            .iter()
            .filter_map(|glob| glob.walk_root())
            .any(|root| {
                let root = Path::new(root.as_str());
                // The walk starts at the root, so an excluded root is never walked.
                if root.starts_with(path) {
                    return descends(root);
                }
                path.starts_with(root)
                    && path
                        .ancestors()
                        .take_while(|directory| directory.starts_with(root))
                        .all(&descends)
            })
}

fn normalized_path(path: &Path) -> Option<PathBuf> {
    path.to_str()
        .map(|path| PathBuf::from(canonicalize_path(path)))
}

#[cfg(test)]
#[path = "watch_binding_tests.rs"]
mod tests;

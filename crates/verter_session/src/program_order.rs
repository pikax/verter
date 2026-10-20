//! The program file order authority: the order a program lists its library
//! and source files in, which decides which library declares a global and
//! orders the declarations of a symbol merged across files (global
//! interfaces and namespaces, `declare global`, module augmentations).
//!
//! The order is TypeScript's: the `files` roots as listed, then the
//! `include` matches (by authored entry, in directory-walk order), each root
//! preceded by the files it pulls in — `/// <reference path>` and
//! `/// <reference types>` targets, then its imports — first visit wins.
//! A module augmentation's name pulls nothing in. The libraries the program
//! loads — its `lib` selection (or its target's default library), every
//! `/// <reference lib>` a program file carries and everything those
//! libraries reference — are listed before every program file, by
//! TypeScript's library priority. A registered ambient library only makes
//! a library's bytes available; whether and where the program loads it is
//! decided here. A host that registers none of the libraries its
//! configuration names supplies its own library environment: every library
//! it registers is a root.
//!
//! Configured program discovery settles before population lookup, including
//! empty and singleton populations, and after batched workspace publication.
//! It reads shallow reference/contribution inventories and lowers no bodies.
//! The same immutable composition supplies imported members' project context,
//! captured with source/project roots, and declaration ranks. Unchanged file
//! edges are reused when roots grow. Results that depend on ordering observe
//! [`FactKey::ProgramFileOrder`](verter_session_query::facts::FactKey::ProgramFileOrder),
//! so a reference edit in a non-contributor can invalidate a changed winner.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_session_query::analysis::types::Hash16;
use verter_session_query::inputs::program_order::{
    library_file_name, needs_program_walk, orders_across_files, ProgramFileOrder,
};
use verter_session_query::resolution::path_utils::{join_paths, parent_dir};
use verter_session_query::resolution::{
    AmbientSymbolHit, ProjectId, ProjectStableKey, SemanticCompilerOptions,
};
use verter_workspace::{
    AmbientLibsByProject, CanonicalPath, ProjectPayload, WorkspaceAccess, WorkspaceSnapshot,
};

/// The workspace state a published order was built from. A state with
/// another stamp may still describe the same program: see
/// [`OrderInputs::unchanged`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProgramOrderStamp {
    authority: verter_workspace::published_state::WorkspaceAuthority,
    snapshot_generation: u64,
    content_generation: u64,
    resolution_generation: u64,
}

impl ProgramOrderStamp {
    /// The live workspace's stamp, `None` before a snapshot is published.
    pub(crate) fn live(workspace: &dyn WorkspaceAccess) -> Option<Self> {
        let root = workspace.published_root()?;
        Some(Self::from_root(workspace, &root))
    }

    fn from_root(
        workspace: &dyn WorkspaceAccess,
        root: &verter_workspace::published_state::PublishedRoot,
    ) -> Self {
        Self {
            authority: root.authority(),
            snapshot_generation: root.snapshot.generation.0,
            content_generation: workspace.content_generation(),
            resolution_generation: workspace.resolution_fact_generation(),
        }
    }
}

/// The set of files a program's `include` roots are matched against —
/// every snapshot member and open document — as an order-independent sum
/// of their hashes and their count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CandidateSet {
    sum: u64,
    count: usize,
}

impl CandidateSet {
    fn of(canonicals: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        use std::hash::{BuildHasher, BuildHasherDefault};
        let hasher = BuildHasherDefault::<rustc_hash::FxHasher>::default();
        let mut set = Self { sum: 0, count: 0 };
        for canonical in canonicals {
            set.sum = set.sum.wrapping_add(hasher.hash_one(canonical.as_ref()));
            set.count += 1;
        }
        set
    }

    fn live(workspace: &dyn WorkspaceAccess) -> Self {
        Self::of(
            workspace
                .snapshot_canonicals()
                .into_iter()
                .chain(workspace.overlay_canonicals()),
        )
    }
}

/// Everything a published order was read from: the workspace state, the
/// registered libraries, the files `include` matched against, the roots,
/// and each file the walk read with the files it pulls in.
struct OrderInputs {
    stamp: ProgramOrderStamp,
    registry: Arc<AmbientLibsByProject>,
    candidates: CandidateSet,
    roots: Vec<Arc<str>>,
    /// Each file the walk read, with the files it pulls in (`None`: the
    /// program cannot load it).
    pulled: FxHashMap<Arc<str>, Option<Vec<Arc<str>>>>,
    /// Paths a `/// <reference path>` looked for, found or not.
    probes: Vec<Arc<str>>,
}

impl OrderInputs {
    /// Whether the program these inputs describe has the same roots in the
    /// live workspace: the configuration, the resolution world, the
    /// registered libraries and the candidate files are the same ones.
    fn same_roots(&self, workspace: &dyn WorkspaceAccess, live: ProgramOrderStamp) -> bool {
        self.same_resolution(workspace, live) && self.candidates == CandidateSet::live(workspace)
    }

    fn same_resolution(&self, workspace: &dyn WorkspaceAccess, live: ProgramOrderStamp) -> bool {
        self.stamp.authority == live.authority
            && self.stamp.snapshot_generation == live.snapshot_generation
            && self.stamp.resolution_generation == live.resolution_generation
            && Arc::ptr_eq(&self.registry, &workspace.ambient_libs_view())
            && !self.probes.iter().any(|probe| self.moved(workspace, probe))
    }

    /// Whether `file`'s content changed after these inputs were read.
    fn moved(&self, workspace: &dyn WorkspaceAccess, file: &str) -> bool {
        workspace.last_content_transition_generation(file) > self.stamp.content_generation
    }

    /// Whether the live workspace lists exactly this program: its roots
    /// are the same and no file the walk read changed since.
    fn unchanged(&self, workspace: &dyn WorkspaceAccess, live: ProgramOrderStamp) -> bool {
        self.same_roots(workspace, live)
            && !self.pulled.keys().any(|file| self.moved(workspace, file))
    }
}

/// A shallow inclusion edge set ingested once per source version. Files
/// without references are independent of the resolution/library world.
#[derive(Clone)]
struct DiscoveredFile {
    transition: u64,
    observed_content: u64,
    published: Arc<verter_workspace::published_state::PublishedRoot>,
    resolution: u64,
    registry: Arc<AmbientLibsByProject>,
    reference_free: bool,
    files: Option<Vec<Arc<str>>>,
    probes: Vec<Arc<str>>,
}

impl DiscoveredFile {
    fn current(&self, workspace: &dyn WorkspaceAccess, file: &str) -> bool {
        self.transition == workspace.last_content_transition_generation(file)
            && workspace
                .published_root()
                .is_some_and(|root| Arc::ptr_eq(&root, &self.published))
            && (self.reference_free
                || (self.resolution == workspace.resolution_fact_generation()
                    && Arc::ptr_eq(&self.registry, &workspace.ambient_libs_view())
                    && self.probes.iter().all(|probe| {
                        workspace.last_content_transition_generation(probe) <= self.observed_content
                    })))
    }
}

/// One program's published order and what it was read from.
#[derive(Clone)]
struct PublishedOrder {
    order: Arc<ProgramFileOrder>,
    inputs: Arc<OrderInputs>,
}

/// The state a project's library order was built from: its configuration
/// and the registered libraries (a registration replaces the registry).
#[derive(Clone)]
struct LibraryStamp {
    authority: verter_workspace::published_state::WorkspaceAuthority,
    snapshot_generation: u64,
    registry: Arc<AmbientLibsByProject>,
}

impl LibraryStamp {
    fn matches(&self, other: &Self) -> bool {
        self.authority == other.authority
            && self.snapshot_generation == other.snapshot_generation
            && Arc::ptr_eq(&self.registry, &other.registry)
    }
}

type DiscoveredFiles = FxHashMap<(Arc<str>, Arc<str>), DiscoveredFile>;

/// Published program orders, one per program, each with the inputs it was
/// read from, and each project's library order.
#[derive(Default)]
pub(crate) struct ProgramOrderIndex {
    published: parking_lot::RwLock<Arc<FxHashMap<Arc<str>, PublishedOrder>>>,
    discovered: parking_lot::RwLock<DiscoveredFiles>,
    libraries:
        parking_lot::RwLock<FxHashMap<ProjectStableKey, (LibraryStamp, Arc<ProgramFileOrder>)>>,
    /// Files whose references a walk read afresh rather than reused.
    #[cfg(test)]
    reference_reads: std::sync::atomic::AtomicU64,
}

impl std::fmt::Debug for ProgramOrderIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgramOrderIndex")
            .field("published", &self.published.read().len())
            .field("libraries", &self.libraries.read().len())
            .finish()
    }
}

impl ProgramOrderIndex {
    /// `program`'s published order, while the live workspace still lists
    /// exactly it: published for `live`, or for an earlier state none of
    /// whose inputs moved since — then restamped for `live`, so the check
    /// runs once per workspace state.
    fn current(
        &self,
        program: &str,
        live: ProgramOrderStamp,
        workspace: &dyn WorkspaceAccess,
    ) -> Option<Arc<ProgramFileOrder>> {
        let inputs = {
            let published = self.published.read();
            let entry = published.get(program)?;
            if entry.inputs.stamp == live {
                return Some(Arc::clone(&entry.order));
            }
            Arc::clone(&entry.inputs)
        };
        let mut refreshed_probes = inputs.probes.clone();
        if !inputs.unchanged(workspace, live) {
            if !inputs.same_roots(workspace, live) {
                return None;
            }
            let discovered = self.discovered.read();
            for (file, previous) in &inputs.pulled {
                if inputs.moved(workspace, file) {
                    let entry = discovered.get(&(Arc::from(program), Arc::clone(file)))?;
                    if !entry.current(workspace, file) || &entry.files != previous {
                        return None;
                    }
                    refreshed_probes.extend(entry.probes.iter().cloned());
                }
            }
        }
        let mut published = self.published.write();
        let entry = Arc::make_mut(&mut published).get_mut(program)?;
        if !Arc::ptr_eq(&entry.inputs, &inputs) {
            return (entry.inputs.stamp == live).then(|| Arc::clone(&entry.order));
        }
        entry.inputs = Arc::new(OrderInputs {
            stamp: live,
            registry: Arc::clone(&inputs.registry),
            candidates: inputs.candidates,
            roots: inputs.roots.clone(),
            pulled: inputs.pulled.clone(),
            probes: refreshed_probes,
        });
        Some(Arc::clone(&entry.order))
    }

    pub(crate) fn capture_membership(
        &self,
        workspace: &dyn WorkspaceAccess,
    ) -> ProgramMembershipRoot {
        let root = workspace.published_root();
        ProgramMembershipRoot {
            programs: Arc::clone(&self.published.read()),
            stamp: root
                .as_ref()
                .map(|root| ProgramOrderStamp::from_root(workspace, root)),
            snapshot: root.map(|root| Arc::clone(&root.snapshot)),
        }
    }

    /// The inputs `program`'s published order was read from.
    fn inputs(&self, program: &str) -> Option<Arc<OrderInputs>> {
        self.published
            .read()
            .get(program)
            .map(|entry| Arc::clone(&entry.inputs))
    }

    /// Publish `order`, read from `inputs`. An order with the published
    /// fingerprint keeps the published instance. Returns the order to
    /// serve and the previously published fingerprint.
    fn publish(
        &self,
        order: Arc<ProgramFileOrder>,
        inputs: OrderInputs,
    ) -> (Arc<ProgramFileOrder>, Option<Hash16>) {
        let Some(program) = order.program().cloned() else {
            return (order, None);
        };
        let mut published = self.published.write();
        let previous = published
            .get(&program)
            .map(|entry| Arc::clone(&entry.order));
        let order = match &previous {
            Some(previous) if previous.fingerprint() == order.fingerprint() => Arc::clone(previous),
            _ => order,
        };
        Arc::make_mut(&mut published).insert(
            program,
            PublishedOrder {
                order: Arc::clone(&order),
                inputs: Arc::new(inputs),
            },
        );
        (order, previous.map(|previous| previous.fingerprint()))
    }

    /// The fingerprint of `program`'s order while the live workspace still
    /// lists exactly it (see [`Self::current`]). `None` once a file it read,
    /// its configuration, its candidate files or the resolution world
    /// moved: the order may differ, and the next read that needs it
    /// rebuilds and republishes it, re-reading only the files that changed.
    pub(crate) fn published_fingerprint(
        &self,
        program: &str,
        live: ProgramOrderStamp,
        workspace: &dyn WorkspaceAccess,
    ) -> Option<Hash16> {
        self.current(program, live, workspace)
            .map(|order| order.fingerprint())
    }

    fn library_order(
        &self,
        project: ProjectStableKey,
        stamp: &LibraryStamp,
    ) -> Option<Arc<ProgramFileOrder>> {
        let libraries = self.libraries.read();
        let (at, order) = libraries.get(&project)?;
        at.matches(stamp).then(|| Arc::clone(order))
    }

    fn publish_library_order(
        &self,
        project: ProjectStableKey,
        stamp: LibraryStamp,
        order: &Arc<ProgramFileOrder>,
    ) {
        self.libraries
            .write()
            .insert(project, (stamp, Arc::clone(order)));
    }

    /// Files whose references a walk read afresh.
    #[cfg(test)]
    pub(crate) fn reference_reads(&self) -> u64 {
        self.reference_reads
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    #[cfg(test)]
    fn note_reference_read(&self) {
        self.reference_reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    #[cfg(not(test))]
    fn note_reference_read(&self) {}
}

/// Immutable program membership captured beside the source/project roots.
/// It shares the same compositions used for contributor ordering, and costs
/// a fixed number of Arc clones to capture, independent of the program's size.
#[derive(Clone, Default)]
pub(crate) struct ProgramMembershipRoot {
    programs: Arc<FxHashMap<Arc<str>, PublishedOrder>>,
    stamp: Option<ProgramOrderStamp>,
    snapshot: Option<Arc<WorkspaceSnapshot>>,
}

impl ProgramMembershipRoot {
    pub(crate) fn project_for_member(
        &self,
        canonical: &str,
        snapshot: &WorkspaceSnapshot,
    ) -> Option<ProjectId> {
        if !self
            .snapshot
            .as_ref()
            .is_some_and(|captured| std::ptr::eq(captured.as_ref(), snapshot))
        {
            return None;
        }
        snapshot.projects.iter().find_map(|project| {
            let ProjectPayload::Configured { tsconfig_path, .. } = &project.payload else {
                return None;
            };
            let entry = self.programs.get(tsconfig_path.as_str())?;
            (self.stamp == Some(entry.inputs.stamp)
                && entry.inputs.stamp.snapshot_generation == snapshot.generation.0
                && entry.order.lists(canonical))
            .then_some(project.id)
        })
    }
}

/// The libraries a project can load: the registered ones a user file does
/// not shadow, and the roots its configuration names.
struct ProjectLibraries {
    registry: Arc<AmbientLibsByProject>,
    key: ProjectStableKey,
    /// Registered library file names a user file does not shadow.
    available: FxHashSet<Arc<str>>,
    /// The program's library roots, as ambient virtual ids.
    roots: Vec<Arc<str>>,
    /// `noLib`: the program loads no library, and a
    /// `/// <reference lib>` none.
    no_lib: bool,
}

impl ProjectLibraries {
    fn of(
        workspace: &dyn WorkspaceAccess,
        registry: &Arc<AmbientLibsByProject>,
        project: ProjectId,
        options: &SemanticCompilerOptions,
    ) -> Option<Self> {
        let key = workspace.project_stable_key(project)?;
        let mut available: FxHashSet<Arc<str>> = FxHashSet::default();
        if let Some(registered) = registry.by_project.get(&key) {
            for name in registered.libs.keys() {
                if workspace.read_ambient_lib(key, name).is_some() {
                    available.insert(Arc::clone(name));
                }
            }
        }
        let mut libraries = Self {
            registry: Arc::clone(registry),
            key,
            available,
            roots: Vec::new(),
            no_lib: options.no_lib,
        };
        if libraries.no_lib {
            return Some(libraries);
        }
        let configured: Vec<Arc<str>> = options
            .effective_lib_file_names()
            .into_iter()
            .filter_map(|name| libraries.target(workspace, name))
            .collect();
        libraries.roots = if configured.is_empty() {
            let mut registered: Vec<&Arc<str>> = libraries.available.iter().collect();
            registered.sort();
            registered
                .into_iter()
                .map(|name| workspace.ambient_virtual_canonical_id(key, name))
                .collect()
        } else {
            configured
        };
        Some(libraries)
    }

    /// The ambient virtual id of the library file `name`, when the project
    /// can load it.
    fn target(&self, workspace: &dyn WorkspaceAccess, name: &str) -> Option<Arc<str>> {
        self.available
            .contains(name)
            .then(|| workspace.ambient_virtual_canonical_id(self.key, name))
    }

    /// The library files `/// <reference lib>` names in `names` load.
    fn referenced(&self, workspace: &dyn WorkspaceAccess, names: &[String]) -> Vec<Arc<str>> {
        if self.no_lib {
            return Vec::new();
        }
        names
            .iter()
            .filter_map(|name| library_file_name(name))
            .filter_map(|file| self.target(workspace, file))
            .collect()
    }
}

impl crate::VerterHost {
    /// Settle configured program discovery before reading any symbol's
    /// population, including symbols with no known contributors. Only
    /// shallow reference/contribution inventories enter the artifact store.
    pub(crate) fn ingest_configured_programs(&self) {
        let workspace = self.workspace();
        let mut incomplete = Vec::new();
        for _ in 0..3 {
            let Some(root) = workspace.published_root() else {
                return;
            };
            incomplete.clear();
            for project in &root.snapshot.projects {
                if !matches!(project.payload, ProjectPayload::Configured { .. }) {
                    continue;
                }
                let order = self.project_program_order(&root.snapshot, project.id);
                let current = ProgramOrderStamp::live(workspace.as_ref()).and_then(|stamp| {
                    self.project_type_store()
                        .indexed()
                        .program_order_index()
                        .current(order.program()?.as_ref(), stamp, workspace.as_ref())
                });
                if current.is_none() && !order.is_empty() {
                    incomplete.push(order);
                }
            }
            if workspace
                .published_root()
                .is_some_and(|live| Arc::ptr_eq(&live.snapshot, &root.snapshot))
                && incomplete.is_empty()
            {
                return;
            }
        }
        for order in incomplete {
            verter_type_engine::resolver_core::resolver_context::observe_program_file_order(&order);
        }
    }

    /// Extend contributor discovery from changed roots/members at ingestion.
    /// A previously settled file's closure is not revisited. Ordering is built
    /// only when consumed, from these same cached shallow edge sets.
    pub(crate) fn ingest_changed_program_files(&self, files: impl IntoIterator<Item = String>) {
        let workspace = self.workspace();
        let Some(root) = workspace.published_root() else {
            return;
        };
        let index = self.project_type_store().indexed().program_order_index();
        let files: Vec<_> = files.into_iter().collect();
        for project in &root.snapshot.projects {
            let ProjectPayload::Configured {
                tsconfig_path,
                membership,
                compiler_options,
                ..
            } = &project.payload
            else {
                continue;
            };
            let owner = verter_session_query::resolution::ProjectOwnership {
                project_root: project.root.as_str().to_owned(),
                tsconfig_path: Some(tsconfig_path.as_str().to_owned()),
            };
            let registry = workspace.ambient_libs_view();
            let options = workspace
                .semantic_compiler_options_for_project(project.id)
                .unwrap_or_else(|| compiler_options.semantic.clone());
            let libraries =
                ProjectLibraries::of(workspace.as_ref(), &registry, project.id, &options);
            let mut pending: Vec<String> = files
                .iter()
                .filter(|file| {
                    membership.contains(&CanonicalPath::new(file))
                        || index.discovered.read().contains_key(&(
                            Arc::from(tsconfig_path.as_str()),
                            Arc::from(file.as_str()),
                        ))
                })
                .cloned()
                .collect();
            let mut visited = FxHashSet::default();
            while let Some(file) = pending.pop() {
                if !visited.insert(file.clone()) {
                    continue;
                }
                let mut provisional = false;
                let mut probes = Vec::new();
                let (children, fresh) = self.discovered_files(
                    workspace.as_ref(),
                    &root.snapshot,
                    libraries.as_ref(),
                    &owner,
                    compiler_options.js_is_member(),
                    &file,
                    &mut provisional,
                    &mut probes,
                );
                if fresh {
                    // Descend only into files this program has not discovered
                    // yet. A discovered child that did not change keeps its
                    // closure: an entry stale against the new publication is
                    // re-read where an order consumes it, so re-walking it
                    // here would re-read the whole import closure below every
                    // changed file on every upsert.
                    let discovered = index.discovered.read();
                    pending.extend(
                        children
                            .into_iter()
                            .flatten()
                            .filter(|child| {
                                !discovered.contains_key(&(
                                    Arc::from(tsconfig_path.as_str()),
                                    Arc::clone(child),
                                ))
                            })
                            .map(|file| file.to_string()),
                    );
                }
            }
        }
    }

    pub(crate) fn program_lists(
        &self,
        snapshot: &WorkspaceSnapshot,
        project: ProjectId,
        canonical: &str,
    ) -> Option<bool> {
        let workspace = self.workspace();
        let root = workspace.published_root()?;
        if !std::ptr::eq(root.snapshot.as_ref(), snapshot) {
            return None;
        }
        let ProjectPayload::Configured { tsconfig_path, .. } = &snapshot.project(project).payload
        else {
            return None;
        };
        let stamp = ProgramOrderStamp::live(workspace.as_ref())?;
        let order = self
            .project_type_store()
            .indexed()
            .program_order_index()
            .current(tsconfig_path.as_str(), stamp, workspace.as_ref())?;
        Some(order.lists(canonical))
    }

    pub(crate) fn project_for_canonical_in_snapshot(
        &self,
        canonical: &str,
        snapshot: &WorkspaceSnapshot,
    ) -> Option<ProjectId> {
        let owners = snapshot.owners_for_file(canonical);
        owners
            .iter()
            .copied()
            .find(|project| {
                matches!(
                    snapshot.project(*project).payload,
                    ProjectPayload::Configured { .. }
                )
            })
            .or_else(|| {
                self.project_type_store()
                    .indexed()
                    .program_order_index()
                    .capture_membership(self.workspace().as_ref())
                    .project_for_member(canonical, snapshot)
            })
            .or_else(|| owners.first().copied())
    }

    /// The project whose program orders `anchor` — the requesting file —
    /// and `ordered` (the files being ordered): `anchor`'s most specific
    /// configured project; when none owns it, the configured project of the
    /// canonically first of `ordered` one owns (a declaration file a
    /// program only imports, listed in no `files` / `include`, still orders
    /// within the program of a file that is listed); else the project that
    /// owns `anchor`. A library belongs to the project it is registered
    /// for.
    fn ordering_project(
        &self,
        workspace: &dyn WorkspaceAccess,
        snapshot: &WorkspaceSnapshot,
        anchor: &str,
        ordered: &mut dyn Iterator<Item = &str>,
    ) -> Option<ProjectId> {
        let configured = |project: &ProjectId| {
            matches!(
                snapshot.project(*project).payload,
                ProjectPayload::Configured { .. }
            )
        };
        let configured_owner = |file: &str| match registered_project(workspace, snapshot, file) {
            Some(project) => Some(project).filter(configured),
            None => snapshot
                .owners_for_file(file)
                .into_iter()
                .find(configured)
                .or_else(|| {
                    self.project_for_canonical_in_snapshot(file, snapshot)
                        .filter(configured)
                }),
        };
        configured_owner(anchor)
            .or_else(|| {
                ordered
                    .filter_map(|file| Some((file, configured_owner(file)?)))
                    .min_by(|(left, _), (right, _)| left.cmp(right))
                    .map(|(_, project)| project)
            })
            .or_else(|| {
                registered_project(workspace, snapshot, anchor)
                    .or_else(|| snapshot.owners_for_file(anchor).first().copied())
            })
    }

    /// Index the libraries `anchor`'s program loads by its configuration
    /// and the libraries' own references, so a population read after this
    /// holds them.
    pub(crate) fn settle_program_libraries(&self, anchor: &str) {
        let workspace = self.workspace();
        let Some(root) = workspace.published_root() else {
            return;
        };
        if let Some(project) = self.ordering_project(
            workspace.as_ref(),
            &root.snapshot,
            anchor,
            &mut std::iter::empty(),
        ) {
            let _ = self.library_order(&root.snapshot, project);
        }
    }

    /// The file order of the program [`Self::ordering_project`] picks for
    /// `anchor` and `ordered`: a configured project's whole program; a
    /// project without configuration its libraries alone; no project, no
    /// order.
    pub(crate) fn program_file_order(
        &self,
        anchor: &str,
        ordered: &mut dyn Iterator<Item = &str>,
    ) -> Arc<ProgramFileOrder> {
        let workspace = self.workspace();
        let Some(root) = workspace.published_root() else {
            return Arc::new(ProgramFileOrder::unordered());
        };
        match self.ordering_project(workspace.as_ref(), &root.snapshot, anchor, ordered) {
            Some(project) => self.project_program_order(&root.snapshot, project),
            None => Arc::new(ProgramFileOrder::unordered()),
        }
    }

    /// The order that ranks `candidates` (the files declaring one symbol)
    /// for a read from `anchor`, or `None` when neither precedence nor
    /// imported reachability affects them. An imported singleton still
    /// depends on membership. Libraries alone are ranked by the project's
    /// [`Self::library_order`] unless a program file's library reference
    /// can change which ones load.
    pub(crate) fn declaration_order<'c>(
        &self,
        anchor: &str,
        candidates: impl Iterator<Item = &'c str> + Clone,
    ) -> Option<Arc<ProgramFileOrder>> {
        let has_library = candidates
            .clone()
            .any(verter_session_query::inputs::contributors::is_automatic_lib_canonical);
        if !has_library {
            let workspace = self.workspace();
            let imported = workspace.published_root().is_some_and(|root| {
                self.project_for_canonical_in_snapshot(anchor, &root.snapshot)
                    .is_some_and(|project| {
                        matches!(
                            root.snapshot.project(project).payload,
                            ProjectPayload::Configured { .. }
                        ) && candidates
                            .clone()
                            .any(|file| !root.snapshot.owners_for_file(file).contains(&project))
                    })
            });
            // Removing the sole imported declaration can change an answer
            // without changing either that declaration or the requester.
            return (imported || orders_across_files(candidates.clone()))
                .then(|| self.program_file_order(anchor, &mut candidates.clone()));
        }
        let workspace = self.workspace();
        let Some(root) = workspace.published_root() else {
            return Some(Arc::new(ProgramFileOrder::unordered()));
        };
        let Some(project) = self.ordering_project(
            workspace.as_ref(),
            &root.snapshot,
            anchor,
            &mut candidates.clone(),
        ) else {
            return Some(Arc::new(ProgramFileOrder::unordered()));
        };
        let libraries = self.library_order(&root.snapshot, project);
        Some(if needs_program_walk(candidates, &libraries) {
            self.project_program_order(&root.snapshot, project)
        } else {
            libraries
        })
    }

    /// The registered library that declares the global `symbol` for
    /// `consumer_project`: of the libraries declaring it, the first the
    /// project's program loads, in program order. The project's libraries
    /// alone decide it when every declaring library is one of them;
    /// otherwise the whole program's order does, and the read observes it.
    pub(crate) fn program_ambient_symbol(
        &self,
        consumer_project: ProjectStableKey,
        symbol: &str,
        space: verter_type_engine::resolver_core::request_ports::AmbientSymbolSpace,
    ) -> Option<AmbientSymbolHit> {
        let workspace = self.workspace();
        let registry = workspace.ambient_libs_view();
        let declaring = registry
            .by_project
            .get(&consumer_project)?
            .symbol_index
            .get(symbol)?;
        let root = workspace.published_root()?;
        let snapshot = &root.snapshot;
        let project = snapshot
            .projects
            .iter()
            .map(|project| project.id)
            .find(|&id| workspace.project_stable_key(id) == Some(consumer_project))?;
        let candidates: Vec<(Arc<str>, Arc<str>, u32)> = declaring
            .iter()
            .map(|(name, lib_order)| {
                (
                    workspace.ambient_virtual_canonical_id(consumer_project, name),
                    Arc::clone(name),
                    *lib_order,
                )
            })
            .collect();
        let libraries = self.library_order(snapshot, project);
        let order = if candidates
            .iter()
            .all(|(virtual_id, _, _)| libraries.lists(virtual_id))
        {
            libraries
        } else {
            let order = self.project_program_order(snapshot, project);
            verter_type_engine::resolver_core::resolver_context::observe_program_file_order(&order);
            order
        };
        let mut candidates: Vec<_> = candidates
            .into_iter()
            .filter(|(virtual_id, _, _)| order.loads_library(virtual_id))
            .collect();
        candidates.sort_by(|(left, _, _), (right, _, _)| {
            order
                .rank(left)
                .cmp(&order.rank(right))
                .then_with(|| left.cmp(right))
        });
        candidates
            .into_iter()
            .find(|(virtual_id, _, _)| {
                // Read only headers, in declaration precedence order. A type-only
                // declaration cannot shadow a later value (or the reverse).
                // The observed serve retains negative namespace lookups as proof.
                let Some(serve) = self.ensure_indexed_ready_serve(virtual_id) else {
                    // Retain the unreadable first candidate so its declaration
                    // read fails normally instead of guessing a later winner.
                    return true;
                };
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                    verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
                        canonical_id: virtual_id.to_string(),
                        hash: serve.indexed.whole_hash,
                    },
                );
                let headers = &serve.indexed.shallow_state.headers;
                let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
                match space {
                    verter_type_engine::resolver_core::request_ports::AmbientSymbolSpace::Type => {
                        headers.type_header_in(owner, symbol).is_some()
                    }
                    verter_type_engine::resolver_core::request_ports::AmbientSymbolSpace::Value => {
                        headers.value_header_in(owner, symbol).is_some()
                    }
                }
            })
            .map(|(virtual_id, canonical_id, lib_order)| AmbientSymbolHit {
                project: consumer_project,
                canonical_id,
                virtual_id,
                lib_order,
            })
    }

    /// `project`'s order: a configured project's whole program, built on
    /// demand and published while the workspace lists exactly it; any
    /// other project's libraries alone. A rebuild after an edit re-reads
    /// only the files that changed: every other file pulls in what it did.
    fn project_program_order(
        &self,
        snapshot: &WorkspaceSnapshot,
        project: ProjectId,
    ) -> Arc<ProgramFileOrder> {
        let ProjectPayload::Configured {
            tsconfig_path,
            membership,
            compiler_options,
            ..
        } = &snapshot.project(project).payload
        else {
            return self.library_order(snapshot, project);
        };
        let workspace = self.workspace();
        let index = self.project_type_store().indexed().program_order_index();
        let stamp = ProgramOrderStamp::live(workspace.as_ref());
        if !workspace
            .published_root()
            .is_some_and(|root| std::ptr::eq(root.snapshot.as_ref(), snapshot))
        {
            let order = Arc::new(ProgramFileOrder::sequence(
                Arc::from(tsconfig_path.as_str()),
                std::iter::empty(),
                |_| None,
            ));
            verter_type_engine::resolver_core::resolver_context::observe_program_file_order(&order);
            return order;
        }
        if let Some(order) =
            stamp.and_then(|stamp| index.current(tsconfig_path.as_str(), stamp, workspace.as_ref()))
        {
            return order;
        }

        // What the published order read that the live workspace still
        // holds: its roots while they are the same ones, and every file
        // that did not change since.
        let previous = stamp.and_then(|stamp| {
            index
                .inputs(tsconfig_path.as_str())
                .filter(|inputs| inputs.same_resolution(workspace.as_ref(), stamp))
        });
        let registry = workspace.ambient_libs_view();
        let options = workspace
            .semantic_compiler_options_for_project(project)
            .unwrap_or_else(|| compiler_options.semantic.clone());
        let libraries = ProjectLibraries::of(workspace.as_ref(), &registry, project, &options);
        let candidates = CandidateSet::live(workspace.as_ref());
        let roots: Vec<Arc<str>> = match &previous {
            Some(previous) if previous.candidates == candidates => previous.roots.clone(),
            _ => {
                let mut files: Vec<CanonicalPath> =
                    membership.materialized_files.iter().cloned().collect();
                files.extend(
                    workspace
                        .snapshot_canonicals()
                        .into_iter()
                        .chain(workspace.overlay_canonicals())
                        .map(|canonical| CanonicalPath::new(&canonical)),
                );
                membership
                    .spec
                    .program_root_files(&files)
                    .iter()
                    .map(|root| Arc::from(root.as_str()))
                    .chain(
                        libraries
                            .iter()
                            .flat_map(|libraries| libraries.roots.iter().cloned()),
                    )
                    .collect()
            }
        };
        let owner = verter_session_query::resolution::ProjectOwnership {
            project_root: snapshot.project(project).root.as_str().to_owned(),
            tsconfig_path: Some(tsconfig_path.as_str().to_owned()),
        };
        let javascript = compiler_options.js_is_member();
        // A file served from outside the store, or a module name the
        // resolver refused, makes this order this request's alone.
        let mut provisional = false;
        let mut pulled: FxHashMap<Arc<str>, Option<Vec<Arc<str>>>> = FxHashMap::default();
        let mut probes: Vec<Arc<str>> = Vec::new();
        let order = Arc::new(ProgramFileOrder::sequence(
            Arc::from(tsconfig_path.as_str()),
            roots.iter().cloned(),
            |file| {
                let reused = previous.as_ref().and_then(|previous| {
                    let (file, files) = previous.pulled.get_key_value(file)?;
                    (!previous.moved(workspace.as_ref(), file))
                        .then(|| (Arc::clone(file), files.clone()))
                });
                let (file, files) = match reused {
                    Some(reused) => reused,
                    None => {
                        let (files, _) = self.discovered_files(
                            workspace.as_ref(),
                            snapshot,
                            libraries.as_ref(),
                            &owner,
                            javascript,
                            file,
                            &mut provisional,
                            &mut probes,
                        );
                        (Arc::from(file), files)
                    }
                };
                pulled.insert(file, files.clone());
                files
            },
        ));
        if let Some(previous) = &previous {
            // A reused file's path references were probed by the walk that
            // first read it.
            probes.extend(
                previous
                    .probes
                    .iter()
                    .filter(|probe| !probes.contains(probe))
                    .cloned()
                    .collect::<Vec<_>>(),
            );
        }
        let Some(stamp) = stamp else {
            return order;
        };
        if provisional
            || ProgramOrderStamp::live(workspace.as_ref()) != Some(stamp)
            || !Arc::ptr_eq(&registry, &workspace.ambient_libs_view())
        {
            verter_type_engine::resolver_core::resolver_context::observe_program_file_order(&order);
            return order;
        }
        if order.is_empty() {
            return order;
        }
        let (order, previous_fingerprint) = index.publish(
            order,
            OrderInputs {
                stamp,
                registry,
                candidates,
                roots,
                pulled,
                probes,
            },
        );
        if previous_fingerprint != Some(order.fingerprint()) {
            self.bump_load_generation();
            verter_type_engine::request_observers::push_structured_event(
                crate::component_meta_audit::StructuredAuditEvent::ProgramFileOrder {
                    program: Arc::from(tsconfig_path.as_str()),
                    prev_fingerprint: previous_fingerprint,
                    new_fingerprint: order.fingerprint(),
                    file_count: u32::try_from(order.len()).unwrap_or(u32::MAX),
                },
            );
        }
        order
    }

    /// The files `file` pulls into its program, in the order the program
    /// visits them: its `/// <reference path>` targets, `/// <reference
    /// types>` packages, `/// <reference lib>` libraries, then its module
    /// names' files. A library pulls in only the libraries it references.
    /// `None` when the program cannot load `file`.
    #[allow(clippy::too_many_arguments)]
    fn discovered_files(
        &self,
        workspace: &dyn WorkspaceAccess,
        snapshot: &WorkspaceSnapshot,
        libraries: Option<&ProjectLibraries>,
        owner: &verter_session_query::resolution::ProjectOwnership,
        javascript: bool,
        file: &str,
        provisional: &mut bool,
        probes: &mut Vec<Arc<str>>,
    ) -> (Option<Vec<Arc<str>>>, bool) {
        let index = self.project_type_store().indexed().program_order_index();
        let Some(published) = workspace
            .published_root()
            .filter(|root| std::ptr::eq(root.snapshot.as_ref(), snapshot))
        else {
            *provisional = true;
            return (None, true);
        };
        let key = (
            Arc::from(owner.tsconfig_path.as_deref().unwrap_or_default()),
            Arc::from(file),
        );
        if let Some(entry) = index
            .discovered
            .read()
            .get(&key)
            .filter(|entry| entry.current(workspace, file))
        {
            probes.extend(entry.probes.iter().cloned());
            return (entry.files.clone(), false);
        }
        index.note_reference_read();
        let Some(before) = ProgramOrderStamp::live(workspace) else {
            *provisional = true;
            return (None, true);
        };
        let registry = workspace.ambient_libs_view();
        if libraries.is_some_and(|libraries| !Arc::ptr_eq(&libraries.registry, &registry)) {
            *provisional = true;
            return (None, true);
        }
        let transition = workspace.last_content_transition_generation(file);
        let mut local_probes = Vec::new();
        let mut local_provisional = false;
        let mut reference_free = false;
        let files = self.read_pulled_files(
            workspace,
            libraries,
            owner,
            javascript,
            file,
            &mut local_provisional,
            &mut local_probes,
            &mut reference_free,
        );
        *provisional |= local_provisional;
        if !local_provisional
            && Some(before) == ProgramOrderStamp::live(workspace)
            && workspace
                .published_root()
                .is_some_and(|root| Arc::ptr_eq(&root, &published))
            && Arc::ptr_eq(&registry, &workspace.ambient_libs_view())
        {
            index.discovered.write().insert(
                key,
                DiscoveredFile {
                    transition,
                    observed_content: before.content_generation,
                    published,
                    resolution: before.resolution_generation,
                    registry,
                    reference_free,
                    files: files.clone(),
                    probes: local_probes.clone(),
                },
            );
        }
        probes.extend(local_probes);
        (files, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn read_pulled_files(
        &self,
        workspace: &dyn WorkspaceAccess,
        libraries: Option<&ProjectLibraries>,
        owner: &verter_session_query::resolution::ProjectOwnership,
        javascript: bool,
        file: &str,
        provisional: &mut bool,
        probes: &mut Vec<Arc<str>>,
        reference_free: &mut bool,
    ) -> Option<Vec<Arc<str>>> {
        if !verter_session_query::inputs::contributors::is_automatic_lib_canonical(file) {
            let Some(source) = workspace.read_file(file) else {
                *provisional |= !matches!(
                    workspace.probe_path(file),
                    verter_session_query::resolution::PathProbe::Absent
                        | verter_session_query::resolution::PathProbe::Directory
                );
                return None;
            };
            // Necessary-condition filter only: the cached parser inventory
            // remains the authority for actual edges and contributions.
            if !source.contains("import")
                && !source.contains("from")
                && !source.contains("reference")
                && !source.contains("require")
                && !source.contains('\\')
                && !crate::global_contributors::source_has_ambient_contribution(&source)
                && !crate::global_contributors::source_may_have_file_scope_global_contribution(
                    &source,
                )
            {
                *reference_free = true;
                return Some(Vec::new());
            }
        }
        let Some(serve) = self.ensure_indexed_ready_serve_unobserved(file) else {
            *provisional = true;
            return None;
        };
        *provisional |= !serve.store_published;
        let references = &serve.indexed.route_inventory.program_references;
        *reference_free = references.paths.is_empty()
            && references.types.is_empty()
            && references.libs.is_empty()
            && references.module_names.is_empty();
        if verter_session_query::inputs::contributors::is_automatic_lib_canonical(file) {
            return Some(libraries.map_or_else(Vec::new, |libraries| {
                libraries.referenced(workspace, &references.libs)
            }));
        }
        let mut pulled: Vec<Arc<str>> = Vec::new();
        for path in &references.paths {
            if let Some(target) = self.triple_slash_path_target(workspace, file, path, probes) {
                pulled.push(target);
            }
        }
        for package in &references.types {
            let types_package = match package.strip_prefix('@') {
                Some(scoped) => format!("@types/{}", scoped.replacen('/', "__", 1)),
                None => format!("@types/{package}"),
            };
            let target = self
                .program_module_target(owner, file, &types_package, provisional)
                .or_else(|| self.program_module_target(owner, file, package, provisional));
            pulled.extend(target);
        }
        pulled.retain(|target| is_program_source(target, javascript));
        if let Some(libraries) = libraries {
            pulled.extend(libraries.referenced(workspace, &references.libs));
        }
        for name in &references.module_names {
            pulled.extend(
                self.program_module_target(owner, file, name, provisional)
                    .filter(|target| is_program_source(target, javascript)),
            );
        }
        Some(pulled)
    }

    /// The libraries `project` loads by its configuration and the
    /// libraries' own `/// <reference lib>` directives, in program order —
    /// the libraries of its program before any program file adds one.
    /// Kept until the configuration or the registered libraries change.
    pub(crate) fn library_order(
        &self,
        snapshot: &WorkspaceSnapshot,
        project: ProjectId,
    ) -> Arc<ProgramFileOrder> {
        let workspace = self.workspace();
        let Some(published) = workspace
            .published_root()
            .filter(|root| std::ptr::eq(root.snapshot.as_ref(), snapshot))
        else {
            return Arc::new(ProgramFileOrder::unordered());
        };
        let Some(key) = workspace.project_stable_key(project) else {
            return Arc::new(ProgramFileOrder::unordered());
        };
        let stamp = LibraryStamp {
            authority: published.authority(),
            snapshot_generation: snapshot.generation.0,
            registry: workspace.ambient_libs_view(),
        };
        let index = self.project_type_store().indexed().program_order_index();
        if let Some(order) = index.library_order(key, &stamp) {
            return order;
        }
        let options = workspace
            .semantic_compiler_options_for_project(project)
            .unwrap_or_default();
        let Some(libraries) =
            ProjectLibraries::of(workspace.as_ref(), &stamp.registry, project, &options)
        else {
            return Arc::new(ProgramFileOrder::unordered());
        };
        let mut provisional = false;
        let order = Arc::new(ProgramFileOrder::libraries(
            libraries.roots.iter().cloned(),
            |file| {
                let Some(serve) = self.ensure_indexed_ready_serve_unobserved(file) else {
                    provisional = true;
                    return None;
                };
                provisional |= !serve.store_published;
                Some(libraries.referenced(
                    workspace.as_ref(),
                    &serve.indexed.route_inventory.program_references.libs,
                ))
            },
        ));
        if !provisional
            && workspace
                .published_root()
                .is_some_and(|root| Arc::ptr_eq(&root, &published))
            && Arc::ptr_eq(&stamp.registry, &workspace.ambient_libs_view())
        {
            index.publish_library_order(key, stamp, &order);
        }
        order
    }

    /// The file `/// <reference path="reference" />` in `file` names
    /// (`resolveTripleslashReference` + `getSourceFileFromReferenceWorker`):
    /// the path relative to `file`'s directory, as written when it carries
    /// an extension, else the first of `.ts`, `.tsx`, `.d.ts` that exists.
    /// Every path it looks for joins `probes`: one that appears or goes
    /// away later changes the answer.
    fn triple_slash_path_target(
        &self,
        workspace: &dyn WorkspaceAccess,
        file: &str,
        reference: &str,
        probes: &mut Vec<Arc<str>>,
    ) -> Option<Arc<str>> {
        let target = join_paths(&parent_dir(file), reference);
        let has_extension = target
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains('.'));
        let candidates: Vec<String> = if has_extension {
            vec![target]
        } else {
            [".ts", ".tsx", ".d.ts"]
                .into_iter()
                .map(|extension| format!("{target}{extension}"))
                .collect()
        };
        for candidate in candidates {
            let candidate: Arc<str> = Arc::from(candidate.as_str());
            probes.push(Arc::clone(&candidate));
            if workspace.file_exists(&candidate) {
                return Some(candidate);
            }
        }
        None
    }

    /// The program file module name `name` resolves to from `file`, through
    /// the one type-route resolution policy.
    fn program_module_target(
        &self,
        owner: &verter_session_query::resolution::ProjectOwnership,
        file: &str,
        name: &str,
        provisional: &mut bool,
    ) -> Option<Arc<str>> {
        let outcome = self.workspace().resolve_import_from_project_outcome(
            owner,
            None,
            file,
            name,
            verter_session_query::resolution::ResolutionContext {
                phase: verter_session_query::resolution::ResolvePhase::ProviderGraph,
                kind: verter_session_query::resolution::ResolveRequestKind::TypeImport,
            },
        );
        match outcome
            .into_publication()
            .map_result(|target| target.source_id)
        {
            verter_workspace::ResolutionPublication::Admitted(admitted) => {
                admitted.result().map(|target| Arc::from(target.as_str()))
            }
            verter_workspace::ResolutionPublication::Refused(_) => {
                *provisional = true;
                None
            }
        }
    }
}

/// The project the library `canonical` (an ambient virtual id) is
/// registered for; `None` for any other file.
fn registered_project(
    workspace: &dyn WorkspaceAccess,
    snapshot: &WorkspaceSnapshot,
    canonical: &str,
) -> Option<ProjectId> {
    let (key, _) = verter_workspace::parse_ambient_virtual_canonical_id(canonical)?;
    snapshot
        .projects
        .iter()
        .map(|project| project.id)
        .find(|&id| workspace.project_stable_key(id) == Some(key))
}

/// Whether a resolved module `target` is a source file of the program:
/// library files are loaded only by library references, JSON and (without
/// `allowJs`) JavaScript are not program sources.
fn is_program_source(target: &str, javascript: bool) -> bool {
    if verter_session_query::inputs::contributors::is_automatic_lib_canonical(target)
        || target.ends_with(".json")
    {
        return false;
    }
    let is_javascript = [".js", ".jsx", ".cjs", ".mjs"]
        .iter()
        .any(|extension| target.ends_with(extension));
    javascript || !is_javascript
}

#[cfg(test)]
#[path = "program_order_tests.rs"]
mod tests;

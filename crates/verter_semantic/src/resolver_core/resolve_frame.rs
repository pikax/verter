//! Request geometry and pure-string memoization retained for one top-level
//! module resolution.

use std::cell::{Cell, OnceCell, RefCell};
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::resolver_core::node_modules_resolution::{
    ancestor_dirs, ancestor_dirs_from_dir, split_package_specifier,
};
use crate::resolver_core::probe_path_resolution::build_probe_candidate_list;
use crate::resolver_core::project_ownership_resolution::{
    nearest_config_for_path, project_for_ownership,
};
use crate::resolver_core::{
    IdeProjectConfig, KernelAttempt, ProjectOwnership, ResolutionBasis, ResolutionContext,
    ResolveRequest, ResolveResult, ResolverAttemptView,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ResolveFrameOperation {
    Request,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpecifierClass {
    RelativeOrAbsolute,
    PackageImports,
    Package,
}

#[derive(Debug, Clone)]
pub(crate) struct MappingCandidate {
    pub(crate) normalized: Arc<str>,
    pub(crate) probe_candidates: Arc<[Arc<str>]>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProjectResolutionGeometry {
    pub(crate) base_url_candidate: Option<MappingCandidate>,
}

#[derive(Debug, Clone)]
pub(crate) struct NodeModulesDirectoryGeometry {
    pub(crate) directory: Arc<str>,
    pub(crate) package_dir: Arc<str>,
    pub(crate) manifest_path: Arc<str>,
    pub(crate) direct_probe_candidates: Arc<[Arc<str>]>,
}

#[derive(Debug, Clone)]
pub(crate) struct ImportsDirectoryGeometry {
    pub(crate) directory: Arc<str>,
    pub(crate) manifest_path: Arc<str>,
}

#[derive(Debug, Clone)]
pub(crate) struct SourceResolutionGeometry {
    pub(crate) owner_index: Option<usize>,
    pub(crate) start: Arc<str>,
    pub(crate) start_is_directory: bool,
    pub(crate) boundary: Option<Arc<str>>,
    pub(crate) specifier: Arc<str>,
    pub(crate) context: ResolutionContext,
    pub(crate) class: SpecifierClass,
    pub(crate) apply_source_sibling: bool,
    pub(crate) prefers_declarations: bool,
    pub(crate) relative_base: Option<Arc<str>>,
    pub(crate) relative_probe_candidates: Arc<[Arc<str>]>,
    pub(crate) package_name: Option<Arc<str>>,
    pub(crate) package_subpath: Arc<str>,
    node_modules_directories: OnceCell<Arc<[NodeModulesDirectoryGeometry]>>,
    imports_directories: OnceCell<Arc<[ImportsDirectoryGeometry]>>,
}

type InternedString = Arc<str>;
type StringByString = FxHashMap<InternedString, InternedString>;
type JoinedStrings = FxHashMap<InternedString, StringByString>;
type ProbeCandidatesByBase = FxHashMap<InternedString, Arc<[InternedString]>>;
type ProbeCandidateMemo = FxHashMap<(bool, bool), ProbeCandidatesByBase>;
type PackagePathCaptures = FxHashMap<Option<InternedString>, InternedString>;
type PackagePathTargets = FxHashMap<InternedString, PackagePathCaptures>;
type PackagePathMemo = FxHashMap<InternedString, PackagePathTargets>;

#[derive(Debug, Default)]
pub(crate) struct ResolutionStringMemo {
    normalized: RefCell<StringByString>,
    joined: RefCell<JoinedStrings>,
    parents: RefCell<StringByString>,
    probe_candidates: RefCell<ProbeCandidateMemo>,
    package_paths: RefCell<PackagePathMemo>,
}

impl ResolutionStringMemo {
    pub(crate) fn clear(&self) {
        self.normalized.borrow_mut().clear();
        self.joined.borrow_mut().clear();
        self.parents.borrow_mut().clear();
        self.probe_candidates.borrow_mut().clear();
        self.package_paths.borrow_mut().clear();
    }

    #[cfg(test)]
    pub(crate) fn retains_probe_base_for_test(&self, base: &str) -> bool {
        self.probe_candidates
            .borrow()
            .values()
            .any(|bases| bases.contains_key(base))
    }

    pub(crate) fn normalize(&self, value: &str) -> Arc<str> {
        if let Some(cached) = self.normalized.borrow().get(value).cloned() {
            return cached;
        }
        let normalized: Arc<str> = Arc::from(crate::resolver_core::normalize_canonical_id(value));
        let mut memo = self.normalized.borrow_mut();
        memo.insert(Arc::from(value), Arc::clone(&normalized));
        memo.entry(Arc::clone(&normalized))
            .or_insert_with(|| Arc::clone(&normalized));
        normalized
    }

    fn remember_canonical(&self, value: &Arc<str>) {
        self.normalized
            .borrow_mut()
            .entry(Arc::clone(value))
            .or_insert_with(|| Arc::clone(value));
    }

    pub(crate) fn join(&self, base: &str, path: &str) -> Arc<str> {
        if let Some(cached) = self
            .joined
            .borrow()
            .get(base)
            .and_then(|paths| paths.get(path))
            .cloned()
        {
            return cached;
        }
        let joined: Arc<str> = if path.is_empty() {
            self.normalize(base)
        } else if crate::resolver_core::is_absolute_specifier(path) {
            Arc::from(crate::resolver_core::collapse_path(path))
        } else {
            let normalized_base = self.normalize(base);
            let normalized_path = self.normalize(path);
            Arc::from(crate::resolver_core::collapse_path(&format!(
                "{}/{}",
                normalized_base.trim_end_matches('/'),
                normalized_path
                    .trim_start_matches("./")
                    .trim_start_matches('/')
            )))
        };
        self.remember_canonical(&joined);
        self.joined
            .borrow_mut()
            .entry(Arc::from(base))
            .or_default()
            .insert(Arc::from(path), Arc::clone(&joined));
        joined
    }

    pub(crate) fn parent(&self, path: &str) -> Arc<str> {
        if let Some(cached) = self.parents.borrow().get(path).cloned() {
            return cached;
        }
        let normalized = self.normalize(path);
        let parent: Arc<str> = normalized
            .rsplit_once('/')
            .map(|(directory, _)| Arc::from(directory))
            .unwrap_or_else(|| Arc::from(""));
        self.remember_canonical(&parent);
        self.parents
            .borrow_mut()
            .insert(Arc::from(path), Arc::clone(&parent));
        parent
    }

    pub(crate) fn probe_candidates(
        &self,
        base: &str,
        apply_source_sibling: bool,
        prefers_declarations: bool,
    ) -> Arc<[Arc<str>]> {
        let flags = (apply_source_sibling, prefers_declarations);
        if let Some(cached) = self
            .probe_candidates
            .borrow()
            .get(&flags)
            .and_then(|bases| bases.get(base))
            .cloned()
        {
            return cached;
        }
        let candidates = arc_strings(build_probe_candidate_list(
            base,
            apply_source_sibling,
            prefers_declarations,
        ));
        for candidate in &*candidates {
            self.remember_canonical(candidate);
        }
        self.probe_candidates
            .borrow_mut()
            .entry(flags)
            .or_default()
            .insert(Arc::from(base), Arc::clone(&candidates));
        candidates
    }

    pub(crate) fn package_path(
        &self,
        package_dir: &str,
        target: &str,
        captured: Option<&str>,
    ) -> Arc<str> {
        let captured_key = captured.map(Arc::from);
        if let Some(cached) = self
            .package_paths
            .borrow()
            .get(package_dir)
            .and_then(|targets| targets.get(target))
            .and_then(|captures| captures.get(&captured_key))
            .cloned()
        {
            return cached;
        }
        let replaced = match captured {
            Some(captured) if target.contains('*') => {
                let star = target.find('*').unwrap_or(0);
                format!("{}{}{}", &target[..star], captured, &target[star + 1..])
            }
            _ => target.to_string(),
        };
        let path = if crate::resolver_core::is_absolute_specifier(&replaced) {
            self.normalize(&replaced)
        } else {
            self.join(package_dir, &replaced)
        };
        self.remember_canonical(&path);
        self.package_paths
            .borrow_mut()
            .entry(Arc::from(package_dir))
            .or_default()
            .entry(Arc::from(target))
            .or_default()
            .insert(captured_key, Arc::clone(&path));
        path
    }
}

pub struct ResolveFrame<'a> {
    pub(crate) projects: &'a [IdeProjectConfig],
    pub(crate) operation: ResolveFrameOperation,
    pub(crate) geometry: SourceResolutionGeometry,
    pub(crate) memo: ResolutionStringMemo,
    active_basis: Cell<Option<ResolutionBasis>>,
}

impl std::fmt::Debug for ResolveFrame<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolveFrame")
            .field("operation", &self.operation)
            .field("geometry", &self.geometry)
            .field("active_basis", &self.active_basis.get())
            .finish_non_exhaustive()
    }
}

impl<'a> ResolveFrame<'a> {
    pub(crate) fn for_request(projects: &'a [IdeProjectConfig], request: &ResolveRequest) -> Self {
        let owner_index = nearest_config_for_path(projects, &request.importer_id)
            .and_then(|owner| project_index(projects, owner));
        let context = ResolutionContext {
            phase: request.phase,
            kind: request.kind,
        };
        let geometry = build_source_geometry(
            projects,
            owner_index,
            &request.importer_id,
            &request.specifier,
            context,
            false,
        );
        Self {
            projects,
            operation: ResolveFrameOperation::Request,
            geometry,
            memo: ResolutionStringMemo::default(),
            active_basis: Cell::new(None),
        }
    }

    pub(crate) fn for_project(
        projects: &'a [IdeProjectConfig],
        owner: &ProjectOwnership,
        specifier: &str,
        context: ResolutionContext,
    ) -> Self {
        let owner_index = project_for_ownership(projects, owner)
            .and_then(|project| project_index(projects, project));
        let start = owner_index
            .map(|index| projects[index].root.as_str())
            .unwrap_or(owner.project_root.as_str());
        let geometry =
            build_source_geometry(projects, owner_index, start, specifier, context, true);
        Self {
            projects,
            operation: ResolveFrameOperation::Project,
            geometry,
            memo: ResolutionStringMemo::default(),
            active_basis: Cell::new(None),
        }
    }

    pub fn attempt(
        &self,
        view: &ResolverAttemptView,
        expected_basis: ResolutionBasis,
    ) -> KernelAttempt<Option<ResolveResult>> {
        view.input_resolution_retention().scope(|| {
            if self.active_basis.replace(Some(expected_basis)) != Some(expected_basis) {
                self.memo.clear();
                self.seed_precomputed_geometry();
            }
            crate::resolver_core::top_level_resolution::resolve_frame_with_reader(
                self,
                view,
                expected_basis,
            )
        })
    }

    fn seed_precomputed_geometry(&self) {
        if let Some(base) = &self.geometry.relative_base {
            self.memo.remember_canonical(base);
        }
        for candidate in &*self.geometry.relative_probe_candidates {
            self.memo.remember_canonical(candidate);
        }
        if let Some(directories) = self.geometry.node_modules_directories.get() {
            for directory in &**directories {
                self.seed_node_modules_directory(directory);
            }
        }
        if let Some(directories) = self.geometry.imports_directories.get() {
            for directory in &**directories {
                self.seed_imports_directory(directory);
            }
        }
    }

    fn seed_project_geometry(&self, geometry: &ProjectResolutionGeometry) {
        for candidate in geometry.base_url_candidate.iter() {
            self.memo.remember_canonical(&candidate.normalized);
            for probe in &*candidate.probe_candidates {
                self.memo.remember_canonical(probe);
            }
        }
    }

    fn seed_node_modules_directory(&self, directory: &NodeModulesDirectoryGeometry) {
        self.memo.remember_canonical(&directory.directory);
        self.memo.remember_canonical(&directory.package_dir);
        self.memo.remember_canonical(&directory.manifest_path);
        for probe in &*directory.direct_probe_candidates {
            self.memo.remember_canonical(probe);
        }
    }

    fn seed_imports_directory(&self, directory: &ImportsDirectoryGeometry) {
        self.memo.remember_canonical(&directory.directory);
        self.memo.remember_canonical(&directory.manifest_path);
    }

    pub(crate) fn project_geometry(
        &self,
        project: &IdeProjectConfig,
    ) -> Option<ProjectResolutionGeometry> {
        let owner_index = self.geometry.owner_index?;
        let index = project_index(self.projects, project)?;
        if !project_is_reachable(self.projects, owner_index, index) {
            return None;
        }
        let geometry = build_project_geometry(
            project,
            &self.geometry.specifier,
            self.geometry.apply_source_sibling,
            self.geometry.prefers_declarations,
        );
        self.seed_project_geometry(&geometry);
        Some(geometry)
    }

    pub(crate) fn path_candidates<'b>(
        &'b self,
        project: &'b IdeProjectConfig,
    ) -> impl Iterator<Item = MappingCandidate> + 'b {
        let base_url = project
            .compiler_options
            .base_url
            .as_deref()
            .unwrap_or(project.root.as_str());
        let candidates =
            project
                .compiler_options
                .paths
                .iter()
                .flat_map(move |(pattern, targets)| {
                    let captured =
                        crate::resolver_core::package_target_resolution::capture_tsconfig_pattern(
                            pattern,
                            &self.geometry.specifier,
                        );
                    targets.iter().filter_map(move |target| {
                        captured.map(|captured| {
                            let normalized =
                                self.memo.package_path(base_url, target, Some(captured));
                            let probe_candidates = self.memo.probe_candidates(
                                &normalized,
                                self.geometry.apply_source_sibling,
                                self.geometry.prefers_declarations,
                            );
                            MappingCandidate {
                                normalized,
                                probe_candidates,
                            }
                        })
                    })
                });
        let mut seen = FxHashSet::default();
        candidates.filter(move |candidate| seen.insert(Arc::clone(&candidate.normalized)))
    }

    pub(crate) fn node_modules_directories(&self) -> &[NodeModulesDirectoryGeometry] {
        self.geometry.node_modules_directories.get_or_init(|| {
            let Some(package_name) = self.geometry.package_name.as_deref() else {
                return Arc::from([]);
            };
            self.ancestor_directories()
                .iter()
                .map(|directory| {
                    let directory: Arc<str> = Arc::from(directory.as_str());
                    self.memo.remember_canonical(&directory);
                    let node_modules = self.memo.join(&directory, "node_modules");
                    let package_dir = self.memo.join(&node_modules, package_name);
                    let manifest_path = self.memo.join(&package_dir, "package.json");
                    let direct_base = if self.geometry.package_subpath.is_empty() {
                        Arc::clone(&package_dir)
                    } else {
                        self.memo.join(&package_dir, &self.geometry.package_subpath)
                    };
                    let geometry = NodeModulesDirectoryGeometry {
                        directory,
                        package_dir,
                        manifest_path,
                        direct_probe_candidates: self.memo.probe_candidates(
                            &direct_base,
                            self.geometry.apply_source_sibling,
                            self.geometry.prefers_declarations,
                        ),
                    };
                    self.seed_node_modules_directory(&geometry);
                    geometry
                })
                .collect::<Vec<_>>()
                .into()
        })
    }

    pub(crate) fn imports_directories(&self) -> &[ImportsDirectoryGeometry] {
        self.geometry.imports_directories.get_or_init(|| {
            self.ancestor_directories()
                .iter()
                .map(|directory| {
                    let directory: Arc<str> = Arc::from(directory.as_str());
                    self.memo.remember_canonical(&directory);
                    let geometry = ImportsDirectoryGeometry {
                        manifest_path: self.memo.join(&directory, "package.json"),
                        directory,
                    };
                    self.seed_imports_directory(&geometry);
                    geometry
                })
                .collect::<Vec<_>>()
                .into()
        })
    }

    fn ancestor_directories(&self) -> Vec<String> {
        let boundary = self.geometry.boundary.as_deref();
        if self.geometry.start_is_directory {
            ancestor_dirs_from_dir(&self.geometry.start, boundary)
        } else {
            ancestor_dirs(&self.geometry.start, boundary)
        }
    }
}

fn build_source_geometry(
    projects: &[IdeProjectConfig],
    owner_index: Option<usize>,
    start: &str,
    specifier: &str,
    context: ResolutionContext,
    start_is_directory: bool,
) -> SourceResolutionGeometry {
    let class = if crate::resolver_core::is_relative_specifier(specifier)
        || crate::resolver_core::is_absolute_specifier(specifier)
    {
        SpecifierClass::RelativeOrAbsolute
    } else if specifier.starts_with('#') {
        SpecifierClass::PackageImports
    } else {
        SpecifierClass::Package
    };
    let prefers_declarations = prefers_declaration_files(context);
    let apply_source_sibling = context.kind != crate::resolver_core::ResolveRequestKind::SfcSrcAttr;

    let relative_base = (class == SpecifierClass::RelativeOrAbsolute).then(|| {
        let base = if crate::resolver_core::is_absolute_specifier(specifier) {
            crate::resolver_core::normalize_canonical_id(specifier)
        } else {
            let directory = if start_is_directory {
                start.to_string()
            } else {
                crate::resolver_core::parent_dir(start)
            };
            crate::resolver_core::join_paths(&directory, specifier)
        };
        Arc::<str>::from(base)
    });
    let relative_probe_candidates = relative_base.as_deref().map_or_else(
        || Arc::from([]),
        |base| {
            arc_strings(build_probe_candidate_list(
                base,
                apply_source_sibling,
                prefers_declarations,
            ))
        },
    );

    let boundary = owner_index.map(|index| Arc::from(projects[index].workspace_root.as_str()));
    let (package_name, package_subpath) = split_package_specifier(specifier)
        .map(|(name, subpath)| (Some(Arc::<str>::from(name)), Arc::<str>::from(subpath)))
        .unwrap_or_else(|| (None, Arc::from("")));

    SourceResolutionGeometry {
        owner_index,
        start: Arc::from(start),
        start_is_directory,
        boundary,
        specifier: Arc::from(specifier),
        context,
        class,
        apply_source_sibling,
        prefers_declarations,
        relative_base,
        relative_probe_candidates,
        package_name,
        package_subpath,
        node_modules_directories: OnceCell::new(),
        imports_directories: OnceCell::new(),
    }
}

fn build_project_geometry(
    project: &IdeProjectConfig,
    specifier: &str,
    apply_source_sibling: bool,
    prefers_declarations: bool,
) -> ProjectResolutionGeometry {
    let mapping = |candidate: String| {
        let normalized: Arc<str> = Arc::from(candidate);
        let probe_candidates = arc_strings(build_probe_candidate_list(
            &normalized,
            apply_source_sibling,
            prefers_declarations,
        ));
        MappingCandidate {
            normalized,
            probe_candidates,
        }
    };

    let base_url_candidate = project
        .compiler_options
        .base_url
        .as_deref()
        .map(|base_url| mapping(crate::resolver_core::join_paths(base_url, specifier)));

    ProjectResolutionGeometry { base_url_candidate }
}

fn project_is_reachable(
    projects: &[IdeProjectConfig],
    owner_index: usize,
    target_index: usize,
) -> bool {
    let mut pending = vec![owner_index];
    let mut seen = FxHashSet::default();
    while let Some(index) = pending.pop() {
        if !seen.insert(index) {
            continue;
        }
        if index == target_index {
            return true;
        }
        for reference in projects[index].references.iter().rev() {
            if let Some(reference_index) = projects.iter().position(|candidate| {
                candidate.tsconfig_path.as_deref() == Some(reference.as_str())
            }) {
                pending.push(reference_index);
            }
        }
    }
    false
}

fn project_index(projects: &[IdeProjectConfig], project: &IdeProjectConfig) -> Option<usize> {
    projects
        .iter()
        .position(|candidate| std::ptr::eq(candidate, project))
}

fn prefers_declaration_files(context: ResolutionContext) -> bool {
    matches!(
        (context.phase, context.kind),
        (
            crate::resolver_core::ResolvePhase::CodegenBlocker,
            crate::resolver_core::ResolveRequestKind::TypeImport
        ) | (crate::resolver_core::ResolvePhase::ProviderGraph, _)
    )
}

fn arc_strings(values: Vec<String>) -> Arc<[Arc<str>]> {
    values
        .into_iter()
        .map(Arc::<str>::from)
        .collect::<Vec<_>>()
        .into()
}

#[cfg(test)]
mod canonical_provenance_tests {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    use super::*;

    fn provenance_test_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("provenance test lock poisoned")
    }

    fn reset_normalize_calls() {
        crate::resolver_core::path_utils::NORMALIZE_CALLS.with(|calls| calls.set(0));
    }

    fn normalize_calls() -> usize {
        crate::resolver_core::path_utils::NORMALIZE_CALLS.with(std::cell::Cell::get)
    }

    fn project(root: &str) -> IdeProjectConfig {
        let mut project = IdeProjectConfig::new(
            root.to_string(),
            root.to_string(),
            Some(format!("{root}/tsconfig.json")),
        );
        project
            .workspace_aliases
            .push(crate::resolver_core::WorkspaceAlias {
                find: "@/".to_string(),
                replacement: format!("{root}/src"),
            });
        project.compiler_options.base_url = Some(format!("{root}/src"));
        project
            .compiler_options
            .paths
            .push(("pkg/*".to_string(), vec![format!("{root}/generated/*")]));
        project
    }

    #[test]
    fn normalized_result_is_registered_as_canonical() {
        let _guard = provenance_test_lock();
        let memo = ResolutionStringMemo::default();
        reset_normalize_calls();

        let canonical = memo.normalize(r"C:\repo\src\main.ts");
        let first_use = normalize_calls();
        assert_eq!(memo.normalize(&canonical), canonical);
        assert_eq!(
            normalize_calls(),
            first_use,
            "a canonical result must not cross the normalizer again"
        );

        // Revert control: remove the canonical->canonical registration in
        // `normalize`; the second lookup above adds one call.
    }

    #[test]
    fn joined_parent_probe_and_manifest_outputs_retain_provenance() {
        let _guard = provenance_test_lock();
        let memo = ResolutionStringMemo::default();
        reset_normalize_calls();

        let joined = memo.join(r"C:\repo\src", "./pkg/../util.ts");
        let after_join = normalize_calls();
        assert_eq!(memo.normalize(&joined), joined);
        assert_eq!(
            normalize_calls(),
            after_join,
            "joined output lost provenance"
        );

        let parent = memo.parent(&joined);
        let after_parent = normalize_calls();
        assert_eq!(memo.normalize(&parent), parent);
        assert_eq!(
            normalize_calls(),
            after_parent,
            "parent output lost provenance"
        );

        let probes = memo.probe_candidates(&joined, true, true);
        let after_probes = normalize_calls();
        for probe in &*probes {
            assert_eq!(memo.normalize(probe), *probe);
        }
        assert_eq!(
            normalize_calls(),
            after_probes,
            "probe candidates lost provenance"
        );

        let manifest_target = memo.package_path(&parent, "./package.json", None);
        let after_manifest = normalize_calls();
        assert_eq!(memo.normalize(&manifest_target), manifest_target);
        assert_eq!(
            normalize_calls(),
            after_manifest,
            "package target lost provenance"
        );

        // Revert controls: remove output registration from `join`, `parent`,
        // `probe_candidates`, or `package_path`; the corresponding equality
        // above gains a normalization call.
    }

    #[test]
    fn joins_reuse_component_normalization() {
        let _guard = provenance_test_lock();
        let memo = ResolutionStringMemo::default();
        reset_normalize_calls();

        let _ = memo.join(r"C:\repo\src", "./first.ts");
        let after_first = normalize_calls();
        let _ = memo.join(r"C:\repo\src", "./second.ts");
        let second_join_calls = normalize_calls() - after_first;
        assert_eq!(
            second_join_calls, 2,
            "a new relative join needs only its new fragment and combined collapse"
        );

        // Revert control: delegate the second join directly to `join_paths`;
        // it re-normalizes the shared base and this becomes three calls.
    }

    #[test]
    fn lazy_geometry_registers_each_canonical_family_when_materialized() {
        let _guard = provenance_test_lock();
        let projects = vec![project("/repo")];
        let request = ResolveRequest {
            importer_id: "/repo/src/main.ts".to_string(),
            specifier: "pkg/item".to_string(),
            kind: crate::resolver_core::ResolveRequestKind::EsmImport,
            phase: crate::resolver_core::ResolvePhase::ProviderGraph,
        };
        let frame = ResolveFrame::for_request(&projects, &request);
        frame.seed_precomputed_geometry();
        reset_normalize_calls();

        let geometry = frame
            .project_geometry(&projects[0])
            .expect("the owning project has geometry");
        for candidate in geometry.base_url_candidate.iter() {
            assert_eq!(
                frame.memo.normalize(&candidate.normalized),
                candidate.normalized
            );
            for probe in &*candidate.probe_candidates {
                assert_eq!(frame.memo.normalize(probe), *probe);
            }
        }
        for candidate in frame.path_candidates(&projects[0]) {
            assert_eq!(
                frame.memo.normalize(&candidate.normalized),
                candidate.normalized
            );
            for probe in &*candidate.probe_candidates {
                assert_eq!(frame.memo.normalize(probe), *probe);
            }
        }
        let after_project_geometry = normalize_calls();
        assert_eq!(
            normalize_calls(),
            after_project_geometry,
            "project geometry was materialized after the initial seed"
        );

        let directories = frame.node_modules_directories();
        let after_node_modules = normalize_calls();
        for directory in directories {
            assert_eq!(
                frame.memo.normalize(&directory.directory),
                directory.directory
            );
            assert_eq!(
                frame.memo.normalize(&directory.package_dir),
                directory.package_dir
            );
            assert_eq!(
                frame.memo.normalize(&directory.manifest_path),
                directory.manifest_path
            );
            for probe in &*directory.direct_probe_candidates {
                assert_eq!(frame.memo.normalize(probe), *probe);
            }
        }
        assert!(
            normalize_calls() <= after_node_modules + 1,
            "node_modules lookups may canonicalize the root sentinel once but must reuse every executable candidate spelling"
        );

        let imports_request = ResolveRequest {
            specifier: "#internal".to_string(),
            ..request
        };
        let imports_frame = ResolveFrame::for_request(&projects, &imports_request);
        imports_frame.seed_precomputed_geometry();
        let directories = imports_frame.imports_directories();
        let after_imports = normalize_calls();
        for directory in directories {
            assert_eq!(
                imports_frame.memo.normalize(&directory.directory),
                directory.directory
            );
            assert_eq!(
                imports_frame.memo.normalize(&directory.manifest_path),
                directory.manifest_path
            );
        }
        assert_eq!(
            normalize_calls(),
            after_imports,
            "package-import geometry was materialized after the initial seed"
        );

        // Revert control: remove the immediate seed from any lazy builder;
        // only that family's post-materialization lookups increase the count.
    }

    #[test]
    fn frame_operation_reuses_geometry_spelling_allocations() {
        assert_eq!(
            std::mem::size_of::<ResolveFrameOperation>(),
            1,
            "the operation discriminator must be fieldless; importer/specifier spellings live once in request-local geometry"
        );

        // Revert control: restore the owned `ResolveRequest` or independently
        // allocated project specifier in `ResolveFrameOperation`; its size
        // grows beyond the fieldless discriminator.
    }

    #[test]
    fn independent_memos_and_basis_clear_each_pay_first_use() {
        let _guard = provenance_test_lock();
        let first = ResolutionStringMemo::default();
        let second = ResolutionStringMemo::default();
        reset_normalize_calls();

        let canonical = first.normalize(r"C:\repo\src\main.ts");
        let after_first = normalize_calls();
        let _ = second.normalize(r"C:\repo\src\main.ts");
        assert_eq!(normalize_calls(), after_first + 1);

        first.clear();
        let before_new_basis = normalize_calls();
        let _ = first.normalize(&canonical);
        assert_eq!(normalize_calls(), before_new_basis + 1);

        // Negative controls: sharing a memo between frames or retaining it
        // across a basis clear makes either +1 assertion fail.
    }
}

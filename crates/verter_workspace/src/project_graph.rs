use crate::canonical_path::CanonicalPath;
use crate::membership::ConfiguredMembership;
use crate::resolver::{IdeProjectCompilerOptions, IdeProjectConfig, WorkspaceAlias};
use crate::snapshot_builder::configured_membership_from_raw;
use crate::types::ProjectOwnership;

/// Source precedence rank for a project configuration.
///
/// Projects are sorted by (rank ASC, root_length DESC). An explicit project
/// always beats discovered, which always beats inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProjectRank {
    /// Injected via API (highest precedence).
    Explicit = 0,
    /// Discovered from tsconfig/vite config.
    Discovered = 1,
    /// Synthetic fallback (lowest precedence).
    Inferred = 2,
}

/// Configuration for a single project within the workspace.
#[derive(Debug, Clone)]
pub struct VfsProjectConfig {
    /// Project root path (forward slashes, no trailing slash).
    pub root: String,
    /// Source precedence rank.
    pub rank: ProjectRank,
    /// Path to tsconfig.json (None for inferred/vite-only projects).
    pub tsconfig_path: Option<String>,
    /// Root files from tsconfig `files` + `include` - `exclude`.
    /// Empty for inferred projects (use raw FS walk instead).
    pub root_files: Vec<String>,
    /// File extensions that this project covers (e.g., [".vue", ".ts", ".tsx"]).
    pub extensions: Vec<String>,
    /// Workspace root that discovered this project.
    pub workspace_root: String,
    /// IDE alias sources (Vite aliases on fallback projects, empty on tsconfig-backed).
    pub workspace_aliases: Vec<WorkspaceAlias>,
    /// Compiler options extracted from tsconfig (baseUrl, paths).
    pub compiler_options: IdeProjectCompilerOptions,
    /// Resolved project-reference edges (canonical tsconfig paths).
    pub references: Vec<String>,
    /// Exact configured membership (the same representation the snapshot's
    /// ownership authority carries). Built from the raw parsed membership via
    /// [`configured_membership_from_raw`] on the legacy path.
    pub membership: ConfiguredMembership,
}

impl VfsProjectConfig {
    /// Check if a file path is under this project's root.
    fn is_under_root(&self, canonical_id: &str) -> bool {
        verter_span::path::is_under_dir(canonical_id, &self.root)
    }

    /// Convert to an `IdeProjectConfig` for the project resolver.
    pub fn to_ide_project_config(&self) -> IdeProjectConfig {
        let mut project = IdeProjectConfig::new(
            self.root.clone(),
            self.workspace_root.clone(),
            self.tsconfig_path.clone(),
        );
        project.workspace_aliases = self.workspace_aliases.clone();
        project.compiler_options = self.compiler_options.clone();
        project.references = self.references.clone();
        project.membership = self.membership.clone();
        project
    }
}

/// The project graph: tracks all projects in the workspace and provides
/// file-to-project ownership queries.
///
/// Projects are sorted by precedence: (rank ASC, root_length DESC).
/// First match wins for file ownership.
#[derive(Debug, Default)]
pub struct ProjectGraph {
    /// Projects sorted by precedence order.
    projects: Vec<VfsProjectConfig>,
    /// Monotonic generation counter for tracking rebuilds.
    generation: u64,
}

impl ProjectGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a project graph from a list of project configs.
    /// Sorts them by precedence: (rank ASC, CANONICAL root_length DESC).
    ///
    /// Precedence among multiple containing projects is "deepest root wins", and
    /// `is_under_root` matches on the CANONICAL form — so the tie-break must rank
    /// by canonical length too. Ranking by raw `root.len()` lets a non-canonical
    /// root (e.g. a `//?/C:/r` extended prefix, raw len 8) outrank a genuinely
    /// deeper canonical root (`c:/r/p`, len 6), winning ownership it should lose.
    pub fn from_configs(mut projects: Vec<VfsProjectConfig>) -> Self {
        projects.sort_by(|a, b| {
            let a_len = verter_span::path::canonicalize_path_cow(&a.root).len();
            let b_len = verter_span::path::canonicalize_path_cow(&b.root).len();
            a.rank.cmp(&b.rank).then_with(|| b_len.cmp(&a_len))
        });
        Self {
            projects,
            generation: 1,
        }
    }

    /// Find the owning project for a file.
    ///
    /// The returned `project_root` is CANONICAL — a project may have been
    /// constructed with a non-canonical root (the tsconfig root comes from a
    /// filesystem walk that can yield backslashes / extended prefixes on
    /// Windows), and that raw form must never leak back out as the project root.
    pub fn owner_for_file(&self, canonical_id: &str) -> Option<ProjectOwnership> {
        self.projects
            .iter()
            .find(|p| p.is_under_root(canonical_id))
            .map(|p| ProjectOwnership {
                project_root: verter_span::path::canonicalize_path(&p.root),
                tsconfig_path: p.tsconfig_path.clone(),
            })
    }

    /// List all root files across all projects matching given extensions.
    pub fn list_root_files(&self, extensions: &[&str]) -> Vec<String> {
        let mut result = Vec::new();
        for project in &self.projects {
            for file in &project.root_files {
                if extensions.iter().any(|ext| file.ends_with(ext)) {
                    result.push(file.clone());
                }
            }
        }
        result
    }

    /// Get current generation counter.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Increment generation (after rebuild).
    pub fn increment_generation(&mut self) {
        self.generation += 1;
    }

    /// Number of projects.
    pub fn len(&self) -> usize {
        self.projects.len()
    }

    /// Whether there are no projects.
    pub fn is_empty(&self) -> bool {
        self.projects.is_empty()
    }

    /// Get the project config at a given index (in precedence order).
    pub fn get(&self, index: usize) -> Option<&VfsProjectConfig> {
        self.projects.get(index)
    }

    /// Iterate over all projects in precedence order.
    pub fn iter(&self) -> impl Iterator<Item = &VfsProjectConfig> {
        self.projects.iter()
    }

    /// Convert the project graph to a `ProjectResolver` for import resolution.
    pub fn to_project_resolver(&self) -> crate::resolver::ProjectResolver {
        crate::resolver::ProjectResolver::new(
            self.projects
                .iter()
                .map(VfsProjectConfig::to_ide_project_config)
                .collect(),
        )
    }
}

/// Result of building a project graph from workspace roots.
#[cfg(not(target_arch = "wasm32"))]
pub struct ProjectGraphBuildResult {
    pub graph: ProjectGraph,
    /// Configs that need user trust before their aliases can be used.
    pub trust_required: Vec<crate::vite_config::ViteConfigTrustInfo>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ProjectGraph {
    /// Build a project graph from workspace roots by discovering tsconfigs
    /// and optionally analyzing vite configs for fallback projects.
    ///
    /// For each root:
    /// 1. Discover all tsconfig.json files and create Discovered configs
    /// 2. Create an Inferred fallback config for the root itself
    /// 3. For fallback projects without tsconfigs, optionally analyze vite.config
    pub fn from_workspace_roots(
        ws: &dyn crate::traits::WorkspaceAccess,
        roots: &[String],
        vite_opts: &crate::vite_config::ViteConfigOptions,
    ) -> ProjectGraphBuildResult {
        use crate::config::{
            discover_tsconfigs, load_compiler_options, load_project_membership,
            load_project_references,
        };
        use crate::vite_config::{analyze_vite_config, ViteConfigAnalysis, ViteConfigTrustInfo};
        use std::path::PathBuf;

        let mut projects = Vec::new();
        let mut trust_required = Vec::new();

        for root_str in roots {
            let canonical = verter_span::path::canonicalize_path(root_str);
            let root_path = PathBuf::from(&canonical);

            // Discover tsconfigs under this root
            let tsconfig_entries = discover_tsconfigs(&root_path);

            for entry in &tsconfig_entries {
                let project_root = entry.root.clone();
                let raw_membership = load_project_membership(ws, &entry.path);
                let compiler_options = load_compiler_options(ws, &entry.path);
                let references = load_project_references(ws, &entry.path);
                let membership = configured_membership_from_raw(
                    &project_root,
                    &raw_membership,
                    &compiler_options,
                );

                projects.push(VfsProjectConfig {
                    root: project_root,
                    rank: ProjectRank::Discovered,
                    tsconfig_path: Some(entry.path.clone()),
                    root_files: vec![],
                    extensions: vec![".vue".to_string(), ".ts".to_string(), ".tsx".to_string()],
                    workspace_root: canonical.clone(),
                    workspace_aliases: vec![],
                    compiler_options,
                    references,
                    membership,
                });
            }

            // Fallback project: covers the root itself as Inferred
            let has_tsconfigs = !tsconfig_entries.is_empty();
            let mut fallback_workspace_aliases = Vec::new();

            // For fallback projects without tsconfigs, optionally analyze vite.config
            if vite_opts.enabled && !has_tsconfigs {
                match analyze_vite_config(ws, &canonical) {
                    ViteConfigAnalysis::Resolved { aliases, .. } => {
                        if !aliases.is_empty() {
                            fallback_workspace_aliases = aliases
                                .iter()
                                .map(|(find, replacement)| WorkspaceAlias {
                                    find: find.clone(),
                                    replacement: replacement.clone(),
                                })
                                .collect();
                        }
                    }
                    ViteConfigAnalysis::Complex {
                        config_path,
                        reason,
                    } => {
                        let is_trusted = crate::vite_config::vite_config_is_trusted(
                            &vite_opts.trusted_files,
                            &config_path,
                        );

                        if is_trusted {
                            if let Some(np) = &vite_opts.node_path {
                                let config_path_buf = PathBuf::from(&config_path);
                                if let Some(result) =
                                    crate::vite_config::execute_trusted_vite_config(
                                        &config_path_buf,
                                        &root_path,
                                        np,
                                    )
                                {
                                    if !result.aliases.is_empty() {
                                        fallback_workspace_aliases = result
                                            .aliases
                                            .iter()
                                            .map(|(find, replacement)| WorkspaceAlias {
                                                find: find.clone(),
                                                replacement: replacement.clone(),
                                            })
                                            .collect();
                                    }
                                } else {
                                    let lkg = crate::vite_config::get_lkg_or_empty(&config_path);
                                    if !lkg.is_empty() {
                                        fallback_workspace_aliases = lkg
                                            .iter()
                                            .map(|(find, replacement)| WorkspaceAlias {
                                                find: find.clone(),
                                                replacement: replacement.clone(),
                                            })
                                            .collect();
                                    }
                                }
                            }
                        } else {
                            trust_required.push(ViteConfigTrustInfo {
                                config_path,
                                workspace_root: canonical.clone(),
                                reason,
                            });
                        }
                    }
                    ViteConfigAnalysis::NotFound => {}
                }
            }

            let fallback_membership =
                ConfiguredMembership::match_all_under_root(&CanonicalPath::new(&canonical));
            projects.push(VfsProjectConfig {
                root: canonical.clone(),
                rank: ProjectRank::Inferred,
                tsconfig_path: None,
                root_files: vec![],
                extensions: vec![".vue".to_string(), ".ts".to_string(), ".tsx".to_string()],
                workspace_root: canonical,
                workspace_aliases: fallback_workspace_aliases,
                compiler_options: IdeProjectCompilerOptions::default(),
                references: vec![],
                membership: fallback_membership,
            });
        }

        let graph = ProjectGraph::from_configs(projects);
        ProjectGraphBuildResult {
            graph,
            trust_required,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(root: &str) -> VfsProjectConfig {
        VfsProjectConfig {
            root: root.to_string(),
            rank: ProjectRank::Inferred,
            tsconfig_path: None,
            root_files: vec![],
            extensions: vec![".vue".to_string()],
            workspace_root: root.to_string(),
            workspace_aliases: vec![],
            compiler_options: IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: ConfiguredMembership::match_all_under_root(&CanonicalPath::new(root)),
        }
    }

    #[test]
    fn is_under_root_is_case_preserving() {
        // Regression: the old whole-path lowercase normalize collapsed distinct
        // case. On case-sensitive filesystems `/proj/App` does NOT contain
        // `/proj/app/x`.
        let project = cfg("/proj/App");
        assert!(!project.is_under_root("/proj/app/x"));
        // Same casing still matches.
        assert!(cfg("/proj/app").is_under_root("/proj/app/x"));
    }

    #[test]
    fn is_under_root_rejects_sibling_prefix() {
        let project = cfg("/proj/App");
        assert!(!project.is_under_root("/proj/Appendix/x"));
        assert!(project.is_under_root("/proj/App"));
        assert!(project.is_under_root("/proj/App/sub/x.vue"));
    }

    #[test]
    fn owner_for_file_returns_canonical_root_not_raw() {
        // A project constructed with a raw extended-prefix root must not leak
        // that raw form back as the project root — `owner_for_file` returns the
        // canonical `c:/repo/pkg`, never the stored `//?/C:/repo/pkg`.
        let graph = ProjectGraph::from_configs(vec![cfg("//?/C:/repo/pkg")]);
        let owner = graph.owner_for_file("c:/repo/pkg/App.vue").unwrap();
        assert_eq!(owner.project_root, "c:/repo/pkg");
        assert_ne!(owner.project_root, "//?/C:/repo/pkg");
    }

    #[test]
    fn from_configs_precedence_ranks_by_canonical_length() {
        // Two same-rank projects both contain the file after canonicalization;
        // the genuinely deeper canonical root (`c:/r/p`) must win precedence,
        // NOT the shallower raw `//?/C:/r` whose inflated raw length (8 > 6)
        // would otherwise sort first and win ownership it should lose.
        let graph = ProjectGraph::from_configs(vec![cfg("//?/C:/r"), cfg("c:/r/p")]);
        let owner = graph.owner_for_file("c:/r/p/App.vue").unwrap();
        assert_eq!(owner.project_root, "c:/r/p");
        // Order-independence: reversed input picks the same canonical-deeper root.
        let graph2 = ProjectGraph::from_configs(vec![cfg("c:/r/p"), cfg("//?/C:/r")]);
        assert_eq!(
            graph2
                .owner_for_file("c:/r/p/App.vue")
                .unwrap()
                .project_root,
            "c:/r/p"
        );
    }
}

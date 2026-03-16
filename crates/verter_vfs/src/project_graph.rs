use crate::resolver::{
    IdeProjectCompilerOptions, IdeProjectConfig, ProjectMembership, WorkspaceAlias,
};
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
    /// Membership filter from tsconfig (files/include/exclude).
    pub membership: ProjectMembership,
}

impl VfsProjectConfig {
    /// Check if a file path is under this project's root.
    fn is_under_root(&self, canonical_id: &str) -> bool {
        let normalized = normalize_path(canonical_id);
        let root = normalize_path(&self.root);
        normalized.starts_with(&root)
            && (normalized.len() == root.len()
                || normalized.as_bytes().get(root.len()) == Some(&b'/'))
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
    /// Sorts them by precedence: (rank ASC, root_length DESC).
    pub fn from_configs(mut projects: Vec<VfsProjectConfig>) -> Self {
        projects.sort_by(|a, b| {
            a.rank
                .cmp(&b.rank)
                .then_with(|| b.root.len().cmp(&a.root.len()))
        });
        Self {
            projects,
            generation: 1,
        }
    }

    /// Find the owning project for a file.
    pub fn owner_for_file(&self, canonical_id: &str) -> Option<ProjectOwnership> {
        self.projects
            .iter()
            .find(|p| p.is_under_root(canonical_id))
            .map(|p| ProjectOwnership {
                project_root: p.root.clone(),
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
            let canonical = root_str.replace('\\', "/");
            let root_path = PathBuf::from(&canonical);

            // Discover tsconfigs under this root
            let tsconfig_entries = discover_tsconfigs(&root_path);

            for entry in &tsconfig_entries {
                let project_root = entry.root.clone();
                let tsconfig_path_buf = PathBuf::from(&entry.path);
                let membership = load_project_membership(&tsconfig_path_buf);
                let compiler_options = load_compiler_options(&tsconfig_path_buf);
                let references = load_project_references(&tsconfig_path_buf);

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
                match analyze_vite_config(&root_path) {
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
                        let is_trusted = vite_opts.trusted_files.iter().any(|tf| {
                            let tf_normalized = tf.replace('\\', "/");
                            tf_normalized == config_path
                        });

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
                membership: ProjectMembership::MatchAll,
            });
        }

        let graph = ProjectGraph::from_configs(projects);
        ProjectGraphBuildResult {
            graph,
            trust_required,
        }
    }
}

/// Normalize a path to lowercase with forward slashes for comparison.
fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

#[cfg(test)]
#[path = "project_graph_tests.rs"]
mod tests;

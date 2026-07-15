//! LSP workspace views: per-project lint, SSR, and vite trust state.
//!
//! [`LspViews`] is the consumer extension stored in
//! [`PublishedRoot::consumer_ext`]. It provides LSP-specific per-project
//! state that depends on the ownership snapshot but is NOT part of VFS.
//!
//! # Relationship to `WorkspaceSnapshot`
//!
//! `LspViews` is always derived from a `WorkspaceSnapshot` — it has one
//! `LspProjectView` per `OwnershipProject` in the snapshot, at the same
//! index. The `ProjectId` used in the snapshot also indexes into
//! `project_views`.
//!
//! For LSP-views-only rebuilds (e.g., `.verterrc.json` changed), the
//! existing `Arc<WorkspaceSnapshot>` is reused and only `LspViews` is
//! rebuilt.

use verter_diagnostics::{Linter, ResolvedLintConfig};
use verter_workspace::workspace_snapshot::ProjectId;

use verter_workspace::{ViteConfigTrustInfo, WorkspaceRead};

/// LSP-specific per-project view.
///
/// One view per `OwnershipProject` in the snapshot, at the same index.
/// Access via `ProjectId` from the snapshot.
pub struct LspProjectView {
    /// Lint configuration resolved from `.verterrc.json` or defaults.
    pub lint_config: ResolvedLintConfig,
    /// Pre-built linter instance (cached to avoid repeated construction).
    pub linter: Linter,
    /// Whether lint was explicitly configured (`.verterrc.json` found).
    pub lint_explicitly_configured: bool,
    /// Path to the analyzed vite config file (fallback projects only).
    pub vite_config_path: Option<String>,
    /// Config file + helper deps for invalidation (fallback projects only).
    pub vite_config_deps: Vec<String>,
    /// Whether this project uses SSR (Nuxt detection or `.verterrc.json`).
    pub ssr_enabled: bool,
}

/// LSP workspace views: stored in `PublishedRoot::consumer_ext`.
///
/// Provides per-project lint, SSR, and vite config state. Always
/// consistent with the published `WorkspaceSnapshot` — same number
/// of projects, same ordering, same `ProjectId` indices.
pub struct LspViews {
    /// Per-project views, indexed by `ProjectId`.
    pub project_views: Vec<LspProjectView>,
    /// Vite configs that need user trust approval.
    pub trust_required: Vec<ViteConfigTrustInfo>,
}

impl std::fmt::Debug for LspViews {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspViews")
            .field("project_count", &self.project_views.len())
            .field("trust_required", &self.trust_required.len())
            .finish()
    }
}

impl std::fmt::Debug for LspProjectView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspProjectView")
            .field("ssr_enabled", &self.ssr_enabled)
            .field(
                "lint_explicitly_configured",
                &self.lint_explicitly_configured,
            )
            .finish_non_exhaustive()
    }
}

impl LspViews {
    /// Get the view for a project by ID.
    pub fn view(&self, id: ProjectId) -> &LspProjectView {
        &self.project_views[id.0 as usize]
    }

    /// Find the linter for a file by consulting the snapshot for ownership.
    ///
    /// Returns the unique owner view, or `None` if the file is unowned or
    /// ambiguously owned.
    pub fn linter_view_for_file(
        &self,
        snapshot: &verter_workspace::WorkspaceSnapshot,
        canonical_id: &str,
    ) -> Option<&LspProjectView> {
        view_owner_for_file(snapshot, canonical_id).map(|id| self.view(id))
    }

    /// Find the project root for a file (for tsserver `projectRootPath`).
    ///
    /// Returns the root of the unique owner project, or `None` if the file
    /// is unowned or ambiguously owned.
    pub fn find_project_root<'a>(
        &self,
        snapshot: &'a verter_workspace::WorkspaceSnapshot,
        canonical_id: &str,
    ) -> Option<&'a str> {
        view_owner_for_file(snapshot, canonical_id).map(|id| snapshot.project(id).root.as_str())
    }

    /// Check if a file is in an SSR context.
    ///
    /// Tier 1: `*.server.vue` → always SSR
    /// Tier 2: `*.client.vue` → never SSR
    /// Tier 3: Inherit from project `ssr_enabled`
    pub fn is_ssr_context(
        &self,
        snapshot: &verter_workspace::WorkspaceSnapshot,
        canonical_id: &str,
    ) -> bool {
        // Tier 1/2: filename suffix override
        if canonical_id.ends_with(".server.vue") {
            return true;
        }
        if canonical_id.ends_with(".client.vue") {
            return false;
        }

        // Tier 3: inherit from project
        view_owner_for_file(snapshot, canonical_id)
            .map(|id| self.view(id).ssr_enabled)
            .unwrap_or(false)
    }
}

/// The single view-owner project for a file: the unique configured owner, else — only
/// on an authoritative configured-`None` — the single fallback owner. A genuine
/// configured overlap (`Ambiguous`) fails closed to `None` and NEVER falls through to
/// a fallback (and a fallback never becomes a configured owner). This is the
/// per-project VIEW lookup (linter / `projectRootPath` / SSR), kept DISTINCT from the
/// carrier-ownership authority (`external_ts::CarrierOwnershipResolution`) — it is not
/// a generic path→singleton selector and never invents a winner for an overlap.
fn view_owner_for_file(
    snapshot: &verter_workspace::WorkspaceSnapshot,
    canonical_id: &str,
) -> Option<ProjectId> {
    use verter_workspace::workspace_snapshot::ConfiguredOwnerResolution;
    match snapshot.configured_owner_resolution_for_file(canonical_id) {
        ConfiguredOwnerResolution::Unique(id) => Some(id),
        ConfiguredOwnerResolution::Ambiguous(_) => None,
        ConfiguredOwnerResolution::None => snapshot.single_fallback_owner_for_file(canonical_id),
    }
}

/// Build `LspProjectView` entries for all projects in a snapshot.
///
/// For each `OwnershipProject`, discovers lint config from the project root
/// and detects SSR projects. All file reads route through the supplied
/// [`WorkspaceRead`] authority so overlays and snapshot caches are honored.
pub fn build_lsp_views(
    workspace: &dyn WorkspaceRead,
    snapshot: &verter_workspace::WorkspaceSnapshot,
    trust_required: Vec<ViteConfigTrustInfo>,
) -> LspViews {
    let mut project_views = Vec::with_capacity(snapshot.projects.len());

    for project in &snapshot.projects {
        let root = project.root.as_str();

        // Discover lint config
        let lint_config = verter_diagnostics::discover_lint_config(workspace, root);
        let lint_explicitly_configured = lint_config.explicitly_configured;
        let linter = Linter::new(lint_config.config.clone());

        // Detect SSR
        let ssr_enabled = detect_ssr_project(workspace, root, &lint_config);

        // Vite config metadata (fallback projects only)
        let (vite_config_path, vite_config_deps) = if project.is_fallback() {
            // In a full implementation, this would analyze vite.config
            // For now, these are populated during the build pipeline
            (None, Vec::new())
        } else {
            (None, Vec::new())
        };

        project_views.push(LspProjectView {
            lint_config,
            linter,
            lint_explicitly_configured,
            vite_config_path,
            vite_config_deps,
            ssr_enabled,
        });
    }

    LspViews {
        project_views,
        trust_required,
    }
}

/// Detect whether a project root is an SSR project.
///
/// Checks Nuxt config, `.nuxt/` directory, and `.verterrc.json` ssr_mode.
/// File-system probes route through `workspace` so overlays and snapshot
/// caches are honored.
fn detect_ssr_project(
    workspace: &dyn WorkspaceRead,
    root: &str,
    lint_config: &ResolvedLintConfig,
) -> bool {
    if lint_config.config.ssr_mode {
        return true;
    }

    let trimmed = root.trim_end_matches('/');
    for ext in &["ts", "js", "mjs", "mts"] {
        if workspace.file_exists(&format!("{trimmed}/nuxt.config.{ext}")) {
            return true;
        }
    }

    if workspace.is_dir(&format!("{trimmed}/.nuxt")) {
        return true;
    }

    false
}

/// Merge VS Code initialization lint options into all non-explicit project views.
///
/// For projects where lint was NOT explicitly configured (no `.verterrc.json`),
/// applies the VS Code-level lint settings.
pub fn apply_default_lint_to_views(views: &mut LspViews, init_options: &serde_json::Value) {
    for view in &mut views.project_views {
        if !view.lint_explicitly_configured {
            crate::config::merge_init_options(&mut view.lint_config, init_options);
            view.linter = Linter::new(view.lint_config.config.clone());
        }
    }
}

/// Propagate `conditional_root_narrowing` to all views and rebuild linters.
pub fn set_conditional_root_narrowing(views: &mut LspViews, enabled: bool) {
    for view in &mut views.project_views {
        view.lint_config.config.conditional_root_narrowing = enabled;
        view.linter = Linter::new(view.lint_config.config.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_workspace::workspace_snapshot::{
        OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration, WorkspaceSnapshot,
    };
    use verter_workspace::ViteConfigTrustInfo;
    use verter_workspace::{
        CanonicalPath, CompiledGlob, FallbackMembership, MemoryOptions, MemoryWorkspace,
        NormalizedGlob, ProjectResolver,
    };

    fn empty_workspace() -> MemoryWorkspace {
        MemoryWorkspace::new(MemoryOptions::default())
    }

    fn fallback_project(id: u32, root: &str) -> OwnershipProject {
        let root_cp = CanonicalPath::new(root);
        OwnershipProject {
            id: ProjectId(id),
            root: root_cp.clone(),
            workspace_root: root_cp.clone(),
            payload: ProjectPayload::Fallback {
                membership: FallbackMembership {
                    root: root_cp,
                    exclude: vec![CompiledGlob::new(NormalizedGlob::new(&format!(
                        "{}/node_modules/**",
                        root
                    )))],
                },
            },
        }
    }

    fn empty_snapshot(projects: Vec<OwnershipProject>) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            projects,
            resolver: ProjectResolver::default(),
            generation: SnapshotGeneration(1),
        }
    }

    fn configured_project(id: u32, root: &str, tsconfig: &str, files: &[&str]) -> OwnershipProject {
        let root_cp = CanonicalPath::new(root);
        OwnershipProject {
            id: ProjectId(id),
            root: root_cp.clone(),
            workspace_root: root_cp.clone(),
            payload: ProjectPayload::Configured {
                tsconfig_path: CanonicalPath::new(tsconfig),
                membership: verter_workspace::ConfiguredMembership {
                    spec: verter_workspace::StaticMembershipSpec {
                        files: files.iter().map(|f| CanonicalPath::new(f)).collect(),
                        include: Vec::new(),
                        exclude: Vec::new(),
                    },
                    materialized_files: files.iter().map(|f| CanonicalPath::new(f)).collect(),
                },
                compiler_options: Default::default(),
                references: vec![],
                workspace_aliases: vec![],
            },
        }
    }

    #[test]
    fn build_lsp_views_creates_one_per_project() {
        let snap = empty_snapshot(vec![
            fallback_project(0, "d:/project"),
            fallback_project(1, "d:/other"),
        ]);

        let ws = empty_workspace();
        let views = build_lsp_views(&ws, &snap, vec![]);
        assert_eq!(views.project_views.len(), 2);
    }

    #[test]
    fn linter_view_for_file_finds_owner() {
        let snap = empty_snapshot(vec![fallback_project(0, "d:/project")]);
        let ws = empty_workspace();
        let views = build_lsp_views(&ws, &snap, vec![]);

        let view = views.linter_view_for_file(&snap, "d:/project/src/foo.vue");
        assert!(view.is_some());
    }

    #[test]
    fn linter_view_for_file_returns_none_outside_projects() {
        let snap = empty_snapshot(vec![fallback_project(0, "d:/project")]);
        let ws = empty_workspace();
        let views = build_lsp_views(&ws, &snap, vec![]);

        let view = views.linter_view_for_file(&snap, "d:/other/foo.vue");
        assert!(view.is_none());
    }

    #[test]
    fn ambiguous_configured_file_has_no_single_linter_view() {
        let shared = "d:/project/src/shared.ts";
        let snap = empty_snapshot(vec![
            configured_project(0, "d:/project", "d:/project/tsconfig.app.json", &[shared]),
            configured_project(
                1,
                "d:/project",
                "d:/project/tsconfig.vitest.json",
                &[shared],
            ),
        ]);
        let ws = empty_workspace();
        let views = build_lsp_views(&ws, &snap, vec![]);

        assert!(
            views.linter_view_for_file(&snap, shared).is_none(),
            "single-view helpers must not invent a winner for ambiguous configured ownership"
        );
        assert!(
            views.find_project_root(&snap, shared).is_none(),
            "ambiguous configured ownership must not collapse to a single project root"
        );
        assert!(
            !views.is_ssr_context(&snap, shared),
            "SSR context helper must fail closed for ambiguous configured ownership"
        );
    }

    #[test]
    fn ssr_context_server_vue_always_true() {
        let snap = empty_snapshot(vec![fallback_project(0, "d:/project")]);
        let ws = empty_workspace();
        let views = build_lsp_views(&ws, &snap, vec![]);

        assert!(views.is_ssr_context(&snap, "d:/project/pages/index.server.vue"));
    }

    #[test]
    fn ssr_context_client_vue_always_false() {
        let snap = empty_snapshot(vec![fallback_project(0, "d:/project")]);
        let ws = empty_workspace();
        let views = build_lsp_views(&ws, &snap, vec![]);

        assert!(!views.is_ssr_context(&snap, "d:/project/pages/index.client.vue"));
    }

    #[test]
    fn ssr_context_inherits_from_project() {
        let snap = empty_snapshot(vec![fallback_project(0, "d:/project")]);
        let ws = empty_workspace();
        let views = build_lsp_views(&ws, &snap, vec![]);

        // Default project is not SSR
        assert!(!views.is_ssr_context(&snap, "d:/project/pages/index.vue"));
    }

    #[test]
    fn trust_required_stored() {
        let snap = empty_snapshot(vec![]);
        let trust = vec![ViteConfigTrustInfo {
            config_path: "d:/project/vite.config.ts".to_string(),
            workspace_root: "d:/project".to_string(),
            reason: "function export".to_string(),
        }];

        let ws = empty_workspace();
        let views = build_lsp_views(&ws, &snap, trust);
        assert_eq!(views.trust_required.len(), 1);
        assert_eq!(
            views.trust_required[0].config_path,
            "d:/project/vite.config.ts"
        );
    }

    #[test]
    fn views_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<LspViews>();
    }
}

use std::path::PathBuf;

/// Check if a workspace has any solution-style `tsconfig.json` (non-empty `references` array).
/// TSGO cannot resolve path aliases from referenced tsconfig files, so this is used by
/// auto-mode provider selection to prefer tsserver when composite tsconfigs are detected.
///
/// All file reads route through `workspace` so overlays and snapshot caches
/// are honored.
pub fn has_solution_style_tsconfig(
    workspace: &dyn verter_workspace::WorkspaceAccess,
    workspace_root: &str,
) -> bool {
    verter_workspace::config::has_solution_style_tsconfig(workspace, workspace_root)
}

pub use verter_diagnostics::{
    discover_lint_config, parse_rule_severity, strip_json_comments, strip_trailing_commas,
    ResolvedLintConfig, VerterProjectConfig,
};

/// Merge VS Code initialization options into a resolved lint config.
pub fn merge_init_options(resolved: &mut ResolvedLintConfig, init_options: &serde_json::Value) {
    if let Some(lint) = init_options.get("lint") {
        if let Some(enabled) = lint.get("enabled").and_then(|v| v.as_bool()) {
            resolved.explicitly_configured = enabled;
        }
        if let Some(preset) = lint.get("preset").and_then(|v| v.as_str()) {
            resolved.config.preset = match preset {
                "essential" => verter_diagnostics::LintPreset::Essential,
                "recommended" => verter_diagnostics::LintPreset::Recommended,
                "all" => verter_diagnostics::LintPreset::All,
                "performance" => verter_diagnostics::LintPreset::Performance,
                "a11y" => verter_diagnostics::LintPreset::A11y,
                "strict" => verter_diagnostics::LintPreset::Strict,
                _ => resolved.config.preset,
            };
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExperimentalInitOptions {
    pub conditional_root_narrowing: bool,
    pub strict_slots: bool,
}

pub fn parse_experimental_init_options(
    init_options: &serde_json::Value,
) -> ExperimentalInitOptions {
    let experimental = init_options.get("experimental");
    ExperimentalInitOptions {
        conditional_root_narrowing: experimental
            .and_then(|v| v.get("conditionalRootNarrowing"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        strict_slots: experimental
            .and_then(|v| v.get("strictSlots"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    }
}

/// Hover-related init options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HoverOptions {
    /// When `true`, hover responses are enriched with a provenance
    /// markdown section showing files loaded and derivation chain.
    /// Default `false` — opt-in.
    pub provenance: bool,
}

/// Parse `HoverOptions` from `initializationOptions.hover.*` (or
/// equivalent). Robust against missing keys and unexpected types —
/// returns defaults for any unrecognized shape.
pub fn parse_hover_init_options(init_options: &serde_json::Value) -> HoverOptions {
    let hover = init_options.get("hover");
    HoverOptions {
        provenance: hover
            .and_then(|v| v.get("provenance"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    }
}

#[cfg(test)]
mod config_migration_tests {
    use super::*;

    #[test]
    fn parse_severity_string() {
        assert_eq!(parse_rule_severity(&serde_json::json!("off")), None);
        assert_eq!(
            parse_rule_severity(&serde_json::json!("warn")),
            Some(verter_diagnostics::Severity::Warning)
        );
        assert_eq!(
            parse_rule_severity(&serde_json::json!("error")),
            Some(verter_diagnostics::Severity::Error)
        );
    }

    #[test]
    fn parse_severity_number() {
        assert_eq!(parse_rule_severity(&serde_json::json!(0)), None);
        assert_eq!(
            parse_rule_severity(&serde_json::json!(1)),
            Some(verter_diagnostics::Severity::Warning)
        );
        assert_eq!(
            parse_rule_severity(&serde_json::json!(2)),
            Some(verter_diagnostics::Severity::Error)
        );
    }

    #[test]
    fn parse_severity_array() {
        assert_eq!(
            parse_rule_severity(&serde_json::json!(["error", {}])),
            Some(verter_diagnostics::Severity::Error)
        );
        assert_eq!(parse_rule_severity(&serde_json::json!(["off"])), None);
    }

    #[test]
    fn verterrc_json_roundtrip() {
        let json = r#"{"lint":{"enabled":true,"preset":"strict","rules":{"no-v-html":"error","unused-css-selector":"off"}}}"#;
        let config: VerterProjectConfig = serde_json::from_str(json).unwrap();
        let lint = config.lint.unwrap();
        assert_eq!(lint.enabled, Some(true));
        assert_eq!(lint.preset.as_deref(), Some("strict"));
        assert!(lint.rules.is_some());
        let rules = lint.rules.unwrap();
        assert_eq!(rules.get("no-v-html").unwrap(), "error");
        assert_eq!(rules.get("unused-css-selector").unwrap(), "off");
    }

    #[test]
    fn merge_init_options_applies_preset() {
        let mut resolved = ResolvedLintConfig::default();
        let opts = serde_json::json!({
            "lint": {
                "enabled": true,
                "preset": "strict"
            }
        });
        merge_init_options(&mut resolved, &opts);
        assert!(resolved.explicitly_configured);
        assert_eq!(
            resolved.config.preset,
            verter_diagnostics::LintPreset::Strict
        );
    }

    #[test]
    fn parse_hover_init_options_defaults_to_provenance_false() {
        // Provenance is opt-in; default is false.
        let opts = serde_json::json!({});
        assert_eq!(
            parse_hover_init_options(&opts),
            HoverOptions { provenance: false }
        );
    }

    #[test]
    fn parse_hover_init_options_reads_provenance_flag() {
        let opts = serde_json::json!({ "hover": { "provenance": true } });
        assert_eq!(
            parse_hover_init_options(&opts),
            HoverOptions { provenance: true }
        );
    }

    #[test]
    fn parse_hover_init_options_ignores_wrong_type() {
        let opts = serde_json::json!({ "hover": { "provenance": "yes" } });
        assert_eq!(
            parse_hover_init_options(&opts),
            HoverOptions { provenance: false },
            "non-bool provenance value should fall back to default"
        );
    }

    #[test]
    fn parse_experimental_init_options_defaults_to_false() {
        let opts = serde_json::json!({});
        assert_eq!(
            parse_experimental_init_options(&opts),
            ExperimentalInitOptions::default()
        );
    }

    #[test]
    fn parse_experimental_init_options_reads_supported_flags() {
        let opts = serde_json::json!({
            "experimental": {
                "conditionalRootNarrowing": true,
                "strictSlots": true
            }
        });
        assert_eq!(
            parse_experimental_init_options(&opts),
            ExperimentalInitOptions {
                conditional_root_narrowing: true,
                strict_slots: true,
            }
        );
    }

    fn fs_workspace() -> verter_workspace::FilesystemWorkspace {
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default())
    }

    fn canonical_str(path: &std::path::Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn discover_no_config_returns_default() {
        let tmp = std::env::temp_dir().join("verter_test_no_config");
        let _ = std::fs::create_dir_all(&tmp);
        let ws = fs_workspace();
        let result = discover_lint_config(&ws, &canonical_str(&tmp));
        assert!(!result.explicitly_configured);
        assert_eq!(
            result.config.preset,
            verter_diagnostics::LintPreset::Recommended
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_verterrc_json() {
        let tmp = std::env::temp_dir().join("verter_test_verterrc");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(
            tmp.join(".verterrc.json"),
            r#"{"lint":{"preset":"essential","rules":{"no-v-html":"off"}}}"#,
        )
        .unwrap();
        let ws = fs_workspace();
        let result = discover_lint_config(&ws, &canonical_str(&tmp));
        assert!(result.explicitly_configured);
        assert_eq!(
            result.config.preset,
            verter_diagnostics::LintPreset::Essential
        );
        assert_eq!(result.config.rules.get("no-v-html"), Some(&None));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_eslintrc_json() {
        let tmp = std::env::temp_dir().join("verter_test_eslintrc");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(
            tmp.join(".eslintrc.json"),
            r#"{"extends":["plugin:vue/vue3-recommended"],"rules":{"vue/no-v-html":"error"}}"#,
        )
        .unwrap();
        let ws = fs_workspace();
        let result = discover_lint_config(&ws, &canonical_str(&tmp));
        assert!(result.explicitly_configured);
        assert_eq!(
            result.config.preset,
            verter_diagnostics::LintPreset::Recommended
        );
        assert_eq!(
            result.config.rules.get("no-v-html"),
            Some(&Some(verter_diagnostics::Severity::Error))
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Component diagnostics configuration
// ═══════════════════════════════════════════════════════════════════════════

/// Diagnostic severity configuration for component usage checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverityConfig {
    Error,
    Warning,
    Information,
    Hint,
    Off,
}

/// Configuration for component usage diagnostics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentDiagnosticsConfig {
    pub unknown_prop_severity: DiagnosticSeverityConfig,
    pub unknown_model_severity: DiagnosticSeverityConfig,
}

impl Default for ComponentDiagnosticsConfig {
    fn default() -> Self {
        Self {
            unknown_prop_severity: DiagnosticSeverityConfig::Warning,
            unknown_model_severity: DiagnosticSeverityConfig::Warning,
        }
    }
}

impl DiagnosticSeverityConfig {
    /// Convert to LSP severity, or None if Off.
    pub fn to_lsp(self) -> Option<tower_lsp_server::ls_types::DiagnosticSeverity> {
        use tower_lsp_server::ls_types::DiagnosticSeverity;
        match self {
            Self::Error => Some(DiagnosticSeverity::ERROR),
            Self::Warning => Some(DiagnosticSeverity::WARNING),
            Self::Information => Some(DiagnosticSeverity::INFORMATION),
            Self::Hint => Some(DiagnosticSeverity::HINT),
            Self::Off => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Per-Project Configuration (monorepo / multi-root workspace support)
// ═══════════════════════════════════════════════════════════════════════════

/// Per-project configuration grouping path alias resolution and lint config.
///
/// In a monorepo, each package may have its own `tsconfig.json` (with different
/// `paths` aliases), `.verterrc.json` (with different lint rules), and vite config.
/// `ProjectConfig` captures all of these for a single project root.
pub struct ProjectConfig {
    /// Directory this project covers (e.g., `packages/ui/`). Always forward slashes.
    pub root: String,
    /// Workspace folder that discovered this project.
    pub workspace_root: String,
    /// Canonical tsconfig path when this project is backed by a discovered config.
    pub tsconfig_path: Option<String>,
    /// Resolved tsconfig file membership for owner selection.
    pub membership: crate::project_resolver::ProjectMembership,
    /// Existing IDE alias sources (currently Vite aliases) injected ahead of tsconfig paths.
    pub workspace_aliases: Vec<crate::project_resolver::WorkspaceAlias>,
    /// Preserved tsconfig compiler options for the native resolver.
    pub compiler_options: crate::project_resolver::IdeProjectCompilerOptions,
    /// Resolved project-reference edges for the native resolver.
    pub references: Vec<String>,
    /// Lint configuration for this project.
    pub lint_config: ResolvedLintConfig,
    /// Linter instance built from `lint_config`. Cached to avoid recreating.
    pub linter: verter_diagnostics::Linter,
    /// Whether lint was explicitly configured for this project.
    pub lint_explicitly_configured: bool,
    /// Path to the analyzed vite config file (only on fallback projects).
    pub vite_config_path: Option<String>,
    /// Config file + helper deps for invalidation (canonical absolute paths).
    /// Always includes the config file itself. Only populated on fallback projects.
    pub vite_config_deps: Vec<String>,
    /// Whether this project uses SSR (detected from Nuxt, `.verterrc.json`, or init options).
    pub ssr_enabled: bool,
}

/// Result of building a project registry, including trust-required entries.
pub struct RegistryBuildResult {
    pub registry: ProjectRegistry,
    /// Configs that need user trust before their aliases can be used.
    pub trust_required: Vec<verter_workspace::ViteConfigTrustInfo>,
}

/// Detect whether a project root is an SSR project.
///
/// Returns `true` if:
/// - `nuxt.config.{ts,js,mjs,mts}` exists (Nuxt project)
/// - `.nuxt/` directory exists
/// - `.verterrc.json` has `"ssr": { "enabled": true }`
///
/// File-system probes route through `workspace` so overlays and snapshot
/// caches are honored.
fn detect_ssr_project(
    workspace: &dyn verter_workspace::WorkspaceRead,
    root: &str,
    lint_config: &ResolvedLintConfig,
) -> bool {
    // Check if the lint config already has ssr_mode set (from .verterrc.json parsing)
    if lint_config.config.ssr_mode {
        return true;
    }

    let trimmed = root.trim_end_matches('/');

    // Detect Nuxt: nuxt.config.{ts,js,mjs,mts}
    for ext in &["ts", "js", "mjs", "mts"] {
        if workspace.file_exists(&format!("{trimmed}/nuxt.config.{ext}")) {
            return true;
        }
    }

    // Detect Nuxt: .nuxt/ directory
    if workspace.is_dir(&format!("{trimmed}/.nuxt")) {
        return true;
    }

    false
}

/// Check if a file path indicates SSR context (e.g., `*.server.vue`).
pub fn is_ssr_file(path: &str) -> bool {
    path.ends_with(".server.vue")
}

/// Check if a file path indicates client-only context (e.g., `*.client.vue`).
pub fn is_client_only_file(path: &str) -> bool {
    path.ends_with(".client.vue")
}

impl ProjectConfig {
    pub fn to_ide_project_config(&self) -> crate::project_resolver::IdeProjectConfig {
        let mut project = crate::project_resolver::IdeProjectConfig::new(
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

/// Registry of per-project configurations for a multi-root workspace.
///
/// Projects are sorted by root prefix length (longest first) so that
/// `find_project()` returns the most specific match.
pub struct ProjectRegistry {
    /// Sorted by root length descending (longest prefix first).
    projects: Vec<ProjectConfig>,
}

impl ProjectRegistry {
    /// Build a registry from workspace roots by discovering tsconfigs, vite configs,
    /// and lint configs.
    ///
    /// For each root, discovers all `tsconfig.json` files, builds per-project resolvers,
    /// and discovers lint config. If a root has no tsconfig, a default project is created
    /// with empty aliases.
    ///
    /// Build a registry from workspace roots. Tsconfig-backed projects use tsconfig
    /// paths exclusively; fallback (no-tsconfig) projects get vite aliases via static
    /// analysis or trusted execution.
    ///
    /// All file reads route through `workspace` so overlays and snapshot
    /// caches are honored.
    pub fn from_workspace_roots(
        workspace: &dyn verter_workspace::WorkspaceAccess,
        roots: &[String],
        vite_opts: &verter_workspace::ViteConfigOptions,
    ) -> RegistryBuildResult {
        let mut projects = Vec::new();
        let mut trust_required = Vec::new();

        for root_uri in roots {
            let canonical = verter_workspace::resolver::normalize_canonical_id(
                &crate::documents::uri_to_canonical_id_from_str(root_uri),
            );
            let root_path = PathBuf::from(&canonical);

            // Discover tsconfigs under this root (VFS config)
            let discovered = verter_workspace::config::discover_tsconfigs(&root_path);

            for entry in &discovered {
                let project_root = entry.root.clone();
                let membership =
                    verter_workspace::config::load_project_membership(workspace, &entry.path);
                let compiler_options =
                    verter_workspace::config::load_compiler_options(workspace, &entry.path);
                let references =
                    verter_workspace::config::load_project_references(workspace, &entry.path);
                // Tsconfig-backed projects use tsconfig paths as the sole alias source.
                // Vite aliases are only applied to fallback (no-tsconfig) projects.
                let workspace_aliases = Vec::new();

                let lint = discover_lint_config(workspace, &project_root);
                let ssr_enabled = detect_ssr_project(workspace, &project_root, &lint);
                let linter = verter_diagnostics::Linter::new(lint.config.clone());

                projects.push(ProjectConfig {
                    root: project_root,
                    workspace_root: canonical.clone(),
                    tsconfig_path: Some(entry.path.clone()),
                    membership,
                    workspace_aliases,
                    compiler_options,
                    references,
                    lint_config: lint.clone(),
                    linter,
                    lint_explicitly_configured: lint.explicitly_configured,
                    vite_config_path: None,
                    vite_config_deps: Vec::new(),
                    ssr_enabled,
                });
            }

            // Fallback project (no tsconfig) — apply vite aliases if enabled.
            // Skip vite analysis when tsconfigs were found for this root: those projects
            // already own alias resolution and the fallback is only a catch-all.
            let has_tsconfigs = !discovered.is_empty();
            let lint = discover_lint_config(workspace, &canonical);
            let linter = verter_diagnostics::Linter::new(lint.config.clone());
            let mut fallback_workspace_aliases = Vec::new();
            let mut fallback_vite_config_path = None;
            let mut fallback_vite_config_deps = Vec::new();

            if vite_opts.enabled && !has_tsconfigs {
                use verter_workspace::{analyze_vite_config, ViteConfigAnalysis};
                match analyze_vite_config(workspace, &canonical) {
                    ViteConfigAnalysis::Resolved {
                        config_path,
                        aliases,
                        dependency_files,
                    } => {
                        if !aliases.is_empty() {
                            tracing::debug!(
                                "statically resolved {} vite aliases for {}",
                                aliases.len(),
                                canonical
                            );
                            fallback_workspace_aliases = aliases
                                .iter()
                                .map(|(find, replacement)| {
                                    crate::project_resolver::WorkspaceAlias {
                                        find: find.clone(),
                                        replacement: replacement.clone(),
                                    }
                                })
                                .collect();
                        }
                        fallback_vite_config_path = Some(config_path);
                        fallback_vite_config_deps = dependency_files;
                    }
                    ViteConfigAnalysis::Complex {
                        config_path,
                        reason,
                    } => {
                        // Check if file is trusted
                        let is_trusted = vite_opts.trusted_files.iter().any(|tf| {
                            let tf_normalized = tf.replace('\\', "/");
                            tf_normalized == config_path
                        });

                        if is_trusted {
                            if let Some(np) = &vite_opts.node_path {
                                let config_path_buf = PathBuf::from(&config_path);
                                if let Some(result) = verter_workspace::execute_trusted_vite_config(
                                    &config_path_buf,
                                    &root_path,
                                    np,
                                ) {
                                    if !result.aliases.is_empty() {
                                        tracing::debug!(
                                            "trusted execution: {} vite aliases for {}",
                                            result.aliases.len(),
                                            canonical
                                        );
                                        fallback_workspace_aliases = result
                                            .aliases
                                            .iter()
                                            .map(|(find, replacement)| {
                                                crate::project_resolver::WorkspaceAlias {
                                                    find: find.clone(),
                                                    replacement: replacement.clone(),
                                                }
                                            })
                                            .collect();
                                    }
                                    fallback_vite_config_deps = result.dependency_files;
                                } else {
                                    // Execution failed, try LKG
                                    let lkg = verter_workspace::get_lkg_or_empty(&config_path);
                                    if !lkg.is_empty() {
                                        fallback_workspace_aliases = lkg
                                            .iter()
                                            .map(|(find, replacement)| {
                                                crate::project_resolver::WorkspaceAlias {
                                                    find: find.clone(),
                                                    replacement: replacement.clone(),
                                                }
                                            })
                                            .collect();
                                    }
                                }
                                fallback_vite_config_path = Some(config_path);
                            }
                        } else {
                            // Not trusted → add to trust_required
                            trust_required.push(verter_workspace::ViteConfigTrustInfo {
                                config_path: config_path.clone(),
                                workspace_root: canonical.clone(),
                                reason,
                            });
                            fallback_vite_config_path = Some(config_path);
                        }
                    }
                    ViteConfigAnalysis::NotFound => {}
                }
            }

            let ssr_enabled = detect_ssr_project(workspace, &canonical, &lint);
            projects.push(ProjectConfig {
                root: canonical,
                workspace_root: crate::documents::uri_to_canonical_id_from_str(root_uri),
                tsconfig_path: None,
                membership: crate::project_resolver::ProjectMembership::MatchAll,
                workspace_aliases: fallback_workspace_aliases,
                compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
                references: Vec::new(),
                lint_config: lint.clone(),
                linter,
                lint_explicitly_configured: lint.explicitly_configured,
                vite_config_path: fallback_vite_config_path,
                vite_config_deps: fallback_vite_config_deps,
                ssr_enabled,
            });
        }

        sort_projects(&mut projects);

        RegistryBuildResult {
            registry: Self { projects },
            trust_required,
        }
    }

    /// Build a registry from canonical paths (not URIs). Used in tests.
    ///
    /// All file reads route through `workspace` so overlays and snapshot
    /// caches are honored.
    pub fn from_canonical_roots(
        workspace: &dyn verter_workspace::WorkspaceAccess,
        roots: &[&str],
    ) -> Self {
        let mut projects = Vec::new();

        for &root in roots {
            let root = verter_workspace::resolver::normalize_canonical_id(root);
            let root_path = PathBuf::from(&root);

            let discovered = verter_workspace::config::discover_tsconfigs(&root_path);

            for entry in &discovered {
                let project_root = entry.root.clone();
                let membership =
                    verter_workspace::config::load_project_membership(workspace, &entry.path);
                let compiler_options =
                    verter_workspace::config::load_compiler_options(workspace, &entry.path);
                let references =
                    verter_workspace::config::load_project_references(workspace, &entry.path);
                let lint = discover_lint_config(workspace, &project_root);
                let ssr_enabled = detect_ssr_project(workspace, &project_root, &lint);
                let linter = verter_diagnostics::Linter::new(lint.config.clone());

                projects.push(ProjectConfig {
                    root: project_root,
                    workspace_root: root.to_string(),
                    tsconfig_path: Some(entry.path.clone()),
                    membership,
                    workspace_aliases: Vec::new(),
                    compiler_options,
                    references,
                    lint_config: lint.clone(),
                    linter,
                    lint_explicitly_configured: lint.explicitly_configured,
                    vite_config_path: None,
                    vite_config_deps: Vec::new(),
                    ssr_enabled,
                });
            }

            let lint = discover_lint_config(workspace, &root);
            let ssr_enabled = detect_ssr_project(workspace, &root, &lint);
            let linter = verter_diagnostics::Linter::new(lint.config.clone());
            projects.push(ProjectConfig {
                root: root.to_string(),
                workspace_root: root.to_string(),
                tsconfig_path: None,
                membership: crate::project_resolver::ProjectMembership::MatchAll,
                workspace_aliases: Vec::new(),
                compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
                references: Vec::new(),
                lint_config: lint.clone(),
                linter,
                lint_explicitly_configured: lint.explicitly_configured,
                vite_config_path: None,
                vite_config_deps: Vec::new(),
                ssr_enabled,
            });
        }

        sort_projects(&mut projects);
        Self { projects }
    }

    /// Find the project that covers a given file path (longest prefix match).
    ///
    /// Falls back to `None` if no project root is a prefix of the file path.
    pub fn find_project(&self, file_path: &str) -> Option<&ProjectConfig> {
        let normalized = verter_workspace::resolver::normalize_canonical_id(file_path);
        self.projects
            .iter()
            .find(|project| project_matches_file(project, &normalized))
    }

    /// Get the project root directory for a file (for tsserver `projectRootPath`).
    pub fn find_project_root(&self, file_path: &str) -> Option<&str> {
        self.find_project(file_path).map(|p| p.root.as_str())
    }

    /// Get the lint config for a file's project.
    pub fn linter_for(&self, file_path: &str) -> Option<&ProjectConfig> {
        self.find_project(file_path)
    }

    /// Check whether a file is in an SSR context.
    ///
    /// Returns `true` if:
    /// - The file is `*.server.vue` (always SSR regardless of project config)
    /// - The project has `ssr_enabled: true` AND the file is NOT `*.client.vue`
    pub fn is_ssr_context(&self, file_path: &str) -> bool {
        // *.server.vue files are always SSR
        if is_ssr_file(file_path) {
            return true;
        }
        // *.client.vue files are never SSR
        if is_client_only_file(file_path) {
            return false;
        }
        // Otherwise, inherit from project config
        self.find_project(file_path).is_some_and(|p| p.ssr_enabled)
    }

    /// Apply default lint config to projects that don't have explicit lint config.
    ///
    /// Used to propagate VS Code `verter.lint` settings to per-project linters
    /// when those projects don't have their own `.verterrc.json`.
    pub fn apply_default_lint(&mut self, config: &verter_diagnostics::LintConfig) {
        for project in &mut self.projects {
            if !project.lint_explicitly_configured {
                project.lint_config.config = config.clone();
                project.linter = verter_diagnostics::Linter::new(config.clone());
            }
        }
    }

    /// Set `conditional_root_narrowing` on all project lint configs and rebuild linters.
    pub fn set_conditional_root_narrowing(&mut self, enabled: bool) {
        for project in &mut self.projects {
            project.lint_config.config.conditional_root_narrowing = enabled;
            project.linter = verter_diagnostics::Linter::new(project.lint_config.config.clone());
        }
    }

    /// Get all project configs.
    pub fn projects(&self) -> &[ProjectConfig] {
        &self.projects
    }

    pub fn to_native_project_resolver(&self) -> crate::project_resolver::NativeProjectResolver {
        crate::project_resolver::NativeProjectResolver::new(
            self.projects
                .iter()
                .map(ProjectConfig::to_ide_project_config)
                .collect(),
        )
    }

    /// Get all project roots.
    pub fn project_roots(&self) -> Vec<&str> {
        self.projects.iter().map(|p| p.root.as_str()).collect()
    }

    /// Get tsconfig coverage patterns from all projects (for workspace scanner).
    pub fn tsconfig_patterns(&self, roots: &[String]) -> Vec<String> {
        let mut patterns = Vec::new();
        for root_uri in roots {
            let canonical = crate::documents::uri_to_canonical_id_from_str(root_uri);
            let root_path = PathBuf::from(&canonical);
            for entry in verter_workspace::config::discover_tsconfigs(&root_path) {
                patterns.push(format!("{}/**", entry.root));
            }
        }
        patterns
    }
}

fn sort_projects(projects: &mut [ProjectConfig]) {
    projects.sort_by(|a, b| {
        b.root
            .len()
            .cmp(&a.root.len())
            .then_with(|| project_rank(a).cmp(&project_rank(b)))
            .then_with(|| a.tsconfig_path.cmp(&b.tsconfig_path))
            .then_with(|| a.root.cmp(&b.root))
    });
}

fn project_rank(project: &ProjectConfig) -> u8 {
    match project.tsconfig_path.as_deref() {
        Some(path) if path.ends_with("/tsconfig.json") => 0,
        Some(_) => 1,
        None => 2,
    }
}

fn project_matches_file(project: &ProjectConfig, file_path: &str) -> bool {
    if !path_has_prefix(file_path, &project.root) {
        return false;
    }

    match &project.membership {
        crate::project_resolver::ProjectMembership::MatchAll => true,
        crate::project_resolver::ProjectMembership::IncludeExclude {
            files,
            include,
            exclude,
        } => {
            if matches_any_pattern(file_path, exclude) {
                return false;
            }

            if files.iter().any(|candidate| candidate == file_path) {
                return true;
            }

            if !include.is_empty() {
                return matches_any_pattern(file_path, include);
            }

            !exclude.is_empty()
        }
    }
}

fn path_has_prefix(path: &str, prefix: &str) -> bool {
    path.starts_with(prefix)
        && (path.len() == prefix.len()
            || prefix.ends_with('/')
            || path.as_bytes().get(prefix.len()) == Some(&b'/'))
}

fn matches_any_pattern(path: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .filter_map(|pattern| glob::Pattern::new(pattern).ok())
        .any(|pattern| pattern.matches(path))
}

#[cfg(test)]
#[allow(clippy::cloned_ref_to_slice_refs)]
mod tests {
    use super::*;

    fn fs_workspace() -> verter_workspace::FilesystemWorkspace {
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default())
    }

    fn canonical_str(path: &std::path::Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    /// @ai-generated - Strip JSON comments handles // and /* */ and preserves strings
    #[test]
    fn test_strip_json_comments() {
        let input = r#"{
  // This is a comment
  "baseUrl": ".", /* inline comment */
  "paths": {
    "@/*": ["src/*"] // trailing comment
  }
}"#;
        let result = strip_json_comments(input);
        // Comments should be removed but strings preserved
        assert!(!result.contains("This is a comment"));
        assert!(!result.contains("inline comment"));
        assert!(!result.contains("trailing comment"));
        assert!(result.contains(r#""baseUrl""#));
        assert!(result.contains(r#""paths""#));
        // The "@/*" path pattern should be preserved (it's inside a string)
        assert!(result.contains(r#""@/*""#));

        // Comments inside strings should be preserved
        let input2 = r#"{ "url": "http://example.com" }"#;
        let result2 = strip_json_comments(input2);
        assert!(result2.contains("http://example.com"));

        // Trailing commas should be stripped (common in tsconfig.json)
        let input3 = r#"{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
  },
  "include": ["src/**/*",],
}"#;
        let result3 = strip_json_comments(input3);
        // Should parse as valid JSON after stripping
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&result3);
        assert!(
            parsed.is_ok(),
            "trailing commas should be stripped: {result3}"
        );

        // Commas inside strings should NOT be stripped
        let input4 = r#"{ "paths": { "@/*": ["src/*",] }, "desc": "a, b," }"#;
        let result4 = strip_json_comments(input4);
        let parsed4: Result<serde_json::Value, _> = serde_json::from_str(&result4);
        assert!(parsed4.is_ok(), "should handle mixed commas: {result4}");
        let v4 = parsed4.unwrap();
        assert_eq!(
            v4["desc"].as_str().unwrap(),
            "a, b,",
            "commas inside strings must be preserved"
        );
    }

    // -- ComponentDiagnosticsConfig tests --

    #[test]
    fn default_config_uses_warning() {
        let config = ComponentDiagnosticsConfig::default();
        // Positive: defaults to Warning
        assert_eq!(
            config.unknown_prop_severity,
            DiagnosticSeverityConfig::Warning
        );
        assert_eq!(
            config.unknown_model_severity,
            DiagnosticSeverityConfig::Warning
        );
        // Negative: NOT Error, NOT Off
        assert_ne!(
            config.unknown_prop_severity,
            DiagnosticSeverityConfig::Error
        );
        assert_ne!(config.unknown_prop_severity, DiagnosticSeverityConfig::Off);
    }

    #[test]
    fn off_severity_returns_none() {
        // Off → to_lsp() returns None (disables diagnostics)
        assert!(DiagnosticSeverityConfig::Off.to_lsp().is_none());
        // Positive: Warning returns Some
        assert!(DiagnosticSeverityConfig::Warning.to_lsp().is_some());
        // Positive: Error returns Some
        assert!(DiagnosticSeverityConfig::Error.to_lsp().is_some());
    }

    #[test]
    fn severity_to_lsp_maps_correctly() {
        use tower_lsp_server::ls_types::DiagnosticSeverity;
        assert_eq!(
            DiagnosticSeverityConfig::Error.to_lsp(),
            Some(DiagnosticSeverity::ERROR)
        );
        assert_eq!(
            DiagnosticSeverityConfig::Warning.to_lsp(),
            Some(DiagnosticSeverity::WARNING)
        );
        assert_eq!(
            DiagnosticSeverityConfig::Information.to_lsp(),
            Some(DiagnosticSeverity::INFORMATION)
        );
        assert_eq!(
            DiagnosticSeverityConfig::Hint.to_lsp(),
            Some(DiagnosticSeverity::HINT)
        );
    }

    // =====================================================================
    // ProjectRegistry tests
    // =====================================================================

    #[test]
    fn registry_find_project_most_specific() {
        let tmp = std::env::temp_dir().join("verter_test_registry_specific");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("packages/ui/src")).unwrap();
        std::fs::create_dir_all(tmp.join("packages/app/src")).unwrap();

        // Create tsconfigs with different aliases
        std::fs::write(
            tmp.join("packages/ui/tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@ui/*":["src/*"]}}}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("packages/app/tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@app/*":["src/*"]}}}"#,
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let registry = ProjectRegistry::from_canonical_roots(&fs_workspace(), &[&root]);

        // File in packages/ui should match packages/ui project
        let ui_file = format!("{root}/packages/ui/src/Button.vue");
        let project = registry.find_project(&ui_file);
        assert!(project.is_some(), "should find project for ui file");
        assert!(
            project.unwrap().root.contains("packages/ui"),
            "should match packages/ui, got: {}",
            project.unwrap().root,
        );

        // File in packages/app should match packages/app project
        let app_file = format!("{root}/packages/app/src/App.vue");
        let project = registry.find_project(&app_file);
        assert!(project.is_some(), "should find project for app file");
        assert!(
            project.unwrap().root.contains("packages/app"),
            "should match packages/app, got: {}",
            project.unwrap().root,
        );

        // File in packages/ui should NOT match packages/app
        let ui_project_root = registry.find_project_root(&ui_file).unwrap();
        assert!(
            !ui_project_root.contains("packages/app"),
            "ui file must not match app project"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn registry_fallback_to_workspace_root() {
        let tmp = std::env::temp_dir().join("verter_test_registry_fallback");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let registry = ProjectRegistry::from_canonical_roots(&fs_workspace(), &[&root]);

        // File in root (no tsconfig) should still find a default project
        let file = format!("{root}/src/App.vue");
        let project = registry.find_project(&file);
        assert!(
            project.is_some(),
            "should fall back to workspace root project"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn registry_per_project_lint_config() {
        let tmp = std::env::temp_dir().join("verter_test_registry_lint");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("packages/strict-pkg/src")).unwrap();
        std::fs::create_dir_all(tmp.join("packages/lax-pkg/src")).unwrap();

        // strict-pkg has verterrc with strict preset
        std::fs::write(
            tmp.join("packages/strict-pkg/.verterrc.json"),
            r#"{"lint":{"preset":"strict"}}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("packages/strict-pkg/tsconfig.json"),
            r#"{"compilerOptions":{}}"#,
        )
        .unwrap();

        // lax-pkg has verterrc with essential preset
        std::fs::write(
            tmp.join("packages/lax-pkg/.verterrc.json"),
            r#"{"lint":{"preset":"essential"}}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("packages/lax-pkg/tsconfig.json"),
            r#"{"compilerOptions":{}}"#,
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let registry = ProjectRegistry::from_canonical_roots(&fs_workspace(), &[&root]);

        let strict_file = format!("{root}/packages/strict-pkg/src/Foo.vue");
        let strict_project = registry.linter_for(&strict_file);
        assert!(strict_project.is_some(), "should find strict project");
        assert_eq!(
            strict_project.unwrap().lint_config.config.preset,
            verter_diagnostics::LintPreset::Strict,
        );

        let lax_file = format!("{root}/packages/lax-pkg/src/Bar.vue");
        let lax_project = registry.linter_for(&lax_file);
        assert!(lax_project.is_some(), "should find lax project");
        assert_eq!(
            lax_project.unwrap().lint_config.config.preset,
            verter_diagnostics::LintPreset::Essential,
        );

        // Verify they DON'T share the same config
        assert_ne!(
            strict_project.unwrap().lint_config.config.preset,
            lax_project.unwrap().lint_config.config.preset,
            "different packages must have different lint presets"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // =====================================================================
    // Tsconfig-first policy tests
    // =====================================================================

    #[test]
    fn tsconfig_backed_project_no_vite_aliases() {
        // Tsconfig-backed projects must NOT get vite aliases merged, even when
        // vite.config.ts exists alongside and vite_config_enabled is true.
        let tmp = std::env::temp_dir().join("verter_test_tsconfig_first_no_vite");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        // Create tsconfig with path aliases
        std::fs::write(
            tmp.join("tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
        )
        .unwrap();

        // Create vite config with different alias
        std::fs::write(
            tmp.join("vite.config.js"),
            "export default { resolve: { alias: { '~': './lib' } } };",
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let fs_ws = verter_workspace::FilesystemWorkspace::new(
            verter_workspace::FilesystemOptions::default(),
        );
        let registry = ProjectRegistry::from_workspace_roots(
            &fs_ws,
            &[root.clone()],
            &verter_workspace::ViteConfigOptions {
                enabled: true,
                trusted_files: Vec::new(),
                node_path: Some("node".to_string()),
            },
        )
        .registry;

        // Find tsconfig-backed project
        let file = format!("{root}/src/App.vue");
        let project = registry.find_project(&file);
        assert!(project.is_some(), "should find tsconfig-backed project");
        let project = project.unwrap();
        assert!(
            project.tsconfig_path.is_some(),
            "project should have tsconfig_path"
        );

        // Negative: no vite aliases in workspace_aliases
        assert!(
            project.workspace_aliases.is_empty(),
            "tsconfig-backed project must have empty workspace_aliases, got {} entries",
            project.workspace_aliases.len()
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // =====================================================================
    // Fallback project vite alias wiring tests
    // =====================================================================

    #[test]
    fn fallback_project_static_vite_aliases() {
        // Fallback (no tsconfig) project with static-analyzable vite config
        // should get aliases populated.
        let tmp = std::env::temp_dir().join("verter_test_fallback_static_vite");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        // No tsconfig.json — this will be a fallback project
        std::fs::write(
            tmp.join("vite.config.js"),
            "export default { resolve: { alias: { '@': './src' } } };",
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let fs_ws = verter_workspace::FilesystemWorkspace::new(
            verter_workspace::FilesystemOptions::default(),
        );
        let build_result = ProjectRegistry::from_workspace_roots(
            &fs_ws,
            &[root.clone()],
            &verter_workspace::ViteConfigOptions {
                enabled: true,
                trusted_files: Vec::new(),
                node_path: None,
            },
        );

        let file = format!("{root}/src/App.vue");
        let project = build_result.registry.find_project(&file);
        assert!(project.is_some(), "should find fallback project");
        let project = project.unwrap();

        // Positive: fallback project has no tsconfig
        assert!(
            project.tsconfig_path.is_none(),
            "should be a fallback project"
        );

        // Positive: should have vite aliases
        assert!(
            !project.workspace_aliases.is_empty(),
            "fallback project should have vite aliases"
        );

        // Positive: vite_config_path and vite_config_deps should be set
        assert!(
            project.vite_config_path.is_some(),
            "vite_config_path should be set"
        );
        assert!(
            !project.vite_config_deps.is_empty(),
            "vite_config_deps should include the config file"
        );

        // Negative: no trust_required entries
        assert!(
            build_result.trust_required.is_empty(),
            "static analysis should not require trust"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fallback_project_complex_config_not_trusted() {
        // Fallback project with complex (function export) vite config and no trust
        // should have empty aliases and generate trust_required entry.
        let tmp = std::env::temp_dir().join("verter_test_fallback_complex_notrusted");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        std::fs::write(
            tmp.join("vite.config.ts"),
            r#"import { defineConfig } from 'vite'
export default defineConfig(({ mode }) => ({
  resolve: { alias: { '@': './src' } }
}))"#,
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let fs_ws = verter_workspace::FilesystemWorkspace::new(
            verter_workspace::FilesystemOptions::default(),
        );
        let build_result = ProjectRegistry::from_workspace_roots(
            &fs_ws,
            &[root.clone()],
            &verter_workspace::ViteConfigOptions {
                enabled: true,
                trusted_files: Vec::new(),
                node_path: None,
            },
        );

        let file = format!("{root}/src/App.vue");
        let project = build_result.registry.find_project(&file).unwrap();

        // Negative: no aliases when not trusted
        assert!(
            project.workspace_aliases.is_empty(),
            "untrusted complex config should have empty aliases"
        );

        // Positive: trust_required should have an entry
        assert_eq!(
            build_result.trust_required.len(),
            1,
            "should have 1 trust_required entry"
        );
        assert!(
            build_result.trust_required[0].reason.contains("function")
                || build_result.trust_required[0].reason.contains("arrow"),
            "reason should mention function/arrow: {}",
            build_result.trust_required[0].reason
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn tsconfig_project_no_vite_config_path() {
        // Tsconfig-backed projects should never have vite_config_path set.
        let tmp = std::env::temp_dir().join("verter_test_tsconfig_no_vite_path");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        std::fs::write(
            tmp.join("tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("vite.config.js"),
            "export default { resolve: { alias: { '@': './src' } } };",
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let fs_ws = verter_workspace::FilesystemWorkspace::new(
            verter_workspace::FilesystemOptions::default(),
        );
        let build_result = ProjectRegistry::from_workspace_roots(
            &fs_ws,
            &[root.clone()],
            &verter_workspace::ViteConfigOptions {
                enabled: true,
                trusted_files: Vec::new(),
                node_path: None,
            },
        );

        let file = format!("{root}/src/App.vue");
        let project = build_result.registry.find_project(&file).unwrap();

        // Positive: tsconfig project
        assert!(project.tsconfig_path.is_some());

        // Negative: no vite_config_path on tsconfig project
        assert!(
            project.vite_config_path.is_none(),
            "tsconfig-backed project must not have vite_config_path"
        );
        assert!(
            project.vite_config_deps.is_empty(),
            "tsconfig-backed project must not have vite_config_deps"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn disabled_vite_fallback_has_no_aliases() {
        // When vite is disabled, fallback projects should have empty aliases.
        let tmp = std::env::temp_dir().join("verter_test_disabled_vite");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        std::fs::write(
            tmp.join("vite.config.js"),
            "export default { resolve: { alias: { '@': './src' } } };",
        )
        .unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let fs_ws = verter_workspace::FilesystemWorkspace::new(
            verter_workspace::FilesystemOptions::default(),
        );
        let build_result = ProjectRegistry::from_workspace_roots(
            &fs_ws,
            &[root.clone()],
            &verter_workspace::ViteConfigOptions {
                enabled: false,
                trusted_files: Vec::new(),
                node_path: None,
            },
        );

        let file = format!("{root}/src/App.vue");
        let project = build_result.registry.find_project(&file).unwrap();

        assert!(
            project.workspace_aliases.is_empty(),
            "disabled vite should mean empty aliases"
        );
        assert!(
            project.vite_config_path.is_none(),
            "disabled vite should not set vite_config_path"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn strip_trailing_commas_preserves_multibyte_utf8() {
        // Multi-byte UTF-8: non-ASCII chars in string values
        let input = r#"{"desc": "Compilé avec succès", "ok": true,}"#;
        let result = strip_trailing_commas(input);
        // Positive: valid JSON after stripping trailing comma
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&result);
        assert!(parsed.is_ok(), "should produce valid JSON: {result}");
        let v = parsed.unwrap();
        assert_eq!(
            v["desc"].as_str().unwrap(),
            "Compilé avec succès",
            "multi-byte chars must be preserved"
        );
        // Negative: no trailing comma before }
        assert!(
            !result.contains(",}"),
            "trailing comma should be removed: {result}"
        );
    }

    #[test]
    fn strip_trailing_commas_preserves_cjk_chars() {
        let input = r#"{"名前": "テスト",}"#;
        let result = strip_trailing_commas(input);
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&result);
        assert!(parsed.is_ok(), "should produce valid JSON: {result}");
        let v = parsed.unwrap();
        assert_eq!(v["名前"].as_str().unwrap(), "テスト");
    }

    #[test]
    fn strip_trailing_commas_roundtrip_bytes() {
        // Verify that strip_trailing_commas preserves byte-exact content
        // for any input that has no trailing commas.
        let input = r#"{"key": "café", "num": 42}"#;
        let result = strip_trailing_commas(input);
        assert_eq!(result, input, "no-op input should be byte-exact preserved");
    }

    #[test]
    fn strip_trailing_commas_bare_multibyte_outside_strings() {
        // strip_json_comments may produce output with non-ASCII chars in
        // positions outside JSON strings (e.g., replaced comments leaving stubs).
        // Also tests that the full pipeline works: strip_json_comments calls
        // strip_trailing_commas, so multi-byte content must survive both passes.
        let input = "{ \"path\": \"@/*\", } // résumé";
        let result = strip_json_comments(input);
        // The comment is stripped; trailing comma is stripped
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&result);
        assert!(parsed.is_ok(), "should produce valid JSON: {result}");
    }

    #[test]
    fn find_project_enforces_path_boundary() {
        // Only /workspace/app exists — NOT /workspace/app-admin.
        // Without path boundary enforcement, /workspace/app-admin/src/Foo.vue
        // would incorrectly match /workspace/app because it starts_with "/workspace/app".
        let registry = ProjectRegistry {
            projects: vec![ProjectConfig {
                root: "/workspace/app".to_string(),
                workspace_root: "/workspace".to_string(),
                tsconfig_path: None,
                membership: crate::project_resolver::ProjectMembership::MatchAll,
                workspace_aliases: Vec::new(),
                compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
                references: Vec::new(),

                lint_config: ResolvedLintConfig::default(),
                linter: verter_diagnostics::Linter::default(),
                lint_explicitly_configured: false,
                vite_config_path: None,
                vite_config_deps: Vec::new(),
                ssr_enabled: false,
            }],
        };

        // Positive: file in /workspace/app/ should match
        let project = registry.find_project("/workspace/app/src/Baz.vue");
        assert!(project.is_some(), "file in /workspace/app/ should match");

        // Negative: file in /workspace/app-admin/ should NOT match /workspace/app
        let project2 = registry.find_project("/workspace/app-admin/src/Foo.vue");
        assert!(
            project2.is_none(),
            "file in /workspace/app-admin/ must not match /workspace/app"
        );
    }

    #[test]
    fn apply_default_lint_only_affects_non_explicit_projects() {
        let mut registry = ProjectRegistry {
            projects: vec![
                ProjectConfig {
                    root: "/workspace/explicit/".to_string(),
                    workspace_root: "/workspace".to_string(),
                    tsconfig_path: None,
                    membership: crate::project_resolver::ProjectMembership::MatchAll,
                    workspace_aliases: Vec::new(),
                    compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
                    references: Vec::new(),

                    lint_config: ResolvedLintConfig {
                        config: verter_diagnostics::LintConfig {
                            preset: verter_diagnostics::LintPreset::Strict,
                            ..Default::default()
                        },
                        explicitly_configured: true,
                    },
                    linter: verter_diagnostics::Linter::default(),
                    lint_explicitly_configured: true,
                    vite_config_path: None,
                    vite_config_deps: Vec::new(),
                    ssr_enabled: false,
                },
                ProjectConfig {
                    root: "/workspace/default/".to_string(),
                    workspace_root: "/workspace".to_string(),
                    tsconfig_path: None,
                    membership: crate::project_resolver::ProjectMembership::MatchAll,
                    workspace_aliases: Vec::new(),
                    compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
                    references: Vec::new(),

                    lint_config: ResolvedLintConfig::default(),
                    linter: verter_diagnostics::Linter::default(),
                    lint_explicitly_configured: false,
                    vite_config_path: None,
                    vite_config_deps: Vec::new(),
                    ssr_enabled: false,
                },
            ],
        };

        let new_config = verter_diagnostics::LintConfig {
            preset: verter_diagnostics::LintPreset::All,
            ..Default::default()
        };
        registry.apply_default_lint(&new_config);

        // Positive: non-explicit project gets the default config
        assert_eq!(
            registry.projects[1].lint_config.config.preset,
            verter_diagnostics::LintPreset::All,
            "non-explicit project should get default lint config"
        );

        // Negative: explicit project should NOT be overridden
        assert_eq!(
            registry.projects[0].lint_config.config.preset,
            verter_diagnostics::LintPreset::Strict,
            "explicitly configured project must keep its own config"
        );
    }

    #[test]
    fn registry_file_outside_all_projects() {
        let tmp = std::env::temp_dir().join("verter_test_registry_outside");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let root = tmp.to_string_lossy().replace('\\', "/");
        let registry = ProjectRegistry::from_canonical_roots(&fs_workspace(), &[&root]);

        // File completely outside the workspace
        let outside = "/some/other/project/App.vue";
        let project = registry.find_project(outside);
        assert!(
            project.is_none(),
            "file outside all workspace roots should return None"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn registry_solution_style_root_does_not_own_member_files() {
        let tmp = std::env::temp_dir().join("verter_test_registry_solution_owner");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();
        std::fs::create_dir_all(tmp.join("tests")).unwrap();

        std::fs::write(
            tmp.join("tsconfig.json"),
            r#"{
  "files": [],
  "references": [
    { "path": "./tsconfig.app.json" },
    { "path": "./tsconfig.vitest.json" }
  ]
}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("tsconfig.app.json"),
            r#"{
  "include": ["src/**/*"],
  "exclude": ["tests/**/*"]
}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("tsconfig.vitest.json"),
            r#"{
  "include": ["tests/**/*"]
}"#,
        )
        .unwrap();

        let root = verter_workspace::resolver::normalize_canonical_id(
            &tmp.to_string_lossy().replace('\\', "/"),
        );
        let registry = ProjectRegistry::from_canonical_roots(&fs_workspace(), &[&root]);
        let source_file = format!("{root}/src/App.vue");
        let expected_app = format!("{root}/tsconfig.app.json");
        let solution_root = format!("{root}/tsconfig.json");

        let project = registry
            .find_project(&source_file)
            .expect("solution-style workspace should still find an owner");

        assert_eq!(
            project.tsconfig_path.as_deref(),
            Some(expected_app.as_str()),
            "src/App.vue should be owned by tsconfig.app.json, not the solution tsconfig"
        );
        assert_ne!(
            project.tsconfig_path.as_deref(),
            Some(solution_root.as_str()),
            "solution-style tsconfig.json must not claim files outside its membership"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn registry_unmatched_root_file_uses_synthetic_workspace_project() {
        let tmp = std::env::temp_dir().join("verter_test_registry_synth_workspace");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();
        std::fs::create_dir_all(tmp.join("scripts")).unwrap();

        std::fs::write(
            tmp.join("tsconfig.app.json"),
            r#"{
  "include": ["src/**/*"]
}"#,
        )
        .unwrap();

        let root = verter_workspace::resolver::normalize_canonical_id(
            &tmp.to_string_lossy().replace('\\', "/"),
        );
        let registry = ProjectRegistry::from_canonical_roots(&fs_workspace(), &[&root]);
        let unmatched = format!("{root}/scripts/tool.ts");

        let project = registry
            .find_project(&unmatched)
            .expect("unmatched file should fall back to a synthetic workspace project");

        assert_eq!(
            project.tsconfig_path, None,
            "scripts/tool.ts should not be assigned to tsconfig.app.json"
        );
        assert_eq!(
            project.root, root,
            "synthetic workspace project should use the workspace root"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn registry_projects_preserve_compiler_options_through_extends() {
        let tmp = std::env::temp_dir().join("verter_test_registry_compiler_options");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        std::fs::write(
            tmp.join("tsconfig.base.json"),
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"]
    }
  }
}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("tsconfig.app.json"),
            r#"{
  "extends": "./tsconfig.base.json",
  "include": ["src/**/*"]
}"#,
        )
        .unwrap();

        let root = verter_workspace::resolver::normalize_canonical_id(
            &tmp.to_string_lossy().replace('\\', "/"),
        );
        let registry = ProjectRegistry::from_canonical_roots(&fs_workspace(), &[&root]);
        let app_file = format!("{root}/src/App.ts");
        let project = registry
            .find_project(&app_file)
            .expect("extended tsconfig project should own src/App.ts");

        assert_eq!(
            project.compiler_options.base_url.as_deref(),
            Some(root.as_str()),
            "baseUrl should be resolved to an absolute canonical path"
        );
        assert_eq!(
            project.compiler_options.paths,
            vec![("@/*".to_string(), vec![format!("{root}/src/*")])],
            "tsconfig paths should be preserved on the project for native resolution"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // =====================================================================
    // has_solution_style_tsconfig tests
    // =====================================================================

    #[test]
    fn has_solution_style_tsconfig_detects_references() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("tsconfig.json"),
            r#"{ "files": [], "references": [{ "path": "./tsconfig.app.json" }] }"#,
        )
        .unwrap();
        assert!(
            has_solution_style_tsconfig(&fs_workspace(), &canonical_str(dir.path())),
            "should detect solution-style tsconfig with references"
        );
    }

    #[test]
    fn has_solution_style_tsconfig_false_for_flat_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("tsconfig.json"),
            r#"{ "compilerOptions": { "strict": true }, "include": ["src"] }"#,
        )
        .unwrap();
        assert!(
            !has_solution_style_tsconfig(&fs_workspace(), &canonical_str(dir.path())),
            "flat tsconfig without references should return false"
        );
    }

    #[test]
    fn has_solution_style_tsconfig_false_for_empty_references() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tsconfig.json"), r#"{ "references": [] }"#).unwrap();
        assert!(
            !has_solution_style_tsconfig(&fs_workspace(), &canonical_str(dir.path())),
            "empty references array should return false"
        );
    }

    #[test]
    fn has_solution_style_tsconfig_false_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            !has_solution_style_tsconfig(&fs_workspace(), &canonical_str(dir.path())),
            "missing tsconfig.json should return false"
        );
    }

    #[test]
    fn has_solution_style_tsconfig_handles_jsonc() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("tsconfig.json"),
            "{\n  // Solution-style config\n  \"files\": [],\n  \"references\": [{ \"path\": \"./tsconfig.app.json\" }],\n}",
        )
        .unwrap();
        assert!(
            has_solution_style_tsconfig(&fs_workspace(), &canonical_str(dir.path())),
            "should handle JSONC with comments and trailing commas"
        );
    }

    #[test]
    fn has_solution_style_tsconfig_detects_monorepo_subdirectory() {
        // Monorepo: root has no tsconfig, but packages/ui/tsconfig.json has references
        let dir = tempfile::tempdir().unwrap();
        let packages = dir.path().join("packages");
        let ui = packages.join("ui");
        std::fs::create_dir_all(&ui).unwrap();
        std::fs::write(
            ui.join("tsconfig.json"),
            r#"{ "composite": true, "files": [], "references": [{ "path": "./tsconfig.app.json" }] }"#,
        )
        .unwrap();
        assert!(
            has_solution_style_tsconfig(&fs_workspace(), &canonical_str(dir.path())),
            "should detect solution-style tsconfig in monorepo subdirectory (packages/ui/)"
        );
        // Negative: root itself has no tsconfig.json file
        assert!(
            !dir.path().join("tsconfig.json").exists(),
            "root tsconfig.json should not exist — solution-style tsconfig is in packages/ui/"
        );
    }

    #[test]
    fn has_solution_style_tsconfig_skips_node_modules() {
        // node_modules should be skipped even if it contains a solution-style tsconfig
        let dir = tempfile::tempdir().unwrap();
        let nm = dir.path().join("node_modules").join("some-pkg");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(
            nm.join("tsconfig.json"),
            r#"{ "files": [], "references": [{ "path": "./tsconfig.app.json" }] }"#,
        )
        .unwrap();
        assert!(
            !has_solution_style_tsconfig(&fs_workspace(), &canonical_str(dir.path())),
            "should not scan node_modules"
        );
    }

    // ── SSR Detection Tests ──────────────────────────────────────────────

    #[test]
    fn detect_ssr_nuxt_config_ts() {
        let tmp = std::env::temp_dir().join("verter_test_ssr_nuxt");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("nuxt.config.ts"), "export default {}").unwrap();
        let lint = ResolvedLintConfig::default();
        assert!(
            detect_ssr_project(&fs_workspace(), &canonical_str(&tmp), &lint),
            "should detect nuxt.config.ts"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_ssr_nuxt_config_js() {
        let tmp = std::env::temp_dir().join("verter_test_ssr_nuxt_js");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("nuxt.config.js"), "export default {}").unwrap();
        let lint = ResolvedLintConfig::default();
        assert!(
            detect_ssr_project(&fs_workspace(), &canonical_str(&tmp), &lint),
            "should detect nuxt.config.js"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_ssr_nuxt_dir() {
        let tmp = std::env::temp_dir().join("verter_test_ssr_nuxt_dir");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::create_dir_all(tmp.join(".nuxt")).unwrap();
        let lint = ResolvedLintConfig::default();
        assert!(
            detect_ssr_project(&fs_workspace(), &canonical_str(&tmp), &lint),
            "should detect .nuxt dir"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_ssr_from_lint_config() {
        let tmp = std::env::temp_dir().join("verter_test_ssr_lint");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let lint = ResolvedLintConfig {
            config: verter_diagnostics::LintConfig {
                ssr_mode: true,
                ..Default::default()
            },
            explicitly_configured: true,
        };
        assert!(
            detect_ssr_project(&fs_workspace(), &canonical_str(&tmp), &lint),
            "should detect ssr_mode from lint config"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn no_ssr_for_plain_vite_project() {
        let tmp = std::env::temp_dir().join("verter_test_no_ssr");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("vite.config.ts"), "export default {}").unwrap();
        let lint = ResolvedLintConfig::default();
        assert!(
            !detect_ssr_project(&fs_workspace(), &canonical_str(&tmp), &lint),
            "plain vite project should not be SSR"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn is_ssr_file_detection() {
        assert!(is_ssr_file("MyComp.server.vue"));
        assert!(is_ssr_file("/path/to/MyComp.server.vue"));
        assert!(!is_ssr_file("MyComp.vue"));
        assert!(!is_ssr_file("MyComp.client.vue"));
    }

    #[test]
    fn is_client_only_file_detection() {
        assert!(is_client_only_file("MyComp.client.vue"));
        assert!(is_client_only_file("/path/to/MyComp.client.vue"));
        assert!(!is_client_only_file("MyComp.vue"));
        assert!(!is_client_only_file("MyComp.server.vue"));
    }

    #[test]
    fn registry_is_ssr_context() {
        let registry = ProjectRegistry {
            projects: vec![ProjectConfig {
                root: "/workspace/app".to_string(),
                workspace_root: "/workspace".to_string(),
                tsconfig_path: None,
                membership: crate::project_resolver::ProjectMembership::MatchAll,
                workspace_aliases: Vec::new(),
                compiler_options: crate::project_resolver::IdeProjectCompilerOptions::default(),
                references: Vec::new(),

                lint_config: ResolvedLintConfig::default(),
                linter: verter_diagnostics::Linter::default(),
                lint_explicitly_configured: false,
                vite_config_path: None,
                vite_config_deps: Vec::new(),
                ssr_enabled: true,
            }],
        };

        // Regular .vue in SSR project → SSR context
        assert!(registry.is_ssr_context("/workspace/app/src/Comp.vue"));
        // *.server.vue → always SSR
        assert!(registry.is_ssr_context("/workspace/app/src/Comp.server.vue"));
        // *.client.vue → never SSR even in SSR project
        assert!(!registry.is_ssr_context("/workspace/app/src/Comp.client.vue"));
        // File outside project → not SSR
        assert!(!registry.is_ssr_context("/other/Comp.vue"));
    }

    #[test]
    fn verterrc_ssr_config_roundtrip() {
        let json = r#"{"lint":{"enabled":true},"ssr":{"enabled":true}}"#;
        let config: verter_diagnostics::VerterProjectConfig = serde_json::from_str(json).unwrap();
        assert!(config.ssr.unwrap().enabled.unwrap());
    }

    #[test]
    fn find_project_normalizes_input_path() {
        // Simulate a Windows root stored with lowercase (production canonical form)
        let root = "c:/users/dev/project";
        let registry = ProjectRegistry::from_canonical_roots(&fs_workspace(), &[root]);

        // Normalized query matches
        assert!(
            registry
                .find_project("c:/users/dev/project/src/App.vue")
                .is_some(),
            "find_project should match normalized path"
        );

        // Uppercase drive letter in query — the actual bug on Windows
        assert!(
            registry
                .find_project("C:/users/dev/project/src/App.vue")
                .is_some(),
            "find_project should match uppercase drive letter query"
        );

        // Backslash query (Windows raw path)
        assert!(
            registry
                .find_project("c:\\users\\dev\\project\\src\\App.vue")
                .is_some(),
            "find_project should match backslash paths"
        );

        // Negative: unrelated path must NOT match
        assert!(
            registry.find_project("d:/other/src/App.vue").is_none(),
            "find_project should not match unrelated paths"
        );
    }

    #[test]
    fn from_canonical_roots_normalizes_stored_roots() {
        // Feed uppercase Windows drive — simulates what std::fs::canonicalize returns on Windows
        let registry =
            ProjectRegistry::from_canonical_roots(&fs_workspace(), &["C:/Users/dev/project"]);
        let workspace_project = registry.projects.last().unwrap();

        // Positive: stored root should have lowercase drive letter (canonical form)
        assert!(
            workspace_project.root.starts_with("c:/"),
            "from_canonical_roots should lowercase the drive letter, got: {}",
            workspace_project.root
        );
        // Negative: must NOT retain uppercase drive letter
        assert!(
            !workspace_project.root.starts_with("C:/"),
            "stored root should not retain uppercase drive letter"
        );

        // Also test Linux-style path passes through unchanged
        let registry2 =
            ProjectRegistry::from_canonical_roots(&fs_workspace(), &["/home/user/project"]);
        let ws2 = registry2.projects.last().unwrap();
        assert_eq!(
            ws2.root, "/home/user/project",
            "Linux-style root should pass through unchanged"
        );
    }
}

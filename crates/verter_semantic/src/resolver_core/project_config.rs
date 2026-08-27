//! Per-project (tsconfig-backed) resolver configuration.
//!
//! The DTO lives with the resolver core. Workspace-specific default membership
//! construction remains in the workspace config-ingress function, while this
//! module accepts the resulting dependency-neutral membership value.

use super::membership::ConfiguredMembership;
use verter_span::path::CanonicalPath;

/// A workspace alias maps a prefix (e.g. `@/`) to a filesystem replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceAlias {
    pub find: String,
    pub replacement: String,
}

/// Compiler options extracted from a tsconfig for resolution.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IdeProjectCompilerOptions {
    pub base_url: Option<String>,
    pub paths: Vec<(String, Vec<String>)>,
    /// `compilerOptions.allowJs` — when set (or `checkJs`), `.js`/`.jsx`/
    /// `.cjs`/`.mjs` join the project's supported-extension set.
    pub allow_js: bool,
    /// `compilerOptions.checkJs` — implies `allowJs` for membership purposes
    /// (TypeScript treats `checkJs` as turning on JS type-checking, which
    /// requires the JS files to be project members).
    pub check_js: bool,
    /// `compilerOptions.allowImportingTsExtensions` — when explicitly true,
    /// tsserver barrel publication preserves authored `.vue`/`.svelte`
    /// specifiers. Missing/false projects receive the `.verter.ts`
    /// compatibility rewrite.
    pub allow_importing_ts_extensions: bool,
    /// `compilerOptions.disableSolutionSearching` — when a solution config
    /// sets it, default-project selection does NOT climb from that solution
    /// to its ancestor solution (mirrors tsgo `DisableSolutionSearching`).
    /// Default `false`.
    pub disable_solution_searching: bool,
}

impl IdeProjectCompilerOptions {
    /// Whether JavaScript files are project members (either `allowJs` or
    /// `checkJs` is set).
    #[must_use]
    pub fn js_is_member(&self) -> bool {
        self.allow_js || self.check_js
    }
}

/// Configuration for a single IDE project (tsconfig-backed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeProjectConfig {
    pub root: String,
    pub workspace_root: String,
    pub tsconfig_path: Option<String>,
    pub provider_root: String,
    pub workspace_aliases: Vec<WorkspaceAlias>,
    pub compiler_options: IdeProjectCompilerOptions,
    pub references: Vec<String>,
    /// Exact configured membership — the SAME [`ConfiguredMembership`] the
    /// host's ownership authority consults, so the resolver and the
    /// ownership authority never diverge on a glob-vs-exact membership
    /// answer. A fallback (tsconfig-less) config carries a match-all
    /// membership under its root.
    pub membership: ConfiguredMembership,
}

impl IdeProjectConfig {
    #[cfg(test)]
    pub(crate) fn new(root: String, workspace_root: String, tsconfig_path: Option<String>) -> Self {
        use rustc_hash::FxHashSet;

        let membership = ConfiguredMembership {
            spec: crate::resolver_core::StaticMembershipSpec {
                files: Vec::new(),
                include: vec![crate::resolver_core::CompiledGlob::new(
                    crate::resolver_core::NormalizedGlob::from_root_and_pattern(
                        &CanonicalPath::new(&root),
                        "**/*",
                    ),
                )],
                exclude: crate::resolver_core::typescript_default_excludes(&CanonicalPath::new(
                    &root,
                )),
            },
            materialized_files: FxHashSet::default(),
        };
        let provider_root = root.clone();
        Self {
            root,
            workspace_root,
            tsconfig_path,
            provider_root,
            workspace_aliases: Vec::new(),
            compiler_options: IdeProjectCompilerOptions::default(),
            references: Vec::new(),
            membership,
        }
    }

    /// Whether `file_id` is a member of this project, per the exact
    /// [`ConfiguredMembership`] (its materialized file set, or the compiled
    /// spec globs for a match-all / filesystem-less membership). One
    /// membership engine — no second glob evaluator.
    pub fn matches_file(&self, file_id: &str) -> bool {
        self.membership.contains(&CanonicalPath::new(file_id))
    }
}

#[cfg(test)]
#[path = "project_config_tests.rs"]
mod tests;

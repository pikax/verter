//! Exact membership types for workspace ownership.
//!
//! These types model TypeScript's `files`/`include`/`exclude` membership
//! semantics exactly:
//!
//! - `files` entries are ALWAYS members — `exclude` does NOT affect them
//! - `exclude` only filters what `include` finds
//! - `include` defaults to `["**/*"]` only when BOTH `files` AND `include` are absent
//! - If `files` IS present but `include` is absent → no implicit include
//! - `exclude` defaults to `["node_modules", "bower_components", "jspm_packages", outDir]`
//! - Solution-style `{ files: [], references: [...] }` → matches nothing

use crate::canonical_path::CanonicalPath;
use crate::normalized_glob::NormalizedGlob;
use rustc_hash::FxHashSet;

/// Static membership specification parsed from a tsconfig.
///
/// Always explicit — no `MatchAll` variant. When a tsconfig has no
/// `files`, no `include`, no `exclude`, the builder fills in TypeScript
/// defaults: `include: ["{dir}/**/*"]`, `exclude: ["{dir}/node_modules/**", ...]`.
#[derive(Debug, Clone)]
pub struct StaticMembershipSpec {
    /// Exact file paths. **Immune to exclude** — always members.
    pub files: Vec<CanonicalPath>,
    /// Glob patterns. Builder fills default `["**/*"]` when needed.
    pub include: Vec<NormalizedGlob>,
    /// Only filters `include`. Builder fills TS defaults when needed.
    pub exclude: Vec<NormalizedGlob>,
}

/// Configured project membership: static spec + materialized file set.
#[derive(Debug, Clone)]
pub struct ConfiguredMembership {
    pub spec: StaticMembershipSpec,
    /// Exact set of files determined to be members of this project.
    /// Populated during snapshot build by expanding the static spec.
    pub materialized_files: FxHashSet<CanonicalPath>,
}

impl ConfiguredMembership {
    /// Check if a file is a member of this configured project.
    ///
    /// If materialized files have been populated, uses exact set membership.
    /// Otherwise falls back to static spec matching (bridge mode during
    /// migration when filesystem walking hasn't been done yet).
    pub fn contains(&self, file_path: &CanonicalPath) -> bool {
        if !self.materialized_files.is_empty() {
            self.materialized_files.contains(file_path)
        } else {
            // Bridge: materialization not yet done, use static spec
            self.spec.matches(file_path)
        }
    }
}

/// Fallback project membership: root-containment minus exclusions.
#[derive(Debug, Clone)]
pub struct FallbackMembership {
    pub root: CanonicalPath,
    pub exclude: Vec<NormalizedGlob>,
}

impl FallbackMembership {
    /// Check if a file is covered by this fallback project.
    ///
    /// True if: file is under root AND not excluded.
    pub fn contains(&self, file_path: &CanonicalPath) -> bool {
        if !file_path.starts_with_dir(&self.root) {
            return false;
        }
        !self.exclude.iter().any(|glob| glob.matches(file_path))
    }
}

impl StaticMembershipSpec {
    /// Check whether a path is a static member according to TypeScript rules.
    ///
    /// Order: `files` first (immune to exclude), then `include - exclude`.
    /// This fixes the bug where the old code checked exclude before files.
    pub fn matches(&self, path: &CanonicalPath) -> bool {
        // Step 1: files are ALWAYS members — immune to exclude
        if self.files.iter().any(|f| f == path) {
            return true;
        }

        // Step 2: check include patterns
        let included = if self.include.is_empty() {
            false
        } else {
            self.include.iter().any(|glob| glob.matches(path))
        };

        if !included {
            return false;
        }

        // Step 3: exclude only filters what include found
        !self.exclude.iter().any(|glob| glob.matches(path))
    }

    /// Create a spec with TypeScript defaults filled in.
    ///
    /// When tsconfig has no `files`, no `include`, no `exclude`:
    /// - `include` defaults to `["{root}/**/*"]`
    /// - `exclude` defaults to `["{root}/node_modules/**", ...]`
    pub fn with_typescript_defaults(root: &CanonicalPath) -> Self {
        Self {
            files: Vec::new(),
            include: vec![NormalizedGlob::from_root_and_pattern(root, "**/*")],
            exclude: typescript_default_excludes(root),
        }
    }
}

/// TypeScript's default exclude patterns for a project root.
pub fn typescript_default_excludes(root: &CanonicalPath) -> Vec<NormalizedGlob> {
    vec![
        NormalizedGlob::from_root_and_pattern(root, "node_modules/**"),
        NormalizedGlob::from_root_and_pattern(root, "bower_components/**"),
        NormalizedGlob::from_root_and_pattern(root, "jspm_packages/**"),
    ]
}

#[cfg(test)]
#[path = "membership_tests.rs"]
mod tests;

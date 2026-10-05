//! Exact membership types for project ownership.
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
//!
//! Construction from raw tsconfig config values (`with_typescript_defaults`,
//! `from_includes`, `with_supported_extension_defaults`, and the
//! `SupportedExtensions`/`ProjectMembership`/`FallbackMembership` types they
//! depend on) is owned by `verter_workspace` as config ingress. This module
//! owns only the
//! dependency-neutral value types and their query-time predicates
//! (`contains`, `directly_includes`, `StaticMembershipSpec::matches`,
//! compiled glob matching, `typescript_default_excludes`).

use super::normalized_glob::CompiledGlob;
use parking_lot::RwLock;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::{Arc, LazyLock};
use verter_span::path::CanonicalPath;

/// Static membership specification parsed from a tsconfig.
///
/// Always explicit — no `MatchAll` variant. When a tsconfig has no
/// `files`, no `include`, no `exclude`, the builder fills in TypeScript
/// defaults: `include: ["{dir}/**/*"]`, `exclude: ["{dir}/node_modules/**", ...]`.
///
/// Glob patterns are stored precompiled ([`CompiledGlob`]): membership
/// match loops run per ownership query, and compiling on every match
/// dominated the query cost. Compilation happens once, at membership
/// construction (snapshot build) time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticMembershipSpec {
    /// Exact file paths. **Immune to exclude** — always members.
    pub files: Vec<CanonicalPath>,
    /// Glob patterns. Builder fills default `["**/*"]` when needed.
    pub include: Vec<CompiledGlob>,
    /// Only filters `include`. Builder fills TS defaults when needed.
    ///
    /// A shared slice: the TS-default exclude set is memoized per root
    /// ([`typescript_default_excludes`]) and shared by every membership
    /// built for that root, so cloning a spec never recompiles globs.
    pub exclude: Arc<[CompiledGlob]>,
}

impl StaticMembershipSpec {
    /// Check whether a path is a static member according to TypeScript rules.
    ///
    /// Order: `files` first (immune to exclude), then `include - exclude`.
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
}

/// Configured project membership: static spec + materialized file set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredMembership {
    pub spec: StaticMembershipSpec,
    /// Exact set of files determined to be members of this project.
    /// Populated during snapshot build by expanding the static spec.
    pub materialized_files: FxHashSet<CanonicalPath>,
}

impl ConfiguredMembership {
    /// Check if a file is a member of this configured project.
    ///
    /// `materialized_files` is a WALK-TIME POSITIVE CACHE, not a negative
    /// ownership authority. It records the spec-matching files that existed
    /// when the snapshot was built, so an exact hit is the fast, precise
    /// "yes". A MISS is NOT terminal: it falls through to `spec.matches`
    /// (the compiled `files`/`include`/`exclude` globs), which is the
    /// ownership authority. This is what lets a file CREATED AFTER the
    /// snapshot walk — absent from the materialized set but matching the
    /// project's globs — still resolve to its owning configured project
    /// instead of failing closed to `NoProject`.
    ///
    /// An empty `materialized_files` — a filesystem-less environment (WASM,
    /// in-memory workspace) or a spec-defined match-all membership — simply
    /// never hits the positive cache and routes straight to `spec.matches`.
    pub fn contains(&self, file_path: &CanonicalPath) -> bool {
        // Fast POSITIVE path: an exact hit on the walk-time materialized set.
        if self.materialized_files.contains(file_path) {
            return true;
        }
        // MISS (incl. a file created after the walk, or an empty set): the
        // compiled `files`/`include`/`exclude` spec is the ownership authority.
        self.spec.matches(file_path)
    }

    /// Whether this configured project's PROGRAM directly includes `file_path`
    /// — the tsgo `GetDefaultProject` / `findDefaultConfiguredProject` model of
    /// membership, distinct from the general [`Self::contains`] ownership query.
    ///
    /// The default-configured-owner walk resolves the ONE configured project
    /// whose loaded program actually contains the file, so a NON-EMPTY
    /// `materialized_files` set (the walk-time program file set) is
    /// AUTHORITATIVE: a file the walk did not materialize is not part of the
    /// program and is not a claimant, even when the project's include glob
    /// would match it.
    ///
    /// An EMPTY `materialized_files` — a filesystem-less environment (WASM,
    /// in-memory workspace) or a spec-defined match-all membership — has no
    /// program file set to be authoritative, so it routes to `spec.matches`,
    /// where every include / `files` hit is a direct inclusion.
    ///
    /// Contrast [`Self::contains`], the general ownership predicate, which
    /// treats `materialized_files` as a positive cache and always falls
    /// through to `spec.matches` on a miss so a file created after the walk
    /// still routes to its owning project.
    pub fn directly_includes(&self, file_path: &CanonicalPath) -> bool {
        // A populated program file set is authoritative for direct inclusion.
        if !self.materialized_files.is_empty() {
            return self.materialized_files.contains(file_path);
        }
        // No program file set (filesystem-less / match-all): the compiled spec
        // is the direct-inclusion authority.
        self.spec.matches(file_path)
    }
}

/// TypeScript's default exclude patterns, relative to a project root.
const TYPESCRIPT_DEFAULT_EXCLUDE_PATTERNS: &[&str] =
    &["node_modules/**", "bower_components/**", "jspm_packages/**"];

/// Bound on distinct roots retained by the default-excludes memo. Real
/// processes see a handful of project roots; the bound only protects
/// long-lived processes that touch many transient roots (e.g. in-process
/// test suites over temp dirs). On overflow the memo is cleared — a pure
/// recompute, never a correctness change.
const DEFAULT_EXCLUDES_MEMO_CAP: usize = 64;

/// Process-wide per-root memo for [`typescript_default_excludes`]: the
/// compiled set is a pure function of the root, and membership
/// construction sites run hot, so each root compiles its three
/// default-exclude globs exactly once per process.
static DEFAULT_EXCLUDES_MEMO: LazyLock<RwLock<FxHashMap<CanonicalPath, Arc<[CompiledGlob]>>>> =
    LazyLock::new(|| RwLock::new(FxHashMap::default()));

/// TypeScript's default exclude patterns for a project root, precompiled.
///
/// Returns a shared `Arc` slice memoized per root: repeated calls for the
/// same root hand out the same allocation instead of recompiling the glob
/// set (see [`DEFAULT_EXCLUDES_MEMO`]).
pub fn typescript_default_excludes(root: &CanonicalPath) -> Arc<[CompiledGlob]> {
    if let Some(hit) = DEFAULT_EXCLUDES_MEMO.read().get(root) {
        return Arc::clone(hit);
    }

    let compiled: Arc<[CompiledGlob]> = TYPESCRIPT_DEFAULT_EXCLUDE_PATTERNS
        .iter()
        .map(|pattern| {
            CompiledGlob::new(
                super::normalized_glob::NormalizedGlob::from_root_and_pattern(root, pattern),
            )
        })
        .collect();

    let mut memo = DEFAULT_EXCLUDES_MEMO.write();
    if memo.len() >= DEFAULT_EXCLUDES_MEMO_CAP {
        memo.clear();
    }
    memo.insert(root.clone(), Arc::clone(&compiled));
    compiled
}

#[cfg(test)]
#[path = "membership_tests.rs"]
mod tests;

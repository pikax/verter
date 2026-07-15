//! Normalized glob pattern for workspace membership filtering.
//!
//! A `NormalizedGlob` is a glob pattern with forward slashes and a
//! canonical root prefix. Unlike [`CanonicalPath`], it may contain
//! wildcard characters (`*`, `?`, `[`, `]`).

use crate::canonical_path::CanonicalPath;

/// A glob pattern normalized for cross-platform matching.
///
/// Invariants:
/// - Forward slashes only
/// - Root prefix is canonical (lowercase drive on Windows)
/// - May contain `*`, `?`, `[`, `]` wildcard characters
///
/// Use [`NormalizedGlob::matches()`] to test whether a [`CanonicalPath`]
/// matches this pattern.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NormalizedGlob(String);

impl NormalizedGlob {
    /// Create from a raw glob string, normalizing slashes and drive letter.
    pub fn new(raw: &str) -> Self {
        let normalized = raw.replace('\\', "/");

        // Lowercase drive letter on Windows
        #[cfg(windows)]
        let normalized = {
            if normalized.len() >= 2 && normalized.as_bytes()[1] == b':' {
                let mut chars = normalized.chars();
                if let Some(first) = chars.next() {
                    format!("{}{}", first.to_ascii_lowercase(), chars.as_str())
                } else {
                    normalized
                }
            } else {
                normalized
            }
        };

        Self(normalized)
    }

    /// Create from a root directory and a relative pattern.
    ///
    /// Joins `root` and `pattern` with `/`, normalizing the root
    /// via [`CanonicalPath`] and the pattern via slash normalization.
    pub fn from_root_and_pattern(root: &CanonicalPath, pattern: &str) -> Self {
        let pattern = pattern.replace('\\', "/");
        let pattern = pattern.trim_start_matches('/');
        Self(format!("{}/{}", root.as_str(), pattern))
    }

    /// Return the inner glob string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Test whether a canonical path matches this glob pattern.
    ///
    /// Uses the `glob` crate's `Pattern` for matching. Compiles the pattern
    /// on every call — fine for one-off checks, but match loops that run per
    /// query must use [`CompiledGlob`] instead.
    pub fn matches(&self, path: &CanonicalPath) -> bool {
        glob::Pattern::new(&self.0)
            .map(|pat| pat.matches_with(path.as_str(), match_options()))
            .unwrap_or(false)
    }
}

/// Match options shared by [`NormalizedGlob::matches`] and
/// [`CompiledGlob::matches`] — the two must never diverge.
///
/// TypeScript's `*` does NOT match `/` — only `**` matches across directory
/// boundaries. `require_literal_separator: true` enforces this so
/// `d:/project/*` won't match `d:/project/src/foo.ts`.
const fn match_options() -> glob::MatchOptions {
    glob::MatchOptions {
        case_sensitive: !cfg!(windows),
        require_literal_separator: true,
        require_literal_leading_dot: false,
    }
}

/// A [`NormalizedGlob`] with its [`glob::Pattern`] compiled once up front.
///
/// [`NormalizedGlob::matches`] compiles the pattern on every call, which
/// dominates the cost of membership match loops that run per ownership
/// query ([`WorkspaceSnapshot::owners_for_file`]). Membership structs
/// therefore store `CompiledGlob`s, compiled once at snapshot/membership
/// construction time — snapshot construction is already the invalidation
/// boundary, so a compiled pattern can never go stale. [`NormalizedGlob`]
/// keeps its plain-string value semantics (`Clone`/`PartialEq`/`Hash`).
///
/// Matching semantics are identical to [`NormalizedGlob::matches`]: same
/// [`glob::MatchOptions`], and an invalid pattern (stored as `pattern:
/// None`) never matches.
///
/// Equality/`Eq` compare the RAW glob only: the compiled pattern is a
/// deterministic function of the raw string, so two `CompiledGlob`s are
/// equal exactly when their raw globs are — the same value semantics the
/// membership specs had when they stored bare [`NormalizedGlob`]s.
///
/// [`WorkspaceSnapshot::owners_for_file`]: crate::workspace_snapshot::WorkspaceSnapshot::owners_for_file
#[derive(Debug, Clone)]
pub struct CompiledGlob {
    raw: NormalizedGlob,
    /// `None` when the raw pattern fails to compile — such globs never match.
    pattern: Option<glob::Pattern>,
}

impl CompiledGlob {
    /// Compile a normalized glob once.
    pub fn new(raw: NormalizedGlob) -> Self {
        let pattern = glob::Pattern::new(raw.as_str()).ok();
        Self { raw, pattern }
    }

    /// The raw normalized glob this was compiled from.
    pub fn raw(&self) -> &NormalizedGlob {
        &self.raw
    }

    /// Return the inner glob string.
    pub fn as_str(&self) -> &str {
        self.raw.as_str()
    }

    /// Test whether a canonical path matches, using the precompiled pattern.
    ///
    /// Same semantics as [`NormalizedGlob::matches`]: identical
    /// [`glob::MatchOptions`], invalid pattern → `false`.
    pub fn matches(&self, path: &CanonicalPath) -> bool {
        self.pattern
            .as_ref()
            .is_some_and(|pat| pat.matches_with(path.as_str(), match_options()))
    }
}

impl From<NormalizedGlob> for CompiledGlob {
    fn from(raw: NormalizedGlob) -> Self {
        Self::new(raw)
    }
}

impl PartialEq for CompiledGlob {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl Eq for CompiledGlob {}

impl std::fmt::Display for NormalizedGlob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for NormalizedGlob {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
#[path = "normalized_glob_tests.rs"]
mod tests;

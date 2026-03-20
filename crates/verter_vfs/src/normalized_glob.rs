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
    /// Uses the `glob` crate's `Pattern` for matching.
    pub fn matches(&self, path: &CanonicalPath) -> bool {
        let options = glob::MatchOptions {
            case_sensitive: !cfg!(windows),
            // TypeScript's `*` does NOT match `/` — only `**` matches
            // across directory boundaries. `require_literal_separator: true`
            // enforces this so `d:/project/*` won't match `d:/project/src/foo.ts`.
            require_literal_separator: true,
            require_literal_leading_dot: false,
        };
        glob::Pattern::new(&self.0)
            .map(|pat| pat.matches_with(path.as_str(), options))
            .unwrap_or(false)
    }
}

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

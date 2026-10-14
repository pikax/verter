//! Normalized glob pattern for workspace membership filtering.
//!
//! A `NormalizedGlob` is a glob pattern with forward slashes and a
//! canonical root prefix. Unlike [`CanonicalPath`], it may contain
//! wildcard characters (`*`, `?`, `[`, `]`).

use verter_span::path::CanonicalPath;

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

/// [`match_options`] for a tsconfig `include` entry: a wildcard does not
/// match a leading `.` (TypeScript's `[^./]` component start).
const fn typescript_include_match_options() -> glob::MatchOptions {
    glob::MatchOptions {
        require_literal_leading_dot: true,
        ..match_options()
    }
}

/// The package folders TypeScript's include wildcards never descend into
/// (`implicitExcludePathRegexPattern`).
const IMPLICIT_EXCLUDED_FOLDERS: [&str; 3] = ["node_modules", "bower_components", "jspm_packages"];

/// Whether every implicitly excluded folder component of `path` is spelled
/// literally by `pattern`: a pattern component that names the folder is the
/// only one that matches it, so `path` may carry no more of each such
/// component than `pattern` spells.
fn admits_implicitly_excluded_folders(pattern: &str, path: &str) -> bool {
    IMPLICIT_EXCLUDED_FOLDERS.iter().all(|folder| {
        let count = |text: &str| text.split('/').filter(|part| part == folder).count();
        count(path) <= count(pattern)
    })
}

/// A [`NormalizedGlob`] with its [`glob::Pattern`] compiled once up front.
///
/// [`NormalizedGlob::matches`] compiles the pattern on every call, which
/// dominates the cost of membership match loops that run per ownership
/// query. Membership structs therefore store `CompiledGlob`s, compiled once
/// at snapshot/membership construction time. [`NormalizedGlob`] keeps its
/// plain-string value semantics (`Clone`/`PartialEq`/`Hash`).
///
/// Matching semantics are identical to [`NormalizedGlob::matches`]: same
/// [`glob::MatchOptions`], and an invalid pattern (stored as `pattern:
/// None`) never matches.
///
/// Equality/`Eq` compare the RAW glob only: the compiled pattern is a
/// deterministic function of the raw string, so two `CompiledGlob`s are
/// equal exactly when their raw globs are — the same value semantics the
/// membership specs had when they stored bare [`NormalizedGlob`]s.
#[derive(Debug, Clone)]
pub struct CompiledGlob {
    raw: NormalizedGlob,
    /// `None` when the raw pattern fails to compile — such globs never match.
    pattern: Option<glob::Pattern>,
    /// Index of the authored `include` entry this glob expands. One entry
    /// may expand into a glob per supported extension; TypeScript lists the
    /// files each entry matches together, in entry order, so the program's
    /// root order needs the authored entry, not the expanded position.
    include_entry: u32,
    /// Whether this glob is an authored tsconfig `include` entry, matched
    /// with TypeScript's include wildcard semantics (see
    /// [`CompiledGlob::as_typescript_include`]).
    typescript_include: bool,
}

impl CompiledGlob {
    /// Compile a normalized glob once.
    pub fn new(raw: NormalizedGlob) -> Self {
        let pattern = glob::Pattern::new(raw.as_str()).ok();
        Self {
            raw,
            pattern,
            include_entry: 0,
            typescript_include: false,
        }
    }

    /// This glob as an expansion of the authored `include` entry `entry`.
    #[must_use]
    pub fn in_include_entry(mut self, entry: u32) -> Self {
        self.include_entry = entry;
        self
    }

    /// Match this glob as a tsconfig `include` entry, as TypeScript's
    /// wildcard matcher does (`getSubPatternFromSpec`): a wildcard (`*`,
    /// `?`, `**`) never matches a path component that starts with `.`, and
    /// never matches a `node_modules`, `bower_components` or `jspm_packages`
    /// component — whatever `exclude` says. Such a component matches only
    /// when the pattern spells it literally.
    #[must_use]
    pub fn as_typescript_include(mut self) -> Self {
        self.typescript_include = true;
        self
    }

    /// Whether a file below directory `dir` could match this glob — false
    /// only when `dir` ends in a component an include wildcard cannot match
    /// (see [`Self::as_typescript_include`]) and the pattern does not spell
    /// it. A directory walk prunes such a subtree.
    pub fn may_match_below(&self, dir: &CanonicalPath) -> bool {
        if !self.typescript_include {
            return true;
        }
        let Some(last) = dir.as_str().trim_end_matches('/').rsplit('/').next() else {
            return true;
        };
        let mut components = self.raw.as_str().split('/');
        if IMPLICIT_EXCLUDED_FOLDERS.contains(&last) {
            return components.any(|component| component == last);
        }
        if last.starts_with('.') {
            return components.any(|component| {
                component == last
                    || (component.starts_with('.') && component.contains(['*', '?', '[']))
            });
        }
        true
    }

    /// The authored `include` entry this glob expands.
    pub fn include_entry(&self) -> u32 {
        self.include_entry
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
    /// Same semantics as [`NormalizedGlob::matches`] (identical
    /// [`glob::MatchOptions`], invalid pattern → `false`), narrowed by the
    /// include wildcard rules for a [`Self::as_typescript_include`] glob.
    pub fn matches(&self, path: &CanonicalPath) -> bool {
        let Some(pattern) = self.pattern.as_ref() else {
            return false;
        };
        if !self.typescript_include {
            return pattern.matches_with(path.as_str(), match_options());
        }
        pattern.matches_with(path.as_str(), typescript_include_match_options())
            && admits_implicitly_excluded_folders(self.raw.as_str(), path.as_str())
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

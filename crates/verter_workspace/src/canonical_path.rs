//! Canonical path newtype for workspace-internal path storage.
//!
//! All paths stored in the workspace snapshot use `CanonicalPath` to ensure
//! consistent normalization:
//! - Windows-style drive prefixes: lowercase drive letter + forward slashes
//! - Linux/macOS paths without drive prefixes: forward slashes only, NO case transformation
//! - `\\?\` extended-length prefix stripped

/// A normalized filesystem path.
///
/// Invariants:
/// - Forward slashes only (no backslashes)
/// - Windows-style drive prefixes: lowercase drive letter, `\\?\` prefix stripped
/// - Paths without drive prefixes: no case transformation
/// - No trailing slash (except root `/` or `C:/`)
///
/// Distinct from [`NormalizedGlob`]: a `CanonicalPath` never contains
/// wildcard characters (`*`, `?`, `[`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CanonicalPath(String);

impl CanonicalPath {
    /// Create a new canonical path from a raw string.
    ///
    /// Applies normalization: backslash→forward slash, strip `\\?\`,
    /// lowercase Windows-style drive prefixes.
    pub fn new(raw: &str) -> Self {
        Self(canonicalize_path(raw))
    }

    /// Return the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume and return the inner String.
    pub fn into_string(self) -> String {
        self.0
    }

    /// Check if this path starts with the given prefix at a directory boundary.
    ///
    /// Returns `true` if `self` starts with `prefix` AND the character
    /// immediately after the prefix (if any) is `/`. This prevents
    /// `c:/project-extra` from matching prefix `c:/project`.
    pub fn starts_with_dir(&self, prefix: &CanonicalPath) -> bool {
        let s = self.as_str();
        let p = prefix.as_str();
        s.starts_with(p) && (s.len() == p.len() || s.as_bytes().get(p.len()) == Some(&b'/'))
    }
}

impl std::fmt::Display for CanonicalPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for CanonicalPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&str> for CanonicalPath {
    fn from(raw: &str) -> Self {
        Self::new(raw)
    }
}

impl From<String> for CanonicalPath {
    fn from(raw: String) -> Self {
        Self::new(&raw)
    }
}

/// Core normalization logic shared by `CanonicalPath::new()` and
/// can be used standalone when only a `String` is needed.
pub fn canonicalize_path(raw: &str) -> String {
    // Step 1: backslash → forward slash
    let normalized = raw.replace('\\', "/");

    // Step 2: strip Windows extended-length prefix
    let normalized = if let Some(rest) = normalized.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = normalized.strip_prefix("//?/") {
        rest.to_string()
    } else {
        normalized
    };

    // Step 3: lowercase Windows-style drive prefixes (C:/ → c:/) even on
    // non-Windows hosts, because canonical IDs may still use Windows paths.
    let normalized = if normalized.len() >= 2 && normalized.as_bytes()[1] == b':' {
        let mut chars = normalized.chars();
        if let Some(first) = chars.next() {
            format!("{}{}", first.to_ascii_lowercase(), chars.as_str())
        } else {
            normalized
        }
    } else {
        normalized
    };

    // Step 4: strip trailing slash (except root "/" or "X:/")
    let is_root = normalized == "/"
        || (normalized.len() == 3
            && normalized.as_bytes().get(1) == Some(&b':')
            && normalized.as_bytes().get(2) == Some(&b'/'));
    if !is_root && normalized.ends_with('/') {
        normalized[..normalized.len() - 1].to_string()
    } else {
        normalized
    }
}

#[cfg(test)]
#[path = "canonical_path_tests.rs"]
mod tests;

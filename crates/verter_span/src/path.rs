//! Canonical filesystem-path normalization — the single owner for the whole
//! workspace.
//!
//! Every consumer crate (LSP, type-runtime/tsserver IPC, scheduler source
//! loader, workspace project graph) routes path-id normalization through this
//! module so the canonical-ID format is identical everywhere.
//!
//! Canonical-ID format:
//! - Forward slashes only (no backslashes).
//! - Windows drive prefixes: lowercase drive letter, colon kept (`c:/Users/Dev`)
//!   — never `C:/`, never the drive-as-segment `/c/` form.
//! - `\\?\` / `\\?\UNC\` extended-length prefixes stripped.
//! - No trailing slash, except the roots `/` and `x:/`.
//! - All other casing is preserved (case-sensitive Linux paths must round-trip
//!   unchanged).

use std::borrow::Cow;

/// Whether `s` ends with a trailing `/` that should be stripped — i.e. it is
/// not the filesystem root `/` and not a Windows drive-root `x:/`.
fn ends_with_strippable_slash(s: &str) -> bool {
    if !s.ends_with('/') || s == "/" {
        return false;
    }
    let b = s.as_bytes();
    // drive-root `x:/`
    !(b.len() == 3 && b[1] == b':' && b[2] == b'/')
}

/// The single canonical normalization, allocating only when a transform is
/// actually required.
///
/// Returns [`Cow::Borrowed`] for the common macOS/Linux case where the input is
/// already canonical (no backslash, no `//?/` prefix, drive already lowercase or
/// absent, no strippable trailing slash); otherwise [`Cow::Owned`].
pub fn canonicalize_path_cow(raw: &str) -> Cow<'_, str> {
    let bytes = raw.as_bytes();
    let has_backslash = raw.contains('\\');
    let has_ext_prefix = raw.starts_with("//?/");
    let drive_needs_lower = bytes.len() >= 2
        && bytes[1] == b':'
        && bytes[0].is_ascii_alphabetic()
        && bytes[0].is_ascii_uppercase();
    let trailing_strip = ends_with_strippable_slash(raw);

    if !has_backslash && !has_ext_prefix && !drive_needs_lower && !trailing_strip {
        return Cow::Borrowed(raw);
    }
    Cow::Owned(canonicalize_owned(raw))
}

fn canonicalize_owned(raw: &str) -> String {
    // Step 1: backslash → forward slash (only allocate if needed).
    let normalized = if raw.contains('\\') {
        raw.replace('\\', "/")
    } else {
        raw.to_string()
    };

    // Step 2: strip Windows extended-length prefix — `//?/UNC/` BEFORE `//?/`.
    let normalized = if let Some(rest) = normalized.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = normalized.strip_prefix("//?/") {
        rest.to_string()
    } else {
        normalized
    };

    // Step 3: lowercase the Windows drive letter only (keep the colon), even on
    // non-Windows hosts — canonical IDs may carry Windows paths. NEVER `/c/`.
    let normalized = {
        let b = normalized.as_bytes();
        if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() && b[0].is_ascii_uppercase() {
            let mut s = String::with_capacity(normalized.len());
            s.push((b[0] as char).to_ascii_lowercase());
            s.push_str(&normalized[1..]);
            s
        } else {
            normalized
        }
    };

    // Step 4: strip ALL trailing slashes except the roots `/` and `x:/`.
    // Looped so the result is idempotent — `/a//` and `/a///` both canonicalize
    // to `/a`, otherwise one call would leave a residual `/a/` and a second call
    // would change it again (two canonical IDs for the same directory).
    if ends_with_strippable_slash(&normalized) {
        let mut s = normalized;
        while ends_with_strippable_slash(&s) {
            s.pop();
        }
        s
    } else {
        normalized
    }
}

/// The single canonical normalization, owned. Equal to
/// `canonicalize_path_cow(raw).into_owned()`.
pub fn canonicalize_path(raw: &str) -> String {
    canonicalize_path_cow(raw).into_owned()
}

/// A normalized filesystem path.
///
/// Invariants:
/// - Forward slashes only (no backslashes).
/// - Windows drive prefixes: lowercase drive letter, colon kept, `\\?\` prefix
///   stripped.
/// - Paths without drive prefixes: no case transformation.
/// - No trailing slash (except root `/` or `x:/`).
///
/// Distinct from the workspace `NormalizedGlob`: a `CanonicalPath` never
/// contains wildcard characters (`*`, `?`, `[`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CanonicalPath(String);

impl CanonicalPath {
    /// Create a new canonical path from a raw string.
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
        s.starts_with(p)
            && (s.len() == p.len() || p.ends_with('/') || s.as_bytes().get(p.len()) == Some(&b'/'))
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

/// Directory-boundary containment test: is `path` equal to or under `root`?
///
/// Both sides are canonicalized first (cheap borrow when already canonical).
/// Case-PRESERVING — a case mismatch is not contained. Rejects sibling-prefix
/// matches (`/a/project-extra` is NOT under `/a/project`).
pub fn is_under_dir(path: &str, root: &str) -> bool {
    let p = canonicalize_path_cow(path);
    let r = canonicalize_path_cow(root);
    let (p, r) = (p.as_ref(), r.as_ref());
    p.starts_with(r)
        && (p.len() == r.len()
            // When the root itself ends in `/` (the canonical roots `/` and
            // `x:/`), the prefix boundary IS the slash — the byte after the
            // prefix is a real path char, not another `/`.
            || r.ends_with('/')
            || p.as_bytes()[r.len()] == b'/')
}

/// Return the LONGEST `root` in `roots` that contains `file`, in CANONICAL form
/// (directory-boundary containment); falls back to `workspace_root` when none
/// match.
///
/// Correctness does not depend on caller sort order — the longest match is
/// computed here. The result is always canonical: the winning root (or the
/// fallback) is returned through [`canonicalize_path_cow`], so a caller that
/// stored a raw extended-prefix root (`//?/C:/repo`) still receives the
/// canonical `c:/repo` and never leaks the raw form back out (e.g. as a
/// `projectRootPath`). The `Cow` borrows when the winner is already canonical
/// (the common case, since callers canonicalize at push time) and only
/// allocates when a transform is actually needed.
pub fn longest_project_root<'a>(
    file: &str,
    roots: &'a [String],
    workspace_root: &'a str,
) -> Cow<'a, str> {
    // Canonicalize `file` once (FIX 2b) rather than per-iteration.
    let p = canonicalize_path_cow(file);
    let p = p.as_ref();

    // Track the best match by CANONICAL length — a raw `//?/` extended prefix
    // inflates `root.len()` and could otherwise beat a genuinely deeper
    // canonical root. The winning raw root is re-canonicalized for the return.
    let mut best: Option<(&'a str, usize)> = None;
    for root in roots {
        let r = canonicalize_path_cow(root);
        let r = r.as_ref();
        let contained = p.starts_with(r)
            && (p.len() == r.len() || r.ends_with('/') || p.as_bytes()[r.len()] == b'/');
        if contained {
            let canon_len = r.len();
            match best {
                Some((_, best_len)) if best_len >= canon_len => {}
                _ => best = Some((root.as_str(), canon_len)),
            }
        }
    }
    match best {
        Some((root, _)) => canonicalize_path_cow(root),
        None => canonicalize_path_cow(workspace_root),
    }
}

#[cfg(test)]
#[path = "path_tests.rs"]
mod tests;

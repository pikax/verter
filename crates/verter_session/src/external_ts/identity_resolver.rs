//! URI/path → canonical [`ProjectIdentity`] reference resolution, applied
//! BEFORE the redirect-reference graph is built.
//!
//! [`mode`](super::mode) consumes an already-resolved
//! [`RedirectRef::Resolved`]`(`[`ProjectIdentity`]`)` / [`RedirectRef::Unresolved`]
//! graph; this module is where a raw project-reference URI or path becomes one
//! of those. A reference is canonicalized to the referenced project's tsconfig
//! path and mapped through the authoritative identity source; a reference that
//! cannot be canonicalized (an unsupported scheme, a malformed URI, an empty
//! path) becomes [`RedirectRef::Unresolved`] — the poison path — NEVER a
//! fabricated identity.
//!
//! ## Canonicalization discipline (cross-platform Win / macOS / Linux)
//!
//! For each reference, in order:
//! 1. Parse the scheme. `file://` (including the Windows drive form
//!    `file:///C:/…` and the UNC form `file://host/share/…`) becomes a path via
//!    the shared [`verter_span::uri::file_uri_to_path`]; any OTHER scheme
//!    (`untitled:`, `http:`, `vscode-vfs:`, a malformed `file:` without `//`) is
//!    UNSUPPORTED and fails closed to [`RedirectRef::Unresolved`]. A bare
//!    `C:/…` drive path is a path, not a scheme.
//! 2. Resolve a relative reference against the REFERENCING tsconfig's directory.
//! 3. Normalize `.` / `..` and separators via the shared
//!    [`verter_workspace::resolver::collapse_path`] (drive-lowercased,
//!    slash-normalized, UNC-aware — never a hand-rolled path joiner).
//! 4. Resolve a directory reference to its `tsconfig.json` target (a reference
//!    that does not already end in `.json` denotes a directory).
//! 5. Realpath / symlink-resolve the config path when it EXISTS (via the
//!    injected [`ConfigPathProbe`]), so symlinked configs share one identity.
//! 6. Map the canonical config path through the [`ProjectIdentitySource`] — a
//!    FOLDED canonical lookup, so two paths denoting the same file under the
//!    host filesystem's case policy resolve to ONE identity.
//!
//! The authoritative identity remains the existing per-canonical function
//! (`host_view_project_identity_for`); this module produces the canonical
//! tsconfig path that FEEDS it. It never hashes an unresolved string into a fake
//! identity.

use std::sync::Arc;

use verter_span::path::canonicalize_path;
use verter_span::uri::file_uri_to_path;
use verter_workspace::resolver::collapse_path;

use crate::file_artifact_store::ProjectIdentity;

use super::mode::{ProjectEligibility, RedirectRef, RedirectReferenceGraph};

/// Symlink / existence probe for config paths — the ONE piece of I/O this pure
/// decision layer needs, injected so the layer stays headless-testable.
///
/// Production wraps `verter_workspace::traits::WorkspaceRead::realpath`; tests
/// supply an explicit map/closure. `realpath` returns the real (symlink-resolved)
/// path for an EXISTING config path, or `None` when the path does not exist or
/// cannot be resolved (in which case the collapsed path is used as-is).
pub trait ConfigPathProbe {
    /// The real (symlink-resolved) path of an existing config path, or `None`.
    fn realpath(&self, canonical: &str) -> Option<String>;
}

impl<F> ConfigPathProbe for F
where
    F: Fn(&str) -> Option<String>,
{
    fn realpath(&self, canonical: &str) -> Option<String> {
        self(canonical)
    }
}

/// The authoritative canonical-config-path → [`ProjectIdentity`] mapping.
///
/// This is a FOLDED canonical lookup: it MUST map two canonical config paths
/// that denote the same file under the host filesystem's case policy
/// ([`verter_span::path::fs_is_case_insensitive`]) to the SAME identity, so
/// case-variant reference URIs of one tsconfig never mint two identities.
/// Production wires the host's per-canonical identity reader
/// (`host_view_project_identity_for`, whose owner resolution folds case through
/// the workspace membership set); headless tests wire an
/// [`verter_span::path::InjectedPathKey`]-folding closure. It is called ONLY with
/// an already-canonical path, and it always returns an identity (identity is a
/// total function of the canonical path — the fail-closed case is handled one
/// level up, by returning [`RedirectRef::Unresolved`] before any identity is
/// requested).
pub trait ProjectIdentitySource {
    /// The identity of the configured project at `canonical_config_path`.
    fn identity_for(&self, canonical_config_path: &str) -> ProjectIdentity;
}

impl<F> ProjectIdentitySource for F
where
    F: Fn(&str) -> ProjectIdentity,
{
    fn identity_for(&self, canonical_config_path: &str) -> ProjectIdentity {
        self(canonical_config_path)
    }
}

/// One project reference as declared by a tsconfig, tagged with whether it
/// participates in source-of-project-reference redirect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceInput {
    /// The raw reference (a tsconfig `references[].path`, or an editor-supplied
    /// URI).
    pub reference: Arc<str>,
    /// Whether this reference participates in source-of-project-reference
    /// redirect. A reference under `disableSourceOfProjectReferenceRedirect:
    /// true` decouples the two Programs (the boundary is the emitted-declaration
    /// API carrier), so it is NOT a graph edge and is EXCLUDED by
    /// [`build_redirect_reference_graph`].
    pub redirect_enabled: bool,
}

impl ReferenceInput {
    /// A redirect-ON reference (a real potential graph edge).
    #[must_use]
    pub fn redirect_on(reference: impl Into<Arc<str>>) -> Self {
        Self {
            reference: reference.into(),
            redirect_enabled: true,
        }
    }

    /// A reference under `disableSourceOfProjectReferenceRedirect: true` — NOT a
    /// graph edge.
    #[must_use]
    pub fn redirect_disabled(reference: impl Into<Arc<str>>) -> Self {
        Self {
            reference: reference.into(),
            redirect_enabled: false,
        }
    }
}

/// One project's node inputs for [`build_redirect_reference_graph`]: its
/// canonical identity, its pre-composed [`ProjectEligibility`], the directory of
/// its own tsconfig (the base for resolving its relative references), and its
/// declared references.
#[derive(Debug, Clone)]
pub struct ProjectGraphInput<'a> {
    /// The project's own canonical identity (already resolved by the caller — a
    /// project knows its own identity without going through this resolver).
    pub identity: ProjectIdentity,
    /// The project's pre-composed SHARED eligibility.
    pub eligibility: ProjectEligibility,
    /// The directory containing this project's tsconfig — the base against which
    /// its relative references resolve.
    pub tsconfig_dir: &'a str,
    /// The project's declared references (both redirect-ON and redirect-disabled;
    /// the builder excludes the disabled ones).
    pub references: &'a [ReferenceInput],
}

/// Resolve one reference to its canonical config path, applying the full
/// canonicalization discipline. Returns `None` (→ [`RedirectRef::Unresolved`])
/// for an unsupported scheme, a malformed URI, or an empty/rootless result.
///
/// The result feeds both the identity lookup ([`resolve_reference_identity`])
/// and — for the referenced project — the warm-cache canonical-tsconfig-path
/// key, so it is exposed directly.
#[must_use]
pub fn resolve_reference_canonical_path(
    reference: &str,
    referencing_tsconfig_dir: &str,
    probe: &dyn ConfigPathProbe,
) -> Option<String> {
    // 1. Scheme: `file://` → path; unsupported/malformed scheme → fail closed.
    let path = reference_to_path(reference)?;
    if path.trim().is_empty() {
        return None;
    }

    // 2. Resolve a relative reference against the referencing tsconfig's dir.
    let joined = if is_absolute_path(&path) {
        path
    } else {
        format!("{referencing_tsconfig_dir}/{path}")
    };

    // 3. Normalize separators + collapse `.`/`..` (drive-lowered, UNC-aware) via
    //    the shared canonicalizer — never a hand-rolled joiner.
    let collapsed = collapse_path(&joined);
    if collapsed.is_empty() {
        return None;
    }

    // 4. A directory reference resolves to its `tsconfig.json` target; a
    //    reference already ending in `.json` is the config file itself.
    let config_path = if ends_with_json(&collapsed) {
        collapsed
    } else {
        format!("{collapsed}/tsconfig.json")
    };

    // 5. Realpath/symlink-resolve when the config EXISTS, so symlinked configs
    //    collapse to one canonical identity; otherwise use the collapsed path.
    let canonical = match probe.realpath(&config_path) {
        Some(real) => canonicalize_path(&real),
        None => config_path,
    };

    if canonical.is_empty() {
        None
    } else {
        Some(canonical)
    }
}

/// Resolve one reference to a [`RedirectRef`]: [`RedirectRef::Resolved`] with the
/// referenced project's canonical identity when the reference canonicalizes, or
/// [`RedirectRef::Unresolved`] (the poison path) when it does not — NEVER a
/// fabricated identity.
#[must_use]
pub fn resolve_reference_identity(
    reference: &str,
    referencing_tsconfig_dir: &str,
    probe: &dyn ConfigPathProbe,
    identity: &dyn ProjectIdentitySource,
) -> RedirectRef {
    match resolve_reference_canonical_path(reference, referencing_tsconfig_dir, probe) {
        Some(canonical) => RedirectRef::Resolved(identity.identity_for(&canonical)),
        None => RedirectRef::Unresolved,
    }
}

/// Build the [`RedirectReferenceGraph`] `mode` consumes from a set of resolved
/// project inputs.
///
/// For each project, every REDIRECT-ON reference (redirect-disabled ones are
/// excluded — they are not edges) is resolved through
/// [`resolve_reference_identity`] BEFORE the node is inserted, so the graph is
/// already canonical-identity-keyed and its [`RedirectRef::Unresolved`] entries
/// are the real poison signal (not a silently-dropped edge).
#[must_use]
pub fn build_redirect_reference_graph(
    projects: &[ProjectGraphInput],
    probe: &dyn ConfigPathProbe,
    identity: &dyn ProjectIdentitySource,
) -> RedirectReferenceGraph {
    let mut graph = RedirectReferenceGraph::new();
    for project in projects {
        let refs: Vec<RedirectRef> = project
            .references
            .iter()
            .filter(|r| r.redirect_enabled)
            .map(|r| {
                resolve_reference_identity(&r.reference, project.tsconfig_dir, probe, identity)
            })
            .collect();
        graph.insert_project(project.identity, project.eligibility, refs);
    }
    graph
}

// ── Canonicalization internals ──

/// Classify a raw reference into a filesystem path, or `None` for an
/// unsupported / malformed scheme (fail closed). A `file://` URI is decoded via
/// the shared parser; a plain path (including a `C:/…` drive path) passes
/// through; any other scheme fails closed.
fn reference_to_path(reference: &str) -> Option<String> {
    match uri_scheme(reference) {
        Some(scheme) if scheme.eq_ignore_ascii_case("file") => {
            // Only the standard `file://` authority form is supported; a
            // malformed opaque `file:/…` (no `//`) fails closed.
            let b = reference.as_bytes();
            if b.get(5) == Some(&b'/') && b.get(6) == Some(&b'/') {
                Some(file_uri_to_path(reference))
            } else {
                None
            }
        }
        // A non-file scheme (untitled:, http:, vscode-vfs:, …) is unsupported.
        Some(_) => None,
        // No scheme → a plain filesystem path.
        None => Some(reference.to_string()),
    }
}

/// The URI scheme of `s`, or `None` when `s` has none. A single ASCII letter
/// before the first `:` is a Windows DRIVE, not a scheme (`c:/x`); a real scheme
/// is ≥2 chars, starts with a letter, and uses only `[A-Za-z0-9+.-]`.
fn uri_scheme(s: &str) -> Option<&str> {
    let colon = s.find(':')?;
    let scheme = &s[..colon];
    // A single-letter prefix is a drive letter, not a scheme.
    if scheme.len() < 2 {
        return None;
    }
    let mut chars = scheme.chars();
    if !chars.next()?.is_ascii_alphabetic() {
        return None;
    }
    if !scheme
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
    {
        return None;
    }
    Some(scheme)
}

/// Whether `path` is absolute: a POSIX root (`/…`), a UNC root (`//host/…`), or a
/// Windows drive (`X:…`). Slash form is normalized first so a backslash drive
/// path (`C:\…`) is recognized.
fn is_absolute_path(path: &str) -> bool {
    let b = path.as_bytes();
    if b.first() == Some(&b'/') || b.first() == Some(&b'\\') {
        return true;
    }
    // Windows drive prefix `X:` (with or without a following separator).
    b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic()
}

/// Whether `path` already denotes a config FILE (`*.json`) rather than a
/// directory. Case-insensitive on the extension only, and char-boundary safe:
/// the `.json` suffix is compared over the trailing BYTES, so a reference path
/// ending inside a multibyte UTF-8 sequence classifies as a directory (fails
/// closed) instead of panicking on a non-char-boundary `&str` slice.
fn ends_with_json(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 5 && bytes[bytes.len() - 5..].eq_ignore_ascii_case(b".json")
}

#[cfg(test)]
#[path = "identity_resolver_tests.rs"]
mod tests;

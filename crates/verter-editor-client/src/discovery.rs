//! Pure server-discovery precedence (Strategy-D).
//!
//! This module DECIDES which `verter-lsp` binary source to launch; it performs
//! no filesystem, spawn, download, or network IO. The host gathers the raw
//! signals (an explicit override path, whether a verified pinned managed binary
//! is present, whether the user opted into PATH discovery, and any PATH hit) and
//! passes them in as plain data. The decision is a total function over those
//! inputs.
//!
//! Precedence, highest first:
//! 1. explicit user override path           → [`ServerSource::Override`]
//! 2. verified pinned managed binary present → [`ServerSource::Managed`]
//! 3. PATH hit, but ONLY if opted in         → [`ServerSource::Path`]
//! 4. nothing resolves                        → a loud [`DiscoveryError`]
//!    (never a silent fallback)
//!
//! # Host-path string constraint
//!
//! All path fields here are `&str`/`String`, not `Path`/`PathBuf`, BY DESIGN: the
//! host boundary for these editor clients is JSON / UTF-8 strings. Both supported
//! hosts expose UTF-8 launch/display paths — Zed extension APIs and Lapce's
//! `VOLT_*` environment hand the client UTF-8 — so these are host-provided UTF-8
//! launch/display strings. The host owns any `Path`/`PathBuf` conversion at the
//! IO boundary; this pure decision crate only routes the string it was given.
//!
//! # Managed-download forward-compat seam
//!
//! In the v0 model the host performs the managed-binary download, SHA-256
//! verification, and archive extraction OUT of this pure crate, then reports an
//! already-verified binary via [`DiscoveryInputs::managed_present`]. A future
//! pinned-download capability — e.g. a `ServerSource::Managed`-adjacent managed
//! descriptor carrying `{ path, version, sha256 }` plus a `DownloadAndLaunch`
//! decision when the binary is absent — is an ADDITIVE extension of the
//! [`ServerSource`] / [`DiscoveryInputs`] vocabulary, not a rewrite of this
//! decision. The download path and the `sha256` fields are intentionally NOT
//! implemented now (no release assets exist yet); this note documents the seam so
//! the extension is known to be additive.

use std::fmt;

/// The host-gathered inputs to a discovery decision.
///
/// All fields are already-resolved facts; this crate does not produce them. Path
/// fields are host-provided UTF-8 launch/display strings (see the module-level
/// "Host-path string constraint" note); the host owns any `Path` conversion.
///
/// This is the v0 input vocabulary: a managed binary is represented purely as an
/// already-verified path ([`Self::managed_present`]). A future pinned-download
/// variant is an additive extension (see the module-level "Managed-download
/// forward-compat seam" note), not a change to the existing fields.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiscoveryInputs<'a> {
    /// A user-set explicit binary path (highest precedence when present).
    pub override_path: Option<&'a str>,
    /// Path to a verified, version-pinned managed binary the host confirmed is
    /// present on disk. `None` when no managed binary is installed/verified.
    ///
    /// In v0 the host does the download + SHA-256 verification + extraction and
    /// only ever passes an already-verified path here.
    pub managed_present: Option<&'a str>,
    /// Whether the user explicitly opted into PATH-based discovery. PATH is
    /// never consulted unless this is `true`.
    pub path_opt_in: bool,
    /// A `verter-lsp` found on `PATH` by the host (e.g. `which verter-lsp`), if any.
    pub path_found: Option<&'a str>,
}

/// The decided launch source.
///
/// This is the v0 launch vocabulary. A future managed-download capability would
/// add an additive variant (e.g. a `DownloadAndLaunch` decision carrying a
/// `{ path, version, sha256 }` descriptor) rather than altering these; see the
/// module-level "Managed-download forward-compat seam" note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerSource {
    /// Launch the user's explicitly overridden binary.
    Override(String),
    /// Launch the verified pinned managed binary.
    Managed(String),
    /// Launch a binary discovered on `PATH` (only reachable when opted in).
    Path(String),
}

impl ServerSource {
    /// The resolved binary path, regardless of source kind.
    pub fn path(&self) -> &str {
        match self {
            ServerSource::Override(p) | ServerSource::Managed(p) | ServerSource::Path(p) => p,
        }
    }
}

/// Why discovery could not resolve a server source.
///
/// The two variants are deliberately distinct so the host can give targeted
/// guidance: a [`Self::PathFoundButNotOptedIn`] is actionable (the user can opt
/// into PATH discovery), whereas a [`Self::NothingResolved`] means no usable
/// binary exists anywhere (install, set an override, or download is required).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryError {
    /// A `verter-lsp` WAS found on `PATH`, but the user has not opted into PATH
    /// discovery — so it is intentionally not launched (opt-in only). This is the
    /// actionable case: the host can prompt the user to enable PATH discovery.
    PathFoundButNotOptedIn {
        /// Human-readable explanation naming PATH + the not-opted-in condition.
        reason: String,
        /// The PATH hit that was found but deliberately not used.
        found_on_path: String,
    },
    /// Nothing usable resolved at all: no override, no managed binary, and no
    /// usable PATH hit (either PATH was empty or, when opted in, nothing was
    /// found). A loud failure — never a silent fallback.
    NothingResolved {
        /// Human-readable explanation of why no server source could be resolved.
        reason: String,
    },
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiscoveryError::PathFoundButNotOptedIn { reason, .. }
            | DiscoveryError::NothingResolved { reason } => {
                write!(f, "could not resolve a verter-lsp server source: {reason}")
            }
        }
    }
}

impl std::error::Error for DiscoveryError {}

/// Decide which server source to launch from the host-gathered inputs.
///
/// Applies the Strategy-D precedence (override → managed → opted-in PATH →
/// loud fail). Returns a [`DiscoveryError`] — [`DiscoveryError::PathFoundButNotOptedIn`]
/// or [`DiscoveryError::NothingResolved`] — when nothing resolves; never a silent
/// default.
///
/// This is the v0 decision: when `managed_present` is `None` the crate does not
/// itself attempt a download (the host owns download + SHA-256 verification +
/// extraction). A future pinned-download capability is an additive extension of
/// the result vocabulary (see the module-level "Managed-download forward-compat
/// seam" note), not a change to this precedence.
pub fn resolve_server(inputs: &DiscoveryInputs) -> Result<ServerSource, DiscoveryError> {
    // 1. Explicit user override — highest precedence.
    if let Some(path) = inputs.override_path {
        return Ok(ServerSource::Override(path.to_string()));
    }

    // 2. Verified pinned managed binary already present on disk.
    if let Some(path) = inputs.managed_present {
        return Ok(ServerSource::Managed(path.to_string()));
    }

    // 3. PATH — only when the user explicitly opted in.
    if inputs.path_opt_in {
        if let Some(path) = inputs.path_found {
            return Ok(ServerSource::Path(path.to_string()));
        }
    }

    // 4. Nothing resolved — fail loud with a precise, DISTINCT reason; never a
    //    silent fallback to PATH or a default.
    if let Some(found) = inputs.path_found {
        if !inputs.path_opt_in {
            // Actionable: a binary IS on PATH; the user just hasn't opted in.
            return Err(DiscoveryError::PathFoundButNotOptedIn {
                reason: "no override or managed binary; a verter-lsp was found on \
                         PATH but PATH discovery was not opted into"
                    .to_string(),
                found_on_path: found.to_string(),
            });
        }
    }
    Err(DiscoveryError::NothingResolved {
        reason: "no override, no managed binary, and nothing usable on PATH".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_wins_regardless_of_other_inputs() {
        let inputs = DiscoveryInputs {
            override_path: Some("/explicit/verter-lsp"),
            managed_present: Some("/managed/verter-lsp"),
            path_opt_in: true,
            path_found: Some("/on/path/verter-lsp"),
        };
        assert_eq!(
            resolve_server(&inputs).unwrap(),
            ServerSource::Override("/explicit/verter-lsp".to_string())
        );
    }

    #[test]
    fn managed_wins_over_path_when_no_override() {
        let inputs = DiscoveryInputs {
            override_path: None,
            managed_present: Some("/managed/verter-lsp"),
            path_opt_in: true,
            path_found: Some("/on/path/verter-lsp"),
        };
        assert_eq!(
            resolve_server(&inputs).unwrap(),
            ServerSource::Managed("/managed/verter-lsp".to_string())
        );
    }

    #[test]
    fn path_used_only_when_opted_in_and_found() {
        let inputs = DiscoveryInputs {
            override_path: None,
            managed_present: None,
            path_opt_in: true,
            path_found: Some("/on/path/verter-lsp"),
        };
        assert_eq!(
            resolve_server(&inputs).unwrap(),
            ServerSource::Path("/on/path/verter-lsp".to_string())
        );
    }

    #[test]
    fn path_found_but_not_opted_in_is_a_distinct_loud_failure() {
        // F9: PATH is opt-in only. A found-but-not-opted-in PATH binary must NOT be
        // silently launched — it must fail loud with the DISTINCT reason that names
        // PATH / not-opted-in, so the host can guide the user to opt in.
        let inputs = DiscoveryInputs {
            override_path: None,
            managed_present: None,
            path_opt_in: false,
            path_found: Some("/on/path/verter-lsp"),
        };
        let err = resolve_server(&inputs).unwrap_err();
        assert!(
            matches!(err, DiscoveryError::PathFoundButNotOptedIn { .. }),
            "expected PathFoundButNotOptedIn, got {err:?}"
        );
        let reason = err.to_string().to_ascii_lowercase();
        assert!(
            reason.contains("path") && (reason.contains("opt") || reason.contains("opted")),
            "reason must mention PATH and opt-in: {reason:?}"
        );
        // It must NOT be confused with the nothing-found reason.
        assert!(
            !matches!(err, DiscoveryError::NothingResolved { .. }),
            "must be distinct from NothingResolved"
        );
    }

    #[test]
    fn nothing_resolves_is_a_distinct_loud_failure() {
        // F9: nothing on disk at all (and no PATH hit) is its OWN reason — not the
        // PATH-not-opted-in one.
        let inputs = DiscoveryInputs::default();
        let err = resolve_server(&inputs).unwrap_err();
        assert!(
            matches!(err, DiscoveryError::NothingResolved { .. }),
            "expected NothingResolved, got {err:?}"
        );
        assert!(
            !matches!(err, DiscoveryError::PathFoundButNotOptedIn { .. }),
            "must be distinct from PathFoundButNotOptedIn"
        );
    }

    #[test]
    fn opted_in_but_nothing_found_is_nothing_resolved() {
        // Opted into PATH but the host found nothing on it: there is no usable
        // binary anywhere, so this is the NothingResolved reason (not the
        // found-but-not-opted-in one — there was no find).
        let inputs = DiscoveryInputs {
            override_path: None,
            managed_present: None,
            path_opt_in: true,
            path_found: None,
        };
        let err = resolve_server(&inputs).unwrap_err();
        assert!(
            matches!(err, DiscoveryError::NothingResolved { .. }),
            "expected NothingResolved, got {err:?}"
        );
    }
}

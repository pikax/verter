//! The rendezvous advertisement: the file a relay-shim writes on startup so a
//! `verter_lsp` control client can DISCOVER it and verify it before connecting.
//!
//! The shim is spawned by the EDITOR, not by `verter_lsp`, so the control
//! client cannot know the endpoint a priori. On startup the shim writes an
//! advertisement JSON into `--control-dir`, keyed by the `--session-key` and
//! its process id, carrying the control endpoint path, a random nonce, the pid,
//! the real-tsgo path (+ a stable hash), the `--api` wire pin, and the editor
//! session generation. A client reads the advertisement, verifies the nonce on
//! `verter/hello`, and verifies the editor binding — through the session
//! eligibility decision, not this handshake step — NEVER "attach to the first
//! live shim".
//!
//! All on-disk names are sanitized to a portable ASCII subset (no
//! NTFS-illegal characters, no reserved device basenames, no trailing dot or
//! space) and built with [`std::path::Path`] joins, never string concatenation.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The advertisement file schema version (independent of the control
/// [`super::messages::PROTOCOL_VERSION`]). Bumped on a breaking change to the
/// advertisement shape below.
pub const ADVERTISEMENT_VERSION: u32 = 1;

/// The filename prefix for a shim advertisement in the control directory.
const ADVERTISEMENT_PREFIX: &str = "verter-relay-shim";
/// The advertisement filename extension.
const ADVERTISEMENT_EXT: &str = "json";

/// The rendezvous advertisement a shim publishes and a client reads + verifies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Advertisement {
    /// The advertisement schema version ([`ADVERTISEMENT_VERSION`]).
    pub advertisement_version: u32,
    /// The control-protocol version the shim speaks ([`super::messages::PROTOCOL_VERSION`]).
    pub protocol: u32,
    /// The control endpoint path (a Windows named pipe / a Unix-domain socket).
    pub endpoint: String,
    /// The rendezvous nonce — the client presents it on `verter/hello`; the
    /// server refuses a mismatch (fail closed).
    pub nonce: String,
    /// The shim process id.
    pub pid: u32,
    /// The `--session-key` this advertisement was published under.
    pub session_key: String,
    /// The real `tsgo` binary path the shim spawned.
    pub real_tsgo: String,
    /// A stable cross-process hash of [`Self::real_tsgo`] (an identity witness a
    /// client can cross-check without re-reading the binary).
    pub real_tsgo_hash: u64,
    /// The `--api` wire pin (codec fingerprint) the shim's engine targets.
    pub wire_pin: u64,
    /// The editor session generation (the rendezvous binding witness).
    pub editor_session_generation: u64,
}

impl Advertisement {
    /// Whether `nonce` matches this advertisement's nonce (constant purpose:
    /// the client confirms it read THIS shim's advertisement, not a stale one).
    #[must_use]
    pub fn verify_nonce(&self, nonce: &str) -> bool {
        self.nonce == nonce
    }

    /// The on-disk advertisement path in `control_dir` for `(session_key, pid)`.
    /// The basename is sanitized to a portable ASCII subset and joined with
    /// [`Path::join`] (never string concatenation).
    #[must_use]
    pub fn file_path(control_dir: &Path, session_key: &str, pid: u32) -> PathBuf {
        control_dir.join(advertisement_file_name(session_key, pid))
    }

    /// Serialize + write this advertisement into `control_dir`, creating the
    /// directory if needed. Returns the written path.
    pub fn write(&self, control_dir: &Path) -> Result<PathBuf, AdvertisementError> {
        std::fs::create_dir_all(control_dir).map_err(AdvertisementError::Io)?;
        let path = Advertisement::file_path(control_dir, &self.session_key, self.pid);
        let json = serde_json::to_vec_pretty(self).map_err(AdvertisementError::Json)?;
        std::fs::write(&path, json).map_err(AdvertisementError::Io)?;
        Ok(path)
    }

    /// Read + parse an advertisement from an explicit path.
    pub fn read_from_path(path: &Path) -> Result<Self, AdvertisementError> {
        let bytes = std::fs::read(path).map_err(AdvertisementError::Io)?;
        serde_json::from_slice(&bytes).map_err(AdvertisementError::Json)
    }

    /// Discover the NEWEST advertisement in `control_dir` published under
    /// `session_key` (the realistic client path — it knows the session key, not
    /// the shim pid). Returns the parsed advertisement + its path. Fails closed
    /// with [`AdvertisementError::NotFound`] when none matches.
    ///
    /// The sanitized filename prefix is a LOSSY candidate FILTER, never the
    /// identity: distinct raw session keys can sanitize to the same on-disk name
    /// (`a/b` and `a-b` both → `a-b`). A candidate is accepted ONLY when its parsed
    /// RAW (unsanitized) [`Self::session_key`] matches `session_key` EXACTLY AND its
    /// [`Self::advertisement_version`] / [`Self::protocol`] match this client's — so
    /// the nonce can never authenticate a COLLIDING or version-incompatible
    /// advertisement's endpoint. A malformed/unparseable candidate is skipped (it
    /// never fails the whole search); when nothing verifies, discovery fails closed.
    pub fn find_for_session_key(
        control_dir: &Path,
        session_key: &str,
    ) -> Result<(PathBuf, Self), AdvertisementError> {
        let prefix = format!(
            "{ADVERTISEMENT_PREFIX}-{}-",
            sanitize_component(session_key)
        );
        let ext = std::ffi::OsStr::new(ADVERTISEMENT_EXT);
        let mut newest: Option<(std::time::SystemTime, PathBuf, Advertisement)> = None;
        let entries = std::fs::read_dir(control_dir).map_err(AdvertisementError::Io)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension() != Some(ext) {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.starts_with(&prefix) {
                continue;
            }
            // Parse the candidate and VERIFY the raw session key + versions — the
            // sanitized filename cannot distinguish `a/b` from `a-b`. A malformed
            // candidate is skipped, not fatal.
            let Ok(adv) = Advertisement::read_from_path(&path) else {
                continue;
            };
            if adv.session_key != session_key
                || adv.advertisement_version != ADVERTISEMENT_VERSION
                || adv.protocol != super::messages::PROTOCOL_VERSION
            {
                continue;
            }
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            if newest.as_ref().is_none_or(|(t, _, _)| mtime >= *t) {
                newest = Some((mtime, path, adv));
            }
        }
        let (_, path, adv) = newest.ok_or_else(|| {
            AdvertisementError::NotFound(format!(
                "no advertisement for session key {session_key:?} in {}",
                control_dir.display()
            ))
        })?;
        Ok((path, adv))
    }
}

/// Remove a written advertisement file (best-effort). The advertisement is an
/// IPC rendezvous artifact on the REAL OS filesystem, so the shim removes it on
/// teardown to keep the control directory from accumulating stale
/// advertisements. Colocated with [`Advertisement::write`] /
/// [`Advertisement::read_from_path`] so the whole real-FS advertisement
/// lifecycle (create / write / read / discover / remove) lives in ONE module
/// — the shim binary never touches `std::fs` directly.
pub fn remove_advertisement(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// The sanitized advertisement file name for `(session_key, pid)`.
#[must_use]
pub fn advertisement_file_name(session_key: &str, pid: u32) -> String {
    format!(
        "{ADVERTISEMENT_PREFIX}-{}-{pid}.{ADVERTISEMENT_EXT}",
        sanitize_component(session_key)
    )
}

/// Sanitize a string for use as a single on-disk path component / IPC name
/// segment: keep ASCII alphanumerics, `.`, `_`, `-`; replace every other byte
/// (path separators, NTFS-illegal `< > : " | ? *`, control chars, whitespace)
/// with `-`; collapse to a bounded length; and guard against an empty result, a
/// trailing dot/space, or a reserved Windows device basename.
#[must_use]
pub fn sanitize_component(raw: &str) -> String {
    // Bound the length so a long session key (e.g. a workspace path) never
    // blows past filesystem/pipe name limits.
    const MAX_LEN: usize = 96;

    let mut out = String::with_capacity(raw.len().min(MAX_LEN));
    for ch in raw.chars() {
        let keep = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-');
        out.push(if keep { ch } else { '-' });
        if out.len() >= MAX_LEN {
            break;
        }
    }
    // No trailing dot or space (illegal as an NTFS name tail).
    while matches!(out.chars().last(), Some('.') | Some(' ')) {
        out.pop();
    }
    if out.is_empty() {
        return "x".to_string();
    }
    // Guard the reserved Windows device basenames (case-insensitive), with or
    // without an extension: prefix so the stem is never exactly reserved.
    const RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    let stem = out.split('.').next().unwrap_or(&out).to_ascii_lowercase();
    if RESERVED.contains(&stem.as_str()) {
        return format!("x-{out}");
    }
    out
}

/// A stable, cross-process hash of a string (FNV-1a, 64-bit). Deterministic
/// regardless of process or platform — unlike `std`'s randomized
/// `DefaultHasher` — so a client can cross-check the shim's `real_tsgo_hash`.
#[must_use]
pub fn stable_hash_str(s: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV_OFFSET;
    for byte in s.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Errors reading, writing, or discovering a rendezvous advertisement.
#[derive(Debug)]
pub enum AdvertisementError {
    /// An I/O failure reading/writing the advertisement file.
    Io(std::io::Error),
    /// A (de)serialization failure of the advertisement JSON.
    Json(serde_json::Error),
    /// No advertisement matched the requested session key (fail closed).
    NotFound(String),
}

impl std::fmt::Display for AdvertisementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdvertisementError::Io(e) => write!(f, "advertisement I/O error: {e}"),
            AdvertisementError::Json(e) => write!(f, "advertisement JSON error: {e}"),
            AdvertisementError::NotFound(m) => write!(f, "advertisement not found: {m}"),
        }
    }
}

impl std::error::Error for AdvertisementError {}

#[cfg(test)]
#[path = "advertisement_tests.rs"]
mod tests;

//! Tsconfig-virtualization identity model.
//!
//! When a configured project's `include`/`files` do NOT enumerate a carrier's
//! companion surface (the `Virtualize` discovery mode), the tsconfig is served
//! to the external engine with the companion paths injected. That virtualized
//! config is a DIFFERENT Program input than the user's real config, so it needs
//! a DISTINCT project identity: it must never alias the non-virtualized
//! config's identity in any cache slot (two engines sharing one slot is the
//! divergence-is-the-bug class).
//!
//! This module owns the IDENTITY (a `verter_workspace` policy concern). The
//! overlay MATERIALIZATION (the augmented bytes) lives in the `verter_lsp`
//! integration; the `verter_tsgo_api` overlay seam stays policy-free.
//!
//! The identity composes three observable inputs:
//!
//! - the user tsconfig's content hash,
//! - the union of its extends-ancestor content hashes (walked via
//!   [`resolve_tsconfig_extends`]), and
//! - the owned-companion-path SET hash (order-independent).
//!
//! Invalidation follows directly: the identity advances on a user-config edit,
//! an extends-ancestor edit, or a companion-set change (a carrier
//! added/removed/renamed). A pure carrier-TEXT edit changes none of those, so
//! the identity is stable across it (carrier content invalidation is a separate
//! content-hash rail).
//!
//! A dedicated salt keeps the virtual identity disjoint from the
//! `project_identity` dimension regardless of inputs, so the no-aliasing
//! invariant holds structurally.
//!
//! [`resolve_tsconfig_extends`]: crate::config::resolve_tsconfig_extends

use verter_scheduler::invalidation::Hash16;
use xxhash_rust::xxh3::xxh3_128;

use crate::config::{resolve_tsconfig_extends, strip_json_comments};
use crate::resolver::parent_dir;
use crate::traits::WorkspaceRead;

/// Salt for the virtual-config identity. Distinct from the five env-hash salts
/// (and `project_identity`'s) so a virtualized config can never collide with
/// the non-virtualized `project_identity` for the same project, whatever the
/// inputs.
const SALT_VIRTUAL_PROJECT_IDENTITY: &[u8] = b"verter-env:virtual-project-identity";

/// Maximum extends-chain depth walked when gathering ancestor content hashes.
/// Mirrors the config loader's cap so the two stay consistent.
const MAX_EXTENDS_DEPTH: u8 = 8;

const SEP: u8 = 0u8;

/// The composed identity of a virtualized tsconfig.
///
/// Carries the three composing fingerprints (for diagnostics / provenance) and
/// the final derived [`Hash16`]. The integration layer wraps [`to_hash16`] into
/// its `ProjectIdentity` newtype.
///
/// [`to_hash16`]: VirtualConfigIdentity::to_hash16
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualConfigIdentity {
    /// Hash of the user tsconfig's own bytes.
    user_config_hash: Hash16,
    /// Hash of the ordered extends-ancestor content sequence.
    extends_ancestors_hash: Hash16,
    /// Hash of the owned-companion-path SET (order-independent).
    companion_set_hash: Hash16,
    /// The final composed identity.
    composed: Hash16,
}

impl VirtualConfigIdentity {
    /// The final composed virtual-config identity.
    #[must_use]
    pub fn to_hash16(&self) -> Hash16 {
        self.composed
    }

    /// The user-config content fingerprint component.
    #[must_use]
    pub fn user_config_hash(&self) -> Hash16 {
        self.user_config_hash
    }

    /// The extends-ancestor content fingerprint component.
    #[must_use]
    pub fn extends_ancestors_hash(&self) -> Hash16 {
        self.extends_ancestors_hash
    }

    /// The owned-companion-path-set fingerprint component.
    #[must_use]
    pub fn companion_set_hash(&self) -> Hash16 {
        self.companion_set_hash
    }
}

/// Compute the virtual-config identity for `tsconfig_path` under `ws`, given the
/// owned-companion-path set that will be injected.
///
/// `companion_paths` is the set of companion surfaces (`Foo.vue.tsx`, …) the
/// virtualization will inject into the config. The function reads the user
/// tsconfig and its extends ancestors through `ws` (the VFS authority), so a
/// content edit to any contributing config is observed.
pub fn compute_virtual_config_identity(
    ws: &dyn WorkspaceRead,
    tsconfig_path: &str,
    companion_paths: &[String],
) -> VirtualConfigIdentity {
    // 1. The user tsconfig's own bytes. A missing config hashes its empty
    //    content (still distinct from any present config via framing).
    let user_content = ws.read_file(tsconfig_path).map(|c| c.to_string());
    let user_config_hash = hash_optional_content(user_content.as_deref());

    // 2. The ordered extends-ancestor content sequence. Walk the `extends`
    //    chain via the same resolver the config loader uses; fold each
    //    ancestor's RAW bytes in walk order so a content edit to ANY ancestor
    //    advances the hash.
    let extends_ancestors_hash = hash_extends_ancestors(ws, tsconfig_path, user_content.as_deref());

    // 3. The owned-companion-path SET. Sort + dedup so the hash is over a SET
    //    (companion enumeration order must not change the identity).
    let companion_set_hash = hash_companion_set(companion_paths);

    // 4. Compose. A dedicated salt keeps this disjoint from `project_identity`.
    let mut buf: Vec<u8> = Vec::with_capacity(64);
    buf.extend_from_slice(SALT_VIRTUAL_PROJECT_IDENTITY);
    buf.push(SEP);
    buf.extend_from_slice(&user_config_hash);
    buf.push(SEP);
    buf.extend_from_slice(&extends_ancestors_hash);
    buf.push(SEP);
    buf.extend_from_slice(&companion_set_hash);
    let composed = compute_hash16(&buf);

    VirtualConfigIdentity {
        user_config_hash,
        extends_ancestors_hash,
        companion_set_hash,
        composed,
    }
}

/// Hash the ordered extends-ancestor content sequence for `tsconfig_path`.
///
/// Starts from the already-read `start_content` (the child config bytes) to
/// avoid a redundant read, then follows the `extends` field through
/// [`resolve_tsconfig_extends`] up to [`MAX_EXTENDS_DEPTH`], folding each
/// resolved ancestor's RAW bytes (and resolved path, to disambiguate two
/// ancestors with identical bytes) in walk order. A cycle / unreadable ancestor
/// terminates the walk.
fn hash_extends_ancestors(
    ws: &dyn WorkspaceRead,
    tsconfig_path: &str,
    start_content: Option<&str>,
) -> Hash16 {
    let mut buf: Vec<u8> = Vec::new();
    let mut current_path = tsconfig_path.to_string();
    let mut current_content = start_content.map(str::to_string);
    let mut seen: Vec<String> = Vec::new();

    for _ in 0..=MAX_EXTENDS_DEPTH {
        let Some(content) = current_content.as_deref() else {
            break;
        };
        let dir = parent_dir(&current_path);
        let Some(base_path) = extends_target(ws, &dir, content) else {
            break;
        };
        // Cycle guard — a config that extends an already-visited ancestor.
        if seen.iter().any(|p| p == &base_path) || base_path == current_path {
            break;
        }
        seen.push(base_path.clone());

        let base_content = ws.read_file(&base_path).map(|c| c.to_string());
        // Fold the ancestor's resolved path AND bytes: the path disambiguates
        // two distinct ancestors that happen to share content, while the bytes
        // are what a content edit changes.
        buf.extend_from_slice(base_path.as_bytes());
        buf.push(SEP);
        match &base_content {
            Some(c) => {
                buf.push(1u8);
                buf.extend_from_slice(c.as_bytes());
            }
            None => buf.push(0u8),
        }
        buf.push(SEP);

        current_path = base_path;
        current_content = base_content;
    }

    compute_hash16(&buf)
}

/// Resolve the `extends` target of a tsconfig's raw bytes, or `None` when the
/// config declares no (string) `extends` or it does not resolve.
fn extends_target(ws: &dyn WorkspaceRead, tsconfig_dir: &str, content: &str) -> Option<String> {
    let cleaned = strip_json_comments(content);
    let json: serde_json::Value = serde_json::from_str(&cleaned).ok()?;
    let extends = json.get("extends")?.as_str()?;
    resolve_tsconfig_extends(ws, tsconfig_dir, extends)
}

/// Hash a set of companion paths, order-independently (sort + dedup first).
fn hash_companion_set(companion_paths: &[String]) -> Hash16 {
    let mut sorted: Vec<&str> = companion_paths.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    let mut buf: Vec<u8> = Vec::new();
    for p in sorted {
        buf.extend_from_slice(p.as_bytes());
        buf.push(SEP);
    }
    compute_hash16(&buf)
}

/// Hash optional content with a present/absent framing byte so an empty present
/// config never collides with a missing one.
fn hash_optional_content(content: Option<&str>) -> Hash16 {
    let mut buf: Vec<u8> = Vec::new();
    match content {
        Some(c) => {
            buf.push(1u8);
            buf.extend_from_slice(c.as_bytes());
        }
        None => buf.push(0u8),
    }
    compute_hash16(&buf)
}

fn compute_hash16(bytes: &[u8]) -> Hash16 {
    xxh3_128(bytes).to_le_bytes()
}

#[cfg(test)]
#[path = "virtual_config_tests.rs"]
mod tests;

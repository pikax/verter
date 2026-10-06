//! Stable per-project identity key for ambient-lib registration and
//! ownership lookup.
//!
//! `ProjectStableKey` is an opaque identity/lookup key: consumers receive
//! it from the host (`WorkspaceAccess::project_stable_key()` or an
//! attempt-view callback), pass it back into ambient-symbol lookup, and
//! never construct it directly — construction (`from_project`) needs the
//! host's own project-payload/workspace-root snapshot types and stays
//! host-owned, minting values through a free function that returns this
//! type. `to_hex_tag`/`parse_hex_tag` are pure value operations and stay
//! with the type.

use crate::analysis::types::Hash16;
use xxhash_rust::xxh3::xxh3_128;

/// Stable identity for a project across snapshot rebuilds.
///
/// Hash inputs include the project's `workspace_root_canonical` path: two
/// workspaces both containing `tsconfig.json` produce distinct keys
/// (different `workspace_root_canonical`).
///
/// Two variants:
/// - `Configured(hash)` — derived from `workspace_root || tsconfig_path || "CONFIGURED"`.
/// - `Fallback(hash)` — derived from `workspace_root || project_root || "FALLBACK"`.
///
/// Cross-machine portability is NOT a goal — ambient libs are per-machine
/// runtime state.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum ProjectStableKey {
    Configured(Hash16),
    Fallback(Hash16),
}

impl ProjectStableKey {
    /// Render as `C<hex>` or `F<hex>` for ambient virtual canonical IDs.
    pub fn to_hex_tag(&self) -> String {
        match self {
            ProjectStableKey::Configured(h) => format!("C{}", encode_hex(h)),
            ProjectStableKey::Fallback(h) => format!("F{}", encode_hex(h)),
        }
    }

    /// Parse a hex tag (inverse of `to_hex_tag`).
    pub fn parse_hex_tag(s: &str) -> Option<Self> {
        if s.len() < 2 {
            return None;
        }
        let (tag, hex_part) = s.split_at(1);
        let hash = decode_hash16_hex(hex_part)?;
        match tag {
            "C" => Some(ProjectStableKey::Configured(hash)),
            "F" => Some(ProjectStableKey::Fallback(hash)),
            _ => None,
        }
    }
}

/// Hash a `(workspace_root, discriminating input, tag)` byte sequence into a
/// [`Hash16`]. Exposed so the host-owned `from_project`-style constructor
/// (which needs its own snapshot types, out of scope for this
/// dependency-neutral module) can build a `ProjectStableKey` using the same
/// hash function this type's own round-trip (`to_hex_tag`/`parse_hex_tag`)
/// tests against.
pub fn compute_hash16(bytes: &[u8]) -> Hash16 {
    xxh3_128(bytes).to_le_bytes()
}

fn encode_hex(h: &Hash16) -> String {
    let mut out = String::with_capacity(h.len() * 2);
    for byte in h {
        out.push(nibble_to_hex(byte >> 4));
        out.push(nibble_to_hex(byte & 0x0f));
    }
    out
}

fn decode_hash16_hex(s: &str) -> Option<Hash16> {
    if s.len() != 32 {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = [0u8; 16];
    for (i, slot) in out.iter_mut().enumerate() {
        let hi = hex_to_nibble(bytes[i * 2])?;
        let lo = hex_to_nibble(bytes[i * 2 + 1])?;
        *slot = (hi << 4) | lo;
    }
    Some(out)
}

fn nibble_to_hex(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + (n - 10)) as char,
        _ => '?',
    }
}

fn hex_to_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + b - b'a'),
        b'A'..=b'F' => Some(10 + b - b'A'),
        _ => None,
    }
}

#[cfg(test)]
#[path = "project_stable_key_tests.rs"]
mod tests;

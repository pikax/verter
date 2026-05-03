//! Stable per-project identity key for ambient-lib registration.
//!
//! (sub-plan A3): `ProjectStableKey` includes a workspace-root
//! discriminator to prevent multi-root collisions. Two workspaces both
//! containing `tsconfig.json` produce distinct keys (different
//! workspace_root_canonical paths).
//!
//! Cross-machine portability is NOT a goal — ambient libs are per-machine
//! runtime state.

use verter_scheduler::invalidation::Hash16;
use xxhash_rust::xxh3::xxh3_128;

use crate::canonical_path::CanonicalPath;
use crate::workspace_snapshot::{OwnershipProject, ProjectPayload};

/// Stable identity for a project across snapshot rebuilds.
///
/// Hash inputs include the project's `workspace_root_canonical` path (A3):
/// two workspaces both containing `tsconfig.json` produce distinct keys
/// (different `workspace_root_canonical`).
///
/// Two variants:
/// - `Configured(hash)` — derived from `workspace_root || tsconfig_path || "CONFIGURED"`.
/// - `Fallback(hash)` — derived from `workspace_root || project_root || "FALLBACK"`.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum ProjectStableKey {
    Configured(Hash16),
    Fallback(Hash16),
}

impl ProjectStableKey {
    /// Build a stable key from a project + the workspace root that owns it.
    pub fn from_project(p: &OwnershipProject, workspace_root: &CanonicalPath) -> Self {
        let mut input: Vec<u8> = Vec::new();
        input.extend_from_slice(workspace_root.as_str().as_bytes());
        input.push(0u8); // separator
        match &p.payload {
            ProjectPayload::Configured { tsconfig_path, .. } => {
                input.extend_from_slice(tsconfig_path.as_str().as_bytes());
                input.push(0u8);
                input.extend_from_slice(b"CONFIGURED");
                ProjectStableKey::Configured(compute_hash16(&input))
            }
            ProjectPayload::Fallback { .. } => {
                input.extend_from_slice(p.root.as_str().as_bytes());
                input.push(0u8);
                input.extend_from_slice(b"FALLBACK");
                ProjectStableKey::Fallback(compute_hash16(&input))
            }
        }
    }

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

fn compute_hash16(bytes: &[u8]) -> Hash16 {
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
mod tests {
    use rustc_hash::FxHashSet;

    use super::*;
    use crate::membership::{ConfiguredMembership, FallbackMembership, StaticMembershipSpec};
    use crate::resolver::IdeProjectCompilerOptions;
    use crate::workspace_snapshot::ProjectId;

    fn make_configured(workspace_root: &str, tsconfig_path: &str) -> OwnershipProject {
        OwnershipProject {
            id: ProjectId(0),
            root: CanonicalPath::new(workspace_root),
            workspace_root: CanonicalPath::new(workspace_root),
            payload: ProjectPayload::Configured {
                tsconfig_path: CanonicalPath::new(tsconfig_path),
                membership: ConfiguredMembership {
                    spec: StaticMembershipSpec {
                        files: Vec::new(),
                        include: Vec::new(),
                        exclude: Vec::new(),
                    },
                    materialized_files: FxHashSet::default(),
                },
                compiler_options: IdeProjectCompilerOptions::default(),
                references: Vec::new(),
                workspace_aliases: Vec::new(),
            },
        }
    }

    fn make_fallback(workspace_root: &str, project_root: &str) -> OwnershipProject {
        OwnershipProject {
            id: ProjectId(0),
            root: CanonicalPath::new(project_root),
            workspace_root: CanonicalPath::new(workspace_root),
            payload: ProjectPayload::Fallback {
                membership: FallbackMembership {
                    root: CanonicalPath::new(project_root),
                    exclude: Vec::new(),
                },
            },
        }
    }

    #[test]
    fn configured_key_distinguishes_sibling_tsconfigs() {
        let workspace_root = CanonicalPath::new("/ws");
        let app = make_configured("/ws", "/ws/tsconfig.app.json");
        let test = make_configured("/ws", "/ws/tsconfig.vitest.json");
        let app_key = ProjectStableKey::from_project(&app, &workspace_root);
        let test_key = ProjectStableKey::from_project(&test, &workspace_root);
        assert_ne!(
            app_key, test_key,
            "sibling tsconfigs MUST produce distinct stable keys"
        );
    }

    #[test]
    fn configured_key_distinguishes_multi_root_workspaces() {
        let ws_a = CanonicalPath::new("/a");
        let ws_b = CanonicalPath::new("/b");
        let proj_a = make_configured("/a", "/a/tsconfig.json");
        let proj_b = make_configured("/b", "/b/tsconfig.json");
        let key_a = ProjectStableKey::from_project(&proj_a, &ws_a);
        let key_b = ProjectStableKey::from_project(&proj_b, &ws_b);
        assert_ne!(
            key_a, key_b,
            "two workspaces both containing tsconfig.json MUST produce distinct keys (A3)"
        );
    }

    #[test]
    fn configured_and_fallback_at_same_path_differ() {
        let workspace_root = CanonicalPath::new("/ws");
        let configured = make_configured("/ws", "/ws/tsconfig.json");
        let fallback = make_fallback("/ws", "/ws");
        let c_key = ProjectStableKey::from_project(&configured, &workspace_root);
        let f_key = ProjectStableKey::from_project(&fallback, &workspace_root);
        // Different variants alone differ; payload separators differ too.
        assert!(matches!(c_key, ProjectStableKey::Configured(_)));
        assert!(matches!(f_key, ProjectStableKey::Fallback(_)));
        // And their hashes should NOT collide.
        let c_hex = c_key.to_hex_tag();
        let f_hex = f_key.to_hex_tag();
        assert_ne!(
            c_hex, f_hex,
            "Configured and Fallback at same path MUST NOT produce same hex tag"
        );
    }

    #[test]
    fn hex_tag_round_trips() {
        let workspace_root = CanonicalPath::new("/ws");
        let proj = make_configured("/ws", "/ws/tsconfig.json");
        let key = ProjectStableKey::from_project(&proj, &workspace_root);
        let hex = key.to_hex_tag();
        let parsed = ProjectStableKey::parse_hex_tag(&hex).expect("hex tag MUST parse back");
        assert_eq!(key, parsed);
    }

    #[test]
    fn hex_tag_format_is_one_letter_prefix_plus_32_hex_chars() {
        let workspace_root = CanonicalPath::new("/ws");
        let configured = make_configured("/ws", "/ws/tsconfig.json");
        let fallback = make_fallback("/ws", "/ws");
        let c_hex = ProjectStableKey::from_project(&configured, &workspace_root).to_hex_tag();
        let f_hex = ProjectStableKey::from_project(&fallback, &workspace_root).to_hex_tag();
        assert_eq!(c_hex.len(), 33, "C + 32 hex chars");
        assert_eq!(f_hex.len(), 33, "F + 32 hex chars");
        assert!(c_hex.starts_with('C'));
        assert!(f_hex.starts_with('F'));
    }

    #[test]
    fn parse_hex_tag_rejects_invalid_inputs() {
        // Wrong prefix.
        assert!(ProjectStableKey::parse_hex_tag("Xabcdef0123456789abcdef0123456789").is_none());
        // Too short.
        assert!(ProjectStableKey::parse_hex_tag("Cabc").is_none());
        // Non-hex chars.
        assert!(
            ProjectStableKey::parse_hex_tag("Cgg23456789abcdef0123456789abcdef").is_none(),
            "non-hex chars MUST fail to parse"
        );
        // Wrong length (31 hex chars instead of 32).
        assert!(
            ProjectStableKey::parse_hex_tag("Cabc23456789abcdef0123456789abcd").is_none(),
            "31 hex chars MUST fail"
        );
        // Wrong length (33 hex chars instead of 32).
        assert!(
            ProjectStableKey::parse_hex_tag("Cabc23456789abcdef0123456789abcdef0").is_none(),
            "33 hex chars MUST fail"
        );
    }

    #[test]
    fn key_is_deterministic_across_calls() {
        let workspace_root = CanonicalPath::new("/ws");
        let proj_1 = make_configured("/ws", "/ws/tsconfig.json");
        let proj_2 = make_configured("/ws", "/ws/tsconfig.json");
        assert_eq!(
            ProjectStableKey::from_project(&proj_1, &workspace_root),
            ProjectStableKey::from_project(&proj_2, &workspace_root),
            "same inputs MUST produce same key"
        );
    }
}

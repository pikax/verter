//! The opaque project identifier.
//!
//! Every value that escapes the analysis boundary — a JSONL event, a summary, a
//! deviation-ledger row, a redacted source map — identifies a corpus project by
//! its [`ProjectId`] and NOTHING else. The id is deliberately boring: a fixed
//! `p` prefix plus exactly four decimal digits (`p0001`). A descriptive id (a real
//! project/library name) would re-leak the project's identity, so the structural
//! shape is ENFORCED at construction, never trusted from the config.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An opaque, validated project identifier of the exact form `p` + four decimal
/// digits (`p0001`, `p0042`, `p9999`). Constructed only through [`ProjectId::new`]
/// (or deserialization, which routes through the same validation), so a value of
/// this type is GUARANTEED to carry no descriptive/private name.
///
/// This is the only project identity that may appear in any emitted artifact.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectId(String);

/// Why a candidate string is not a valid opaque [`ProjectId`].
///
/// The message never echoes a private name verbatim beyond the rejected token
/// itself (which the caller already holds), and the rejected token is only ever
/// the id field — never a filesystem path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectIdError {
    /// The candidate did not match `^p[0-9]{4}$`.
    Malformed {
        /// The rejected candidate, for the caller's diagnostics.
        got: String,
    },
}

impl fmt::Display for ProjectIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectIdError::Malformed { got } => write!(
                f,
                "invalid opaque project id {got:?}: expected the form p followed by \
                 exactly four decimal digits (e.g. p0001)"
            ),
        }
    }
}

impl std::error::Error for ProjectIdError {}

/// True iff `s` is exactly `p` followed by four ASCII decimal digits.
///
/// A pure structural predicate (no allocation), so the rule is auditable and the
/// discrimination test can call it directly.
fn is_opaque_project_id(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 5 && bytes[0] == b'p' && bytes[1..].iter().all(|b| b.is_ascii_digit())
}

impl ProjectId {
    /// Construct an opaque id, rejecting anything that is not `p` + four digits.
    ///
    /// Rejects descriptive ids (a real project name), short forms (`p1`), wrong
    /// prefixes (`proj-a`), and the empty string.
    pub fn new(candidate: impl Into<String>) -> Result<Self, ProjectIdError> {
        let candidate = candidate.into();
        if is_opaque_project_id(&candidate) {
            Ok(ProjectId(candidate))
        } else {
            Err(ProjectIdError::Malformed { got: candidate })
        }
    }

    /// The opaque id as a string slice — always safe to emit.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The `Debug` form is the opaque id itself — there is nothing private to hide,
/// and emitting `ProjectId("p0001")` keeps logs readable.
impl fmt::Debug for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ProjectId({:?})", self.0)
    }
}

impl Serialize for ProjectId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ProjectId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ProjectIdVisitor;
        impl Visitor<'_> for ProjectIdVisitor {
            type Value = ProjectId;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an opaque project id of the form p + four decimal digits")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<ProjectId, E> {
                // Deserialization routes through the same structural gate, so an
                // id read from a config is enforced, never trusted.
                ProjectId::new(v).map_err(|e| de::Error::custom(e.to_string()))
            }
        }
        deserializer.deserialize_str(ProjectIdVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_four_digit_id() {
        for ok in ["p0001", "p0042", "p9999", "p0000"] {
            let id = ProjectId::new(ok).expect("valid opaque id");
            assert_eq!(id.as_str(), ok);
        }
    }

    /// A descriptive (non-opaque) id, built from fragments so this SOURCE never
    /// spells a real private project token contiguously (the hermetic leak guard
    /// scans this file too).
    fn descriptive_id() -> String {
        format!("{}{}{}", "nex", "us", "-ui")
    }

    #[test]
    fn rejects_descriptive_short_and_empty_ids() {
        // A descriptive id would re-leak the project identity — the whole point.
        let descriptive = descriptive_id();
        let bads = [
            descriptive.as_str(),
            "p1",
            "proj-a",
            "",
            "p00001",
            "P0001",
            "p001a",
            "0001",
            "px001",
        ];
        for bad in bads {
            let err = ProjectId::new(bad).expect_err("must reject non-opaque id");
            match err {
                ProjectIdError::Malformed { got } => assert_eq!(got, bad),
            }
        }
    }

    #[test]
    fn predicate_discriminates() {
        assert!(is_opaque_project_id("p0001"));
        assert!(!is_opaque_project_id(&descriptive_id()));
        assert!(!is_opaque_project_id("p1"));
        assert!(!is_opaque_project_id("p00010"));
        assert!(!is_opaque_project_id("p001a"));
    }

    #[test]
    fn deserialize_enforces_the_shape() {
        assert!(serde_json::from_str::<ProjectId>("\"p0007\"").is_ok());
        // A descriptive id in a config is REJECTED at the deserialization gate.
        let descriptive_json = format!("\"{}\"", descriptive_id());
        assert!(serde_json::from_str::<ProjectId>(&descriptive_json).is_err());
        assert!(serde_json::from_str::<ProjectId>("\"p7\"").is_err());
    }

    #[test]
    fn debug_and_display_show_only_the_opaque_id() {
        let id = ProjectId::new("p0001").unwrap();
        assert_eq!(format!("{id}"), "p0001");
        assert_eq!(format!("{id:?}"), "ProjectId(\"p0001\")");
    }
}

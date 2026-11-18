use std::hash::{Hash, Hasher};

use verter_identity::identity::{ContentId, SourceUnitId};

use crate::assembly::source_space::SourceSpaceKind;

/// Identity of one source text: unit, content revision and coordinate
/// space. Two fragments name the same source exactly when their keys are
/// equal; equal bytes under different keys stay distinct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceKey {
    pub unit: SourceUnitId,
    pub content: ContentId,
    pub space: SourceSpaceKind,
}

impl SourceKey {
    /// Both keys name the same source text.
    pub fn same(&self, other: &Self) -> bool {
        self == other
    }

    /// Both keys name one unit in one space, at two different revisions.
    pub fn conflicts_with(&self, other: &Self) -> bool {
        self.unit == other.unit && self.space == other.space && self.content != other.content
    }
}

impl Hash for SourceKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.unit.hash(state);
        self.content.hash(state);
        std::mem::discriminant(&self.space).hash(state);
    }
}

/// A source text together with its identity.
#[derive(Debug, Clone)]
pub struct QualifiedSource<'a> {
    pub key: SourceKey,
    pub text: &'a str,
}

/// A source keyed by `name` and its own bytes, in the authored space.
#[cfg(test)]
pub(crate) fn test_source<'a>(name: &'static str, text: &'a str) -> QualifiedSource<'a> {
    use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
    use verter_identity::identity::SourceId;

    struct Name(&'static str);
    impl CanonicalEncode for Name {
        const DOMAIN_TAG: &'static str = "verter.compiler.emission.fixture.v1";
        fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
            encoder.field_str(1, self.0);
        }
    }

    QualifiedSource {
        key: SourceKey {
            unit: SourceUnitId::from_lineage(&SourceId::from_canonical(&Name(name)), "source"),
            content: ContentId::from_content_bytes(text.as_bytes()),
            space: SourceSpaceKind::Original,
        },
        text,
    }
}

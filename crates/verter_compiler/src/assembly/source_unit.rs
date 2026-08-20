//! Logical source-unit identity: a stable carrier-unit lineage plus its
//! exact revision and content, minted through `verter_identity`'s digest
//! types rather than a locally-invented parallel identity.

use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
pub use verter_identity::identity::{ContentId, SourceId, SourceRevision, SourceUnitId};

/// One caller-supplied descriptor: which logical carrier block, inside
/// which source, at which revision, with which exact bytes. Two calls with
/// the same four fields mint the same [`SourceUnitId`] — the identity is a
/// pure function of them, never a counter.
pub struct SourceUnitDescriptor<'a> {
    pub source_id: &'a SourceId,
    pub revision: &'a SourceRevision,
    /// Stable logical role inside the source (`"script"`, `"template"`,
    /// `"style:0"`, …) — distinguishes multiple units minted from the same
    /// `(source_id, revision)`.
    pub logical_role: &'a str,
    pub content: &'a ContentId,
}

impl CanonicalEncode for SourceUnitDescriptor<'_> {
    const DOMAIN_TAG: &'static str = "verter.compiler.assembly.source_unit.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_bytes(1, self.source_id.digest().as_bytes());
        encoder.field_bytes(2, self.revision.digest().as_bytes());
        encoder.field_str(3, self.logical_role);
        encoder.field_bytes(4, self.content.digest().as_bytes());
    }
}

/// A logical source unit: stable lineage, exact revision, exact content —
/// the identity every [`super::fragment::Fragment`] is minted against.
#[derive(Debug, Clone)]
pub struct SourceUnit {
    id: SourceUnitId,
    source_id: SourceId,
    revision: SourceRevision,
    content: ContentId,
    logical_role: String,
}

impl SourceUnit {
    /// Mint a source unit from its four identity-bearing fields. `content`
    /// is the exact-bytes identity of this unit's authored content — the
    /// caller derives it from the same bytes it will hand the fragment
    /// producer, never a placeholder.
    pub fn mint(
        source_id: SourceId,
        revision: SourceRevision,
        logical_role: impl Into<String>,
        content: ContentId,
    ) -> Self {
        let logical_role = logical_role.into();
        let id = SourceUnitId::from_canonical(&SourceUnitDescriptor {
            source_id: &source_id,
            revision: &revision,
            logical_role: &logical_role,
            content: &content,
        });
        Self {
            id,
            source_id,
            revision,
            content,
            logical_role,
        }
    }

    pub fn id(&self) -> &SourceUnitId {
        &self.id
    }

    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    pub fn revision(&self) -> &SourceRevision {
        &self.revision
    }

    pub fn content(&self) -> &ContentId {
        &self.content
    }

    pub fn logical_role(&self) -> &str {
        &self.logical_role
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_id() -> SourceId {
        SourceId::from_canonical(&Tag("Comp.vue"))
    }
    fn revision() -> SourceRevision {
        SourceRevision::from_canonical(&Tag("rev-1"))
    }

    struct Tag(&'static str);
    impl CanonicalEncode for Tag {
        const DOMAIN_TAG: &'static str = "verter.compiler.assembly.source_unit.test.tag.v1";
        fn encode_fields(&self, e: &mut CanonicalEncoder) {
            e.field_str(1, self.0);
        }
    }

    #[test]
    fn same_fields_mint_the_same_id() {
        let content = ContentId::from_content_bytes(b"<template/>");
        let a = SourceUnit::mint(source_id(), revision(), "template", content.clone());
        let b = SourceUnit::mint(source_id(), revision(), "template", content);
        assert_eq!(a.id(), b.id());
    }

    #[test]
    fn different_logical_role_mints_a_different_id() {
        let content = ContentId::from_content_bytes(b"same bytes");
        let script = SourceUnit::mint(source_id(), revision(), "script", content.clone());
        let template = SourceUnit::mint(source_id(), revision(), "template", content);
        assert_ne!(
            script.id(),
            template.id(),
            "two logical units minted from the same source/revision/content \
             must not collide just because their bytes coincide"
        );
    }

    #[test]
    fn different_content_mints_a_different_id() {
        let a = SourceUnit::mint(
            source_id(),
            revision(),
            "script",
            ContentId::from_content_bytes(b"const a = 1"),
        );
        let b = SourceUnit::mint(
            source_id(),
            revision(),
            "script",
            ContentId::from_content_bytes(b"const a = 2"),
        );
        assert_ne!(a.id(), b.id());
    }
}

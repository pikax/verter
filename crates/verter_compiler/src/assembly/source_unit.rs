//! Logical source-unit identity: a stable carrier-unit lineage plus its
//! exact revision and content, minted through `verter_identity`'s digest
//! types rather than a locally-invented parallel identity.

use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
pub use verter_identity::identity::{ContentId, SourceId, SourceRevision, SourceUnitId};

/// A logical source unit: stable lineage, exact revision, exact content —
/// the identity every [`super::fragment::Fragment`] is minted against.
///
/// [`SourceUnitId`] is lineage only (`SourceId` + logical role). Revision
/// and content are stored as neighbouring facts and never enter that id.
#[derive(Debug, Clone)]
pub struct SourceUnit {
    id: SourceUnitId,
    source_id: SourceId,
    revision: SourceRevision,
    content: ContentId,
    logical_role: String,
}

impl SourceUnit {
    /// Mint a source unit from its lineage plus neighbouring revision and
    /// content facts. `content` is the exact-bytes identity of this unit's
    /// authored content — the caller derives it from the same bytes it will
    /// hand the fragment producer, never a placeholder.
    pub fn mint(
        source_id: SourceId,
        revision: SourceRevision,
        logical_role: impl Into<String>,
        content: ContentId,
    ) -> Self {
        let logical_role = logical_role.into();
        let id = SourceUnitId::from_lineage(&source_id, &logical_role);
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

/// Lineage identity for a compiler fragment: logical source (from its
/// canonical id) plus the unit's role. Revision and content stay off this
/// constructor — hashing a path string into [`SourceUnitId`] is forbidden.
pub(crate) fn source_unit_id(canonical_id: &str, logical_role: &str) -> SourceUnitId {
    struct CanonicalSource<'a>(&'a str);
    impl CanonicalEncode for CanonicalSource<'_> {
        const DOMAIN_TAG: &'static str = "verter.compiler.assembly.logical_source.v1";
        fn encode_fields(&self, e: &mut CanonicalEncoder) {
            e.field_str(1, self.0);
        }
    }
    SourceUnitId::from_lineage(
        &SourceId::from_canonical(&CanonicalSource(canonical_id)),
        logical_role,
    )
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
    fn content_and_revision_do_not_change_source_unit_id() {
        let content_a = ContentId::from_content_bytes(b"const a = 1");
        let content_b = ContentId::from_content_bytes(b"const a = 2");
        let a = SourceUnit::mint(source_id(), revision(), "script", content_a.clone());
        let b = SourceUnit::mint(source_id(), revision(), "script", content_b);
        assert_eq!(
            a.id(),
            b.id(),
            "same source and role keep the same unit identity when content changes"
        );
        assert_ne!(
            a.content(),
            b.content(),
            "distinct contents must not alias onto one ContentId"
        );

        let other_revision = SourceRevision::from_canonical(&Tag("rev-2"));
        let c = SourceUnit::mint(source_id(), other_revision, "script", content_a);
        assert_eq!(
            a.id(),
            c.id(),
            "same source and role keep the same unit identity when revision changes"
        );
        assert_ne!(
            a.revision(),
            c.revision(),
            "distinct revisions must not alias onto one SourceRevision"
        );
    }
}

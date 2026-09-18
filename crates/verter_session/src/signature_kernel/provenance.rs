//! Origin metadata: declaration-group/parent, source and overload ordinals,
//! origin relationships, and cached effective overload order.
//!
//! Composite constituent sequences retain arm identity and repeated
//! contributors. They are interned as one sequence, not a per-edge `Vec`
//! on the hot read path.

use super::records::{SignatureDescriptorId, SignatureProvenanceId};

/// Declaration-group identity (overload group, merged interface group, …).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct DeclarationGroupId(u32);

impl DeclarationGroupId {
    #[must_use]
    pub const fn from_raw(id: u32) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

/// Declaration-parent identity (containing class/module/namespace).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct DeclarationParentId(u32);

impl DeclarationParentId {
    #[must_use]
    pub const fn from_raw(id: u32) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

/// Stable source locator handle (not a recycled arena offset).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct SourceLocatorId(u32);

impl SourceLocatorId {
    #[must_use]
    pub const fn from_raw(id: u32) -> Self {
        Self(id)
    }
}

/// Cached effective overload order. Reading it does not walk provenance.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct OverloadOrder {
    pub group: DeclarationGroupId,
    pub ordinal: u32,
}

/// Origin relationship through instantiation and synthesis.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum OriginRelation {
    Authored,
    Synthesized { from: SignatureProvenanceId },
    Instantiated { from: SignatureProvenanceId },
}

/// Provenance record. Semantic binder mappings live on descriptors, not here.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SignatureProvenance {
    pub declaration_group: DeclarationGroupId,
    pub declaration_parent: DeclarationParentId,
    pub source_ordinal: u32,
    pub overload_ordinal: u32,
    pub source_locator: SourceLocatorId,
    pub origin: OriginRelation,
    pub effective_overload_order: OverloadOrder,
}

impl SignatureProvenance {
    #[must_use]
    pub fn authored(
        group: DeclarationGroupId,
        parent: DeclarationParentId,
        source_ordinal: u32,
        overload_ordinal: u32,
    ) -> Self {
        Self {
            declaration_group: group,
            declaration_parent: parent,
            source_ordinal,
            overload_ordinal,
            source_locator: SourceLocatorId::from_raw(0),
            origin: OriginRelation::Authored,
            effective_overload_order: OverloadOrder {
                group,
                ordinal: overload_ordinal,
            },
        }
    }
}

/// Arm identity inside a composite sequence. Repeated contributors stay.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ArmIdentity {
    pub ordinal: u32,
    pub contributor: SignatureDescriptorId,
}

/// One mapped constituent edge: declaration → residual (explicit, not a set id).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct MappedConstituent {
    pub arm: ArmIdentity,
    pub declaration: SignatureDescriptorId,
    pub residual: SignatureDescriptorId,
}

/// Interned constituent sequence. Built once; prefixes are not allocated.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ConstituentSequence {
    pub edges: Box<[MappedConstituent]>,
}

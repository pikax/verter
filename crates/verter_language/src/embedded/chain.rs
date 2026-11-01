//! Composition of independently identified map stages.

use std::sync::Arc;

use verter_identity::identity::{ContentId, SourceUnitId};
use verter_identity::mapping::MapRevision;
use verter_span::SourceByteRange;

use super::lineage::EmbeddedAttachment;
use super::map::{
    AuthoredOrigin, AuthoredProvenance, CoordinateDomain, DomainOffset, DomainRange, MapStage,
    OriginRef, StageKind, Unmappable,
};

/// The exact basis of one map endpoint: unit, domain, content and map
/// revision. A position is valid only against the basis it was computed on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MapBasis {
    pub unit: SourceUnitId,
    pub domain: CoordinateDomain,
    pub content: ContentId,
    pub map_revision: MapRevision,
}

impl MapBasis {
    /// Refuse anything but the exact expected basis: same unit and domain with
    /// another content or map revision is stale, anything else mismatched.
    pub(super) fn require(&self, actual: &MapBasis, stage: StageKind) -> Result<(), Unmappable> {
        if self == actual {
            Ok(())
        } else if self.unit == actual.unit && self.domain == actual.domain {
            Err(Unmappable::StaleBasis { stage })
        } else {
            Err(Unmappable::MismatchedBasis { stage })
        }
    }
}

/// An ordered composition of map stages from authored bytes to a final output.
///
/// Stages are shared, not flattened, so no stage retains a copy of a prefix and
/// a lookup visits each stage once. Each keeps its own kind and bases: a syntax
/// map never reads as runtime provenance, or the reverse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoredMapChain {
    attachment: EmbeddedAttachment,
    /// Never empty: `new` installs the first stage and nothing removes one.
    stages: Vec<Arc<MapStage>>,
}

impl AuthoredMapChain {
    /// Start a chain at authored bytes of the attachment's source unit.
    pub fn new(attachment: EmbeddedAttachment, first: Arc<MapStage>) -> Result<Self, Unmappable> {
        if first.input().domain != CoordinateDomain::Source
            || first.input().unit != *attachment.lineage().unit()
        {
            return Err(Unmappable::MismatchedBasis {
                stage: first.kind(),
            });
        }
        Ok(Self {
            attachment,
            stages: vec![first],
        })
    }

    /// Join a stage whose input basis exactly equals this chain's output basis.
    pub fn then(mut self, stage: Arc<MapStage>) -> Result<Self, Unmappable> {
        self.output_basis().require(stage.input(), stage.kind())?;
        self.stages.push(stage);
        Ok(self)
    }

    pub fn attachment(&self) -> &EmbeddedAttachment {
        &self.attachment
    }

    /// Stages in composition order, each with its own kind and bases.
    pub fn stages(&self) -> &[Arc<MapStage>] {
        &self.stages
    }

    pub fn output_basis(&self) -> &MapBasis {
        self.stages[self.stages.len() - 1].output()
    }

    /// Every authored source unit that contributes, root first, then in stage
    /// order. A unit appears once.
    pub fn contributing_units(&self) -> Vec<SourceUnitId> {
        let mut units = vec![self.attachment.lineage().unit().clone()];
        for stage in &self.stages {
            stage.push_foreign_units(&mut units);
        }
        units
    }

    fn require_query(&self, basis: &MapBasis) -> Result<(), Unmappable> {
        let last = &self.stages[self.stages.len() - 1];
        self.output_basis().require(basis, last.kind())
    }

    /// The authored position of the scalar starting at `offset`; at the end of
    /// the output, the authored end of the last scalar. The answer is an empty
    /// authored range.
    pub fn authored_position(
        &self,
        basis: &MapBasis,
        offset: DomainOffset,
    ) -> Result<AuthoredOrigin, Unmappable> {
        self.require_query(basis)?;
        let mut current = offset;
        for stage in self.stages.iter().rev() {
            current = stage.position(current)?;
        }
        match current {
            DomainOffset::Source(authored) => {
                Ok(self.authored_origin(SourceByteRange::new(authored, authored)))
            }
            _ => Err(self.root_mismatch()),
        }
    }

    /// Every authored origin of `range`, in output order, for display and
    /// blame. Several origins are provenance, never a contiguous span; a single
    /// exact origin is still not edit authority.
    pub fn provenance(
        &self,
        basis: &MapBasis,
        range: DomainRange,
    ) -> Result<AuthoredProvenance, Unmappable> {
        self.require_query(basis)?;
        let origins = provenance_through(&self.stages, range)?
            .into_iter()
            .map(|origin| match origin {
                OriginRef::Foreign(foreign) => Ok(foreign),
                OriginRef::Input(DomainRange::Source(range)) => Ok(self.authored_origin(range)),
                OriginRef::Input(_) => Err(self.root_mismatch()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AuthoredProvenance::new(origins))
    }

    /// The unique authored range an edit of `range` may replace, proven by
    /// every stage in turn. Mapping the endpoints alone proves nothing, so the
    /// whole range travels. The caller still spells replacement text through
    /// the profile's reverse encoding.
    pub fn editable_preimage(
        &self,
        basis: &MapBasis,
        range: DomainRange,
    ) -> Result<AuthoredOrigin, Unmappable> {
        self.require_query(basis)?;
        let mut current = range;
        for stage in self.stages.iter().rev() {
            current = stage.editable_preimage(current)?;
        }
        match current {
            DomainRange::Source(authored) => Ok(self.authored_origin(authored)),
            _ => Err(self.root_mismatch()),
        }
    }

    fn authored_origin(&self, range: SourceByteRange) -> AuthoredOrigin {
        AuthoredOrigin {
            unit: self.attachment.lineage().unit().clone(),
            base_uri: self.attachment.base_uri().clone(),
            range,
        }
    }

    fn root_mismatch(&self) -> Unmappable {
        Unmappable::MismatchedBasis {
            stage: self.stages[0].kind(),
        }
    }
}

/// Walk `range` back through `stages`, last first. The exact-basis join
/// guarantees each stage sees ranges in its own output domain.
fn provenance_through(
    stages: &[Arc<MapStage>],
    range: DomainRange,
) -> Result<Vec<OriginRef>, Unmappable> {
    let mut current = vec![OriginRef::Input(range)];
    let mut next = Vec::new();
    for stage in stages.iter().rev() {
        next.clear();
        for origin in &current {
            match origin {
                OriginRef::Foreign(_) => next.push(origin.clone()),
                OriginRef::Input(input) => stage.provenance(*input, &mut next)?,
            }
        }
        std::mem::swap(&mut current, &mut next);
    }
    Ok(current)
}

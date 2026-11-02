//! Nested region lineage, attachment and base-URI identity, and the embedded
//! language table. Identities come from `verter_identity`; there is no second
//! embedded-source identity.

use std::sync::Arc;

use verter_identity::identity::SourceUnitId;
use verter_identity::successor::{AttachmentId, CarrierProfileId, RegionId};

use super::map::StageKind;
use super::policy::EmbeddedTextCodec;

/// The admitted nesting depth of a request or profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NestingBudget {
    pub max_depth: u32,
}

/// A nested region past its [`NestingBudget`]. A nested decode needs the child
/// lineage first, so this stops the work before it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NestingExhausted {
    pub max_depth: u32,
}

/// One region of one source unit together with its parent regions, outermost
/// first.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RegionLineage {
    unit: SourceUnitId,
    region: RegionId,
    ancestors: Box<[RegionId]>,
}

impl RegionLineage {
    pub fn root(unit: SourceUnitId, region: RegionId) -> Self {
        Self {
            unit,
            region,
            ancestors: Box::new([]),
        }
    }

    /// Mint a nested child, refused past `budget`.
    pub fn nested(
        &self,
        region: RegionId,
        budget: NestingBudget,
    ) -> Result<Self, NestingExhausted> {
        let depth = self.ancestors.len() + 1;
        if depth > budget.max_depth as usize {
            return Err(NestingExhausted {
                max_depth: budget.max_depth,
            });
        }
        let mut ancestors = Vec::with_capacity(depth);
        ancestors.extend_from_slice(&self.ancestors);
        ancestors.push(self.region.clone());
        Ok(Self {
            unit: self.unit.clone(),
            region,
            ancestors: ancestors.into_boxed_slice(),
        })
    }

    pub fn unit(&self) -> &SourceUnitId {
        &self.unit
    }

    pub fn region(&self) -> &RegionId {
        &self.region
    }

    pub fn ancestors(&self) -> &[RegionId] {
        &self.ancestors
    }
}

/// The authored resource a region's relative references resolve against. An
/// external resource has its own, never its component's.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BaseUri(Arc<str>);

impl BaseUri {
    pub fn new(uri: &str) -> Self {
        Self(Arc::from(uri))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A profile-qualified claim on a region, its base URI, and the context that
/// reached it when that differs from the region's own unit: one external
/// template shared by two components has one unit and base URI, and a context
/// per component.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EmbeddedAttachment {
    lineage: RegionLineage,
    attachment: AttachmentId,
    context: Option<AttachmentId>,
    base_uri: BaseUri,
}

impl EmbeddedAttachment {
    pub fn new(
        lineage: RegionLineage,
        attachment: AttachmentId,
        context: Option<AttachmentId>,
        base_uri: BaseUri,
    ) -> Self {
        Self {
            lineage,
            attachment,
            context,
            base_uri,
        }
    }

    pub fn lineage(&self) -> &RegionLineage {
        &self.lineage
    }

    pub fn attachment(&self) -> &AttachmentId {
        &self.attachment
    }

    pub fn context(&self) -> Option<&AttachmentId> {
        self.context.as_ref()
    }

    pub fn base_uri(&self) -> &BaseUri {
        &self.base_uri
    }
}

/// One embedded-language declaration: which carrier, codec and chain shape a
/// host carrier's region role has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedRegionRow {
    pub host_carrier: CarrierProfileId,
    pub region_role: Box<str>,
    pub embedded_carrier: CarrierProfileId,
    pub codec: Arc<EmbeddedTextCodec>,
    pub map_chain: Box<[StageKind]>,
}

/// Two rows claiming one host carrier and region role. Rows never overwrite
/// each other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateEmbeddedRegionRow {
    pub host_carrier: CarrierProfileId,
    pub region_role: Box<str>,
}

/// The rows of one catalog, keyed by host carrier and region role. Selecting a
/// row takes no source text, so selection alone decodes nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedRegionTable {
    rows: Box<[EmbeddedRegionRow]>,
}

impl EmbeddedRegionTable {
    pub fn new(mut rows: Vec<EmbeddedRegionRow>) -> Result<Self, DuplicateEmbeddedRegionRow> {
        rows.sort_by(|a, b| {
            (&a.host_carrier, &a.region_role).cmp(&(&b.host_carrier, &b.region_role))
        });
        if let Some(pair) = rows.windows(2).find(|pair| {
            pair[0].host_carrier == pair[1].host_carrier
                && pair[0].region_role == pair[1].region_role
        }) {
            return Err(DuplicateEmbeddedRegionRow {
                host_carrier: pair[0].host_carrier.clone(),
                region_role: pair[0].region_role.clone(),
            });
        }
        Ok(Self {
            rows: rows.into_boxed_slice(),
        })
    }

    pub fn select(
        &self,
        host_carrier: &CarrierProfileId,
        region_role: &str,
    ) -> Option<&EmbeddedRegionRow> {
        self.rows
            .binary_search_by(|row| {
                (&row.host_carrier, &*row.region_role).cmp(&(host_carrier, region_role))
            })
            .ok()
            .map(|index| &self.rows[index])
    }
}

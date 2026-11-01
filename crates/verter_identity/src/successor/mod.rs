//! VID0's successor identity newtypes (`identities.md`): carrier, release,
//! framework-profile, project-profile, configured-project, attachment and
//! region identities, plus the descriptor-schema epoch they compose under.
//!
//! Types and tests only — no consumer switches and no route deletion here;
//! the consumers VID0 assigns adopt them in their own nodes. Every type is
//! a distinct Rust nominal type minted through
//! [`crate::macros::digest_identity!`], and each ratified derivation
//! (`tests/kernel/VID0/products/identity-inventory.v1.json`, `I03`–`I08`,
//! `I11`) gets one constructor that names exactly the inputs its row
//! allows. The existing `CapabilityId` (`verter_language`) and
//! [`crate::identity::SourceUnitId`] stay where they are; this module
//! never defines a second one. Profile identities are not threaded into
//! `HostConfig`/`CompileProfile`/`CodegenOptions` (`R16`).

use crate::canonical::Canonical;
use crate::encoding::CanonicalEncoder;
use crate::identity::SourceUnitId;

pub mod release;

pub use release::{ReleaseCoordinates, ReleaseDescriptorError, ReleaseId};

digest_identity!(
    /// Descriptor-schema epoch behind the successor profile identities
    /// (VID0 `I11`): an epoch, never a wall-clock or build timestamp. It
    /// is the explicit input the domain-tag version plays for every other
    /// digest identity — bumping it re-mints every profile identity
    /// derived under the bumped schema and nothing else (`R02`).
    ProfileSchemaEpoch
);

impl ProfileSchemaEpoch {
    const DOMAIN_TAG: &'static str = "verter.identity.successor.profile_schema_epoch.v1";

    /// The epoch of one descriptor schema family. `epoch` is the schema's
    /// compatibility generation, not a calendar fact.
    pub fn for_schema_family(schema_family: &str, epoch: u32) -> Self {
        let mut encoder = CanonicalEncoder::new(Self::DOMAIN_TAG);
        encoder.field_str(1, schema_family);
        encoder.field_u32(2, epoch);
        Self(Canonical::from_encoder(&encoder))
    }
}

digest_identity!(
    /// Syntax-carrier profile identity (VID0 `I03`): bytes, geometry,
    /// parse and recovery. Never a release, a semantic or project profile,
    /// a source unit, or a path/extension spelling.
    CarrierProfileId
);

impl CarrierProfileId {
    const DOMAIN_TAG: &'static str = "verter.identity.successor.carrier_profile.v1";

    /// Mint from the carrier family key and the carrier syntax-schema
    /// epoch — the only inputs `I03` names.
    pub fn from_carrier(carrier_family: &str, syntax_schema: &ProfileSchemaEpoch) -> Self {
        let mut encoder = CanonicalEncoder::new(Self::DOMAIN_TAG);
        encoder.field_str(1, carrier_family);
        encoder.field_bytes(2, syntax_schema.digest().as_bytes());
        Self(Canonical::from_encoder(&encoder))
    }
}

digest_identity!(
    /// Semantic (framework) profile identity (VID0 `I05`): the meaning of
    /// exactly one [`ReleaseId`] over the carriers it claims. Never a
    /// second release, a source unit, a project, or the display family
    /// alone.
    FrameworkProfileId
);

impl FrameworkProfileId {
    const DOMAIN_TAG: &'static str = "verter.identity.successor.framework_profile.v1";

    /// Mint from one exact release, the set of carrier profiles it claims
    /// and the descriptor-schema epoch. The claims are a canonically
    /// ordered set: manifest listing order or a repeated claim mints no
    /// second identity, while an additional carrier always does.
    pub fn from_release_and_carriers(
        release: &ReleaseId,
        claimed_carriers: &[CarrierProfileId],
        schema: &ProfileSchemaEpoch,
    ) -> Self {
        let mut encoder = CanonicalEncoder::new(Self::DOMAIN_TAG);
        encoder.field_bytes(1, release.digest().as_bytes());
        encoder.field_sorted_set(
            2,
            claimed_carriers
                .iter()
                .map(|carrier| *carrier.digest().as_bytes()),
        );
        encoder.field_bytes(3, schema.digest().as_bytes());
        Self(Canonical::from_encoder(&encoder))
    }
}

digest_identity!(
    /// Project-profile overlay identity (VID0 `I07`): one meta-framework
    /// release's overlay. References the semantic profiles it overlays and
    /// never re-mints them; never a configured project.
    ProjectProfileId
);

impl ProjectProfileId {
    const DOMAIN_TAG: &'static str = "verter.identity.successor.project_profile.v1";

    /// Mint from the project-profile family key, the exact release of the
    /// meta-framework and the descriptor-schema epoch.
    pub fn from_project_release(
        project_family: &str,
        release: &ReleaseId,
        schema: &ProfileSchemaEpoch,
    ) -> Self {
        let mut encoder = CanonicalEncoder::new(Self::DOMAIN_TAG);
        encoder.field_str(1, project_family);
        encoder.field_bytes(2, release.digest().as_bytes());
        encoder.field_bytes(3, schema.digest().as_bytes());
        Self(Canonical::from_encoder(&encoder))
    }
}

digest_identity!(
    /// Configured-project identity (VID0 `I08`): one configured project
    /// (one project configuration file and its derived membership). The
    /// caller's descriptor is the canonical configuration identity;
    /// [`crate::identity::ConfigurationRevision`] is a neighbour, never
    /// folded in. PM1 owns the derivation; this crate owns only the type.
    /// Never carries a profile or backend/process identity.
    ConfiguredProjectId
);

digest_identity!(
    /// Addressed-region identity (VID0 `I06`): one region of one source
    /// unit, addressed by its carrier-minted role — never a byte offset.
    /// Survives edits that keep the region.
    RegionId
);

impl RegionId {
    const DOMAIN_TAG: &'static str = "verter.identity.successor.region.v1";

    /// Mint from the unit and the carrier-minted region role
    /// (`"module-root"`, `"template"` …) — the only inputs `I06` names.
    pub fn from_region_role(unit: &SourceUnitId, region_role: &str) -> Self {
        let mut encoder = CanonicalEncoder::new(Self::DOMAIN_TAG);
        encoder.field_bytes(1, unit.digest().as_bytes());
        encoder.field_str(2, region_role);
        Self(Canonical::from_encoder(&encoder))
    }
}

digest_identity!(
    /// Attachment identity (VID0 `I06`): one profile-qualified semantic
    /// claim on one addressed region of one unit. Revision and content are
    /// never part of it — an edit that keeps the region keeps the
    /// attachment. Competing claims are grouped by the
    /// `(SourceUnitId, RegionId)` they share, never by attachment equality
    /// (`R10`).
    AttachmentId
);

impl AttachmentId {
    const DOMAIN_TAG: &'static str = "verter.identity.successor.attachment.v1";

    /// Mint from the claimed unit, the addressed region and the claiming
    /// profile — the only inputs `I06` names.
    pub fn from_claim(
        unit: &SourceUnitId,
        region: &RegionId,
        profile: &FrameworkProfileId,
    ) -> Self {
        let mut encoder = CanonicalEncoder::new(Self::DOMAIN_TAG);
        encoder.field_bytes(1, unit.digest().as_bytes());
        encoder.field_bytes(2, region.digest().as_bytes());
        encoder.field_bytes(3, profile.digest().as_bytes());
        Self(Canonical::from_encoder(&encoder))
    }
}

// Nominal distinctness: an `AttachmentId` must not type-check where a
// `RegionId` is required.
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{SourceId, SourceUnitId};
use verter_identity::successor::{
    AttachmentId, CarrierProfileId, FrameworkProfileId, ProfileSchemaEpoch, RegionId,
    ReleaseCoordinates, ReleaseId,
};

/// Caller-owned logical source identity for the fixture.
struct FixtureSource;
impl CanonicalEncode for FixtureSource {
    const DOMAIN_TAG: &'static str = "compile-fail.fixture_source.v1";
    fn encode_fields(&self, _: &mut CanonicalEncoder) {}
}

fn expects_region(_: RegionId) {}

fn main() {
    let schema = ProfileSchemaEpoch::for_schema_family("successor-compile-fail", 1);
    let unit = SourceUnitId::from_lineage(&SourceId::from_canonical(&FixtureSource), "script");
    let region = RegionId::from_region_role(&unit, "module-root");
    let coordinates =
        ReleaseCoordinates::decode("fixture-framework", &["1.0.0"], &[1]).expect("exact release");
    let release = ReleaseId::from_coordinates(&coordinates, &schema);
    let profile = FrameworkProfileId::from_release_and_carriers(
        &release,
        &[CarrierProfileId::from_carrier("fixture-carrier", &schema)],
        &schema,
    );
    let attachment = AttachmentId::from_claim(&unit, &region, &profile);
    expects_region(attachment);
}

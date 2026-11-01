// Fixed-derivation successor identities are minted only by their named
// constructors. A `from_canonical` taking any canonical descriptor would
// let a floating tag, a versions array, or a descriptor missing its
// schema epoch mint these types, bypassing the admission their
// constructors enforce.
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::successor::{
    AttachmentId, CarrierProfileId, FrameworkProfileId, ProfileSchemaEpoch, ProjectProfileId,
    RegionId, ReleaseId,
};

struct Descriptor;
impl CanonicalEncode for Descriptor {
    const DOMAIN_TAG: &'static str = "compile-fail.successor_descriptor.v1";
    fn encode_fields(&self, _: &mut CanonicalEncoder) {}
}

fn main() {
    let _ = ProfileSchemaEpoch::from_canonical(&Descriptor);
    let _ = CarrierProfileId::from_canonical(&Descriptor);
    let _ = FrameworkProfileId::from_canonical(&Descriptor);
    let _ = ProjectProfileId::from_canonical(&Descriptor);
    let _ = ReleaseId::from_canonical(&Descriptor);
    let _ = RegionId::from_canonical(&Descriptor);
    let _ = AttachmentId::from_canonical(&Descriptor);
}

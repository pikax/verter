// `SourceUnitId` is lineage-only. A constructor that takes a caller
// descriptor would let revision or content re-enter the unit identity.
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::SourceUnitId;

struct Descriptor;
impl CanonicalEncode for Descriptor {
    const DOMAIN_TAG: &'static str = "compile-fail.source_unit_id.v1";
    fn encode_fields(&self, _: &mut CanonicalEncoder) {}
}

fn main() {
    let _ = SourceUnitId::from_canonical(&Descriptor);
}

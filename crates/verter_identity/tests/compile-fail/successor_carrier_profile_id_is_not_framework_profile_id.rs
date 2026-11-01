// VID0T-AC1 / VC07: a `CarrierProfileId` must not type-check where a
// `FrameworkProfileId` is required.
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::successor::{CarrierProfileId, FrameworkProfileId};

struct Args(u64);
impl CanonicalEncode for Args {
    const DOMAIN_TAG: &'static str = "successor-compile-fail.args.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_u64(1, self.0);
    }
}

fn expects_framework_profile(_: FrameworkProfileId) {}

fn main() {
    let carrier = CarrierProfileId::from_canonical(&Args(1));
    expects_framework_profile(carrier);
}

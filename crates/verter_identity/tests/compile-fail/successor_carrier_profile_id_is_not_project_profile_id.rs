// VID0T-AC1 / VC07: a `CarrierProfileId` must not type-check where a
// `ProjectProfileId` is required either.
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::successor::{CarrierProfileId, ProjectProfileId};

struct Args(u64);
impl CanonicalEncode for Args {
    const DOMAIN_TAG: &'static str = "successor-compile-fail.args.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_u64(1, self.0);
    }
}

fn expects_project_profile(_: ProjectProfileId) {}

fn main() {
    let carrier = CarrierProfileId::from_canonical(&Args(1));
    expects_project_profile(carrier);
}

// VID0T-AC1 / VC07: an `AttachmentId` must not type-check where a
// `RegionId` is required.
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::successor::{AttachmentId, RegionId};

struct Args(u64);
impl CanonicalEncode for Args {
    const DOMAIN_TAG: &'static str = "successor-compile-fail.args.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_u64(1, self.0);
    }
}

fn expects_region(_: RegionId) {}

fn main() {
    let attachment = AttachmentId::from_canonical(&Args(1));
    expects_region(attachment);
}

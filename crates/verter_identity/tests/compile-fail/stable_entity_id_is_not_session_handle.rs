// Reverse direction of `session_handle_is_not_stable_entity_id.rs`: a
// `StableEntityId` must not type-check where a `SessionHandle` is required.
use verter_identity::identity::{SessionHandle, StableEntityId};
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};

struct Args(u64);
impl CanonicalEncode for Args {
    const DOMAIN_TAG: &'static str = "compile-fail-test.args.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_u64(1, self.0);
    }
}

fn expects_session_handle(_: SessionHandle) {}

fn main() {
    let id = StableEntityId::from_canonical(&Args(1));
    expects_session_handle(id);
}

// `StableEntityId` and `SessionHandle` must be non-interchangeable
// (architecture.md §3.1; identity-encoding.md §3). A `SessionHandle` must
// not type-check where a `StableEntityId` is required, even though both are
// ultimately backed by digest-shaped data.
use verter_identity::identity::{SessionHandle, StableEntityId};
use verter_identity::encoding::CanonicalDigest;

fn expects_stable_entity_id(_: StableEntityId) {}

fn main() {
    let handle = SessionHandle::mint(
        CanonicalDigest::of_bytes(b"cohort"),
        1,
        CanonicalDigest::of_bytes(b"nonce"),
    );
    expects_stable_entity_id(handle);
}

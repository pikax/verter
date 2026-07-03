//! Compile-FAIL fixture: a content-addressed DTO / query-identity key cannot
//! carry a session hot handle in its identity. `HotTypeRef` deliberately does
//! NOT derive `Hash` (it is a generation-local arena ordinal — R6-forbidden as a
//! cache key), so a `#[derive(Hash)]` DTO that names it as a field fails to
//! compile. This is the structural proof that a session-only handle cannot leak
//! into a keyable/lower-crate DTO's identity.

use verter_session::semantic_query::HotTypeRef;

#[derive(PartialEq, Eq, Hash)]
struct Dto {
    handle: HotTypeRef,
}

fn main() {
    let _ = std::mem::size_of::<Dto>();
}

// `ParseInstanceId::generation` is a nominal type, not a bare `u64`
// (`architecture.md` §6.1). The field is populated here rather than a
// standalone helper called, so the fixture fails if the field itself
// regresses to a raw integer — not merely if the nominal type stops
// existing. The other two fields are parameters: this is a type check, and
// constructing a real `ParseKey` would add nothing.
use verter_identity::identity::{ParseInstanceId, ParseKey, ParseOwnerDomainId};

fn build(owner_domain: ParseOwnerDomainId, key: ParseKey) -> ParseInstanceId {
    ParseInstanceId {
        owner_domain,
        key,
        generation: 1u64,
    }
}

fn main() {
    let _ = build;
}

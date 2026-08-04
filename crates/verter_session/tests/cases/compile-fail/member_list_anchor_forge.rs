//! T-A7 Rail 2 witness, ctor half: `MemberListAnchor::new` is crate-private
//! to `verter_semantic` — a code-action or fixture that minted an anchor from
//! arithmetic would reintroduce the source-offset guessing the unit deleted.
//! ONLY the ctor call lives in this fixture: pairing it with the
//! struct-literal vector would let either seal mask a regression of the other
//! (the fixture would still fail to compile, and trybuild would still pass).

fn main() {
    let _forged = verter_semantic::analysis::types::MemberListAnchor::new(4, false);
}

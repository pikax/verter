//! T-A7 Rail 2 witness, struct-literal half: `MemberListAnchor`'s fields are
//! private, so the literal-forging vector a public field would open must fail
//! to compile on its own — separate from the ctor fixture, so neither seal
//! can mask a regression of the other.

fn main() {
    let _literal = verter_semantic::analysis::types::MemberListAnchor {
        insert_offset: 4,
        is_empty: false,
    };
}

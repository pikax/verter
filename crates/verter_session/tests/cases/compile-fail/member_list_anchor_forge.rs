//! Constructor half of the member-list anchor seal: minting an anchor demands
//! the analyzer-only `MemberListAnchorMint` authority, which a crate can obtain
//! only by naming `verter_analyzer_mint` — a dependency the session does not
//! have. A code-action or fixture that minted an anchor from arithmetic would
//! reintroduce the source-offset guessing the analyzer-minted anchor removed.
//! ONLY the ctor call lives in this fixture: pairing it with the
//! struct-literal vector would let either seal mask a regression of the other
//! (the fixture would still fail to compile, and trybuild would still pass).

fn main() {
    let _forged = verter_session_query::analysis::types::MemberListAnchor::new(
        verter_analyzer_mint::MemberListAnchorMint::grant(),
        4,
        false,
    );
}

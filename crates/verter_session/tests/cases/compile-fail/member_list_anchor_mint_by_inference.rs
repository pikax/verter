//! Inference half of the member-list anchor seal: a crate that cannot name
//! `verter_analyzer_mint` must not obtain the mint authority through a
//! value-producing trait the compiler can infer from the constructor's
//! parameter type. The authority implements no such trait, so a consumer that
//! never names the authority's crate still cannot mint an anchor.

fn main() {
    let _forged = verter_session_query::analysis::types::MemberListAnchor::new(
        Default::default(),
        4,
        false,
    );
}

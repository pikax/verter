//! Construction authorities reserved for the syntax analyzer.
//!
//! Some analysis records may only be minted by the analyzer that walks the
//! live syntax tree, yet are defined in the parser-free record crate the
//! analyzer depends on. Rust has no crate-to-crate visibility, so the record
//! type cannot restrict its constructor to that one foreign producer. The
//! record's constructor instead demands an authority value defined HERE.
//!
//! The authority is sealed by the dependency graph: its only constructor is
//! an inherent function of a type with a private field, and it implements no
//! trait that can produce a value (`Default`, `From`, `Deserialize`), so a
//! crate can obtain one only by naming this crate — which requires a direct
//! dependency on it. The permitted direct dependents are the analyzer
//! (`verter_semantic`) and the record crate whose constructors take the
//! authority (`verter_session_query`); a consumer such as the session or the
//! language server cannot mint a record from source-offset arithmetic.

/// Authority to mint a `MemberListAnchor`: the append position of an authored
/// macro member list, which only the analyzer derives from a live syntax node.
#[derive(Debug, Clone, Copy)]
pub struct MemberListAnchorMint {
    _sealed: (),
}

impl MemberListAnchorMint {
    /// Take the authority. Reachable only from a crate that depends on this
    /// crate directly (see the crate docs).
    #[must_use]
    pub const fn grant() -> Self {
        Self { _sealed: () }
    }
}

//! Excess-property provenance ([`ExcessPropertyOrigin`]) and the ordered
//! object-literal spread entry ([`SpreadMember`]) — the identity-bearing
//! member-origin surface of the shared type IR.

use crate::TypeExpr;

/// The excess-property provenance of an object member — whether the member is
/// an excess-property CANDIDATE when its owning surface is the fresh source of
/// an excess-checked relation.
///
/// This is a property of the TYPE, not of a request: it participates in node
/// interning identity (Eq / Hash) on every member carrier, and it is
/// semantically read ONLY by excess-property candidate selection. Ordinary
/// property matching, value assignability, optionality, index-signature
/// checking, union relation, and signature relation ignore it.
///
/// - [`FreshOwn`](Self::FreshOwn): the member was written directly in an object
///   literal (direct property, shorthand, method, getter, setter) and no later
///   spread overlapped it — an excess candidate.
/// - [`SpreadTainted`](Self::SpreadTainted): the member arrived through (or was
///   overlapped by) a spread during the literal fold — exempt from being
///   REPORTED as excess, but its value still relates normally when the name is
///   known.
/// - [`NonLiteral`](Self::NonLiteral): every non-literal origin — type
///   annotations, declarations, synthesized/merged members,
///   variable/reference/declaration materialization. Never an excess candidate.
///
/// `Default` is [`NonLiteral`](Self::NonLiteral) ONLY for serde wire
/// compatibility (`#[serde(default)]` on pre-existing JSON without the field);
/// Rust producers construct the field explicitly — the struct literal /
/// constructor sites are the exhaustive-choice points.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
    verter_no_typeexpr::NoTypeExpr,
    verter_no_storedspan::NoStoredSpan,
)]
#[serde(rename_all = "camelCase")]
pub enum ExcessPropertyOrigin {
    /// Directly authored in an object literal and untouched by later spreads.
    FreshOwn,
    /// Introduced or overlapped by a spread during the literal fold.
    SpreadTainted,
    /// Any non-literal origin (annotation / declaration / synthesis /
    /// reference materialization) — the wire-compat default.
    #[default]
    NonLiteral,
}

/// An object-literal spread entry (`{ ...operand }`) carried in source order
/// on [`ObjectMember::Spread`]. `ty` is the spread OPERAND's type — not a
/// member surface. The shared spread materializer folds the ordered entry
/// list; the pre-fold IR never fabricates an exact member surface for the
/// operand.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SpreadMember {
    /// The spread operand's type.
    pub ty: TypeExpr,
}

impl SpreadMember {
    /// Construct a spread entry from the operand's type.
    #[must_use]
    pub fn new(ty: TypeExpr) -> Self {
        Self { ty }
    }
}

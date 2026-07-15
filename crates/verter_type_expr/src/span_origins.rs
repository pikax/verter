//! Producer-emitted span-recovery origin locators.
//!
//! Span-bearing IR structs (`ObjectProperty`, `MethodSignature`, `FunctionExpr`,
//! `IndexSignature`, `FunctionParam`) put their span field(s) in `Eq`/`Hash`, so
//! member spans participate in node identity. A closed fact NEVER stores a
//! `Span` (that is the [`NoStoredSpan`] contract). Instead, a fact that
//! reconstructs / hash-interns such a node carries a producer-emitted ORIGIN
//! LOCATOR — sufficient to recover the exact spans from the producing canonical's
//! retained parse snapshot BEFORE any `Eq`/`Hash`/interning — one per
//! identity-participating span class.
//!
//! Each origin is content-relative to the specific parse it was emitted against:
//! the `contributor_index` and member ordinals name positions within one
//! `Program` body. The enclosing fact carries the whole-hash self-root that
//! content-validates the origin, so a content edit invalidates the fact and
//! re-emits fresh positions.
//!
//! [`NoStoredSpan`]: verter_no_storedspan::NoStoredSpan

use std::sync::Arc;
use verter_no_storedspan::NoStoredSpan;
use verter_no_typeexpr::NoTypeExpr;

/// Which authored top-level statement (contributor) in the producing snapshot
/// holds the decl body an origin addresses. Producer-emitted; indexes
/// `program.body[contributor_index]`. A small named index, never a byte span.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct DeclContributorAnchor {
    /// Index into the retained `Program` body identifying the contributing
    /// statement whose declaration body this origin descends from.
    pub contributor_index: u32,
}

/// Marker for a truly synthetic node with no authored origin. Recovery of a
/// synthetic origin yields the default (all-absent) spans — an honest absence,
/// never a fabricated byte-0 span. Permitted ONLY where a producer genuinely
/// synthesizes a node (documented per site), never as a fallback for an authored
/// node whose origin locator was omitted.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct SourceSynthetic;

/// Origin sufficient to recover a `MemberSpans` (declaration / name /
/// type-annotation spans) for a property or method member.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum MemberSpansOrigin {
    /// An authored member reached by descending `member_path` member-ordinals
    /// from the decl body at `anchor`. Each ordinal selects a member of the
    /// current object/interface surface; a non-final ordinal descends into that
    /// member's value-type surface (a nested type literal). The final ordinal
    /// names the member whose spans are recovered.
    Authored {
        anchor: DeclContributorAnchor,
        member_path: Arc<[u32]>,
    },
    /// A genuinely synthetic member with no authored origin.
    Synthetic(SourceSynthetic),
}

/// Origin sufficient to recover an `IndexSignatureSpans` (declaration / key /
/// value spans) for an index-signature member.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum IndexSignatureSpansOrigin {
    /// The index-signature member reached by `member_path` from the decl body.
    Authored {
        anchor: DeclContributorAnchor,
        member_path: Arc<[u32]>,
    },
    /// A synthetic index signature with no authored origin.
    Synthetic(SourceSynthetic),
}

/// Origin sufficient to recover a `FunctionSpans` (signature / return-type
/// spans) for a function-like node.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum FunctionSpansOrigin {
    /// The decl body at `anchor` is itself a bare function type
    /// (`type F = (..) => ..`). No member path.
    AliasBody { anchor: DeclContributorAnchor },
    /// The function of the method / call-signature / construct-signature member
    /// reached by `member_path` from the decl body.
    Member {
        anchor: DeclContributorAnchor,
        member_path: Arc<[u32]>,
    },
    /// A synthetic function with no authored origin.
    Synthetic(SourceSynthetic),
}

/// Selects one parameter of a located function for `FunctionParam.span` recovery.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum FunctionParamSelector {
    /// A positional parameter (`params.items[ordinal]`).
    Positional { ordinal: u32 },
    /// The rest parameter (`params.rest`).
    Rest,
}

/// Origin sufficient to recover a `FunctionParam.span` (which participates in the
/// hand-written `FunctionParam` identity). Locates the enclosing function, then
/// selects one parameter.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct FunctionParamSpanOrigin {
    /// Origin of the enclosing function.
    pub function: FunctionSpansOrigin,
    /// Which parameter of that function.
    pub param: FunctionParamSelector,
}

//! Reusable authored embedded-region geometry.
//!
//! An embedded region is text whose bytes are not (or not only) the authored
//! bytes around it: a template in a tagged literal, a stylesheet in a string,
//! a preprocessor's output. This module owns the geometry every such consumer
//! shares (`docs/arch/kernel/embedded-codecs.md`):
//!
//! - [`EmbeddedTextCodec`] is a profile's declared policy: raw or cooked
//!   selection, delimiters, escapes, line terminators, indentation and
//!   interpolation holes. Every policy is data; the codec has no arm for any
//!   language and never decides what a tag or property means.
//! - [`CookedText`] is scalar UTF-8. A value that is not Unicode scalar text
//!   is a typed [`NonUnicodeCookedLiteral`], an absent one a typed
//!   [`MissingCookedValue`]; neither yields text or a position, so no embedded
//!   parser can run on them.
//! - [`MapStage`] and [`AuthoredMapChain`] answer every query with exact
//!   authored UTF-8 geometry or a typed [`Unmappable`]. Syntax, preprocessor,
//!   compiler, runtime-build and code-generation stages stay distinct, and a
//!   stage joins only an exactly matching [`MapBasis`].
//!
//! Hole and literal geometry is parser-minted input ([`LiteralGeometry`]): the
//! codec never rediscovers where an expression ends. Offsets use the checked
//! coordinate domains of `verter_span`; code-unit indices stay private to
//! validation and never leave this module.

mod chain;
mod cooked;
mod decode;
mod lineage;
mod map;
mod policy;

pub use chain::{AuthoredMapChain, MapBasis};
pub use cooked::{
    CookedPartiality, CookedText, MissingCookedValue, NonUnicodeCookedLiteral, SurrogateKind,
};
pub use decode::{
    CodecRefusal, DecodeInterrupt, DecodeRequest, DecodeWork, DecodedChunk, DecodedLiteral,
    GeometryError, HoleGeometry, HoleRegion, LiteralEdit, LiteralGeometry, NeverInterrupted,
};
pub use lineage::{
    BaseUri, DuplicateEmbeddedRegionRow, EmbeddedAttachment, EmbeddedRegionRow,
    EmbeddedRegionTable, NestingBudget, NestingExhausted, RegionLineage,
};
pub use map::{
    AuthoredOrigin, AuthoredProvenance, CoordinateDomain, DomainOffset, DomainRange,
    EditCertification, Elision, ElisionKind, MapSegment, MapStage, MapStageError, OriginRef,
    SegmentOrigin, StageKind, StageSegments, Unmappable, WholeKind,
};
pub use policy::{
    CookedPolicy, DelimiterPolicy, EmbeddedTextCodec, EscapePolicy, HolePolicy, IdentityEscapes,
    IndentationPolicy, LineTerminatorPolicy, NumericEscape, PlaceholderSpelling, ProfileHoleClass,
    ReverseEncoding, SimpleEscape, TextSelection, UnencodableReplacement,
};

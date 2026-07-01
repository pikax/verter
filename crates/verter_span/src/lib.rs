//! Typed byte-offset spans for the Verter Vue compiler.
//!
//! Four span types enforce coordinate-system safety at compile time:
//!
//! | Type | Meaning | Serde? |
//! |------|---------|--------|
//! | [`Span`] | SFC-absolute byte offsets | Yes |
//! | [`RelativeSpan`] | Relative to a base stored elsewhere | No |
//! | [`PartialGeneratedSpan`] | Unresolved position in generated output | No |
//! | [`GeneratedSpan`] | Resolved generated + SFC origin mapping | No |
//!
//! # Rules
//!
//! 1. All data crossing a serialization boundary (serde, MCP, LSP, FFI) MUST use [`Span`].
//! 2. Inter-crate stored types prefer [`Span`]. [`RelativeSpan`] is for intra-crate processing.
//! 3. [`RelativeSpan`] is 8 bytes, same as [`Span`]. The base offset lives in context.

pub mod path;
pub use path::{
    canonicalize_path, canonicalize_path_cow, fs_is_case_insensitive, fs_paths_equal, is_under_dir,
    longest_project_root, CanonicalPath, InjectedPathKey,
};

pub mod tsgo_offset;
pub use tsgo_offset::{utf16_offset_to_byte, utf16_offset_to_line_col};

// ======================== Span (SFC-absolute) ========================

/// SFC-absolute byte offset span. `[start, end)` half-open.
///
/// All positions are relative to the start of the `.vue` file.
/// This is the **only** span type that implements `Serialize`/`Deserialize`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    #[inline]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    #[inline]
    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    /// Alias for `len()` — compatible with `oxc_span::Span`.
    #[inline]
    pub fn size(&self) -> u32 {
        self.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Extract the slice from the source string.
    #[inline]
    pub fn slice<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start as usize..self.end as usize]
    }

    /// Whether `offset` falls within `[start, end)`.
    #[inline]
    pub fn contains_offset(&self, offset: u32) -> bool {
        offset >= self.start && offset < self.end
    }

    /// Convert to a relative span by subtracting a base offset.
    #[inline]
    pub fn to_relative(&self, base: u32) -> RelativeSpan {
        RelativeSpan {
            start: self.start.saturating_sub(base),
            end: self.end.saturating_sub(base),
        }
    }

    /// Rebase the span by a signed byte `delta`, saturating each endpoint at the
    /// `u32` bounds.
    ///
    /// Used to translate spans produced against one buffer into another buffer's
    /// coordinates (e.g. a type lowered from a synthetic wrapper string rebased
    /// into its real source file). `delta` is signed because the target offset
    /// may be lower than the source buffer's; endpoints clamp to `[0, u32::MAX]`
    /// rather than wrapping.
    #[inline]
    #[must_use]
    pub fn shifted(&self, delta: i64) -> Span {
        let apply =
            |value: u32| -> u32 { (i64::from(value) + delta).clamp(0, i64::from(u32::MAX)) as u32 };
        Span {
            start: apply(self.start),
            end: apply(self.end),
        }
    }
}

impl From<oxc_span::Span> for Span {
    #[inline]
    fn from(span: oxc_span::Span) -> Self {
        Self {
            start: span.start,
            end: span.end,
        }
    }
}

// ======================== RelativeSpan ========================

/// Byte offset span relative to some base (expression start, style block content, script block).
///
/// The base offset is stored alongside in context (e.g., `BindingContext::base_offset`,
/// `StyleBlockAnalysis::content_offset`, `OxcParsedExpression::offset`).
///
/// **Does NOT implement `Serialize`/`Deserialize`.** Must be converted to [`Span`] via
/// [`to_absolute(base)`](RelativeSpan::to_absolute) before crossing a module boundary
/// or serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RelativeSpan {
    pub start: u32,
    pub end: u32,
}

impl RelativeSpan {
    #[inline]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    #[inline]
    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Extract the slice from the content string (relative to the same base).
    #[inline]
    pub fn slice<'a>(&self, content: &'a str) -> &'a str {
        &content[self.start as usize..self.end as usize]
    }

    /// Convert to SFC-absolute [`Span`] by adding the base offset.
    #[inline]
    pub fn to_absolute(&self, base: u32) -> Span {
        Span {
            start: self.start + base,
            end: self.end + base,
        }
    }
}

impl From<oxc_span::Span> for RelativeSpan {
    #[inline]
    fn from(span: oxc_span::Span) -> Self {
        Self {
            start: span.start,
            end: span.end,
        }
    }
}

// ======================== PartialGeneratedSpan ========================

/// An unresolved position in generated output (TSX, compiled template, SSR).
///
/// Used before source map / PositionMapper resolution is performed. Convert to
/// [`GeneratedSpan`] via [`resolve(origin)`](PartialGeneratedSpan::resolve) once
/// the SFC-absolute origin is known.
///
/// **Does NOT implement `Serialize`/`Deserialize`.**
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PartialGeneratedSpan {
    pub start: u32,
    pub end: u32,
}

impl PartialGeneratedSpan {
    #[inline]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    #[inline]
    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Extract the slice from the generated output string.
    #[inline]
    pub fn slice<'a>(&self, generated_source: &'a str) -> &'a str {
        &generated_source[self.start as usize..self.end as usize]
    }

    /// Convert to a [`Span`] representing the position in generated output.
    /// Useful when you need the generated position as a plain span.
    #[inline]
    pub fn as_generated_span(&self) -> Span {
        Span::new(self.start, self.end)
    }

    /// Resolve this partial span into a fully resolved [`GeneratedSpan`]
    /// by providing the SFC-absolute origin span.
    #[inline]
    pub fn resolve(&self, origin: Span) -> GeneratedSpan {
        GeneratedSpan {
            generated: Span::new(self.start, self.end),
            origin,
        }
    }
}

// ======================== GeneratedSpan ========================

/// A fully resolved position mapping from generated output back to original SFC source.
///
/// Only created **after** source map / PositionMapper resolution — always contains both
/// the generated position and the resolved SFC-absolute origin. Never in a "pending" state.
///
/// **Does NOT implement `Serialize`/`Deserialize`.** Use `origin` for display purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GeneratedSpan {
    /// Position in generated output (TSX, compiled template, SSR).
    pub generated: Span,
    /// Resolved SFC-absolute origin.
    pub origin: Span,
}

impl GeneratedSpan {
    /// Create a resolved mapping from known generated and origin spans.
    #[inline]
    pub const fn new(generated: Span, origin: Span) -> Self {
        Self { generated, origin }
    }
}

// ======================== Serde Helpers ========================

/// Helpers for custom `Serialize`/`Deserialize` impls on analysis types that embed [`Span`].
///
/// These preserve the existing JSON field names (`"spanStart"` / `"spanEnd"`) as flat keys
/// without using `#[serde(flatten)]` (which has ~2-3x overhead due to intermediate Map).
pub mod serde_helpers {
    use super::Span;
    use serde::ser::SerializeMap;

    /// Serialize a [`Span`] as flat `"spanStart"` / `"spanEnd"` fields into an existing map.
    pub fn serialize_span_fields<M: SerializeMap>(
        span: &Span,
        map: &mut M,
    ) -> Result<(), M::Error> {
        map.serialize_entry("spanStart", &span.start)?;
        map.serialize_entry("spanEnd", &span.end)?;
        Ok(())
    }

    /// Serialize two [`Span`]s as `"spanStart"` / `"spanEnd"` and
    /// `"argSpanStart"` / `"argSpanEnd"`.
    pub fn serialize_span_and_arg_span_fields<M: SerializeMap>(
        span: &Span,
        arg_span: &Span,
        map: &mut M,
    ) -> Result<(), M::Error> {
        map.serialize_entry("spanStart", &span.start)?;
        map.serialize_entry("spanEnd", &span.end)?;
        map.serialize_entry("argSpanStart", &arg_span.start)?;
        map.serialize_entry("argSpanEnd", &arg_span.end)?;
        Ok(())
    }

    /// Construct a [`Span`] from deserialized `spanStart` / `spanEnd` values.
    #[inline]
    pub fn span_from_fields(span_start: u32, span_end: u32) -> Span {
        Span::new(span_start, span_end)
    }
}

// ======================== Typed LSP / generated-TSX coordinate wrappers ========================
//
// Intra-process boundary types for the LSP `PositionMapper` and the cross-file
// navigation stack. They carry NO serde (they never cross a serialization
// boundary — `Span` is the only serde span type, rule 1 above).
//
// The point of these newtypes is the SAME rule that governs the span types:
// there is no `From` between a source-side type and a generated-side type, so a
// generated-TSX coordinate can never be silently stored where a real-`.vue`
// source coordinate is expected (and vice versa). In particular there is no
// `From<GeneratedByteRange> for SourceByteRange`. Likewise `LspPosition`
// (negotiated-encoding `.vue` position) and `TsPosition` (generated-TSX
// position) are distinct, so a TSX position can never be passed where a Vue LSP
// position is expected.
//
// Conversions provided are only the total, lossless ones WITHIN a single
// coordinate space (e.g. building a range from two same-space offsets). There
// is deliberately no byte<->utf16 conversion here: that requires the source
// text and belongs to the `LineIndex`, not to a plain newtype.

/// Byte offset into the original `.vue` source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceByteOffset(pub u32);

/// Byte offset into the generated TSX output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GeneratedByteOffset(pub u32);

/// Length (in bytes) of a generated-TSX content region (the `content_offset` domain).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GeneratedByteLen(pub u32);

/// UTF-16 code-unit offset into the original `.vue` source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceUtf16Offset(pub u32);

/// UTF-16 code-unit offset into the generated TSX output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GeneratedUtf16Offset(pub u32);

/// Byte range `[start, end)` in the original `.vue` source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceByteRange {
    pub start: SourceByteOffset,
    pub end: SourceByteOffset,
}

impl SourceByteRange {
    #[inline]
    pub const fn new(start: SourceByteOffset, end: SourceByteOffset) -> Self {
        Self { start, end }
    }

    #[inline]
    pub fn len(&self) -> u32 {
        self.end.0.saturating_sub(self.start.0)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start.0 >= self.end.0
    }
}

/// Byte range `[start, end)` in the generated TSX output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GeneratedByteRange {
    pub start: GeneratedByteOffset,
    pub end: GeneratedByteOffset,
}

impl GeneratedByteRange {
    #[inline]
    pub const fn new(start: GeneratedByteOffset, end: GeneratedByteOffset) -> Self {
        Self { start, end }
    }

    #[inline]
    pub fn len(&self) -> u32 {
        self.end.0.saturating_sub(self.start.0)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start.0 >= self.end.0
    }
}

/// A 0-based position in the original `.vue` source, in the LSP-negotiated encoding
/// (UTF-16 columns in the default Verter configuration). The Vue side of the mapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

impl LspPosition {
    #[inline]
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// A 0-based position in the generated TSX output. The TSX side of the mapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TsPosition {
    pub line: u32,
    pub character: u32,
}

impl TsPosition {
    #[inline]
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

// ======================== Tests ========================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_basic() {
        let s = Span::new(10, 20);
        assert_eq!(s.len(), 10);
        assert_eq!(s.size(), 10);
        assert!(!s.is_empty());
        assert!(s.contains_offset(10));
        assert!(s.contains_offset(15));
        assert!(!s.contains_offset(20)); // half-open
        assert!(!s.contains_offset(9));
    }

    #[test]
    fn span_empty() {
        let s = Span::new(5, 5);
        assert_eq!(s.len(), 0);
        assert!(s.is_empty());
    }

    #[test]
    fn span_slice() {
        let source = "Hello, World!";
        let s = Span::new(7, 12);
        assert_eq!(s.slice(source), "World");
    }

    #[test]
    fn span_default() {
        let s = Span::default();
        assert_eq!(s.start, 0);
        assert_eq!(s.end, 0);
        assert!(s.is_empty());
    }

    #[test]
    fn span_from_oxc() {
        let oxc = oxc_span::Span::new(5, 15);
        let s: Span = oxc.into();
        assert_eq!(s.start, 5);
        assert_eq!(s.end, 15);
    }

    #[test]
    fn span_serde_roundtrip() {
        let s = Span::new(42, 100);
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("42"));
        assert!(json.contains("100"));
        let deserialized: Span = serde_json::from_str(&json).unwrap();
        assert_eq!(s, deserialized);
    }

    #[test]
    fn relative_span_basic() {
        let r = RelativeSpan::new(0, 10);
        assert_eq!(r.len(), 10);
        assert!(!r.is_empty());
    }

    #[test]
    fn relative_span_to_absolute() {
        let r = RelativeSpan::new(5, 15);
        let abs = r.to_absolute(100);
        assert_eq!(abs.start, 105);
        assert_eq!(abs.end, 115);
    }

    #[test]
    fn span_to_relative() {
        let s = Span::new(105, 115);
        let r = s.to_relative(100);
        assert_eq!(r.start, 5);
        assert_eq!(r.end, 15);
    }

    #[test]
    fn relative_span_roundtrip() {
        let original = Span::new(105, 115);
        let relative = original.to_relative(100);
        let back = relative.to_absolute(100);
        assert_eq!(original, back);
    }

    #[test]
    fn relative_span_from_oxc() {
        let oxc = oxc_span::Span::new(3, 8);
        let r: RelativeSpan = oxc.into();
        assert_eq!(r.start, 3);
        assert_eq!(r.end, 8);
    }

    #[test]
    fn relative_span_slice() {
        let content = "count + 1";
        let r = RelativeSpan::new(0, 5);
        assert_eq!(r.slice(content), "count");
    }

    #[test]
    fn partial_generated_span_basic() {
        let p = PartialGeneratedSpan::new(50, 75);
        assert_eq!(p.len(), 25);
        assert!(!p.is_empty());
    }

    #[test]
    fn partial_generated_span_resolve() {
        let partial = PartialGeneratedSpan::new(50, 75);
        let origin = Span::new(10, 35);
        let resolved = partial.resolve(origin);
        assert_eq!(resolved.generated, Span::new(50, 75));
        assert_eq!(resolved.origin, Span::new(10, 35));
    }

    #[test]
    fn partial_generated_span_as_span() {
        let p = PartialGeneratedSpan::new(50, 75);
        let s = p.as_generated_span();
        assert_eq!(s, Span::new(50, 75));
    }

    #[test]
    fn generated_span_new() {
        let g = GeneratedSpan::new(Span::new(50, 75), Span::new(10, 35));
        assert_eq!(g.generated.start, 50);
        assert_eq!(g.generated.end, 75);
        assert_eq!(g.origin.start, 10);
        assert_eq!(g.origin.end, 35);
    }

    // Type safety: these should NOT compile.
    // Uncomment to verify the compiler catches mixing:
    //
    // fn _compile_fail_relative_serialize() {
    //     let r = RelativeSpan::new(0, 10);
    //     serde_json::to_string(&r).unwrap(); // ERROR: RelativeSpan doesn't impl Serialize
    // }
    //
    // fn _compile_fail_span_from_relative(r: RelativeSpan) -> Span {
    //     r.into() // ERROR: no From<RelativeSpan> for Span
    // }

    #[test]
    fn type_sizes() {
        assert_eq!(std::mem::size_of::<Span>(), 8);
        assert_eq!(std::mem::size_of::<RelativeSpan>(), 8);
        assert_eq!(std::mem::size_of::<PartialGeneratedSpan>(), 8);
        assert_eq!(std::mem::size_of::<GeneratedSpan>(), 16);
    }

    #[test]
    fn typed_coord_sizes_are_minimal() {
        // Newtype offsets are zero-cost wrappers over u32.
        assert_eq!(std::mem::size_of::<SourceByteOffset>(), 4);
        assert_eq!(std::mem::size_of::<GeneratedByteOffset>(), 4);
        assert_eq!(std::mem::size_of::<GeneratedByteLen>(), 4);
        assert_eq!(std::mem::size_of::<SourceUtf16Offset>(), 4);
        assert_eq!(std::mem::size_of::<GeneratedUtf16Offset>(), 4);
        assert_eq!(std::mem::size_of::<SourceByteRange>(), 8);
        assert_eq!(std::mem::size_of::<GeneratedByteRange>(), 8);
        assert_eq!(std::mem::size_of::<LspPosition>(), 8);
        assert_eq!(std::mem::size_of::<TsPosition>(), 8);
    }

    #[test]
    fn typed_ranges_len_and_empty() {
        let s = SourceByteRange::new(SourceByteOffset(10), SourceByteOffset(20));
        assert_eq!(s.len(), 10);
        assert!(!s.is_empty());
        assert!(SourceByteRange::new(SourceByteOffset(5), SourceByteOffset(5)).is_empty());

        let g = GeneratedByteRange::new(GeneratedByteOffset(3), GeneratedByteOffset(9));
        assert_eq!(g.len(), 6);
        assert!(!g.is_empty());
        assert!(GeneratedByteRange::new(GeneratedByteOffset(7), GeneratedByteOffset(7)).is_empty());
    }

    #[test]
    fn typed_positions_are_distinct_and_constructed() {
        // LspPosition (Vue side) and TsPosition (TSX side) are deliberately
        // distinct types so a TSX coordinate cannot be passed where a Vue LSP
        // coordinate is expected. They only share field shape, not identity.
        let vue = LspPosition::new(3, 14);
        assert_eq!(vue.line, 3);
        assert_eq!(vue.character, 14);

        let tsx = TsPosition::new(0, 7);
        assert_eq!(tsx.line, 0);
        assert_eq!(tsx.character, 7);
    }

    // Type safety: these should NOT compile (no cross-space `From`):
    //
    // fn _compile_fail_generated_range_to_source(g: GeneratedByteRange) -> SourceByteRange {
    //     g.into() // ERROR: no From<GeneratedByteRange> for SourceByteRange
    // }
    //
    // fn _compile_fail_ts_pos_as_lsp_pos(t: TsPosition) -> LspPosition {
    //     t.into() // ERROR: no From<TsPosition> for LspPosition
    // }
}

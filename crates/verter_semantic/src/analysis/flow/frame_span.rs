//! [`FrameSpan`] — the flow substrate's own coordinate system.
//!
//! The flow artifacts ([`FunctionBodySkeleton`](super::FunctionBodySkeleton),
//! [`FlowSliceIR`](super::flow_ir::FlowSliceIR)) are CONTENT-ADDRESSED: each is
//! memoized per function content version and reused for any file content its
//! key admits. An absolute file offset is not a property of that content — a
//! blank line ANYWHERE above the function moves every one of them while
//! changing nothing the key can see — so a stored absolute offset makes the
//! artifact a function of more than its key, which is exactly the property the
//! flow module's own contract says cannot happen.
//!
//! Keeping every stored offset frame-relative was a CONVENTION once, applied
//! record family by record family, and a convention applied per family is a
//! convention that gets applied to five of seven: the two footprint families
//! (reads, calls) kept absolute offsets inside an otherwise anchor-relative
//! artifact, and a source-order sort that mixed the two silently ordered every
//! call after every write at any non-zero anchor.
//!
//! So the two coordinate systems are DIFFERENT TYPES. `FrameSpan`'s fields are
//! private, its only constructor from a source position is [`FrameSpan::rebase`]
//! (the one crossing IN), and the only way back to a live file position is
//! [`FrameSpan::to_absolute`] (the one crossing OUT, which has to be handed the
//! anchor). A stored field typed `FrameSpan` cannot hold an absolute offset,
//! and `frame_span.start() < absolute.start` does not compile.
//!
//! [`verter_span::RelativeSpan`] cannot serve this: its fields are PUBLIC (so a
//! mixed comparison compiles), it has a `new(start, end)` constructor and a
//! `From<oxc_span::Span>` that rebases NOTHING — an absolute offset wearing the
//! relative type is exactly one call away. That is correct for its own users,
//! where the sub-parser's spans are already content-relative and the type only
//! records which base they belong to; it is the opposite of what this rail
//! needs, which is a type whose ONLY inhabitants have been rebased.

use verter_no_typeexpr::NoTypeExpr;

/// A source span expressed RELATIVE to one function frame's own anchor.
///
/// Ordering is by `(start, end)`, which is source order WITHIN one frame —
/// the only frame it means anything in. Two `FrameSpan`s from different
/// frames are not comparable in any meaningful sense, and nothing in the
/// substrate mixes them: every artifact is per-function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, NoTypeExpr)]
pub struct FrameSpan {
    start: u32,
    end: u32,
}

impl FrameSpan {
    /// THE crossing IN: rebase one ABSOLUTE source span onto `anchor`.
    ///
    /// The anchor precedes the function's parameter list and a named
    /// function expression's own name, so every rebased span is
    /// non-negative; the saturating subtraction is a floor, not a policy.
    #[must_use]
    pub fn rebase(anchor: u32, span: verter_span::Span) -> Self {
        Self {
            start: span.start.saturating_sub(anchor),
            end: span.end.saturating_sub(anchor),
        }
    }

    /// THE crossing OUT: project back onto the LIVE file `anchor` was taken
    /// from.
    ///
    /// Only correct against the same file content the anchor came from, which
    /// is why the anchor has to be supplied again rather than carried: a
    /// `FrameSpan` outlives the file version it was recorded against, and
    /// pairing it with the wrong anchor is the bug this type exists to make
    /// visible at the call site.
    #[must_use]
    pub fn to_absolute(self, anchor: u32) -> verter_span::Span {
        verter_span::Span::new(
            self.start.saturating_add(anchor),
            self.end.saturating_add(anchor),
        )
    }

    /// The frame-relative start offset.
    #[must_use]
    pub fn start(self) -> u32 {
        self.start
    }

    /// The frame-relative end offset.
    #[must_use]
    pub fn end(self) -> u32 {
        self.end
    }

    /// The width in bytes.
    #[must_use]
    pub fn width(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    /// Whether this span fully CONTAINS `other` (inclusive at both edges).
    #[must_use]
    pub fn contains(self, other: Self) -> bool {
        self.start <= other.start && self.end >= other.end
    }
}

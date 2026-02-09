/// A simple byte offset span into the source text.
/// More lightweight than SourceLocation - no line/column info, no source string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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

    /// Alias for `len()` - compatible with oxc_span::Span
    #[inline]
    pub fn size(&self) -> u32 {
        self.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Extract the slice from the source string
    #[inline]
    pub fn slice<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start as usize..self.end as usize]
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

/// Positions are byte-based and copy-only for fast propagation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize)]
pub struct Position {
    pub column: u32,
    pub line: u32,

    // Byte offset from start of file
    #[serde(skip)]
    pub offset: u32,

    #[serde(rename = "offset")]
    pub offset_utf16: u32,
}

impl Position {
    #[inline]
    pub const fn new(offset: u32, line: u32, column: u32, offset_utf16: u32) -> Self {
        Self {
            offset,
            line,
            column,
            offset_utf16,
        }
    }
}

/// The node's range. The `start` is inclusive and `end` is exclusive: [start, end).
/// Stores a borrowed slice of the source text to avoid allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize)]
pub struct SourceLocation<'a> {
    pub start: Position,
    pub end: Position,
    /// Borrowed slice of the input covering this location.
    pub source: &'a str,
}

impl<'a> SourceLocation<'a> {
    /// Create a location with empty source; populate with `with_source` or via helpers.
    #[inline]
    pub const fn new(start: Position, end: Position) -> Self {
        Self {
            start,
            end,
            source: "",
        }
    }

    #[inline]
    pub fn from_source(source: &'a str, start: Position, end: Position) -> Self {
        Self {
            start,
            end,
            source: &source[start.offset as usize..end.offset as usize],
        }
    }

    /// Create a location with an explicit borrowed slice of the input.
    #[inline]
    pub const fn new_with_source(start: Position, end: Position, source: &'a str) -> Self {
        Self { start, end, source }
    }

    /// Create an empty location at `pos` with an empty source slice.
    #[inline]
    pub const fn empty_at(pos: Position) -> Self {
        Self {
            start: pos,
            end: pos,
            source: "",
        }
    }
}

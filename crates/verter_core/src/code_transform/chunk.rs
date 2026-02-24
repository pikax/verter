/// Represents a piece of the output string.
///
/// Each variant models a distinct operation, eliminating impossible states
/// (e.g. `was_moved: true` with no original span) that the previous
/// `Edited { Option<u32>, Option<u32>, bool }` design allowed.
#[derive(Debug, Clone, Copy)]
pub(super) enum Chunk<'a> {
    /// Original content from source (byte range)
    Original { start: u32, end: u32 },
    /// Pure insertion — not from original source, no source map mapping
    Inserted { content: &'a str },
    /// Replaced content — maps to the original span for source maps
    Overwritten {
        start: u32,
        end: u32,
        content: &'a str,
    },
    /// Content moved from its original position — maps back to source
    /// line-by-line for accurate source maps
    #[allow(dead_code)] // Used by move_slice() which is test/API only for now
    Moved {
        start: u32,
        end: u32,
        content: &'a str,
    },
}

impl<'a> Chunk<'a> {
    /// Create a chunk referencing the original source
    pub fn from_source(start: u32, end: u32) -> Self {
        Self::Original { start, end }
    }

    /// Create an inserted chunk (not from original source)
    pub fn inserted(content: &'a str) -> Self {
        Self::Inserted { content }
    }

    /// Create an overwritten chunk (replaces original content)
    pub fn overwritten(start: u32, end: u32, content: &'a str) -> Self {
        Self::Overwritten {
            start,
            end,
            content,
        }
    }

    /// Create a moved chunk (content moved from original position, maps back to source)
    #[allow(dead_code)]
    pub fn moved(start: u32, end: u32, content: &'a str) -> Self {
        Self::Moved {
            start,
            end,
            content,
        }
    }

    /// Check if this chunk represents original content
    #[allow(dead_code)]
    pub fn is_original(&self) -> bool {
        matches!(self, Self::Original { .. })
    }
}

#![allow(dead_code)]

/// Represents a piece of the output string
#[derive(Debug, Clone, Copy)]
pub(super) enum Chunk<'a> {
    /// Original content from source (byte range)
    Original { start: u32, end: u32 },
    /// Inserted or overwritten content (allocated in bump allocator)
    Edited {
        /// Original span this replaces (for source maps)
        original_start: Option<u32>,
        original_end: Option<u32>,
        /// The new content (allocated in the bump allocator)
        content: &'a str,
        /// Whether this chunk has already been moved (to prevent double-moving)
        was_moved: bool,
    },
}

impl<'a> Chunk<'a> {
    /// Create a chunk referencing the original source
    pub fn from_source(start: u32, end: u32) -> Self {
        Self::Original { start, end }
    }

    /// Create an inserted chunk (not from original source)
    pub fn inserted(content: &'a str) -> Self {
        Self::Edited {
            original_start: None,
            original_end: None,
            content,
            was_moved: false,
        }
    }

    /// Create an overwritten chunk (replaces original content)
    pub fn overwritten(start: u32, end: u32, content: &'a str) -> Self {
        Self::Edited {
            original_start: Some(start),
            original_end: Some(end),
            content,
            was_moved: false,
        }
    }

    /// Create a moved chunk (content moved from original position, maps back to source)
    pub fn moved(original_start: u32, original_end: u32, content: &'a str) -> Self {
        Self::Edited {
            original_start: Some(original_start),
            original_end: Some(original_end),
            content,
            was_moved: true, // Mark as moved to prevent double-moving
        }
    }

    /// Check if this chunk is empty
    pub fn is_empty(&self, _source: &str) -> bool {
        match self {
            Self::Original { start, end } => start >= end,
            Self::Edited { content, .. } => content.is_empty(),
        }
    }

    /// Get the output length of this chunk
    pub fn output_len(&self, _source: &str) -> usize {
        match self {
            Self::Original { start, end } => (*end - *start) as usize,
            Self::Edited { content, .. } => content.len(),
        }
    }

    /// Get the original span (for source maps)
    pub fn original_span(&self) -> Option<(u32, u32)> {
        match self {
            Self::Original { start, end } => Some((*start, *end)),
            Self::Edited {
                original_start: Some(start),
                original_end: Some(end),
                ..
            } => Some((*start, *end)),
            _ => None,
        }
    }

    /// Check if this chunk was already moved
    pub fn was_moved(&self) -> bool {
        match self {
            Self::Original { .. } => false,
            Self::Edited { was_moved, .. } => *was_moved,
        }
    }

    /// Check if this chunk represents original content
    pub fn is_original(&self) -> bool {
        matches!(self, Self::Original { .. })
    }

    /// Write this chunk to a string buffer
    pub fn write_to(&self, source: &str, buffer: &mut String) {
        match self {
            Self::Original { start, end } => {
                buffer.push_str(&source[*start as usize..*end as usize]);
            }
            Self::Edited { content, .. } => {
                buffer.push_str(content);
            }
        }
    }
}

/// Insertion affinity at a chunk boundary, following the `magic-string`
/// left/right model: at one offset, LEFT-affinity content renders before
/// RIGHT-affinity content, and range edits treat LEFT content as belonging to
/// the chunk ENDING at the offset and RIGHT content as belonging to the chunk
/// STARTING there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InsertAffinity {
    /// Belongs to the chunk ending at the anchor (`appendLeft`).
    Left,
    /// Belongs to the chunk starting at the anchor (`prependRight` /
    /// `appendRight`).
    Right,
}

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
    /// A run of chunks a move took out of its range and placed elsewhere. The members are the chunks the range held when the move
    /// ran, kept by reference in the bump arena: no text is copied, and each
    /// member keeps its own correspondence (authored bytes, replacement,
    /// mapped insertion, segmented anchors). Readers walk the members in
    /// order and treat them as relocated.
    Relocated { members: &'a [Chunk<'a>] },
    /// Inserted content mapped to a specific source position.
    /// Unlike `Inserted` (unmapped), this emits a source map token at `source_start`.
    /// Used for generated code that corresponds to a known source location
    /// (e.g., resolved v-if expression relocated to ternary prefix).
    ///
    /// `content_offset` shifts the source map token within the content. Characters
    /// before `content_offset` are unmapped; the token is placed at `content_offset`,
    /// mapping to `source_start`. This accounts for binding prefixes like `__props.`
    /// or `_ctx.` that precede the original identifier in the resolved expression.
    InsertedMapped {
        content: &'a str,
        source_start: u32,
        content_offset: u32,
    },
    /// Pure insertion attached to a source offset with an explicit boundary
    /// affinity — the `magic-string` insertion model used by the checked
    /// [`CodeTransform::try_append_left`] / [`try_prepend_right`] /
    /// [`try_append_right`] operations. Renders exactly like `Inserted`
    /// (unmapped); the anchor + affinity let the range replacements
    /// (`update` / `overwrite` / `remove`) apply boundary-attachment
    /// semantics: RIGHT-affinity content at a replaced range's start and
    /// LEFT-affinity content at its chunks' end boundaries belong to the
    /// range (cleared by non-content-only edits), while the complementary
    /// affinities belong to the neighboring chunks (always preserved).
    ///
    /// [`CodeTransform::try_append_left`]: super::CodeTransform::try_append_left
    /// [`try_prepend_right`]: super::CodeTransform::try_prepend_right
    /// [`try_append_right`]: super::CodeTransform::try_append_right
    InsertedAnchored {
        content: &'a str,
        anchor: u32,
        affinity: InsertAffinity,
    },
    /// Replaced content carrying MULTIPLE per-offset authored anchors —
    /// produced ONLY by the crate-private, opt-in
    /// [`CodeTransform::try_overwrite_segmented`](super::code_transform::CodeTransform::try_overwrite_segmented).
    /// Unlike `Overwritten` (ONE token at `start`, no character
    /// correspondence — MagicString convention), each entry in `anchors`
    /// is a REAL, checked correspondence: a byte span inside `content`
    /// that is an AUTHORED lexeme copied verbatim, mapping to its own
    /// authored source position. Bytes outside every anchor (including
    /// before the first and after the last) are synthetic scaffolding and
    /// carry no source-map token. See `code_transform/segmented.rs`'s
    /// module doc for the full contract.
    OverwrittenSegmented {
        start: u32,
        end: u32,
        content: &'a str,
        anchors: &'a [super::segmented::SegmentAnchor],
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

    /// Create an inserted chunk mapped to a source position, with a content offset.
    /// Characters before `content_offset` are unmapped; the source map token is
    /// placed at `content_offset`, pointing to `source_start`.
    pub fn inserted_mapped_with_offset(
        content: &'a str,
        source_start: u32,
        content_offset: u32,
    ) -> Self {
        Self::InsertedMapped {
            content,
            source_start,
            content_offset,
        }
    }

    /// Create an affinity-anchored insertion (the `magic-string` model used
    /// by the checked insertion operations).
    pub fn inserted_anchored(content: &'a str, anchor: u32, affinity: InsertAffinity) -> Self {
        Self::InsertedAnchored {
            content,
            anchor,
            affinity,
        }
    }

    /// Create a segmented-overwrite chunk — see `OverwrittenSegmented`'s
    /// own doc comment. Crate-private: only
    /// [`CodeTransform::try_overwrite_segmented`](super::code_transform::CodeTransform::try_overwrite_segmented)
    /// constructs one.
    pub(super) fn overwritten_segmented(
        start: u32,
        end: u32,
        content: &'a str,
        anchors: &'a [super::segmented::SegmentAnchor],
    ) -> Self {
        Self::OverwrittenSegmented {
            start,
            end,
            content,
            anchors,
        }
    }

    /// Check if this chunk represents original content
    #[allow(dead_code)]
    pub fn is_original(&self) -> bool {
        matches!(self, Self::Original { .. })
    }

    /// The number of output bytes this chunk renders.
    pub fn output_len(&self) -> u32 {
        match self {
            Self::Original { start, end } => end - start,
            Self::Relocated { members } => members.iter().map(Chunk::output_len).sum(),
            Self::Inserted { content }
            | Self::Overwritten { content, .. }
            | Self::InsertedMapped { content, .. }
            | Self::InsertedAnchored { content, .. }
            | Self::OverwrittenSegmented { content, .. } => content.len() as u32,
        }
    }
}

/// Depth-first walk over chunks in output order, expanding every
/// [`Chunk::Relocated`] run into its members. Each leaf comes with whether
/// it sits inside a relocated run.
pub(super) struct LeafChunks<'c, 'a> {
    stack: smallvec::SmallVec<[(std::slice::Iter<'c, Chunk<'a>>, bool); 4]>,
}

impl<'c, 'a> LeafChunks<'c, 'a> {
    pub(super) fn new(chunks: &'c [Chunk<'a>]) -> Self {
        let mut stack = smallvec::SmallVec::new();
        stack.push((chunks.iter(), false));
        Self { stack }
    }
}

impl<'c, 'a: 'c> Iterator for LeafChunks<'c, 'a> {
    type Item = (&'c Chunk<'a>, bool);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (iter, relocated) = self.stack.last_mut()?;
            let relocated = *relocated;
            match iter.next() {
                Some(Chunk::Relocated { members }) => {
                    self.stack.push((members.iter(), true));
                }
                Some(chunk) => return Some((chunk, relocated)),
                None => {
                    self.stack.pop();
                }
            }
        }
    }
}

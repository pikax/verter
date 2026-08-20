//! Assembled-module source map: the two authorized script rewrites,
//! fragment placement, boundary segments, and the output artifact.

use std::borrow::Cow;

use verter_compiler::code_transform::{advance_generated_position, CodeTransform};
use verter_compiler::oxc_sourcemap::{SourceMap, Token};

use super::map_input::{DecodedFragmentMap, SourcePayload, WireSegment};

/// Pass one globally replaces this with [`RENAME_TO`].
pub(crate) const RENAME_FROM: &str = "__sfc__";
/// Pass one's replacement. Two bytes longer than what it replaces, which is
/// what makes the rename observable in the map at all.
pub(crate) const RENAME_TO: &str = "_sfc_main";
/// Pass two, on pass one's OUTPUT coordinate space, globally removes this — so
/// the pattern is spelled with pass one's output identifier.
pub(crate) const EXPORT_REMOVAL: &str = "export default _sfc_main;\n";

/// Where a segment entered composition. Attached at ingestion, never
/// inferred from a final coordinate, never serialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SegmentOrigin {
    Script,
    Template,
    AssemblyBoundary,
}

/// A segment in the ASSEMBLED module's coordinate space, with its table indices
/// already remapped into the composed tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AssembledSegment {
    pub(crate) generated_line: u32,
    pub(crate) generated_column: u32,
    pub(crate) payload: Option<SourcePayload>,
    pub(crate) origin: SegmentOrigin,
}

// The two authorized rewrites

/// Every literal, non-overlapping, left-to-right occurrence of `needle` — the
/// same match set `str::replace` visits, so driving the rewrites through
/// `CodeTransform` reproduces the pinned bytes rather than merely resembling
/// them. Matching is deliberately NOT identifier-aware: `___sfc__` contains
/// `__sfc__` at offset 1 and is rewritten, which is the pinned behaviour.
fn literal_occurrences(haystack: &str, needle: &str) -> Vec<u32> {
    let mut found = Vec::new();
    let mut from = 0usize;
    while let Some(relative) = haystack[from..].find(needle) {
        let at = from + relative;
        found.push(at as u32);
        from = at + needle.len();
    }
    found
}

/// Two authorized script rewrites, sequential `CodeTransform`s (pass two
/// over pass one's output). They run whether or not a map was requested —
/// a second map-free rewrite path would be two implementations of one
/// operation.
pub(crate) fn rewrite_script(
    code: &str,
    map: Option<&DecodedFragmentMap>,
) -> (String, Option<Vec<WireSegment>>) {
    let allocator = oxc_allocator::Allocator::default();

    let mut pass_one = CodeTransform::new(code, &allocator);
    for start in literal_occurrences(code, RENAME_FROM) {
        pass_one.overwrite(start, start + RENAME_FROM.len() as u32, RENAME_TO);
    }
    let renamed = pass_one.build_string();

    let mut pass_two = CodeTransform::new(&renamed, &allocator);
    for start in literal_occurrences(&renamed, EXPORT_REMOVAL) {
        pass_two.overwrite(start, start + EXPORT_REMOVAL.len() as u32, "");
    }
    let rewritten = pass_two.build_string();

    let chained = map.map(|map| {
        // Overwrite-only transforms over validated positions: a panic here
        // is a defect, not an input to report.
        let after_one = pass_one
            .chain_source_map(&to_source_map(map))
            .expect("the rename pass is an overwrite-only transform over the validated script");
        let after_two = pass_two.chain_source_map(&after_one).expect(
            "the export-removal pass is an overwrite-only transform over pass one's output",
        );
        segments_of(&after_two)
    });

    (rewritten, chained)
}

/// Lift a decoded map into the typed wire form the chain consumes. Tables ride
/// along untouched; only the segment sequence is what chaining acts on.
fn to_source_map(map: &DecodedFragmentMap) -> SourceMap<'static> {
    let tokens: Vec<Token> = map
        .segments
        .iter()
        .map(|segment| match segment.payload {
            Some(payload) => Token::new(
                segment.generated_line,
                segment.generated_column,
                payload.source_line,
                payload.source_column,
                Some(payload.source_index),
                payload.name_index,
            ),
            None => Token::new(
                segment.generated_line,
                segment.generated_column,
                0,
                0,
                None,
                None,
            ),
        })
        .collect();

    SourceMap::new(
        None,
        map.names
            .iter()
            .map(|name| Cow::Owned(name.clone()))
            .collect(),
        map.source_root.clone().map(Cow::Owned),
        map.sources
            .iter()
            .map(|source| Cow::Owned(source.clone()))
            .collect(),
        map.sources_content
            .as_ref()
            .map(|rows| rows.iter().map(|row| row.clone().map(Cow::Owned)).collect())
            .unwrap_or_default(),
        tokens.into_boxed_slice(),
        None,
    )
}

fn segments_of(map: &SourceMap<'_>) -> Vec<WireSegment> {
    map.get_tokens()
        .map(|token| WireSegment {
            generated_line: token.get_dst_line(),
            generated_column: token.get_dst_col(),
            payload: token.get_source_id().map(|source_index| SourcePayload {
                source_index,
                source_line: token.get_src_line(),
                source_column: token.get_src_col(),
                name_index: token.get_name_id(),
            }),
        })
        .collect()
}

// Placement, boundaries, and the output artifact

/// Accumulates the assembled artifact as the write grammar runs.
#[derive(Debug, Default)]
pub(crate) struct MapComposer {
    sources: Vec<String>,
    names: Vec<String>,
    sources_content: Vec<Option<String>>,
    ignore_list: Vec<u32>,
    segments: Vec<AssembledSegment>,
}

impl MapComposer {
    /// Append table rows and ignore-list entries; return `(source, name)`
    /// bases. Stable append, no dedup: merging identical rows would claim
    /// two independently declared identities are one.
    fn contribute_tables(&mut self, map: &DecodedFragmentMap) -> (u32, u32) {
        let source_base = self.sources.len() as u32;
        let name_base = self.names.len() as u32;

        for (index, source) in map.sources.iter().enumerate() {
            self.sources.push(source.clone());
            // Ignore status is a property of a ROW, not of a path.
            self.sources_content.push(
                map.sources_content
                    .as_ref()
                    .and_then(|rows| rows.get(index))
                    .and_then(|row| row.clone()),
            );
        }
        self.names.extend(map.names.iter().cloned());
        // Step 1.23 already bound-checked against this fragment's table;
        // narrowing to `u32` is exact. Binary64 storage is for earlier
        // type/agreement checks only.
        self.ignore_list.extend(
            map.ignore_list
                .iter()
                .map(|entry| *entry as u32 + source_base),
        );

        (source_base, name_base)
    }

    /// Place one fragment's chained segments at the write cursor its first byte
    /// was written at.
    ///
    /// A segment on the fragment's first line is offset by BOTH the placement
    /// line and column; every later line keeps its own column, because the
    /// fragment's own newline started that line.
    fn place(
        &mut self,
        segments: &[WireSegment],
        placement: (u32, u32),
        origin: SegmentOrigin,
        source_base: u32,
        name_base: u32,
    ) {
        let (line_offset, column_offset) = placement;
        for segment in segments {
            self.segments.push(AssembledSegment {
                generated_line: segment.generated_line + line_offset,
                generated_column: if segment.generated_line == 0 {
                    segment.generated_column + column_offset
                } else {
                    segment.generated_column
                },
                payload: segment.payload.map(|payload| SourcePayload {
                    source_index: payload.source_index + source_base,
                    source_line: payload.source_line,
                    source_column: payload.source_column,
                    name_index: payload.name_index.map(|index| index + name_base),
                }),
                origin,
            });
        }
    }

    /// Fragment-end boundary. Condition is "final code ends with a newline",
    /// not "end cursor column is zero" — they disagree on an empty present
    /// fragment (cursor at column 0, newline patch fires). Firing there
    /// would shadow a carried authored segment. When the code does end with
    /// a newline, the sourceless boundary (after every fragment segment)
    /// stops assembly-owned bytes from inheriting that line's authored
    /// position.
    fn boundary(&mut self, at_line: u32) {
        self.segments.push(AssembledSegment {
            generated_line: at_line,
            generated_column: 0,
            payload: None,
            origin: SegmentOrigin::AssemblyBoundary,
        });
    }

    #[cfg(test)]
    pub(crate) fn segments(&self) -> &[AssembledSegment] {
        &self.segments
    }

    /// Serialize the composed artifact.
    ///
    /// `file` and `debugId` describe the GENERATED document, which no longer
    /// exists once fragments have been assembled into a different module, so
    /// both are absent — inheriting a fragment's would be a false claim and
    /// minting a new one would create a contract this layer does not own.
    /// Unknown members are dropped for the same reason. `sourceRoot` and the
    /// ignore list describe the SOURCES table and are carried.
    fn finish(self, source_root: Option<String>) -> String {
        let tokens: Vec<Token> = self
            .segments
            .iter()
            .map(|segment| match segment.payload {
                Some(payload) => Token::new(
                    segment.generated_line,
                    segment.generated_column,
                    payload.source_line,
                    payload.source_column,
                    Some(payload.source_index),
                    payload.name_index,
                ),
                None => Token::new(
                    segment.generated_line,
                    segment.generated_column,
                    0,
                    0,
                    None,
                    None,
                ),
            })
            .collect();

        let mut artifact = SourceMap::new(
            None,
            self.names.into_iter().map(Cow::Owned).collect(),
            source_root.map(Cow::Owned),
            self.sources.into_iter().map(Cow::Owned).collect(),
            self.sources_content
                .into_iter()
                .map(|content| content.map(Cow::Owned))
                .collect(),
            tokens.into_boxed_slice(),
            None,
        );
        // Present iff non-empty; the encoder emits `sourcesContent` iff some
        // row carries content, which is the same rule.
        if !self.ignore_list.is_empty() {
            artifact.set_x_google_ignore_list(self.ignore_list);
        }
        artifact.to_json_string()
    }
}

/// Assembled bytes plus the write cursor, derived as the assembler writes
/// — never supplied, scanned, or reconstructed. Same primitive as the map
/// walk, so the coordinate spaces cannot disagree.
pub(crate) struct ModuleWriter {
    out: String,
    line: u32,
    column: u32,
}

impl ModuleWriter {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            out: String::with_capacity(capacity),
            line: 0,
            column: 0,
        }
    }

    pub(crate) fn push_str(&mut self, text: &str) {
        advance_generated_position(text, &mut self.line, &mut self.column);
        self.out.push_str(text);
    }

    pub(crate) fn push(&mut self, character: char) {
        let mut buffer = [0u8; 4];
        self.push_str(character.encode_utf8(&mut buffer));
    }

    /// The generated position of the next byte to be written.
    pub(crate) fn cursor(&self) -> (u32, u32) {
        (self.line, self.column)
    }

    pub(crate) fn into_string(self) -> String {
        self.out
    }
}

impl std::fmt::Write for ModuleWriter {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.push_str(text);
        Ok(())
    }
}

/// One fragment's contribution, written and mapped together.
pub(crate) struct FragmentWrite<'a> {
    pub(crate) code: &'a str,
    /// `Some` only for a CONTRIBUTING map — a fragment that is present AND
    /// carries a non-empty map that passed validation.
    pub(crate) chained: Option<&'a [WireSegment]>,
    pub(crate) map: Option<&'a DecodedFragmentMap>,
    pub(crate) origin: SegmentOrigin,
}

impl MapComposer {
    /// Write one fragment's code. A present fragment with no map still
    /// writes bytes but contributes nothing to the map — a validated-empty
    /// sequence would sprout sourceless segments that cannot change lookup.
    pub(crate) fn write_fragment(
        &mut self,
        writer: &mut ModuleWriter,
        fragment: FragmentWrite<'_>,
    ) {
        let placement = writer.cursor();
        writer.push_str(fragment.code);

        let (Some(chained), Some(map)) = (fragment.chained, fragment.map) else {
            return;
        };

        let (source_base, name_base) = self.contribute_tables(map);
        self.place(chained, placement, fragment.origin, source_base, name_base);

        if fragment.code.ends_with('\n') {
            let (line, column) = writer.cursor();
            debug_assert_eq!(
                column, 0,
                "code ending in a newline leaves the cursor at column 0"
            );
            self.boundary(line);
        }
    }

    pub(crate) fn into_artifact(self, source_root: Option<String>) -> String {
        self.finish(source_root)
    }
}

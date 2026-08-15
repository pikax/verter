//! Composing the assembled module's source map: the two authorized script
//! rewrites, fragment placement, boundary segments, and the output artifact.

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

/// Where a segment entered the composition.
///
/// Composition-time bookkeeping only. The tag is attached at INGESTION, rides
/// every later operation, is never inferred from a final coordinate or a
/// spelling, and is never serialized: no member of the emitted artifact carries
/// it.
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

// ── The two authorized rewrites ────────────────────────────────────────────

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

/// Apply the two authorized script rewrites and, when the fragment contributes
/// a map, chain that map through both passes.
///
/// The passes are real `CodeTransform` transforms applied SEQUENTIALLY, pass
/// two over pass one's output coordinate space, each driving both the output
/// code and the output map from one chunk list. They run whether or not a map
/// was requested, because they determine the module's BYTES and the code
/// baseline is pinned regardless of any map — routing the bytes through a
/// second, map-free rewrite path would be two implementations of one operation
/// with nothing forcing them to agree.
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
        // Both refusals are excluded by construction at this one call site:
        // each transform is built from `overwrite` alone over its own input, so
        // it holds only retained and replaced runs and carries no intro or
        // outro; pass one's segment positions were validated in bounds against
        // this exact code, and pass two's input positions are ones pass one's
        // own walk emitted. A panic here would mean one of those stopped being
        // true, which is a defect to fix rather than an input to report.
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

// ── Placement, boundaries, and the output artifact ─────────────────────────

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
    /// Append one contributing map's table rows and ignore-list entries, and
    /// return the `(source, name)` base offsets its segments must shift by.
    ///
    /// Tables are a STABLE APPEND in contribution order with NO deduplication:
    /// two fragments declaring the same spelling contribute two rows, and a row
    /// no segment references is still contributed. Merging identical rows would
    /// be an assertion that two independently declared identities are one, and
    /// declared identities are carried opaquely. Confronted with two rows that
    /// differ only in ignore status, a merging rule would have to union the
    /// flags, intersect them, or decline — and only declining publishes nothing
    /// beyond what an input declared.
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
        // Each entry already passed step 1.23's bound check against this
        // fragment's own (small) `sources` table, so narrowing to `u32` here
        // is exact even before adding the running `source_base` offset — the
        // binary64 storage on `DecodedFragmentMap` exists only so the earlier
        // type/agreement checks and the bound itself run at full binary64
        // identity; a validated entry is always a small integral value.
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

    /// Emit the fragment-end boundary segment at a fragment's transition out of
    /// mapped content.
    ///
    /// The condition is that the fragment's FINAL code ends with a newline —
    /// equivalently, that its newline patch does not fire. It is deliberately
    /// NOT "the end cursor column is zero": those disagree on a real, legal
    /// input. A present fragment whose code is empty leaves the cursor at
    /// column 0 while its newline patch DOES fire, terminating a line that
    /// contains no characters at all, so the next assembly-owned write begins
    /// on the following line and there is nothing on the fragment's line to
    /// protect. Firing there would be worse than redundant: the boundary would
    /// land on the same coordinate as the fragment's own carried segment and,
    /// being placed after it, would shadow a faithfully composed authored
    /// position.
    ///
    /// When the code DOES end with a newline, the fragment's trailing empty
    /// line is where the next assembly-owned write begins — and a segment can
    /// legitimately sit there, at column 0, the only in-bounds column an empty
    /// line has. Without this boundary such a segment would answer for every
    /// column of that line, including the ones assembly-owned bytes occupy,
    /// giving synthetic scaffolding an authored position. The boundary is
    /// sourceless and is placed AFTER every segment of the fragment it bounds,
    /// so the last-applicable lookup resolves to it across that whole line.
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

/// The module writer: the assembled bytes plus the write cursor they advanced.
///
/// The cursor is DERIVED as the assembler writes. It is never supplied as an
/// input, never recovered by scanning the generated output, and never
/// reconstructed by concatenating code and computing offsets afterward — so a
/// fragment's placement cannot drift from the write grammar that produced it.
/// The advance is the same primitive the map walk uses, so the two coordinate
/// spaces cannot disagree.
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
    /// Write one mapped fragment's code and compose its contribution.
    ///
    /// A present fragment carrying no map still has its code written — the
    /// passes determine the module's bytes — but contributes NOTHING to the
    /// map: no carried segments, no replacement or resume segments, no table
    /// rows, no ignore-list entries, and no boundary segment. Treating its map
    /// as a validated-but-empty sequence would be a different rule with a
    /// different result, sprouting a sourceless segment at every position where
    /// a lookup came up empty — segments that could never change what any
    /// position resolves to.
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

//! Fragment placement/splice composition — the typed replacement for the
//! former `framework_common::generated_chunk` engine. Same position math
//! (byte offset -> line/column, table composition into one source map),
//! driven by [`ValidatedFragment`] + [`PlacementSlot::Hole`] instead of a
//! raw `Range<u32>` plus ad hoc `&str` source-space tags — the owner/hole
//! relationship is checked against the fragment's OWN declared placement,
//! never re-derived by scanning generated text for a landmark.

use oxc_sourcemap::{SourceMap, SourceMapBuilder, Token};

use crate::code_transform::advance_generated_position;

use super::fragment::{FragmentId, PlacementSlot, ValidatedFragment};
use super::source_space::AssembledOffset;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedOutput {
    pub code: String,
    pub source_map: String,
    /// The ASSEMBLED-space byte offset at which the primary contributed
    /// fragment's own bytes begin in [`Self::code`] — `prepend_preamble`'s
    /// prepended-to fragment, `splice_into_hole`'s spliced-in fragment.
    /// Typed so a caller cannot mistake this for a fragment- or
    /// original-space offset.
    pub fragment_starts_at: AssembledOffset,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeRefusal {
    /// `fragment`'s own declared placement is not a `Hole` owned by
    /// `owner_id` — assembly fails closed rather than guessing a splice
    /// point.
    NotAHolePlacement,
    /// A declared byte offset does not land on a UTF-8 boundary of the
    /// relevant fragment's own code.
    InvalidBoundary,
    /// A fragment's declared source map failed to decode.
    UncomposableMap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    line: u32,
    column: u32,
}

fn byte_position(code: &str, offset: u32) -> Option<Position> {
    let offset = usize::try_from(offset).ok()?;
    if offset > code.len() || !code.is_char_boundary(offset) {
        return None;
    }
    let mut position = Position { line: 0, column: 0 };
    for character in code[..offset].chars() {
        if character == '\n' {
            position.line += 1;
            position.column = 0;
        } else {
            position.column += character.len_utf16() as u32;
        }
    }
    Some(position)
}

fn relative(position: Position, origin: Position) -> Option<Position> {
    (position >= origin).then(|| {
        if position.line == origin.line {
            Position {
                line: 0,
                column: position.column - origin.column,
            }
        } else {
            Position {
                line: position.line - origin.line,
                column: position.column,
            }
        }
    })
}

fn append(origin: Position, relative: Position) -> Position {
    if relative.line == 0 {
        Position {
            line: origin.line,
            column: origin.column + relative.column,
        }
    } else {
        Position {
            line: origin.line + relative.line,
            column: relative.column,
        }
    }
}

fn token_position(token: Token) -> Position {
    Position {
        line: token.get_dst_line(),
        column: token.get_dst_col(),
    }
}

/// Per-decoded-map lookup tables into the composed builder's OWN `sources`/
/// `names` tables — built once per contributing map so every token from that
/// map remaps to the right composed row. Never a synthetic placeholder
/// source: every row here is copied verbatim from the map being composed.
struct CopiedTables {
    source_ids: Vec<u32>,
    name_ids: Vec<u32>,
}

fn copy_tables(builder: &mut SourceMapBuilder, decoded: &SourceMap<'_>) -> CopiedTables {
    // `get_source_content(i)` (index lookup, `None` on a short/absent
    // `sourcesContent` table) rather than zipping `get_sources()` with
    // `get_source_contents()` — the two iterators are NOT guaranteed the
    // same length (an input map with no `sourcesContent` member decodes to
    // a zero-length content table, which would silently truncate a zip to
    // zero pairs and drop every source).
    let source_ids = decoded
        .get_sources()
        .enumerate()
        .map(|(index, source)| {
            let content = decoded.get_source_content(index as u32).unwrap_or("");
            builder.add_source_and_content(source, content)
        })
        .collect();
    let name_ids = decoded
        .get_names()
        .map(|name| builder.add_name(name))
        .collect();
    CopiedTables {
        source_ids,
        name_ids,
    }
}

/// `decoded`'s own ignore-list entries, remapped through `tables`' source
/// index table — an ignore-listed source stays ignore-listed after
/// composition instead of silently losing that marker.
fn copied_ignore_list(decoded: &SourceMap<'_>, tables: &CopiedTables) -> Vec<u32> {
    decoded
        .get_x_google_ignore_list()
        .unwrap_or(&[])
        .iter()
        .filter_map(|&index| tables.source_ids.get(index as usize).copied())
        .collect()
}

fn add_token(
    builder: &mut SourceMapBuilder,
    token: Token,
    position: Position,
    tables: &CopiedTables,
) {
    let source_id = token
        .get_source_id()
        .and_then(|id| tables.source_ids.get(id as usize).copied());
    let name_id = token
        .get_name_id()
        .and_then(|id| tables.name_ids.get(id as usize).copied());
    builder.add_token(
        position.line,
        position.column,
        token.get_src_line(),
        token.get_src_col(),
        source_id,
        name_id,
    );
}

/// Prepend `preamble` (assembly-owned bytes with no source mapping of
/// their own — e.g. a synthesized `import { ... } from "..."` line) ahead
/// of `code`, shifting `source_map`'s segments into the resulting
/// coordinate space so the map keeps describing the same bytes it
/// described before the prepend. `source_map` is decoded and re-encoded
/// once here rather than left to silently drift out of sync with `code` —
/// exactly the failure mode of concatenating text onto already-mapped
/// bytes with a bare `format!`.
pub fn prepend_preamble(
    preamble: &str,
    code: &str,
    source_map: Option<&str>,
) -> Result<ComposedOutput, ComposeRefusal> {
    let output_origin =
        byte_position(preamble, preamble.len() as u32).ok_or(ComposeRefusal::InvalidBoundary)?;

    let mut composed_code = String::with_capacity(preamble.len() + code.len());
    composed_code.push_str(preamble);
    composed_code.push_str(code);

    let decoded = source_map
        .map(SourceMap::from_json_string)
        .transpose()
        .map_err(|_| ComposeRefusal::UncomposableMap)?;

    let mut builder = SourceMapBuilder::default();
    let mut ignore_list = Vec::new();
    if let Some(decoded) = &decoded {
        let tables = copy_tables(&mut builder, decoded);
        for token in decoded.get_tokens() {
            let position = token_position(token);
            add_token(
                &mut builder,
                token,
                append(output_origin, position),
                &tables,
            );
        }
        ignore_list = copied_ignore_list(decoded, &tables);
    }

    let mut result = builder.into_sourcemap();
    if !ignore_list.is_empty() {
        result.set_x_google_ignore_list(ignore_list);
    }

    Ok(ComposedOutput {
        code: composed_code,
        source_map: result.to_json_string(),
        fragment_starts_at: AssembledOffset(preamble.len() as u32),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequencedOutput {
    pub code: String,
    pub source_map: String,
}

/// Table accumulator for [`assemble_sequence`] — STABLE APPEND, no dedup.
/// Unlike [`copy_tables`] (used by `prepend_preamble`/`splice_into_hole`,
/// which have no "one row per contributing fragment" contract to honor),
/// two fragments that happen to declare the identical source path are two
/// independently declared identities, not one: collapsing them would
/// destroy the one-to-one row-to-fragment attribution a consumer's lookup
/// depends on. `SourceMapBuilder::add_source_and_content`/`add_name`
/// dedupe by string identity, so this accumulator bypasses the builder
/// entirely and constructs the final [`SourceMap`] directly.
#[derive(Default)]
struct SequenceTables {
    sources: Vec<String>,
    names: Vec<String>,
    sources_content: Vec<Option<String>>,
    ignore_list: Vec<u32>,
    tokens: Vec<Token>,
}

impl SequenceTables {
    /// Append one fragment's tables unconditionally; returns the
    /// `(source_base, name_base)` this fragment's own indices shift by.
    fn contribute(&mut self, decoded: &SourceMap<'_>) -> (u32, u32) {
        let source_base = self.sources.len() as u32;
        let name_base = self.names.len() as u32;
        for (index, source) in decoded.get_sources().enumerate() {
            self.sources.push(source.to_owned());
            self.sources_content
                .push(decoded.get_source_content(index as u32).map(str::to_owned));
        }
        self.names.extend(decoded.get_names().map(str::to_owned));
        self.ignore_list.extend(
            decoded
                .get_x_google_ignore_list()
                .unwrap_or(&[])
                .iter()
                .map(|entry| entry + source_base),
        );
        (source_base, name_base)
    }

    fn push_token(&mut self, token: Token, position: Position, source_base: u32, name_base: u32) {
        let source_id = token.get_source_id().map(|id| id + source_base);
        let (src_line, src_col, name_id) = match source_id {
            Some(_) => (
                token.get_src_line(),
                token.get_src_col(),
                token.get_name_id().map(|id| id + name_base),
            ),
            None => (0, 0, None),
        };
        self.tokens.push(Token::new(
            position.line,
            position.column,
            src_line,
            src_col,
            source_id,
            name_id,
        ));
    }

    fn push_boundary(&mut self, at_line: u32) {
        self.tokens.push(Token::new(at_line, 0, 0, 0, None, None));
    }

    fn finish(self, source_root: Option<&str>) -> SourceMap<'static> {
        let mut map = SourceMap::new(
            None,
            self.names
                .into_iter()
                .map(std::borrow::Cow::Owned)
                .collect(),
            source_root.map(|s| std::borrow::Cow::Owned(s.to_owned())),
            self.sources
                .into_iter()
                .map(std::borrow::Cow::Owned)
                .collect(),
            self.sources_content
                .into_iter()
                .map(|c| c.map(std::borrow::Cow::Owned))
                .collect(),
            self.tokens.into_boxed_slice(),
            None,
        );
        if !self.ignore_list.is_empty() {
            map.set_x_google_ignore_list(self.ignore_list);
        }
        map
    }
}

/// Write `fragments` in order into one module, composing every
/// contributing map into the result's coordinate space — the general
/// N-fragment placement engine multi-fragment module assembly (Vue main-
/// module assembly among its callers) is built on, replacing the former
/// session-private `MapComposer`/`ModuleWriter`/`FragmentWrite` (same
/// algorithm: stable no-dedup table append, and a sourceless BOUNDARY
/// segment follows each mapped fragment whose own code ends in a newline,
/// so trailing assembly-owned bytes never inherit that fragment's last
/// authored position).
///
/// Takes [`ValidatedFragment`] references exclusively — there is no raw
/// `{code, source_map}` overload. A caller cannot sequence a fragment whose
/// bytes were never proven to parse under its own declared contract; see
/// `tests/cases/compile-fail/assemble_sequence_requires_validated_fragment.rs`
/// for the compile-time proof.
pub fn assemble_sequence(
    fragments: &[&ValidatedFragment],
    source_root: Option<&str>,
) -> Result<SequencedOutput, ComposeRefusal> {
    let mut code = String::new();
    let mut cursor = Position { line: 0, column: 0 };
    let mut tables = SequenceTables::default();

    for fragment in fragments {
        let f = fragment.fragment();
        let placement = cursor;
        advance_generated_position(&f.code, &mut cursor.line, &mut cursor.column);
        code.push_str(&f.code);

        let Some(source_map) = f.source_map.as_deref().filter(|m| !m.is_empty()) else {
            continue;
        };
        let decoded =
            SourceMap::from_json_string(source_map).map_err(|_| ComposeRefusal::UncomposableMap)?;
        let (source_base, name_base) = tables.contribute(&decoded);
        for token in decoded.get_tokens() {
            let position = append(placement, token_position(token));
            tables.push_token(token, position, source_base, name_base);
        }
        if f.code.ends_with('\n') {
            // Boundary: a sourceless segment at the write cursor AFTER
            // this fragment, so a following fragment's assembly-owned
            // bytes (or a plain literal one) do not inherit this
            // fragment's trailing authored position.
            tables.push_boundary(cursor.line);
        }
    }

    let result = tables.finish(source_root);

    Ok(SequencedOutput {
        code,
        source_map: result.to_json_string(),
    })
}

/// Splice `fragment`'s bytes into the hole its own declared
/// [`PlacementSlot`] names inside `owner`'s bytes (a hole this call
/// confirms is owned by `owner_id`), composing both maps into the
/// result's coordinate space. No authored input is concatenated or
/// reparsed — both sides already passed their native fragment producer.
pub fn splice_into_hole(
    preamble: &str,
    owner_id: FragmentId,
    owner: &ValidatedFragment,
    fragment: &ValidatedFragment,
) -> Result<ComposedOutput, ComposeRefusal> {
    let hole = match &fragment.fragment().placement {
        PlacementSlot::Hole {
            owner: declared_owner,
            hole,
        } if *declared_owner == owner_id => hole.range.clone(),
        _ => return Err(ComposeRefusal::NotAHolePlacement),
    };

    let owner_fragment = owner.fragment();
    let inserted_fragment = fragment.fragment();
    let shell_code = owner_fragment.code.as_str();
    let fragment_code = inserted_fragment.code.as_str();

    let output_origin =
        byte_position(preamble, preamble.len() as u32).ok_or(ComposeRefusal::InvalidBoundary)?;
    let hole_start =
        byte_position(shell_code, hole.start).ok_or(ComposeRefusal::InvalidBoundary)?;
    let hole_end = byte_position(shell_code, hole.end).ok_or(ComposeRefusal::InvalidBoundary)?;
    let fragment_text = fragment_code;
    let fragment_start = byte_position(fragment_code, 0).ok_or(ComposeRefusal::InvalidBoundary)?;
    let fragment_end = byte_position(fragment_code, fragment_code.len() as u32)
        .ok_or(ComposeRefusal::InvalidBoundary)?;
    let inserted_text = format!("\n{fragment_text}\n");
    let fragment_origin = append(hole_start, Position { line: 1, column: 0 });
    let inserted_end = append(
        hole_start,
        byte_position(&inserted_text, inserted_text.len() as u32)
            .ok_or(ComposeRefusal::InvalidBoundary)?,
    );

    let mut code = String::with_capacity(
        preamble.len() + shell_code.len() - (hole.end - hole.start) as usize + inserted_text.len(),
    );
    code.push_str(preamble);
    code.push_str(
        shell_code
            .get(..hole.start as usize)
            .ok_or(ComposeRefusal::InvalidBoundary)?,
    );
    code.push_str(&inserted_text);
    code.push_str(
        shell_code
            .get(hole.end as usize..)
            .ok_or(ComposeRefusal::InvalidBoundary)?,
    );

    let shell_map = owner_fragment
        .source_map
        .as_deref()
        .map(SourceMap::from_json_string)
        .transpose()
        .map_err(|_| ComposeRefusal::UncomposableMap)?;
    let fragment_map = inserted_fragment
        .source_map
        .as_deref()
        .map(SourceMap::from_json_string)
        .transpose()
        .map_err(|_| ComposeRefusal::UncomposableMap)?;

    let mut builder = SourceMapBuilder::default();
    let shell_tables = shell_map
        .as_ref()
        .map(|shell_map| copy_tables(&mut builder, shell_map));
    let fragment_tables = fragment_map
        .as_ref()
        .map(|fragment_map| copy_tables(&mut builder, fragment_map));

    if let (Some(shell_map), Some(shell_tables)) = (&shell_map, &shell_tables) {
        for token in shell_map.get_tokens() {
            let position = token_position(token);
            if position < hole_start {
                add_token(
                    &mut builder,
                    token,
                    append(output_origin, position),
                    shell_tables,
                );
            }
        }
    }

    if let (Some(fragment_map), Some(fragment_tables)) = (&fragment_map, &fragment_tables) {
        for token in fragment_map.get_tokens() {
            let position = token_position(token);
            if position >= fragment_start && position < fragment_end {
                let rebased = append(
                    output_origin,
                    append(
                        fragment_origin,
                        relative(position, fragment_start)
                            .ok_or(ComposeRefusal::InvalidBoundary)?,
                    ),
                );
                add_token(&mut builder, token, rebased, fragment_tables);
            }
        }
    }

    if let (Some(shell_map), Some(shell_tables)) = (&shell_map, &shell_tables) {
        for token in shell_map.get_tokens() {
            let position = token_position(token);
            if position >= hole_end {
                let rebased = append(
                    output_origin,
                    append(
                        inserted_end,
                        relative(position, hole_end).ok_or(ComposeRefusal::InvalidBoundary)?,
                    ),
                );
                add_token(&mut builder, token, rebased, shell_tables);
            }
        }
    }

    let mut ignore_list = Vec::new();
    if let (Some(shell_map), Some(shell_tables)) = (&shell_map, &shell_tables) {
        ignore_list.extend(copied_ignore_list(shell_map, shell_tables));
    }
    if let (Some(fragment_map), Some(fragment_tables)) = (&fragment_map, &fragment_tables) {
        ignore_list.extend(copied_ignore_list(fragment_map, fragment_tables));
    }
    let mut result = builder.into_sourcemap();
    if !ignore_list.is_empty() {
        result.set_x_google_ignore_list(ignore_list);
    }

    // The inserted fragment's own bytes begin one byte after
    // `inserted_text`'s leading `\n` — see its construction above.
    let fragment_starts_at = AssembledOffset(preamble.len() as u32 + hole.start + 1);

    Ok(ComposedOutput {
        code,
        source_map: result.to_json_string(),
        fragment_starts_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::fragment::{
        Fragment, FragmentDialect, FrameworkDomain, SourceSpaceKind, SyntacticContract,
    };
    use crate::assembly::source_unit::SourceUnitId;
    use crate::compile_request::ProductKind;

    fn unit(tag: &str) -> SourceUnitId {
        struct Tag<'a>(&'a str);
        impl verter_identity::encoding::CanonicalEncode for Tag<'_> {
            const DOMAIN_TAG: &'static str = "verter.compiler.assembly.compose.test.tag.v1";
            fn encode_fields(&self, e: &mut verter_identity::encoding::CanonicalEncoder) {
                e.field_str(1, self.0);
            }
        }
        SourceUnitId::from_canonical(&Tag(tag))
    }

    fn owner_fragment(id: FragmentId, code: &str) -> ValidatedFragment {
        let _ = id;
        Fragment {
            domain: FrameworkDomain::Vue,
            product: ProductKind::RuntimeClient,
            source_unit: unit("owner"),
            source_space: SourceSpaceKind::GeneratedFragment,
            placement: super::super::fragment::PlacementSlot::ModuleBody,
            contract: SyntacticContract::CompleteModule,
            dialect: FragmentDialect::Tsx,
            code: code.to_string(),
            source_map: None,
            imports: Vec::new(),
            exports: Vec::new(),
            helpers: Vec::new(),
            dependencies: Vec::new(),
        }
        .validate()
        .expect("owner fixture parses")
    }

    fn hole_fragment(
        owner_id: FragmentId,
        hole: std::ops::Range<u32>,
        code: &str,
    ) -> ValidatedFragment {
        Fragment {
            domain: FrameworkDomain::Vue,
            product: ProductKind::RuntimeClient,
            source_unit: unit("piece"),
            source_space: SourceSpaceKind::GeneratedFragment,
            placement: super::super::fragment::PlacementSlot::Hole {
                owner: owner_id,
                hole: super::super::source_space::FragmentRange {
                    fragment: owner_id,
                    range: hole,
                },
            },
            contract: SyntacticContract::Expression,
            dialect: FragmentDialect::Tsx,
            code: code.to_string(),
            source_map: None,
            imports: Vec::new(),
            exports: Vec::new(),
            helpers: Vec::new(),
            dependencies: Vec::new(),
        }
        .validate()
        .expect("hole fixture parses")
    }

    #[test]
    fn splices_fragment_bytes_into_the_declared_hole() {
        let owner_id = FragmentId(0);
        let owner = owner_fragment(owner_id, "const x = /*HOLE*/0/*END*/;");
        let hole_start = owner.fragment().code.find("0/*END*/").unwrap() as u32;
        let hole_end = hole_start + 1;
        let piece = hole_fragment(owner_id, hole_start..hole_end, "42");

        let composed = splice_into_hole("", owner_id, &owner, &piece).expect("splice succeeds");
        assert!(
            composed.code.contains("42"),
            "spliced output must contain the inserted fragment's bytes, got:\n{}",
            composed.code
        );
        assert!(
            !composed.code.contains("const x = /*HOLE*/0/*END*/;"),
            "the hole's original bytes must be replaced, got:\n{}",
            composed.code
        );
    }

    #[test]
    fn splice_into_hole_preserves_both_sides_original_source_identity() {
        let owner_id = FragmentId(0);
        let mut owner_map_builder = SourceMapBuilder::default();
        let owner_source = owner_map_builder.add_source_and_content("owner.vue", "owner text");
        owner_map_builder.add_token(0, 6, 1, 6, Some(owner_source), None);
        let owner_map = owner_map_builder.into_sourcemap().to_json_string();

        let mut piece_map_builder = SourceMapBuilder::default();
        let piece_source = piece_map_builder.add_source_and_content("piece.vue", "piece text");
        piece_map_builder.add_token(0, 0, 3, 1, Some(piece_source), None);
        let piece_map = piece_map_builder.into_sourcemap().to_json_string();

        let owner = Fragment {
            domain: FrameworkDomain::Vue,
            product: ProductKind::RuntimeClient,
            source_unit: unit("owner"),
            source_space: SourceSpaceKind::GeneratedFragment,
            placement: super::super::fragment::PlacementSlot::ModuleBody,
            contract: SyntacticContract::CompleteModule,
            dialect: FragmentDialect::Tsx,
            code: "const x = /*HOLE*/0/*END*/;".to_string(),
            source_map: Some(owner_map),
            imports: Vec::new(),
            exports: Vec::new(),
            helpers: Vec::new(),
            dependencies: Vec::new(),
        }
        .validate()
        .expect("owner fixture parses");
        let hole_start = owner.fragment().code.find("0/*END*/").unwrap() as u32;
        let hole_end = hole_start + 1;
        let piece = Fragment {
            domain: FrameworkDomain::Vue,
            product: ProductKind::RuntimeClient,
            source_unit: unit("piece"),
            source_space: SourceSpaceKind::GeneratedFragment,
            placement: super::super::fragment::PlacementSlot::Hole {
                owner: owner_id,
                hole: super::super::source_space::FragmentRange {
                    fragment: owner_id,
                    range: hole_start..hole_end,
                },
            },
            contract: SyntacticContract::Expression,
            dialect: FragmentDialect::Tsx,
            code: "42".to_string(),
            source_map: Some(piece_map),
            imports: Vec::new(),
            exports: Vec::new(),
            helpers: Vec::new(),
            dependencies: Vec::new(),
        }
        .validate()
        .expect("piece fixture parses");

        let composed = splice_into_hole("", owner_id, &owner, &piece).expect("splice succeeds");
        let decoded = SourceMap::from_json_string(&composed.source_map).unwrap();
        let sources: Vec<&str> = decoded.get_sources().collect();
        assert!(
            sources.contains(&"owner.vue") && sources.contains(&"piece.vue"),
            "both contributing fragments' real source identities must survive splicing, \
             never a debug-formatted or synthetic placeholder, got: {sources:?}"
        );
        let contents: Vec<Option<&str>> = decoded.get_source_contents().collect();
        assert!(
            contents.contains(&Some("owner text")) && contents.contains(&Some("piece text")),
            "both fragments' sourcesContent must survive, got: {contents:?}"
        );
    }

    #[test]
    fn refuses_a_fragment_whose_placement_names_a_different_owner() {
        let owner_id = FragmentId(0);
        let owner = owner_fragment(owner_id, "const x = 0;");
        // Declares itself owned by fragment 99, not the real owner (0).
        let piece = hole_fragment(FragmentId(99), 10..11, "42");
        let err = splice_into_hole("", owner_id, &owner, &piece).unwrap_err();
        assert_eq!(err, ComposeRefusal::NotAHolePlacement);
    }

    #[test]
    fn refuses_a_fragment_whose_placement_is_not_a_hole_at_all() {
        let owner_id = FragmentId(0);
        let owner = owner_fragment(owner_id, "const x = 0;");
        let piece = Fragment {
            domain: FrameworkDomain::Vue,
            product: ProductKind::RuntimeClient,
            source_unit: unit("piece"),
            source_space: SourceSpaceKind::GeneratedFragment,
            placement: super::super::fragment::PlacementSlot::ModuleBody,
            contract: SyntacticContract::Expression,
            dialect: FragmentDialect::Tsx,
            code: "42".to_string(),
            source_map: None,
            imports: Vec::new(),
            exports: Vec::new(),
            helpers: Vec::new(),
            dependencies: Vec::new(),
        }
        .validate()
        .expect("piece fixture parses");
        let err = splice_into_hole("", owner_id, &owner, &piece).unwrap_err();
        assert_eq!(err, ComposeRefusal::NotAHolePlacement);
    }

    // ── prepend_preamble ────────────────────────────────────────────

    #[test]
    fn prepend_preamble_keeps_the_map_pointing_at_the_same_authored_position() {
        // One segment at generated (0, 6) — the first byte of `n` in
        // `const n = 1`.
        let code = "const n = 1";
        let map = "{\"version\":3,\"sources\":[\"Comp.vue\"],\"names\":[],\"mappings\":\"MACM\"}";
        let preamble = "import { ref } from \"vue\"\n";

        let composed =
            prepend_preamble(preamble, code, Some(map)).expect("prepend composes cleanly");
        assert_eq!(composed.code, format!("{preamble}{code}"));

        let decoded = SourceMap::from_json_string(&composed.source_map).unwrap();
        let token = decoded
            .get_tokens()
            .next()
            .expect("the composed map carries the shifted segment");
        // The preamble is exactly one line, so the segment's generated
        // LINE advances by one and its column is unchanged (the preamble
        // ends in a newline, landing at column 0).
        assert_eq!(
            token.get_dst_line(),
            1,
            "segment must move to line 1 after a one-line preamble"
        );
        assert_eq!(token.get_dst_col(), 6, "segment's column must be unchanged");
        assert_eq!(
            token.get_src_line(),
            1,
            "the AUTHORED position the segment points at must not change"
        );
        assert_eq!(token.get_src_col(), 6);
    }

    #[test]
    fn prepend_preamble_with_no_map_produces_no_segments() {
        let composed = prepend_preamble("import x from 'y'\n", "const a = 1", None)
            .expect("prepend without a map still composes");
        let decoded = SourceMap::from_json_string(&composed.source_map).unwrap();
        assert_eq!(
            decoded.get_tokens().count(),
            0,
            "no input map means no segments — never a fabricated empty-but-present map with content"
        );
    }

    /// The multi-line `append` branch (`else` arm: `line: origin.line +
    /// relative.line`) only fires for a segment whose OWN fragment-relative
    /// line is >= 1 — every other test in this file uses a single-line
    /// fragment body and is structurally unable to reach it. Built via
    /// `SourceMapBuilder` directly (not hand-derived VLQ) so the INPUT is
    /// unambiguous; every assertion below is independently reasoned from
    /// the concatenation semantics, not read back from this
    /// implementation's own output.
    #[test]
    fn prepend_preamble_shifts_a_fragment_internal_second_line_through_the_multiline_branch() {
        let mut input = SourceMapBuilder::default();
        let source_id = input.add_source_and_content("Comp.vue", "authored text");
        // Fragment's own first line, column 6 -> authored (1, 6).
        input.add_token(0, 6, 1, 6, Some(source_id), None);
        // Fragment's own SECOND line, column 3 -> authored (2, 3).
        input.add_token(1, 3, 2, 3, Some(source_id), None);
        let input_map = input.into_sourcemap().to_json_string();

        let code = "const a = 1\nconst b = 2"; // two physical lines
                                               // A two-line preamble: `output_origin` = (2, 0).
        let preamble = "import { a } from \"vue\"\nimport { b } from \"vue\"\n";

        let composed =
            prepend_preamble(preamble, code, Some(&input_map)).expect("prepend composes cleanly");
        assert_eq!(composed.code, format!("{preamble}{code}"));

        let decoded = SourceMap::from_json_string(&composed.source_map).unwrap();
        let tokens: Vec<_> = decoded.get_tokens().collect();
        assert_eq!(tokens.len(), 2, "both segments must survive composition");

        // First segment: fragment-relative line 0 — the CONTINUATION
        // branch. dst_line stays at output_origin.line (2); dst_col is
        // output_origin.col (0, the preamble ends in `\n`) plus the
        // fragment's own column (6).
        assert_eq!(tokens[0].get_dst_line(), 2);
        assert_eq!(tokens[0].get_dst_col(), 6);
        assert_eq!(tokens[0].get_src_line(), 1);
        assert_eq!(tokens[0].get_src_col(), 6);

        // Second segment: fragment-relative line 1 — the MULTI-LINE
        // branch under test. dst_line = output_origin.line (2) + the
        // fragment's own relative line (1) = 3; dst_col is the fragment's
        // OWN column (3), independent of the preamble's trailing column.
        assert_eq!(
            tokens[1].get_dst_line(),
            3,
            "a fragment-internal second line must land at preamble_lines + fragment_line"
        );
        assert_eq!(
            tokens[1].get_dst_col(),
            3,
            "a fragment-internal line's column is its OWN column, never offset by the \
             preamble's trailing column"
        );
        assert_eq!(tokens[1].get_src_line(), 2);
        assert_eq!(tokens[1].get_src_col(), 3);
    }

    #[test]
    fn prepend_preamble_preserves_original_source_identity_and_content() {
        let mut input = SourceMapBuilder::default();
        let source_id = input.add_source_and_content("src/Comp.vue", "<script>real</script>");
        input.add_name("msg");
        input.add_token(0, 6, 1, 6, Some(source_id), Some(0));
        let input_map = input.into_sourcemap().to_json_string();

        let composed = prepend_preamble("import x from 'y'\n", "const n = 1", Some(&input_map))
            .expect("prepend composes");
        let decoded = SourceMap::from_json_string(&composed.source_map).unwrap();

        let sources: Vec<&str> = decoded.get_sources().collect();
        assert_eq!(
            sources,
            vec!["src/Comp.vue"],
            "the original source path must survive — never a synthetic \"fragment\" placeholder"
        );
        let contents: Vec<Option<&str>> = decoded.get_source_contents().collect();
        assert_eq!(
            contents,
            vec![Some("<script>real</script>")],
            "the original sourcesContent must survive, never dropped to empty"
        );
        let names: Vec<&str> = decoded.get_names().collect();
        assert_eq!(names, vec!["msg"], "the original names table must survive");
        let token = decoded.get_tokens().next().unwrap();
        assert_eq!(
            token.get_name_id(),
            Some(0),
            "a token's name reference must survive composition"
        );
    }

    #[test]
    fn prepend_preamble_with_empty_preamble_leaves_positions_unchanged() {
        let code = "const n = 1";
        let map = "{\"version\":3,\"sources\":[\"Comp.vue\"],\"names\":[],\"mappings\":\"MACM\"}";
        let composed = prepend_preamble("", code, Some(map)).expect("prepend composes");
        assert_eq!(composed.code, code);
        let decoded = SourceMap::from_json_string(&composed.source_map).unwrap();
        let token = decoded.get_tokens().next().unwrap();
        assert_eq!(token.get_dst_line(), 0);
        assert_eq!(token.get_dst_col(), 6);
    }

    // ── assemble_sequence ───────────────────────────────────────────

    /// One VALIDATED fragment for `assemble_sequence`'s own tests —
    /// `assemble_sequence` no longer accepts a raw `{code, source_map}`
    /// pair, so every test piece must genuinely pass `Fragment::validate`.
    /// `role` keeps each piece's `SourceUnitId` distinct.
    fn seq_fragment(role: &str, code: &str, source_map: Option<&str>) -> ValidatedFragment {
        Fragment {
            domain: FrameworkDomain::Vue,
            product: ProductKind::RuntimeClient,
            source_unit: unit(role),
            source_space: SourceSpaceKind::GeneratedFragment,
            placement: super::super::fragment::PlacementSlot::ModuleBody,
            contract: SyntacticContract::CompleteModule,
            dialect: FragmentDialect::Tsx,
            code: code.to_string(),
            source_map: source_map.map(str::to_string),
            imports: Vec::new(),
            exports: Vec::new(),
            helpers: Vec::new(),
            dependencies: Vec::new(),
        }
        .validate()
        .expect("sequence fixture parses")
    }

    #[test]
    fn assemble_sequence_concatenates_code_in_order() {
        let a = seq_fragment("a", "import \"a\"\n", None);
        let b = seq_fragment("b", "const x = 1\n", None);
        let c = seq_fragment("c", "export default {}", None);
        let output = assemble_sequence(&[&a, &b, &c], None).expect("assembles cleanly");
        assert_eq!(output.code, "import \"a\"\nconst x = 1\nexport default {}");
    }

    #[test]
    fn assemble_sequence_places_a_second_fragments_map_after_the_first_boundary() {
        // Two mapped fragments, exactly the script-then-template shape.
        // First: one segment at (0,6) -> authored (1,6).
        let script_map =
            "{\"version\":3,\"sources\":[\"Comp.vue\"],\"names\":[],\"mappings\":\"MACM\"}";
        // Second: one segment at (0,9) -> authored (9,2).
        let template_map =
            "{\"version\":3,\"sources\":[\"Comp.vue\"],\"names\":[],\"mappings\":\"SAAA;A\"}";

        let script = seq_fragment("script", "const n = 1\n", Some(script_map));
        let template = seq_fragment("template", "function render() {}\n", Some(template_map));
        let output = assemble_sequence(&[&script, &template], None).expect("assembles cleanly");
        assert_eq!(output.code, "const n = 1\nfunction render() {}\n");

        let decoded = SourceMap::from_json_string(&output.source_map).unwrap();
        let tokens: Vec<_> = decoded.get_tokens().collect();

        // Script's own segment: unshifted (it's the first fragment,
        // placement is (0,0)).
        assert_eq!(tokens[0].get_dst_line(), 0);
        assert_eq!(tokens[0].get_dst_col(), 6);
        assert_eq!(tokens[0].get_src_line(), 1);

        // A sourceless BOUNDARY segment at line 1 col 0 — the script ends
        // in `\n`, so the write cursor when the template starts is
        // exactly where the boundary lands.
        let boundary = tokens
            .iter()
            .find(|t| t.get_dst_line() == 1 && t.get_dst_col() == 0 && t.get_source_id().is_none());
        assert!(
            boundary.is_some(),
            "expected a sourceless boundary segment at (1,0), got tokens: {tokens:?}"
        );

        // Template's own segment, placed at line 1 (the template started
        // on line 1 of the assembled module).
        let template_token = tokens
            .iter()
            .find(|t| t.get_source_id().is_some() && t.get_dst_line() == 1)
            .expect("the template's own segment must survive, shifted onto line 1");
        assert_eq!(template_token.get_dst_col(), 9);
    }

    #[test]
    fn assemble_sequence_emits_no_boundary_when_the_fragment_does_not_end_in_newline() {
        let map = "{\"version\":3,\"sources\":[\"Comp.vue\"],\"names\":[],\"mappings\":\"MACM\"}";
        let a = seq_fragment("a", "const n = 1", Some(map)); // no trailing newline
        let b = seq_fragment("b", "\nexport default {}", None);
        let output = assemble_sequence(&[&a, &b], None).expect("assembles cleanly");
        let decoded = SourceMap::from_json_string(&output.source_map).unwrap();
        let sourceless_count = decoded
            .get_tokens()
            .filter(|t| t.get_source_id().is_none())
            .count();
        assert_eq!(
            sourceless_count, 0,
            "no boundary segment must be emitted when the fragment's own code does not end \
             in a newline"
        );
    }

    #[test]
    fn assemble_sequence_attaches_the_requested_source_root() {
        let map = "{\"version\":3,\"sources\":[\"Comp.vue\"],\"names\":[],\"mappings\":\"MACM\"}";
        let a = seq_fragment("a", "const n = 1", Some(map));
        let output = assemble_sequence(&[&a], Some("/src")).expect("assembles cleanly");
        let decoded = SourceMap::from_json_string(&output.source_map).unwrap();
        assert_eq!(decoded.get_source_root(), Some("/src"));
        // The segment itself must still survive the sourceRoot rebuild.
        let token = decoded.get_tokens().next().unwrap();
        assert_eq!(token.get_dst_col(), 6);
    }

    /// Stable append, NO dedup: two fragments that happen to declare the
    /// identical source path stay two distinct rows — collapsing them
    /// would destroy the one-to-one row-to-fragment attribution. Mirrors
    /// `verter_session`'s own pinned
    /// `vector_v4_stable_append_with_boundary_segments`.
    #[test]
    fn assemble_sequence_does_not_dedup_identical_source_paths_across_fragments() {
        let script_map =
            "{\"version\":3,\"sources\":[\"Comp.vue\"],\"names\":[],\"mappings\":\"MACM\"}";
        let template_map =
            "{\"version\":3,\"sources\":[\"Comp.vue\"],\"names\":[],\"mappings\":\"SAAA\"}";
        let script = seq_fragment("script", "const n = 1\n", Some(script_map));
        let template = seq_fragment("template", "function render() {}\n", Some(template_map));
        let output = assemble_sequence(&[&script, &template], None).expect("assembles cleanly");
        let decoded = SourceMap::from_json_string(&output.source_map).unwrap();
        let sources: Vec<&str> = decoded.get_sources().collect();
        assert_eq!(
            sources,
            vec!["Comp.vue", "Comp.vue"],
            "a deduplicating merge would collapse these to one row and destroy the \
             one-to-one row-to-fragment attribution"
        );
        // The template's token must reference the SECOND row (index 1), not
        // the first — proving the two rows are genuinely distinct, not
        // merely duplicated text.
        let template_token = decoded
            .get_tokens()
            .find(|t| t.get_dst_line() == 1 && t.get_source_id().is_some())
            .expect("the template's own segment survives");
        assert_eq!(template_token.get_source_id(), Some(1));
    }

    #[test]
    fn assemble_sequence_with_no_maps_produces_an_empty_map() {
        let a = seq_fragment("a", "export default {}", None);
        let output = assemble_sequence(&[&a], None).expect("assembles cleanly");
        let decoded = SourceMap::from_json_string(&output.source_map).unwrap();
        assert_eq!(decoded.get_tokens().count(), 0);
    }
}

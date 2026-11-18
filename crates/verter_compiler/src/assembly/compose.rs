//! Fragment placement/splice composition over the shared emission writer.
//! Each input enters one emission plan as a piece: authored inputs with an
//! encoded map as external pieces over their admitted decoded map, assembly
//! bytes as synthetic pieces. The plan is written once, and its decoded map
//! is encoded once, at this boundary. The owner/hole relationship is checked
//! against the fragment's OWN declared placement, never re-derived by
//! scanning generated text for a landmark.

use oxc_sourcemap::SourceMap;
#[cfg(test)]
use oxc_sourcemap::SourceMapBuilder;
use verter_audit::observe::Uncaptured;

use super::emit_plan::{EmissionBuilder, EmitPlanBuilder, EmittedArtifact, MapMode, MapSource};
use super::fragment::{FragmentId, PlacementSlot, ValidatedFragment};
use super::map_input::{validate_and_decode, DecodedFragmentMap};
use super::source_space::AssembledOffset;
use crate::work_ledger::CompilerWorkLedger;

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

/// A fragment's declared map, admitted against its own code.
fn admitted(
    code: &str,
    source_map: Option<&str>,
) -> Result<Option<DecodedFragmentMap>, ComposeRefusal> {
    source_map
        .map(|raw| validate_and_decode(raw, code).map_err(|_| ComposeRefusal::UncomposableMap))
        .transpose()
}

/// Push `code` as an external piece over `map`, or as synthetic bytes when
/// it declared none.
fn push_code<'a>(
    fragment: &mut EmissionBuilder<'a>,
    code: &'a str,
    map: Option<DecodedFragmentMap>,
) {
    match map {
        Some(map) => fragment.push_external(code, map),
        None => fragment.push_borrowed(code),
    }
}

/// Write one fragment as a whole artifact.
fn write(fragment: EmissionBuilder<'_>) -> Result<EmittedArtifact, ComposeRefusal> {
    let mut plan = EmitPlanBuilder::new(MapMode::Map);
    plan.push_fragment(
        fragment
            .finish()
            .map_err(|_| ComposeRefusal::InvalidBoundary)?,
    )
    .map_err(|_| ComposeRefusal::InvalidBoundary)?;
    plan.finish()
        .and_then(|plan| plan.write(&mut CompilerWorkLedger::<Uncaptured>::new()))
        .map_err(|_| ComposeRefusal::InvalidBoundary)
}

/// The artifact's text and its map encoded at this external boundary, over
/// the inputs' own tables; `None` for a text-only artifact.
pub(crate) fn encoded(
    artifact: EmittedArtifact,
    sources: &[MapSource<'_>],
    source_root: Option<&str>,
) -> (String, Option<SourceMap<'static>>) {
    let (code, map) = artifact.into_parts();
    let map = map.map(|map| map.to_source_map(&code, sources, source_root));
    (code, map)
}

/// A mapped composition's text and encoded map.
fn mapped(
    artifact: EmittedArtifact,
    source_root: Option<&str>,
) -> Result<(String, SourceMap<'static>), ComposeRefusal> {
    match encoded(artifact, &[], source_root) {
        (code, Some(map)) => Ok((code, map)),
        (_, None) => Err(ComposeRefusal::UncomposableMap),
    }
}

/// Prepend `preamble` (assembly-owned bytes with no source mapping of
/// their own — e.g. a synthesized `import { ... } from "..."` line) ahead
/// of `code`, so `source_map` keeps describing the same bytes it described
/// before the prepend. The map is admitted once against `code` and encoded
/// once for the result — exactly what concatenating text onto
/// already-mapped bytes with a bare `format!` would get wrong.
pub fn prepend_preamble(
    preamble: &str,
    code: &str,
    source_map: Option<&str>,
) -> Result<ComposedOutput, ComposeRefusal> {
    let map = admitted(code, source_map)?;
    let mut fragment = EmissionBuilder::new(MapMode::Map);
    fragment.push_borrowed(preamble);
    push_code(&mut fragment, code, map);
    let (code, source_map) = mapped(write(fragment)?, None)?;
    Ok(ComposedOutput {
        code,
        source_map: source_map.to_json_string(),
        fragment_starts_at: AssembledOffset(preamble.len() as u32),
    })
}

#[derive(Debug, Clone)]
pub struct SequencedOutput {
    pub code: String,
    pub source_map: SourceMap<'static>,
}

/// Write `fragments` in order into one module, composing every
/// contributing map into the result's coordinate space — the general
/// N-fragment placement engine multi-fragment module assembly (Vue main-
/// module assembly among its callers) is built on. Tables are a stable
/// append with no dedup: two fragments that happen to declare the identical
/// source path are two independently declared identities, and collapsing
/// them would destroy the one-to-one row-to-fragment attribution a
/// consumer's lookup depends on. A declared map boundary follows each
/// mapped fragment whose own code ends in a newline, so trailing
/// assembly-owned bytes never inherit that fragment's last authored
/// position.
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
    let mut builder = EmissionBuilder::new(MapMode::Map);
    for fragment in fragments {
        let f = fragment.fragment();
        let map = admitted(
            &f.code,
            f.source_map.as_deref().filter(|map| !map.is_empty()),
        )?;
        let mapped = map.is_some();
        push_code(&mut builder, &f.code, map);
        if mapped && f.code.ends_with('\n') {
            builder.push_map_stop();
        }
    }
    let (code, source_map) = mapped(write(builder)?, source_root)?;
    Ok(SequencedOutput { code, source_map })
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
    let before = shell_code
        .get(..hole.start as usize)
        .ok_or(ComposeRefusal::InvalidBoundary)?;
    let after = shell_code
        .get(hole.end as usize..)
        .ok_or(ComposeRefusal::InvalidBoundary)?;

    let shell_map = admitted(shell_code, owner_fragment.source_map.as_deref())?;
    let fragment_map = admitted(fragment_code, inserted_fragment.source_map.as_deref())?;
    let (before_map, after_map) = match &shell_map {
        Some(map) => (
            Some(
                map.slice(shell_code, 0, Some(hole.start))
                    .ok_or(ComposeRefusal::InvalidBoundary)?,
            ),
            Some(
                map.slice(shell_code, hole.end, None)
                    .ok_or(ComposeRefusal::InvalidBoundary)?,
            ),
        ),
        None => (None, None),
    };
    let fragment_map = fragment_map
        .map(|map| map.slice(fragment_code, 0, Some(fragment_code.len() as u32)))
        .map(|map| map.ok_or(ComposeRefusal::InvalidBoundary))
        .transpose()?;

    let mut builder = EmissionBuilder::new(MapMode::Map);
    builder.push_borrowed(preamble);
    push_code(&mut builder, before, before_map);
    builder.push_borrowed("\n");
    push_code(&mut builder, fragment_code, fragment_map);
    builder.push_borrowed("\n");
    push_code(&mut builder, after, after_map);
    let (code, source_map) = mapped(write(builder)?, None)?;

    // The inserted fragment's own bytes begin one byte after the leading
    // `\n` pushed above.
    let fragment_starts_at = AssembledOffset(preamble.len() as u32 + hole.start + 1);

    Ok(ComposedOutput {
        code,
        source_map: source_map.to_json_string(),
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
        crate::assembly::source_unit::source_unit_id("source", tag)
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

        let tokens: Vec<_> = output.source_map.get_tokens().collect();

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
        let sourceless_count = output
            .source_map
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
    fn assemble_sequence_preserves_a_carried_position_across_empty_table_only_maps() {
        let carried_map =
            "{\"version\":3,\"sources\":[\"carried.vue\"],\"names\":[],\"mappings\":\"AAIA\"}";
        let tables_map =
            "{\"version\":3,\"sources\":[\"tables.vue\"],\"names\":[\"unused\"],\"mappings\":\"\"}";
        let carried = seq_fragment("carried", "", Some(carried_map));
        let tables = seq_fragment("tables", "", Some(tables_map));
        let output = assemble_sequence(&[&carried, &tables], None).expect("empty maps compose");

        assert_eq!(output.code, "");
        assert_eq!(
            output.source_map.get_sources().collect::<Vec<_>>(),
            ["carried.vue", "tables.vue"]
        );
        assert_eq!(
            output.source_map.get_names().collect::<Vec<_>>(),
            ["unused"]
        );
        let tokens = output.source_map.get_tokens().collect::<Vec<_>>();
        assert_eq!(
            tokens.len(),
            1,
            "table-only maps must introduce no boundary"
        );
        assert_eq!(tokens[0].get_source_id(), Some(0));
        assert_eq!((tokens[0].get_dst_line(), tokens[0].get_dst_col()), (0, 0));
        assert_eq!((tokens[0].get_src_line(), tokens[0].get_src_col()), (4, 0));
    }

    #[test]
    fn assemble_sequence_attaches_the_requested_source_root() {
        let map = "{\"version\":3,\"sources\":[\"Comp.vue\"],\"names\":[],\"mappings\":\"MACM\"}";
        let a = seq_fragment("a", "const n = 1", Some(map));
        let output = assemble_sequence(&[&a], Some("/src")).expect("assembles cleanly");
        assert_eq!(output.source_map.get_source_root(), Some("/src"));
        // The segment itself must still survive the sourceRoot rebuild.
        let token = output.source_map.get_tokens().next().unwrap();
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
        let sources: Vec<&str> = output.source_map.get_sources().collect();
        assert_eq!(
            sources,
            vec!["Comp.vue", "Comp.vue"],
            "a deduplicating merge would collapse these to one row and destroy the \
             one-to-one row-to-fragment attribution"
        );
        // The template's token must reference the SECOND row (index 1), not
        // the first — proving the two rows are genuinely distinct, not
        // merely duplicated text.
        let template_token = output
            .source_map
            .get_tokens()
            .find(|t| t.get_dst_line() == 1 && t.get_source_id().is_some())
            .expect("the template's own segment survives");
        assert_eq!(template_token.get_source_id(), Some(1));
    }

    #[test]
    fn composition_builds_one_buffer() {
        let map = "{\"version\":3,\"sources\":[\"Comp.vue\"],\"names\":[],\"mappings\":\"MACM\"}";
        let a = seq_fragment("a", "import \"a\"\n", None);
        let b = seq_fragment("b", "const n = 1\n", Some(map));
        let c = seq_fragment("c", "export default {}", None);
        crate::emission::take_text_buffers();
        let output = assemble_sequence(&[&a, &b, &c], None).expect("assembles cleanly");
        assert_eq!(output.code, "import \"a\"\nconst n = 1\nexport default {}");
        // Every input is written straight into the one output buffer: no
        // fragment holds a copy of its bytes on the way.
        assert_eq!(crate::emission::take_text_buffers(), 0);
        assert_eq!(output.code.capacity(), output.code.len());
    }

    #[test]
    fn assemble_sequence_with_no_maps_produces_an_empty_map() {
        let a = seq_fragment("a", "export default {}", None);
        let output = assemble_sequence(&[&a], None).expect("assembles cleanly");
        assert_eq!(output.source_map.get_tokens().count(), 0);
    }
}

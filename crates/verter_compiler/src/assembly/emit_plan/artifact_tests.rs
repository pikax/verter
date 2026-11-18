use verter_audit::observe::Uncaptured;
use verter_span::Span;

use super::super::{
    ArtifactBoundary, DecodedMapSegment, EmissionBuilder, EmitPlan, EmitPlanBuilder,
    EmittedArtifact, MapMode, MapSegmentKind, MapSource,
};
use crate::assembly::fragment::SyntacticContract;
use crate::assembly::map_input::{take_json_map_decodes, validate_and_decode};
use crate::code_transform::advance_generated_position;
use crate::emission::{test_source, QualifiedSource};
use crate::work_ledger::CompilerWorkLedger;

fn write(plan: EmitPlan<'_>) -> EmittedArtifact {
    plan.write(&mut CompilerWorkLedger::<Uncaptured>::new())
        .unwrap()
}

/// "abcdef" with "cd" replaced by an unmapped "X".
fn replaced_middle<'s>(source: &QualifiedSource<'s>, mode: MapMode) -> EmitPlan<'s> {
    let mut builder = EmitPlanBuilder::new(mode);
    builder.push_source(source, Span::new(0, 2)).unwrap();
    builder.push_unmapped("X");
    builder.push_source(source, Span::new(4, 6)).unwrap();
    builder.finish().unwrap()
}

fn segment(
    generated: std::ops::Range<u32>,
    source: u32,
    source_start: u32,
    kind: MapSegmentKind,
) -> DecodedMapSegment {
    DecodedMapSegment {
        generated,
        source,
        source_start,
        kind,
    }
}

/// `(line, UTF-16 column)` of byte `offset` in `text`.
fn position(text: &str, offset: u32) -> (u32, u32) {
    let (mut line, mut column) = (0, 0);
    advance_generated_position(&text[..offset as usize], &mut line, &mut column);
    (line, column)
}

fn tokens(map: &oxc_sourcemap::SourceMap<'_>) -> Vec<(u32, u32, Option<u32>, u32, u32)> {
    map.get_tokens()
        .map(|token| {
            (
                token.get_dst_line(),
                token.get_dst_col(),
                token.get_source_id(),
                token.get_src_line(),
                token.get_src_col(),
            )
        })
        .collect()
}

#[test]
fn a_text_only_write_returns_text_only() {
    let source = test_source("App.vue", "abcdef");
    let artifact = write(replaced_middle(&source, MapMode::NoMap));
    assert_eq!(artifact.text(), "abXef");
    assert_eq!(artifact.map(), None);
    assert!(artifact.boundaries().is_empty());
}

#[test]
fn remapping_one_source_preserves_the_other_source_and_host_metadata() {
    let carrier = test_source("App.vue", "abc");
    let supplied = test_source("view.html", "count");
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    builder.push_source(&carrier, Span::new(0, 2)).unwrap();
    builder.push_source(&supplied, Span::new(0, 5)).unwrap();
    let artifact = write(builder.finish().unwrap());
    let mut map = artifact.map().unwrap().clone();
    let unchanged = map
        .segments()
        .iter()
        .filter(|segment| segment.source == 0)
        .cloned()
        .collect::<Vec<_>>();
    let host = validate_and_decode(
        r#"{"version":3,"sources":["view.pug"],"sourcesContent":["\n😀 count"],"names":["count"],"mappings":"AACGA","ignoreList":[0]}"#,
        "count",
    ).unwrap();
    let rows = [
        MapSource {
            name: "App.vue",
            content: Some("abc"),
        },
        MapSource {
            name: "view.html",
            content: Some("count"),
        },
        MapSource {
            name: "view.pug",
            content: Some("\n😀 count"),
        },
    ];
    let expected_lookups = map
        .wire
        .iter()
        .filter(|token| {
            token
                .payload
                .is_some_and(|payload| payload.source_index == 1)
        })
        .count();
    super::take_host_remap_lookups();
    map.remap_source(artifact.text(), 1, &host, &rows).unwrap();
    assert_eq!(super::take_host_remap_lookups(), expected_lookups);
    assert_eq!(map.lookup(1), Some((0, 1)));
    assert_eq!(map.lookup(3), Some((2, 6)));
    assert_eq!(
        map.segments()
            .iter()
            .filter(|segment| segment.source == 0)
            .cloned()
            .collect::<Vec<_>>(),
        unchanged
    );
    let encoded = map.to_source_map(artifact.text(), &rows, None);
    let token = encoded
        .lookup_token(&encoded.generate_lookup_table(), 0, 3)
        .unwrap();
    assert_eq!(token.get_source_id(), Some(2));
    assert_eq!((token.get_src_line(), token.get_src_col()), (1, 3));
    assert_eq!(
        encoded.get_name(token.get_name_id().unwrap()),
        Some("count")
    );
    assert_eq!(encoded.get_ignore_list(), Some(&[2][..]));
}

#[test]
fn declared_marker_ranges_follow_fills_in_the_single_write() {
    use crate::emission::{MarkerId, Piece, PieceText, Provenance};
    for mode in [MapMode::NoMap, MapMode::Map] {
        let (hole, tail, inner) = (MarkerId::fresh(), MarkerId::fresh(), MarkerId::fresh());
        let mut host = EmissionBuilder::new(mode);
        host.push_borrowed("prefix ");
        host.push_marked(
            Piece::new(PieceText::Borrowed("?"), Provenance::Unmapped),
            hole,
        )
        .unwrap();
        host.push_marked(
            Piece::new(PieceText::Borrowed("tail"), Provenance::Unmapped),
            tail,
        )
        .unwrap();
        let host = host.finish().unwrap();
        let slot = host.slot(hole).unwrap();
        let mut fill = EmissionBuilder::new(mode);
        fill.push_marked(
            Piece::new(PieceText::Borrowed("replacement"), Provenance::Unmapped),
            inner,
        )
        .unwrap();
        let mut plan = EmitPlanBuilder::new(mode);
        plan.push_unmapped("intro ");
        plan.push_fragment(host).unwrap();
        plan.replace_marked(slot, fill.finish().unwrap()).unwrap();
        let (written, markers) = plan
            .finish()
            .unwrap()
            .write_marked(&mut CompilerWorkLedger::<Uncaptured>::new())
            .unwrap();
        assert_eq!(written.text(), "intro prefix replacementtail");
        assert_eq!(markers, vec![(inner, 13..24), (tail, 24..28)]);
        assert_eq!(written.map().is_some(), mode == MapMode::Map);
    }
}

#[test]
fn output_capacity_is_exact() {
    let source = test_source("App.vue", "abcdef");
    for mode in [MapMode::NoMap, MapMode::Map] {
        let (text, _) = write(replaced_middle(&source, mode)).into_parts();
        assert_eq!(text.capacity(), text.len(), "{mode:?}");
    }
}

#[test]
fn boundaries_report_output_offsets() {
    let mut inner = EmitPlanBuilder::new(MapMode::NoMap);
    inner.push_unmapped("i");
    inner.push_boundary(ArtifactBoundary(2));
    let inner = inner.finish().unwrap();

    let mut builder = EmitPlanBuilder::new(MapMode::NoMap);
    builder.push_boundary(ArtifactBoundary(0));
    builder.push_unmapped("ab");
    builder.push_boundary(ArtifactBoundary(1));
    let slot = builder.reserve_slot(SyntacticContract::StatementList);
    builder.push_unmapped_fmt(format_args!("{}", 42));
    builder.push_boundary(ArtifactBoundary(3));
    builder
        .fill_plan(slot, inner, SyntacticContract::StatementList)
        .unwrap();
    let artifact = write(builder.finish().unwrap());
    assert_eq!(artifact.text(), "abi42");
    assert_eq!(
        artifact.boundaries(),
        &[
            (ArtifactBoundary(0), 0),
            (ArtifactBoundary(1), 2),
            (ArtifactBoundary(2), 3),
            (ArtifactBoundary(3), 5),
        ]
    );
}

#[test]
fn a_nested_plan_is_written_in_place() {
    let outer_source = test_source("Outer.vue", "outer");
    let inner_source = test_source("Inner.vue", "inner");

    let mut inner = EmitPlanBuilder::new(MapMode::Map);
    inner.push_source(&inner_source, Span::new(1, 3)).unwrap();
    let inner = inner.finish().unwrap();

    let mut outer = EmitPlanBuilder::new(MapMode::Map);
    outer.push_unmapped("<");
    let slot = outer.reserve_slot(SyntacticContract::Expression);
    outer.push_source(&outer_source, Span::new(0, 2)).unwrap();
    outer
        .fill_plan(slot, inner, SyntacticContract::Expression)
        .unwrap();
    let plan = outer.finish().unwrap();
    assert_eq!(plan.len(), 5);

    let artifact = write(plan);
    assert_eq!(artifact.text(), "<nnou");
    // The nested source joined the outer table when the slot was filled.
    assert!(artifact.sources()[0].same(&inner_source.key));
    assert!(artifact.sources()[1].same(&outer_source.key));
    assert_eq!(
        artifact.map().unwrap().segments(),
        &[
            segment(1..3, 0, 1, MapSegmentKind::Identity),
            segment(3..5, 1, 0, MapSegmentKind::Identity),
        ]
    );
}

/// One run of every kind over one source, then an external piece.
const SOURCE: &str = "let a = b;\nconst é = 1;\n";

fn every_kind<'s>(source: &QualifiedSource<'s>, external: &'s str) -> EmitPlan<'s> {
    let map = validate_and_decode(
        r#"{"version":3,"sources":["lib.ts"],"sourcesContent":["ext()"],"names":[],"mappings":"AAAA"}"#,
        external,
    )
    .unwrap();
    let mut fragment = EmissionBuilder::new(MapMode::Map).with_source(source.clone());
    fragment.push_source(Span::new(0, 4)).unwrap();
    fragment.push_rewritten("Bee", Span::new(4, 5));
    fragment.push_unmapped(" /*x*/ ");
    fragment.push_anchored("_ctx.b", 8);
    fragment.push_source(Span::new(9, 25)).unwrap();
    fragment.push_external(external, map);
    fragment.push_unmapped("\n");
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    builder.push_fragment(fragment.finish().unwrap()).unwrap();
    builder.finish().unwrap()
}

#[test]
fn the_four_kinds_stay_distinct_through_the_writer() {
    let source = test_source("App.vue", SOURCE);
    let artifact = write(every_kind(&source, "ext()"));
    assert_eq!(
        artifact.text(),
        "let Bee /*x*/ _ctx.b;\nconst é = 1;\next()\n"
    );
    assert_eq!(
        artifact.map().unwrap().segments(),
        &[
            segment(0..4, 0, 0, MapSegmentKind::Identity),
            segment(4..7, 0, 4, MapSegmentKind::Rewritten),
            segment(14..20, 0, 8, MapSegmentKind::Anchor),
            segment(20..36, 0, 9, MapSegmentKind::Identity),
        ]
    );
}

#[test]
fn lookup_and_serialized_map_agree() {
    let source = test_source("App.vue", SOURCE);
    let artifact = write(every_kind(&source, "ext()"));
    let text = artifact.text();
    let map = artifact.map().unwrap();
    let encoded = map.to_source_map(
        text,
        &[MapSource {
            name: "App.vue",
            content: Some(SOURCE),
        }],
        None,
    );
    assert_eq!(
        encoded.get_sources().collect::<Vec<_>>(),
        vec!["App.vue", "lib.ts"]
    );
    let table = encoded.generate_lookup_table();
    let external = 36..41;
    for generated in 0..text.len() as u32 {
        // A line break ends its line's mappings: it is not a position a
        // consumer reads, and the encoded map leaves it to the preceding
        // token.
        if !text.is_char_boundary(generated as usize)
            || text.as_bytes()[generated as usize] == b'\n'
        {
            continue;
        }
        let (line, column) = position(text, generated);
        let token = encoded
            .lookup_token(&table, line, column)
            .filter(|token| token.get_source_id().is_some());
        if external.contains(&generated) {
            // An external piece is answered by its own admitted map.
            assert_eq!(map.lookup(generated), None, "{generated}");
            assert_eq!(token.and_then(|token| token.get_source_id()), Some(1));
            continue;
        }
        let decoded = map.lookup(generated);
        assert_eq!(
            decoded.map(|(source, _)| source),
            token.and_then(|token| token.get_source_id()),
            "byte {generated} of {text:?}"
        );
        let (Some((_, offset)), Some(token)) = (decoded, token) else {
            continue;
        };
        // A V3 token applies until the next one; an identity copy is read
        // at the same distance along its line.
        let identity = map.segments().iter().any(|segment| {
            segment.generated.contains(&generated) && segment.kind == MapSegmentKind::Identity
        });
        let expected = if identity {
            (
                token.get_src_line(),
                token.get_src_col() + column - token.get_dst_col(),
            )
        } else {
            (token.get_src_line(), token.get_src_col())
        };
        assert_eq!(position(SOURCE, offset), expected, "byte {generated}");
    }
    // The three-byte replacement is mapped at every byte, not only its first.
    assert_eq!(map.lookup(6), Some((0, 4)));
}

#[test]
fn a_synthetic_overwrite_is_never_an_authored_anchor() {
    // "cd" replaced by synthetic text: no decoded segment covers it and the
    // encoded map stops the preceding mapping at its first byte.
    let source = test_source("App.vue", "abcdef");
    let artifact = write(replaced_middle(&source, MapMode::Map));
    let map = artifact.map().unwrap();
    assert_eq!(map.lookup(2), None);
    assert!(map
        .segments()
        .iter()
        .all(|segment| !segment.generated.contains(&2)));
    let encoded = map.to_source_map(
        artifact.text(),
        &[MapSource {
            name: "App.vue",
            content: None,
        }],
        None,
    );
    assert_eq!(
        tokens(&encoded),
        vec![
            (0, 0, Some(0), 0, 0),
            (0, 2, None, 0, 0),
            (0, 3, Some(0), 0, 4)
        ]
    );
}

#[test]
fn an_external_map_is_shifted_not_reencoded() {
    let source = test_source("App.vue", "line1\nab");
    let map = validate_and_decode(
        r#"{"version":3,"sources":["lib.ts"],"names":["y"],"mappings":"AAAA;AACAA","ignoreList":[0]}"#,
        "x\ny",
    )
    .unwrap();
    assert_eq!(take_json_map_decodes(), 1);

    let mut fragment = EmissionBuilder::new(MapMode::Map).with_source(source.clone());
    fragment.push_source(Span::new(0, 8)).unwrap();
    fragment.push_external("x\ny", map);
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    builder.push_fragment(fragment.finish().unwrap()).unwrap();
    let artifact = write(builder.finish().unwrap());
    assert_eq!(artifact.text(), "line1\nabx\ny");

    let encoded = artifact.map().unwrap().to_source_map(
        artifact.text(),
        &[MapSource {
            name: "App.vue",
            content: None,
        }],
        None,
    );
    assert_eq!(take_json_map_decodes(), 0);
    assert_eq!(
        tokens(&encoded),
        vec![
            (0, 0, Some(0), 0, 0),
            (1, 0, Some(0), 1, 0),
            (1, 2, Some(1), 0, 0),
            (2, 0, Some(1), 1, 0),
        ]
    );
    assert_eq!(encoded.get_names().collect::<Vec<_>>(), vec!["y"]);
    assert_eq!(encoded.get_token(3).unwrap().get_name_id(), Some(0));
    assert_eq!(encoded.get_ignore_list(), Some(&[1][..]));
}

#[test]
fn to_source_map_emits_a_token_per_line_of_an_identity_segment() {
    let source = test_source("App.vue", "a\nbc\nd");
    let mut builder = EmitPlanBuilder::new(MapMode::Map);
    builder.push_unmapped("x");
    builder.push_source(&source, Span::new(0, 6)).unwrap();
    builder.push_unmapped(";");
    let artifact = write(builder.finish().unwrap());
    assert_eq!(artifact.text(), "xa\nbc\nd;");

    let map = artifact.map().unwrap().to_source_map(
        artifact.text(),
        &[MapSource {
            name: "App.vue",
            content: Some("a\nbc\nd"),
        }],
        None,
    );
    assert_eq!(
        tokens(&map),
        vec![
            (0, 1, Some(0), 0, 0),
            (1, 0, Some(0), 1, 0),
            (2, 0, Some(0), 2, 0),
            // The trailing unmapped ";" does not inherit the last mapping.
            (2, 1, None, 0, 0),
        ]
    );
    assert_eq!(map.get_sources().collect::<Vec<_>>(), vec!["App.vue"]);
}

#[cfg(feature = "semantic-observe")]
#[test]
fn a_text_only_write_records_zero_map_segments() {
    use crate::work_ledger::WorkKind;
    use verter_audit::observe::Captured;

    let source = test_source("App.vue", "abcdef");

    let mut text_ledger = CompilerWorkLedger::<Captured>::new();
    replaced_middle(&source, MapMode::NoMap)
        .write(&mut text_ledger)
        .unwrap();
    let text_counts = text_ledger
        .snapshot()
        .expect("a captured ledger exports counts");
    assert_eq!(text_counts.count(WorkKind::EmissionMapSegments), 0);
    assert_eq!(text_counts.count(WorkKind::EmissionAllocations), 1);
    assert_eq!(text_counts.count(WorkKind::EmissionCopyBytes), 5);

    let mut mapped_ledger = CompilerWorkLedger::<Captured>::new();
    replaced_middle(&source, MapMode::Map)
        .write(&mut mapped_ledger)
        .unwrap();
    let mapped_counts = mapped_ledger
        .snapshot()
        .expect("a captured ledger exports counts");
    assert_eq!(mapped_counts.count(WorkKind::EmissionMapSegments), 2);
    assert_eq!(mapped_counts.count(WorkKind::EmissionAllocations), 1);
}

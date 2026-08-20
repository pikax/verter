//! Assembled-map composition tests.
//!
//! Three groups: reproductions of the enumerated coverage vectors; the
//! composition algebra's own hard cases (placement, boundary segments, table
//! composition, provenance); and the fail-closed validation taxonomy.
//!
//! Every expectation here is derived from the composition rules, not read back
//! from this implementation's output. Where a vector's own stated expectation
//! is incomplete relative to those rules, the test asserts the rules' answer
//! and says so.

use verter_compiler::framework_common::{
    RuntimeCompileOutput, RuntimeOutputDescriptor, RuntimeScriptBlock, RuntimeTemplateBlock,
    SourceMapFidelity, TemplateRenderExport,
};

use super::map_input::{
    validate_and_decode, DecodedFragmentMap, MapFragment, UncomposableCode, UncomposableFamily,
    WireSegment,
};
use super::{assemble_vue_main_module, AssembleMapFailure, VueMainAssemblyFailure};
use crate::types::{CompileProfile, FileMeta};

// Fixtures

fn descriptor(code: &str) -> RuntimeOutputDescriptor {
    RuntimeOutputDescriptor::generated(
        code,
        None,
        &[("test:space", "test:artifact")],
        SourceMapFidelity::Approximate,
    )
}

fn script(code: &str, source_map: &str) -> RuntimeScriptBlock {
    RuntimeScriptBlock {
        code: code.to_string(),
        source_map: source_map.to_string(),
        setup: true,
        output_descriptor: descriptor(code),
        generated_template_hole: None,
        runtime_imports: Vec::new(),
        sfc_export_placement: super::map_compose::literal_scan_placement_for_fixture(code),
    }
}

fn template(code: &str, source_map: &str) -> RuntimeTemplateBlock {
    RuntimeTemplateBlock {
        code: code.to_string(),
        source_map: source_map.to_string(),
        imports: Vec::new(),
        ssr_imports: Vec::new(),
        render_export: TemplateRenderExport::Render,
        output_descriptor: descriptor(code),
    }
}

fn map_json(sources: &str, names: &str, mappings: &str) -> String {
    format!("{{\"version\":3,\"sources\":{sources},\"names\":{names},\"mappings\":\"{mappings}\"}}")
}

/// Encode a declarative segment list to a `mappings` string.
///
/// Only the INPUT encoding is mechanical. Every EXPECTED output below is
/// derived from the composition rules by hand — writing the inputs
/// declaratively just keeps a base64 slip in a fixture from masquerading as a
/// composition result.
fn encode_mappings(segments: &[Seg]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    fn field(delta: i64, out: &mut String) {
        let mut word = if delta < 0 {
            (((-delta) as u64) << 1) | 1
        } else {
            (delta as u64) << 1
        };
        loop {
            let mut digit = (word & 31) as usize;
            word >>= 5;
            if word > 0 {
                digit |= 32;
            }
            out.push(ALPHABET[digit] as char);
            if word == 0 {
                return;
            }
        }
    }

    let mut out = String::new();
    let (mut line, mut column) = (0u32, 0i64);
    let (mut source, mut source_line, mut source_column, mut name) = (0i64, 0i64, 0i64, 0i64);
    let mut first_on_line = true;

    for &(generated_line, generated_column, payload) in segments {
        while line < generated_line {
            out.push(';');
            line += 1;
            column = 0;
            first_on_line = true;
        }
        if !first_on_line {
            out.push(',');
        }
        first_on_line = false;

        field(i64::from(generated_column) - column, &mut out);
        column = i64::from(generated_column);

        if let Some((index, authored_line, authored_column, name_index)) = payload {
            field(i64::from(index) - source, &mut out);
            source = i64::from(index);
            field(i64::from(authored_line) - source_line, &mut out);
            source_line = i64::from(authored_line);
            field(i64::from(authored_column) - source_column, &mut out);
            source_column = i64::from(authored_column);
            if let Some(index) = name_index {
                field(i64::from(index) - name, &mut out);
                name = i64::from(index);
            }
        }
    }
    out
}

/// A source-bearing input segment against source row 0.
fn seg(line: u32, column: u32, authored_line: u32, authored_column: u32) -> Seg {
    (
        line,
        column,
        Some((0, authored_line, authored_column, None)),
    )
}

/// A sourceless input segment.
fn sourceless(line: u32, column: u32) -> Seg {
    (line, column, None)
}

fn mapping_profile() -> CompileProfile {
    CompileProfile {
        source_map: true,
        ..CompileProfile::default()
    }
}

/// `(genLine, genCol, srcIdx, srcLine, srcCol, nameIdx)`, with the four
/// authored fields absent for a sourceless segment.
type Seg = (u32, u32, Option<(u32, u32, u32, Option<u32>)>);

/// Decode an emitted artifact back to its compared shape. Decoding through the
/// same validating reader the inputs go through means the assertions are over
/// the artifact a consumer sees, not over this crate's in-memory form.
struct Artifact {
    sources: Vec<String>,
    names: Vec<String>,
    sources_content: Option<Vec<Option<String>>>,
    source_root: Option<String>,
    ignore_list: Vec<u32>,
    segments: Vec<Seg>,
    raw: String,
}

fn decode_artifact(raw: &str) -> Artifact {
    // The artifact describes the ASSEMBLED module, so the coordinate checks
    // need a code long and wide enough to admit every position; the assembled
    // code itself is supplied by callers that care.
    let permissive = "\u{20}".repeat(4096) + "\n";
    let permissive = permissive.repeat(64);
    let decoded = validate_and_decode(raw, &permissive)
        .expect("the emitted artifact is itself a valid flat v3 map");
    Artifact {
        sources: decoded.sources,
        names: decoded.names,
        sources_content: decoded.sources_content,
        source_root: decoded.source_root,
        // The emitted artifact's own ignore-list entries are always small
        // integral values (they already passed step 1.23's bound check
        // against the fragment's own table before composition), so narrowing
        // from the decoder's binary64 storage to u32 here is exact — see the
        // comment on `map_compose.rs`'s own composition step.
        ignore_list: decoded
            .ignore_list
            .iter()
            .map(|entry| *entry as u32)
            .collect(),
        segments: decoded
            .segments
            .iter()
            .map(|segment: &WireSegment| {
                (
                    segment.generated_line,
                    segment.generated_column,
                    segment.payload.map(|payload| {
                        (
                            payload.source_index,
                            payload.source_line,
                            payload.source_column,
                            payload.name_index,
                        )
                    }),
                )
            })
            .collect(),
        raw: raw.to_string(),
    }
}

/// Assemble a script-only module and return its code plus decoded artifact.
fn assemble_script_only(code: &str, map: &str) -> (String, Artifact) {
    let compiled = RuntimeCompileOutput {
        script: Some(script(code, map)),
        ..RuntimeCompileOutput::default()
    };
    let meta = FileMeta {
        has_script: true,
        ..FileMeta::default()
    };
    let assembled = assemble_vue_main_module("Comp.vue", &compiled, &meta, &mapping_profile())
        .expect("composes");
    let artifact = decode_artifact(
        assembled
            .source_map
            .as_deref()
            .expect("a map was requested"),
    );
    (assembled.code, artifact)
}

/// The script fragment's own segments, i.e. everything before the module's
/// trailing assembly-owned scaffolding. A script-only module places the script
/// at line 0, so its fragment segments are exactly the ones this returns.
fn script_fragment_segments(artifact: &Artifact) -> Vec<Seg> {
    artifact.segments.clone()
}

fn expect_failure(
    script_block: Option<RuntimeScriptBlock>,
    template_block: Option<RuntimeTemplateBlock>,
    meta: FileMeta,
) -> AssembleMapFailure {
    let compiled = RuntimeCompileOutput {
        script: script_block,
        template: template_block,
        ..RuntimeCompileOutput::default()
    };
    // Every fixture in this suite is specifically about INPUT-MAP
    // validation (missing/uncomposable maps, source-root disagreement) —
    // any other `VueMainAssemblyFailure` variant here is a genuine
    // regression in what the fixture actually exercises, not an
    // equivalent outcome to unwrap past.
    match assemble_vue_main_module("Comp.vue", &compiled, &meta, &mapping_profile()) {
        Err(VueMainAssemblyFailure::InputMap(failure)) => failure,
        Err(other) => panic!("expected an input-map failure, got: {other:?}"),
        Ok(_) => panic!("this input must fail closed"),
    }
}

fn expect_script_code(raw_map: &str) -> UncomposableCode {
    match expect_failure(
        Some(script("const x = 1\n", raw_map)),
        None,
        FileMeta {
            has_script: true,
            ..FileMeta::default()
        },
    ) {
        AssembleMapFailure::UncomposableInputMap { fragment, code } => {
            assert_eq!(fragment, MapFragment::Script);
            code
        }
        other => panic!("expected an uncomposable-input-map outcome, got {other:?}"),
    }
}

// Enumerated coverage vectors

/// V1 — rename token geometry, plus a TERMINAL removal, which therefore has no
/// following-chunk segment.
#[test]
fn vector_v1_rename_geometry_and_terminal_removal() {
    let mappings = encode_mappings(&[
        seg(0, 0, 1, 0),
        seg(0, 6, 1, 6),
        seg(1, 0, 2, 0),
        seg(1, 15, 2, 15),
    ]);
    let (code, artifact) = assemble_script_only(
        "const __sfc__ = {}\nexport default __sfc__;\n",
        &map_json("[\"Comp.vue\"]", "[]", &mappings),
    );

    assert!(code.starts_with("const _sfc_main = {}\n"));
    assert_eq!(
        script_fragment_segments(&artifact)[..3],
        [
            (0, 0, Some((0, 1, 0, None))),
            (0, 6, Some((0, 1, 6, None))),
            (0, 15, Some((0, 1, 6, None))),
        ]
    );
}

/// V2 — a NON-terminal removal DOES have a following-chunk segment, and that
/// segment is sourceless when its line declares nothing at or before it.
#[test]
fn vector_v2_non_terminal_removal_resumes_sourcelessly() {
    let mappings = encode_mappings(&[
        seg(0, 0, 1, 0),
        seg(0, 6, 1, 6),
        seg(1, 0, 2, 0),
        seg(1, 15, 2, 15),
        seg(2, 6, 3, 6),
    ]);
    let (code, artifact) = assemble_script_only(
        "const __sfc__ = {}\nexport default __sfc__;\nconst tail = 1\n",
        &map_json("[\"Comp.vue\"]", "[]", &mappings),
    );

    assert!(code.starts_with("const _sfc_main = {}\nconst tail = 1\n"));
    assert_eq!(
        script_fragment_segments(&artifact)[..5],
        [
            (0, 0, Some((0, 1, 0, None))),
            (0, 6, Some((0, 1, 6, None))),
            (0, 15, Some((0, 1, 6, None))),
            (1, 0, None),
            (1, 6, Some((0, 3, 6, None))),
        ],
        "an implementation that omits the following-chunk segment, or performs \
         a global rather than line-scoped lookup and inherits authored (2,15), \
         fails here"
    );
}

/// V3 — two segments sharing one coordinate keep their wire order. A multiset
/// or column-sorted comparison cannot distinguish this from its swap.
#[test]
fn vector_v3_coincident_segments_keep_wire_order() {
    // Two segments at ONE coordinate; the second encodes a zero column delta.
    let mappings = encode_mappings(&[seg(0, 0, 1, 0), seg(0, 0, 5, 5)]);
    let (code, artifact) = assemble_script_only(
        "const x = 1\n",
        &map_json("[\"Comp.vue\"]", "[]", &mappings),
    );

    assert!(code.starts_with("const x = 1\n"));
    assert_eq!(
        script_fragment_segments(&artifact)[..2],
        [(0, 0, Some((0, 1, 0, None))), (0, 0, Some((0, 5, 5, None)))],
        "swapping these two leaves every multiset and every column-sorted list \
         identical while changing what a consumer at (0,0) resolves"
    );
    assert!(
        artifact.raw.contains(&format!(
            "\"mappings\":\"{}",
            encode_mappings(&[seg(0, 0, 1, 0), seg(0, 0, 5, 5)])
        )),
        "the zero column delta must survive re-encoding as an ADDITIONAL \
         segment rather than replacing the first, got:\n{}",
        artifact.raw
    );
}

/// V4 — stable append, no table dedup; template indices shift. The vector's
/// stated sequence omits both fragment-end boundaries; this asserts all five.
#[test]
fn vector_v4_stable_append_with_boundary_segments() {
    let compiled = RuntimeCompileOutput {
        // One segment at (0,6)->(1,6) carrying name 0.
        script: Some(script(
            "const __sfc__ = {}\n",
            &map_json(
                "[\"Comp.vue\"]",
                "[\"count\"]",
                &encode_mappings(&[(0, 6, Some((0, 1, 6, Some(0))))]),
            ),
        )),
        // One segment at (0,9)->(9,2) carrying name 0.
        template: Some(template(
            "function render() {}\n",
            &map_json(
                "[\"Comp.vue\"]",
                "[\"count\"]",
                &encode_mappings(&[(0, 9, Some((0, 9, 2, Some(0))))]),
            ),
        )),
        ..RuntimeCompileOutput::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let assembled = assemble_vue_main_module("Comp.vue", &compiled, &meta, &mapping_profile())
        .expect("composes");
    let artifact = decode_artifact(
        assembled
            .source_map
            .as_deref()
            .expect("a map was requested"),
    );

    assert_eq!(
        artifact.sources,
        ["Comp.vue", "Comp.vue"],
        "a deduplicating merge would collapse these to one row and destroy the \
         one-to-one row-to-fragment attribution"
    );
    assert_eq!(artifact.names, ["count", "count"]);
    assert_eq!(
        artifact.segments,
        [
            (0, 6, Some((0, 1, 6, Some(0)))),
            (0, 15, Some((0, 1, 6, Some(0)))),
            (1, 0, None),
            (2, 9, Some((1, 9, 2, Some(1)))),
            (3, 0, None),
        ],
        "the script code ends with a newline so the cursor is at line 1; the \
         assembler then writes one separator newline, putting the template at \
         line 2. Reading the separator as absent, and placing the template at \
         line 1, is the off-by-one this vector exists to catch."
    );
}

/// V5 — columns are UTF-16 code units, not code points and not UTF-8 bytes.
/// For the same position those are 11, 10, and 13.
#[test]
fn vector_v5_columns_are_utf16_code_units() {
    let source = "const \u{1D400} = __sfc__\n";
    assert_eq!(source.find("__sfc__"), Some(13), "the UTF-8 byte offset");
    assert_eq!(
        source.chars().take_while(|c| *c != '_').count(),
        10,
        "the code-point index"
    );

    let (code, artifact) = assemble_script_only(
        source,
        &map_json(
            "[\"Comp.vue\"]",
            "[]",
            &encode_mappings(&[seg(0, 0, 1, 0), seg(0, 11, 1, 11)]),
        ),
    );

    assert!(code.starts_with("const \u{1D400} = _sfc_main\n"));
    assert_eq!(
        script_fragment_segments(&artifact)[..3],
        [
            (0, 0, Some((0, 1, 0, None))),
            (0, 11, Some((0, 1, 11, None))),
            (0, 20, Some((0, 1, 11, None))),
        ],
        "an implementation counting code points or bytes fails this and no other"
    );
}

/// V6 — a CR retained before a LF occupies a real column; lines split on LF
/// only. The vector's own recorded weakness is that every asserted coordinate
/// sits at or left of the CR, so stripping the CR would change nothing; this
/// adds a CR-sensitive coordinate, which is what makes the case discriminate.
#[test]
fn vector_v6_a_retained_carriage_return_occupies_a_column() {
    // The middle segment sits ON the CR, at column 11 of line 0.
    let mappings = encode_mappings(&[seg(0, 0, 1, 0), seg(0, 11, 1, 11), seg(1, 6, 2, 6)]);
    let (code, artifact) = assemble_script_only(
        "const a = 1\r\nconst __sfc__ = {}\r\n",
        &map_json("[\"Comp.vue\"]", "[]", &mappings),
    );

    assert!(code.starts_with("const a = 1\r\nconst _sfc_main = {}\r\n"));
    assert_eq!(
        script_fragment_segments(&artifact)[..4],
        [
            (0, 0, Some((0, 1, 0, None))),
            (0, 11, Some((0, 1, 11, None))),
            (1, 6, Some((0, 2, 6, None))),
            (1, 15, Some((0, 2, 6, None))),
        ],
        "column 11 of line 0 is the CR: stripping it would shift this coordinate"
    );
}

/// V7 — a sourceless segment is a BARRIER. Both lookups after it stay
/// sourceless rather than skipping past it to the nearest source-bearing one.
#[test]
fn vector_v7_a_sourceless_segment_is_a_barrier() {
    let (code, artifact) = assemble_script_only(
        "const __sfc__ = {}\n",
        &map_json(
            "[\"Comp.vue\"]",
            "[]",
            &encode_mappings(&[seg(0, 0, 1, 0), sourceless(0, 3)]),
        ),
    );

    assert!(code.starts_with("const _sfc_main = {}\n"));
    assert_eq!(
        script_fragment_segments(&artifact)[..4],
        [
            (0, 0, Some((0, 1, 0, None))),
            (0, 3, None),
            (0, 6, None),
            (0, 15, None),
        ],
        "an implementation treating a sourceless segment as transparent reports \
         authored (1,0) at both later positions and so fabricates provenance \
         for text the fragment deliberately left unmapped"
    );
}

/// F1 — malformed map JSON is uncomposable, never a degraded success.
#[test]
fn vector_f1_malformed_json() {
    assert_eq!(
        expect_script_code("{ not json"),
        UncomposableCode::MapBytesNotJson
    );
}

/// F2 — a non-3 version is rejected before any composition.
#[test]
fn vector_f2_bad_version() {
    assert_eq!(
        expect_script_code("{\"version\":2,\"sources\":[],\"names\":[],\"mappings\":\"\"}"),
        UncomposableCode::VersionNot3
    );
}

/// F3 — an absent `mappings` member is NOT silently read as an empty map.
#[test]
fn vector_f3_absent_mappings() {
    assert_eq!(
        expect_script_code("{\"version\":3,\"sources\":[\"Comp.vue\"],\"names\":[]}"),
        UncomposableCode::MappingsMemberAbsent
    );
}

/// F4 — out-of-range VLQ is rejected, not wrapped to zero. `"A"` and
/// `"ggggggE"` both yield 0 in lenient decoders; only `"A"` is conforming.
#[test]
fn vector_f4_vlq_out_of_range() {
    assert_eq!(
        expect_script_code(&map_json("[\"Comp.vue\"]", "[]", "ggggggE")),
        UncomposableCode::VlqFieldOutOfRange
    );
    // The control: the conforming encoding of the same value composes.
    let (_, artifact) =
        assemble_script_only("const x = 1\n", &map_json("[\"Comp.vue\"]", "[]", "A"));
    assert_eq!(
        artifact.segments,
        [(0, 0, None), (1, 0, None)],
        "the carried sourceless segment, then the fragment-end boundary"
    );
}

/// F5 — a source index with no table row is rejected, and specifically as a
/// dangling index rather than as a malformed segment: `"ACAA"` is a well-formed
/// four-field segment, where `"AC"` would decode to two fields and be rejected
/// earlier under a different code.
#[test]
fn vector_f5_source_index_out_of_table() {
    assert_eq!(
        expect_script_code(&map_json("[\"Comp.vue\"]", "[]", "ACAA")),
        UncomposableCode::SourceIndexOutOfTable
    );
    assert_eq!(
        expect_script_code(&map_json("[\"Comp.vue\"]", "[]", "AC")),
        UncomposableCode::SegmentFieldCount,
        "arity beats index bounds — the distinction this vector depends on"
    );
}

/// F6 — an indexed (`sections`) map is not a flat map. A conforming consumer
/// prefers `sections` over `mappings`, so such a map does not describe its
/// generated document through `mappings` at all.
#[test]
fn vector_f6_indexed_map() {
    assert_eq!(
        expect_script_code("{\"version\":3,\"sections\":[]}"),
        UncomposableCode::SectionsMemberPresent
    );
}

/// F7 — a template-only cell whose compiler produced a SYNTHETIC script block
/// with an empty map is NOT a missing required map. Requiredness comes from the
/// authored-fragment inventory, never from the presence of a compiled block.
#[test]
fn vector_f7_synthetic_script_is_not_a_missing_required_map() {
    let compiled = RuntimeCompileOutput {
        script: Some(script("const __sfc__ = {}\nexport default __sfc__;\n", "")),
        template: Some(template(
            "function render() {}\n",
            &map_json("[\"Comp.vue\"]", "[]", "SACS"),
        )),
        ..RuntimeCompileOutput::default()
    };
    let meta = FileMeta {
        has_script: false,
        has_template: true,
        ..FileMeta::default()
    };

    let assembled = assemble_vue_main_module("Comp.vue", &compiled, &meta, &mapping_profile())
        .expect("a synthetic sourceless script composes rather than failing closed");
    let artifact = decode_artifact(
        assembled
            .source_map
            .as_deref()
            .expect("a map was requested"),
    );

    assert_eq!(
        artifact.sources,
        ["Comp.vue"],
        "only the template contributes table rows"
    );
    assert_eq!(
        artifact.segments,
        [(2, 9, Some((0, 1, 9, None))), (3, 0, None)],
        "the mapless script contributes no carried, replacement, resume, table \
         or boundary segment — but its code is still written and rewritten, so \
         the template still lands at line 2"
    );
    assert!(
        assembled.code.starts_with("const _sfc_main = {}\n"),
        "the synthetic script's own rewrites still ran, got:\n{}",
        assembled.code
    );
}

// Composition algebra

/// The boundary segment fires for a contributing fragment whose final code ends
/// with a newline, and it wins the lookup for every column of that line — so
/// the assembly-owned bytes that begin there resolve to nothing rather than to
/// the fragment's authored position.
#[test]
fn a_trailing_empty_line_segment_is_shadowed_by_the_boundary_segment() {
    // Two segments: (0,0)->(1,0) and (1,0)->(9,0), the latter on the trailing
    // empty line, which admits column 0 only.
    let (_, artifact) = assemble_script_only(
        "const x = 1\n",
        &map_json("[\"Comp.vue\"]", "[]", "AACA;AAQA"),
    );

    assert_eq!(
        artifact.segments,
        [
            (0, 0, Some((0, 1, 0, None))),
            (1, 0, Some((0, 9, 0, None))),
            (1, 0, None),
        ]
    );
    assert_eq!(
        artifact.segments.last().map(|segment| segment.2),
        Some(None),
        "the boundary is emitted AFTER every segment of the fragment it bounds, \
         so the last-applicable lookup resolves to it across the whole line"
    );
}

/// Boundary condition is "final code ends with a newline", not "end cursor
/// column is zero". They disagree on an empty present fragment: firing
/// there would shadow a carried authored segment. Constructible: a script
/// that rewrites to empty.
#[test]
fn an_empty_present_fragment_receives_no_boundary_segment() {
    let (code, artifact) = assemble_script_only(
        "export default __sfc__;\n",
        // One segment at (0,0)->(4,0), and one at the end position (1,0)->(7,0).
        &map_json("[\"Comp.vue\"]", "[]", "AAIA;AAGA"),
    );

    assert_eq!(
        code, "\nexport default _sfc_main",
        "the fragment rewrites to empty, so the newline patch fires over a line \
         with no characters at all — the module's first byte is that newline. \
         (mapping_profile() defaults to hmr_strategy: None, so __file is \
         correctly absent — see assemble_main_module_no_hmr_strategy_skips_file_even_in_dev.)"
    );
    assert_eq!(
        artifact.segments,
        [(0, 0, Some((0, 7, 0, None)))],
        "the surviving end-position segment is carried and NOT shadowed: a \
         boundary here would make a faithfully composed authored position \
         unobservable"
    );
}

/// A fragment whose code does not end with a newline gets no boundary segment
/// either: the assembly-owned bytes that follow begin on the NEXT line, outside
/// the fragment's coordinate space, so there is nothing on its lines to protect.
#[test]
fn a_fragment_not_ending_in_a_newline_receives_no_boundary_segment() {
    let (code, artifact) =
        assemble_script_only("const x = 1", &map_json("[\"Comp.vue\"]", "[]", "AACA"));

    assert!(code.starts_with("const x = 1\n"), "the newline patch fires");
    assert_eq!(artifact.segments, [(0, 0, Some((0, 1, 0, None)))]);
}

/// Assembly-owned bytes contribute no segments of their own. A module whose
/// scaffolding is maximal — styles, custom blocks, `__file`, HMR, SSR — still
/// carries exactly the fragment segments plus the boundary.
#[test]
fn assembly_owned_scaffolding_contributes_no_segments() {
    use verter_compiler::framework_common::{RuntimeCustomBlock, RuntimeStyleBlock};

    let compiled = RuntimeCompileOutput {
        script: Some(script(
            "const x = 1\n",
            &map_json("[\"Comp.vue\"]", "[]", "AACA"),
        )),
        styles: vec![RuntimeStyleBlock {
            code: ".a{}".to_string(),
            source_map: None,
            lang: None,
            scope_hash: None,
            has_global: false,
            output_descriptor: descriptor(".a{}"),
        }],
        custom_blocks: vec![RuntimeCustomBlock {
            block_type: "i18n".to_string(),
            content: "{}".to_string(),
        }],
        ..RuntimeCompileOutput::default()
    };
    let meta = FileMeta {
        has_script: true,
        style_langs: vec![None],
        custom_types: vec!["i18n".to_string()],
        custom_langs: vec![None],
        ..FileMeta::default()
    };
    let profile = CompileProfile {
        source_map: true,
        is_production: false,
        ssr: true,
        ..CompileProfile::default()
    };

    let assembled =
        assemble_vue_main_module("Comp.vue", &compiled, &meta, &profile).expect("composes");
    let artifact = decode_artifact(
        assembled
            .source_map
            .as_deref()
            .expect("a map was requested"),
    );

    // Three assembly-owned lines precede the script: two virtual imports and a
    // blank separator.
    assert!(assembled.code.starts_with(
        "import \"Comp.vue?vue&type=style&index=0&lang.css\"\n\
         import block0 from \"Comp.vue?vue&type=i18n&index=0\"\n\
         \nconst x = 1\n"
    ));
    assert_eq!(
        artifact.segments,
        [(3, 0, Some((0, 1, 0, None))), (4, 0, None)],
        "only the placed fragment segment and its boundary — the imports, the \
         custom-block invocation, `__file`, the SSR wrapper and the export all \
         contribute nothing"
    );
}

/// Placement offsets a fragment's FIRST line by both the placement line and
/// column, and every later line by the line alone — the fragment's own newline
/// started those lines, so their columns are already absolute.
#[test]
fn placement_offsets_only_the_first_line_by_the_column() {
    // Segments at (0,0), (0,5) and (1,3), so the second line's column must
    // survive placement unshifted.
    let compiled = RuntimeCompileOutput {
        template: Some(template(
            "function render() {\n  return 1\n}\n",
            &map_json(
                "[\"Comp.vue\"]",
                "[]",
                &encode_mappings(&[seg(0, 0, 1, 0), seg(0, 5, 1, 5), seg(1, 3, 2, 3)]),
            ),
        )),
        ..RuntimeCompileOutput::default()
    };
    let meta = FileMeta {
        has_template: true,
        ..FileMeta::default()
    };
    let assembled = assemble_vue_main_module("Comp.vue", &compiled, &meta, &mapping_profile())
        .expect("composes");
    let artifact = decode_artifact(
        assembled
            .source_map
            .as_deref()
            .expect("a map was requested"),
    );

    // No script block, so the assembler writes `const _sfc_main = {}\n` then a
    // separator newline: the template begins at line 2, column 0.
    assert_eq!(
        artifact.segments,
        [
            (2, 0, Some((0, 1, 0, None))),
            (2, 5, Some((0, 1, 5, None))),
            (3, 3, Some((0, 2, 3, None))),
            (5, 0, None),
        ]
    );
}

/// Table rows a fragment declares but no segment references are still
/// contributed, and template indices shift past them.
#[test]
fn unreferenced_table_rows_are_contributed_and_shift_later_indices() {
    let compiled = RuntimeCompileOutput {
        script: Some(script(
            "const x = 1\n",
            // Two sources and two names declared; one segment referencing
            // source 0 and name 0.
            &map_json("[\"a.vue\",\"b.vue\"]", "[\"one\",\"two\"]", "AACAA"),
        )),
        template: Some(template(
            "function render() {}\n",
            &map_json("[\"c.vue\"]", "[\"three\"]", "AACAA"),
        )),
        ..RuntimeCompileOutput::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let assembled = assemble_vue_main_module("Comp.vue", &compiled, &meta, &mapping_profile())
        .expect("composes");
    let artifact = decode_artifact(
        assembled
            .source_map
            .as_deref()
            .expect("a map was requested"),
    );

    assert_eq!(artifact.sources, ["a.vue", "b.vue", "c.vue"]);
    assert_eq!(artifact.names, ["one", "two", "three"]);
    assert_eq!(
        artifact.segments,
        [
            (0, 0, Some((0, 1, 0, Some(0)))),
            (1, 0, None),
            (2, 0, Some((2, 1, 0, Some(2)))),
            (3, 0, None),
        ],
        "the template's local index 0 shifts past BOTH unreferenced script rows"
    );
}

/// `sourcesContent` is a parallel concatenation, present iff some row carries
/// content; ignore-list entries shift by the same source base offset.
#[test]
fn sources_content_and_ignore_list_are_carried_and_remapped() {
    let script_map = "{\"version\":3,\"sources\":[\"a.vue\"],\"sourcesContent\":[\"SCRIPT\"],\
                      \"names\":[],\"mappings\":\"AACA\"}";
    let template_map = "{\"version\":3,\"sources\":[\"b.vue\"],\"names\":[],\
                        \"x_google_ignoreList\":[0],\"mappings\":\"AACA\"}";
    let compiled = RuntimeCompileOutput {
        script: Some(script("const x = 1\n", script_map)),
        template: Some(template("function render() {}\n", template_map)),
        ..RuntimeCompileOutput::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let assembled = assemble_vue_main_module("Comp.vue", &compiled, &meta, &mapping_profile())
        .expect("composes");
    let artifact = decode_artifact(
        assembled
            .source_map
            .as_deref()
            .expect("a map was requested"),
    );

    assert_eq!(
        artifact.sources_content,
        Some(vec![Some("SCRIPT".to_string()), None]),
        "the template declared no content, so its row is null rather than absent"
    );
    assert_eq!(
        artifact.ignore_list,
        [1],
        "the template's local entry 0 shifts by the script's one source row"
    );
}

/// An agreed `sourceRoot` carries through; a disagreement fails closed, because
/// a composed map has one root for all rows and dropping or folding it would
/// silently change every declared source identity.
#[test]
fn source_root_agrees_or_fails_closed() {
    let with_root = |root: &str| {
        format!(
            "{{\"version\":3,\"sourceRoot\":\"{root}\",\"sources\":[\"a.vue\"],\
             \"names\":[],\"mappings\":\"AACA\"}}"
        )
    };

    let compiled = RuntimeCompileOutput {
        script: Some(script("const x = 1\n", &with_root("/root"))),
        template: Some(template("function render() {}\n", &with_root("/root"))),
        ..RuntimeCompileOutput::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let assembled = assemble_vue_main_module("Comp.vue", &compiled, &meta, &mapping_profile())
        .expect("agreeing roots compose");
    let artifact = decode_artifact(
        assembled
            .source_map
            .as_deref()
            .expect("a map was requested"),
    );
    assert_eq!(artifact.source_root, Some("/root".to_string()));

    let conflicting = RuntimeCompileOutput {
        script: Some(script("const x = 1\n", &with_root("/one"))),
        template: Some(template("function render() {}\n", &with_root("/two"))),
        ..RuntimeCompileOutput::default()
    };
    let Err(VueMainAssemblyFailure::InputMap(failure)) =
        assemble_vue_main_module("Comp.vue", &conflicting, &meta, &mapping_profile())
    else {
        panic!("disagreeing roots must fail closed with an input-map failure");
    };
    assert_eq!(
        failure,
        AssembleMapFailure::UncomposableInputMap {
            fragment: MapFragment::Template,
            code: UncomposableCode::SourceRootConflict
        }
    );
}

/// A single contributing map's `sourceRoot` carries through unchanged: the
/// agreement runs over the contributing set at ANY cardinality, so a
/// script-only compile does not skip it. `""` is a declared value distinct from
/// absent.
#[test]
fn a_single_contributing_map_carries_its_own_empty_source_root() {
    let (_, artifact) = assemble_script_only(
        "const x = 1\n",
        "{\"version\":3,\"sourceRoot\":\"\",\"sources\":[\"a.vue\"],\"names\":[],\
         \"mappings\":\"AACA\"}",
    );
    assert_eq!(artifact.source_root, Some(String::new()));
    assert!(
        artifact.raw.contains("\"sourceRoot\":\"\""),
        "an empty root is not interpreted as the identity root, got:\n{}",
        artifact.raw
    );
}

/// With a map requested but zero contributing fragments, a map is still
/// produced — empty, with `sourceRoot`, `sourcesContent` and the ignore list
/// all absent. Degrading it to "no map" would make a map-enabled cell
/// indistinguishable from a map-disabled one.
#[test]
fn a_requested_map_is_produced_even_with_zero_contributing_fragments() {
    let compiled = RuntimeCompileOutput::default();
    let assembled = assemble_vue_main_module(
        "Comp.vue",
        &compiled,
        &FileMeta::default(),
        &mapping_profile(),
    )
    .expect("composes");

    let raw = assembled.source_map.expect("a map was requested");
    assert_eq!(
        raw, "{\"version\":3,\"names\":[],\"sources\":[],\"mappings\":\"\"}",
        "the truthful artifact for a module none of whose bytes carry a mapping"
    );
}

/// With no map requested the result carries NO map — asserted positively, not
/// by omitting the check — and a fragment's non-empty map string is ignored
/// rather than composed unasked.
#[test]
fn a_disabled_map_is_absent_rather_than_empty() {
    let compiled = RuntimeCompileOutput {
        script: Some(script(
            "const x = 1\n",
            &map_json("[\"Comp.vue\"]", "[]", "AACA"),
        )),
        ..RuntimeCompileOutput::default()
    };
    let meta = FileMeta {
        has_script: true,
        ..FileMeta::default()
    };
    let assembled =
        assemble_vue_main_module("Comp.vue", &compiled, &meta, &CompileProfile::default())
            .expect("composes");

    assert_eq!(assembled.source_map, None);
}

/// The map-disabled and map-enabled paths produce the SAME bytes: composing a
/// map alongside the code does not change which code is produced.
#[test]
fn the_assembled_code_is_identical_with_and_without_a_map() {
    let compiled = RuntimeCompileOutput {
        script: Some(script(
            "const __sfc__ = _defineComponent({});\nexport default __sfc__;\n",
            &map_json("[\"Comp.vue\"]", "[]", "AACA"),
        )),
        template: Some(template(
            "function render(_ctx) {\n  return 1\n}",
            &map_json("[\"Comp.vue\"]", "[]", "AACA"),
        )),
        ..RuntimeCompileOutput::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };

    let with_map = assemble_vue_main_module("Comp.vue", &compiled, &meta, &mapping_profile())
        .expect("composes");
    let without_map =
        assemble_vue_main_module("Comp.vue", &compiled, &meta, &CompileProfile::default())
            .expect("composes");

    assert_eq!(with_map.code, without_map.code);
    assert!(with_map.source_map.is_some() && without_map.source_map.is_none());
}

/// `rewrite_script` applies ONLY the ranges a producer-declared
/// `SfcExportPlacement` fact names — never a scan for the `__sfc__` /
/// `export default` landmark strings. Every declared binding is renamed;
/// the declared export statement (including its own internal binding) is
/// removed wholesale, not separately renamed then removed.
/// `authored_text_matching_the_landmarks_is_left_untouched` below is the
/// companion collision proof: an UNDECLARED occurrence of either landmark
/// string is left untouched.
#[test]
fn rewrite_applies_only_the_declared_ranges() {
    use verter_compiler::assembly::SfcExportPlacement;

    // A binding NOT part of the export statement, plus the export
    // statement's own internal binding — the general two-binding shape a
    // real producer declares.
    let code = "const __sfc__ = {}\n__sfc__.__scopeId = \"x\";\nexport default __sfc__;\n";
    let first_start = code.find("__sfc__").unwrap() as u32;
    let first = first_start..first_start + 7;
    let second_start = code[first.end as usize..].find("__sfc__").unwrap() as u32 + first.end;
    let second = second_start..second_start + 7;
    let export_start = code.find("export default").unwrap() as u32;
    let export = export_start..code.len() as u32;
    let export_binding = export_start + "export default ".len() as u32;
    let export_binding = export_binding..export_binding + 7;
    let fact = SfcExportPlacement {
        binding_ranges: vec![first, second, export_binding],
        export_statement_range: Some(export),
    };
    let (rewritten, _) = super::map_compose::rewrite_script(code, Some(&fact), None)
        .expect("a fact whose declared ranges match the script's own bytes is accepted");
    assert_eq!(
        rewritten, "const _sfc_main = {}\n_sfc_main.__scopeId = \"x\";\n",
        "every declared binding is renamed, and the declared export statement \
         (including its own internal binding) is removed wholesale, not \
         separately renamed then removed"
    );
}

/// Authored source text containing the literal strings `__sfc__` or
/// `export default _sfc_main` is left untouched when no fact declares it as
/// a rename/removal target — `rewrite_script` acts only on declared ranges,
/// never on an incidental text match.
#[test]
fn authored_text_matching_the_landmarks_is_left_untouched() {
    use verter_compiler::assembly::SfcExportPlacement;

    let code = "const __sfc__ = {}\n\
                 const decoy = \"__sfc__\";\n\
                 const other = \"export default _sfc_main;\";\n\
                 export default __sfc__;\n";
    let binding = 6..13; // the ONLY declared binding: the real one.
    let export_start = code.rfind("export default __sfc__;\n").unwrap() as u32;
    let export = export_start..export_start + "export default __sfc__;\n".len() as u32;
    let export_binding_start = export_start + "export default ".len() as u32;
    let export_binding = export_binding_start..export_binding_start + 7;
    let fact = SfcExportPlacement {
        binding_ranges: vec![binding, export_binding],
        export_statement_range: Some(export),
    };
    let (rewritten, _) = super::map_compose::rewrite_script(code, Some(&fact), None)
        .expect("a fact whose declared ranges match the script's own bytes is accepted");
    assert!(
        rewritten.contains("const decoy = \"__sfc__\";"),
        "an UNDECLARED `__sfc__` occurrence inside authored text must survive \
         verbatim, got:\n{rewritten}"
    );
    assert!(
        rewritten.contains("const other = \"export default _sfc_main;\";"),
        "an UNDECLARED `export default _sfc_main` occurrence inside authored \
         text must survive verbatim, got:\n{rewritten}"
    );
    assert!(
        rewritten.contains("const _sfc_main = {}"),
        "the DECLARED binding is still renamed, got:\n{rewritten}"
    );
    assert!(
        !rewritten.contains("export default _sfc_main;\n\n") && rewritten.ends_with('\n'),
        "the DECLARED export statement is still removed, got:\n{rewritten}"
    );
}

/// A declared range whose bytes do not match the fact's claim (a producer
/// defect) is a typed refusal — never silently rescanned or half-applied.
#[test]
fn inconsistent_declared_range_is_a_typed_refusal() {
    use verter_compiler::assembly::SfcExportPlacement;

    let code = "const __sfc__ = {}\n";
    let wrong_range = 0..7; // "const _" — not "__sfc__"
    let fact = SfcExportPlacement {
        binding_ranges: vec![wrong_range],
        export_statement_range: None,
    };
    let err = super::map_compose::rewrite_script(code, Some(&fact), None).unwrap_err();
    assert!(
        matches!(
            err,
            super::map_compose::SfcRewriteRefusal::InconsistentBindingRange { start: 0, end: 7 }
        ),
        "got {err:?}"
    );
}

/// A missing fact (`None`) is indistinguishable, without scanning, from a
/// genuinely empty declared fact — `rewrite_script` treats it the same:
/// zero edits, `code` returned verbatim. Never a refusal (a script with no
/// `__sfc__` at all is legitimate — e.g. a fixture built purely to exercise
/// unrelated map-composition mechanics) and never a scan to find out which
/// case it is.
#[test]
fn missing_fact_is_treated_as_an_empty_fact() {
    let code = "const x = 1\n";
    let (rewritten, _) = super::map_compose::rewrite_script(code, None, None)
        .expect("a missing fact is never itself a refusal");
    assert_eq!(rewritten, code);
}

/// `rewrite_script` chains its own overwrite-only transform onto the
/// caller-supplied script map (`CodeTransform::chain_source_map`), which
/// genuinely returns failures for a map whose declared generated position
/// does not exist in the transform's own text. Exercised DIRECTLY against
/// `rewrite_script` (not through [`super::assemble_vue_main_module`]):
/// through the one production call site, `map_input::validate_and_decode`'s
/// own generated-position bound check (step 1.24) already rejects any
/// segment naming a nonexistent line/column against this SAME text
/// (`script.code`) before `rewrite_script` ever runs, so a genuinely
/// out-of-bounds segment cannot reach this call by way of
/// `assemble_vue_main_module`'s public entry point today. `rewrite_script`'s
/// own signature does not, and must not, assume its `map` argument was
/// already validated against `code` — this proves the typed refusal fires
/// (not a panic) when that invariant is broken directly, which is exactly
/// the discipline the removed `.expect()` violated.
#[test]
fn chain_source_map_failure_is_a_typed_refusal_not_a_panic() {
    let code = "const x = 1\n"; // one line, plus the trailing-newline empty line
    let map = DecodedFragmentMap {
        sources: vec!["a.vue".to_string()],
        names: Vec::new(),
        sources_content: None,
        source_root: None,
        ignore_list: Vec::new(),
        segments: vec![WireSegment {
            // `code` has only lines 0 and 1 (the trailing empty line); line 99
            // names a generated position `chain_source_map`'s own text tiling
            // cannot resolve.
            generated_line: 99,
            generated_column: 0,
            payload: None,
        }],
    };
    match super::map_compose::rewrite_script(code, None, Some(&map)) {
        Err(super::map_compose::SfcRewriteRefusal::ChainFailed(_)) => {}
        other => panic!("expected a typed ChainFailed refusal, got: {other:?}"),
    }
}

/// Provenance: the composed map's wire form carries no fragment-identity
/// tag. `assemble_sequence`'s own `SequenceTables`/`Token` composition has
/// no such field to serialize — the composed artifact stays plain
/// oxc-sourcemap JSON with no extra member.
#[test]
fn provenance_never_reaches_the_wire() {
    let compiled = RuntimeCompileOutput {
        script: Some(script(
            "const x = 1\n",
            &map_json("[\"a.vue\"]", "[]", "AACA"),
        )),
        template: Some(template(
            "function r() {}\n",
            &map_json("[\"b.vue\"]", "[]", "AACA"),
        )),
        ..RuntimeCompileOutput::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };
    let assembled =
        assemble_vue_main_module("Comp.vue", &compiled, &meta, &mapping_profile()).unwrap();
    let raw = assembled.source_map.expect("map requested");
    for token in [
        "Script",
        "Template",
        "AssemblyBoundary",
        "origin",
        "provenance",
    ] {
        assert!(
            !raw.contains(token),
            "provenance must not reach the wire, found {token:?} in:\n{raw}"
        );
    }
}

// Fail-closed validation

/// A fragment that is both AUTHORED and PRESENT must carry a map. This is a
/// distinct outcome from the eight uncomposable families, because a missing map
/// and an uncomposable map have different owners.
#[test]
fn an_authored_and_present_fragment_without_a_map_fails_closed() {
    assert_eq!(
        expect_failure(
            Some(script("const x = 1\n", "")),
            None,
            FileMeta {
                has_script: true,
                ..FileMeta::default()
            },
        ),
        AssembleMapFailure::MissingRequiredInputMap {
            fragment: MapFragment::Script
        }
    );

    assert_eq!(
        expect_failure(
            None,
            Some(template("function render() {}\n", "")),
            FileMeta {
                has_template: true,
                ..FileMeta::default()
            },
        ),
        AssembleMapFailure::MissingRequiredInputMap {
            fragment: MapFragment::Template
        }
    );
}

/// A fragment that is AUTHORED but NOT PRESENT requires nothing, because it
/// contributes no bytes — the inline topology, where the render closure lives
/// inside `setup()` and no template block exists.
#[test]
fn an_authored_but_absent_fragment_requires_no_map() {
    let compiled = RuntimeCompileOutput {
        script: Some(script(
            "const x = 1\n",
            &map_json("[\"Comp.vue\"]", "[]", "AACA"),
        )),
        template: None,
        ..RuntimeCompileOutput::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };

    assert!(assemble_vue_main_module("Comp.vue", &compiled, &meta, &mapping_profile()).is_ok());
}

/// Every sub-code of the taxonomy is reachable from a real input, and each
/// belongs to the family the taxonomy assigns it.
#[test]
fn every_uncomposable_sub_code_is_reachable_from_a_real_input() {
    use UncomposableCode as C;

    let cases: Vec<(&str, C)> = vec![
        ("{ not json", C::MapBytesNotJson),
        ("[1,2,3]", C::MapRootNotObject),
        (
            "{\"version\":3,\"sources\":[],\"names\":[]}",
            C::MappingsMemberAbsent,
        ),
        (
            "{\"version\":3,\"sources\":[],\"names\":[],\"mappings\":7}",
            C::MappingsMemberNotAString,
        ),
        (
            "{\"version\":3,\"names\":[],\"mappings\":\"\"}",
            C::SourcesMemberAbsentOrNotAnArray,
        ),
        (
            "{\"version\":3,\"sources\":[],\"mappings\":\"\"}",
            C::NamesMemberAbsentOrNotAnArray,
        ),
        (
            "{\"version\":3,\"sources\":[],\"names\":[],\"mappings\":\"\",\"sourceRoot\":7}",
            C::MetadataMemberWrongType,
        ),
        (
            "{\"version\":3,\"version\":3,\"sources\":[],\"names\":[],\"mappings\":\"\"}",
            C::DuplicateObjectMember,
        ),
        (
            "{\"version\":1e400,\"sources\":[],\"names\":[],\"mappings\":\"\"}",
            C::NumberOutsideInteroperableDomain,
        ),
        (
            "{\"version\":3,\"sources\":[\"\\uD800\"],\"names\":[],\"mappings\":\"\"}",
            C::StringNotWellFormedUnicode,
        ),
        (
            "{\"sources\":[],\"names\":[],\"mappings\":\"\"}",
            C::VersionMemberAbsent,
        ),
        (
            "{\"version\":\"3\",\"sources\":[],\"names\":[],\"mappings\":\"\"}",
            C::VersionNotAnInteger,
        ),
        (
            "{\"version\":2,\"sources\":[],\"names\":[],\"mappings\":\"\"}",
            C::VersionNot3,
        ),
        (
            "{\"version\":3,\"sources\":[],\"names\":[],\"mappings\":\"A!A\"}",
            C::VlqInvalidCharacter,
        ),
        (
            "{\"version\":3,\"sources\":[],\"names\":[],\"mappings\":\"g\"}",
            C::VlqTruncatedSegment,
        ),
        (
            "{\"version\":3,\"sources\":[],\"names\":[],\"mappings\":\"AC\"}",
            C::SegmentFieldCount,
        ),
        (
            "{\"version\":3,\"sources\":[],\"names\":[],\"mappings\":\"ggggggE\"}",
            C::VlqFieldOutOfRange,
        ),
        (
            "{\"version\":3,\"sources\":[],\"names\":[],\"mappings\":\"D\"}",
            C::AccumulatorOutOfRange,
        ),
        (
            "{\"version\":3,\"sources\":[],\"names\":[],\"mappings\":\"K,F\"}",
            C::GeneratedColumnAccumulatorDecreased,
        ),
        (
            "{\"version\":3,\"sources\":[7],\"names\":[],\"mappings\":\"\"}",
            C::SourceRowNotAString,
        ),
        (
            "{\"version\":3,\"sources\":[],\"names\":[7],\"mappings\":\"\"}",
            C::NameRowNotAString,
        ),
        (
            "{\"version\":3,\"sources\":[\"a\"],\"sourcesContent\":[7],\"names\":[],\
             \"mappings\":\"\"}",
            C::SourcesContentRowNotStringOrNull,
        ),
        (
            "{\"version\":3,\"sources\":[\"a\"],\"sourcesContent\":[],\"names\":[],\
             \"mappings\":\"\"}",
            C::SourcesContentLengthMismatch,
        ),
        ("{\"version\":3,\"sections\":[]}", C::SectionsMemberPresent),
        (
            "{\"version\":3,\"sources\":[\"a\"],\"names\":[],\"mappings\":\"ACAA\"}",
            C::SourceIndexOutOfTable,
        ),
        (
            "{\"version\":3,\"sources\":[\"a\"],\"names\":[],\"mappings\":\"AAAAC\"}",
            C::NameIndexOutOfTable,
        ),
        (
            "{\"version\":3,\"sources\":[\"a\"],\"names\":[],\"x_google_ignoreList\":[4],\
             \"mappings\":\"\"}",
            C::IgnoreListIndexOutOfTable,
        ),
        (
            // §4.3 step 1.15 places no upper bound on a legally-typed ignore-list
            // entry — only "non-negative integral" — and defers the actual bound
            // to step 1.23 (U6.3) against `sources.length`. An entry beyond
            // i32::MAX is still a legally-TYPED entry; on an empty `sources`
            // table it is out of range at U6.3, not a wrong-type rejection.
            "{\"version\":3,\"sources\":[],\"names\":[],\"ignoreList\":[2147483648],\
             \"mappings\":\"\"}",
            C::IgnoreListIndexOutOfTable,
        ),
        (
            "{\"version\":3,\"sources\":[\"a\"],\"names\":[],\"mappings\":\";;;;;AAAA\"}",
            C::GeneratedLineOutOfFragment,
        ),
        (
            "{\"version\":3,\"sources\":[\"a\"],\"names\":[],\"mappings\":\"yBAAA\"}",
            C::GeneratedColumnOutOfFragment,
        ),
    ];

    for (raw, expected) in cases {
        assert_eq!(
            expect_script_code(raw),
            expected,
            "input {raw:?} must report {}",
            expected.as_str()
        );
    }

    // The surrogate-split column needs a fragment containing an astral
    // character, so it does not fit the shared `const x = 1\n` fixture.
    let compiled = RuntimeCompileOutput {
        script: Some(script(
            "\u{1D400}x\n",
            // A single segment at generated column 1, between the two halves.
            &map_json("[\"a\"]", "[]", "CAAA"),
        )),
        ..RuntimeCompileOutput::default()
    };
    let meta = FileMeta {
        has_script: true,
        ..FileMeta::default()
    };
    let Err(VueMainAssemblyFailure::InputMap(failure)) =
        assemble_vue_main_module("Comp.vue", &compiled, &meta, &mapping_profile())
    else {
        panic!("a surrogate-splitting column must fail closed with an input-map failure");
    };
    assert_eq!(
        failure,
        AssembleMapFailure::UncomposableInputMap {
            fragment: MapFragment::Script,
            code: C::GeneratedColumnSplitsASurrogatePair
        }
    );

    // Family assignment is exhaustive over the taxonomy.
    assert_eq!(
        C::MapBytesNotJson.family(),
        UncomposableFamily::MalformedJson
    );
    assert_eq!(C::VersionNot3.family(), UncomposableFamily::Version);
    assert_eq!(C::SegmentFieldCount.family(), UncomposableFamily::WireData);
    assert_eq!(
        C::SourceRowNotAString.family(),
        UncomposableFamily::TableRows
    );
    assert_eq!(
        C::SectionsMemberPresent.family(),
        UncomposableFamily::IndexedMap
    );
    assert_eq!(
        C::SourceIndexOutOfTable.family(),
        UncomposableFamily::DanglingIndex
    );
    assert_eq!(
        C::GeneratedLineOutOfFragment.family(),
        UncomposableFamily::Coordinate
    );
    assert_eq!(
        C::SourceRootConflict.family(),
        UncomposableFamily::CrossFragmentMetadata
    );
}

/// The stage tie-breaks the validation order fixes, each with an input for
/// which BOTH conditions hold so the answer is only determined by the order.
#[test]
fn the_validation_order_decides_inputs_for_which_several_checks_hold() {
    // Duplicate-member detection precedes every member read, so no later check
    // can silently read whichever duplicate a parser happened to keep.
    assert_eq!(
        expect_script_code(
            "{\"version\":3,\"version\":2,\"sources\":[],\"names\":[],\"mappings\":\"\"}"
        ),
        UncomposableCode::DuplicateObjectMember
    );

    // Numbers are checked before strings.
    assert_eq!(
        expect_script_code(
            "{\"version\":1e400,\"sources\":[\"\\uD800\"],\"names\":[],\"mappings\":\"\"}"
        ),
        UncomposableCode::NumberOutsideInteroperableDomain
    );

    // Version beats indexed-map.
    assert_eq!(
        expect_script_code("{\"version\":2,\"sections\":[],\"sources\":[],\"names\":[]}"),
        UncomposableCode::VersionNot3
    );

    // Indexed-map beats missing `mappings`: an indexed map legitimately has none.
    assert_eq!(
        expect_script_code("{\"version\":3,\"sections\":[],\"sources\":[],\"names\":[]}"),
        UncomposableCode::SectionsMemberPresent
    );

    // Row typing beats wire decoding.
    assert_eq!(
        expect_script_code("{\"version\":3,\"sources\":[7],\"names\":[],\"mappings\":\"!\"}"),
        UncomposableCode::SourceRowNotAString
    );

    // `sources` rows beat `names` rows.
    assert_eq!(
        expect_script_code("{\"version\":3,\"sources\":[7],\"names\":[7],\"mappings\":\"\"}"),
        UncomposableCode::SourceRowNotAString
    );

    // Arity beats every accumulator property: a two-field segment whose first
    // field also drives the column accumulator negative reports the arity,
    // because a two-field segment has no interpretation at all.
    assert_eq!(
        expect_script_code("{\"version\":3,\"sources\":[\"a\"],\"names\":[],\"mappings\":\"DC\"}"),
        UncomposableCode::SegmentFieldCount
    );

    // Within accumulator application, range beats ordering: a column driven
    // NEGATIVE is out of range, while one that merely decreases is a decrease.
    assert_eq!(
        expect_script_code("{\"version\":3,\"sources\":[\"a\"],\"names\":[],\"mappings\":\"K,N\"}"),
        UncomposableCode::AccumulatorOutOfRange
    );
    assert_eq!(
        expect_script_code("{\"version\":3,\"sources\":[\"a\"],\"names\":[],\"mappings\":\"K,J\"}"),
        UncomposableCode::GeneratedColumnAccumulatorDecreased
    );

    // Index bounds beat coordinate bounds, as a STAGE precedence: segment 0's
    // column is far past the end of `const x = 1`, and segment 1's source index
    // has no row — the LATER index violation still wins.
    assert_eq!(
        expect_script_code(&map_json(
            "[\"a\"]",
            "[]",
            &encode_mappings(&[
                (0, 25, Some((0, 0, 0, None))),
                (0, 26, Some((1, 0, 0, None))),
            ]),
        )),
        UncomposableCode::SourceIndexOutOfTable
    );
    // …and with the index violation removed, the coordinate violation is what
    // the same input reports, so the precedence above is doing real work.
    assert_eq!(
        expect_script_code(&map_json(
            "[\"a\"]",
            "[]",
            &encode_mappings(&[
                (0, 25, Some((0, 0, 0, None))),
                (0, 26, Some((0, 0, 0, None))),
            ]),
        )),
        UncomposableCode::GeneratedColumnOutOfFragment
    );

    // Script beats template: a malformed script map and a dangling-index
    // template map report the script's outcome.
    match expect_failure(
        Some(script("const x = 1\n", "{ not json")),
        Some(template(
            "function render() {}\n",
            &map_json("[\"a\"]", "[]", "ACAA"),
        )),
        FileMeta {
            has_script: true,
            has_template: true,
            ..FileMeta::default()
        },
    ) {
        AssembleMapFailure::UncomposableInputMap { fragment, code } => {
            assert_eq!(fragment, MapFragment::Script);
            assert_eq!(code, UncomposableCode::MapBytesNotJson);
        }
        other => panic!("expected the script's outcome, got {other:?}"),
    }
}

/// Every staged tie-break has a LIVE loser.
///
/// [`the_validation_order_decides_inputs_for_which_several_checks_hold`] asserts
/// which check WINS on an input several checks hold for. On its own that is
/// weaker than it looks: if the losing check would not have fired anyway, the
/// winner wins by default and the stage order is carrying no weight — the test
/// would keep passing with the order reversed, or with the loser deleted.
///
/// Each row below pairs a tie-break input with the SAME input minus the
/// winner's own trigger, and asserts the loser then reports. Together the two
/// halves say: both checks are armed, and the order is what decides. Weakening
/// or reordering either check fails one half.
///
/// Three pairs already live elsewhere and are not restated: version-beats-
/// indexed-map (in the tie-break test above, which asserts both halves),
/// and `sourceRoot` agreement, whose companion — the same two fragments
/// composing once the roots agree — is
/// [`source_root_agrees_or_fails_closed`]. The index-bounds/coordinate-bounds
/// pair IS restated here, because it is the only live-loser evidence for
/// families `U6` and `U7` and this test is where that property is audited.
#[test]
fn every_staged_tie_break_has_a_live_loser() {
    use UncomposableCode as C;

    // `(what the order decides, the tie-break input, its winner, the same
    // input with the winner's trigger removed, the loser it then reports)`.
    let pairs: &[(&str, &str, C, &str, C)] = &[
        (
            // Step 1.2 before 1.4-1.6. The loser is only reachable at all
            // under a last-wins object model, which is precisely what
            // `DECISION` D-2 refuses to let a parser decide.
            "duplicate-member beats version",
            "{\"version\":3,\"version\":2,\"sources\":[],\"names\":[],\"mappings\":\"\"}",
            C::DuplicateObjectMember,
            "{\"version\":2,\"sources\":[],\"names\":[],\"mappings\":\"\"}",
            C::VersionNot3,
        ),
        (
            // Step 1.7 before 1.8: an indexed map legitimately has no
            // `mappings`, so the loser would fire on every such document.
            "indexed-map beats missing `mappings`",
            "{\"version\":3,\"sections\":[],\"sources\":[],\"names\":[]}",
            C::SectionsMemberPresent,
            "{\"version\":3,\"sources\":[],\"names\":[]}",
            C::MappingsMemberAbsent,
        ),
        (
            // Steps 1.17-1.20 before 1.21: index-bounds and coordinate checks
            // presuppose a typed table, so row typing runs first.
            "row typing beats wire decoding",
            "{\"version\":3,\"sources\":[7],\"names\":[],\"mappings\":\"!\"}",
            C::SourceRowNotAString,
            "{\"version\":3,\"sources\":[\"a\"],\"names\":[],\"mappings\":\"!\"}",
            C::VlqInvalidCharacter,
        ),
        (
            // §1.21 phase b before phase c. `"DC"` is two fields whose first would
            // also drive the column accumulator negative; dropping the second
            // field leaves the accumulator violation alone.
            "arity beats accumulator range",
            "{\"version\":3,\"sources\":[\"a\"],\"names\":[],\"mappings\":\"DC\"}",
            C::SegmentFieldCount,
            "{\"version\":3,\"sources\":[\"a\"],\"names\":[],\"mappings\":\"D\"}",
            C::AccumulatorOutOfRange,
        ),
        (
            // §1.21 phase b before step 1.22 — the distinction seed vector F5
            // depends on. `"AC"` is a two-field segment; `"ACAA"` is the
            // well-formed four-field version of the same dangling index.
            "arity beats index bounds",
            "{\"version\":3,\"sources\":[\"a\"],\"names\":[],\"mappings\":\"AC\"}",
            C::SegmentFieldCount,
            "{\"version\":3,\"sources\":[\"a\"],\"names\":[],\"mappings\":\"ACAA\"}",
            C::SourceIndexOutOfTable,
        ),
    ];

    for (decides, tie_break, winner, reduced, loser) in pairs {
        assert_eq!(
            expect_script_code(tie_break),
            *winner,
            "{decides}: the tie-break input must report {}",
            winner.as_str()
        );
        assert_eq!(
            expect_script_code(reduced),
            *loser,
            "{decides}: with the winner's trigger removed the SAME input must \
             report {} — otherwise the loser was never armed and the tie-break \
             above proves nothing about the order",
            loser.as_str()
        );
    }

    // Step 1.22 before 1.24, across segments: segment 0's column is far past
    // the end of `const x = 1` and segment 1's source index has no row, so the
    // LATER index violation wins. Removing the index violation leaves the
    // coordinate violation, which is what makes the precedence load-bearing.
    let two_segments = |second_source: u32| {
        map_json(
            "[\"a\"]",
            "[]",
            &encode_mappings(&[
                (0, 25, Some((0, 0, 0, None))),
                (0, 26, Some((second_source, 0, 0, None))),
            ]),
        )
    };
    assert_eq!(
        expect_script_code(&two_segments(1)),
        C::SourceIndexOutOfTable,
        "index bounds beat coordinate bounds"
    );
    assert_eq!(
        expect_script_code(&two_segments(0)),
        C::GeneratedColumnOutOfFragment,
        "with the index violation removed the coordinate violation is what the \
         same input reports, so the precedence above is doing real work"
    );
}

/// A sourceless segment carries null in every authored field, and null is in no
/// index range — so the dangling-index checks must be guarded on the field
/// being non-null. An unguarded check would reject every one-field segment and
/// take the whole barrier algebra with it.
#[test]
fn a_sourceless_segment_is_never_a_dangling_index() {
    let (_, artifact) = assemble_script_only(
        "const x = 1\n",
        // An empty table and one sourceless segment.
        &map_json("[]", "[]", "A"),
    );
    assert_eq!(artifact.sources, Vec::<String>::new());
    assert_eq!(artifact.segments, [(0, 0, None), (1, 0, None)]);
}

/// Two ignore-list spellings are one field: both are accepted, and they must
/// agree.
#[test]
fn disagreeing_ignore_list_spellings_are_rejected() {
    assert_eq!(
        expect_script_code(
            "{\"version\":3,\"sources\":[\"a\"],\"names\":[],\"ignoreList\":[0],\
             \"x_google_ignoreList\":[],\"mappings\":\"\"}"
        ),
        UncomposableCode::MetadataMemberWrongType
    );

    let (_, artifact) = assemble_script_only(
        "const x = 1\n",
        "{\"version\":3,\"sources\":[\"a\"],\"names\":[],\"ignoreList\":[0],\
         \"x_google_ignoreList\":[0],\"mappings\":\"AACA\"}",
    );
    assert_eq!(artifact.ignore_list, [0]);
}

/// The two-spelling agreement check compares the CONVERTED binary64 values —
/// never a narrowed integer representation. 2^64 and 2^65 are both exactly
/// representable, legally-typed (non-negative, integral) binary64 entries, but
/// neither fits an unsigned 64-bit integer: a saturating pre-narrow collapses
/// both to the same value and wrongly reports the two spellings as agreeing,
/// letting the input fall through to the table-bounds check instead of the
/// wrong-type rejection the disagreement demands.
#[test]
fn disagreeing_ignore_list_spellings_beyond_u64_are_rejected_as_wrong_type() {
    assert_eq!(
        expect_script_code(
            "{\"version\":3,\"sources\":[\"a\"],\"names\":[],\
             \"ignoreList\":[18446744073709551616],\
             \"x_google_ignoreList\":[36893488147419103232],\"mappings\":\"\"}"
        ),
        UncomposableCode::MetadataMemberWrongType
    );
}

/// Generated-side metadata describes a document that no longer exists once
/// fragments have been assembled into a different module, so it is dropped.
/// Source-side metadata is carried.
#[test]
fn generated_side_metadata_is_dropped() {
    let (_, artifact) = assemble_script_only(
        "const x = 1\n",
        "{\"version\":3,\"file\":\"Comp.vue\",\"debugId\":\"abc\",\"sources\":[\"a.vue\"],\
         \"names\":[],\"mappings\":\"AACA\"}",
    );

    assert!(
        !artifact.raw.contains("\"file\"") && !artifact.raw.contains("debugId"),
        "inheriting a fragment's generated-side identity would be a false \
         claim about the assembled module, got:\n{}",
        artifact.raw
    );
    assert_eq!(artifact.sources, ["a.vue"]);
}

/// Original coordinates are NOT validated: a mechanically composable but
/// implausible authored coordinate is carried forward faithfully rather than
/// rejected, because this layer holds no authored file to check it against.
#[test]
fn implausible_authored_coordinates_are_carried_not_rejected() {
    let (_, artifact) = assemble_script_only(
        "const x = 1\n",
        &map_json(
            "[\"a.vue\"]",
            "[]",
            &encode_mappings(&[seg(0, 0, 500, 900)]),
        ),
    );
    assert_eq!(
        artifact.segments,
        [(0, 0, Some((0, 500, 900, None))), (1, 0, None)],
        "carried verbatim: this layer holds no authored file to check it against"
    );
}

/// Serialization is deterministic across repeated identical invocations,
/// because the assembled hash is computed over raw serialized bytes: two valid
/// but differently encoded serializations of one logical artifact would defeat
/// that hash even though both would pass a decoded comparison.
#[test]
fn serialization_is_deterministic_across_identical_invocations() {
    let compiled = RuntimeCompileOutput {
        script: Some(script(
            "const __sfc__ = {}\nexport default __sfc__;\n",
            "{\"version\":3,\"sourceRoot\":\"/r\",\"sources\":[\"a.vue\"],\
             \"sourcesContent\":[\"AUTHORED\"],\"names\":[\"n\"],\"ignoreList\":[0],\
             \"mappings\":\"AACAA,MAAM\"}",
        )),
        template: Some(template(
            "function render() {}\n",
            "{\"version\":3,\"sourceRoot\":\"/r\",\"sources\":[\"b.vue\"],\"names\":[],\
             \"mappings\":\"SACS\"}",
        )),
        ..RuntimeCompileOutput::default()
    };
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        ..FileMeta::default()
    };

    let first = assemble_vue_main_module("Comp.vue", &compiled, &meta, &mapping_profile())
        .expect("composes");
    for _ in 0..4 {
        let again = assemble_vue_main_module("Comp.vue", &compiled, &meta, &mapping_profile())
            .expect("composes");
        assert_eq!(first, again);
    }
}

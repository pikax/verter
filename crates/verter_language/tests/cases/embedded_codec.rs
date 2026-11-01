//! The embedded text codec's corpus: profile-owned results over shared
//! primitives, typed cooked partiality, exact authored maps, composition,
//! incremental equivalence and deterministic work.
//!
//! Every policy here is test data shaped like a real literal syntax; the codec
//! itself knows no language. Expected geometry is written by hand or derived
//! from a generator's own record of what it spelled, never from the decoder.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{ContentId, SourceId, SourceUnitId};
use verter_identity::mapping::MapRevision;
use verter_identity::successor::{
    AttachmentId, CarrierProfileId, FrameworkProfileId, ProfileSchemaEpoch, RegionId,
    ReleaseCoordinates, ReleaseId,
};
use verter_language::embedded::{
    AuthoredMapChain, BaseUri, CodecRefusal, CookedPartiality, CookedPolicy, CookedText,
    CoordinateDomain, DecodeInterrupt, DecodeRequest, DecodedLiteral, DelimiterPolicy,
    DomainOffset, DomainRange, DuplicateEmbeddedRegionRow, EditCertification, Elision, ElisionKind,
    EmbeddedAttachment, EmbeddedRegionRow, EmbeddedRegionTable, EmbeddedTextCodec, EscapePolicy,
    GeometryError, HoleGeometry, HolePolicy, IdentityEscapes, IndentationPolicy,
    LineTerminatorPolicy, LiteralEdit, LiteralGeometry, MapBasis, MapSegment, MapStage,
    MapStageError, MissingCookedValue, NestingBudget, NestingExhausted, NeverInterrupted,
    NonUnicodeCookedLiteral, NumericEscape, OriginRef, PlaceholderSpelling, ProfileHoleClass,
    RegionLineage, ReverseEncoding, SegmentOrigin, SimpleEscape, StageKind, StageSegments,
    SurrogateKind, TextSelection, UnencodableReplacement, Unmappable, WholeKind,
};
use verter_span::{
    CoordError, EmbeddedByteOffset, EmbeddedByteRange, GeneratedByteOffset, GeneratedByteRange,
    SourceByteOffset, SourceByteRange,
};

// ---------------------------------------------------------------- allocator

/// Counts this thread's allocation calls and requested bytes. Thread-local, so
/// other tests in the binary cannot disturb a measured window, and inert for
/// everything that does not read it.
struct CountingAllocator;

thread_local! {
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    static ALLOCATED_BYTES: Cell<u64> = const { Cell::new(0) };
}

fn count_allocation(bytes: usize) {
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
    let _ = ALLOCATED_BYTES.try_with(|total| total.set(total.get() + bytes as u64));
}

// SAFETY: every call delegates unchanged to the system allocator; the counters
// use `const`-initialized thread-locals, which never allocate.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count_allocation(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count_allocation(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// `(allocation calls, requested bytes)` during `work`.
fn measured<T>(work: impl FnOnce() -> T) -> (T, u64, u64) {
    let (calls, bytes) = (ALLOCATIONS.get(), ALLOCATED_BYTES.get());
    let value = work();
    (
        value,
        ALLOCATIONS.get() - calls,
        ALLOCATED_BYTES.get() - bytes,
    )
}

// ------------------------------------------------------------------ fixtures

struct Descriptor(&'static str);

impl CanonicalEncode for Descriptor {
    const DOMAIN_TAG: &'static str = "embedded-codec-test.descriptor.v1";
    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, self.0);
    }
}

fn unit(name: &'static str) -> SourceUnitId {
    SourceUnitId::from_lineage(&SourceId::from_canonical(&Descriptor(name)), "module")
}

fn map_revision(name: &'static str) -> MapRevision {
    MapRevision::from_canonical(&Descriptor(name))
}

fn host() -> SourceUnitId {
    unit("host.ts")
}

fn basis_of(source: &str) -> MapBasis {
    MapBasis {
        unit: host(),
        domain: CoordinateDomain::Source,
        content: ContentId::from_content_bytes(source.as_bytes()),
        map_revision: map_revision("authored"),
    }
}

fn basis(
    unit: SourceUnitId,
    domain: CoordinateDomain,
    content: &str,
    revision: &'static str,
) -> MapBasis {
    MapBasis {
        unit,
        domain,
        content: ContentId::from_content_bytes(content.as_bytes()),
        map_revision: map_revision(revision),
    }
}

fn root_lineage() -> RegionLineage {
    RegionLineage::root(host(), RegionId::from_region_role(&host(), "literal"))
}

fn sr(start: u32, end: u32) -> SourceByteRange {
    SourceByteRange::new(SourceByteOffset::new(start), SourceByteOffset::new(end))
}

fn er(start: u32, end: u32) -> EmbeddedByteRange {
    EmbeddedByteRange::new(EmbeddedByteOffset::new(start), EmbeddedByteOffset::new(end))
}

fn gr(start: u32, end: u32) -> GeneratedByteRange {
    GeneratedByteRange::new(
        GeneratedByteOffset::new(start),
        GeneratedByteOffset::new(end),
    )
}

fn eo(offset: u32) -> EmbeddedByteOffset {
    EmbeddedByteOffset::new(offset)
}

fn so(offset: u32) -> SourceByteOffset {
    SourceByteOffset::new(offset)
}

fn digit_chars() -> Box<[char]> {
    Box::new(['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'])
}

fn js_escapes() -> EscapePolicy {
    let simple = |marker, cooked| SimpleEscape {
        marker,
        cooked,
        invalid_before: Box::new([]),
    };
    EscapePolicy {
        simple: Box::new([
            simple('n', '\n'),
            simple('t', '\t'),
            simple('r', '\r'),
            SimpleEscape {
                marker: '0',
                cooked: '\0',
                invalid_before: digit_chars(),
            },
        ]),
        numeric: Box::new([
            NumericEscape::Braced {
                marker: 'u',
                open: '{',
                close: '}',
                radix: 16,
            },
            NumericEscape::Fixed {
                marker: 'u',
                radix: 16,
                digits: 4,
            },
            NumericEscape::Fixed {
                marker: 'x',
                radix: 16,
                digits: 2,
            },
        ]),
        invalid_markers: Box::new(['1', '2', '3', '4', '5', '6', '7', '8', '9']),
        identity: IdentityEscapes::AnyOther,
        continuations: Box::new([
            "\n".into(),
            "\r".into(),
            "\r\n".into(),
            "\u{2028}".into(),
            "\u{2029}".into(),
        ]),
    }
}

fn cooked_policy(
    line_terminators: LineTerminatorPolicy,
    indentation: IndentationPolicy,
    reverse: ReverseEncoding,
) -> TextSelection {
    TextSelection::Cooked(CookedPolicy {
        escapes: js_escapes(),
        line_terminators,
        indentation,
        reverse,
    })
}

fn backticks() -> DelimiterPolicy {
    DelimiterPolicy {
        open: "`".into(),
        close: "`".into(),
        escape_introducer: Some('\\'),
    }
}

fn admit_holes(placeholder: Option<PlaceholderSpelling>) -> HolePolicy {
    HolePolicy::Admit {
        opener: "${".into(),
        class: ProfileHoleClass("binding".into()),
        placeholder,
    }
}

/// A template-literal-shaped cooked selection admitting holes.
fn template_cooked() -> EmbeddedTextCodec {
    template_with(IndentationPolicy::Preserve)
}

fn template_with(indentation: IndentationPolicy) -> EmbeddedTextCodec {
    EmbeddedTextCodec::new(
        cooked_policy(
            LineTerminatorPolicy::NormalizeToLf,
            indentation,
            ReverseEncoding::ThroughEscapes,
        ),
        backticks(),
        admit_holes(None),
    )
}

fn template_raw() -> EmbeddedTextCodec {
    EmbeddedTextCodec::new(TextSelection::Raw, backticks(), admit_holes(None))
}

/// A quoted-string-shaped selection: no holes, no unescaped line terminators.
fn quoted_string() -> EmbeddedTextCodec {
    EmbeddedTextCodec::new(
        cooked_policy(
            LineTerminatorPolicy::Forbidden,
            IndentationPolicy::Preserve,
            ReverseEncoding::ThroughEscapes,
        ),
        DelimiterPolicy {
            open: "\"".into(),
            close: "\"".into(),
            escape_introducer: Some('\\'),
        },
        HolePolicy::Absent,
    )
}

fn hole_region(ordinal: usize) -> RegionId {
    RegionId::from_region_role(&host(), &format!("literal/hole/{ordinal}"))
}

/// The geometry a host parser reports for a simple template literal starting at
/// `start`: `${`..`}` holes with no braces inside, escapes respected. An
/// unterminated hole or literal comes out as text, leaving the codec to refuse.
fn parsed_template(source: &str, start: usize) -> LiteralGeometry {
    let bytes = source.as_bytes();
    let mut holes = Vec::new();
    let mut pos = start + 1;
    let mut close = None;
    while pos < bytes.len() {
        match bytes[pos] {
            b'\\' => {
                pos += 2;
                continue;
            }
            b'`' => {
                close = Some(pos);
                break;
            }
            b'$' if bytes.get(pos + 1) == Some(&b'{') => {
                if let Some(length) = source[pos..].find('}') {
                    let ordinal = holes.len();
                    holes.push(HoleGeometry {
                        outer: sr(pos as u32, (pos + length + 1) as u32),
                        expression: sr((pos + 2) as u32, (pos + length) as u32),
                        region: hole_region(ordinal),
                    });
                    pos += length + 1;
                    continue;
                }
            }
            _ => {}
        }
        pos += 1;
    }
    let (body_end, outer_end) = match close {
        Some(close) => (close, close + 1),
        None => (bytes.len(), bytes.len()),
    };
    LiteralGeometry::new(
        sr(start as u32, outer_end as u32),
        sr((start + 1) as u32, body_end as u32),
        holes,
    )
    .expect("the fixture parser reports ordered geometry")
}

fn quoted_geometry(source: &str) -> LiteralGeometry {
    let end = source.len() as u32;
    LiteralGeometry::new(sr(0, end), sr(1, end - 1), Vec::new()).expect("quoted geometry")
}

fn decode_at(
    codec: &EmbeddedTextCodec,
    source: &str,
    geometry: &LiteralGeometry,
) -> Result<DecodedLiteral, CodecRefusal> {
    codec.decode(
        &DecodeRequest {
            source,
            geometry,
            lineage: &root_lineage(),
            input: &basis_of(source),
        },
        &NeverInterrupted,
    )
}

fn decode_template(
    codec: &EmbeddedTextCodec,
    source: &str,
) -> Result<DecodedLiteral, CodecRefusal> {
    decode_at(codec, source, &parsed_template(source, 0))
}

fn decoded(codec: &EmbeddedTextCodec, source: &str) -> DecodedLiteral {
    decode_template(codec, source).expect("the literal decodes")
}

fn attachment_for(lineage: RegionLineage, context: Option<AttachmentId>) -> EmbeddedAttachment {
    let schema = ProfileSchemaEpoch::for_schema_family("framework", 1);
    let release = ReleaseId::from_coordinates(
        &ReleaseCoordinates::decode("test-framework", &["1.0.0"], &[1])
            .expect("exact admitted release"),
        &schema,
    );
    let carrier = CarrierProfileId::from_carrier("template", &schema);
    let profile = FrameworkProfileId::from_release_and_carriers(&release, &[carrier], &schema);
    let attachment = AttachmentId::from_claim(lineage.unit(), lineage.region(), &profile);
    EmbeddedAttachment::new(
        lineage,
        attachment,
        context,
        BaseUri::new("file:///src/host.ts"),
    )
}

fn embedded(start: u32, end: u32) -> DomainRange {
    DomainRange::Embedded(er(start, end))
}

fn generated(start: u32, end: u32) -> DomainRange {
    DomainRange::Generated(gr(start, end))
}

fn authored(start: u32, end: u32) -> DomainRange {
    DomainRange::Source(sr(start, end))
}

fn verbatim(output: DomainRange, input: DomainRange) -> MapSegment {
    MapSegment {
        output,
        origin: SegmentOrigin::Verbatim { input },
    }
}

fn mapped_stage(
    kind: StageKind,
    certification: EditCertification,
    input: MapBasis,
    output: MapBasis,
    extent: u32,
    segments: Vec<MapSegment>,
) -> Arc<MapStage> {
    Arc::new(
        MapStage::new(
            kind,
            certification,
            input,
            output,
            extent,
            StageSegments {
                segments,
                ..StageSegments::default()
            },
        )
        .expect("a consistent stage"),
    )
}

// ------------------------------------------------- shared primitives, profiles

/// Three profiles over one literal: identical shared geometry where their
/// policies agree, their own hole result where they differ, and their own
/// activation row. The labels are opaque: a profile renamed to anything behaves
/// the same, so nothing in the codec reads them.
#[test]
fn profiles_share_geometry_and_own_activation_grammar_and_holes() {
    let selection = || {
        cooked_policy(
            LineTerminatorPolicy::NormalizeToLf,
            IndentationPolicy::Preserve,
            ReverseEncoding::ThroughEscapes,
        )
    };
    let refuse = |why: &str| HolePolicy::Refuse {
        opener: "${".into(),
        disposition: ProfileHoleClass(why.into()),
    };
    let admit = |label: &str| HolePolicy::Admit {
        opener: "${".into(),
        class: ProfileHoleClass(label.into()),
        placeholder: Some(PlaceholderSpelling {
            prefix: "<!--?".into(),
            suffix: "-->".into(),
        }),
    };
    let vue_like = EmbeddedTextCodec::new(selection(), backticks(), refuse("dynamic-template"));
    let angular_like = EmbeddedTextCodec::new(selection(), backticks(), refuse("unsupported"));
    let lit_like = EmbeddedTextCodec::new(selection(), backticks(), admit("binding"));

    let plain = "`<b>\\u00E9</b>`";
    let answers = [&vue_like, &angular_like, &lit_like].map(|codec| decoded(codec, plain));
    for answer in &answers {
        assert_eq!(answer.text(), "<b>é</b>");
        assert_eq!(answer.map_range(er(3, 5)), Ok(vec![sr(4, 10)]));
    }
    // Same text, but each result is qualified by its own exact policy.
    assert_ne!(answers[0], answers[1]);
    assert_ne!(vue_like.policy_digest(), lit_like.policy_digest());

    let holed = "`<b>${x}</b>`";
    let refused = |codec: &EmbeddedTextCodec, why: &str| {
        assert_eq!(
            decode_template(codec, holed),
            Err(CodecRefusal::HoleRefused {
                ordinal: 0,
                authored: sr(4, 8),
                disposition: Some(ProfileHoleClass(why.into())),
            })
        );
    };
    refused(&vue_like, "dynamic-template");
    refused(&angular_like, "unsupported");
    let admitted = decoded(&lit_like, holed);
    assert_eq!(admitted.text(), "<b><!--?0--></b>");
    assert_eq!(
        admitted.holes()[0].class,
        ProfileHoleClass("binding".into())
    );

    // Renaming a profile's opaque label moves only that label.
    let renamed = decoded(
        &EmbeddedTextCodec::new(selection(), backticks(), admit("zzz")),
        holed,
    );
    assert_eq!(renamed.text(), admitted.text());
    assert_eq!(renamed.holes()[0].class, ProfileHoleClass("zzz".into()));
    assert_eq!(renamed.holes()[0].output, admitted.holes()[0].output);

    // Activation and grammar come from each profile's own row, never the text.
    let schema = ProfileSchemaEpoch::for_schema_family("carrier", 1);
    let carrier = |name: &str| CarrierProfileId::from_carrier(name, &schema);
    let row = |host_name: &str, embedded_name: &str, codec: &EmbeddedTextCodec| EmbeddedRegionRow {
        host_carrier: carrier(host_name),
        region_role: "template-literal".into(),
        embedded_carrier: carrier(embedded_name),
        codec: Arc::new(codec.clone()),
        map_chain: Box::new([StageKind::Syntax]),
    };
    let table = EmbeddedRegionTable::new(vec![
        row("vue-host", "vue-template", &vue_like),
        row("angular-host", "angular-template", &angular_like),
        row("lit-host", "html-template", &lit_like),
    ])
    .expect("distinct keys");
    let select = |name: &str| table.select(&carrier(name), "template-literal");
    assert_eq!(
        select("lit-host").map(|row| row.embedded_carrier.clone()),
        Some(carrier("html-template"))
    );
    assert_eq!(select("vue-host").map(|row| &*row.codec), Some(&vue_like));
    assert_eq!(select("plain-host"), None);
}

// ---------------------------------------------------------------- unicode

#[test]
fn lone_surrogate_code_units_are_refused_before_any_text_exists() {
    let cases: [(&[u16], SurrogateKind); 5] = [
        (&[0xD800], SurrogateKind::High),
        (&[0xDC00], SurrogateKind::Low),
        (&[0x61, 0xD800], SurrogateKind::High),
        (&[0xDC00, 0x61], SurrogateKind::Low),
        (&[0xD83D, 0xD83D, 0xDE00], SurrogateKind::High),
    ];
    for (units, surrogate) in cases {
        assert_eq!(
            CookedText::from_code_units(units),
            Err(NonUnicodeCookedLiteral {
                surrogate,
                authored: None,
            }),
            "{units:x?}"
        );
    }
    let pair = CookedText::from_code_units(&[0xD83D, 0xDE00]).expect("a pair is one scalar");
    assert_eq!(pair.as_str().as_bytes(), [0xF0, 0x9F, 0x98, 0x80]);
    assert_eq!(
        CookedText::from_host_value(None),
        Err(CookedPartiality::Missing(MissingCookedValue {
            authored: None
        }))
    );
    assert_eq!(
        CookedText::from_host_value(Some(&[0xD800])),
        Err(CookedPartiality::NonUnicode(NonUnicodeCookedLiteral {
            surrogate: SurrogateKind::High,
            authored: None,
        }))
    );
}

#[test]
fn lone_surrogate_escapes_are_refused_with_their_authored_escape() {
    let cases: [(&str, SurrogateKind, SourceByteRange); 6] = [
        ("`\\uD83D`", SurrogateKind::High, sr(1, 7)),
        ("`\\uDE00`", SurrogateKind::Low, sr(1, 7)),
        ("`\\u{D800}`", SurrogateKind::High, sr(1, 9)),
        ("`é\\uD83Dx`", SurrogateKind::High, sr(3, 9)),
        // A low half before its high half pairs with nothing.
        ("`\\uDE00\\uD83D`", SurrogateKind::Low, sr(1, 7)),
        // A pair must be adjacent: another scalar escape breaks it.
        ("`\\uD83D\\u0041`", SurrogateKind::High, sr(1, 7)),
    ];
    let mut parsed = 0;
    for (source, surrogate, escape) in cases {
        match decode_template(&template_cooked(), source) {
            Err(CodecRefusal::NonUnicodeCookedLiteral(refusal)) => {
                assert_eq!(refusal.surrogate, surrogate, "{source}");
                assert_eq!(refusal.authored, Some(escape), "{source}");
            }
            Err(other) => panic!("{source}: unexpected refusal {other:?}"),
            Ok(_) => parsed += 1,
        }
    }
    assert_eq!(parsed, 0, "no lone surrogate may reach an embedded parser");
}

/// Both spellings of an astral scalar are one whole escape: the scalar maps to
/// the whole authored spelling and its interior bytes have no position.
#[test]
fn a_surrogate_pair_is_one_scalar_whose_interior_has_no_position() {
    for (source, end) in [
        ("`\\uD83D\\uDE00`", 13),
        ("`\\uD83D\\u{DE00}`", 15),
        ("`\\u{1F600}`", 10),
    ] {
        let literal = decoded(&template_cooked(), source);
        assert_eq!(literal.text().as_bytes(), [0xF0, 0x9F, 0x98, 0x80]);
        assert_eq!(literal.map_range(er(0, 4)), Ok(vec![sr(1, end)]));
        assert_eq!(literal.map_position(eo(0)), Ok(so(1)));
        assert_eq!(literal.map_position(eo(4)), Ok(so(end)));
        for interior in 1..4 {
            assert_eq!(
                literal.map_position(eo(interior)),
                Err(Unmappable::InvalidBoundary(CoordError::NotCharBoundary)),
                "{source} interior {interior}"
            );
            assert_eq!(
                literal.map_range(er(0, interior)),
                Err(Unmappable::InvalidBoundary(CoordError::NotCharBoundary)),
            );
        }
    }
}

#[test]
fn an_absent_cooked_value_never_falls_back_to_raw_text() {
    let source = "`a\\u{ZZ}b`";
    assert_eq!(
        decode_template(&template_cooked(), source),
        Err(CodecRefusal::MissingCookedValue(MissingCookedValue {
            authored: Some(sr(2, 4)),
        }))
    );
    // Each of these spellings leaves the literal without a cooked value.
    for invalid in [
        "`\\u12`",
        "`\\xG0`",
        "`\\1`",
        "`\\01`",
        "`\\u{110000}`",
        "`\\u{}`",
    ] {
        assert!(
            matches!(
                decode_template(&template_cooked(), invalid),
                Err(CodecRefusal::MissingCookedValue(_))
            ),
            "{invalid}"
        );
    }

    // Only an explicitly raw profile admits the spelling, with no cooked identity.
    let raw = decoded(&template_raw(), source);
    assert_eq!(raw.text(), "a\\u{ZZ}b");
    assert_eq!(raw.cooked(), None);
    assert_eq!(raw.map_position(eo(2)), Ok(so(3)));
    assert_eq!(raw.map_range(er(0, 8)), Ok(vec![sr(1, 9)]));
}

#[test]
fn literal_scalars_keep_their_spelling_and_scalar_boundaries() {
    let spelling = "é😀e\u{301}\u{200D}\u{FEFF}";
    let source = format!("`{spelling}`");
    let literal = decoded(&template_cooked(), &source);
    assert_eq!(literal.text(), spelling);
    assert_eq!(literal.stage().segments().len(), 1);
    for offset in 0..=spelling.len() {
        let answer = literal.map_position(eo(offset as u32));
        if spelling.is_char_boundary(offset) {
            assert_eq!(answer, Ok(so(offset as u32 + 1)), "offset {offset}");
        } else {
            assert_eq!(
                answer,
                Err(Unmappable::InvalidBoundary(CoordError::NotCharBoundary)),
                "offset {offset}"
            );
        }
    }
    assert_eq!(
        literal.map_position(eo(spelling.len() as u32 + 1)),
        Err(Unmappable::InvalidBoundary(CoordError::InvalidBoundary))
    );
}

// ---------------------------------------------------------------- escapes

/// `((output start, end), (authored start, end))` of one whole escape.
type OutputAndAuthored = ((u32, u32), (u32, u32));

#[test]
fn escapes_map_whole_and_declared_delimiters_are_never_holes() {
    struct Case {
        codec: EmbeddedTextCodec,
        source: &'static str,
        text: &'static str,
        /// `(output range, authored range)` of each whole escape.
        whole: &'static [OutputAndAuthored],
    }
    let cases = [
        Case {
            codec: template_cooked(),
            source: "`\\u00E9x`",
            text: "éx",
            whole: &[((0, 2), (1, 7))],
        },
        Case {
            codec: template_cooked(),
            source: "`a\\`b`",
            text: "a`b",
            whole: &[((1, 2), (2, 4))],
        },
        Case {
            codec: template_cooked(),
            source: "`\\'`",
            text: "'",
            whole: &[((0, 1), (1, 3))],
        },
        Case {
            codec: template_cooked(),
            source: "`\\${a}`",
            text: "${a}",
            whole: &[((0, 1), (1, 3))],
        },
        Case {
            codec: template_cooked(),
            source: "`\\n\\t\\x41`",
            text: "\n\tA",
            whole: &[((0, 1), (1, 3)), ((1, 2), (3, 5)), ((2, 3), (5, 9))],
        },
    ];
    for case in cases {
        let literal = decoded(&case.codec, case.source);
        assert_eq!(literal.text(), case.text, "{}", case.source);
        assert!(literal.holes().is_empty(), "{}", case.source);
        let whole: Vec<_> = literal
            .stage()
            .segments()
            .iter()
            .filter(|segment| matches!(segment.origin, SegmentOrigin::Whole { .. }))
            .cloned()
            .collect();
        let expected: Vec<_> = case
            .whole
            .iter()
            .map(|&((out_start, out_end), (in_start, in_end))| MapSegment {
                output: embedded(out_start, out_end),
                origin: SegmentOrigin::Whole {
                    input: authored(in_start, in_end),
                    kind: WholeKind::Escape,
                },
            })
            .collect();
        assert_eq!(whole, expected, "{}", case.source);
    }

    // The bytes after an escape stay exact; the escape's are not interpolated.
    let literal = decoded(&template_cooked(), "`\\u00E9xy`");
    assert_eq!(literal.map_position(eo(2)), Ok(so(7)));
    assert_eq!(literal.map_position(eo(3)), Ok(so(8)));
    assert_eq!(literal.map_range(er(2, 4)), Ok(vec![sr(7, 9)]));

    // An escaped delimiter keeps its spelling under raw selection, and an
    // escaped opener is no hole there either.
    let raw = decoded(&template_raw(), "`\\${a}\\``");
    assert_eq!(raw.text(), "\\${a}\\`");
    assert!(raw.holes().is_empty());

    let quoted = "\"a\\\"b\"";
    let literal = decode_at(&quoted_string(), quoted, &quoted_geometry(quoted)).expect("decodes");
    assert_eq!(literal.text(), "a\"b");
}

#[test]
fn unescaped_syntax_the_parser_did_not_report_is_refused_not_guessed() {
    // The parser's body says this is text, so the opener inside it invents no hole.
    let unreported = LiteralGeometry::new(sr(0, 6), sr(1, 5), Vec::new()).expect("geometry");
    assert_eq!(
        decode_at(&template_cooked(), "`${a}`", &unreported),
        Err(CodecRefusal::UndeclaredHole { authored: sr(1, 3) })
    );
    assert_eq!(
        decode_at(&template_raw(), "`${a}`", &unreported),
        Err(CodecRefusal::UndeclaredHole { authored: sr(1, 3) })
    );
    let inner_quote = "\"a\"b\"";
    assert_eq!(
        decode_at(&quoted_string(), inner_quote, &quoted_geometry(inner_quote)),
        Err(CodecRefusal::UnescapedDelimiter { authored: sr(2, 3) })
    );
    // The same opener text under a syntax with no holes is ordinary text.
    let dollars = "\"${a}\"";
    assert_eq!(
        decode_at(&quoted_string(), dollars, &quoted_geometry(dollars))
            .expect("decodes")
            .text(),
        "${a}"
    );
    // Delimiters are checked against the policy, never assumed.
    assert_eq!(
        decode_at(&template_cooked(), "'abc'", &quoted_geometry("'abc'")),
        Err(CodecRefusal::DelimiterMismatch { authored: sr(0, 1) })
    );
    assert_eq!(
        decode_at(&template_cooked(), "`abc'", &quoted_geometry("`abc'")),
        Err(CodecRefusal::DelimiterMismatch { authored: sr(4, 5) })
    );
}

// ---------------------------------------------------------------- terminators

#[test]
fn line_terminators_follow_policy_and_record_their_contraction() {
    let source = "`a\r\nb\rc\nd`";

    let raw = decoded(&template_raw(), source);
    assert_eq!(raw.text(), "a\r\nb\rc\nd");
    assert_eq!(raw.stage().segments().len(), 1);

    let cooked = decoded(&template_cooked(), source);
    assert_eq!(cooked.text(), "a\nb\nc\nd");
    let contraction = |output: DomainRange, input: DomainRange| MapSegment {
        output,
        origin: SegmentOrigin::Whole {
            input,
            kind: WholeKind::LineTerminatorContraction,
        },
    };
    assert_eq!(
        cooked.stage().segments(),
        [
            verbatim(embedded(0, 1), authored(1, 2)),
            contraction(embedded(1, 2), authored(2, 4)),
            verbatim(embedded(2, 3), authored(4, 5)),
            contraction(embedded(3, 4), authored(5, 6)),
            // A bare LF is already normalized, so it stays verbatim.
            verbatim(embedded(4, 7), authored(6, 9)),
        ]
    );
    // Offsets after a contraction stay exact.
    assert_eq!(cooked.map_position(eo(2)), Ok(so(4)));
    assert_eq!(cooked.map_position(eo(4)), Ok(so(6)));
    assert_eq!(cooked.map_range(er(0, 7)), Ok(vec![sr(1, 9)]));

    let preserved = EmbeddedTextCodec::new(
        cooked_policy(
            LineTerminatorPolicy::Preserve,
            IndentationPolicy::Preserve,
            ReverseEncoding::ThroughEscapes,
        ),
        backticks(),
        admit_holes(None),
    );
    assert_eq!(decoded(&preserved, source).text(), "a\r\nb\rc\nd");

    let newline = "\"a\r\nb\"";
    assert_eq!(
        decode_at(&quoted_string(), newline, &quoted_geometry(newline)),
        Err(CodecRefusal::ForbiddenLineTerminator { authored: sr(2, 4) })
    );
}

#[test]
fn a_line_continuation_is_elided_and_keeps_the_next_scalars_origin() {
    for (source, continuation_end) in [
        ("`a\\\nb`", 4u32),
        ("`a\\\r\nb`", 5),
        ("`a\\\rb`", 4),
        ("`a\\\u{2028}b`", 6),
    ] {
        let literal = decoded(&template_cooked(), source);
        assert_eq!(literal.text(), "ab", "{source:?}");
        let elisions: Vec<_> = literal
            .stage()
            .elisions()
            .iter()
            .filter(|elision| elision.kind == ElisionKind::LineContinuation)
            .copied()
            .collect();
        assert_eq!(
            elisions,
            [Elision {
                at: DomainOffset::Embedded(eo(1)),
                input: authored(2, continuation_end),
                kind: ElisionKind::LineContinuation,
            }],
            "{source:?}"
        );
        // No zero-width mapped run: the map still has only `a` and `b`.
        assert_eq!(literal.stage().segments().len(), 2, "{source:?}");
        assert_eq!(literal.map_position(eo(1)), Ok(so(continuation_end)));
        assert_eq!(literal.map_position(eo(2)), Ok(so(continuation_end + 1)));
        assert_eq!(
            literal.map_range(er(0, 2)),
            Ok(vec![sr(1, 2), sr(continuation_end, continuation_end + 1)])
        );
        assert_eq!(
            literal.editable_range(er(0, 2)),
            Err(Unmappable::ElidedSource)
        );
        assert_eq!(
            literal.editable_range(er(1, 1)),
            Err(Unmappable::ElidedSource)
        );
        assert_eq!(literal.editable_range(er(0, 1)), Ok(sr(1, 2)));
    }
    // A literal holding nothing but a continuation has no position to name.
    let only = decoded(&template_cooked(), "`\\\n`");
    assert_eq!(only.text(), "");
    assert_eq!(only.map_position(eo(0)), Err(Unmappable::ElidedSource));
}

#[test]
fn declared_indentation_records_exactly_what_it_removes() {
    let strip = |include_first_line| IndentationPolicy::StripCommonLeading {
        whitespace: Box::new([' ', '\t', '\u{3000}']),
        include_first_line,
    };
    struct Case {
        source: &'static str,
        include_first_line: bool,
        text: &'static str,
        removed: &'static [(u32, u32, u32)],
    }
    let cases = [
        Case {
            source: "`\n\t  one\n\n\t  é\n\t    two\n`",
            include_first_line: false,
            text: "\none\n\né\n  two\n",
            removed: &[(1, 2, 5), (6, 10, 13), (9, 16, 19)],
        },
        // A non-ASCII indent character is removed whole.
        Case {
            source: "`\n\u{3000}a\n\u{3000}b`",
            include_first_line: false,
            text: "\na\nb",
            removed: &[(1, 2, 5), (3, 7, 10)],
        },
        // Mixed tab and space lines share nothing, so nothing is normalized.
        Case {
            source: "`\n\ta\n b`",
            include_first_line: false,
            text: "\n\ta\n b",
            removed: &[],
        },
        Case {
            source: "`  a\n  b`",
            include_first_line: true,
            text: "a\nb",
            removed: &[(0, 1, 3), (2, 5, 7)],
        },
        // A blank line loses at most the common prefix and counts for nothing.
        Case {
            source: "`\n  a\n \n  b`",
            include_first_line: false,
            text: "\na\n\nb",
            removed: &[(1, 2, 4), (3, 6, 7), (4, 8, 10)],
        },
    ];
    for case in cases {
        let codec = template_with(strip(case.include_first_line));
        let literal = decoded(&codec, case.source);
        assert_eq!(literal.text(), case.text, "{:?}", case.source);
        let removed: Vec<_> = literal
            .stage()
            .elisions()
            .iter()
            .filter(|elision| elision.kind == ElisionKind::Indentation)
            .copied()
            .collect();
        let expected: Vec<_> = case
            .removed
            .iter()
            .map(|&(at, start, end)| Elision {
                at: DomainOffset::Embedded(eo(at)),
                input: authored(start, end),
                kind: ElisionKind::Indentation,
            })
            .collect();
        assert_eq!(removed, expected, "{:?}", case.source);
    }

    // A line whose first content is a hole has content, so it sets the indent.
    let holed = "`\n  ${x}\n    y\n`";
    let literal = decoded(&template_with(strip(false)), holed);
    assert_eq!(literal.text(), "\n\n  y\n");
    assert_eq!(literal.map_position(eo(4)), Ok(so(13)));

    // Undeclared indentation is never removed.
    let plain = decoded(&template_cooked(), "`\n  a\n  b`");
    assert_eq!(plain.text(), "\n  a\n  b");
    assert!(plain
        .stage()
        .elisions()
        .iter()
        .all(|elision| elision.kind != ElisionKind::Indentation));
}

// ---------------------------------------------------------------- empty, holes

#[test]
fn empty_chunks_keep_holes_ordered_and_offsets_stay_exact() {
    let empty = decoded(&template_cooked(), "``");
    assert_eq!(empty.text(), "");
    assert_eq!(empty.chunks().len(), 1);
    assert_eq!(empty.map_position(eo(0)), Ok(so(1)));
    assert_eq!(empty.editable_range(er(0, 0)), Ok(sr(1, 1)));
    assert_eq!(
        empty.map_position(eo(1)),
        Err(Unmappable::InvalidBoundary(CoordError::InvalidBoundary))
    );

    let adjacent = decoded(&template_cooked(), "`${a}${b}`");
    assert_eq!(adjacent.text(), "");
    assert_eq!(adjacent.chunks().len(), 3);
    assert!(adjacent
        .chunks()
        .iter()
        .all(|chunk| chunk.output() == er(0, 0)));
    let holes: Vec<_> = adjacent
        .holes()
        .iter()
        .map(|hole| (hole.ordinal, hole.outer, hole.expression))
        .collect();
    assert_eq!(holes, [(0, sr(1, 5), sr(3, 4)), (1, sr(5, 9), sr(7, 8))],);
    assert_eq!(adjacent.map_position(eo(0)), Err(Unmappable::Hole));

    let leading = decoded(&template_cooked(), "`${a}b`");
    assert_eq!(leading.chunks()[0].authored(), sr(1, 1));
    assert_eq!(leading.chunks()[1].authored(), sr(5, 6));
    assert_eq!(leading.map_position(eo(0)), Ok(so(5)));

    let trailing = decoded(&template_cooked(), "`a${b}`");
    assert_eq!(trailing.chunks()[1].authored(), sr(6, 6));
    assert_eq!(trailing.map_position(eo(1)), Ok(so(2)));
}

/// Holes keep their authored order and lineage, their placeholders are
/// synthetic, and a nested literal is a child region decoded on its own.
#[test]
fn hole_placeholders_are_synthetic_and_nested_literals_keep_lineage() {
    let codec = EmbeddedTextCodec::new(
        cooked_policy(
            LineTerminatorPolicy::NormalizeToLf,
            IndentationPolicy::Preserve,
            ReverseEncoding::ThroughEscapes,
        ),
        backticks(),
        admit_holes(Some(PlaceholderSpelling {
            prefix: "<!--?".into(),
            suffix: "-->".into(),
        })),
    );
    let source = "`<p a=\"${user.name}\">é${x}</p>`";
    let literal = decoded(&codec, source);
    assert_eq!(literal.text(), "<p a=\"<!--?0-->\">é<!--?1--></p>");

    let holes = literal.holes();
    let expression = |hole: usize| {
        let range = holes[hole].expression;
        &source[range.start().get() as usize..range.end().get() as usize]
    };
    assert_eq!(expression(0), "user.name");
    assert_eq!(expression(1), "x");
    assert_eq!(holes[0].region, hole_region(0));
    assert_eq!(holes[1].region, hole_region(1));

    // Placeholder bytes are not authored expression bytes.
    let placeholder = holes[0].output;
    assert_eq!(placeholder, er(6, 15));
    assert_eq!(
        literal.stage().segments()[1],
        MapSegment {
            output: DomainRange::Embedded(placeholder),
            origin: SegmentOrigin::Synthetic { hole: Some(0) },
        }
    );
    assert_eq!(
        literal.map_position(placeholder.start()),
        Err(Unmappable::SyntheticOutput)
    );
    assert_eq!(
        literal.map_range(placeholder),
        Err(Unmappable::SyntheticOutput)
    );
    assert_eq!(
        literal.editable_range(placeholder),
        Err(Unmappable::SyntheticOutput)
    );
    // Text beside the placeholder is exact, and a range across it is refused.
    assert_eq!(literal.map_range(er(0, 6)), Ok(vec![sr(1, 7)]));
    assert_eq!(literal.map_range(er(15, 17)), Ok(vec![sr(19, 21)]));
    assert_eq!(literal.editable_range(er(0, 17)), Err(Unmappable::Hole));

    // Without a placeholder the hole still divides the text.
    let spliced = decoded(&template_cooked(), "`a${x}b`");
    assert_eq!(spliced.text(), "ab");
    assert_eq!(spliced.map_range(er(0, 2)), Ok(vec![sr(1, 2), sr(6, 7)]));
    assert_eq!(spliced.editable_range(er(0, 2)), Err(Unmappable::Hole));
    assert_eq!(spliced.editable_range(er(1, 1)), Err(Unmappable::Hole));

    // The hole's expression holds a nested literal: a child with parent lineage.
    let budget = NestingBudget { max_depth: 4 };
    let child = root_lineage()
        .nested(hole_region(0), budget)
        .expect("within budget");
    let nested_source = "`outer ${`<i>é</i>`}`";
    let nested_geometry = LiteralGeometry::new(sr(9, 20), sr(10, 19), Vec::new()).expect("nested");
    let nested = codec
        .decode(
            &DecodeRequest {
                source: nested_source,
                geometry: &nested_geometry,
                lineage: &child,
                input: &basis_of(nested_source),
            },
            &NeverInterrupted,
        )
        .expect("the nested literal decodes");
    assert_eq!(nested.text(), "<i>é</i>");
    assert_eq!(
        nested.lineage().ancestors(),
        [root_lineage().region().clone()]
    );
    assert_eq!(nested.lineage().region(), &hole_region(0));
    assert_eq!(nested.map_position(eo(3)), Ok(so(13)));
}

#[test]
fn nesting_past_the_admitted_depth_is_refused_before_any_work() {
    let budget = NestingBudget { max_depth: 2 };
    let first = root_lineage()
        .nested(hole_region(1), budget)
        .expect("depth one");
    let at_limit = first.nested(hole_region(2), budget).expect("depth two");
    assert_eq!(at_limit.ancestors().len(), 2);

    let source = "`<b>`";
    let at_limit_answer = template_cooked()
        .decode(
            &DecodeRequest {
                source,
                geometry: &parsed_template(source, 0),
                lineage: &at_limit,
                input: &basis_of(source),
            },
            &NeverInterrupted,
        )
        .expect("the literal at the limit decodes exactly");
    assert_eq!(at_limit_answer.map_position(eo(0)), Ok(so(1)));

    // The child a nested decode requires does not exist past the limit.
    assert_eq!(
        at_limit.nested(hole_region(3), budget),
        Err(NestingExhausted { max_depth: 2 })
    );
}

// ---------------------------------------------------------------- composition

/// Authored → cooked → preprocessed → compiled → runtime. The answer travels
/// every stage and lands on authored UTF-8; each stage keeps its own identity.
#[test]
fn composition_keeps_stages_distinct_and_lands_on_authored_bytes() {
    let source = "`é\\nz`";
    let literal = decoded(&template_cooked(), source);
    assert_eq!(literal.text(), "é\nz");
    let syntax = literal.stage().clone();

    let unit = host();
    let cooked_basis = syntax.output().clone();
    // The preprocessor wraps the text: `[[é\nz]]`.
    let preprocessed = basis(unit.clone(), CoordinateDomain::Embedded, "[[é\nz]]", "pre");
    let preprocessor = mapped_stage(
        StageKind::Preprocessor,
        EditCertification::Reversible,
        cooked_basis,
        preprocessed.clone(),
        8,
        vec![
            MapSegment {
                output: embedded(0, 2),
                origin: SegmentOrigin::Synthetic { hole: None },
            },
            verbatim(embedded(2, 6), embedded(0, 4)),
            MapSegment {
                output: embedded(6, 8),
                origin: SegmentOrigin::Synthetic { hole: None },
            },
        ],
    );
    // The compiler keeps the text inside `out("…")`.
    let compiled = basis(unit.clone(), CoordinateDomain::Generated, "c", "compiler");
    let compiler = mapped_stage(
        StageKind::Compiler,
        EditCertification::Reversible,
        preprocessed,
        compiled.clone(),
        15,
        vec![verbatim(generated(5, 13), embedded(0, 8))],
    );
    let runtime_output = basis(unit.clone(), CoordinateDomain::Generated, "r", "runtime");
    let runtime = mapped_stage(
        StageKind::RuntimeBuild,
        EditCertification::ProvenanceOnly,
        compiled.clone(),
        runtime_output.clone(),
        40,
        vec![verbatim(generated(20, 35), generated(0, 15))],
    );

    let chain = AuthoredMapChain::new(attachment_for(root_lineage(), None), syntax)
        .and_then(|chain| chain.then(preprocessor))
        .and_then(|chain| chain.then(compiler))
        .expect("each basis joins exactly");
    let kinds = |chain: &AuthoredMapChain| -> Vec<_> {
        chain.stages().iter().map(|stage| stage.kind()).collect()
    };
    assert_eq!(
        kinds(&chain),
        [
            StageKind::Syntax,
            StageKind::Preprocessor,
            StageKind::Compiler
        ]
    );

    // Without a runtime stage: `é` at generated 7..9 is authored 1..3.
    let answer = chain
        .provenance(&compiled, generated(7, 9))
        .expect("an exact answer");
    let exact = answer.exact().expect("one origin");
    assert_eq!(exact.range, sr(1, 3));
    assert_eq!(exact.unit, host());
    assert_eq!(exact.base_uri, BaseUri::new("file:///src/host.ts"));
    // The whole escape, from the compiler's bytes.
    assert_eq!(
        chain
            .provenance(&compiled, generated(9, 10))
            .expect("exact")
            .exact()
            .map(|origin| origin.range),
        Some(sr(3, 5))
    );
    assert_eq!(
        chain
            .authored_position(
                &compiled,
                DomainOffset::Generated(GeneratedByteOffset::new(10))
            )
            .map(|origin| origin.range),
        Ok(sr(5, 5))
    );
    assert_eq!(
        chain
            .editable_preimage(&compiled, generated(7, 11))
            .map(|origin| origin.range),
        Ok(sr(1, 6))
    );
    // Generated bytes outside the kept text have no segment, and the wrapper is
    // synthetic: neither borrows the nearest authored byte.
    assert_eq!(
        chain.provenance(&compiled, generated(0, 5)),
        Err(Unmappable::MissingMap {
            stage: StageKind::Compiler
        })
    );
    assert_eq!(
        chain.provenance(&compiled, generated(5, 7)),
        Err(Unmappable::SyntheticOutput)
    );

    // An absent runtime map is absence: its basis names nothing in this chain.
    assert_eq!(
        chain.provenance(&runtime_output, generated(27, 29)),
        Err(Unmappable::StaleBasis {
            stage: StageKind::Compiler
        })
    );

    // With a runtime stage, blame still reaches authored bytes, but a runtime
    // map certifies provenance and never a safe edit.
    let with_runtime = chain.then(runtime).expect("joins");
    assert_eq!(kinds(&with_runtime).len(), 4);
    assert_eq!(
        with_runtime
            .provenance(&runtime_output, generated(27, 29))
            .expect("exact")
            .exact()
            .map(|origin| origin.range),
        Some(sr(1, 3))
    );
    assert_eq!(
        with_runtime.editable_preimage(&runtime_output, generated(27, 29)),
        Err(Unmappable::NotEditable {
            stage: StageKind::RuntimeBuild
        })
    );
}

fn syntax_chain_over(source: &str) -> (DecodedLiteral, AuthoredMapChain) {
    let literal = decoded(&template_raw(), source);
    let chain = AuthoredMapChain::new(
        attachment_for(root_lineage(), None),
        literal.stage().clone(),
    )
    .expect("a syntax stage starts a chain");
    (literal, chain)
}

/// Repeated output, non-invertible expansion, and a transform with no
/// trustworthy map: each is told truthfully, and none grants an edit.
#[test]
fn expansions_and_missing_maps_yield_provenance_or_a_reason_never_an_edit() {
    let (literal, chain) = syntax_chain_over("`mixin body`");
    let input = literal.stage().output().clone();
    let output = basis(host(), CoordinateDomain::Embedded, "expanded", "pug");
    let imported = unit_origin("import.scss", 4, 9);
    let stage = mapped_stage(
        StageKind::Preprocessor,
        EditCertification::Reversible,
        input.clone(),
        output.clone(),
        40,
        vec![
            // One output run with two ordered origins: a repeated mixin body.
            MapSegment {
                output: embedded(0, 5),
                origin: SegmentOrigin::Provenance {
                    origins: Box::new([
                        OriginRef::Input(embedded(6, 10)),
                        OriginRef::Input(embedded(0, 5)),
                    ]),
                },
            },
            // A rewrite: it blames its input as a whole, and has no inverse.
            MapSegment {
                output: embedded(5, 15),
                origin: SegmentOrigin::Whole {
                    input: embedded(0, 5),
                    kind: WholeKind::NonInvertibleExpansion,
                },
            },
            // Inlined text from another authored file.
            MapSegment {
                output: embedded(15, 20),
                origin: SegmentOrigin::Provenance {
                    origins: Box::new([OriginRef::Foreign(imported.clone())]),
                },
            },
            verbatim(embedded(20, 25), embedded(0, 5)),
        ],
    );
    let chain = chain.then(stage).expect("joins");

    let repeated = chain
        .provenance(&output, embedded(0, 5))
        .expect("ordered provenance");
    let ranges: Vec<_> = repeated.origins().iter().map(|o| o.range).collect();
    assert_eq!(ranges, [sr(7, 11), sr(1, 6)]);
    assert_eq!(repeated.exact(), None);
    assert_eq!(
        chain.editable_preimage(&output, embedded(0, 5)),
        Err(Unmappable::AmbiguousPreimage)
    );

    assert_eq!(
        chain
            .provenance(&output, embedded(5, 15))
            .expect("the whole rewrite")
            .exact()
            .map(|origin| origin.range),
        Some(sr(1, 6))
    );
    assert_eq!(
        chain.provenance(&output, embedded(6, 9)),
        Err(Unmappable::NonInvertibleExpansion)
    );
    assert_eq!(
        chain.editable_preimage(&output, embedded(5, 15)),
        Err(Unmappable::NonInvertibleExpansion)
    );

    let inlined = chain
        .provenance(&output, embedded(15, 20))
        .expect("another file's bytes");
    assert_eq!(inlined.exact(), Some(&imported));
    assert_eq!(
        chain.editable_preimage(&output, embedded(15, 20)),
        Err(Unmappable::AmbiguousPreimage)
    );
    assert_eq!(chain.contributing_units(), [host(), unit("import.scss")]);

    // Exact bytes remain exact beside all of that.
    assert_eq!(
        chain
            .editable_preimage(&output, embedded(20, 25))
            .map(|origin| origin.range),
        Ok(sr(1, 6))
    );

    // A transform without a map is in the chain, and says so.
    let (_, chain) = syntax_chain_over("`a{color:red}`");
    let (_, syntax_only) = syntax_chain_over("`a{color:red}`");
    let transformed = basis(
        host(),
        CoordinateDomain::Embedded,
        "a { color: red }",
        "css",
    );
    let missing = Arc::new(MapStage::missing(
        StageKind::Preprocessor,
        syntax_only.output_basis().clone(),
        transformed.clone(),
        16,
    ));
    let chain = chain.then(missing).expect("joins");
    let refused = Unmappable::MissingMap {
        stage: StageKind::Preprocessor,
    };
    assert_eq!(chain.provenance(&transformed, embedded(0, 4)), Err(refused));
    assert_eq!(
        chain.editable_preimage(&transformed, embedded(0, 4)),
        Err(refused)
    );
    assert_eq!(
        chain.authored_position(&transformed, DomainOffset::Embedded(eo(0))),
        Err(refused)
    );
}

fn unit_origin(
    name: &'static str,
    start: u32,
    end: u32,
) -> verter_language::embedded::AuthoredOrigin {
    verter_language::embedded::AuthoredOrigin {
        unit: unit(name),
        base_uri: BaseUri::new("file:///src/import.scss"),
        range: sr(start, end),
    }
}

/// One shared external resource: one unit and base URI, one context per
/// component, and blame always on the resource.
#[test]
fn a_shared_external_resource_keeps_its_identity_across_component_contexts() {
    let resource = unit("shared.html");
    let source = "<img src=\"./a.png\">";
    let region = RegionId::from_region_role(&resource, "external-template");
    let lineage = RegionLineage::root(resource.clone(), region.clone());
    let delimiters = DelimiterPolicy {
        open: "".into(),
        close: "".into(),
        escape_introducer: None,
    };
    let codec = EmbeddedTextCodec::new(TextSelection::Raw, delimiters, HolePolicy::Absent);
    let input = MapBasis {
        unit: resource.clone(),
        domain: CoordinateDomain::Source,
        content: ContentId::from_content_bytes(source.as_bytes()),
        map_revision: map_revision("authored"),
    };
    let end = source.len() as u32;
    let geometry = LiteralGeometry::new(sr(0, end), sr(0, end), Vec::new()).expect("geometry");
    let literal = codec
        .decode(
            &DecodeRequest {
                source,
                geometry: &geometry,
                lineage: &lineage,
                input: &input,
            },
            &NeverInterrupted,
        )
        .expect("decodes");

    let component_claim = |name: &'static str| -> AttachmentId {
        let component = unit(name);
        let claim = RegionLineage::root(
            component.clone(),
            RegionId::from_region_role(&component, "decorator"),
        );
        attachment_for(claim, None).attachment().clone()
    };
    let (first_context, second_context) = (component_claim("a.ts"), component_claim("b.ts"));
    assert_ne!(first_context, second_context);

    let resource_attachment = |context: AttachmentId| {
        let shared = attachment_for(lineage.clone(), Some(context));
        EmbeddedAttachment::new(
            lineage.clone(),
            shared.attachment().clone(),
            shared.context().cloned(),
            BaseUri::new("file:///src/shared.html"),
        )
    };
    let first = AuthoredMapChain::new(
        resource_attachment(first_context.clone()),
        literal.stage().clone(),
    )
    .expect("the resource starts its chain");
    let second = AuthoredMapChain::new(
        resource_attachment(second_context.clone()),
        literal.stage().clone(),
    )
    .expect("the resource starts its chain");

    assert_eq!(first.attachment().context(), Some(&first_context));
    assert_eq!(second.attachment().context(), Some(&second_context));
    assert_eq!(
        first.attachment().attachment(),
        second.attachment().attachment()
    );
    assert_ne!(first, second);
    let basis = literal.stage().output().clone();
    for chain in [&first, &second] {
        let answer = chain.provenance(&basis, embedded(10, 17)).expect("exact");
        let origin = answer.exact().expect("one origin");
        assert_eq!(origin.unit, resource);
        assert_eq!(origin.base_uri.as_str(), "file:///src/shared.html");
        assert_eq!(origin.range, sr(10, 17));
    }

    // A component's identity cannot stand in for the resource it references.
    let aliased = attachment_for(
        RegionLineage::root(unit("a.ts"), RegionId::from_region_role(&unit("a.ts"), "x")),
        None,
    );
    assert_eq!(
        AuthoredMapChain::new(aliased, literal.stage().clone()),
        Err(Unmappable::MismatchedBasis {
            stage: StageKind::Syntax
        })
    );
}

/// Regions of several carrier shapes: each decodes from its own parser-minted
/// geometry, and an edit in one keeps the others' identity and exact bytes.
#[test]
fn region_identity_and_geometry_survive_an_edit_of_a_sibling() {
    struct Region {
        role: &'static str,
        open: &'static str,
        close: &'static str,
        body: &'static str,
    }
    let regions = [
        Region {
            role: "frontmatter",
            open: "---\n",
            close: "---\n",
            body: "const é = 1;\n",
        },
        Region {
            role: "template-tag",
            open: "<template>",
            close: "</template>",
            body: "<i>{{x}}</i>",
        },
        Region {
            role: "style",
            open: "<style>",
            close: "</style>",
            body: "a{}",
        },
        Region {
            role: "esm",
            open: "",
            close: "",
            body: "export const y = 2;",
        },
    ];
    let render = |prefix: &str| {
        let mut source = String::from(prefix);
        let mut bounds = Vec::new();
        for region in &regions {
            let outer_start = source.len() as u32;
            source.push_str(region.open);
            let body_start = source.len() as u32;
            source.push_str(region.body);
            let body_end = source.len() as u32;
            source.push_str(region.close);
            bounds.push((outer_start, body_start, body_end, source.len() as u32));
        }
        (source, bounds)
    };
    let decode_all = |source: &str, bounds: &[(u32, u32, u32, u32)]| {
        regions
            .iter()
            .zip(bounds)
            .map(
                |(region, &(outer_start, body_start, body_end, outer_end))| {
                    let codec = EmbeddedTextCodec::new(
                        TextSelection::Raw,
                        DelimiterPolicy {
                            open: region.open.into(),
                            close: region.close.into(),
                            escape_introducer: None,
                        },
                        HolePolicy::Absent,
                    );
                    let lineage = RegionLineage::root(
                        host(),
                        RegionId::from_region_role(&host(), region.role),
                    );
                    let geometry = LiteralGeometry::new(
                        sr(outer_start, outer_end),
                        sr(body_start, body_end),
                        Vec::new(),
                    )
                    .expect("geometry");
                    codec
                        .decode(
                            &DecodeRequest {
                                source,
                                geometry: &geometry,
                                lineage: &lineage,
                                input: &basis_of(source),
                            },
                            &NeverInterrupted,
                        )
                        .expect("decodes")
                },
            )
            .collect::<Vec<_>>()
    };
    let (source, bounds) = render("");
    let (edited, edited_bounds) = render("/* é */");
    let before = decode_all(&source, &bounds);
    let after = decode_all(&edited, &edited_bounds);
    let shift = 8;
    for ((old, new), region) in before.iter().zip(&after).zip(&regions) {
        assert_eq!(old.text(), region.body);
        assert_eq!(new.text(), region.body);
        assert_eq!(old.lineage(), new.lineage(), "{}", region.role);
        let old_start = old.map_position(eo(0)).expect("exact").get();
        assert_eq!(new.map_position(eo(0)), Ok(so(old_start + shift)));
        assert_ne!(
            old.stage().input(),
            new.stage().input(),
            "the authored basis moved with the edit"
        );
    }
}

#[test]
fn stale_or_mismatched_bases_refuse_composition_and_queries() {
    let (literal, chain) = syntax_chain_over("`abc`");
    let live = literal.stage().output().clone();
    let stale = MapBasis {
        map_revision: map_revision("older"),
        ..live.clone()
    };
    let old_content = MapBasis {
        content: ContentId::from_content_bytes(b"older"),
        ..live.clone()
    };
    let wrong_unit = MapBasis {
        unit: unit("other.ts"),
        ..live.clone()
    };
    let wrong_domain = MapBasis {
        domain: CoordinateDomain::Generated,
        ..live.clone()
    };
    let output = basis(host(), CoordinateDomain::Generated, "next", "next");
    let stage = |input: MapBasis| {
        mapped_stage(
            StageKind::Compiler,
            EditCertification::Reversible,
            input,
            output.clone(),
            3,
            Vec::new(),
        )
    };
    let compiler = StageKind::Compiler;
    for (input, expected) in [
        (&stale, Unmappable::StaleBasis { stage: compiler }),
        (&old_content, Unmappable::StaleBasis { stage: compiler }),
        (&wrong_unit, Unmappable::MismatchedBasis { stage: compiler }),
        (
            &wrong_domain,
            Unmappable::MismatchedBasis { stage: compiler },
        ),
    ] {
        assert_eq!(
            chain.clone().then(stage(input.clone())),
            Err(expected),
            "no chain exists over {input:?}"
        );
    }
    assert_eq!(chain.stages().len(), 1);

    // A query on an old basis is refused, never mapped approximately.
    let syntax = StageKind::Syntax;
    for (basis, expected) in [
        (&stale, Unmappable::StaleBasis { stage: syntax }),
        (&wrong_unit, Unmappable::MismatchedBasis { stage: syntax }),
    ] {
        assert_eq!(chain.provenance(basis, embedded(0, 3)), Err(expected));
        assert_eq!(
            chain.editable_preimage(basis, embedded(0, 3)),
            Err(expected)
        );
        assert_eq!(
            chain.authored_position(basis, DomainOffset::Embedded(eo(0))),
            Err(expected)
        );
    }
    assert!(chain.provenance(&live, embedded(0, 3)).is_ok());
    // A range in a domain the output basis does not name is refused too.
    assert_eq!(
        chain.provenance(&live, generated(0, 3)),
        Err(Unmappable::MismatchedBasis { stage: syntax })
    );
    assert_eq!(
        chain.provenance(&live, embedded(0, 4)),
        Err(Unmappable::InvalidBoundary(CoordError::InvalidBoundary))
    );
}

/// Mapping both endpoints of a range can succeed while the range has no unique
/// preimage. Only whole-range proof grants an edit.
#[test]
fn an_edit_needs_a_whole_range_proof_at_every_stage() {
    let (literal, chain) = syntax_chain_over("`abcd`");
    let input = literal.stage().output().clone();
    let output = basis(host(), CoordinateDomain::Embedded, "cdab", "relocated");
    // `cdab`: two runs swapped, each exact on its own.
    let relocated = mapped_stage(
        StageKind::Preprocessor,
        EditCertification::Reversible,
        input,
        output.clone(),
        4,
        vec![
            verbatim(embedded(0, 2), embedded(2, 4)),
            verbatim(embedded(2, 4), embedded(0, 2)),
        ],
    );
    let chain = chain.then(relocated).expect("joins");

    let position = |offset| {
        chain
            .authored_position(&output, DomainOffset::Embedded(eo(offset)))
            .map(|origin| origin.range.start())
    };
    assert_eq!(position(0), Ok(so(3)));
    assert_eq!(position(4), Ok(so(3)));
    let across = chain
        .provenance(&output, embedded(0, 4))
        .expect("provenance");
    let ranges: Vec<_> = across.origins().iter().map(|o| o.range).collect();
    assert_eq!(ranges, [sr(3, 5), sr(1, 3)]);
    assert_eq!(
        chain.editable_preimage(&output, embedded(0, 4)),
        Err(Unmappable::AmbiguousPreimage)
    );
    assert_eq!(
        chain
            .editable_preimage(&output, embedded(0, 2))
            .map(|origin| origin.range),
        Ok(sr(3, 5))
    );

    // A cooked edit also needs the profile's declared reverse encoding.
    let undeclared = EmbeddedTextCodec::new(
        cooked_policy(
            LineTerminatorPolicy::NormalizeToLf,
            IndentationPolicy::Preserve,
            ReverseEncoding::Undeclared,
        ),
        backticks(),
        admit_holes(None),
    );
    let literal = decoded(&undeclared, "`abc`");
    assert_eq!(literal.map_range(er(0, 3)), Ok(vec![sr(1, 4)]));
    assert_eq!(
        literal.editable_range(er(0, 3)),
        Err(Unmappable::NotEditable {
            stage: StageKind::Syntax
        })
    );
    assert_eq!(
        undeclared.encode_replacement("x"),
        Err(UnencodableReplacement::NoReverseEncoding)
    );
}

/// Whatever a profile's reverse encoding spells decodes back to the same text,
/// and text it cannot spell is refused rather than written unescaped.
#[test]
fn reverse_encoding_round_trips_or_refuses() {
    let replacements = [
        "plain é😀",
        "`tick`",
        "back\\slash",
        "${not a hole}",
        "a\nb",
        "tab\there",
        "cr\rhere",
    ];
    for codec in [template_cooked(), quoted_with_backticks()] {
        for replacement in replacements {
            let spelled = codec
                .encode_replacement(replacement)
                .expect("the policy spells it");
            let source = format!("`{spelled}`");
            let literal = decode_at(&codec, &source, &parsed_template(&source, 0))
                .expect("the spelling decodes");
            assert_eq!(literal.text(), replacement, "{spelled:?}");
            assert!(literal.holes().is_empty(), "{spelled:?}");
        }
    }

    // A marker with a following-character rule is not used as a spelling.
    assert_eq!(
        template_cooked().encode_replacement("\0"),
        Ok("\0".to_owned())
    );

    let raw = template_raw();
    assert_eq!(raw.encode_replacement("plain"), Ok("plain".to_owned()));
    assert_eq!(
        raw.encode_replacement("a`b"),
        Err(UnencodableReplacement::Character('`'))
    );
    assert_eq!(
        raw.encode_replacement("${x}"),
        Err(UnencodableReplacement::Character('$'))
    );

    // An indentation policy would reinterpret a new line, and no table entry
    // can spell it without one.
    let no_newline_escape = EmbeddedTextCodec::new(
        TextSelection::Cooked(CookedPolicy {
            escapes: EscapePolicy {
                simple: Box::new([]),
                numeric: Box::new([]),
                invalid_markers: Box::new(['\n']),
                identity: IdentityEscapes::Invalid,
                continuations: Box::new([]),
            },
            line_terminators: LineTerminatorPolicy::Forbidden,
            indentation: IndentationPolicy::Preserve,
            reverse: ReverseEncoding::ThroughEscapes,
        }),
        backticks(),
        HolePolicy::Absent,
    );
    assert_eq!(
        no_newline_escape.encode_replacement("a\nb"),
        Err(UnencodableReplacement::Character('\n'))
    );
}

/// A cooked template whose terminators are forbidden, so newlines must escape.
fn quoted_with_backticks() -> EmbeddedTextCodec {
    EmbeddedTextCodec::new(
        cooked_policy(
            LineTerminatorPolicy::Forbidden,
            IndentationPolicy::Preserve,
            ReverseEncoding::ThroughEscapes,
        ),
        backticks(),
        admit_holes(None),
    )
}

#[test]
fn discovery_order_never_changes_an_answer() {
    let input = basis(host(), CoordinateDomain::Source, "abcdefgh", "authored");
    let output = basis(host(), CoordinateDomain::Embedded, "out", "out");
    let build = |segments: Vec<MapSegment>| {
        MapStage::new(
            StageKind::Preprocessor,
            EditCertification::Reversible,
            input.clone(),
            output.clone(),
            8,
            StageSegments {
                segments,
                ..StageSegments::default()
            },
        )
    };
    let a = verbatim(embedded(0, 2), authored(0, 2));
    let b = verbatim(embedded(2, 5), authored(4, 7));
    let third = MapSegment {
        output: embedded(5, 8),
        origin: SegmentOrigin::Whole {
            input: authored(2, 4),
            kind: WholeKind::NonInvertibleExpansion,
        },
    };
    assert_eq!(
        build(vec![verbatim(embedded(5, 8), authored(2, 4))]),
        Err(MapStageError::VerbatimLengthMismatch),
        "a verbatim run cannot change length"
    );

    let forward = build(vec![a.clone(), b.clone(), third.clone()]).expect("stage");
    for permutation in [
        vec![b.clone(), a.clone(), third.clone()],
        vec![third.clone(), b.clone(), a.clone()],
        vec![third.clone(), a.clone(), b.clone()],
    ] {
        assert_eq!(build(permutation).expect("stage"), forward);
    }

    // Coincident segments that agree are one segment.
    let agreeing = build(vec![a.clone(), b.clone(), a.clone(), third.clone()]).expect("stage");
    assert_eq!(agreeing, forward);

    // Coincident segments that disagree are ambiguous, in either order, and the
    // producer's declared order stays visible.
    let other_a = verbatim(embedded(0, 2), authored(6, 8));
    let conflicted = |segments| -> AuthoredMapChain {
        let stage = Arc::new(build(segments).expect("stage"));
        let attachment = attachment_for(root_lineage(), None);
        AuthoredMapChain::new(attachment, stage).expect("starts at authored bytes")
    };
    let first_wins = conflicted(vec![a.clone(), other_a.clone()]);
    let second_wins = conflicted(vec![other_a.clone(), a.clone()]);
    for chain in [&first_wins, &second_wins] {
        assert_eq!(
            chain.provenance(&output, embedded(0, 2)),
            Err(Unmappable::AmbiguousPreimage)
        );
        assert_eq!(
            chain.authored_position(&output, DomainOffset::Embedded(eo(0))),
            Err(Unmappable::AmbiguousPreimage)
        );
    }
    assert_eq!(
        first_wins.stages()[0].segments()[0].origin,
        SegmentOrigin::Conflicting {
            origins: Box::new([a.origin.clone(), other_a.origin.clone()])
        }
    );

    // Partial overlap has no canonical order at all.
    assert_eq!(
        build(vec![
            verbatim(embedded(0, 4), authored(0, 4)),
            verbatim(embedded(2, 6), authored(2, 6))
        ]),
        Err(MapStageError::OverlappingSegments)
    );
    assert_eq!(
        build(vec![verbatim(embedded(2, 2), authored(2, 2))]),
        Err(MapStageError::EmptyOutputRun)
    );
    assert_eq!(
        build(vec![verbatim(embedded(0, 9), authored(0, 9))]),
        Err(MapStageError::OutOfExtent)
    );
    assert_eq!(
        build(vec![verbatim(generated(0, 2), authored(0, 2))]),
        Err(MapStageError::DomainMismatch)
    );
}

#[test]
fn literal_geometry_is_refused_rather_than_repaired() {
    let hole = |outer, expression| HoleGeometry {
        outer,
        expression,
        region: hole_region(0),
    };
    assert_eq!(
        LiteralGeometry::new(sr(2, 5), sr(1, 5), Vec::new()),
        Err(GeometryError::BodyOutsideLiteral)
    );
    assert_eq!(
        LiteralGeometry::new(sr(0, 9), sr(1, 8), vec![hole(sr(0, 3), sr(1, 2))]),
        Err(GeometryError::HoleOutsideBody { ordinal: 0 })
    );
    assert_eq!(
        LiteralGeometry::new(
            sr(0, 9),
            sr(1, 8),
            vec![hole(sr(4, 6), sr(4, 6)), hole(sr(2, 3), sr(2, 3))]
        ),
        Err(GeometryError::HoleOutOfOrder { ordinal: 1 })
    );
    assert_eq!(
        LiteralGeometry::new(sr(0, 9), sr(1, 8), vec![hole(sr(2, 4), sr(3, 6))]),
        Err(GeometryError::ExpressionOutsideHole { ordinal: 0 })
    );

    // Geometry inside a scalar or past the source is typed, never snapped.
    let source = "`é`";
    let inside = LiteralGeometry::new(sr(0, 4), sr(2, 3), Vec::new()).expect("ordered");
    assert_eq!(
        decode_at(&template_raw(), source, &inside),
        Err(CodecRefusal::InvalidGeometry(CoordError::NotCharBoundary))
    );
    let past = LiteralGeometry::new(sr(0, 9), sr(1, 8), Vec::new()).expect("ordered");
    assert_eq!(
        decode_at(&template_raw(), source, &past),
        Err(CodecRefusal::InvalidGeometry(CoordError::InvalidBoundary))
    );

    // A basis naming another unit than the lineage is refused.
    let foreign = MapBasis {
        unit: unit("other.ts"),
        ..basis_of(source)
    };
    assert_eq!(
        template_raw().decode(
            &DecodeRequest {
                source,
                geometry: &parsed_template(source, 0),
                lineage: &root_lineage(),
                input: &foreign,
            },
            &NeverInterrupted,
        ),
        Err(CodecRefusal::BasisRefused(Unmappable::MismatchedBasis {
            stage: StageKind::Syntax
        }))
    );
}

// ---------------------------------------------------------------- incremental

/// Replace `[start, end)` of `source`, returning the new source and the edit.
fn splice(source: &str, start: usize, end: usize, inserted: &str) -> (String, LiteralEdit) {
    let edited = format!("{}{}{}", &source[..start], inserted, &source[end..]);
    let edit = LiteralEdit {
        base: basis_of(source),
        range: sr(start as u32, end as u32),
        inserted_len: inserted.len() as u32,
    };
    (edited, edit)
}

fn splice_decode(
    codec: &EmbeddedTextCodec,
    previous: &DecodedLiteral,
    edit: &LiteralEdit,
    source: &str,
    literal_start: usize,
) -> Result<DecodedLiteral, CodecRefusal> {
    codec.decode_splice(
        previous,
        edit,
        &DecodeRequest {
            source,
            geometry: &parsed_template(source, literal_start),
            lineage: &root_lineage(),
            input: &basis_of(source),
        },
        &NeverInterrupted,
    )
}

/// Every edit inside, between and across the structures a decoder cares about:
/// the splice answers exactly what a fresh decode answers, refusals included.
#[test]
fn a_splice_equals_a_fresh_decode_for_every_edit_position() {
    let prefix = "let s = ";
    let source = format!("{prefix}`aa\\u00E9b${{x}}c\\uD83D\\uDE00d\r\ne${{y}}f`;");
    let start = prefix.len();
    let at = |needle: &str| source.find(needle).expect("the fixture spells it");
    let escape = at("\\u00E9");
    let pair_gap = at("\\uDE00");
    let crlf = at("\r\n");
    let first_hole = at("${x}");
    let second_hole = at("${y}");
    let closer = source.rfind('`').expect("closes");

    struct Edit {
        name: &'static str,
        start: usize,
        end: usize,
        inserted: &'static str,
        /// Chunks a fresh decode would not repeat, when the edit succeeds.
        reused: Option<u32>,
    }
    let edits = [
        Edit {
            name: "inside an escape, breaking it",
            start: escape + 3,
            end: escape + 4,
            inserted: "Z",
            reused: None,
        },
        Edit {
            name: "inside an escape, keeping it valid",
            start: escape + 5,
            end: escape + 6,
            inserted: "A",
            reused: Some(2),
        },
        Edit {
            name: "between surrogate escapes",
            start: pair_gap,
            end: pair_gap,
            inserted: "q",
            reused: None,
        },
        Edit {
            name: "removing a surrogate half",
            start: pair_gap,
            end: pair_gap + 6,
            inserted: "",
            reused: None,
        },
        Edit {
            name: "inside a CRLF",
            start: crlf + 1,
            end: crlf + 1,
            inserted: "z",
            reused: Some(2),
        },
        Edit {
            name: "removing the CR of a CRLF",
            start: crlf,
            end: crlf + 1,
            inserted: "",
            reused: Some(2),
        },
        Edit {
            name: "an unescaped delimiter in text",
            start: escape,
            end: escape,
            inserted: "`",
            reused: None,
        },
        Edit {
            name: "removing the closing delimiter",
            start: closer,
            end: closer + 1,
            inserted: "",
            reused: None,
        },
        Edit {
            name: "breaking a hole opener",
            start: first_hole,
            end: first_hole + 1,
            inserted: "",
            reused: Some(1),
        },
        Edit {
            name: "inside a hole expression only",
            start: first_hole + 2,
            end: first_hole + 3,
            inserted: "longer_name",
            reused: Some(3),
        },
        Edit {
            name: "adding a hole",
            start: second_hole,
            end: second_hole,
            inserted: "${z}",
            reused: Some(3),
        },
        Edit {
            name: "removing a hole",
            start: second_hole,
            end: second_hole + 4,
            inserted: "",
            reused: Some(1),
        },
        Edit {
            name: "before the literal",
            start: 0,
            end: 3,
            inserted: "const",
            reused: Some(3),
        },
        Edit {
            name: "after the literal",
            start: closer + 1,
            end: closer + 2,
            inserted: "",
            reused: Some(3),
        },
        Edit {
            name: "an astral scalar at a chunk end",
            start: first_hole,
            end: first_hole,
            inserted: "😀",
            reused: Some(2),
        },
    ];

    let codec = template_cooked();
    let previous = decode_at(&codec, &source, &parsed_template(&source, start)).expect("decodes");
    assert_eq!(previous.text(), "aaéb".to_owned() + "c😀d\ne" + "f");

    for edit in edits {
        let literal_start = if edit.end <= start {
            start + edit.inserted.len() + edit.start - edit.end
        } else {
            start
        };
        let (edited, literal_edit) = splice(&source, edit.start, edit.end, edit.inserted);
        let fresh = decode_at(&codec, &edited, &parsed_template(&edited, literal_start));
        let incremental = splice_decode(&codec, &previous, &literal_edit, &edited, literal_start);
        assert_eq!(incremental, fresh, "{}", edit.name);
        if let Some(reused) = edit.reused {
            let work = incremental.expect("this edit decodes").work();
            assert_eq!(work.chunks_reused, reused, "{}", edit.name);
        }

        // Reverting from the edited state lands exactly on the original.
        if let Ok(edited_literal) =
            decode_at(&codec, &edited, &parsed_template(&edited, literal_start))
        {
            let removed = &source[edit.start..edit.end];
            let (reverted, revert) = splice(
                &edited,
                edit.start,
                edit.start + edit.inserted.len(),
                removed,
            );
            assert_eq!(reverted, source, "{}", edit.name);
            assert_eq!(
                splice_decode(&codec, &edited_literal, &revert, &reverted, start).as_ref(),
                Ok(&previous),
                "revert of {}",
                edit.name
            );
        }
    }

    // Unaffected chunks are reused, not re-scanned: only the edited chunk's
    // bytes are charged.
    let (edited, edit) = splice(&source, escape + 5, escape + 6, "A");
    let work = splice_decode(&codec, &previous, &edit, &edited, start)
        .expect("decodes")
        .work();
    assert_eq!(work.chunks_decoded, 1);
    assert_eq!(work.bytes_scanned, 9);
    let (edited, edit) = splice(&source, 0, 0, "// ");
    let work = splice_decode(&codec, &previous, &edit, &edited, start + 3)
        .expect("decodes")
        .work();
    assert_eq!((work.chunks_decoded, work.bytes_scanned), (0, 0));

    // A policy that depends on every line re-decodes everything and still agrees.
    let dedent = template_with(IndentationPolicy::StripCommonLeading {
        whitespace: Box::new([' ']),
        include_first_line: false,
    });
    let indented = "`\n  a${x}\n    b\n  c`";
    let before = decoded(&dedent, indented);
    assert_eq!(before.text(), "\na\n  b\nc");
    let line = indented.find("  a").expect("the fixture spells it");
    let (edited, edit) = splice(indented, line, line + 1, "");
    let incremental = splice_decode(&dedent, &before, &edit, &edited, 0);
    assert_eq!(incremental, decode_template(&dedent, &edited));
    assert_eq!(incremental.expect("decodes").text(), "\na\n   b\n c");

    // A different policy never reuses this one's chunks.
    let (edited, edit) = splice(&source, 0, 0, " ");
    let raw_after_cooked =
        splice_decode(&template_raw(), &previous, &edit, &edited, start + 1).expect("raw decodes");
    assert_eq!(raw_after_cooked.work().chunks_reused, 0);
    assert_eq!(
        raw_after_cooked,
        decode_at(
            &template_raw(),
            &edited,
            &parsed_template(&edited, start + 1)
        )
        .expect("raw decodes")
    );
}

/// Interrupts after `allowed` polls.
struct InterruptAfter {
    allowed: Cell<u32>,
}

impl DecodeInterrupt for InterruptAfter {
    fn is_interrupted(&self) -> bool {
        let left = self.allowed.get();
        if left == 0 {
            return true;
        }
        self.allowed.set(left - 1);
        false
    }
}

/// Malformed input, cancellation and a stale edit all produce a refusal, with
/// no partial value to retain, and leave the last complete value unchanged and
/// answering only for its own basis.
#[test]
fn degraded_decodes_produce_nothing_to_retain() {
    let codec = template_cooked();
    let good = "`ab${x}cd`";
    let previous = decoded(&codec, good);
    let snapshot = previous.clone();

    // Malformed, then repaired.
    let (broken, edit) = splice(good, 1, 2, "\\u{ZZ}");
    assert_eq!(
        splice_decode(&codec, &previous, &edit, &broken, 0),
        Err(CodecRefusal::MissingCookedValue(MissingCookedValue {
            authored: Some(sr(1, 3)),
        }))
    );
    assert_eq!(previous, snapshot);
    let (repaired, repair) = splice(good, 1, 2, "é");
    assert_eq!(
        splice_decode(&codec, &previous, &repair, &repaired, 0),
        decode_template(&codec, &repaired)
    );

    // Cancelled during decoding, at every chunk.
    for allowed in 0..2 {
        let (edited, edit) = splice(good, 1, 2, "z");
        let interrupt = InterruptAfter {
            allowed: Cell::new(allowed),
        };
        let cancelled = codec.decode_splice(
            &previous,
            &edit,
            &DecodeRequest {
                source: &edited,
                geometry: &parsed_template(&edited, 0),
                lineage: &root_lineage(),
                input: &basis_of(&edited),
            },
            &interrupt,
        );
        assert_eq!(cancelled, Err(CodecRefusal::Interrupted));
    }
    let interrupted_fresh = codec.decode(
        &DecodeRequest {
            source: good,
            geometry: &parsed_template(good, 0),
            lineage: &root_lineage(),
            input: &basis_of(good),
        },
        &InterruptAfter {
            allowed: Cell::new(1),
        },
    );
    assert_eq!(interrupted_fresh, Err(CodecRefusal::Interrupted));

    // An edit computed against another revision is stale, not applied.
    let (edited, edit) = splice("`other`", 1, 2, "z");
    assert_eq!(
        splice_decode(&codec, &previous, &edit, &edited, 0),
        Err(CodecRefusal::BasisRefused(Unmappable::StaleBasis {
            stage: StageKind::Syntax
        }))
    );
    assert_eq!(previous, snapshot);

    // The old value still answers for its basis, and for no other.
    let chain = AuthoredMapChain::new(
        attachment_for(root_lineage(), None),
        previous.stage().clone(),
    )
    .expect("starts");
    let current = decoded(&codec, &repaired);
    assert_eq!(
        chain.provenance(current.stage().output(), embedded(0, 1)),
        Err(Unmappable::StaleBasis {
            stage: StageKind::Syntax
        })
    );
    assert!(chain
        .provenance(previous.stage().output(), embedded(0, 1))
        .is_ok());
}

// ---------------------------------------------------------------- fuzz

/// Deterministic generator: `xorshift64*`, seeded per case.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// `(authored spelling, cooked text)`. The table is the oracle: it states what
/// each spelling means without any decoding.
const ATOMS: [(&str, &str); 14] = [
    ("a", "a"),
    ("é", "é"),
    ("😀", "😀"),
    ("\\n", "\n"),
    ("\\t", "\t"),
    ("\\u00E9", "é"),
    ("\\u{1F600}", "😀"),
    ("\\uD83D\\uDE00", "😀"),
    ("\\`", "`"),
    ("\\$", "$"),
    ("\\\n", ""),
    ("\\\r\n", ""),
    ("\r\n", "\n"),
    ("\r", "\n"),
];
const LONE_HIGH: &str = "\\uD83D";
const HOLE: &str = "${h}";

#[derive(Clone, Copy, PartialEq)]
enum Atom {
    Table(usize),
    LoneHigh,
    Hole,
}

fn spell(atoms: &[Atom]) -> String {
    let mut source = String::from("`");
    for atom in atoms {
        source.push_str(match atom {
            Atom::Table(index) => ATOMS[*index].0,
            Atom::LoneHigh => LONE_HIGH,
            Atom::Hole => HOLE,
        });
    }
    source.push('`');
    source
}

/// What the generator knows the literal must answer.
fn check_against_generator(atoms: &[Atom], literal: &DecodedLiteral, seed: u64) {
    let mut cooked = String::new();
    let mut authored = 1u32;
    let mut pending = Vec::new();
    for atom in atoms {
        match atom {
            Atom::Table(index) => {
                let (spelling, value) = ATOMS[*index];
                if !value.is_empty() {
                    pending.push((cooked.len() as u32, authored, spelling.len() as u32));
                }
                cooked.push_str(value);
                authored += spelling.len() as u32;
            }
            Atom::Hole => authored += HOLE.len() as u32,
            Atom::LoneHigh => unreachable!("a lone surrogate never decodes"),
        }
    }
    assert_eq!(literal.text(), cooked, "seed {seed}");
    for (cooked_start, authored_start, spelling_len) in pending {
        assert_eq!(
            literal.map_position(eo(cooked_start)),
            Ok(so(authored_start)),
            "seed {seed}"
        );
        // An atom's whole output answers with the atom's whole spelling, or for
        // a run of plain text with exactly its own bytes.
        let value_len = literal
            .text()
            .get(cooked_start as usize..)
            .and_then(|rest| rest.chars().next())
            .map_or(0, char::len_utf8) as u32;
        assert_eq!(
            literal.map_range(er(cooked_start, cooked_start + value_len)),
            Ok(vec![sr(authored_start, authored_start + spelling_len)]),
            "seed {seed}"
        );
    }
}

#[test]
fn generated_literals_agree_with_their_generator_fresh_and_spliced() {
    let codec = template_cooked();
    let mut refusals = 0;
    let mut admitted = 0;
    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let pick = |rng: &mut Rng| match rng.below(20) {
            0 => Atom::LoneHigh,
            1 | 2 => Atom::Hole,
            _ => Atom::Table(rng.below(ATOMS.len())),
        };
        let mut atoms: Vec<Atom> = (0..rng.below(24)).map(|_| pick(&mut rng)).collect();
        let source = spell(&atoms);
        let geometry = parsed_template(&source, 0);
        let fresh = decode_at(&codec, &source, &geometry);

        match atoms.iter().position(|atom| *atom == Atom::LoneHigh) {
            Some(first) => {
                let start = 1 + spell(&atoms[..first]).len() - 2;
                assert_eq!(
                    fresh,
                    Err(CodecRefusal::NonUnicodeCookedLiteral(
                        NonUnicodeCookedLiteral {
                            surrogate: SurrogateKind::High,
                            authored: Some(sr(start as u32, (start + LONE_HIGH.len()) as u32)),
                        }
                    )),
                    "seed {seed}"
                );
                refusals += 1;
                continue;
            }
            None => {
                let literal = fresh.as_ref().expect("no lone surrogate");
                check_against_generator(&atoms, literal, seed);
                admitted += 1;
            }
        }

        // A short random edit history: each step agrees with a fresh decode.
        let mut current = fresh.expect("checked above");
        let mut current_source = source;
        for _ in 0..3 {
            let replaced = rng.below(atoms.len().max(1));
            let replacement = pick(&mut rng);
            let before = if atoms.is_empty() {
                atoms.push(replacement);
                1usize
            } else {
                let before = spell(&atoms[..replaced]).len() - 1;
                atoms[replaced] = replacement;
                before
            };
            let next_source = spell(&atoms);
            // The smallest edit between the two spellings.
            let common_start = current_source
                .bytes()
                .zip(next_source.bytes())
                .take_while(|(a, b)| a == b)
                .count()
                .min(before);
            let common_end = current_source.as_bytes()[common_start..]
                .iter()
                .rev()
                .zip(next_source.as_bytes()[common_start..].iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            let old_end = current_source.len() - common_end;
            let new_end = next_source.len() - common_end;
            let (edited, edit) = splice(
                &current_source,
                common_start,
                old_end,
                &next_source[common_start..new_end],
            );
            assert_eq!(edited, next_source, "seed {seed}");
            let fresh = decode_template(&codec, &edited);
            let incremental = splice_decode(&codec, &current, &edit, &edited, 0);
            assert_eq!(incremental, fresh, "seed {seed}");
            let Ok(next) = incremental else { break };
            check_against_generator(&atoms, &next, seed);
            current = next;
            current_source = edited;
        }
    }
    // The generator must exercise both outcomes to mean anything.
    assert!(refusals > 20 && admitted > 100, "{refusals} / {admitted}");
}

/// A chain of random byte permutations answers exactly what composing the
/// generator's own byte tables answers, or refuses.
#[test]
fn composed_lookups_agree_with_directly_composed_byte_tables() {
    const LEN: u32 = 48;
    let text: String = "x".repeat(LEN as usize);
    let source = format!("`{text}`");
    for seed in 1..=120u64 {
        let mut rng = Rng(seed.wrapping_mul(0xD134_2543_DE82_EF95) | 1);
        let (literal, mut chain) = syntax_chain_over(&source);
        let mut basis = literal.stage().output().clone();
        // `table[output byte] = authored byte`, built only from the generator.
        let mut table: Vec<u32> = (1..=LEN).collect();
        for stage_index in 0..1 + rng.below(3) {
            let mut cuts = vec![0, LEN];
            for _ in 0..rng.below(5) {
                cuts.push(1 + rng.below(LEN as usize - 1) as u32);
            }
            cuts.sort_unstable();
            cuts.dedup();
            let mut runs: Vec<(u32, u32)> = cuts.windows(2).map(|w| (w[0], w[1])).collect();
            for index in (1..runs.len()).rev() {
                runs.swap(index, rng.below(index + 1));
            }
            let mut segments = Vec::new();
            let mut output_start = 0;
            let mut next_table = Vec::with_capacity(table.len());
            for (input_start, input_end) in runs {
                let length = input_end - input_start;
                segments.push(verbatim(
                    embedded(output_start, output_start + length),
                    embedded(input_start, input_end),
                ));
                next_table.extend_from_slice(&table[input_start as usize..input_end as usize]);
                output_start += length;
            }
            table = next_table;
            let revision: &'static str = ["s0", "s1", "s2"][stage_index];
            let output = MapBasis {
                map_revision: map_revision(revision),
                content: ContentId::from_content_bytes(format!("{seed}/{revision}").as_bytes()),
                ..basis.clone()
            };
            let stage = mapped_stage(
                StageKind::Preprocessor,
                EditCertification::Reversible,
                basis,
                output.clone(),
                LEN,
                segments,
            );
            chain = chain.then(stage).expect("joins");
            basis = output;
        }

        for _ in 0..24 {
            let start = rng.below(LEN as usize) as u32;
            let end = start + 1 + rng.below((LEN - start) as usize) as u32;
            let answer = chain
                .provenance(&basis, embedded(start, end))
                .expect("a permutation always answers");
            let bytes: Vec<u32> = answer
                .origins()
                .iter()
                .flat_map(|origin| origin.range.start().get()..origin.range.end().get())
                .collect();
            assert_eq!(
                bytes,
                table[start as usize..end as usize],
                "seed {seed} range {start}..{end}"
            );
            let contiguous = table[start as usize..end as usize]
                .windows(2)
                .all(|pair| pair[1] == pair[0] + 1);
            // A stage may split a run the table sees as contiguous only by
            // keeping it adjacent, which still joins. Anything else is refused.
            let edit = chain.editable_preimage(&basis, embedded(start, end));
            if !contiguous {
                assert_eq!(edit, Err(Unmappable::AmbiguousPreimage), "seed {seed}");
            }
            if let Ok(origin) = edit {
                assert!(contiguous, "seed {seed}");
                assert_eq!(
                    origin.range,
                    sr(table[start as usize], table[end as usize - 1] + 1)
                );
            }
            assert_eq!(
                chain
                    .authored_position(&basis, DomainOffset::Embedded(eo(start)))
                    .map(|origin| origin.range.start()),
                Ok(so(table[start as usize]))
            );
        }
    }
}

// ---------------------------------------------------------------- work

fn literal_of(unit_text: &str, repeats: usize) -> String {
    format!("`{}`", unit_text.repeat(repeats))
}

/// `(calls, bytes)` for one decode, after asserting the charged work.
fn decode_cost(
    codec: &EmbeddedTextCodec,
    source: &str,
    segments_per_byte_bound: u64,
) -> (u64, u64) {
    let geometry = parsed_template(source, 0);
    let (literal, calls, bytes) = measured(|| decode_at(codec, source, &geometry));
    let literal = literal.expect("decodes");
    let work = literal.work();
    assert_eq!(
        work.bytes_scanned,
        u64::from(geometry.body().len()) - holes_bytes(&geometry),
        "every chunk byte is scanned exactly once"
    );
    assert!(
        work.segments_emitted <= u64::from(geometry.body().len()) / segments_per_byte_bound + 2,
        "{} segments for {} bytes",
        work.segments_emitted,
        geometry.body().len()
    );
    (calls, bytes)
}

fn holes_bytes(geometry: &LiteralGeometry) -> u64 {
    geometry
        .holes()
        .iter()
        .map(|hole| u64::from(hole.outer.len()))
        .sum()
}

/// Allocation grows with the input, never with a copy per prefix. Calls grow
/// logarithmically at most, requested bytes linearly: a decoder that rebuilt a
/// buffer per chunk or per escape would show a ratio near four here.
#[test]
fn decode_allocation_grows_linearly_with_input() {
    const N: usize = 2_000;
    let cases: [(&str, EmbeddedTextCodec, &str, u64); 4] = [
        ("identity raw", template_raw(), "abcdefgh", 1),
        (
            "transformed cooked",
            template_cooked(),
            "ab\\u00E9\\n\r\n",
            2,
        ),
        ("many chunks and holes", template_cooked(), "ab${h}", 1),
        ("non-ASCII text", template_cooked(), "éééé", 1),
    ];
    for (name, codec, unit_text, segment_bound) in cases {
        let (calls, bytes) = decode_cost(&codec, &literal_of(unit_text, N), segment_bound);
        let (calls_2n, bytes_2n) =
            decode_cost(&codec, &literal_of(unit_text, N * 2), segment_bound);
        assert!(
            calls_2n <= calls + 32,
            "{name}: allocation calls grew from {calls} to {calls_2n}"
        );
        assert!(
            bytes_2n * 10 <= bytes * 25,
            "{name}: requested bytes grew from {bytes} to {bytes_2n}"
        );
    }
}

/// A warm query walks the retained map: nothing decodes, a position allocates
/// nothing, and neither depends on the input size.
#[test]
fn warm_queries_do_no_decode_and_do_not_scale_with_input() {
    const N: usize = 4_000;
    let codec = template_cooked();
    let answers_for = |repeats: usize| {
        let source = literal_of("ab\\u00E9", repeats);
        let literal = decoded(&codec, &source);
        let work = literal.work();
        let middle = repeats as u32 * 2;
        let (position, position_calls, _) = measured(|| literal.map_position(eo(middle)));
        assert!(position.is_ok());
        assert_eq!(position_calls, 0, "a position lookup allocates nothing");
        let (range, range_calls, _) = measured(|| literal.map_range(er(middle, middle + 1)));
        assert!(range.is_ok());
        assert_eq!(literal.work(), work, "a query charges no decode work");
        range_calls
    };
    assert_eq!(answers_for(N), answers_for(N * 2));
}

/// A lookup visits each stage once and retains nothing: its cost follows the
/// chain's depth, not the size of the text beneath it.
#[test]
fn chain_lookup_cost_follows_depth_not_text_size() {
    let build = |len: u32, depth: usize| {
        let text = "x".repeat(len as usize);
        let (literal, mut chain) = syntax_chain_over(&format!("`{text}`"));
        let mut basis = literal.stage().output().clone();
        let names = ["d0", "d1", "d2", "d3"];
        for name in &names[..depth] {
            let output = MapBasis {
                map_revision: map_revision(name),
                ..basis.clone()
            };
            let stage = mapped_stage(
                StageKind::Preprocessor,
                EditCertification::Reversible,
                basis,
                output.clone(),
                len,
                vec![verbatim(embedded(0, len), embedded(0, len))],
            );
            chain = chain.then(stage).expect("joins");
            basis = output;
        }
        (chain, basis)
    };
    let cost = |len: u32, depth: usize| {
        let (chain, basis) = build(len, depth);
        let (answer, calls, _) =
            measured(|| chain.authored_position(&basis, DomainOffset::Embedded(eo(len / 2))));
        assert_eq!(answer.map(|o| o.range.start()), Ok(so(len / 2 + 1)));
        let (range, range_calls, _) = measured(|| chain.provenance(&basis, embedded(1, 3)));
        assert_eq!(
            range.expect("exact").exact().map(|o| o.range),
            Some(sr(2, 4))
        );
        (calls, range_calls)
    };
    assert_eq!(cost(4_000, 3), cost(8_000, 3));
    assert_eq!(
        cost(4_000, 3).0,
        0,
        "a position through a chain allocates nothing"
    );
}

/// A splice re-decodes one chunk however large the rest of the literal is.
#[test]
fn splice_work_follows_the_edited_chunk() {
    let codec = template_cooked();
    let run = |repeats: usize| {
        let source = literal_of("ab\\u00E9${h}", repeats);
        let previous = decoded(&codec, &source);
        let (edited, edit) = splice(&source, 1, 2, "z");
        let work = splice_decode(&codec, &previous, &edit, &edited, 0)
            .expect("decodes")
            .work();
        (work, repeats as u32)
    };
    for (work, repeats) in [run(100), run(200)] {
        assert_eq!(work.chunks_decoded, 1);
        assert_eq!(work.chunks_reused, repeats);
        assert_eq!(work.bytes_scanned, 8);
    }
}

/// Selecting a region costs nothing and observes no text: a plain carrier and
/// an unrequested region do no decode at all, and decoding is an explicit act.
#[test]
fn selection_alone_does_no_decode_work() {
    let schema = ProfileSchemaEpoch::for_schema_family("carrier", 1);
    let carrier = |name: &str| CarrierProfileId::from_carrier(name, &schema);
    let row = |host_name: &str, role: &str| EmbeddedRegionRow {
        host_carrier: carrier(host_name),
        region_role: role.into(),
        embedded_carrier: carrier("template"),
        codec: Arc::new(template_cooked()),
        map_chain: Box::new([StageKind::Syntax]),
    };
    let table = EmbeddedRegionTable::new(vec![
        row("host-a", "template"),
        row("host-a", "style"),
        row("host-b", "template"),
    ])
    .expect("distinct keys");

    let (selected_host, plain_host) = (carrier("host-a"), carrier("plain-script"));
    let (selected, calls, _) = measured(|| table.select(&selected_host, "template"));
    assert_eq!(calls, 0, "selection allocates nothing");
    let selected = selected.expect("the row exists");
    let (plain, calls, _) = measured(|| table.select(&plain_host, "template"));
    assert_eq!((plain, calls), (None, 0));
    let (inactive, calls, _) = measured(|| table.select(&selected_host, "docs"));
    assert_eq!((inactive, calls), (None, 0));

    // Only the requested decode does work, and it is charged.
    let source = "`abc`";
    let work = selected
        .codec
        .decode(
            &DecodeRequest {
                source,
                geometry: &parsed_template(source, 0),
                lineage: &root_lineage(),
                input: &basis_of(source),
            },
            &NeverInterrupted,
        )
        .expect("decodes")
        .work();
    assert_eq!((work.chunks_decoded, work.bytes_scanned), (1, 3));

    assert_eq!(
        EmbeddedRegionTable::new(vec![row("host-a", "template"), row("host-a", "template")]),
        Err(DuplicateEmbeddedRegionRow {
            host_carrier: carrier("host-a"),
            region_role: "template".into(),
        }),
        "two rows for one key never overwrite each other"
    );
}

/// The measurements above mean something only if the counter sees this thread.
#[test]
fn counting_allocator_counts_this_threads_allocations() {
    let (_, calls, bytes) = measured(|| Vec::<u8>::with_capacity(4096));
    assert!(calls >= 1 && bytes >= 4096, "{calls} calls, {bytes} bytes");
    let (_, calls, _) = measured(|| 1 + 1);
    assert_eq!(calls, 0);
}

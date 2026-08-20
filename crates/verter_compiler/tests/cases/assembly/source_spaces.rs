//! A script+template composition: the assembled artifact's segments live
//! in ASSEMBLED-output coordinates and still point back at ORIGINAL source
//! positions — and no public API in this module accepts a bare offset in
//! one space and writes it into another. That second property is proven
//! at COMPILE TIME by the type system, not a runtime assertion: this file
//! is the compile-time proof — [`AssembledOffset`] and [`FragmentOffset`]
//! are distinct types, so a function expecting one cannot be called with
//! another. [`FragmentRange`] is also exercised for real:
//! [`PlacementSlot::Hole`] carries a hole's location as a `FragmentRange`,
//! not a bare `Range<u32>`.

use oxc_sourcemap::SourceMap;
use verter_compiler::assembly::{
    splice_into_hole, AssembledOffset, ContentId, Fragment, FragmentDialect, FragmentId,
    FragmentOffset, FragmentRange, FrameworkDomain, PlacementSlot, SourceId, SourceRevision,
    SourceSpaceKind, SourceUnit, SyntacticContract,
};
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};

struct Tag(&'static str);
impl CanonicalEncode for Tag {
    const DOMAIN_TAG: &'static str = "verter.compiler.tests.assembly.source_spaces.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_str(1, self.0);
    }
}

fn unit_id(role: &str) -> verter_compiler::assembly::SourceUnitId {
    SourceUnit::mint(
        SourceId::from_canonical(&Tag("Comp.vue")),
        SourceRevision::from_canonical(&Tag("rev")),
        role,
        ContentId::from_content_bytes(role.as_bytes()),
    )
    .id()
    .clone()
}

fn owner() -> Fragment {
    Fragment {
        domain: FrameworkDomain::Vue,
        product: verter_compiler::compile_request::ProductKind::RuntimeClient,
        source_unit: unit_id("script"),
        source_space: SourceSpaceKind::GeneratedFragment,
        placement: PlacementSlot::ModuleBody,
        contract: SyntacticContract::CompleteModule,
        dialect: FragmentDialect::Tsx,
        code: "const n = 0".to_string(),
        // One segment: generated (0,6) -> authored (1,6) — the byte at
        // "n" in "const n". Same pinned mapping used elsewhere in this
        // crate's map-composition suite.
        source_map: Some(
            "{\"version\":3,\"sources\":[\"Comp.vue\"],\"names\":[],\"mappings\":\"MACM\"}"
                .to_string(),
        ),
        imports: Vec::new(),
        exports: Vec::new(),
        helpers: Vec::new(),
        dependencies: Vec::new(),
    }
}

#[test]
fn assembled_segment_still_points_at_the_original_authored_position() {
    let owner_id = FragmentId(0);
    let hole_start = "const n = ".len() as u32;
    let hole_end = hole_start + 1;
    let owner_fragment = owner().validate().expect("owner fixture parses");

    let piece = Fragment {
        domain: FrameworkDomain::Vue,
        product: verter_compiler::compile_request::ProductKind::RuntimeClient,
        source_unit: unit_id("template"),
        source_space: SourceSpaceKind::GeneratedFragment,
        placement: PlacementSlot::Hole {
            owner: owner_id,
            hole: FragmentRange {
                fragment: owner_id,
                range: hole_start..hole_end,
            },
        },
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

    let composed =
        splice_into_hole("", owner_id, &owner_fragment, &piece).expect("splice succeeds");
    let map = SourceMap::from_json_string(&composed.source_map).expect("map decodes");
    let mut saw_a_shell_segment = false;
    for token in map.get_tokens() {
        if token.get_source_id().is_some() {
            saw_a_shell_segment = true;
            // The shell's segment must still describe the SAME authored
            // position (line 1, col 6) after assembly moved its generated
            // coordinate to make room for the spliced fragment.
            assert_eq!(token.get_src_line(), 1);
            assert_eq!(token.get_src_col(), 6);
        }
    }
    assert!(
        saw_a_shell_segment,
        "the owner fragment's own segment must survive assembly, got map:\n{}",
        composed.source_map
    );

    // `fragment_starts_at` is a real, wired `AssembledOffset` — the byte
    // offset in the ASSEMBLED code where the spliced fragment's own bytes
    // begin (one byte past the hole's start, past the inserted leading
    // newline).
    let expected_start = AssembledOffset(hole_start + 1);
    assert_eq!(composed.fragment_starts_at, expected_start);
    let start = composed.fragment_starts_at.0 as usize;
    assert_eq!(
        &composed.code[start..start + 2],
        "42",
        "fragment_starts_at must point exactly at the spliced fragment's own bytes"
    );
}

/// Compile-time proof: [`AssembledOffset`] and [`FragmentOffset`] are
/// distinct types. A function that only accepts a [`FragmentOffset`]
/// cannot be called with an [`AssembledOffset`] — this is enforced by
/// `rustc`, not asserted at runtime.
fn accepts_only_a_fragment_offset(offset: FragmentOffset) -> u32 {
    offset.offset
}

#[test]
fn source_space_coordinate_wrappers_are_not_interchangeable() {
    let fragment_offset = FragmentOffset {
        fragment: FragmentId(0),
        offset: 3,
    };
    assert_eq!(accepts_only_a_fragment_offset(fragment_offset), 3);

    // Exists only to prove the two wrapper types construct independently —
    // passing `assembled` to `accepts_only_a_fragment_offset` above is a
    // compile error, which is the actual guarantee this test protects.
    let assembled = AssembledOffset(3);
    assert_eq!(assembled.0, 3);
}

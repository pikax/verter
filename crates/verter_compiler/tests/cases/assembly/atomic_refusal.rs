//! Every assembly failure mode returns a typed refusal and constructs
//! nothing — a positive control proves the harness is not vacuously green.

use verter_compiler::assembly::{
    splice_into_hole, ArtifactContribution, ComposeRefusal, ContentId, Fragment, FragmentDialect,
    FragmentId, FragmentRange, FrameworkDomain, PlacementSlot, ProductPlan, SourceId,
    SourceRevision, SourceSpaceKind, SourceUnit, SyntacticContract,
};
use verter_compiler::compile_request::{
    CompileProduct, CompileRequest, FrameworkCompileRequest, IdeProductRequest, VueCompileRequest,
};
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};

struct Tag(&'static str);
impl CanonicalEncode for Tag {
    const DOMAIN_TAG: &'static str = "verter.compiler.tests.assembly.atomic_refusal.v1";
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

fn base(contract: SyntacticContract, code: &str) -> Fragment {
    Fragment {
        domain: FrameworkDomain::Vue,
        product: verter_compiler::compile_request::ProductKind::RuntimeClient,
        source_unit: unit_id("fixture"),
        source_space: SourceSpaceKind::GeneratedFragment,
        placement: PlacementSlot::ModuleBody,
        contract,
        dialect: FragmentDialect::Tsx,
        code: code.to_string(),
        source_map: None,
        imports: Vec::new(),
        exports: Vec::new(),
        helpers: Vec::new(),
        dependencies: Vec::new(),
    }
}

// ── Positive control ────────────────────────────────────────────────

#[test]
fn a_well_formed_assembly_succeeds() {
    let fragment = base(SyntacticContract::CompleteModule, "export default {}");
    assert!(fragment.validate().is_ok());
}

// ── Fragment parse failure ──────────────────────────────────────────

#[test]
fn fragment_parse_failure_is_refused() {
    let fragment = base(SyntacticContract::CompleteModule, "export default {");
    assert!(fragment.validate().is_err());
}

// ── Missing required projection map ─────────────────────────────────

#[test]
fn missing_required_projection_map_refuses_the_whole_publish() {
    let request = CompileRequest::new(
        vec![CompileProduct::IdeCompanion(IdeProductRequest::default())],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        None,
        None,
        false,
        false,
    )
    .unwrap();
    let plan = ProductPlan::from_request(&request);
    let contribution = ArtifactContribution {
        kind: verter_compiler::compile_request::ProductKind::IdeCompanion,
        fragments: Vec::new(),
        code: "export default {}".to_string(),
        emitted_imports: Vec::new(),
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    let result = verter_compiler::assembly::publish(&plan, vec![contribution]);
    assert!(result.is_err(), "must refuse, publishing no artifact");
}

// ── Uncomposable map ─────────────────────────────────────────────────

#[test]
fn uncomposable_map_is_refused() {
    let owner_id = FragmentId(0);
    let owner = base(SyntacticContract::CompleteModule, "const n = 0")
        .validate()
        .expect("owner parses");
    let mut malformed_owner = owner.into_fragment();
    malformed_owner.source_map = Some("not json at all".to_string());
    let malformed_owner = malformed_owner
        .validate()
        .expect("contract validation does not look at the map");

    let piece = Fragment {
        placement: PlacementSlot::Hole {
            owner: owner_id,
            hole: FragmentRange {
                fragment: owner_id,
                range: 6..7,
            },
        },
        ..base(SyntacticContract::Expression, "1")
    }
    .validate()
    .expect("piece parses");

    let err = splice_into_hole("", owner_id, &malformed_owner, &piece).unwrap_err();
    assert_eq!(err, ComposeRefusal::UncomposableMap);
}

// ── Missing declared hole ───────────────────────────────────────────

#[test]
fn a_fragment_with_no_declared_hole_at_all_is_refused() {
    let owner_id = FragmentId(0);
    let owner = base(SyntacticContract::CompleteModule, "const n = 0")
        .validate()
        .expect("owner parses");
    // Declares `ModuleBody` placement, not a `Hole` — nothing names where
    // in `owner`'s bytes this piece should land.
    let piece = base(SyntacticContract::Expression, "1")
        .validate()
        .expect("piece parses");
    let err = splice_into_hole("", owner_id, &owner, &piece).unwrap_err();
    assert_eq!(err, ComposeRefusal::NotAHolePlacement);
}

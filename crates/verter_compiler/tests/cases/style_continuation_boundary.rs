//! The compiler's stage-qualified external-style continuation boundary.
//!
//! Proves the one thing this authority does: it reads a completed external
//! preprocessing result's stated identity, stage and basis, refuses anything
//! that does not describe the style unit it claims, and turns what it admits
//! into an artifact whose bytes, provenance, source-unit relation and map
//! spaces round-trip deterministically.
//!
//! What it must NOT do is equally load-bearing and is asserted negatively
//! throughout: it never inspects, parses, preprocesses or normalises CSS, so
//! every refusal below is provoked by a wrong FACT, never by wrong bytes, and
//! bytes the boundary has no opinion on survive verbatim.

use std::collections::BTreeSet;

use verter_compiler::assembly::{
    ArtifactContent, ArtifactMapFamily, ArtifactMapSegment, ArtifactProvenance, ArtifactRelation,
    ArtifactRelationKind, ArtifactSourceUnit, ContentId, SourceId, SourceRevision, SourceUnit,
};
use verter_compiler::compile_request::ProductKind;
use verter_compiler::style_planner::{
    describe_style_continuations, ExternalStyleContinuation, StyleContinuationInput,
    StyleContinuationRefusal,
};
use verter_css_syntax::{
    CssDialect, ExternalStyleProducer, PreprocessorIdentity, QualifiedStyleResult, StyleDiagnostic,
    StyleStage,
};
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{InputBasisId, ResultContractId};
use verter_span::Span;

static_assertions::assert_not_impl_any!(ExternalStyleContinuation: Default);

struct Tag(&'static str);
impl CanonicalEncode for Tag {
    const DOMAIN_TAG: &'static str = "verter.compiler.style_continuation.test.tag.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_str(1, self.0);
    }
}

const AUTHORED: &str = ".a { color: $brand; }";
const PRODUCED: &str = ".a { color: red; }";
/// Authored extent inside the carrier: `<style lang="scss">` opens at byte 40.
const AUTHORED_EXTENT: Span = Span::new(40, 40 + AUTHORED.len() as u32);

fn style_unit(source: &'static str, role: &str) -> SourceUnit {
    SourceUnit::mint(
        SourceId::from_canonical(&Tag(source)),
        SourceRevision::from_canonical(&Tag("rev-1")),
        role,
        ContentId::from_content_bytes(AUTHORED.as_bytes()),
    )
}

fn sass() -> PreprocessorIdentity {
    PreprocessorIdentity::Named(
        ExternalStyleProducer::new("sass", Some("1.77.0"), None).expect("named producer"),
    )
}

fn completed(diagnostics: Vec<StyleDiagnostic>) -> QualifiedStyleResult {
    QualifiedStyleResult::preprocessed(sass(), PRODUCED, diagnostics)
}

/// A well-formed input with one anchor over the whole block. Every negative
/// case below mutates exactly one field of this, so the refusal it provokes is
/// attributable to that field and to nothing else.
fn input(unit: SourceUnit) -> StyleContinuationInput {
    let anchor = ArtifactMapSegment {
        generated: 0..PRODUCED.len() as u32,
        source_unit: unit.id().clone(),
        source_span: AUTHORED_EXTENT,
    };
    StyleContinuationInput {
        authored_dialect: CssDialect::Scss,
        consumed_basis: unit.content().clone(),
        authored_extent: AUTHORED_EXTENT,
        provenance: ArtifactProvenance {
            input_basis: InputBasisId::from_canonical(&Tag("observed inputs")),
            producer: ResultContractId::from_canonical(&Tag("external preprocessing")),
            inputs: BTreeSet::from([unit.id().clone()]),
        },
        result: completed(Vec::new()),
        anchors: vec![anchor],
        product: ProductKind::RuntimeClient,
        style_unit: unit,
    }
}

fn admit(input: StyleContinuationInput) -> ExternalStyleContinuation {
    ExternalStyleContinuation::admit(input).expect("well-formed continuation is admitted")
}

fn refusal(input: StyleContinuationInput) -> StyleContinuationRefusal {
    ExternalStyleContinuation::admit(input).expect_err("continuation must be refused")
}

#[test]
fn completed_external_preprocessing_round_trips_as_a_qualified_style_artifact() {
    let unit = style_unit("Comp.vue", "style:0");
    let diagnostics = vec![
        StyleDiagnostic::new(
            StyleStage::Authored,
            "unused variable",
            Some(Span::new(5, 9)),
        ),
        StyleDiagnostic::new(StyleStage::Preprocessed, "deprecated colour", None),
    ];
    let mut supplied = input(unit.clone());
    supplied.result = completed(diagnostics.clone());

    let continuation = admit(supplied);
    assert_eq!(continuation.authored_dialect(), CssDialect::Scss);
    assert_eq!(continuation.result().stage(), StyleStage::Preprocessed);
    assert_eq!(
        continuation.result().dialect(),
        CssDialect::Css,
        "completed preprocessing has left the authored dialect behind"
    );
    assert_eq!(continuation.result().code(), PRODUCED);
    assert_eq!(
        continuation.diagnostics(),
        diagnostics.as_slice(),
        "diagnostics survive admission in production order, values intact"
    );
    assert_eq!(continuation.style_unit().id(), unit.id());
    assert_eq!(continuation.authored_extent(), AUTHORED_EXTENT);

    let set = describe_style_continuations(vec![continuation], Vec::new())
        .expect("an admitted continuation describes a valid artifact set");
    assert_eq!(set.artifacts().len(), 1);
    let artifact = &set.artifacts()[0];
    assert_eq!(
        artifact.content,
        ArtifactContent::Available(PRODUCED.to_owned()),
        "the produced bytes reach the artifact verbatim, never re-emitted"
    );
    assert_eq!(
        artifact.source_unit(),
        unit.id(),
        "the artifact's source-unit relation names the style block it continues"
    );
    assert_eq!(artifact.language().as_str(), "css");
    assert_eq!(artifact.product(), ProductKind::RuntimeClient);

    assert_eq!(artifact.maps.len(), 1);
    let map = &artifact.maps[0];
    assert_eq!(map.family, ArtifactMapFamily::SourceProjection);
    assert_eq!(map.generated, *artifact.id());
    assert_eq!(map.input_basis, artifact.provenance.input_basis);
    assert_eq!(
        map.generated_content,
        ContentId::from_content_bytes(PRODUCED.as_bytes()),
        "the map is bound to the exact produced bytes"
    );
    assert_eq!(map.sources, BTreeSet::from([unit.id().clone()]));
    assert_eq!(map.segments.len(), 1);
    assert_eq!(map.segments[0].source_span, AUTHORED_EXTENT);
    assert_eq!(map.segments[0].generated, 0..PRODUCED.len() as u32);

    let wire: serde_json::Value =
        serde_json::from_str(&set.to_json().expect("validated sets serialize"))
            .expect("terminal json");
    assert_eq!(wire["artifacts"][0]["content"]["text"], PRODUCED);
    assert_eq!(
        wire["artifacts"][0]["provenance"]["inputBasis"],
        hex::encode(artifact.provenance.input_basis.canonical_bytes())
    );
    assert_eq!(
        wire["artifacts"][0]["sourceUnit"],
        hex::encode(unit.id().canonical_bytes())
    );
    assert_eq!(
        wire["artifacts"][0]["maps"][0]["family"],
        "sourceProjection"
    );
}

#[test]
fn only_a_completed_preprocessed_result_is_admissible() {
    let unit = style_unit("Comp.vue", "style:0");
    let uncontinuable = |result: QualifiedStyleResult| {
        let mut supplied = input(unit.clone());
        supplied.anchors.clear();
        supplied.result = result;
        refusal(supplied)
    };

    assert_eq!(
        uncontinuable(QualifiedStyleResult::authored(
            CssDialect::Scss,
            AUTHORED,
            Vec::new()
        )),
        StyleContinuationRefusal::WrongStage,
        "authored bytes are a different byte space, however valid they are"
    );
    assert_eq!(
        uncontinuable(QualifiedStyleResult::framework_rewritten(
            CssDialect::Css,
            PRODUCED,
            Vec::new()
        )),
        StyleContinuationRefusal::WrongStage,
        "a rewrite's output is downstream of the continuation, not its input"
    );
    assert_eq!(
        uncontinuable(QualifiedStyleResult::refused(
            StyleStage::Preprocessed,
            CssDialect::Css,
            Vec::new()
        )),
        StyleContinuationRefusal::RefusedResult,
        "a partial/cancelled run produced no bytes to continue from"
    );
    assert_eq!(
        uncontinuable(QualifiedStyleResult::refused(
            StyleStage::Authored,
            CssDialect::Scss,
            Vec::new()
        )),
        StyleContinuationRefusal::RefusedResult,
    );

    // An empty-but-produced result is NOT a refusal: `<style></style>` runs
    // through a preprocessor and legitimately yields nothing.
    let mut empty = input(unit);
    empty.anchors.clear();
    empty.result = QualifiedStyleResult::preprocessed(sass(), "", Vec::new());
    let continuation = admit(empty);
    assert_eq!(continuation.result().code(), "");
}

#[test]
fn a_result_whose_basis_is_not_the_units_content_is_refused() {
    let unit = style_unit("Comp.vue", "style:0");
    let mut stale = input(unit);
    stale.consumed_basis = ContentId::from_content_bytes(b".a { color: $old; }");
    assert_eq!(
        refusal(stale),
        StyleContinuationRefusal::StaleContentBasis,
        "bytes preprocessed from another revision do not describe this unit"
    );
}

#[test]
fn provenance_must_observe_the_style_unit_it_continues() {
    let unit = style_unit("Comp.vue", "style:0");
    let mut disowned = input(unit);
    disowned.provenance.inputs = BTreeSet::from([style_unit("Comp.vue", "script").id().clone()]);
    assert_eq!(
        refusal(disowned),
        StyleContinuationRefusal::ProvenanceMissingStyleUnit
    );
}

#[test]
fn a_diagnostic_addressing_an_uncarried_stage_is_refused() {
    let unit = style_unit("Comp.vue", "style:0");
    let mut mis_staged = input(unit);
    mis_staged.result = completed(vec![StyleDiagnostic::new(
        StyleStage::FrameworkRewritten,
        "scoped selector refused",
        Some(Span::new(0, 2)),
    )]);
    assert_eq!(
        refusal(mis_staged),
        StyleContinuationRefusal::UnqualifiedDiagnostic,
        "no rewrite has run, so a rewritten-space position resolves to nothing"
    );

    // The two spaces a continuation DOES carry are both accepted, up to and
    // including their last byte, and a positionless diagnostic is too.
    let unit = style_unit("Comp.vue", "style:0");
    let mut carried = input(unit.clone());
    carried.result = completed(vec![
        StyleDiagnostic::new(StyleStage::Authored, "in", Some(Span::new(0, 1))),
        StyleDiagnostic::new(StyleStage::Preprocessed, "out", Some(Span::new(0, 1))),
        StyleDiagnostic::new(
            StyleStage::Authored,
            "whole block",
            Some(Span::new(0, AUTHORED.len() as u32)),
        ),
        StyleDiagnostic::new(
            StyleStage::Preprocessed,
            "whole output",
            Some(Span::new(0, PRODUCED.len() as u32)),
        ),
        StyleDiagnostic::new(StyleStage::Preprocessed, "somewhere", None),
    ]);
    assert_eq!(admit(carried).diagnostics().len(), 5);

    // A carried stage's position must still resolve inside that stage's bytes.
    let unresolvable = |diagnostic: StyleDiagnostic| {
        let mut supplied = input(unit.clone());
        supplied.result = completed(vec![diagnostic]);
        refusal(supplied)
    };
    assert_eq!(
        unresolvable(StyleDiagnostic::new(
            StyleStage::Preprocessed,
            "past the output",
            Some(Span::new(0, PRODUCED.len() as u32 + 1)),
        )),
        StyleContinuationRefusal::UnqualifiedDiagnostic,
        "a produced-space position past the produced bytes addresses nothing"
    );
    assert_eq!(
        unresolvable(StyleDiagnostic::new(
            StyleStage::Authored,
            "past the block",
            Some(Span::new(0, AUTHORED.len() as u32 + 1)),
        )),
        StyleContinuationRefusal::UnqualifiedDiagnostic,
        "an authored position past the block addresses nothing"
    );
    assert_eq!(
        unresolvable(StyleDiagnostic::new(
            StyleStage::Authored,
            "carrier-absolute",
            Some(AUTHORED_EXTENT),
        )),
        StyleContinuationRefusal::UnqualifiedDiagnostic,
        "authored diagnostic spans are block-relative; a carrier-absolute one \
         is a second convention the boundary does not carry"
    );
    assert_eq!(
        unresolvable(StyleDiagnostic::new(
            StyleStage::Preprocessed,
            "inverted",
            Some(Span::new(2, 1)),
        )),
        StyleContinuationRefusal::UnqualifiedDiagnostic
    );
}

#[test]
fn overlapping_anchors_are_refused_at_admission_not_by_the_schema() {
    let unit = style_unit("Comp.vue", "style:0");
    let anchor = |generated: std::ops::Range<u32>, source: Span| ArtifactMapSegment {
        generated,
        source_unit: unit.id().clone(),
        source_span: source,
    };
    let start = AUTHORED_EXTENT.start;
    let with_anchors = |anchors: Vec<ArtifactMapSegment>| {
        let mut supplied = input(unit.clone());
        supplied.anchors = anchors;
        supplied
    };

    assert_eq!(
        refusal(with_anchors(vec![
            anchor(0..1, Span::new(start, start + 1)),
            anchor(0..1, Span::new(start + 1, start + 2)),
        ])),
        StyleContinuationRefusal::UnqualifiedMap,
        "two anchors cannot both own one generated range"
    );
    assert_eq!(
        refusal(with_anchors(vec![
            anchor(2..5, Span::new(start + 2, start + 5)),
            anchor(0..3, Span::new(start, start + 3)),
        ])),
        StyleContinuationRefusal::UnqualifiedMap,
        "overlap is judged over generated order, not supply order"
    );
    assert_eq!(
        refusal(with_anchors(vec![
            anchor(0..1, Span::new(start, start + 1)),
            anchor(0..0, Span::new(start + 1, start + 1)),
        ])),
        StyleContinuationRefusal::UnqualifiedMap,
        "an empty range sharing another anchor's start is still two claims on one position"
    );

    // Adjacent ranges are not overlapping. Admission keeps the supplied order,
    // and what it admits the schema accepts.
    let supplied = vec![
        anchor(2..4, Span::new(start + 2, start + 4)),
        anchor(0..2, Span::new(start, start + 2)),
    ];
    let continuation = admit(with_anchors(supplied.clone()));
    assert_eq!(continuation.anchors(), supplied.as_slice());
    let set = describe_style_continuations(vec![continuation], Vec::new())
        .expect("an admitted map is a valid map");
    assert_eq!(set.artifacts()[0].maps[0].segments.len(), 2);
}

#[test]
fn a_map_anchored_to_another_source_unit_is_refused() {
    let unit = style_unit("Comp.vue", "style:0");
    let sibling = style_unit("Comp.vue", "style:1");
    let mut aliased = input(unit);
    aliased.anchors[0].source_unit = sibling.id().clone();
    assert_eq!(
        refusal(aliased),
        StyleContinuationRefusal::MapSourceMismatch,
        "a continuation maps exactly one authored space: its own"
    );
}

#[test]
fn a_map_leaving_the_authored_or_produced_space_is_refused() {
    let unit = style_unit("Comp.vue", "style:0");

    let mut before = input(unit.clone());
    before.anchors[0].source_span = Span::new(AUTHORED_EXTENT.start - 1, AUTHORED_EXTENT.end);
    assert_eq!(refusal(before), StyleContinuationRefusal::UnqualifiedMap);

    let mut after = input(unit.clone());
    after.anchors[0].source_span = Span::new(AUTHORED_EXTENT.start, AUTHORED_EXTENT.end + 1);
    assert_eq!(refusal(after), StyleContinuationRefusal::UnqualifiedMap);

    let mut inverted = input(unit.clone());
    inverted.anchors[0].source_span = Span::new(AUTHORED_EXTENT.end, AUTHORED_EXTENT.start);
    assert_eq!(refusal(inverted), StyleContinuationRefusal::UnqualifiedMap);

    let mut overruns = input(unit.clone());
    overruns.anchors[0].generated = 0..PRODUCED.len() as u32 + 1;
    assert_eq!(
        refusal(overruns),
        StyleContinuationRefusal::UnqualifiedMap,
        "a generated range must address bytes the run actually produced"
    );

    let mut inverted_extent = input(unit);
    inverted_extent.authored_extent = Span::new(AUTHORED_EXTENT.end, AUTHORED_EXTENT.start);
    assert_eq!(
        refusal(inverted_extent),
        StyleContinuationRefusal::InvalidAuthoredExtent
    );
}

#[test]
fn no_anchors_means_no_map_rather_than_an_identity_one() {
    let unit = style_unit("Comp.vue", "style:0");
    let mut unmapped = input(unit);
    unmapped.anchors.clear();
    let continuation = admit(unmapped);
    assert!(continuation.anchors().is_empty());

    let set = describe_style_continuations(vec![continuation], Vec::new()).expect("valid set");
    assert!(
        set.artifacts()[0].maps.is_empty(),
        "an absent map stays absent; this boundary synthesizes no geometry"
    );

    let empty = describe_style_continuations(Vec::new(), Vec::new()).expect("valid empty set");
    assert_eq!(empty.artifacts().len(), 0);
    assert_eq!(empty.source_units().len(), 0);
}

#[test]
fn two_continuations_cannot_claim_one_style_unit() {
    let unit = style_unit("Comp.vue", "style:0");
    let one = admit(input(unit.clone()));
    let two = admit(input(unit.clone()));
    assert_eq!(
        describe_style_continuations(vec![one, two], Vec::new())
            .expect_err("a duplicate continuation is refused"),
        StyleContinuationRefusal::DuplicateContinuation
    );

    let solo = admit(input(unit.clone()));
    assert_eq!(
        describe_style_continuations(
            vec![solo],
            vec![ArtifactSourceUnit {
                unit,
                source_span: AUTHORED_EXTENT,
            }],
        )
        .expect_err("a companion may not reuse a continuation's identity"),
        StyleContinuationRefusal::SourceAliasing
    );
}

#[test]
fn a_companion_no_continuation_observes_is_refused() {
    let unit = style_unit("Comp.vue", "style:0");
    let script = style_unit("Comp.vue", "script");
    let script_extent = Span::new(0, 20);
    let companion = || ArtifactSourceUnit {
        unit: script.clone(),
        source_span: script_extent,
    };

    // Nothing in the continuation's provenance names the script unit, so the
    // schema resolves no reference to it and it would enter the set's declared
    // source space unrelated to anything in it.
    assert_eq!(
        describe_style_continuations(vec![admit(input(unit.clone()))], vec![companion()])
            .expect_err("an unobserved companion is refused"),
        StyleContinuationRefusal::UnobservedCompanion
    );

    // The same companion, once a continuation actually observes it, is the
    // case the parameter exists for: the schema needs it to resolve that
    // input, and it contributes no artifact of its own.
    let mut observes_script = input(unit.clone());
    observes_script
        .provenance
        .inputs
        .insert(script.id().clone());
    let set = describe_style_continuations(vec![admit(observes_script)], vec![companion()])
        .expect("an observed companion is declared");
    assert_eq!(
        set.artifacts().len(),
        1,
        "a companion contributes no artifact"
    );
    let declared: BTreeSet<_> = set.source_units().map(|s| s.unit.id().clone()).collect();
    assert_eq!(
        declared,
        BTreeSet::from([unit.id().clone(), script.id().clone()]),
        "both the continued style unit and its observed companion are declared"
    );

    // Without the companion the schema cannot resolve the input the
    // continuation declares, which is why the parameter is not optional.
    let mut observes_script = input(unit);
    observes_script
        .provenance
        .inputs
        .insert(script.id().clone());
    assert_eq!(
        describe_style_continuations(vec![admit(observes_script)], Vec::new())
            .expect_err("an unresolvable provenance input is refused"),
        StyleContinuationRefusal::Schema(
            verter_compiler::assembly::ArtifactSchemaError::UnknownSourceUnit
        )
    );
}

#[test]
fn sibling_style_blocks_are_companions_and_the_description_is_order_independent() {
    let first = style_unit("Comp.vue", "style:0");
    let second = style_unit("Comp.vue", "style:1");
    let foreign = style_unit("Other.vue", "style:0");

    let describe = |units: [&SourceUnit; 3]| {
        let continuations = units
            .into_iter()
            .map(|unit| admit(input(unit.clone())))
            .collect();
        describe_style_continuations(continuations, Vec::new()).expect("valid set")
    };

    let forward = describe([&first, &second, &foreign]);
    let reversed = describe([&foreign, &second, &first]);
    assert_eq!(
        forward.to_json().unwrap(),
        reversed.to_json().unwrap(),
        "supply order must not change the described set"
    );

    let artifact_for = |set: &verter_compiler::assembly::CompileArtifactSet, unit: &SourceUnit| {
        set.artifacts()
            .iter()
            .find(|a| a.source_unit() == unit.id())
            .expect("every continuation contributes an artifact")
            .clone()
    };
    let first_artifact = artifact_for(&forward, &first);
    let second_artifact = artifact_for(&forward, &second);
    let foreign_artifact = artifact_for(&forward, &foreign);

    assert_eq!(
        first_artifact.relations,
        BTreeSet::from([ArtifactRelation {
            kind: ArtifactRelationKind::CompanionOf,
            target: second_artifact.id().clone(),
        }]),
        "sibling <style> blocks of one component are companions"
    );
    assert_eq!(
        second_artifact.relations,
        BTreeSet::from([ArtifactRelation {
            kind: ArtifactRelationKind::CompanionOf,
            target: first_artifact.id().clone(),
        }])
    );
    assert!(
        foreign_artifact.relations.is_empty(),
        "a block of another source is not a companion of these"
    );
}

#[test]
fn produced_bytes_and_identity_travel_together_into_the_artifact_identity() {
    let unit = style_unit("Comp.vue", "style:0");
    let baseline = describe_style_continuations(vec![admit(input(unit.clone()))], Vec::new())
        .expect("valid set")
        .to_json()
        .unwrap();

    let mut other_bytes = input(unit.clone());
    other_bytes.anchors[0].generated = 0..1;
    other_bytes.result = QualifiedStyleResult::preprocessed(sass(), ".a{color:blue}", Vec::new());
    let changed_bytes = describe_style_continuations(vec![admit(other_bytes)], Vec::new())
        .expect("valid set")
        .to_json()
        .unwrap();
    assert_ne!(baseline, changed_bytes);

    let mut other_product = input(unit);
    other_product.product = ProductKind::RuntimeServer;
    let changed_product = describe_style_continuations(vec![admit(other_product)], Vec::new())
        .expect("valid set")
        .to_json()
        .unwrap();
    assert_ne!(
        baseline, changed_product,
        "the product a stylesheet belongs to is part of its artifact identity"
    );
}

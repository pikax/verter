use super::*;
use std::collections::BTreeSet;
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{InputBasisId, ResultContractId};
use verter_language::LanguageId;
use verter_span::Span;

static_assertions::assert_not_impl_any!(CompileArtifactSet: Default, serde::de::DeserializeOwned);
static_assertions::assert_not_impl_any!(CompileArtifact: serde::Serialize);
static_assertions::assert_not_impl_any!(ArtifactMapSegment: serde::Serialize);

struct Tag(&'static str);
impl CanonicalEncode for Tag {
    const DOMAIN_TAG: &'static str = "verter.compiler.artifact.fixture.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_str(1, self.0);
    }
}

fn source(role: &str, revision: &'static str) -> ArtifactSourceUnit {
    ArtifactSourceUnit {
        unit: SourceUnit::mint(
            SourceId::from_canonical(&Tag("Component.vue")),
            SourceRevision::from_canonical(&Tag(revision)),
            role,
            ContentId::from_content_bytes(b"hello"),
        ),
        source_span: Span::new(20, 25),
    }
}

fn artifact(unit: &SourceUnit, kind: crate::compile_request::ProductKind) -> CompileArtifact {
    CompileArtifact::new(
        unit.id().clone(),
        kind,
        LanguageId::new("typescript"),
        "main",
        ArtifactProvenance {
            input_basis: InputBasisId::from_canonical(&Tag("inputs")),
            producer: ResultContractId::from_canonical(&Tag("projection")),
            inputs: BTreeSet::from([unit.id().clone()]),
        },
        ArtifactContent::Available("hello".into()),
    )
}

#[test]
fn artifact_identity_is_stable_across_source_revisions_and_output_content() {
    use crate::compile_request::ProductKind;
    let first = source("script", "one");
    let second = source("script", "two");
    let a = artifact(&first.unit, ProductKind::IdeCompanion);
    let mut b = artifact(&second.unit, ProductKind::IdeCompanion);
    b.content = ArtifactContent::Available("different output".into());
    assert_eq!(a.id(), b.id());
    assert_ne!(
        a.id(),
        artifact(&first.unit, ProductKind::Declarations).id()
    );
    assert_ne!(
        a.id(),
        artifact(&source("template", "one").unit, ProductKind::IdeCompanion).id()
    );
    let set = CompileArtifactSet::new(vec![first], vec![a]).unwrap();
    let changed = CompileArtifactSet::new(vec![second], vec![b]).unwrap();
    assert_ne!(set.to_json().unwrap(), changed.to_json().unwrap());
}

#[test]
fn artifact_identity_separates_language_and_slot_and_serialization_preserves_each_basis() {
    let s = source("script", "one");
    let a = artifact(&s.unit, crate::compile_request::ProductKind::IdeCompanion);
    for (language, slot) in [("javascript", "main"), ("typescript", "helpers")] {
        let other = CompileArtifact::new(
            s.unit.id().clone(),
            a.product(),
            LanguageId::new(language),
            slot,
            a.provenance.clone(),
            a.content.clone(),
        );
        assert_ne!(a.id(), other.id());
    }
    let serialize = |a| {
        CompileArtifactSet::new(vec![s.clone()], vec![a])
            .unwrap()
            .to_json()
            .unwrap()
    };
    let original = serialize(a.clone());
    let wire: serde_json::Value = serde_json::from_str(&original).unwrap();
    assert_eq!(
        wire["artifacts"][0]["provenance"]["inputBasis"],
        hex::encode(a.provenance.input_basis.canonical_bytes())
    );
    assert_eq!(
        wire["artifacts"][0]["provenance"]["producer"],
        hex::encode(a.provenance.producer.canonical_bytes())
    );
    assert_eq!(
        wire["sourceUnits"][0]["content"],
        hex::encode(s.unit.content().canonical_bytes())
    );
    let mut changed_basis = a.clone();
    changed_basis.provenance.input_basis = InputBasisId::from_canonical(&Tag("other inputs"));
    let mut changed_producer = a.clone();
    changed_producer.provenance.producer = ResultContractId::from_canonical(&Tag("other producer"));
    assert_ne!(original, serialize(changed_basis));
    assert_ne!(original, serialize(changed_producer));
    let id = a.id().clone();
    let set = CompileArtifactSet::new(vec![s], vec![a]).unwrap();
    assert_eq!(set.artifact(&id).unwrap().id(), &id);
    assert_eq!(set.source_units().len(), 1);
    assert_eq!(set.artifacts().len(), 1);
    assert_eq!(
        CompileArtifactSet::new(vec![], vec![])
            .unwrap()
            .to_json()
            .unwrap(),
        r#"{"artifacts":[],"coordinates":"utf8-bytes","customBlocks":[],"schemaVersion":1,"sourceUnits":[]}"#
    );
}

fn mapped_artifact(unit: &SourceUnit) -> CompileArtifact {
    let mut a = artifact(unit, crate::compile_request::ProductKind::IdeCompanion);
    a.maps.push(QualifiedArtifactMap {
        family: ArtifactMapFamily::SourceProjection,
        generated: a.id().clone(),
        generated_content: ContentId::from_content_bytes(b"hello"),
        input_basis: a.provenance.input_basis.clone(),
        sources: BTreeSet::from([unit.id().clone()]),
        segments: vec![ArtifactMapSegment {
            generated: 0..5,
            source_unit: unit.id().clone(),
            source_span: Span::new(20, 25),
        }],
    });
    a
}

#[test]
fn terminal_order_is_independent_of_artifact_source_map_and_relation_insertion() {
    use crate::compile_request::ProductKind;
    let script = source("script", "one");
    let template = source("template", "one");
    let runtime = artifact(&script.unit, ProductKind::RuntimeClient);
    let declarations = artifact(&script.unit, ProductKind::Declarations);
    let mut ide = mapped_artifact(&script.unit);
    ide.provenance.inputs.insert(template.unit.id().clone());
    ide.relations = BTreeSet::from([
        ArtifactRelation {
            kind: ArtifactRelationKind::CompanionOf,
            target: runtime.id().clone(),
        },
        ArtifactRelation {
            kind: ArtifactRelationKind::DependsOn,
            target: declarations.id().clone(),
        },
    ]);
    ide.maps[0].sources.insert(template.unit.id().clone());
    ide.maps[0].segments[0].generated = 0..2;
    ide.maps[0].segments[0].source_span = Span::new(20, 22);
    ide.maps[0].segments.push(ArtifactMapSegment {
        generated: 2..5,
        source_unit: template.unit.id().clone(),
        source_span: Span::new(22, 25),
    });
    let mut runtime = runtime;
    runtime.maps.push(QualifiedArtifactMap {
        family: ArtifactMapFamily::RuntimeSourceMap,
        generated: runtime.id().clone(),
        generated_content: ContentId::from_content_bytes(b"hello"),
        input_basis: runtime.provenance.input_basis.clone(),
        sources: BTreeSet::from([script.unit.id().clone()]),
        segments: vec![],
    });
    let mut runtime_projection = runtime.maps[0].clone();
    runtime_projection.family = ArtifactMapFamily::SourceProjection;
    runtime.maps.push(runtime_projection);
    let expected = CompileArtifactSet::new(
        vec![script.clone(), template.clone()],
        vec![ide.clone(), runtime.clone(), declarations.clone()],
    )
    .unwrap()
    .to_json()
    .unwrap();
    ide.maps[0].segments.reverse();
    runtime.maps.reverse();
    let actual =
        CompileArtifactSet::new(vec![template, script], vec![declarations, runtime, ide]).unwrap();
    assert_eq!(expected, actual.to_json().unwrap());
    let wire: serde_json::Value = serde_json::from_str(&expected).unwrap();
    assert_eq!(wire["schemaVersion"], 1);
    assert_eq!(wire["coordinates"], "utf8-bytes");
    let ide_wire = wire["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["product"] == "ideCompanion")
        .unwrap();
    assert_eq!(
        ide_wire["provenance"]["inputs"].as_array().unwrap().len(),
        2
    );
    assert_eq!(ide_wire["relations"].as_array().unwrap().len(), 2);
    assert_eq!(ide_wire["maps"][0]["family"], "sourceProjection");
    assert_eq!(ide_wire["maps"][0]["generated"], ide_wire["id"]);
    assert_eq!(
        ide_wire["maps"][0]["segments"][0]["sourceBytes"],
        serde_json::json!({"start": 20, "end": 22})
    );
    let runtime_wire = wire["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["product"] == "runtimeClient")
        .unwrap();
    assert_eq!(runtime_wire["maps"][1]["family"], "runtimeSourceMap");
}

#[test]
fn artifact_set_rejects_aliasing_and_incomplete_relations_or_provenance() {
    use ArtifactSchemaError as E;
    let s = source("script", "one");
    let a = mapped_artifact(&s.unit);
    assert_eq!(
        CompileArtifactSet::new(vec![s.clone()], vec![a.clone(), a.clone()]).unwrap_err(),
        E::DuplicateArtifact
    );
    assert_eq!(
        CompileArtifactSet::new(vec![s.clone(), s.clone()], vec![a.clone()]).unwrap_err(),
        E::DuplicateSourceUnit
    );
    assert_eq!(
        CompileArtifactSet::new(vec![s.clone(), source("template", "two")], vec![a.clone()])
            .unwrap_err(),
        E::ConflictingSourceRevision
    );
    assert_eq!(
        CompileArtifactSet::new(vec![], vec![a.clone()]).unwrap_err(),
        E::UnknownSourceUnit
    );
    let foreign = source("foreign", "one");
    let foreign_id = artifact(
        &foreign.unit,
        crate::compile_request::ProductKind::RuntimeClient,
    )
    .id()
    .clone();
    let mut missing_input = a.clone();
    missing_input.provenance.inputs.clear();
    let mut foreign_input = a.clone();
    foreign_input
        .provenance
        .inputs
        .insert(foreign.unit.id().clone());
    let mut missing_relation = a.clone();
    missing_relation.relations.insert(ArtifactRelation {
        kind: ArtifactRelationKind::DependsOn,
        target: foreign_id,
    });
    let mut self_relation = a.clone();
    self_relation.relations.insert(ArtifactRelation {
        kind: ArtifactRelationKind::CompanionOf,
        target: a.id().clone(),
    });
    for (candidate, error) in [
        (missing_input, E::MissingPrimaryInput),
        (foreign_input, E::UnknownSourceUnit),
        (missing_relation, E::MissingRelationTarget),
        (self_relation, E::SelfRelation),
    ] {
        assert_eq!(
            CompileArtifactSet::new(vec![s.clone()], vec![candidate]).unwrap_err(),
            error
        );
    }
    let mut invalid_source = s;
    invalid_source.source_span = Span::new(25, 20);
    assert_eq!(
        CompileArtifactSet::new(vec![invalid_source], vec![a]).unwrap_err(),
        E::InvalidSourceSpan
    );
}

#[test]
fn qualified_maps_fail_closed_on_foreign_missing_and_ambiguous_spaces() {
    use ArtifactSchemaError as E;
    let s = source("script", "one");
    let foreign = source("template", "one");
    let a = mapped_artifact(&s.unit);
    let mut duplicate = a.clone();
    duplicate.maps.push(duplicate.maps[0].clone());
    let mut unqualified = a.clone();
    unqualified.maps[0].sources.clear();
    unqualified.maps[0].segments.clear();
    let mut destination = a.clone();
    destination.maps[0].generated = artifact(
        &foreign.unit,
        crate::compile_request::ProductKind::IdeCompanion,
    )
    .id()
    .clone();
    let mut undeclared = a.clone();
    undeclared.maps[0].segments[0].source_unit = foreign.unit.id().clone();
    let mut outside_inputs = a.clone();
    outside_inputs.maps[0]
        .sources
        .insert(foreign.unit.id().clone());
    let mut relative = a.clone();
    relative.maps[0].segments[0].source_span = Span::new(0, 5);
    let mut past_source = a.clone();
    past_source.maps[0].segments[0].source_span.end = 26;
    let mut reversed_source = a.clone();
    reversed_source.maps[0].segments[0].source_span = Span::new(24, 22);
    let mut past_generated = a.clone();
    past_generated.maps[0].segments[0].generated = 0..6;
    let mut overlap = a.clone();
    let duplicate_segment = overlap.maps[0].segments[0].clone();
    overlap.maps[0].segments.push(duplicate_segment);
    let mut unavailable = a;
    unavailable.content = ArtifactContent::Unavailable(ArtifactUnavailableReason::InvalidInput);
    for (candidate, error) in [
        (duplicate, E::DuplicateMapFamily),
        (unqualified, E::UnqualifiedMap),
        (destination, E::WrongGeneratedSpace),
        (undeclared, E::UnqualifiedMap),
        (outside_inputs, E::MapSourceOutsideProvenance),
        (relative, E::InvalidSourceRange),
        (past_source, E::InvalidSourceRange),
        (reversed_source, E::InvalidSourceRange),
        (past_generated, E::InvalidGeneratedRange),
        (overlap, E::OverlappingMappings),
        (unavailable, E::UnavailableMappedContent),
    ] {
        assert_eq!(
            CompileArtifactSet::new(vec![s.clone(), foreign.clone()], vec![candidate]).unwrap_err(),
            error
        );
    }
}

#[test]
fn unicode_maps_keep_absolute_source_bytes_and_separate_generated_bytes() {
    let s = source("script", "one");
    let mut a = mapped_artifact(&s.unit);
    a.content = ArtifactContent::Available("😀x".into());
    a.maps[0].generated_content = ContentId::from_content_bytes("😀x".as_bytes());
    a.maps[0].segments[0].generated = 0..4;
    a.maps[0].segments[0].source_span = Span::new(20, 24);
    let wire: serde_json::Value = serde_json::from_str(
        &CompileArtifactSet::new(vec![s.clone()], vec![a.clone()])
            .unwrap()
            .to_json()
            .unwrap(),
    )
    .unwrap();
    let segment = &wire["artifacts"][0]["maps"][0]["segments"][0];
    assert_eq!(
        segment["generatedBytes"],
        serde_json::json!({"start": 0, "end": 4})
    );
    assert_eq!(
        segment["sourceBytes"],
        serde_json::json!({"start": 20, "end": 24})
    );
    a.maps[0].segments[0].generated.end = 2;
    assert_eq!(
        CompileArtifactSet::new(vec![s], vec![a]).unwrap_err(),
        ArtifactSchemaError::InvalidGeneratedRange
    );
}

#[test]
fn qualified_maps_reject_stale_output_bytes_and_input_basis_without_renaming_artifacts() {
    let s = source("script", "one");
    let a = mapped_artifact(&s.unit);
    let mut changed_output = a.clone();
    changed_output.content = ArtifactContent::Available("world".into());
    assert_eq!(changed_output.id(), a.id());
    assert_eq!(
        CompileArtifactSet::new(vec![s.clone()], vec![changed_output]).unwrap_err(),
        ArtifactSchemaError::StaleGeneratedContent
    );
    let mut changed_inputs = a;
    changed_inputs.provenance.input_basis = InputBasisId::from_canonical(&Tag("changed inputs"));
    assert_eq!(
        CompileArtifactSet::new(vec![s], vec![changed_inputs]).unwrap_err(),
        ArtifactSchemaError::StaleMapInputBasis
    );
}

#[test]
fn schema_construction_preserves_unavailable_and_opaque_content_without_compile_work() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use verter_audit::{AuditEvent, AuditObserver};
    struct Observer(AtomicUsize);
    impl AuditObserver for Observer {
        fn record_event(&self, event: AuditEvent) {
            if event == AuditEvent::CompileCodeTransformOp {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
    let observer = Arc::new(Observer(AtomicUsize::new(0)));
    let _guard = verter_audit::observer::install_observer(observer.clone());
    let s = source("script", "one");
    let mut a = artifact(&s.unit, crate::compile_request::ProductKind::RuntimeClient);
    a.content = ArtifactContent::Available("this is opaque, not parseable TypeScript {{{".into());
    let available = CompileArtifactSet::new(vec![s.clone()], vec![a.clone()])
        .unwrap()
        .to_json()
        .unwrap();
    a.content = ArtifactContent::Unavailable(ArtifactUnavailableReason::Unsupported);
    let unavailable = CompileArtifactSet::new(vec![s], vec![a])
        .unwrap()
        .to_json()
        .unwrap();
    assert!(available.contains("not parseable TypeScript"));
    assert!(unavailable.contains("unsupported"));
    assert_ne!(available, unavailable);
    assert_eq!(observer.0.load(Ordering::SeqCst), 0);
    // The observer sees real transform work, so zero above discriminates it.
    let allocator = oxc_allocator::Allocator::default();
    let mut transform = crate::code_transform::CodeTransform::new("x", &allocator);
    transform.overwrite(0, 1, "y");
    assert!(observer.0.load(Ordering::SeqCst) > 0);
}

/// Staging binds a root the set actually contains. A handoff whose root is
/// absent, or whose root produced no content, refuses — so every later read
/// of the staged module's bytes, language and map is answering about an
/// artifact the compiler really published.
#[test]
fn staging_refuses_an_absent_or_unproduced_root() {
    use crate::compile_request::ProductKind;
    let unit = source("main", "one");
    let main = artifact(&unit.unit, ProductKind::RuntimeClient);
    let root = main.id().clone();

    let set = CompileArtifactSet::new(vec![unit.clone()], vec![main.clone()]).unwrap();
    let staged = StagedCompileArtifacts::stage(
        set,
        root.clone(),
        FragmentDialect::TypeScript,
        Some("{\"version\":3}".to_string()),
    )
    .expect("a produced root stages");
    assert_eq!(staged.root().id(), &root);
    assert_eq!(&**staged.code(), "hello");
    assert_eq!(staged.lang(), "ts");
    assert_eq!(staged.product(), ProductKind::RuntimeClient);
    assert_eq!(staged.source_map(), Some("{\"version\":3}"));
    assert_eq!(staged.set().artifacts().len(), 1);

    let foreign = artifact(&source("other", "two").unit, ProductKind::Declarations);
    let set = CompileArtifactSet::new(vec![unit.clone()], vec![main.clone()]).unwrap();
    assert_eq!(
        StagedCompileArtifacts::stage(
            set,
            foreign.id().clone(),
            FragmentDialect::TypeScript,
            None,
        )
        .unwrap_err(),
        ArtifactSchemaError::UnknownRootArtifact
    );

    let mut unproduced = main;
    unproduced.content = ArtifactContent::Unavailable(ArtifactUnavailableReason::NotProduced);
    let set = CompileArtifactSet::new(vec![unit], vec![unproduced]).unwrap();
    assert_eq!(
        StagedCompileArtifacts::stage(set, root, FragmentDialect::TypeScript, None).unwrap_err(),
        ArtifactSchemaError::UnavailableRootArtifact
    );
}

/// An empty map payload stages as "no map", not as a produced-but-empty
/// one: downstream carries the map demand by presence alone, so an empty
/// string would publish a map node for a map-disabled compile.
#[test]
fn staging_normalizes_an_empty_source_map_to_absent() {
    use crate::compile_request::ProductKind;
    let unit = source("main", "one");
    let main = artifact(&unit.unit, ProductKind::RuntimeClient);
    let root = main.id().clone();
    let set = CompileArtifactSet::new(vec![unit], vec![main]).unwrap();
    let staged =
        StagedCompileArtifacts::stage(set, root, FragmentDialect::JavaScript, Some(String::new()))
            .expect("a produced root stages");
    assert_eq!(staged.source_map(), None);
    assert_eq!(staged.lang(), "js");
}

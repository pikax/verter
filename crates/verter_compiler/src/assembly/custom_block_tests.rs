use super::*;
use std::collections::BTreeSet;
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{InputBasisId, ResultContractId};
use verter_language::LanguageId;
use verter_span::Span;

static_assertions::assert_not_impl_any!(CustomBlockDescriptor: Default, serde::de::DeserializeOwned);
static_assertions::assert_not_impl_any!(CustomBlockDescriptorId: Default);

struct Tag(&'static str);
impl CanonicalEncode for Tag {
    const DOMAIN_TAG: &'static str = "verter.compiler.custom_block.fixture.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_str(1, self.0);
    }
}

fn source_named(tag: &'static str, role: &str, revision: &'static str) -> ArtifactSourceUnit {
    ArtifactSourceUnit {
        unit: SourceUnit::mint(
            SourceId::from_canonical(&Tag(tag)),
            SourceRevision::from_canonical(&Tag(revision)),
            role,
            ContentId::from_content_bytes(b"hello"),
        ),
        source_span: Span::new(20, 40),
    }
}

fn source(role: &str, revision: &'static str) -> ArtifactSourceUnit {
    source_named("Component.vue", role, revision)
}

fn artifact(unit: &SourceUnit) -> CompileArtifact {
    CompileArtifact::new(
        unit.id().clone(),
        crate::compile_request::ProductKind::RuntimeClient,
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

#[allow(clippy::too_many_arguments)]
fn request(
    unit: &ArtifactSourceUnit,
    attached_to: ArtifactId,
    role: &str,
    order: u32,
    region: Span,
    content: CustomBlockContent,
    src: Option<&str>,
    lang: Option<&str>,
    attributes: Vec<(String, String)>,
    lifecycle: CustomBlockLifecycle,
) -> CustomBlockDescriptorRequest {
    CustomBlockDescriptorRequest {
        source_unit: unit.unit.id().clone(),
        source_id: unit.unit.source_id().clone(),
        revision: unit.unit.revision().clone(),
        source_content: unit.unit.content().clone(),
        role: role.into(),
        lang: lang.map(str::to_string),
        src: src.map(str::to_string),
        attributes,
        source_order: order,
        region,
        content,
        provenance: ArtifactProvenance {
            input_basis: InputBasisId::from_canonical(&Tag("inputs")),
            producer: ResultContractId::from_canonical(&Tag("custom-block")),
            inputs: BTreeSet::from([unit.unit.id().clone()]),
        },
        attached_to,
        lifecycle,
    }
}

fn local_text(text: &str) -> CustomBlockContent {
    CustomBlockContent::Local {
        content: ContentId::from_content_bytes(text.as_bytes()),
        text: text.into(),
    }
}

#[allow(clippy::too_many_arguments)]
fn complete(
    unit: &ArtifactSourceUnit,
    attached_to: ArtifactId,
    role: &str,
    order: u32,
    region: Span,
    content: CustomBlockContent,
    src: Option<&str>,
    lang: Option<&str>,
    attributes: Vec<(String, String)>,
) -> CustomBlockDescriptor {
    CustomBlockDescriptor::try_new(request(
        unit,
        attached_to,
        role,
        order,
        region,
        content,
        src,
        lang,
        attributes,
        CustomBlockLifecycle::Complete,
    ))
    .unwrap()
}

fn set_for(unit: ArtifactSourceUnit) -> (CompileArtifact, CompileArtifactSet) {
    let a = artifact(&unit.unit);
    let set = CompileArtifactSet::new(vec![unit], vec![a.clone()]).unwrap();
    (a, set)
}

#[test]
fn local_src_backed_and_empty_round_trip_named_fields_without_copying_external_content() {
    let unit = source("custom:i18n", "one");
    let (artifact, set) = set_for(unit.clone());
    let local = complete(
        &unit,
        artifact.id().clone(),
        "i18n",
        0,
        Span::new(20, 27),
        local_text("{\"a\":1}"),
        None,
        Some("json"),
        vec![("z".into(), "1".into()), ("lang".into(), "json".into())],
    );
    let src_backed = complete(
        &unit,
        artifact.id().clone(),
        "docs",
        1,
        Span::new(27, 31),
        CustomBlockContent::SrcBacked,
        Some("./docs.md"),
        None,
        vec![("src".into(), "./docs.md".into())],
    );
    let empty = complete(
        &unit,
        artifact.id().clone(),
        "note",
        2,
        Span::new(31, 31),
        CustomBlockContent::Empty,
        None,
        None,
        vec![],
    );
    let unavailable = complete(
        &unit,
        artifact.id().clone(),
        "unknown",
        3,
        Span::new(32, 40),
        CustomBlockContent::Unavailable(ArtifactUnavailableReason::Unsupported),
        None,
        None,
        vec![],
    );
    match src_backed.content() {
        CustomBlockContent::SrcBacked => {}
        other => panic!("src-backed stored content: {other:?}"),
    }
    match unavailable.content() {
        CustomBlockContent::Unavailable(ArtifactUnavailableReason::Unsupported) => {}
        other => panic!("unavailable stored content: {other:?}"),
    }
    let attached = set
        .attach_custom_blocks(vec![
            unavailable.clone(),
            empty.clone(),
            src_backed.clone(),
            local.clone(),
        ])
        .unwrap();
    attached.warm_custom_block(&local).unwrap();
    attached.warm_custom_block(&src_backed).unwrap();
    attached.warm_custom_block(&empty).unwrap();
    let wire: serde_json::Value = serde_json::from_str(&attached.to_json().unwrap()).unwrap();
    let blocks = wire["customBlocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 4);
    assert_eq!(blocks[0]["role"], "i18n");
    assert_eq!(blocks[0]["sourceOrder"], 0);
    assert_eq!(blocks[0]["lang"], "json");
    assert_eq!(blocks[0]["content"]["availability"], "local");
    assert_eq!(blocks[0]["content"]["text"], "{\"a\":1}");
    assert_eq!(
        local.attributes(),
        &[
            ("z".to_string(), "1".to_string()),
            ("lang".to_string(), "json".to_string())
        ]
    );
    assert_eq!(local.source_content(), unit.unit.content());
    assert_eq!(local.lifecycle(), CustomBlockLifecycle::Complete);
    assert_eq!(
        blocks[0]["region"],
        serde_json::json!({"start": 20, "end": 27})
    );
    assert_eq!(
        blocks[0]["attributes"],
        serde_json::json!([{"name": "z", "value": "1"}, {"name": "lang", "value": "json"}])
    );
    assert_eq!(
        blocks[0]["sourceContent"],
        hex::encode(unit.unit.content().canonical_bytes())
    );
    assert_eq!(blocks[0]["lifecycle"], "complete");
    assert_eq!(
        blocks[0]["provenance"]["inputBasis"],
        hex::encode(local.provenance().input_basis.canonical_bytes())
    );
    assert_eq!(
        blocks[0]["provenance"]["producer"],
        hex::encode(local.provenance().producer.canonical_bytes())
    );
    assert_eq!(
        blocks[0]["provenance"]["inputs"],
        serde_json::json!([hex::encode(unit.unit.id().canonical_bytes())])
    );
    assert_eq!(
        blocks[0]["sourceUnit"],
        hex::encode(unit.unit.id().canonical_bytes())
    );
    assert_eq!(
        blocks[0]["source"],
        hex::encode(unit.unit.source_id().canonical_bytes())
    );
    assert_eq!(
        blocks[0]["revision"],
        hex::encode(unit.unit.revision().canonical_bytes())
    );
    assert_eq!(
        blocks[0]["relation"]["target"],
        hex::encode(artifact.id().canonical_bytes())
    );
    assert_eq!(blocks[0]["relation"]["kind"], "attachedTo");
    assert_eq!(blocks[1]["content"]["availability"], "srcBacked");
    assert!(blocks[1]["content"].get("text").is_none());
    assert_eq!(blocks[1]["src"], "./docs.md");
    assert_eq!(blocks[2]["content"]["availability"], "empty");
    assert_eq!(blocks[3]["content"]["availability"], "unavailable");
    assert_eq!(blocks[3]["content"]["reason"], "unsupported");
    assert_eq!(
        attached.custom_blocks()[0].id(),
        local.id(),
        "deterministic order is source_order, not insertion"
    );
}

#[test]
fn identity_binds_unit_revision_role_order_region_attributes_lang_src_and_content() {
    let unit = source("custom:i18n", "one");
    let (artifact, _) = set_for(unit.clone());
    let base = complete(
        &unit,
        artifact.id().clone(),
        "i18n",
        0,
        Span::new(20, 24),
        local_text("body"),
        None,
        Some("json"),
        vec![("lang".into(), "json".into())],
    );
    let mut other_role = request(
        &unit,
        artifact.id().clone(),
        "docs",
        0,
        Span::new(20, 24),
        local_text("body"),
        None,
        Some("json"),
        vec![("lang".into(), "json".into())],
        CustomBlockLifecycle::Complete,
    );
    assert_ne!(
        CustomBlockDescriptor::try_new(other_role.clone())
            .unwrap()
            .id(),
        base.id()
    );
    other_role.role = "i18n".into();
    other_role.source_order = 1;
    assert_ne!(
        CustomBlockDescriptor::try_new(other_role.clone())
            .unwrap()
            .id(),
        base.id()
    );
    other_role.source_order = 0;
    other_role.region = Span::new(21, 25);
    assert_ne!(
        CustomBlockDescriptor::try_new(other_role.clone())
            .unwrap()
            .id(),
        base.id()
    );
    other_role.region = Span::new(20, 24);
    other_role.lang = Some("yaml".into());
    other_role.attributes = vec![("lang".into(), "yaml".into())];
    assert_ne!(
        CustomBlockDescriptor::try_new(other_role.clone())
            .unwrap()
            .id(),
        base.id()
    );
    other_role.lang = Some("json".into());
    other_role.attributes = vec![("foo".into(), "bar".into()), ("lang".into(), "json".into())];
    assert_ne!(
        CustomBlockDescriptor::try_new(other_role.clone())
            .unwrap()
            .id(),
        base.id()
    );
    other_role.attributes = vec![("lang".into(), "json".into())];
    other_role.content = CustomBlockContent::Empty;
    assert_ne!(
        CustomBlockDescriptor::try_new(other_role).unwrap().id(),
        base.id()
    );
    let other_rev = source("custom:i18n", "two");
    let other = complete(
        &other_rev,
        artifact.id().clone(),
        "i18n",
        0,
        Span::new(20, 24),
        local_text("body"),
        None,
        Some("json"),
        vec![("lang".into(), "json".into())],
    );
    assert_ne!(other.id(), base.id());
}

#[test]
fn malformed_alias_order_and_region_fail_closed() {
    use CustomBlockDescriptorError as E;
    let unit = source("custom:i18n", "one");
    let (host, set) = set_for(unit.clone());
    let id = host.id().clone();
    assert_eq!(
        CustomBlockDescriptor::try_new(request(
            &unit,
            id.clone(),
            "",
            0,
            Span::new(20, 30),
            CustomBlockContent::Empty,
            None,
            None,
            vec![],
            CustomBlockLifecycle::Complete,
        ))
        .unwrap_err(),
        E::Malformed
    );
    assert_eq!(
        CustomBlockDescriptor::try_new(request(
            &unit,
            id.clone(),
            "i18n",
            0,
            Span::new(30, 20),
            CustomBlockContent::Empty,
            None,
            None,
            vec![],
            CustomBlockLifecycle::Complete,
        ))
        .unwrap_err(),
        E::Malformed
    );
    assert_eq!(
        CustomBlockDescriptor::try_new(request(
            &unit,
            id.clone(),
            "i18n",
            0,
            Span::new(20, 30),
            CustomBlockContent::Empty,
            None,
            Some("json"),
            vec![
                ("lang".into(), "json".into()),
                ("lang".into(), "yaml".into())
            ],
            CustomBlockLifecycle::Complete,
        ))
        .unwrap_err(),
        E::AliasedAttribute
    );
    assert_eq!(
        CustomBlockDescriptor::try_new(request(
            &unit,
            id.clone(),
            "i18n",
            0,
            Span::new(20, 30),
            CustomBlockContent::Empty,
            None,
            Some("json"),
            vec![("lang".into(), "yaml".into())],
            CustomBlockLifecycle::Complete,
        ))
        .unwrap_err(),
        E::AliasedAttribute
    );
    assert_eq!(
        CustomBlockDescriptor::try_new(request(
            &unit,
            id.clone(),
            "i18n",
            0,
            Span::new(20, 24),
            local_text("body"),
            Some("./x"),
            None,
            vec![("src".into(), "./x".into())],
            CustomBlockLifecycle::Complete,
        ))
        .unwrap_err(),
        E::Malformed
    );
    assert_eq!(
        CustomBlockDescriptor::try_new(request(
            &unit,
            id.clone(),
            "i18n",
            0,
            Span::new(20, 24),
            local_text("body"),
            None,
            None,
            vec![("src".into(), "./docs.md".into())],
            CustomBlockLifecycle::Complete,
        ))
        .unwrap_err(),
        E::AliasedAttribute
    );
    assert_eq!(
        CustomBlockDescriptor::try_new(request(
            &unit,
            id.clone(),
            "i18n",
            0,
            Span::new(20, 30),
            CustomBlockContent::Empty,
            None,
            Some("json"),
            vec![],
            CustomBlockLifecycle::Complete,
        ))
        .unwrap_err(),
        E::AliasedAttribute
    );
    assert_eq!(
        CustomBlockDescriptor::try_new(request(
            &unit,
            id.clone(),
            "i18n",
            0,
            Span::new(20, 24),
            local_text("{\"a\":1}"),
            None,
            None,
            vec![],
            CustomBlockLifecycle::Complete,
        ))
        .unwrap_err(),
        E::InvalidRegion
    );
    let first = complete(
        &unit,
        id.clone(),
        "i18n",
        0,
        Span::new(20, 21),
        local_text("a"),
        None,
        None,
        vec![],
    );
    let duplicate_order = complete(
        &unit,
        id.clone(),
        "docs",
        0,
        Span::new(30, 40),
        CustomBlockContent::Empty,
        None,
        None,
        vec![],
    );
    assert_eq!(
        set.clone()
            .attach_custom_blocks(vec![first.clone(), duplicate_order])
            .unwrap_err(),
        E::DuplicateOrder
    );
    let outside = CustomBlockDescriptor::try_new(request(
        &unit,
        id.clone(),
        "docs",
        1,
        Span::new(10, 25),
        CustomBlockContent::Empty,
        None,
        None,
        vec![],
        CustomBlockLifecycle::Complete,
    ))
    .unwrap();
    assert_eq!(
        set.clone().attach_custom_blocks(vec![outside]).unwrap_err(),
        E::InvalidRegion
    );
    let overlap_left = complete(
        &unit,
        id.clone(),
        "i18n",
        0,
        Span::new(20, 30),
        CustomBlockContent::Empty,
        None,
        None,
        vec![],
    );
    let overlap = CustomBlockDescriptor::try_new(request(
        &unit,
        id.clone(),
        "docs",
        1,
        Span::new(25, 35),
        CustomBlockContent::Empty,
        None,
        None,
        vec![],
        CustomBlockLifecycle::Complete,
    ))
    .unwrap();
    assert_eq!(
        set.clone()
            .attach_custom_blocks(vec![overlap_left, overlap])
            .unwrap_err(),
        E::InvalidRegion
    );
    let reversed_later = complete(
        &unit,
        id.clone(),
        "i18n",
        0,
        Span::new(30, 34),
        CustomBlockContent::Empty,
        None,
        None,
        vec![],
    );
    let reversed_earlier = complete(
        &unit,
        id.clone(),
        "docs",
        1,
        Span::new(20, 24),
        CustomBlockContent::Empty,
        None,
        None,
        vec![],
    );
    assert_eq!(
        set.clone()
            .attach_custom_blocks(vec![reversed_later, reversed_earlier])
            .unwrap_err(),
        E::InvalidOrder
    );
    let second = complete(
        &unit,
        id.clone(),
        "docs",
        1,
        Span::new(30, 40),
        CustomBlockContent::Empty,
        None,
        None,
        vec![],
    );
    let sequential = set
        .clone()
        .attach_custom_blocks(vec![first.clone()])
        .unwrap()
        .attach_custom_blocks(vec![second.clone()])
        .unwrap();
    assert_eq!(sequential.custom_blocks().len(), 2);
    assert_eq!(sequential.custom_blocks()[0].id(), first.id());
    assert_eq!(sequential.custom_blocks()[1].id(), second.id());
    let after_empty = sequential.attach_custom_blocks(vec![]).unwrap();
    assert_eq!(after_empty.custom_blocks().len(), 2);

    let other_unit = source_named("Other.vue", "custom:i18n", "one");
    let other_artifact = artifact(&other_unit.unit);
    let multi_source = CompileArtifactSet::new(
        vec![unit.clone(), other_unit.clone()],
        vec![host.clone(), other_artifact.clone()],
    )
    .unwrap();
    let left = complete(
        &unit,
        id.clone(),
        "i18n",
        0,
        Span::new(20, 24),
        CustomBlockContent::Empty,
        None,
        None,
        vec![],
    );
    let right = complete(
        &other_unit,
        other_artifact.id().clone(),
        "i18n",
        0,
        Span::new(20, 24),
        CustomBlockContent::Empty,
        None,
        None,
        vec![],
    );
    let attached = multi_source
        .attach_custom_blocks(vec![left, right])
        .unwrap();
    assert_eq!(attached.custom_blocks().len(), 2);

    let sibling = source("custom:docs", "one");
    let sibling_artifact = artifact(&sibling.unit);
    let same_source = CompileArtifactSet::new(
        vec![unit.clone(), sibling.clone()],
        vec![host.clone(), sibling_artifact.clone()],
    )
    .unwrap();
    let unit_block = complete(
        &unit,
        id.clone(),
        "i18n",
        0,
        Span::new(20, 30),
        CustomBlockContent::Empty,
        None,
        None,
        vec![],
    );
    let sibling_overlap = complete(
        &sibling,
        sibling_artifact.id().clone(),
        "docs",
        1,
        Span::new(25, 35),
        CustomBlockContent::Empty,
        None,
        None,
        vec![],
    );
    assert_eq!(
        same_source
            .attach_custom_blocks(vec![unit_block, sibling_overlap])
            .unwrap_err(),
        E::InvalidRegion
    );
}

#[test]
fn stale_cancelled_partial_and_source_mismatch_cannot_publish_or_warm() {
    use CustomBlockDescriptorError as E;
    let unit = source("custom:i18n", "one");
    let (artifact, set) = set_for(unit.clone());
    let id = artifact.id().clone();
    let complete_ok = complete(
        &unit,
        id.clone(),
        "i18n",
        0,
        Span::new(20, 24),
        local_text("body"),
        None,
        None,
        vec![],
    );
    for (lifecycle, error) in [
        (CustomBlockLifecycle::Stale, E::Stale),
        (CustomBlockLifecycle::Cancelled, E::Cancelled),
        (CustomBlockLifecycle::Partial, E::Partial),
    ] {
        let degraded = CustomBlockDescriptor::try_new(request(
            &unit,
            id.clone(),
            "i18n",
            0,
            Span::new(20, 24),
            local_text("body"),
            None,
            None,
            vec![],
            lifecycle,
        ))
        .unwrap();
        assert_eq!(
            set.clone()
                .attach_custom_blocks(vec![degraded.clone()])
                .unwrap_err(),
            error
        );
        assert_eq!(set.warm_custom_block(&degraded).unwrap_err(), error);
    }
    let stale_revision = source("custom:i18n", "two");
    let mismatched_rev = CustomBlockDescriptor::try_new(request(
        &stale_revision,
        id.clone(),
        "i18n",
        0,
        Span::new(20, 24),
        local_text("body"),
        None,
        None,
        vec![],
        CustomBlockLifecycle::Complete,
    ))
    .unwrap();
    assert_eq!(
        set.clone()
            .attach_custom_blocks(vec![mismatched_rev.clone()])
            .unwrap_err(),
        E::Stale
    );
    assert_eq!(
        set.warm_custom_block(&mismatched_rev).unwrap_err(),
        E::Stale
    );
    let mut mismatched_source = request(
        &unit,
        id,
        "i18n",
        0,
        Span::new(20, 24),
        local_text("body"),
        None,
        None,
        vec![],
        CustomBlockLifecycle::Complete,
    );
    mismatched_source.source_content = ContentId::from_content_bytes(b"other");
    let mismatched_source = CustomBlockDescriptor::try_new(mismatched_source).unwrap();
    assert_eq!(
        set.clone()
            .attach_custom_blocks(vec![mismatched_source.clone()])
            .unwrap_err(),
        E::SourceMismatch
    );
    assert_eq!(
        set.warm_custom_block(&mismatched_source).unwrap_err(),
        E::SourceMismatch
    );
    set.clone()
        .attach_custom_blocks(vec![complete_ok.clone()])
        .unwrap()
        .warm_custom_block(&complete_ok)
        .unwrap();
}

#[test]
fn construction_is_metadata_only_and_unknown_absent_cells_do_zero_work() {
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
    let unit = source("custom:i18n", "one");
    let (artifact, set) = set_for(unit.clone());
    let opaque = complete(
        &unit,
        artifact.id().clone(),
        "i18n",
        0,
        Span::new(20, 38),
        local_text("not executable {{{"),
        None,
        None,
        vec![],
    );
    let absent = CompileArtifactSet::new(vec![unit], vec![artifact])
        .unwrap()
        .attach_custom_blocks(vec![])
        .unwrap();
    assert!(absent.custom_blocks().is_empty());
    set.attach_custom_blocks(vec![opaque]).unwrap();
    assert_eq!(observer.0.load(Ordering::SeqCst), 0);
    let allocator = oxc_allocator::Allocator::default();
    let mut transform = crate::code_transform::CodeTransform::new("x", &allocator);
    transform.overwrite(0, 1, "y");
    assert!(observer.0.load(Ordering::SeqCst) > 0);
}

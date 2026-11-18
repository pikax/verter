use std::sync::Arc;

use verter_audit::observe::Uncaptured;
use verter_css_syntax::{CssDialect, QualifiedStyleResult, StyleStage};
use verter_identity::identity::{InputBasisId, SourceId};
use verter_span::Span;

use super::{
    VueArtifactSet, VueArtifactSetError, VueComponentMetadata, VueModuleOutput, VueModuleSource,
    VueStyleMetadata,
};
use crate::assembly::artifact_graph::{ArtifactGraph, ArtifactRole};
use crate::assembly::emit_plan::{EmitPlanBuilder, EmittedArtifact, MapMode};
use crate::assembly::fragment::{ArtifactContent, CompileArtifact, FragmentDialect};
use crate::assembly::source_space::SourceSpaceKind;
use crate::assembly::source_unit::{ArtifactSourceUnit, ContentId, SourceRevision, SourceUnit};
use crate::assembly::ArtifactSchemaError;
use crate::compile_request::ProductKind;
use crate::compile_transaction::{CompileAttempt, DirectSetAdmission, DirectSetTag};
use crate::emission::{QualifiedSource, SourceKey};
use crate::framework::vue::style::{VueStyleBlockInput, VueStyleBlockPlan, VueStyleOutcome};
use crate::style_planner::AuthoredStyleInput;
use crate::work_ledger::CompilerWorkLedger;

const SFC: &str = "<script setup>\nconst msg = 'hi'\n</script>\n<style scoped>.a{}</style>\n<style>.b{}</style>\n";
const CANONICAL: &str = "/src/App.vue";

fn tag(name: &str) -> DirectSetTag<'_> {
    DirectSetTag {
        canonical_id: name,
        framework: "vue",
        project: CompileAttempt::UNBOUND_PROJECT,
    }
}

fn admission() -> DirectSetAdmission<'static> {
    DirectSetAdmission {
        input_basis: InputBasisId::from_canonical(&tag("fixture-input")),
        canonical_id: CANONICAL,
        framework: "vue",
        is_production: false,
        force_js: false,
        source_digest: [7; 32],
        project: CompileAttempt::UNBOUND_PROJECT,
    }
}

/// The registered component source as one unit, and its emission key.
fn sfc_unit() -> ArtifactSourceUnit {
    ArtifactSourceUnit {
        unit: SourceUnit::mint(
            SourceId::from_canonical(&tag("fixture-sfc")),
            SourceRevision::from_canonical(&tag("fixture-sfc-revision")),
            "sfc",
            ContentId::from_content_bytes(SFC.as_bytes()),
        ),
        source_span: Span::new(0, SFC.len() as u32),
    }
}

fn sfc_source(unit: &ArtifactSourceUnit) -> QualifiedSource<'static> {
    QualifiedSource {
        key: SourceKey {
            unit: unit.unit.id().clone(),
            content: ContentId::from_content_bytes(SFC.as_bytes()),
            space: SourceSpaceKind::Original,
        },
        text: SFC,
    }
}

/// A module that copies the authored `const msg = 'hi'` between two
/// generated pieces.
fn module(product: ProductKind, mode: MapMode) -> VueModuleOutput {
    let unit = sfc_unit();
    let source = sfc_source(&unit);
    let statement = Span::new(15, 31);
    assert_eq!(&SFC[15..31], "const msg = 'hi'");
    let mut builder = EmitPlanBuilder::new(mode);
    builder.push_unmapped(if product == ProductKind::RuntimeServer {
        "// server\n"
    } else {
        "// client\n"
    });
    builder.push_source(&source, statement).unwrap();
    builder.push_unmapped("\nexport default { setup() { return { msg } } }\n");
    let written: EmittedArtifact = builder
        .finish()
        .unwrap()
        .write(&mut CompilerWorkLedger::<Uncaptured>::new())
        .unwrap();
    VueModuleOutput {
        product,
        dialect: FragmentDialect::JavaScript,
        module: written,
        sources: vec![VueModuleSource {
            unit,
            name: CANONICAL.to_owned(),
            text: (mode == MapMode::Map).then(|| Arc::from(SFC)),
        }],
    }
}

fn style_inputs() -> Vec<VueStyleBlockInput<'static>> {
    [(0, true, ".a{}"), (1, false, ".b{}")]
        .into_iter()
        .map(|(index, scoped, code)| VueStyleBlockInput {
            index,
            scoped,
            module: None,
            authored: AuthoredStyleInput::new(code, CssDialect::Css, CANONICAL, "style", "content"),
            prepared: None,
            stage: StyleStage::Authored,
        })
        .collect()
}

/// Block 0 rewritten for its scope; block 1 published as supplied.
fn style_blocks() -> Vec<VueStyleBlockPlan> {
    let rewritten = ".a[data-v-7ba5bd90]{}";
    vec![
        VueStyleBlockPlan {
            index: 0,
            scoped: true,
            module: None,
            outcome: VueStyleOutcome::Rewritten {
                code: rewritten.to_owned(),
                map: None,
                result: Box::new(QualifiedStyleResult::framework_rewritten(
                    CssDialect::Css,
                    rewritten,
                    Vec::new(),
                )),
            },
        },
        VueStyleBlockPlan {
            index: 1,
            scoped: false,
            module: None,
            outcome: VueStyleOutcome::Unchanged,
        },
    ]
}

fn metadata(styles: Vec<VueStyleMetadata>) -> VueComponentMetadata {
    VueComponentMetadata {
        scope_id: "7ba5bd90".to_owned(),
        has_scoped_style: true,
        inline_template: false,
        has_script: true,
        has_template: true,
        styles,
        custom_blocks: Vec::new(),
    }
}

fn two_styles() -> Vec<VueStyleMetadata> {
    vec![
        VueStyleMetadata {
            index: 0,
            scoped: true,
            module: None,
            lang: "css".to_owned(),
        },
        VueStyleMetadata {
            index: 1,
            scoped: false,
            module: None,
            lang: "css".to_owned(),
        },
    ]
}

fn build(modules: Vec<VueModuleOutput>) -> VueArtifactSet {
    VueArtifactSet::build(
        admission(),
        modules,
        &style_blocks(),
        &style_inputs(),
        &metadata(two_styles()),
        Default::default(),
        Vec::new(),
    )
    .unwrap()
}

fn text(artifact: &CompileArtifact) -> &str {
    match &artifact.content {
        ArtifactContent::Available(code) => code,
        ArtifactContent::Unavailable(reason) => panic!("no content: {reason:?}"),
    }
}

#[test]
fn a_client_and_server_request_publishes_two_modules_and_shared_styles() {
    let set = build(vec![
        module(ProductKind::RuntimeClient, MapMode::NoMap),
        module(ProductKind::RuntimeServer, MapMode::NoMap),
    ]);
    let graph = set.set();
    let client = graph.module(ProductKind::RuntimeClient).unwrap();
    let server = graph.module(ProductKind::RuntimeServer).unwrap();
    assert_eq!(set.module(ProductKind::RuntimeClient), Some(client.id()));
    assert_eq!(set.module(ProductKind::RuntimeServer), Some(server.id()));
    assert!(text(client).starts_with("// client\n"));
    assert!(text(server).starts_with("// server\n"));
    assert_eq!(client.name(), "main");

    let client_styles = graph.styles_of(client.id());
    let server_styles = graph.styles_of(server.id());
    assert_eq!(client_styles.len(), 2);
    assert_eq!(server_styles.len(), 2);
    for (index, (from_client, from_server)) in client_styles.iter().zip(&server_styles).enumerate()
    {
        assert_eq!(from_client.name(), format!("style/{index}"));
        assert_eq!(from_client.role(), ArtifactRole::Style);
        assert_eq!(from_client.language().as_str(), "css");
        assert_eq!(text(from_client), text(from_server), "style {index}");
        assert_eq!(from_client.style().unwrap().index, index as u32);
    }
    assert_eq!(text(client_styles[0]), ".a[data-v-7ba5bd90]{}");
    assert_eq!(
        client_styles[0].style().unwrap().stage,
        Some(StyleStage::FrameworkRewritten)
    );
    assert_eq!(text(client_styles[1]), ".b{}");
    assert_eq!(
        client_styles[1].style().unwrap().stage,
        Some(StyleStage::Authored)
    );

    let client_meta = graph.metadata_of(client.id()).unwrap();
    let server_meta = graph.metadata_of(server.id()).unwrap();
    assert_ne!(client_meta.id(), server_meta.id());
    assert_eq!(text(client_meta), text(server_meta));
    let metadata_count = graph
        .artifacts()
        .iter()
        .filter(|artifact| artifact.role() == ArtifactRole::Metadata)
        .count();
    assert_eq!(metadata_count, 2);
}

#[test]
fn metadata_json_is_stable() {
    let set = VueArtifactSet::build(
        admission(),
        vec![module(ProductKind::RuntimeClient, MapMode::NoMap)],
        &style_blocks()[..1],
        &style_inputs(),
        &metadata(two_styles()[..1].to_vec()),
        Default::default(),
        Vec::new(),
    )
    .unwrap();
    let client = set.set().module(ProductKind::RuntimeClient).unwrap();
    let meta = set.set().metadata_of(client.id()).unwrap();
    assert_eq!(meta.name(), "meta");
    assert_eq!(meta.language().as_str(), "json");
    assert_eq!(
        text(meta),
        r#"{"customBlocks":[],"hasScopedStyle":true,"hasScript":true,"hasTemplate":true,"inlineTemplate":false,"scopeId":"7ba5bd90","styles":[{"index":0,"lang":"css","module":null,"scoped":true}]}"#
    );
}

#[test]
fn stage_returns_the_modules_bytes_and_map() {
    let set = build(vec![module(ProductKind::RuntimeClient, MapMode::Map)]);
    let staged = set.stage(ProductKind::RuntimeClient).unwrap();
    let client = set.set().module(ProductKind::RuntimeClient).unwrap();
    assert_eq!(staged.root().id(), client.id());
    assert_eq!(&**staged.code(), text(client));
    assert_eq!(staged.product(), ProductKind::RuntimeClient);
    assert_eq!(staged.dialect(), FragmentDialect::JavaScript);

    // The authored statement maps byte for byte into the module, both in
    // the set and in the V3 map the handoff carries.
    let generated = text(client).find("const msg").unwrap() as u32;
    let segment = client.maps[0]
        .segments
        .iter()
        .find(|segment| segment.generated.start == generated)
        .unwrap();
    assert_eq!(segment.source_span, Span::new(15, 31));
    let map = oxc_sourcemap::SourceMap::from_json_string(staged.source_map().unwrap()).unwrap();
    assert_eq!(
        map.get_sources()
            .map(|name| name.to_string())
            .collect::<Vec<_>>(),
        [CANONICAL.to_owned()]
    );
    let token = map
        .get_tokens()
        .find(|token| token.get_dst_line() == 1 && token.get_dst_col() == 0)
        .unwrap();
    assert_eq!((token.get_src_line(), token.get_src_col()), (1, 0));
}

#[test]
fn a_set_without_the_requested_product_cannot_stage() {
    let set = build(vec![module(ProductKind::RuntimeClient, MapMode::NoMap)]);
    assert!(set.module(ProductKind::RuntimeServer).is_none());
    assert_eq!(
        set.stage(ProductKind::RuntimeServer).unwrap_err(),
        ArtifactSchemaError::UnknownRootArtifact
    );
    let staged = set.stage(ProductKind::RuntimeClient).unwrap();
    assert_eq!(staged.source_map(), None);
}

#[test]
fn a_no_map_handoff_keeps_no_source_text() {
    let set = build(vec![module(ProductKind::RuntimeClient, MapMode::NoMap)]);
    let id = set.module(ProductKind::RuntimeClient).unwrap();
    assert!(set.handoff.get(id).unwrap().rows.is_empty());
    assert_eq!(
        set.stage(ProductKind::RuntimeClient).unwrap().source_map(),
        None
    );
}

#[test]
fn an_unwitnessed_invalid_module_cannot_publish() {
    let mut plan = EmitPlanBuilder::new(MapMode::NoMap);
    plan.push_unmapped("const broken = ;");
    let module = plan
        .finish()
        .unwrap()
        .write(&mut CompilerWorkLedger::<Uncaptured>::new())
        .unwrap();
    let result = VueArtifactSet::build(
        admission(),
        vec![VueModuleOutput {
            product: ProductKind::RuntimeClient,
            dialect: FragmentDialect::JavaScript,
            module,
            sources: Vec::new(),
        }],
        &[],
        &[],
        &metadata(Vec::new()),
        Default::default(),
        Vec::new(),
    );
    assert!(matches!(
        result,
        Err(VueArtifactSetError::InvalidModule {
            product: ProductKind::RuntimeClient,
            ..
        })
    ));
}

#[test]
fn a_source_row_at_another_content_is_refused() {
    let mut output = module(ProductKind::RuntimeClient, MapMode::Map);
    let written = sfc_unit();
    output.sources[0].unit = ArtifactSourceUnit {
        unit: SourceUnit::mint(
            SourceId::from_canonical(&tag("fixture-sfc")),
            SourceRevision::from_canonical(&tag("fixture-sfc-revision")),
            "sfc",
            ContentId::from_content_bytes(b"<template></template>\n"),
        ),
        ..written
    };
    let refused = VueArtifactSet::build(
        admission(),
        vec![output],
        &style_blocks(),
        &style_inputs(),
        &metadata(two_styles()),
        Default::default(),
        Vec::new(),
    );
    assert!(
        matches!(
            refused,
            Err(VueArtifactSetError::ModuleSourcesMismatch {
                product: ProductKind::RuntimeClient
            })
        ),
        "the module was written from other bytes than the row names"
    );
}

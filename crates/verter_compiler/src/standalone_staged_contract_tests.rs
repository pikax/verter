//! Coverage for the staged compile contracts on the direct facade result:
//! the canonical [`crate::assembly::CompileArtifactSet`] every published
//! product is admitted to, the staged runtime-module handoff minted over
//! it, and the per-kind SFC-shaped projection bound to that set. The
//! projection must remain a view over the set — the same byte allocation,
//! joined by typed artifact identity — so the facade carries one artifact
//! authority, never two.

use super::*;
use crate::compile_request::{
    CompileProduct, IdeProductRequest, ProductKind, RuntimeProductRequest,
};

const VUE_SIMPLE: &str = include_str!("../../verter_bench/benches/fixtures/simple.vue");
const SVELTE_PROPS: &str = include_str!(
    "../../verter_svelte_conformance/corpus/fixtures/attr-dyn-lit-attr-q-el-ext-plain-m.svelte"
);

fn vue_request(products: Vec<CompileProduct>) -> CompileRequest {
    CompileRequest::new(
        products,
        FrameworkCompileRequest::Vue(crate::compile_request::VueCompileRequest::default()),
        None,
        Some("Comp.vue".to_string()),
        None,
        false,
        false,
    )
    .expect("test request constructs")
}

fn svelte_request(products: Vec<CompileProduct>) -> CompileRequest {
    CompileRequest::new(
        products,
        FrameworkCompileRequest::Svelte(crate::compile_request::SvelteCompileRequest::default()),
        None,
        Some("Comp.svelte".to_string()),
        None,
        false,
        false,
    )
    .expect("test request constructs")
}

static LEAKED_VUE_EXECUTION_INPUTS: &VueExecutionInputs = &VueExecutionInputs {
    macro_runtime: None,
    prop_constness_overrides: None,
    style_v_bind_vars: Vec::new(),
    style_v_bind_usage_complete: None,
    template_binding_metadata: None,
    template_used_vars: None,
    runtime_template_hole: false,
    runtime_inline_template_chunk: false,
    prepared_styles: Vec::new(),
};
static LEAKED_VUE_MACROS: &VueMacroSemanticInput = &VueMacroSemanticInput::Unavailable;
static LEAKED_SVELTE_EXECUTION_INPUTS: &SvelteExecutionInputs = &SvelteExecutionInputs {
    css_hash_override: None,
    prepared_styles: Vec::new(),
};

fn vue_inputs() -> DirectExecutionInputs<'static> {
    DirectExecutionInputs::Vue {
        execution: LEAKED_VUE_EXECUTION_INPUTS,
        macros: LEAKED_VUE_MACROS,
    }
}

fn svelte_inputs() -> DirectExecutionInputs<'static> {
    DirectExecutionInputs::Svelte {
        execution: LEAKED_SVELTE_EXECUTION_INPUTS,
    }
}

#[test]
fn runtime_compile_stages_the_complete_artifact_handoff() {
    let request = vue_request(vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
        runtime_source_map: true,
        ..Default::default()
    })]);
    let output = StandaloneCompiler
        .compile(VUE_SIMPLE, &request, vue_inputs())
        .expect("runtime client compile must succeed");

    let (row_code, row_map) = {
        let row = output
            .artifact(ProductKind::RuntimeClient)
            .expect("the runtime client product must project");
        (
            row.code().to_string(),
            row.runtime_source_map().map(str::to_string),
        )
    };
    let staged = output
        .into_staged_main()
        .expect("the set conversion must validate")
        .expect("a runtime product must stage a handoff");
    assert_eq!(
        staged.root().product(),
        ProductKind::RuntimeClient,
        "the staged root is the runtime module artifact"
    );
    assert_eq!(&**staged.code(), row_code.as_str());
    assert_eq!(staged.source_map(), row_map.as_deref());
    assert!(
        staged.set().artifact(staged.root().id()).is_some(),
        "the staged root must name an artifact of the staged set"
    );
}

#[test]
fn svelte_runtime_compile_stages_the_module_handoff() {
    let request = svelte_request(vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
        runtime_source_map: true,
        ..Default::default()
    })]);
    let output = StandaloneCompiler
        .compile(SVELTE_PROPS, &request, svelte_inputs())
        .expect("svelte runtime client compile must succeed");
    let staged = output
        .into_staged_main()
        .expect("the set conversion must validate")
        .expect("a svelte runtime product must stage a handoff");
    assert_eq!(staged.lang(), "js");
    assert!(
        staged.code().contains("svelte"),
        "the staged module must carry the svelte runtime import"
    );
}

#[test]
fn dual_leg_compile_stages_the_server_root_and_projects_both_kinds() {
    let request = vue_request(vec![
        CompileProduct::RuntimeServer(RuntimeProductRequest {
            runtime_source_map: true,
            ..Default::default()
        }),
        CompileProduct::RuntimeClient(RuntimeProductRequest {
            runtime_source_map: true,
            ..Default::default()
        }),
    ]);
    let output = StandaloneCompiler
        .compile(VUE_SIMPLE, &request, vue_inputs())
        .expect("a dual-kind request must compile");
    assert!(
        output.artifact(ProductKind::RuntimeClient).is_some()
            && output.artifact(ProductKind::RuntimeServer).is_some(),
        "both runtime legs must project"
    );
    let staged = output
        .into_staged_main()
        .expect("the set conversion must validate")
        .expect("a runtime product must stage a handoff");
    assert_eq!(
        staged.root().product(),
        ProductKind::RuntimeServer,
        "the server leg is the primary staged module"
    );
}

#[test]
fn ide_only_compile_carries_the_companion_without_a_runtime_handoff() {
    let request = vue_request(vec![CompileProduct::IdeCompanion(IdeProductRequest {
        want_source_map: true,
        ..Default::default()
    })]);
    let output = StandaloneCompiler
        .compile(VUE_SIMPLE, &request, vue_inputs())
        .expect("an IDE-only request must compile");
    let row = output
        .artifact(ProductKind::IdeCompanion)
        .expect("the companion must project");
    assert!(
        row.source_projection_map().is_some(),
        "an IDE companion always carries its projection map"
    );
    assert_eq!(
        output.set().artifacts().len(),
        1,
        "the set admits exactly the published companion"
    );
    assert!(
        output
            .into_staged_main()
            .expect("the set conversion must validate")
            .is_none(),
        "no runtime product was requested, so no module stages"
    );
}

#[test]
fn projection_rows_are_bound_to_the_artifact_set() {
    let request = vue_request(vec![
        CompileProduct::RuntimeClient(RuntimeProductRequest {
            runtime_source_map: true,
            ..Default::default()
        }),
        CompileProduct::IdeCompanion(IdeProductRequest {
            want_source_map: true,
            ..Default::default()
        }),
    ]);
    let output = StandaloneCompiler
        .compile(VUE_SIMPLE, &request, vue_inputs())
        .expect("a co-planned request must compile");
    assert!(
        output.projection_over_set(),
        "every row must name a set artifact sharing its byte allocation"
    );
    for row in output.artifacts() {
        let artifact = output
            .set()
            .artifact(row.artifact_id())
            .expect("each row joins a set artifact by typed identity");
        assert_eq!(artifact.product(), row.kind());
        match &artifact.content {
            ArtifactContent::Available(content) => assert!(
                std::sync::Arc::ptr_eq(content, row.code_allocation()),
                "row bytes and set content must be one allocation"
            ),
            ArtifactContent::Unavailable(_) => {
                panic!("a published product's artifact content is available")
            }
        }
    }
}

#[test]
fn qualified_maps_and_diagnostics_survive_the_conversion() {
    let request = vue_request(vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
        runtime_source_map: true,
        ..Default::default()
    })]);
    let output = StandaloneCompiler
        .compile(VUE_SIMPLE, &request, vue_inputs())
        .expect("runtime client compile must succeed");
    let root = output
        .set()
        .artifacts()
        .iter()
        .find(|artifact| artifact.product() == ProductKind::RuntimeClient)
        .expect("runtime client artifact");
    let runtime_map = root
        .maps
        .iter()
        .find(|map| map.family == ArtifactMapFamily::RuntimeSourceMap)
        .expect("the demanded runtime map qualifies inside the set");
    assert!(
        !runtime_map.segments.is_empty(),
        "the qualified map carries real segments for a mapped compile"
    );
    assert!(
        output.diagnostics.is_empty(),
        "a clean fixture compiles without diagnostics"
    );
}

#[test]
fn a_one_shot_compile_has_no_source_backed_custom_block_descriptors() {
    // A one-shot compile holds no registered carrier lineage, so the
    // source-backed descriptor set stays absent — descriptors are never
    // reconstructed from block metadata without lineage.
    let source = "<template><p>hi</p></template>\n<config>{\"a\":1}</config>\n";
    let request = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let output = StandaloneCompiler
        .compile(source, &request, vue_inputs())
        .expect("a custom-block carrier still compiles its runtime module");
    assert!(output.custom_block_descriptors().is_empty());
}

#[test]
fn revision_and_basis_follow_the_published_bytes() {
    let request = vue_request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let first = StandaloneCompiler
        .compile(VUE_SIMPLE, &request, vue_inputs())
        .expect("compile");
    let edited_source = format!("{VUE_SIMPLE}\n<!-- edit -->\n");
    let second = StandaloneCompiler
        .compile(edited_source.as_str(), &request, vue_inputs())
        .expect("compile");
    let first_unit = first
        .set()
        .source_units()
        .find(|unit| unit.unit.logical_role() == "source")
        .expect("authored source unit");
    let second_unit = second
        .set()
        .source_units()
        .find(|unit| unit.unit.logical_role() == "source")
        .expect("authored source unit");
    assert_ne!(
        first_unit.unit.revision(),
        second_unit.unit.revision(),
        "an authored edit must change the source revision"
    );
}

/// Publish a synthetic artifact set over `source` — the direct facade's own
/// atomicity boundary, driven here with contributions whose maps are built
/// to order so the staged conversion's map geometry is observable.
fn published_set(
    request: &CompileRequest,
    runtime_map_json: Option<String>,
    ide_map_json: Option<String>,
) -> crate::assembly::ArtifactSet {
    use crate::assembly::fragment::FragmentDialect;
    use crate::assembly::publish::{publish, ArtifactContribution};
    use crate::assembly::ProductPlan;

    let plan = ProductPlan::from_request(request);
    let mut contributions: Vec<ArtifactContribution<'_>> = Vec::new();
    for kind in [
        ProductKind::RuntimeServer,
        ProductKind::RuntimeClient,
        ProductKind::IdeCompanion,
    ] {
        if !plan.wants(kind) {
            continue;
        }
        let (runtime_source_map, source_projection_map) = match kind {
            ProductKind::IdeCompanion => (None, ide_map_json.clone().or(Some(String::new()))),
            _ => (runtime_map_json.clone().or(Some(String::new())), None),
        };
        contributions.push(ArtifactContribution {
            kind,
            fragments: Vec::new(),
            code: format!("export const leg = 1;\nexport const {kind} = 2;\n"),
            emitted_imports: Vec::new(),
            dialect: FragmentDialect::Tsx,
            source_projection_map,
            runtime_source_map,
        });
    }
    publish(&plan, contributions).expect("synthetic publication")
}

fn carrier_row_map(source: &str) -> String {
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let row = builder.add_source_and_content("Comp.vue", source);
    builder.add_token(0, 0, 0, 0, Some(row), None);
    builder.into_sourcemap().to_json_string()
}

#[test]
fn artifact_provenance_names_no_sibling_product_unit() {
    let source = "<template><p>hi</p></template>\n";
    let request = vue_request(vec![
        CompileProduct::RuntimeServer(RuntimeProductRequest {
            runtime_source_map: true,
            ..Default::default()
        }),
        CompileProduct::RuntimeClient(RuntimeProductRequest {
            runtime_source_map: true,
            ..Default::default()
        }),
        CompileProduct::IdeCompanion(IdeProductRequest {
            want_source_map: true,
            ..Default::default()
        }),
    ]);
    let published = published_set(
        &request,
        Some(carrier_row_map(source)),
        Some(carrier_row_map(source)),
    );
    let output = stage_published_products(
        source,
        &request,
        "vue",
        &published,
        Vec::new(),
        None,
        Vec::new(),
    )
    .expect("the staged conversion validates");

    let unit_id = |role: &str| {
        output
            .set()
            .source_units()
            .find(|unit| unit.unit.logical_role() == role)
            .unwrap_or_else(|| panic!("unit with role {role}"))
            .unit
            .id()
            .clone()
    };
    let authored = unit_id("source");
    for kind in [
        ProductKind::RuntimeServer,
        ProductKind::RuntimeClient,
        ProductKind::IdeCompanion,
    ] {
        let artifact = output
            .set()
            .artifacts()
            .iter()
            .find(|artifact| artifact.product() == kind)
            .expect("published product artifact");
        let own = unit_id(kind.wire_tag());
        assert_eq!(
            artifact.provenance.inputs,
            std::collections::BTreeSet::from([authored.clone(), own]),
            "each artifact's inputs are exactly the authored carrier and \
             its own product unit — a sibling product is never an input"
        );
    }
}

#[test]
fn map_segments_bind_each_declared_source_space_not_the_carrier() {
    let source = "<template><p>hi</p></template>\n";
    let external = "const selected = 1;\n";
    let request = vue_request(vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
        runtime_source_map: true,
        ..Default::default()
    })]);
    let mut map = oxc_sourcemap::SourceMapBuilder::default();
    let carrier_row = map.add_source_and_content("Comp.vue", source);
    let external_row = map.add_source_and_content("selected.ts", external);
    map.add_token(0, 0, 0, 0, Some(carrier_row), None);
    map.add_token(0, 6, 0, 6, Some(external_row), None);
    let published = published_set(&request, Some(map.into_sourcemap().to_json_string()), None);
    let output = stage_published_products(
        source,
        &request,
        "vue",
        &published,
        Vec::new(),
        None,
        Vec::new(),
    )
    .expect("the staged conversion validates");

    let minted = output
        .set()
        .source_units()
        .find(|unit| unit.unit.logical_role().starts_with("map-source-"))
        .expect("the external declared space mints its own unit");
    assert_eq!(
        minted.unit.content(),
        &crate::assembly::source_unit::ContentId::from_content_bytes(external.as_bytes()),
        "the minted unit carries the declared space's own bytes"
    );
    assert_eq!(
        minted.source_span,
        verter_span::Span::new(0, external.len() as u32),
        "the minted unit's extent is the declared content, not the carrier"
    );

    let artifact = output
        .set()
        .artifacts()
        .iter()
        .find(|artifact| artifact.product() == ProductKind::RuntimeClient)
        .expect("runtime client artifact");
    let runtime_map = artifact
        .maps
        .iter()
        .find(|map| map.family == ArtifactMapFamily::RuntimeSourceMap)
        .expect("the demanded runtime map qualifies inside the set");
    assert!(
        runtime_map
            .segments
            .iter()
            .any(|segment| segment.source_unit == *minted.unit.id()
                && segment.source_span.end <= external.len() as u32),
        "a token over the external space binds the minted unit inside its \
         own extent"
    );
    assert!(
        runtime_map.sources.contains(minted.unit.id()),
        "the qualified map declares the minted unit among its sources"
    );
    assert!(
        artifact.provenance.inputs.contains(minted.unit.id()),
        "the artifact's provenance names the declared source space its map \
         binds"
    );
}

#[test]
fn a_map_source_without_content_refuses_the_conversion() {
    let source = "<template><p>hi</p></template>\n";
    let request = vue_request(vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
        runtime_source_map: true,
        ..Default::default()
    })]);
    // One token addressing a source row that declares no content: its
    // authored identity cannot be presented as a source unit, so the
    // conversion refuses instead of rebinding the token to the carrier.
    let map_json = r#"{"version":3,"sources":["selected.ts"],"names":[],"mappings":"AAAA"}"#;
    let published = published_set(&request, Some(map_json.to_string()), None);
    let err = stage_published_products(
        source,
        &request,
        "vue",
        &published,
        Vec::new(),
        None,
        Vec::new(),
    )
    .expect_err("a contentless declared source must refuse the conversion");
    assert_eq!(
        err,
        crate::assembly::publish::ArtifactSchemaError::MapSourceWithoutContent
    );
}

/// A host-supplied selected-template map may carry valid positions without
/// the bytes its rows address (`sourcesContent` absent). Chaining through
/// such rows would publish tokens over rows no staged artifact can resolve,
/// so the supplied map is never admitted for chaining: the compile still
/// succeeds and publishes the compiler's own map over the selected bytes,
/// every declared row of which carries content.
#[test]
fn a_contentless_host_supplied_selected_template_map_is_omitted_not_chained() {
    let source = "<template><p>hi</p></template>\n";
    let selected = "<p>{{ count }}</p>";
    let request = vue_request(vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
        runtime_source_map: true,
        ..Default::default()
    })]);
    // Positions the host holds without the bytes they address: one
    // token-addressed row declaring no `sourcesContent` entry.
    let host_map = r#"{"version":3,"sources":["Comp.vue"],"names":[],"mappings":"AAAA"}"#;
    let parsed = crate::compile::parse_sfc(source, None, None);
    let block_content = RuntimeBlockContentInputs {
        template: Some(RuntimeBlockContentInput {
            code: std::sync::Arc::from(selected),
            source_map: Some(std::sync::Arc::from(host_map)),
            lang: "html".to_string(),
            content_artifact_token: "content:template".to_string(),
            source_space_token: "space:template".to_string(),
            parsed: None,
            producer: None,
            authored_basis: None,
            diagnostics: Vec::new(),
        }),
        ..Default::default()
    };
    let output = compile_vue_parsed_runtime(
        source,
        &parsed,
        &request,
        LEAKED_VUE_EXECUTION_INPUTS,
        LEAKED_VUE_MACROS,
        &block_content,
        &[],
    )
    .expect("a contentless host map over the selected bytes must not fail the compile");

    let row = output
        .artifact(ProductKind::RuntimeClient)
        .expect("the runtime client product must project");
    let map_json = row
        .runtime_source_map()
        .expect("the demanded runtime map is still published over the selected bytes");
    let published = oxc_sourcemap::SourceMap::from_json_string(map_json)
        .expect("the published map is valid JSON");
    assert!(
        published.get_sources().count() > 0 && published.get_tokens().next().is_some(),
        "the demanded runtime map must retain source rows and mappings"
    );
    assert!(
        (0..published.get_sources().count()).all(|index| published
            .get_source_content(index as u32)
            .is_some_and(|content| !content.is_empty())),
        "every declared row of the published map carries its own content"
    );
    output
        .into_staged_main()
        .expect("the set conversion must validate")
        .expect("a runtime product must stage a handoff");
}

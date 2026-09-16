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

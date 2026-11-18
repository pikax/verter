use super::*;
use verter_compiler::assembly::ContentId;
use verter_compiler::framework_common::RuntimeBlockContentInput;
use verter_compiler::style_planner::{ExternalStyleProducer, PreprocessorIdentity};

#[test]
fn vue_main_assembly_failure_keeps_the_stable_host_code() {
    let unsupported = verter_compiler::framework_common::CompileUnsupported::VueMainAssemblyFailed(
        "the script fragment is authored and present but carries no source map".to_string(),
    );
    assert_eq!(
        compile_unsupported_code(&unsupported),
        "HOST_MAIN_MODULE_ASSEMBLY_FAILED"
    );
    let artifact = verter_compiler::framework_common::registered_carrier_projection::parse_registered_source_for_tests(
        verter_language::FileLanguage::vue(),
        verter_compiler::framework_common::vue_grammar_config(
            "{{",
            "}}",
            std::iter::empty::<&str>(),
        )
        .unwrap(),
        "<script setup>const n = 1</script><template><div/></template>",
    );
    let snapshot = runtime_bundle_unsupported_diagnostics(&artifact, "Comp.vue", 12, &unsupported);
    assert_eq!(
        snapshot.diagnostics[0].code,
        "HOST_MAIN_MODULE_ASSEMBLY_FAILED"
    );
    assert!(
        snapshot.diagnostics[0]
            .message
            .contains("carries no source map"),
        "typed assembly reason must reach the host diagnostic, got {}",
        snapshot.diagnostics[0].message
    );
}

#[test]
fn compile_host_products_assembly_refusal_maps_to_the_stable_host_code() {
    use verter_compiler::compile_request::{CompileProduct, RuntimeProductRequest};
    use verter_compiler::framework_common::FrameworkHostIntegrationBackend as _;
    use verter_compiler::framework_common::{
        VueHostCompileRefusal, VueHostExecutionInputs, VueHostIntegrationBackend,
        VueHostMultiProductDemand,
    };

    let artifact = verter_compiler::framework_common::registered_carrier_projection::parse_registered_source_for_tests(
        verter_language::FileLanguage::vue(),
        verter_compiler::framework_common::vue_grammar_config(
            "{{",
            "}}",
            std::iter::empty::<&str>(),
        )
        .unwrap(),
        "<script setup>const n = 1</script><template><div>{{ n }}</div></template>",
    );
    let alloc = oxc_allocator::Allocator::new();
    let backend = VueHostIntegrationBackend::registered();
    let admission = backend
        .admit_host_products(
            artifact.as_ref(),
            VueHostMultiProductDemand {
                products: vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
                    runtime_source_map: true,
                    ..Default::default()
                })],
                ..Default::default()
            },
        )
        .expect("admits maps-on Main");
    let refusal = backend
        .compile_host_products(
            admission,
            artifact.as_ref(),
            &VueHostExecutionInputs {
                want_main: true,
                has_script: true,
                has_template: true,
                canonical_id: "/src/Maps.vue".to_string(),
                drop_required_script_map: true,
                ..Default::default()
            },
            &alloc,
        )
        .expect_err("host products lane must refuse missing required script map");
    let VueHostCompileRefusal::Unsupported(unsupported) = refusal else {
        panic!("expected CompileUnsupported assembly refusal, got {refusal:?}");
    };
    let snapshot = runtime_bundle_unsupported_diagnostics(
        artifact.as_ref(),
        "/src/Maps.vue",
        12,
        &unsupported,
    );
    assert_eq!(
        snapshot.diagnostics[0].code,
        "HOST_MAIN_MODULE_ASSEMBLY_FAILED"
    );
    assert!(
        !snapshot.diagnostics[0].message.is_empty(),
        "typed assembly reason must reach the host diagnostic"
    );
}

const AUTHORED: &str = "$tone: red;\n.card { color: $tone; }";
const PRODUCED: &str = ".card { color: red; }";

fn selected_style(producer: Option<PreprocessorIdentity>) -> RuntimeBlockContentInput {
    RuntimeBlockContentInput {
        code: std::sync::Arc::from(PRODUCED),
        source_map: None,
        lang: "css".to_string(),
        content_artifact_token: "host-artifact:produced".to_string(),
        source_space_token: "host-space:produced".to_string(),
        parsed: None,
        producer,
        authored_basis: Some(ContentId::from_content_bytes(AUTHORED.as_bytes())),
        diagnostics: Vec::new(),
    }
}

/// A selected style slot reaches the compiler with the basis the
/// PROJECTION states — the authored bytes this selection was validated
/// against, not the produced bytes it carries and not a value re-derived
/// from whatever the carrier holds later.
#[test]
fn a_selected_style_carries_its_projections_authored_basis() {
    const HOST_MAP: &str = r#"{"version":3,"sources":["Card.scss"],"names":[],"mappings":"AACA"}"#;
    let reported = vec![verter_css_syntax::StyleDiagnostic::with_severity(
        verter_css_syntax::StyleStage::Authored,
        verter_css_syntax::StyleDiagnosticSeverity::Warning,
        "deprecated division",
        None,
    )];
    let supplied = svelte_supplied_style(RuntimeBlockContentInput {
        source_map: Some(std::sync::Arc::from(HOST_MAP)),
        diagnostics: reported.clone(),
        ..selected_style(Some(PreprocessorIdentity::Named(
            ExternalStyleProducer::new("sass", Some("1.77.0"), None).expect("named producer"),
        )))
    });
    assert_eq!(
        supplied.source_map.as_deref(),
        Some(HOST_MAP),
        "the host's produced-to-authored map reaches the compiler with its bytes"
    );
    assert_eq!(
        supplied.diagnostics, reported,
        "the tool's diagnostics reach the compiler with its bytes"
    );
    assert_eq!(supplied.code.as_ref(), PRODUCED);
    assert_eq!(
        supplied.consumed_basis,
        Some(ContentId::from_content_bytes(AUTHORED.as_bytes())),
        "the basis names the authored bytes the selection was validated against"
    );
    assert_ne!(
        supplied.consumed_basis,
        Some(ContentId::from_content_bytes(PRODUCED.as_bytes())),
        "the produced bytes are not their own basis"
    );
    assert_eq!(supplied.source_space_token, "host-space:produced");
    assert_eq!(supplied.content_artifact_token, "host-artifact:produced");
}

/// A slot the host names no external producer for still continues its
/// block, anonymously. Dropping it would hand the compiler no result for
/// a block that has one, and the authored bytes would be compiled in its
/// place with no diagnostic.
#[test]
fn a_producerless_selected_style_continues_its_block_anonymously() {
    let supplied = svelte_supplied_style(selected_style(None));
    assert_eq!(supplied.producer, PreprocessorIdentity::Anonymous);
    assert_eq!(supplied.code.as_ref(), PRODUCED);
    assert_eq!(
        supplied.consumed_basis,
        Some(ContentId::from_content_bytes(AUTHORED.as_bytes())),
    );
}

/// The profile-borne Svelte option axes (`css`, the custom-element
/// `tag`/`shadow` descriptor axis, `compatibility`) survive
/// [`svelte_host_products_demand`] onto the bound demand's typed
/// option attempt — they must REACH the backend's issuance (where the
/// unroutable ones refuse typed) rather than being silently dropped
/// by the demand builder.
#[test]
fn svelte_option_axes_survive_onto_the_bound_demand_attempt() {
    let profile = CompileProfile {
        target: CompileTarget::BUNDLER,
        svelte_css: Some("injected".to_string()),
        svelte_custom_element_tag: Some("my-widget".to_string()),
        svelte_custom_element_shadow: Some(false),
        svelte_compatibility: Some(true),
        ..CompileProfile::default()
    };
    let demand = svelte_host_products_demand(&profile, "/w.svelte", true, false, false)
        .expect("every axis decodes; refusal is the backend issuance's, not the decoder's");
    let attempt = &demand.svelte_options;
    assert_eq!(
        attempt.css,
        Some(verter_compiler::compile_request::svelte::SvelteCssRequest::Injected)
    );
    let descriptor = attempt
        .custom_element_descriptor
        .as_ref()
        .expect("tag/shadow set => descriptor constructed");
    assert_eq!(descriptor.tag.as_deref(), Some("my-widget"));
    assert_eq!(descriptor.shadow, Some(false));
    assert!(descriptor.props.is_empty());
    assert!(attempt.compatibility.is_some());
}

/// Negative control: leaving every descriptor field unset must NOT
/// construct a descriptor at all (an all-`None` descriptor is a
/// different demand from no descriptor — see
/// `resolve_custom_element`'s `compile_option_descriptor` fallback
/// precedence).
#[test]
fn unset_custom_element_fields_construct_no_descriptor() {
    let profile = CompileProfile {
        target: CompileTarget::BUNDLER,
        ..CompileProfile::default()
    };
    let demand = svelte_host_products_demand(&profile, "/w.svelte", true, false, false)
        .expect("default profile decodes");
    assert!(demand.svelte_options.custom_element_descriptor.is_none());
    assert_eq!(demand.svelte_options.css, None);
}

/// `svelte_generate_module` set on the session `CompileProfile` must
/// refuse canonical request construction — `ModuleCompileOptions.generate`
/// is gated by the `SVELTE-MODULE` capability, `unsupported
/// fail-closed` per `capability-matrix.tsv`. The bound demand decodes
/// (the axis is a valid attempt), and the refusal fires at the SAME
/// option-admission boundary the backend's issuance composes the
/// canonical request through (`SvelteOptionAttempt::into_request`).
#[test]
fn svelte_generate_module_refuses_the_svelte_module_capability() {
    let profile = CompileProfile {
        target: CompileTarget::BUNDLER,
        svelte_generate_module: Some(true),
        ..CompileProfile::default()
    };
    let demand = svelte_host_products_demand(&profile, "/w.svelte", true, false, false)
        .expect("the axis decodes; the refusal is admission's");
    let err = demand
        .svelte_options
        .into_request()
        .expect_err("generate_module must refuse — SVELTE-MODULE is unsupported fail-closed");
    match err {
        verter_compiler::compile_request::CompileRequestError::UnsupportedOption {
            capability,
            ..
        } => assert_eq!(
            capability,
            Some(verter_compiler::compile_request::CapabilityCell::SvelteModule)
        ),
        other => panic!("expected UnsupportedOption naming SvelteModule, got {other:?}"),
    }
}

/// Same refusal for `svelte_experimental_async` — the SAME capability
/// cell gates `ModuleCompileOptions.experimental.async`.
#[test]
fn svelte_experimental_async_refuses_the_svelte_module_capability() {
    let profile = CompileProfile {
        target: CompileTarget::BUNDLER,
        svelte_experimental_async: Some(true),
        ..CompileProfile::default()
    };
    let demand = svelte_host_products_demand(&profile, "/w.svelte", true, false, false)
        .expect("the axis decodes; the refusal is admission's");
    let err = demand
        .svelte_options
        .into_request()
        .expect_err("experimental_async must refuse — SVELTE-MODULE is unsupported fail-closed");
    match err {
        verter_compiler::compile_request::CompileRequestError::UnsupportedOption {
            capability,
            ..
        } => assert_eq!(
            capability,
            Some(verter_compiler::compile_request::CapabilityCell::SvelteModule)
        ),
        other => panic!("expected UnsupportedOption naming SvelteModule, got {other:?}"),
    }
}

/// A malformed `svelte_css` value refuses at the demand's decode
/// boundary, matching `svelte_namespace`/`svelte_fragments`'s
/// decode-boundary refusal — never a silent default.
#[test]
fn malformed_svelte_css_value_refuses() {
    let profile = CompileProfile {
        target: CompileTarget::BUNDLER,
        svelte_css: Some("not-a-real-mode".to_string()),
        ..CompileProfile::default()
    };
    let err = svelte_host_products_demand(&profile, "/w.svelte", true, false, false)
        .expect_err("an unrecognized css mode string must refuse");
    assert!(matches!(
        err,
        verter_compiler::compile_request::CompileRequestError::MalformedOptionValue { .. }
    ));
}

/// The runtime product's KIND follows the profile's `ssr` axis at
/// EVERY construction site, including the placeholder synthesized when
/// the call demands no product at all. A placeholder that hard-coded
/// the client kind would hand the backend a runtime product
/// contradicting the profile's own ssr axis — the one demand shape the
/// backend refuses as unproducible, turning a zero-demand call into an
/// admission refusal.
#[test]
fn the_synthesized_placeholder_product_follows_the_profiles_ssr_axis() {
    use verter_compiler::compile_request::{CompileProduct, ProductKind};

    for (ssr, expected) in [
        (false, ProductKind::RuntimeClient),
        (true, ProductKind::RuntimeServer),
    ] {
        let profile = CompileProfile {
            target: CompileTarget::empty(),
            ssr,
            ..CompileProfile::default()
        };
        // Nothing demanded: the builder must still yield exactly one
        // product, because canonical request construction refuses an
        // empty product set outright.
        let placeholder = demanded_products(&profile, false, false, false);
        assert_eq!(
            placeholder
                .iter()
                .map(CompileProduct::kind)
                .collect::<Vec<_>>(),
            vec![expected],
            "the synthesized placeholder must carry the profile's own runtime kind"
        );
        // The demanded branch agrees by construction — same builder.
        let demanded = demanded_products(&profile, true, false, false);
        assert_eq!(
            demanded
                .iter()
                .map(CompileProduct::kind)
                .collect::<Vec<_>>(),
            vec![expected],
        );
    }
}

/// The response's artifact-set field carries the set's terminal JSON
/// verbatim: a module and its style, with their roles and the
/// style-to-module relation, read back at schema version 2.
#[test]
fn a_compile_response_can_carry_an_artifact_set() {
    use std::collections::BTreeSet;
    use verter_compiler::assembly::artifact_graph::{ArtifactRole, StyleArtifactFacts};
    use verter_compiler::assembly::{
        ArtifactContent, ArtifactProvenance, ArtifactRelation, ArtifactRelationKind,
        ArtifactSourceUnit, CompileArtifact, CompileArtifactSet, SourceId, SourceRevision,
        SourceUnit,
    };
    use verter_compiler::compile_request::ProductKind;
    use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
    use verter_identity::identity::{InputBasisId, ResultContractId};

    struct Tag(&'static str);
    impl CanonicalEncode for Tag {
        const DOMAIN_TAG: &'static str = "verter.session.compile-response.fixture.v1";
        fn encode_fields(&self, e: &mut CanonicalEncoder) {
            e.field_str(1, self.0);
        }
    }
    const SOURCE: &str = "<template><p/></template><style>p{}</style>";
    let unit = SourceUnit::mint(
        SourceId::from_canonical(&Tag("/src/Comp.vue")),
        SourceRevision::from_canonical(&Tag("one")),
        "main",
        ContentId::from_content_bytes(SOURCE.as_bytes()),
    );
    let artifact = |name: &str, language: &str, role: ArtifactRole| {
        CompileArtifact::new(
            unit.id().clone(),
            ProductKind::RuntimeClient,
            verter_language::LanguageId::new(language),
            name,
            ArtifactProvenance {
                input_basis: InputBasisId::from_canonical(&Tag("inputs")),
                producer: ResultContractId::from_canonical(&Tag("vue")),
                inputs: BTreeSet::from([unit.id().clone()]),
            },
            ArtifactContent::Available(format!("/* {name} */").into()),
        )
        .with_role(role)
    };
    let module = artifact("main", "javascript", ArtifactRole::Module);
    let mut style =
        artifact("style/0", "css", ArtifactRole::Style).with_style(StyleArtifactFacts {
            index: 0,
            scoped: false,
            module: None,
            lang: "css".to_string(),
            stage: None,
            needs_external_stage: false,
        });
    style.relations.insert(ArtifactRelation {
        kind: ArtifactRelationKind::StyleOf,
        target: module.id().clone(),
    });
    let set = CompileArtifactSet::new(
        vec![ArtifactSourceUnit {
            unit: unit.clone(),
            source_span: verter_span::Span::new(0, SOURCE.len() as u32),
        }],
        vec![module, style],
    )
    .expect("a module with one owned style is a valid set");

    let response = CompileRequestResponse {
        canonical_id: "/src/Comp.vue".to_string(),
        diagnostics: DiagnosticsSnapshot::default(),
        products: Vec::new(),
        artifact_set: Some(set.to_json().expect("validated sets serialize")),
    };
    let wire: serde_json::Value = serde_json::from_str(
        response
            .artifact_set
            .as_deref()
            .expect("the field is present when a set was published"),
    )
    .expect("the field holds JSON");
    assert_eq!(wire["schemaVersion"], 2);
    let artifacts = wire["artifacts"].as_array().expect("artifacts");
    let named = |name: &str| {
        artifacts
            .iter()
            .find(|artifact| artifact["name"] == name)
            .unwrap_or_else(|| panic!("artifact {name} is on the wire"))
    };
    assert_eq!(named("main")["role"], "module");
    assert_eq!(named("style/0")["role"], "style");
    assert_eq!(named("style/0")["style"]["index"], 0);
    assert_eq!(named("style/0")["relations"][0]["kind"], "styleOf");
    assert_eq!(
        named("style/0")["relations"][0]["target"],
        named("main")["id"]
    );
}

/// A compile that needs a style block at an external stage refuses on
/// both lanes with the block content diagnostic, naming the block and
/// the stage, and publishes nothing.
#[test]
fn style_need_inputs_maps_to_a_refusal_naming_the_stage() {
    use verter_compiler::compile_request::demand::InputDemand;
    use verter_compiler::framework_common::VueHostCompileRefusal;
    use verter_compiler::style_planner::StyleStage;

    const SOURCE: &str =
        "<template><p/></template><style>p{}</style><style lang=\"scss\">$a: 1;</style>";
    let artifact = verter_compiler::framework_common::registered_carrier_projection::parse_registered_source_for_tests(
        verter_language::FileLanguage::vue(),
        verter_compiler::framework_common::vue_grammar_config(
            "{{",
            "}}",
            std::iter::empty::<&str>(),
        )
        .unwrap(),
        SOURCE,
    );
    let needed = || {
        VueHostCompileRefusal::NeedInputs(vec![InputDemand::ExternalStyleStage {
            style_index: 1,
            stage: StyleStage::Preprocessed,
        }])
    };
    let len = SOURCE.len() as u32;
    let assert_names_the_stage = |snapshot: &DiagnosticsSnapshot| {
        assert!(snapshot.has_errors);
        assert_eq!(snapshot.diagnostics.len(), 1);
        let diagnostic = &snapshot.diagnostics[0];
        assert_eq!(diagnostic.code, "HOST_BLOCK_CONTENT_RUNTIME_UNAVAILABLE");
        assert!(
            diagnostic.message.contains("style block 1")
                && diagnostic.message.contains("preprocessed stage")
                && diagnostic.message.contains("/src/Comp.vue"),
            "the refusal names the block, the stage and the source, got {}",
            diagnostic.message
        );
    };

    let HostProductsFailure::Fatal(snapshot) =
        vue_products_execution_failure(artifact.as_ref(), "/src/Comp.vue", len, needed())
    else {
        panic!("needed inputs are a refused compile, not a refused runtime surface");
    };
    assert_names_the_stage(&snapshot);

    let RenderExecutionRefusal::Fatal(snapshot) =
        vue_render_execution_refusal(artifact.as_ref(), "/src/Comp.vue", len, needed())
    else {
        panic!("needed inputs are a refused compile on the render lane too");
    };
    assert_names_the_stage(&snapshot);
}

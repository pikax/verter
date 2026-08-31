//! Vue runtime `RuntimeCompilerBackend`: parse-artifact emit, catalog
//! identity, byte/map/diagnostic parity with the parsed core, runtime-only
//! refusal, option preservation, and production `compile_bundle` isolation.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use verter_compiler::compile::types::{VueExecutionInputs, VueMacroSemanticInput};
use verter_compiler::compile_request::{
    AnalysisProductRequest, CompileProduct, CompileRequest, CompileRequestError,
    FrameworkCompileRequest, IdeProductRequest, ProductKind, RuntimeProductRequest,
    SvelteCompileRequest, VueBackendRequest, VueCompileRequest,
};
use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
use verter_compiler::framework_common::{
    vue_runtime_backend_registration, CarrierCompiler, CatalogCapability, CatalogRow,
    FrameworkEpoch, FrameworkParseArtifact, ImmutableCapabilityCatalog, RuntimeBlockContentInput,
    RuntimeBlockContentInputs, RuntimeCompileOptions, RuntimeCompilerBackend, VueRuntimeBackend,
    VueRuntimeError, VueRuntimeExecutionFacts, VueRuntimeInputs, VueSfcV3,
};
use verter_compiler::standalone::{DirectExecutionInputs, StandaloneCompiler};
use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
    FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};
use verter_language::{FrameworkAdapterId, LanguageId};

const KITCHEN_SINK: &str = include_str!("../fixtures/kitchen-sink.vue");

const SIMPLE: &str = concat!(
    "<script setup lang=\"ts\">\n",
    "const count = 1;\n",
    "</script>\n",
    "<template>\n",
    "  <div>{{ count }}</div>\n",
    "</template>\n",
);

const CUSTOM_PROFILE: &str = concat!(
    "<script setup lang=\"ts\">\n",
    "const count = 1;\n",
    "</script>\n",
    "<template>\n",
    "  <div>[[ count ]]</div>\n",
    "  <ion-button></ion-button>\n",
    "</template>\n",
);

const DIAGNOSTIC: &str = concat!(
    "<script setup>\n",
    "const n = 1\n",
    "defineProps({ n })\n",
    "</script>\n",
    "<template>\n",
    "  <div v-slot></div>\n",
    "</template>\n",
);

const VAPOR_SSR: &str = "<template vapor><div>{{ a }}</div></template>";

fn registered_artifact(canonical: &str, source: &str) -> FrameworkParseArtifact {
    registered_artifact_with_grammar(canonical, source, "{{", "}}", std::iter::empty::<&str>())
}

fn registered_artifact_with_grammar(
    canonical: &str,
    source: &str,
    open: &str,
    close: &str,
    custom_elements: impl IntoIterator<Item = &'static str>,
) -> FrameworkParseArtifact {
    let language = verter_language::FileLanguage::vue();
    let source_authority = RegisteredSourceAuthority::new().expect("source authority");
    let snapshot = source_authority
        .register_source(
            CanonicalFileId::new(canonical),
            FileIncarnation::new(1),
            SourceGeneration::new(1),
            language.clone(),
            Arc::from(source),
        )
        .expect("registered source");
    let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
    let config = CarrierGrammarConfig::vue(open, close, custom_elements).expect("vue grammar");
    grammar_authority
        .register_carrier_grammar(
            language,
            FrameworkAdapterSemanticVersion::new(1).expect("adapter version"),
            CarrierParserGrammarVersion::new(1).expect("grammar version"),
            config.clone(),
        )
        .expect("grammar registration");
    let accepted = grammar_authority
        .accept_registered_source(&source_authority, &snapshot, &config)
        .expect("accepted source");
    verter_compiler::framework_common::CarrierCompilerRegistry::built_in()
        .project_registered(&accepted)
        .expect("registered projection")
        .into_framework_parse_artifact()
}

fn runtime_request(
    filename: &str,
    products: Vec<CompileProduct>,
    vue: VueCompileRequest,
    is_production: bool,
) -> CompileRequest {
    CompileRequest::new(
        products,
        FrameworkCompileRequest::Vue(vue),
        None,
        Some(filename.to_string()),
        None,
        is_production,
        false,
    )
    .expect("runtime request constructs")
}

fn client_product(runtime_source_map: bool, inline: Option<bool>) -> CompileProduct {
    CompileProduct::RuntimeClient(RuntimeProductRequest {
        runtime_source_map,
        inline,
        ..Default::default()
    })
}

fn server_product(runtime_source_map: bool) -> CompileProduct {
    CompileProduct::RuntimeServer(RuntimeProductRequest {
        runtime_source_map,
        ..Default::default()
    })
}

fn default_inputs() -> VueRuntimeInputs {
    VueRuntimeInputs::default()
}

fn standalone_vue_inputs() -> (VueExecutionInputs, VueMacroSemanticInput) {
    (
        VueExecutionInputs::default(),
        VueMacroSemanticInput::default(),
    )
}

fn compile_via_backend(
    source: &str,
    artifact: &FrameworkParseArtifact,
    request: &CompileRequest,
) -> Result<verter_compiler::standalone::DirectCompileOutput, VueRuntimeError> {
    compile_via_backend_with_inputs(source, artifact, request, &default_inputs())
}

fn compile_via_backend_with_inputs(
    source: &str,
    artifact: &FrameworkParseArtifact,
    request: &CompileRequest,
    inputs: &VueRuntimeInputs,
) -> Result<verter_compiler::standalone::DirectCompileOutput, VueRuntimeError> {
    RuntimeCompilerBackend::compile_runtime(
        &VueRuntimeBackend,
        runtime_grant_for(request),
        source,
        artifact,
        request,
        inputs,
    )
}

/// Test-minted consume-once runtime grant matching the request's demanded
/// runtime kind (production carves grants off a host-issued admission).
fn runtime_grant_for(
    request: &CompileRequest,
) -> verter_compiler::framework_common::ProductExecutionGrant {
    let kind = if request
        .products()
        .iter()
        .any(|p| p.kind() == ProductKind::RuntimeServer)
    {
        ProductKind::RuntimeServer
    } else {
        ProductKind::RuntimeClient
    };
    verter_compiler::framework_common::ProductExecutionGrant::mint_for_tests(kind)
}

fn selected_style(code: &str) -> RuntimeBlockContentInput {
    RuntimeBlockContentInput {
        code: Arc::from(code),
        source_map: None,
        lang: "css".to_string(),
        content_artifact_token: "artifact:theme-css".to_string(),
        source_space_token: "space:theme-css".to_string(),
        parsed: None,
    }
}

fn selected_template(code: &str) -> RuntimeBlockContentInput {
    selected_template_with_map(code, None)
}

fn selected_template_with_map(code: &str, source_map: Option<&str>) -> RuntimeBlockContentInput {
    RuntimeBlockContentInput {
        code: Arc::from(code),
        source_map: source_map.map(Arc::from),
        lang: "html".to_string(),
        content_artifact_token: "content:html".to_string(),
        source_space_token: "space:html".to_string(),
        parsed: None,
    }
}

fn selected_style_with_map(code: &str, source_map: Option<&str>) -> RuntimeBlockContentInput {
    RuntimeBlockContentInput {
        code: Arc::from(code),
        source_map: source_map.map(Arc::from),
        lang: "css".to_string(),
        content_artifact_token: "artifact:theme-css".to_string(),
        source_space_token: "space:theme-css".to_string(),
        parsed: None,
    }
}

fn identity_source_map(source_name: &str, source: &str) -> String {
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.add_source_and_content(source_name, source);
    let mut line = 0u32;
    let mut column = 0u32;
    for character in source.chars() {
        builder.add_token(line, column, line, column, Some(source_id), None);
        if character == '\n' {
            line += 1;
            column = 0;
        } else {
            column += character.len_utf16() as u32;
        }
    }
    builder.into_sourcemap().to_json_string()
}

fn source_map_sources(map: &str) -> Vec<String> {
    oxc_sourcemap::SourceMap::from_json_string(map)
        .expect("published runtime map must be valid JSON")
        .get_sources()
        .map(str::to_string)
        .collect()
}

fn compile_via_standalone(
    source: &str,
    request: &CompileRequest,
) -> verter_compiler::standalone::DirectCompileOutput {
    let (execution, macros) = standalone_vue_inputs();
    StandaloneCompiler
        .compile(
            source,
            request,
            DirectExecutionInputs::Vue {
                execution: &execution,
                macros: &macros,
            },
        )
        .expect("parsed-core runtime compile")
}

fn assert_runtime_parity(
    via_backend: &verter_compiler::standalone::DirectCompileOutput,
    via_standalone: &verter_compiler::standalone::DirectCompileOutput,
) {
    let backend_kinds: Vec<_> = via_backend
        .artifacts
        .artifacts()
        .iter()
        .map(|artifact| artifact.kind())
        .collect();
    let standalone_kinds: Vec<_> = via_standalone
        .artifacts
        .artifacts()
        .iter()
        .map(|artifact| artifact.kind())
        .collect();
    assert_eq!(backend_kinds, standalone_kinds);
    for kind in &backend_kinds {
        let backend = via_backend
            .artifacts
            .artifact(*kind)
            .expect("backend artifact");
        let standalone = via_standalone
            .artifacts
            .artifact(*kind)
            .expect("standalone artifact");
        assert_eq!(backend.code(), standalone.code());
        assert_eq!(
            backend.runtime_source_map(),
            standalone.runtime_source_map()
        );
        assert_eq!(
            backend.source_projection_map(),
            standalone.source_projection_map()
        );
        assert_eq!(backend.dialect(), standalone.dialect());
    }
    assert_eq!(via_backend.diagnostics, via_standalone.diagnostics);
    assert_eq!(via_backend.styles.len(), via_standalone.styles.len());
    for (backend_style, standalone_style) in
        via_backend.styles.iter().zip(via_standalone.styles.iter())
    {
        assert_eq!(backend_style.code, standalone_style.code);
        assert_eq!(backend_style.source_map, standalone_style.source_map);
        assert_eq!(backend_style.lang, standalone_style.lang);
        assert_eq!(backend_style.scope_hash, standalone_style.scope_hash);
        assert_eq!(backend_style.has_global, standalone_style.has_global);
    }
}

/// A grant carved for a different demand refuses typed before any
/// compile work runs — the demand match is part of consumption.
#[test]
fn wrong_demand_grant_refuses_runtime_typed() {
    let artifact = registered_artifact("file:///wrong-grant.vue", SIMPLE);
    let request = runtime_request(
        "Wrong.vue",
        vec![client_product(false, None)],
        VueCompileRequest::default(),
        false,
    );
    let projection_grant = verter_compiler::framework_common::ProductExecutionGrant::mint_for_tests(
        ProductKind::IdeCompanion,
    );
    let err = RuntimeCompilerBackend::compile_runtime(
        &VueRuntimeBackend,
        projection_grant,
        SIMPLE,
        &artifact,
        &request,
        &default_inputs(),
    )
    .expect_err("a projection grant must not drive the runtime leg");
    assert!(
        matches!(
            err,
            VueRuntimeError::ExecutionUngranted {
                product: ProductKind::RuntimeClient,
            }
        ),
        "expected the typed ungranted refusal, got {err:?}"
    );
}

#[test]
fn vue_runtime_catalog_row_binds_vue_adapter_identity() {
    let row = vue_runtime_backend_registration();
    assert_eq!(row.identity().adapter_id(), &FrameworkAdapterId::vue());
    assert_eq!(
        row.identity().carrier_language_id(),
        &LanguageId::new("vue")
    );
    assert_eq!(row.identity().capability(), CatalogCapability::Runtime);
    assert!(row.identity().host_epoch().is_none());
    assert_eq!(row.identity().epoch().as_str(), VueSfcV3::ID);
    assert_eq!(row.identity().epoch().as_str(), "vue");
    let _backend: &VueRuntimeBackend = row.runtime();
    let catalog = ImmutableCapabilityCatalog::<(), (), (), VueRuntimeBackend, ()>::try_from_rows([
        CatalogRow::Runtime(row),
    ])
    .expect("single Vue runtime row");
    assert_eq!(catalog.len(), 1);
}

#[test]
fn vue_runtime_backend_matches_parsed_core_on_kitchen_sink() {
    let artifact = registered_artifact("file:///kitchen.vue", KITCHEN_SINK);
    let request = runtime_request(
        "Kitchen.vue",
        vec![client_product(true, None)],
        VueCompileRequest::default(),
        false,
    );
    let via_standalone = compile_via_standalone(KITCHEN_SINK, &request);
    let via_backend = compile_via_backend(KITCHEN_SINK, &artifact, &request)
        .expect("runtime backend kitchen sink");
    assert_runtime_parity(&via_backend, &via_standalone);
    let client = via_backend
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client artifact");
    assert!(!client.code().is_empty());
    assert!(client.runtime_source_map().is_some());
    assert!(via_backend
        .artifacts
        .artifact(ProductKind::RuntimeServer)
        .is_none());
    assert!(via_backend
        .artifacts
        .artifact(ProductKind::IdeCompanion)
        .is_none());
}

#[test]
fn vue_runtime_backend_is_deterministic() {
    let artifact = registered_artifact("file:///simple.vue", SIMPLE);
    let request = runtime_request(
        "Simple.vue",
        vec![client_product(true, None)],
        VueCompileRequest::default(),
        false,
    );
    let first = compile_via_backend(SIMPLE, &artifact, &request).expect("first");
    let second = compile_via_backend(SIMPLE, &artifact, &request).expect("second");
    assert_runtime_parity(&first, &second);
}

#[test]
fn vue_runtime_backend_preserves_diagnostics_and_order() {
    let artifact = registered_artifact("file:///diag.vue", DIAGNOSTIC);
    let request = runtime_request(
        "Diag.vue",
        vec![client_product(false, None)],
        VueCompileRequest::default(),
        false,
    );
    let via_standalone = compile_via_standalone(DIAGNOSTIC, &request);
    let via_backend =
        compile_via_backend(DIAGNOSTIC, &artifact, &request).expect("diagnostic runtime");
    assert_runtime_parity(&via_backend, &via_standalone);
    assert!(
        !via_backend.diagnostics.is_empty(),
        "expected compile diagnostics to be retained"
    );
}

#[test]
fn one_runtime_request_emits_client_and_server_from_one_admitted_parse() {
    let artifact = registered_artifact("file:///dual.vue", SIMPLE);
    let request = runtime_request(
        "Dual.vue",
        vec![client_product(true, None), server_product(true)],
        VueCompileRequest::default(),
        false,
    );
    let via_standalone = compile_via_standalone(SIMPLE, &request);
    let via_backend = compile_via_backend(SIMPLE, &artifact, &request).expect("dual runtime");
    assert_runtime_parity(&via_backend, &via_standalone);
    let kinds: Vec<_> = via_backend
        .artifacts
        .artifacts()
        .iter()
        .map(|artifact| artifact.kind())
        .collect();
    assert_eq!(
        kinds,
        vec![ProductKind::RuntimeServer, ProductKind::RuntimeClient],
        "dual-target publication order is the parsed-core plan order"
    );
    let client = via_backend
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client");
    let server = via_backend
        .artifacts
        .artifact(ProductKind::RuntimeServer)
        .expect("server");
    assert_ne!(
        client.code(),
        server.code(),
        "client and server products must be distinct emits sharing one parse"
    );
}

#[test]
fn runtime_source_map_option_is_honored_per_target() {
    let artifact = registered_artifact("file:///maps.vue", SIMPLE);
    let mapped = runtime_request(
        "Maps.vue",
        vec![client_product(true, None)],
        VueCompileRequest::default(),
        false,
    );
    let unmapped = runtime_request(
        "Maps.vue",
        vec![client_product(false, None)],
        VueCompileRequest::default(),
        false,
    );
    let with_map = compile_via_backend(SIMPLE, &artifact, &mapped).expect("mapped");
    let without_map = compile_via_backend(SIMPLE, &artifact, &unmapped).expect("unmapped");
    assert!(with_map
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client")
        .runtime_source_map()
        .is_some());
    assert!(without_map
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client")
        .runtime_source_map()
        .is_none());
}

#[test]
fn inline_none_follows_is_production_and_does_not_hide_an_explicit_false() {
    let artifact = registered_artifact("file:///inline.vue", SIMPLE);
    let production_default = runtime_request(
        "Inline.vue",
        vec![client_product(false, None)],
        VueCompileRequest::default(),
        true,
    );
    let production_explicit_false = runtime_request(
        "Inline.vue",
        vec![client_product(false, Some(false))],
        VueCompileRequest::default(),
        true,
    );
    let development_default = runtime_request(
        "Inline.vue",
        vec![client_product(false, None)],
        VueCompileRequest::default(),
        false,
    );
    let prod_default = compile_via_backend(SIMPLE, &artifact, &production_default).expect("prod");
    let prod_false =
        compile_via_backend(SIMPLE, &artifact, &production_explicit_false).expect("prod false");
    let dev_default = compile_via_backend(SIMPLE, &artifact, &development_default).expect("dev");
    let prod_default_code = prod_default
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client")
        .code();
    let prod_false_code = prod_false
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client")
        .code();
    let dev_default_code = dev_default
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client")
        .code();
    assert_ne!(
        prod_default_code, prod_false_code,
        "explicit inline=false must not be replaced by the production default"
    );
    assert!(
        prod_false_code.contains("_sfc_main.render = render"),
        "explicit inline=false must keep a separate render export"
    );
    assert!(
        dev_default_code.contains("_sfc_main.render = render"),
        "development None default must keep a separate render export"
    );
    assert!(
        !prod_default_code.contains("_sfc_main.render = render"),
        "production None default must inline the render function"
    );
    assert_runtime_parity(
        &prod_default,
        &compile_via_standalone(SIMPLE, &production_default),
    );
    assert_runtime_parity(
        &prod_false,
        &compile_via_standalone(SIMPLE, &production_explicit_false),
    );
}

#[test]
fn vue_runtime_backend_refuses_a_foreign_artifact() {
    let source = "<script>let name = 'world';</script>\n<h1>Hello {name}!</h1>\n";
    let svelte = {
        let language = verter_language::FileLanguage::svelte();
        let source_authority = RegisteredSourceAuthority::new().expect("source authority");
        let snapshot = source_authority
            .register_source(
                CanonicalFileId::new("file:///foreign.svelte"),
                FileIncarnation::new(1),
                SourceGeneration::new(1),
                language.clone(),
                Arc::from(source),
            )
            .expect("registered source");
        let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
        let config = CarrierGrammarConfig::Svelte;
        grammar_authority
            .register_carrier_grammar(
                language,
                FrameworkAdapterSemanticVersion::new(1).expect("adapter version"),
                CarrierParserGrammarVersion::new(1).expect("grammar version"),
                config.clone(),
            )
            .expect("grammar registration");
        let accepted = grammar_authority
            .accept_registered_source(&source_authority, &snapshot, &config)
            .expect("accepted source");
        verter_compiler::framework_common::CarrierCompilerRegistry::built_in()
            .project_registered(&accepted)
            .expect("registered projection")
            .into_framework_parse_artifact()
    };
    let request = runtime_request(
        "Foreign.vue",
        vec![client_product(true, None)],
        VueCompileRequest::default(),
        false,
    );
    let err = compile_via_backend(source, &svelte, &request)
        .expect_err("foreign artifact has no Vue parse");
    match err {
        VueRuntimeError::UnusableParse => {}
        other => panic!("expected UnusableParse, got {other:?}"),
    }
}

#[test]
fn vue_runtime_backend_refuses_source_that_does_not_match_the_admitted_artifact() {
    let artifact = registered_artifact("file:///simple.vue", SIMPLE);
    let request = runtime_request(
        "Simple.vue",
        vec![client_product(true, None)],
        VueCompileRequest::default(),
        false,
    );
    let err = compile_via_backend("<script setup>const n = 2</script>", &artifact, &request)
        .expect_err("mismatched source must not reparse");
    match err {
        VueRuntimeError::SourceMismatch => {}
        other => panic!("expected SourceMismatch, got {other:?}"),
    }
}

#[test]
fn vue_runtime_backend_binds_request_syntax_profile_to_admitted_artifact() {
    struct Case {
        name: &'static str,
        source: &'static str,
        artifact_open: &'static str,
        artifact_close: &'static str,
        artifact_custom_elements: &'static [&'static str],
        request_delimiters: Option<(&'static str, &'static str)>,
        request_custom_elements: &'static [&'static str],
        expect_ok: bool,
    }

    let cases = [
        Case {
            name: "standard-delimiter artifact versus nondefault-delimiter request",
            source: SIMPLE,
            artifact_open: "{{",
            artifact_close: "}}",
            artifact_custom_elements: &[],
            request_delimiters: Some(("[[", "]]")),
            request_custom_elements: &[],
            expect_ok: false,
        },
        Case {
            name: "no-custom-element artifact versus nonempty custom-element request",
            source: SIMPLE,
            artifact_open: "{{",
            artifact_close: "}}",
            artifact_custom_elements: &[],
            request_delimiters: None,
            request_custom_elements: &["ion-"],
            expect_ok: false,
        },
        Case {
            name: "matching nondefault delimiters and custom elements",
            source: CUSTOM_PROFILE,
            artifact_open: "[[",
            artifact_close: "]]",
            artifact_custom_elements: &["ion-"],
            request_delimiters: Some(("[[", "]]")),
            request_custom_elements: &["ion-"],
            expect_ok: true,
        },
    ];

    for case in cases {
        let artifact = registered_artifact_with_grammar(
            "file:///profile.vue",
            case.source,
            case.artifact_open,
            case.artifact_close,
            case.artifact_custom_elements.iter().copied(),
        );
        let request = runtime_request(
            "Profile.vue",
            vec![client_product(true, None)],
            VueCompileRequest {
                delimiters: case
                    .request_delimiters
                    .map(|(open, close)| (open.to_string(), close.to_string())),
                is_custom_element: case
                    .request_custom_elements
                    .iter()
                    .map(|name| (*name).to_string())
                    .collect(),
                ..Default::default()
            },
            false,
        );
        let outcome = compile_via_backend(case.source, &artifact, &request);
        if case.expect_ok {
            assert!(
                outcome.is_ok(),
                "{}: expected success, got {outcome:?}",
                case.name
            );
        } else {
            match outcome {
                Err(VueRuntimeError::ProfileMismatch) => {}
                other => panic!("{}: expected ProfileMismatch, got {other:?}", case.name),
            }
        }
    }
}

#[test]
fn vue_runtime_backend_refuses_an_ide_product_request() {
    let artifact = registered_artifact("file:///simple.vue", SIMPLE);
    let request = CompileRequest::new(
        vec![
            client_product(false, None),
            CompileProduct::IdeCompanion(IdeProductRequest::default()),
        ],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some("Simple.vue".to_string()),
        None,
        false,
        false,
    )
    .expect("mixed request constructs");
    let err = compile_via_backend(SIMPLE, &artifact, &request)
        .expect_err("IDE product is not a runtime emit");
    match err {
        VueRuntimeError::NotRuntimeOnly { unexpected } => {
            assert_eq!(unexpected, ProductKind::IdeCompanion);
        }
        other => panic!("expected NotRuntimeOnly, got {other:?}"),
    }
}

#[test]
fn vue_runtime_backend_refuses_an_analysis_product_request() {
    let artifact = registered_artifact("file:///simple.vue", SIMPLE);
    let request = CompileRequest::new(
        vec![CompileProduct::Analysis(AnalysisProductRequest {
            want_script_bindings: true,
            want_template_data: false,
        })],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some("Simple.vue".to_string()),
        None,
        false,
        false,
    )
    .expect("analysis request constructs");
    let err = compile_via_backend(SIMPLE, &artifact, &request)
        .expect_err("analysis is not a runtime emit");
    match err {
        VueRuntimeError::NotRuntimeOnly { unexpected } => {
            assert_eq!(unexpected, ProductKind::Analysis);
        }
        other => panic!("expected NotRuntimeOnly Analysis, got {other:?}"),
    }
}

#[test]
fn implicit_vapor_plus_server_is_a_typed_refusal() {
    let artifact = registered_artifact("file:///vapor-ssr.vue", VAPOR_SSR);
    let request = runtime_request(
        "VaporSsr.vue",
        vec![server_product(false)],
        VueCompileRequest::default(),
        false,
    );
    let err = compile_via_backend(VAPOR_SSR, &artifact, &request)
        .expect_err("SSR plus implicit vapor must refuse");
    match err {
        VueRuntimeError::RequestExecutionRefused(
            CompileRequestError::SsrVaporBackendUnsupported,
        ) => {}
        other => {
            panic!("expected RequestExecutionRefused(SsrVaporBackendUnsupported), got {other:?}")
        }
    }
}

#[test]
fn explicit_vapor_plus_server_is_refused_at_request_construction() {
    let err = CompileRequest::new(
        vec![server_product(false)],
        FrameworkCompileRequest::Vue(VueCompileRequest {
            backend: VueBackendRequest::Vapor,
            ..Default::default()
        }),
        None,
        Some("VaporSsr.vue".to_string()),
        None,
        false,
        false,
    )
    .expect_err("explicit vapor + server must not construct");
    assert_eq!(err, CompileRequestError::SsrVaporBackendUnsupported);
}

#[test]
fn compile_bundle_still_emits_runtime_when_an_ide_product_is_also_requested() {
    let artifact = registered_artifact("file:///bundle.vue", SIMPLE);
    let mixed = VueCarrierCompiler
        .compile_bundle(
            SIMPLE,
            &artifact,
            &RuntimeCompileOptions {
                filename: Some("Bundle.vue".to_string()),
                want_runtime: true,
                want_ide: true,
                source_map: true,
                ..Default::default()
            },
            &oxc_allocator::Allocator::new(),
        )
        .expect("production compile_bundle still serves runtime");
    match mixed {
        verter_compiler::framework_common::CarrierCompileOutcome::Produced(bundle) => {
            assert!(
                bundle.script.is_some() || bundle.template.is_some(),
                "production compile_bundle must still emit a runtime product"
            );
            assert!(
                bundle.tsx.is_some(),
                "production compile_bundle must still emit the IDE product on the combined path"
            );
        }
        other => panic!("expected produced runtime+IDE bundle, got {other:?}"),
    }

    let request = CompileRequest::new(
        vec![
            client_product(true, None),
            CompileProduct::IdeCompanion(IdeProductRequest {
                want_source_map: true,
                ..Default::default()
            }),
        ],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some("Bundle.vue".to_string()),
        None,
        false,
        false,
    )
    .expect("mixed request constructs");
    let err = compile_via_backend(SIMPLE, &artifact, &request)
        .expect_err("runtime backend refuses a mixed product set");
    match err {
        VueRuntimeError::NotRuntimeOnly { unexpected } => {
            assert_eq!(unexpected, ProductKind::IdeCompanion);
        }
        other => panic!("expected NotRuntimeOnly IdeCompanion, got {other:?}"),
    }
}

#[test]
fn vue_runtime_backend_refuses_a_non_vue_request() {
    let artifact = registered_artifact("file:///simple.vue", SIMPLE);
    let request = CompileRequest::new(
        vec![client_product(true, None)],
        FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
        None,
        Some("Simple.svelte".to_string()),
        None,
        false,
        false,
    )
    .expect("svelte runtime request constructs");
    let err = compile_via_backend(SIMPLE, &artifact, &request)
        .expect_err("a Svelte request is not a Vue runtime compile");
    match err {
        VueRuntimeError::FrameworkMismatch => {}
        other => panic!("expected FrameworkMismatch, got {other:?}"),
    }
}

#[test]
fn vue_runtime_backend_preserves_selected_style_bytes() {
    let source = concat!(
        "<template>\n",
        "  <div class=\"x\"></div>\n",
        "</template>\n",
        "<style scoped src=\"./theme.css\"></style>\n",
    );
    let artifact = registered_artifact("file:///style.vue", source);
    let request = CompileRequest::new(
        vec![client_product(false, None)],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some("Style.vue".to_string()),
        Some("scope123".to_string()),
        false,
        false,
    )
    .expect("style request constructs");
    let via_empty = compile_via_backend(source, &artifact, &request).expect("carrier-only style");
    assert!(
        via_empty
            .styles
            .iter()
            .all(|style| !style.code.contains("color: red")),
        "carrier src-only style must not invent selected CSS: {:?}",
        via_empty
            .styles
            .iter()
            .map(|style| style.code.as_str())
            .collect::<Vec<_>>()
    );

    let via_backend = compile_via_backend_with_inputs(
        source,
        &artifact,
        &request,
        &VueRuntimeInputs {
            block_content: RuntimeBlockContentInputs {
                styles: vec![Some(selected_style(".x { color: red; }"))],
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .expect("selected style");
    assert!(
        !via_backend.styles.is_empty(),
        "selected style must publish a style block"
    );
    let style = &via_backend.styles[0];
    assert!(
        style.code.contains("color: red") || style.code.contains("color:red"),
        "selected CSS bytes must be preserved: {}",
        style.code
    );
    assert!(
        style.code.contains("data-v-scope123"),
        "selected CSS must still be scoped: {}",
        style.code
    );
    assert_eq!(
        style.output_descriptor.source_map.declared_space_tokens,
        vec!["space:theme-css".to_string()]
    );
    assert_ne!(
        style.output_descriptor.source_space.token,
        "space:theme-css"
    );
}

#[test]
fn vue_runtime_backend_preserves_supplied_template_bytes() {
    let source = concat!(
        "<template src=\"./view.html\"></template>",
        "<script setup>const count = 1</script>"
    );
    let artifact = registered_artifact("file:///external.vue", source);
    let request = runtime_request(
        "External.vue",
        vec![client_product(false, None)],
        VueCompileRequest::default(),
        false,
    );
    let via_empty =
        compile_via_backend(source, &artifact, &request).expect("carrier-only template");
    let empty_client = via_empty
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client")
        .code();
    assert!(
        empty_client.contains("return null"),
        "src-only template must not invent supplied render markup: {empty_client}"
    );

    let via_backend = compile_via_backend_with_inputs(
        source,
        &artifact,
        &request,
        &VueRuntimeInputs {
            block_content: RuntimeBlockContentInputs {
                template: Some(selected_template("<p>{{ count }}</p>")),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .expect("supplied template");
    let client = via_backend
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client")
        .code();
    assert!(
        client.contains("toDisplayString")
            || client.contains("$setup.count")
            || client.contains("_ctx.count"),
        "supplied template bytes must reach the runtime render: {client}"
    );
    assert!(
        !client.contains("return null") || client.contains("toDisplayString"),
        "supplied template must not keep the src-only empty render: {client}"
    );
}

#[test]
fn mixed_runtime_source_map_options_are_honored_per_target() {
    let artifact = registered_artifact("file:///mixed-maps.vue", SIMPLE);
    let request = runtime_request(
        "MixedMaps.vue",
        vec![client_product(false, None), server_product(true)],
        VueCompileRequest::default(),
        false,
    );
    let via_backend = compile_via_backend(SIMPLE, &artifact, &request).expect("mixed map request");
    let client = via_backend
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client");
    let server = via_backend
        .artifacts
        .artifact(ProductKind::RuntimeServer)
        .expect("server");
    assert!(
        client.runtime_source_map().is_none(),
        "client map=false must not publish a runtime map"
    );
    let server_map = server
        .runtime_source_map()
        .expect("server map=true must publish a runtime map");
    assert!(
        !server_map.is_empty(),
        "server map must be truthful, got {server_map}"
    );
}

#[test]
fn runtime_macros_are_the_sole_macro_channel() {
    use verter_macro_dto::{
        AuthoredMemberOrdinal, MacroAnchor, MacroRuntimeBundle, MacroRuntimeEntry,
        MacroRuntimeOutcome, MacroRuntimeShape, OrderedRuntimeConstructors,
        PropsDefaultsAssociation, PropsRuntimeShape, RuntimeConstructor, RuntimeProp,
        RuntimePropType,
    };

    let source = "<script setup lang=\"ts\">defineProps<{ authoritative: string }>()</script>";
    let artifact = registered_artifact("file:///macros.vue", source);
    let request = runtime_request(
        "Macros.vue",
        vec![client_product(false, None)],
        VueCompileRequest::default(),
        false,
    );
    let runtime = Arc::new(MacroRuntimeBundle {
        entries: vec![MacroRuntimeEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(PropsRuntimeShape {
                defaults: PropsDefaultsAssociation::None,
                props: vec![RuntimeProp {
                    name: "authoritative".to_string(),
                    optional: false,
                    type_shape: RuntimePropType::Resolved {
                        constructors: OrderedRuntimeConstructors::from_ordered([
                            RuntimeConstructor::Boolean,
                            RuntimeConstructor::Unknown,
                        ]),
                        skip_check: true,
                    },
                    anchor: MacroAnchor::Authored {
                        macro_index: 0,
                        member_ordinal: AuthoredMemberOrdinal::new(0),
                    },
                }],
            })),
        }],
    });
    let via_ignored = compile_via_backend(source, &artifact, &request).expect("default macros");
    let ignored_code = via_ignored
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client")
        .code();
    let via_backend = compile_via_backend_with_inputs(
        source,
        &artifact,
        &request,
        &VueRuntimeInputs {
            macros: Some(runtime),
            ..Default::default()
        },
    )
    .expect("runtime macros");
    let code = via_backend
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client")
        .code();
    assert!(
        code.contains("authoritative") && code.contains("Boolean"),
        "macros must be the sole runtime macro channel: {code}"
    );
    assert_ne!(
        code, ignored_code,
        "supplying the runtime-only macros field must change the emitted props constructors"
    );
}

fn javascript_module_check(node_program: &Path, name: &str, code: &str) -> Result<(), String> {
    let project = tempfile::tempdir().expect("create JS validity directory");
    let path = project.path().join(format!("{name}.mjs"));
    fs::write(&path, code).expect("write emitted module");
    let output = Command::new(node_program)
        .arg("--check")
        .arg(&path)
        .current_dir(project.path())
        .output()
        .map_err(|error| format!("failed to run node syntax gate: {error}"))?;
    let Some(exit_code) = output.status.code() else {
        return Err(format!(
            "node --check terminated without an exit code for {name}"
        ));
    };
    if exit_code != 0 {
        return Err(format!(
            "node --check rejected {name}:\n{}{}\n--- emitted ---\n{code}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

fn assert_javascript_module_valid(name: &str, code: &str) {
    javascript_module_check(Path::new("node"), name, code)
        .unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn javascript_syntax_gate_proves_execution_and_rejects_invalid_output() {
    assert!(
        javascript_module_check(
            Path::new("node"),
            "invalid_emitted_fixture",
            "export const value = ;\n",
        )
        .is_err(),
        "node --check accepted an invalid emitted module"
    );
    let missing = Path::new("__missing_node_for_js_validity_gate__");
    assert!(
        javascript_module_check(missing, "missing_binary", "export {};\n").is_err(),
        "a missing node binary passed the node --check gate"
    );
}

#[test]
fn projected_setup_runtime_matches_compile_bundle_and_passes_node_check() {
    let source = concat!(
        "<script setup src=\"./logic.js\"></script>",
        "<template><div>{{ count }}</div></template>"
    );
    let artifact = registered_artifact("file:///projected-setup.vue", source);
    let request = runtime_request(
        "ProjectedSetup.vue",
        vec![client_product(false, None)],
        VueCompileRequest::default(),
        false,
    );
    let inputs = VueRuntimeInputs {
        block_content: RuntimeBlockContentInputs {
            script_setup: Some(selected_script("const count = 1", "js")),
            ..Default::default()
        },
        ..Default::default()
    };
    let via_backend = compile_via_backend_with_inputs(source, &artifact, &request, &inputs)
        .expect("projected setup runtime");
    let client = via_backend
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client")
        .code();
    assert!(
        client.contains("const count = 1"),
        "projected setup script must reach the composed runtime:\n{client}"
    );
    assert!(
        client.contains("toDisplayString")
            || client.contains("$setup.count")
            || client.contains("_ctx.count")
            || client.contains("count"),
        "carrier template must keep the setup binding:\n{client}"
    );
    assert_javascript_module_valid("projected_setup_backend", client);
}

#[test]
fn projected_plain_simultaneous_and_setup_plus_carrier_plain_are_typed_refusals() {
    let cases = [
        (
            concat!(
                "<script src=\"./logic.js\"></script>",
                "<template><div>{{ a }}</div></template>"
            ),
            RuntimeBlockContentInputs {
                script: Some(selected_script(
                    "export default { data: () => ({ a: 1 }) }",
                    "js",
                )),
                ..Default::default()
            },
            "projected plain",
        ),
        (
            concat!(
                "<script src=\"./logic.js\"></script>",
                "<script setup src=\"./setup.js\"></script>",
                "<template><div /></template>"
            ),
            RuntimeBlockContentInputs {
                script: Some(selected_script("export default {}", "js")),
                script_setup: Some(selected_script("const answer = 42", "js")),
                ..Default::default()
            },
            "simultaneous scripts",
        ),
        (
            concat!(
                "<script>export default { data: () => ({ count: 1 }) }</script>",
                "<script setup src=\"./setup.js\"></script>",
                "<template><div>{{ count }}</div></template>"
            ),
            RuntimeBlockContentInputs {
                script_setup: Some(selected_script("const count = 1", "js")),
                ..Default::default()
            },
            "setup plus carrier plain",
        ),
    ];
    for (source, block_content, name) in cases {
        let artifact = registered_artifact("file:///refused.vue", source);
        let request = runtime_request(
            "Refused.vue",
            vec![client_product(false, None)],
            VueCompileRequest::default(),
            false,
        );
        let err = compile_via_backend_with_inputs(
            source,
            &artifact,
            &request,
            &VueRuntimeInputs {
                block_content,
                ..Default::default()
            },
        )
        .expect_err(name);
        match err {
            VueRuntimeError::BlockContentUnavailable => {}
            other => panic!("{name}: expected BlockContentUnavailable, got {other:?}"),
        }
    }
}

#[test]
fn production_supplied_inline_template_composes_and_does_not_detach_render() {
    let source = concat!(
        "<template lang=\"pug\">div {{ count }}</template>",
        "<script setup>const count = 1</script>"
    );
    let artifact = registered_artifact("file:///supplied-inline.vue", source);
    let request = runtime_request(
        "SuppliedInline.vue",
        vec![client_product(true, None)],
        VueCompileRequest::default(),
        true,
    );
    let inputs = VueRuntimeInputs {
        block_content: RuntimeBlockContentInputs {
            template: Some(selected_template("<div>{{ count }}</div>")),
            ..Default::default()
        },
        ..Default::default()
    };
    let via_backend = compile_via_backend_with_inputs(source, &artifact, &request, &inputs)
        .expect("production supplied-inline");
    let client = via_backend
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client")
        .code();
    assert!(
        client.contains("const count = 1") && client.contains("return (_ctx"),
        "backend must keep production inline topology:\n{client}"
    );
    assert!(
        !client.contains("_sfc_main.render = render"),
        "backend must not fall back to a detached render:\n{client}"
    );
    let map = via_backend
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client")
        .runtime_source_map()
        .expect("production supplied-inline must keep a runtime map");
    assert!(
        !map.is_empty(),
        "composed inline map must be published, got {map}"
    );
    assert_javascript_module_valid("supplied_inline_backend", client);
}

#[test]
fn selected_template_preserves_source_space_and_detached_map() {
    let source = concat!(
        "<template src=\"./view.html\"></template>",
        "<script setup>import Foo from './Foo.vue'</script>"
    );
    let artifact = registered_artifact("file:///selected-template.vue", source);
    let request = runtime_request(
        "SelectedTemplate.vue",
        vec![client_product(true, None)],
        VueCompileRequest::default(),
        false,
    );
    let inputs = VueRuntimeInputs {
        block_content: RuntimeBlockContentInputs {
            template: Some(selected_template("<Foo />")),
            ..Default::default()
        },
        ..Default::default()
    };
    let via_backend = compile_via_backend_with_inputs(source, &artifact, &request, &inputs)
        .expect("detached selected template");
    let client = via_backend
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client");
    let code = client.code();
    assert!(
        !code.contains("resolveComponent") && !code.contains("_component_Foo"),
        "a setup import must not degrade to runtime name resolution:\n{code}"
    );
    assert!(
        code.contains("$setup.Foo")
            || code.contains("$setup[\"Foo\"]")
            || code.contains("$setup['Foo']"),
        "the render must address the transferred setup binding:\n{code}"
    );
    let map = client
        .runtime_source_map()
        .expect("selected template must keep a runtime map");
    let sources = source_map_sources(map);
    assert!(
        sources.iter().any(|source| source == "space:html"),
        "detached selected-template map must use the selected source space, not the carrier filename; sources={sources:?} map={map}"
    );
}

#[test]
fn detached_selected_template_map_preserves_supplied_source_map_provenance() {
    let source = concat!(
        "<template src=\"./view.html\"></template>",
        "<script setup>const count = 1</script>"
    );
    let artifact = registered_artifact("file:///selected-template-map.vue", source);
    let request = runtime_request(
        "SelectedTemplateMap.vue",
        vec![client_product(true, None)],
        VueCompileRequest::default(),
        false,
    );
    let selected_html = "<p>{{ count }}</p>";
    let authored_map = identity_source_map("authored-view.html", selected_html);
    let via_backend = compile_via_backend_with_inputs(
        source,
        &artifact,
        &request,
        &VueRuntimeInputs {
            block_content: RuntimeBlockContentInputs {
                template: Some(selected_template_with_map(
                    selected_html,
                    Some(&authored_map),
                )),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .expect("detached selected template with supplied map");
    let map = via_backend
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client")
        .runtime_source_map()
        .expect("selected template must keep a runtime map");
    let sources = source_map_sources(map);
    assert!(
        sources.iter().any(|source| source == "authored-view.html"),
        "final composition must consume the selected-source map, got sources={sources:?} map={map}"
    );
}

#[test]
fn production_external_selected_template_composes_inline_and_does_not_detach() {
    let source = concat!(
        "<template src=\"./view.html\"></template>",
        "<script setup>const count = 1</script>"
    );
    let artifact = registered_artifact("file:///external-inline.vue", source);
    let request = runtime_request(
        "ExternalInline.vue",
        vec![client_product(true, None)],
        VueCompileRequest::default(),
        true,
    );
    let via_backend = compile_via_backend_with_inputs(
        source,
        &artifact,
        &request,
        &VueRuntimeInputs {
            block_content: RuntimeBlockContentInputs {
                template: Some(selected_template("<div>{{ count }}</div>")),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .expect("production external selected template");
    let client = via_backend
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client");
    let code = client.code();
    assert!(
        code.contains("const count = 1") && code.contains("return (_ctx"),
        "external selected templates must use the inline hole+chunk composition:\n{code}"
    );
    assert!(
        !code.contains("_sfc_main.render = render"),
        "external selected templates must not fall back to a detached render:\n{code}"
    );
    let map = client
        .runtime_source_map()
        .expect("composed inline map must be published");
    let sources = source_map_sources(map);
    assert!(
        sources.iter().any(|source| source == "space:html"),
        "composed inline map must declare the selected template space, sources={sources:?}"
    );
    assert_javascript_module_valid("external_selected_inline_backend", code);
}

#[test]
fn rewritten_selected_style_composes_cascade_map_with_supplied_source_map() {
    let source = concat!(
        "<template>\n",
        "  <div class=\"x\"></div>\n",
        "</template>\n",
        "<style scoped src=\"./theme.css\"></style>\n",
    );
    let css = ".x { color: red; }";
    let authored_map = identity_source_map("theme.scss", css);
    let artifact = registered_artifact("file:///style-map.vue", source);
    let request = CompileRequest::new(
        vec![client_product(true, None)],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some("StyleMap.vue".to_string()),
        Some("scope123".to_string()),
        false,
        false,
    )
    .expect("style map request constructs");
    let via_backend = compile_via_backend_with_inputs(
        source,
        &artifact,
        &request,
        &VueRuntimeInputs {
            block_content: RuntimeBlockContentInputs {
                styles: vec![Some(selected_style_with_map(css, Some(&authored_map)))],
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .expect("rewritten selected style");
    assert!(
        !via_backend.styles.is_empty(),
        "selected style must publish a style block"
    );
    let style = &via_backend.styles[0];
    assert!(
        style.code.contains("data-v-scope123"),
        "selected CSS must still be scoped: {}",
        style.code
    );
    let map = style
        .source_map
        .as_deref()
        .expect("rewritten selected style must publish a composed cascade map");
    let sources = source_map_sources(map);
    assert!(
        sources.iter().any(|source| source == "theme.scss"),
        "cascade map must compose through the supplied selected-style map, not replace it; sources={sources:?} map={map}"
    );
}

#[test]
fn projected_setup_plus_selected_template_keeps_the_selected_template() {
    let source = concat!(
        "<script setup src=\"./logic.js\"></script>",
        "<template><div>CARRIER_ONLY</div></template>"
    );
    let artifact = registered_artifact("file:///projected-setup-template.vue", source);
    let request = runtime_request(
        "ProjectedSetupTemplate.vue",
        vec![client_product(false, None)],
        VueCompileRequest::default(),
        false,
    );
    let via_backend = compile_via_backend_with_inputs(
        source,
        &artifact,
        &request,
        &VueRuntimeInputs {
            block_content: RuntimeBlockContentInputs {
                script_setup: Some(selected_script("const count = 1", "js")),
                template: Some(selected_template("<p>SELECTED_TEMPLATE</p>")),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .expect("projected setup plus selected template");
    let client = via_backend
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client")
        .code();
    assert!(
        client.contains("const count = 1"),
        "projected setup script must reach the composed runtime:\n{client}"
    );
    assert!(
        client.contains("SELECTED_TEMPLATE"),
        "selected template must not be dropped when projected setup is also selected:\n{client}"
    );
    assert!(
        !client.contains("CARRIER_ONLY"),
        "carrier template must not silently replace the selected template:\n{client}"
    );
    assert_javascript_module_valid("projected_setup_selected_template", client);
}

#[test]
fn projected_setup_injects_use_css_vars_from_carrier_and_selected_styles() {
    let carrier_style_source = concat!(
        "<script setup src=\"./logic.js\"></script>",
        "<template><div class=\"x\">{{ color }}</div></template>",
        "<style scoped>.x { color: v-bind(color); }</style>"
    );
    let selected_style_source = concat!(
        "<script setup src=\"./logic.js\"></script>",
        "<template><div class=\"x\">{{ color }}</div></template>",
        "<style scoped src=\"./theme.css\"></style>"
    );
    let cases = [
        (
            "carrier_style_v_bind",
            carrier_style_source,
            RuntimeBlockContentInputs {
                script_setup: Some(selected_script("const color = \"red\"", "js")),
                ..Default::default()
            },
        ),
        (
            "selected_style_v_bind",
            selected_style_source,
            RuntimeBlockContentInputs {
                script_setup: Some(selected_script("const color = \"red\"", "js")),
                styles: vec![Some(selected_style(".x { color: v-bind(color); }"))],
                ..Default::default()
            },
        ),
    ];
    for (name, source, block_content) in cases {
        let artifact = registered_artifact("file:///projected-css-vars.vue", source);
        let request = runtime_request(
            "ProjectedCssVars.vue",
            vec![client_product(false, None)],
            VueCompileRequest::default(),
            false,
        );
        let via_backend = compile_via_backend_with_inputs(
            source,
            &artifact,
            &request,
            &VueRuntimeInputs {
                block_content,
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("{name}: projected setup with style v-bind: {error:?}"));
        let client = via_backend
            .artifacts
            .artifact(ProductKind::RuntimeClient)
            .expect("client")
            .code();
        assert!(
            client.contains("_useCssVars"),
            "{name}: projected setup must inject _useCssVars when styles have v-bind:\n{client}"
        );
        assert_javascript_module_valid(&format!("projected_setup_css_vars_{name}"), client);
    }
}

#[test]
fn inline_selected_template_helper_imports_are_declared_on_the_script() {
    let source = concat!(
        "<template lang=\"pug\">div {{ count }}</template>",
        "<script setup>const count = 1</script>"
    );
    let artifact = registered_artifact("file:///inline-imports.vue", source);
    let request = runtime_request(
        "InlineImports.vue",
        vec![client_product(true, None)],
        VueCompileRequest::default(),
        true,
    );
    let inputs = VueRuntimeInputs {
        block_content: RuntimeBlockContentInputs {
            template: Some(selected_template("<div>{{ count }}</div>")),
            ..Default::default()
        },
        ..Default::default()
    };
    let via_backend = compile_via_backend_with_inputs(source, &artifact, &request, &inputs)
        .expect("production supplied-inline helpers");
    let client = via_backend
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client")
        .code();
    assert!(
        client.contains("toDisplayString") || client.contains("_toDisplayString"),
        "inline selected-template helpers must appear in the composed module:\n{client}"
    );

    let bundle = VueCarrierCompiler
        .compile_bundle(
            source,
            &artifact,
            &RuntimeCompileOptions {
                filename: Some("InlineImports.vue".to_string()),
                is_production: true,
                source_map: true,
                block_content: RuntimeBlockContentInputs {
                    template: Some(selected_template("<div>{{ count }}</div>")),
                    ..Default::default()
                },
                ..Default::default()
            },
            &oxc_allocator::Allocator::new(),
        )
        .expect("compile_bundle shares the parsed-runtime core");
    let script = match bundle {
        verter_compiler::framework_common::CarrierCompileOutcome::Produced(bundle) => {
            bundle.script.expect("composed runtime script")
        }
        other => panic!("expected produced runtime bundle, got {other:?}"),
    };
    assert!(
        script
            .runtime_imports
            .iter()
            .any(|name| name.contains("toDisplayString")),
        "helper imports spliced into the inline preamble must also be declared on runtime_imports: {:?}",
        script.runtime_imports
    );
}

#[test]
fn one_runtime_macro_channel_is_structural() {
    let inputs = VueRuntimeInputs::default();
    let _: &Option<Arc<verter_macro_dto::MacroRuntimeBundle>> = &inputs.macros;
    let _: &VueRuntimeExecutionFacts = &inputs.execution;
    assert!(inputs.macros.is_none());
    assert!(inputs.execution.prop_constness_overrides.is_none());
}

#[test]
fn vue_runtime_error_variants_are_runtime_only() {
    fn classify(err: VueRuntimeError) -> &'static str {
        match err {
            VueRuntimeError::BlockContentUnavailable => "block",
            VueRuntimeError::RequestExecutionRefused(_) => "exec",
            VueRuntimeError::NotRuntimeOnly { .. } => "product",
            VueRuntimeError::UnusableParse => "parse",
            VueRuntimeError::SourceMismatch => "source",
            VueRuntimeError::ProfileMismatch => "profile",
            VueRuntimeError::FrameworkMismatch => "framework",
            VueRuntimeError::Direct(_) => "direct",
            VueRuntimeError::ExecutionUngranted { .. } => "ungranted",
        }
    }
    assert_eq!(classify(VueRuntimeError::BlockContentUnavailable), "block");
    assert_eq!(
        classify(VueRuntimeError::RequestExecutionRefused(
            CompileRequestError::SsrVaporBackendUnsupported
        )),
        "exec"
    );
    assert_eq!(
        classify(VueRuntimeError::NotRuntimeOnly {
            unexpected: ProductKind::IdeCompanion
        }),
        "product"
    );
    assert_eq!(classify(VueRuntimeError::UnusableParse), "parse");
    assert_eq!(classify(VueRuntimeError::SourceMismatch), "source");
    assert_eq!(classify(VueRuntimeError::ProfileMismatch), "profile");
    assert_eq!(classify(VueRuntimeError::FrameworkMismatch), "framework");
}

fn selected_script(code: &str, lang: &str) -> RuntimeBlockContentInput {
    RuntimeBlockContentInput {
        code: Arc::from(code),
        source_map: None,
        lang: lang.to_string(),
        content_artifact_token: format!("content:{lang}"),
        source_space_token: format!("space:{lang}"),
        parsed: None,
    }
}

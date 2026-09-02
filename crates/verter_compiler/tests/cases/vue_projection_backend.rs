//! Vue IDE `ProjectionBackend`: parse-artifact projection, catalog identity,
//! byte/map parity with `compile_ide`, IDE-only refusal, and determinism.

use std::sync::Arc;

use verter_compiler::compile::types::{VueExecutionInputs, VueMacroSemanticInput};
use verter_compiler::compile_request::{
    AnalysisProductRequest, CompileProduct, CompileRequest, FrameworkCompileRequest,
    IdeProductRequest, ProductKind, RuntimeProductRequest, VueCompileRequest,
};
use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
use verter_compiler::framework_common::{
    vue_projection_backend_registration, CarrierCompiler, CatalogCapability, CatalogRow,
    CompileUnsupported, FrameworkEpoch, FrameworkParseArtifact, IdeCompileOptions,
    ImmutableCapabilityCatalog, ProjectionBackend, RuntimeBlockContentInput,
    RuntimeBlockContentInputs, RuntimeCompileOptions, RuntimeOutputDescriptor,
    VueProjectionBackend, VueProjectionError, VueProjectionInputs, VueSfcV3,
};
use verter_compiler::standalone::StandaloneCompiler;
use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
    FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};
use verter_language::{FrameworkAdapterId, LanguageId};

const KITCHEN_SINK: &str = include_str!("../fixtures/kitchen-sink.vue");

/// A genuine consume-once projection grant: issued through the registered
/// Vue host-integration backend and carved off the admission — the only
/// out-of-crate source of execution grants.
fn ide_grant() -> verter_compiler::framework_common::ProductExecutionGrant {
    use verter_compiler::framework_common::{
        FrameworkHostIntegrationBackend as _, VueHostIntegrationBackend, VueHostMultiProductDemand,
    };
    let artifact = registered_artifact("file:///grant-mint.vue", SIMPLE);
    VueHostIntegrationBackend::registered()
        .admit_host_products(
            &artifact,
            VueHostMultiProductDemand {
                products: vec![CompileProduct::IdeCompanion(IdeProductRequest::default())],
                ..Default::default()
            },
        )
        .expect("the grant-mint admission issues")
        .into_execution_grants()
        .projection
        .expect("the projection leg was admitted")
}

/// A genuine consume-once RUNTIME grant, for wrong-demand discrimination.
fn runtime_grant() -> verter_compiler::framework_common::ProductExecutionGrant {
    use verter_compiler::framework_common::{
        FrameworkHostIntegrationBackend as _, VueHostIntegrationBackend, VueHostRuntimeRenderDemand,
    };
    let artifact = registered_artifact("file:///grant-mint.vue", SIMPLE);
    VueHostIntegrationBackend::registered()
        .admit_runtime_render(&artifact, VueHostRuntimeRenderDemand::default())
        .expect("the grant-mint admission issues")
        .into_execution_grants()
        .runtime
        .expect("the runtime leg was admitted")
}

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

fn ide_only_request(filename: &str, want_source_map: bool) -> CompileRequest {
    ide_only_request_with_vue(filename, want_source_map, VueCompileRequest::default())
}

fn ide_only_request_with_vue(
    filename: &str,
    want_source_map: bool,
    vue: VueCompileRequest,
) -> CompileRequest {
    CompileRequest::new(
        vec![CompileProduct::IdeCompanion(IdeProductRequest {
            want_source_map,
            ..Default::default()
        })],
        FrameworkCompileRequest::Vue(vue),
        None,
        Some(filename.to_string()),
        None,
        false,
        false,
    )
    .expect("ide-only request constructs")
}

fn projected_template(code: &str) -> RuntimeBlockContentInput {
    RuntimeBlockContentInput {
        code: Arc::from(code),
        source_map: None,
        lang: "html".to_string(),
        content_artifact_token: "content:html".to_string(),
        source_space_token: "space:html".to_string(),
        parsed: None,
        producer: None,
    }
}

/// A grant carved for a different demand refuses typed before any
/// projection work runs — the demand match is part of consumption.
#[test]
fn wrong_demand_grant_refuses_projection_typed() {
    let artifact = registered_artifact("file:///wrong-grant.vue", SIMPLE);
    let err = VueProjectionBackend
        .project_ide(
            runtime_grant(),
            SIMPLE,
            &artifact,
            &ide_only_request("Wrong.vue", false),
            &VueProjectionInputs::default(),
        )
        .expect_err("a runtime grant must not drive the projection leg");
    assert!(
        matches!(
            err,
            VueProjectionError::Unsupported(CompileUnsupported::ProductExecutionUngranted {
                product: ProductKind::IdeCompanion,
            })
        ),
        "expected the typed ungranted refusal, got {err:?}"
    );
}

#[test]
fn vue_projection_catalog_row_binds_vue_adapter_identity() {
    let row = vue_projection_backend_registration();
    assert_eq!(row.identity().adapter_id(), &FrameworkAdapterId::vue());
    assert_eq!(
        row.identity().carrier_language_id(),
        &LanguageId::new("vue")
    );
    assert_eq!(row.identity().capability(), CatalogCapability::Projection);
    assert!(row.identity().host_epoch().is_none());
    assert_eq!(row.identity().epoch().as_str(), VueSfcV3::ID);
    assert_eq!(row.identity().epoch().as_str(), "vue");
    let _backend: &VueProjectionBackend = row.projection();
    let catalog =
        ImmutableCapabilityCatalog::<(), VueProjectionBackend, (), (), ()>::try_from_rows([
            CatalogRow::Projection(row),
        ])
        .expect("single Vue projection row");
    assert_eq!(catalog.len(), 1);
}

#[test]
fn vue_ide_projection_matches_compile_ide_on_kitchen_sink() {
    let artifact = registered_artifact("file:///kitchen.vue", KITCHEN_SINK);
    let opts = IdeCompileOptions {
        filename: Some("Kitchen.vue".to_string()),
        ..Default::default()
    };
    let via_compile_ide = VueCarrierCompiler
        .compile_ide(KITCHEN_SINK, &artifact, &opts)
        .expect("compile_ide kitchen sink");
    let request = ide_only_request("Kitchen.vue", !opts.skip_source_map);
    let via_backend = VueProjectionBackend
        .project_ide(
            ide_grant(),
            KITCHEN_SINK,
            &artifact,
            &request,
            &VueProjectionInputs::default(),
        )
        .expect("projection backend");
    assert_eq!(via_backend.ide.code, via_compile_ide.code);
    assert_eq!(via_backend.ide.source_map, via_compile_ide.source_map);
    assert_eq!(via_backend.ide.is_jsx, via_compile_ide.is_jsx);
    assert_eq!(
        via_backend
            .ide
            .output_descriptor
            .source_map
            .declared_space_tokens,
        via_compile_ide
            .output_descriptor
            .source_map
            .declared_space_tokens
    );
    assert!(!via_backend.ide.is_jsx);
    assert!(!via_backend.ide.code.is_empty());
}

#[test]
fn vue_ide_projection_is_deterministic() {
    let artifact = registered_artifact("file:///simple.vue", SIMPLE);
    let request = ide_only_request("Simple.vue", true);
    let first = VueProjectionBackend
        .project_ide(
            ide_grant(),
            SIMPLE,
            &artifact,
            &request,
            &VueProjectionInputs::default(),
        )
        .expect("first");
    let second = VueProjectionBackend
        .project_ide(
            ide_grant(),
            SIMPLE,
            &artifact,
            &request,
            &VueProjectionInputs::default(),
        )
        .expect("second");
    assert_eq!(first.ide.code, second.ide.code);
    assert_eq!(first.ide.source_map, second.ide.source_map);
    assert_eq!(first.ide.is_jsx, second.ide.is_jsx);
    assert_eq!(first.diagnostics, second.diagnostics);
}

#[test]
fn vue_ide_projection_refuses_a_foreign_artifact() {
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
    let request = ide_only_request("Foreign.vue", true);
    let err = VueProjectionBackend
        .project_ide(
            ide_grant(),
            source,
            &svelte,
            &request,
            &VueProjectionInputs::default(),
        )
        .expect_err("foreign artifact has no Vue parse");
    match err {
        VueProjectionError::Unsupported(CompileUnsupported::NoIdeProjection { adapter_id }) => {
            assert_eq!(adapter_id, FrameworkAdapterId::vue());
        }
        other => panic!("expected NoIdeProjection, got {other:?}"),
    }
}

#[test]
fn vue_ide_projection_refuses_source_that_does_not_match_the_admitted_artifact() {
    let artifact = registered_artifact("file:///simple.vue", SIMPLE);
    let request = ide_only_request("Simple.vue", true);
    let err = VueProjectionBackend
        .project_ide(
            ide_grant(),
            "<script setup>const n = 2</script>",
            &artifact,
            &request,
            &VueProjectionInputs::default(),
        )
        .expect_err("mismatched source must not reparse");
    match err {
        VueProjectionError::Unsupported(CompileUnsupported::NoIdeProjection { .. }) => {}
        other => panic!("expected NoIdeProjection for source mismatch, got {other:?}"),
    }
}

#[test]
fn vue_ide_projection_binds_request_syntax_profile_to_admitted_artifact() {
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
        let request = ide_only_request_with_vue(
            "Profile.vue",
            true,
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
        );
        let outcome = VueProjectionBackend.project_ide(
            ide_grant(),
            case.source,
            &artifact,
            &request,
            &VueProjectionInputs::default(),
        );
        if case.expect_ok {
            assert!(
                outcome.is_ok(),
                "{}: expected success, got {outcome:?}",
                case.name
            );
        } else {
            match outcome {
                Err(VueProjectionError::Unsupported(CompileUnsupported::NoIdeProjection {
                    adapter_id,
                })) => {
                    assert_eq!(adapter_id, FrameworkAdapterId::vue(), "{}", case.name);
                }
                other => panic!("{}: expected NoIdeProjection, got {other:?}", case.name),
            }
        }
    }
}

#[test]
fn compile_ide_accepts_an_admitted_nondefault_delimiter_artifact() {
    let artifact = registered_artifact_with_grammar(
        "file:///delimiters.vue",
        CUSTOM_PROFILE,
        "[[",
        "]]",
        std::iter::empty::<&str>(),
    );
    let output = VueCarrierCompiler
        .compile_ide(
            CUSTOM_PROFILE,
            &artifact,
            &IdeCompileOptions {
                filename: Some("Delimiters.vue".to_string()),
                ..Default::default()
            },
        )
        .expect("compile_ide must honor the admitted [[ ]] syntax profile");
    assert!(!output.code.is_empty());
}

#[test]
fn compile_ide_accepts_an_admitted_nondefault_custom_element_artifact() {
    let artifact = registered_artifact_with_grammar(
        "file:///custom-elements.vue",
        CUSTOM_PROFILE,
        "{{",
        "}}",
        ["ion-"],
    );
    let output = VueCarrierCompiler
        .compile_ide(
            CUSTOM_PROFILE,
            &artifact,
            &IdeCompileOptions {
                filename: Some("CustomElements.vue".to_string()),
                ..Default::default()
            },
        )
        .expect("compile_ide must honor the admitted custom-element syntax profile");
    assert!(!output.code.is_empty());
}

#[test]
fn compile_bundle_ide_request_uses_admitted_artifact_syntax_profile() {
    let artifact = registered_artifact_with_grammar(
        "file:///bundle-profile.vue",
        CUSTOM_PROFILE,
        "[[",
        "]]",
        ["ion-"],
    );
    let outcome = VueCarrierCompiler.compile_bundle(
        CUSTOM_PROFILE,
        &artifact,
        &RuntimeCompileOptions {
            filename: Some("BundleProfile.vue".to_string()),
            want_runtime: false,
            want_ide: true,
            ..Default::default()
        },
        &oxc_allocator::Allocator::new(),
    );
    match outcome {
        Ok(verter_compiler::framework_common::CarrierCompileOutcome::Produced(bundle)) => {
            assert!(
                bundle.tsx.is_some(),
                "IDE product must be present for a matching admitted profile"
            );
        }
        Ok(other) => panic!("expected produced IDE bundle, got {other:?}"),
        Err(error) => panic!(
            "compile_bundle IDE must not refuse an admitted [[ ]] / ion- artifact, got {error:?}"
        ),
    }
}

#[test]
fn vue_ide_projection_refuses_a_runtime_product_request() {
    let artifact = registered_artifact("file:///simple.vue", SIMPLE);
    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some("Simple.vue".to_string()),
        None,
        false,
        false,
    )
    .expect("runtime request constructs");
    let err = VueProjectionBackend
        .project_ide(
            ide_grant(),
            SIMPLE,
            &artifact,
            &request,
            &VueProjectionInputs::default(),
        )
        .expect_err("runtime product is not an IDE projection");
    match err {
        VueProjectionError::NotIdeOnly { unexpected } => {
            assert_eq!(unexpected, ProductKind::RuntimeClient);
        }
        other => panic!("expected NotIdeOnly, got {other:?}"),
    }
}

#[test]
fn vue_ide_projection_supplied_template_matches_compile_ide_without_a_runtime_product() {
    let source = concat!(
        "<template src=\"./view.html\"></template>",
        "<script setup>const count = 1</script>"
    );
    let artifact = registered_artifact("file:///direct.vue", source);
    let block_content = RuntimeBlockContentInputs {
        template: Some(projected_template("<p>{{ count }}</p>")),
        ..Default::default()
    };
    let via_compile_ide = VueCarrierCompiler
        .compile_ide(
            source,
            &artifact,
            &IdeCompileOptions {
                filename: Some("Direct.vue".to_string()),
                block_content: block_content.clone(),
                ..Default::default()
            },
        )
        .expect("compile_ide block-content");
    let request = ide_only_request("Direct.vue", true);
    assert_eq!(request.products().len(), 1);
    assert!(matches!(
        request.products()[0],
        CompileProduct::IdeCompanion(_)
    ));
    let via_backend = VueProjectionBackend
        .project_ide(
            ide_grant(),
            source,
            &artifact,
            &request,
            &VueProjectionInputs {
                block_content,
                execution: VueExecutionInputs::default(),
                macros: VueMacroSemanticInput::default(),
            },
        )
        .expect("projection backend block-content");
    assert_eq!(request.products().len(), 1);
    assert!(
        matches!(request.products()[0], CompileProduct::IdeCompanion(_)),
        "supplied-template projection must keep the inbound request IDE-only"
    );
    assert!(
        request
            .products()
            .iter()
            .all(|product| !matches!(product, CompileProduct::Analysis(_))),
        "supplied-template projection must not add Analysis onto the inbound request"
    );
    assert_eq!(via_backend.ide.code, via_compile_ide.code);
    assert_eq!(via_backend.ide.source_map, via_compile_ide.source_map);
    assert!(via_backend.ide.code.contains("{ count }"));
    assert_eq!(
        via_backend
            .ide
            .output_descriptor
            .source_map
            .declared_space_tokens
            .len(),
        2
    );
}

#[test]
fn vue_ide_projection_carrier_diagnostics_use_the_carrier_source_space() {
    let source = concat!(
        "<script setup>\n",
        "const n = 1\n",
        "defineProps({ n })\n",
        "</script>\n",
        "<template>\n",
        "  <div v-slot></div>\n",
        "</template>\n",
    );
    let artifact = registered_artifact("file:///carrier-diag.vue", source);
    let request = ide_only_request("CarrierDiag.vue", true);
    let (carrier_token, _) = RuntimeOutputDescriptor::carrier_source(source);
    let via_backend = VueProjectionBackend
        .project_ide(
            ide_grant(),
            source,
            &artifact,
            &request,
            &VueProjectionInputs::default(),
        )
        .expect("carrier diagnostics");
    assert!(
        !via_backend.diagnostics.is_empty(),
        "expected carrier compile diagnostics"
    );
    for tagged in &via_backend.diagnostics {
        assert_eq!(tagged.source_space_token, carrier_token);
    }
}

#[test]
fn vue_ide_projection_tags_selected_template_diagnostics_with_selected_source_space() {
    let source = concat!(
        "<template src=\"./view.html\"></template>",
        "<script setup>\n",
        "let n = 1\n",
        "defineProps({ n })\n",
        "</script>"
    );
    let selected = projected_template("<div v-slot>{{ n }}</div>");
    let artifact = registered_artifact("file:///selected-diag.vue", source);
    let request = ide_only_request("SelectedDiag.vue", true);
    let (carrier_token, _) = RuntimeOutputDescriptor::carrier_source(source);
    let via_backend = VueProjectionBackend
        .project_ide(
            ide_grant(),
            source,
            &artifact,
            &request,
            &VueProjectionInputs {
                block_content: RuntimeBlockContentInputs {
                    template: Some(selected.clone()),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .expect("selected-template diagnostics");

    let carrier_count = via_backend
        .diagnostics
        .iter()
        .filter(|tagged| tagged.source_space_token == carrier_token)
        .count();
    let selected_count = via_backend
        .diagnostics
        .iter()
        .filter(|tagged| tagged.source_space_token == selected.source_space_token)
        .count();
    assert!(
        carrier_count > 0,
        "expected carrier diagnostics tagged with the carrier source space"
    );
    assert!(
        selected_count > 0,
        "expected selected-template diagnostics tagged with the selected source space"
    );
    assert_eq!(
        via_backend.diagnostics.len(),
        carrier_count + selected_count,
        "every diagnostic must use either the carrier or selected source space"
    );

    let last_carrier = via_backend
        .diagnostics
        .iter()
        .rposition(|tagged| tagged.source_space_token == carrier_token)
        .expect("carrier diagnostic");
    let first_selected = via_backend
        .diagnostics
        .iter()
        .position(|tagged| tagged.source_space_token == selected.source_space_token)
        .expect("selected diagnostic");
    assert!(
        last_carrier < first_selected,
        "carrier diagnostics must precede selected-template diagnostics"
    );
}

#[test]
fn vue_ide_projection_plain_script_external_template_is_typed_unavailable() {
    let source = concat!(
        "<template src=\"./view.html\"></template>",
        "<script>export default { data: () => ({ count: 1 }) }</script>"
    );
    let artifact = registered_artifact("file:///plain.vue", source);
    let request = ide_only_request("ExternalPlain.vue", true);
    let err = VueProjectionBackend
        .project_ide(
            ide_grant(),
            source,
            &artifact,
            &request,
            &VueProjectionInputs {
                block_content: RuntimeBlockContentInputs {
                    template: Some(projected_template("<div>{{ count }}</div>")),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .expect_err("plain script + selected template is unavailable");
    match err {
        VueProjectionError::Unsupported(CompileUnsupported::BlockContentIdeUnavailable {
            ..
        }) => {}
        other => panic!("expected BlockContentIdeUnavailable, got {other:?}"),
    }
}

#[test]
fn vue_ide_projection_refuses_an_analysis_product_request() {
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
    let err = VueProjectionBackend
        .project_ide(
            ide_grant(),
            SIMPLE,
            &artifact,
            &request,
            &VueProjectionInputs::default(),
        )
        .expect_err("analysis is not an IDE projection");
    match err {
        VueProjectionError::NotIdeOnly { unexpected } => {
            assert_eq!(unexpected, ProductKind::Analysis);
        }
        other => panic!("expected NotIdeOnly Analysis, got {other:?}"),
    }
}

#[test]
fn parsed_core_ide_only_request_does_not_publish_a_runtime_artifact() {
    let request = ide_only_request("Simple.vue", true);
    let execution = VueExecutionInputs::default();
    let macros = VueMacroSemanticInput::Unavailable;
    let output = StandaloneCompiler
        .compile(
            SIMPLE,
            &request,
            verter_compiler::standalone::DirectExecutionInputs::Vue {
                execution: &execution,
                macros: &macros,
            },
        )
        .expect("ide-only parsed core");
    assert!(output
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .is_none());
    assert!(output
        .artifacts
        .artifact(ProductKind::RuntimeServer)
        .is_none());
    assert!(output
        .artifacts
        .artifact(ProductKind::IdeCompanion)
        .is_some());
}

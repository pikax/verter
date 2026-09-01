//! Svelte runtime `RuntimeCompilerBackend`: parse-artifact emit, catalog
//! identity, byte/map/diagnostic parity with the parsed core, runtime-only
//! refusal, option preservation, and production `compile_bundle` isolation.

use std::collections::BTreeMap;
use std::sync::Arc;

use verter_compiler::compile_request::svelte::{
    AdmittedSvelteCustomElementDescriptor, SvelteCssRequest, SvelteCustomElementDescriptor,
    SvelteCustomElementPropDescriptor, SvelteCustomElementPropType, SvelteNamespaceRequest,
    SvelteOption,
};
use verter_compiler::compile_request::{
    AnalysisProductRequest, CompileProduct, CompileRequest, CompileRequestError,
    FrameworkCompileRequest, FrameworkOption, IdeProductRequest, ProductKind,
    RuntimeProductRequest, SvelteCompileRequest, SvelteOptionAttempt, VueCompileRequest,
};
use verter_compiler::framework_common::{
    CarrierCompiler, CatalogCapability, CatalogRow, FrameworkEpoch, FrameworkParseArtifact,
    ImmutableCapabilityCatalog, RuntimeCompileOptions, RuntimeCompilerBackend,
};
use verter_compiler::standalone::{
    DirectCompileError, DirectCompileOutput, DirectExecutionInputs, StandaloneCompiler,
    SvelteExecutionInputs,
};
use verter_compiler::style_planner::{prepare_supplied_plain_css, PreparedStyleIr};
use verter_compiler::svelte::runtime::{ClientCompileError, UnsupportedSvelteRuntimeSurface};
use verter_compiler::svelte::{
    svelte_runtime_backend_registration, SvelteCarrierCompiler, SvelteRuntimeBackend,
    SvelteRuntimeError, SvelteRuntimeInputs, SvelteSfc5,
};
use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
    FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};
use verter_language::{FileLanguage, FrameworkAdapterId, LanguageId, ParseOptions};

const KITCHEN_SINK: &str = concat!(
    "<script>let name = $state('world'); let count = $state(0);</script>\n",
    "<h1>Hello {name}!</h1>\n",
    "<input bind:value={name} />\n",
    "<button onclick={() => count += 1}>clicks: {count}</button>\n",
);

const SIMPLE: &str = concat!(
    "<script lang=\"ts\">\n",
    "let count = $state(1);\n",
    "</script>\n",
    "<div>{count}</div>\n",
);

const STYLED: &str = "<div class=\"card\">x</div>\n<style>.card{color:blue}</style>\n";

const CE_PROPS: &str = concat!(
    "<script>\n",
    "let { count } = $props();\n",
    "</script>\n",
    "<h1>{count}</h1>\n",
);

const INLINE_CE: &str = concat!(
    "<svelte:options customElement=\"inline-el\" />\n",
    "<script>let count = $state(1);</script>\n",
    "<div>{count}</div>\n",
);

fn registered_artifact(canonical: &str, source: &str, svelte: bool) -> FrameworkParseArtifact {
    let language = if svelte {
        FileLanguage::svelte()
    } else {
        FileLanguage::vue()
    };
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
    let config = if svelte {
        CarrierGrammarConfig::Svelte
    } else {
        CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>()).expect("vue grammar")
    };
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

fn svelte_artifact(canonical: &str, source: &str) -> FrameworkParseArtifact {
    registered_artifact(canonical, source, true)
}

fn runtime_request(
    filename: &str,
    products: Vec<CompileProduct>,
    svelte: SvelteCompileRequest,
    is_production: bool,
) -> CompileRequest {
    CompileRequest::new(
        products,
        FrameworkCompileRequest::Svelte(svelte),
        None,
        Some(filename.to_string()),
        None,
        is_production,
        false,
    )
    .expect("runtime request constructs")
}

fn client_product(runtime_source_map: bool) -> CompileProduct {
    CompileProduct::RuntimeClient(RuntimeProductRequest {
        runtime_source_map,
        ..Default::default()
    })
}

fn server_product(runtime_source_map: bool) -> CompileProduct {
    CompileProduct::RuntimeServer(RuntimeProductRequest {
        runtime_source_map,
        ..Default::default()
    })
}

fn default_inputs() -> SvelteRuntimeInputs {
    SvelteRuntimeInputs::default()
}

fn compile_via_backend(
    source: &str,
    artifact: &FrameworkParseArtifact,
    request: &CompileRequest,
) -> Result<DirectCompileOutput, SvelteRuntimeError> {
    compile_via_backend_with_inputs(source, artifact, request, &default_inputs())
}

fn compile_via_backend_with_inputs(
    source: &str,
    artifact: &FrameworkParseArtifact,
    request: &CompileRequest,
    inputs: &SvelteRuntimeInputs,
) -> Result<DirectCompileOutput, SvelteRuntimeError> {
    RuntimeCompilerBackend::compile_runtime(
        &SvelteRuntimeBackend,
        runtime_grant_for(request),
        source,
        artifact,
        request,
        inputs,
    )
}

/// A genuine consume-once runtime grant matching the request's demanded
/// runtime kind: issued through the registered Svelte host-integration
/// backend and carved off the admission — the only out-of-crate source of
/// execution grants.
fn runtime_grant_for(
    request: &CompileRequest,
) -> verter_compiler::framework_common::ProductExecutionGrant {
    use verter_compiler::framework_common::{
        FrameworkHostIntegrationBackend as _, SvelteHostIntegrationBackend,
        SvelteHostRuntimeRenderDemand,
    };
    let ssr = request
        .products()
        .iter()
        .any(|p| p.kind() == ProductKind::RuntimeServer);
    let artifact = registered_artifact("file:///grant-mint.svelte", SIMPLE, true);
    SvelteHostIntegrationBackend::registered()
        .admit_runtime_render(
            &artifact,
            SvelteHostRuntimeRenderDemand {
                ssr,
                ..Default::default()
            },
        )
        .expect("the grant-mint admission issues")
        .into_execution_grants()
        .runtime
        .expect("the runtime leg was admitted")
}

fn compile_via_standalone(
    source: &str,
    request: &CompileRequest,
    execution: &SvelteExecutionInputs,
) -> Result<DirectCompileOutput, DirectCompileError> {
    StandaloneCompiler.compile(source, request, DirectExecutionInputs::Svelte { execution })
}

fn assert_runtime_parity(via_backend: &DirectCompileOutput, via_standalone: &DirectCompileOutput) {
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

fn assert_server_generate_refusal(error: SvelteRuntimeError) {
    match error {
        SvelteRuntimeError::Direct(DirectCompileError::Svelte(
            ClientCompileError::Unsupported(UnsupportedSvelteRuntimeSurface::ServerGenerate {
                ..
            }),
        )) => {}
        other => panic!("expected Direct(Svelte(Unsupported(ServerGenerate))), got {other:?}"),
    }
}

#[test]
fn svelte_runtime_catalog_row_binds_svelte_adapter_identity() {
    let row = svelte_runtime_backend_registration();
    assert_eq!(row.identity().adapter_id(), &FrameworkAdapterId::svelte());
    assert_eq!(
        row.identity().carrier_language_id(),
        &LanguageId::new("svelte")
    );
    assert_eq!(row.identity().capability(), CatalogCapability::Runtime);
    assert!(row.identity().host_epoch().is_none());
    assert_eq!(row.identity().epoch().as_str(), SvelteSfc5::ID);
    assert_eq!(row.identity().epoch().as_str(), "svelte");
    let _backend: &SvelteRuntimeBackend = row.runtime();
    let catalog =
        ImmutableCapabilityCatalog::<(), (), (), SvelteRuntimeBackend, ()>::try_from_rows([
            CatalogRow::Runtime(row),
        ])
        .expect("single Svelte runtime row");
    assert_eq!(catalog.len(), 1);
}

#[test]
fn svelte_runtime_backend_matches_parsed_core_on_kitchen_sink() {
    let artifact = svelte_artifact("file:///kitchen.svelte", KITCHEN_SINK);
    let request = runtime_request(
        "Kitchen.svelte",
        vec![client_product(true)],
        SvelteCompileRequest::default(),
        false,
    );
    let via_standalone =
        compile_via_standalone(KITCHEN_SINK, &request, &SvelteExecutionInputs::default())
            .expect("parsed-core runtime compile");
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
    assert!(via_backend.diagnostics.is_empty());
}

#[test]
fn svelte_runtime_backend_is_deterministic() {
    let artifact = svelte_artifact("file:///simple.svelte", SIMPLE);
    let request = runtime_request(
        "Simple.svelte",
        vec![client_product(true)],
        SvelteCompileRequest::default(),
        false,
    );
    let first = compile_via_backend(SIMPLE, &artifact, &request).expect("first");
    let second = compile_via_backend(SIMPLE, &artifact, &request).expect("second");
    assert_runtime_parity(&first, &second);
}

#[test]
fn runtime_source_map_option_is_honored() {
    let artifact = svelte_artifact("file:///maps.svelte", SIMPLE);
    let mapped = runtime_request(
        "Maps.svelte",
        vec![client_product(true)],
        SvelteCompileRequest::default(),
        false,
    );
    let unmapped = runtime_request(
        "Maps.svelte",
        vec![client_product(false)],
        SvelteCompileRequest::default(),
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
    assert_runtime_parity(
        &with_map,
        &compile_via_standalone(SIMPLE, &mapped, &SvelteExecutionInputs::default())
            .expect("mapped core"),
    );
    assert_runtime_parity(
        &without_map,
        &compile_via_standalone(SIMPLE, &unmapped, &SvelteExecutionInputs::default())
            .expect("unmapped core"),
    );
}

#[test]
fn css_hash_override_is_preserved_and_does_not_hide_an_absent_override() {
    let artifact = svelte_artifact("file:///App.svelte", STYLED);
    let request = runtime_request(
        "App.svelte",
        vec![client_product(false)],
        SvelteCompileRequest::default(),
        false,
    );
    let overridden = SvelteRuntimeInputs {
        execution: SvelteExecutionInputs {
            css_hash_override: Some("zzoverride1".to_string()),
            prepared_styles: Vec::new(),
        },
    };
    let absent = SvelteRuntimeInputs::default();
    let via_override = compile_via_backend_with_inputs(STYLED, &artifact, &request, &overridden)
        .expect("override");
    let via_absent =
        compile_via_backend_with_inputs(STYLED, &artifact, &request, &absent).expect("absent");
    assert_ne!(
        via_override
            .artifacts
            .artifact(ProductKind::RuntimeClient)
            .expect("client")
            .code(),
        via_absent
            .artifacts
            .artifact(ProductKind::RuntimeClient)
            .expect("client")
            .code(),
        "an explicit css hash override must change the published client"
    );
    assert_eq!(
        via_override.styles[0].scope_hash.as_deref(),
        Some("zzoverride1")
    );
    assert_ne!(
        via_absent.styles[0].scope_hash.as_deref(),
        Some("zzoverride1")
    );
    assert_runtime_parity(
        &via_override,
        &compile_via_standalone(STYLED, &request, &overridden.execution).expect("override core"),
    );
    assert_runtime_parity(
        &via_absent,
        &compile_via_standalone(STYLED, &request, &absent.execution).expect("absent core"),
    );
}

#[test]
fn explicit_dev_true_is_not_silently_emitted_as_production() {
    let artifact = svelte_artifact("file:///dev.svelte", SIMPLE);
    let explicit_true = runtime_request(
        "Dev.svelte",
        vec![client_product(false)],
        SvelteCompileRequest {
            dev: Some(true),
            ..Default::default()
        },
        false,
    );
    let omitted = runtime_request(
        "Dev.svelte",
        vec![client_product(false)],
        SvelteCompileRequest::default(),
        false,
    );
    let via_true = compile_via_backend(SIMPLE, &artifact, &explicit_true);
    let via_omitted =
        compile_via_backend(SIMPLE, &artifact, &omitted).expect("dev omitted compiles");
    assert!(
        via_true.is_err(),
        "explicit dev=true must refuse rather than emit production output: {via_true:?}"
    );
    let standalone_true =
        compile_via_standalone(SIMPLE, &explicit_true, &SvelteExecutionInputs::default());
    match (via_true, standalone_true) {
        (Err(SvelteRuntimeError::Direct(backend)), Err(standalone)) => {
            assert_eq!(backend, standalone);
        }
        other => panic!("expected matching Direct refusals, got {other:?}"),
    }
    assert_runtime_parity(
        &via_omitted,
        &compile_via_standalone(SIMPLE, &omitted, &SvelteExecutionInputs::default())
            .expect("omitted core"),
    );
}

#[test]
fn svelte_runtime_backend_refuses_a_server_request() {
    let artifact = svelte_artifact("file:///server.svelte", SIMPLE);
    let request = runtime_request(
        "Server.svelte",
        vec![server_product(false)],
        SvelteCompileRequest::default(),
        false,
    );
    let err = compile_via_backend(SIMPLE, &artifact, &request)
        .expect_err("Svelte server runtime is unproducible");
    assert_server_generate_refusal(err);
}

#[test]
fn svelte_runtime_backend_refuses_client_and_server_together_with_no_partial() {
    let artifact = svelte_artifact("file:///dual.svelte", SIMPLE);
    let request = runtime_request(
        "Dual.svelte",
        vec![client_product(true), server_product(true)],
        SvelteCompileRequest::default(),
        false,
    );
    let err = compile_via_backend(SIMPLE, &artifact, &request)
        .expect_err("unproducible server refuses the whole request");
    assert_server_generate_refusal(err);
}

#[test]
fn svelte_runtime_backend_refuses_a_foreign_artifact() {
    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "const count = 1;\n",
        "</script>\n",
        "<template>\n",
        "  <div>{{ count }}</div>\n",
        "</template>\n",
    );
    let vue = registered_artifact("file:///foreign.vue", source, false);
    let request = runtime_request(
        "Foreign.svelte",
        vec![client_product(true)],
        SvelteCompileRequest::default(),
        false,
    );
    let err = compile_via_backend(source, &vue, &request)
        .expect_err("foreign artifact has no Svelte parse");
    match err {
        SvelteRuntimeError::UnusableParse => {}
        other => panic!("expected UnusableParse, got {other:?}"),
    }
}

#[test]
fn svelte_runtime_backend_refuses_source_that_does_not_match_the_admitted_artifact() {
    let artifact = svelte_artifact("file:///simple.svelte", SIMPLE);
    let request = runtime_request(
        "Simple.svelte",
        vec![client_product(true)],
        SvelteCompileRequest::default(),
        false,
    );
    let err = compile_via_backend("<script>let n = $state(2)</script>", &artifact, &request)
        .expect_err("mismatched source must not reparse");
    match err {
        SvelteRuntimeError::SourceMismatch => {}
        other => panic!("expected SourceMismatch, got {other:?}"),
    }
}

#[test]
fn svelte_runtime_backend_binds_request_syntax_profile_to_admitted_artifact() {
    let artifact = svelte_artifact("file:///profile.svelte", SIMPLE);
    let language = FileLanguage::svelte();
    let admitted = verter_language::syntax_profile_id_for(
        &language,
        &ParseOptions {
            svelte_loose: false,
            ..ParseOptions::default()
        },
    )
    .expect("strict Svelte profile");
    let loose = verter_language::syntax_profile_id_for(
        &language,
        &ParseOptions {
            svelte_loose: true,
            ..ParseOptions::default()
        },
    )
    .expect("loose Svelte profile");
    assert_eq!(artifact.syntax_profile(), &admitted);
    assert_ne!(
        admitted, loose,
        "loose mode must mint a distinct syntax profile so a mismatch can refuse"
    );

    let request = runtime_request(
        "Profile.svelte",
        vec![client_product(true)],
        SvelteCompileRequest::default(),
        false,
    );
    compile_via_backend(SIMPLE, &artifact, &request)
        .expect("strict artifact versus strict request");

    let vue = registered_artifact("file:///profile.vue", SIMPLE, false);
    assert_ne!(
        vue.syntax_profile(),
        artifact.syntax_profile(),
        "a Vue parse of the same bytes must carry a different syntax profile"
    );
    let err = compile_via_backend(SIMPLE, &vue, &request)
        .expect_err("admitted Vue syntax profile must not satisfy a Svelte request");
    match err {
        SvelteRuntimeError::UnusableParse | SvelteRuntimeError::ProfileMismatch => {}
        other => panic!("expected UnusableParse or ProfileMismatch, got {other:?}"),
    }
}

#[test]
fn svelte_runtime_backend_refuses_an_ide_product_request() {
    let artifact = svelte_artifact("file:///simple.svelte", SIMPLE);
    let request = CompileRequest::new(
        vec![
            client_product(false),
            CompileProduct::IdeCompanion(IdeProductRequest::default()),
        ],
        FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
        None,
        Some("Simple.svelte".to_string()),
        None,
        false,
        false,
    )
    .expect("mixed request constructs");
    let err = compile_via_backend(SIMPLE, &artifact, &request)
        .expect_err("IDE product is not a runtime emit");
    match err {
        SvelteRuntimeError::NotRuntimeOnly { unexpected } => {
            assert_eq!(unexpected, ProductKind::IdeCompanion);
        }
        other => panic!("expected NotRuntimeOnly, got {other:?}"),
    }
}

#[test]
fn svelte_runtime_backend_refuses_an_analysis_product_request() {
    let artifact = svelte_artifact("file:///simple.svelte", SIMPLE);
    let request = CompileRequest::new(
        vec![CompileProduct::Analysis(AnalysisProductRequest {
            want_script_bindings: true,
            want_template_data: false,
        })],
        FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
        None,
        Some("Simple.svelte".to_string()),
        None,
        false,
        false,
    )
    .expect("analysis request constructs");
    let err = compile_via_backend(SIMPLE, &artifact, &request)
        .expect_err("analysis is not a runtime emit");
    match err {
        SvelteRuntimeError::NotRuntimeOnly { unexpected } => {
            assert_eq!(unexpected, ProductKind::Analysis);
        }
        other => panic!("expected NotRuntimeOnly Analysis, got {other:?}"),
    }
}

#[test]
fn compile_bundle_still_emits_runtime_when_an_ide_product_is_also_requested() {
    let artifact = svelte_artifact("file:///bundle.svelte", SIMPLE);
    let mixed = SvelteCarrierCompiler
        .compile_bundle(
            SIMPLE,
            &artifact,
            &RuntimeCompileOptions {
                filename: Some("Bundle.svelte".to_string()),
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
                bundle.has_runtime_surface(),
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
            client_product(true),
            CompileProduct::IdeCompanion(IdeProductRequest {
                want_source_map: true,
                ..Default::default()
            }),
        ],
        FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
        None,
        Some("Bundle.svelte".to_string()),
        None,
        false,
        false,
    )
    .expect("mixed request constructs");
    let err = compile_via_backend(SIMPLE, &artifact, &request)
        .expect_err("runtime backend refuses a mixed product set");
    match err {
        SvelteRuntimeError::NotRuntimeOnly { unexpected } => {
            assert_eq!(unexpected, ProductKind::IdeCompanion);
        }
        other => panic!("expected NotRuntimeOnly IdeCompanion, got {other:?}"),
    }
}

#[test]
fn svelte_runtime_backend_refuses_a_non_svelte_request() {
    let artifact = svelte_artifact("file:///simple.svelte", SIMPLE);
    let request = CompileRequest::new(
        vec![client_product(true)],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some("Simple.vue".to_string()),
        None,
        false,
        false,
    )
    .expect("vue runtime request constructs");
    let err = compile_via_backend(SIMPLE, &artifact, &request)
        .expect_err("a Vue request is not a Svelte runtime compile");
    match err {
        SvelteRuntimeError::FrameworkMismatch => {}
        other => panic!("expected FrameworkMismatch, got {other:?}"),
    }
}

#[test]
fn svelte_runtime_backend_refuses_a_foreign_namespace() {
    let artifact = svelte_artifact("file:///foreign-ns.svelte", SIMPLE);
    let request = runtime_request(
        "ForeignNs.svelte",
        vec![client_product(false)],
        SvelteCompileRequest {
            namespace: Some(SvelteNamespaceRequest::Foreign),
            ..Default::default()
        },
        false,
    );
    let err = compile_via_backend(SIMPLE, &artifact, &request)
        .expect_err("Foreign namespace has no compiler-internal representation");
    match err {
        SvelteRuntimeError::Direct(DirectCompileError::UnsupportedSvelteNamespace) => {}
        other => panic!("expected Direct(UnsupportedSvelteNamespace), got {other:?}"),
    }
}

#[test]
fn svelte_runtime_error_is_closed_without_request_execution_refused() {
    fn classify(err: SvelteRuntimeError) -> &'static str {
        match err {
            SvelteRuntimeError::NotRuntimeOnly { .. } => "product",
            SvelteRuntimeError::UnusableParse => "parse",
            SvelteRuntimeError::SourceMismatch => "source",
            SvelteRuntimeError::ProfileMismatch => "profile",
            SvelteRuntimeError::FrameworkMismatch => "framework",
            SvelteRuntimeError::Direct(_) => "direct",
            SvelteRuntimeError::ExecutionUngranted { .. } => "ungranted",
        }
    }
    assert_eq!(
        classify(SvelteRuntimeError::NotRuntimeOnly {
            unexpected: ProductKind::IdeCompanion
        }),
        "product"
    );
    assert_eq!(classify(SvelteRuntimeError::UnusableParse), "parse");
    assert_eq!(classify(SvelteRuntimeError::SourceMismatch), "source");
    assert_eq!(classify(SvelteRuntimeError::ProfileMismatch), "profile");
    assert_eq!(classify(SvelteRuntimeError::FrameworkMismatch), "framework");
    assert_eq!(
        classify(SvelteRuntimeError::Direct(
            DirectCompileError::UnsupportedSvelteNamespace
        )),
        "direct"
    );
}

#[test]
fn empty_svelte_product_set_is_refused_at_request_construction() {
    let err = CompileRequest::new(
        Vec::new(),
        FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
        None,
        Some("Empty.svelte".to_string()),
        None,
        false,
        false,
    )
    .expect_err("an empty product set cannot construct");
    assert_eq!(err, CompileRequestError::EmptyProductSet);
}

fn svelte_request(filename: &str, svelte: SvelteCompileRequest) -> CompileRequest {
    runtime_request(filename, vec![client_product(false)], svelte, false)
}

fn compile_backend_and_core(
    source: &str,
    filename: &str,
    svelte: SvelteCompileRequest,
    inputs: &SvelteRuntimeInputs,
) -> Result<DirectCompileOutput, SvelteRuntimeError> {
    let artifact = svelte_artifact(&format!("file:///{filename}"), source);
    let request = svelte_request(filename, svelte);
    let via_backend = compile_via_backend_with_inputs(source, &artifact, &request, inputs)?;
    let via_standalone = compile_via_standalone(source, &request, &inputs.execution)
        .map_err(SvelteRuntimeError::Direct)?;
    assert_runtime_parity(&via_backend, &via_standalone);
    Ok(via_backend)
}

fn client_js(output: &DirectCompileOutput) -> &str {
    output
        .artifacts
        .artifact(ProductKind::RuntimeClient)
        .expect("client artifact")
        .code()
}

fn matching_prepared_styles(source: &str) -> Vec<Option<PreparedStyleIr>> {
    let parsed = verter_compiler::svelte::parse_svelte(source);
    let content = parsed
        .styles
        .first()
        .and_then(|style| style.content)
        .expect("style body span");
    let ir = verter_css_syntax::parse_style_body(source, content).expect("style body parses");
    vec![Some(PreparedStyleIr::new(ir))]
}

fn wrong_dialect_prepared_styles(source: &str) -> Vec<Option<PreparedStyleIr>> {
    let parsed = verter_compiler::svelte::parse_svelte(source);
    let content = parsed
        .styles
        .first()
        .and_then(|style| style.content)
        .expect("style body span");
    let body = source
        .get(content.start as usize..content.end as usize)
        .expect("style body bytes");
    let ir = verter_css_syntax::parse_style_ir(
        verter_css_syntax::CssSource::new(Arc::from(body), content.start)
            .expect("style body fits the parser"),
        verter_css_syntax::CssDialect::Scss,
        verter_css_syntax::CssParseMode::Recover,
    )
    .expect("scss-tagged same-bytes parse");
    assert_ne!(
        ir.dialect(),
        verter_css_syntax::CssDialect::Css,
        "the prepared hint must carry a non-CSS dialect"
    );
    vec![Some(PreparedStyleIr::new(ir))]
}

/// The tagged custom-element descriptor, admitted from the caller-facing
/// spelling `spelling` through the one admission authority. Every test
/// below reaches the admitted descriptor this way rather than fabricating
/// one, so what they compile is exactly what a caller can obtain.
fn admit_tagged_ce_descriptor(
    spelling: &str,
) -> Result<AdmittedSvelteCustomElementDescriptor, CompileRequestError> {
    let mut props = BTreeMap::new();
    props.insert(
        "count".to_string(),
        SvelteCustomElementPropDescriptor {
            attribute: Some("data-count".to_string()),
            reflect: Some(true),
            prop_type: Some(spelling.to_string()),
        },
    );
    let attempt = SvelteOptionAttempt {
        custom_element_descriptor: Some(SvelteCustomElementDescriptor {
            tag: Some("x-props".to_string()),
            shadow: Some(false),
            props,
        }),
        ..Default::default()
    };
    Ok(attempt
        .into_request()?
        .custom_element_descriptor
        .expect("the admitted request keeps the descriptor"))
}

fn tagged_ce_descriptor() -> AdmittedSvelteCustomElementDescriptor {
    admit_tagged_ce_descriptor("Number").expect("`Number` is an admitted spelling")
}

fn assert_malformed_custom_element(err: SvelteRuntimeError, option: SvelteOption, value: &str) {
    match err {
        SvelteRuntimeError::Direct(DirectCompileError::SvelteOption(
            CompileRequestError::MalformedOptionValue {
                option: FrameworkOption::Svelte(got_option),
                value: got_value,
            },
        )) => {
            assert_eq!(got_option, option);
            assert_eq!(got_value, value);
        }
        SvelteRuntimeError::Direct(DirectCompileError::Vue(inner)) => {
            panic!("Svelte option refusal must not wrap as Vue({inner:?})");
        }
        other => panic!(
            "expected Direct(SvelteOption(MalformedOptionValue {{ option: {option:?}, value: {value:?} }})), got {other:?}"
        ),
    }
}

#[test]
fn injected_css_request_inlines_styles_and_publishes_no_external_artifact() {
    let svelte = SvelteCompileRequest {
        css: Some(SvelteCssRequest::Injected),
        ..Default::default()
    };
    let output = compile_backend_and_core(STYLED, "App.svelte", svelte, &default_inputs())
        .expect("injected css compiles");
    let js = client_js(&output);
    assert!(
        js.contains("$.append_styles($$anchor, $$css);"),
        "injected css must emit append_styles:\n{js}"
    );
    assert!(
        output.styles.is_empty(),
        "injected css must not publish an external style artifact, got {:?}",
        output.styles
    );
}

#[test]
fn external_css_request_retains_the_external_style_artifact() {
    let svelte = SvelteCompileRequest {
        css: Some(SvelteCssRequest::External),
        ..Default::default()
    };
    let output = compile_backend_and_core(STYLED, "App.svelte", svelte, &default_inputs())
        .expect("external css compiles");
    let js = client_js(&output);
    assert!(
        !js.contains("$.append_styles"),
        "external css must not inline append_styles:\n{js}"
    );
    assert_eq!(
        output.styles.len(),
        1,
        "external css must publish the style artifact"
    );
}

#[test]
fn omitted_css_request_stays_external_without_custom_element_or_inline_injected() {
    let output = compile_backend_and_core(
        STYLED,
        "App.svelte",
        SvelteCompileRequest::default(),
        &default_inputs(),
    )
    .expect("default css compiles");
    let js = client_js(&output);
    assert!(
        !js.contains("$.append_styles"),
        "omitted css must stay external:\n{js}"
    );
    assert_eq!(
        output.styles.len(),
        1,
        "omitted css must publish the external style artifact"
    );
}

#[test]
fn custom_element_descriptor_emits_tagged_create_with_shadow_none_and_prop_fields() {
    let svelte = SvelteCompileRequest {
        custom_element_descriptor: Some(tagged_ce_descriptor()),
        custom_element: Some(false),
        ..Default::default()
    };
    let output = compile_backend_and_core(CE_PROPS, "App.svelte", svelte, &default_inputs())
        .expect("tagged custom-element descriptor compiles");
    let js = client_js(&output);
    assert!(
        js.contains(
            "customElements.define('x-props', $.create_custom_element(App, { count: { attribute: 'data-count', reflect: true, type: 'Number' } }, [], []));"
        ),
        "tagged descriptor must define with shadow-none (arg5 omitted) and prop fields:\n{js}"
    );
}

#[test]
fn empty_custom_element_descriptor_emits_untagged_create_without_define() {
    let svelte = SvelteCompileRequest {
        custom_element_descriptor: Some(AdmittedSvelteCustomElementDescriptor::default()),
        ..Default::default()
    };
    let output = compile_backend_and_core(SIMPLE, "App.svelte", svelte, &default_inputs())
        .expect("empty custom-element descriptor compiles as an untagged custom element");
    let js = client_js(&output);
    assert!(
        js.contains("$.create_custom_element(App, {}, [], [], { mode: 'open' });"),
        "a present empty descriptor is an active untagged custom element:\n{js}"
    );
    assert!(
        !js.contains("customElements.define"),
        "an untagged custom element must not define:\n{js}"
    );
}

#[test]
fn invalid_custom_element_tag_refuses_before_emission() {
    let svelte = SvelteCompileRequest {
        custom_element_descriptor: Some(AdmittedSvelteCustomElementDescriptor {
            tag: Some("Div".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let artifact = svelte_artifact("file:///App.svelte", SIMPLE);
    let request = svelte_request("App.svelte", svelte);
    let err = compile_via_backend(SIMPLE, &artifact, &request)
        .expect_err("an invalid custom-element tag must refuse");
    assert_malformed_custom_element(err, SvelteOption::CustomElementTag, "Div");
    let standalone = compile_via_standalone(SIMPLE, &request, &SvelteExecutionInputs::default());
    match standalone {
        Err(DirectCompileError::SvelteOption(CompileRequestError::MalformedOptionValue {
            option: FrameworkOption::Svelte(SvelteOption::CustomElementTag),
            value,
        })) => assert_eq!(value, "Div"),
        other => panic!("parsed-core must refuse the same malformed tag, got {other:?}"),
    }
}

/// The prop-type vocabulary is decided at request construction, so an
/// unrecognised spelling never reaches emission: it cannot be placed on a
/// [`SvelteCompileRequest`] at all. The refusal identity and the offending
/// value are the ones the emission-stage check used to report.
#[test]
fn invalid_custom_element_prop_type_refuses_at_request_construction() {
    match admit_tagged_ce_descriptor("Nope")
        .expect_err("an unrecognised prop type must refuse at admission")
    {
        CompileRequestError::MalformedOptionValue {
            option: FrameworkOption::Svelte(option),
            value,
        } => {
            assert_eq!(option, SvelteOption::CustomElementPropsType);
            assert_eq!(value, "Nope");
        }
        other => panic!("expected MalformedOptionValue, got {other:?}"),
    }
}

/// Every casing outside the ten admitted spellings is refused, including
/// casings of an otherwise-valid word: admission is a closed set of
/// spellings, not a case-insensitive comparison.
#[test]
fn custom_element_prop_type_casing_outside_the_admitted_set_refuses() {
    for spelling in [
        "NUMBER", "nUmBeR", "sTring", "OBJECT", "Arrays", "symbol", "",
    ] {
        let err = admit_tagged_ce_descriptor(spelling)
            .err()
            .unwrap_or_else(|| panic!("`{spelling}` is not an admitted spelling"));
        match err {
            CompileRequestError::MalformedOptionValue {
                option: FrameworkOption::Svelte(option),
                value,
            } => {
                assert_eq!(option, SvelteOption::CustomElementPropsType);
                assert_eq!(value, spelling);
            }
            other => panic!("expected MalformedOptionValue for `{spelling}`, got {other:?}"),
        }
    }
}

/// Both admitted spellings of every prop type render the SAME Svelte
/// backend spelling, so the emitted custom-element registration is
/// byte-identical whichever one the caller wrote.
///
/// The cases are derived from the canonical vocabulary, so a sixth prop
/// type added to that one list is covered here without editing this test.
#[test]
fn both_admitted_spellings_of_every_prop_type_render_identical_output() {
    for prop_type in SvelteCustomElementPropType::ALL {
        let capitalised = prop_type.as_svelte_name();
        let expected = format!(
            "customElements.define('x-props', $.create_custom_element(App, {{ count: {{ \
             attribute: 'data-count', reflect: true, type: '{capitalised}' }} }}, [], []));"
        );
        let mut rendered = Vec::new();
        for spelling in [capitalised.to_string(), capitalised.to_ascii_lowercase()] {
            let svelte = SvelteCompileRequest {
                custom_element_descriptor: Some(
                    admit_tagged_ce_descriptor(&spelling)
                        .unwrap_or_else(|e| panic!("`{spelling}` must be admitted, got {e:?}")),
                ),
                custom_element: Some(false),
                ..Default::default()
            };
            let output =
                compile_backend_and_core(CE_PROPS, "App.svelte", svelte, &default_inputs())
                    .unwrap_or_else(|e| panic!("`{spelling}` must compile, got {e:?}"));
            let js = client_js(&output).to_string();
            assert!(
                js.contains(&expected),
                "`{spelling}` must render `type: '{capitalised}'`:\n{js}"
            );
            rendered.push(js);
        }
        assert_eq!(
            rendered[0], rendered[1],
            "`{capitalised}` and its lowercase spelling must emit byte-identical output"
        );
    }
}

#[test]
fn inline_custom_element_tag_wins_over_a_conflicting_request_descriptor() {
    let svelte = SvelteCompileRequest {
        custom_element_descriptor: Some(AdmittedSvelteCustomElementDescriptor {
            tag: Some("request-el".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let output = compile_backend_and_core(INLINE_CE, "App.svelte", svelte, &default_inputs())
        .expect("inline customElement wins");
    let js = client_js(&output);
    assert!(
        js.contains("customElements.define('inline-el', $.create_custom_element("),
        "inline <svelte:options customElement> must win:\n{js}"
    );
    assert!(
        !js.contains("request-el"),
        "the request descriptor tag must not leak past the inline value:\n{js}"
    );
}

#[test]
fn matching_prepared_styles_reuse_the_admitted_ir_and_match_an_empty_carrier() {
    let matching = SvelteRuntimeInputs {
        execution: SvelteExecutionInputs {
            css_hash_override: None,
            prepared_styles: matching_prepared_styles(STYLED),
        },
    };
    let empty = SvelteRuntimeInputs::default();
    let artifact = svelte_artifact("file:///App.svelte", STYLED);
    let request = svelte_request("App.svelte", SvelteCompileRequest::default());

    let before_matching = verter_css_syntax::parse_style_ir_thread_invocations();
    let via_matching =
        compile_via_backend_with_inputs(STYLED, &artifact, &request, &matching).expect("matching");
    let matching_parses = verter_css_syntax::parse_style_ir_thread_invocations() - before_matching;

    let before_empty = verter_css_syntax::parse_style_ir_thread_invocations();
    let via_empty =
        compile_via_backend_with_inputs(STYLED, &artifact, &request, &empty).expect("empty");
    let empty_parses = verter_css_syntax::parse_style_ir_thread_invocations() - before_empty;

    assert_eq!(
        matching_parses, 0,
        "matching prepared styles must reuse the admitted IR instead of reparsing"
    );
    assert_eq!(
        empty_parses, 1,
        "an empty prepared carrier still parses the style body once"
    );
    assert_runtime_parity(&via_matching, &via_empty);
    assert_runtime_parity(
        &via_matching,
        &compile_via_standalone(STYLED, &request, &matching.execution).expect("matching core"),
    );
    assert_runtime_parity(
        &via_empty,
        &compile_via_standalone(STYLED, &request, &empty.execution).expect("empty core"),
    );
}

#[test]
fn mismatched_prepared_styles_still_compile_via_safe_reparse() {
    let origin_zero = prepare_supplied_plain_css(".card{color:blue}").expect("origin-0 css parses");
    assert_eq!(
        origin_zero.ir().source().origin(),
        0,
        "prepare_supplied_plain_css must stay origin 0 so a style-body mismatch is real"
    );
    let wrong_bytes = prepare_supplied_plain_css(".other{color:red}").expect("decoy css parses");
    for prepared in [origin_zero, wrong_bytes] {
        let inputs = SvelteRuntimeInputs {
            execution: SvelteExecutionInputs {
                css_hash_override: None,
                prepared_styles: vec![Some(prepared)],
            },
        };
        let output = compile_backend_and_core(
            STYLED,
            "App.svelte",
            SvelteCompileRequest::default(),
            &inputs,
        )
        .expect("mismatched prepared styles must compile via safe reparse");
        let empty = compile_backend_and_core(
            STYLED,
            "App.svelte",
            SvelteCompileRequest::default(),
            &default_inputs(),
        )
        .expect("empty carrier compiles");
        assert_runtime_parity(&output, &empty);
    }
}

#[test]
fn wrong_dialect_prepared_styles_still_compile_via_safe_reparse() {
    let wrong_dialect = SvelteRuntimeInputs {
        execution: SvelteExecutionInputs {
            css_hash_override: None,
            prepared_styles: wrong_dialect_prepared_styles(STYLED),
        },
    };
    let empty = SvelteRuntimeInputs::default();
    let artifact = svelte_artifact("file:///App.svelte", STYLED);
    let request = svelte_request("App.svelte", SvelteCompileRequest::default());

    let before_wrong = verter_css_syntax::parse_style_ir_thread_invocations();
    let via_wrong = compile_via_backend_with_inputs(STYLED, &artifact, &request, &wrong_dialect)
        .expect("wrong-dialect prepared styles must compile via safe reparse");
    let wrong_parses = verter_css_syntax::parse_style_ir_thread_invocations() - before_wrong;

    assert_eq!(
        wrong_parses, 1,
        "a same-bytes non-CSS dialect hint must not be admitted; the style body is reparsed as CSS"
    );

    let via_empty =
        compile_via_backend_with_inputs(STYLED, &artifact, &request, &empty).expect("empty");
    assert_runtime_parity(&via_wrong, &via_empty);
    assert_runtime_parity(
        &via_wrong,
        &compile_via_standalone(STYLED, &request, &wrong_dialect.execution)
            .expect("wrong-dialect core"),
    );
}

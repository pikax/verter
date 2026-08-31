//! Svelte IDE `ProjectionBackend`: parse-artifact projection, catalog identity,
//! byte/map parity with `compile_ide`, IDE-only refusal, and determinism.

use std::sync::Arc;

use verter_compiler::compile_request::svelte::SvelteNamespaceRequest;
use verter_compiler::compile_request::{
    AnalysisProductRequest, CompileProduct, CompileRequest, FrameworkCompileRequest,
    IdeProductRequest, ProductKind, RuntimeProductRequest, SvelteCompileRequest,
};
use verter_compiler::framework_common::{
    CarrierCompiler, CatalogCapability, CatalogRow, CompileUnsupported, FrameworkEpoch,
    FrameworkParseArtifact, IdeCompileOptions, ImmutableCapabilityCatalog, ProjectionBackend,
    RuntimeOutputDescriptor,
};
use verter_compiler::svelte::{
    svelte_projection_backend_registration, SvelteCarrierCompiler, SvelteProjectionBackend,
    SvelteProjectionError, SvelteProjectionInputs, SvelteSfc5,
};
use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
    FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};
use verter_language::{FileLanguage, FrameworkAdapterId, LanguageId, ParseOptions};

/// A genuine consume-once projection grant: issued through the registered
/// Svelte host-integration backend and carved off the admission — the
/// only out-of-crate source of execution grants.
fn ide_grant() -> verter_compiler::framework_common::ProductExecutionGrant {
    use verter_compiler::compile_request::{CompileProduct, IdeProductRequest};
    use verter_compiler::framework_common::{
        FrameworkHostIntegrationBackend as _, SvelteHostIntegrationBackend,
        SvelteHostMultiProductDemand,
    };
    let artifact = registered_artifact("file:///grant-mint.svelte", SIMPLE, true);
    SvelteHostIntegrationBackend::registered()
        .admit_host_products(
            &artifact,
            SvelteHostMultiProductDemand {
                products: vec![CompileProduct::IdeCompanion(IdeProductRequest::default())],
                ..Default::default()
            },
        )
        .expect("the grant-mint admission issues")
        .into_execution_grants()
        .projection
        .expect("the projection leg was admitted")
}

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

const JS_CARRIER: &str = concat!(
    "<script>\n",
    "let count = $state(0);\n",
    "</script>\n",
    "<button onclick={() => count += 1}>{count}</button>\n",
);

const AWAIT_EXPR: &str = "<div>{await thing}</div>";

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

fn ide_only_request(filename: &str, want_source_map: bool) -> CompileRequest {
    CompileRequest::new(
        vec![CompileProduct::IdeCompanion(IdeProductRequest {
            want_source_map,
            ..Default::default()
        })],
        FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
        None,
        Some(filename.to_string()),
        None,
        false,
        false,
    )
    .expect("ide-only request constructs")
}

fn svelte_standard_parse_options() -> ParseOptions {
    ParseOptions {
        svelte_loose: false,
        ..ParseOptions::vue_standard()
    }
}

#[test]
fn svelte_projection_catalog_row_binds_svelte_adapter_identity() {
    let row = svelte_projection_backend_registration();
    assert_eq!(row.identity().adapter_id(), &FrameworkAdapterId::svelte());
    assert_eq!(
        row.identity().carrier_language_id(),
        &LanguageId::new("svelte")
    );
    assert_eq!(row.identity().capability(), CatalogCapability::Projection);
    assert!(row.identity().host_epoch().is_none());
    assert_eq!(row.identity().epoch().as_str(), SvelteSfc5::ID);
    assert_eq!(row.identity().epoch().as_str(), "svelte");
    let _backend: &SvelteProjectionBackend = row.projection();
    let catalog =
        ImmutableCapabilityCatalog::<(), SvelteProjectionBackend, (), (), ()>::try_from_rows([
            CatalogRow::Projection(row),
        ])
        .expect("single Svelte projection row");
    assert_eq!(catalog.len(), 1);
}

#[test]
fn svelte_ide_projection_matches_compile_ide_on_kitchen_sink() {
    let artifact = svelte_artifact("file:///kitchen.svelte", KITCHEN_SINK);
    let opts = IdeCompileOptions {
        filename: Some("Kitchen.svelte".to_string()),
        ..Default::default()
    };
    let via_compile_ide = SvelteCarrierCompiler
        .compile_ide(KITCHEN_SINK, &artifact, &opts)
        .expect("compile_ide kitchen sink");
    let request = ide_only_request("Kitchen.svelte", !opts.skip_source_map);
    let via_backend = SvelteProjectionBackend
        .project_ide(
            ide_grant(),
            KITCHEN_SINK,
            &artifact,
            &request,
            &SvelteProjectionInputs,
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
    assert!(via_backend.ide.is_jsx);
    assert!(!via_backend.ide.code.is_empty());
}

#[test]
fn svelte_ide_projection_matches_compile_ide_on_a_typescript_carrier() {
    let artifact = svelte_artifact("file:///simple.svelte", SIMPLE);
    let opts = IdeCompileOptions {
        filename: Some("Simple.svelte".to_string()),
        ..Default::default()
    };
    let via_compile_ide = SvelteCarrierCompiler
        .compile_ide(SIMPLE, &artifact, &opts)
        .expect("compile_ide simple");
    let via_backend = SvelteProjectionBackend
        .project_ide(
            ide_grant(),
            SIMPLE,
            &artifact,
            &ide_only_request("Simple.svelte", true),
            &SvelteProjectionInputs,
        )
        .expect("projection backend");
    assert_eq!(via_backend.ide.code, via_compile_ide.code);
    assert_eq!(via_backend.ide.source_map, via_compile_ide.source_map);
    assert!(!via_backend.ide.is_jsx);
}

#[test]
fn svelte_carrier_compile_ide_still_projects() {
    let artifact = svelte_artifact("file:///js.svelte", JS_CARRIER);
    let out = SvelteCarrierCompiler
        .compile_ide(JS_CARRIER, &artifact, &IdeCompileOptions::default())
        .expect("production compile_ide route");
    assert!(out.is_jsx);
    assert!(out
        .code
        .starts_with("/** @jsxImportSource @verter/svelte-jsx */"));
}

#[test]
fn svelte_ide_projection_is_deterministic() {
    let artifact = svelte_artifact("file:///simple.svelte", SIMPLE);
    let request = ide_only_request("Simple.svelte", true);
    let first = SvelteProjectionBackend
        .project_ide(
            ide_grant(),
            SIMPLE,
            &artifact,
            &request,
            &SvelteProjectionInputs,
        )
        .expect("first");
    let second = SvelteProjectionBackend
        .project_ide(
            ide_grant(),
            SIMPLE,
            &artifact,
            &request,
            &SvelteProjectionInputs,
        )
        .expect("second");
    assert_eq!(first.ide.code, second.ide.code);
    assert_eq!(first.ide.source_map, second.ide.source_map);
    assert_eq!(first.ide.is_jsx, second.ide.is_jsx);
    assert_eq!(first.diagnostics, second.diagnostics);
}

#[test]
fn svelte_ide_projection_refuses_a_foreign_artifact() {
    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "const count = 1;\n",
        "</script>\n",
        "<template>\n",
        "  <div>{{ count }}</div>\n",
        "</template>\n",
    );
    let vue = registered_artifact("file:///foreign.vue", source, false);
    let request = ide_only_request("Foreign.svelte", true);
    let err = SvelteProjectionBackend
        .project_ide(ide_grant(), source, &vue, &request, &SvelteProjectionInputs)
        .expect_err("foreign artifact has no Svelte parse");
    match err {
        SvelteProjectionError::Unsupported(CompileUnsupported::NoIdeProjection { adapter_id }) => {
            assert_eq!(adapter_id, FrameworkAdapterId::svelte());
        }
        other => panic!("expected NoIdeProjection, got {other:?}"),
    }
}

#[test]
fn svelte_ide_projection_refuses_source_that_does_not_match_the_admitted_artifact() {
    let artifact = svelte_artifact("file:///simple.svelte", SIMPLE);
    let request = ide_only_request("Simple.svelte", true);
    let err = SvelteProjectionBackend
        .project_ide(
            ide_grant(),
            "<script>let n = 2</script>",
            &artifact,
            &request,
            &SvelteProjectionInputs,
        )
        .expect_err("mismatched source must not reparse");
    match err {
        SvelteProjectionError::Unsupported(CompileUnsupported::NoIdeProjection { .. }) => {}
        other => panic!("expected NoIdeProjection for source mismatch, got {other:?}"),
    }
}

#[test]
fn svelte_ide_projection_binds_request_syntax_profile_to_admitted_artifact() {
    let artifact = svelte_artifact("file:///profile.svelte", SIMPLE);
    let language = FileLanguage::svelte();
    let admitted =
        verter_language::syntax_profile_id_for(&language, &svelte_standard_parse_options())
            .expect("strict Svelte profile");
    let loose = verter_language::syntax_profile_id_for(
        &language,
        &ParseOptions {
            svelte_loose: true,
            ..ParseOptions::vue_standard()
        },
    )
    .expect("loose Svelte profile");
    let defaulted = verter_language::syntax_profile_id_for(&language, &ParseOptions::default())
        .expect("default Svelte profile");
    assert_eq!(artifact.syntax_profile(), &admitted);
    assert_eq!(
        admitted, defaulted,
        "Svelte's parse-affecting default is strict (`svelte_loose: false`)"
    );
    assert_ne!(
        admitted, loose,
        "loose mode must mint a distinct syntax profile so a mismatch can refuse"
    );

    let ok = SvelteProjectionBackend.project_ide(
        ide_grant(),
        SIMPLE,
        &artifact,
        &ide_only_request("Profile.svelte", true),
        &SvelteProjectionInputs,
    );
    assert!(ok.is_ok(), "strict artifact versus strict request: {ok:?}");

    let vue = registered_artifact("file:///profile.vue", SIMPLE, false);
    assert_ne!(
        vue.syntax_profile(),
        artifact.syntax_profile(),
        "a Vue parse of the same bytes must carry a different syntax profile"
    );
    let err = SvelteProjectionBackend
        .project_ide(
            ide_grant(),
            SIMPLE,
            &vue,
            &ide_only_request("Profile.svelte", true),
            &SvelteProjectionInputs,
        )
        .expect_err("admitted Vue syntax profile must not satisfy a Svelte request");
    match err {
        SvelteProjectionError::Unsupported(CompileUnsupported::NoIdeProjection { adapter_id }) => {
            assert_eq!(adapter_id, FrameworkAdapterId::svelte());
        }
        other => panic!("expected NoIdeProjection for profile mismatch, got {other:?}"),
    }
}

#[test]
fn svelte_ide_projection_succeeds_for_foreign_namespace_like_compile_ide() {
    let artifact = svelte_artifact("file:///foreign-ns.svelte", SIMPLE);
    let opts = IdeCompileOptions {
        filename: Some("ForeignNs.svelte".to_string()),
        ..Default::default()
    };
    let via_compile_ide = SvelteCarrierCompiler
        .compile_ide(SIMPLE, &artifact, &opts)
        .expect("compile_ide has no namespace axis");
    let request = CompileRequest::new(
        vec![CompileProduct::IdeCompanion(IdeProductRequest {
            want_source_map: !opts.skip_source_map,
            ..Default::default()
        })],
        FrameworkCompileRequest::Svelte(SvelteCompileRequest {
            namespace: Some(SvelteNamespaceRequest::Foreign),
            ..Default::default()
        }),
        None,
        Some("ForeignNs.svelte".to_string()),
        None,
        false,
        false,
    )
    .expect("Foreign namespace constructs at the request layer");
    let via_backend = SvelteProjectionBackend
        .project_ide(
            ide_grant(),
            SIMPLE,
            &artifact,
            &request,
            &SvelteProjectionInputs,
        )
        .expect("IDE-only Foreign namespace must project like compile_ide");
    assert_eq!(via_backend.ide.code, via_compile_ide.code);
    assert_eq!(via_backend.ide.source_map, via_compile_ide.source_map);
    assert_eq!(via_backend.ide.is_jsx, via_compile_ide.is_jsx);
}

#[test]
fn svelte_ide_projection_refuses_a_runtime_product_request() {
    let artifact = svelte_artifact("file:///simple.svelte", SIMPLE);
    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
        None,
        Some("Simple.svelte".to_string()),
        None,
        false,
        false,
    )
    .expect("runtime request constructs");
    let err = SvelteProjectionBackend
        .project_ide(
            ide_grant(),
            SIMPLE,
            &artifact,
            &request,
            &SvelteProjectionInputs,
        )
        .expect_err("runtime product is not an IDE projection");
    match err {
        SvelteProjectionError::NotIdeOnly { unexpected } => {
            assert_eq!(unexpected, ProductKind::RuntimeClient);
        }
        other => panic!("expected NotIdeOnly, got {other:?}"),
    }
}

#[test]
fn svelte_ide_projection_refuses_an_analysis_product_request() {
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
    let err = SvelteProjectionBackend
        .project_ide(
            ide_grant(),
            SIMPLE,
            &artifact,
            &request,
            &SvelteProjectionInputs,
        )
        .expect_err("analysis is not an IDE projection");
    match err {
        SvelteProjectionError::NotIdeOnly { unexpected } => {
            assert_eq!(unexpected, ProductKind::Analysis);
        }
        other => panic!("expected NotIdeOnly Analysis, got {other:?}"),
    }
}

#[test]
fn svelte_ide_projection_carrier_diagnostics_use_the_carrier_source_space() {
    let artifact = svelte_artifact("file:///await.svelte", AWAIT_EXPR);
    let request = ide_only_request("Await.svelte", true);
    let (carrier_token, _) = RuntimeOutputDescriptor::carrier_source(AWAIT_EXPR);
    let via_backend = SvelteProjectionBackend
        .project_ide(
            ide_grant(),
            AWAIT_EXPR,
            &artifact,
            &request,
            &SvelteProjectionInputs,
        )
        .expect("carrier diagnostics");
    assert!(
        via_backend
            .diagnostics
            .iter()
            .any(|tagged| tagged.diagnostic.code == "svelte-await-experimental"),
        "expected the experimental await-expression diagnostic, got {:?}",
        via_backend.diagnostics
    );
    for tagged in &via_backend.diagnostics {
        assert_eq!(tagged.source_space_token, carrier_token);
    }
}

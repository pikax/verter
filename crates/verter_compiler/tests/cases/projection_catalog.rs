//! Route-boundary evidence that Vue/Svelte IDE projection is selected from
//! the built-in catalog (adapter × epoch × Projection) and that combined
//! compile_ide / compile_bundle IDE products delegate to that backend.

use std::sync::Arc;

use oxc_allocator::Allocator;
use verter_compiler::compile::types::{VueExecutionInputs, VueMacroSemanticInput};
use verter_compiler::compile_request::{
    CompileProduct, CompileRequest, FrameworkCompileRequest, IdeProductRequest,
    SvelteCompileRequest, VueCompileRequest,
};
use verter_compiler::framework_common::registered_carrier_projection::{
    built_in_projection_catalog, project_ide_from_catalog, registered_projection_for,
    take_projection_producer_invocations, ProjectionCatalogInputs,
};
use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
use verter_compiler::framework_common::{
    CarrierCompileOutcome, CarrierCompiler, CatalogCapability, CompileUnsupported,
    FrameworkParseArtifact, IdeCompileOptions, ProjectionBackend, RuntimeCompileOptions,
    RuntimeDiagnostic, VueProjectionBackend, VueProjectionInputs,
};
use verter_compiler::svelte::{
    SvelteCarrierCompiler, SvelteProjectionBackend, SvelteProjectionInputs,
};
use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
    FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};
use verter_language::{FileLanguage, FrameworkAdapterId};

const VUE_SIMPLE: &str = concat!(
    "<script setup lang=\"ts\">\n",
    "const count = 1;\n",
    "</script>\n",
    "<template>\n",
    "  <div>{{ count }}</div>\n",
    "</template>\n",
);

const SVELTE_SIMPLE: &str = concat!(
    "<script lang=\"ts\">\n",
    "let count = $state(1);\n",
    "</script>\n",
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

fn vue_ide_request(filename: &str) -> CompileRequest {
    CompileRequest::new(
        vec![CompileProduct::IdeCompanion(IdeProductRequest {
            want_source_map: true,
            ..Default::default()
        })],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some(filename.to_string()),
        None,
        false,
        false,
    )
    .expect("vue ide-only request")
}

fn svelte_ide_request(filename: &str) -> CompileRequest {
    CompileRequest::new(
        vec![CompileProduct::IdeCompanion(IdeProductRequest {
            want_source_map: true,
            ..Default::default()
        })],
        FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
        None,
        Some(filename.to_string()),
        None,
        false,
        false,
    )
    .expect("svelte ide-only request")
}

fn produced_bundle(
    compiler: &impl CarrierCompiler,
    source: &str,
    artifact: &FrameworkParseArtifact,
    opts: &RuntimeCompileOptions,
) -> verter_compiler::framework_common::RuntimeCompileOutput {
    match compiler
        .compile_bundle(source, artifact, opts, &Allocator::new())
        .expect("bundle compiles")
    {
        CarrierCompileOutcome::Produced(bundle) => bundle,
        other => panic!("expected produced bundle, got {other:?}"),
    }
}

/// Test-minted consume-once projection grant (production carves grants off
/// a host-issued admission).
fn ide_grant() -> verter_compiler::framework_common::ProductExecutionGrant {
    verter_compiler::framework_common::ProductExecutionGrant::mint_for_tests(
        verter_compiler::compile_request::ProductKind::IdeCompanion,
    )
}

#[test]
fn built_in_projection_catalog_installs_vue_and_svelte_rows() {
    let catalog = built_in_projection_catalog();
    assert_eq!(
        catalog.len(),
        2,
        "exactly one Vue and one Svelte projection row"
    );
    let identities: Vec<_> = catalog
        .iter()
        .map(|row| {
            let identity = row.identity();
            assert_eq!(identity.capability(), CatalogCapability::Projection);
            assert!(identity.host_epoch().is_none());
            (
                identity.adapter_id().clone(),
                identity.epoch().as_str().to_string(),
            )
        })
        .collect();
    assert!(
        identities
            .iter()
            .any(|(adapter, epoch)| adapter == &FrameworkAdapterId::vue() && epoch == "vue"),
        "catalog must contain the Vue projection row: {identities:?}"
    );
    assert!(
        identities
            .iter()
            .any(|(adapter, epoch)| adapter == &FrameworkAdapterId::svelte() && epoch == "svelte"),
        "catalog must contain the Svelte projection row: {identities:?}"
    );
}

#[test]
fn projection_lookup_is_adapter_times_epoch_identity() {
    let vue = registered_artifact("file:///lookup.vue", VUE_SIMPLE, false);
    let svelte = registered_artifact("file:///lookup.svelte", SVELTE_SIMPLE, true);
    assert!(
        registered_projection_for(vue.adapter_id(), vue.epoch()).is_some(),
        "Vue adapter × Vue epoch must select a projection row"
    );
    assert!(
        registered_projection_for(svelte.adapter_id(), svelte.epoch()).is_some(),
        "Svelte adapter × Svelte epoch must select a projection row"
    );
    assert!(
        registered_projection_for(vue.adapter_id(), svelte.epoch()).is_none(),
        "a Vue adapter must not select a Svelte projection row"
    );
    assert!(
        registered_projection_for(svelte.adapter_id(), vue.epoch()).is_none(),
        "a Svelte adapter must not select a Vue projection row"
    );
}

#[test]
fn unknown_epoch_is_a_catalog_miss_not_a_projection() {
    let artifact = registered_artifact("file:///miss.vue", VUE_SIMPLE, false);
    let reminted = artifact.remint_epoch_for_tests("unknown-epoch");
    let _ = take_projection_producer_invocations();
    let err = project_ide_from_catalog(
        ide_grant(),
        &reminted,
        VUE_SIMPLE,
        &vue_ide_request("Miss.vue"),
        &ProjectionCatalogInputs::default(),
    )
    .expect_err("unknown epoch must refuse");
    match err {
        CompileUnsupported::NoIdeProjection { adapter_id } => {
            assert_eq!(adapter_id, *reminted.adapter_id());
        }
        other => panic!("expected NoIdeProjection, got {other:?}"),
    }
    assert_eq!(
        take_projection_producer_invocations(),
        0,
        "a catalog miss must not invoke a projection backend"
    );
}

#[test]
fn catalog_vue_projection_matches_the_vue_backend() {
    let artifact = registered_artifact("file:///catalog.vue", VUE_SIMPLE, false);
    let request = vue_ide_request("Catalog.vue");
    let via_backend = VueProjectionBackend
        .project_ide(
            ide_grant(),
            VUE_SIMPLE,
            &artifact,
            &request,
            &VueProjectionInputs::default(),
        )
        .expect("vue backend");
    let _ = take_projection_producer_invocations();
    let via_catalog = project_ide_from_catalog(
        ide_grant(),
        &artifact,
        VUE_SIMPLE,
        &request,
        &ProjectionCatalogInputs::default(),
    )
    .expect("catalog vue projection");
    assert_eq!(take_projection_producer_invocations(), 1);
    assert_eq!(via_catalog.ide.code, via_backend.ide.code);
    assert_eq!(via_catalog.ide.source_map, via_backend.ide.source_map);
    assert_eq!(via_catalog.ide.is_jsx, via_backend.ide.is_jsx);
    assert_eq!(
        via_catalog
            .ide
            .output_descriptor
            .source_map
            .declared_space_tokens,
        via_backend
            .ide
            .output_descriptor
            .source_map
            .declared_space_tokens
    );
}

#[test]
fn catalog_svelte_projection_matches_the_svelte_backend() {
    let artifact = registered_artifact("file:///catalog.svelte", SVELTE_SIMPLE, true);
    let request = svelte_ide_request("Catalog.svelte");
    let via_backend = SvelteProjectionBackend
        .project_ide(
            ide_grant(),
            SVELTE_SIMPLE,
            &artifact,
            &request,
            &SvelteProjectionInputs,
        )
        .expect("svelte backend");
    let _ = take_projection_producer_invocations();
    let via_catalog = project_ide_from_catalog(
        ide_grant(),
        &artifact,
        SVELTE_SIMPLE,
        &request,
        &ProjectionCatalogInputs::default(),
    )
    .expect("catalog svelte projection");
    assert_eq!(take_projection_producer_invocations(), 1);
    assert_eq!(via_catalog.ide.code, via_backend.ide.code);
    assert_eq!(via_catalog.ide.source_map, via_backend.ide.source_map);
    assert_eq!(via_catalog.ide.is_jsx, via_backend.ide.is_jsx);
}

#[test]
fn vue_compile_ide_delegates_to_the_catalog_backend_once() {
    let artifact = registered_artifact("file:///ide.vue", VUE_SIMPLE, false);
    let opts = IdeCompileOptions {
        filename: Some("Ide.vue".to_string()),
        ..Default::default()
    };
    let request = vue_ide_request("Ide.vue");
    let via_backend = VueProjectionBackend
        .project_ide(
            ide_grant(),
            VUE_SIMPLE,
            &artifact,
            &request,
            &VueProjectionInputs::default(),
        )
        .expect("vue backend");
    let _ = take_projection_producer_invocations();
    let via_compile_ide = VueCarrierCompiler
        .compile_ide(VUE_SIMPLE, &artifact, &opts)
        .expect("compile_ide");
    assert_eq!(
        take_projection_producer_invocations(),
        1,
        "compile_ide must project through the catalog once"
    );
    assert_eq!(via_compile_ide.code, via_backend.ide.code);
    assert_eq!(via_compile_ide.source_map, via_backend.ide.source_map);
    assert_eq!(via_compile_ide.is_jsx, via_backend.ide.is_jsx);
}

#[test]
fn svelte_compile_ide_delegates_to_the_catalog_backend_once() {
    let artifact = registered_artifact("file:///ide.svelte", SVELTE_SIMPLE, true);
    let opts = IdeCompileOptions {
        filename: Some("Ide.svelte".to_string()),
        ..Default::default()
    };
    let request = svelte_ide_request("Ide.svelte");
    let via_backend = SvelteProjectionBackend
        .project_ide(
            ide_grant(),
            SVELTE_SIMPLE,
            &artifact,
            &request,
            &SvelteProjectionInputs,
        )
        .expect("svelte backend");
    let _ = take_projection_producer_invocations();
    let via_compile_ide = SvelteCarrierCompiler
        .compile_ide(SVELTE_SIMPLE, &artifact, &opts)
        .expect("compile_ide");
    assert_eq!(
        take_projection_producer_invocations(),
        1,
        "compile_ide must project through the catalog once"
    );
    assert_eq!(via_compile_ide.code, via_backend.ide.code);
    assert_eq!(via_compile_ide.source_map, via_backend.ide.source_map);
    assert_eq!(via_compile_ide.is_jsx, via_backend.ide.is_jsx);
}

#[test]
fn vue_compile_bundle_ide_product_delegates_to_the_catalog_backend_once() {
    let artifact = registered_artifact("file:///bundle.vue", VUE_SIMPLE, false);
    let request = vue_ide_request("Bundle.vue");
    let via_backend = VueProjectionBackend
        .project_ide(
            ide_grant(),
            VUE_SIMPLE,
            &artifact,
            &request,
            &VueProjectionInputs {
                execution: VueExecutionInputs::default(),
                macros: VueMacroSemanticInput::default(),
                ..Default::default()
            },
        )
        .expect("vue backend");
    let _ = take_projection_producer_invocations();
    let bundle = produced_bundle(
        &VueCarrierCompiler,
        VUE_SIMPLE,
        &artifact,
        &RuntimeCompileOptions {
            filename: Some("Bundle.vue".to_string()),
            source_map: true,
            want_runtime: false,
            want_ide: true,
            ..Default::default()
        },
    );
    assert_eq!(
        take_projection_producer_invocations(),
        1,
        "the combined adapter must project its IDE product through the catalog once"
    );
    let tsx = bundle.tsx.expect("IDE product present");
    assert_eq!(tsx.code, via_backend.ide.code);
    assert_eq!(tsx.source_map, via_backend.ide.source_map);
    assert_eq!(tsx.is_jsx, via_backend.ide.is_jsx);
}

#[test]
fn svelte_compile_bundle_ide_product_delegates_to_the_catalog_backend_once() {
    let artifact = registered_artifact("file:///bundle.svelte", SVELTE_SIMPLE, true);
    let request = svelte_ide_request("Bundle.svelte");
    let via_backend = SvelteProjectionBackend
        .project_ide(
            ide_grant(),
            SVELTE_SIMPLE,
            &artifact,
            &request,
            &SvelteProjectionInputs,
        )
        .expect("svelte backend");
    let _ = take_projection_producer_invocations();
    let bundle = produced_bundle(
        &SvelteCarrierCompiler,
        SVELTE_SIMPLE,
        &artifact,
        &RuntimeCompileOptions {
            filename: Some("Bundle.svelte".to_string()),
            source_map: true,
            want_runtime: false,
            want_ide: true,
            ..Default::default()
        },
    );
    assert_eq!(
        take_projection_producer_invocations(),
        1,
        "the combined adapter must project its IDE product through the catalog once"
    );
    let tsx = bundle.tsx.expect("IDE product present");
    assert_eq!(tsx.code, via_backend.ide.code);
    assert_eq!(tsx.source_map, via_backend.ide.source_map);
    assert_eq!(tsx.is_jsx, via_backend.ide.is_jsx);
}

#[test]
fn vue_ide_only_compile_bundle_diagnostics_equal_catalog_companion() {
    const SOURCE: &str = concat!(
        "<script setup>\n",
        "let n = 1\n",
        "defineProps({ n })\n",
        "</script>\n",
        "<template>\n",
        "  <div v-slot></div>\n",
        "</template>\n",
    );
    let artifact = registered_artifact("file:///dup-diag.vue", SOURCE, false);
    let request = vue_ide_request("DupDiag.vue");
    let companion = project_ide_from_catalog(
        ide_grant(),
        &artifact,
        SOURCE,
        &request,
        &ProjectionCatalogInputs::default(),
    )
    .expect("catalog companion");
    assert!(
        companion
            .diagnostics
            .iter()
            .any(|d| d.message.contains("defineProps")),
        "fixture must emit the setup-scope macro diagnostic: {companion_codes:?}",
        companion_codes = companion
            .diagnostics
            .iter()
            .map(|d| format!("{}:{}", d.code, d.message))
            .collect::<Vec<_>>()
    );
    assert!(
        companion
            .diagnostics
            .iter()
            .any(|d| d.code.contains("VSlot") || d.message.contains("v-slot")),
        "fixture must emit the template v-slot diagnostic"
    );

    let bundle = produced_bundle(
        &VueCarrierCompiler,
        SOURCE,
        &artifact,
        &RuntimeCompileOptions {
            filename: Some("DupDiag.vue".to_string()),
            want_runtime: false,
            want_ide: true,
            ..Default::default()
        },
    );

    let catalog: Vec<_> = companion
        .diagnostics
        .iter()
        .map(|d| (d.code.as_str(), d.message.as_str(), d.span))
        .collect();
    let bundled: Vec<_> = bundle
        .diagnostics
        .iter()
        .map(|d| (d.code.as_str(), d.message.as_str(), d.span))
        .collect();
    assert_eq!(
        bundled, catalog,
        "IDE-only compile_bundle.diagnostics must equal the catalog companion (code, message, span, order)"
    );

    for (index, diagnostic) in bundle.diagnostics.iter().enumerate() {
        let copies = bundle
            .diagnostics
            .iter()
            .filter(|other| {
                other.code == diagnostic.code
                    && other.message == diagnostic.message
                    && other.span == diagnostic.span
            })
            .count();
        assert_eq!(
            copies, 1,
            "diagnostic {index} ({:?}) must appear once, not {copies} times",
            diagnostic.code
        );
    }
}

fn diagnostic_identity(diagnostic: &RuntimeDiagnostic) -> (&str, &str, verter_span::Span) {
    (
        diagnostic.code.as_str(),
        diagnostic.message.as_str(),
        diagnostic.span,
    )
}

fn format_diagnostic_identities(diagnostics: &[RuntimeDiagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            format!(
                "{}:{}:{:?}",
                diagnostic.code, diagnostic.message, diagnostic.span
            )
        })
        .collect()
}

/// Combined runtime+IDE `compile_bundle` unique-merges catalog diagnostics
/// onto the runtime prefix. Shared `(code, message, span)` identity stays
/// once as the runtime instance; catalog-only rows append once in catalog
/// order. A naive `.extend()` would duplicate every overlapping identity
/// into the suffix, and replacing the runtime vector with the catalog
/// companion would drop runtime-only diagnostics.
#[test]
fn vue_compile_bundle_runtime_and_ide_merges_unique_diagnostics() {
    const SOURCE: &str = concat!(
        "<script setup>\n",
        "let n = 1\n",
        "defineProps({ n })\n",
        "</script>\n",
        "<template>\n",
        "  <div v-slot></div>\n",
        "  <div>{{ count + }}</div>\n",
        "</template>\n",
    );
    let artifact = registered_artifact("file:///unique-diag.vue", SOURCE, false);
    let request = vue_ide_request("UniqueDiag.vue");
    let opts = RuntimeCompileOptions {
        filename: Some("UniqueDiag.vue".to_string()),
        want_runtime: true,
        want_ide: false,
        ..Default::default()
    };

    let runtime = produced_bundle(&VueCarrierCompiler, SOURCE, &artifact, &opts);
    assert!(
        runtime.tsx.is_none(),
        "runtime-only compile must not publish an IDE product"
    );
    assert!(
        runtime.script.is_some() || runtime.template.is_some(),
        "runtime-only compile must publish a runtime product"
    );

    let companion = project_ide_from_catalog(
        ide_grant(),
        &artifact,
        SOURCE,
        &request,
        &ProjectionCatalogInputs::default(),
    )
    .expect("catalog companion");

    let runtime_ids: Vec<_> = runtime
        .diagnostics
        .iter()
        .map(diagnostic_identity)
        .collect();
    let catalog_ids: Vec<_> = companion
        .diagnostics
        .iter()
        .map(diagnostic_identity)
        .collect();
    let overlap: Vec<_> = catalog_ids
        .iter()
        .copied()
        .filter(|identity| runtime_ids.contains(identity))
        .collect();
    let catalog_only: Vec<_> = catalog_ids
        .iter()
        .copied()
        .filter(|identity| !runtime_ids.contains(identity))
        .collect();
    assert!(
        !overlap.is_empty(),
        "fixture must produce a runtime/catalog-overlapping diagnostic; runtime={:?} catalog={:?}",
        format_diagnostic_identities(&runtime.diagnostics),
        format_diagnostic_identities(&companion.diagnostics)
    );
    assert!(
        runtime_ids
            .iter()
            .any(|identity| !catalog_ids.contains(identity)),
        "fixture must produce a runtime-only diagnostic so replacement with the catalog companion cannot masquerade as a unique merge; runtime={:?} catalog={:?}",
        format_diagnostic_identities(&runtime.diagnostics),
        format_diagnostic_identities(&companion.diagnostics)
    );

    let _ = take_projection_producer_invocations();
    let combined = produced_bundle(
        &VueCarrierCompiler,
        SOURCE,
        &artifact,
        &RuntimeCompileOptions {
            want_ide: true,
            ..opts
        },
    );
    assert_eq!(
        take_projection_producer_invocations(),
        1,
        "catalog projection must run exactly once for the combined runtime+IDE compile"
    );
    assert!(
        combined.tsx.is_some(),
        "combined compile must publish the IDE product"
    );
    assert!(
        combined.script.is_some() || combined.template.is_some(),
        "combined compile must publish a runtime product"
    );

    let combined_ids: Vec<_> = combined
        .diagnostics
        .iter()
        .map(diagnostic_identity)
        .collect();
    assert!(
        combined_ids.len() >= runtime_ids.len(),
        "runtime diagnostics must remain the prefix; runtime={:?} combined={:?}",
        format_diagnostic_identities(&runtime.diagnostics),
        format_diagnostic_identities(&combined.diagnostics)
    );
    assert_eq!(
        &combined.diagnostics[..runtime.diagnostics.len()],
        &runtime.diagnostics[..],
        "runtime diagnostic order must remain the prefix, retaining the runtime instance"
    );
    assert_eq!(
        &combined_ids[runtime_ids.len()..],
        catalog_only.as_slice(),
        "catalog-only diagnostics must append once in catalog order; a naive extend would also append overlapping identities. runtime={:?} catalog={:?} combined={:?}",
        format_diagnostic_identities(&runtime.diagnostics),
        format_diagnostic_identities(&companion.diagnostics),
        format_diagnostic_identities(&combined.diagnostics)
    );

    for identity in &overlap {
        let copies = combined_ids
            .iter()
            .filter(|other| *other == identity)
            .count();
        assert_eq!(
            copies, 1,
            "shared identity {identity:?} must appear once, retaining the runtime instance; a naive extend would duplicate it"
        );
    }
    for (index, identity) in combined_ids.iter().enumerate() {
        let copies = combined_ids
            .iter()
            .filter(|other| *other == identity)
            .count();
        assert_eq!(
            copies, 1,
            "diagnostic {index} {identity:?} must appear once, not {copies} times"
        );
    }
}

#[test]
fn runtime_only_bundle_does_not_invoke_projection() {
    let artifact = registered_artifact("file:///runtime.vue", VUE_SIMPLE, false);
    let _ = take_projection_producer_invocations();
    let bundle = produced_bundle(
        &VueCarrierCompiler,
        VUE_SIMPLE,
        &artifact,
        &RuntimeCompileOptions {
            filename: Some("Runtime.vue".to_string()),
            want_runtime: true,
            want_ide: false,
            ..Default::default()
        },
    );
    assert!(bundle.tsx.is_none(), "runtime-only must not publish IDE");
    assert_eq!(take_projection_producer_invocations(), 0);
}

#[test]
fn rejected_projection_does_not_publish_an_ide_product() {
    let artifact = registered_artifact("file:///reject.vue", VUE_SIMPLE, false);
    let _ = take_projection_producer_invocations();
    let err = VueCarrierCompiler
        .compile_ide(
            "<script setup>const n = 2</script>",
            &artifact,
            &IdeCompileOptions {
                filename: Some("Reject.vue".to_string()),
                ..Default::default()
            },
        )
        .expect_err("mismatched source must refuse");
    match err {
        CompileUnsupported::NoIdeProjection { .. } => {}
        other => panic!("expected NoIdeProjection, got {other:?}"),
    }
    let invocations = take_projection_producer_invocations();
    assert!(
        invocations <= 1,
        "a refused projection must not retry or duplicate, got {invocations}"
    );
}

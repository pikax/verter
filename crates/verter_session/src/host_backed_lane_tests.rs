//! Host-backed bound compile-lane tests: the multi-product route consumes
//! its request-scoped bound host request — the SAME artifact and demand set
//! for admission and execution.
//!
//! Two structural/discriminating proofs:
//! - attribution injection: a binding bound for a DIFFERENT file trips the
//!   lane's bound-attribution gate before any admission or execution;
//! - admitted-demand-set execution: the executed multi-product population
//!   is exactly the admitted one, byte-checked per product against a
//!   directly-driven registered-backend oracle, with the sibling-absence
//!   negatives (an unadmitted product never publishes).

use std::sync::Arc;

use crate::host_resolve::CompileEntryOutcome;
use crate::types::{CompileProfile, DiagnosticsSnapshot, FileLanguage, VirtualNodeKind};
use crate::{CompileTarget, HostConfig, UpsertRequest, VerterHost};

fn new_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert(host: &VerterHost, canonical_id: &str, source: &str) {
    let language = if canonical_id.ends_with(".svelte") {
        FileLanguage::svelte()
    } else {
        FileLanguage::vue()
    };
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::from(source),
            file_language: language,
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert {canonical_id}: {e:?}"));
}

/// A minimal well-formed `CompileInput` over the live snapshot of
/// `canonical_id` — the same construction shape the compile routes feed
/// `compile_entry` with for a simple (no external `src=`, no override)
/// carrier.
fn compile_input(host: &VerterHost, canonical_id: &str) -> crate::types::CompileInput {
    let snap = host
        .scheduler
        .try_get_source(canonical_id)
        .expect("the upserted source is live");
    let efs = host
        .effective_file_state_from_snapshot(&snap, canonical_id, None)
        .expect("the upserted source carries host data");
    crate::types::CompileInput {
        canonical_id: canonical_id.to_string(),
        source: efs.source,
        whole_hash: efs.whole_hash,
        meta: efs.meta,
        parse_diagnostics: DiagnosticsSnapshot::default(),
        src_blocks: Vec::new(),
        external_requests: Vec::new(),
        has_supplied_block_content: false,
        block_content_inputs: Default::default(),
        macro_type_deps: Vec::new(),
        script_imports: Vec::new(),
        script_macros: Vec::new(),
        script_bindings: Vec::new(),
        script_macro_usage: None,
        script_vue_api_calls: Vec::new(),
        framework_parse: efs.framework_parse,
        style_v_bind_vars: Vec::new(),
        style_v_bind_usage_complete: true,
        prepared_styles: Vec::new(),
    }
}

fn bind(
    host: &VerterHost,
    canonical_id: &str,
) -> crate::host_resolve::native_host_binding::BoundNativeHostRequest {
    let snap = host
        .scheduler
        .try_get_source(canonical_id)
        .expect("the source is live");
    let efs = host
        .effective_file_state_from_snapshot(&snap, canonical_id, None)
        .expect("the source carries host data");
    host.bind_native_host_compile_attempt(
        efs.framework_parse.as_deref(),
        canonical_id,
        snap.source.len() as u32,
        &snap,
        crate::types::CompileCacheMode::Session,
    )
    .expect("the registered identity binds")
    .expect("a carrier registers a framework parse artifact")
}

const VUE_SRC: &str = "<script setup lang=\"ts\">\nconst n = 1\n</script>\n<template><div class=\"a\">{{ n }}</div></template>\n<style scoped>.a { color: red }</style>\n";

// ---------------------------------------------------------------------------
// Proof 1 — the artifact admitted/executed must be the BOUND one
// ---------------------------------------------------------------------------

/// Injecting a binding bound for a DIFFERENT file's snapshot into the
/// host-backed execution trips the lane's bound-attribution invariant
/// before any admission or execution: the bound snapshot identity must
/// name the executed request's canonical id. DISCRIMINATING against the
/// admission gate itself: both files are Vue, so a lane without this gate
/// would compose a VALID admission over the presented artifact and
/// execute it under the foreign request's attribution — no refusal, no
/// panic — and this test would fail.
#[test]
#[should_panic(
    expected = "host-backed bound attribution must name the executed request's \
                           canonical id"
)]
fn host_backed_bound_attribution_must_name_the_executed_artifact() {
    let host = new_host();
    let executed = "/proj/HostBackedAttributionA.vue";
    let foreign = "/proj/HostBackedAttributionB.vue";
    upsert(&host, executed, VUE_SRC);
    upsert(&host, foreign, "<template><div>other</div></template>\n");

    let input = compile_input(&host, executed);
    let foreign_binding = bind(&host, foreign);

    // The lane must trip its bound-attribution invariant rather than admit
    // or execute the mismatched pairing.
    let _ = host.compile_entry(&input, &CompileProfile::default(), Some(foreign_binding));
}

// ---------------------------------------------------------------------------
// Proof 2 — the executed product population is exactly the admitted one
// ---------------------------------------------------------------------------

/// The host-backed MULTI-PRODUCT execution consumes ONE admission whose
/// demand set is exactly what executes:
///
/// - per-product byte oracles: the published Script / Style / IDE payloads
///   of one runtime+IDE transaction byte-match a DIRECT registered-backend
///   drive (`admit_host_products` → `compile_host_products`) of the same
///   artifact under the same demand — the session lane publishes the bound
///   backend's own admitted population, nothing else;
/// - one population per demand: the runtime leg's assembled `Main` bytes
///   are identical with and without the sibling IDE product in the demand;
/// - sibling absence: a runtime-only demand publishes NO IDE payload, and
///   an IDE-only demand publishes NO runtime node — an unadmitted product
///   never publishes.
///
/// DISCRIMINATING: a lane that executed a second, differently-shaped
/// compile for one of the products (or served a product its admission
/// never carried) diverges on at least one of the byte/absence checks.
#[test]
fn host_backed_multi_product_executes_exactly_the_admitted_population() {
    use verter_compiler::compile_request::{
        CompileProduct, IdeProductRequest, RuntimeProductRequest, VueBackendRequest,
        VueOptionAttempt,
    };
    use verter_compiler::framework_common::{
        FrameworkHostIntegrationBackend as _, VueHostExecutionInputs, VueHostMultiProductDemand,
    };

    let host = new_host();
    let canonical = "/proj/HostBackedPopulation.vue";
    upsert(&host, canonical, VUE_SRC);
    let input = compile_input(&host, canonical);

    let multi_profile = CompileProfile {
        target: CompileTarget::BUNDLER | CompileTarget::IDE,
        ..CompileProfile::default()
    };

    // The session lane's multi-product transaction.
    let produced = match host
        .compile_entry(&input, &multi_profile, Some(bind(&host, canonical)))
        .expect("the multi-product transaction produces")
    {
        CompileEntryOutcome::Produced(produced) => produced,
        CompileEntryOutcome::RuntimeSurfaceRefused(refusal) => {
            panic!("unexpected runtime-surface refusal: {}", refusal.message)
        }
    };
    let script = &produced.outputs[&VirtualNodeKind::Script];
    let style = &produced.outputs[&VirtualNodeKind::Style { index: 0 }];
    let tsx = produced
        .tsx
        .as_ref()
        .expect("the admitted IDE product publishes its payload");
    assert!(
        produced.outputs.contains_key(&VirtualNodeKind::Main),
        "the admitted runtime product publishes the assembled Main"
    );

    // The DIRECT registered-backend oracle: the same artifact, the same
    // demand set, driven straight through the backend's admission +
    // by-value execution — independently of the session lane.
    let binding = bind(&host, canonical);
    let crate::host_resolve::native_host_binding::BoundNativeHostRequest::Vue(vue) = binding else {
        panic!("a Vue carrier binds the Vue catalog arm");
    };
    let (backend, _attribution) = vue.into_host_backend();
    let artifact = input
        .framework_parse
        .as_deref()
        .expect("a Vue carrier registers a framework parse artifact");
    let want_template_data = host.config.effective_scope().needs_template_analysis();
    let mut products = vec![
        CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
        CompileProduct::IdeCompanion(IdeProductRequest::default()),
    ];
    if want_template_data {
        products.push(CompileProduct::Analysis(
            verter_compiler::compile_request::AnalysisProductRequest {
                want_script_bindings: false,
                want_template_data: true,
            },
        ));
    }
    let macro_output = host.produce_vue_macro_codegen(
        canonical,
        crate::typeinfo::vue_macro_codegen::VueMacroCodegenDemand::for_compile_target(
            multi_profile.target,
        )
        .unwrap_or(crate::typeinfo::vue_macro_codegen::VueMacroCodegenDemand::RuntimeBindingNames),
    );
    let vue_facts = verter_compiler::compile::types::VueExecutionInputs {
        macro_runtime: macro_output.runtime,
        style_v_bind_usage_complete: Some(true),
        ..Default::default()
    };
    let demand = VueHostMultiProductDemand {
        products,
        vue_options: VueOptionAttempt {
            backend: VueBackendRequest::Inferred,
            runtime_module_name: Some("vue".to_string()),
            script_custom_element: Some(false),
            ..Default::default()
        },
        filename: Some(canonical.to_string()),
        component_id: None,
        is_production: false,
        force_js: false,
    };
    let alloc = oxc_allocator::Allocator::new();
    let admission = backend
        .admit_host_products(artifact, demand)
        .expect("the oracle demand admits");
    let oracle = backend
        .compile_host_products(
            admission,
            artifact,
            &VueHostExecutionInputs {
                vue_facts: Some(vue_facts),
                ..Default::default()
            },
            &alloc,
        )
        .expect("the oracle execution produces");
    let oracle_bundle = oracle
        .runtime_client_bundle()
        .expect("the admitted client runtime bundle publishes");
    assert_eq!(
        script.code.as_ref(),
        oracle_bundle
            .script
            .as_ref()
            .expect("the oracle bundle carries the script block")
            .code
            .as_str(),
        "the published Script bytes must be the bound backend's own admitted script product"
    );
    assert_eq!(
        style.code.as_ref(),
        oracle_bundle.styles[0].code.as_str(),
        "the published Style bytes must be the bound backend's own admitted style product"
    );
    assert_eq!(
        tsx.code.as_ref(),
        oracle
            .ide_companion()
            .expect("the admitted IDE product publishes on the oracle too")
            .code
            .as_str(),
        "the published IDE bytes must be the bound backend's own admitted IDE product"
    );

    // One population per demand: the sibling IDE product must not perturb
    // the runtime leg's bytes.
    let runtime_only = CompileProfile {
        target: CompileTarget::BUNDLER,
        ..CompileProfile::default()
    };
    let runtime_produced = match host
        .compile_entry(&input, &runtime_only, Some(bind(&host, canonical)))
        .expect("the runtime-only transaction produces")
    {
        CompileEntryOutcome::Produced(produced) => produced,
        CompileEntryOutcome::RuntimeSurfaceRefused(refusal) => {
            panic!("unexpected runtime-surface refusal: {}", refusal.message)
        }
    };
    assert_eq!(
        produced.outputs[&VirtualNodeKind::Main].code,
        runtime_produced.outputs[&VirtualNodeKind::Main].code,
        "the runtime leg executes the same admitted population with or without the \
         sibling IDE demand"
    );
    // Sibling absence: a runtime-only demand admits no IDE product, so no
    // IDE payload publishes.
    assert!(
        runtime_produced.tsx.is_none(),
        "a runtime-only demand must not publish an IDE payload"
    );

    // Sibling absence, other direction: an IDE-only demand admits no
    // runtime product, so no runtime virtual node publishes.
    let ide_only = CompileProfile {
        target: CompileTarget::IDE,
        ..CompileProfile::default()
    };
    let ide_produced = match host
        .compile_entry(&input, &ide_only, Some(bind(&host, canonical)))
        .expect("the IDE-only transaction produces")
    {
        CompileEntryOutcome::Produced(produced) => produced,
        CompileEntryOutcome::RuntimeSurfaceRefused(refusal) => {
            panic!("unexpected runtime-surface refusal: {}", refusal.message)
        }
    };
    assert!(
        ide_produced.outputs.is_empty(),
        "an IDE-only demand must publish no runtime virtual node, got {:?}",
        ide_produced.outputs.keys().collect::<Vec<_>>()
    );
    let ide_tsx = ide_produced
        .tsx
        .expect("the IDE-only demand publishes its admitted IDE payload");
    assert_eq!(
        ide_tsx.code, tsx.code,
        "the IDE leg executes the same admitted population with or without the \
         sibling runtime demand"
    );
}

// ---------------------------------------------------------------------------
// Proof 3 — `ssr` is an axis of the runtime product, not a global gate
// ---------------------------------------------------------------------------

/// `ssr` selects WHICH runtime bundle a profile demands. A profile that
/// demands no runtime product at all — IDE/TSX only — runs no runtime
/// leg, so the axis drives nothing and must not gate admission: the
/// profile serves, and publishes its IDE payload, whatever `ssr` says.
///
/// DISCRIMINATING — the admission leg: a route that validated the axis
/// against the demanded runtime kind unconditionally refuses the
/// `ssr: true` iteration at admission and publishes no TSX at all, so
/// this test fails at the panic below.
///
/// The cross-iteration byte-equality check is NOT independent coverage of
/// an ssr-insensitive IDE projection, and is not presented as such: the
/// admitted runtime options derive their ssr mode from the ADMITTED
/// PRODUCT SET, so an IDE-only demand derives the same mode on both
/// iterations and the two payloads are equal by construction. It is kept
/// as a consistency check on that construction, next to a non-empty check
/// that both iterations publish REAL bytes rather than an empty payload.
/// It would not catch an IDE projection newly made to read a
/// profile-sourced ssr flag.
#[test]
fn an_ide_only_profile_publishes_its_tsx_whatever_the_ssr_axis_says() {
    let host = new_host();
    let canonical = "/proj/IdeOnlySsrAxis.vue";
    upsert(&host, canonical, VUE_SRC);
    let input = compile_input(&host, canonical);

    let mut published = Vec::new();
    for ssr in [false, true] {
        let profile = CompileProfile {
            target: CompileTarget::IDE,
            ssr,
            ..CompileProfile::default()
        };
        let produced = match host
            .compile_entry(&input, &profile, Some(bind(&host, canonical)))
            .unwrap_or_else(|diagnostics| {
                panic!(
                    "an IDE-only profile with ssr={ssr} must publish, got {:?}",
                    diagnostics
                        .diagnostics
                        .iter()
                        .map(|d| (d.code.clone(), d.message.clone()))
                        .collect::<Vec<_>>()
                )
            }) {
            CompileEntryOutcome::Produced(produced) => produced,
            CompileEntryOutcome::RuntimeSurfaceRefused(refusal) => panic!(
                "an IDE-only profile with ssr={ssr} demands no runtime surface to refuse: {} {}",
                refusal.diagnostic_code, refusal.message
            ),
        };
        assert!(
            produced.outputs.is_empty(),
            "no runtime product is demanded, so no runtime node publishes"
        );
        published.push(
            produced
                .tsx
                .expect("the admitted IDE product publishes its payload")
                .code,
        );
    }
    assert!(
        !published[0].is_empty(),
        "the admitted IDE product publishes real TSX bytes, not an empty payload"
    );
    assert_eq!(
        published[0], published[1],
        "the admitted runtime mode is derived from the product set, so an IDE-only demand \
         derives the same mode on both iterations and publishes the same bytes"
    );
}

/// The empty-demand placeholder rides the same rail: a profile demanding
/// NO product still publishes, because the synthesized runtime product
/// carries the profile's own ssr kind rather than a hard-coded client
/// kind that would contradict the axis at admission.
///
/// DISCRIMINATING: a placeholder pinned to the client kind refuses this
/// `ssr: true` profile at admission and publishes nothing.
#[test]
fn a_profile_demanding_no_product_publishes_under_either_ssr_axis() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        // No template-fact bit: the analysis product is undemanded too, so
        // the profile below demands nothing at all.
        analysis_scope: Some(verter_semantic::analysis::AnalysisScope::empty()),
        ..HostConfig::default()
    }));
    let canonical = "/proj/EmptyDemandSsrAxis.vue";
    upsert(&host, canonical, VUE_SRC);
    assert!(
        !host.config.effective_scope().needs_template_analysis(),
        "this scope must not demand template facts, or the placeholder branch is unreached"
    );
    let input = compile_input(&host, canonical);

    for ssr in [false, true] {
        let profile = CompileProfile {
            target: CompileTarget::empty(),
            ssr,
            ..CompileProfile::default()
        };
        let produced = match host
            .compile_entry(&input, &profile, Some(bind(&host, canonical)))
            .unwrap_or_else(|diagnostics| {
                panic!(
                    "a zero-demand profile with ssr={ssr} must publish its placeholder, got {:?}",
                    diagnostics
                        .diagnostics
                        .iter()
                        .map(|d| (d.code.clone(), d.message.clone()))
                        .collect::<Vec<_>>()
                )
            }) {
            CompileEntryOutcome::Produced(produced) => produced,
            CompileEntryOutcome::RuntimeSurfaceRefused(refusal) => panic!(
                "the placeholder runtime product must be producible, got {} {}",
                refusal.diagnostic_code, refusal.message
            ),
        };
        assert!(
            produced.outputs.is_empty() && produced.tsx.is_none(),
            "nothing was demanded, so nothing publishes — the placeholder exists only to \
             keep canonical request construction admissible, got {:?}",
            produced.outputs.keys().collect::<Vec<_>>()
        );
    }
}

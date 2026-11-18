//! The session side of the bound host compile routes: demand
//! construction, bound-backend execution dispatch over prepared execution
//! inputs, refusal mapping, and the result-carrier plumbing shared by
//! [`crate::host_resolve::virtual_file_pipeline`]'s compile lanes.
//!
//! Execution inputs are prepared before any function here is called
//! ([`super::compile_inputs`]); nothing here waits on a dependency.
//!
//! The two profile-derived lanes construct no session-side
//! `CompileRequest`: each builds the framework host backend's own typed
//! demand from the profile axes — the host-backed multi-product demand
//! ([`vue_host_products_demand`] / [`svelte_host_products_demand`],
//! executed through [`execute_bound_host_products`]) or the render-only
//! demand ([`vue_runtime_render_demand`] /
//! [`svelte_runtime_render_demand`]) — and the bound backend composes and
//! admission-checks the canonical request inside its issued consume-once
//! admission.
//!
//! The caller-supplied-request lane ([`execute_supplied_host_request`])
//! is the third route and inverts only that step: the caller's canonical
//! request IS the demand document, admitted verbatim, and NOTHING on that
//! route derives a demand from a `CompileProfile` or reconstructs one from
//! the request. It shares every other stage — the same prepared execution
//! inputs, the same bound backend, the same consume-once admission, the
//! same single product execution.
//!
//! There is no shared framework fork here: the demand constructor is
//! selected by the request-scoped native host binding's catalog arm (the
//! sole framework-identity derivation site for host compile requests),
//! and the two profile-derived lanes share the same per-framework
//! option-attempt builders so they can never diverge on which profile
//! axes reach option admission.

use super::compile_inputs::PreparedExecutionInputs;
use crate::types::*;
use verter_compiler::framework_common::{
    SvelteHostExecutionInputs, SvelteHostMultiProductDemand, SvelteHostRuntimeRenderDemand,
    SvelteSuppliedStyle, VueHostMultiProductDemand, VueHostRuntimeRenderDemand,
};

/// The demanded product set shared by both framework constructors.
///
/// `want_runtime`/`want_ide`/`want_template_data` are the caller's already-
/// computed demand booleans (unchanged from the pre-existing
/// `RuntimeCompileOptions` construction); request construction's own job is
/// ADMISSION, not re-deriving what output the caller wants.
fn demanded_products(
    profile: &CompileProfile,
    want_runtime: bool,
    want_ide: bool,
    want_template_data: bool,
) -> Vec<verter_compiler::compile_request::CompileProduct> {
    use verter_compiler::compile_request::{
        AnalysisProductRequest, CompileProduct, IdeProductRequest,
    };

    let mut products = Vec::new();
    if want_runtime {
        products.push(runtime_product(profile));
    }
    if want_ide {
        products.push(CompileProduct::IdeCompanion(IdeProductRequest {
            want_source_map: profile.source_map,
            embed_ambient_types: profile.embed_ambient_types,
            conditional_root_narrowing: profile.conditional_root_narrowing,
            strict_slots: profile.strict_slots,
            types_module_name: profile.types_module_name.clone(),
            ..Default::default()
        }));
    }
    if want_template_data {
        products.push(CompileProduct::Analysis(AnalysisProductRequest {
            want_script_bindings: false,
            want_template_data: true,
        }));
    }
    if products.is_empty() {
        // No product actually demanded by this call. The placeholder
        // exists for the same reason as the one in
        // `host_compile_audit::request_from_target` — `CompileRequest::new`
        // refuses an empty product set outright, and every caller of either
        // route has always received SOME compiled surface, never a
        // deliberate zero-output compile — but the two select their
        // runtime KIND differently, and must. That route's overrides carry
        // no ssr axis at all, so its placeholder is unconditionally the
        // client kind; this route's profile does carry one, so its
        // placeholder is built by the SAME constructor the demanded branch
        // uses and therefore can never contradict the axis the caller
        // asked for.
        products.push(runtime_product(profile));
    }
    products
}

/// The one runtime product a profile demands. Its KIND is the profile's
/// `ssr` axis at every construction site, so no construction path can
/// hand the backend a runtime product whose kind disagrees with the axis
/// the caller asked for.
fn runtime_product(profile: &CompileProfile) -> verter_compiler::compile_request::CompileProduct {
    use verter_compiler::compile_request::{CompileProduct, RuntimeProductRequest};

    let request = RuntimeProductRequest {
        inline: profile.inline,
        runtime_source_map: profile.source_map,
        ..Default::default()
    };
    if profile.ssr {
        CompileProduct::RuntimeServer(request)
    } else {
        CompileProduct::RuntimeClient(request)
    }
}

/// The host-backed multi-product Vue-BOUND demand: every requested
/// product plus every Vue option axis the profile carries, riding the
/// same typed attempt the render lane's demand builds from. The bound
/// backend owns canonical request construction and admission — an
/// unrecognized option value or combination refuses typed at issuance,
/// before any codegen input is built. The caller selects this constructor
/// from its request-scoped native host binding's Vue arm — never from
/// language classification.
pub(crate) fn vue_host_products_demand(
    profile: &CompileProfile,
    canonical_id: &str,
    want_runtime: bool,
    want_ide: bool,
    want_template_data: bool,
) -> VueHostMultiProductDemand {
    VueHostMultiProductDemand {
        products: demanded_products(profile, want_runtime, want_ide, want_template_data),
        vue_options: vue_option_attempt_from_profile(profile),
        filename: profile
            .filename
            .clone()
            .or_else(|| Some(canonical_id.to_string())),
        component_id: profile.component_id.clone(),
        is_production: profile.is_production,
        force_js: profile.force_js,
        hmr_strategy: crate::compile::hmr_strategy(profile.hmr_strategy),
        ssr_module_id: profile.ssr_module_id.clone(),
        emit_ssr_module_registration: profile.emit_ssr_module_registration,
    }
}

/// The typed Vue option attempt every Vue option the session
/// `CompileProfile` carries maps onto — shared by the host-backed request
/// constructor and the runtime-render bound demand so the two routes can
/// never diverge on which profile axes reach Vue option admission.
fn vue_option_attempt_from_profile(
    profile: &CompileProfile,
) -> verter_compiler::compile_request::VueOptionAttempt {
    use verter_compiler::compile_request::{VueBackendRequest, VueOptionAttempt};
    VueOptionAttempt {
        backend: if profile.force_vapor {
            VueBackendRequest::Vapor
        } else {
            VueBackendRequest::Inferred
        },
        ssr: profile.ssr,
        is_custom_element: profile.custom_elements.clone().unwrap_or_default(),
        delimiters: profile.delimiters.clone(),
        comments: profile.comments,
        runtime_module_name: profile.runtime_module_name.clone(),
        script_custom_element: Some(profile.custom_element),
        ..Default::default()
    }
}

/// The host-backed compile lane's ONE bound execution: the consumed
/// binding's catalog arm yields the backend and takes its own framework's
/// prepared execution inputs, the backend issues the demand-specific
/// multi-product admission over the SAME presented artifact, and the
/// product execution consumes that admission by value — one request, one
/// backend call, one admitted parse/semantic/projection/plan/emit
/// population. The lane holds no framework selector, no session-side
/// request construction, and no registry dispatch; issuance and execution
/// are paired by the admission's parse key, so admitting one artifact and
/// executing another is unrepresentable at this seam.
///
/// Framework EXECUTION INPUTS arrive prepared and complete
/// ([`super::compile_inputs::prepare_execution_inputs`]): the caller
/// resolved the cross-file semantic inputs, restated the dependency axis
/// and settled the dependency verdict before this call, which waits on
/// nothing. Each arm takes only its own framework's inputs; inputs
/// prepared for another framework are refused with the execution-refused
/// diagnostic, never compiled.
///
/// Every refusal is typed and fail-closed — never a fallback lane,
/// framework, or compatibility compiler:
/// - a session-side demand-decode refusal (a malformed Svelte option
///   token) and a backend construction refusal keep the
///   `HOST_COMPILE_REQUEST_EXECUTION_REFUSED` code;
/// - every other issuance refusal (unavailable capability, unsupported or
///   unproducible demand, non-composable parse) surfaces as
///   `HOST_COMPILE_ADMISSION_REFUSED`;
/// - a refused runtime surface is the typed all-or-none
///   [`HostProductsFailure::Surface`] arm — no sibling product publishes
///   after it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_bound_host_products(
    binding: super::native_host_binding::BoundNativeHostRequest,
    artifact: &verter_compiler::framework_common::FrameworkParseArtifact,
    profile: &CompileProfile,
    snapshot: &CompileInput,
    inputs: PreparedExecutionInputs,
    want_runtime: bool,
    want_ide: bool,
    want_template_data: bool,
    want_main: bool,
    alloc: &oxc_allocator::Allocator,
) -> Result<BoundCompiledProducts, HostProductsFailure> {
    use super::native_host_binding::BoundNativeHostRequest;
    use crate::host_compile_audit::{debug_assert_compile_bound_attribution, BoundCompileRoute};
    use verter_compiler::framework_common::FrameworkHostIntegrationBackend as _;

    let source_len = snapshot.source.len() as u32;
    match binding {
        BoundNativeHostRequest::Vue(bound) => {
            let (backend, attribution) = bound.into_host_backend();
            debug_assert_compile_bound_attribution(
                BoundCompileRoute::HostBacked,
                &attribution,
                artifact,
                &snapshot.canonical_id,
            );
            let PreparedExecutionInputs::Vue(mut inputs) = inputs else {
                return Err(HostProductsFailure::Fatal(
                    inputs_for_another_framework_diagnostics(&snapshot.canonical_id, source_len),
                ));
            };
            inputs.want_main = want_main;
            let demand = vue_host_products_demand(
                profile,
                &snapshot.canonical_id,
                want_runtime,
                want_ide,
                want_template_data,
            );
            let admission = backend
                .admit_host_products(artifact, demand)
                .map_err(|refusal| {
                    HostProductsFailure::Fatal(vue_admission_refused_diagnostics(
                        &snapshot.canonical_id,
                        source_len,
                        refusal,
                    ))
                })?;
            backend
                .compile_host_products(admission, artifact, &inputs, alloc)
                .map(BoundCompiledProducts::Vue)
                .map_err(|refusal| {
                    vue_products_execution_failure(
                        artifact,
                        &snapshot.canonical_id,
                        source_len,
                        refusal,
                    )
                })
        }
        BoundNativeHostRequest::Svelte(bound) => {
            let (backend, attribution) = bound.into_host_backend();
            debug_assert_compile_bound_attribution(
                BoundCompileRoute::HostBacked,
                &attribution,
                artifact,
                &snapshot.canonical_id,
            );
            let PreparedExecutionInputs::Svelte(inputs) = inputs else {
                return Err(HostProductsFailure::Fatal(
                    inputs_for_another_framework_diagnostics(&snapshot.canonical_id, source_len),
                ));
            };
            // The Svelte-bound demand decodes the profile's Svelte option
            // tokens through the SAME typed decode boundary the render
            // lane uses: a malformed token refuses HERE, never a silent
            // default.
            let demand = svelte_host_products_demand(
                profile,
                &snapshot.canonical_id,
                want_runtime,
                want_ide,
                want_template_data,
            )
            .map_err(|error| {
                HostProductsFailure::Fatal(request_construction_refused_diagnostics(
                    &snapshot.canonical_id,
                    source_len,
                    &error,
                ))
            })?;
            let admission = backend
                .admit_host_products(artifact, demand)
                .map_err(|refusal| {
                    HostProductsFailure::Fatal(svelte_admission_refused_diagnostics(
                        &snapshot.canonical_id,
                        source_len,
                        refusal,
                    ))
                })?;
            backend
                .compile_host_products(admission, artifact, &inputs, alloc)
                .map(BoundCompiledProducts::Svelte)
                .map_err(|refusal| {
                    svelte_products_execution_failure(
                        artifact,
                        &snapshot.canonical_id,
                        source_len,
                        refusal,
                    )
                })
        }
    }
}

/// The Svelte execution inputs every host route hands the compiler.
///
/// A supplied style slot — a completed external preprocessing result the
/// host admitted — is handed over ONCE, as the stage-qualified result the
/// compiler binds to its block: the produced bytes, their producer, the
/// host-minted identity of those bytes, the host's produced-to-authored map
/// of them, the host's parse of them, and the
/// content identity of the authored bytes the selection was validated
/// against. Every one of those facts is read off the projection the
/// block-content capture fence stamped, so the compiler binds the revision
/// the host actually validated rather than whatever is live when it runs.
///
/// The raw style slots never reach the Svelte compiler, so no stage can fall
/// back to them. That holds for EVERY present slot, including one the host
/// names no external producer for (a carrier-own or `src`-external
/// selection): it continues its block anonymously and is refused if it
/// cannot, rather than being dropped back onto the authored block silently.
pub(crate) fn svelte_host_execution_inputs(
    snapshot: &CompileInput,
    css_hash_override: Option<String>,
) -> SvelteHostExecutionInputs {
    let mut block_content = snapshot.block_content_inputs.clone();
    let supplied_styles = std::mem::take(&mut block_content.styles)
        .into_iter()
        .map(|slot| slot.map(svelte_supplied_style))
        .collect();
    SvelteHostExecutionInputs {
        block_content,
        css_hash_override,
        prepared_styles: snapshot.prepared_styles.clone(),
        supplied_styles,
    }
}

/// One selected style slot, as the stage-qualified result the compiler binds
/// to its block.
///
/// EVERY present slot maps. A slot the host names no external producer for
/// is still a selection the host made and validated, so it continues its
/// block as an anonymous result: the compiler then binds it, or refuses.
/// Discarding it would leave the compiler no result for a block that has
/// one and let the authored bytes be compiled in its place — silently, and
/// exactly the fallback this boundary exists to prevent.
///
/// `consumed_basis` is taken from the slot rather than recomputed, because
/// only the projection states the authored bytes THIS selection was
/// validated against; re-reading the carrier would state whatever revision
/// is live by then.
fn svelte_supplied_style(
    input: verter_compiler::framework_common::RuntimeBlockContentInput,
) -> SvelteSuppliedStyle {
    SvelteSuppliedStyle {
        producer: input
            .producer
            .unwrap_or(verter_compiler::style_planner::PreprocessorIdentity::Anonymous),
        code: input.code,
        source_space_token: input.source_space_token,
        content_artifact_token: input.content_artifact_token,
        source_map: input.source_map,
        diagnostics: input.diagnostics,
        parsed: input.parsed,
        consumed_basis: input.authored_basis,
    }
}

/// Executing a caller-supplied canonical request refused before any
/// product published.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) enum SuppliedRequestFailure {
    /// The supplied request names a framework the bound backend is not.
    /// The backend that owns the registered carrier is the sole authority
    /// for this refusal, so nothing above it derives a framework identity
    /// to compare against.
    FrameworkMismatch {
        /// The framework the request names.
        requested: &'static str,
        /// The framework the registered carrier is compiled by.
        registered: String,
    },
    /// A demanded product kind the bound host integration produces no
    /// route for — the public-API and declaration kinds. Kept typed and
    /// distinct so the seam reports the refused KIND rather than a
    /// message a consumer would have to parse.
    UnsupportedProduct {
        /// The product kind with no host production route.
        kind: verter_compiler::compile_request::ProductKind,
        /// The refusal's diagnostics, naming the refused kind.
        diagnostics: DiagnosticsSnapshot,
    },
    /// Every other refusal, in the same typed shape both existing compile
    /// lanes already map their refusals onto.
    Products(HostProductsFailure),
}

/// The caller-supplied-request lane's ONE bound execution.
///
/// The supplied request IS the demand document: this function passes it
/// to the bound backend's canonical-request admission verbatim and never
/// re-derives products, framework options, or source identity from any
/// other vocabulary. Everything else is identical to the profile-derived
/// lane — the same prepared execution inputs, the same bound backend, the
/// same consume-once admission, the same single product execution —
/// because the demand document is the only thing that differs between the
/// two.
///
/// The inputs arrive prepared and complete
/// ([`super::compile_inputs::prepare_execution_inputs_pollable`]); this
/// function waits on nothing. Inputs prepared for another framework than
/// the binding's are refused with the execution-refused diagnostic.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn execute_supplied_host_request(
    binding: super::native_host_binding::BoundNativeHostRequest,
    artifact: &verter_compiler::framework_common::FrameworkParseArtifact,
    request: verter_compiler::compile_request::CompileRequest,
    snapshot: &CompileInput,
    inputs: PreparedExecutionInputs,
) -> Result<BoundCompiledProducts, SuppliedRequestFailure> {
    use super::native_host_binding::BoundNativeHostRequest;
    use crate::host_compile_audit::{debug_assert_compile_bound_attribution, BoundCompileRoute};
    use verter_compiler::framework_common::FrameworkHostIntegrationBackend as _;

    let source_len = snapshot.source.len() as u32;
    let inputs_for_another_framework = || {
        SuppliedRequestFailure::Products(HostProductsFailure::Fatal(
            inputs_for_another_framework_diagnostics(&snapshot.canonical_id, source_len),
        ))
    };
    match binding {
        BoundNativeHostRequest::Vue(bound) => {
            let PreparedExecutionInputs::Vue(inputs) = inputs else {
                return Err(inputs_for_another_framework());
            };
            let (backend, attribution) = bound.into_host_backend();
            debug_assert_compile_bound_attribution(
                BoundCompileRoute::HostBacked,
                &attribution,
                artifact,
                &snapshot.canonical_id,
            );
            let alloc = oxc_allocator::Allocator::new();
            let alloc = &alloc;
            let admission =
                backend
                    .admit_canonical_request(artifact, request)
                    .map_err(|refusal| {
                        match refusal {
                    verter_compiler::framework_common::VueHostAdmissionRefusal::RequestConstructionRefused(
                        verter_compiler::compile_request::CompileRequestError::FrameworkMismatch {
                            expected,
                            actual,
                        },
                    ) => SuppliedRequestFailure::FrameworkMismatch {
                        requested: actual,
                        registered: expected.to_string(),
                    },
                    verter_compiler::framework_common::VueHostAdmissionRefusal::UnsupportedProduct(
                        kind,
                    ) => SuppliedRequestFailure::UnsupportedProduct {
                        kind,
                        diagnostics: unsupported_product_diagnostics(
                            &snapshot.canonical_id,
                            source_len,
                            kind,
                        ),
                    },
                    other => SuppliedRequestFailure::Products(HostProductsFailure::Fatal(
                        vue_admission_refused_diagnostics(
                            &snapshot.canonical_id,
                            source_len,
                            other,
                        ),
                    )),
                }
                    })?;
            backend
                .compile_host_products(admission, artifact, &inputs, alloc)
                .map(BoundCompiledProducts::Vue)
                .map_err(|refusal| {
                    SuppliedRequestFailure::Products(vue_products_execution_failure(
                        artifact,
                        &snapshot.canonical_id,
                        source_len,
                        refusal,
                    ))
                })
        }
        BoundNativeHostRequest::Svelte(bound) => {
            let (backend, attribution) = bound.into_host_backend();
            debug_assert_compile_bound_attribution(
                BoundCompileRoute::HostBacked,
                &attribution,
                artifact,
                &snapshot.canonical_id,
            );
            let PreparedExecutionInputs::Svelte(inputs) = inputs else {
                return Err(inputs_for_another_framework());
            };
            let alloc = oxc_allocator::Allocator::new();
            let alloc = &alloc;
            let admission = backend
                .admit_canonical_request(artifact, request)
                .map_err(|refusal| match refusal {
                    verter_compiler::framework_common::SvelteHostAdmissionRefusal::RequestConstructionRefused(
                        verter_compiler::compile_request::CompileRequestError::FrameworkMismatch {
                            expected,
                            actual,
                        },
                    ) => SuppliedRequestFailure::FrameworkMismatch {
                        requested: actual,
                        registered: expected.to_string(),
                    },
                    verter_compiler::framework_common::SvelteHostAdmissionRefusal::UnsupportedProduct(
                        kind,
                    ) => SuppliedRequestFailure::UnsupportedProduct {
                        kind,
                        diagnostics: unsupported_product_diagnostics(
                            &snapshot.canonical_id,
                            source_len,
                            kind,
                        ),
                    },
                    other => SuppliedRequestFailure::Products(HostProductsFailure::Fatal(
                        svelte_admission_refused_diagnostics(
                            &snapshot.canonical_id,
                            source_len,
                            other,
                        ),
                    )),
                })?;
            backend
                .compile_host_products(admission, artifact, &inputs, alloc)
                .map(BoundCompiledProducts::Svelte)
                .map_err(|refusal| {
                    SuppliedRequestFailure::Products(svelte_products_execution_failure(
                        artifact,
                        &snapshot.canonical_id,
                        source_len,
                        refusal,
                    ))
                })
        }
    }
}

/// `HOST_COMPILE_REQUEST_EXECUTION_REFUSED` for execution inputs prepared
/// for another framework than the bound carrier's. The callers prepare
/// from the binding's own arm, so this is a refusal rather than a panic for
/// a pairing they cannot build.
fn inputs_for_another_framework_diagnostics(
    canonical_id: &str,
    source_len: u32,
) -> DiagnosticsSnapshot {
    DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
        severity: HostSeverity::Error,
        code: "HOST_COMPILE_REQUEST_EXECUTION_REFUSED".to_string(),
        message: format!(
            "compile execution refused for '{canonical_id}': the execution inputs were \
             prepared for another framework"
        ),
        arguments: Vec::new(),
        span: verter_span::Span::new(0, source_len),
    }])
}

/// `HOST_COMPILE_PRODUCT_UNSUPPORTED`: the bound host integration has no
/// production route for a demanded product kind. The kind is named so the
/// refusal is readable without parsing the message.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn unsupported_product_diagnostics(
    canonical_id: &str,
    source_len: u32,
    kind: verter_compiler::compile_request::ProductKind,
) -> DiagnosticsSnapshot {
    DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
        severity: HostSeverity::Error,
        code: "HOST_COMPILE_PRODUCT_UNSUPPORTED".to_string(),
        message: format!("the host integration produces no {kind:?} product for '{canonical_id}'"),
        arguments: Vec::new(),
        span: verter_span::Span::new(0, source_len),
    }])
}

/// The render lane's Vue-BOUND demand: the render's whole subject is the
/// runtime `Main` module, so exactly one runtime product is demanded
/// (client, or server under `ssr`) plus the optional template-fact
/// DIAGNOSTICS companion, and every Vue option axis the profile carries
/// rides the same typed attempt the host-backed constructor validates.
/// The bound backend owns request composition and admission; this
/// function only translates the profile into the backend's demand
/// document.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn vue_runtime_render_demand(
    profile: &CompileProfile,
    canonical_id: &str,
    template_fact_diagnostics: bool,
    style_processing: verter_compiler::compile_request::RuntimeStyleProcessing,
) -> VueHostRuntimeRenderDemand {
    use verter_compiler::compile_request::RuntimeProductRequest;
    VueHostRuntimeRenderDemand {
        runtime: RuntimeProductRequest {
            inline: profile.inline,
            runtime_source_map: profile.source_map,
            // The render caller owns which style stages this product runs: a
            // bundler with its own style-module lane demands `AuthoredOnly`,
            // every other caller the complete authored-to-published cascade.
            style_processing,
            ..Default::default()
        },
        template_fact_diagnostics,
        vue_options: vue_option_attempt_from_profile(profile),
        filename: profile
            .filename
            .clone()
            .or_else(|| Some(canonical_id.to_string())),
        component_id: profile.component_id.clone(),
        is_production: profile.is_production,
        force_js: profile.force_js,
        hmr_strategy: crate::compile::hmr_strategy(profile.hmr_strategy),
        ssr_module_id: profile.ssr_module_id.clone(),
        emit_ssr_module_registration: profile.emit_ssr_module_registration,
    }
}

/// The render lane's Svelte-BOUND demand: one runtime product (client, or
/// server under `ssr`) with the exact requested style policy over the
/// option axes the profile can express, decoded through the SAME typed
/// admission the host-backed Svelte constructor uses — a malformed token
/// refuses HERE, never a silent default, and an axis the bound execution
/// cannot honor refuses typed at the backend's issuance.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn svelte_runtime_render_demand(
    profile: &CompileProfile,
    canonical_id: &str,
    style_processing: verter_compiler::compile_request::RuntimeStyleProcessing,
) -> Result<SvelteHostRuntimeRenderDemand, verter_compiler::compile_request::CompileRequestError> {
    use verter_compiler::compile_request::RuntimeProductRequest;
    Ok(SvelteHostRuntimeRenderDemand {
        runtime: RuntimeProductRequest {
            inline: profile.inline,
            runtime_source_map: profile.source_map,
            // Same caller-owned style policy as the Vue arm above.
            style_processing,
            ..Default::default()
        },
        ssr: profile.ssr,
        svelte_options: svelte_option_attempt_from_profile(profile)?,
        filename: profile
            .filename
            .clone()
            .or_else(|| Some(canonical_id.to_string())),
        is_production: profile.is_production,
        force_js: profile.force_js,
    })
}

/// The host-backed multi-product Svelte-BOUND demand: every requested
/// product plus every Svelte option axis the profile can express, decoded
/// through the SAME typed decode boundary the render lane uses — a
/// malformed token refuses HERE, never a silent default, and an axis the
/// bound execution cannot honor refuses typed at the backend's issuance.
/// The caller selects this constructor from its request-scoped native
/// host binding's Svelte arm — never from language classification.
pub(crate) fn svelte_host_products_demand(
    profile: &CompileProfile,
    canonical_id: &str,
    want_runtime: bool,
    want_ide: bool,
    want_template_data: bool,
) -> Result<SvelteHostMultiProductDemand, verter_compiler::compile_request::CompileRequestError> {
    Ok(SvelteHostMultiProductDemand {
        products: demanded_products(profile, want_runtime, want_ide, want_template_data),
        svelte_options: svelte_option_attempt_from_profile(profile)?,
        filename: profile
            .filename
            .clone()
            .or_else(|| Some(canonical_id.to_string())),
        is_production: profile.is_production,
        force_js: profile.force_js,
    })
}

/// The typed Svelte option attempt every Svelte option the session
/// `CompileProfile` carries maps onto — shared by the host-backed request
/// constructor and the runtime-render bound demand so the two routes can
/// never diverge on which profile axes reach Svelte option admission, or
/// on the decode-boundary refusals below.
fn svelte_option_attempt_from_profile(
    profile: &CompileProfile,
) -> Result<
    verter_compiler::compile_request::SvelteOptionAttempt,
    verter_compiler::compile_request::CompileRequestError,
> {
    use verter_compiler::compile_request::svelte::{
        SvelteCompatibilityRequest, SvelteCssRequest, SvelteCustomElementDescriptor,
        SvelteFragmentsRequest, SvelteNamespaceRequest, SvelteRunesRequest,
    };
    use verter_compiler::compile_request::{
        CompileRequestError, FrameworkOption, SvelteOption, SvelteOptionAttempt,
    };

    {
        // `svelte_namespace`/`svelte_fragments` are the decode-boundary
        // concern the Svelte carrier's own parsers document themselves as
        // NOT owning (`parse_svelte_namespace`/`parse_svelte_fragments` in
        // `crates/verter_compiler/src/svelte/carrier.rs`): an unrecognized
        // token refuses HERE, at construction, instead of silently
        // resolving to the carrier's own default.
        let namespace = profile
            .svelte_namespace
            .as_deref()
            .map(|token| match token {
                "html" => Ok(SvelteNamespaceRequest::Html),
                "svg" => Ok(SvelteNamespaceRequest::Svg),
                "mathml" => Ok(SvelteNamespaceRequest::MathMl),
                other => Err(CompileRequestError::malformed_option_value(
                    FrameworkOption::Svelte(SvelteOption::CompileOptionsNamespace),
                    other,
                )),
            })
            .transpose()?;
        let fragments = profile
            .svelte_fragments
            .as_deref()
            .map(|token| match token {
                "html" => Ok(SvelteFragmentsRequest::Html),
                "tree" => Ok(SvelteFragmentsRequest::Tree),
                other => Err(CompileRequestError::malformed_option_value(
                    FrameworkOption::Svelte(SvelteOption::CompileOptionsFragments),
                    other,
                )),
            })
            .transpose()?;
        // Same decode-boundary rationale as `namespace`/`fragments` above.
        let css = profile
            .svelte_css
            .as_deref()
            .map(|token| match token {
                "injected" => Ok(SvelteCssRequest::Injected),
                "external" => Ok(SvelteCssRequest::External),
                other => Err(CompileRequestError::malformed_option_value(
                    FrameworkOption::Svelte(SvelteOption::CompileOptionsCss),
                    other,
                )),
            })
            .transpose()?;
        // A descriptor is constructed only when the caller actually set
        // one of its fields — an all-`None` descriptor is not the same
        // request as no descriptor at all (the latter defers entirely to
        // the plain `custom_element: bool` axis; see
        // `resolve_custom_element`'s `compile_option_descriptor`
        // fallback). The per-prop map (`SvelteOptions.customElement.
        // props.*`) has no `CompileProfile` channel yet — always empty
        // here.
        let custom_element_descriptor = (profile.svelte_custom_element_tag.is_some()
            || profile.svelte_custom_element_shadow.is_some())
        .then(|| SvelteCustomElementDescriptor {
            tag: profile.svelte_custom_element_tag.clone(),
            shadow: profile.svelte_custom_element_shadow,
            props: Default::default(),
        });
        Ok(SvelteOptionAttempt {
            dev: profile.svelte_dev,
            generate_module: profile.svelte_generate_module,
            experimental_async: profile.svelte_experimental_async,
            custom_element: Some(profile.custom_element),
            custom_element_descriptor,
            namespace,
            css,
            preserve_comments: profile.svelte_preserve_comments,
            preserve_whitespace: profile.svelte_preserve_whitespace,
            fragments,
            runes: profile.svelte_runes.map(|runes| {
                if runes {
                    SvelteRunesRequest::True
                } else {
                    SvelteRunesRequest::False
                }
            }),
            disclose_version: profile.svelte_disclose_version,
            compatibility: profile
                .svelte_compatibility
                .unwrap_or(false)
                .then(SvelteCompatibilityRequest::default),
            ..Default::default()
        })
    }
}

/// Maps a [`verter_compiler::compile_request::CompileRequestError`] (a
/// construction refusal) onto the same
/// `HOST_COMPILE_REQUEST_EXECUTION_REFUSED` diagnostic code the
/// `CompileUnsupported::RequestExecutionRefused` arm already reports — the
/// request-construction refusal and the post-parse resolution refusal both
/// name the same host-facing code, only the message differs.
///
/// The message's words are the compiler's own — `CompileRequestError`'s
/// `Display`, the one refusal vocabulary every transport renders. `{:?}`
/// would publish the Rust variant spelling into a user-visible diagnostic
/// (`MalformedOptionValue { option: Svelte(CompileOptionsCss), value:
/// "not-a-real-mode" }`) and would name the option by a spelling no
/// caller's request object has.
pub(crate) fn request_construction_refused_diagnostics(
    canonical_id: &str,
    source_len: u32,
    error: &verter_compiler::compile_request::CompileRequestError,
) -> DiagnosticsSnapshot {
    DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
        severity: HostSeverity::Error,
        code: "HOST_COMPILE_REQUEST_EXECUTION_REFUSED".to_string(),
        message: format!("compile request construction refused for '{canonical_id}': {error}"),
        arguments: Vec::new(),
        span: verter_span::Span::new(0, source_len),
    }])
}

/// Maps a bound framework host backend's Vue admission refusal onto the
/// host diagnostics surface — shared by BOTH bound compile lanes
/// (host-backed multi-product and runtime-render) so the mapping cannot
/// drift: a canonical-request construction refusal keeps the SAME
/// `HOST_COMPILE_REQUEST_EXECUTION_REFUSED` code the session's own
/// demand-decode refusal reports for the identical demand, and every
/// other typed issuance refusal (unavailable capability, unsupported or
/// unproducible demand, non-composable parse) surfaces as
/// `HOST_COMPILE_ADMISSION_REFUSED` — never a fallback lane, framework,
/// or compatibility compiler.
pub(crate) fn vue_admission_refused_diagnostics(
    canonical_id: &str,
    source_len: u32,
    refusal: verter_compiler::framework_common::VueHostAdmissionRefusal,
) -> DiagnosticsSnapshot {
    match refusal {
        verter_compiler::framework_common::VueHostAdmissionRefusal::RequestConstructionRefused(
            error,
        ) => request_construction_refused_diagnostics(canonical_id, source_len, &error),
        other => admission_refused_diagnostics(canonical_id, source_len, &format!("{other:?}")),
    }
}

/// Svelte sibling of [`vue_admission_refused_diagnostics`].
pub(crate) fn svelte_admission_refused_diagnostics(
    canonical_id: &str,
    source_len: u32,
    refusal: verter_compiler::framework_common::SvelteHostAdmissionRefusal,
) -> DiagnosticsSnapshot {
    match refusal {
        verter_compiler::framework_common::SvelteHostAdmissionRefusal::RequestConstructionRefused(
            error,
        ) => request_construction_refused_diagnostics(canonical_id, source_len, &error),
        other => admission_refused_diagnostics(canonical_id, source_len, &format!("{other:?}")),
    }
}

/// `HOST_COMPILE_ADMISSION_REFUSED`: the bound framework host backend
/// refused to issue the demand-specific compile admission (outside the
/// request-construction class, which keeps its own code above).
pub(crate) fn admission_refused_diagnostics(
    canonical_id: &str,
    source_len: u32,
    detail: &str,
) -> DiagnosticsSnapshot {
    DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
        severity: HostSeverity::Error,
        code: "HOST_COMPILE_ADMISSION_REFUSED".to_string(),
        message: format!("compile admission refused for '{canonical_id}': {detail}"),
        arguments: Vec::new(),
        span: verter_span::Span::new(0, source_len),
    }])
}

/// `HOST_STACK_UNAVAILABLE`: a parse or walk-stack lease the execution
/// needed was refused its stack, so its bundle was built from an empty
/// program in place of the source's and nothing it produced publishes.
pub(crate) fn stack_unavailable_diagnostics(
    canonical_id: &str,
    source_len: u32,
    unavailable: verter_parser::oxc_parse::StackUnavailable,
) -> DiagnosticsSnapshot {
    DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
        severity: HostSeverity::Error,
        code: crate::types::HOST_STACK_UNAVAILABLE.to_string(),
        message: format!("compile of '{canonical_id}' is incomplete: {unavailable}"),
        arguments: Vec::new(),
        span: verter_span::Span::new(0, source_len),
    }])
}

/// A compile that needs inputs the host has not supplied, mapped onto the
/// refused failure a style block the compiler cannot process already
/// reports (`HOST_BLOCK_CONTENT_RUNTIME_UNAVAILABLE`), one diagnostic per
/// needed input, each naming the block and the stage it must arrive at.
/// No dependency-gap outcome describes a style stage, so none is used.
pub(crate) fn needed_inputs_diagnostics(
    canonical_id: &str,
    source_len: u32,
    needed: &[verter_compiler::compile_request::demand::InputDemand],
) -> DiagnosticsSnapshot {
    use verter_compiler::compile_request::demand::InputDemand;
    use verter_compiler::style_planner::StyleStage;
    DiagnosticsSnapshot::from_vec(
        needed
            .iter()
            .map(|demand| {
                let InputDemand::ExternalStyleStage { style_index, stage } = *demand;
                let stage = match stage {
                    StyleStage::Authored => "authored",
                    StyleStage::Preprocessed => "preprocessed",
                    StyleStage::FrameworkRewritten => "framework-rewritten",
                };
                HostDiagnostic {
                    severity: HostSeverity::Error,
                    code: "HOST_BLOCK_CONTENT_RUNTIME_UNAVAILABLE".to_string(),
                    message: format!(
                        "style block {style_index} of '{canonical_id}' must be supplied at the {stage} stage before it can compile"
                    ),
                    arguments: Vec::new(),
                    span: verter_span::Span::new(0, source_len),
                }
            })
            .collect(),
    )
}

/// `HOST_NO_CARRIER_ARTIFACT`: the input has no framework parse artifact,
/// so no registered identity exists — no binding, no registry dispatch,
/// and no runtime compile route.
pub(crate) fn no_carrier_artifact_diagnostics(
    canonical_id: &str,
    source_len: u32,
) -> DiagnosticsSnapshot {
    DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
        severity: HostSeverity::Error,
        code: "HOST_NO_CARRIER_ARTIFACT".to_string(),
        message: format!(
            "no framework parse artifact for '{canonical_id}' — cannot route the runtime compile"
        ),
        arguments: Vec::new(),
        span: verter_span::Span::new(0, source_len),
    }])
}

/// The bound compile lanes' fatal diagnostic for a bound backend
/// execution the shared orchestration refused
/// (`verter_compiler::framework_common::CompileUnsupported`): one stable
/// per-arm code and carrier message shared by the host-backed and
/// runtime-render lanes, so the two cannot drift on this surface.
pub(crate) fn runtime_bundle_unsupported_diagnostics(
    artifact: &verter_compiler::framework_common::FrameworkParseArtifact,
    canonical_id: &str,
    source_len: u32,
    unsupported: &verter_compiler::framework_common::CompileUnsupported,
) -> DiagnosticsSnapshot {
    let message = match unsupported {
        verter_compiler::framework_common::CompileUnsupported::VueMainAssemblyFailed(reason) => {
            reason.clone()
        }
        _ => format!(
            "carrier '{}' cannot produce a runtime bundle for '{}'",
            artifact.adapter_id().as_str(),
            canonical_id
        ),
    };
    DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
        severity: HostSeverity::Error,
        code: compile_unsupported_code(unsupported).to_string(),
        message,
        arguments: Vec::new(),
        span: verter_span::Span::new(0, source_len),
    }])
}

/// A bound runtime-render EXECUTION refusal, mapped for the render lane:
/// either the typed runtime-surface refusal (the render's subject is
/// absent — surfaced as `HostError::RuntimeSurfaceRefused`) or a fatal
/// diagnostics payload (shared-orchestration refusal keeping the
/// host-backed lane's exact code/message; an issuance/execution pairing
/// breach — structurally unreachable on the lane — mapped typed rather
/// than unwrapped).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) enum RenderExecutionRefusal {
    /// The requested runtime surface was refused; carries the carrier's
    /// structural code and message.
    Surface {
        diagnostic_code: String,
        message: String,
    },
    /// A fatal refusal: the diagnostics payload for the compile failure.
    Fatal(DiagnosticsSnapshot),
}

/// Maps the Vue bound backend's execution refusal for the render lane.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn vue_render_execution_refusal(
    artifact: &verter_compiler::framework_common::FrameworkParseArtifact,
    canonical_id: &str,
    source_len: u32,
    refusal: verter_compiler::framework_common::VueHostCompileRefusal,
) -> RenderExecutionRefusal {
    use verter_compiler::framework_common::VueHostCompileRefusal;
    match refusal {
        VueHostCompileRefusal::RuntimeSurfaceRefused {
            diagnostic_code,
            message,
            ..
        } => RenderExecutionRefusal::Surface {
            diagnostic_code,
            message,
        },
        VueHostCompileRefusal::Unsupported(unsupported) => {
            RenderExecutionRefusal::Fatal(runtime_bundle_unsupported_diagnostics(
                artifact,
                canonical_id,
                source_len,
                &unsupported,
            ))
        }
        VueHostCompileRefusal::StackUnavailable(unavailable) => RenderExecutionRefusal::Fatal(
            stack_unavailable_diagnostics(canonical_id, source_len, unavailable),
        ),
        refusal @ (VueHostCompileRefusal::AdmissionParseMismatch
        | VueHostCompileRefusal::WrongDemand { .. }) => RenderExecutionRefusal::Fatal(
            admission_refused_diagnostics(canonical_id, source_len, &format!("{refusal:?}")),
        ),
        VueHostCompileRefusal::NeedInputs(needed) => RenderExecutionRefusal::Fatal(
            needed_inputs_diagnostics(canonical_id, source_len, &needed),
        ),
    }
}

/// Maps a bound render EXECUTION refusal onto the lane's `HostError`
/// surface: a runtime-surface refusal is the typed
/// `HostError::RuntimeSurfaceRefused`; everything else is a fatal compile
/// failure carrying the request's diagnostics plus the refusal's payload.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn render_execution_error(
    refusal: RenderExecutionRefusal,
    canonical_id: &str,
    diagnostics: DiagnosticsSnapshot,
    requested_mode: CompileCacheMode,
) -> crate::HostError {
    match refusal {
        RenderExecutionRefusal::Surface {
            diagnostic_code,
            message,
        } => crate::HostError::RuntimeSurfaceRefused {
            canonical_id: canonical_id.to_string(),
            diagnostic_code,
            message,
        },
        RenderExecutionRefusal::Fatal(payload) => crate::HostError::CompileError(CompileFailure {
            diagnostics: diagnostics.merge(payload),
            requested_mode,
            actual_mode: requested_mode,
            downgrade_reason: None,
        }),
    }
}

/// The bound render handoff of either catalog arm, held so the render
/// lane's shared `Main`-assembly tail borrows ONE `RuntimeCompileOutput`
/// regardless of which framework backend produced it. Not a dispatch
/// surface: both arms come from the same bound execution, and the sum
/// mirrors the sealed binding sum.
#[cfg(not(target_arch = "wasm32"))]
#[allow(
    clippy::large_enum_variant,
    reason = "Both variants are large render handoffs; inline storage avoids a heap allocation for every compile."
)]
pub(crate) enum BoundRenderedMain {
    /// The Vue backend's render-only handoff.
    Vue(verter_compiler::framework_common::VueHostRenderedMain),
    /// The Svelte backend's render-only handoff.
    Svelte(verter_compiler::framework_common::SvelteHostRenderedMain),
}

#[cfg(not(target_arch = "wasm32"))]
impl BoundRenderedMain {
    /// The runtime bundle for host `Main` assembly.
    pub(crate) fn runtime_bundle(
        &self,
    ) -> &verter_compiler::framework_common::RuntimeCompileOutput {
        match self {
            Self::Vue(rendered) => rendered.runtime_bundle(),
            Self::Svelte(rendered) => rendered.runtime_bundle(),
        }
    }
}

/// Svelte sibling of [`vue_render_execution_refusal`].
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn svelte_render_execution_refusal(
    artifact: &verter_compiler::framework_common::FrameworkParseArtifact,
    canonical_id: &str,
    source_len: u32,
    refusal: verter_compiler::framework_common::SvelteHostCompileRefusal,
) -> RenderExecutionRefusal {
    use verter_compiler::framework_common::SvelteHostCompileRefusal;
    match refusal {
        SvelteHostCompileRefusal::RuntimeSurfaceRefused {
            diagnostic_code,
            message,
            ..
        } => RenderExecutionRefusal::Surface {
            diagnostic_code,
            message,
        },
        SvelteHostCompileRefusal::Unsupported(unsupported) => {
            RenderExecutionRefusal::Fatal(runtime_bundle_unsupported_diagnostics(
                artifact,
                canonical_id,
                source_len,
                &unsupported,
            ))
        }
        SvelteHostCompileRefusal::StackUnavailable(unavailable) => RenderExecutionRefusal::Fatal(
            stack_unavailable_diagnostics(canonical_id, source_len, unavailable),
        ),
        refusal @ (SvelteHostCompileRefusal::AdmissionParseMismatch
        | SvelteHostCompileRefusal::WrongDemand { .. }) => RenderExecutionRefusal::Fatal(
            admission_refused_diagnostics(canonical_id, source_len, &format!("{refusal:?}")),
        ),
    }
}

/// The bound host-backed handoff of either catalog arm, held so the
/// host-backed lane's publication tail reads ONE per-product-gated
/// surface regardless of which framework backend produced it. Not a
/// dispatch surface: both arms come from the same bound execution, and
/// the sum mirrors the sealed binding sum. Every accessor delegates to
/// the backend's own admitted-set gate, so a prerequisite that was
/// produced but not admitted is never published.
#[allow(
    clippy::large_enum_variant,
    reason = "Both variants are large publication handoffs; inline storage avoids a heap allocation for every compile."
)]
pub(crate) enum BoundCompiledProducts {
    /// The Vue backend's multi-product publication payloads.
    Vue(verter_compiler::framework_common::VueHostCompiledProducts),
    /// The Svelte backend's multi-product publication payloads.
    Svelte(verter_compiler::framework_common::SvelteHostCompiledProducts),
}

impl BoundCompiledProducts {
    /// The admitted runtime bundle (client or server — a dual-kind demand
    /// is refused at issuance, so at most one is ever admitted), when a
    /// runtime product was admitted.
    pub(crate) fn runtime_bundle(
        &self,
    ) -> Option<&verter_compiler::framework_common::RuntimeCompileOutput> {
        match self {
            Self::Vue(products) => products
                .runtime_client_bundle()
                .or_else(|| products.runtime_server_bundle()),
            Self::Svelte(products) => products
                .runtime_client_bundle()
                .or_else(|| products.runtime_server_bundle()),
        }
    }

    /// The IDE companion publication payload, when admitted.
    pub(crate) fn ide_companion(&self) -> Option<&verter_compiler::framework_common::IdeOutput> {
        match self {
            Self::Vue(products) => products.ide_companion(),
            Self::Svelte(products) => products.ide_companion(),
        }
    }

    /// The admitted template facts, when the analysis product was admitted.
    pub(crate) fn template_facts(
        &self,
    ) -> Option<
        &verter_compiler::framework_common::registered_carrier_projection::TemplateFactsProduct,
    > {
        match self {
            Self::Vue(products) => products.template_facts(),
            Self::Svelte(products) => products.template_facts(),
        }
    }

    /// Aggregated non-fatal diagnostics of the whole admitted compile.
    pub(crate) fn diagnostics(&self) -> &[verter_compiler::framework_common::RuntimeDiagnostic] {
        match self {
            Self::Vue(products) => products.diagnostics(),
            Self::Svelte(products) => products.diagnostics(),
        }
    }

    /// The complete Vue artifact set, when the compile published one.
    pub(crate) fn artifact_set(
        &self,
    ) -> Option<&verter_compiler::framework::vue::assembly::VueArtifactSet> {
        match self {
            Self::Vue(products) => products.artifact_set(),
            Self::Svelte(_) => None,
        }
    }

    /// The wire form of [`Self::artifact_set`] the compile response
    /// carries. A set that cannot serialize refuses the compile rather than
    /// publishing the response without it.
    pub(crate) fn artifact_set_json(
        &self,
        canonical_id: &str,
        source_len: u32,
    ) -> Result<Option<String>, DiagnosticsSnapshot> {
        self.artifact_set()
            .map(|set| set.set().to_json())
            .transpose()
            .map_err(|error| {
                DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
                    severity: HostSeverity::Error,
                    code: "HOST_MAIN_MODULE_ASSEMBLY_FAILED".to_string(),
                    message: format!(
                        "the artifact set of '{canonical_id}' cannot be serialized: {error}"
                    ),
                    arguments: Vec::new(),
                    span: verter_span::Span::new(0, source_len),
                }])
            })
    }
}

/// A bound host-backed compile failure, mapped for the host-backed lane:
/// either the typed all-or-none runtime-surface refusal (the requested
/// runtime surface is absent; no sibling product publishes after it) or
/// a fatal diagnostics payload (admission refusal, demand-decode
/// refusal, shared-orchestration refusal keeping the same stable per-arm
/// code, or an issuance/execution pairing breach — structurally
/// unreachable on the lane — mapped typed rather than unwrapped).
pub(crate) enum HostProductsFailure {
    /// The requested runtime surface was refused; carries the carrier's
    /// structural code, message, refusing span, and the non-fatal
    /// diagnostics collected before the refusal.
    Surface {
        /// Structural refusal code.
        diagnostic_code: String,
        /// Human-readable refusal reason.
        message: String,
        /// Carrier-absolute span of the refusing construct (whole-source
        /// when the refusing arm carries no narrower span).
        span: verter_span::Span,
        /// Non-fatal diagnostics collected before the refusal.
        diagnostics: Vec<verter_compiler::framework_common::RuntimeDiagnostic>,
    },
    /// A fatal refusal: the diagnostics payload for the compile failure.
    Fatal(DiagnosticsSnapshot),
}

/// Maps the Vue bound backend's multi-product execution refusal for the
/// host-backed lane. The Vue refusal arm carries no span of its own, so
/// the surface refusal anchors at the whole source (the accepted span
/// asymmetry between the per-framework refusal payloads).
fn vue_products_execution_failure(
    artifact: &verter_compiler::framework_common::FrameworkParseArtifact,
    canonical_id: &str,
    source_len: u32,
    refusal: verter_compiler::framework_common::VueHostCompileRefusal,
) -> HostProductsFailure {
    use verter_compiler::framework_common::VueHostCompileRefusal;
    match refusal {
        VueHostCompileRefusal::RuntimeSurfaceRefused {
            diagnostic_code,
            message,
            diagnostics,
        } => HostProductsFailure::Surface {
            diagnostic_code,
            message,
            span: verter_span::Span::new(0, source_len),
            diagnostics,
        },
        VueHostCompileRefusal::Unsupported(unsupported) => {
            HostProductsFailure::Fatal(runtime_bundle_unsupported_diagnostics(
                artifact,
                canonical_id,
                source_len,
                &unsupported,
            ))
        }
        VueHostCompileRefusal::StackUnavailable(unavailable) => HostProductsFailure::Fatal(
            stack_unavailable_diagnostics(canonical_id, source_len, unavailable),
        ),
        refusal @ (VueHostCompileRefusal::AdmissionParseMismatch
        | VueHostCompileRefusal::WrongDemand { .. }) => HostProductsFailure::Fatal(
            admission_refused_diagnostics(canonical_id, source_len, &format!("{refusal:?}")),
        ),
        VueHostCompileRefusal::NeedInputs(needed) => {
            HostProductsFailure::Fatal(needed_inputs_diagnostics(canonical_id, source_len, &needed))
        }
    }
}

/// Svelte sibling of [`vue_products_execution_failure`]; the Svelte
/// refusal arm carries the refusing construct's own span.
fn svelte_products_execution_failure(
    artifact: &verter_compiler::framework_common::FrameworkParseArtifact,
    canonical_id: &str,
    source_len: u32,
    refusal: verter_compiler::framework_common::SvelteHostCompileRefusal,
) -> HostProductsFailure {
    use verter_compiler::framework_common::SvelteHostCompileRefusal;
    match refusal {
        SvelteHostCompileRefusal::RuntimeSurfaceRefused {
            diagnostic_code,
            message,
            span,
            diagnostics,
        } => HostProductsFailure::Surface {
            diagnostic_code,
            message,
            span,
            diagnostics,
        },
        SvelteHostCompileRefusal::Unsupported(unsupported) => {
            HostProductsFailure::Fatal(runtime_bundle_unsupported_diagnostics(
                artifact,
                canonical_id,
                source_len,
                &unsupported,
            ))
        }
        SvelteHostCompileRefusal::StackUnavailable(unavailable) => HostProductsFailure::Fatal(
            stack_unavailable_diagnostics(canonical_id, source_len, unavailable),
        ),
        refusal @ (SvelteHostCompileRefusal::AdmissionParseMismatch
        | SvelteHostCompileRefusal::WrongDemand { .. }) => HostProductsFailure::Fatal(
            admission_refused_diagnostics(canonical_id, source_len, &format!("{refusal:?}")),
        ),
    }
}

/// The stable host diagnostic code for each `verter_compiler::framework_common::CompileUnsupported` arm —
/// shared by both compile routes so the mapping cannot drift.
pub(crate) fn compile_unsupported_code(
    unsupported: &verter_compiler::framework_common::CompileUnsupported,
) -> &'static str {
    match unsupported {
        verter_compiler::framework_common::CompileUnsupported::TargetMissingIde => {
            "HOST_COMPILE_TARGET_MISSING_IDE"
        }
        verter_compiler::framework_common::CompileUnsupported::NoIdeProjection { .. } => {
            "HOST_COMPILE_UNSUPPORTED"
        }
        verter_compiler::framework_common::CompileUnsupported::BlockContentRuntimeUnavailable {
            ..
        } => "HOST_BLOCK_CONTENT_RUNTIME_UNAVAILABLE",
        verter_compiler::framework_common::CompileUnsupported::BlockContentIdeUnavailable {
            ..
        } => "HOST_BLOCK_CONTENT_IDE_UNAVAILABLE",
        verter_compiler::framework_common::CompileUnsupported::RequestExecutionRefused(_) => {
            "HOST_COMPILE_REQUEST_EXECUTION_REFUSED"
        }
        verter_compiler::framework_common::CompileUnsupported::ProductExecutionUngranted {
            ..
        } => "HOST_COMPILE_PRODUCT_EXECUTION_UNGRANTED",
        verter_compiler::framework_common::CompileUnsupported::VueMainAssemblyFailed(_) => {
            "HOST_MAIN_MODULE_ASSEMBLY_FAILED"
        }
    }
}

#[cfg(test)]
#[path = "compile_request_build_tests.rs"]
mod tests;

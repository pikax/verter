//! The session side of the bound host compile routes: demand
//! construction, bound-backend execution dispatch, the host-backed
//! lane's arm-local framework execution-input preparation, refusal
//! mapping, and the result-carrier plumbing shared by
//! [`crate::host_resolve::virtual_file_pipeline`]'s compile lanes.
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
//! the request. It shares every other stage — the same arm-local
//! execution-input preparation, the same bound backend, the same
//! consume-once admission, the same single product execution.
//!
//! There is no shared framework fork here: the demand constructor is
//! selected by the request-scoped native host binding's catalog arm (the
//! sole framework-identity derivation site for host compile requests),
//! and the two profile-derived lanes share the same per-framework
//! option-attempt builders so they can never diverge on which profile
//! axes reach option admission.

use crate::types::*;
use verter_compiler::framework_common::{
    SvelteHostExecutionInputs, SvelteHostMultiProductDemand, SvelteHostRuntimeRenderDemand,
    VueHostExecutionInputs, VueHostMultiProductDemand, VueHostRuntimeRenderDemand,
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
/// binding's catalog arm yields the backend, that arm prepares its own
/// framework execution inputs, the backend issues the demand-specific
/// multi-product admission over the SAME presented artifact, and the
/// product execution consumes that admission by value — one request, one
/// backend call, one admitted parse/semantic/projection/plan/emit
/// population. The lane holds no framework selector, no session-side
/// request construction, and no registry dispatch; issuance and execution
/// are paired by the admission's parse key, so admitting one artifact and
/// executing another is unrepresentable at this seam.
///
/// Framework EXECUTION INPUTS are prepared inside the arm that consumes
/// them, never by the generic route above this dispatch: an arm resolves
/// the cross-file semantic inputs its own backend reads, restates the
/// compiled file's dependency/semantic axis with the transitive
/// dependencies that resolution observed, and refuses on its own
/// dependency diagnostics. A carrier therefore cannot reach another
/// framework's producer, refusal, or execution-input carrier — the
/// generic route has no framework execution-input value to hand it.
///
/// The axis restatement is arm-local but not framework-conditional: it is
/// a property of the compiled file, it REPLACES rather than merges, and
/// it runs BEFORE the arm's own dependency refusal so a compile refused
/// for an unresolvable macro type still records the dependencies whose
/// repair must invalidate it.
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
    host: &crate::VerterHost,
    binding: super::native_host_binding::BoundNativeHostRequest,
    artifact: &verter_compiler::framework_common::FrameworkParseArtifact,
    profile: &CompileProfile,
    snapshot: &CompileInput,
    want_runtime: bool,
    want_ide: bool,
    want_template_data: bool,
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
            // The Vue macro bundle demand FOLLOWS the caller's target
            // instead of always asking for the heaviest one. A TSX-only
            // (IDE) compile takes the public binding names; only a target
            // that renders the runtime `props` option object pays for
            // per-member broad-runtime classification, which resolves every
            // member's type through the shared semantic engine. This arm
            // always produces at least the names bundle, because the shared
            // payload resolution underneath it is what yields this file's
            // macro dependency diagnostics and its transitive macro type
            // dependencies.
            let macro_demand =
                crate::typeinfo::vue_macro_codegen::VueMacroCodegenDemand::for_compile_target(
                    profile.target,
                )
                .unwrap_or(
                    crate::typeinfo::vue_macro_codegen::VueMacroCodegenDemand::RuntimeBindingNames,
                );
            let inputs = prepare_vue_execution_inputs(
                host,
                snapshot,
                macro_demand,
                SharedDependencyAxis::Restate,
            )?;
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
            let inputs = prepare_svelte_execution_inputs(
                host,
                snapshot,
                profile.svelte_css_hash_override.clone(),
                SharedDependencyAxis::Restate,
            );
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

/// Whether a compile route restates the compiled file's shared
/// dependency/semantic axis.
///
/// The axis is REPLACED, not merged, and it is the invalidation input for
/// the cache slots the profile-derived lanes publish. A route that
/// publishes a slot therefore MUST restate it. A route that publishes
/// nothing has no slot to invalidate — and restating from its own,
/// possibly narrower, macro demand would overwrite a cached lane's
/// recorded edges with a set that no longer covers them, so a
/// cache-writing lane could then miss an invalidation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SharedDependencyAxis {
    /// Replace the file's recorded transitive dependency/semantic edges
    /// with this compile's own — the profile-derived lanes, which publish
    /// cache slots that invalidate against the axis.
    Restate,
    /// Leave the recorded axis exactly as it stands. The stateless
    /// caller-supplied-request route, which publishes no cache slot: it
    /// owes no invalidation record and must not narrow another lane's.
    LeaveUntouched,
}

/// The Vue arm's own cross-file execution-input preparation: resolve the
/// macro bundle at the demanded depth, restate the compiled file's
/// dependency/semantic axis (when the route owns one) with the transitive
/// dependencies that resolution observed, refuse on this file's macro
/// dependency diagnostics, and build the ephemeral execution-input
/// carrier.
///
/// The axis restatement runs BEFORE the dependency refusal so a compile
/// refused for an unresolvable macro type still records the dependencies
/// whose repair must invalidate it — otherwise fixing the missing type
/// would never re-run this file.
///
/// Shared by both Vue issuance routes so a caller-supplied canonical
/// request and a profile-derived demand execute over identical resolved
/// inputs; the routes differ only in the demand document they admit and
/// in whether they own the shared dependency axis.
fn prepare_vue_execution_inputs(
    host: &crate::VerterHost,
    snapshot: &CompileInput,
    macro_demand: crate::typeinfo::vue_macro_codegen::VueMacroCodegenDemand,
    axis: SharedDependencyAxis,
) -> Result<VueHostExecutionInputs, HostProductsFailure> {
    let macro_output = host.produce_vue_macro_codegen(&snapshot.canonical_id, macro_demand);
    let macro_dependency_diagnostics =
        super::vue_macro_dependency_diagnostics::collect(host, snapshot, &macro_output);
    if axis == SharedDependencyAxis::Restate {
        let transitive_macro_type_deps: std::collections::BTreeSet<String> =
            macro_output.transitive_canonicals.iter().cloned().collect();
        host.sync_transitive_macro_type_dependencies(
            &snapshot.canonical_id,
            &transitive_macro_type_deps,
        );
    }
    if !macro_dependency_diagnostics.is_empty() {
        return Err(HostProductsFailure::Fatal(DiagnosticsSnapshot::from_vec(
            macro_dependency_diagnostics,
        )));
    }
    // The host-resolved Vue cross-file inputs ride on the typed,
    // ephemeral `VueExecutionInputs` carrier — excluded from
    // `CompileRequest` identity, and reachable only from here.
    let vue_facts = verter_compiler::compile::types::VueExecutionInputs {
        macro_runtime: macro_output.runtime,
        prop_constness_overrides: None, // populated by the cross-file optimizer
        style_v_bind_vars: snapshot.style_v_bind_vars.clone(),
        style_v_bind_usage_complete: Some(snapshot.style_v_bind_usage_complete),
        template_binding_metadata: None,
        template_used_vars: None,
        runtime_template_hole: false,
        runtime_inline_template_chunk: false,
        prepared_styles: snapshot.prepared_styles.clone(),
    };
    Ok(VueHostExecutionInputs {
        block_content: snapshot.block_content_inputs.clone(),
        vue_facts: Some(vue_facts),
        prepared_styles: snapshot.prepared_styles.clone(),
    })
}

/// The Svelte arm's own execution-input preparation.
///
/// This arm's execution inputs come entirely from the presented artifact
/// and the caller's inputs, so it contributes no cross-file semantic type
/// dependencies. A route that OWNS the axis still restates it with that
/// empty contribution: the axis is REPLACED, not merged, so skipping the
/// restatement would leave a previous compute's edges standing for this
/// file. Placed before the caller's demand decode for the same reason the
/// Vue arm places it before its dependency refusal — a refused compile
/// still restates the axis.
fn prepare_svelte_execution_inputs(
    host: &crate::VerterHost,
    snapshot: &CompileInput,
    css_hash_override: Option<String>,
    axis: SharedDependencyAxis,
) -> SvelteHostExecutionInputs {
    if axis == SharedDependencyAxis::Restate {
        host.sync_transitive_macro_type_dependencies(
            &snapshot.canonical_id,
            &std::collections::BTreeSet::new(),
        );
    }
    SvelteHostExecutionInputs {
        block_content: snapshot.block_content_inputs.clone(),
        css_hash_override,
        prepared_styles: snapshot.prepared_styles.clone(),
    }
}

/// Executing a caller-supplied canonical request refused before any
/// product published.
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
/// lane — the same arm-local execution-input preparation, the same bound
/// backend, the same consume-once admission, the same single product
/// execution — because the demand document is the only thing that
/// differs between the two.
///
/// The Vue macro bundle depth follows the request's OWN product set: a
/// runtime product renders the runtime `props` / `model` option objects
/// and is the one demand that pays for per-member broad-runtime
/// classification; every other demand takes the public binding names.
pub(crate) fn execute_supplied_host_request(
    host: &crate::VerterHost,
    binding: super::native_host_binding::BoundNativeHostRequest,
    artifact: &verter_compiler::framework_common::FrameworkParseArtifact,
    request: verter_compiler::compile_request::CompileRequest,
    snapshot: &CompileInput,
    alloc: &oxc_allocator::Allocator,
) -> Result<BoundCompiledProducts, SuppliedRequestFailure> {
    use super::native_host_binding::BoundNativeHostRequest;
    use crate::host_compile_audit::{debug_assert_compile_bound_attribution, BoundCompileRoute};
    use crate::typeinfo::vue_macro_codegen::VueMacroCodegenDemand;
    use verter_compiler::compile_request::ProductKind;
    use verter_compiler::framework_common::FrameworkHostIntegrationBackend as _;

    let source_len = snapshot.source.len() as u32;
    let wants_runtime = request.products().iter().any(|product| {
        matches!(
            product.kind(),
            ProductKind::RuntimeClient | ProductKind::RuntimeServer
        )
    });
    match binding {
        BoundNativeHostRequest::Vue(bound) => {
            let (backend, attribution) = bound.into_host_backend();
            debug_assert_compile_bound_attribution(
                BoundCompileRoute::HostBacked,
                &attribution,
                artifact,
                &snapshot.canonical_id,
            );
            let macro_demand = if wants_runtime {
                VueMacroCodegenDemand::Runtime
            } else {
                VueMacroCodegenDemand::RuntimeBindingNames
            };
            let inputs = prepare_vue_execution_inputs(
                host,
                snapshot,
                macro_demand,
                SharedDependencyAxis::LeaveUntouched,
            )
            .map_err(SuppliedRequestFailure::Products)?;
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
            // The Svelte `cssHash` scope-class override is a host-resolved
            // EXECUTION input with no canonical-request field, so this
            // route leaves it unset and the carrier derives its own scope
            // class. A caller that needs a specific class still has the
            // profile-derived lane.
            let inputs = prepare_svelte_execution_inputs(
                host,
                snapshot,
                None,
                SharedDependencyAxis::LeaveUntouched,
            );
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

/// `HOST_COMPILE_PRODUCT_UNSUPPORTED`: the bound host integration has no
/// production route for a demanded product kind. The kind is named so the
/// refusal is readable without parsing the message.
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
pub(crate) fn vue_runtime_render_demand(
    profile: &CompileProfile,
    canonical_id: &str,
    template_fact_diagnostics: bool,
) -> VueHostRuntimeRenderDemand {
    use verter_compiler::compile_request::RuntimeProductRequest;
    VueHostRuntimeRenderDemand {
        runtime: RuntimeProductRequest {
            inline: profile.inline,
            runtime_source_map: profile.source_map,
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
    }
}

/// The render lane's Svelte-BOUND demand: one runtime product (client, or
/// server under `ssr`) with the exact requested style policy over the
/// option axes the profile can express, decoded through the SAME typed
/// admission the host-backed Svelte constructor uses — a malformed token
/// refuses HERE, never a silent default, and an axis the bound execution
/// cannot honor refuses typed at the backend's issuance.
pub(crate) fn svelte_runtime_render_demand(
    profile: &CompileProfile,
    canonical_id: &str,
) -> Result<SvelteHostRuntimeRenderDemand, verter_compiler::compile_request::CompileRequestError> {
    use verter_compiler::compile_request::RuntimeProductRequest;
    Ok(SvelteHostRuntimeRenderDemand {
        runtime: RuntimeProductRequest {
            inline: profile.inline,
            runtime_source_map: profile.source_map,
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
                other => Err(CompileRequestError::MalformedOptionValue {
                    option: FrameworkOption::Svelte(SvelteOption::CompileOptionsNamespace),
                    value: other.to_string(),
                }),
            })
            .transpose()?;
        let fragments = profile
            .svelte_fragments
            .as_deref()
            .map(|token| match token {
                "html" => Ok(SvelteFragmentsRequest::Html),
                "tree" => Ok(SvelteFragmentsRequest::Tree),
                other => Err(CompileRequestError::MalformedOptionValue {
                    option: FrameworkOption::Svelte(SvelteOption::CompileOptionsFragments),
                    value: other.to_string(),
                }),
            })
            .transpose()?;
        // Same decode-boundary rationale as `namespace`/`fragments` above.
        let css = profile
            .svelte_css
            .as_deref()
            .map(|token| match token {
                "injected" => Ok(SvelteCssRequest::Injected),
                "external" => Ok(SvelteCssRequest::External),
                other => Err(CompileRequestError::MalformedOptionValue {
                    option: FrameworkOption::Svelte(SvelteOption::CompileOptionsCss),
                    value: other.to_string(),
                }),
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
pub(crate) fn request_construction_refused_diagnostics(
    canonical_id: &str,
    source_len: u32,
    error: &verter_compiler::compile_request::CompileRequestError,
) -> DiagnosticsSnapshot {
    DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
        severity: HostSeverity::Error,
        code: "HOST_COMPILE_REQUEST_EXECUTION_REFUSED".to_string(),
        message: format!("compile request construction refused for '{canonical_id}': {error:?}"),
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
    DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
        severity: HostSeverity::Error,
        code: compile_unsupported_code(unsupported).to_string(),
        message: format!(
            "carrier '{}' cannot produce a runtime bundle for '{}'",
            artifact.adapter_id().as_str(),
            canonical_id
        ),
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
        refusal @ (VueHostCompileRefusal::AdmissionParseMismatch
        | VueHostCompileRefusal::WrongDemand { .. }) => RenderExecutionRefusal::Fatal(
            admission_refused_diagnostics(canonical_id, source_len, &format!("{refusal:?}")),
        ),
    }
}

/// Maps a bound render EXECUTION refusal onto the lane's `HostError`
/// surface: a runtime-surface refusal is the typed
/// `HostError::RuntimeSurfaceRefused`; everything else is a fatal compile
/// failure carrying the request's diagnostics plus the refusal's payload.
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
        refusal @ (VueHostCompileRefusal::AdmissionParseMismatch
        | VueHostCompileRefusal::WrongDemand { .. }) => HostProductsFailure::Fatal(
            admission_refused_diagnostics(canonical_id, source_len, &format!("{refusal:?}")),
        ),
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let err = demand.svelte_options.into_request().expect_err(
            "experimental_async must refuse — SVELTE-MODULE is unsupported fail-closed",
        );
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
}

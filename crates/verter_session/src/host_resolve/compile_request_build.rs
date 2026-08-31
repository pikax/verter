//! The session's per-file/virtual-product `CompileRequest` construction
//! authority — the construct-then-derive pattern
//! [`crate::host_resolve::virtual_file_pipeline`]'s `compile_entry` routes
//! every host-backed compile through: [`build_vue_compile_request`] /
//! [`build_svelte_compile_request`] admission-check every option
//! `CompileProfile` carries and return the canonical, validated request;
//! then [`derive_runtime_compile_options`] reads the framework-neutral
//! `RuntimeCompileOptions` back off that validated request — never the
//! reverse. Mirrors the pattern the internal one-shot compile route
//! (`crate::compile::derive_legacy_vue_options`, in reverse) already
//! establishes.
//!
//! There is no shared framework fork here: the caller selects the
//! framework constructor from its request-scoped native host binding (the
//! sole framework-identity derivation site for host compile requests).
//! The runtime-render lane does not construct a session-side
//! `CompileRequest` at all: it builds the framework host backend's own
//! render-only demand ([`vue_runtime_render_demand`] /
//! [`svelte_runtime_render_demand`]) from the same profile axes, and the
//! bound backend composes the canonical request inside its issued
//! admission.

use crate::types::*;
use verter_compiler::framework_common::{
    RuntimeCompileOptions, SvelteHostRuntimeRenderDemand, VueHostRuntimeRenderDemand,
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
        AnalysisProductRequest, CompileProduct, IdeProductRequest, RuntimeProductRequest,
    };

    let mut products = Vec::new();
    if want_runtime {
        let runtime_product = RuntimeProductRequest {
            inline: profile.inline,
            runtime_source_map: profile.source_map,
            ..Default::default()
        };
        products.push(if profile.ssr {
            CompileProduct::RuntimeServer(runtime_product)
        } else {
            CompileProduct::RuntimeClient(runtime_product)
        });
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
        // No product actually demanded by this call — mirrors
        // `host_compile_audit::request_from_target`'s identical fallback:
        // `CompileRequest::new` refuses an empty product set outright, and
        // every caller of this route has always received SOME compiled
        // surface, never a deliberate zero-output compile.
        products.push(CompileProduct::RuntimeClient(RuntimeProductRequest {
            inline: profile.inline,
            runtime_source_map: profile.source_map,
            ..Default::default()
        }));
    }
    products
}

/// Finishes a validated framework request into the canonical
/// `CompileRequest` — the common construction tail both framework
/// constructors share.
fn finish_compile_request(
    products: Vec<verter_compiler::compile_request::CompileProduct>,
    framework: verter_compiler::compile_request::FrameworkCompileRequest,
    profile: &CompileProfile,
    canonical_id: &str,
) -> Result<
    verter_compiler::compile_request::CompileRequest,
    verter_compiler::compile_request::CompileRequestError,
> {
    verter_compiler::compile_request::CompileRequest::new(
        products,
        framework,
        None,
        profile
            .filename
            .clone()
            .or_else(|| Some(canonical_id.to_string())),
        profile.component_id.clone(),
        profile.is_production,
        profile.force_js,
    )
}

/// Builds and admission-checks the canonical VUE-shaped `CompileRequest`
/// for a session compile: every Vue option `CompileProfile` carries is
/// validated through `VueOptionAttempt::into_request`, so an unrecognized
/// option value or combination refuses HERE, before any codegen input is
/// built. The caller selects this constructor from its request-scoped
/// native host binding's Vue arm — never from language classification.
pub(crate) fn build_vue_compile_request(
    profile: &CompileProfile,
    canonical_id: &str,
    want_runtime: bool,
    want_ide: bool,
    want_template_data: bool,
) -> Result<
    verter_compiler::compile_request::CompileRequest,
    verter_compiler::compile_request::CompileRequestError,
> {
    use verter_compiler::compile_request::FrameworkCompileRequest;

    let products = demanded_products(profile, want_runtime, want_ide, want_template_data);
    let attempt = vue_option_attempt_from_profile(profile);
    let framework = FrameworkCompileRequest::Vue(attempt.into_request()?);
    finish_compile_request(products, framework, profile, canonical_id)
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

/// Selects the framework constructor from the consumed request-scoped
/// binding's catalog arm — the sole framework-identity derivation site for
/// host compile requests. Used by the host-backed `compile_entry` route;
/// the runtime-render route builds no session-side request at all (see the
/// bound render-demand constructors below).
pub(crate) fn build_bound_compile_request(
    binding: &super::native_host_binding::BoundNativeHostRequest,
    profile: &CompileProfile,
    canonical_id: &str,
    want_runtime: bool,
    want_ide: bool,
    want_template_data: bool,
) -> Result<
    verter_compiler::compile_request::CompileRequest,
    verter_compiler::compile_request::CompileRequestError,
> {
    use super::native_host_binding::BoundNativeHostRequest;
    match binding {
        BoundNativeHostRequest::Vue(_) => build_vue_compile_request(
            profile,
            canonical_id,
            want_runtime,
            want_ide,
            want_template_data,
        ),
        BoundNativeHostRequest::Svelte(_) => build_svelte_compile_request(
            profile,
            canonical_id,
            want_runtime,
            want_ide,
            want_template_data,
        ),
    }
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

/// Builds and admission-checks the canonical SVELTE-shaped
/// `CompileRequest` for a session compile: every Svelte option
/// `CompileProfile` carries is validated through
/// `SvelteOptionAttempt::into_request`, so an unrecognized option value or
/// combination refuses HERE, before any codegen input is built. The caller
/// selects this constructor from its request-scoped native host binding's
/// Svelte arm — never from language classification.
pub(crate) fn build_svelte_compile_request(
    profile: &CompileProfile,
    canonical_id: &str,
    want_runtime: bool,
    want_ide: bool,
    want_template_data: bool,
) -> Result<
    verter_compiler::compile_request::CompileRequest,
    verter_compiler::compile_request::CompileRequestError,
> {
    use verter_compiler::compile_request::FrameworkCompileRequest;

    let products = demanded_products(profile, want_runtime, want_ide, want_template_data);
    let attempt = svelte_option_attempt_from_profile(profile)?;
    let framework = FrameworkCompileRequest::Svelte(attempt.into_request()?);
    finish_compile_request(products, framework, profile, canonical_id)
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

/// Reads the framework-neutral `RuntimeCompileOptions` back off a
/// constructor-validated request ([`build_vue_compile_request`] / [`build_svelte_compile_request`]) — the companion half of the
/// construct-then-derive pattern. `svelte_namespace`/`svelte_fragments`
/// (raw strings) and `svelte_runes` (a bare bool) read from `profile`
/// directly rather than round-tripping back out of the request's typed
/// enums: request construction already validated them as part of
/// constructing this EXACT request (a namespace/fragments token that
/// failed to parse never reaches this function at all — construction
/// returned `Err` first), so re-reading the pre-validated originals here
/// carries no admission risk, it just avoids an enum-to-string round trip
/// the carrier's own string-typed fields don't need.
pub(crate) fn derive_runtime_compile_options(
    request: &verter_compiler::compile_request::CompileRequest,
    profile: &CompileProfile,
    block_content: verter_compiler::framework_common::RuntimeBlockContentInputs,
    vue_facts: Option<verter_compiler::compile::types::VueExecutionInputs>,
    prepared_styles: Vec<Option<verter_compiler::style_planner::PreparedStyleIr>>,
) -> RuntimeCompileOptions {
    use verter_compiler::compile_request::{CompileProduct, VueBackendRequest};

    let runtime = request.products().iter().find_map(|p| match p {
        CompileProduct::RuntimeClient(r) | CompileProduct::RuntimeServer(r) => Some(r),
        _ => None,
    });
    let ide = request.products().iter().find_map(|p| match p {
        CompileProduct::IdeCompanion(i) => Some(i),
        _ => None,
    });
    let want_template_data = request
        .products()
        .iter()
        .any(|p| matches!(p, CompileProduct::Analysis(a) if a.want_template_data));
    let ssr = request
        .products()
        .iter()
        .any(|p| matches!(p, CompileProduct::RuntimeServer(_)));

    let vue = request.vue();
    let svelte = request.svelte();

    let mut prepared_styles = prepared_styles;
    for (index, slot) in block_content.styles.iter().enumerate() {
        if let Some(parsed) = slot.as_ref().and_then(|input| input.parsed.clone()) {
            if prepared_styles.len() <= index {
                prepared_styles.resize(index + 1, None);
            }
            prepared_styles[index] = Some(parsed);
        }
    }

    RuntimeCompileOptions {
        filename: request.filename().map(str::to_string),
        is_production: request.is_production(),
        custom_element: vue
            .and_then(|v| v.script_custom_element)
            .or_else(|| svelte.and_then(|s| s.custom_element))
            .unwrap_or(false),
        source_map: runtime.is_some_and(|r| r.runtime_source_map)
            || ide.is_some_and(|i| i.want_source_map),
        // The session's compatibility route keeps its historical shared
        // map flag: `None` couples the IDE leg to `source_map` unchanged.
        ide_source_map: None,
        ssr,
        runtime_module_name: vue.and_then(|v| v.runtime_module_name.clone()),
        component_id: request.component_id().map(str::to_string),
        svelte_css_hash_override: profile.svelte_css_hash_override.clone(),
        force_js: request.force_js(),
        force_vapor: vue.is_some_and(|v| matches!(v.backend, VueBackendRequest::Vapor)),
        inline: runtime.and_then(|r| r.inline),
        comments: vue.and_then(|v| v.comments),
        delimiters: vue.and_then(|v| v.delimiters.clone()),
        custom_elements: vue
            .map(|v| v.is_custom_element.clone())
            .filter(|v| !v.is_empty()),
        want_runtime: runtime.is_some(),
        want_ide: ide.is_some(),
        want_template_data,
        types_module_name: ide.and_then(|i| i.types_module_name.clone()),
        embed_ambient_types: ide.is_some_and(|i| i.embed_ambient_types),
        conditional_root_narrowing: ide.is_some_and(|i| i.conditional_root_narrowing),
        strict_slots: ide.is_some_and(|i| i.strict_slots),
        svelte_dev: svelte.and_then(|s| s.dev),
        svelte_runes: profile.svelte_runes,
        svelte_namespace: profile.svelte_namespace.clone(),
        svelte_fragments: profile.svelte_fragments.clone(),
        svelte_preserve_whitespace: svelte.and_then(|s| s.preserve_whitespace),
        svelte_preserve_comments: svelte.and_then(|s| s.preserve_comments),
        svelte_disclose_version: svelte.and_then(|s| s.disclose_version),
        block_content,
        vue_facts,
        prepared_styles,
    }
}

/// Maps a [`verter_compiler::compile_request::CompileRequestError`] (a
/// construction refusal) onto the same
/// `HOST_COMPILE_REQUEST_EXECUTION_REFUSED` diagnostic code the
/// `compile_bundle`-level `CompileUnsupported::RequestExecutionRefused`
/// arm already reports — the request-construction refusal and the
/// post-parse resolution refusal both name the same host-facing code, only
/// the message differs.
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

/// Maps a bound framework host backend's Vue runtime-render admission
/// refusal onto the host diagnostics surface: a canonical-request
/// construction refusal keeps the SAME
/// `HOST_COMPILE_REQUEST_EXECUTION_REFUSED` code the host-backed
/// constructor route reports for the identical demand, and every other
/// typed issuance refusal (unavailable capability, unproducible demand,
/// non-composable parse) surfaces as `HOST_COMPILE_ADMISSION_REFUSED` —
/// never a fallback lane, framework, or compatibility compiler.
pub(crate) fn vue_render_admission_refused_diagnostics(
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

/// Svelte sibling of [`vue_render_admission_refused_diagnostics`].
pub(crate) fn svelte_render_admission_refused_diagnostics(
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

/// `HOST_NO_CARRIER_COMPILER`: the artifact's registered identity has no
/// carrier compiler registered for its adapter/language pair.
pub(crate) fn no_carrier_compiler_diagnostics(
    artifact: &verter_compiler::framework_common::FrameworkParseArtifact,
    source_len: u32,
) -> DiagnosticsSnapshot {
    DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
        severity: HostSeverity::Error,
        code: "HOST_NO_CARRIER_COMPILER".to_string(),
        message: format!(
            "no carrier compiler for adapter '{}' / language '{}'",
            artifact.adapter_id().as_str(),
            artifact.language_id().as_str()
        ),
        arguments: Vec::new(),
        span: verter_span::Span::new(0, source_len),
    }])
}

/// The runtime-render lane's fatal diagnostic for a bound backend
/// execution the shared orchestration refused
/// (`verter_compiler::framework_common::CompileUnsupported`): the same
/// stable per-arm code and carrier message the host-backed route reports
/// for the identical refusal, so the two lanes cannot drift on this
/// surface.
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

    /// The newly-wired Svelte `CompileProfile` fields (`css`, the
    /// custom-element `tag`/`shadow` descriptor axis, `compatibility`)
    /// survive `build_svelte_compile_request` end to end onto the canonical
    /// `SvelteCompileRequest` — the same representation guarantee
    /// `svelte_runes`/`svelte_namespace`/etc already had.
    /// `svelte_generate_module`/`svelte_experimental_async` are NOT
    /// asserted here — they refuse at construction; see
    /// `svelte_generate_module_refuses_the_svelte_module_capability` /
    /// `svelte_experimental_async_refuses_the_svelte_module_capability`
    /// below.
    #[test]
    fn newly_wired_svelte_fields_survive_request_construction() {
        let profile = CompileProfile {
            target: CompileTarget::BUNDLER,
            svelte_css: Some("injected".to_string()),
            svelte_custom_element_tag: Some("my-widget".to_string()),
            svelte_custom_element_shadow: Some(false),
            svelte_compatibility: Some(true),
            ..CompileProfile::default()
        };
        let request = build_svelte_compile_request(&profile, "/w.svelte", true, false, false)
            .expect("every newly-wired field is individually supported-canonical");
        let svelte = request.svelte().expect("Svelte framework request");
        assert_eq!(
            svelte.css,
            Some(verter_compiler::compile_request::svelte::SvelteCssRequest::Injected)
        );
        let descriptor = svelte
            .custom_element_descriptor
            .as_ref()
            .expect("tag/shadow set => descriptor constructed");
        assert_eq!(descriptor.tag.as_deref(), Some("my-widget"));
        assert_eq!(descriptor.shadow, Some(false));
        assert!(descriptor.props.is_empty());
        assert!(svelte.compatibility.is_some());
    }

    /// Negative control: leaving every new field unset must NOT construct
    /// a descriptor at all (an all-`None` descriptor is a different
    /// request from no descriptor — see `resolve_custom_element`'s
    /// `compile_option_descriptor` fallback precedence).
    #[test]
    fn unset_custom_element_fields_construct_no_descriptor() {
        let profile = CompileProfile {
            target: CompileTarget::BUNDLER,
            ..CompileProfile::default()
        };
        let request = build_svelte_compile_request(&profile, "/w.svelte", true, false, false)
            .expect("default profile constructs");
        let svelte = request.svelte().expect("Svelte framework request");
        assert!(svelte.custom_element_descriptor.is_none());
        assert_eq!(svelte.css, None);
    }

    /// `svelte_generate_module` set on the session `CompileProfile` must
    /// refuse construction — `ModuleCompileOptions.generate` is gated by
    /// the `SVELTE-MODULE` capability, `unsupported fail-closed` per
    /// `capability-matrix.tsv`. Session-level regression for the same
    /// refusal `SvelteOptionAttempt::into_request` proves directly.
    #[test]
    fn svelte_generate_module_refuses_the_svelte_module_capability() {
        let profile = CompileProfile {
            target: CompileTarget::BUNDLER,
            svelte_generate_module: Some(true),
            ..CompileProfile::default()
        };
        let err = build_svelte_compile_request(&profile, "/w.svelte", true, false, false)
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
        let err = build_svelte_compile_request(&profile, "/w.svelte", true, false, false)
            .expect_err(
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

    /// A malformed `svelte_css` value refuses at construction, matching
    /// `svelte_namespace`/`svelte_fragments`'s decode-boundary refusal.
    #[test]
    fn malformed_svelte_css_value_refuses() {
        let profile = CompileProfile {
            target: CompileTarget::BUNDLER,
            svelte_css: Some("not-a-real-mode".to_string()),
            ..CompileProfile::default()
        };
        let err = build_svelte_compile_request(&profile, "/w.svelte", true, false, false)
            .expect_err("an unrecognized css mode string must refuse");
        assert!(matches!(
            err,
            verter_compiler::compile_request::CompileRequestError::MalformedOptionValue { .. }
        ));
    }
}

//! The session's per-file/virtual-product `CompileRequest` construction
//! authority — the construct-then-derive pattern
//! [`crate::host_resolve::virtual_file_pipeline`]'s `compile_entry` /
//! `compile_entry_runtime_render` route every compile through:
//! [`build_vue_compile_request`] / [`build_svelte_compile_request`]
//! admission-check every option `CompileProfile` carries and return the
//! canonical, validated request; then [`derive_runtime_compile_options`]
//! reads the framework-neutral `RuntimeCompileOptions` back off that
//! validated request — never the reverse. Mirrors the pattern the internal
//! one-shot compile route (`crate::compile::derive_legacy_vue_options`, in
//! reverse) already establishes.
//!
//! There is no shared framework fork here: the caller selects the
//! framework constructor from its request-scoped native host binding (the
//! sole framework-identity derivation site for host compile requests), and
//! the runtime-render compatibility route uses its own fixed-Vue
//! constructor ([`render_lane_vue_compile_request`]).

use crate::types::*;
use verter_compiler::framework_common::RuntimeCompileOptions;

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
    use verter_compiler::compile_request::{
        FrameworkCompileRequest, VueBackendRequest, VueOptionAttempt,
    };

    let products = demanded_products(profile, want_runtime, want_ide, want_template_data);
    let attempt = VueOptionAttempt {
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
    };
    let framework = FrameworkCompileRequest::Vue(attempt.into_request()?);
    finish_compile_request(products, framework, profile, canonical_id)
}

/// Selects the framework constructor from the consumed request-scoped
/// binding's catalog arm — the sole framework-identity derivation site for
/// host compile requests. Used by the host-backed `compile_entry` route;
/// the runtime-render route keeps its own fixed-Vue constructor below.
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

/// The runtime-render compatibility route's OWN fixed-Vue request
/// constructor: the render lane's whole subject is the runtime `Main`
/// module, so the runtime products are always demanded regardless of the
/// caller's target bits, and the request is built in the Vue shape for
/// every carrier (the characterized transitional request shape — see
/// `runtime_render_builds_a_vue_shaped_request_for_a_svelte_carrier`).
/// Render-lane-only: it exists solely for this compatibility route and is
/// deleted with it once the route executes through its bound backend; the
/// request-scoped binding already supplies the route's identity/audit
/// coherence.
pub(crate) fn render_lane_vue_compile_request(
    profile: &CompileProfile,
    canonical_id: &str,
    want_ide: bool,
    want_template_data: bool,
) -> Result<
    verter_compiler::compile_request::CompileRequest,
    verter_compiler::compile_request::CompileRequestError,
> {
    build_vue_compile_request(profile, canonical_id, true, want_ide, want_template_data)
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
    use verter_compiler::compile_request::svelte::{
        SvelteCompatibilityRequest, SvelteCssRequest, SvelteCustomElementDescriptor,
        SvelteFragmentsRequest, SvelteNamespaceRequest, SvelteRunesRequest,
    };
    use verter_compiler::compile_request::{
        CompileRequestError, FrameworkCompileRequest, FrameworkOption, SvelteOption,
        SvelteOptionAttempt,
    };

    let products = demanded_products(profile, want_runtime, want_ide, want_template_data);
    let framework = {
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
        let attempt = SvelteOptionAttempt {
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
        };
        FrameworkCompileRequest::Svelte(attempt.into_request()?)
    };
    finish_compile_request(products, framework, profile, canonical_id)
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

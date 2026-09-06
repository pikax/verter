//! FFI → host input conversions: config, profile, file/node kinds, upserts,
//! block overrides, and virtual-query requests.

use std::sync::Arc;

use verter_session as host;
use verter_session::StaticClassification;

use crate::types::*;

use super::error::FfiConversionError;

pub fn ffi_config_to_host(input: FfiHostConfig) -> Result<host::HostConfig, FfiConversionError> {
    let mut out = host::HostConfig::default();
    if let Some(dev_mode) = input.dev_mode {
        out.dev_mode = dev_mode;
    }
    if let Some(policy) = input.compile_error_policy {
        out.compile_error_policy = if policy.eq_ignore_ascii_case("strict")
            || policy.eq_ignore_ascii_case("strict_error")
            || policy.eq_ignore_ascii_case("strictError")
        {
            host::CompileErrorPolicy::StrictError
        } else if policy.eq_ignore_ascii_case("dev")
            || policy.eq_ignore_ascii_case("dev_serve_last_known_good")
            || policy.eq_ignore_ascii_case("devServeLastKnownGood")
        {
            host::CompileErrorPolicy::DevServeLastKnownGood
        } else {
            return Err(FfiConversionError::InvalidCompileErrorPolicy(policy));
        };
    }
    if let Some(lsp_scheme) = input.lsp_scheme {
        out.lsp_scheme = lsp_scheme;
    }
    if let Some(max_profiles) = input.max_profiles_per_file {
        out.max_profiles_per_file = max_profiles as usize;
    }
    if let Some(extensions) = input.resolve_extensions {
        out.resolve_extensions = extensions;
    }
    if let Some(level) = input.analysis_level {
        out.analysis_level = if level.eq_ignore_ascii_case("none") {
            host::AnalysisLevel::None
        } else if level.eq_ignore_ascii_case("essential") {
            host::AnalysisLevel::Essential
        } else if level.eq_ignore_ascii_case("full") {
            host::AnalysisLevel::Full
        } else {
            return Err(FfiConversionError::InvalidAnalysisLevel(level));
        };
    }
    if let Some(audit) = input.audit_enabled {
        out.audit_enabled = audit;
    }
    if let Some(footprint) = input.footprint_capture {
        out.footprint_capture = footprint;
    }
    if let Some(cap) = input.typeinfo_scratch_cache_capacity {
        out.typeinfo_scratch_cache_capacity = Some(cap as usize);
    }
    if let Some(threads) = input.host_cpu_threads {
        out.host_cpu_threads = Some(threads as usize);
    }
    if let Some(metrics_enabled) = input.metrics_enabled {
        out.metrics_enabled = metrics_enabled;
    }
    Ok(out)
}

/// Convert FFI compile profile to internal compile profile.
///
/// EXHAUSTIVELY destructures `FfiCompileProfile` (no `..` rest pattern) so
/// a field added to the wire struct without a corresponding admission arm
/// here is a COMPILE ERROR, not a silently-dropped option — the
/// per-field-admission half of the boundary's "no silently ignored
/// option" contract. `#[serde(deny_unknown_fields)]` on `FfiCompileProfile`
/// itself is the companion decode-time half: an unrecognized wire KEY
/// refuses before this function ever runs; this exhaustive match is what
/// keeps every recognized field's mapping honest once it does.
pub fn ffi_profile_to_host(
    input: Option<FfiCompileProfile>,
) -> Result<host::CompileProfile, FfiConversionError> {
    let mut out = host::CompileProfile::default();
    let Some(input) = input else {
        return Ok(out);
    };
    let FfiCompileProfile {
        filename,
        is_production,
        custom_element,
        ssr,
        ssr_module_id,
        hmr_strategy,
        component_id,
        delimiters,
        custom_elements,
        comments,
        runtime_module_name,
        types_module_name,
        force_vapor,
        force_js,
        source_map,
        target,
        inline,
        strict_slots,
        requested_mode,
    } = input;

    out.filename = filename;
    if let Some(is_production) = is_production {
        out.is_production = is_production;
    }
    if let Some(custom_element) = custom_element {
        out.custom_element = custom_element;
    }
    if let Some(ssr) = ssr {
        out.ssr = ssr;
    }
    out.ssr_module_id = ssr_module_id;
    if let Some(hmr_strategy) = hmr_strategy {
        out.hmr_strategy = if hmr_strategy.eq_ignore_ascii_case("vite") {
            host::HmrStrategy::Vite
        } else if hmr_strategy.eq_ignore_ascii_case("webpack") {
            host::HmrStrategy::Webpack
        } else if hmr_strategy.eq_ignore_ascii_case("none") {
            host::HmrStrategy::None
        } else {
            return Err(FfiConversionError::InvalidHmrStrategy(hmr_strategy));
        };
    }
    out.component_id = component_id;
    out.delimiters = if let Some(d) = delimiters {
        if d.len() != 2 {
            return Err(FfiConversionError::InvalidDelimiters(d.len()));
        }
        Some((d[0].clone(), d[1].clone()))
    } else {
        None
    };
    out.custom_elements = custom_elements;
    out.comments = comments;
    if let Some(runtime_module_name) = runtime_module_name {
        out.runtime_module_name = Some(runtime_module_name);
    }
    if let Some(types_module_name) = types_module_name {
        out.types_module_name = Some(types_module_name);
    }
    if let Some(force_vapor) = force_vapor {
        out.force_vapor = force_vapor;
    }
    if let Some(force_js) = force_js {
        out.force_js = force_js;
    }
    if let Some(source_map) = source_map {
        out.source_map = source_map;
    }
    if let Some(target) = target {
        out.target = ffi_target_to_compile_target(&target)?;
    }
    out.inline = inline;
    if let Some(strict_slots) = strict_slots {
        out.strict_slots = strict_slots;
    }
    if let Some(requested_mode) = requested_mode {
        out.requested_mode = ffi_compile_cache_mode_to_host(&requested_mode)?;
    }
    Ok(out)
}

/// Parse a compile-cache-mode string to the host enum. Defaults are
/// applied by the caller (a missing field keeps the profile default
/// `Session`); this only parses an explicitly-supplied value. Exposed
/// for the NAPI / WASM batch bindings, which parse a per-input mode
/// string into [`host::CompileBatchInput::requested_mode`].
pub fn ffi_compile_cache_mode_to_host(
    mode: &str,
) -> Result<host::CompileCacheMode, FfiConversionError> {
    match mode.to_ascii_lowercase().as_str() {
        "stateless" => Ok(host::CompileCacheMode::Stateless),
        "content" => Ok(host::CompileCacheMode::Content),
        "session" => Ok(host::CompileCacheMode::Session),
        other => Err(FfiConversionError::InvalidCompileCacheMode(
            other.to_string(),
        )),
    }
}

/// Parse a `getPublicApi` mode string to the host [`host::PublicApiMode`].
/// An absent mode defaults to `Public` (backward-compatible with the
/// existing modeless callers); an unknown string is a typed error, never a
/// silent default. Exposed for the NAPI / WASM `getPublicApi` bindings —
/// the ONE shared allow-list, so the two bindings cannot diverge on which
/// mode strings they accept.
pub fn ffi_public_api_mode_to_host(
    mode: Option<&str>,
) -> Result<host::PublicApiMode, FfiConversionError> {
    let Some(mode) = mode else {
        return Ok(host::PublicApiMode::Public);
    };
    match mode.to_ascii_lowercase().as_str() {
        "public" => Ok(host::PublicApiMode::Public),
        "testing" => Ok(host::PublicApiMode::Testing),
        "declaration" => Ok(host::PublicApiMode::Declaration),
        other => Err(FfiConversionError::InvalidPublicApiMode(other.to_string())),
    }
}

/// Convert a wire preprocessor diagnostic to the host's typed record.
fn ffi_preprocessor_diagnostic_to_host(
    diagnostic: FfiPreprocessorDiagnostic,
) -> Result<host::PreprocessorDiagnostic, FfiConversionError> {
    let severity = match diagnostic.severity.to_ascii_lowercase().as_str() {
        "error" => host::HostSeverity::Error,
        "warning" => host::HostSeverity::Warning,
        "info" => host::HostSeverity::Info,
        other => {
            return Err(FfiConversionError::InvalidPreprocessorDiagnosticSeverity(
                other.to_string(),
            ))
        }
    };
    Ok(host::PreprocessorDiagnostic {
        severity,
        message: diagnostic.message,
        line: diagnostic.line,
        column: diagnostic.column,
    })
}

/// Convert a target string to `CompileTarget` bitflags.
pub(super) fn ffi_target_to_compile_target(
    target: &str,
) -> Result<host::CompileTarget, FfiConversionError> {
    use host::CompileTarget;
    match target.to_ascii_lowercase().as_str() {
        "bundler" => Ok(CompileTarget::BUNDLER),
        "ide" => Ok(CompileTarget::IDE),
        "analysis" => Ok(CompileTarget::ANALYSIS),
        "full" => Ok(CompileTarget::BUNDLER | CompileTarget::TSX | CompileTarget::TEMPLATE_DATA),
        other => Err(FfiConversionError::InvalidTarget(other.to_string())),
    }
}

/// Resolve the FFI `fileKind` string (plus the request's canonical
/// path) to the host [`host::FileLanguage`].
///
/// Accepted kind strings (add-only): `"vue"` / `"sfc"` / `"vue_sfc"` →
/// the Vue carrier; `"svelte"` → the Svelte carrier (paired with its
/// `LanguageRegistry` row — a carrier-less row serves the typed
/// unsupported-language state at dispatch); `"non_sfc"` / `"text"` /
/// `"file"` → a plain script (dialect derived from the path when one
/// is present).
///
/// ABSENT kind → STATIC-ONLY classification of the canonical path via
/// [`host::LanguageRegistry::classify_static`] — the ONE (logged)
/// lenient-inference point for the NAPI/WASM JS boundaries. FFI-time
/// classification never consults project capabilities, so a
/// gated-candidate extension can NEVER classify by inference here:
/// gated rows REQUIRE an explicit kind string (typed error otherwise).
/// Absent kind with no path is a typed error.
pub fn ffi_file_language_to_host(
    kind: Option<&str>,
    canonical_path: Option<&str>,
) -> Result<host::FileLanguage, FfiConversionError> {
    classify_ffi_file_language(host::LanguageRegistry::global(), kind, canonical_path)
}

/// Registry-parameterized core of [`ffi_file_language_to_host`] (unit
/// tests exercise the gated-row arm with a fixture registry).
pub(crate) fn classify_ffi_file_language(
    registry: &verter_session::LanguageRegistry,
    kind: Option<&str>,
    canonical_path: Option<&str>,
) -> Result<host::FileLanguage, FfiConversionError> {
    use verter_session::FileLanguage;
    match kind {
        Some(kind) => match kind.to_ascii_lowercase().as_str() {
            "vue" | "sfc" | "vue_sfc" => Ok(FileLanguage::vue()),
            "svelte" => Ok(FileLanguage::svelte()),
            "non_sfc" | "text" | "file" => {
                // Explicit plain-script request: derive the dialect from
                // the path when it statically resolves to a script row.
                let dialect = canonical_path
                    .map(|path| registry.classify_static(path).static_resolution())
                    .and_then(|language| language.script_source_type())
                    .unwrap_or(verter_session::ScriptSourceType::Ts);
                Ok(FileLanguage::script(dialect))
            }
            other => Err(FfiConversionError::InvalidFileKind(other.to_string())),
        },
        None => {
            let Some(path) = canonical_path else {
                return Err(FfiConversionError::MissingFileLanguagePath);
            };
            match registry.classify_static(path) {
                StaticClassification::Resolved(language) => {
                    log::debug!("file language inferred from path '{path}': {language:?}");
                    Ok(language)
                }
                StaticClassification::Unknown => {
                    log::debug!(
                        "file language inferred from path '{path}': unknown extension, \
                         routing as plain script"
                    );
                    Ok(FileLanguage::script_ts())
                }
                StaticClassification::Gated(_) => Err(
                    FfiConversionError::GatedFileLanguageRequiresExplicitKind(path.to_string()),
                ),
            }
        }
    }
}

/// Parse a virtual node kind from its FFI representation.
pub fn ffi_node_kind_to_host(
    input: FfiVirtualNodeKind,
) -> Result<host::VirtualNodeKind, FfiConversionError> {
    match input.kind.to_ascii_lowercase().as_str() {
        "main" => Ok(host::VirtualNodeKind::Main),
        "script" => Ok(host::VirtualNodeKind::Script),
        "template" => Ok(host::VirtualNodeKind::Template),
        "style" => Ok(host::VirtualNodeKind::Style {
            index: input.index.unwrap_or(0) as usize,
        }),
        "custom" => Ok(host::VirtualNodeKind::Custom {
            index: input.index.unwrap_or(0) as usize,
        }),
        other => Err(FfiConversionError::InvalidNodeKind(other.to_string())),
    }
}

/// Convert FFI upsert request to host upsert request.
pub fn ffi_upsert_to_host(
    input: FfiUpsertRequest,
) -> Result<host::UpsertRequest, FfiConversionError> {
    let classification_path = input
        .canonical_id
        .as_deref()
        .or(Some(input.input_id.as_str()));
    let file_language = ffi_file_language_to_host(input.file_kind.as_deref(), classification_path)?;
    Ok(host::UpsertRequest {
        canonical_id: input.canonical_id,
        input_id: input.input_id,
        source: Arc::from(input.source),
        file_language,
        aliases: input.aliases.unwrap_or_default(),
    })
}

/// Convert FFI block override request to host block override request.
pub fn ffi_block_override_to_host(
    input: FfiBlockOverrideRequest,
) -> Result<host::BlockOverrideRequest, FfiConversionError> {
    let FfiBlockOverrideRequest {
        canonical_id,
        compile_profile,
        overrides,
    } = input;
    let captured_canonical_id = canonical_id.clone();
    Ok(host::BlockOverrideRequest {
        canonical_id,
        compile_profile: ffi_profile_to_host(compile_profile)?,
        overrides: overrides
            .into_iter()
            .map(|entry| {
                let correlation_token =
                    host::BlockContentCorrelationToken::parse_untrusted(entry.correlation_token)
                        .ok_or(FfiConversionError::InvalidBlockContentToken(
                            "correlationToken",
                        ))?;
                let block_token =
                    host::carrier_publication_store::ArtifactBlockToken::parse_untrusted(
                        entry.block_token,
                    )
                    .ok_or(FfiConversionError::InvalidBlockContentToken("blockToken"))?;
                let owner_revision =
                    host::BlockContentOwnerRevisionToken::parse_untrusted(entry.owner_revision)
                        .ok_or(FfiConversionError::InvalidBlockContentToken(
                            "ownerRevision",
                        ))?;
                let artifact_token =
                    host::carrier_publication_store::FrameworkArtifactToken::parse_untrusted(
                        entry.artifact_token,
                    )
                    .ok_or(FfiConversionError::InvalidBlockContentToken(
                        "artifactToken",
                    ))?;
                let prior_basis_token = entry
                    .prior_basis_token
                    .map(|value| {
                        host::BlockContentBasisToken::parse_untrusted(value).ok_or(
                            FfiConversionError::InvalidBlockContentToken("priorBasisToken"),
                        )
                    })
                    .transpose()?;
                let basis_token = host::BlockContentBasisToken::parse_untrusted(entry.basis_token)
                    .ok_or(FfiConversionError::InvalidBlockContentToken("basisToken"))?;
                let captured_echo = host::BlockContentCapturedEcho {
                    request: host::BlockContentPreCaptureEcho {
                        correlation_token: correlation_token.clone(),
                        canonical_id: captured_canonical_id.clone(),
                        block_token: block_token.clone(),
                        owner_revision: owner_revision.clone(),
                        artifact_token: artifact_token.clone(),
                        expected_language: entry.expected_language,
                        prior_basis_token,
                    },
                    basis_token: basis_token.clone(),
                };
                Ok(host::BlockOverrideEntry {
                    correlation_token,
                    block_token,
                    owner_revision,
                    artifact_token,
                    basis_token,
                    captured_echo,
                    source_space_token: host::BlockContentSourceSpaceToken::parse_untrusted(
                        entry.source_space_token,
                    )
                    .ok_or(FfiConversionError::InvalidBlockContentToken(
                        "sourceSpaceToken",
                    ))?,
                    code: Arc::from(entry.code),
                    code_hash: host::BlockContentHashToken::parse_untrusted(entry.code_hash)
                        .ok_or(FfiConversionError::InvalidBlockContentToken("codeHash"))?,
                    source_map: entry.source_map.map(Arc::from),
                    source_map_hash: entry
                        .source_map_hash
                        .map(|value| {
                            host::BlockContentHashToken::parse_untrusted(value).ok_or(
                                FfiConversionError::InvalidBlockContentToken("sourceMapHash"),
                            )
                        })
                        .transpose()?,
                    dependencies: entry.dependencies,
                    diagnostics: entry
                        .diagnostics
                        .into_iter()
                        .map(ffi_preprocessor_diagnostic_to_host)
                        .collect::<Result<Vec<_>, FfiConversionError>>()?,
                    processor_identity: entry.processor_identity.unwrap_or_default(),
                    processor_version: entry.processor_version.unwrap_or_default(),
                    config_fingerprint: entry
                        .config_fingerprint
                        .map(|value| {
                            host::BlockContentHashToken::parse_untrusted(value).ok_or(
                                FfiConversionError::InvalidBlockContentToken("configFingerprint"),
                            )
                        })
                        .transpose()?,
                })
            })
            .collect::<Result<Vec<_>, FfiConversionError>>()?,
    })
}
// ── framework-discriminated host compile request → canonical request ─────

use verter_compiler::compile_request::svelte::{
    SvelteCompatibilityRequest, SvelteCssRequest, SvelteCustomElementDescriptor,
    SvelteCustomElementPropDescriptor, SvelteFragmentsRequest, SvelteNamespaceRequest,
    SvelteRunesRequest,
};
use verter_compiler::compile_request::vue::{
    VueAssetUrlOptions, VueAssetUrlTransform, VueBackendRequest, VueCssModuleLocalsConvention,
    VueCssModuleScopeBehaviour, VueCssModulesOptions, VueParsePad, VueWhitespaceStrategy,
};
use verter_compiler::compile_request::{
    AnalysisProductRequest, CompileProduct, CompileRequest, CompileRequestError,
    DeclarationProductRequest, FrameworkCompileRequest, FrameworkOption, IdeProductRequest,
    PublicApiProductRequest, RuntimeHmrStrategy, RuntimeProductRequest, RuntimeStyleProcessing,
    SvelteOptionAttempt, VueOption, VueOptionAttempt,
};
use verter_identity::profile::{
    OutputProfileId, PresentationProfileId, SerializationProfileId, TypeScriptSemanticProfileId,
};

/// Maps a wire enum onto its 1:1 canonical counterpart. A wire variant left
/// out of the list is a non-exhaustive-match compile error, so a variant
/// added later can never fall through to a substituted value.
macro_rules! map_variants {
    ($value:expr, $wire:ident => $canonical:ident { $($variant:ident),+ $(,)? }) => {
        match $value { $($wire::$variant => $canonical::$variant),+ }
    };
}

/// The profile identities the host derives for one compile. They are
/// content-derived digests, not caller-supplied values, so the wire schema
/// has no field to ask for them and the conversion never invents one: the
/// caller states them here, and each is placed on exactly the canonical
/// product slots that carry it.
///
/// Deliberately has no `Default`: an absent profile must be an explicit
/// `None` at the call site, never an implicit one inside the conversion.
///
/// Recorded limitation: one `output` / `presentation` / `serialization`
/// identity is fanned across every product of the compile. The canonical
/// products keep INDEPENDENT per-product profile slots on purpose (see
/// `verter_compiler::compile_request::product`), and this parameter cannot
/// express a compile whose products carry different ones. Nothing is
/// served wrongly — a uniform assignment is a legal point in the canonical
/// space — but per-product variation is unreachable through this entry
/// point. It is a host-side parameter with no wire representation, so
/// widening it to per-product identities later is not a wire change.
#[derive(Debug, Clone)]
pub struct HostResolvedCompileProfiles {
    pub semantic: Option<TypeScriptSemanticProfileId>,
    pub output: Option<OutputProfileId>,
    pub presentation: Option<PresentationProfileId>,
    pub serialization: Option<SerializationProfileId>,
}

/// Converts a framework-discriminated wire request into exactly ONE
/// canonical [`CompileRequest`].
///
/// The canonical request stays the single request authority: this function
/// only decodes and maps, then delegates every construction-time rule
/// (product minimality, backend/product legality, option admission) to it,
/// and returns its typed [`CompileRequestError`] verbatim. There is no
/// second refusal vocabulary and no substituted value on any path — an
/// option the wire omits is `None` at the boundary and stays whatever the
/// canonical request derives it to be.
///
/// Every wire struct on this path is DESTRUCTURED with no `..` rest
/// pattern, nested ones included, so a field added anywhere in the schema
/// is a compile error here rather than a silently dropped option.
pub fn ffi_host_compile_request_to_compile_request(
    request: FfiHostCompileRequest,
    profiles: &HostResolvedCompileProfiles,
) -> Result<CompileRequest, CompileRequestError> {
    let (identity, wire_products, framework) = match request {
        FfiHostCompileRequest::Vue(FfiVueHostCompileRequest {
            identity,
            products,
            options,
        }) => (
            identity,
            products,
            FrameworkCompileRequest::Vue(vue_options_to_attempt(options)?.into_request()?),
        ),
        FfiHostCompileRequest::Svelte(FfiSvelteHostCompileRequest {
            identity,
            products,
            options,
        }) => (
            identity,
            products,
            FrameworkCompileRequest::Svelte(svelte_options_to_attempt(options).into_request()?),
        ),
    };
    let FfiHostCompileIdentity {
        filename,
        component_id,
        is_production,
        force_js,
        ssr_module_id,
        hmr_strategy,
    } = identity;

    let products = wire_products
        .into_iter()
        .map(|product| requested_product_to_canonical(product, profiles))
        .collect();

    CompileRequest::new(
        products,
        framework,
        profiles.semantic.clone(),
        filename,
        component_id,
        is_production,
        force_js,
    )
    .map(|request| {
        request.with_host_assembly_axes(
            ssr_module_id,
            hmr_strategy.map_or(RuntimeHmrStrategy::None, |strategy| {
                map_variants!(strategy, FfiHmrStrategy => RuntimeHmrStrategy { None, Vite, Webpack })
            }),
        )
    })
}

fn requested_product_to_canonical(
    product: FfiRequestedProduct,
    profiles: &HostResolvedCompileProfiles,
) -> CompileProduct {
    let runtime = |FfiRuntimeProductRequest {
                       inline,
                       runtime_source_map,
                       style_processing,
                   }| RuntimeProductRequest {
        inline,
        runtime_source_map,
        style_processing: style_processing
            .map(|s| {
                map_variants!(s, FfiRuntimeStyleProcessing => RuntimeStyleProcessing {
                    Complete, AuthoredOnly
                })
            })
            .unwrap_or_default(),
        output_profile: profiles.output.clone(),
        serialization: profiles.serialization.clone(),
    };
    match product {
        FfiRequestedProduct::RuntimeClient(wire) => CompileProduct::RuntimeClient(runtime(wire)),
        FfiRequestedProduct::RuntimeServer(wire) => CompileProduct::RuntimeServer(runtime(wire)),
        FfiRequestedProduct::IdeCompanion(FfiIdeProductRequest {
            want_source_map,
            embed_ambient_types,
            conditional_root_narrowing,
            strict_slots,
            types_module_name,
            ide_chunk_boundaries,
        }) => CompileProduct::IdeCompanion(IdeProductRequest {
            want_source_map,
            embed_ambient_types,
            conditional_root_narrowing,
            strict_slots,
            types_module_name,
            ide_chunk_boundaries,
            output_profile: profiles.output.clone(),
            diagnostics: profiles.presentation.clone(),
            serialization: profiles.serialization.clone(),
        }),
        FfiRequestedProduct::PublicApi => CompileProduct::PublicApi(PublicApiProductRequest {
            output_profile: profiles.output.clone(),
            serialization: profiles.serialization.clone(),
        }),
        FfiRequestedProduct::Declarations => {
            CompileProduct::Declarations(DeclarationProductRequest {
                output_profile: profiles.output.clone(),
                serialization: profiles.serialization.clone(),
            })
        }
        FfiRequestedProduct::Analysis(FfiAnalysisProductRequest {
            want_script_bindings,
            want_template_data,
        }) => CompileProduct::Analysis(AnalysisProductRequest {
            want_script_bindings,
            want_template_data,
        }),
    }
}

/// Maps the wire Vue options onto the canonical admission surface. The
/// refused slots cross as presence, not value, so the canonical surface —
/// not this function — decides the refusal and names the row.
///
/// Destructured with no `..` rest pattern: a field added to the wire struct
/// without a mapping here is a COMPILE ERROR, never a silently dropped
/// option.
fn vue_options_to_attempt(
    options: FfiVueCompileOptions,
) -> Result<VueOptionAttempt, CompileRequestError> {
    let FfiVueCompileOptions {
        backend,
        ssr,
        is_custom_element,
        delimiters,
        whitespace,
        comments,
        hoist_static,
        cache_handlers,
        hmr,
        optimize_imports,
        runtime_module_name,
        ssr_runtime_module_name,
        parse_pad,
        ignore_empty,
        babel_parser_plugins,
        gen_default_as,
        props_destructure,
        script_custom_element,
        transform_asset_urls,
        style_trim,
        css_modules,
        compat_config,
        compat_config_mode,
        compat_config_compiler_is_on_element,
        compat_config_compiler_v_bind_sync,
        compat_config_compiler_v_if_v_for_precedence,
        compat_config_compiler_v_bind_object_order,
        compat_config_compiler_v_on_native,
        compat_config_compiler_native_template,
        compat_config_compiler_inline_template,
        compat_config_compiler_filters,
        transform_compat_config,
        codegen_mode,
    } = options;

    Ok(VueOptionAttempt {
        backend: map_variants!(backend, FfiVueBackend => VueBackendRequest {
            Inferred, Vdom, Vapor
        }),
        ssr,
        is_custom_element,
        delimiters: delimiters.map(vue_delimiter_pair).transpose()?,
        whitespace: whitespace.map(
            |w| map_variants!(w, FfiVueWhitespace => VueWhitespaceStrategy { Preserve, Condense }),
        ),
        comments,
        hoist_static,
        cache_handlers,
        hmr,
        optimize_imports,
        runtime_module_name,
        ssr_runtime_module_name,
        parse_pad: parse_pad
            .map(|p| map_variants!(p, FfiVueParsePad => VueParsePad { Space, Line, Off })),
        ignore_empty,
        babel_parser_plugins,
        gen_default_as,
        props_destructure,
        script_custom_element,
        transform_asset_urls: transform_asset_urls.map(|t| match t {
            FfiVueAssetUrlTransform::Disabled => VueAssetUrlTransform::Disabled,
            FfiVueAssetUrlTransform::Enabled(FfiVueAssetUrlOptions {
                base,
                include_absolute,
                tags,
            }) => VueAssetUrlTransform::Enabled(VueAssetUrlOptions {
                base,
                include_absolute,
                tags,
            }),
        }),
        style_trim,
        css_modules: css_modules.map(
            |FfiVueCssModules {
                 scope_behaviour,
                 hash_prefix,
                 locals_convention,
                 export_globals,
             }| VueCssModulesOptions {
                scope_behaviour: scope_behaviour.map(|s| {
                    map_variants!(s, FfiVueCssModuleScopeBehaviour => VueCssModuleScopeBehaviour {
                        Local, Global
                    })
                }),
                hash_prefix,
                locals_convention: locals_convention.map(|c| {
                    map_variants!(c, FfiVueCssModuleLocalsConvention => VueCssModuleLocalsConvention {
                        CamelCase, CamelCaseOnly, Dashes, DashesOnly, AsIs
                    })
                }),
                export_globals,
            },
        ),
        compat_config,
        compat_config_mode,
        compat_config_compiler_is_on_element,
        compat_config_compiler_v_bind_sync,
        compat_config_compiler_v_if_v_for_precedence,
        compat_config_compiler_v_bind_object_order,
        compat_config_compiler_v_on_native,
        compat_config_compiler_native_template,
        compat_config_compiler_inline_template,
        compat_config_compiler_filters,
        transform_compat_config,
        codegen_mode,
    })
}

/// A delimiter pair is exactly two strings. Any other arity is a typed
/// malformed-value refusal — never a fall back to the framework default.
fn vue_delimiter_pair(raw: Vec<String>) -> Result<(String, String), CompileRequestError> {
    let [open, close] = <[String; 2]>::try_from(raw).map_err(|raw| {
        CompileRequestError::malformed_option_value(
            FrameworkOption::Vue(VueOption::ParserOptionsDelimiters),
            raw.join(","),
        )
    })?;
    Ok((open, close))
}

/// The Svelte half of [`vue_options_to_attempt`], under the same
/// exhaustive-destructure and presence-not-value rules. Infallible: every
/// Svelte slot is a total mapping, so no refusal is reachable before
/// [`SvelteOptionAttempt::into_request`].
fn svelte_options_to_attempt(options: FfiSvelteCompileOptions) -> SvelteOptionAttempt {
    let FfiSvelteCompileOptions {
        dev,
        generate_module,
        experimental_async,
        custom_element,
        custom_element_descriptor,
        namespace,
        css,
        preserve_comments,
        preserve_whitespace,
        fragments,
        runes,
        disclose_version,
        compatibility,
        loose,
        accessors,
        immutable,
        compatibility_component_api,
        hmr,
        custom_element_extend,
    } = options;

    SvelteOptionAttempt {
        dev,
        generate_module,
        experimental_async,
        custom_element,
        custom_element_descriptor: custom_element_descriptor.map(svelte_custom_element_descriptor),
        namespace: namespace.map(|n| {
            map_variants!(n, FfiSvelteNamespace => SvelteNamespaceRequest {
                Html, Svg, MathMl, Foreign
            })
        }),
        css: css.map(|c| map_variants!(c, FfiSvelteCss => SvelteCssRequest { Injected, External })),
        preserve_comments,
        preserve_whitespace,
        fragments: fragments
            .map(|f| map_variants!(f, FfiSvelteFragments => SvelteFragmentsRequest { Html, Tree })),
        runes: runes
            .map(|r| map_variants!(r, FfiSvelteRunes => SvelteRunesRequest { True, False, Infer })),
        disclose_version,
        // The canonical compatibility request is a sealed marker with one
        // inhabitant, so `default()` is its only constructor, not a
        // substituted value: presence maps to presence.
        compatibility: compatibility
            .map(|FfiSvelteCompatibility {}| SvelteCompatibilityRequest::default()),
        loose,
        accessors,
        immutable,
        compatibility_component_api,
        hmr,
        custom_element_extend,
    }
}

fn svelte_custom_element_descriptor(
    descriptor: FfiSvelteCustomElementDescriptor,
) -> SvelteCustomElementDescriptor {
    let FfiSvelteCustomElementDescriptor { tag, shadow, props } = descriptor;
    SvelteCustomElementDescriptor {
        tag,
        shadow,
        props: props
            .into_iter()
            .map(
                |(
                    name,
                    FfiSvelteCustomElementProp {
                        attribute,
                        reflect,
                        prop_type,
                    },
                )| {
                    (
                        name,
                        SvelteCustomElementPropDescriptor {
                            attribute,
                            reflect,
                            prop_type,
                        },
                    )
                },
            )
            .collect(),
    }
}

pub fn ffi_virtual_query_to_host(
    input: FfiVirtualQuery,
) -> Result<host::VirtualQuery, FfiConversionError> {
    let node_kind = input.node_kind.map(ffi_node_kind_to_host).transpose()?;
    Ok(host::VirtualQuery {
        raw_id: input.raw_id,
        canonical_id: input.canonical_id,
        node_kind,
        compile_profile: ffi_profile_to_host(input.compile_profile)?,
    })
}

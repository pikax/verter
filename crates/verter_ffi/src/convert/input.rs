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
    Ok(out)
}

/// Convert FFI compile profile to internal compile profile.
pub fn ffi_profile_to_host(
    input: Option<FfiCompileProfile>,
) -> Result<host::CompileProfile, FfiConversionError> {
    let mut out = host::CompileProfile::default();
    if let Some(input) = input {
        out.filename = input.filename;
        if let Some(is_production) = input.is_production {
            out.is_production = is_production;
        }
        if let Some(custom_element) = input.custom_element {
            out.custom_element = custom_element;
        }
        if let Some(ssr) = input.ssr {
            out.ssr = ssr;
        }
        if let Some(hmr_strategy) = input.hmr_strategy {
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
        out.component_id = input.component_id;
        out.delimiters = if let Some(d) = input.delimiters {
            if d.len() != 2 {
                return Err(FfiConversionError::InvalidDelimiters(d.len()));
            }
            Some((d[0].clone(), d[1].clone()))
        } else {
            None
        };
        out.custom_elements = input.custom_elements;
        out.comments = input.comments;
        if let Some(runtime_module_name) = input.runtime_module_name {
            out.runtime_module_name = Some(runtime_module_name);
        }
        if let Some(types_module_name) = input.types_module_name {
            out.types_module_name = Some(types_module_name);
        }
        if let Some(force_vapor) = input.force_vapor {
            out.force_vapor = force_vapor;
        }
        if let Some(force_js) = input.force_js {
            out.force_js = force_js;
        }
        if let Some(source_map) = input.source_map {
            out.source_map = source_map;
        }
        if let Some(target) = input.target {
            out.target = ffi_target_to_compile_target(&target)?;
        }
        out.inline = input.inline;
        if let Some(strict_slots) = input.strict_slots {
            out.strict_slots = strict_slots;
        }
        if let Some(requested_mode) = input.requested_mode {
            out.requested_mode = ffi_compile_cache_mode_to_host(&requested_mode)?;
        }
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

/// Parse a block type string to the host `PreprocessorBlockType` enum.
pub(super) fn ffi_block_type_to_host(s: &str) -> host::PreprocessorBlockType {
    match s {
        "template" => host::PreprocessorBlockType::Template,
        "script" => host::PreprocessorBlockType::Script,
        "style" => host::PreprocessorBlockType::Style,
        _ => host::PreprocessorBlockType::Custom,
    }
}

/// Convert FFI block override request to host block override request.
pub fn ffi_block_override_to_host(
    input: FfiBlockOverrideRequest,
) -> Result<host::BlockOverrideRequest, FfiConversionError> {
    Ok(host::BlockOverrideRequest {
        canonical_id: input.canonical_id,
        compile_profile: ffi_profile_to_host(input.compile_profile)?,
        overrides: input
            .overrides
            .into_iter()
            .map(|entry| host::BlockOverrideEntry {
                block_type: ffi_block_type_to_host(&entry.block_type),
                index: entry.index as usize,
                code: Arc::from(entry.code),
                source_map: entry.source_map.map(Arc::from),
            })
            .collect(),
    })
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

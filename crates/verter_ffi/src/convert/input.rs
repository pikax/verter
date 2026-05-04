//! FFI → host input conversions: config, profile, file/node kinds, upserts,
//! block overrides, and virtual-query requests.

use std::sync::Arc;

use verter_session as host;

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
        if let Some(strict_slots) = input.strict_slots {
            out.strict_slots = strict_slots;
        }
    }
    Ok(out)
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

/// Parse a file kind string to the host enum.
pub fn ffi_file_kind_to_host(input: Option<&str>) -> Result<host::FileKind, FfiConversionError> {
    match input.unwrap_or("vue").to_ascii_lowercase().as_str() {
        "vue" | "sfc" | "vue_sfc" => Ok(host::FileKind::VueSfc),
        "non_sfc" | "text" | "file" => Ok(host::FileKind::NonSfc),
        other => Err(FfiConversionError::InvalidFileKind(other.to_string())),
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
    Ok(host::UpsertRequest {
        canonical_id: input.canonical_id,
        input_id: input.input_id,
        source: Arc::from(input.source),
        file_kind: ffi_file_kind_to_host(input.file_kind.as_deref())?,
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

//! Resolve-result construction and provider/source projection.
//!
//! Provider-graph/carrier projection is pure: no
//! `KernelAttempt`/`ResolverAttemptView` involvement.
//! `path_is_carrier`/`carrier_ide_provider_path`/`carrier_api_provider_path`
//! are already `pub fn` free functions on `verter_workspace::resolver`
//! (re-exported at the crate root) and are called directly because they
//! are registry-backed, dependency-neutral utilities with no resolver
//! instance state. `provider_id_for_source` is likewise pure
//! (never touches `projects`) and remains a bare
//! free function; only `source_id_from_provider_id` and the two
//! `build_*_resolve_result` functions need `projects: &[IdeProjectConfig]`
//! (via [`super::project_ownership_resolution::nearest_config_for_path`]).
//!
//! Workspace-engine test hooks remain outside this pure projection layer and
//! have no production semantic effect.

#![allow(dead_code)]

use crate::project_ownership_resolution::nearest_config_for_path;

/// Computes a provider ID from a source ID without project-instance state.
pub(crate) fn provider_id_for_source(source_id: &str) -> Option<String> {
    let normalized_source = verter_session_query::resolution::normalize_canonical_id(source_id);
    if verter_session_query::resolution::path_is_carrier(&normalized_source) {
        Some(format!(
            "{normalized_source}{}",
            verter_session_query::resolution::CARRIER_API_VIRTUAL_SUFFIX
        ))
    } else {
        Some(normalized_source)
    }
}

/// Computes the IDE-facing provider identity for a carrier source.
pub(crate) fn provider_ide_id_for_source(source_id: &str, is_jsx: bool) -> Option<String> {
    let normalized_source = verter_session_query::resolution::normalize_canonical_id(source_id);
    if !verter_session_query::resolution::path_is_carrier(&normalized_source) {
        return None;
    }
    Some(verter_session_query::resolution::carrier_ide_provider_path(
        &normalized_source,
        is_jsx,
    ))
}

/// Reverses a provider identity to a configured source identity.
pub(crate) fn source_id_from_provider_id(
    projects: &[verter_session_query::resolution::IdeProjectConfig],
    provider_id: &str,
) -> Option<String> {
    let normalized = verter_session_query::resolution::normalize_canonical_id(provider_id);

    if (normalized.ends_with(".tsx") || normalized.ends_with(".jsx"))
        && verter_session_query::resolution::path_is_carrier(&normalized[..normalized.len() - 4])
    {
        let candidate = &normalized[..normalized.len() - 4];
        if nearest_config_for_path(projects, candidate).is_some() {
            return Some(candidate.to_string());
        }
    }

    if let Some(candidate) =
        normalized.strip_suffix(verter_session_query::resolution::CARRIER_API_VIRTUAL_SUFFIX)
    {
        if verter_session_query::resolution::path_is_carrier(candidate)
            && nearest_config_for_path(projects, candidate).is_some()
        {
            return Some(candidate.to_string());
        }
    }

    if nearest_config_for_path(projects, &normalized).is_some() {
        return Some(normalized);
    }

    None
}

/// Mirrors `split_path_parts` exactly — pure, no I/O.
fn split_path_parts(path: &str) -> Vec<String> {
    verter_session_query::resolution::normalize_canonical_id(path)
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

/// Mirrors `relative_specifier` exactly — pure, no I/O.
fn relative_specifier(from_file: &str, to_file: &str) -> String {
    let from_dir = verter_session_query::resolution::parent_dir(from_file);
    let from_dir = verter_session_query::resolution::normalize_canonical_id(&from_dir);
    let to_file = verter_session_query::resolution::normalize_canonical_id(to_file);
    let from_parts = split_path_parts(&from_dir);
    let to_parts = split_path_parts(&to_file);

    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(left, right)| left == right)
        .count();
    let mut segments = Vec::new();
    for _ in common..from_parts.len() {
        segments.push("..".to_string());
    }
    for part in &to_parts[common..] {
        segments.push(part.clone());
    }

    match segments.as_slice() {
        [] => "./".to_string(),
        _ => {
            let joined = segments.join("/");
            if joined.starts_with("../") || joined == ".." {
                joined
            } else {
                format!("./{joined}")
            }
        }
    }
}

fn provider_id_for_source_with_memo(
    source_id: &str,
    memo: &crate::resolve_frame::ResolutionStringMemo,
) -> String {
    let normalized = memo.normalize(source_id);
    if verter_session_query::resolution::path_is_carrier(&normalized) {
        format!(
            "{normalized}{}",
            verter_session_query::resolution::CARRIER_API_VIRTUAL_SUFFIX
        )
    } else {
        normalized.to_string()
    }
}

fn relative_specifier_with_memo(
    from_file: &str,
    to_file: &str,
    memo: &crate::resolve_frame::ResolutionStringMemo,
) -> String {
    let from_dir = memo.parent(from_file);
    let from_dir = memo.normalize(&from_dir);
    let to_file = memo.normalize(to_file);
    let from_parts = from_dir
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let to_parts = to_file
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let common = from_parts
        .iter()
        .zip(&to_parts)
        .take_while(|(left, right)| left == right)
        .count();
    let mut segments = vec![".."; from_parts.len() - common];
    segments.extend(to_parts[common..].iter().copied());
    if segments.is_empty() {
        "./".to_string()
    } else {
        let joined = segments.join("/");
        if joined.starts_with("../") || joined == ".." {
            joined
        } else {
            format!("./{joined}")
        }
    }
}

pub(crate) fn build_resolve_result_with_memo(
    projects: &[verter_session_query::resolution::IdeProjectConfig],
    importer_id: &str,
    specifier: &str,
    source_id: String,
    resolution_kind: verter_session_query::resolution::ResolutionKind,
    memo: &crate::resolve_frame::ResolutionStringMemo,
) -> verter_session_query::resolution::ResolveResult {
    let target_owner = crate::project_ownership_resolution::nearest_config_for_path_with_memo(
        projects, &source_id, memo,
    );
    let provider_id = target_owner
        .map(|_| provider_id_for_source_with_memo(&source_id, memo))
        .unwrap_or_else(|| source_id.clone());
    let provider_target = match target_owner {
        Some(_)
            if verter_session_query::resolution::path_is_carrier(&memo.normalize(&source_id)) =>
        {
            verter_session_query::resolution::ProviderTarget::CarrierPublicApi
        }
        Some(_) => verter_session_query::resolution::ProviderTarget::ShadowSourceFile,
        None => verter_session_query::resolution::ProviderTarget::SourceFile,
    };
    let provider_specifier = if target_owner.is_some()
        && provider_target == verter_session_query::resolution::ProviderTarget::CarrierPublicApi
    {
        let importer_provider_id = provider_id_for_source_with_memo(importer_id, memo);
        relative_specifier_with_memo(&importer_provider_id, &provider_id, memo)
    } else {
        specifier.to_string()
    };
    verter_session_query::resolution::ResolveResult {
        owner_tsconfig_path: target_owner.and_then(|project| project.tsconfig_path.clone()),
        source_id,
        provider_id,
        provider_specifier,
        provider_target,
        resolution_kind,
    }
}

pub(crate) fn build_project_resolve_result_with_memo(
    projects: &[verter_session_query::resolution::IdeProjectConfig],
    specifier: &str,
    source_id: String,
    resolution_kind: verter_session_query::resolution::ResolutionKind,
    memo: &crate::resolve_frame::ResolutionStringMemo,
) -> verter_session_query::resolution::ResolveResult {
    let target_owner = crate::project_ownership_resolution::nearest_config_for_path_with_memo(
        projects, &source_id, memo,
    );
    let provider_id = target_owner
        .map(|_| provider_id_for_source_with_memo(&source_id, memo))
        .unwrap_or_else(|| source_id.clone());
    let provider_target = match target_owner {
        Some(_)
            if verter_session_query::resolution::path_is_carrier(&memo.normalize(&source_id)) =>
        {
            verter_session_query::resolution::ProviderTarget::CarrierPublicApi
        }
        Some(_) => verter_session_query::resolution::ProviderTarget::ShadowSourceFile,
        None => verter_session_query::resolution::ProviderTarget::SourceFile,
    };
    verter_session_query::resolution::ResolveResult {
        owner_tsconfig_path: target_owner.and_then(|project| project.tsconfig_path.clone()),
        source_id,
        provider_id,
        provider_specifier: specifier.to_string(),
        provider_target,
        resolution_kind,
    }
}

/// Looks up `nearest_config_for_path` on the TARGET (not importer) for correct
/// `provider_id`/`provider_specifier`/`provider_target`/
/// `owner_tsconfig_path`.
pub(crate) fn build_resolve_result(
    projects: &[verter_session_query::resolution::IdeProjectConfig],
    request: &verter_session_query::resolution::ResolveRequest,
    source_id: String,
    resolution_kind: verter_session_query::resolution::ResolutionKind,
) -> verter_session_query::resolution::ResolveResult {
    let target_owner = nearest_config_for_path(projects, &source_id);
    let provider_id = target_owner
        .and_then(|_| provider_id_for_source(&source_id))
        .unwrap_or_else(|| source_id.clone());
    let provider_target = match target_owner {
        Some(_)
            if verter_session_query::resolution::path_is_carrier(
                &verter_session_query::resolution::normalize_canonical_id(&source_id),
            ) =>
        {
            verter_session_query::resolution::ProviderTarget::CarrierPublicApi
        }
        Some(_) => verter_session_query::resolution::ProviderTarget::ShadowSourceFile,
        None => verter_session_query::resolution::ProviderTarget::SourceFile,
    };
    let provider_specifier = if target_owner.is_some() {
        match provider_target {
            verter_session_query::resolution::ProviderTarget::CarrierPublicApi => {
                let importer_provider_id = provider_id_for_source(&request.importer_id)
                    .unwrap_or_else(|| {
                        verter_session_query::resolution::normalize_canonical_id(
                            &request.importer_id,
                        )
                    });
                relative_specifier(&importer_provider_id, &provider_id)
            }
            verter_session_query::resolution::ProviderTarget::ShadowSourceFile
            | verter_session_query::resolution::ProviderTarget::SourceFile => {
                request.specifier.clone()
            }
        }
    } else {
        request.specifier.clone()
    };

    verter_session_query::resolution::ResolveResult {
        owner_tsconfig_path: target_owner.and_then(|project| project.tsconfig_path.clone()),
        source_id,
        provider_id,
        provider_specifier,
        provider_target,
        resolution_kind,
    }
}

/// Builds a resolve result for an explicitly selected project request.
pub(crate) fn build_project_resolve_result(
    projects: &[verter_session_query::resolution::IdeProjectConfig],
    specifier: &str,
    source_id: String,
    resolution_kind: verter_session_query::resolution::ResolutionKind,
) -> verter_session_query::resolution::ResolveResult {
    let target_owner = nearest_config_for_path(projects, &source_id);
    let provider_id = target_owner
        .and_then(|_| provider_id_for_source(&source_id))
        .unwrap_or_else(|| source_id.clone());
    let provider_target = match target_owner {
        Some(_)
            if verter_session_query::resolution::path_is_carrier(
                &verter_session_query::resolution::normalize_canonical_id(&source_id),
            ) =>
        {
            verter_session_query::resolution::ProviderTarget::CarrierPublicApi
        }
        Some(_) => verter_session_query::resolution::ProviderTarget::ShadowSourceFile,
        None => verter_session_query::resolution::ProviderTarget::SourceFile,
    };

    verter_session_query::resolution::ResolveResult {
        owner_tsconfig_path: target_owner.and_then(|project| project.tsconfig_path.clone()),
        source_id,
        provider_id,
        provider_specifier: specifier.to_string(),
        provider_target,
        resolution_kind,
    }
}

/// Thin `build_resolve_result` wrapper that always tags the result
/// `ResolutionKind::Bundler` — used by callers that already know the
/// exact resolved `source_id` (bundler-driven exact resolution) and
/// only need the provider-graph projection recomputed.
pub(crate) fn project_exact_result(
    projects: &[verter_session_query::resolution::IdeProjectConfig],
    importer_id: &str,
    specifier: &str,
    source_id: String,
    context: verter_session_query::resolution::ResolutionContext,
) -> verter_session_query::resolution::ResolveResult {
    build_resolve_result(
        projects,
        &verter_session_query::resolution::ResolveRequest {
            importer_id: importer_id.to_owned(),
            specifier: specifier.to_owned(),
            kind: context.kind,
            phase: context.phase,
        },
        source_id,
        verter_session_query::resolution::ResolutionKind::Bundler,
    )
}

#[cfg(test)]
#[path = "provider_projection_resolution_tests.rs"]
mod provider_projection_resolution_tests;

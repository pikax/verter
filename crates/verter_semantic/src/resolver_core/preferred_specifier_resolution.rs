//! Preferred-specifier candidate generation and reverse tsconfig-path
//! projection.
//!
//! `preferred_specifier_candidates` and its private helper
//! `reverse_tsconfig_path` are pure: they perform no I/O and emit no
//! observation witness. Round-trip verification and shortest-choice
//! orchestration belong to the driver above this kernel.

#![allow(dead_code)]

use crate::resolver_core::project_ownership_resolution::nearest_config_for_path;

/// Mirrors `reverse_tsconfig_path` exactly — pure string logic, no I/O.
fn reverse_tsconfig_path(
    base_url: &str,
    pattern: &str,
    target_template: &str,
    target_id: &str,
) -> Option<String> {
    let (target_prefix, target_suffix) = if let Some(star) = target_template.find('*') {
        let prefix_part = &target_template[..star];
        let suffix_part = &target_template[star + 1..];
        let mut prefix = if crate::resolver_core::is_absolute_specifier(prefix_part) {
            crate::resolver_core::normalize_canonical_id(prefix_part)
        } else {
            crate::resolver_core::join_paths(base_url, prefix_part)
        };
        if prefix_part.ends_with('/') && !prefix.ends_with('/') {
            prefix.push('/');
        }
        (prefix, suffix_part.to_string())
    } else {
        let abs = if crate::resolver_core::is_absolute_specifier(target_template) {
            crate::resolver_core::normalize_canonical_id(target_template)
        } else {
            crate::resolver_core::join_paths(base_url, target_template)
        };
        return if crate::resolver_core::normalize_canonical_id(target_id) == abs {
            Some(pattern.to_string())
        } else {
            None
        };
    };

    let normalized_target = crate::resolver_core::normalize_canonical_id(target_id);
    if !normalized_target.starts_with(&target_prefix) {
        return None;
    }
    if !target_suffix.is_empty() && !normalized_target.ends_with(&target_suffix) {
        return None;
    }
    let captured_end = normalized_target.len().saturating_sub(target_suffix.len());
    if target_prefix.len() > captured_end {
        return None;
    }
    let captured = &normalized_target[target_prefix.len()..captured_end];

    if let Some(star) = pattern.find('*') {
        Some(format!(
            "{}{}{}",
            &pattern[..star],
            captured,
            &pattern[star + 1..]
        ))
    } else {
        Some(pattern.to_string())
    }
}

/// Returns every tsconfig-`paths` reverse match plus every workspace-alias
/// reverse match, unordered/unverified — the caller (Engine's
/// round-trip-verify-and-pick-shortest orchestration) is responsible
/// for filtering to the ones that actually round-trip and picking the
/// shortest.
pub(crate) fn preferred_specifier_candidates(
    projects: &[crate::resolver_core::IdeProjectConfig],
    importer_id: &str,
    target_id: &str,
) -> Option<Vec<String>> {
    let owner = nearest_config_for_path(projects, importer_id)?;
    let normalized_target = crate::resolver_core::normalize_canonical_id(target_id);
    let mut candidates: Vec<String> = Vec::new();

    let base_url = owner
        .compiler_options
        .base_url
        .as_deref()
        .unwrap_or(owner.root.as_str());

    for (pattern, targets) in &owner.compiler_options.paths {
        for target_template in targets {
            if let Some(specifier) =
                reverse_tsconfig_path(base_url, pattern, target_template, &normalized_target)
            {
                candidates.push(specifier);
            }
        }
    }

    for alias in &owner.workspace_aliases {
        let mut replacement = crate::resolver_core::normalize_canonical_id(&alias.replacement);
        if !replacement.ends_with('/') {
            replacement.push('/');
        }
        if let Some(remainder) = normalized_target.strip_prefix(replacement.as_str()) {
            let specifier = format!("{}{}", alias.find, remainder);
            candidates.push(specifier);
        }
    }

    Some(candidates)
}

#[cfg(test)]
#[path = "preferred_specifier_resolution_tests.rs"]
mod preferred_specifier_resolution_tests;

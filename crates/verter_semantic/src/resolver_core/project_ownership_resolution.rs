//! Project ownership selection and ordering.
//!
//! Owner/project selection is pure: no I/O and no
//! `KernelAttempt`/`ResolverAttemptView` involvement
//! at all: `IdeProjectConfig::matches_file` delegates to
//! `ConfiguredMembership::contains`, a completed-value in-memory glob
//! match.
//! These are plain functions over `projects: &[IdeProjectConfig]`, not
//! part of the `KernelAttempt` observation seam.
//!
//! `projects` is assumed PRE-SORTED by [`compare_projects`]'s precedence
//! order (nearest-root-first), the invariant established by
//! [`ModuleResolverCore`](crate::resolver_core::ModuleResolverCore)
//! construction. This module does not itself sort on
//! every call; [`sorted_by_precedence`] is provided for a constructor
//! (or a test) to establish that invariant once.

#![allow(dead_code)]

use crate::resolver_core::source_id_resolution::normalized_starts_with;

/// Mirrors `project_rank` exactly — pure, no I/O.
fn project_rank(project: &crate::resolver_core::IdeProjectConfig) -> u8 {
    match project.tsconfig_path.as_deref() {
        Some(path)
            if crate::resolver_core::normalize_canonical_id(path).ends_with("/tsconfig.json") =>
        {
            0
        }
        Some(_) => 1,
        None => 2,
    }
}

/// Mirrors `compare_projects` exactly — pure, no I/O.
pub(crate) fn compare_projects(
    a: &crate::resolver_core::IdeProjectConfig,
    b: &crate::resolver_core::IdeProjectConfig,
) -> std::cmp::Ordering {
    crate::resolver_core::normalize_canonical_id(&b.root)
        .len()
        .cmp(&crate::resolver_core::normalize_canonical_id(&a.root).len())
        .then_with(|| project_rank(a).cmp(&project_rank(b)))
        .then_with(|| a.tsconfig_path.cmp(&b.tsconfig_path))
        .then_with(|| a.root.cmp(&b.root))
}

/// Establishes the precedence-order invariant every resolver function below
/// assumes and centralizes construction of ordered project vectors.
pub(crate) fn sorted_by_precedence(
    mut projects: Vec<crate::resolver_core::IdeProjectConfig>,
) -> Vec<crate::resolver_core::IdeProjectConfig> {
    projects.sort_by(compare_projects);
    projects
}

/// Returns every configured project claiming `file_id`, nearest-root-pruned (a strict
/// ancestor root loses to a deeper co-claiming root); when no configured
/// project claims it, the matching tsconfig-less fallback configs.
pub(crate) fn effective_configs_for_path<'a>(
    projects: &'a [crate::resolver_core::IdeProjectConfig],
    file_id: &str,
) -> Vec<&'a crate::resolver_core::IdeProjectConfig> {
    let configured: Vec<&crate::resolver_core::IdeProjectConfig> = projects
        .iter()
        .filter(|project| project.tsconfig_path.is_some() && project.matches_file(file_id))
        .collect();

    if !configured.is_empty() {
        return configured
            .iter()
            .copied()
            .filter(|candidate| {
                let candidate_root = crate::resolver_core::normalize_canonical_id(&candidate.root);
                !configured.iter().any(|other| {
                    if std::ptr::eq(*other, *candidate) {
                        return false;
                    }
                    let other_root = crate::resolver_core::normalize_canonical_id(&other.root);
                    other_root.len() > candidate_root.len()
                        && normalized_starts_with(&other_root, &candidate_root)
                })
            })
            .collect();
    }

    projects
        .iter()
        .filter(|project| project.tsconfig_path.is_none() && project.matches_file(file_id))
        .collect()
}

/// Returns the first effective configuration in precedence order.
pub(crate) fn nearest_config_for_path<'a>(
    projects: &'a [crate::resolver_core::IdeProjectConfig],
    file_id: &str,
) -> Option<&'a crate::resolver_core::IdeProjectConfig> {
    effective_configs_for_path(projects, file_id)
        .into_iter()
        .next()
}

pub(crate) fn nearest_config_for_path_with_memo<'a>(
    projects: &'a [crate::resolver_core::IdeProjectConfig],
    file_id: &str,
    memo: &crate::resolver_core::resolve_frame::ResolutionStringMemo,
) -> Option<&'a crate::resolver_core::IdeProjectConfig> {
    let configured = projects
        .iter()
        .filter(|project| project.tsconfig_path.is_some() && project.matches_file(file_id))
        .collect::<Vec<_>>();
    if !configured.is_empty() {
        return configured.iter().copied().find(|candidate| {
            let candidate_root = memo.normalize(&candidate.root);
            !configured.iter().any(|other| {
                if std::ptr::eq(*other, *candidate) {
                    return false;
                }
                let other_root = memo.normalize(&other.root);
                other_root.len() > candidate_root.len()
                    && other_root.starts_with(candidate_root.as_ref())
                    && (other_root.len() == candidate_root.len()
                        || candidate_root.ends_with('/')
                        || other_root.as_bytes().get(candidate_root.len()) == Some(&b'/'))
            })
        });
    }
    projects
        .iter()
        .find(|project| project.tsconfig_path.is_none() && project.matches_file(file_id))
}

/// Requires an exact `(root, tsconfig_path)` match, refusing (returning `None`) on a
/// genuine duplicate rather than picking either.
pub(crate) fn project_for_ownership<'a>(
    projects: &'a [crate::resolver_core::IdeProjectConfig],
    owner: &crate::resolver_core::ProjectOwnership,
) -> Option<&'a crate::resolver_core::IdeProjectConfig> {
    let normalized_root = crate::resolver_core::normalize_canonical_id(&owner.project_root);
    let normalized_tsconfig = owner
        .tsconfig_path
        .as_ref()
        .map(|path| crate::resolver_core::normalize_canonical_id(path));
    let mut matched: Option<&crate::resolver_core::IdeProjectConfig> = None;

    for project in projects {
        if crate::resolver_core::normalize_canonical_id(&project.root) != normalized_root {
            continue;
        }
        let project_tsconfig = project
            .tsconfig_path
            .as_ref()
            .map(|path| crate::resolver_core::normalize_canonical_id(path));
        if project_tsconfig != normalized_tsconfig {
            continue;
        }
        if matched.is_some() {
            return None;
        }
        matched = Some(project);
    }

    matched
}

#[cfg(test)]
#[path = "project_ownership_resolution_tests.rs"]
mod project_ownership_resolution_tests;

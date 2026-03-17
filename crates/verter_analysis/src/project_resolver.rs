//! Project resolver — re-exported from `verter_vfs::resolver`.
//!
//! All resolver types and the
//! `ProjectResolver` (aliased as `NativeProjectResolver`) now live in
//! `verter_vfs`. This module re-exports them for backward compatibility
//! and adds analysis-dependent utility functions that reference
//! `crate::AnalyzedModuleReference`.

use std::collections::HashSet;

// ── Re-exports from verter_vfs ──
//
// Everything that was originally defined here is now in verter_vfs::resolver
// and verter_vfs::types.  Downstream code (`verter_host`, `verter_lsp`,
// `verter_napi`, `verter_wasm`) can keep importing via
// `verter_analysis::project_resolver::*` unchanged.

pub use verter_vfs::resolver::{
    build_known_file_index, collapse_path, is_absolute_specifier, is_relative_specifier,
    join_paths, normalize_canonical_id, normalize_known_file_id, parent_dir,
    resolve_known_dependency_base, resolve_known_dependency_id, IdeProjectCompilerOptions,
    IdeProjectConfig, NativeProjectResolver, ProjectMembership, ProjectResolver, WorkspaceAlias,
};
pub use verter_vfs::types::{
    ProviderTarget, ResolutionContext, ResolutionKind, ResolvePhase, ResolveRequest,
    ResolveRequestKind, ResolveResult,
};

// ── Analysis-dependent functions ──
//
// These functions reference `crate::AnalyzedModuleReference` and so must
// stay in `verter_analysis`.  They delegate to the verter_vfs path helpers.

pub fn collect_resolvable_module_reference_specifiers(
    module_references: &[crate::AnalyzedModuleReference],
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut specifiers = Vec::new();

    for reference in module_references {
        let candidates: &[String] = match reference.analyzability {
            crate::ModuleReferenceAnalyzability::Exact => {
                if let Some(specifier) = reference.literal_specifier.as_ref() {
                    if !specifier.is_empty() && seen.insert(specifier.clone()) {
                        specifiers.push(specifier.clone());
                    }
                }
                continue;
            }
            crate::ModuleReferenceAnalyzability::FiniteSet => &reference.finite_specifiers,
            crate::ModuleReferenceAnalyzability::UnknownDynamic => continue,
        };

        for specifier in candidates {
            if specifier.is_empty() || !seen.insert(specifier.clone()) {
                continue;
            }
            specifiers.push(specifier.clone());
        }
    }

    specifiers
}

pub fn resolve_known_module_reference_dependencies(
    owner_id: &str,
    module_references: &[crate::AnalyzedModuleReference],
    known_ids: &[String],
    extensions: &[String],
) -> Vec<String> {
    let known_index = build_known_file_index(known_ids);
    let owner_id = normalize_known_file_id(owner_id);
    let mut resolved = Vec::new();
    let mut seen = HashSet::new();

    for specifier in collect_resolvable_module_reference_specifiers(module_references) {
        let Some(match_id) =
            resolve_known_dependency_id(owner_id.as_str(), &specifier, &known_index, extensions)
        else {
            continue;
        };
        let match_id_normalized = normalize_known_file_id(&match_id);
        if match_id_normalized == owner_id || !seen.insert(match_id_normalized) {
            continue;
        }
        resolved.push(match_id);
    }

    resolved
}

#[cfg(test)]
#[path = "project_resolver_tests.rs"]
mod project_resolver_tests;

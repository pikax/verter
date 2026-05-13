//! Dependency tracking helpers.
//!
//! Contains the logic for determining whether an import resolves to a given
//! dependency and stripping configured extensions for extensionless
//! matching. Per R3, downstream caches revalidate lazily through their
//! own `fact_dep_signature` checks; this module no longer carries
//! change-detection helpers for eager invalidation. The reverse-dep
//! graph is content-addressed for memory-bound GC and affected-files
//! surfacing only (R22).
#![allow(dead_code)]

use std::collections::BTreeSet;

use crate::id;
use crate::types::*;

/// Strip a configured extension from a path, returning the stem.
/// Used to match extensionless import specifiers (e.g. `./types` → `/src/types`)
/// against canonical IDs that include extensions (e.g. `/src/types.ts`).
///
/// Extensions are tried in the given order. When `script_lang` is set (from the
/// SFC's `<script lang="...">` attribute), matching extensions are tried first.
/// For example, `script_lang = "ts"` prioritises `.ts`/`.tsx` over `.js`/`.jsx`.
pub(crate) fn strip_configured_extension<'a>(
    path: &'a str,
    resolve_extensions: &[String],
    script_lang: Option<&str>,
) -> Option<&'a str> {
    // If a script lang is specified, try extensions matching that lang first
    if let Some(lang) = script_lang {
        let prefix = format!(".{}", lang);
        for ext in resolve_extensions {
            if ext.starts_with(&prefix) {
                if let Some(stem) = path.strip_suffix(ext.as_str()) {
                    return Some(stem);
                }
            }
        }
    }
    // Then try all configured extensions
    for ext in resolve_extensions {
        if let Some(stem) = path.strip_suffix(ext.as_str()) {
            return Some(stem);
        }
    }
    None
}

/// Lightweight view of a dependent file's data needed for invalidation.
///
/// Populated from scheduler `HostAnalysisData` + `CompileCacheEntry`.
pub(crate) struct DependentView {
    pub(crate) canonical_id: String,
    pub(crate) import_routes: rustc_hash::FxHashMap<String, DependencyResolution>,
    pub(crate) dependencies: BTreeSet<String>,
    pub(crate) script_lang: Option<String>,
    pub(crate) macro_type_deps: Vec<verter_semantic::analysis::MacroTypeDep>,
    pub(crate) imports: Vec<verter_semantic::analysis::AnalyzedImport>,
    pub(crate) resolved_type_hashes: rustc_hash::FxHashMap<(String, String), Hash16>,
}

/// Check if an import source from `view` resolves to `dependency_id`.
/// Handles both relative paths (resolved via resolve_external) and
/// non-relative paths (matched via the file's registered dependencies).
///
/// Checks structured `import_routes` first for exact matches,
/// then falls back to heuristic resolution.
fn import_resolves_to_dep_view(
    view: &DependentView,
    import_source: &str,
    dependency_id: &str,
    resolve_extensions: &[String],
) -> bool {
    if let Some(resolution) = view.import_routes.get(import_source) {
        // Use effective_target() for TS-first single-candidate selection.
        // This matches only against the highest-priority candidate, not all possibles.
        if let Some(target) = resolution.effective_target() {
            return target == dependency_id;
        }
    }

    if import_source.starts_with('.') {
        let resolved = id::resolve_external(&view.canonical_id, import_source);
        if resolved == dependency_id {
            return true;
        }
        if let Some(stem) = strip_configured_extension(
            dependency_id,
            resolve_extensions,
            view.script_lang.as_deref(),
        ) {
            return resolved == stem;
        }
        false
    } else {
        view.dependencies.contains(dependency_id)
    }
}

/// Check if an import source from `view` resolves to `dependency_id`.
/// Test-only wrapper that exposes the private `import_resolves_to_dep_view`.
#[cfg(test)]
pub(crate) fn import_resolves_to_dep(
    view: &DependentView,
    import_source: &str,
    dependency_id: &str,
    resolve_extensions: &[String],
) -> bool {
    import_resolves_to_dep_view(view, import_source, dependency_id, resolve_extensions)
}

// `should_invalidate_dependent_view` retired with the R3 cross-file
// invalidation cutover. Downstream caches revalidate lazily through
// their own `fact_dep_signature` checks on read; the dependent-SFC
// invalidation predicate is no longer needed. The path-resolution
// helpers above (`import_resolves_to_dep`) survive as they remain
// useful for LSP affected-files surfacing via the R22 reverse-dep
// graph.

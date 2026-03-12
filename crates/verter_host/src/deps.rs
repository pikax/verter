//! Dependency tracking and smart invalidation.
//!
//! Contains the logic for determining whether an import resolves to a given
//! dependency, stripping configured extensions for extensionless matching,
//! and the tiered invalidation decision (`should_invalidate_dependent`).

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::id;
use crate::shared::{read_lock, write_lock};
use crate::types::*;
use crate::upsert::compute_changed_exports;

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

/// Check if an import source from `file` resolves to `dependency_id`.
/// Handles both relative paths (resolved via resolve_external) and
/// non-relative paths (matched via the file's registered dependencies).
///
/// Checks structured `dependency_resolutions` first for exact matches,
/// then falls back to heuristic resolution.
pub(crate) fn import_resolves_to_dep(
    file: &FileEntry,
    import_source: &str,
    dependency_id: &str,
    resolve_extensions: &[String],
) -> bool {
    // Check structured resolution records first (exact match).
    if let Some(resolution) = file.dependency_resolutions.get(import_source) {
        if let Some(ref resolved_id) = resolution.resolved_canonical_id {
            return resolved_id == dependency_id;
        }
        if resolution
            .possible_canonical_ids
            .iter()
            .any(|c| c == dependency_id)
        {
            return true;
        }
    }

    if import_source.starts_with('.') {
        let resolved = id::resolve_external(&file.canonical_id, import_source);
        if resolved == dependency_id {
            return true;
        }
        // Handle extensionless imports: `import './types'` resolves to `/src/types`,
        // but dep canonical_id might be `/src/types.ts`. Use the SFC's script lang
        // to prioritise matching extensions.
        if let Some(stem) = strip_configured_extension(
            dependency_id,
            resolve_extensions,
            file.meta.script_lang.as_deref(),
        ) {
            return resolved == stem;
        }
        false
    } else {
        // Non-relative import: it resolves to this dep if the dep is in the file's
        // dependency set (registered via set_import_dependencies or auto-discovered)
        file.dependencies.contains(dependency_id)
    }
}

/// Determine whether a dependent SFC should be invalidated given
/// which exports changed in a dependency.
///
/// When `dep_source` is available, Tier 3 resolution is attempted: the type
/// is resolved from the dep file and hashed. If the resolved shape is unchanged,
/// invalidation is skipped even though the export text changed.
pub(crate) fn should_invalidate_dependent(
    file: &mut FileEntry,
    dependency_id: &str,
    changed_exports: &BTreeSet<String>,
    no_signatures: bool,
    dep_source: Option<&str>,
    resolve_extensions: &[String],
) -> bool {
    // If no export signatures available (Tier 1 fallback), always invalidate
    if no_signatures {
        return true;
    }

    // If no exports changed, no invalidation needed
    if changed_exports.is_empty() {
        return false;
    }

    // Check if the dependent has macro type deps on this dependency
    let macro_type_deps: Vec<&verter_analysis::MacroTypeDep> = file
        .script_analysis
        .macro_type_deps
        .iter()
        .filter(|dep| {
            import_resolves_to_dep(file, &dep.import_source, dependency_id, resolve_extensions)
        })
        .collect();

    if !macro_type_deps.is_empty() {
        // Collect type names that Tier 2 considers changed
        let tier2_changed: Vec<&str> = macro_type_deps
            .iter()
            .filter(|dep| changed_exports.contains(&dep.type_name))
            .map(|dep| dep.type_name.as_str())
            .collect();

        if tier2_changed.is_empty() {
            // Tier 2: no macro-consumed types in changed exports
            return false;
        }

        // Tier 3: try cross-file type resolution if dep source is available
        if let Some(dep_src) = dep_source {
            let alloc = oxc_allocator::Allocator::new();
            let mut any_shape_changed = false;

            for type_name in &tier2_changed {
                let key = (dependency_id.to_string(), type_name.to_string());

                if let Some(resolved) =
                    verter_core::utils::oxc::vue::resolve_type::resolve_external_type(
                        type_name, dep_src, &alloc,
                    )
                {
                    let new_hash = verter_core::utils::oxc::vue::resolve_type::hash_resolved_type(
                        &resolved,
                        dep_src.as_bytes(),
                    );

                    if let Some(old_hash) = file.resolved_type_hashes.get(&key) {
                        if *old_hash == new_hash {
                            // Tier 3: resolved shape unchanged — skip this type
                            file.resolved_type_hashes.insert(key, new_hash);
                            continue;
                        }
                    }

                    // Shape changed (or first time seeing this type)
                    file.resolved_type_hashes.insert(key, new_hash);
                    any_shape_changed = true;
                } else {
                    // Can't resolve type — fall back to Tier 2 (assume changed)
                    any_shape_changed = true;
                }
            }

            return any_shape_changed;
        }

        // No dep source available — Tier 2: invalidate
        return true;
    }

    // Check if the dependent has any runtime (non-type-only) imports from this dep
    let has_runtime_import = file.script_analysis.imports.iter().any(|imp| {
        !imp.is_type_only
            && import_resolves_to_dep(file, &imp.source, dependency_id, resolve_extensions)
    });

    if has_runtime_import {
        // Runtime imports: conservatively invalidate on any export change
        return true;
    }

    // If the file has this dependency registered but no matching imports in analysis
    // (e.g., src block dependency, or external deps without script imports),
    // conservatively invalidate.
    if file.dependencies.contains(dependency_id)
        && file.script_analysis.imports.iter().all(|imp| {
            !import_resolves_to_dep(file, &imp.source, dependency_id, resolve_extensions)
        })
    {
        return true;
    }

    // Type-only imports not used by macros: no invalidation needed
    false
}

/// Smart invalidation: when a dependency changes, only invalidate dependent
/// SFCs whose macro-consumed types were actually affected.
///
/// Uses tiered precision:
/// - Tier 1: No export signatures → full invalidation
/// - Tier 2: Export-level hashing → invalidate only if macro-consumed exports changed
/// - Tier 3: Cross-file type resolution → invalidate only if resolved type shape changed
pub(crate) fn smart_invalidate_dependents(
    files: &crate::shared::Shared<rustc_hash::FxHashMap<String, FileEntry>>,
    reverse_dependencies: &crate::shared::Shared<rustc_hash::FxHashMap<String, BTreeSet<String>>>,
    config: &HostConfig,
    dependency_id: &str,
    old_export_signatures: &[verter_analysis::ExportSignature],
    new_export_signatures: &[verter_analysis::ExportSignature],
) {
    let resolve_extensions = &config.resolve_extensions;

    let owners = {
        let rev = read_lock(reverse_dependencies);
        let mut owners = rev.get(dependency_id).cloned().unwrap_or_default();
        // Also check extensionless variant: auto-discovered relative imports
        // (e.g. `import './types'`) resolve to `/src/types` (no extension),
        // so reverse_dependencies is keyed by `/src/types` while the dep's
        // canonical_id is `/src/types.ts`. Use config extensions + no specific
        // lang hint at this level (per-file lang is checked in import_resolves_to_dep).
        if let Some(stem) = strip_configured_extension(dependency_id, resolve_extensions, None) {
            if let Some(more) = rev.get(stem) {
                for o in more {
                    owners.insert(o.clone());
                }
            }
        }
        owners
    };

    if owners.is_empty() {
        return;
    }

    // Compute which export names changed between old and new signatures
    let changed_exports = compute_changed_exports(old_export_signatures, new_export_signatures);

    let mut files = write_lock(files);

    // Get dep source for Tier 3 resolution (clone Arc to avoid borrow conflict)
    let dep_source = files.get(dependency_id).map(|f| Arc::clone(&f.source));

    for owner in owners {
        if let Some(file) = files.get_mut(&owner) {
            if should_invalidate_dependent(
                file,
                dependency_id,
                &changed_exports,
                old_export_signatures.is_empty() && new_export_signatures.is_empty(),
                dep_source.as_deref(),
                resolve_extensions,
            ) {
                file.compile_slots.clear();
            }
        }
    }
}

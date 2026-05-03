//! Dependency tracking and smart invalidation.
//!
//! Contains the logic for determining whether an import resolves to a given
//! dependency, stripping configured extensions for extensionless matching,
//! and the tiered invalidation decision (`should_invalidate_dependent`).

use std::collections::BTreeSet;

use verter_semantic::analysis::project_resolver::{ResolvePhase, ResolveRequestKind};

use crate::id;
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

fn import_resolves_to_dep_with_resolver_data(
    canonical_id: &str,
    import_routes: &mut rustc_hash::FxHashMap<String, DependencyResolution>,
    dependencies: &mut BTreeSet<String>,
    script_lang: Option<&str>,
    import_source: &str,
    dependency_id: &str,
    resolve_extensions: &[String],
    workspace: Option<&dyn verter_workspace::WorkspaceAccess>,
) -> bool {
    let cached_view = DependentView {
        canonical_id: canonical_id.to_string(),
        import_routes: import_routes.clone(),
        dependencies: dependencies.clone(),
        script_lang: script_lang.map(str::to_string),
        macro_type_deps: Vec::new(),
        imports: Vec::new(),
        resolved_type_hashes: rustc_hash::FxHashMap::default(),
    };
    if import_resolves_to_dep_view(
        &cached_view,
        import_source,
        dependency_id,
        resolve_extensions,
    ) {
        return true;
    }

    if let Some(ws) = workspace {
        let ctx = verter_workspace::ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        };
        if let Some(result) = ws.resolve_import(canonical_id, import_source, ctx) {
            dependencies.insert(result.source_id.clone());
            import_routes.insert(
                import_source.to_string(),
                DependencyResolution {
                    specifier: import_source.to_string(),
                    resolved_canonical_id: Some(result.source_id.clone()),
                    possible_canonical_ids: vec![result.source_id.clone()],
                },
            );
            return result.source_id == dependency_id;
        }
    }

    false
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

fn import_resolves_to_dep_with_resolver_view(
    view: &mut DependentView,
    import_source: &str,
    dependency_id: &str,
    resolve_extensions: &[String],
    workspace: Option<&dyn verter_workspace::WorkspaceAccess>,
) -> bool {
    import_resolves_to_dep_with_resolver_data(
        &view.canonical_id,
        &mut view.import_routes,
        &mut view.dependencies,
        view.script_lang.as_deref(),
        import_source,
        dependency_id,
        resolve_extensions,
        workspace,
    )
}

/// Determine whether a dependent SFC should be invalidated given
/// which exports changed in a dependency.
///
/// When `dep_source` is available, Tier 3 resolution is attempted: the type
/// is resolved from the dep file and hashed. If the resolved shape is unchanged,
/// invalidation is skipped even though the export text changed.
///
/// Returns `(should_invalidate, updated_resolved_type_hashes)`. The caller must
/// write back the updated hashes to `CompileCacheEntry`.
pub(crate) fn should_invalidate_dependent_view(
    view: &mut DependentView,
    dependency_id: &str,
    changed_exports: &BTreeSet<String>,
    no_signatures: bool,
    dep_source: Option<&str>,
    resolve_extensions: &[String],
    workspace: Option<&dyn verter_workspace::WorkspaceAccess>,
) -> bool {
    if no_signatures {
        return true;
    }

    if changed_exports.is_empty() {
        return false;
    }

    let mut macro_type_deps = Vec::new();
    let macro_import_sources: Vec<_> = view
        .macro_type_deps
        .iter()
        .map(|dep| dep.import_source.clone())
        .collect();
    for (index, import_source) in macro_import_sources.iter().enumerate() {
        if import_resolves_to_dep_with_resolver_view(
            view,
            import_source,
            dependency_id,
            resolve_extensions,
            workspace,
        ) {
            macro_type_deps.push(view.macro_type_deps[index].clone());
        }
    }

    if !macro_type_deps.is_empty() {
        let tier2_changed: Vec<&str> = macro_type_deps
            .iter()
            .filter(|dep| changed_exports.contains(&dep.type_name))
            .map(|dep| dep.type_name.as_str())
            .collect();

        if tier2_changed.is_empty() {
            return false;
        }

        if let Some(dep_src) = dep_source {
            let alloc = oxc_allocator::Allocator::new();
            let mut any_shape_changed = false;

            for type_name in &tier2_changed {
                let key = (dependency_id.to_string(), type_name.to_string());

                if let Some(resolved) =
                    verter_compiler::utils::oxc::vue::resolve_type::resolve_external_type(
                        type_name, dep_src, &alloc,
                    )
                {
                    let new_hash =
                        verter_compiler::utils::oxc::vue::resolve_type::hash_resolved_type(
                            &resolved,
                            dep_src.as_bytes(),
                        );

                    if let Some(old_hash) = view.resolved_type_hashes.get(&key) {
                        if *old_hash == new_hash {
                            view.resolved_type_hashes.insert(key, new_hash);
                            continue;
                        }
                    }

                    view.resolved_type_hashes.insert(key, new_hash);
                    any_shape_changed = true;
                } else {
                    any_shape_changed = true;
                }
            }

            return any_shape_changed;
        }

        return true;
    }

    let import_checks: Vec<_> = view
        .imports
        .iter()
        .map(|imp| (imp.source.clone(), imp.is_type_only))
        .collect();
    let mut has_runtime_import = false;
    for (import_source, is_type_only) in &import_checks {
        if !*is_type_only
            && import_resolves_to_dep_with_resolver_view(
                view,
                import_source,
                dependency_id,
                resolve_extensions,
                workspace,
            )
        {
            has_runtime_import = true;
            break;
        }
    }

    if has_runtime_import {
        return true;
    }

    if view.dependencies.contains(dependency_id)
        && import_checks.iter().all(|(import_source, _)| {
            !import_resolves_to_dep_with_resolver_view(
                view,
                import_source,
                dependency_id,
                resolve_extensions,
                workspace,
            )
        })
    {
        return true;
    }

    false
}

/// Smart invalidation using a pre-computed set of owner canonical IDs.
///
/// Used by the native (non-WASM) path where reverse deps are read from
/// the workspace's authoritative `EdgeStore` — the workspace is the
/// sole authority; there is no host-side `reverse_dependencies`
/// mirror.
/// Scheduler-backed smart invalidation. Reads analysis from scheduler snapshots
/// and dependency metadata from compile_cache. Clears compile_cache slots directly.
#[allow(clippy::too_many_arguments)]
/// Returns the set of owner canonical IDs that were actually invalidated
/// (compile slots cleared). Callers can use this to evict other caches
/// (e.g., `IndexedReadyDb`) only for affected dependents.
pub(crate) fn smart_invalidate_dependents_via_scheduler(
    scheduler: &verter_scheduler::scheduler::Scheduler,
    compile_cache: &dashmap::DashMap<String, crate::types::CompileCacheEntry>,
    owners: BTreeSet<String>,
    workspace: Option<&dyn verter_workspace::WorkspaceAccess>,
    config: &HostConfig,
    dependency_id: &str,
    old_export_signatures: &[verter_semantic::analysis::ExportSignature],
    new_export_signatures: &[verter_semantic::analysis::ExportSignature],
) -> BTreeSet<String> {
    if owners.is_empty() {
        return BTreeSet::new();
    }

    let changed_exports = compute_changed_exports(old_export_signatures, new_export_signatures);
    let no_signatures = old_export_signatures.is_empty() && new_export_signatures.is_empty();

    let dep_source = scheduler
        .try_get_source(dependency_id)
        .map(|s| s.source.clone());

    let mut cleared = BTreeSet::new();
    for owner in owners {
        let Some(mut view) = build_dependent_view(scheduler, compile_cache, &owner) else {
            continue;
        };
        let should_clear = should_invalidate_dependent_view(
            &mut view,
            dependency_id,
            &changed_exports,
            no_signatures,
            dep_source.as_deref(),
            &config.resolve_extensions,
            workspace,
        );
        if let Some(mut cc) = compile_cache.get_mut(&owner) {
            if should_clear {
                cc.compile_slots.clear();
                cc.cached_resolved_meta.clear();
                cc.cached_meta_payload = None;
                cc.cached_fallthrough = None;
                cleared.insert(owner.clone());
            }
            cc.resolved_type_hashes = view.resolved_type_hashes;
            cc.import_routes = view.import_routes;
            cc.dependencies = view.dependencies;
        }
    }
    cleared
}

/// Build a `DependentView` from scheduler analysis + compile_cache metadata.
fn build_dependent_view(
    scheduler: &verter_scheduler::scheduler::Scheduler,
    compile_cache: &dashmap::DashMap<String, crate::types::CompileCacheEntry>,
    canonical_id: &str,
) -> Option<DependentView> {
    use crate::host_executor::{HostAnalysisData, HostSourceData};

    let source_snap = scheduler.try_get_source(canonical_id)?;
    let hd = source_snap.downcast_data::<HostSourceData>()?;
    let script_lang = hd.parse.meta.script_lang.clone();
    drop(source_snap);

    let analysis_snap = scheduler.try_get_analysis(canonical_id)?;
    let ad = analysis_snap.downcast_data::<HostAnalysisData>()?;
    let macro_type_deps = ad.script_analysis.macro_type_deps.clone();
    let imports = ad.script_analysis.imports.clone();
    drop(analysis_snap);

    let (import_routes, dependencies, resolved_type_hashes) =
        if let Some(cc) = compile_cache.get(canonical_id) {
            (
                cc.import_routes.clone(),
                cc.dependencies.clone(),
                cc.resolved_type_hashes.clone(),
            )
        } else {
            Default::default()
        };

    Some(DependentView {
        canonical_id: canonical_id.to_string(),
        import_routes,
        dependencies,
        script_lang,
        macro_type_deps,
        imports,
        resolved_type_hashes,
    })
}

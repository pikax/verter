//! Dependency tracking and smart invalidation.
//!
//! Contains the logic for determining whether an import resolves to a given
//! dependency, stripping configured extensions for extensionless matching,
//! and the tiered invalidation decision (`should_invalidate_dependent`).

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_analysis::project_resolver::{
    NativeProjectResolver, ResolvePhase, ResolveRequest, ResolveRequestKind,
};

use crate::id;
use crate::shared::read_lock;
#[cfg(not(feature = "scheduler"))]
use crate::shared::write_lock;
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
/// On the scheduler path, populated from `HostAnalysisData` + `CompileCacheEntry`.
/// On the WASM path, populated from `FileEntry`. Avoids passing `&FileEntry` into
/// invalidation logic, enabling the `files` map to be gated to WASM-only.
pub(crate) struct DependentView {
    pub(crate) canonical_id: String,
    pub(crate) dependency_resolutions: rustc_hash::FxHashMap<String, DependencyResolution>,
    pub(crate) dependencies: BTreeSet<String>,
    pub(crate) script_lang: Option<String>,
    pub(crate) macro_type_deps: Vec<verter_analysis::MacroTypeDep>,
    pub(crate) imports: Vec<verter_analysis::AnalyzedImport>,
    pub(crate) resolved_type_hashes: rustc_hash::FxHashMap<(String, String), Hash16>,
}

impl DependentView {
    #[cfg(any(not(feature = "scheduler"), test))]
    pub(crate) fn from_file_entry(entry: &FileEntry) -> Self {
        Self {
            canonical_id: entry.canonical_id.clone(),
            dependency_resolutions: entry.dependency_resolutions.clone(),
            dependencies: entry.dependencies.clone(),
            script_lang: entry.meta.script_lang.clone(),
            macro_type_deps: entry.script_analysis.macro_type_deps.clone(),
            imports: entry.script_analysis.imports.clone(),
            resolved_type_hashes: entry.resolved_type_hashes.clone(),
        }
    }
}

/// Check if an import source from `view` resolves to `dependency_id`.
/// Handles both relative paths (resolved via resolve_external) and
/// non-relative paths (matched via the file's registered dependencies).
///
/// Checks structured `dependency_resolutions` first for exact matches,
/// then falls back to heuristic resolution.
fn import_resolves_to_dep_view(
    view: &DependentView,
    import_source: &str,
    dependency_id: &str,
    resolve_extensions: &[String],
) -> bool {
    if let Some(resolution) = view.dependency_resolutions.get(import_source) {
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

/// Check if an import source from `file` resolves to `dependency_id`.
/// Legacy wrapper that delegates to `import_resolves_to_dep_view`.
#[cfg(any(not(feature = "scheduler"), test))]
pub(crate) fn import_resolves_to_dep(
    file: &FileEntry,
    import_source: &str,
    dependency_id: &str,
    resolve_extensions: &[String],
) -> bool {
    let view = DependentView::from_file_entry(file);
    import_resolves_to_dep_view(&view, import_source, dependency_id, resolve_extensions)
}

/// A lightweight reader backed by a set of known file IDs.
/// Used during smart invalidation where only `file_exists` is needed
/// and the full file map is mutably borrowed.
struct FileIdSetReader<'a> {
    ids: &'a rustc_hash::FxHashSet<String>,
}

impl verter_vfs::WorkspaceAccess for FileIdSetReader<'_> {
    fn file_exists(&self, canonical_id: &str) -> bool {
        self.ids.contains(canonical_id)
    }
    fn read_file(&self, _canonical_id: &str) -> Option<Arc<str>> {
        None // Not needed for existence-only resolution
    }
    fn realpath(&self, canonical_id: &str) -> Option<String> {
        if self.ids.contains(canonical_id) {
            Some(canonical_id.to_string())
        } else {
            None
        }
    }
}

fn import_resolves_to_dep_with_resolver_view(
    view: &DependentView,
    import_source: &str,
    dependency_id: &str,
    resolve_extensions: &[String],
    resolver: Option<&NativeProjectResolver>,
    file_ids: &rustc_hash::FxHashSet<String>,
) -> bool {
    if import_resolves_to_dep_view(view, import_source, dependency_id, resolve_extensions) {
        return true;
    }

    if let Some(resolver) = resolver {
        let reader = FileIdSetReader { ids: file_ids };
        let request = ResolveRequest {
            importer_id: view.canonical_id.clone(),
            specifier: import_source.to_string(),
            kind: ResolveRequestKind::EsmImport,
            phase: ResolvePhase::CodegenBlocker,
        };
        if let Some(result) = resolver.resolve_with_reader(&reader, &request) {
            return result.source_id == dependency_id;
        }
    }

    false
}

/// Like [`import_resolves_to_dep`], but also consults the project resolver
/// for aliased specifiers that heuristic matching cannot handle.
#[cfg(not(feature = "scheduler"))]
pub(crate) fn import_resolves_to_dep_with_resolver(
    file: &FileEntry,
    import_source: &str,
    dependency_id: &str,
    resolve_extensions: &[String],
    resolver: Option<&NativeProjectResolver>,
    file_ids: &rustc_hash::FxHashSet<String>,
) -> bool {
    let view = DependentView::from_file_entry(file);
    import_resolves_to_dep_with_resolver_view(
        &view,
        import_source,
        dependency_id,
        resolve_extensions,
        resolver,
        file_ids,
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
/// write back the updated hashes to the appropriate store (FileEntry or CompileCacheEntry).
#[allow(clippy::too_many_arguments)]
pub(crate) fn should_invalidate_dependent_view(
    view: &mut DependentView,
    dependency_id: &str,
    changed_exports: &BTreeSet<String>,
    no_signatures: bool,
    dep_source: Option<&str>,
    resolve_extensions: &[String],
    resolver: Option<&NativeProjectResolver>,
    file_ids: &rustc_hash::FxHashSet<String>,
) -> bool {
    if no_signatures {
        return true;
    }

    if changed_exports.is_empty() {
        return false;
    }

    let macro_type_deps: Vec<&verter_analysis::MacroTypeDep> = view
        .macro_type_deps
        .iter()
        .filter(|dep| {
            import_resolves_to_dep_with_resolver_view(
                view,
                &dep.import_source,
                dependency_id,
                resolve_extensions,
                resolver,
                file_ids,
            )
        })
        .collect();

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
                    verter_core::utils::oxc::vue::resolve_type::resolve_external_type(
                        type_name, dep_src, &alloc,
                    )
                {
                    let new_hash = verter_core::utils::oxc::vue::resolve_type::hash_resolved_type(
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

    let has_runtime_import = view.imports.iter().any(|imp| {
        !imp.is_type_only
            && import_resolves_to_dep_with_resolver_view(
                view,
                &imp.source,
                dependency_id,
                resolve_extensions,
                resolver,
                file_ids,
            )
    });

    if has_runtime_import {
        return true;
    }

    if view.dependencies.contains(dependency_id)
        && view.imports.iter().all(|imp| {
            !import_resolves_to_dep_with_resolver_view(
                view,
                &imp.source,
                dependency_id,
                resolve_extensions,
                resolver,
                file_ids,
            )
        })
    {
        return true;
    }

    false
}

/// Legacy wrapper for should_invalidate_dependent_view using FileEntry.
#[cfg(not(feature = "scheduler"))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn should_invalidate_dependent(
    file: &mut FileEntry,
    dependency_id: &str,
    changed_exports: &BTreeSet<String>,
    no_signatures: bool,
    dep_source: Option<&str>,
    resolve_extensions: &[String],
    resolver: Option<&NativeProjectResolver>,
    file_ids: &rustc_hash::FxHashSet<String>,
) -> bool {
    let mut view = DependentView::from_file_entry(file);
    let result = should_invalidate_dependent_view(
        &mut view,
        dependency_id,
        changed_exports,
        no_signatures,
        dep_source,
        resolve_extensions,
        resolver,
        file_ids,
    );
    // Write back updated type hashes
    file.resolved_type_hashes = view.resolved_type_hashes;
    result
}

/// Smart invalidation using a pre-computed set of owner canonical IDs.
///
/// Used by the native (non-WASM) path where reverse deps are read from the
/// workspace's authoritative EdgeStore instead of the host's legacy
/// `reverse_dependencies` map.
#[cfg(not(feature = "scheduler"))]
pub(crate) fn smart_invalidate_dependents_with_owners(
    files: &crate::shared::Shared<rustc_hash::FxHashMap<String, FileEntry>>,
    owners: BTreeSet<String>,
    project_resolver: &crate::shared::Shared<Option<NativeProjectResolver>>,
    config: &HostConfig,
    dependency_id: &str,
    old_export_signatures: &[verter_analysis::ExportSignature],
    new_export_signatures: &[verter_analysis::ExportSignature],
) {
    if owners.is_empty() {
        return;
    }

    let changed_exports = compute_changed_exports(old_export_signatures, new_export_signatures);
    let resolver = read_lock(project_resolver);
    let mut files = write_lock(files);
    let dep_source = files.get(dependency_id).map(|f| Arc::clone(&f.source));
    let file_ids: rustc_hash::FxHashSet<String> = if resolver.is_some() {
        files.keys().cloned().collect()
    } else {
        rustc_hash::FxHashSet::default()
    };

    for owner in owners {
        if let Some(file) = files.get_mut(&owner) {
            if should_invalidate_dependent(
                file,
                dependency_id,
                &changed_exports,
                old_export_signatures.is_empty() && new_export_signatures.is_empty(),
                dep_source.as_deref(),
                &config.resolve_extensions,
                resolver.as_ref(),
                &file_ids,
            ) {
                file.compile_slots.clear();
            }
        }
    }
}

/// Scheduler-backed smart invalidation. Reads analysis from scheduler snapshots
/// and dependency metadata from compile_cache. Clears compile_cache slots directly.
#[cfg(feature = "scheduler")]
pub(crate) fn smart_invalidate_dependents_via_scheduler(
    scheduler: &verter_scheduler::scheduler::Scheduler,
    compile_cache: &dashmap::DashMap<String, crate::types::CompileCacheEntry>,
    owners: BTreeSet<String>,
    project_resolver: &crate::shared::Shared<Option<NativeProjectResolver>>,
    config: &HostConfig,
    dependency_id: &str,
    old_export_signatures: &[verter_analysis::ExportSignature],
    new_export_signatures: &[verter_analysis::ExportSignature],
) {
    if owners.is_empty() {
        return;
    }

    let changed_exports = compute_changed_exports(old_export_signatures, new_export_signatures);
    let no_signatures = old_export_signatures.is_empty() && new_export_signatures.is_empty();
    let resolver = read_lock(project_resolver);

    let dep_source = scheduler
        .try_get_source(dependency_id)
        .map(|s| s.source.clone());

    let file_ids: rustc_hash::FxHashSet<String> = if resolver.is_some() {
        scheduler.node_ids().into_iter().collect()
    } else {
        rustc_hash::FxHashSet::default()
    };

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
            resolver.as_ref(),
            &file_ids,
        );
        if let Some(mut cc) = compile_cache.get_mut(&owner) {
            if should_clear {
                cc.compile_slots.clear();
            }
            cc.resolved_type_hashes = view.resolved_type_hashes;
        }
    }
}

/// Build a `DependentView` from scheduler analysis + compile_cache metadata.
#[cfg(feature = "scheduler")]
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

    let (dependency_resolutions, dependencies, resolved_type_hashes) =
        if let Some(cc) = compile_cache.get(canonical_id) {
            (
                cc.dependency_resolutions.clone(),
                cc.dependencies.clone(),
                cc.resolved_type_hashes.clone(),
            )
        } else {
            Default::default()
        };

    Some(DependentView {
        canonical_id: canonical_id.to_string(),
        dependency_resolutions,
        dependencies,
        script_lang,
        macro_type_deps,
        imports,
        resolved_type_hashes,
    })
}

/// Smart invalidation: when a dependency changes, only invalidate dependent
/// SFCs whose macro-consumed types were actually affected.
///
/// Uses tiered precision:
/// - Tier 1: No export signatures → full invalidation
/// - Tier 2: Export-level hashing → invalidate only if macro-consumed exports changed
/// - Tier 3: Cross-file type resolution → invalidate only if resolved type shape changed
#[cfg(not(feature = "scheduler"))]
pub(crate) fn smart_invalidate_dependents(
    files: &crate::shared::Shared<rustc_hash::FxHashMap<String, FileEntry>>,
    reverse_dependencies: &crate::shared::Shared<rustc_hash::FxHashMap<String, BTreeSet<String>>>,
    project_resolver: &crate::shared::Shared<Option<NativeProjectResolver>>,
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

    let resolver = read_lock(project_resolver);
    let mut files = write_lock(files);

    // Get dep source for Tier 3 resolution (clone Arc to avoid borrow conflict)
    let dep_source = files.get(dependency_id).map(|f| Arc::clone(&f.source));

    // Build a read-only snapshot of file IDs for the resolver reader.
    // We need this because we hold a write lock on files for get_mut(),
    // but the resolver reader needs shared access to check file_exists().
    // Only built if the resolver is actually configured.
    let file_ids: rustc_hash::FxHashSet<String> = if resolver.is_some() {
        files.keys().cloned().collect()
    } else {
        rustc_hash::FxHashSet::default()
    };

    for owner in owners {
        if let Some(file) = files.get_mut(&owner) {
            if should_invalidate_dependent(
                file,
                dependency_id,
                &changed_exports,
                old_export_signatures.is_empty() && new_export_signatures.is_empty(),
                dep_source.as_deref(),
                resolve_extensions,
                resolver.as_ref(),
                &file_ids,
            ) {
                file.compile_slots.clear();
            }
        }
    }
}

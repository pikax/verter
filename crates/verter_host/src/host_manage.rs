//! `impl VerterHost` — file management, analysis, and diagnostics methods.
//!
//! Contains [`VerterHost::remove`], [`VerterHost::get_analysis`],
//! [`VerterHost::get_diagnostics`], and [`VerterHost::set_import_dependencies`].

use std::sync::Arc;

use verter_analysis::project_resolver::NativeProjectResolver;

use crate::hash::compile_profile_hash;
use crate::id::canonicalize_id;
use crate::shared::{read_lock, write_lock};
use crate::types::*;
use crate::VerterHost;

impl VerterHost {
    /// Returns the original source for a file by canonical ID or alias.
    /// Returns `None` when the file does not exist in the host.
    pub fn get_source(&self, canonical_or_alias: &str) -> Option<std::sync::Arc<str>> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let files = read_lock(&self.files);
        files.get(&canonical).map(|entry| entry.source.clone())
    }

    /// Resolve imported type definitions for a file's macro type dependencies.
    ///
    /// For each `MacroTypeDep` (e.g., `defineProps<ButtonProps>()` importing from `./types`),
    /// resolves the type by reading the dependency source from the host cache and running
    /// `resolve_external_type()`. Returns expanded type text suitable for the type registry.
    pub fn resolve_imported_types(
        &self,
        canonical_or_alias: &str,
    ) -> Vec<verter_analysis::ResolvedLocalType> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let files = read_lock(&self.files);
        let Some(entry) = files.get(&canonical) else {
            return Vec::new();
        };

        let macro_type_deps = entry.script_analysis.macro_type_deps.clone();
        if macro_type_deps.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::new();
        let alloc = oxc_allocator::Allocator::new();

        for dep in macro_type_deps.iter() {
            // Resolve import source to canonical dep ID
            let dep_canonical = if dep.import_source.starts_with('.') {
                crate::id::resolve_external(&canonical, &dep.import_source)
            } else {
                // Non-relative imports need dependency_resolutions
                if let Some(res) = entry.dependency_resolutions.get(&dep.import_source) {
                    if let Some(ref id) = res.resolved_canonical_id {
                        id.clone()
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            };

            // Try to get the dependency source from the host cache
            let Some(dep_entry) = files.get(&dep_canonical) else {
                // Try with common extensions for relative imports
                let mut found_source = None;
                for ext in &[".ts", ".tsx", "/index.ts", ".d.ts"] {
                    let with_ext = format!("{}{}", dep_canonical, ext);
                    if let Some(e) = files.get(&with_ext) {
                        found_source = Some(e.source.clone());
                        break;
                    }
                }
                let Some(source) = found_source else {
                    continue;
                };
                // Resolve and add to result
                if let Some(resolved) =
                    verter_core::utils::oxc::vue::resolve_type::resolve_external_type(
                        &dep.type_name,
                        &source,
                        &alloc,
                    )
                {
                    let expanded = resolved_elements_to_expanded_text(&resolved, &source);
                    result.push(verter_analysis::ResolvedLocalType {
                        name: dep.type_name.clone(),
                        expanded,
                        span: verter_span::Span::default(),
                    });
                }
                continue;
            };

            let dep_source = dep_entry.source.clone();
            if let Some(resolved) =
                verter_core::utils::oxc::vue::resolve_type::resolve_external_type(
                    &dep.type_name,
                    &dep_source,
                    &alloc,
                )
            {
                let expanded = resolved_elements_to_expanded_text(&resolved, &dep_source);
                result.push(verter_analysis::ResolvedLocalType {
                    name: dep.type_name.clone(),
                    expanded,
                    span: verter_span::Span::default(),
                });
            }
        }

        result
    }

    /// Returns a serializable snapshot of the file's static analysis data.
    /// Returns `None` if the file doesn't exist.
    /// When `eager_analysis` is false, computes analysis on demand from stored source.
    ///
    /// Template analysis is included when it has been computed during a prior
    /// compilation (requires template scope flags and a `get_virtual_file()` call).
    ///
    /// Import `resolved_canonical_id` fields are populated lazily using the host's
    /// file map, alias map, and parent dependency set.
    pub fn get_analysis(&self, canonical_or_alias: &str) -> Option<FileAnalysisSnapshot> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let files = read_lock(&self.files);
        let entry = files.get(&canonical)?;

        // If analysis wasn't fully computed during upsert, compute missing parts on demand
        let scope = self.config.effective_scope();
        if entry.file_kind == FileKind::VueSfc
            && (!scope.needs_script_analysis() || !scope.needs_style_analysis())
        {
            let source = entry.source.clone();
            let stored_script = entry.script_analysis.clone();
            let stored_styles = Arc::clone(&entry.style_analyses);
            let template = entry.template_analysis.clone();
            let cached_parse = entry.cached_parse.clone();
            let export_sigs = entry.export_signatures.clone();
            drop(files);

            let mut script_analysis = if !scope.needs_script_analysis() {
                if let Some(parsed) = cached_parse.as_deref() {
                    crate::parse::build_script_analysis_from_parsed(parsed, &source)
                } else {
                    crate::parse::build_script_analysis_from_source(&source)
                }
            } else {
                // Script analysis was already computed during upsert
                stored_script
            };
            let style_analyses = if !scope.needs_style_analysis() {
                if let Some(parsed) = cached_parse.as_deref() {
                    Arc::new(crate::parse::build_style_analyses_from_parsed(
                        parsed, &source, &canonical,
                    ))
                } else {
                    Arc::new(crate::parse::build_style_analyses_from_source(
                        &source, &canonical,
                    ))
                }
            } else {
                stored_styles
            };
            // Cross-reference: mark script bindings referenced by CSS v-bind()
            if !style_analyses.is_empty() && !script_analysis.bindings.is_empty() {
                script_analysis.mark_bindings_used_in_style(&style_analyses);
            }
            // On-demand path: build fresh Arcs (not cached, but avoids deep clone)
            let mut snapshot = FileAnalysisSnapshot {
                imports: script_analysis.imports,
                module_references: Arc::new(script_analysis.module_references),
                bindings: script_analysis.bindings,
                macros: Arc::new(script_analysis.macros),
                macro_type_deps: Arc::new(script_analysis.macro_type_deps),
                script_flags: script_analysis.flags.bits(),
                styles: style_analyses,
                template,
                vue_api_calls: Arc::new(script_analysis.vue_api_calls),
                dom_query_calls: Arc::new(script_analysis.dom_query_calls),
                css_var_manipulations: Arc::new(script_analysis.css_var_manipulations),
                script_binding_occurrences: Arc::new(script_analysis.script_binding_occurrences),
                export_signatures: Arc::new(export_sigs),
                options_api: script_analysis.options_api,
                store_usages: Arc::new(script_analysis.store_usages),
                store_definitions: Arc::new(script_analysis.store_definitions),
                is_typescript: script_analysis.is_typescript,
            };
            self.resolve_snapshot_imports(&canonical, &mut snapshot);
            self.enrich_destructured_bindings(&mut snapshot);
            return Some(snapshot);
        }

        // Fast path: Arc::clone for 9 immutable fields, deep clone only imports + bindings
        let mut snapshot = Self::build_snapshot_from_entry(entry);
        // Drop the files lock before resolving (resolve_snapshot_imports acquires its own)
        drop(files);
        self.resolve_snapshot_imports(&canonical, &mut snapshot);
        self.enrich_destructured_bindings(&mut snapshot);
        Some(snapshot)
    }

    /// Returns the semantic hash for a file by canonical ID or alias.
    ///
    /// The semantic hash changes when the file's semantically significant content
    /// changes (script, template, scoped styles). Returns `None` for missing files.
    pub fn get_semantic_hash(&self, canonical_or_alias: &str) -> Option<Hash16> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let files = read_lock(&self.files);
        files.get(&canonical).map(|entry| entry.semantic_hash)
    }

    /// Returns the compile-blocking dependencies for a Vue SFC.
    ///
    /// This exposes the SFC's external `src` blocks and macro type dependencies
    /// so embedding environments can resolve/load them before triggering codegen.
    pub fn get_compile_blockers(
        &self,
        canonical_or_alias: &str,
    ) -> Option<CompileBlockersSnapshot> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let files = read_lock(&self.files);
        let entry = files.get(&canonical)?;
        if entry.file_kind != FileKind::VueSfc {
            return None;
        }

        Some(CompileBlockersSnapshot {
            external_source_requests: entry.external_requests.clone(),
            macro_type_deps: Arc::clone(&entry.arc_script_cache.macro_type_deps),
        })
    }

    /// Returns analysis snapshots for multiple files in a single lock acquisition.
    ///
    /// More efficient than calling `get_analysis()` in a loop: acquires the
    /// files read-lock once for all files instead of N separate acquisitions.
    ///
    /// Accepts canonical IDs, aliases, or `None` to return all files.
    /// When `canonical_ids` is `None`, returns snapshots for every file in the host.
    pub fn get_analysis_batch(
        &self,
        canonical_ids: &[&str],
    ) -> Vec<(String, FileAnalysisSnapshot)> {
        let files = read_lock(&self.files);
        let mut results = Vec::with_capacity(canonical_ids.len());

        for &id in canonical_ids {
            let canonical = self.resolve_alias_or_canonical(id);
            if let Some(entry) = files.get(&canonical) {
                let snapshot = Self::build_snapshot_from_entry(entry);
                results.push((canonical, snapshot));
            }
        }
        drop(files);

        // Post-process: resolve imports and enrich bindings for all
        for (canonical, snapshot) in &mut results {
            self.resolve_snapshot_imports(canonical, snapshot);
            self.enrich_destructured_bindings(snapshot);
        }
        results
    }

    /// Returns analysis snapshots for all files in the host.
    ///
    /// Single lock acquisition for the entire file map. Use instead of
    /// `list_files()` + loop when you need analysis for every file.
    pub fn get_analysis_all(&self) -> Vec<(String, FileAnalysisSnapshot)> {
        let files = read_lock(&self.files);
        let mut results = Vec::with_capacity(files.len());

        for (canonical, entry) in files.iter() {
            let snapshot = Self::build_snapshot_from_entry(entry);
            results.push((canonical.clone(), snapshot));
        }
        drop(files);

        for (canonical, snapshot) in &mut results {
            self.resolve_snapshot_imports(canonical, snapshot);
            self.enrich_destructured_bindings(snapshot);
        }
        results
    }

    /// Build a `FileAnalysisSnapshot` from a `FileEntry` using Arc::clone
    /// for immutable fields and deep clone for mutable fields (imports, bindings).
    fn build_snapshot_from_entry(entry: &crate::FileEntry) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            imports: entry.script_analysis.imports.clone(),
            bindings: entry.script_analysis.bindings.clone(),
            // Arc::clone — cheap pointer bump, no deep copy
            module_references: Arc::clone(&entry.arc_script_cache.module_references),
            macros: Arc::clone(&entry.arc_script_cache.macros),
            macro_type_deps: Arc::clone(&entry.arc_script_cache.macro_type_deps),
            script_flags: entry.script_analysis.flags.bits(),
            styles: Arc::clone(&entry.style_analyses),
            template: entry.template_analysis.clone(),
            vue_api_calls: Arc::clone(&entry.arc_script_cache.vue_api_calls),
            dom_query_calls: Arc::clone(&entry.arc_script_cache.dom_query_calls),
            css_var_manipulations: Arc::clone(&entry.arc_script_cache.css_var_manipulations),
            script_binding_occurrences: Arc::clone(
                &entry.arc_script_cache.script_binding_occurrences,
            ),
            export_signatures: Arc::new(entry.export_signatures.clone()),
            options_api: entry.script_analysis.options_api.clone(),
            store_usages: Arc::clone(&entry.arc_script_cache.store_usages),
            store_definitions: Arc::clone(&entry.arc_script_cache.store_definitions),
            is_typescript: entry.script_analysis.is_typescript,
        }
    }

    /// Populate `resolved_canonical_id` on each import in the snapshot
    /// using the host's file map, alias map, and parent's dependency set.
    fn resolve_snapshot_imports(
        &self,
        parent_canonical_id: &str,
        snapshot: &mut FileAnalysisSnapshot,
    ) {
        let files = read_lock(&self.files);
        let alias_map = read_lock(&self.alias_to_canonical);
        let resolver = read_lock(&self.project_resolver);
        let Some(entry) = files.get(parent_canonical_id) else {
            return;
        };
        for import in &mut snapshot.imports {
            if import.resolved_canonical_id.is_none() {
                import.resolved_canonical_id = crate::cross_file::resolve_import_to_canonical(
                    &files,
                    &alias_map,
                    resolver.as_ref(),
                    entry,
                    &import.source,
                );
            }
        }
    }

    /// Enrich destructured composable bindings with per-field reactivity info.
    ///
    /// When a binding has `reactivity_kind: MaybeRef` and its initializer is a
    /// `FunctionCall` to a composable, look up the composable's `return_shape`
    /// from the resolved file's `exported_functions`. If it's `Object(fields)`,
    /// match binding names to field names and replace `MaybeRef` with the
    /// field's actual `ReactivityKind`.
    fn enrich_destructured_bindings(&self, snapshot: &mut FileAnalysisSnapshot) {
        use verter_analysis::types::{BindingInitializer, ComposableReturn, ReactivityKind};

        // Build a map of import source → resolved canonical ID from the snapshot
        let import_resolved: rustc_hash::FxHashMap<&str, &str> = snapshot
            .imports
            .iter()
            .filter_map(|imp| {
                imp.resolved_canonical_id
                    .as_deref()
                    .map(|resolved| (imp.source.as_str(), resolved))
            })
            .collect();

        let files = read_lock(&self.files);

        for binding in &mut snapshot.bindings {
            if binding.reactivity_kind != ReactivityKind::MaybeRef {
                continue;
            }

            let Some(BindingInitializer::FunctionCall {
                callee,
                callee_import_source,
                ..
            }) = &binding.initializer
            else {
                continue;
            };

            // Resolve the composable's source file
            let import_source = match callee_import_source {
                Some(src) => src.as_str(),
                None => continue, // Local function, can't do cross-file
            };

            let canonical_id = match import_resolved.get(import_source) {
                Some(id) => *id,
                None => continue,
            };

            let Some(entry) = files.get(canonical_id) else {
                continue;
            };

            // Find the exported function matching the callee name
            let composable_info = entry
                .script_analysis
                .exported_functions
                .iter()
                .find(|f| f.name == *callee)
                .and_then(|f| f.composable.as_ref());

            let Some(info) = composable_info else {
                continue;
            };

            match &info.return_shape {
                ComposableReturn::Object(fields) => {
                    if let Some(field) = fields.iter().find(|f| f.name == binding.name) {
                        binding.reactivity_kind = field.reactivity;
                        binding.is_reactive = !matches!(field.reactivity, ReactivityKind::None);
                    }
                }
                ComposableReturn::Single(kind) => {
                    binding.reactivity_kind = *kind;
                    binding.is_reactive = !matches!(kind, ReactivityKind::None);
                }
                _ => {}
            }
        }
    }

    /// Returns stored diagnostics for a file+profile without triggering compilation.
    /// Returns `None` if the file doesn't exist or has no diagnostics for this profile.
    pub fn get_diagnostics(
        &self,
        canonical_or_alias: &str,
        profile: &CompileProfile,
    ) -> Option<DiagnosticsSnapshot> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let profile_hash = compile_profile_hash(profile);
        let files = read_lock(&self.files);
        let entry = files.get(&canonical)?;
        entry.latest_diagnostics.get(&profile_hash).cloned()
    }

    /// Returns the monotonic diagnostics generation counter for a file.
    /// Incremented on every write to `latest_diagnostics`. Used by the LSP
    /// cache to detect host-driven recompiles without a document version change.
    pub fn get_diagnostics_generation(&self, canonical_or_alias: &str) -> Option<u64> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let files = read_lock(&self.files);
        files.get(&canonical).map(|e| e.diagnostics_generation)
    }

    /// Bump the diagnostics generation counter for a file without changing
    /// its diagnostics. This causes the LSP diagnostic cache to treat the
    /// next `compute_verter_diagnostics_for` call as a cache miss, forcing
    /// a fresh recomputation. Used after hydrating compile blockers (e.g.,
    /// macro type deps) where the source hasn't changed but stale error
    /// diagnostics need to be cleared.
    pub fn bump_diagnostics_generation(&self, canonical_or_alias: &str) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let mut files = write_lock(&self.files);
        if let Some(entry) = files.get_mut(&canonical) {
            entry.diagnostics_generation += 1;
        }
    }

    /// Clear all compile slots for a specific file so `ensure_compiled` will
    /// recompile it on the next call. Use this after loading new dependencies
    /// (e.g., macro type deps via hydration) that affect the compilation
    /// output even though the source itself hasn't changed.
    pub fn invalidate_compile_slots(&self, canonical_or_alias: &str) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let mut files = write_lock(&self.files);
        if let Some(entry) = files.get_mut(&canonical) {
            entry.compile_slots.clear();
        }
    }

    /// Invalidate compile outputs of files that depend on the given path.
    ///
    /// Unlike `remove()`, this works even when the dependency file was never
    /// loaded into the host but reverse-dependency edges were registered.
    pub fn invalidate_dependents_of(&self, canonical_or_alias: &str) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        self.smart_invalidate_dependents(&canonical, &[], &[]);
    }

    /// Remove a file from the host, cleaning up aliases, dependencies,
    /// and invalidating compile slots of any dependents.
    pub fn remove(&self, canonical_or_alias: &str) -> Option<HostRemoveResult> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        let removed = {
            let mut files = write_lock(&self.files);
            files.remove(&canonical)
        }?;

        {
            let mut alias_map = write_lock(&self.alias_to_canonical);
            for alias in &removed.aliases {
                alias_map.remove(alias);
            }
        }

        // Collect dependents before modifying the reverse_dependencies map.
        let dependents = {
            let rev = read_lock(&self.reverse_dependencies);
            rev.get(&canonical).cloned().unwrap_or_default()
        };

        {
            let mut rev = write_lock(&self.reverse_dependencies);
            for dep in &removed.dependencies {
                if let Some(owners) = rev.get_mut(dep) {
                    owners.remove(&canonical);
                    if owners.is_empty() {
                        rev.remove(dep);
                    }
                }
            }
            rev.remove(&canonical);
        }

        // Invalidate compile slots of files that depended on the removed file.
        if !dependents.is_empty() {
            let mut files = write_lock(&self.files);
            for owner in dependents {
                if let Some(file) = files.get_mut(&owner) {
                    file.compile_slots.clear();
                }
            }
        }

        // Clean up VFS state (overlay, snapshot, edges) so the file is
        // no longer resolvable or tracked after deletion.
        #[cfg(not(target_arch = "wasm32"))]
        self.ws().notify_delete(&canonical);

        Some(HostRemoveResult {
            canonical_id: canonical,
        })
    }

    /// Returns the list of virtual node kinds for a file.
    /// Returns an empty vec if the file doesn't exist.
    pub fn list_virtual_nodes(&self, canonical_or_alias: &str) -> Vec<VirtualNodeKind> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let files = read_lock(&self.files);
        files
            .get(&canonical)
            .map(|e| e.all_virtual_nodes())
            .unwrap_or_default()
    }

    /// Provide caller-resolved import dependency resolution records.
    ///
    /// Called after `upsert()` when the caller resolves import specifiers
    /// (tsconfig paths, vite aliases, etc.) using bundler/LSP resolution.
    /// Each record maps a raw import specifier to its resolved canonical ID
    /// (or a list of candidate canonical IDs).
    ///
    /// Records are merged into the file's `dependency_resolutions` map (keyed by
    /// specifier). The flat `dependencies` set is updated in parallel for
    /// reverse-dependency tracking.
    pub fn set_import_dependencies(
        &self,
        canonical_or_alias: &str,
        resolutions: Vec<DependencyResolution>,
    ) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        // Build VFS exact resolutions before the loop consumes the data.
        #[cfg(not(target_arch = "wasm32"))]
        let vfs_resolutions: Vec<verter_vfs::ExactResolution> = resolutions
            .iter()
            .map(|r| verter_vfs::ExactResolution {
                specifier: r.specifier.clone(),
                resolved_canonical_id: r.resolved_canonical_id.as_ref().map(|id| {
                    let norm = canonicalize_id(id);
                    norm.into_owned()
                }),
                possible_canonical_ids: r
                    .possible_canonical_ids
                    .iter()
                    .map(|c| {
                        let norm = canonicalize_id(c);
                        norm.into_owned()
                    })
                    .collect(),
            })
            .collect();

        // Single write lock to avoid TOCTOU race between read-read-write.
        let (old_deps, new_deps) = {
            let mut files = write_lock(&self.files);
            let Some(entry) = files.get_mut(&canonical) else {
                return;
            };
            let old_deps = entry.dependencies.clone();
            for mut res in resolutions {
                // Normalize paths so Windows backslashes / drive-letter case
                // match the canonical IDs used by `upsert()`.
                if let Some(ref mut id) = res.resolved_canonical_id {
                    let norm = canonicalize_id(id);
                    if norm != id.as_str() {
                        *id = norm.into_owned();
                    }
                }
                for candidate in &mut res.possible_canonical_ids {
                    let norm = canonicalize_id(candidate);
                    if norm != candidate.as_str() {
                        *candidate = norm.into_owned();
                    }
                }
                // Derive flat dependency set for reverse-dep tracking
                if let Some(ref canonical_id) = res.resolved_canonical_id {
                    entry.dependencies.insert(canonical_id.clone());
                } else {
                    for candidate in &res.possible_canonical_ids {
                        entry.dependencies.insert(candidate.clone());
                    }
                }
                // Store structured record for exact resolution lookups
                entry
                    .dependency_resolutions
                    .insert(res.specifier.clone(), res);
            }
            let new_deps = entry.dependencies.clone();
            (old_deps, new_deps)
        };

        self.update_reverse_deps(&canonical, &old_deps, &new_deps);

        // Sync exact resolutions to workspace.
        #[cfg(not(target_arch = "wasm32"))]
        self.ws().set_exact_resolutions(&canonical, vfs_resolutions);
    }

    /// Returns all known canonical file IDs and their file kinds.
    pub fn list_files(&self) -> Vec<(String, FileKind)> {
        let files = read_lock(&self.files);
        files
            .iter()
            .map(|(id, entry)| (id.clone(), entry.file_kind))
            .collect()
    }

    /// Returns cross-component CSS variable flow for a given variable name.
    ///
    /// Scans all files in the host to find where the variable is defined (in `<style>`),
    /// referenced via `var()` (in `<style>`), set via `:style` bindings (in `<template>`),
    /// and manipulated via DOM APIs (in `<script>`).
    pub fn css_var_flow(&self, var_name: &str) -> verter_analysis::CssVarFlow {
        let files = read_lock(&self.files);
        let mut flow = verter_analysis::CssVarFlow {
            name: var_name.to_string(),
            ..Default::default()
        };

        for (canonical_id, entry) in files.iter() {
            let path: std::sync::Arc<std::path::Path> =
                std::sync::Arc::from(std::path::Path::new(canonical_id.as_str()));

            // Check style blocks for definitions and var() references
            for style in entry.style_analyses.iter() {
                if let Some(ref css) = style.css {
                    let has_def = css.custom_properties.iter().any(|p| p.name == var_name);
                    if has_def {
                        flow.style_definitions.push(std::sync::Arc::clone(&path));
                    }

                    let has_ref = css.var_usages.iter().any(|u| u.reference.name == var_name);
                    if has_ref {
                        flow.style_var_usages.push(std::sync::Arc::clone(&path));
                    }
                }
            }

            // Check template for :style CSS variable bindings
            if let Some(ref tmpl) = entry.template_analysis {
                if tmpl.css_var_names.iter().any(|n| n == var_name) {
                    flow.template_definitions.push(std::sync::Arc::clone(&path));
                }
            }

            // Check script for DOM API CSS variable manipulations
            if entry
                .script_analysis
                .css_var_manipulations
                .iter()
                .any(|m| m.var_name == var_name)
            {
                flow.script_manipulations.push(std::sync::Arc::clone(&path));
            }
        }

        flow
    }

    /// Look up the byte span of an exported name in a target file.
    ///
    /// For `.vue` files: searches `ScriptAnalysisSnapshot.bindings` (script-setup
    /// auto-exports) — spans are SFC-absolute.
    /// For `.ts`/`.js` files: searches `FileEntry.export_signatures` — spans are
    /// file-absolute.
    ///
    /// Returns `None` if the file doesn't exist or the name isn't exported.
    pub fn get_export_span(
        &self,
        canonical_or_alias: &str,
        binding_name: &str,
    ) -> Option<(u32, u32)> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let files = read_lock(&self.files);
        let entry = files.get(&canonical)?;

        // For Vue SFCs, look up script bindings (script-setup auto-exports)
        if entry.file_kind == FileKind::VueSfc {
            // Check bindings first (covers refs, functions, etc.)
            if let Some(binding) = entry
                .script_analysis
                .bindings
                .iter()
                .find(|b| b.name == binding_name)
            {
                if binding.span.start > 0 || binding.span.end > 0 {
                    return Some((binding.span.start, binding.span.end));
                }
            }
            // Check macro binding names (defineProps → props, defineEmits → emit, etc.)
            for mac in &entry.script_analysis.macros {
                if mac.binding_name.as_deref() == Some(binding_name)
                    && (mac.span.start > 0 || mac.span.end > 0)
                {
                    return Some((mac.span.start, mac.span.end));
                }
            }
            // "default" export for .vue files → first script block start
            if binding_name == "default" {
                // For default imports of .vue files, the component IS the default export.
                // Point to the <script setup> tag or first binding as a reasonable target.
                if let Some(first_binding) = entry.script_analysis.bindings.first() {
                    if first_binding.span.start > 0 || first_binding.span.end > 0 {
                        return Some((first_binding.span.start, first_binding.span.end));
                    }
                }
                // No bindings — try first macro (e.g., `defineProps<...>()` without assignment)
                if let Some(first_macro) = entry.script_analysis.macros.first() {
                    if first_macro.span.start > 0 || first_macro.span.end > 0 {
                        return Some((first_macro.span.start, first_macro.span.end));
                    }
                }
                // Last resort: point to file start
                return Some((0, 0));
            }
            return None;
        }

        // For .ts/.js files, look up export_signatures
        if let Some(sig) = entry
            .export_signatures
            .iter()
            .find(|s| s.name == binding_name)
        {
            if sig.span.start > 0 || sig.span.end > 0 {
                return Some((sig.span.start, sig.span.end));
            }
        }

        None
    }

    /// Follow re-exports to find the ultimate definition span.
    ///
    /// For a re-export like `export { default as Popup } from './Popup.vue'`,
    /// this follows the chain to find where `Popup` is actually defined.
    /// Returns `(canonical_id, start, end)` of the final definition.
    ///
    /// Uses cycle detection (visited set keyed on `(canonical_id, binding_name)`)
    /// instead of a depth counter. For local exports (no re-export), returns the
    /// span in the same file.
    pub fn get_export_span_follow_reexports(
        &self,
        canonical_or_alias: &str,
        binding_name: &str,
    ) -> Option<(String, u32, u32)> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let files = read_lock(&self.files);
        let alias_map = read_lock(&self.alias_to_canonical);
        let resolver = read_lock(&self.project_resolver);
        let mut visited = rustc_hash::FxHashSet::default();

        self.follow_reexport_chain(
            &files,
            &alias_map,
            resolver.as_ref(),
            &canonical,
            binding_name,
            &mut visited,
        )
    }

    /// Internal recursive helper for following re-export chains.
    /// Uses a visited set keyed on `(canonical_id, binding_name)` to detect cycles.
    fn follow_reexport_chain(
        &self,
        files: &rustc_hash::FxHashMap<String, crate::FileEntry>,
        alias_map: &rustc_hash::FxHashMap<String, String>,
        project_resolver: Option<&NativeProjectResolver>,
        canonical_id: &str,
        binding_name: &str,
        visited: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<(String, u32, u32)> {
        // Cycle detection: if we've seen this (file, binding) pair before, stop
        if !visited.insert((canonical_id.to_string(), binding_name.to_string())) {
            return None;
        }

        let entry = files.get(canonical_id)?;

        // For Vue SFCs, resolve directly (they don't have re-exports in export_signatures)
        if entry.file_kind == crate::FileKind::VueSfc {
            // Check bindings
            if let Some(binding) = entry
                .script_analysis
                .bindings
                .iter()
                .find(|b| b.name == binding_name)
            {
                if binding.span.start > 0 || binding.span.end > 0 {
                    return Some((
                        canonical_id.to_string(),
                        binding.span.start,
                        binding.span.end,
                    ));
                }
            }
            // Check macro bindings
            for mac in &entry.script_analysis.macros {
                if mac.binding_name.as_deref() == Some(binding_name)
                    && (mac.span.start > 0 || mac.span.end > 0)
                {
                    return Some((canonical_id.to_string(), mac.span.start, mac.span.end));
                }
            }
            // "default" export → first binding, then first macro, then file start
            if binding_name == "default" {
                if let Some(first_binding) = entry.script_analysis.bindings.first() {
                    if first_binding.span.start > 0 || first_binding.span.end > 0 {
                        return Some((
                            canonical_id.to_string(),
                            first_binding.span.start,
                            first_binding.span.end,
                        ));
                    }
                }
                // No bindings — try first macro (e.g., `defineProps<...>()` without assignment)
                if let Some(first_macro) = entry.script_analysis.macros.first() {
                    if first_macro.span.start > 0 || first_macro.span.end > 0 {
                        return Some((
                            canonical_id.to_string(),
                            first_macro.span.start,
                            first_macro.span.end,
                        ));
                    }
                }
                // Last resort: point to file start (the SFC itself IS the default export)
                return Some((canonical_id.to_string(), 0, 0));
            }
            return None;
        }

        // For .ts/.js files, look up export_signatures
        if let Some(sig) = entry
            .export_signatures
            .iter()
            .find(|s| s.name == binding_name)
        {
            // If it's a re-export, follow the chain
            if let (Some(ref source), Some(ref local_name)) =
                (&sig.reexport_source, &sig.reexport_local)
            {
                // Resolve the source module to a canonical ID
                if let Some(target_canonical) = crate::cross_file::resolve_import_to_canonical(
                    files,
                    alias_map,
                    project_resolver,
                    entry,
                    source,
                ) {
                    return self.follow_reexport_chain(
                        files,
                        alias_map,
                        project_resolver,
                        &target_canonical,
                        local_name,
                        visited,
                    );
                }
                // Can't resolve source → stop
                return None;
            }

            // Local export — return span in this file
            if sig.span.start > 0 || sig.span.end > 0 {
                return Some((canonical_id.to_string(), sig.span.start, sig.span.end));
            }
        }

        None
    }

    /// Resolve an import specifier to its canonical ID using the host's file map,
    /// alias map, and parent's resolved dependencies.
    ///
    /// Returns `None` if the import cannot be resolved to a known file
    /// (e.g., bare specifiers like `vue` or unregistered files).
    pub fn resolve_import(&self, parent_canonical_id: &str, import_source: &str) -> Option<String> {
        let files = read_lock(&self.files);
        let alias_map = read_lock(&self.alias_to_canonical);
        let resolver = read_lock(&self.project_resolver);
        let entry = files.get(parent_canonical_id)?;
        crate::cross_file::resolve_import_to_canonical(
            &files,
            &alias_map,
            resolver.as_ref(),
            entry,
            import_source,
        )
    }

    /// Returns all exports of a file, following re-export chains to their ultimate source.
    ///
    /// For barrel files like `export { default as Button } from './Button.vue'`, this
    /// resolves through the chain to return the ultimate source file and name. For
    /// `export * from './module'`, it recursively resolves the target file's exports.
    ///
    /// Uses cycle detection to prevent infinite loops in circular re-exports.
    pub fn resolve_exports(&self, canonical_or_alias: &str) -> Vec<ResolvedExport> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let files = read_lock(&self.files);
        let alias_map = read_lock(&self.alias_to_canonical);
        let resolver = read_lock(&self.project_resolver);
        let mut visiting = rustc_hash::FxHashSet::default();
        self.collect_resolved_exports(
            &files,
            &alias_map,
            resolver.as_ref(),
            &canonical,
            &mut visiting,
        )
    }

    /// Recursively collect resolved exports from a file, following re-export chains.
    fn collect_resolved_exports(
        &self,
        files: &rustc_hash::FxHashMap<String, crate::FileEntry>,
        alias_map: &rustc_hash::FxHashMap<String, String>,
        project_resolver: Option<&NativeProjectResolver>,
        canonical_id: &str,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Vec<ResolvedExport> {
        // Cycle detection
        if !visiting.insert(canonical_id.to_string()) {
            return Vec::new();
        }

        let Some(entry) = files.get(canonical_id) else {
            visiting.remove(canonical_id);
            return Vec::new();
        };

        let mut results = Vec::new();

        // For Vue SFCs, export "default" pointing to the first binding
        if entry.file_kind == crate::FileKind::VueSfc {
            results.push(ResolvedExport {
                name: "default".to_string(),
                is_type: false,
                source_canonical_id: None,
                source_name: "default".to_string(),
            });
            visiting.remove(canonical_id);
            return results;
        }

        // For .ts/.js files, iterate export_signatures
        for sig in &entry.export_signatures {
            if sig.name == "*" {
                // Wildcard re-export: export * from './module'
                if let Some(ref source) = sig.reexport_source {
                    if let Some(target) = crate::cross_file::resolve_import_to_canonical(
                        files,
                        alias_map,
                        project_resolver,
                        entry,
                        source,
                    ) {
                        let nested = self.collect_resolved_exports(
                            files,
                            alias_map,
                            project_resolver,
                            &target,
                            visiting,
                        );
                        for mut export in nested {
                            // Trace the source through to the ultimate origin
                            if export.source_canonical_id.is_none() {
                                export.source_canonical_id = Some(target.clone());
                            }
                            results.push(export);
                        }
                    }
                }
                continue;
            }

            if let (Some(ref source), Some(ref local_name)) =
                (&sig.reexport_source, &sig.reexport_local)
            {
                // Named re-export: follow chain to find ultimate source
                if let Some(target) = crate::cross_file::resolve_import_to_canonical(
                    files,
                    alias_map,
                    project_resolver,
                    entry,
                    source,
                ) {
                    let resolved = self.resolve_single_export(
                        files,
                        alias_map,
                        project_resolver,
                        &target,
                        local_name,
                        visiting,
                    );
                    let (src_id, src_name) = match resolved {
                        Some((cid, n)) => (Some(cid), n),
                        None => (Some(target.clone()), local_name.clone()),
                    };
                    results.push(ResolvedExport {
                        name: sig.name.clone(),
                        is_type: sig.is_type,
                        source_canonical_id: src_id,
                        source_name: src_name,
                    });
                } else {
                    // Can't resolve target — include as unresolved re-export
                    results.push(ResolvedExport {
                        name: sig.name.clone(),
                        is_type: sig.is_type,
                        source_canonical_id: None,
                        source_name: local_name.clone(),
                    });
                }
            } else {
                // Local export
                results.push(ResolvedExport {
                    name: sig.name.clone(),
                    is_type: sig.is_type,
                    source_canonical_id: None,
                    source_name: sig.name.clone(),
                });
            }
        }

        visiting.remove(canonical_id);
        results
    }

    /// Follow a re-export chain for a single named export.
    /// Returns (ultimate_canonical_id, ultimate_name) or None if unresolvable.
    fn resolve_single_export(
        &self,
        files: &rustc_hash::FxHashMap<String, crate::FileEntry>,
        alias_map: &rustc_hash::FxHashMap<String, String>,
        project_resolver: Option<&NativeProjectResolver>,
        canonical_id: &str,
        name: &str,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Option<(String, String)> {
        let entry = files.get(canonical_id)?;

        // Vue SFCs — always resolve here
        if entry.file_kind == crate::FileKind::VueSfc {
            return Some((canonical_id.to_string(), name.to_string()));
        }

        // Look up in export_signatures
        let sig = entry.export_signatures.iter().find(|s| s.name == name)?;

        if let (Some(ref source), Some(ref local)) = (&sig.reexport_source, &sig.reexport_local) {
            // Another re-export — follow if no cycle
            if visiting.contains(canonical_id) {
                return Some((canonical_id.to_string(), name.to_string()));
            }
            visiting.insert(canonical_id.to_string());
            let target = crate::cross_file::resolve_import_to_canonical(
                files,
                alias_map,
                project_resolver,
                entry,
                source,
            );
            visiting.remove(canonical_id);

            if let Some(target_id) = target {
                self.resolve_single_export(
                    files,
                    alias_map,
                    project_resolver,
                    &target_id,
                    local,
                    visiting,
                )
                .or(Some((target_id, local.clone())))
            } else {
                Some((canonical_id.to_string(), name.to_string()))
            }
        } else {
            // Local — found the ultimate source
            Some((canonical_id.to_string(), name.to_string()))
        }
    }
}

/// Convert `ResolvedElements` props to an expanded type text string.
///
/// Produces `"{ name: type; name2?: type2 }"` format matching `build_expanded_type_text`
/// in `verter_analysis::macros`.
fn resolved_elements_to_expanded_text(
    resolved: &verter_core::utils::oxc::vue::resolve_type::ResolvedElements,
    source: &str,
) -> String {
    let source_bytes = source.as_bytes();
    let mut parts = Vec::new();
    for prop in &resolved.props {
        let name = prop.key_name.as_deref().unwrap_or_else(|| {
            let start = prop.key.start as usize;
            let end = prop.key.end as usize;
            if end <= source_bytes.len() {
                std::str::from_utf8(&source_bytes[start..end]).unwrap_or("unknown")
            } else {
                "unknown"
            }
        });
        let opt = if prop.optional { "?" } else { "" };
        let ty = if let Some(type_span) = prop.type_span {
            let start = type_span.start as usize;
            let end = type_span.end as usize;
            if end <= source_bytes.len() {
                std::str::from_utf8(&source_bytes[start..end]).unwrap_or("unknown")
            } else {
                "unknown"
            }
        } else {
            "unknown"
        };
        parts.push(format!("{}{}: {}", name, opt, ty));
    }
    format!("{{ {} }}", parts.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    use std::sync::Arc;

    const LAZY_ANALYSIS_SFC: &str = r#"<template><div>{{ msg }}</div></template>
<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>
<style>
.foo { color: red; }
</style>"#;

    fn make_host() -> VerterHost {
        VerterHost::new_standalone(HostConfig::default())
    }

    fn make_lazy_host() -> VerterHost {
        VerterHost::new_standalone(HostConfig {
            analysis_level: AnalysisLevel::None,
            ..HostConfig::default()
        })
    }

    fn upsert_vue(host: &VerterHost, id: &str, src: &str) {
        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .unwrap();
    }

    fn mutate_lazy_analysis_source(host: &VerterHost) {
        let mut files = crate::shared::write_lock(&host.files);
        let entry = files.get_mut("App.vue").expect("App.vue should exist");
        let broken = entry
            .source
            .replace("<script", "<scripx")
            .replace("</script>", "</scripx>")
            .replace("<style", "<styla")
            .replace("</style>", "</styla>");
        entry.source = Arc::from(broken);
    }

    fn clear_cached_parse(host: &VerterHost) {
        let mut files = crate::shared::write_lock(&host.files);
        let entry = files.get_mut("App.vue").expect("App.vue should exist");
        entry.cached_parse = None;
    }

    /// @ai-generated - get_analysis populates resolved_canonical_id for relative imports
    #[test]
    fn get_analysis_resolves_relative_import() {
        let host = make_host();
        upsert_vue(
            &host,
            "Child.vue",
            "<script setup>\ndefineProps({ msg: String })\n</script>\n<template><div>{{ msg }}</div></template>",
        );
        upsert_vue(
            &host,
            "Parent.vue",
            "<script setup>\nimport Child from './Child.vue'\n</script>\n<template><Child msg=\"hello\" /></template>",
        );

        let analysis = host.get_analysis("Parent.vue").unwrap();
        let child_import = analysis
            .imports
            .iter()
            .find(|i| i.source == "./Child.vue")
            .unwrap();
        assert_eq!(
            child_import.resolved_canonical_id.as_deref(),
            Some("Child.vue"),
            "relative import should resolve to canonical ID"
        );
    }

    /// @ai-generated - get_analysis resolves imports via alias map
    #[test]
    fn get_analysis_resolves_alias_import() {
        let host = make_host();
        upsert_vue(
            &host,
            "/project/src/components/Child.vue",
            "<script setup>\ndefineProps({ msg: String })\n</script>\n<template><div/></template>",
        );
        upsert_vue(
            &host,
            "/project/src/App.vue",
            "<script setup>\nimport Child from '@/components/Child.vue'\n</script>\n<template><Child/></template>",
        );
        // Register alias
        {
            let mut aliases = crate::shared::write_lock(&host.alias_to_canonical);
            aliases.insert(
                "@/components/Child.vue".to_string(),
                "/project/src/components/Child.vue".to_string(),
            );
        }

        let analysis = host.get_analysis("/project/src/App.vue").unwrap();
        let child_import = analysis
            .imports
            .iter()
            .find(|i| i.source == "@/components/Child.vue")
            .unwrap();
        assert_eq!(
            child_import.resolved_canonical_id.as_deref(),
            Some("/project/src/components/Child.vue"),
            "alias import should resolve via alias map"
        );
    }

    /// @ai-generated - get_analysis resolves imports with extension guessing
    #[test]
    fn get_analysis_resolves_extension_guessing() {
        let host = make_host();
        upsert_vue(
            &host,
            "Child.vue",
            "<script setup>\n</script>\n<template><div/></template>",
        );
        upsert_vue(
            &host,
            "Parent.vue",
            "<script setup>\nimport Child from './Child'\n</script>\n<template><Child/></template>",
        );

        let analysis = host.get_analysis("Parent.vue").unwrap();
        let child_import = analysis
            .imports
            .iter()
            .find(|i| i.source == "./Child")
            .unwrap();
        assert_eq!(
            child_import.resolved_canonical_id.as_deref(),
            Some("Child.vue"),
            "extension-less import should resolve via .vue guessing"
        );
    }

    /// @ai-generated - get_analysis leaves bare specifiers unresolved
    #[test]
    fn get_analysis_bare_specifier_unresolved() {
        let host = make_host();
        upsert_vue(
            &host,
            "App.vue",
            "<script setup>\nimport { ref } from 'vue'\n</script>\n<template><div/></template>",
        );

        let analysis = host.get_analysis("App.vue").unwrap();
        let vue_import = analysis.imports.iter().find(|i| i.source == "vue").unwrap();
        assert!(
            vue_import.resolved_canonical_id.is_none(),
            "bare specifier 'vue' should not resolve (no node_modules resolution)"
        );
    }

    /// @ai-generated - get_analysis leaves unregistered file imports unresolved
    #[test]
    fn get_analysis_missing_file_unresolved() {
        let host = make_host();
        upsert_vue(
            &host,
            "App.vue",
            "<script setup>\nimport Missing from './Missing.vue'\n</script>\n<template><div/></template>",
        );

        let analysis = host.get_analysis("App.vue").unwrap();
        let missing_import = analysis
            .imports
            .iter()
            .find(|i| i.source == "./Missing.vue")
            .unwrap();
        assert!(
            missing_import.resolved_canonical_id.is_none(),
            "import of unregistered file should not resolve"
        );
    }

    #[test]
    fn get_analysis_uses_cached_parse_for_lazy_analysis() {
        let host = make_lazy_host();
        upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);
        mutate_lazy_analysis_source(&host);

        let analysis = host.get_analysis("App.vue").unwrap();

        assert!(
            analysis.bindings.iter().any(|b| b.name == "msg"),
            "lazy script analysis should reuse cached parse for bindings"
        );
        assert_eq!(
            analysis.styles.len(),
            1,
            "lazy style analysis should reuse cached parse for style blocks"
        );
        let css = analysis.styles[0]
            .css
            .as_ref()
            .expect("CSS analysis should exist for cached style block");
        assert!(
            css.classes.iter().any(|class| class.name == "foo"),
            "lazy style analysis should preserve CSS classes"
        );
        assert!(
            analysis
                .module_references
                .iter()
                .any(|reference| reference.literal_specifier.as_deref() == Some("vue")),
            "lazy script analysis should preserve module references"
        );
    }

    #[test]
    fn get_analysis_falls_back_when_cached_parse_missing() {
        let host = make_lazy_host();
        upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);
        clear_cached_parse(&host);

        let analysis = host.get_analysis("App.vue").unwrap();

        assert!(
            analysis.bindings.iter().any(|b| b.name == "msg"),
            "source fallback should still recover bindings"
        );
        assert_eq!(
            analysis.styles.len(),
            1,
            "source fallback should still recover style blocks"
        );
        let css = analysis.styles[0]
            .css
            .as_ref()
            .expect("CSS analysis should exist for fallback style block");
        assert!(
            css.classes.iter().any(|class| class.name == "foo"),
            "source fallback should preserve CSS classes"
        );
        assert!(
            analysis
                .module_references
                .iter()
                .any(|reference| reference.literal_specifier.as_deref() == Some("vue")),
            "source fallback should preserve module references"
        );
    }

    /// @ai-generated - get_export_span for .vue file returns binding span
    #[test]
    fn get_export_span_vue_binding() {
        let host = make_host();
        upsert_vue(
            &host,
            "Child.vue",
            "<script setup>\nconst msg = 'hello'\n</script>\n<template><div/></template>",
        );

        let span = host.get_export_span("Child.vue", "msg");
        assert!(span.is_some(), "should find 'msg' binding in .vue file");
        let (start, end) = span.unwrap();
        let source = host.get_source("Child.vue").unwrap();
        let spanned = &source[start as usize..end as usize];
        assert_eq!(spanned, "msg", "span should cover the binding identifier");
    }

    /// @ai-generated - get_export_span for .vue file returns None for unknown binding
    #[test]
    fn get_export_span_vue_unknown_binding() {
        let host = make_host();
        upsert_vue(
            &host,
            "Child.vue",
            "<script setup>\nconst msg = 'hello'\n</script>\n<template><div/></template>",
        );

        assert!(
            host.get_export_span("Child.vue", "nonexistent").is_none(),
            "unknown binding should return None"
        );
    }

    /// @ai-generated - get_export_span for .ts file returns export signature span
    #[test]
    fn get_export_span_ts_file() {
        let host = make_host();
        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: "utils.ts".to_string(),
            source: Arc::from("export function helper() { return 1; }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

        let span = host.get_export_span("utils.ts", "helper");
        assert!(span.is_some(), "should find 'helper' export in .ts file");
        let (start, end) = span.unwrap();
        let source = host.get_source("utils.ts").unwrap();
        let spanned = &source[start as usize..end as usize];
        assert_eq!(
            spanned, "helper",
            "span should cover the function identifier"
        );
    }

    /// @ai-generated - get_export_span for .vue default import finds first binding
    #[test]
    fn get_export_span_vue_default() {
        let host = make_host();
        upsert_vue(
            &host,
            "Child.vue",
            "<script setup>\nconst msg = 'hello'\n</script>\n<template><div/></template>",
        );

        let span = host.get_export_span("Child.vue", "default");
        assert!(
            span.is_some(),
            "default export of .vue should resolve to first binding"
        );
    }

    /// @ai-generated - resolve_import public method works
    #[test]
    fn resolve_import_public_method() {
        let host = make_host();
        upsert_vue(&host, "Child.vue", "<template><div/></template>");
        upsert_vue(
            &host,
            "Parent.vue",
            "<script setup>\nimport Child from './Child.vue'\n</script>\n<template><Child/></template>",
        );

        assert_eq!(
            host.resolve_import("Parent.vue", "./Child.vue").as_deref(),
            Some("Child.vue")
        );
        // Bare specifiers that aren't in the file map resolve to None
        assert!(host.resolve_import("Parent.vue", "lodash").is_none());
    }

    #[test]
    fn resolve_import_public_method_handles_relative_full_paths() {
        let host = make_host();
        upsert_vue(
            &host,
            "/project/src/components/BarrelComp.vue",
            "<script setup>\nconst emit = defineEmits<{ custom: [] }>()\n</script>\n",
        );
        upsert_ts(
            &host,
            "/project/src/components/index.ts",
            "export { default as BarrelComp } from './BarrelComp.vue'",
        );
        upsert_vue(
            &host,
            "/project/src/App.vue",
            "<script setup>\nimport { BarrelComp } from './components'\n</script>\n<template><BarrelComp /></template>",
        );

        assert_eq!(
            host.resolve_import("/project/src/components/index.ts", "./BarrelComp.vue")
                .as_deref(),
            Some("/project/src/components/BarrelComp.vue"),
            "relative imports from full-path barrel files should resolve to the child SFC"
        );
    }

    fn upsert_ts(host: &VerterHost, id: &str, src: &str) {
        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();
    }

    #[test]
    fn enriches_destructured_composable_bindings() {
        let host = make_host();

        // Composable that returns { x: ref, y: ref, reset: function }
        upsert_ts(
            &host,
            "useMouse.ts",
            r#"
import { ref } from 'vue'
export function useMouse() {
    const x = ref(0)
    const y = ref(0)
    function reset() { x.value = 0; y.value = 0 }
    return { x, y, reset }
}
"#,
        );

        // SFC that destructures the composable return
        upsert_vue(
            &host,
            "App.vue",
            r#"<script setup>
import { useMouse } from './useMouse.ts'
const { x, y, reset } = useMouse()
</script>
<template><div>{{ x }} {{ y }}</div></template>"#,
        );

        let analysis = host.get_analysis("App.vue").unwrap();

        // x and y should be enriched to Ref (from composable return shape)
        let x_binding = analysis.bindings.iter().find(|b| b.name == "x").unwrap();
        assert_eq!(
            x_binding.reactivity_kind,
            verter_analysis::ReactivityKind::Ref,
            "x should be enriched from MaybeRef to Ref via composable return shape"
        );

        let y_binding = analysis.bindings.iter().find(|b| b.name == "y").unwrap();
        assert_eq!(
            y_binding.reactivity_kind,
            verter_analysis::ReactivityKind::Ref,
            "y should be enriched from MaybeRef to Ref via composable return shape"
        );

        // reset should stay as a function (ReactivityKind::None since it's not reactive)
        let reset_binding = analysis
            .bindings
            .iter()
            .find(|b| b.name == "reset")
            .unwrap();
        assert_eq!(
            reset_binding.reactivity_kind,
            verter_analysis::ReactivityKind::None,
            "reset (a function) should be None, not reactive"
        );

        // Negative: non-enriched bindings should not be affected
        assert!(
            !x_binding.is_reactive
                || x_binding.reactivity_kind != verter_analysis::ReactivityKind::MaybeRef,
            "x should NOT remain MaybeRef after enrichment"
        );
    }

    #[test]
    fn get_export_span_follows_reexport_to_vue() {
        let host = make_host();

        // Target: Popup.vue with a binding
        upsert_vue(
            &host,
            "Popup.vue",
            "<script setup>\nconst message = 'hello'\n</script>\n<template><div>{{ message }}</div></template>",
        );

        // Barrel: index.ts re-exports Popup.vue as default
        upsert_ts(
            &host,
            "index.ts",
            "export { default as Popup } from './Popup.vue'",
        );

        // Follow the re-export: "Popup" in index.ts → default in Popup.vue
        let result = host.get_export_span_follow_reexports("index.ts", "Popup");

        assert!(result.is_some(), "should follow re-export to Popup.vue");
        let (canonical_id, start, end) = result.unwrap();
        assert_eq!(
            canonical_id, "Popup.vue",
            "should resolve to Popup.vue canonical ID"
        );
        assert!(
            start < end,
            "should have a valid span in Popup.vue (start={start}, end={end})"
        );
        // Negative: should NOT return index.ts
        assert_ne!(
            canonical_id, "index.ts",
            "must NOT return the barrel file itself"
        );
    }

    #[test]
    fn get_export_span_follows_reexport_to_vue_full_paths() {
        let host = make_host();

        upsert_vue(
            &host,
            "/project/src/components/BarrelComp.vue",
            "<script setup>\nconst emit = defineEmits<{ custom: [] }>()\n</script>\n",
        );
        upsert_ts(
            &host,
            "/project/src/components/index.ts",
            "export { default as BarrelComp } from './BarrelComp.vue'",
        );

        let result =
            host.get_export_span_follow_reexports("/project/src/components/index.ts", "BarrelComp");

        assert!(
            result.is_some(),
            "should follow full-path barrel re-export to BarrelComp.vue"
        );
        let (canonical_id, start, end) = result.unwrap();
        assert_eq!(
            canonical_id, "/project/src/components/BarrelComp.vue",
            "should resolve to the full child Vue canonical ID"
        );
        assert!(start < end, "should return a valid span in BarrelComp.vue");
    }

    #[test]
    fn get_export_span_follows_named_reexport() {
        let host = make_host();

        // Target: utils.ts with an exported function
        upsert_ts(&host, "utils.ts", "export function helper() { return 42 }");

        // Barrel: re-exports helper as myHelper
        upsert_ts(
            &host,
            "index.ts",
            "export { helper as myHelper } from './utils.ts'",
        );

        let result = host.get_export_span_follow_reexports("index.ts", "myHelper");

        assert!(result.is_some(), "should follow named re-export");
        let (canonical_id, start, end) = result.unwrap();
        assert_eq!(canonical_id, "utils.ts", "should resolve to utils.ts");
        assert!(start < end, "should have a valid span");
        // Negative: should NOT return barrel
        assert_ne!(canonical_id, "index.ts");
    }

    #[test]
    fn get_export_span_follows_multi_hop_chain() {
        let host = make_host();

        upsert_ts(&host, "a.ts", "export { b } from './b.ts'");
        upsert_ts(&host, "b.ts", "export { c as b } from './c.ts'");
        upsert_ts(&host, "c.ts", "export const c = 42");

        // Should follow a→b→c (no depth limit, cycle detection only)
        let result = host.get_export_span_follow_reexports("a.ts", "b");
        assert!(result.is_some(), "should follow the chain");
        let (canonical_id, _, _) = result.unwrap();
        assert_eq!(canonical_id, "c.ts", "should reach c.ts");
    }

    #[test]
    fn get_export_span_local_export_unchanged() {
        let host = make_host();

        upsert_ts(&host, "utils.ts", "export function foo() { return 1 }");

        // Local export — no re-export, returns span in same file
        let result = host.get_export_span_follow_reexports("utils.ts", "foo");

        assert!(result.is_some(), "should find local export");
        let (canonical_id, start, end) = result.unwrap();
        assert_eq!(
            canonical_id, "utils.ts",
            "local export should return same file"
        );
        assert!(start < end, "should have a valid span");
    }

    #[test]
    fn follow_reexport_cycle_same_binding() {
        let host = make_host();

        // A re-exports foo from B, B re-exports foo from A → cycle
        upsert_ts(&host, "a.ts", "export { foo } from './b.ts'");
        upsert_ts(&host, "b.ts", "export { foo } from './a.ts'");

        let result = host.get_export_span_follow_reexports("a.ts", "foo");
        assert!(
            result.is_none(),
            "cycle on same binding should return None, got: {result:?}"
        );
    }

    #[test]
    fn follow_reexport_same_file_different_binding() {
        let host = make_host();

        // A re-exports foo from B (as foo→bar), B re-exports bar from A (as bar→baz),
        // A has a local baz export. Different bindings each hop → not a cycle.
        upsert_ts(
            &host,
            "a.ts",
            "export { bar as foo } from './b.ts'\nexport const baz = 99",
        );
        upsert_ts(&host, "b.ts", "export { baz as bar } from './a.ts'");

        let result = host.get_export_span_follow_reexports("a.ts", "foo");
        assert!(
            result.is_some(),
            "different bindings through same files should resolve, not be treated as cycle"
        );
        let (canonical_id, _, _) = result.unwrap();
        assert_eq!(
            canonical_id, "a.ts",
            "should resolve to a.ts local baz export"
        );
    }

    #[test]
    fn follow_reexport_indirect_cycle() {
        let host = make_host();

        // A→B→C→A with same binding name "x" at each hop
        upsert_ts(&host, "a.ts", "export { x } from './b.ts'");
        upsert_ts(&host, "b.ts", "export { x } from './c.ts'");
        upsert_ts(&host, "c.ts", "export { x } from './a.ts'");

        let result = host.get_export_span_follow_reexports("a.ts", "x");
        assert!(
            result.is_none(),
            "indirect 3-file cycle should return None, got: {result:?}"
        );
    }

    #[test]
    fn follow_reexport_deep_chain_no_limit() {
        let host = make_host();

        // 15-hop chain: f0→f1→f2→...→f14→terminal.ts
        // Each hop renames: val0→val1→...→val14→val
        for i in 0..15 {
            let next = if i < 14 {
                format!("f{}.ts", i + 1)
            } else {
                "terminal.ts".to_string()
            };
            let next_binding = if i < 14 {
                format!("val{}", i + 1)
            } else {
                "val".to_string()
            };
            let src = format!(
                "export {{ {} as val{} }} from './{}'",
                next_binding, i, next
            );
            upsert_ts(&host, &format!("f{}.ts", i), &src);
        }
        upsert_ts(&host, "terminal.ts", "export const val = 'done'");

        let result = host.get_export_span_follow_reexports("f0.ts", "val0");
        assert!(
            result.is_some(),
            "15-hop chain should resolve without depth limit"
        );
        let (canonical_id, start, end) = result.unwrap();
        assert_eq!(canonical_id, "terminal.ts", "should reach terminal.ts");
        assert!(start < end, "should have a valid span");
    }

    fn compile_template(host: &VerterHost, id: &str) {
        host.get_virtual_file(crate::types::VirtualQuery {
            raw_id: Some(format!("{id}?vue&type=template")),
            canonical_id: None,
            node_kind: None,
            compile_profile: crate::types::CompileProfile::default(),
        })
        .unwrap();
    }

    #[test]
    fn prop_shorthand_detected() {
        let host = make_host();
        upsert_vue(
            &host,
            "MyComp.vue",
            "<script setup>\ndefineProps<{ bar: number }>()\n</script>\n<template><div/></template>",
        );
        // `:bar` with no value → shorthand; `:bar="bar"` → not shorthand
        upsert_vue(
            &host,
            "App.vue",
            r#"<script setup>
import MyComp from './MyComp.vue'
const bar = 1
</script>
<template><MyComp :bar /><MyComp :bar="bar" /></template>"#,
        );
        compile_template(&host, "App.vue");

        let analysis = host.get_analysis("App.vue").unwrap();
        let tmpl = analysis
            .template
            .as_ref()
            .expect("should have template analysis");
        assert!(
            tmpl.components.len() >= 2,
            "should have at least 2 component usages, got {}",
            tmpl.components.len()
        );

        // First usage: `:bar` (shorthand)
        let comp1 = &tmpl.components[0];
        assert_eq!(comp1.props.len(), 1, "first usage has 1 prop");
        assert!(
            comp1.props[0].is_shorthand,
            "`:bar` (no value) should be shorthand"
        );

        // Second usage: `:bar="bar"` (not shorthand)
        let comp2 = &tmpl.components[1];
        assert_eq!(comp2.props.len(), 1, "second usage has 1 prop");
        assert!(
            !comp2.props[0].is_shorthand,
            "`:bar=\"bar\"` should NOT be shorthand"
        );
    }

    #[test]
    fn prop_name_span_covers_name() {
        let host = make_host();
        upsert_vue(
            &host,
            "MyComp.vue",
            "<script setup>\ndefineProps<{ bar: number }>()\n</script>\n<template><div/></template>",
        );
        let sfc = r#"<script setup>
import MyComp from './MyComp.vue'
const bar = 1
</script>
<template><MyComp :bar="bar" foo="static" /></template>"#;
        upsert_vue(&host, "App.vue", sfc);
        compile_template(&host, "App.vue");

        let analysis = host.get_analysis("App.vue").unwrap();
        let tmpl = analysis
            .template
            .as_ref()
            .expect("should have template analysis");
        assert!(!tmpl.components.is_empty());

        let comp = &tmpl.components[0];
        // Find the bound prop `:bar`
        let bound_prop = comp.props.iter().find(|p| p.name == "bar").unwrap();
        let source = host.get_source("App.vue").unwrap();
        let name_text =
            &source[bound_prop.name_span.start as usize..bound_prop.name_span.end as usize];
        assert_eq!(
            name_text, "bar",
            "name_span should cover 'bar' (the arg, not ':')"
        );
        assert!(
            bound_prop.name_span.start >= bound_prop.span.start,
            "name_span should be within the full prop span"
        );

        // Find the static prop `foo`
        let static_prop = comp.props.iter().find(|p| p.name == "foo").unwrap();
        let name_text =
            &source[static_prop.name_span.start as usize..static_prop.name_span.end as usize];
        assert_eq!(name_text, "foo", "static prop name_span should cover 'foo'");
        assert!(
            !static_prop.is_shorthand,
            "static prop should not be shorthand"
        );
    }

    #[test]
    fn arc_shared_fields_are_pointer_equal() {
        let host = make_host();
        upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);

        let a1 = host.get_analysis("App.vue").unwrap();
        let a2 = host.get_analysis("App.vue").unwrap();

        // Arc-shared fields should be pointer-equal between two calls
        // on the same unchanged file.
        assert!(
            Arc::ptr_eq(&a1.module_references, &a2.module_references),
            "module_references should be Arc-shared (pointer equal)"
        );
        assert!(
            Arc::ptr_eq(&a1.macros, &a2.macros),
            "macros should be Arc-shared (pointer equal)"
        );
        assert!(
            Arc::ptr_eq(&a1.styles, &a2.styles),
            "styles should be Arc-shared (pointer equal)"
        );
        assert!(
            Arc::ptr_eq(&a1.vue_api_calls, &a2.vue_api_calls),
            "vue_api_calls should be Arc-shared (pointer equal)"
        );
    }

    #[test]
    fn enriched_imports_do_not_affect_stored_data() {
        let host = make_host();
        upsert_vue(
            &host,
            "Child.vue",
            "<script setup>\nconst x = 1\n</script>\n<template><div/></template>",
        );
        upsert_vue(
            &host,
            "Parent.vue",
            "<script setup>\nimport Child from './Child.vue'\n</script>\n<template><Child/></template>",
        );

        // First call: enriches imports with resolved_canonical_id
        let a1 = host.get_analysis("Parent.vue").unwrap();
        assert!(
            a1.imports[0].resolved_canonical_id.is_some(),
            "enriched import should have resolved_canonical_id"
        );

        // Verify stored data is not mutated by checking that the
        // internal FileEntry's imports still have None
        {
            let files = crate::shared::read_lock(&host.files);
            let entry = files.get("Parent.vue").unwrap();
            assert!(
                entry.script_analysis.imports[0]
                    .resolved_canonical_id
                    .is_none(),
                "stored import should NOT be mutated by get_analysis enrichment"
            );
        }
    }

    #[test]
    fn get_analysis_batch_returns_all_existing() {
        let host = make_host();
        upsert_vue(
            &host,
            "A.vue",
            "<script setup>\nconst a = 1\n</script>\n<template><div/></template>",
        );
        upsert_vue(
            &host,
            "B.vue",
            "<script setup>\nconst b = 2\n</script>\n<template><div/></template>",
        );

        let results = host.get_analysis_batch(&["A.vue", "B.vue", "NonExistent.vue"]);
        assert_eq!(results.len(), 2, "should return only existing files");
        assert!(
            results.iter().any(|(id, _)| id == "A.vue"),
            "should contain A.vue"
        );
        assert!(
            results.iter().any(|(id, _)| id == "B.vue"),
            "should contain B.vue"
        );
        // Negative: should NOT contain non-existent
        assert!(
            !results.iter().any(|(id, _)| id == "NonExistent.vue"),
            "should not contain non-existent file"
        );
    }

    #[test]
    fn get_analysis_batch_matches_individual() {
        let host = make_host();
        upsert_vue(
            &host,
            "A.vue",
            "<script setup>\nimport { ref } from 'vue'\nconst x = ref(0)\n</script>\n<template><div/></template>",
        );

        let individual = host.get_analysis("A.vue").unwrap();
        let batch = host.get_analysis_batch(&["A.vue"]);
        assert_eq!(batch.len(), 1);
        let (_, batch_snap) = &batch[0];

        assert_eq!(
            individual.bindings.len(),
            batch_snap.bindings.len(),
            "batch bindings count should match individual"
        );
        assert_eq!(
            individual.imports.len(),
            batch_snap.imports.len(),
            "batch imports count should match individual"
        );
        assert_eq!(
            individual.script_flags, batch_snap.script_flags,
            "batch script_flags should match individual"
        );
    }

    #[test]
    fn get_analysis_batch_empty_returns_empty() {
        let host = make_host();
        let results = host.get_analysis_batch(&[]);
        assert!(results.is_empty(), "empty batch should return empty vec");
    }

    // ── Export signature tests ──────────────────────────────────────

    fn upsert_ts_result(host: &VerterHost, id: &str, src: &str) -> crate::HostUpdateResult {
        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap()
    }

    /// @ai-generated - upsert of .ts file returns export signatures
    #[test]
    fn upsert_returns_export_signatures_for_ts() {
        let host = make_host();
        let result = upsert_ts_result(
            &host,
            "index.ts",
            r#"export const foo = 1;
export type Bar = string;
export { default as Button } from './Button.vue';
"#,
        );

        assert!(
            !result.export_signatures.is_empty(),
            "upsert should return export signatures for .ts files"
        );

        let foo_sig = result
            .export_signatures
            .iter()
            .find(|s| s.name == "foo")
            .expect("should have 'foo' export");
        assert!(!foo_sig.is_type, "foo is a value export");
        assert!(
            foo_sig.reexport_source.is_none(),
            "foo is local, not a re-export"
        );

        let bar_sig = result
            .export_signatures
            .iter()
            .find(|s| s.name == "Bar")
            .expect("should have 'Bar' export");
        assert!(bar_sig.is_type, "Bar is a type export");

        let button_sig = result
            .export_signatures
            .iter()
            .find(|s| s.name == "Button")
            .expect("should have 'Button' re-export");
        assert_eq!(
            button_sig.reexport_source.as_deref(),
            Some("./Button.vue"),
            "Button re-export source should be './Button.vue'"
        );
        assert_eq!(
            button_sig.reexport_local.as_deref(),
            Some("default"),
            "Button re-export local name should be 'default'"
        );
    }

    /// @ai-generated - get_analysis includes export signatures
    #[test]
    fn get_analysis_includes_export_signatures() {
        let host = make_host();
        upsert_ts(
            &host,
            "utils.ts",
            "export function helper() { return 1; }\nexport type Util = number;",
        );

        let analysis = host.get_analysis("utils.ts").unwrap();
        assert!(
            !analysis.export_signatures.is_empty(),
            "analysis should include export signatures"
        );

        let helper_sig = analysis
            .export_signatures
            .iter()
            .find(|s| s.name == "helper")
            .expect("should have 'helper' export");
        assert!(!helper_sig.is_type);

        let util_sig = analysis
            .export_signatures
            .iter()
            .find(|s| s.name == "Util")
            .expect("should have 'Util' export");
        assert!(util_sig.is_type);
    }

    /// @ai-generated - resolve_exports follows re-export chains
    #[test]
    fn resolve_exports_follows_reexport_chains() {
        let host = make_host();

        upsert_vue(
            &host,
            "Button.vue",
            "<script setup>\ndefineProps({ label: String })\n</script>\n<template><button>{{ label }}</button></template>",
        );

        upsert_ts(
            &host,
            "components/index.ts",
            "export { default as Button } from './Button.vue';",
        );

        // Set up dependency so ./Button.vue resolves from components/index.ts
        host.set_import_dependencies(
            "components/index.ts",
            vec![crate::DependencyResolution {
                specifier: "./Button.vue".to_string(),
                resolved_canonical_id: Some("Button.vue".to_string()),
                possible_canonical_ids: vec![],
            }],
        );

        let exports = host.resolve_exports("components/index.ts");
        assert!(
            !exports.is_empty(),
            "barrel file should have resolved exports"
        );

        let button = exports
            .iter()
            .find(|e| e.name == "Button")
            .expect("should have 'Button' resolved export");
        assert_eq!(
            button.source_canonical_id.as_deref(),
            Some("Button.vue"),
            "Button should resolve to Button.vue"
        );
        assert_eq!(
            button.source_name, "default",
            "Button maps to 'default' in the source file"
        );
    }

    /// @ai-generated - resolve_exports handles direct local exports
    #[test]
    fn resolve_exports_local_exports() {
        let host = make_host();
        upsert_ts(
            &host,
            "utils.ts",
            "export const FOO = 1;\nexport type Bar = string;",
        );

        let exports = host.resolve_exports("utils.ts");
        assert_eq!(exports.len(), 2, "should have 2 exports");

        let foo = exports.iter().find(|e| e.name == "FOO").unwrap();
        assert!(
            foo.source_canonical_id.is_none(),
            "local export has no source file"
        );
        assert_eq!(foo.source_name, "FOO");
        assert!(!foo.is_type);

        let bar = exports.iter().find(|e| e.name == "Bar").unwrap();
        assert!(bar.is_type);
    }

    /// @ai-generated - resolve_exports handles wildcard re-exports
    #[test]
    fn resolve_exports_wildcard_reexports() {
        let host = make_host();

        upsert_ts(
            &host,
            "types.ts",
            "export type Foo = string;\nexport type Bar = number;",
        );
        upsert_ts(&host, "index.ts", "export * from './types';");

        host.set_import_dependencies(
            "index.ts",
            vec![crate::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("types.ts".to_string()),
                possible_canonical_ids: vec![],
            }],
        );

        let exports = host.resolve_exports("index.ts");
        assert!(
            exports.iter().any(|e| e.name == "Foo"),
            "wildcard re-export should include Foo"
        );
        assert!(
            exports.iter().any(|e| e.name == "Bar"),
            "wildcard re-export should include Bar"
        );

        let foo = exports.iter().find(|e| e.name == "Foo").unwrap();
        assert_eq!(
            foo.source_canonical_id.as_deref(),
            Some("types.ts"),
            "Foo should trace back to types.ts"
        );
    }

    /// @ai-generated - resolve_exports detects circular re-exports
    #[test]
    fn resolve_exports_circular_protection() {
        let host = make_host();

        upsert_ts(&host, "a.ts", "export * from './b';");
        upsert_ts(&host, "b.ts", "export * from './a';");

        host.set_import_dependencies(
            "a.ts",
            vec![crate::DependencyResolution {
                specifier: "./b".to_string(),
                resolved_canonical_id: Some("b.ts".to_string()),
                possible_canonical_ids: vec![],
            }],
        );
        host.set_import_dependencies(
            "b.ts",
            vec![crate::DependencyResolution {
                specifier: "./a".to_string(),
                resolved_canonical_id: Some("a.ts".to_string()),
                possible_canonical_ids: vec![],
            }],
        );

        // Should not infinite loop
        let exports = host.resolve_exports("a.ts");
        // The result is empty because both files only re-export each other with no local exports
        assert!(
            exports.is_empty(),
            "circular re-exports with no local exports should return empty"
        );
    }

    /// @ai-generated - resolve_exports multi-level barrel chain
    #[test]
    fn resolve_exports_multi_level_barrel() {
        let host = make_host();

        upsert_ts(&host, "deep.ts", "export const DEEP = 42;");
        upsert_ts(&host, "mid.ts", "export { DEEP } from './deep';");
        upsert_ts(&host, "top.ts", "export { DEEP } from './mid';");

        host.set_import_dependencies(
            "mid.ts",
            vec![crate::DependencyResolution {
                specifier: "./deep".to_string(),
                resolved_canonical_id: Some("deep.ts".to_string()),
                possible_canonical_ids: vec![],
            }],
        );
        host.set_import_dependencies(
            "top.ts",
            vec![crate::DependencyResolution {
                specifier: "./mid".to_string(),
                resolved_canonical_id: Some("mid.ts".to_string()),
                possible_canonical_ids: vec![],
            }],
        );

        let exports = host.resolve_exports("top.ts");
        let deep = exports
            .iter()
            .find(|e| e.name == "DEEP")
            .expect("should have DEEP");
        assert_eq!(
            deep.source_canonical_id.as_deref(),
            Some("deep.ts"),
            "should trace through two levels to deep.ts"
        );
    }

    #[test]
    fn get_semantic_hash_returns_hash_for_loaded_file() {
        let host = make_host();
        upsert_vue(&host, "App.vue", "<template><div>hi</div></template>");
        let hash = host.get_semantic_hash("App.vue");
        assert!(hash.is_some(), "loaded file should return a semantic hash");
        assert_ne!(hash.unwrap(), [0u8; 16], "hash should not be all zeros");
    }

    #[test]
    fn get_semantic_hash_returns_none_for_missing_file() {
        let host = make_host();
        assert!(
            host.get_semantic_hash("nonexistent.vue").is_none(),
            "missing file should return None"
        );
    }

    #[test]
    fn get_semantic_hash_changes_on_content_change() {
        let host = make_host();
        upsert_vue(&host, "App.vue", "<template><div>a</div></template>");
        let h1 = host.get_semantic_hash("App.vue").unwrap();
        upsert_vue(&host, "App.vue", "<template><div>b</div></template>");
        let h2 = host.get_semantic_hash("App.vue").unwrap();
        assert_ne!(h1, h2, "semantic hash should change when content changes");
    }

    #[test]
    fn resolve_imported_type_from_ts_dep() {
        let host = make_host();
        // Upsert the .ts type file
        upsert_ts(
            &host,
            "/types.ts",
            "export interface ButtonProps { label: string; size?: number }",
        );
        // Upsert the .vue file that imports from ./types
        upsert_vue(
            &host,
            "/Button.vue",
            r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script><template><div /></template>"#,
        );

        let types = host.resolve_imported_types("/Button.vue");
        assert_eq!(types.len(), 1, "should resolve one imported type");
        assert_eq!(types[0].name, "ButtonProps");
        assert!(
            types[0].expanded.contains("label"),
            "expanded should contain 'label', got: {}",
            types[0].expanded
        );
        assert!(
            types[0].expanded.contains("size"),
            "expanded should contain 'size', got: {}",
            types[0].expanded
        );
        // Negative: should not return empty or unresolved
        assert!(
            !types[0].expanded.is_empty(),
            "expanded should not be empty"
        );
    }

    #[test]
    fn resolve_imported_types_returns_empty_for_no_deps() {
        let host = make_host();
        upsert_vue(
            &host,
            "/Simple.vue",
            r#"<script setup lang="ts">
defineProps<{ count: number }>()
</script><template><div /></template>"#,
        );
        let types = host.resolve_imported_types("/Simple.vue");
        assert!(
            types.is_empty(),
            "should return empty when no imported types"
        );
    }
}

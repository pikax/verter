//! `impl VerterHost` — file management, analysis, and diagnostics methods.
//!
//! Contains [`VerterHost::remove`], [`VerterHost::get_analysis`],
//! [`VerterHost::get_diagnostics`], and [`VerterHost::set_import_dependencies`].

use std::sync::Arc;

use crate::hash::compile_profile_hash;
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
            drop(files);

            let script_analysis = if !scope.needs_script_analysis() {
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
        let Some(entry) = files.get(parent_canonical_id) else {
            return;
        };
        for import in &mut snapshot.imports {
            if import.resolved_canonical_id.is_none() {
                import.resolved_canonical_id = crate::cross_file::resolve_import_to_canonical(
                    &files,
                    &alias_map,
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

    /// Provide caller-resolved import dependency canonical IDs.
    /// Called after upsert() when the caller resolves non-relative import paths
    /// (tsconfig paths, vite aliases, etc.) using bundler/LSP resolution.
    pub fn set_import_dependencies(&self, canonical_or_alias: &str, resolved_deps: Vec<String>) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        // Single write lock to avoid TOCTOU race between read-read-write.
        let (old_deps, new_deps) = {
            let mut files = write_lock(&self.files);
            let Some(entry) = files.get_mut(&canonical) else {
                return;
            };
            let old_deps = entry.dependencies.clone();
            for dep in resolved_deps {
                entry.dependencies.insert(dep);
            }
            let new_deps = entry.dependencies.clone();
            (old_deps, new_deps)
        };

        self.update_reverse_deps(&canonical, &old_deps, &new_deps);
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
    /// `max_depth` limits recursion to prevent infinite loops on circular re-exports.
    /// For local exports (no re-export), returns the span in the same file.
    pub fn get_export_span_follow_reexports(
        &self,
        canonical_or_alias: &str,
        binding_name: &str,
        max_depth: u32,
    ) -> Option<(String, u32, u32)> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let files = read_lock(&self.files);
        let alias_map = read_lock(&self.alias_to_canonical);

        self.follow_reexport_chain(&files, &alias_map, &canonical, binding_name, max_depth)
    }

    /// Internal recursive helper for following re-export chains.
    fn follow_reexport_chain(
        &self,
        files: &std::collections::HashMap<String, crate::FileEntry>,
        alias_map: &std::collections::HashMap<String, String>,
        canonical_id: &str,
        binding_name: &str,
        remaining_depth: u32,
    ) -> Option<(String, u32, u32)> {
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
            // "default" export → first binding
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
            }
            return None;
        }

        // For .ts/.js files, look up export_signatures
        if let Some(sig) = entry
            .export_signatures
            .iter()
            .find(|s| s.name == binding_name)
        {
            // If it's a re-export and we have depth budget, follow the chain
            if let (Some(ref source), Some(ref local_name)) =
                (&sig.reexport_source, &sig.reexport_local)
            {
                if remaining_depth > 0 {
                    // Resolve the source module to a canonical ID
                    if let Some(target_canonical) = crate::cross_file::resolve_import_to_canonical(
                        files, alias_map, entry, source,
                    ) {
                        return self.follow_reexport_chain(
                            files,
                            alias_map,
                            &target_canonical,
                            local_name,
                            remaining_depth - 1,
                        );
                    }
                }
                // Can't follow further (no depth or unresolved source)
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
        let entry = files.get(parent_canonical_id)?;
        crate::cross_file::resolve_import_to_canonical(&files, &alias_map, entry, import_source)
    }
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
        VerterHost::new(HostConfig::default())
    }

    fn make_lazy_host() -> VerterHost {
        VerterHost::new(HostConfig {
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
        let result = host.get_export_span_follow_reexports("index.ts", "Popup", 5);

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

        let result = host.get_export_span_follow_reexports("index.ts", "myHelper", 5);

        assert!(result.is_some(), "should follow named re-export");
        let (canonical_id, start, end) = result.unwrap();
        assert_eq!(canonical_id, "utils.ts", "should resolve to utils.ts");
        assert!(start < end, "should have a valid span");
        // Negative: should NOT return barrel
        assert_ne!(canonical_id, "index.ts");
    }

    #[test]
    fn get_export_span_stops_at_max_depth() {
        let host = make_host();

        upsert_ts(&host, "a.ts", "export { b } from './b.ts'");
        upsert_ts(&host, "b.ts", "export { c as b } from './c.ts'");
        upsert_ts(&host, "c.ts", "export const c = 42");

        // max_depth=0 → should return None (can't follow any re-exports)
        let result = host.get_export_span_follow_reexports("a.ts", "b", 0);
        assert!(
            result.is_none(),
            "max_depth=0 should not follow any re-exports"
        );

        // max_depth=2 → should follow a→b→c
        let result = host.get_export_span_follow_reexports("a.ts", "b", 2);
        assert!(result.is_some(), "max_depth=2 should follow the chain");
        let (canonical_id, _, _) = result.unwrap();
        assert_eq!(canonical_id, "c.ts", "should reach c.ts");
    }

    #[test]
    fn get_export_span_local_export_unchanged() {
        let host = make_host();

        upsert_ts(&host, "utils.ts", "export function foo() { return 1 }");

        // Local export — no re-export, returns span in same file
        let result = host.get_export_span_follow_reexports("utils.ts", "foo", 5);

        assert!(result.is_some(), "should find local export");
        let (canonical_id, start, end) = result.unwrap();
        assert_eq!(
            canonical_id, "utils.ts",
            "local export should return same file"
        );
        assert!(start < end, "should have a valid span");
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
}

//! `impl VerterHost` — file management, analysis, and diagnostics methods.
//!
//! Contains [`VerterHost::remove`], [`VerterHost::get_analysis`],
//! [`VerterHost::get_diagnostics`], and [`VerterHost::set_import_dependencies`].

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
            let template = entry.template_analysis.clone();
            drop(files);

            let script_analysis = if !scope.needs_script_analysis() {
                crate::parse::build_script_analysis_from_source(&source)
            } else {
                // Script analysis was already computed during upsert
                stored_script
            };
            let style_analyses =
                crate::parse::build_style_analyses_from_source(&source, &canonical);
            let vue_api_calls = script_analysis.vue_api_calls.clone();
            let dom_query_calls = script_analysis.dom_query_calls.clone();
            let css_var_manipulations = script_analysis.css_var_manipulations.clone();
            let mut snapshot = FileAnalysisSnapshot {
                imports: script_analysis.imports,
                bindings: script_analysis.bindings,
                macros: script_analysis.macros,
                macro_type_deps: script_analysis.macro_type_deps,
                script_flags: script_analysis.flags.bits(),
                styles: style_analyses,
                template,
                vue_api_calls,
                dom_query_calls,
                css_var_manipulations,
            };
            self.resolve_snapshot_imports(&canonical, &mut snapshot);
            return Some(snapshot);
        }

        let mut snapshot = FileAnalysisSnapshot {
            imports: entry.script_analysis.imports.clone(),
            bindings: entry.script_analysis.bindings.clone(),
            macros: entry.script_analysis.macros.clone(),
            macro_type_deps: entry.script_analysis.macro_type_deps.clone(),
            script_flags: entry.script_analysis.flags.bits(),
            styles: entry.style_analyses.clone(),
            template: entry.template_analysis.clone(),
            vue_api_calls: entry.script_analysis.vue_api_calls.clone(),
            dom_query_calls: entry.script_analysis.dom_query_calls.clone(),
            css_var_manipulations: entry.script_analysis.css_var_manipulations.clone(),
        };
        // Drop the files lock before resolving (resolve_snapshot_imports acquires its own)
        drop(files);
        self.resolve_snapshot_imports(&canonical, &mut snapshot);
        Some(snapshot)
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
            for style in &entry.style_analyses {
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

    fn make_host() -> VerterHost {
        VerterHost::new(HostConfig::default())
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
}

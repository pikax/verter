//! `impl VerterHost` — resolve and virtual file retrieval methods.
//!
//! Contains [`VerterHost::resolve`], [`VerterHost::get_virtual_file`],
//! [`VerterHost::list_virtual_files`], and the internal [`VerterHost::compile_entry`]
//! helper that drives on-demand compilation.

use std::sync::Arc;

use rustc_hash::FxHashMap;

#[cfg(not(target_arch = "wasm32"))]
#[cfg(feature = "host_metrics")]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
#[cfg(feature = "host_metrics")]
use web_time::Instant;

use oxc_allocator::Allocator;
use verter_core::compile::CodegenOptions;
use verter_core::compile::{
    compile as compile_sfc, compile_from_parsed, format_import_specifier, VerterCompileOptions,
};

use crate::cache::enforce_profile_cap;
use crate::compile::{assemble_main_module, merge_external_sources};
use crate::hash::compile_profile_hash;
use crate::id::{parse_raw_id, render_ids, render_single_id};
use crate::shared::{read_lock, write_lock};
use crate::types::*;
use crate::VerterHost;

type ResolvedExternalTypes =
    rustc_hash::FxHashMap<String, verter_core::utils::oxc::vue::resolve_type::ResolvedElements>;

type ExternalTypeCache = rustc_hash::FxHashMap<
    (String, String),
    Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>,
>;

impl VerterHost {
    /// Expand a relative import specifier into all candidate canonical IDs.
    ///
    /// Given an owner file and a relative specifier (e.g. `./types`), returns
    /// a list of candidates: the direct path, then with each resolve extension,
    /// then `/index` variants. Used by pre-snapshot blocker hydration to probe
    /// the filesystem without a full resolver.
    pub fn expand_relative_candidates(
        &self,
        owner_canonical: &str,
        specifier: &str,
    ) -> Vec<String> {
        let direct = crate::id::resolve_external(owner_canonical, specifier);
        let mut candidates = vec![direct.clone()];
        for ext in &self.config.resolve_extensions {
            candidates.push(format!("{direct}{ext}"));
        }
        for ext in &self.config.resolve_extensions {
            candidates.push(format!("{direct}/index{ext}"));
        }
        candidates
    }

    pub(crate) fn resolve_loaded_dependency_canonical(
        &self,
        _files: &FxHashMap<String, FileEntry>,
        owner_canonical: &str,
        import_source: &str,
        kind: verter_vfs::ResolveRequestKind,
    ) -> Option<String> {
        self.ws()
            .resolve_import(
                owner_canonical,
                import_source,
                verter_vfs::ResolutionContext {
                    phase: verter_vfs::ResolvePhase::CodegenBlocker,
                    kind,
                },
            )
            .map(|r| r.source_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resolve_external_type_from_loaded_files(
        &self,
        files: &FxHashMap<String, FileEntry>,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        cache: &mut ExternalTypeCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
        required_root_dep: bool,
        kind: verter_vfs::ResolveRequestKind,
    ) -> Result<Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>, ()> {
        let Some(dep_canonical) =
            self.resolve_loaded_dependency_canonical(files, owner_canonical, import_source, kind)
        else {
            return if required_root_dep { Err(()) } else { Ok(None) };
        };
        let cache_key = (dep_canonical.clone(), type_name.to_string());
        if let Some(cached) = cache.get(&cache_key) {
            return Ok(cached.clone());
        }
        if !visiting.insert(cache_key.clone()) {
            return Ok(None);
        }

        // Try host file cache first, then workspace-read fallback.
        let effective_source: String = if let Some(dep_entry) = files.get(&dep_canonical) {
            // For Vue SFC files, extract only <script>/<script setup> content.
            if dep_entry.file_kind == FileKind::VueSfc {
                match extract_vue_script_content(dep_entry) {
                    Some(s) => s,
                    None => {
                        visiting.remove(&cache_key);
                        cache.insert(cache_key, None);
                        return Ok(None);
                    }
                }
            } else {
                dep_entry.source.to_string()
            }
        } else {
            // Workspace-read fallback: read from disk via VFS when not in host cache.
            let ws = self.ws();
            match ws.read_file(&dep_canonical) {
                Some(source) => {
                    if dep_canonical.ends_with(".vue") {
                        match extract_script_from_raw_source(&source) {
                            Some(s) => s,
                            None => {
                                visiting.remove(&cache_key);
                                cache.insert(cache_key, None);
                                return Ok(None);
                            }
                        }
                    } else {
                        source.to_string()
                    }
                }
                None => {
                    visiting.remove(&cache_key);
                    return if required_root_dep { Err(()) } else { Ok(None) };
                }
            }
        };

        let import_alloc = oxc_allocator::Allocator::new();
        let extracted = verter_core::utils::oxc::vue::resolve_type::extract_imported_type_bindings(
            &effective_source,
            &import_alloc,
        );

        // Optimization: if the target type is directly re-exported from this file,
        // follow the re-export chain immediately instead of resolving ALL bindings.
        // This avoids O(N) workspace reads for barrel files with many re-exports.
        let direct_reexport = extracted
            .bindings
            .iter()
            .find(|b| b.local_name == type_name);
        if let Some(target) = direct_reexport {
            if let Some(resolved) = self.resolve_external_type_from_loaded_files(
                files,
                &dep_canonical,
                &target.source,
                &target.imported_name,
                cache,
                visiting,
                false,
                kind,
            )? {
                visiting.remove(&cache_key);
                cache.insert(cache_key, Some(resolved.clone()));
                return Ok(Some(resolved));
            }
        }

        let mut companion_types = rustc_hash::FxHashMap::default();
        for binding in &extracted.bindings {
            if let Some(resolved) = self.resolve_external_type_from_loaded_files(
                files,
                &dep_canonical,
                &binding.source,
                &binding.imported_name,
                cache,
                visiting,
                false,
                kind,
            )? {
                companion_types
                    .entry(binding.local_name.clone())
                    .or_insert(resolved);
            }
        }

        let resolve_alloc = oxc_allocator::Allocator::new();
        let mut resolved =
            verter_core::utils::oxc::vue::resolve_type::resolve_external_type_with_companion(
                type_name,
                &effective_source,
                &companion_types,
                &resolve_alloc,
            );

        // If the type wasn't found directly, try `export * from` wildcard re-export sources.
        // This handles barrel files like `export * from './Drawer'`.
        if resolved.is_none() {
            for source in &extracted.wildcard_reexport_sources {
                if let Some(found) = self.resolve_external_type_from_loaded_files(
                    files,
                    &dep_canonical,
                    source,
                    type_name,
                    cache,
                    visiting,
                    false,
                    kind,
                )? {
                    resolved = Some(found);
                    break;
                }
            }
        }

        visiting.remove(&cache_key);
        cache.insert(cache_key, resolved.clone());
        Ok(resolved)
    }

    fn collect_external_types_from_loaded_files(
        &self,
        owner_canonical: &str,
        macro_type_deps: &[verter_analysis::MacroTypeDep],
        script_imports: &[verter_analysis::AnalyzedImport],
    ) -> (Option<ResolvedExternalTypes>, Vec<HostDiagnostic>) {
        let files = read_lock(&self.files);
        let mut resolved = rustc_hash::FxHashMap::default();
        let mut missing = Vec::new();
        let mut cache = rustc_hash::FxHashMap::default();
        let mut visiting = rustc_hash::FxHashSet::default();

        for dep in macro_type_deps {
            match self.resolve_external_type_from_loaded_files(
                &files,
                owner_canonical,
                &dep.import_source,
                &dep.type_name,
                &mut cache,
                &mut visiting,
                true,
                verter_vfs::ResolveRequestKind::EsmImport,
            ) {
                Ok(Some(elements)) => {
                    resolved.insert(dep.type_name.clone(), elements);
                }
                Ok(None) => {}
                Err(()) => {
                    let span = script_imports
                        .iter()
                        .find(|import| import.source == dep.import_source)
                        .map(|import| import.span);
                    missing.push(HostDiagnostic {
                        severity: HostSeverity::Error,
                        code: "HOST_MISSING_MACRO_TYPE_DEP".to_string(),
                        message: format!(
                            "missing macro type dependency '{}' for type '{}' in '{}'",
                            dep.import_source, dep.type_name, owner_canonical
                        ),
                        span,
                    });
                }
            }
        }

        (
            if resolved.is_empty() {
                None
            } else {
                Some(resolved)
            },
            missing,
        )
    }
}

impl VerterHost {
    /// Resolve a raw import identifier (bundler query string or LSP `._VERTER_.` format)
    /// to its canonical ID, virtual node kind, and rendered bundler/LSP IDs.
    ///
    /// Returns `None` if the raw ID cannot be parsed.
    pub fn resolve(&self, raw_id: &str) -> Option<ResolvedId> {
        #[cfg(feature = "host_metrics")]
        self.metrics
            .resolves
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let parsed = parse_raw_id(raw_id)?;
        let canonical = self.resolve_alias_or_canonical(&parsed.canonical_id);
        let (exists, bundler_id, lsp_id) = {
            #[cfg(feature = "scheduler")]
            {
                use crate::host_executor::HostSourceData;
                let meta = self.scheduler.try_get_source(&canonical).and_then(|s| {
                    s.downcast_data::<HostSourceData>()
                        .map(|h| h.parse.meta.clone())
                });
                match meta {
                    Some(m) => {
                        let (b, l) = render_ids(&canonical, &parsed.node_kind, &m);
                        (true, b, l)
                    }
                    None => {
                        let default_meta = FileMeta::default();
                        let (b, l) = render_ids(&canonical, &parsed.node_kind, &default_meta);
                        (false, b, l)
                    }
                }
            }
            #[cfg(not(feature = "scheduler"))]
            {
                let files = read_lock(&self.files);
                match files.get(&canonical) {
                    Some(f) => {
                        let (b, l) = render_ids(&canonical, &parsed.node_kind, &f.meta);
                        (true, b, l)
                    }
                    None => {
                        let default_meta = FileMeta::default();
                        let (b, l) = render_ids(&canonical, &parsed.node_kind, &default_meta);
                        (false, b, l)
                    }
                }
            }
        };
        Some(ResolvedId {
            canonical_id: canonical,
            node_kind: parsed.node_kind,
            exists_in_host: exists,
            bundler_id,
            lsp_id,
        })
    }

    /// Ensure a file is compiled and cached for the given profile.
    ///
    /// Unlike [`get_virtual_file`](Self::get_virtual_file), this does not require
    /// specifying a `VirtualNodeKind`. It simply ensures the compilation cache is
    /// populated so that subsequent `get_ide()`, `get_analysis()`, or
    /// `get_virtual_file()` calls hit the cache.
    ///
    /// Returns `Ok(())` on success (cache hit or successful compilation).
    /// Returns `Err(HostError)` if the file is missing or compilation fails.
    pub fn ensure_compiled(
        &self,
        canonical_id: &str,
        profile: &CompileProfile,
    ) -> Result<(), HostError> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let profile_hash = compile_profile_hash(profile);

        // Check cache
        {
            #[cfg(feature = "scheduler")]
            {
                use crate::host_executor::HostSourceData;
                let snap = self.scheduler.try_get_source(&canonical).ok_or_else(|| {
                    HostError::MissingSource {
                        canonical_id: canonical.clone(),
                    }
                })?;
                let hd = snap.downcast_data::<HostSourceData>().ok_or_else(|| {
                    HostError::MissingSource {
                        canonical_id: canonical.clone(),
                    }
                })?;
                if hd.file_kind == FileKind::NonSfc {
                    return Ok(());
                }
                if let Some(cc) = self.compile_cache.get(&canonical) {
                    let soh = cc
                        .style_overrides
                        .get(&profile_hash)
                        .map(|o| o.hash)
                        .unwrap_or(0);
                    if let Some(slot) = cc.compile_slots.get(&profile_hash) {
                        if slot.semantic_hash == hd.parse.semantic_hash
                            && slot.style_override_hash == soh
                        {
                            return Ok(());
                        }
                    }
                }
            }
            #[cfg(not(feature = "scheduler"))]
            {
                let files = read_lock(&self.files);
                let entry = files
                    .get(&canonical)
                    .ok_or_else(|| HostError::MissingSource {
                        canonical_id: canonical.clone(),
                    })?;
                if entry.file_kind == FileKind::NonSfc {
                    return Ok(());
                }
                let soh = entry
                    .style_overrides
                    .get(&profile_hash)
                    .map(|o| o.hash)
                    .unwrap_or(0);
                if let Some(slot) = entry.compile_slots.get(&profile_hash) {
                    if slot.semantic_hash == entry.semantic_hash && slot.style_override_hash == soh
                    {
                        return Ok(());
                    }
                }
            }
        }

        // Cache miss — compile by requesting the Main virtual file.
        // This populates ALL cached outputs (script, template, styles, TSX, etc.)
        // for the given profile.
        let _ = self.get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile.clone(),
        })?;
        Ok(())
    }

    /// Retrieve a compiled virtual file (script, template, style, or main bundle).
    ///
    /// On cache hit, returns immediately. On cache miss, compiles the file using
    /// `verter_core::compile`, caches the result, and returns the requested node.
    /// In dev mode with [`CompileErrorPolicy::DevServeLastKnownGood`], falls back
    /// to the last successful compilation when the current source has errors.
    pub fn get_virtual_file(&self, query: VirtualQuery) -> Result<VirtualFileResponse, HostError> {
        #[cfg(feature = "host_metrics")]
        self.metrics
            .virtual_loads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let (canonical_id, node_kind, raw_was_lsp) = if let Some(raw) = query.raw_id.clone() {
            let parsed = parse_raw_id(&raw).ok_or(HostError::InvalidQuery)?;
            (
                self.resolve_alias_or_canonical(&parsed.canonical_id),
                parsed.node_kind,
                parsed.was_lsp_like,
            )
        } else if let (Some(canonical), Some(node_kind)) =
            (query.canonical_id.clone(), query.node_kind.clone())
        {
            (
                self.resolve_alias_or_canonical(&canonical),
                node_kind,
                false,
            )
        } else {
            return Err(HostError::InvalidQuery);
        };

        let profile_hash = compile_profile_hash(&query.compile_profile);

        // Cache hit check and compile input extraction under a single read lock.
        // This avoids cloning the full FileEntry (with all compile_slots, style_overrides, etc.)
        // on the hot path.
        struct CacheMiss {
            compile_input: CompileInput,
            fallback_last_good: Option<FxHashMap<VirtualNodeKind, CachedVirtualFile>>,
            meta: FileMeta,
            /// Captured under read lock so the compile slot is stored with the
            /// semantic_hash that was current when we decided to compile.
            semantic_hash: Hash16,
        }

        // Capture scheduler source state at compile START for artifact commit.
        #[cfg(feature = "scheduler")]
        let sched_snapshot_at_start = self.scheduler.try_get_source(&canonical_id);

        let cache_miss = {
            #[cfg(feature = "scheduler")]
            {
                use crate::host_executor::{HostAnalysisData, HostSourceData};

                let source_snap =
                    self.scheduler
                        .try_get_source(&canonical_id)
                        .ok_or_else(|| HostError::MissingSource {
                            canonical_id: canonical_id.clone(),
                        })?;
                let hd = source_snap
                    .downcast_data::<HostSourceData>()
                    .ok_or_else(|| HostError::MissingSource {
                        canonical_id: canonical_id.clone(),
                    })?;
                let parse = &hd.parse;

                let cc_ref = self.compile_cache.get(&canonical_id);

                // Cache hit check from compile_cache
                let soh = cc_ref
                    .as_ref()
                    .and_then(|cc| cc.style_overrides.get(&profile_hash).map(|o| o.hash))
                    .unwrap_or(0);
                let coh = cc_ref
                    .as_ref()
                    .and_then(|cc| {
                        cc.content_overrides
                            .get(&profile_hash)
                            .map(|o| o.layer.hash)
                    })
                    .unwrap_or(0);

                if let Some(ref cc) = cc_ref {
                    if let Some(slot) = cc.compile_slots.get(&profile_hash) {
                        if slot.semantic_hash == parse.semantic_hash
                            && slot.style_override_hash == soh
                            && slot.content_override_hash == coh
                        {
                            #[cfg(feature = "host_metrics")]
                            self.metrics
                                .compile_cache_hits
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                            // Build effective meta for cache-hit render_ids
                            let mut hit_meta = parse.meta.clone();
                            if let Some(so) = cc.style_overrides.get(&profile_hash) {
                                for (idx, lang) in so.lang_overrides.iter().enumerate() {
                                    if let Some(ref l) = lang {
                                        if idx < hit_meta.style_langs.len() {
                                            hit_meta.style_langs[idx] = Some(l.clone());
                                        }
                                    }
                                }
                            }

                            if let Some(found) = slot.outputs.get(&node_kind) {
                                return Ok(VirtualFileResponse {
                                    id: render_single_id(
                                        &canonical_id,
                                        &node_kind,
                                        &hit_meta,
                                        raw_was_lsp,
                                    ),
                                    code: found.code.clone(),
                                    source_map: found.source_map.clone(),
                                    lang: found.lang.clone(),
                                    stale: false,
                                    diagnostics: slot.diagnostics.clone(),
                                    meta: found.meta.clone(),
                                });
                            }
                        }
                    }
                }

                // Cache miss — use effective_* helpers for override-aware state
                let efs = self
                    .effective_file_state(&canonical_id, Some(profile_hash))
                    .ok_or_else(|| HostError::MissingSource {
                        canonical_id: canonical_id.clone(),
                    })?;
                let effective_meta = self
                    .effective_meta(&canonical_id, Some(profile_hash))
                    .unwrap_or_else(|| parse.meta.clone());

                let style_override_layer = cc_ref.as_ref().and_then(|cc| {
                    cc.style_overrides
                        .get(&profile_hash)
                        .map(|o| o.layer.clone())
                });
                let content_override_layer = cc_ref.as_ref().and_then(|cc| {
                    cc.content_overrides
                        .get(&profile_hash)
                        .map(|o| o.layer.clone())
                });
                let fallback_last_good = cc_ref.as_ref().and_then(|cc| {
                    cc.compile_slots
                        .get(&profile_hash)
                        .and_then(|slot| slot.last_good_outputs.clone())
                });

                // Style v-bind vars from raw analysis (override-independent)
                let analysis_snap = self.scheduler.try_get_analysis(&canonical_id);
                let style_analyses: Arc<Vec<verter_analysis::StyleBlockAnalysis>> = analysis_snap
                    .as_ref()
                    .and_then(|a| a.downcast_data::<HostAnalysisData>())
                    .map(|ad| Arc::clone(&ad.style_analyses))
                    .unwrap_or_default();

                drop(cc_ref);

                CacheMiss {
                    compile_input: CompileInput {
                        canonical_id: canonical_id.clone(),
                        source: efs.source,
                        meta: effective_meta.clone(),
                        parse_diagnostics: parse.parse_diagnostics.clone(),
                        src_blocks: parse.src_blocks.clone(),
                        external_requests: parse.external_requests.clone(),
                        style_override_layer,
                        content_override_layer,
                        macro_type_deps: efs.script_analysis.macro_type_deps.clone(),
                        script_imports: efs.script_analysis.imports.clone(),
                        cached_parse: efs.cached_parse,
                        style_v_bind_vars: style_analyses
                            .iter()
                            .flat_map(|sa| {
                                sa.v_binds.iter().map(|vb| {
                                    vb.expression
                                        .split('.')
                                        .next()
                                        .unwrap_or(&vb.expression)
                                        .to_string()
                                })
                            })
                            .collect(),
                    },
                    fallback_last_good,
                    meta: effective_meta,
                    semantic_hash: parse.semantic_hash,
                }
            }

            #[cfg(not(feature = "scheduler"))]
            {
                let files = read_lock(&self.files);
                let entry = files
                    .get(&canonical_id)
                    .ok_or_else(|| HostError::MissingSource {
                        canonical_id: canonical_id.clone(),
                    })?;

                let soh = entry
                    .style_overrides
                    .get(&profile_hash)
                    .map(|o| o.hash)
                    .unwrap_or(0);
                let coh = entry
                    .content_overrides
                    .get(&profile_hash)
                    .map(|o| o.hash)
                    .unwrap_or(0);

                if let Some(slot) = entry.compile_slots.get(&profile_hash) {
                    if slot.semantic_hash == entry.semantic_hash
                        && slot.style_override_hash == soh
                        && slot.content_override_hash == coh
                    {
                        #[cfg(feature = "host_metrics")]
                        self.metrics
                            .compile_cache_hits
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                        if let Some(found) = slot.outputs.get(&node_kind) {
                            return Ok(VirtualFileResponse {
                                id: render_single_id(
                                    &canonical_id,
                                    &node_kind,
                                    &entry.meta,
                                    raw_was_lsp,
                                ),
                                code: found.code.clone(),
                                source_map: found.source_map.clone(),
                                lang: found.lang.clone(),
                                stale: false,
                                diagnostics: slot.diagnostics.clone(),
                                meta: found.meta.clone(),
                            });
                        }
                    }
                }

                let fallback_last_good = entry
                    .compile_slots
                    .get(&profile_hash)
                    .and_then(|slot| slot.last_good_outputs.clone());

                CacheMiss {
                    compile_input: CompileInput {
                        canonical_id: entry.canonical_id.clone(),
                        source: entry.source.clone(),
                        meta: entry.meta.clone(),
                        parse_diagnostics: entry.parse_diagnostics.clone(),
                        src_blocks: entry.src_blocks.clone(),
                        external_requests: entry.external_requests.clone(),
                        style_override_layer: entry.style_overrides.get(&profile_hash).cloned(),
                        content_override_layer: entry.content_overrides.get(&profile_hash).cloned(),
                        macro_type_deps: entry.script_analysis.macro_type_deps.clone(),
                        script_imports: entry.script_analysis.imports.clone(),
                        cached_parse: entry.cached_parse.clone(),
                        style_v_bind_vars: entry
                            .style_analyses
                            .iter()
                            .flat_map(|sa| {
                                sa.v_binds.iter().map(|vb| {
                                    vb.expression
                                        .split('.')
                                        .next()
                                        .unwrap_or(&vb.expression)
                                        .to_string()
                                })
                            })
                            .collect(),
                    },
                    fallback_last_good,
                    meta: entry.meta.clone(),
                    semantic_hash: entry.semantic_hash,
                }
            }
        };

        let CacheMiss {
            compile_input,
            fallback_last_good,
            meta,
            semantic_hash: captured_semantic_hash,
        } = cache_miss;

        #[cfg(feature = "host_metrics")]
        self.metrics
            .compile_requests
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        #[cfg(feature = "host_metrics")]
        let compile_start = Instant::now();

        let style_override_hash = compile_input
            .style_override_layer
            .as_ref()
            .map(|o| o.hash)
            .unwrap_or(0);
        let content_override_hash = compile_input
            .content_override_layer
            .as_ref()
            .map(|o| o.hash)
            .unwrap_or(0);

        let (compiled_outputs, diagnostics, stale, compiled_tsx, compiled_template_analysis) =
            match self.compile_entry(&compile_input, &query.compile_profile) {
                Ok((outputs, diagnostics, tsx, tpl)) => (outputs, diagnostics, false, tsx, tpl),
                Err(diagnostics) => {
                    self.store_latest_diagnostics(&canonical_id, profile_hash, diagnostics.clone());
                    let policy = self.config.compile_error_policy;
                    if self.config.dev_mode && policy == CompileErrorPolicy::DevServeLastKnownGood {
                        if let Some(last_good) = fallback_last_good.clone() {
                            (last_good, diagnostics, true, None, None)
                        } else {
                            return Err(HostError::CompileError { diagnostics });
                        }
                    } else {
                        return Err(HostError::CompileError { diagnostics });
                    }
                }
            };

        #[cfg(feature = "host_metrics")]
        {
            let compile_elapsed_us = compile_start.elapsed().as_micros() as u64;
            self.metrics
                .compile_time_us_total
                .fetch_add(compile_elapsed_us, std::sync::atomic::Ordering::Relaxed);
            if let Ok(mut per_profile) = self.metrics.compile_time_us_total_by_profile.lock() {
                let entry = per_profile.entry(profile_hash).or_insert(0);
                *entry = entry.saturating_add(compile_elapsed_us);
            }
            if let Ok(mut per_profile_count) = self.metrics.compile_count_by_profile.lock() {
                let entry = per_profile_count.entry(profile_hash).or_insert(0);
                *entry = entry.saturating_add(1);
            }
        }

        let last_tick = self.tick.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Store compile results.
        // compile_cache is the authority for profile state.
        #[cfg(feature = "scheduler")]
        {
            if let Some(mut cc) = self.compile_cache.get_mut(&canonical_id) {
                cc.compile_slots.insert(
                    profile_hash,
                    CompileSlot {
                        semantic_hash: captured_semantic_hash,
                        style_override_hash,
                        content_override_hash,
                        outputs: compiled_outputs.clone(),
                        diagnostics: diagnostics.clone(),
                        last_good_outputs: if stale {
                            fallback_last_good.clone()
                        } else {
                            Some(compiled_outputs.clone())
                        },
                        last_access_tick: last_tick,
                        tsx: compiled_tsx.clone(),
                        template_analysis: compiled_template_analysis.clone(),
                    },
                );
                cc.latest_diagnostics
                    .insert(profile_hash, diagnostics.clone());
                cc.diagnostics_generation += 1;
            }
        }

        // Write per-profile state to files (profile-keyed, no shared field mutation).
        // This keeps cross_file and other readers working during transition.
        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical_id) {
                let last_good_outputs = if stale {
                    fallback_last_good.clone()
                } else {
                    Some(compiled_outputs.clone())
                };
                // template_analysis on FileEntry is per-file (not per-profile),
                // populated from the latest compile. This is acceptable because
                // template analysis doesn't vary by profile.
                if compiled_template_analysis.is_some() {
                    entry.template_analysis = compiled_template_analysis.clone().map(Arc::new);
                }
                entry.compile_slots.insert(
                    profile_hash,
                    CompileSlot {
                        semantic_hash: captured_semantic_hash,
                        style_override_hash,
                        content_override_hash,
                        outputs: compiled_outputs.clone(),
                        diagnostics: diagnostics.clone(),
                        last_good_outputs,
                        last_access_tick: last_tick,
                        tsx: compiled_tsx,
                        template_analysis: compiled_template_analysis,
                    },
                );
                entry
                    .latest_diagnostics
                    .insert(profile_hash, diagnostics.clone());
                entry.diagnostics_generation += 1;
                enforce_profile_cap(entry, self.config.max_profiles_per_file.max(1));

                // Commit to scheduler artifact snapshot.
                #[cfg(feature = "scheduler")]
                {
                    if let Some(ref snap) = sched_snapshot_at_start {
                        if snap.whole_hash == entry.whole_hash {
                            let gen = snap.generation;
                            drop(files);
                            self.scheduler.commit_artifact(
                                &canonical_id,
                                profile_hash,
                                verter_scheduler::node::ArtifactSnapshot {
                                    generation: gen,
                                    profile_hash,
                                    data: Arc::new(crate::host_executor::HostArtifactData {
                                        outputs: compiled_outputs.clone(),
                                        diagnostics: diagnostics.clone(),
                                    }),
                                },
                            );
                        } else {
                            drop(files);
                        }
                    } else {
                        drop(files);
                    }
                }
                #[cfg(not(feature = "scheduler"))]
                drop(files);
            }
        }

        let found =
            compiled_outputs
                .get(&node_kind)
                .ok_or_else(|| HostError::MissingVirtualNode {
                    canonical_id: canonical_id.clone(),
                })?;

        Ok(VirtualFileResponse {
            id: render_single_id(&canonical_id, &node_kind, &meta, raw_was_lsp),
            code: found.code.clone(),
            source_map: found.source_map.clone(),
            lang: found.lang.clone(),
            stale,
            diagnostics,
            meta: found.meta.clone(),
        })
    }

    /// List all virtual node kinds for a file (Main, Script, Template, Style, Custom).
    pub fn list_virtual_files(&self, canonical_id: &str) -> Vec<VirtualNodeKind> {
        self.list_virtual_nodes(canonical_id)
    }

    /// Retrieve the combined TSX output for LSP type checking.
    ///
    /// Returns the IDE code (TSX or JSX) and optional source map for the given file and profile.
    /// This is a dedicated API separate from the virtual file system, since IDE
    /// output is only consumed by the LSP and playground, never by bundlers.
    pub fn get_ide(&self, canonical_id: &str, profile: &CompileProfile) -> Option<IdeResponse> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let profile_hash = compile_profile_hash(profile);
        let files = read_lock(&self.files);
        let entry = files.get(&canonical)?;
        let slot = entry.compile_slots.get(&profile_hash)?;
        let tsx = slot.tsx.as_ref()?;
        Some(IdeResponse {
            code: tsx.code.clone(),
            source_map: tsx.source_map.clone(),
            is_jsx: tsx.is_jsx,
            destructured_block: tsx.destructured_block.clone(),
        })
    }

    /// Generate public API output for a Vue SFC — minimal TypeScript declarations.
    ///
    /// Unlike [`get_ide`](Self::get_ide), this does NOT require a prior
    /// [`get_virtual_file`](Self::get_virtual_file) call. It performs
    /// macro-only extraction (OXC parse → defineProps/Emits/Model/Options)
    /// and generates a `ComponentPublicInstance`-based declaration.
    ///
    /// Returns `None` if the file is not in the host or not a Vue SFC.
    pub fn get_public_api(&self, canonical_id: &str) -> Option<TscResponse> {
        self.get_public_api_with_mode(canonical_id, PublicApiMode::Public)
    }

    /// Generate public API output for a Vue SFC using the requested surface mode.
    ///
    /// `PublicApiMode::Public` matches the default application-facing instance shape.
    /// `PublicApiMode::Testing` exposes internal `<script setup>` bindings in a
    /// Vue Test Utils-like debug surface.
    pub fn get_public_api_with_mode(
        &self,
        canonical_id: &str,
        mode: PublicApiMode,
    ) -> Option<TscResponse> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);

        #[cfg(feature = "scheduler")]
        if let Some(cc) = self.compile_cache.get(&canonical) {
            if cc.evicted {
                return None;
            }
        }

        #[cfg(feature = "scheduler")]
        let (source, file_kind, macro_type_deps, script_imports, cached_extract) = {
            use crate::host_executor::HostSourceData;
            let snap = self.scheduler.try_get_source(&canonical)?;
            let hd = snap.downcast_data::<HostSourceData>()?;
            if hd.file_kind != FileKind::VueSfc {
                return None;
            }
            let cached = self
                .compile_cache
                .get(&canonical)
                .and_then(|cc| cc.cached_tsc_extract.as_ref().map(|(_, e)| Arc::clone(e)));
            (
                snap.source.clone(),
                hd.file_kind,
                hd.parse.script_analysis.macro_type_deps.clone(),
                hd.parse.script_analysis.imports.clone(),
                cached,
            )
        };

        #[cfg(not(feature = "scheduler"))]
        let (source, file_kind, macro_type_deps, script_imports, cached_extract) = {
            let files = read_lock(&self.files);
            let entry = files.get(&canonical)?;
            (
                entry.source.clone(),
                entry.file_kind,
                entry.script_analysis.macro_type_deps.clone(),
                entry.script_analysis.imports.clone(),
                entry.cached_tsc_extract.clone(),
            )
        };
        if file_kind != FileKind::VueSfc {
            return None;
        }
        // Derive component name from canonical_id: last path segment, strip .vue extension.
        let component_name = canonical
            .rsplit('/')
            .next()
            .unwrap_or(&canonical)
            .trim_end_matches(".vue")
            .to_string();
        let (external_types, _) = self.collect_external_types_from_loaded_files(
            &canonical,
            &macro_type_deps,
            &script_imports,
        );
        let tsc_mode = match mode {
            PublicApiMode::Public => verter_core::tsc::TscMode::Public,
            PublicApiMode::Testing => verter_core::tsc::TscMode::Testing,
        };

        // Try cached extract path: avoids re-parsing SFC + OXC on cache hit.
        let extract = if let Some(cached) = cached_extract {
            cached
        } else if let Some(fresh) = verter_core::tsc::extract_tsc_state(
            &source,
            &component_name,
            &verter_core::tsc::TscExtractOptions {
                filename: Some(canonical.clone()),
            },
        ) {
            let arc = Arc::new(fresh);
            // Store in compile_cache
            #[cfg(feature = "scheduler")]
            {
                use crate::host_executor::HostSourceData;
                if let Some(mut cc) = self.compile_cache.get_mut(&canonical) {
                    let wh = self
                        .scheduler
                        .try_get_source(&canonical)
                        .and_then(|s| {
                            s.downcast_data::<HostSourceData>()
                                .map(|h| h.parse.whole_hash)
                        })
                        .unwrap_or([0; 16]);
                    cc.cached_tsc_extract = Some((wh, Arc::clone(&arc)));
                }
            }
            // Also store in files
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical) {
                entry.cached_tsc_extract = Some(Arc::clone(&arc));
            }
            arc
        } else {
            // No <script setup> — fall through to direct path for empty stub
            let tsc_out = verter_core::tsc::generate_tsc_output_with_options(
                &source,
                &component_name,
                &verter_core::tsc::TscGenOptions {
                    conditional_root_narrowing: false,
                    filename: Some(canonical.clone()),
                    external_types,
                    mode: tsc_mode,
                },
            );
            return Some(TscResponse {
                code: Arc::from(tsc_out.code),
                source_map: if tsc_out.source_map.is_empty() {
                    None
                } else {
                    Some(Arc::from(tsc_out.source_map))
                },
            });
        };

        let tsc_out = verter_core::tsc::generate_tsc_from_state(
            &extract,
            &source,
            &component_name,
            tsc_mode,
            external_types.as_ref(),
        );
        Some(TscResponse {
            code: Arc::from(tsc_out.code),
            source_map: if tsc_out.source_map.is_empty() {
                None
            } else {
                Some(Arc::from(tsc_out.source_map))
            },
        })
    }

    /// Store diagnostics from a failed compile without triggering recompilation.
    pub(crate) fn store_latest_diagnostics(
        &self,
        canonical_id: &str,
        profile_hash: u64,
        diagnostics: DiagnosticsSnapshot,
    ) {
        #[cfg(feature = "scheduler")]
        if let Some(mut cc) = self.compile_cache.get_mut(canonical_id) {
            cc.latest_diagnostics
                .insert(profile_hash, diagnostics.clone());
            cc.diagnostics_generation += 1;
        }

        // Per-profile write to files (keyed by profile_hash, no shared field mutation)
        let mut files = write_lock(&self.files);
        if let Some(entry) = files.get_mut(canonical_id) {
            entry.latest_diagnostics.insert(profile_hash, diagnostics);
            entry.diagnostics_generation += 1;
        }
    }

    #[allow(clippy::type_complexity)]
    pub(crate) fn compile_entry(
        &self,
        snapshot: &CompileInput,
        profile: &CompileProfile,
    ) -> Result<
        (
            FxHashMap<VirtualNodeKind, CachedVirtualFile>,
            DiagnosticsSnapshot,
            Option<CachedTsx>,
            Option<verter_analysis::template::TemplateAnalysisSnapshot>,
        ),
        DiagnosticsSnapshot,
    > {
        let mut diagnostics = snapshot.parse_diagnostics.clone();

        let mut merged_source = snapshot.source.to_string();
        if !snapshot.src_blocks.is_empty() {
            let ext_sources = {
                let files = read_lock(&self.files);
                let mut map = FxHashMap::default();
                for req in &snapshot.external_requests {
                    let resolved_dep_id = files
                        .contains_key(&req.resolved_canonical_id)
                        .then(|| req.resolved_canonical_id.clone())
                        .or_else(|| {
                            self.resolve_loaded_dependency_canonical(
                                &files,
                                &snapshot.canonical_id,
                                &req.specifier,
                                verter_vfs::ResolveRequestKind::EsmImport,
                            )
                        });
                    if let Some(dep_entry) = resolved_dep_id
                        .as_deref()
                        .and_then(|canonical_id| files.get(canonical_id))
                    {
                        map.insert(req.resolved_canonical_id.clone(), dep_entry.source.clone());
                    }
                }
                map
            };

            for (idx, req) in snapshot.external_requests.iter().enumerate() {
                if !ext_sources.contains_key(&req.resolved_canonical_id) {
                    let span = snapshot.src_blocks.get(idx).map(|block| {
                        verter_span::Span::new(block.tag_open_start, block.tag_open_end)
                    });
                    diagnostics =
                        diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
                            severity: HostSeverity::Error,
                            code: "HOST_MISSING_EXTERNAL_SOURCE".to_string(),
                            message: format!(
                                "missing external source '{}' for '{}'",
                                req.specifier, snapshot.canonical_id
                            ),
                            span,
                        }]));
                }
            }

            if diagnostics.has_errors {
                return Err(diagnostics);
            }

            merged_source =
                merge_external_sources(&merged_source, &snapshot.src_blocks, &ext_sources);
        }

        let alloc = Allocator::new();
        let core_opts = CodegenOptions {
            filename: profile
                .filename
                .clone()
                .or_else(|| Some(snapshot.canonical_id.clone())),
            is_production: profile.is_production,
            // Host always assembles a standalone `function render()` via
            // assemble_main_module, so inline mode must be off — otherwise the
            // template emits bare identifiers (missing `$setup.` prefix).
            inline: Some(false),
            component_id: profile.component_id.clone(),
            delimiters: profile.delimiters.clone(),
            custom_elements: profile.custom_elements.clone(),
            comments: profile.comments,
            runtime_module_name: profile.runtime_module_name.clone(),
            types_module_name: profile.types_module_name.clone(),
            target: profile.target,
            embed_ambient_types: profile.embed_ambient_types,
            conditional_root_narrowing: profile.conditional_root_narrowing,
            strict_slots: profile.strict_slots,
            ..CodegenOptions::default()
        };

        let mut unresolved_macro_type_diags = Vec::new();

        let (external_types, missing_macro_type_diags) = self
            .collect_external_types_from_loaded_files(
                &snapshot.canonical_id,
                &snapshot.macro_type_deps,
                &snapshot.script_imports,
            );
        unresolved_macro_type_diags.extend(missing_macro_type_diags);

        if !unresolved_macro_type_diags.is_empty() {
            diagnostics =
                diagnostics.merge(DiagnosticsSnapshot::from_vec(unresolved_macro_type_diags));
            return Err(diagnostics);
        }

        let scope = self.config.effective_scope();
        let verter_opts = VerterCompileOptions {
            force_vapor: profile.force_vapor,
            force_js: profile.force_js,
            source_map: profile.source_map,
            ssr: profile.ssr,
            external_types,
            extract_template_data: scope.needs_template_analysis(),
            prop_constness_overrides: None, // TODO(Phase 6): populated by cross-file optimizer
            style_v_bind_vars: snapshot.style_v_bind_vars.clone(),
        };

        // Reuse cached parse when source wasn't modified by external src= merging
        // and no custom delimiters/elements that would change parse behavior.
        let can_use_cache = snapshot.src_blocks.is_empty()
            && profile.delimiters.is_none()
            && profile.custom_elements.is_none();

        let compiled = if can_use_cache {
            if let Some(ref cached) = snapshot.cached_parse {
                compile_from_parsed(&merged_source, cached, &core_opts, &verter_opts, &alloc)
            } else {
                compile_sfc(&merged_source, &core_opts, &verter_opts, &alloc)
            }
        } else {
            compile_sfc(&merged_source, &core_opts, &verter_opts, &alloc)
        };

        let mut compile_diags = diagnostics.clone();
        if !compiled.errors.is_empty() {
            compile_diags = compile_diags.merge(DiagnosticsSnapshot::from_vec(
                compiled
                    .errors
                    .iter()
                    .map(|d| HostDiagnostic {
                        severity: match d.severity {
                            verter_core::compile::CompileDiagnosticSeverity::Error => {
                                HostSeverity::Error
                            }
                            verter_core::compile::CompileDiagnosticSeverity::Warning => {
                                HostSeverity::Warning
                            }
                            verter_core::compile::CompileDiagnosticSeverity::Info => {
                                HostSeverity::Info
                            }
                        },
                        code: d.code.clone(),
                        message: d.message.clone(),
                        span: d.span,
                    })
                    .collect(),
            ));
        }

        if compile_diags.has_errors {
            return Err(compile_diags);
        }

        let mut outputs = FxHashMap::default();

        let main_code =
            assemble_main_module(&snapshot.canonical_id, &compiled, &snapshot.meta, profile);
        outputs.insert(
            VirtualNodeKind::Main,
            CachedVirtualFile {
                code: Arc::from(main_code),
                source_map: None,
                lang: Some(if profile.force_js {
                    "js".to_string()
                } else {
                    snapshot
                        .meta
                        .script_lang
                        .as_deref()
                        .unwrap_or("js")
                        .to_string()
                }),
                meta: VirtualMeta {
                    scope_id: if compiled.scope_id.is_empty() {
                        None
                    } else {
                        Some(compiled.scope_id.clone())
                    },
                    ..VirtualMeta::default()
                },
            },
        );

        if let Some(script) = compiled.script {
            outputs.insert(
                VirtualNodeKind::Script,
                CachedVirtualFile {
                    code: Arc::from(script.code),
                    source_map: if script.source_map.is_empty() {
                        None
                    } else {
                        Some(Arc::from(script.source_map))
                    },
                    lang: Some("ts".to_string()),
                    meta: VirtualMeta::default(),
                },
            );
        }

        if let Some(template) = compiled.template {
            let code = if template.imports.is_empty() {
                template.code
            } else {
                let runtime = profile.runtime_module_name.as_deref().unwrap_or("vue");
                let specifiers: Vec<String> = template
                    .imports
                    .iter()
                    .map(|name| format_import_specifier(name))
                    .collect();
                format!(
                    "import {{ {} }} from \"{}\"\n{}",
                    specifiers.join(", "),
                    runtime,
                    template.code,
                )
            };
            outputs.insert(
                VirtualNodeKind::Template,
                CachedVirtualFile {
                    code: Arc::from(code),
                    source_map: if template.source_map.is_empty() {
                        None
                    } else {
                        Some(Arc::from(template.source_map))
                    },
                    lang: Some("tsx".to_string()),
                    meta: VirtualMeta::default(),
                },
            );
        }

        let style_layer = snapshot.style_override_layer.as_ref();

        for (i, style) in compiled.styles.into_iter().enumerate() {
            let override_entry = style_layer.and_then(|layer| layer.by_index.get(&i));
            outputs.insert(
                VirtualNodeKind::Style { index: i },
                CachedVirtualFile {
                    code: override_entry
                        .map(|e| e.code.clone())
                        .unwrap_or_else(|| Arc::from(style.code)),
                    source_map: override_entry.and_then(|e| e.source_map.clone()),
                    lang: Some(style.lang.unwrap_or_else(|| "css".to_string())),
                    meta: VirtualMeta {
                        style_index: Some(i),
                        ..VirtualMeta::default()
                    },
                },
            );
        }

        for (i, block) in compiled.custom_blocks.into_iter().enumerate() {
            outputs.insert(
                VirtualNodeKind::Custom { index: i },
                CachedVirtualFile {
                    code: Arc::from(block.content),
                    source_map: None,
                    lang: snapshot.meta.custom_langs.get(i).cloned().flatten(),
                    meta: VirtualMeta {
                        custom_index: Some(i),
                        block_type: Some(block.block_type),
                        ..VirtualMeta::default()
                    },
                },
            );
        }

        // Combined IDE output (TSX/JSX) for LSP type checking — stored separately, not as virtual file
        let cached_tsx = compiled.tsx.map(|tsx| CachedTsx {
            code: Arc::from(tsx.code),
            source_map: if tsx.source_map.is_empty() {
                None
            } else {
                Some(Arc::from(tsx.source_map))
            },
            is_jsx: tsx.is_jsx,
            destructured_block: tsx.destructured_block,
        });

        // Convert raw template data into analysis types when available
        let template_analysis = compiled.template_data.as_ref().map(|raw| {
            // Build script import pairs for component → source resolution
            let script_imports: Vec<(String, String)> = snapshot
                .macro_type_deps
                .iter()
                .map(|dep| (dep.type_name.clone(), dep.import_source.clone()))
                .collect::<Vec<_>>();
            // Also collect script imports from the file's analysis if available
            let files = read_lock(&self.files);
            let (all_imports, binding_class_unions, props_binding_name) = if let Some(entry) =
                files.get(&snapshot.canonical_id)
            {
                let imports: Vec<(String, String)> = entry
                    .script_analysis
                    .imports
                    .iter()
                    .flat_map(|imp| {
                        imp.bindings
                            .iter()
                            .map(|b| (b.name.clone(), imp.source.clone()))
                    })
                    .chain(script_imports)
                    .collect();

                // Build string literal union map from props + local bindings
                let mut unions: Vec<(String, Vec<String>)> = Vec::new();

                // Props from defineProps macro
                let define_props = entry
                    .script_analysis
                    .macros
                    .iter()
                    .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps);
                if let Some(dp) = define_props {
                    for field in &dp.prop_fields {
                        if let Some(type_ann) = &field.type_annotation {
                            let classes = verter_analysis::parse_string_literal_union(type_ann);
                            if !classes.is_empty() {
                                unions.push((field.name.clone(), classes));
                            }
                        }
                    }
                }

                // Local bindings with string literal union types
                for binding in &entry.script_analysis.bindings {
                    if let Some(type_ann) = &binding.type_annotation {
                        let effective_type =
                            verter_analysis::unwrap_reactive_type(type_ann).unwrap_or(type_ann);
                        let classes = verter_analysis::parse_string_literal_union(effective_type);
                        if !classes.is_empty() {
                            unions.push((binding.name.clone(), classes));
                        }
                    }
                }

                // Extract props binding name (e.g., "props" from `const props = defineProps()`)
                let props_name = define_props.and_then(|dp| dp.binding_name.clone());

                (imports, unions, props_name)
            } else {
                (script_imports, Vec::new(), None)
            };
            drop(files);
            crate::template_convert::convert_raw_to_analysis(
                raw,
                &all_imports,
                &binding_class_unions,
                props_binding_name.as_deref(),
            )
        });

        Ok((outputs, compile_diags, cached_tsx, template_analysis))
    }
}

/// Extract concatenated script content from raw Vue SFC source text.
/// Used as workspace-read fallback when the file is not in the host cache.
fn extract_script_from_raw_source(source: &str) -> Option<String> {
    let parsed = verter_core::compile::parse_sfc(source, None, None);
    let mut combined = String::new();
    for script in [parsed.script_setup(), parsed.script()]
        .into_iter()
        .flatten()
    {
        if let Some(span) = script.content {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str(&source[span.start as usize..span.end as usize]);
        }
    }
    if combined.is_empty() {
        None
    } else {
        Some(combined)
    }
}

/// Extract concatenated script content from a Vue SFC FileEntry.
/// Uses `cached_parse` (populated during upsert) to locate `<script>` and
/// `<script setup>` content spans, then slices the original source.
fn extract_vue_script_content(entry: &FileEntry) -> Option<String> {
    let source = entry.source.as_ref();
    let parsed = entry
        .cached_parse
        .as_deref()
        .map(std::borrow::Cow::Borrowed)
        .unwrap_or_else(|| {
            std::borrow::Cow::Owned(verter_core::compile::parse_sfc(source, None, None))
        });

    let mut combined = String::new();
    // Concatenate both blocks (setup first, then companion).
    // Order doesn't matter — type/interface collection is by name.
    for script in [parsed.script_setup(), parsed.script()]
        .into_iter()
        .flatten()
    {
        if let Some(span) = script.content {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str(&source[span.start as usize..span.end as usize]);
        }
    }
    if combined.is_empty() {
        None
    } else {
        Some(combined)
    }
}

#[cfg(test)]
#[path = "host_resolve_tests.rs"]
mod host_resolve_tests;

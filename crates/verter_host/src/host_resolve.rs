//! `impl VerterHost` — resolve and virtual file retrieval methods.
//!
//! Contains [`VerterHost::resolve`], [`VerterHost::get_virtual_file`],
//! [`VerterHost::list_virtual_files`], and the internal [`VerterHost::compile_entry`]
//! helper that drives on-demand compilation.

use std::collections::HashMap;
use std::sync::Arc;

use oxc_allocator::Allocator;
use verter_core::compile::CodegenOptions;
use verter_core::compile::{compile as compile_sfc, format_import_specifier, VerterCompileOptions};

use crate::cache::enforce_profile_cap;
use crate::compile::{assemble_main_module, merge_external_sources};
use crate::hash::compile_profile_hash;
use crate::id::{parse_raw_id, render_ids, render_single_id};
use crate::shared::{read_lock, write_lock};
use crate::types::*;
use crate::VerterHost;

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
        };
        Some(ResolvedId {
            canonical_id: canonical,
            node_kind: parsed.node_kind,
            exists_in_host: exists,
            bundler_id,
            lsp_id,
        })
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
            fallback_last_good: Option<HashMap<VirtualNodeKind, CachedVirtualFile>>,
            meta: FileMeta,
            /// Captured under read lock so the compile slot is stored with the
            /// semantic_hash that was current when we decided to compile.
            semantic_hash: Hash16,
        }

        let cache_miss = {
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

            if let Some(slot) = entry.compile_slots.get(&profile_hash) {
                if slot.semantic_hash == entry.semantic_hash && slot.style_override_hash == soh {
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

            // Cache miss — extract only the data needed for compilation
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
                    macro_type_deps: entry.script_analysis.macro_type_deps.clone(),
                },
                fallback_last_good,
                meta: entry.meta.clone(),
                semantic_hash: entry.semantic_hash,
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
        let compile_start = std::time::Instant::now();

        let style_override_hash = compile_input
            .style_override_layer
            .as_ref()
            .map(|o| o.hash)
            .unwrap_or(0);

        let (compiled_outputs, diagnostics, stale) =
            match self.compile_entry(&compile_input, &query.compile_profile) {
                Ok((outputs, diagnostics)) => (outputs, diagnostics, false),
                Err(diagnostics) => {
                    self.store_latest_diagnostics(&canonical_id, profile_hash, diagnostics.clone());
                    let policy = self.config.compile_error_policy;
                    if self.config.dev_mode && policy == CompileErrorPolicy::DevServeLastKnownGood {
                        if let Some(last_good) = fallback_last_good.clone() {
                            (last_good, diagnostics, true)
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

        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical_id) {
                let last_good_outputs = if stale {
                    fallback_last_good.clone()
                } else {
                    Some(compiled_outputs.clone())
                };
                entry.compile_slots.insert(
                    profile_hash,
                    CompileSlot {
                        semantic_hash: captured_semantic_hash,
                        style_override_hash,
                        outputs: compiled_outputs.clone(),
                        diagnostics: diagnostics.clone(),
                        last_good_outputs,
                        last_access_tick: last_tick,
                    },
                );
                entry
                    .latest_diagnostics
                    .insert(profile_hash, diagnostics.clone());
                enforce_profile_cap(entry, self.config.max_profiles_per_file.max(1));
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
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let files = read_lock(&self.files);
        files
            .get(&canonical)
            .map(|f| f.all_virtual_nodes())
            .unwrap_or_default()
    }

    /// Store diagnostics from a failed compile without triggering recompilation.
    pub(crate) fn store_latest_diagnostics(
        &self,
        canonical_id: &str,
        profile_hash: u64,
        diagnostics: DiagnosticsSnapshot,
    ) {
        let mut files = write_lock(&self.files);
        if let Some(entry) = files.get_mut(canonical_id) {
            entry.latest_diagnostics.insert(profile_hash, diagnostics);
        }
    }

    pub(crate) fn compile_entry(
        &self,
        snapshot: &CompileInput,
        profile: &CompileProfile,
    ) -> Result<
        (
            HashMap<VirtualNodeKind, CachedVirtualFile>,
            DiagnosticsSnapshot,
        ),
        DiagnosticsSnapshot,
    > {
        let mut diagnostics = snapshot.parse_diagnostics.clone();

        let mut merged_source = snapshot.source.to_string();
        if !snapshot.src_blocks.is_empty() {
            let ext_sources = {
                let files = read_lock(&self.files);
                let mut map = HashMap::new();
                for req in &snapshot.external_requests {
                    if let Some(dep_entry) = files.get(&req.resolved_canonical_id) {
                        map.insert(req.resolved_canonical_id.clone(), dep_entry.source.clone());
                    }
                }
                map
            };

            for req in &snapshot.external_requests {
                if !ext_sources.contains_key(&req.resolved_canonical_id) {
                    diagnostics =
                        diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
                            severity: HostSeverity::Error,
                            code: "HOST_MISSING_EXTERNAL_SOURCE".to_string(),
                            message: format!(
                                "missing external source '{}' for '{}'",
                                req.specifier, snapshot.canonical_id
                            ),
                            span_start: None,
                            span_end: None,
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
            ..CodegenOptions::default()
        };

        // Pre-resolve external types for cross-file type resolution in defineProps/defineEmits.
        // For each macro_type_dep, resolve the import source to a dep file, then call
        // resolve_external_type() on its source to get the resolved type elements.
        let external_types = {
            let files = read_lock(&self.files);
            let mut resolved = rustc_hash::FxHashMap::default();
            for dep in &snapshot.macro_type_deps {
                // Resolve relative import source to canonical dep path
                let dep_canonical =
                    crate::id::resolve_external(&snapshot.canonical_id, &dep.import_source);
                // Try exact match first, then with configured extensions
                let dep_source: Option<std::sync::Arc<str>> = files
                    .get(&*dep_canonical)
                    .map(|e| e.source.clone())
                    .or_else(|| {
                        self.config.resolve_extensions.iter().find_map(|ext| {
                            let with_ext = format!("{}{}", dep_canonical, ext);
                            files.get(with_ext.as_str()).map(|e| e.source.clone())
                        })
                    });
                if let Some(source) = dep_source {
                    let resolve_alloc = oxc_allocator::Allocator::new();
                    if let Some(elements) =
                        verter_core::utils::oxc::vue::resolve_type::resolve_external_type(
                            &dep.type_name,
                            &source,
                            &resolve_alloc,
                        )
                    {
                        resolved.insert(dep.type_name.clone(), elements);
                    }
                }
            }
            if resolved.is_empty() {
                None
            } else {
                Some(resolved)
            }
        };

        let verter_opts = VerterCompileOptions {
            force_vapor: profile.force_vapor,
            force_js: profile.force_js,
            source_map: profile.source_map,
            external_types,
        };

        let compiled = compile_sfc(&merged_source, &core_opts, &verter_opts, &alloc);

        let mut compile_diags = diagnostics.clone();
        if !compiled.errors.is_empty() {
            // Build UTF-16 resolver lazily — only when diagnostics have spans
            let resolver = if compiled.errors.iter().any(|d| d.span.is_some()) {
                Some(verter_core::cursor::position::PositionResolver::new(
                    &merged_source,
                ))
            } else {
                None
            };
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
                        span_start: d.span.map(|s| {
                            resolver
                                .as_ref()
                                .map(|r| r.offset_to_line_col(s.start as usize).2 as u32)
                                .unwrap_or(s.start)
                        }),
                        span_end: d.span.map(|s| {
                            resolver
                                .as_ref()
                                .map(|r| r.offset_to_line_col(s.end as usize).2 as u32)
                                .unwrap_or(s.end)
                        }),
                    })
                    .collect(),
            ));
        }

        if compile_diags.has_errors {
            return Err(compile_diags);
        }

        let mut outputs = HashMap::new();

        let main_code =
            assemble_main_module(&snapshot.canonical_id, &compiled, &snapshot.meta, profile);
        outputs.insert(
            VirtualNodeKind::Main,
            CachedVirtualFile {
                code: Arc::from(main_code),
                source_map: None,
                lang: Some(
                    snapshot
                        .meta
                        .script_lang
                        .as_deref()
                        .unwrap_or("js")
                        .to_string(),
                ),
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

        Ok((outputs, compile_diags))
    }
}

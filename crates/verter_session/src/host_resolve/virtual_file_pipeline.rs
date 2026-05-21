//! `impl VerterHost` — the SFC virtual-file pipeline.
//!
//! Owns the public `resolve` / `ensure_compiled` / `compile_slot_is_warm`
//! / `get_virtual_file` / `list_virtual_files` / `get_ide` /
//! `get_public_api*` accessors, the `store_latest_diagnostics` writer,
//! and the internal `compile_entry` / `hydrate_compile_blockers` helpers
//! that drive on-demand SFC compilation through the scheduler-backed
//! cache substrate.

use std::sync::Arc;

use rustc_hash::FxHashMap;

#[cfg(feature = "session_metrics")]
use crate::instant::Instant;

use super::vue_script_extract::template_converter_inputs;
use crate::compile::{assemble_main_module, merge_external_sources};
use crate::hash::compile_profile_hash;
use crate::id::{parse_raw_id, render_ids, render_single_id};
use crate::types::*;
use crate::VerterHost;
use oxc_allocator::Allocator;
use verter_compiler::compile::CodegenOptions;
use verter_compiler::compile::{
    compile as compile_sfc, compile_from_parsed, format_import_specifier, VerterCompileOptions,
};

impl VerterHost {
    /// Resolve a raw import identifier (bundler query string or LSP `._VERTER_.` format)
    /// to its canonical ID, virtual node kind, and rendered bundler/LSP IDs.
    ///
    /// Returns `None` if the raw ID cannot be parsed.
    pub fn resolve(&self, raw_id: &str) -> Option<ResolvedId> {
        #[cfg(feature = "session_metrics")]
        self.metrics
            .resolves
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let parsed = parse_raw_id(raw_id)?;
        let canonical = self.resolve_alias_or_canonical(&parsed.canonical_id);
        let (exists, bundler_id, lsp_id) = {
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
    fn hydrate_compile_blockers(&self, canonical_id: &str) {
        let Some(blockers) = self.get_compile_blockers(canonical_id) else {
            return;
        };

        let workspace = self.workspace();
        let mut blocker_ids = std::collections::BTreeSet::new();

        for request in blockers.external_source_requests {
            let resolved = workspace
                .resolve_import(
                    canonical_id,
                    &request.specifier,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: verter_workspace::ResolveRequestKind::SfcSrcAttr,
                    },
                )
                .map(|resolution| {
                    self.cache_positive_import_route_result(
                        canonical_id,
                        &request.specifier,
                        &resolution.source_id,
                    );
                    resolution.source_id
                })
                .unwrap_or(request.resolved_canonical_id);
            if resolved != canonical_id {
                blocker_ids.insert(resolved);
            }
        }

        for dep in blockers.macro_type_deps.iter() {
            let resolved = workspace
                .resolve_import(
                    canonical_id,
                    &dep.import_source,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: verter_workspace::ResolveRequestKind::TypeImport,
                    },
                )
                .inspect(|resolution| {
                    self.cache_positive_import_route_result(
                        canonical_id,
                        &dep.import_source,
                        &resolution.source_id,
                    );
                })
                .or_else(|| {
                    workspace
                        .resolve_import(
                            canonical_id,
                            &dep.import_source,
                            verter_workspace::ResolutionContext {
                                phase: verter_workspace::ResolvePhase::CodegenBlocker,
                                kind: verter_workspace::ResolveRequestKind::EsmImport,
                            },
                        )
                        .inspect(|resolution| {
                            self.cache_positive_import_route_result(
                                canonical_id,
                                &dep.import_source,
                                &resolution.source_id,
                            );
                        })
                })
                .map(|resolution| resolution.source_id);
            if let Some(resolved) = resolved.filter(|resolved| resolved != canonical_id) {
                blocker_ids.insert(resolved);
            }
        }

        for blocker_id in blocker_ids {
            let _ = self.ensure_loaded(&blocker_id);
        }
    }

    /// R3/R26/R28 cold-compute prefetch: resolve and load the cross-file
    /// dependency surface the compile-tier fact tracer will observe
    /// before the tracer is installed.
    ///
    /// The compile-tier tracer in `observe_compile_tier_dependencies`
    /// reads two pieces of state per macro-type-dep:
    ///
    /// 1. `derived_raw_cache().get(owner).import_routes` — the
    ///    owner's per-import resolution table; needed to translate
    ///    `dep.import_source` to a canonical id.
    /// 2. `VerterHost::current_content_pinned_artifacts(dep)` — the
    ///    dependency's content-pinned `FileArtifacts` entry; needed to
    ///    look up the `Member` / `MemberPresence` fact hashes. The
    ///    content pin (scheduler-authoritative hash, artifact-only
    ///    fallback) keeps the lookup off a stale lingering artifact.
    ///
    /// On a cold compute of the owner SFC neither of those is
    /// pre-populated, so without this prefetch the tracer silently
    /// records an empty signature and the consumer would never
    /// invalidate on a cross-file edit.
    ///
    /// Strategy: prefetch the dependency surface OUTSIDE the tracer
    /// scope (so the load itself is not part of the observed read
    /// set). For each macro-type-dep:
    ///
    /// - Resolve `dep.import_source` via `workspace.resolve_import`
    ///   (Type-import first, ESM-import fallback) and cache the route
    ///   in `derived_raw_cache().import_routes`.
    /// - Call `ensure_indexed_ready(dep_canonical)` to publish the
    ///   dependency's `IndexedReady` into `FileArtifactStore`. Just
    ///   `ensure_loaded` is insufficient — fact lookup reads the
    ///   indexed-artifact's `facts` registry, which is only populated
    ///   by the indexed-ready materialiser.
    ///
    /// Script imports (used by the augmentation observation) reach
    /// `FileArtifactStore` via `ensure_indexed_ready` on each
    /// resolvable specifier. Unresolved specifiers (external packages
    /// without a workspace fallback) are skipped: the augmentation
    /// observation uses the index-level fingerprint snapshot rather
    /// than per-canonical artifacts and tolerates a missing canonical.
    fn prefetch_compile_tier_observation_targets(
        &self,
        owner_canonical: &str,
        script_imports: &[verter_semantic::analysis::AnalyzedImport],
        macro_type_deps: &[verter_semantic::analysis::MacroTypeDep],
        external_requests: &[ExternalSourceRequest],
    ) {
        // Owner's indexed-ready must be present so the tracer can
        // resolve owner-relative import surfaces; the owner's own
        // FileArtifactStore entry is also a producer-side dependency
        // of route observation (R26).
        let _ = self.ensure_indexed_ready(owner_canonical);

        let workspace = self.workspace();
        let mut resolved_deps = std::collections::BTreeSet::<String>::new();

        // Macro-type deps: TypeImport first, ESM fallback. Cache the
        // import-route so `resolve_import_source_to_canonical` in
        // `compile_fact_emission` finds it.
        for dep in macro_type_deps {
            let resolved = workspace
                .resolve_import(
                    owner_canonical,
                    &dep.import_source,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: verter_workspace::ResolveRequestKind::TypeImport,
                    },
                )
                .or_else(|| {
                    workspace.resolve_import(
                        owner_canonical,
                        &dep.import_source,
                        verter_workspace::ResolutionContext {
                            phase: verter_workspace::ResolvePhase::CodegenBlocker,
                            kind: verter_workspace::ResolveRequestKind::EsmImport,
                        },
                    )
                });
            if let Some(resolution) = resolved {
                self.cache_positive_import_route_result(
                    owner_canonical,
                    &dep.import_source,
                    &resolution.source_id,
                );
                if resolution.source_id != owner_canonical {
                    resolved_deps.insert(resolution.source_id);
                }
            }
        }

        // Script imports: cache the import route + indexed-ready so
        // the tracer's ImportRef + augmentation observations have
        // populated state. Type-only imports use the TypeImport
        // phase; value imports use EsmImport.
        for import in script_imports {
            let kind = if import.is_type_only {
                verter_workspace::ResolveRequestKind::TypeImport
            } else {
                verter_workspace::ResolveRequestKind::EsmImport
            };
            if let Some(resolution) = workspace.resolve_import(
                owner_canonical,
                import.source.as_str(),
                verter_workspace::ResolutionContext {
                    phase: verter_workspace::ResolvePhase::CodegenBlocker,
                    kind,
                },
            ) {
                self.cache_positive_import_route_result(
                    owner_canonical,
                    import.source.as_str(),
                    &resolution.source_id,
                );
                if resolution.source_id != owner_canonical {
                    resolved_deps.insert(resolution.source_id);
                }
            }
        }

        // External `src=` blocks. The compile-tier producer observes
        // a `FileWholeHash` of each external canonical, so each
        // external dep must reach the store before the tracer runs.
        //
        // A pre-existing import-route for the `src=` specifier is
        // AUTHORITATIVE — it was placed by `set_import_dependencies`
        // (caller-supplied exact resolution) or by an earlier
        // resolution pass. This prefetch must NOT overwrite it: an
        // aliased `src=` (`@/partials/panel.html`) does not resolve
        // through `workspace.resolve_import`, so re-resolving and
        // caching the fallback would clobber the correct caller route
        // and break external-source merging. The prefetch only
        // authors a route when none exists yet.
        for request in external_requests {
            let existing_route = self
                .derived_raw_cache()
                .get(owner_canonical)
                .and_then(|d| d.import_routes.get(&request.specifier).cloned());
            let resolved = if let Some(route) = existing_route {
                // Use the authoritative route's canonical for the
                // indexed-ready prefetch; leave the route untouched.
                route
                    .resolved_canonical_id
                    .clone()
                    .or_else(|| route.effective_target().map(str::to_string))
                    .unwrap_or_else(|| request.resolved_canonical_id.clone())
            } else {
                // No route yet — resolve through the SfcSrcAttr phase
                // and cache the result so the producer's
                // `resolve_import_source_to_canonical` finds it. Fall
                // back to the parse-time canonical when the workspace
                // cannot resolve the specifier.
                let resolved = workspace
                    .resolve_import(
                        owner_canonical,
                        &request.specifier,
                        verter_workspace::ResolutionContext {
                            phase: verter_workspace::ResolvePhase::CodegenBlocker,
                            kind: verter_workspace::ResolveRequestKind::SfcSrcAttr,
                        },
                    )
                    .map(|resolution| resolution.source_id)
                    .unwrap_or_else(|| request.resolved_canonical_id.clone());
                if !resolved.is_empty() && resolved != owner_canonical {
                    self.cache_positive_import_route_result(
                        owner_canonical,
                        &request.specifier,
                        &resolved,
                    );
                }
                resolved
            };
            if !resolved.is_empty() && resolved != owner_canonical {
                resolved_deps.insert(resolved);
            }
        }

        // Drive each resolved dep to IndexedReady so its
        // `FileArtifactStore` entry (including the `facts` registry)
        // is published before the tracer queries fact hashes. Calls
        // are idempotent / cache-hit on warm reads.
        for dep_canonical in resolved_deps {
            let _ = self.ensure_indexed_ready(&dep_canonical);
        }
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn ensure_compiled(
        &self,
        canonical_id: &str,
        profile: &CompileProfile,
    ) -> Result<(), HostError> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let profile_hash = compile_profile_hash(profile);

        // Check cache
        {
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
                if let Some(cc) = self.compile_cache().get(&canonical) {
                    let soh = cc
                        .style_overrides
                        .get(&profile_hash)
                        .map(|o| o.hash)
                        .unwrap_or(0);
                    let coh = cc
                        .content_overrides
                        .get(&profile_hash)
                        .map(|o| o.layer.hash)
                        .unwrap_or(0);
                    if let Some(slot) = cc.compile_slots.get(&profile_hash) {
                        // R3/R26/R28: the warm hit must validate the
                        // SAME predicate `get_virtual_file` /
                        // `compile_slot_is_warm` use — own-content
                        // identity (`semantic_hash`), both override
                        // hashes, AND the cross-file fact signature.
                        // `semantic_hash` only covers this canonical's
                        // own content; a cross-file dependency edit
                        // (runtime import, macro type dep, external
                        // `src=` file) surfaces solely through
                        // `compile_slot_fact_signature_validates`.
                        // Omitting it here would serve a stale slot
                        // and return `Ok(())` without recompiling.
                        if slot.semantic_hash == hd.parse.semantic_hash
                            && slot.style_override_hash == soh
                            && slot.content_override_hash == coh
                            && self.compile_slot_fact_signature_validates(slot)
                        {
                            return Ok(());
                        }
                    }
                }
            }
        }

        self.hydrate_compile_blockers(&canonical);

        // Cache miss â€” compile by requesting the Main virtual file.
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

    /// R3/R26/R28 warm-hit fact-signature oracle.
    ///
    /// Validates every fact recorded on `slot.fact_dep_signature`
    /// against the host's current `HostStoreView`. A single mismatch
    /// returns `false` and the warm hit misses; the caller falls
    /// through to cold recompute. Empty signature (cold-compute
    /// paths not yet wired to the tracer) trivially validates so
    /// the existing pre-fact semantic_hash / override-hash fast path
    /// remains the gating predicate until every compile-tier cold
    /// path wires the tracer.
    ///
    /// `O(signature.len())` per call, zero allocation on the empty
    /// path. Validation reads through the `HostStoreView` snapshot
    /// captured at the start of the request; concurrent edits do
    /// NOT race against this read.
    #[inline]
    pub(crate) fn compile_slot_fact_signature_validates(
        &self,
        slot: &crate::types::CompileSlot,
    ) -> bool {
        if slot.fact_dep_signature.is_empty() {
            return true;
        }
        let view = self.resolver_store_view();
        use crate::resolver_core::StoreView;
        slot.fact_dep_signature
            .iter()
            .all(|fact| view.validates(fact))
    }

    /// Read-only predicate: would `get_virtual_file(query)` for this
    /// `(canonical_id, profile)` hit the compile cache without doing any
    /// work?
    ///
    /// Mirrors the freshness predicate the writer uses inside
    /// `get_virtual_file` (`slot.semantic_hash == parse.semantic_hash
    /// && slot.style_override_hash == soh && slot.content_override_hash
    /// == coh && fact-signature validates`). The predicate stays in
    /// lockstep with the writer; if the writer's predicate ever
    /// changes, this accessor changes with it.
    ///
    /// R3 fact-validation gates the warm hit: the `slot.semantic_hash`
    /// check covers the owning canonical's own content identity, but
    /// cross-file dependency edits (e.g. `/src/types.ts` mutates while
    /// `/src/Comp.vue` is unchanged) only surface through
    /// `compile_slot_fact_signature_validates`. A consumer with a
    /// stale fact_dep_signature lookup mismatches the active view
    /// here and the predicate returns `false`, which routes the
    /// caller through cold recompute.
    pub fn compile_slot_is_warm(&self, canonical_id: &str, profile: &CompileProfile) -> bool {
        use crate::host_executor::HostSourceData;
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let profile_hash = compile_profile_hash(profile);

        let snap = match self.scheduler.try_get_source(&canonical) {
            Some(s) => s,
            None => return false,
        };
        let hd = match snap.downcast_data::<HostSourceData>() {
            Some(h) => h,
            None => return false,
        };
        let parse = &hd.parse;

        let cc = match self.compile_cache().get(&canonical) {
            Some(c) => c,
            None => return false,
        };
        let soh = cc
            .style_overrides
            .get(&profile_hash)
            .map(|o| o.hash)
            .unwrap_or(0);
        let coh = cc
            .content_overrides
            .get(&profile_hash)
            .map(|o| o.layer.hash)
            .unwrap_or(0);
        let slot = match cc.compile_slots.get(&profile_hash) {
            Some(s) => s,
            None => return false,
        };
        slot.semantic_hash == parse.semantic_hash
            && slot.style_override_hash == soh
            && slot.content_override_hash == coh
            && self.compile_slot_fact_signature_validates(slot)
    }

    /// Public R3/R26/R28 inspector: returns a clone of the compile
    /// slot's `fact_dep_signature` for the given `(canonical, profile)`
    /// pair, or `None` if no slot has been admitted.
    ///
    /// Used by integration tests + downstream observability to verify
    /// the producer actually recorded the cross-file fact set the
    /// consumer's read-side fact-validation oracle depends on.
    pub fn compile_slot_fact_dep_signature(
        &self,
        canonical_id: &str,
        profile: &CompileProfile,
    ) -> Option<Arc<[crate::resolver_core::FactVersionRef]>> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let profile_hash = compile_profile_hash(profile);
        self.compile_cache().get(&canonical).and_then(|cc| {
            cc.compile_slots
                .get(&profile_hash)
                .map(|s| Arc::clone(&s.fact_dep_signature))
        })
    }

    /// Retrieve a compiled virtual file (script, template, style, or main bundle).
    ///
    /// On cache hit, returns immediately. On cache miss, compiles the file using
    /// `verter_compiler::compile`, caches the result, and returns the requested node.
    /// In dev mode with [`CompileErrorPolicy::DevServeLastKnownGood`], falls back
    /// to the last successful compilation when the current source has errors.
    pub fn get_virtual_file(&self, query: VirtualQuery) -> Result<VirtualFileResponse, HostError> {
        #[cfg(feature = "session_metrics")]
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
        let sched_snapshot_at_start = self.scheduler.try_get_source(&canonical_id);

        let cache_miss = {
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

                let cc_ref = self.compile_cache().get(&canonical_id);

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
                            && self.compile_slot_fact_signature_validates(slot)
                        {
                            #[cfg(feature = "session_metrics")]
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
                let style_analyses: Arc<Vec<verter_semantic::analysis::StyleBlockAnalysis>> =
                    analysis_snap
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
                        script_macros: efs.script_analysis.macros.clone(),
                        script_bindings: efs.script_analysis.bindings.clone(),
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
        };

        let CacheMiss {
            compile_input,
            fallback_last_good,
            meta,
            semantic_hash: captured_semantic_hash,
        } = cache_miss;

        #[cfg(feature = "session_metrics")]
        self.metrics
            .compile_requests
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        #[cfg(feature = "session_metrics")]
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

        // R3/R26/R28 cold-compute prefetch (pre-tracer): make sure
        // the cross-file dependency surface the tracer will observe is
        // resolved + indexed-ready before the tracer is installed.
        // Performed outside `with_fact_tracer` so that load / index
        // mutations are not folded into the consumer's observed read
        // set. See `prefetch_compile_tier_observation_targets` for
        // the contract.
        self.prefetch_compile_tier_observation_targets(
            &canonical_id,
            &compile_input.script_imports,
            &compile_input.macro_type_deps,
            &compile_input.external_requests,
        );

        // R3/R26/R28 cold-compute fact-observation scope. The tracer
        // accumulates every cross-file fact (per-`Member` /
        // `MemberPresence` for macro type deps, `ImportRef` per script
        // import, `ModuleAugmentationIndexShape` per augmented
        // specifier) that the compile pass reads. The finalised
        // signature lands on the new `CompileSlot.fact_dep_signature`
        // and validates the slot on every warm-hit read.
        let (compile_result, fact_read_set) = self.with_fact_tracer(|| {
            crate::compile_fact_emission::observe_compile_tier_dependencies(
                self,
                &canonical_id,
                &compile_input.script_imports,
                &compile_input.macro_type_deps,
                &compile_input.external_requests,
            );
            self.compile_entry(&compile_input, &query.compile_profile)
        });
        let compile_fact_dep_signature =
            crate::compile_fact_emission::finalise_signature_or_empty(fact_read_set);
        let (compiled_outputs, diagnostics, stale, compiled_tsx, compiled_template_analysis) =
            match compile_result {
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

        #[cfg(feature = "session_metrics")]
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
        {
            if let Some(mut cc) = self.compile_cache().get_mut(&canonical_id) {
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
                        fact_dep_signature: Arc::clone(&compile_fact_dep_signature),
                    },
                );
                cc.latest_diagnostics
                    .insert(profile_hash, diagnostics.clone());
                cc.diagnostics_generation += 1;
            }
        }

        // Commit to scheduler artifact snapshot (scheduler path only).
        {
            // Persist raw template analysis on DerivedRawState (D48
            // split — profileless source-derived cache). Only for
            // non-override compiles.
            if compiled_template_analysis.is_some()
                && compile_input.content_override_layer.is_none()
            {
                let mut derived_ref = self
                    .derived_raw_cache()
                    .entry(canonical_id.clone())
                    .or_default();
                derived_ref.value_mut().raw_template_analysis =
                    compiled_template_analysis.clone().map(Arc::new);
            }

            if let Some(ref snap) = sched_snapshot_at_start {
                self.scheduler.commit_artifact(
                    &canonical_id,
                    profile_hash,
                    verter_scheduler::node::ArtifactSnapshot {
                        generation: snap.generation,
                        profile_hash,
                        data: Arc::new(crate::host_executor::HostArtifactData {
                            outputs: compiled_outputs.clone(),
                            diagnostics: diagnostics.clone(),
                        }),
                    },
                );
            }
        }

        // Write per-profile state to files (WASM path only).

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

        {
            if self.is_canonical_evicted(&canonical) {
                return None;
            }
            let cc = self.compile_cache().get(&canonical)?;
            let slot = cc.compile_slots.get(&profile_hash)?;
            let tsx = slot.tsx.as_ref()?;
            Some(IdeResponse {
                code: tsx.code.clone(),
                source_map: tsx.source_map.clone(),
                is_jsx: tsx.is_jsx,
                destructured_block: tsx.destructured_block.clone(),
            })
        }
    }

    /// Generate public API output for a Vue SFC â€” minimal TypeScript declarations.
    ///
    /// Unlike [`get_ide`](Self::get_ide), this does NOT require a prior
    /// [`get_virtual_file`](Self::get_virtual_file) call. It performs
    /// macro-only extraction (OXC parse â†’ defineProps/Emits/Model/Options)
    /// and generates a `ComponentPublicInstance`-based declaration.
    ///
    /// Returns `None` if the file is not in the host or not a Vue SFC.
    pub fn get_public_api(&self, canonical_id: &str) -> Option<TscResponse> {
        self.get_public_api_with_mode(canonical_id, PublicApiMode::Public, None)
    }

    /// Generate public API output for a Vue SFC using the requested surface mode.
    ///
    /// `PublicApiMode::Public` matches the default application-facing instance shape.
    /// `PublicApiMode::Testing` exposes internal `<script setup>` bindings in a
    /// Vue Test Utils-like debug surface.
    ///
    /// When `profile` is provided, script/content overrides for that compile
    /// profile are reflected in the generated API surface.
    pub fn get_public_api_with_mode(
        &self,
        canonical_id: &str,
        mode: PublicApiMode,
        profile: Option<&CompileProfile>,
    ) -> Option<TscResponse> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let profile_hash = profile.map(compile_profile_hash);

        if self.is_canonical_evicted(&canonical) {
            return None;
        }

        let (source, file_kind, macro_type_deps, script_imports, cached_extract, whole_hash) = {
            let efs = self.effective_file_state(&canonical, profile_hash)?;
            let file_kind = self.scheduler.try_get_source(&canonical).and_then(|snap| {
                snap.downcast_data::<crate::host_executor::HostSourceData>()
                    .map(|hd| hd.file_kind)
            })?;
            if file_kind != FileKind::VueSfc {
                return None;
            }
            // cached_tsc_extract lives on DerivedRawState (D48 split).
            let cached = self.derived_raw_cache().get(&canonical).and_then(|cc| {
                cc.cached_tsc_extract.as_ref().and_then(|(hash, extract)| {
                    if *hash == efs.whole_hash {
                        Some(Arc::clone(extract))
                    } else {
                        None
                    }
                })
            });
            (
                efs.source,
                file_kind,
                efs.script_analysis.macro_type_deps.clone(),
                efs.script_analysis.imports.clone(),
                cached,
                efs.whole_hash,
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
        let store_view = self.resolver_store_view();
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(self, &store_view, overlay);
        let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
        let (external_types, _, transitive_macro_type_deps) = self
            .collect_external_types_from_loaded_files(
                ctx,
                &canonical,
                &macro_type_deps,
                &script_imports,
                profile_hash,
            );
        self.sync_transitive_macro_type_dependencies(&canonical, &transitive_macro_type_deps);
        let tsc_mode = match mode {
            PublicApiMode::Public => verter_compiler::tsc::TscMode::Public,
            PublicApiMode::Testing => verter_compiler::tsc::TscMode::Testing,
        };

        // Try cached extract path: avoids re-parsing SFC + OXC on cache hit.
        let extract = if let Some(cached) = cached_extract {
            cached
        } else if let Some(fresh) = verter_compiler::tsc::extract_tsc_state(
            &source,
            &component_name,
            &verter_compiler::tsc::TscExtractOptions {
                filename: Some(canonical.clone()),
            },
        ) {
            let arc = Arc::new(fresh);
            {
                // cached_tsc_extract lives on DerivedRawState (D48 split).
                let mut derived_ref = self
                    .derived_raw_cache()
                    .entry(canonical.clone())
                    .or_default();
                derived_ref.value_mut().cached_tsc_extract = Some((whole_hash, Arc::clone(&arc)));
            }

            arc
        } else {
            // No <script setup> â€” fall through to direct path for empty stub
            let tsc_out = verter_compiler::tsc::generate_tsc_output_with_options(
                &source,
                &component_name,
                &verter_compiler::tsc::TscGenOptions {
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

        let tsc_out = verter_compiler::tsc::generate_tsc_from_state(
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
        if let Some(mut cc) = self.compile_cache().get_mut(canonical_id) {
            cc.latest_diagnostics.insert(profile_hash, diagnostics);
            cc.diagnostics_generation += 1;
        }
    }

    #[allow(clippy::type_complexity)]
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn compile_entry(
        &self,
        snapshot: &CompileInput,
        profile: &CompileProfile,
    ) -> Result<
        (
            FxHashMap<VirtualNodeKind, CachedVirtualFile>,
            DiagnosticsSnapshot,
            Option<CachedTsx>,
            Option<verter_semantic::analysis::template::TemplateAnalysisSnapshot>,
        ),
        DiagnosticsSnapshot,
    > {
        let mut diagnostics = snapshot.parse_diagnostics.clone();

        let mut merged_source = snapshot.source.to_string();
        if !snapshot.src_blocks.is_empty() {
            let ext_sources = {
                let mut map = FxHashMap::default();
                for req in &snapshot.external_requests {
                    if let Some(dep_source) = self.resolve_dep_source(
                        &snapshot.canonical_id,
                        &req.resolved_canonical_id,
                        &req.specifier,
                    ) {
                        map.insert(req.resolved_canonical_id.clone(), dep_source);
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
            // assemble_main_module, so inline mode must be off â€” otherwise the
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
        let profile_hash = compile_profile_hash(profile);

        let store_view = self.resolver_store_view();
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(self, &store_view, overlay);
        let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
        let (external_types, missing_macro_type_diags, transitive_macro_type_deps) = self
            .collect_external_types_from_loaded_files(
                ctx,
                &snapshot.canonical_id,
                &snapshot.macro_type_deps,
                &snapshot.script_imports,
                Some(profile_hash),
            );
        self.sync_transitive_macro_type_dependencies(
            &snapshot.canonical_id,
            &transitive_macro_type_deps,
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
            prop_constness_overrides: None, // TODO: populated by cross-file optimizer,
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
                            verter_compiler::compile::CompileDiagnosticSeverity::Error => {
                                HostSeverity::Error
                            }
                            verter_compiler::compile::CompileDiagnosticSeverity::Warning => {
                                HostSeverity::Warning
                            }
                            verter_compiler::compile::CompileDiagnosticSeverity::Info => {
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

        // Combined IDE output (TSX/JSX) for LSP type checking â€” stored separately, not as virtual file
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
            // Build script import pairs for component â†’ source resolution
            let (all_imports, binding_class_unions, props_binding_name) = template_converter_inputs(
                &snapshot.script_imports,
                &snapshot.script_macros,
                &snapshot.script_bindings,
            );
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

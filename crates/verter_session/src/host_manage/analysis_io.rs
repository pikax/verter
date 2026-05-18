//! `host_manage::analysis_io` — file analysis / source / template /
//! diagnostics / files / set-import-dependencies / css-var-flow /
//! export-graph methods.
//!
//! Domain J. Holds the public host
//! file-management surface (`get_source`, `get_analysis*`,
//! `get_diagnostics*`, `remove`, `list_files`, `list_virtual_nodes`,
//! etc.) plus the supporting analysis-snapshot pipeline, template
//! analysis materialisation, css-var-flow inspection, and
//! export-graph resolution helpers. Public surface remains rooted at
//! `crate::host_manage::*`; this file contributes a continuation
//! `impl VerterHost { … }` block.

use std::sync::Arc;

use crate::instant::Instant;

use crate::hash::compile_profile_hash;
use crate::id::canonicalize_id;
use crate::resolver_core::{
    get_export_span_follow_reexports_from_graph as resolver_get_export_span_follow_reexports_from_graph,
    resolve_exports_from_graph_best_effort as resolver_resolve_exports_from_graph_best_effort,
    resolve_named_export_from_graph as resolver_resolve_named_export_from_graph,
};
use crate::shared::write_lock;
use crate::types::*;
use crate::VerterHost;

use super::{
    component_meta_debug, component_meta_debug_enabled,
    exact_resolution_uses_type_preferred_target, HostExportGraphResolver,
};

impl VerterHost {
    pub fn get_source(&self, canonical_or_alias: &str) -> Option<std::sync::Arc<str>> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        {
            if self.is_canonical_evicted(&canonical) {
                return None;
            }
            self.scheduler
                .try_get_source(&canonical)
                .map(|s| s.source.clone())
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_template_analysis(
        &self,
        canonical: &str,
        source: &Arc<str>,
        cached_parse: Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
        src_blocks: &[crate::SrcBlockInfo],
        external_requests: &[crate::ExternalSourceRequest],
        imports: &[verter_semantic::analysis::AnalyzedImport],
        macros: &[verter_semantic::analysis::AnalyzedMacro],
        bindings: &[verter_semantic::analysis::AnalyzedBinding],
    ) -> Option<Arc<verter_semantic::analysis::template::TemplateAnalysisSnapshot>> {
        let ext_map = if !src_blocks.is_empty() {
            let mut map = rustc_hash::FxHashMap::default();
            for req in external_requests {
                let dep_source =
                    self.resolve_dep_source(canonical, &req.resolved_canonical_id, &req.specifier);
                if let Some(source) = dep_source {
                    map.insert(req.resolved_canonical_id.clone(), source);
                }
            }
            map
        } else {
            rustc_hash::FxHashMap::default()
        };

        for req in external_requests {
            if !ext_map.contains_key(&req.resolved_canonical_id) {
                return None;
            }
        }

        let merged_source = if !src_blocks.is_empty() {
            std::borrow::Cow::Owned(crate::compile::merge_external_sources(
                source, src_blocks, &ext_map,
            ))
        } else {
            std::borrow::Cow::Borrowed(source.as_ref())
        };

        let parsed = if src_blocks.is_empty() {
            cached_parse
                .as_deref()
                .map(std::borrow::Cow::Borrowed)
                .unwrap_or_else(|| {
                    std::borrow::Cow::Owned(verter_compiler::compile::parse_sfc(
                        &merged_source,
                        None,
                        None,
                    ))
                })
        } else {
            std::borrow::Cow::Owned(verter_compiler::compile::parse_sfc(
                &merged_source,
                None,
                None,
            ))
        };

        let alloc = oxc_allocator::Allocator::new();
        let options = verter_compiler::compile::CodegenOptions {
            target: verter_compiler::compile::CompileTarget::META,
            filename: Some(canonical.to_string()),
            ..verter_compiler::compile::CodegenOptions::default()
        };
        let verter_opts = verter_compiler::compile::VerterCompileOptions {
            extract_template_data: true,
            ..verter_compiler::compile::VerterCompileOptions::default()
        };
        let compiled = verter_compiler::compile::compile_from_parsed(
            &merged_source,
            &parsed,
            &options,
            &verter_opts,
            &alloc,
        );

        let has_structural_errors = compiled.errors.iter().any(|d| {
            matches!(
                d.severity,
                verter_compiler::compile::CompileDiagnosticSeverity::Error,
            ) && !d.code.starts_with("XInvalidMacroType")
                && !d.code.starts_with("XMissingMacroType")
        });
        if has_structural_errors {
            return None;
        }

        let raw = compiled.template_data?;
        let (imports, unions, props_name) =
            crate::host_resolve::template_converter_inputs(imports, macros, bindings);
        Some(Arc::new(crate::template_convert::convert_raw_to_analysis(
            &raw,
            &imports,
            &unions,
            props_name.as_deref(),
        )))
    }

    /// Lazily compute template analysis for a VueSfc file that hasn't been compiled.
    ///
    /// Uses `CompileTarget::META` (= SCRIPT + TEMPLATE_DATA) via the core
    /// `compile_from_parsed()` â€” bypassing the host `compile_entry()` which fails
    /// on unresolved macro type deps. External-src blocks are merged using the
    /// same `merge_external_sources()` helper. Results are persisted on the entry
    /// for inline-template files to avoid recomputation.
    pub(crate) fn compute_template_analysis_if_missing(
        &self,
        canonical: &str,
        snapshot: &mut FileAnalysisSnapshot,
    ) {
        if snapshot.template.is_some() {
            return;
        }
        if !canonical.ends_with(".vue") {
            return;
        }

        let (source, cached_parse, src_blocks, external_requests) = {
            use crate::host_executor::HostSourceData;
            if let Some(snap) = self.scheduler.try_get_source(canonical) {
                let Some(hd) = snap.downcast_data::<HostSourceData>() else {
                    return;
                };
                if hd.file_kind != FileKind::VueSfc {
                    return;
                }
                (
                    snap.source.clone(),
                    hd.cached_parse.clone(),
                    hd.parse.src_blocks.clone(),
                    hd.parse.external_requests.clone(),
                )
            } else {
                let Some(source) = self.read_analysis_source(canonical) else {
                    return;
                };
                let (parse, parsed) = crate::parse::parse_vue_snapshot(
                    canonical,
                    &source,
                    self.config.effective_scope(),
                );
                (
                    source,
                    Some(Arc::new(parsed)),
                    parse.src_blocks,
                    parse.external_requests,
                )
            }
        };

        // Resolve external src blocks (e.g., <template src="./tpl.html">)
        let ext_map = if !src_blocks.is_empty() {
            let mut map = rustc_hash::FxHashMap::default();
            for req in &external_requests {
                if let Some(dep_source) =
                    self.resolve_dep_source(canonical, &req.resolved_canonical_id, &req.specifier)
                {
                    map.insert(req.resolved_canonical_id.clone(), dep_source);
                }
            }
            map
        } else {
            rustc_hash::FxHashMap::default()
        };

        // Abort if any external src blocks are unresolved (same guard as compile_entry)
        for req in &external_requests {
            if !ext_map.contains_key(&req.resolved_canonical_id) {
                return;
            }
        }

        let merged_source = if !src_blocks.is_empty() {
            std::borrow::Cow::Owned(crate::compile::merge_external_sources(
                &source,
                &src_blocks,
                &ext_map,
            ))
        } else {
            std::borrow::Cow::Borrowed(source.as_ref())
        };

        // Parse SFC (reuse cached parse when no external src)
        let parsed = if src_blocks.is_empty() {
            cached_parse
                .as_deref()
                .map(std::borrow::Cow::Borrowed)
                .unwrap_or_else(|| {
                    std::borrow::Cow::Owned(verter_compiler::compile::parse_sfc(
                        &merged_source,
                        None,
                        None,
                    ))
                })
        } else {
            std::borrow::Cow::Owned(verter_compiler::compile::parse_sfc(
                &merged_source,
                None,
                None,
            ))
        };

        // Compile with META target â€” script codegen + template data, no JS/TSX output
        let alloc = oxc_allocator::Allocator::new();
        let options = verter_compiler::compile::CodegenOptions {
            target: verter_compiler::compile::CompileTarget::META,
            filename: Some(canonical.to_string()),
            ..verter_compiler::compile::CodegenOptions::default()
        };
        let verter_opts = verter_compiler::compile::VerterCompileOptions {
            extract_template_data: true,
            ..verter_compiler::compile::VerterCompileOptions::default()
        };
        let compiled = verter_compiler::compile::compile_from_parsed(
            &merged_source,
            &parsed,
            &options,
            &verter_opts,
            &alloc,
        );

        // Bail on structural compile errors that would invalidate template data.
        // Skip type-resolution errors (XInvalidMacroType, XMissingMacroType) since
        // template slot extraction doesn't depend on type resolution.
        let has_structural_errors = compiled.errors.iter().any(|d| {
            matches!(
                d.severity,
                verter_compiler::compile::CompileDiagnosticSeverity::Error,
            ) && !d.code.starts_with("XInvalidMacroType")
                && !d.code.starts_with("XMissingMacroType")
        });
        if has_structural_errors {
            return;
        }

        // Convert RawTemplateData â†’ TemplateAnalysisSnapshot using existing converter
        if let Some(raw) = compiled.template_data {
            // Build converter inputs from snapshot (already computed, not stale entry)
            let imports: Vec<(String, String)> = snapshot
                .imports
                .iter()
                .flat_map(|imp| {
                    imp.bindings
                        .iter()
                        .map(|b| (b.name.clone(), imp.source.clone()))
                })
                .collect();

            // Build binding_class_unions + props_binding_name from snapshot
            let mut unions: Vec<(String, Vec<String>)> = Vec::new();
            let define_props = snapshot
                .macros
                .iter()
                .find(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps);
            if let Some(dp) = define_props {
                for field in &dp.prop_fields {
                    if let Some(type_ann) = &field.type_annotation {
                        let classes =
                            verter_semantic::analysis::parse_string_literal_union(type_ann);
                        if !classes.is_empty() {
                            unions.push((field.name.clone(), classes));
                        }
                    }
                }
            }
            for binding in &snapshot.bindings {
                if let Some(type_ann) = &binding.type_annotation {
                    let effective = verter_semantic::analysis::unwrap_reactive_type(type_ann)
                        .unwrap_or(type_ann);
                    let classes = verter_semantic::analysis::parse_string_literal_union(effective);
                    if !classes.is_empty() {
                        unions.push((binding.name.clone(), classes));
                    }
                }
            }
            let props_name = define_props.and_then(|dp| dp.binding_name.clone());

            let tpl = crate::template_convert::convert_raw_to_analysis(
                &raw,
                &imports,
                &unions,
                props_name.as_deref(),
            );
            let tpl_arc = Arc::new(tpl);
            snapshot.template = Some(Arc::clone(&tpl_arc));

            // Persist for inline templates only. Files with external src
            // blocks are NOT persisted to avoid stale cache when the external
            // dep changes (reverse-dep invalidation only clears compile_slots).
            if src_blocks.is_empty() {
                // raw_template_analysis lives on DerivedRawState (D48 split).
                let mut derived_ref = self
                    .derived_raw_cache()
                    .entry(canonical.to_string())
                    .or_default();
                derived_ref.value_mut().raw_template_analysis = Some(tpl_arc);
            }
        }
    }

    pub fn get_analysis(&self, canonical_or_alias: &str) -> Option<FileAnalysisSnapshot> {
        // Route through the view-aware entry point with a `HostViewRef`
        // so the single resolver-tier surface stays view-shaped (R17 / R18).
        // A base-only `HostViewRef` never tombstones and reports `None`
        // from `overlay_content_hash_for`, so the overlay path inside
        // `get_analysis_via_view` is a genuine no-op for this case and
        // the call routes through the existing internal pipeline.
        let view = crate::session_view::HostViewRef::new(self);
        self.get_analysis_via_view(canonical_or_alias, &view)
    }

    pub(super) fn get_analysis_snapshot_internal(
        &self,
        canonical: &str,
        analysis_started: Option<Instant>,
    ) -> Option<FileAnalysisSnapshot> {
        // Eviction gate (scheduler path) — DerivedRawState owns the
        // evicted flag (D48 split).
        if self.is_canonical_evicted(canonical) {
            return None;
        }

        {
            use crate::host_executor::HostSourceData;

            let Some(source_snap) = self.scheduler.try_get_source(canonical) else {
                let source = self.read_analysis_source(canonical)?;
                let snapshot = self.build_snapshot_from_source(canonical, &source);
                return Some(self.finalize_analysis_snapshot(
                    canonical,
                    snapshot,
                    self.config.effective_scope().needs_template_analysis(),
                    analysis_started,
                ));
            };
            let hd = source_snap.downcast_data::<HostSourceData>()?;
            let file_kind = hd.file_kind;
            let source = source_snap.source.clone();
            let cached_parse = hd.cached_parse.clone();

            let scope = self.config.effective_scope();
            if file_kind == FileKind::VueSfc
                && (!scope.needs_script_analysis() || !scope.needs_style_analysis())
            {
                let stored_script = hd.parse.script_analysis.clone();
                let stored_styles = self
                    .scheduler
                    .try_get_analysis(canonical)
                    .and_then(|a| {
                        a.downcast_data::<crate::host_executor::HostAnalysisData>()
                            .map(|ad| Arc::clone(&ad.style_analyses))
                    })
                    .unwrap_or_else(|| Arc::new(Vec::new()));
                let template = self
                    .derived_raw_cache()
                    .get(canonical)
                    .and_then(|cc| cc.raw_template_analysis.clone());
                let export_sigs = self
                    .scheduler
                    .try_get_analysis(canonical)
                    .and_then(|a| {
                        a.downcast_data::<crate::host_executor::HostAnalysisData>()
                            .map(|ad| ad.export_signatures.clone())
                    })
                    .unwrap_or_default();
                drop(source_snap);

                let mut script_analysis = if !scope.needs_script_analysis() {
                    if let Some(parsed) = cached_parse.as_deref() {
                        crate::parse::build_script_analysis_from_parsed(parsed, &source)
                    } else {
                        crate::parse::build_script_analysis_from_source(&source)
                    }
                } else {
                    stored_script
                };
                let style_analyses = if !scope.needs_style_analysis() {
                    if let Some(parsed) = cached_parse.as_deref() {
                        Arc::new(crate::parse::build_style_analyses_from_parsed(
                            parsed, &source, canonical,
                        ))
                    } else {
                        Arc::new(crate::parse::build_style_analyses_from_source(
                            &source, canonical,
                        ))
                    }
                } else {
                    stored_styles
                };
                if !style_analyses.is_empty() && !script_analysis.bindings.is_empty() {
                    script_analysis.mark_bindings_used_in_style(&style_analyses);
                }
                let snapshot = FileAnalysisSnapshot {
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
                    script_binding_occurrences: Arc::new(
                        script_analysis.script_binding_occurrences,
                    ),
                    export_signatures: Arc::new(export_sigs),
                    options_api: script_analysis.options_api,
                    store_usages: Arc::new(script_analysis.store_usages),
                    store_definitions: Arc::new(script_analysis.store_definitions),
                    is_typescript: script_analysis.is_typescript,
                };
                return Some(self.finalize_analysis_snapshot(
                    canonical,
                    snapshot,
                    scope.needs_template_analysis(),
                    analysis_started,
                ));
            }
            drop(source_snap);

            let snapshot = self.build_snapshot_from_scheduler(canonical)?;
            Some(self.finalize_analysis_snapshot(
                canonical,
                snapshot,
                self.config.effective_scope().needs_template_analysis(),
                analysis_started,
            ))
        }
    }

    /// View-aware variant of [`Self::get_analysis`].
    ///
    /// R17 / R18 — Consults the supplied [`SessionView`] for an overlay
    /// before falling back to the base-host analysis path:
    ///
    /// 1. If `view.is_tombstoned(canonical)` → returns `None`
    ///    (overlay-Deleted canonical is hidden from the consumer).
    /// 2. If `view.overlay_content_hash_for(canonical)` is `Some` (the
    ///    view carries an explicit overlay-Upsert for this canonical)
    ///    → builds the snapshot directly from the overlay source via
    ///    `build_snapshot_from_source`, then runs the same
    ///    `finalize_analysis_snapshot` enrichment (import resolution,
    ///    destructured binding metadata, template analysis on
    ///    demand). The base host's caches are NOT mutated; the
    ///    overlay-shaped snapshot is returned by value.
    /// 3. Otherwise → routes through the existing
    ///    `get_analysis_snapshot_internal` cold path so cached
    ///    artefacts on the base host still serve warm reads.
    ///
    /// Used by `MetaSession::get_analysis` so an overlayed canonical
    /// reports the overlay's analysis content (R17 / R18). Base-only
    /// views (`HostView`, `HostViewRef`) report `None` from
    /// `overlay_content_hash_for`, so they fall through to the existing
    /// flow — the overlay path is a genuine no-op for the base case.
    pub fn get_analysis_via_view(
        &self,
        canonical_or_alias: &str,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<FileAnalysisSnapshot> {
        self.provenance
            .get_analysis_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let analysis_started = component_meta_debug_enabled().then(Instant::now);

        // Tombstone short-circuit (R17): an overlay-Delete is the
        // explicit signal; never inferred from `source().is_none()`
        // (which also fires for unloaded canonicals).
        if view.is_tombstoned(canonical.as_str()) {
            return None;
        }

        // Overlay-source path: when the view carries an **explicit
        // overlay** for this canonical, the analysis must reflect the
        // overlay content.
        //
        // Overlay detection uses the **strict** `overlay_content_hash_for`,
        // NOT a `content_hash_for`-vs-base hash comparison.
        // `content_hash_for` falls through to the base host's
        // `FileArtifactStore`-derived content hash for an unmasked
        // canonical — the same content-agnostic, canonical-only scan
        // as `get_any`, which can surface a STALE lingering artifact's
        // hash once the own-canonical drain is retired. Comparing that
        // stale hash against the scheduler's current `base_hash` would
        // read `overlay_hash != base_hash` for a canonical with NO
        // overlay and re-parse the overlay source when none was needed.
        // `overlay_content_hash_for` reports `Some` ONLY when the
        // session installed an actual overlay-Upsert, so an unmasked
        // canonical correctly falls through to the base path — and a
        // base-passthrough `HostViewRef` (used by the no-overlay
        // `get_analysis` entry point) reports `overlay_covers = false`,
        // restoring the documented "no-op for the base case" invariant.
        let overlay_source = view.source(canonical.as_str());
        let overlay_covers =
            view.overlay_content_hash_for(canonical.as_str()).is_some() && overlay_source.is_some();

        if overlay_covers {
            // Overlay path — parse + analyse the overlay source on
            // the call-thread, then run the same enrichment passes
            // the base path uses. The base host's caches are not
            // mutated (R17 invariant).
            let source =
                overlay_source.expect("overlay_covers true implies overlay_source is Some");
            let snapshot = self.build_snapshot_from_source(canonical.as_str(), &source);
            return Some(self.finalize_analysis_snapshot(
                canonical.as_str(),
                snapshot,
                self.config.effective_scope().needs_template_analysis(),
                analysis_started,
            ));
        }

        // Base path — no overlay coverage; existing flow.
        self.get_analysis_snapshot_internal(&canonical, analysis_started)
    }

    /// View-aware variant of [`Self::evaluate_types`].
    ///
    /// R17 / R18 — Consults the supplied [`SessionView`] for
    /// tombstone detection and overlay-priority source. When the
    /// view carries an overlay for the owner canonical, the
    /// overlay's IndexedReady is pre-warmed into the file-artifact
    /// store via [`crate::resolver_core::SessionResolverContext`]
    /// so the cold compute below reads from the overlay candidate.
    /// The downstream [`Self::resolve_component_meta_with_view`]
    /// threads the view fingerprint into the singleflight cache key
    /// so two sessions with different overlays admit distinct slots.
    pub fn evaluate_types_via_view(
        &self,
        canonical_or_alias: &str,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedComponentTypes> {
        self.provenance
            .evaluate_types_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        // R17 tombstone short-circuit: an overlay-Delete is the
        // explicit signal — base host's evaluate_types must not
        // be consulted.
        if view.is_tombstoned(canonical.as_str()) {
            return None;
        }

        // Overlay-priority pre-warm for owner + every dep the view
        // carries an overlay for.
        {
            crate::host_manage::overlay_priority::prewarm_view_overlays(self, view);
        }

        let resolved = self.resolve_component_meta_with_view(
            canonical_or_alias,
            crate::types::ProjectionMode::Expanded,
            view,
        )?;
        resolved.evaluated_types
    }

    /// Get the current whole_hash for a file.
    pub(crate) fn get_whole_hash(&self, canonical: &str) -> Option<Hash16> {
        {
            use crate::host_executor::HostSourceData;
            // Pre-1C-β semantics required a `compile_cache` entry to exist
            // before consulting the scheduler — an entry absence meant
            // "this canonical is not host-tracked, do not eagerly read
            // the scheduler". Post-D48 split the same gate is "any of
            // the three sub-state DBs has an entry AND the entry is not
            // evicted". We use the source-content-domain DB because
            // `evicted` lives there; absence on derived_raw_cache means
            // the canonical is not host-tracked.
            let entry_visible = self
                .derived_raw_cache()
                .get(canonical)
                .is_some_and(|d| !d.evicted);
            if entry_visible {
                if let Some(snap) = self.scheduler.try_get_source(canonical) {
                    let hd = snap.downcast_data::<HostSourceData>()?;
                    return Some(hd.parse.whole_hash);
                }
            }
            self.project_type_store
                .indexed()
                .get_any(canonical)
                .map(|facts| facts.whole_hash)
        }
    }

    /// Authoritative current content hash for a canonical — the
    /// **scheduler-only** content-hash source with no permissive
    /// fallback.
    ///
    /// Returns the scheduler's `HostSourceData.parse.whole_hash`
    /// **only** when the canonical's [`DerivedRawState`] entry is
    /// visible and not evicted (the same gate
    /// [`Self::get_whole_hash`] applies to its scheduler branch).
    /// Returns `None` otherwise — in particular when the canonical
    /// has been evicted or deleted but a stale `IndexedReady` still
    /// lingers in [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore).
    ///
    /// This is the distinguishing contract: unlike
    /// [`Self::get_whole_hash`], this accessor never falls back to
    /// `FileArtifactStore::get_any` (a content-agnostic
    /// `FileArtifactStore` scan). A `get_any`-derived hash is the
    /// stale artifact's *own* hash,
    /// so feeding it into a content-pinned lookup would resolve the
    /// stale artifact instead of yielding a miss — exactly the
    /// failure a content pin exists to prevent. Content-pinned
    /// callers MUST resolve their hash through here; non-pinning
    /// callers may keep using `get_whole_hash` with its permissive
    /// fallback.
    #[must_use]
    pub(crate) fn authoritative_current_content_hash(&self, canonical: &str) -> Option<Hash16> {
        use crate::host_executor::HostSourceData;
        // Eviction gate — mirrors `get_whole_hash`'s scheduler branch.
        // An evicted entry means the canonical is no longer live; any
        // artifact still in `FileArtifactStore` is stale and must not
        // back a "current content" pin.
        let entry_visible = self
            .derived_raw_cache()
            .get(canonical)
            .is_some_and(|d| !d.evicted);
        if !entry_visible {
            return None;
        }
        let snap = self.scheduler.try_get_source(canonical)?;
        let hd = snap.downcast_data::<HostSourceData>()?;
        Some(hd.parse.whole_hash)
    }

    /// Content-pinned [`crate::project_type_store::IndexedReady`] lookup.
    ///
    /// Resolves the canonical's authoritative current content hash via
    /// [`Self::authoritative_current_content_hash`] (scheduler
    /// `parse.whole_hash`, gated on the entry being non-evicted; no
    /// `get_any` fallback) and reads the artifact store **pinned to
    /// that hash** via
    /// [`crate::file_artifact_store::FileArtifactStore::get_for_current_content`].
    ///
    /// Returns `None` when the canonical has no authoritative current
    /// content hash (unloaded / evicted / deleted) OR when the only
    /// cached artifact is a stale candidate for an older content hash.
    /// Correctness-sensitive readers (route-hash / import-route-hash
    /// fact production) MUST use this instead of the permissive
    /// `get_any`: with eager `evict_canonical` retired a stale
    /// `IndexedReady` can coexist with the live content, and sampling
    /// its `route_hash` / `import_route_hash` as "current" would
    /// confirm a stale cache entry to the fact validator. Deriving the
    /// pin from a `get_any`-backed hash would let the same stale
    /// artifact answer its own pin, so the hash source is restricted
    /// to the authoritative scheduler value.
    #[must_use]
    pub(crate) fn current_content_pinned_indexed(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        let current_hash = self.authoritative_current_content_hash(canonical)?;
        self.project_type_store
            .indexed()
            .get_for_current_content(canonical, current_hash)
    }

    /// Artifact-current [`crate::project_type_store::IndexedReady`]
    /// authority for a canonical that is **genuinely artifact-backed** —
    /// reachable through [`crate::file_artifact_store::FileArtifactStore`]
    /// with no live scheduler `DerivedRawState`.
    ///
    /// A foreign-source-loaded file or a test-seeded `IndexedReady`
    /// that was never registered with the scheduler as a live
    /// `DerivedRawState` has a perfectly valid artifact in
    /// `FileArtifactStore` but no scheduler source — so
    /// [`Self::current_content_pinned_indexed`] (scheduler-pinned)
    /// returns `None` for it. This accessor answers for exactly those
    /// scopes WITHOUT widening the strict scheduler oracle.
    ///
    /// It is NOT a content-agnostic `get_any`: it answers ONLY for a
    /// canonical the scheduler does not track at all — a canonical with
    /// **no `DerivedRawState` entry**. The presence of a `DerivedRawState`
    /// entry is the oracle that splits the three possible states:
    ///
    /// - **no `DerivedRawState` entry** → the canonical is genuinely
    ///   artifact-only (foreign source / test seed); the scheduler is
    ///   not its authority, so the single `FileArtifactStore` artifact
    ///   (`insert` drains prior versions, so there is at most one) is
    ///   the current artifact → `Some`. This is the case this accessor
    ///   exists for.
    /// - **a `DerivedRawState` entry that is `!evicted`** → the
    ///   canonical is a **live scheduler scope**. The scheduler is its
    ///   sole content authority and [`Self::current_content_pinned_indexed`]
    ///   already answered for it (it is tried first). If that pinned
    ///   read missed, the `FileArtifactStore` artifact — if any — is for
    ///   an OLDER content hash than the scheduler's current one; with
    ///   eager `evict_canonical` retired such a stale artifact can
    ///   linger. A `get_any` here would self-root the materialized value
    ///   under that stale hash. So this returns `None` rather than the
    ///   stale artifact — a stale artifact must never be promoted as the
    ///   live scope's current identity.
    /// - **a `DerivedRawState` entry that is `evicted`** → the scope was
    ///   evicted; any surviving `FileArtifactStore` artifact is a stale
    ///   leftover → `None`.
    ///
    /// Returns `None` when no artifact is cached at all.
    #[must_use]
    pub(crate) fn artifact_current_indexed(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        // The artifact-current authority answers ONLY for a canonical
        // with no `DerivedRawState` entry — a genuinely artifact-only
        // scope the scheduler never tracked. Any `DerivedRawState` entry
        // (whether `evicted` or live) means the scheduler is the
        // content authority: a live scope is served by
        // `current_content_pinned_indexed` and a `get_any` artifact
        // would risk self-rooting under a stale older hash; an evicted
        // scope's surviving artifact is a stale leftover.
        if self.derived_raw_cache().get(canonical).is_some() {
            return None;
        }
        self.project_type_store.indexed().get_any(canonical)
    }

    /// Current-content-pinned [`crate::file_artifact_store::FileArtifacts`]
    /// lookup — the `FileArtifacts` analogue of
    /// [`Self::current_content_pinned_indexed`] /
    /// [`Self::artifact_current_indexed`].
    ///
    /// A `FileArtifacts` payload carries the file's parse-domain
    /// `FileFacts` registry. A parse-fact producer that emits a
    /// `ParseFactRef` into a consumer's `fact_dep_signature` MUST read
    /// the fact registry of the **content version it actually
    /// observed** — not whichever artifact a content-agnostic
    /// `get_artifacts_any` walk happens to return. With the
    /// own-canonical drain retired a stale pre-edit `FileArtifacts` can
    /// linger past a same-canonical edit, so a `get_artifacts_any` read
    /// would let the producer fingerprint the stale registry and the
    /// consumer would validate against pre-edit facts.
    ///
    /// Resolution order mirrors [`Self::observe_materialize_scope`]:
    /// 1. The scheduler-authoritative current content hash
    ///    ([`Self::authoritative_current_content_hash`]) pins a strict
    ///    `get_artifacts` read — a content-current artifact, or `None`.
    /// 2. For a genuinely artifact-only canonical (no scheduler
    ///    `DerivedRawState` — foreign source / test seed) the permissive
    ///    `get_artifacts_any` is the current artifact, exactly as
    ///    [`Self::artifact_current_indexed`] reasons for `IndexedReady`.
    ///
    /// Returns `None` when neither answers — a live scheduler scope
    /// whose pinned read missed (a stale older-content candidate is the
    /// only entry), or an evicted scope's stale leftover.
    #[must_use]
    pub(crate) fn current_content_pinned_artifacts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::file_artifact_store::FileArtifacts>> {
        if let Some(current_hash) = self.authoritative_current_content_hash(canonical) {
            let key = crate::file_artifact_store::FileArtifactKey::legacy(
                Arc::from(canonical),
                current_hash,
            );
            return self.project_type_store.indexed().get_artifacts(&key);
        }
        // Genuinely artifact-only canonical — no scheduler authority, so
        // the single retained `FileArtifacts` is the current one.
        if self.derived_raw_cache().get(canonical).is_some() {
            return None;
        }
        self.project_type_store
            .indexed()
            .get_artifacts_any(canonical)
    }

    /// Establish ONE tear-free
    /// [`crate::resolver_core::MaterializeScopeObservation`] for a
    /// materialize-memo scope canonical (base-host path).
    ///
    /// Pins the scope to a single `Arc<IndexedReady>` whose `whole_hash`
    /// roots BOTH the materialiser's lowering `NodeScopeId` and the
    /// `MaterializeMemoDb` signature self-root, plus the scope's
    /// `SyntacticExportSet` parse fact pinned to that same version.
    ///
    /// The artifact is resolved by, in order:
    ///
    /// 1. [`Self::current_content_pinned_indexed`] — the scheduler
    ///    authority, when the scope has a live non-evicted
    ///    `DerivedRawState`. This is the steady-state path.
    /// 2. [`Self::artifact_current_indexed`] — the artifact-current
    ///    authority, for a genuinely artifact-only scope (foreign
    ///    source / test seed) with no scheduler `DerivedRawState`.
    ///
    /// Returns `None` when neither authority answers:
    ///
    /// - a live (non-evicted) scheduler scope whose
    ///   `current_content_pinned_indexed` missed — the scheduler is the
    ///   authority and `artifact_current_indexed` deliberately declines
    ///   (a `get_any` could surface a stale older artifact);
    /// - an evicted / deleted scope whose only `FileArtifactStore`
    ///   artifact is a stale leftover.
    ///
    /// A `None` observation makes the publish site skip shared-cache
    /// admission while still returning the freshly-computed value; it
    /// never lowers under a fabricated all-zero scope hash.
    #[must_use]
    pub(crate) fn observe_materialize_scope(
        &self,
        canonical: &str,
    ) -> Option<crate::resolver_core::MaterializeScopeObservation> {
        let indexed = self
            .current_content_pinned_indexed(canonical)
            .or_else(|| self.artifact_current_indexed(canonical))?;
        let observed_whole_hash = indexed.whole_hash;
        let ctx: &dyn crate::resolver_core::ResolverContext = self;
        let syntactic_export_set =
            crate::fact_signature_helpers::parse_fact_ref_for_observed_current_content(
                ctx,
                canonical,
                observed_whole_hash,
                verter_semantic::facts::FactKey::SyntacticExportSet,
                verter_semantic::facts::FactLane::Semantic,
            );
        Some(crate::resolver_core::MaterializeScopeObservation {
            canonical_id: Arc::from(canonical),
            indexed,
            syntactic_export_set,
        })
    }

    /// Return the scheduler's authoritative [`oxc_span::SourceType`] for a loaded
    /// canonical file, or `None` if the canonical has not been processed by the
    /// scheduler (WASM / unloaded / pre-parse routing).
    ///
    /// Used by cache-key sites that need a stable `source_type` for the same
    /// `(canonical_id, whole_hash)` pair regardless of whether the caller
    /// currently holds the parsed SFC. See [`crate::host_executor::imported_eval_source_type`]
    /// for the pure function the scheduler invokes once at parse time.
    pub(crate) fn authoritative_source_type_for(
        &self,
        canonical: &str,
    ) -> Option<oxc_span::SourceType> {
        use crate::host_executor::HostSourceData;
        let snap = self.scheduler.try_get_source(canonical)?;
        let hd = snap.downcast_data::<HostSourceData>()?;
        Some(hd.source_type)
    }

    /// Test-only accessor for the host-owned named-type cache size.
    #[cfg(test)]
    pub(crate) fn host_owned_resolved_named_types_len_for_test(&self) -> usize {
        self.project_type_store
            .semantic_graph()
            .resolved_named_type_count()
    }

    /// Cheap feasibility predicate for candidate-path probing.
    ///
    /// Replaces the pattern `current_eval_state(&candidate).is_some()`
    /// — which materialized raw source + parse + eval state to answer the
    /// predicate — with a live-host `get_whole_hash` lookup (itself a cheap
    /// `compile_cache` + `FileArtifactStore` lookup, no parse, no disk read).
    ///
    /// Per §4.3 Sub-task B this is the canonical shallow-probe API. Candidate
    /// paths that are not already loaded return `false`; probing does not
    /// implicitly trigger loading.
    pub(crate) fn is_evalable(&self, canonical: &str) -> bool {
        self.get_whole_hash(canonical).is_some()
    }

    /// Returns the semantic hash for a file by canonical ID or alias.
    ///
    /// The semantic hash changes when the file's semantically significant content
    /// changes (script, template, scoped styles). Returns `None` for missing files.
    pub fn get_semantic_hash(&self, canonical_or_alias: &str) -> Option<Hash16> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        {
            use crate::host_executor::HostSourceData;
            if self.is_canonical_evicted(&canonical) {
                return None;
            }
            let snap = self.scheduler.try_get_source(&canonical)?;
            let hd = snap.downcast_data::<HostSourceData>()?;
            Some(hd.parse.semantic_hash)
        }
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

        {
            use crate::host_executor::{HostAnalysisData, HostSourceData};
            if self.is_canonical_evicted(&canonical) {
                return None;
            }
            let snap = self.scheduler.try_get_source(&canonical)?;
            let hd = snap.downcast_data::<HostSourceData>()?;
            if hd.file_kind != FileKind::VueSfc {
                return None;
            }
            // Use pre-built AnalysisArcs for cheap pointer clone instead of Vec clone
            let macro_type_deps = self
                .scheduler
                .try_get_analysis(&canonical)
                .and_then(|a| {
                    a.downcast_data::<HostAnalysisData>()
                        .map(|ad| Arc::clone(&ad.arcs.macro_type_deps))
                })
                .unwrap_or_else(|| Arc::new(hd.parse.script_analysis.macro_type_deps.clone()));
            Some(CompileBlockersSnapshot {
                external_source_requests: hd.parse.external_requests.clone(),
                macro_type_deps,
            })
        }
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
        let mut results = Vec::with_capacity(canonical_ids.len());

        {
            for &id in canonical_ids {
                let canonical = self.resolve_alias_or_canonical(id);
                if self.is_canonical_evicted(&canonical) {
                    continue;
                }
                if let Some(snapshot) = self.build_snapshot_from_scheduler(&canonical) {
                    results.push((canonical, snapshot));
                }
            }
        }

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
        let mut results = {
            let ids = self.scheduler.node_ids();
            let mut results = Vec::with_capacity(ids.len());
            for id in ids {
                if self.is_canonical_evicted(&id) {
                    continue;
                }
                if let Some(snapshot) = self.build_snapshot_from_scheduler(&id) {
                    results.push((id, snapshot));
                }
            }
            results
        };

        for (canonical, snapshot) in &mut results {
            self.resolve_snapshot_imports(canonical, snapshot);
            self.enrich_destructured_bindings(snapshot);
        }
        results
    }

    /// Build a `FileAnalysisSnapshot` from scheduler snapshots and compile_cache.
    ///
    /// Reads `HostAnalysisData` for script analysis, export signatures, styles,
    /// and pre-computed `AnalysisArcs`. Template analysis comes from compile_cache
    /// (raw_template_analysis). Uses Arc::clone for all immutable fields.
    pub(crate) fn build_snapshot_from_scheduler(
        &self,
        canonical: &str,
    ) -> Option<FileAnalysisSnapshot> {
        use crate::host_executor::HostAnalysisData;

        let analysis_snap = self.scheduler.try_get_analysis(canonical)?;
        let ad = analysis_snap.downcast_data::<HostAnalysisData>()?;

        let template = self
            .derived_raw_cache()
            .get(canonical)
            .and_then(|cc| cc.raw_template_analysis.clone());

        Some(FileAnalysisSnapshot {
            imports: ad.script_analysis.imports.clone(),
            bindings: ad.script_analysis.bindings.clone(),
            module_references: Arc::clone(&ad.arcs.module_references),
            macros: Arc::clone(&ad.arcs.macros),
            macro_type_deps: Arc::clone(&ad.arcs.macro_type_deps),
            script_flags: ad.script_analysis.flags.bits(),
            styles: Arc::clone(&ad.style_analyses),
            template,
            vue_api_calls: Arc::clone(&ad.arcs.vue_api_calls),
            dom_query_calls: Arc::clone(&ad.arcs.dom_query_calls),
            css_var_manipulations: Arc::clone(&ad.arcs.css_var_manipulations),
            script_binding_occurrences: Arc::clone(&ad.arcs.script_binding_occurrences),
            export_signatures: Arc::new(ad.export_signatures.clone()),
            options_api: ad.script_analysis.options_api.clone(),
            store_usages: Arc::clone(&ad.arcs.store_usages),
            store_definitions: Arc::clone(&ad.arcs.store_definitions),
            is_typescript: ad.script_analysis.is_typescript,
        })
    }

    /// Resolve the source code of a dependency file.
    ///
    /// Tries scheduler (native) or files map (WASM) first, falling back to
    /// VFS resolution + disk read. Used by template analysis and external src
    /// block resolution.
    pub(crate) fn resolve_dep_source(
        &self,
        owner_canonical: &str,
        resolved_canonical_id: &str,
        specifier: &str,
    ) -> Option<Arc<str>> {
        // SFC <template src> / <script src> external-block path: read the
        // workspace directly without promoting the file into host state.
        // Caches/scheduler would treat these as compilable; they aren't.
        if let Some(source) = self.get_source(resolved_canonical_id) {
            return Some(source);
        }
        // Artifact-store `raw_source` fallback — content-pinned via
        // `artifact_current_indexed` (no content-agnostic `get_any`).
        // `get_source` above is the scheduler authority for any
        // scheduler-tracked canonical; this fallback answers only for a
        // genuinely artifact-only canonical (no `DerivedRawState`), so a
        // scheduler-tracked-but-stale scope never returns its stale
        // artifact's source here.
        if let Some(indexed) = self.artifact_current_indexed(resolved_canonical_id) {
            return Some(Arc::clone(&indexed.raw_source));
        }
        if let Some(source) = self.ws().read_file(resolved_canonical_id) {
            return Some(source);
        }

        let dep_id = self
            .resolve_loaded_dependency_canonical(
                owner_canonical,
                specifier,
                verter_workspace::ResolveRequestKind::SfcSrcAttr,
            )
            .or_else(|| {
                self.resolve_loaded_dependency_canonical(
                    owner_canonical,
                    specifier,
                    verter_workspace::ResolveRequestKind::EsmImport,
                )
            })?;

        self.get_source(&dep_id)
            .or_else(|| {
                // Content-pinned artifact fallback — `artifact_current_indexed`
                // answers only for a genuinely artifact-only canonical,
                // mirroring the `resolved_canonical_id` read above.
                self.artifact_current_indexed(&dep_id)
                    .map(|indexed| Arc::clone(&indexed.raw_source))
            })
            .or_else(|| self.ws().read_file(&dep_id))
    }

    /// Populate `resolved_canonical_id` on each import in the snapshot
    /// using the host's file map, alias map, and parent's dependency set.
    pub(crate) fn resolve_snapshot_imports(
        &self,
        parent_canonical_id: &str,
        snapshot: &mut FileAnalysisSnapshot,
    ) {
        for import in &mut snapshot.imports {
            if import.resolved_canonical_id.is_none() {
                import.resolved_canonical_id = self
                    .authoritative_import_route(parent_canonical_id, &import.source)
                    .and_then(|resolution| {
                        resolution
                            .resolved_canonical_id
                            .clone()
                            .or_else(|| resolution.effective_target().map(str::to_string))
                    })
                    .or_else(|| {
                        let ctx = verter_workspace::ResolutionContext {
                            phase: verter_workspace::ResolvePhase::CodegenBlocker,
                            kind: if import.is_type_only {
                                verter_workspace::ResolveRequestKind::TypeImport
                            } else {
                                verter_workspace::ResolveRequestKind::EsmImport
                            },
                        };
                        self.resolve_via_vfs(parent_canonical_id, &import.source, ctx)
                    });
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
    pub(crate) fn enrich_destructured_bindings(&self, snapshot: &mut FileAnalysisSnapshot) {
        use verter_semantic::analysis::types::{
            BindingInitializer, ComposableReturn, ReactivityKind,
        };

        // Build a map of import source â†’ resolved canonical ID from the snapshot
        let import_resolved: rustc_hash::FxHashMap<&str, &str> = snapshot
            .imports
            .iter()
            .filter_map(|imp| {
                imp.resolved_canonical_id
                    .as_deref()
                    .map(|resolved| (imp.source.as_str(), resolved))
            })
            .collect();

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

            let import_source = match callee_import_source {
                Some(src) => src.as_str(),
                None => continue,
            };

            let canonical_id = match import_resolved.get(import_source) {
                Some(id) => *id,
                None => continue,
            };

            // Look up exported_functions from the dep's analysis
            let composable_info = self.scheduler.try_get_analysis(canonical_id).and_then(|a| {
                a.downcast_data::<crate::host_executor::HostAnalysisData>()
                    .and_then(|ad| {
                        ad.script_analysis
                            .exported_functions
                            .iter()
                            .find(|f| f.name == *callee)
                            .and_then(|f| f.composable.clone())
                    })
            });

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

        {
            if self.is_canonical_evicted(&canonical) {
                return None;
            }
            let cc = self.compile_cache().get(&canonical)?;
            cc.latest_diagnostics.get(&profile_hash).cloned()
        }
    }

    /// Returns the monotonic diagnostics generation counter for a file.
    /// Incremented on every write to `latest_diagnostics`. Used by the LSP
    /// cache to detect host-driven recompiles without a document version change.
    pub fn get_diagnostics_generation(&self, canonical_or_alias: &str) -> Option<u64> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        {
            if self.is_canonical_evicted(&canonical) {
                return None;
            }
            let cc = self.compile_cache().get(&canonical)?;
            Some(cc.diagnostics_generation)
        }
    }

    /// Bump the diagnostics generation counter for a file without changing
    /// its diagnostics.
    pub fn bump_diagnostics_generation(&self, canonical_or_alias: &str) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        if let Some(mut cc) = self.compile_cache().get_mut(&canonical) {
            cc.diagnostics_generation += 1;
        }
    }

    /// Clear all compile slots for a specific file.
    pub fn invalidate_compile_slots(&self, canonical_or_alias: &str) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        // ProfileState owns compile_slots; DerivedRawState owns the
        // cached_resolved_meta + cached_meta_payload (D48 split).
        if let Some(mut cc) = self.compile_cache().get_mut(&canonical) {
            cc.compile_slots.clear();
        }
        if let Some(mut derived) = self.derived_raw_cache().get_mut(&canonical) {
            derived.cached_resolved_meta.clear();
            derived.cached_meta_payload = None;
        }
    }

    /// Remove a file from the host, cleaning up aliases, dependencies,
    /// and invalidating compile slots of any dependents.
    ///
    /// Sub-: workspace-authoritative — read dependents via
    /// `ws().reverse_deps_for(canonical)` BEFORE `notify_delete` fires
    /// (which clears the workspace's per-owner state and reverse-axis
    /// entries via `EdgeStore::remove_file`).
    pub fn remove(&self, canonical_or_alias: &str) -> Option<HostRemoveResult> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        // Read aliases from DependencyState (D48 split — aliases live
        // in the dep-closure-domain DB).
        let aliases = {
            let dep = self.dependency_cache().get(&canonical)?;
            dep.aliases.clone()
        };

        {
            let mut alias_map = write_lock(&self.alias_to_canonical);
            for alias in &aliases {
                alias_map.remove(alias);
            }
        }

        // Workspace-authoritative: read dependents BEFORE notify_delete.
        // ProfileState owns compile_slots; DerivedRawState owns the
        // cached_resolved_meta + cached_meta_payload (D48 split).
        let dependents = self.ws().reverse_deps_for(&canonical);
        for owner in &dependents {
            if let Some(mut cc) = self.compile_cache().get_mut(owner) {
                cc.compile_slots.clear();
            }
            if let Some(mut derived) = self.derived_raw_cache().get_mut(owner) {
                derived.cached_resolved_meta.clear();
                derived.cached_meta_payload = None;
            }
        }

        // notify_delete fires EdgeStore::remove_file (surgical per-owner
        // canonical-axis + active-stem cleanup; closes Gemini CRITICAL
        // PERFORMANCE).
        self.ws().notify_delete(&canonical);
        // File deletion drops all three D48 sub-states at once.
        self.drop_all_per_canonical_compile_caches(&canonical);
        self.scheduler.remove(&canonical);
        // Evict all resolver caches so that untracked-file acceptance in
        // the store view's `validates` method does not return stale facts
        // for a deleted file.
        self.resolver.runtime.hard_evict_canonical(&canonical);
        self.project_type_store.evict_canonical(&canonical);
        // Also evict component_meta results keyed by this canonical.
        self.resolver
            .runtime
            .component_meta
            .retain(|key| key.symbol_id != canonical);

        self.bump_store_view_epoch();
        Some(HostRemoveResult {
            canonical_id: canonical,
        })
    }

    /// Returns the list of virtual node kinds for a file.
    /// Returns an empty vec if the file doesn't exist.
    pub fn list_virtual_nodes(&self, canonical_or_alias: &str) -> Vec<VirtualNodeKind> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        {
            use crate::host_executor::HostSourceData;
            if self.is_canonical_evicted(&canonical) {
                return Vec::new();
            }
            if let Some(snap) = self.scheduler.try_get_source(&canonical) {
                if let Some(hd) = snap.downcast_data::<HostSourceData>() {
                    return hd.parse.meta.virtual_nodes();
                }
            }
            Vec::new()
        }
    }

    /// Convert `cc.import_routes` (caller-provided `DependencyResolution` map)
    /// into workspace `ExactResolution` `Vec`, fanning each resolution out
    /// across the (phase, kind) matrix per existing `set_import_dependencies`
    /// semantics.
    ///
    /// Sub- (R7): used by both [`Self::set_import_dependencies`]
    /// (existing flow) AND [`Self::integrate_scheduler_snapshot`]'s post-
    /// `record_parsed_edges` re-apply path (preserves bundler pre-load route
    /// state across ensure_loaded reloads).
    pub(crate) fn build_exact_resolutions_from_routes(
        &self,
        canonical_id: &str,
        import_routes: &rustc_hash::FxHashMap<String, DependencyResolution>,
    ) -> Vec<verter_workspace::ExactResolution> {
        let mut vfs_resolutions = Vec::new();
        for resolution in import_routes.values() {
            let resolved = resolution.resolved_canonical_id.as_ref().map(|id| {
                let norm = canonicalize_id(id);
                norm.into_owned()
            });
            let possible: Vec<String> = resolution
                .possible_canonical_ids
                .iter()
                .map(|candidate| {
                    let norm = canonicalize_id(candidate);
                    norm.into_owned()
                })
                .collect();
            let normalized_resolution = DependencyResolution {
                specifier: resolution.specifier.clone(),
                resolved_canonical_id: resolved.clone(),
                possible_canonical_ids: possible.clone(),
            };
            let mut exact_summaries = Vec::new();

            use verter_workspace::{ResolvePhase as P, ResolveRequestKind as K};
            for (phase, kind) in [
                (P::CodegenBlocker, K::EsmImport),
                (P::CodegenBlocker, K::TypeImport),
                (P::ProviderGraph, K::EsmImport),
                (P::ProviderGraph, K::TypeImport),
            ] {
                let exact = if exact_resolution_uses_type_preferred_target(phase, kind) {
                    self.derive_type_preferred_exact_target(&normalized_resolution)
                        .map(|target| verter_workspace::ExactResolution {
                            specifier: resolution.specifier.clone(),
                            phase,
                            kind,
                            resolved_canonical_id: Some(target),
                            possible_canonical_ids: Vec::new(),
                        })
                } else {
                    Some(verter_workspace::ExactResolution {
                        specifier: resolution.specifier.clone(),
                        phase,
                        kind,
                        resolved_canonical_id: resolved.clone(),
                        possible_canonical_ids: possible.clone(),
                    })
                };
                if let Some(exact) = exact {
                    exact_summaries.push(format!(
                        "{phase:?}/{kind:?}->{:?}",
                        exact
                            .resolved_canonical_id
                            .as_deref()
                            .or_else(|| exact.possible_canonical_ids.first().map(String::as_str))
                    ));
                    vfs_resolutions.push(exact);
                } else {
                    exact_summaries.push(format!("{phase:?}/{kind:?}-><resolver>"));
                }
            }
            if component_meta_debug_enabled() {
                component_meta_debug(format!(
                    "build_exact_resolutions owner={} specifier={} resolved={:?} possible=[{}] exacts=[{}]",
                    canonical_id,
                    resolution.specifier,
                    normalized_resolution.resolved_canonical_id,
                    normalized_resolution.possible_canonical_ids.join(", "),
                    exact_summaries.join("; "),
                ));
            }
        }
        vfs_resolutions
    }

    /// Provide caller-resolved import dependency resolution records.
    ///
    /// Called after `upsert()` when the caller resolves import specifiers
    /// (tsconfig paths, vite aliases, etc.) using bundler/LSP resolution.
    /// Each record maps a raw import specifier to its resolved canonical ID
    /// (or a list of candidate canonical IDs).
    ///
    /// Records are merged into the file's `import_routes` map (keyed by
    /// specifier). The flat `dependencies` set is updated in parallel for
    /// reverse-dependency tracking.
    ///
    /// **Architectural contract** (see also
    /// `tests/import_route_writer_guard.rs`):
    ///
    /// `set_import_dependencies` is the **complete caller-supplied
    /// route-snapshot writer** for
    /// [`DerivedRawState::import_routes`](crate::types::DerivedRawState)
    /// AND the **single producer** of
    /// [`DerivedRawState::import_routes_known_miss_recorded_at_generation`].
    /// For every known-miss specifier in the supplied snapshot (no
    /// resolved canonical, no candidates, no effective target), the
    /// current workspace `content_generation` is recorded so the
    /// reader can detect when a new canonical may now satisfy a
    /// previously unresolvable specifier. Positive resolutions do
    /// not need a generation tag — they stay valid until the
    /// owner's source content changes.
    ///
    /// Positive-only route point admission lives in
    /// [`Self::cache_positive_import_route_result`]: that helper
    /// must NOT touch the known-miss generation sidecar, and this
    /// method must NOT be used as a positive-only point cache (doing
    /// so would risk re-stamping a previously admitted known miss at
    /// the current `content_generation` and incorrectly extending a
    /// stale negative answer that should have re-resolved).
    pub fn set_import_dependencies(
        &self,
        canonical_or_alias: &str,
        resolutions: Vec<DependencyResolution>,
    ) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let parse_deps = self.parse_dependency_set_for_file(&canonical);

        // Normalize resolutions and persist direct import resolutions.
        let mut import_routes = rustc_hash::FxHashMap::default();
        for mut res in resolutions {
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
            import_routes.insert(res.specifier.clone(), res);
        }

        // Compute exact resolutions via the shared helper (§2.12).
        let vfs_resolutions = self.build_exact_resolutions_from_routes(&canonical, &import_routes);

        // Preserve already-discovered transitive macro-type deps; compilation
        // refreshes them, but direct import updates should not discard them.
        // D48 split: dependencies live on DependencyState, import_routes
        // on DerivedRawState. Update each in turn while preserving the
        // old-deps difference for transitive macro-type-dependency sync.
        let old_deps = self
            .dependency_cache()
            .get(&canonical)
            .map(|d| d.dependencies.clone())
            .unwrap_or_default();
        let old_direct_deps = {
            let mut deps = parse_deps.clone();
            let existing_routes = self
                .derived_raw_cache()
                .get(&canonical)
                .map(|d| d.import_routes.clone())
                .unwrap_or_default();
            deps.extend(Self::resolved_dependency_targets(&existing_routes));
            deps
        };
        // Replace import_routes on DerivedRawState.
        // R3/R26/R28 Gap 2: for each known-miss in the new map,
        // record the workspace `content_generation` at admission so
        // the reader can detect when a new canonical (which advances
        // content_generation) may now satisfy the previously
        // unresolvable specifier. Positive resolutions do not need a
        // generation tag — they stay valid until the owner's source
        // content changes.
        let current_generation = self.ws().content_generation();
        let mut known_miss_generations: rustc_hash::FxHashMap<String, u64> =
            rustc_hash::FxHashMap::default();
        for (specifier, res) in import_routes.iter() {
            if res.resolved_canonical_id.is_none() && res.possible_canonical_ids.is_empty() {
                known_miss_generations.insert(specifier.clone(), current_generation);
            }
        }
        {
            let mut derived_ref = self
                .derived_raw_cache()
                .entry(canonical.clone())
                .or_default();
            derived_ref.value_mut().import_routes = import_routes.clone();
            derived_ref
                .value_mut()
                .import_routes_known_miss_recorded_at_generation = known_miss_generations;
        }
        let old_transitive_deps = old_deps
            .difference(&old_direct_deps)
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();

        self.sync_transitive_macro_type_dependencies(&canonical, &old_transitive_deps);

        // Admit the resolved-import facts bundle for the owner so
        // downstream consumers (`RouteDb`, materialiser, etc.) can
        // read resolved facts directly instead of re-walking the
        // AST. First-writer-wins on the cache key composed of the
        // owner's per-canonical env hashes. Skipping on duplicate
        // keys keeps `Arc` identity stable for in-flight readers.
        let _ = self.admit_resolved_import_facts_for_owner(&canonical, &import_routes);

        // Sync exact resolutions to workspace.
        self.ws().set_exact_resolutions(&canonical, vfs_resolutions);
        // Soft-invalidate: file content didn't change, only import routes.
        //
        // R3 target end-state: eager dependent invalidation goes
        // away once producer admission carries the dep-precise
        // signatures the fact-validation oracle needs. The local
        // drain below is the backstop for the route-only
        // observation surface; the read-side fact oracle remains
        // the correctness oracle for cached values.
        self.invalidate_route_owned_shallow_cache(&canonical);
        self.resolver.runtime.invalidate_canonical(&canonical);
        self.project_type_store.evict_canonical(&canonical);
        self.resolved_type_cache().clear();
        // R4 producer: rebuild parse-domain facts for the externally
        // changed canonical.
        self.register_facts_for_new_content(&canonical);
        self.bump_store_view_epoch();
    }

    /// Returns all known canonical file IDs and their file kinds.
    pub fn list_files(&self) -> Vec<(String, FileKind)> {
        {
            use crate::host_executor::HostSourceData;
            self.scheduler
                .node_ids()
                .into_iter()
                .filter_map(|id| {
                    if self.is_canonical_evicted(&id) {
                        return None;
                    }
                    let snap = self.scheduler.try_get_source(&id)?;
                    let hd = snap.downcast_data::<HostSourceData>()?;
                    Some((id, hd.file_kind))
                })
                .collect()
        }
    }

    pub(crate) fn raw_template_analysis_for_file(
        &self,
        canonical: &str,
    ) -> Option<Arc<verter_semantic::analysis::template::TemplateAnalysisSnapshot>> {
        {
            use crate::host_executor::HostSourceData;
            if self.is_canonical_evicted(canonical) {
                return None;
            }
            let source_snap = self.scheduler.try_get_source(canonical)?;
            let hd = source_snap.downcast_data::<HostSourceData>()?;
            if hd.file_kind != FileKind::VueSfc {
                return None;
            }
            drop(source_snap);
            let mut snapshot = self.build_snapshot_from_scheduler(canonical)?;
            self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            snapshot.template
        }
    }

    pub(super) fn compute_override_template_analysis(
        &self,
        canonical: &str,
        profile_hash: u64,
    ) -> Option<Arc<verter_semantic::analysis::template::TemplateAnalysisSnapshot>> {
        let override_with_parse = {
            let cc = self.compile_cache().get(canonical)?;
            cc.content_overrides.get(&profile_hash)?.clone()
        };

        self.build_template_analysis(
            canonical,
            &override_with_parse.source,
            override_with_parse.cached_parse.clone(),
            &override_with_parse.parse.src_blocks,
            &override_with_parse.parse.external_requests,
            &override_with_parse.parse.script_analysis.imports,
            &override_with_parse.parse.script_analysis.macros,
            &override_with_parse.parse.script_analysis.bindings,
        )
    }

    /// Returns cross-component CSS variable flow for a given variable name.
    ///
    /// Scans all files in the host to find where the variable is defined (in `<style>`),
    /// referenced via `var()` (in `<style>`), set via `:style` bindings (in `<template>`),
    /// and manipulated via DOM APIs (in `<script>`).
    ///
    /// When `profile` is provided, override-aware style/template/script state is
    /// used for that compile profile. `None` keeps the read profileless/raw.
    pub fn css_var_flow(
        &self,
        var_name: &str,
        profile: Option<&CompileProfile>,
    ) -> verter_semantic::analysis::CssVarFlow {
        let profile_hash = profile.map(compile_profile_hash);

        let canonical_ids: Vec<String> = self
            .scheduler
            .node_ids()
            .into_iter()
            .filter(|id| !self.is_canonical_evicted(id))
            .collect();

        let mut flow = verter_semantic::analysis::CssVarFlow {
            name: var_name.to_string(),
            ..Default::default()
        };

        for canonical_id in canonical_ids {
            let path: std::sync::Arc<std::path::Path> =
                std::sync::Arc::from(std::path::Path::new(canonical_id.as_str()));

            let style_analyses = self
                .effective_style_analyses(&canonical_id, profile_hash)
                .unwrap_or_default();

            // Check style blocks for definitions and var() references
            for style in &style_analyses {
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
            let template_analysis = if let Some(profile_hash) = profile_hash {
                self.compile_cache()
                    .get(&canonical_id)
                    .and_then(|cc| {
                        if cc.content_overrides.contains_key(&profile_hash) {
                            cc.compile_slots
                                .get(&profile_hash)
                                .and_then(|slot| slot.template_analysis.clone())
                                .map(Arc::new)
                        } else {
                            None
                        }
                    })
                    .or_else(|| {
                        self.compute_override_template_analysis(&canonical_id, profile_hash)
                    })
                    .or_else(|| self.raw_template_analysis_for_file(&canonical_id))
            } else {
                self.raw_template_analysis_for_file(&canonical_id)
            };

            if let Some(ref tmpl) = template_analysis {
                if tmpl.css_var_names.iter().any(|n| n == var_name) {
                    flow.template_definitions.push(std::sync::Arc::clone(&path));
                }
            }

            // Check script for DOM API CSS variable manipulations
            let script_has_manipulation = self
                .effective_file_state(&canonical_id, profile_hash)
                .map(|efs| {
                    efs.script_analysis
                        .css_var_manipulations
                        .iter()
                        .any(|m| m.var_name == var_name)
                })
                .unwrap_or(false);

            if script_has_manipulation {
                flow.script_manipulations.push(std::sync::Arc::clone(&path));
            }
        }

        flow
    }

    pub(crate) fn load_export_graph_analysis(
        &self,
        canonical_or_alias: &str,
    ) -> Option<(
        FileKind,
        verter_semantic::analysis::ScriptAnalysisSnapshot,
        Vec<verter_semantic::analysis::ExportSignature>,
    )> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        {
            if self.is_canonical_evicted(&canonical) {
                return None;
            }

            let current_hash = self
                .current_or_read_whole_hash(&canonical)
                .unwrap_or_default();
            if self.store_view_allows_current_whole_hash(&canonical, current_hash) {
                if let (Some(source_snap), Some(analysis_snap)) = (
                    self.scheduler.try_get_source(&canonical),
                    self.scheduler.try_get_analysis(&canonical),
                ) {
                    let file_kind = source_snap
                        .downcast_data::<crate::host_executor::HostSourceData>()?
                        .file_kind;
                    let analysis =
                        analysis_snap.downcast_data::<crate::host_executor::HostAnalysisData>()?;
                    return Some((
                        file_kind,
                        analysis.script_analysis.clone(),
                        analysis.export_signatures.clone(),
                    ));
                }
            }
        }

        if let Some(facts) = self.ensure_indexed_ready(&canonical) {
            if let (Some(script_analysis), Some(export_signatures)) = (
                facts.script_analysis.as_ref(),
                facts.export_signatures.as_ref(),
            ) {
                return Some((
                    if canonical.ends_with(".vue") {
                        FileKind::VueSfc
                    } else {
                        FileKind::NonSfc
                    },
                    script_analysis.as_ref().clone(),
                    export_signatures.as_ref().clone(),
                ));
            }
        }
        None
    }

    /// Look up the byte span of an exported name in a target file.
    ///
    /// For `.vue` files: searches `ScriptAnalysisSnapshot.bindings` (script-setup
    /// auto-exports) — spans are SFC-absolute.
    /// For `.ts`/`.js` files: searches `CompileCacheEntry.export_signatures` —
    /// spans are file-absolute.
    ///
    /// Returns `None` if the file doesn't exist or the name isn't exported.
    pub fn get_export_span(
        &self,
        canonical_or_alias: &str,
        binding_name: &str,
    ) -> Option<(u32, u32)> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        {
            use crate::host_executor::{HostAnalysisData, HostSourceData};

            if self.is_canonical_evicted(&canonical) {
                return None;
            }
            let source_snap = self.scheduler.try_get_source(&canonical)?;
            let hd = source_snap.downcast_data::<HostSourceData>()?;
            let file_kind = hd.file_kind;
            drop(source_snap);

            let analysis_snap = self.scheduler.try_get_analysis(&canonical)?;
            let ad = analysis_snap.downcast_data::<HostAnalysisData>()?;

            Self::find_export_span(
                file_kind,
                &ad.script_analysis,
                &ad.export_signatures,
                binding_name,
            )
        }
    }

    /// Shared logic for finding an export span from analysis data.
    pub(super) fn find_export_span(
        file_kind: FileKind,
        script_analysis: &verter_semantic::analysis::ScriptAnalysisSnapshot,
        export_signatures: &[verter_semantic::analysis::ExportSignature],
        binding_name: &str,
    ) -> Option<(u32, u32)> {
        if file_kind == FileKind::VueSfc {
            if let Some(binding) = script_analysis
                .bindings
                .iter()
                .find(|b| b.name == binding_name)
            {
                if binding.span.start > 0 || binding.span.end > 0 {
                    return Some((binding.span.start, binding.span.end));
                }
            }
            for mac in &script_analysis.macros {
                if mac.binding_name.as_deref() == Some(binding_name)
                    && (mac.span.start > 0 || mac.span.end > 0)
                {
                    return Some((mac.span.start, mac.span.end));
                }
            }
            if binding_name == "default" {
                if let Some(first_binding) = script_analysis.bindings.first() {
                    if first_binding.span.start > 0 || first_binding.span.end > 0 {
                        return Some((first_binding.span.start, first_binding.span.end));
                    }
                }
                if let Some(first_macro) = script_analysis.macros.first() {
                    if first_macro.span.start > 0 || first_macro.span.end > 0 {
                        return Some((first_macro.span.start, first_macro.span.end));
                    }
                }
                return Some((0, 0));
            }
            return None;
        }

        if let Some(sig) = export_signatures.iter().find(|s| s.name == binding_name) {
            if sig.reexport_source.is_some() {
                return None;
            }
            let span = sig.local_span.unwrap_or(sig.span);
            if span.start > 0 || span.end > 0 {
                return Some((span.start, span.end));
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
        if self.is_canonical_evicted(&canonical) {
            return None;
        }
        let resolver = HostExportGraphResolver { host: self };
        resolver_get_export_span_follow_reexports_from_graph(&resolver, &canonical, binding_name)
    }

    /// Resolve an import specifier to its canonical ID using the host's file map,
    /// alias map, and parent's resolved dependencies.
    ///
    /// Returns `None` if the import cannot be resolved to a known file
    /// (e.g., bare specifiers like `vue` or unregistered files).
    pub fn resolve_import(&self, parent_canonical_id: &str, import_source: &str) -> Option<String> {
        let canonical_parent = self.resolve_alias_or_canonical(parent_canonical_id);
        if self.is_canonical_evicted(&canonical_parent) {
            return None;
        }
        let ctx = verter_workspace::ResolutionContext {
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
        };
        self.resolve_loaded_dependency_canonical(&canonical_parent, import_source, ctx.kind)
    }

    pub(crate) fn resolve_named_export(
        &self,
        canonical_or_alias: &str,
        binding_name: &str,
        is_type: Option<bool>,
    ) -> Option<ResolvedExport> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        if self.is_canonical_evicted(&canonical) {
            return None;
        }
        let resolver = HostExportGraphResolver { host: self };
        let is_declaration_file = canonical.ends_with(".d.ts")
            || canonical.ends_with(".d.mts")
            || canonical.ends_with(".d.cts");
        let resolved = resolver_resolve_named_export_from_graph(
            &resolver,
            &canonical,
            binding_name,
            is_type,
            false,
        )
        .or_else(|| {
            if is_type == Some(true) && is_declaration_file {
                resolver_resolve_named_export_from_graph(
                    &resolver,
                    &canonical,
                    binding_name,
                    None,
                    false,
                )
            } else {
                None
            }
        })?;
        Some(ResolvedExport {
            name: resolved.name,
            is_type: resolved.is_type,
            source_canonical_id: resolved.source_canonical_id,
            source_name: resolved.source_name,
        })
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
        if self.is_canonical_evicted(&canonical) {
            return Vec::new();
        }
        let resolver = HostExportGraphResolver { host: self };
        let resolved = resolver_resolve_exports_from_graph_best_effort(&resolver, &canonical);
        resolved
            .into_iter()
            .map(|export| ResolvedExport {
                name: export.name,
                is_type: export.is_type,
                source_canonical_id: export.source_canonical_id,
                source_name: export.source_name,
            })
            .collect()
    }
}

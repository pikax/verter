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
use verter_language::FileLanguage;

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
        file_language: &FileLanguage,
        source: &Arc<str>,
        framework_parse: Option<Arc<verter_language::FrameworkParseArtifact>>,
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

        let raw = crate::parse::compile_template_data(
            file_language,
            &merged_source,
            framework_parse.as_deref(),
            src_blocks.is_empty(),
            &self.provenance,
        )?;
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
    /// `compile_from_parsed()` — bypassing the host `compile_entry()` which fails
    /// on unresolved macro type deps. External-src blocks are merged using the
    /// same `merge_external_sources()` helper. The computed template is served
    /// on the caller's snapshot unconditionally; whether it ALSO persists into
    /// the shared raw-template slot is decided by the slot's write authority
    /// ([`Self::persist_raw_template_analysis`] →
    /// [`crate::types::RawTemplateSlotAdmission::admitted_generation`]), to
    /// which this lane only attests its facts (threaded `store_published` and
    /// `source_generation`, its own src-block inventory, default extraction —
    /// this lane compiles with default `CodegenOptions`).
    ///
    /// Source acquisition is caller-threaded ONLY: `inputs` carry the
    /// SAME source + parse the caller's snapshot was built from,
    /// captured by value at the caller's own read site — the
    /// computation never consults the scheduler itself. The conversion
    /// below derives its imports/bindings from `snapshot`, so a
    /// computation whose bytes come from a different read than the
    /// snapshot mixes generations (or mixes an overlay's conversion
    /// context into base content); a caller without coherent inputs
    /// (torn generation join, non-SFC) simply skips the computation
    /// and the template stays absent — fail closed, never mixed.
    pub(crate) fn compute_template_analysis_if_missing(
        &self,
        canonical: &str,
        snapshot: &mut FileAnalysisSnapshot,
        inputs: crate::types::VueTemplateInputs,
    ) {
        if snapshot.template.is_some() {
            return;
        }

        let crate::types::VueTemplateInputs {
            source,
            framework_parse,
            store_published,
            source_generation,
        } = inputs;

        // The file's resolved carrier row — template-data ingestion is gated on
        // whether its adapter has a registered carrier compiler (NOT a hardcoded
        // `.vue` / `is_vue()` check), and feeds the registry-dispatched
        // `compile_template_data` below.
        let file_language = self.language_classifier().classify(canonical);
        if !crate::parse::file_language_has_template_data_compiler(&file_language) {
            return;
        }

        // ONE carrier parse at most on this lane: reuse the threaded/scheduler
        // artifact; a missing artifact runs exactly one counted carrier parse of
        // its own (no caller ran one on this source). The Vue `<template src>`
        // inventory walks the typed Vue parse opened from the artifact; a
        // carrier with no src-block surface (Svelte) yields none.
        let framework_parse = framework_parse.or_else(|| {
            crate::parse::build_carrier_parse_artifact_from_source(
                &file_language,
                source.as_ref(),
                &self.provenance,
            )
        });
        let (src_blocks, external_requests) = framework_parse
            .as_deref()
            .and_then(crate::typeinfo::adapters::vue::vue_parse)
            .map(|parsed| crate::parse::collect_vue_src_blocks(canonical, source.as_ref(), parsed))
            .unwrap_or_default();

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

        // Registry-dispatched template-data extraction: route through the file's
        // carrier compiler (Vue's bridge runs the META compile, Svelte walks the
        // typed template tree), re-using the carrier parse when no external src
        // merged the source.
        let raw = crate::parse::compile_template_data(
            &file_language,
            &merged_source,
            framework_parse.as_deref(),
            src_blocks.is_empty(),
            &self.provenance,
        );

        // Convert RawTemplateData â†’ TemplateAnalysisSnapshot using existing converter
        if let Some(raw) = raw {
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

            #[cfg(test)]
            self.fire_template_persist_seam();

            self.persist_raw_template_analysis(
                canonical,
                tpl_arc,
                crate::types::RawTemplateSlotAdmission {
                    store_published,
                    source_generation,
                    has_src_blocks: !src_blocks.is_empty(),
                    // This lane compiles with default `CodegenOptions`
                    // — no parse-affecting profile options reach it.
                    default_extraction: true,
                },
            );
        }
    }

    /// Host-level persist chokepoint for the profileless
    /// raw-template slot. Every persist site — the lazy
    /// template-analysis computation above and the Session
    /// compile-publish lane — states its context through
    /// [`crate::types::RawTemplateSlotAdmission`] and routes here; the
    /// admission rules and the monotonic install live on the slot's
    /// structural write authority
    /// ([`crate::types::DerivedRawState::install_raw_template_analysis`]
    /// gating via
    /// [`crate::types::RawTemplateSlotAdmission::admitted_generation`])
    /// — the slot field is module-private, so no writer can exist
    /// outside that gate. The pre-check below only avoids creating a
    /// `DerivedRawState` entry for a statement the gate would decline.
    pub(crate) fn persist_raw_template_analysis(
        &self,
        canonical: &str,
        template: Arc<verter_semantic::analysis::template::TemplateAnalysisSnapshot>,
        admission: crate::types::RawTemplateSlotAdmission,
    ) {
        if admission.admitted_generation().is_none() {
            return;
        }
        // raw_template_analysis lives on DerivedRawState (D48 split).
        let mut derived_ref = self
            .derived_raw_cache()
            .entry(canonical.to_string())
            .or_default();
        derived_ref
            .value_mut()
            .install_raw_template_analysis(template, admission);
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
                // Scheduler-missed lane: ONE snapshot build whose parse
                // products are threaded into the template-analysis
                // computation — the lane performs exactly one SFC
                // structure parse and one script-program parse total.
                let source = self.read_analysis_source(canonical)?;
                // This lane's own `read_analysis_source` read —
                // store-authoritative (live scheduler/workspace read, or
                // the artifact-current authority for an artifact-only
                // canonical; never a fenced serve).
                let (snapshot, template_inputs) =
                    self.build_snapshot_and_template_inputs_from_source(canonical, &source, true);
                return Some(self.finalize_analysis_snapshot(
                    canonical,
                    snapshot,
                    self.config.effective_scope().needs_template_analysis(),
                    template_inputs,
                    analysis_started,
                ));
            };
            let hd = source_snap.downcast_data::<HostSourceData>()?;
            let file_language = hd.file_language.clone();
            let source = source_snap.source.clone();
            let framework_parse = hd.framework_parse.clone();
            let scope = self.config.effective_scope();
            if file_language.is_vue()
                && (!scope.needs_script_analysis() || !scope.needs_style_analysis())
            {
                #[cfg(test)]
                self.fire_narrowed_scope_serve_seam();
                // Template inputs from the SAME source read this branch
                // builds its script analysis from — the template-analysis
                // computation never re-reads the scheduler. The
                // full-scope path below instead joins its own inputs at
                // its analysis snapshot's generation (its snapshot comes
                // from an analysis read, not from this source read), so
                // only this branch captures here.
                let template_inputs = Some(crate::types::VueTemplateInputs {
                    source: source.clone(),
                    framework_parse: framework_parse.clone(),
                    // Live scheduler read — store-authoritative.
                    store_published: true,
                    source_generation: Some(source_snap.generation),
                });
                // This branch builds an OWNED, mutated snapshot (it calls
                // `mark_bindings_used_in_style` and moves fields out), so it
                // needs an owned value, not the shared `Arc`. The scheduler
                // still holds the snapshot, so materialise one owned copy.
                let stored_script = (*hd.parse.script_analysis).clone();
                // Style analyses and export signatures from the SAME
                // held source read: the analysis stage repackages
                // exactly these parse products at the source's
                // generation, so deriving them here keeps the served
                // snapshot single-generation — an independent analysis
                // read could observe a newer node mid-window and pair
                // this source's script analysis with the moved
                // generation's products.
                let stored_styles = Arc::new(hd.parse.style_analyses.clone());
                // Generation rail: accept the persisted template only
                // at THIS branch's own source generation — a late
                // persist stamped with a superseded generation must
                // not serve as current.
                let template = self.derived_raw_cache().get(canonical).and_then(|cc| {
                    cc.raw_template_analysis()
                        .filter(|entry| entry.source_generation == source_snap.generation)
                        .map(|entry| Arc::clone(&entry.template))
                });
                let export_sigs = hd.parse.export_signatures.clone();
                drop(source_snap);

                let mut script_analysis = if !scope.needs_script_analysis() {
                    let mut rebuilt = crate::parse::build_script_analysis_for_artifact(
                        framework_parse.as_deref(),
                        &source,
                        &self.provenance,
                    );
                    // Producer-side locator absolutization for the narrowed-scope
                    // rebuild lane (the artifact-facing builder is canonical-free;
                    // the stored-snapshot branch was absolutized at snapshot build).
                    crate::parse::absolutize_macro_payload_anchors(&mut rebuilt.macros, canonical);
                    rebuilt
                } else {
                    stored_script
                };
                let style_analyses = if !scope.needs_style_analysis() {
                    Arc::new(crate::parse::build_style_analyses_for_artifact(
                        framework_parse.as_deref(),
                        &source,
                        canonical,
                        &self.provenance,
                    ))
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
                    template_inputs,
                    analysis_started,
                ));
            }
            drop(source_snap);

            // Snapshot + template inputs joined at the analysis
            // snapshot's generation — never the earlier source read
            // above paired with an independent later analysis read. A
            // torn join carries `None` inputs and this caller serves
            // without a template (fail closed, never mixed).
            let (snapshot, joined_inputs) =
                self.build_snapshot_from_scheduler_with_template_inputs(canonical)?;
            Some(self.finalize_analysis_snapshot(
                canonical,
                snapshot,
                self.config.effective_scope().needs_template_analysis(),
                joined_inputs,
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
            // Snapshot AND template inputs from the SAME overlay read:
            // the template derives from the overlay's own bytes, in
            // the overlay snapshot's conversion context — one coherent
            // read, never base scheduler bytes converted with overlay
            // imports/bindings. `store_published = false` is the
            // conversion-context attestation: an overlay/session
            // conversion serves this caller only and never populates
            // the base `derived_raw_cache` slot (overlay results never
            // populate base caches; R17 — the base host's caches are
            // not mutated).
            let (snapshot, template_inputs) = self.build_snapshot_and_template_inputs_from_source(
                canonical.as_str(),
                &source,
                false,
            );
            return Some(self.finalize_analysis_snapshot(
                canonical.as_str(),
                snapshot,
                self.config.effective_scope().needs_template_analysis(),
                template_inputs,
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
            // Scheduler branch — gated only on an EXPLICIT evicted
            // marker (`evicted` lives on the source-content-domain DB).
            // An absent `DerivedRawState` entry does NOT hide a present
            // scheduler source: per-canonical derived state can be
            // dropped while the scheduler still holds the canonical —
            // the scheduler stays the content authority (mirrors
            // `is_artifact_only_scope` / `authoritative_current_content_hash`).
            let entry_evicted = self
                .derived_raw_cache()
                .get(canonical)
                .is_some_and(|d| d.evicted);
            if !entry_evicted {
                if let Some(snap) = self.scheduler.try_get_source(canonical) {
                    let hd = snap.downcast_data::<HostSourceData>()?;
                    return Some(hd.parse.whole_hash);
                }
            }
            // Artifact-only fallback through the ONE authority gate
            // (`artifact_current_indexed_raw`: artifact-only scope
            // oracle + workspace `file_exists` + the content-transition
            // ledger). A bare `get_any` here reported a deleted /
            // superseded canonical's stale artifact's OWN hash as the
            // file's current hash — bypassing the inventory every other
            // artifact-only lane answers through.
            self.artifact_current_indexed_raw(canonical)
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
        // An EXPLICITLY evicted entry means the canonical is no longer
        // live; any artifact still in `FileArtifactStore` is stale and
        // must not back a "current content" pin. An ABSENT entry does
        // NOT hide a present scheduler source: per-canonical derived
        // state can be dropped while the scheduler still holds the
        // canonical — the scheduler stays the content authority
        // (mirrors `is_artifact_only_scope`).
        let entry_evicted = self
            .derived_raw_cache()
            .get(canonical)
            .is_some_and(|d| d.evicted);
        if entry_evicted {
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
    ///
    /// The lookup is keyed by the **normalised analysis canonical**
    /// ([`Self::normalized_analysis_canonical`] — e.g. a runtime `.js`
    /// whose `.d.ts` companion is the analysis target): every base
    /// `IndexedReady` artifact is published under the normalised id as
    /// `FileArtifactKey::canonical`, and the scheduler tracks the
    /// normalised analysis target's `parse.whole_hash`. Normalising here
    /// lets a caller pass the RAW requested canonical — the
    /// architectural id before an overlay-detection point — without the
    /// base read mis-keying for a non-identity `.js`. Normalisation is
    /// idempotent, so a caller that already holds a normalised id is
    /// unaffected. Overlay detection is NOT this method's concern — it
    /// reads the base candidate only; the overlay-aware
    /// [`crate::resolver_core::SessionResolverContext::indexed_for_current_content`]
    /// gates on the raw id and routes the overlay branch through
    /// [`crate::host_manage::overlay_materialize::OverlayArtifactIdentity`].
    #[must_use]
    pub(crate) fn current_content_pinned_indexed(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        let analysis_canonical = self.normalized_analysis_canonical(canonical);
        let analysis_canonical = analysis_canonical.as_ref();
        let current_hash = self.authoritative_current_content_hash(analysis_canonical)?;
        let indexed = self
            .project_type_store
            .indexed()
            .get_for_current_content(analysis_canonical, current_hash)?;
        // Edge-currency gate. The content pin keys only on the OWNER's content
        // hash, but a wildcard `export *` surface bakes its edge `canonical_id`s
        // from the dependency file set; a dependency appearing or retargeting
        // (the file set changes, the owner's content does not) leaves those
        // edges stale while the content pin still matches. Re-index from BASE
        // content through `ensure_indexed_ready_serve` — whose reuse is itself
        // edge-gated, so it re-resolves the edges against the live file set and
        // republishes — and return the fresh artifact. A non-wildcard surface
        // is always edge-current and returns directly. This base accessor is
        // the choke point every base reader (`shallow_file_state`,
        // `observe_materialize_scope`, `current_import_route_table`, the
        // `HostStoreView` ImportRoute snapshot, …) funnels through.
        if self.indexed_surface_is_current(analysis_canonical, &indexed) {
            return Some(indexed);
        }
        // Re-index arm: a fenced rebuild is served bare here (the
        // accessor's contract is artifact-or-nothing), but the fenced
        // consumption is visible to every enclosing traced admission
        // point via the serve chokepoint flag, so it can no longer be
        // laundered into a warm shared-cache entry.
        self.ensure_indexed_ready_serve(analysis_canonical)
            .map(|serve| serve.indexed)
    }

    /// **Observe-only** variant of [`Self::current_content_pinned_indexed`]:
    /// the same content-pinned read (scheduler-authoritative hash when one
    /// exists, the non-recursing artifact-only authority otherwise) with
    /// **no re-index arm** — it NEVER calls `ensure_indexed_ready_serve`, so it
    /// never materialises, publishes, or refreshes anything.
    ///
    /// This is the read fact-capture uses
    /// (`current_derived_fact_hash(Route)`): fact capture must observe,
    /// never build — a capture that cold-builds breadth-walks every
    /// unrelated import of the owner just to sign a result. Callers that
    /// WANT the re-index on a stale surface use
    /// [`Self::current_content_pinned_indexed`] /
    /// [`Self::artifact_current_indexed`] instead. The returned artifact
    /// is NOT currency-filtered; observers apply
    /// [`crate::VerterHost::indexed_surface_is_current`] themselves and
    /// decline (rather than rebuild) on a stale surface.
    #[must_use]
    pub(crate) fn observe_content_pinned_indexed(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        let analysis_canonical = self.normalized_analysis_canonical(canonical);
        let analysis_canonical = analysis_canonical.as_ref();
        match self.authoritative_current_content_hash(analysis_canonical) {
            Some(current_hash) => self
                .project_type_store
                .indexed()
                .get_for_current_content(analysis_canonical, current_hash),
            None => self.artifact_current_indexed_raw(analysis_canonical),
        }
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
    ///
    /// Both the `DerivedRawState` gate and the `get_any` artifact lookup
    /// are keyed by the **normalised analysis canonical**
    /// ([`Self::normalized_analysis_canonical`]) — the identity every
    /// base `IndexedReady` artifact is published under and the same
    /// identity [`Self::current_content_pinned_indexed`] reasons about,
    /// so the two artifact-current authorities stay in lockstep. A
    /// caller may therefore pass the RAW requested canonical; the
    /// rewrite is idempotent for an already-normalised id.
    #[must_use]
    pub(crate) fn artifact_current_indexed(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        let analysis_canonical = self.normalized_analysis_canonical(canonical);
        let analysis_canonical = analysis_canonical.as_ref();
        let indexed = self
            .project_type_store
            .indexed()
            .get_any(analysis_canonical)?;
        // The single artifact-only authority gate (same contract as the
        // raw peek): a `DerivedRawState` entry means the scheduler is the
        // content authority, and an absent file's artifact must never
        // serve nor rebuild — decline. Evaluated AFTER the artifact
        // lookup so a canonical with no artifact at all (the common probe
        // shape for unresolvable specifiers) costs no workspace
        // `file_exists` probe.
        if !self.artifact_only_candidate_is_fresh(analysis_canonical, indexed.edge_generation) {
            return None;
        }
        // Edge-currency gate (same rationale as
        // `current_content_pinned_indexed`). A genuinely artifact-only
        // canonical has no scheduler `DerivedRawState`, so re-indexing it
        // through `ensure_indexed_ready_serve` re-reads the artifact source and
        // re-resolves its wildcard `export *` edges against the live file set.
        //
        // `ensure_indexed_ready_serve` MUST NOT route its own artifact fast-path back
        // through this method (it uses the non-recursing
        // [`Self::artifact_current_indexed_raw`] instead): this method calls
        // `ensure_indexed_ready_serve` on stale, so a back-edge would mutually
        // recurse and overflow the stack for an artifact-only edge-stale
        // wildcard barrel.
        if self.indexed_surface_is_current(analysis_canonical, &indexed) {
            return Some(indexed);
        }
        // Re-index arm — same chokepoint-covered serve as
        // `current_content_pinned_indexed` above.
        self.ensure_indexed_ready_serve(analysis_canonical)
            .map(|serve| serve.indexed)
    }

    /// TRUE when `analysis_canonical` is a genuinely **artifact-only**
    /// scope: the scheduler is NOT its content authority — no
    /// `DerivedRawState` entry (live OR evicted) exists for it AND the
    /// scheduler holds no source for it. THE single artifact-only-ness
    /// oracle: the serving authorities, the signal-driven eviction
    /// wrapper (`evict_artifact_only_canonical`), and the base
    /// store-view snapshot all consult this one predicate so they
    /// cannot drift.
    ///
    /// Absence of a `DerivedRawState` entry ALONE is not
    /// scheduler-untrackedness: per-canonical derived state can be
    /// dropped (an authority-reset wide clear, a domain sweep) while
    /// the scheduler still holds the canonical's source — flipping such
    /// a canonical into the artifact-only class would route it through
    /// the permissive `get_any` lanes against the scheduler's live
    /// content. A scheduler source present (and not explicitly evicted
    /// — an evicted scope carries the flag on its surviving entry)
    /// means the scheduler is the content authority.
    ///
    /// Expects the normalised analysis canonical (the identity
    /// `DerivedRawState` entries are keyed under).
    pub(crate) fn is_artifact_only_scope(&self, analysis_canonical: &str) -> bool {
        if self.derived_raw_cache().get(analysis_canonical).is_some() {
            return false;
        }
        self.scheduler.try_get_source(analysis_canonical).is_none()
    }

    /// THE single authority gate for serving **artifact-only** state:
    /// the canonical is a genuinely artifact-only scope
    /// ([`Self::is_artifact_only_scope`]) AND its file is still present
    /// in the (possibly swapped) workspace. Every artifact-only read —
    /// the raw peek ([`Self::artifact_current_indexed_raw`]), the
    /// re-indexing authority ([`Self::artifact_current_indexed`]), the
    /// `FileArtifacts` lane
    /// ([`Self::current_content_pinned_artifacts`]), the
    /// `analysis_source_exists` probe, and the base
    /// `HostStoreView::build` snapshot — gates here, so state the
    /// accessors reject can never serve OR validate anywhere.
    ///
    /// The freshness leg (`ws().file_exists`) covers a deleted / closed /
    /// never-present file, whose artifact must never serve (and is not
    /// rebuildable, so callers correctly observe "no artifact").
    /// CONTENT-supersession freshness is the serving-class predicate's
    /// job ([`Self::artifact_only_candidate_is_fresh`] — the workspace's
    /// per-canonical content-transition ledger); the host-wrapper
    /// evictions (`notify_close` / `notify_upsert`) remain as the
    /// immediate memory-release path, and `set_workspace` / `close`
    /// clear the whole `FileArtifactStore` (the workspace content
    /// authority swapped out from under every artifact).
    ///
    /// A `workspace_generation` equality clause is deliberately NOT part
    /// of this gate: it would invalidate every artifact-only artifact on
    /// every unrelated content transition, while package-backed
    /// (`node_modules`) artifact-only surfaces must keep serving across
    /// unrelated epoch bumps
    /// (`cached_import_route_resolution_reuses_untracked_current_version_across_epoch_bumps`
    /// pins this). The transition ledger is per-canonical, so it carries
    /// none of that collateral; file-set sensitivity of baked edges is
    /// the edge-currency oracle's job (`route_surface_is_edge_current` +
    /// the known-miss generation sidecar), not this gate's.
    ///
    /// Applies ONLY to the artifact-only lane — scheduler-tracked
    /// canonicals are content-pinned to the scheduler's authoritative
    /// hash instead and never reach this predicate.
    pub(crate) fn artifact_only_authority_allows(&self, analysis_canonical: &str) -> bool {
        self.is_artifact_only_scope(analysis_canonical) && self.ws().file_exists(analysis_canonical)
    }

    /// Serving-class freshness for an artifact-only CANDIDATE: the
    /// authority gate ([`Self::artifact_only_authority_allows`]) PLUS the
    /// workspace's per-canonical content-transition rail — the
    /// candidate's build generation (`IndexedReady.edge_generation`, the
    /// `content_generation` captured when the artifact was built from
    /// live workspace content) must be at-or-after the canonical's last
    /// recorded content transition. The workspace records transitions at
    /// its OWN mutation chokepoints (`notify_upsert` / `notify_close` /
    /// `write_file` / `copy_file` / deletes), so mutators that bypass
    /// the host wrappers — a JS embedder firing the NAPI `Workspace`
    /// methods directly — are covered by construction (read-side
    /// authoritative; the wrapper evictions are a memory-release
    /// optimization, not the authority). Every artifact-BEARING
    /// artifact-only lane gates here; the existence probe
    /// ([`Self::artifact_only_entry_exists`]) deliberately stays on the
    /// authority gate alone — supersession changes WHICH content serves,
    /// not whether the canonical resolves to an analysis source.
    pub(crate) fn artifact_only_candidate_is_fresh(
        &self,
        analysis_canonical: &str,
        build_generation: u64,
    ) -> bool {
        self.artifact_only_authority_allows(analysis_canonical)
            && build_generation
                >= self
                    .ws()
                    .last_content_transition_generation(analysis_canonical)
    }

    /// Non-normalizing artifact-only existence probe for
    /// `analysis_source_exists` — the canonical is probed AS GIVEN (no
    /// `normalized_analysis_canonical` rewrite) because that existence
    /// probe is itself the oracle the normalizer
    /// (`resolve_eval_dependency_canonical`) consults; normalizing here
    /// would mutually recurse. Same single authority gate as the raw
    /// peek (`artifact_only_authority_allows`, conjunction reordered so
    /// the workspace `file_exists` probe runs ONLY when an artifact
    /// actually exists — `analysis_source_exists` is called per
    /// normalization candidate and must not fan workspace probes out
    /// over candidates that have no artifact at all).
    #[must_use]
    pub(crate) fn artifact_only_entry_exists(&self, canonical_id: &str) -> bool {
        self.is_artifact_only_scope(canonical_id)
            && self
                .project_type_store
                .indexed()
                .get_any(canonical_id)
                .is_some()
            && self.ws().file_exists(canonical_id)
    }

    /// Non-recursing raw peek of the artifact-current `IndexedReady` — the
    /// store read [`Self::artifact_current_indexed`] performs WITHOUT its
    /// stale re-index. It honours the same single authority gate
    /// ([`Self::artifact_only_authority_allows`]: a `DerivedRawState`
    /// entry — live or evicted — means the scheduler is the content
    /// authority, and an absent file's artifact never serves), but never
    /// calls `ensure_indexed_ready_serve`: a content-stale candidate yields
    /// `None` and the caller decides whether to rebuild.
    ///
    /// `ensure_indexed_ready_serve`'s own artifact fast-path uses THIS helper (then
    /// edge-filters and falls through to its single `materialize` re-index on
    /// stale) so it has no back-edge into the re-indexing
    /// [`Self::artifact_current_indexed`].
    #[must_use]
    pub(crate) fn artifact_current_indexed_raw(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        let analysis_canonical = self.normalized_analysis_canonical(canonical);
        let analysis_canonical = analysis_canonical.as_ref();
        let indexed = self
            .project_type_store
            .indexed()
            .get_any(analysis_canonical)?;
        // The artifact-current authority answers ONLY through the single
        // authority gate: a canonical with no `DerivedRawState` entry — a
        // genuinely artifact-only scope the scheduler never tracked —
        // whose file is still present. Any `DerivedRawState` entry
        // (whether `evicted` or live) means the scheduler is the
        // content authority: a live scope is served by
        // `current_content_pinned_indexed` and a `get_any` artifact
        // would risk self-rooting under a stale older hash; an evicted
        // scope's surviving artifact is a stale leftover. Gate evaluated
        // AFTER the artifact lookup so a canonical with no artifact costs
        // no workspace `file_exists` probe.
        self.artifact_only_candidate_is_fresh(analysis_canonical, indexed.edge_generation)
            .then_some(indexed)
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
    ///
    /// Keyed by the **normalised analysis canonical**
    /// ([`Self::normalized_analysis_canonical`]) — the `FileArtifacts`
    /// payload is published under the normalised id, matching
    /// [`Self::current_content_pinned_indexed`] /
    /// [`Self::artifact_current_indexed`]. A caller may pass the RAW
    /// requested canonical; the rewrite is idempotent for an
    /// already-normalised id.
    #[must_use]
    pub(crate) fn current_content_pinned_artifacts(
        &self,
        canonical: &str,
    ) -> Option<Arc<crate::file_artifact_store::FileArtifacts>> {
        let analysis_canonical = self.normalized_analysis_canonical(canonical);
        let analysis_canonical = analysis_canonical.as_ref();
        if let Some(current_hash) = self.authoritative_current_content_hash(analysis_canonical) {
            let key = crate::file_artifact_store::FileArtifactKey::base(
                Arc::from(analysis_canonical),
                current_hash,
            );
            return self.project_type_store.indexed().get_artifacts(&key);
        }
        // Genuinely artifact-only canonical — no scheduler authority, so
        // the single retained `FileArtifacts` is the current one. Gated
        // by the SAME single authority predicate as the `IndexedReady`
        // lane: an absent-file canonical the IndexedReady accessors
        // reject must not serve through the `FileArtifacts` lane either.
        // Gate evaluated AFTER the lookup (no-artifact probes cost no
        // workspace `file_exists`).
        let artifacts = self
            .project_type_store
            .indexed()
            .get_artifacts_any(analysis_canonical)?;
        self.artifact_only_candidate_is_fresh(analysis_canonical, artifacts.indexed.edge_generation)
            .then_some(artifacts)
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
    /// Both products derive from ONE held source snapshot — the served snapshot
    /// is single-generation by construction.
    pub fn get_compile_blockers(
        &self,
        canonical_or_alias: &str,
    ) -> Option<CompileBlockersSnapshot> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        {
            use crate::host_executor::HostSourceData;
            if self.is_canonical_evicted(&canonical) {
                return None;
            }
            let snap = self.scheduler.try_get_source(&canonical)?;
            let hd = snap.downcast_data::<HostSourceData>()?;
            if !hd.file_language.is_vue() {
                return None;
            }
            #[cfg(test)]
            self.fire_compile_blockers_serve_seam();
            // Macro type deps from the SAME held source read the
            // external source requests come from: the analysis stage
            // repackages exactly this parse product at the source's
            // generation, so deriving it here keeps the served
            // snapshot single-generation — an independent analysis
            // read could observe a newer node mid-window and pair
            // this source's external requests with the moved
            // generation's macro type deps.
            Some(CompileBlockersSnapshot {
                external_source_requests: hd.parse.external_requests.clone(),
                macro_type_deps: Arc::new(hd.parse.script_analysis.macro_type_deps.clone()),
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
        let analysis_snap = self.scheduler.try_get_analysis(canonical)?;
        self.build_snapshot_from_analysis_snap(canonical, &analysis_snap)
    }

    /// [`Self::build_snapshot_from_scheduler`] plus template-analysis
    /// inputs joined at the SAME generation the analysis snapshot was
    /// read at.
    ///
    /// The lazy template computation needs the canonical's source
    /// bytes; reading them in a second independent scheduler consult
    /// races a mid-flight source move — the computed template would
    /// derive from bytes the snapshot was not built from, and its
    /// `derived_raw_cache` persist (canonical-keyed, no content rail)
    /// would land AFTER the racing upsert's clear, surviving as
    /// poison every subsequent read serves as current. The join here
    /// is by-value and generation-gated: inputs are returned only when
    /// the live source snapshot carries the analysis snapshot's
    /// generation; a torn join returns `None` inputs and the caller
    /// serves without a template — fail closed, never mixed.
    pub(crate) fn build_snapshot_from_scheduler_with_template_inputs(
        &self,
        canonical: &str,
    ) -> Option<(
        FileAnalysisSnapshot,
        Option<crate::types::VueTemplateInputs>,
    )> {
        use crate::host_executor::HostSourceData;

        let analysis_snap = self.scheduler.try_get_analysis(canonical)?;
        let snapshot = self.build_snapshot_from_analysis_snap(canonical, &analysis_snap)?;
        #[cfg(test)]
        self.fire_raw_snapshot_template_join_seam();
        let template_inputs = self
            .scheduler
            .try_get_source(canonical)
            .filter(|source_snap| source_snap.generation == analysis_snap.generation)
            .and_then(|source_snap| {
                let hd = source_snap.downcast_data::<HostSourceData>()?;
                // Template-data ingestion is gated on whether the file's adapter
                // has a registered carrier compiler (registry-dispatched,
                // Svelte-capable), NOT a hardcoded `is_vue()` check — a `.svelte`
                // owner reaches `compile_template_data` the same as a `.vue` one.
                if !crate::parse::file_language_has_template_data_compiler(&hd.file_language) {
                    return None;
                }
                Some(crate::types::VueTemplateInputs {
                    source: source_snap.source.clone(),
                    framework_parse: hd.framework_parse.clone(),
                    // Live scheduler reads at one generation —
                    // store-authoritative.
                    store_published: true,
                    source_generation: Some(source_snap.generation),
                })
            });
        Some((snapshot, template_inputs))
    }

    fn build_snapshot_from_analysis_snap(
        &self,
        canonical: &str,
        analysis_snap: &verter_scheduler::node::AnalysisSnapshot,
    ) -> Option<FileAnalysisSnapshot> {
        use crate::host_executor::HostAnalysisData;

        let ad = analysis_snap.downcast_data::<HostAnalysisData>()?;

        // Generation rail: accept the persisted template only at the
        // analysis snapshot's own generation (analysis snapshots are
        // generation-coherent with their source) — a late persist
        // stamped with a superseded generation must not serve as
        // current.
        let template = self.derived_raw_cache().get(canonical).and_then(|cc| {
            cc.raw_template_analysis()
                .filter(|entry| entry.source_generation == analysis_snap.generation)
                .map(|entry| Arc::clone(&entry.template))
        });

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

        // ProfileState owns the compile-output slots; DerivedRawState
        // owns the cached_resolved_meta + cached_meta_payload (D48
        // split). The compile-output slots are cleared through the
        // typed session node.
        if let Some(mut cc) = self.compile_cache().get_mut(&canonical) {
            let session_node = crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
            session_node.clear_compile_outputs_for_file(&mut cc);
        }
        if let Some(mut derived) = self.derived_raw_cache().get_mut(&canonical) {
            derived.cached_resolved_meta.clear();
            derived.cached_meta_payload = None;
        }
        // The content-addressed node is keyed independently of the
        // per-profile session slots, so a targeted invalidation must flush
        // it explicitly — a `Content` key carries no fact rail, so a
        // same-content recompile would otherwise warm-hit and break the
        // force-recompute contract.
        self.compile_output_pure_content()
            .remove_canonical(&canonical);
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
                let session_node =
                    crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
                session_node.clear_compile_outputs_for_file(&mut cc);
            }
            if let Some(mut derived) = self.derived_raw_cache().get_mut(owner) {
                derived.cached_resolved_meta.clear();
                derived.cached_meta_payload = None;
            }
            // Flush each dependent owner's content-addressed entries too —
            // the removed file may have contributed to their compiled
            // output, and the content node is keyed independently of the
            // session slots cleared above.
            self.compile_output_pure_content().remove_canonical(owner);
        }

        // notify_delete fires EdgeStore::remove_file (surgical per-owner
        // canonical-axis + active-stem cleanup; avoids a full-graph
        // rescan on delete).
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
    /// `tests/cases/g_misc3/import_route_writer_guard.rs`):
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
        let existing_routes = self
            .derived_raw_cache()
            .get(&canonical)
            .map(|d| d.import_routes.clone())
            .unwrap_or_default();
        // No-op oracle, half 1: the admitted per-canonical route table.
        // The bundler re-calls this method after every upsert with an
        // unchanged snapshot in the steady state — a value-identical
        // re-push must not trigger the project-wide invalidation cascade
        // below.
        let routes_changed = existing_routes != import_routes;
        let old_direct_deps = {
            let mut deps = parse_deps.clone();
            deps.extend(Self::resolved_dependency_targets(&existing_routes));
            deps
        };
        // Replace import_routes on DerivedRawState.
        // R3/R26/R28: for each known-miss in the new map,
        // record the workspace `content_generation` at admission so
        // the reader can detect when a new canonical (which advances
        // content_generation) may now satisfy the previously
        // unresolvable specifier. The pushed POSITIVE resolutions are
        // caller-supplied authoritative routes (the bundler tells the
        // host how ITS resolver resolves and re-pushes on its own
        // watch events), so they carry NO positive generation stamp —
        // the wholesale replace also drops any host-memoized stamps
        // from the previous table.
        //
        // EXEMPTION from the capture-before-resolve stamp discipline:
        // this stamp is a LIVE read at record time, unlike the
        // host-memoized positive stamps (which are captured by the
        // resolving caller before its resolution runs). The resolution
        // here ran in the CALLER's process (the bundler's own
        // resolver) — there is no host-side pre-resolve point to
        // capture. The record-time read is the tightest capture
        // available, and the residual window (a file appears after the
        // bundler resolved but before this push records — the miss is
        // then stamped current for the remainder of this generation) is
        // covered by the caller-authority contract itself: the bundler
        // re-pushes on its own watch events, and any subsequent
        // content-generation move stales the stamp and re-resolves the
        // miss host-side.
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
            derived_ref
                .value_mut()
                .import_routes_positive_recorded_at_generation
                .clear();
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

        // Sync exact resolutions to workspace. The workspace's
        // `replace_exact_resolutions` is value-idempotent and reports
        // whether the stored table actually changed — the no-op oracle's
        // half 2 (the exact table can change even when `import_routes`
        // is value-identical, e.g. after a workspace-driven
        // effective-target shift).
        let exacts_changed = self
            .ws()
            .set_exact_resolutions(&canonical, vfs_resolutions)
            .changed;
        if !routes_changed && !exacts_changed {
            // TRUE no-op: nothing route-observable changed, so there is
            // nothing for a fence to make visible and nothing to evict.
            // Bumping here anyway would read-invalidate every
            // `validated_at_generation`-gated cache project-wide and stamp
            // every cross-file-edge `IndexedReady` stale on EVERY
            // steady-state bundler push. The known-miss generation sidecar
            // re-stamp above is deliberately retained — it records the
            // caller's fresh (still-miss) resolution observation and moves
            // no fence dimension.
            return;
        }
        // `set_import_dependencies` is a route-resolution mutation: the
        // per-canonical route table (`DerivedRawState.import_routes`,
        // mutated above) and the workspace exact-resolution table both
        // changed while `content_generation` stays put. Bump
        // `project_generation` so the mutation is FENCE-VISIBLE — an
        // in-flight materialise that captured the pre-mutation stamp must
        // trip the pre-publish fence (ReturnOnly) instead of publishing a
        // stale route surface that afterwards passes
        // `indexed_surface_is_current`. MUTATE-FIRST ordering (the bump
        // strictly follows every route-affecting write above) — see
        // `VerterHost::set_exact_resolutions` for why bump-before-mutate
        // is a fence-defeating order.
        //
        // STAMP-ONLY bump (not `bump_project_generation_and_evict`): the
        // wide evict variant clears `derived_raw_cache_db` wholesale,
        // which would destroy the very `import_routes` /
        // known-miss-generation state this method just admitted (and
        // every other canonical's bundler-preloaded routes). The stamp
        // move alone is what the fence and the
        // `indexed_surface_is_current` read gate consume; per-canonical
        // derived layers are drained right below, and OTHER canonicals'
        // cross-edge surfaces refresh lazily through the stamp gate
        // (edge refresh — no re-parse).
        self.project_type_store.bump_project_generation();
        // Soft-invalidate: file content didn't change, only import routes.
        // The content-addressed `IndexedReady` payload is RETAINED — the
        // project-stamp read gate routes the next read through the
        // edge-refresh materialise (route surface rebuilt, no re-parse),
        // the same shape as the `set_exact_resolutions` wrapper cascade.
        self.resolver.runtime.invalidate_canonical(&canonical);
        self.project_type_store
            .evict_canonical_for_route_mutation(&canonical);
        // R4 producer: rebuild parse-domain facts for the externally
        // changed canonical.
        self.register_facts_for_new_content(&canonical);
        self.bump_store_view_epoch();
    }

    /// Returns all known canonical file IDs and their file languages.
    pub fn list_files(&self) -> Vec<(String, FileLanguage)> {
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
                    Some((id, hd.file_language.clone()))
                })
                .collect()
        }
    }

    pub(crate) fn raw_template_analysis_for_file(
        &self,
        canonical: &str,
    ) -> Option<Arc<verter_semantic::analysis::template::TemplateAnalysisSnapshot>> {
        {
            if self.is_canonical_evicted(canonical) {
                return None;
            }
            // Snapshot + template inputs joined at one generation — a
            // torn join carries `None` inputs and this caller serves
            // without a template (fail closed, never mixed). The
            // template-data compiler gate lives inside
            // `compute_template_analysis_if_missing` (registry-dispatched on the
            // file's carrier row), so a non-carrier serves no template here.
            let (mut snapshot, template_inputs) =
                self.build_snapshot_from_scheduler_with_template_inputs(canonical)?;
            if let Some(inputs) = template_inputs {
                self.compute_template_analysis_if_missing(canonical, &mut snapshot, inputs);
            }
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

        let file_language = self.language_classifier().classify(canonical);
        self.build_template_analysis(
            canonical,
            &file_language,
            &override_with_parse.source,
            override_with_parse.framework_parse.clone(),
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
                            let session_node =
                                crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
                            session_node
                                .peek_template_analysis(&cc, profile_hash)
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
        FileLanguage,
        Arc<verter_semantic::analysis::ScriptAnalysisSnapshot>,
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
                    let file_language = source_snap
                        .downcast_data::<crate::host_executor::HostSourceData>()?
                        .file_language
                        .clone();
                    let analysis =
                        analysis_snap.downcast_data::<crate::host_executor::HostAnalysisData>()?;
                    return Some((
                        file_language,
                        Arc::clone(&analysis.script_analysis),
                        analysis.export_signatures.clone(),
                    ));
                }
            }
        }

        if let Some(facts) = self
            .ensure_indexed_ready_serve(&canonical)
            .map(|serve| serve.indexed)
        {
            if let (Some(script_analysis), Some(export_signatures)) = (
                facts.script_analysis.as_ref(),
                facts.export_signatures.as_ref(),
            ) {
                return Some((
                    self.language_classifier.classify(&canonical),
                    Arc::clone(script_analysis),
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
            let file_language = hd.file_language.clone();
            drop(source_snap);

            let analysis_snap = self.scheduler.try_get_analysis(&canonical)?;
            let ad = analysis_snap.downcast_data::<HostAnalysisData>()?;

            Self::find_export_span(
                &file_language,
                &ad.script_analysis,
                &ad.export_signatures,
                binding_name,
            )
        }
    }

    /// Shared logic for finding an export span from analysis data.
    pub(super) fn find_export_span(
        file_language: &FileLanguage,
        script_analysis: &verter_semantic::analysis::ScriptAnalysisSnapshot,
        export_signatures: &[verter_semantic::analysis::ExportSignature],
        binding_name: &str,
    ) -> Option<(u32, u32)> {
        // Framework component files (Vue included) synthesize one semantic
        // default export. Adapters without an authored source token for that
        // value still need an honest definition anchor so export-graph
        // traversal can terminate at the component instead of aborting at the
        // final barrel hop — anchor at the file start.
        if file_language.is_framework_carrier() && binding_name == "default" {
            return Some((0, 0));
        }

        if file_language.is_vue() {
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

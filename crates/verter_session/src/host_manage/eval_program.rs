//! `host_manage::eval_program` — eval-source / parsed-program / external-type-analysis bridge.
//!
//! Domain D. Holds the host-instance-scoped
//! parsed-program / type-context caches and the bridge between raw
//! source loading and the OXC-based external-type analyzer. Public
//! surface remains rooted at `crate::host_manage::*`; this file
//! contributes a private `impl VerterHost { … }` block that
//! continues the parent shell's impl chain.

use std::rc::Rc;
use std::sync::Arc;

use crate::types::*;
use crate::VerterHost;

use super::{
    component_meta_trace_custom, is_raw_import_specifier_id, read_analysis_source_result_detail,
    ExternalTypeResolutionInputs, HostNamedTypeCacheAdapter,
};

impl VerterHost {
    pub(crate) fn store_view_allows_current_whole_hash(
        &self,
        _canonical_id: &str,
        _whole_hash: Hash16,
    ) -> bool {
        // No ambient request view gates hash acceptance. Live-host
        // probes operate directly on the project-global caches, whose
        // candidates fact-validate on warm read against the live
        // `StoreView`.
        true
    }

    /// Build script-setup generic type parameter bindings for a Vue SFC.
    /// Called once during `PreparedDeclBundle` materialization. Returns an
    /// empty map for non-Vue files or Vue files without `<script setup>` generics.
    ///
    /// The result type is `FxHashMap<String, TypeParamBinding>` —
    /// script-setup parameters are stored directly rather than
    /// wrapped in a `PreparedTypeDecl`. Constraint / default lowering
    /// threads its own `name_resolution` table through
    /// `shallow_lower_type_expr`, so a separate wrapper-side
    /// `name_resolution` table on the binding would be dead
    /// allocation.
    pub(super) fn build_script_setup_type_bindings(
        &self,
        canonical_id: &str,
        _state: &crate::resolver_core::ShallowFileState,
        _dep_edges: &rustc_hash::FxHashMap<String, String>,
    ) -> rustc_hash::FxHashMap<String, crate::resolver_core::prepared_decl::TypeParamBinding> {
        use crate::resolver_core::prepared_decl::TypeParamBinding;

        let mut bindings = rustc_hash::FxHashMap::default();

        let Some((raw_source, framework_parse, _)) = self.current_eval_state(canonical_id) else {
            return bindings;
        };

        for (idx, param) in crate::host_resolve::sfc_script_setup_type_params(
            raw_source.as_ref(),
            framework_parse.as_deref(),
        )
        .into_iter()
        .enumerate()
        {
            bindings.insert(
                param.name.clone(),
                TypeParamBinding {
                    name: std::sync::Arc::from(param.name.as_str()),
                    // 0-based clause position so multiple
                    // `<script setup generic="T, U">` params get
                    // distinct identity tuples.
                    ordinal: u16::try_from(idx).unwrap_or(u16::MAX),
                    constraint: param.constraint.clone(),
                    default: param.default.clone(),
                },
            );
        }

        bindings
    }

    /// Central caller for the `IndexedReady.eval_source` body.
    ///
    /// For a framework CARRIER (any adapter — Vue, Svelte, …) this returns the
    /// **position-preserving** script-only source: each script block's content
    /// sits at its RAW carrier byte offsets and every other byte is
    /// whitespace-blanked (line terminators preserved), so every OXC-produced
    /// span — eval-env decls, external-type analysis, member/signature spans —
    /// is carrier-absolute by construction. The blanking is CARRIER-NEUTRAL: it
    /// reads the neutral `FrameworkParseCommon.script_regions` the carrier's
    /// producer populated (BOTH the instance and module script blocks), so a new
    /// carrier needs no per-adapter eval-source branch here. For a non-carrier
    /// file the source is returned unchanged (its offsets are already
    /// file-absolute).
    pub(crate) fn build_eval_script_source(
        source: &str,
        framework_parse: Option<&verter_language::FrameworkParseArtifact>,
    ) -> String {
        Self::build_eval_script_source_with_extraction(source, framework_parse).0
    }

    /// [`Self::build_eval_script_source`] plus the extraction provenance: the
    /// `bool` is `true` iff the returned text is the position-preserving
    /// extracted CARRIER script — exactly the case where the flight's
    /// eval-program parse over this text IS the snapshot's script program and
    /// can be threaded into the snapshot build
    /// ([`crate::parse::VueScriptProgram::Shared`] for Vue,
    /// [`crate::parse::FrameworkScriptProgram::Shared`] for other carriers).
    /// `false` means the raw source passed through unchanged (non-carrier
    /// files, or a carrier with no extractable script), where the eval program
    /// covers different bytes than a script-program walk would.
    ///
    /// Carrier-NEUTRAL: a non-Vue carrier blanks from the producer's recorded
    /// `script_regions` (both instance + module blocks), Vue keeps its exact
    /// `extract_vue_script_content` behaviour, and a non-carrier passes through.
    pub(crate) fn build_eval_script_source_with_extraction(
        source: &str,
        framework_parse: Option<&verter_language::FrameworkParseArtifact>,
    ) -> (String, bool) {
        // A NON-Vue framework carrier blanks NEUTRALLY from the producer's
        // recorded `script_regions` (BOTH the instance and module script blocks)
        // — so a `.svelte` eval-source carries both scripts at their raw carrier
        // offsets with the markup/styles whitespace-blanked. A new carrier needs
        // no per-adapter branch here. The blanked text IS a position-preserving
        // extracted script (the eval program over it is shareable), so the
        // provenance is `true`.
        if let Some(artifact) = framework_parse {
            let is_vue = crate::typeinfo::adapters::vue::vue_parse(artifact).is_some();
            if !is_vue {
                let mut spans: Vec<(u32, u32)> = artifact
                    .common
                    .script_regions
                    .iter()
                    .map(|region| (region.span.start, region.span.end))
                    .filter(|(start, end)| end > start)
                    .collect();
                spans.sort_by_key(|(start, _)| *start);
                // Even with NO script regions (a pure-markup `.svelte`) the
                // eval-source is the FULLY-BLANKED, line-preserving source — never
                // the raw markup. `build_position_preserving_script_source` over an
                // empty span set blanks every non-line-terminator byte, so a shared
                // eval-source consumer parses an empty TS program, not HTML.
                let blanked =
                    crate::host_resolve::build_position_preserving_script_source(source, &spans);
                return (blanked, true);
            }
        }
        // Vue (and the no-artifact fallback) keep the EXACT existing extraction:
        // the parser-vs-raw-scan agreement + the forgiving raw scan when no
        // parsed SFC is available + the inter-script `\n` injection. This is the
        // byte-identical pre-existing behaviour (a Vue file arriving without a
        // framework_parse still extracts its `<script>` from the raw markup).
        let parsed = framework_parse.and_then(crate::typeinfo::adapters::vue::vue_parse);
        match crate::host_resolve::extract_vue_script_content(source, parsed.map(|p| p.as_ref())) {
            Some(script) => (script, true),
            None => (source.to_string(), false),
        }
    }

    /// Selects the `.vue` snapshot build's script-program input for a cold
    /// materialise flight — the SINGLE decision point both the base and overlay
    /// flights share. The flight's eval program IS the snapshot's script program
    /// exactly when the eval source is the position-preserving extracted script:
    /// [`crate::parse::VueScriptProgram::Shared`] on a live parse,
    /// [`crate::parse::VueScriptProgram::SharedFatal`] on a fatal one (a re-parse
    /// of the same bytes under the same source type fails identically), and
    /// [`crate::parse::VueScriptProgram::ParseHere`] when the raw source passed
    /// through unextracted (the eval program covers different bytes than a
    /// script-program walk would).
    pub(crate) fn vue_flight_script_program<'a>(
        eval_is_extracted_script: bool,
        parsed_eval_program: Option<&'a crate::ParsedEvalProgram>,
    ) -> crate::parse::VueScriptProgram<'a> {
        if !eval_is_extracted_script {
            return crate::parse::VueScriptProgram::ParseHere;
        }
        match parsed_eval_program {
            Some(program) => crate::parse::VueScriptProgram::Shared(program),
            None => crate::parse::VueScriptProgram::SharedFatal,
        }
    }

    /// The carrier-neutral analog of [`Self::vue_flight_script_program`] — the
    /// SINGLE decision point selecting a non-Vue carrier snapshot's script
    /// program for a cold materialise flight (Svelte today). The flight's eval
    /// program IS the snapshot's script program exactly when the eval source is
    /// the position-preserving extracted carrier script, so the snapshot walks
    /// the retained parse instead of re-parsing the same bytes.
    pub(crate) fn framework_flight_script_program<'a>(
        eval_is_extracted_script: bool,
        parsed_eval_program: Option<&'a crate::ParsedEvalProgram>,
    ) -> crate::parse::FrameworkScriptProgram<'a> {
        if !eval_is_extracted_script {
            return crate::parse::FrameworkScriptProgram::ParseHere;
        }
        match parsed_eval_program {
            Some(program) => crate::parse::FrameworkScriptProgram::Shared(program),
            None => crate::parse::FrameworkScriptProgram::SharedFatal,
        }
    }

    /// Resolve the authoritative `source_type` for cache-key purposes.
    ///
    /// Prefers the scheduler-stored [`crate::host_executor::HostSourceData::source_type`]
    /// (set once at `execute_source` time). Falls back to a pure recomputation for
    /// canonicals the scheduler has not yet processed — WASM path, first-time routing,
    /// or snapshot construction for files the scheduler does not own.
    pub(crate) fn imported_eval_source_type_for(
        &self,
        canonical_id: &str,
        framework_parse: Option<&verter_language::FrameworkParseArtifact>,
    ) -> oxc_span::SourceType {
        {
            if let Some(st) = self.authoritative_source_type_for(canonical_id) {
                return st;
            }
        }
        crate::parse::imported_eval_source_type(
            &self.language_classifier.classify(canonical_id),
            framework_parse,
        )
    }

    /// View-aware variant of [`Self::read_analysis_source`].
    ///
    /// Consults `view.source(canonical)` FIRST so overlay-only
    /// sources are visible to the resolver tier; falls back to the
    /// base host's read path on miss.
    ///
    /// Per R17: the view does NOT mutate the host. Per R18: the view
    /// is threaded explicitly (no TLS view globals). Only call sites
    /// with a view in scope (those rooted at
    /// `get_component_meta_via_view` / `evaluate_types` session
    /// entry-points) use this variant.
    ///
    /// Substrate-only API. Resolver-tier migration of deep callers
    /// (e.g., `extract_component_meta_from_inputs`,
    /// `compute_template_analysis_if_missing`) is consumer-side
    /// work tracked separately.
    #[allow(dead_code)]
    pub(crate) fn read_analysis_source_via_view(
        &self,
        canonical_id: &str,
        view: &dyn crate::session_view::SessionView,
    ) -> Option<Arc<str>> {
        if canonical_id.is_empty() {
            return None;
        }
        if let Some(source) = view.source(canonical_id) {
            component_meta_trace_custom!(
                "read_analysis_source_result",
                read_analysis_source_result_detail(
                    canonical_id,
                    "session-view",
                    source.len(),
                    false
                ),
            );
            return Some(source);
        }
        self.read_analysis_source(canonical_id)
    }

    pub(crate) fn read_analysis_source(&self, canonical_id: &str) -> Option<Arc<str>> {
        component_meta_trace_custom!("read_analysis_source", format!("owner={canonical_id}"));
        if canonical_id.is_empty() {
            component_meta_trace_custom!(
                "read_analysis_source_result",
                read_analysis_source_result_detail("", "empty-canonical", 0, true),
            );
            return None;
        }
        if let Some(source) = self.get_source(canonical_id) {
            component_meta_trace_custom!(
                "read_analysis_source_result",
                read_analysis_source_result_detail(canonical_id, "host-cache", source.len(), false,),
            );
            return Some(source);
        }

        // Project-global IndexedReady cache for cached raw_source —
        // **current-content-pinned** (no `get_any`). `read_analysis_source`
        // feeds the routed-shallow prepared-decl materialiser (a
        // route-fact producer) and the cold analysis path; a stale pre-edit
        // `IndexedReady` (which can linger past a same-canonical edit with
        // the own-canonical drain retired) would seed those producers with
        // pre-edit source. `artifact_current_indexed` answers ONLY for a
        // genuinely artifact-only canonical (no scheduler `DerivedRawState`)
        // — exactly the foreign-source / test-seed scope this fallback
        // exists for. A scheduler-tracked canonical (whose authoritative
        // source is the scheduler, read above via `get_source`) gets `None`
        // here, so a live-but-stale scope falls through to `ensure_loaded`
        // rather than reading the stale artifact.
        if let Some(facts) = self.artifact_current_indexed(canonical_id) {
            component_meta_trace_custom!(
                "read_analysis_source_result",
                read_analysis_source_result_detail(
                    canonical_id,
                    "module-facts-db",
                    facts.raw_source.len(),
                    false,
                ),
            );
            return Some(Arc::clone(&facts.raw_source));
        }

        if is_raw_import_specifier_id(canonical_id) {
            component_meta_trace_custom!(
                "read_analysis_source_result",
                read_analysis_source_result_detail(canonical_id, "raw-import-specifier", 0, true,),
            );
            return None;
        }

        // Native: scheduler is the sole parser + source authority. On a cache
        // miss, submit through `ensure_loaded` (canonical loading path via
        // the scheduler). If the scheduler still has no source, the file
        // genuinely doesn't exist.
        //
        // WASM: no scheduler; the `files` map is the authority and the
        // workspace fallback is legitimate.
        {
            if self.ensure_loaded(canonical_id) {
                if let Some(source) = self.get_source(canonical_id) {
                    component_meta_trace_custom!(
                        "read_analysis_source_result",
                        read_analysis_source_result_detail(
                            canonical_id,
                            "ensure-loaded",
                            source.len(),
                            false,
                        ),
                    );
                    // supplement §5.D.0 r17 — fresh load,
                    // count for the host-level test audit. Fresh
                    // means we missed both `get_source` and
                    // `FileArtifactStore::get_any` before reaching here.
                    #[cfg(test)]
                    self.test_audit.record_read(canonical_id);
                    return Some(source);
                }
                // Post-`ensure_loaded` artifact fallback — content-pinned
                // via `artifact_current_indexed` for the same reason as the
                // pre-`ensure_loaded` read above: a stale lingering
                // artifact must not seed the cold analysis / route-fact
                // producers with pre-edit source.
                if let Some(facts) = self.artifact_current_indexed(canonical_id) {
                    component_meta_trace_custom!(
                        "read_analysis_source_result",
                        read_analysis_source_result_detail(
                            canonical_id,
                            "ensure-loaded-module-facts",
                            facts.raw_source.len(),
                            false,
                        ),
                    );
                    #[cfg(test)]
                    self.test_audit.record_read(canonical_id);
                    return Some(Arc::clone(&facts.raw_source));
                }
            }
            component_meta_trace_custom!(
                "read_analysis_source_result",
                read_analysis_source_result_detail(canonical_id, "not-loaded", 0, true,),
            );
            None
        }
    }

    pub(super) fn analysis_source_exists(&self, canonical_id: &str) -> bool {
        if canonical_id.is_empty() {
            return false;
        }

        {
            if let Some(state) = self.effective_file_state(canonical_id, None) {
                return self.store_view_allows_current_whole_hash(canonical_id, state.whole_hash);
            }
        }

        // Artifact-only existence probe — gated by the single authority
        // predicate (`artifact_only_authority_allows`) so a retained
        // artifact the accessors reject (absent file,
        // scheduler-superseded scope) does not claim existence here.
        // Non-normalizing by contract: this probe is the oracle the
        // canonical normalizer consults.
        if self.artifact_only_entry_exists(canonical_id) {
            return true;
        }

        if is_raw_import_specifier_id(canonical_id) {
            return false;
        }

        self.ws().file_exists(canonical_id)
    }

    /// THE single host parse entry for the borrowed eval-program form.
    ///
    /// Produces a fresh `ParsedEvalProgram` per call. The parse lives and dies
    /// on the caller's stack (the OXC arena is `!Send` and must never enter host
    /// caches or thread-locals); the `IndexedReady` materialise closure threads
    /// the parsed program by reference so a cold canonical build parses exactly
    /// once. Concurrent cold callers collapse on `indexed_singleflight`.
    ///
    /// Returns `None` when the parse panicked (fatal syntax fault) —
    /// callers fall back to default analysis / an empty env rather than
    /// re-parsing under a different source type.
    pub(crate) fn parse_eval_program(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        eval_source: &Arc<str>,
        source_type: oxc_span::SourceType,
    ) -> Option<Rc<crate::ParsedEvalProgram>> {
        self.provenance
            .eval_program_parses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let parsed = crate::ParsedEvalProgram::parse(Arc::clone(eval_source), source_type);
        component_meta_trace_custom!(
            "parse_eval_program",
            format!(
                "owner={} bytes={} whole_hash={whole_hash:?} parse_failed={}",
                canonical_id,
                eval_source.len(),
                parsed.is_none(),
            ),
        );
        parsed.map(Rc::new)
    }

    /// Builds a fresh `ParsedTypeResolutionContext` per call.
    ///
    /// This per-call parse sits on the query-time OXC element-resolver
    /// path (tracked-debt on the single-engine shrinking ledger — the
    /// shared typed-IR dispatch is the sole sanctioned query-time
    /// resolver); the parse routes through [`Self::parse_eval_program`]
    /// so the parse-entry pin (`no_direct_oxc_parser_calls_outside_scheduler_path`)
    /// keeps covering it.
    pub(super) fn build_type_resolution_context(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        eval_source: &Arc<str>,
        source_type: oxc_span::SourceType,
    ) -> Option<Rc<crate::ParsedTypeResolutionContext>> {
        let program =
            self.parse_eval_program(canonical_id, whole_hash, eval_source, source_type)?;

        let graph = std::sync::Arc::clone(self.project_type_store.semantic_graph());
        // Snapshot the resolved-named-type reset epoch at adapter
        // construction. Every `insert` this adapter performs is fenced
        // against this snapshot: if a `bump_project_generation_and_evict`
        // moves the epoch while this build is in flight, the build's
        // straggler inserts are rejected — see
        // `SemanticGraphStore::insert_resolved_named_type`.
        let named_type_generation = graph.named_type_generation();
        // Env-scope the resolved named-type identity (R T L J) from the
        // defining canonical's per-project env view — two resolutions of
        // the same content under different envs must not collide.
        let env = self.host_view_env_hashes_for(canonical_id);
        let project_identity = self.host_view_project_identity_for(canonical_id).fold_u32();
        let adapter: std::sync::Arc<
            dyn verter_compiler::utils::oxc::vue::named_type_keys::NamedTypeCache + Send + Sync,
        > = std::sync::Arc::new(HostNamedTypeCacheAdapter {
            graph,
            canonical_id: Arc::<str>::from(canonical_id),
            whole_hash,
            resolve_env_hash: env.resolve_env_hash,
            type_env_hash: env.type_env_hash,
            lib_env_hash: env.lib_env_hash,
            project_identity,
            named_type_generation,
        });
        let type_context = Rc::new(crate::ParsedTypeResolutionContext::new(
            program,
            |parsed_program| {
                let program = parsed_program.borrow_dependent();
                let mut ctx = verter_compiler::utils::oxc::script::type_surface::build_type_context(
                    program,
                    parsed_program.source_bytes(),
                    0,
                );
                ctx.set_trace_label(canonical_id.to_string());
                ctx.set_named_type_cache(Some(adapter));
                ctx
            },
        ));
        component_meta_trace_custom!(
            "build_type_resolution_context",
            format!(
                "owner={} bytes={} whole_hash={whole_hash:?}",
                canonical_id,
                eval_source.len(),
            ),
        );
        Some(type_context)
    }

    pub(super) fn external_type_resolution_inputs(
        &self,
        canonical_id: &str,
    ) -> Option<ExternalTypeResolutionInputs> {
        self.external_type_resolution_inputs_with_view(canonical_id, None)
    }

    /// View-aware variant of [`Self::external_type_resolution_inputs`].
    ///
    /// When the active session view carries parse artifacts for `canonical_id`
    /// (i.e. an overlay candidate has been published into FileArtifactStore
    /// under the overlay content hash), the inputs are read from the view's
    /// artifacts so the session-bearing cold compute sees overlay-rooted
    /// shallow state and analysis. Base callers (`view = None`) read the
    /// content-pinned `FileArtifactStore` fast path and fall through to
    /// the singleflighted `ensure_indexed_ready_serve` cold build.
    pub(super) fn external_type_resolution_inputs_with_view(
        &self,
        canonical_id: &str,
        view: Option<&dyn crate::session_view::SessionView>,
    ) -> Option<ExternalTypeResolutionInputs> {
        // Two-identity split. `canonical_id` is the RAW dependency
        // canonical the caller requested; `identity` carries it
        // alongside `analysis_canonical` (the `normalized_analysis_canonical`
        // rewrite). The overlay-artifact read below MUST go through the
        // raw owner (the `SessionView` overlay maps + the overlay-set
        // discriminator are raw-keyed) while resolving to the
        // normalised `FileArtifactStore` identity; the project-global
        // fast path and the `ensure_indexed_ready_serve` fall-through below
        // key on the normalised analysis canonical (the artifact /
        // type-context cache identity).
        let identity = self.overlay_artifact_identity(canonical_id);
        let canonical_id = identity.analysis_canonical();

        // Overlay-priority: when the session view carries the published
        // overlay artifact for this canonical, return it so the session
        // path sees the overlay content. `lookup_overlay_artifacts`
        // builds the exact `overlay_scoped` key the overlay materialiser
        // published under — raw-owner hash + discriminator, normalised
        // `FileArtifactKey.canonical` — so it reaches the candidate even
        // when `normalize(raw) != raw`.
        if let Some(view) = view {
            if view
                .overlay_content_hash_for(identity.raw_overlay_owner())
                .is_some()
            {
                // GENUINELY OVERLAID canonical: route through the gated overlay
                // materialiser so an edge-stale wildcard `export *` surface
                // re-resolves from the OVERLAY source (never the base surface).
                if let Some(indexed) = self
                    .materialize_overlay_indexed_ready_serve_with_view(
                        identity.raw_overlay_owner(),
                        view,
                    )
                    .map(|serve| serve.indexed)
                {
                    return Some(ExternalTypeResolutionInputs {
                        framework_parse: indexed.framework_parse.clone(),
                        whole_hash: indexed.whole_hash,
                        eval_source: Arc::clone(&indexed.eval_source),
                        analysis: Arc::clone(&indexed.external_type_analysis),
                        analysis_cache_hit: true,
                    });
                }
            }
            // Unmasked (non-overlaid) canonical: fall through to the base
            // accessor below (`current_content_pinned_indexed`), which is
            // edge-gated and re-indexes a stale wildcard surface.
        }

        // Project-global `FileArtifactStore` fast path. The read is
        // **current-content-pinned** — never the content-agnostic
        // `get_any`. `ExternalTypeResolutionInputs` carries the dep's
        // `whole_hash` and `external_type_analysis`; that analysis feeds
        // cross-file macro-type resolution (`defineProps<Foo>` etc.) and
        // the observed `whole_hash` roots the consumer's
        // `fact_dep_signature`. With the own-canonical drain retired, a
        // `get_any` read would surface a stale pre-edit `IndexedReady`
        // after a same-canonical edit, so the consumer would resolve the
        // stale `Foo` body and root its signature on the stale hash.
        // `current_content_pinned_indexed` serves only a content-current
        // artifact for a scheduler-tracked canonical;
        // `artifact_current_indexed` answers for a genuinely artifact-only
        // canonical (foreign source / test seed). A stale older-content
        // artifact for a live scope misses both — the singleflighted
        // `ensure_indexed_ready_serve` build below rematerialises from
        // current content.
        let cached_facts = self
            .current_content_pinned_indexed(canonical_id)
            .or_else(|| self.artifact_current_indexed(canonical_id));
        if let Some(facts) = cached_facts {
            let inputs = ExternalTypeResolutionInputs {
                framework_parse: facts.framework_parse.clone(),
                whole_hash: facts.whole_hash,
                eval_source: Arc::clone(&facts.eval_source),
                analysis: Arc::clone(&facts.external_type_analysis),
                analysis_cache_hit: true,
            };
            return Some(inputs);
        }

        // Cold fall-through: JOIN the canonical `IndexedReady` build —
        // external-type analysis is built exactly once per
        // `(canonical, whole_hash)` on the single materialise path and
        // shared with every other reader.
        let indexed = self.ensure_indexed_ready_serve(canonical_id)?.indexed;
        let inputs = ExternalTypeResolutionInputs {
            framework_parse: indexed.framework_parse.clone(),
            whole_hash: indexed.whole_hash,
            eval_source: Arc::clone(&indexed.eval_source),
            analysis: Arc::clone(&indexed.external_type_analysis),
            // The materialiser publishes once per content generation; warm
            // hits return the same artifact through the fast path. Treat
            // warm hits as cache hits for telemetry.
            analysis_cache_hit: true,
        };
        Some(inputs)
    }

    /// Epoch-bump hook: clears the host-owned resolved-named-type
    /// identities on the shared `SemanticGraphStore` (the only
    /// epoch-scoped cache this hook owns — parse results live on
    /// `IndexedReady` / the scheduler, never in thread-locals or a
    /// separate eval-env cache).
    pub(crate) fn clear_resolved_named_type_cache(&self) {
        // Clear host-owned named-type cache on epoch bump. Entries live
        // on the shared `SemanticGraphStore` under
        // `HostResolvedNamedTypeKey` identities, scoped by
        // `(canonical_id, whole_hash)`. Whole_hash reflects one
        // workspace content generation, so a bumped epoch means at
        // least one canonical's facts changed; we prefer to drop stale
        // entries over validating each one lazily (which would require
        // a per-canonical invalidation pass).
        self.project_type_store
            .semantic_graph()
            .clear_resolved_named_types();
    }
}

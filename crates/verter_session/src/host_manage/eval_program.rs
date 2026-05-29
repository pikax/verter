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
    ExternalTypeResolutionInputs, HostNamedTypeCacheAdapter, ParsedEvalProgramCacheEntry,
    ParsedEvalProgramCacheKey,
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

        let Some((raw_source, cached_parse, _)) = self.current_eval_state(canonical_id) else {
            return bindings;
        };

        for (idx, param) in
            Self::sfc_script_setup_type_params(raw_source.as_ref(), cached_parse.as_deref())
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

    pub(crate) fn sfc_script_setup_type_params(
        source: &str,
        cached_parse: Option<&verter_compiler::parser::types::ParsedSfc>,
    ) -> Vec<verter_type_expr::TypeParam> {
        let Some(setup) = cached_parse.and_then(|parsed| parsed.script_setup()) else {
            return Vec::new();
        };
        let Some(generic_span) = setup.generic else {
            return Vec::new();
        };
        let clause = source[generic_span.start as usize..generic_span.end as usize].trim();
        if clause.is_empty() {
            return Vec::new();
        }
        verter_semantic::analysis::type_eval_build::parse_type_parameter_clause(clause)
    }

    fn apply_sfc_script_setup_type_params(
        env: &mut verter_semantic::analysis::type_eval::EvalEnv,
        source: &str,
        cached_parse: Option<&verter_compiler::parser::types::ParsedSfc>,
    ) {
        for param in Self::sfc_script_setup_type_params(source, cached_parse) {
            env.type_bindings.insert(
                param.name.clone(),
                Arc::new(verter_type_expr::TypeExpr::type_parameter(param)),
            );
        }
    }

    pub(crate) fn build_eval_script_source(
        source: &str,
        cached_parse: Option<&verter_compiler::parser::types::ParsedSfc>,
    ) -> String {
        crate::host_resolve::extract_vue_script_content(source, cached_parse)
            .unwrap_or_else(|| source.to_string())
    }

    /// Resolve the authoritative `source_type` for cache-key purposes.
    ///
    /// Prefers the scheduler-stored [`crate::host_executor::HostSourceData::source_type`]
    /// (set once at `execute_source` time). Falls back to a pure recomputation for
    /// canonicals the scheduler has not yet processed — WASM path, first-time routing,
    /// or snapshot construction for files the scheduler does not own.
    pub(super) fn imported_eval_source_type_for(
        &self,
        canonical_id: &str,
        raw_source: &str,
        cached_parse: Option<&verter_compiler::parser::types::ParsedSfc>,
    ) -> oxc_span::SourceType {
        {
            if let Some(st) = self.authoritative_source_type_for(canonical_id) {
                return st;
            }
        }
        crate::parse::imported_eval_source_type(canonical_id, raw_source, cached_parse)
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
        // feeds the route-owned-shallow materialiser (a route-fact
        // producer) and the cold analysis path; a stale pre-edit
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

        if self
            .project_type_store
            .indexed()
            .get_any(canonical_id)
            .is_some()
        {
            return true;
        }

        if is_raw_import_specifier_id(canonical_id) {
            return false;
        }

        self.ws().file_exists(canonical_id)
    }

    pub(super) fn clone_cached_eval_env_arc(
        &self,
        cache_key: &str,
        whole_hash: Hash16,
    ) -> Option<Arc<verter_semantic::analysis::type_eval::EvalEnv>> {
        // The legacy `Arc<EvalEnv>` storage is keyed by the full R21
        // parse-artifact identity. Compose the `FileArtifactKey` for
        // this `(canonical, content_hash)` pair from the canonical's
        // per-project `parse_env_hash` so a parse-env change is a key
        // miss rather than a stale hit.
        let key = self.legacy_eval_env_key(cache_key, whole_hash);
        self.eval_env_cache().legacy_env_for(&key)
    }

    /// Compose the full R21 [`crate::file_artifact_store::FileArtifactKey`]
    /// under which the legacy `Arc<EvalEnv>` for `(canonical,
    /// content_hash)` is cached.
    ///
    /// `EvalEnv` is a pure parse artifact, so its cache identity is
    /// the same `(canonical, content_hash, parse_env_hash,
    /// parser_version)` quadruple every other parse artifact uses.
    /// `parse_env_hash` is the canonical's per-project parse-env
    /// dimension; `parser_version` is the live-path parser version
    /// (the `FileArtifactStore` legacy surface uses the same
    /// constant). Because both the content hash AND the parse-env
    /// hash are part of the key, a stale entry under a different
    /// content or parse-env cannot be hit — the cache is correct by
    /// key identity alone, with no eviction sweep required.
    pub(super) fn legacy_eval_env_key(
        &self,
        canonical: &str,
        content_hash: Hash16,
    ) -> crate::file_artifact_store::FileArtifactKey {
        crate::file_artifact_store::FileArtifactKey {
            canonical: Arc::from(canonical),
            content_hash,
            parse_env_hash: self.host_view_env_hashes_for(canonical).parse_env_hash,
            parser_version: crate::file_artifact_store::LEGACY_PARSER_VERSION,
        }
    }

    /// Tier 1A — produces a fresh `ParsedEvalProgram` per call.
    ///
    /// The previous warm-cache lived in the
    /// `HOST_PARSED_EVAL_PROGRAM_CACHE` thread-local, which is now
    /// retired (§3.2.4). The new typed [`crate::project_type_store::EvalEnvCacheDb`]
    /// stores `Arc<crate::owned_artifacts::OwnedEvalProgram>` once
    /// Tier 1C-α migrates the consumer; in 1A this method falls
    /// through to direct compute.
    ///
    /// `_cache_key` is constructed for trace fidelity with the old
    /// cache-key shape so 1C-α's migration can reuse the
    /// `(canonical_id, whole_hash, source_type)` identity tuple
    /// without a wrapper. The single parse authority (host_executor's
    /// `execute_source`) lowers the OXC arena to
    /// [`OwnedEvalProgram`](crate::owned_artifacts::OwnedEvalProgram)
    /// and drops the arena at the boundary; this method is the
    /// borrowed-form interim path until consumers migrate.
    pub(super) fn cached_parsed_eval_program_entry(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        eval_source: &Arc<str>,
        source_type: oxc_span::SourceType,
    ) -> ParsedEvalProgramCacheEntry {
        let _cache_key = ParsedEvalProgramCacheKey {
            host_instance_id: self.instance_id,
            canonical_id: canonical_id.to_string(),
            source_type,
            whole_hash,
        };
        let parsed = crate::ParsedEvalProgram::parse(Arc::clone(eval_source), source_type);
        let parse_failed = parsed.is_none();
        let program =
            Rc::new(parsed.unwrap_or_else(|| crate::ParsedEvalProgram::empty(source_type)));
        let entry = ParsedEvalProgramCacheEntry {
            whole_hash,
            parse_failed,
            program,
        };
        component_meta_trace_custom!(
            "cached_parsed_eval_program_store",
            format!(
                "owner={} bytes={} whole_hash={whole_hash:?} parse_failed={}",
                canonical_id,
                eval_source.len(),
                entry.parse_failed,
            ),
        );
        entry
    }

    /// Tier 1A — builds a fresh `ParsedTypeResolutionContext` per call.
    ///
    /// The previous warm-cache lived in the
    /// `HOST_PARSED_TYPE_CONTEXT_CACHE` thread-local, which is now
    /// retired (§3.2.4). The new typed [`crate::project_type_store::TypeResolutionContextDb`]
    /// stores `Arc<crate::owned_artifacts::OwnedTypeResolutionContext>`
    /// once Tier 1C-α migrates the consumer; in 1A this method falls
    /// through to direct compute against the borrowed-form
    /// `ParsedTypeResolutionContext`.
    pub(super) fn cached_type_resolution_context_entry(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        eval_source: &Arc<str>,
        source_type: oxc_span::SourceType,
    ) -> Option<Rc<crate::ParsedTypeResolutionContext>> {
        let _cache_key = ParsedEvalProgramCacheKey {
            host_instance_id: self.instance_id,
            canonical_id: canonical_id.to_string(),
            source_type,
            whole_hash,
        };
        let parsed_eval_program = self.cached_parsed_eval_program_entry(
            canonical_id,
            whole_hash,
            eval_source,
            source_type,
        );
        if parsed_eval_program.parse_failed {
            return None;
        }

        let graph = std::sync::Arc::clone(self.project_type_store.semantic_graph());
        // Snapshot the resolved-named-type reset epoch at adapter
        // construction. Every `insert` this adapter performs is fenced
        // against this snapshot: if a `bump_project_generation_and_evict`
        // moves the epoch while this build is in flight, the build's
        // straggler inserts are rejected — see
        // `SemanticGraphStore::insert_resolved_named_type`.
        let named_type_generation = graph.named_type_generation();
        let adapter: std::sync::Arc<
            dyn verter_compiler::utils::oxc::vue::resolve_type::cache_keys::NamedTypeCache
                + Send
                + Sync,
        > = std::sync::Arc::new(HostNamedTypeCacheAdapter {
            graph,
            canonical_id: Arc::<str>::from(canonical_id),
            whole_hash,
            named_type_generation,
        });
        let type_context = Rc::new(crate::ParsedTypeResolutionContext::new(
            Rc::clone(&parsed_eval_program.program),
            |parsed_program| {
                let program = parsed_program.borrow_dependent();
                let mut ctx = verter_compiler::utils::oxc::vue::resolve_type::build_type_context(
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
            "cached_type_resolution_context_store",
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
    /// shallow state and analysis. Base callers (`view = None`) get the
    /// historical content-agnostic `get_any` fast path followed by the
    /// route-owned materialiser fall-through.
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
        // fast path and the route-owned materialiser below key on the
        // normalised analysis canonical (the artifact / type-context
        // cache identity).
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
            if let Some(facts) = identity.lookup_overlay_artifacts(self, view) {
                let inputs = ExternalTypeResolutionInputs {
                    raw_source: Arc::clone(&facts.indexed.raw_source),
                    cached_parse: facts.indexed.cached_parse.clone(),
                    whole_hash: facts.indexed.whole_hash,
                    eval_source: Arc::clone(&facts.indexed.eval_source),
                    analysis: Arc::clone(&facts.indexed.external_type_analysis),
                    analysis_cache_hit: true,
                };
                return Some(inputs);
            }
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
        // artifact for a live scope misses both — the route-owned
        // materialiser (freshness-gated) rebuilds below.
        let cached_facts = self
            .current_content_pinned_indexed(canonical_id)
            .or_else(|| self.artifact_current_indexed(canonical_id));
        if let Some(facts) = cached_facts {
            let inputs = ExternalTypeResolutionInputs {
                raw_source: Arc::clone(&facts.raw_source),
                cached_parse: facts.cached_parse.clone(),
                whole_hash: facts.whole_hash,
                eval_source: Arc::clone(&facts.eval_source),
                analysis: Arc::clone(&facts.external_type_analysis),
                analysis_cache_hit: true,
            };
            return Some(inputs);
        }

        // F6 reader migration. Drive the route-only
        // fall-through path through the shared materialiser so external-type
        // analysis is built exactly once per `(canonical, whole_hash)` and
        // shared with F7's `route_shallow_state` reader. The materialiser's
        // tiered staleness gate (route_owned_entry_is_fresh) is the
        // authoritative freshness check; the legacy
        // `cached_external_type_analysis_entry` mutex cache is gone.
        let entry = self.ensure_route_owned_shallow_entry(canonical_id)?;
        let inputs = ExternalTypeResolutionInputs {
            raw_source: Arc::clone(&entry.raw_source),
            cached_parse: entry.cached_parse.clone(),
            whole_hash: entry.whole_hash,
            eval_source: Arc::clone(&entry.eval_source),
            analysis: Arc::clone(&entry.external_type_analysis),
            // The materialiser publishes once per content generation; warm
            // hits return the same entry through the pre-flight fast path.
            // Treat warm hits (singleflight returned the published entry)
            // as cache hits for telemetry.
            analysis_cache_hit: true,
        };
        Some(inputs)
    }

    /// Tier 1A — no-op after thread-local retirement (§3.2.4).
    ///
    /// The previous implementation drained the
    /// `HOST_PARSED_EVAL_PROGRAM_CACHE` /
    /// `HOST_PARSED_TYPE_CONTEXT_CACHE` thread-locals for this host
    /// instance. Both caches are gone in 1A; this method is preserved
    /// as a stable name for callers (Tier 1C-α reintroduces the
    /// behaviour through the typed `EvalEnvCacheDb` /
    /// `TypeResolutionContextDb` typed-DB clear methods).
    pub(crate) fn clear_thread_local_parsed_eval_program_cache(&self) {
        // Tier 1A — `HOST_PARSED_EVAL_PROGRAM_CACHE` /
        // `HOST_PARSED_TYPE_CONTEXT_CACHE` thread-locals are retired
        // (§3.2.4). Tier 1C-α replaces with:
        //   self.project_type_store.eval_env_cache().clear();
        //   self.project_type_store.type_resolution_context_cache().clear();
        // The `external_type_analysis_cache` host mutex is already
        // folded into `RouteOwnedShallowDb` (unified F6/F7 entry). The
        // discipline is per-canonical tiered staleness gate + atomic
        // `project_type_store.evict_canonical` cascade on file change +
        // bulk `route_owned_shallow.clear_all` on route-resolution
        // mutation. NO cache-wide epoch-bump clear — that was
        // over-clearing and the per-entry `route_owned_entry_is_fresh`
        // gate is precise.
        //
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

    pub(super) fn build_external_type_analysis(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        raw_source: &str,
        cached_parse: Option<&verter_compiler::parser::types::ParsedSfc>,
        eval_source: &Arc<str>,
    ) -> Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource> {
        let parsed_eval_program = self.cached_parsed_eval_program_entry(
            canonical_id,
            whole_hash,
            eval_source,
            self.imported_eval_source_type_for(canonical_id, raw_source, cached_parse),
        );
        if parsed_eval_program.parse_failed {
            return Arc::new(
                verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(
                ),
            );
        }

        let program = parsed_eval_program.program.borrow_dependent();
        Arc::new(
            verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_program(program),
        )
    }

    pub(crate) fn build_eval_env_and_external_type_analysis(
        &self,
        canonical_id: &str,
        whole_hash: Hash16,
        raw_source: &str,
        cached_parse: Option<&verter_compiler::parser::types::ParsedSfc>,
        eval_source: &Arc<str>,
    ) -> (
        Arc<verter_semantic::analysis::type_eval::EvalEnv>,
        Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource>,
    ) {
        let parsed_eval_program = self.cached_parsed_eval_program_entry(
            canonical_id,
            whole_hash,
            eval_source,
            self.imported_eval_source_type_for(canonical_id, raw_source, cached_parse),
        );
        if parsed_eval_program.parse_failed {
            let mut env = verter_semantic::analysis::type_eval_build::parse_and_build_env(
                eval_source.as_ref(),
            );
            Self::apply_sfc_script_setup_type_params(&mut env, raw_source, cached_parse);
            return (
                Arc::new(env),
                Arc::new(
                    verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(
                    ),
                ),
            );
        }

        let program = parsed_eval_program.program.borrow_dependent();
        let mut env = verter_semantic::analysis::type_eval_build::build_eval_env(
            program,
            eval_source.as_ref(),
        );
        Self::apply_sfc_script_setup_type_params(&mut env, raw_source, cached_parse);
        (
            Arc::new(env),
            Arc::new(
                verter_compiler::utils::oxc::vue::resolve_type::analyze_external_type_program(
                    program,
                ),
            ),
        )
    }
}

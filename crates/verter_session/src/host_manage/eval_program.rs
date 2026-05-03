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
        // Post-cut: no ambient request view gates hash acceptance. Live-host
        // probes operate directly on the project-global caches, which are
        // validated by `HostFenceValidator` at publish time.
        true
    }

    /// Build script-setup generic type parameter bindings for a Vue SFC.
    /// Called once during `PreparedDeclBundle` materialization. Returns an
    /// empty map for non-Vue files or Vue files without `<script setup>` generics.
    ///
    /// Per Path C C3, the result type is `FxHashMap<String,
    /// TypeParamBinding>` — script-setup parameters are no longer
    /// wrapped in a `PreparedTypeDecl`. Pre-C3 the wrapper carried a
    /// large `name_resolution` table populated from the SFC's symbols /
    /// imports; that table was unused by the lowering hot path
    /// (constraint / default lowering threads its own `name_resolution`
    /// through `shallow_lower_type_expr`), so dropping the wrapper also
    /// drops dead allocation.
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
                    // Path C C6a item 1: 0-based clause position so
                    // multiple `<script setup generic="T, U">` params
                    // get distinct identity tuples.
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
    ) -> Vec<verter_semantic::analysis::type_expr::TypeParam> {
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
                Arc::new(verter_semantic::analysis::type_expr::TypeExpr::type_parameter(param)),
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

        // Check the project-global IndexedReady cache for cached raw_source.
        if let Some(facts) = self.project_type_store.indexed().get_any(canonical_id) {
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
                    // `IndexedReadyDb::get_any` before reaching here.
                    #[cfg(test)]
                    self.test_audit.record_read(canonical_id);
                    return Some(source);
                }
                if let Some(facts) = self.project_type_store.indexed().get_any(canonical_id) {
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
        self.eval_env_cache
            .lock()
            .get(cache_key)
            .and_then(|(cached_hash, cached_env)| {
                (*cached_hash == whole_hash).then(|| Arc::clone(cached_env))
            })
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

        let adapter: std::sync::Arc<
            dyn verter_compiler::utils::oxc::vue::resolve_type::cache_keys::NamedTypeCache
                + Send
                + Sync,
        > = std::sync::Arc::new(HostNamedTypeCacheAdapter {
            graph: std::sync::Arc::clone(self.project_type_store.semantic_graph()),
            canonical_id: Arc::<str>::from(canonical_id),
            whole_hash,
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
        let normalized_canonical_id = self.normalized_analysis_canonical(canonical_id);
        let canonical_id = normalized_canonical_id.as_ref();

        // The per-request `external_inputs_memo` was.
        // The project-global `IndexedReadyDb` already returns cached state,
        // so the old memo was just a redundant lookup memo layered over a
        // host-owned cache.
        let cached_facts = self.project_type_store.indexed().get_any(canonical_id);
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

    /// Resolve a named type from an imported dependency and project its macro
    /// surfaces, reusing the host-cached parsed program and external type
    /// analysis so this path never parses raw source again.
    ///
    /// a raw-source projection helper allocated a fresh
    /// oxc arena and reparsed the source on every call; that path
    /// is deleted under the graph-only resolver. Enrichment/lookup
    /// paths (for example JSDoc collection for imported props) use
    /// this method so imported-file parses stay single-shot per
    /// content hash.
    pub(crate) fn project_imported_macro_surfaces(
        &self,
        dep_canonical: &str,
        exported_name: &str,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind,
    ) -> Option<crate::resolver_core::surface_projector::ProjectedMacroSurfaces> {
        let inputs = self.external_type_resolution_inputs(dep_canonical)?;
        let source_type = self.imported_eval_source_type_for(
            dep_canonical,
            inputs.raw_source.as_ref(),
            inputs.cached_parse.as_deref(),
        );
        let _cache_key = ParsedEvalProgramCacheKey {
            host_instance_id: self.instance_id,
            canonical_id: dep_canonical.to_string(),
            source_type,
            whole_hash: inputs.whole_hash,
        };
        // Tier 1A: thread-local cache deleted (§3.2.4); compute fresh
        // until 1C-α wires the typed `EvalEnvCacheDb` consumer.
        let parsed =
            crate::ParsedEvalProgram::parse(Arc::clone(&inputs.eval_source), source_type);
        let parse_failed = parsed.is_none();
        let program =
            Rc::new(parsed.unwrap_or_else(|| crate::ParsedEvalProgram::empty(source_type)));
        let entry = ParsedEvalProgramCacheEntry {
            whole_hash: inputs.whole_hash,
            parse_failed,
            program,
        };
        if entry.parse_failed {
            return None;
        }
        let program_ref = entry.program.borrow_dependent();
        let source_bytes = inputs.eval_source.as_bytes();
        let resolved = verter_compiler::utils::oxc::vue::resolve_type::resolve_external_type_in_program_with_analyzed_symbol_companion(
            exported_name,
            program_ref,
            source_bytes,
            inputs.analysis.as_ref(),
            &rustc_hash::FxHashMap::default(),
        )?;
        Some(crate::resolver_core::surface_projector::project_macro_surfaces(
            Some(inputs.eval_source.as_ref()),
            macro_kind,
            &resolved,
        ))
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

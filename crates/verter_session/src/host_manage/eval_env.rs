//! `host_manage::eval_env` — eval-env builders, file-analysis snapshot
//! constructors, and evaluated-type computation.
//!
//! Domain G. Owns the host's `base_eval_env` artifact read, the
//! `FileAnalysisSnapshot` builders for parse / source flows, and the
//! per-owner evaluated-type compute path. Public surface
//! remains rooted at `crate::host_manage::*`; this file contributes a
//! continuation `impl VerterHost { … }` block.

use std::sync::Arc;

use crate::instant::Instant;
use crate::types::*;
use crate::VerterHost;

use super::{
    component_meta_debug, component_meta_debug_enabled, component_meta_trace_custom,
    is_raw_import_specifier_id, log_snapshot_debug, resolve_eval_dependency_canonical_with,
    ComputedEvaluatedTypes, ValueDeclIdentity,
};

impl VerterHost {
    /// The canonical per-file `EvalEnv` for a base (non-overlay) read.
    ///
    /// A DEMAND product for whole-file consumers (fallthrough, runtime
    /// values, value-alias peeling): the artifact's lazy declaration-body
    /// memo materialises the whole-file env once through the retained
    /// scheduler-side parse snapshot and memoizes it (script-setup type
    /// params applied). The per-symbol query path never touches it, and
    /// publishing the artifact never builds it. There is no separate env
    /// cache and no env-only build path.
    pub(crate) fn base_eval_env_arc(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_eval::EvalEnv>> {
        component_meta_trace_custom!(
            "base_eval_env",
            format!("owner={} store_view={}", canonical_id, false),
        );
        let resolved_canonical_id = self
            .resolve_eval_dependency_canonical(canonical_id)
            .unwrap_or_else(|| canonical_id.to_string());
        // The base whole-file eval env is the memo-backed product of the
        // canonical's `IndexedReady`: `ensure_indexed_ready_serve` performs the
        // one cold materialise (or joins the published artifact), and the
        // memo's `whole_env()` lowers the file's declaration set once through
        // the retained eval program — applying SFC `<script setup generic>`
        // params and the Svelte rune ambient env per file. No eager rebuild,
        // no second parse.
        let indexed = self
            .ensure_indexed_ready_serve(resolved_canonical_id.as_str())?
            .indexed;
        component_meta_trace_custom!(
            "base_eval_env_built",
            format!("owner={} whole_hash={:?}", canonical_id, indexed.whole_hash),
        );
        Some(indexed.shallow_state.decl_bodies().whole_env())
    }

    pub(crate) fn local_type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<verter_semantic::analysis::type_eval::DeclarationId> {
        if self
            .routed_shallow_state(canonical_source)
            .is_some_and(|state| state.import_target(resolved_name).is_some())
        {
            return None;
        }

        self.base_eval_env(canonical_source)
            .and_then(|env| env.type_declaration_id(resolved_name))
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn base_eval_env(
        &self,
        canonical_id: &str,
    ) -> Option<verter_semantic::analysis::type_eval::EvalEnv> {
        self.base_eval_env_arc(canonical_id)
            .map(|env| (*env).clone())
    }

    fn peel_value_decl_alias(&self, canonical_id: &str, name: &str) -> ValueDeclIdentity {
        let mut current = ValueDeclIdentity {
            canonical_id: canonical_id.to_string(),
            name: name.to_string(),
        };
        let mut visited = rustc_hash::FxHashSet::default();

        loop {
            if !visited.insert(current.clone()) {
                break;
            }

            let Some(env) = self.base_eval_env_arc(current.canonical_id.as_str()) else {
                break;
            };
            let Some(group) = env.value_symbols.get(current.name.as_str()) else {
                break;
            };
            let decl = group.primary();
            let Some(verter_type_expr::TypeExpr::TypeOf(value_ref)) = decl.type_annotation.as_ref()
            else {
                break;
            };
            let Some(next_name) = value_ref.path.first() else {
                break;
            };
            if value_ref.path.len() != 1 || *next_name == current.name {
                break;
            }
            if !env.value_symbols.contains_key(next_name.as_str()) {
                break;
            }

            current.name = next_name.clone();
        }

        current
    }

    pub(crate) fn resolve_value_export_target(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> Option<ValueDeclIdentity> {
        let export = self.resolve_named_export(dep_canonical_id, imported_name, Some(false))?;
        let source_canonical_id = export
            .source_canonical_id
            .unwrap_or_else(|| dep_canonical_id.to_string());
        Some(self.peel_value_decl_alias(source_canonical_id.as_str(), export.source_name.as_str()))
    }

    pub(crate) fn build_snapshot_from_parse(parse: crate::ParseSnapshot) -> FileAnalysisSnapshot {
        // The snapshot is shared by `Arc`; this builder consumes a freshly
        // parsed `ParseSnapshot` whose snapshot is uniquely held, so
        // `unwrap_or_clone` moves the inner value out without copying (it only
        // deep-copies in the rare case the snapshot is still shared).
        let script_analysis = Arc::unwrap_or_clone(parse.script_analysis);
        FileAnalysisSnapshot {
            imports: script_analysis.imports,
            bindings: script_analysis.bindings,
            module_references: Arc::new(script_analysis.module_references),
            macros: Arc::new(script_analysis.macros),
            macro_type_deps: Arc::new(script_analysis.macro_type_deps),
            script_flags: script_analysis.flags.bits(),
            styles: Arc::new(parse.style_analyses),
            template: None,
            vue_api_calls: Arc::new(script_analysis.vue_api_calls),
            dom_query_calls: Arc::new(script_analysis.dom_query_calls),
            css_var_manipulations: Arc::new(script_analysis.css_var_manipulations),
            script_binding_occurrences: Arc::new(script_analysis.script_binding_occurrences),
            export_signatures: Arc::new(parse.export_signatures),
            options_api: script_analysis.options_api,
            store_usages: Arc::new(script_analysis.store_usages),
            store_definitions: Arc::new(script_analysis.store_definitions),
            is_typescript: script_analysis.is_typescript,
        }
    }

    pub(crate) fn build_snapshot_and_template_inputs_from_source(
        &self,
        canonical: &str,
        source: &Arc<str>,
        store_published: bool,
    ) -> (
        FileAnalysisSnapshot,
        Option<crate::types::VueTemplateInputs>,
    ) {
        component_meta_trace_custom!(
            "build_snapshot_from_source",
            format!("owner={} bytes={}", canonical, source.len()),
        );
        let file_language = self.language_classifier.classify(canonical);
        if file_language.is_vue() {
            component_meta_trace_custom!("parse_vue_snapshot", format!("owner={canonical}"));
            let (parse, parsed) = crate::parse::parse_vue_snapshot(
                canonical,
                source,
                self.config.effective_scope(),
                &self.provenance,
            );
            component_meta_trace_custom!(
                "parse_vue_snapshot_result",
                format!(
                    "owner={} imports={} macros={} export_signatures={}",
                    canonical,
                    parse.script_analysis.imports.len(),
                    parse.script_analysis.macros.len(),
                    parse.export_signatures.len(),
                ),
            );
            let template_inputs = crate::types::VueTemplateInputs {
                source: Arc::clone(source),
                framework_parse: Some(parsed),
                store_published,
                // This builder reads no scheduler node, so it can
                // never attest a node generation; the computed
                // template serves the caller but never persists.
                source_generation: None,
            };
            (
                Self::build_snapshot_from_parse(parse),
                Some(template_inputs),
            )
        } else {
            component_meta_trace_custom!("parse_non_sfc_snapshot", format!("owner={canonical}"));
            let parse = crate::parse::parse_non_sfc_snapshot(
                canonical,
                source,
                &file_language,
                &self.provenance,
            );
            component_meta_trace_custom!(
                "parse_non_sfc_snapshot_result",
                format!(
                    "owner={} imports={} macros={} export_signatures={}",
                    canonical,
                    parse.script_analysis.imports.len(),
                    parse.script_analysis.macros.len(),
                    parse.export_signatures.len(),
                ),
            );
            (Self::build_snapshot_from_parse(parse), None)
        }
    }

    pub(in crate::host_manage) fn finalize_analysis_snapshot(
        &self,
        canonical: &str,
        mut snapshot: FileAnalysisSnapshot,
        needs_template_analysis: bool,
        template_inputs: Option<crate::types::VueTemplateInputs>,
        analysis_started: Option<Instant>,
    ) -> FileAnalysisSnapshot {
        self.resolve_snapshot_imports(canonical, &mut snapshot);
        self.enrich_destructured_bindings(&mut snapshot);
        if needs_template_analysis {
            // No coherent inputs (torn generation join, non-SFC) →
            // the template stays absent for this caller — fail closed.
            if let Some(inputs) = template_inputs {
                self.compute_template_analysis_if_missing(canonical, &mut snapshot, inputs);
            }
        }
        if let Some(started) = analysis_started {
            log_snapshot_debug("get_analysis", canonical, started, &snapshot);
        }
        snapshot
    }

    fn is_expanded_types_empty(
        result: &verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    ) -> bool {
        result.is_empty()
    }

    pub(crate) fn current_eval_state(
        &self,
        canonical_id: &str,
    ) -> Option<(
        Arc<str>,
        Option<Arc<verter_language::FrameworkParseArtifact>>,
        Hash16,
    )> {
        if canonical_id.is_empty() {
            return None;
        }

        let normalized_canonical_id = self.normalized_analysis_canonical(canonical_id);
        // The uncached path already hits the project-global `FileArtifactStore`
        // for repeated probes, so no per-request memo layer is needed.
        self.current_eval_state_uncached(normalized_canonical_id.as_ref())
    }

    fn current_eval_state_uncached(
        &self,
        canonical_id: &str,
    ) -> Option<(
        Arc<str>,
        Option<Arc<verter_language::FrameworkParseArtifact>>,
        Hash16,
    )> {
        component_meta_trace_custom!("current_eval_state", format!("owner={}", canonical_id),);

        // FileArtifactStore fast path — **current-content-pinned** (no
        // `get_any`). `current_eval_state` returns the canonical's source
        // for the cold type-evaluation recompute; a stale artifact would
        // feed pre-edit source into the evaluation. With the own-canonical
        // drain retired a stale pre-edit `IndexedReady` can linger past a
        // same-canonical edit, so the artifact read is pinned to the
        // canonical's authoritative current content hash:
        // `current_content_pinned_indexed` serves only a content-current
        // artifact for a scheduler-tracked canonical, and
        // `artifact_current_indexed` answers for a genuinely artifact-only
        // canonical. A stale candidate for a live scope misses both — the
        // scheduler source path below is the authoritative current content.
        let cached_facts = self
            .current_content_pinned_indexed(canonical_id)
            .or_else(|| self.artifact_current_indexed(canonical_id));
        if let Some(facts) = cached_facts {
            return Some((
                Arc::clone(&facts.raw_source),
                facts.framework_parse.clone(),
                facts.whole_hash,
            ));
        }

        // Scheduler source path for files loaded via `ensure_loaded` but not
        // yet materialized into `FileArtifactStore`. The scheduler is the sole
        // source authority; on miss, call `ensure_loaded` once.
        if let Some(state) = self.effective_file_state(canonical_id, None) {
            return Some((state.source, state.framework_parse, state.whole_hash));
        }
        if !canonical_id.is_empty()
            && !is_raw_import_specifier_id(canonical_id)
            && self.ensure_loaded(canonical_id)
        {
            if let Some(state) = self.effective_file_state(canonical_id, None) {
                return Some((state.source, state.framework_parse, state.whole_hash));
            }
        }
        None
    }

    pub(crate) fn resolve_eval_dependency_canonical(&self, dep_canonical: &str) -> Option<String> {
        resolve_eval_dependency_canonical_with(dep_canonical, |candidate| {
            self.analysis_source_exists(candidate)
        })
    }

    pub(crate) fn normalized_analysis_canonical<'a>(
        &self,
        canonical_id: &'a str,
    ) -> std::borrow::Cow<'a, str> {
        if canonical_id.is_empty() || is_raw_import_specifier_id(canonical_id) {
            return std::borrow::Cow::Borrowed(canonical_id);
        }

        self.resolve_eval_dependency_canonical(canonical_id)
            .map(std::borrow::Cow::Owned)
            .unwrap_or_else(|| std::borrow::Cow::Borrowed(canonical_id))
    }

    pub(crate) fn cache_dependency_candidates_from_snapshot(
        &self,
        owner_canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
    ) -> std::collections::BTreeSet<String> {
        let mut candidates = std::collections::BTreeSet::new();

        if let Some(serve) = self.ensure_indexed_ready_serve(owner_canonical_id) {
            let facts = &serve.indexed;
            // Baked-edge currency gate. The artifact's cross-file edge
            // `canonical_id`s were resolved at materialise time; a
            // FENCED (ReturnOnly) serve carries edges baked against the
            // pre-mutation file set, so trusting them would track the
            // superseded dependency targets. Consume baked edges only
            // from a store-published serve (published artifacts are
            // store-current at publish; a stale one re-materialises
            // through the serve's own currency gates); a fenced serve
            // re-resolves every raw source specifier through the live
            // resolver instead — the same discipline as the
            // augmentation probe's re-export walk.
            let baked_edges_current = serve.store_published;
            for target in facts.shallow_state.import_targets.values() {
                if baked_edges_current && !target.canonical_id.is_empty() {
                    candidates.insert(target.canonical_id.clone());
                    continue;
                }
                if let Some(resolved) =
                    self.resolve_route_type_edge(owner_canonical_id, &target.source_specifier)
                {
                    candidates.insert(resolved);
                }
            }

            for export in facts.shallow_state.exports.values() {
                if let crate::resolver_core::ExportTarget::Reexport {
                    canonical_id,
                    source_specifier,
                    ..
                } = export
                {
                    if baked_edges_current && !canonical_id.is_empty() {
                        candidates.insert(canonical_id.clone());
                    } else if let Some(resolved) =
                        self.resolve_route_type_edge(owner_canonical_id, source_specifier)
                    {
                        candidates.insert(resolved);
                    }
                }
            }

            for wildcard in &facts.shallow_state.wildcard_reexports {
                if baked_edges_current && !wildcard.canonical_id.is_empty() {
                    candidates.insert(wildcard.canonical_id.clone());
                } else if let Some(resolved) =
                    self.resolve_route_type_edge(owner_canonical_id, &wildcard.source_specifier)
                {
                    candidates.insert(resolved);
                }
            }
        }

        for import in &snapshot.imports {
            if let Some(resolved) = import.resolved_canonical_id.as_deref() {
                candidates.insert(resolved.to_string());
                continue;
            }

            if let Some(target) = self.resolve_route_type_edge(owner_canonical_id, &import.source) {
                candidates.insert(target);
                continue;
            }

            if import.source.starts_with('.') {
                candidates
                    .extend(self.expand_relative_candidates(owner_canonical_id, &import.source));
            }
        }

        candidates
    }

    /// View-aware macro-argument-type expansion entry point.
    /// Routes the resolver-tier reads (query engine, dispatch
    /// lowering, prepared-decl bundle) through the supplied
    /// `ResolverContext` so overlay-bearing sessions observe overlay
    /// candidates for cross-file macro-argument-type expansion.
    pub(crate) fn compute_evaluated_types_with_tracking_from_owner_context_with_ctx(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        owner_eval_source: Option<&str>,
        purpose: crate::resolver_core::ComponentMetaResolutionPurpose,
    ) -> Option<ComputedEvaluatedTypes> {
        let eval_source = owner_eval_source.map(str::to_string).or_else(|| {
            self.current_eval_state(canonical)
                .map(|(source, framework_parse, _)| {
                    Self::build_eval_script_source(&source, framework_parse.as_deref())
                })
        })?;
        self.compute_evaluated_types_from_owner_context_with_ctx(
            ctx,
            canonical,
            snapshot,
            &eval_source,
            purpose,
        )
    }

    fn component_meta_binding_type_entries(
        &self,
        canonical: &str,
        requested_binding_names: &rustc_hash::FxHashSet<String>,
    ) -> Vec<(String, verter_type_expr::TypeExpr)> {
        if requested_binding_names.is_empty() {
            return Vec::new();
        }

        let _ = self.shallow_file_state(canonical);

        requested_binding_names
            .iter()
            .filter_map(|name| {
                self.prepared_value_decl(canonical, name)
                    .and_then(|decl| decl.type_annotation.clone().map(|ty| (name.clone(), ty)))
            })
            .collect()
    }

    /// Macro-argument-type expander entry point. The expander uses
    /// `ctx` for the query-engine and dispatch construction so the
    /// cross-file type lookups observe overlay candidates when the
    /// session view carries them.
    pub(crate) fn compute_evaluated_types_from_owner_context_with_ctx(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        eval_source: &str,
        purpose: crate::resolver_core::ComponentMetaResolutionPurpose,
    ) -> Option<ComputedEvaluatedTypes> {
        {
            component_meta_trace_custom!(
                "compute_evaluated_types_seed_owner_cache",
                format!("owner={} store_view={}", canonical, false),
            );
            let _ = ctx.ensure_indexed_ready_serve(canonical);
        }
        let requested_binding_names =
            if purpose == crate::resolver_core::ComponentMetaResolutionPurpose::Full {
                crate::resolver_core::collect_requested_binding_names(snapshot.macros.as_ref())
            } else {
                rustc_hash::FxHashSet::default()
            };
        let binding_entries = {
            component_meta_trace_custom!(
                "compute_evaluated_types_binding_entries",
                format!(
                    "owner={} requested_bindings={} store_view={}",
                    canonical,
                    requested_binding_names.len(),
                    false,
                ),
            );
            self.component_meta_binding_type_entries(canonical, &requested_binding_names)
        };
        // the retired `external_engine` branch is
        // gone; there is only one `expand_macro_types` entry point left.
        // Step 9.1 / D32: surface-id sidecar capture buffers. Populated
        // when audit is on; the dispatch round-trip inside the closure
        // gives a SemanticNodeId for the produced expanded type, which
        // is stored in the buffer keyed by FieldKind. After the closure
        // returns, the buffers feed `SurfaceNodeIdentities` so Step
        // 9.2's scoped origin export reverse-walks only the reachable
        // subgraph rooted at these ids.
        let audit_enabled = self.config.audit_enabled;
        let prop_node_ids: std::cell::RefCell<Vec<Option<crate::semantic_query::SemanticNodeId>>> =
            std::cell::RefCell::new(Vec::new());
        let emit_node_ids: std::cell::RefCell<Vec<Option<crate::semantic_query::SemanticNodeId>>> =
            std::cell::RefCell::new(Vec::new());
        let slot_binding_node_ids: std::cell::RefCell<
            Vec<Option<crate::semantic_query::SemanticNodeId>>,
        > = std::cell::RefCell::new(Vec::new());
        let binding_node_ids: std::cell::RefCell<
            Vec<Option<crate::semantic_query::SemanticNodeId>>,
        > = std::cell::RefCell::new(Vec::new());
        let result = {
            component_meta_trace_custom!(
                "compute_evaluated_types_expand_macros",
                format!(
                    "owner={} macros={} bindings={} store_view={}",
                    canonical,
                    snapshot.macros.len(),
                    binding_entries.len(),
                    false,
                ),
            );
            let mut engine = crate::resolver_core::ComponentMetaQueryEngine::new(ctx);
            verter_semantic::analysis::type_eval_build::expand_macro_types_impl_with_expander(
                snapshot.macros.as_ref(),
                Some(eval_source),
                binding_entries.as_slice(),
                None,
                match purpose {
                    crate::resolver_core::ComponentMetaResolutionPurpose::Full => {
                        verter_semantic::analysis::type_eval_build::MacroExpansionScope::Full
                    }
                    crate::resolver_core::ComponentMetaResolutionPurpose::Fallthrough => {
                        verter_semantic::analysis::type_eval_build::MacroExpansionScope::Fallthrough
                    }
                },
                |ctx, parsed| {
                    use crate::resolver_core::component_meta_query_engine::{
                        FastShallowFieldExpr, FastShallowFieldExprExactness,
                    };
                    use verter_semantic::analysis::type_expand::{
                        ExpandedNormalizedExpr, ExpansionResult,
                    };

                    fn fast_to_expansion(
                        fast: FastShallowFieldExpr,
                    ) -> ExpansionResult<ExpandedNormalizedExpr> {
                        match fast.exactness {
                            FastShallowFieldExprExactness::Symbolic => {
                                ExpansionResult::exact_symbolic(ExpandedNormalizedExpr {
                                    expr: fast.expr,
                                })
                            }
                            FastShallowFieldExprExactness::Concrete => {
                                ExpansionResult::exact_concrete(ExpandedNormalizedExpr {
                                    expr: fast.expr,
                                })
                            }
                        }
                    }

                    // Capture
                    // the production-path SemanticNodeId for this
                    // field. Each branch that lowers via dispatch
                    // sets this variable to the produced terminal
                    // node id. Branches that do not dispatch (fast
                    // path, shallow-preserve, defineModel-without-
                    // type-arg, etc.) leave it as `None`. The
                    // captured id replaces the retired audit-only
                    // re-lowering at the closure's tail (no
                    // duplicate dispatch round-trip — audit is now a
                    // pure reader of production work).
                    let mut produced_node_id: Option<crate::semantic_query::SemanticNodeId> = None;

                    let expansion = if let Some(fast) =
                        engine.try_fast_shallow_field_expr(canonical, parsed)
                    {
                        fast_to_expansion(fast)
                    } else if engine.should_preserve_shallow_field_expr(canonical, parsed) {
                        ExpansionResult::exact_symbolic(ExpandedNormalizedExpr {
                            expr: parsed.clone(),
                        })
                    } else {
                        // Dispatch-projection branch. Lower the macro's
                        // parent shell once via dispatch (using the
                        // cache-owned parsed_type_argument), then
                        // project the closure's output_path off the
                        // lowered base. On any failure (no
                        // parsed_type_argument, empty output_path,
                        // lowering miss, projection unknown, raise
                        // failed) emit a structured trace event and
                        // fall back to symbolic preservation.
                        use crate::project_semantic_dispatch::ProjectSemanticDispatch;
                        use crate::semantic_query::{
                            PathSegment as SemanticPathSegment, ProjectionMode, QueryResult,
                            SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
                        };
                        use verter_semantic::analysis::type_eval_build::PathSegment as MacroPathSegment;

                        let preserve_parsed_symbolically = || {
                            ExpansionResult::exact_symbolic(ExpandedNormalizedExpr {
                                expr: parsed.clone(),
                            })
                        };

                        let macro_type_arg = snapshot
                            .macros
                            .get(ctx.macro_index)
                            .and_then(|m| m.parsed_type_argument.clone());
                        let macro_kind = snapshot.macros.get(ctx.macro_index).map(|m| m.kind);

                        // Issue #3 — field-level fast path. When the
                        // macro's parent shell is a named generic /
                        // non-generic carrier and the field's parsed
                        // expression does NOT reference any of the
                        // parent's type parameters (modulo shadowing in
                        // mapped types and function-type parameter
                        // lists), the closure short-circuits to
                        // `ExpansionResult::exact_concrete(parsed)`. The
                        // parsed field expression is the answer; no
                        // parent projection runs. Skipping the parent
                        // lower means we do NOT dispatch
                        // `Instantiate { base = <heritage>, .. }` for
                        // any of the shell's `extends`-chain types,
                        // which is the source of the cold-time blow-up
                        // when the heritage points into a third-party
                        // package (the `defineProps<ChatMessageProps>()
                        // extends UIMessage from 'ai'` regression).
                        //
                        // The fast path is parse-local and does NOT
                        // populate any host-cached entry — the parsed
                        // field expression is canonical to the file's
                        // most recent parse. The owner SFC's parse is
                        // already cached by the scheduler; recomputing
                        // this predicate per `getComponentMeta` call is
                        // cheaper than an extra DB lookup. The
                        // `defineModel<T>()` arm BELOW retains its
                        // existing direct-lower path; the fast path
                        // applies only when the slow `output_path`
                        // projection branch would otherwise run.
                        //
                        // The early-exit assigns `expansion` rather than
                        // returning from the closure: the audit-gated
                        // push at the bottom of the closure must still
                        // run to keep per-FieldKind cardinality in
                        // sync with the macro emitter's field count.
                        let fast_path_applied = if !ctx.output_path.is_empty()
                            && !matches!(
                                macro_kind,
                                Some(verter_semantic::analysis::AnalyzedMacroKind::DefineModel)
                            ) {
                            if let Some(macro_type_arg) = macro_type_arg.as_ref() {
                                !engine.field_needs_parent_projection(
                                    canonical,
                                    parsed,
                                    macro_type_arg.as_ref(),
                                )
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        if fast_path_applied {
                            ExpansionResult::exact_concrete(ExpandedNormalizedExpr {
                                expr: parsed.clone(),
                            })
                        } else {
                            // `defineModel<T>()` prop /
                            // model lowering. The `expand_macro_types_impl_with_expander`
                            // emits the model's prop field with
                            // `output_path = [Member(<model_name>)]`, but the
                            // macro's `parsed_type_argument` is `T` itself —
                            // not a parent shell whose member is the type.
                            // Dispatching `ProjectPath { base, [Member(model)],
                            // Expanded }` always misses because `T` is
                            // typically a `Primitive` / `Ref` / `Union` (no
                            // member to navigate). The closure used to fall
                            // through to symbolic preservation, but symbolic
                            // preservation is gated on
                            // `should_preserve_shallow_field_expr`, which is
                            // false for primitive-leaf types — leaving the
                            // dispatch arm to produce `Unknown { raw:
                            // "semanticMiss" }`.
                            //
                            // routes `DefineModel` prop / model
                            // fields through a direct lower+raise of
                            // `macro_type_arg` (the type IS the field's
                            // type), bypassing the path projection. Mirrors
                            // the empty-output_path arm semantically. Closes
                            // `fixture_models` deferred fixture (re-homed
                            // from 5k per §5.13 r15 table).
                            if matches!(
                                macro_kind,
                                Some(verter_semantic::analysis::AnalyzedMacroKind::DefineModel)
                            ) {
                                if macro_type_arg.is_some() {
                                    let dispatch = ProjectSemanticDispatch::new(engine.ctx());
                                    // Read the macro arg's mode-neutral mirror
                                    // handle (the ONE producer) and resolve it
                                    // through the shared dispatch at this
                                    // demand's mode (`Expanded` — the model
                                    // value type IS the field's type). A
                                    // different DEMAND on the same handle, not
                                    // a second lowering of the macro arg.
                                    if let Some(base_id) =
                                        crate::macro_hot_mirror::macro_type_arg_hot_ref(
                                            engine.ctx(),
                                            canonical,
                                            ctx.macro_index,
                                        )
                                        .map(|handle| {
                                            dispatch.resolve_hot_handle_at_mode(
                                                handle,
                                                ProjectionMode::Expanded,
                                            )
                                        })
                                    {
                                        // Capture
                                        // production node id for the audit
                                        // record (replaces the retired
                                        // audit-only re-lowering sidecar).
                                        produced_node_id = Some(base_id);
                                        if let Some(raised) =
                                            dispatch.raise_node_to_type_expr(base_id)
                                        {
                                            ExpansionResult::exact_concrete(
                                                ExpandedNormalizedExpr { expr: raised },
                                            )
                                        } else {
                                            // Lowering succeeded but raise
                                            // failed — fall back to `parsed`
                                            // (already the model's type).
                                            ExpansionResult::exact_concrete(
                                                ExpandedNormalizedExpr {
                                                    expr: parsed.clone(),
                                                },
                                            )
                                        }
                                    } else {
                                        // Lowering miss — fall back to `parsed`.
                                        ExpansionResult::exact_concrete(ExpandedNormalizedExpr {
                                            expr: parsed.clone(),
                                        })
                                    }
                                } else {
                                    // No `parsed_type_argument` — fall back to
                                    // `parsed` directly (the macro's
                                    // `prop_fields[0].type_annotation` per
                                    // `extract_define_model_type` IS the
                                    // macro's first type argument).
                                    ExpansionResult::exact_concrete(ExpandedNormalizedExpr {
                                        expr: parsed.clone(),
                                    })
                                }
                            } else {
                                match (ctx.output_path.is_empty(), macro_type_arg) {
                                    (true, _) | (_, None) => {
                                        component_meta_trace_custom!(
                                    "macro_projection_failover",
                                    format!(
                                        "macro_index={} field_kind={:?} reason=no_parsed_type_argument",
                                        ctx.macro_index, ctx.kind,
                                    ),
                                );
                                        preserve_parsed_symbolically()
                                    }
                                    (false, Some(macro_type_arg)) => {
                                        let dispatch = ProjectSemanticDispatch::new(engine.ctx());
                                        // Issue #3 — selective carrier-mode demotion.
                                        // Path-precise contract (`/type-resolution`):
                                        // when the carrier is a named `Ref` (e.g.
                                        // `defineProps<UIMessage>()` or
                                        // `defineProps<ChatMessageProps>()`), the
                                        // shell is an intermediate hop on the way
                                        // to the field — the field itself is the
                                        // terminal hop. Lower the carrier in
                                        // `Navigate` mode so the shell expands
                                        // only as much as navigation needs; the
                                        // terminal `ProjectPath` query below runs
                                        // in `Expanded` and owns the full
                                        // expansion of the requested member.
                                        //
                                        // For compound carriers (anonymous object
                                        // literals, conditionals, mapped types,
                                        // intersections, etc.) the field's parsed
                                        // body may reference parent-generic params
                                        // and depend on slow-path `Expanded`
                                        // resolution to instantiate the body
                                        // correctly — keep `Expanded` for those.
                                        let carrier_lower_mode = {
                                            use verter_type_expr::TypeExpr;
                                            // Mirror the shallow_preserve helper's
                                            // "is this a Ref carrier" check.
                                            let stripped = {
                                                let mut e = macro_type_arg.as_ref();
                                                while let TypeExpr::Parenthesized(inner) = e {
                                                    e = inner.as_ref();
                                                }
                                                e
                                            };
                                            if matches!(stripped, TypeExpr::Ref { .. }) {
                                                ProjectionMode::Navigate
                                            } else {
                                                ProjectionMode::Expanded
                                            }
                                        };
                                        // Read the macro arg's mode-neutral
                                        // mirror handle (the ONE producer) and
                                        // resolve it through the shared dispatch
                                        // at `carrier_lower_mode` (a different
                                        // DEMAND on the same handle, never a
                                        // second lowering of the macro arg).
                                        let lowered =
                                            crate::macro_hot_mirror::macro_type_arg_hot_ref(
                                                engine.ctx(),
                                                canonical,
                                                ctx.macro_index,
                                            )
                                            .map(
                                                |handle| {
                                                    dispatch.resolve_hot_handle_at_mode(
                                                        handle,
                                                        carrier_lower_mode,
                                                    )
                                                },
                                            );
                                        match lowered {
                                            None => {
                                                component_meta_trace_custom!(
                                        "macro_projection_failover",
                                        format!(
                                            "macro_index={} field_kind={:?} reason=opaque_scope_or_uninterpretable",
                                            ctx.macro_index, ctx.kind,
                                        ),
                                    );
                                                preserve_parsed_symbolically()
                                            }
                                            Some(base_id) => {
                                                // slot-binding-parameter
                                                // type lowering migrates from the engine's
                                                // analysis path to dispatch via the
                                                // `ResolveMacroPayload` variant +
                                                // `MaterializeSurface { Slots }` codepath.
                                                //
                                                // The closure dispatched
                                                // `ProjectPath { base, [Member(slot),
                                                // Member(binding)], Expanded }` directly,
                                                // but the walker emits `Opaque(Miss)` when
                                                // it reaches the slot's `Function` value
                                                // with `Member(binding)` remaining (per
                                                // `walk.rs` Function arm at the catch-all
                                                // `opaque_miss` fall-through). The slot
                                                // value's bindings live inside the
                                                // function's first-parameter Object, not
                                                // as a direct member of the Function.
                                                //
                                                // routes slot-binding lowering
                                                // through the new helper
                                                // `project_slot_binding_member` which
                                                // composes existing variants to descend
                                                // through `Function` -> `params[0].ty`
                                                // -> `Member(binding)`. This closes the
                                                // `slot_shapes` seed and the
                                                // `fixture_slots_typed` deferred fixture.
                                                if matches!(
                                            ctx.kind,
                                            verter_semantic::analysis::type_eval_build::FieldKind::SlotBinding
                                        ) {
                                            // SlotBinding output_path always has
                                            // exactly two segments per
                                            // `type_eval_build.rs` SlotBinding
                                            // emission: [Member(slot),
                                            // Member(binding)]. Anything else is
                                            // a closure-emission contract
                                            // violation; fall back to symbolic.
                                            let mut iter = ctx.output_path.iter();
                                            match (iter.next(), iter.next(), iter.next()) {
                                                (
                                                    Some(MacroPathSegment::Member(slot)),
                                                    Some(MacroPathSegment::Member(binding)),
                                                    None,
                                                ) => {
                                                    //
                                                    // terminal-id variant
                                                    // exposes the production
                                                    // SemanticNodeId so the
                                                    // audit record captures
                                                    // identity from the
                                                    // dispatch path (no
                                                    // audit-only re-lower).
                                                    let slot_binding = dispatch
                                                        .project_slot_binding_member_with_terminal_id(
                                                            base_id,
                                                            slot.as_ref(),
                                                            binding.as_ref(),
                                                            ProjectionMode::Expanded,
                                                        );
                                                    match slot_binding.value {
                                                        QueryResult::Value((
                                                            terminal_id,
                                                            raised,
                                                        )) => {
                                                            produced_node_id = Some(terminal_id);
                                                            ExpansionResult::exact_concrete(
                                                                ExpandedNormalizedExpr {
                                                                    expr: raised,
                                                                },
                                                            )
                                                        }
                                                        _ => {
                                                            component_meta_trace_custom!(
                                                                "macro_projection_failover",
                                                                format!(
                                                                    "macro_index={} field_kind={:?} reason=slot_binding_projection_miss",
                                                                    ctx.macro_index, ctx.kind,
                                                                ),
                                                            );
                                                            preserve_parsed_symbolically()
                                                        }
                                                    }
                                                }
                                                _ => {
                                                    component_meta_trace_custom!(
                                                        "macro_projection_failover",
                                                        format!(
                                                            "macro_index={} field_kind={:?} reason=slot_binding_unexpected_path",
                                                            ctx.macro_index, ctx.kind,
                                                        ),
                                                    );
                                                    preserve_parsed_symbolically()
                                                }
                                            }
                                        } else {
                                        let dispatch_path: std::sync::Arc<[SemanticPathSegment]> =
                                            std::sync::Arc::from(
                                                ctx.output_path
                                                    .iter()
                                                    .map(|seg| match seg {
                                                        MacroPathSegment::Member(name) => {
                                                            SemanticPathSegment::Member(
                                                                std::sync::Arc::clone(name),
                                                            )
                                                        }
                                                    })
                                                    .collect::<Vec<_>>(),
                                            );
                                        let projected =
                                            dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
                                                base: base_id,
                                                path: dispatch_path,
                                                context: crate::semantic_query::ProjectionReductionContext::published(ProjectionMode::Expanded),
                                            });
                                        match projected {
                                            QueryResult::Value(SemanticQueryOutput { value: node_id, .. }) => {
                                                //
                                                // capture production node
                                                // id for the audit record
                                                // before raise.
                                                produced_node_id = Some(node_id);
                                                match dispatch.raise_node_to_type_expr(node_id) {
                                                    Some(raised) => {
                                                        ExpansionResult::exact_concrete(
                                                            ExpandedNormalizedExpr { expr: raised },
                                                        )
                                                    }
                                                    None => {
                                                        component_meta_trace_custom!(
                                                        "macro_projection_failover",
                                                        format!(
                                                            "macro_index={} field_kind={:?} reason=raise_failed",
                                                            ctx.macro_index, ctx.kind,
                                                        ),
                                                    );
                                                        preserve_parsed_symbolically()
                                                    }
                                                }
                                            }
                                            _ => {
                                                component_meta_trace_custom!(
                                                "macro_projection_failover",
                                                format!(
                                                    "macro_index={} field_kind={:?} reason=projection_unknown",
                                                    ctx.macro_index, ctx.kind,
                                                ),
                                            );
                                                preserve_parsed_symbolically()
                                            }
                                        }
                                        }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    };

                    // The
                    // audit-gated re-lowering sidecar is RETIRED.
                    // `produced_node_id` was captured directly off
                    // each production dispatch branch above (or left
                    // as `None` for fast-path / symbolic / failed-
                    // raise branches that legitimately have no
                    // semantic node to publish). The buffer push is
                    // unconditional so audit-on/off perform IDENTICAL
                    // semantic work — the only audit-side cost is a
                    // `Vec::push(Option<SemanticNodeId>)` per field,
                    // which is microseconds. The
                    // `SurfaceNodeIdentities` assembly below remains
                    // audit-gated (it materialises the per-FieldKind
                    // vectors into the audit record only when audit
                    // is on).
                    let target = match ctx.kind {
                        verter_semantic::analysis::type_eval_build::FieldKind::Prop => {
                            &prop_node_ids
                        }
                        verter_semantic::analysis::type_eval_build::FieldKind::Emit => {
                            &emit_node_ids
                        }
                        verter_semantic::analysis::type_eval_build::FieldKind::SlotBinding => {
                            &slot_binding_node_ids
                        }
                        verter_semantic::analysis::type_eval_build::FieldKind::Binding => {
                            &binding_node_ids
                        }
                    };
                    target.borrow_mut().push(produced_node_id);

                    expansion
                },
            )
        };
        // Dependency tracking comes from the frontier/shallow-file-state path.
        let discovered_dependencies = std::collections::BTreeSet::<String>::new();
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "compute_evaluated_types owner={} props={} define_props={} emits={} slot_bindings={} bindings={}",
                canonical,
                result.props.len(),
                result.define_props.len(),
                result.emits.len(),
                result.slot_bindings.len(),
                result.bindings.len(),
            ));
        }
        // Step 9.1: assemble SurfaceNodeIdentities from the audit-gated
        // capture buffers. Length-equality with the corresponding output
        // vectors is guaranteed by the closure being called once per
        // FieldKind-tagged field in the same order
        // expand_macro_types_impl_with_expander pushes into props/emits/
        // slot_bindings/bindings.
        let surface_identities =
            if audit_enabled {
                let prop_ids = prop_node_ids.into_inner();
                let emit_ids = emit_node_ids.into_inner();
                let slot_binding_ids = slot_binding_node_ids.into_inner();
                let binding_ids = binding_node_ids.into_inner();
                // Sanity invariant — debug panic in tests, fall back to None
                // in release if the closure-call cardinality somehow differs.
                let lengths_match = prop_ids.len() == result.props.len()
                    && emit_ids.len() == result.emits.len()
                    && slot_binding_ids.len() == result.slot_bindings.len()
                    && binding_ids.len() == result.bindings.len();
                if lengths_match {
                    Some(crate::meta_resolve::SurfaceNodeIdentities {
                        prop_node_ids: prop_ids,
                        emit_node_ids: emit_ids,
                        slot_binding_node_ids: slot_binding_ids,
                        binding_node_ids: binding_ids,
                        registry_node_ids: Vec::new(),
                    })
                } else {
                    debug_assert!(
                    lengths_match,
                    "surface_identities length mismatch — closure-call cardinality drifted from \
                     ExpandedComponentTypes vector lengths. props {}/{}, emits {}/{}, \
                     slot_bindings {}/{}, bindings {}/{}.",
                    prop_ids.len(), result.props.len(),
                    emit_ids.len(), result.emits.len(),
                    slot_binding_ids.len(), result.slot_bindings.len(),
                    binding_ids.len(), result.bindings.len(),
                );
                    None
                }
            } else {
                None
            };
        Some(ComputedEvaluatedTypes {
            evaluated_types: (!Self::is_expanded_types_empty(&result)).then_some(result),
            discovered_dependencies,
            surface_identities,
        })
    }
}

#[cfg(test)]
#[path = "eval_env_tests.rs"]
mod eval_env_tests;

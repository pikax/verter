//! `host_manage::fallthrough` — fallthrough resolution pipeline + runtime
//! node ↔ resolution conversions + cached fallthrough mirror.
//!
//! Domain I. Owns the
//! `resolve_fallthrough_surface` family of methods, the runtime
//! `FallthroughResolverHost`/`FallthroughComputeHost` adapters' compute
//! callbacks, and the per-canonical fallthrough cache mirror used by
//! `compile_cache`. Public surface remains rooted at
//! `crate::host_manage::*`; this file contributes a continuation
//! `impl VerterHost { … }` block.

use std::sync::Arc;

use crate::instant::Instant;

use crate::resolver_core::{
    collect_dynamic_root_candidates_from_type,
    component_meta_resolved_macros as resolver_component_meta_resolved_macros,
    component_meta_type_registry as resolver_component_meta_type_registry, fallthrough_cache_key,
    materialize_imported_runtime_values_into_env, push_partial_reason,
    resolve_fallthrough_surface as resolver_resolve_fallthrough_surface, DynamicRootCandidate,
    RequestSource, ResolvedConsumedBindings, SingleflightRole,
};
use crate::types::*;
use crate::VerterHost;

use super::component_meta_extract::{
    collect_required_root_fallthrough_runtime_value_names,
    collect_required_template_runtime_value_names,
};
use super::{
    component_meta_debug, component_meta_debug_enabled, component_meta_trace_custom,
    HostFallthroughResolver, HostRuntimeValueResolver, STORE_VIEW_STABILITY_MAX_ATTEMPTS,
};

/// Internal carrier bundling a fallthrough cold compute's RESOLUTION with the
/// COMPUTE completeness it produced.
///
/// The completeness travels WITH the resolution it describes — captured
/// per-call under a fresh [`crate::request_context::ColdComputeCompletenessScope`]
/// — rather than via a caller scope that could outlive a discarded
/// completion-fence retry and taint a later complete attempt. The direct
/// extract paths consume this; the singleflight path carries the same
/// completeness out via `RequestRunResult.completeness` (the executor scopes
/// per attempt).
pub(crate) struct FallthroughComputeOutcome {
    /// The resolved fallthrough surface (`None` when the owner has no
    /// resolvable fallthrough state).
    pub(crate) resolution: Option<crate::types::FallthroughResolution>,
    /// The COMPUTE completeness of this resolution — `Partial` when a budget
    /// trip / fatal read / same-path truncation folded into the per-call
    /// scope. The publish gate refuses to warm a `Partial`.
    pub(crate) completeness: crate::semantic_query::ResultCompleteness,
}

impl VerterHost {
    pub fn resolve_fallthrough_surface(
        &self,
        canonical_id: &str,
    ) -> Option<crate::types::FallthroughResolution> {
        // Direct-entry request context: when this public entry is reached
        // outside an audited component-meta request (compile_cache and other
        // direct callers), no `RequestContext` is installed, so the projection
        // budget AND the partial-completeness gate would be inert on this path.
        // Install one (mirroring the component-meta entry, threading
        // `config.projection_op_budget`) ONLY when none is active, so a call
        // nested inside `get_component_meta` inherits the existing context
        // unchanged rather than clobbering it. The recursion runs through
        // `_internal_with_overrides`, so installing at this top-level public fn
        // means every child inherits the context.
        let _ctx_guard = self.install_request_budget_context_if_none(
            self.next_request_id(),
            canonical_id,
            false,
        );
        let mut visiting = rustc_hash::FxHashSet::default();
        self.resolve_fallthrough_surface_internal(canonical_id, &mut visiting)
    }

    /// Internal recursive method with cycle detection.
    pub(super) fn resolve_fallthrough_surface_internal(
        &self,
        canonical_id: &str,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Option<crate::types::FallthroughResolution> {
        self.resolve_fallthrough_surface_internal_with_overrides(canonical_id, None, visiting)
    }

    pub(super) fn resolve_fallthrough_surface_internal_with_overrides(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<&crate::resolver_core::FallthroughPropOverrideSet>,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Option<crate::types::FallthroughResolution> {
        use verter_semantic::analysis::component_meta::*;
        let started = component_meta_debug_enabled().then(Instant::now);
        component_meta_trace_custom!(
            "resolve_fallthrough_surface",
            format!(
                "owner={} overrides={} visiting={} store_view={}",
                canonical_id,
                prop_type_overrides
                    .map(|overrides| overrides.entries.len())
                    .unwrap_or_default(),
                visiting.len(),
                false,
            ),
        );

        // Cycle detection
        if !visiting.insert(canonical_id.to_string()) {
            self.provenance
                .resolver_cycle_detections
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            component_meta_trace_custom!(
                "resolve_fallthrough_cycle",
                format!("owner={} visiting={}", canonical_id, visiting.len()),
            );
            return Some(crate::types::FallthroughResolution {
                accepted_props: Vec::new(),
                accepted_events: Vec::new(),
                accepted_surface_completeness: AcceptedSurfaceCompleteness::LowerBound,
                fallthrough_surface: FallthroughSurface::Branches {
                    branches: vec![FallthroughBranch {
                        branch_key: "0".to_string(),
                        condition_text: None,
                        props: Vec::new(),
                        events: Vec::new(),
                        root_chain: vec![ResolvedRootStep::Unresolved {
                            tag: "component".to_string(),
                            reason: UnresolvedBranchReason::Cycle {
                                canonical_id: canonical_id.to_string(),
                            },
                        }],
                        status: BranchStatus::Unresolved {
                            reason: UnresolvedBranchReason::Cycle {
                                canonical_id: canonical_id.to_string(),
                            },
                        },
                    }],
                },
                fact_versions: self.current_dependency_fact_versions(
                    canonical_id,
                    &std::collections::BTreeSet::new(),
                ),
            });
        }

        // Override-bearing fallthrough is wholesale uncacheable. The owner-layer
        // `run_fallthrough_request` enforces it centrally: a non-cacheable key
        // skips warm lookup, cache admission, AND singleflight (computed cold,
        // returned-only). No host-side special case is needed here.
        //
        // Per-cold-compute completeness scoping lives INSIDE the request
        // executor (`FallthroughRequestExecutor`): it enters a FRESH held
        // scope per `compute` attempt — spanning the admission `store_stable`
        // -> `cache_fallthrough_result` so a partial fallthrough never warms —
        // and the rendezvous carries the COMPUTE completeness out. Each
        // attempt's held scope is DISCARDED (popped without bubbling), so a
        // discarded retry's partiality never taints the enclosing scope; the
        // FINAL completeness reaches the parent SOLELY via the
        // `fold_result_completeness(result.completeness)` surface-boundary
        // fold below (a FOLLOWER's join folds the same partiality via
        // `fold_follower_completeness`). No stale-across-retries caller scope
        // outlives the attempt it describes.
        //
        // No-poison CACHEABILITY scope: it BRACKETS the whole request, so the
        // cold compute (`compute_fallthrough_surface_uncached` and every child
        // it recurses into) runs INSIDE it and the executor's `store_stable`
        // samples the probe AFTER that compute. A scope opened at the store
        // site instead would start after the compute's fenced serve / broken
        // decl-body lease / unrootable route had already been consumed and
        // would prove nothing. Nested child scopes (`resolve_child_fallthrough`,
        // `intrinsic_members_for_tag`, `resolve_root_consumption`) fan their
        // non-cacheable reads out to EVERY active tracer, so a poisoned child
        // read also refuses THIS owner's admission.
        let (result, _non_cacheable) =
            crate::fact_signature_helpers::with_cacheability_scope(self, |probe| {
                crate::resolver_core::run_fallthrough_request(
                    self,
                    &self.resolver_runtime().top_level_fallthrough_singleflight,
                    canonical_id,
                    prop_type_overrides,
                    visiting,
                    None,
                    probe,
                    STORE_VIEW_STABILITY_MAX_ATTEMPTS,
                )
            });

        if matches!(result.source, RequestSource::Cache) {
            self.provenance
                .resolver_node_cache_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if !(matches!(result.source, RequestSource::Cache) && result.attempts == 1) {
            self.provenance
                .resolver_node_cache_misses
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if let RequestSource::Flight { role, forked_lane } = result.source {
            if role == SingleflightRole::Follower {
                self.provenance
                    .resolver_singleflight_coalesced
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if forked_lane {
                self.provenance
                    .resolver_cross_view_lane_forks
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }

        visiting.remove(canonical_id);
        if let Some(started) = started {
            match result.source {
                RequestSource::Cache => component_meta_debug(format!(
                    "resolve_fallthrough owner={} cached attempt={} took {:?}",
                    canonical_id,
                    result.attempts.saturating_sub(1),
                    started.elapsed(),
                )),
                RequestSource::Flight { role, .. } => component_meta_debug(format!(
                    "resolve_fallthrough owner={} role={:?} stable attempt={} took {:?}",
                    canonical_id,
                    role,
                    result.attempts.saturating_sub(1),
                    started.elapsed(),
                )),
                RequestSource::Fallback => component_meta_debug(format!(
                    "resolve_fallthrough owner={} retries_exhausted took {:?}",
                    canonical_id,
                    started.elapsed(),
                )),
            }
        }
        if let Some(resolution) = result.value.as_ref() {
            component_meta_trace_custom!(
                "resolve_fallthrough_result",
                format!(
                    "owner={} accepted_props={} accepted_events={} fact_versions={} completeness={:?}",
                    canonical_id,
                    resolution.accepted_props.len(),
                    resolution.accepted_events.len(),
                    resolution.fact_versions.len(),
                    resolution.accepted_surface_completeness,
                ),
            );
        }
        // Surface-boundary read of the rendezvous COMPUTE completeness: a
        // partial fallthrough surface (a budget trip / fatal read folded into
        // the executor's per-attempt scope, or a leader's partiality a
        // follower joined) folds into the ENCLOSING cold-compute scope + the
        // request suppress flag, so a parent fallthrough compute or the extract
        // helper observes this child's partiality without re-deriving it. A
        // `Complete` result is a no-op, so a complete surface still warms.
        crate::request_context::fold_result_completeness(result.completeness);
        result.value
    }

    pub(super) fn compute_fallthrough_surface_uncached(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<&crate::resolver_core::FallthroughPropOverrideSet>,
        visiting: &mut rustc_hash::FxHashSet<String>,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    ) -> Option<crate::types::FallthroughResolution> {
        // Try to reuse an already-cached Expanded resolved state before recomputing.
        // get_component_meta() typically resolves Expanded just before calling fallthrough,
        // so the cache should be warm.
        let resolved: Option<crate::meta_resolve::ResolvedComponentMetaState> = None;
        let resolved = if let Some(cached) = resolved {
            cached
        } else {
            let whole_hash = self
                .current_or_read_whole_hash(canonical_id)
                .unwrap_or_default();
            self.compute_component_meta_state_for_fallthrough(canonical_id, whole_hash, ctx)?
        };
        self.compute_fallthrough_surface_from_resolved_state(
            canonical_id,
            &resolved,
            prop_type_overrides,
            visiting,
            ctx,
        )
    }

    /// Run [`Self::compute_fallthrough_surface_from_resolved_state`] under a
    /// FRESH per-call cold-compute completeness scope and capture the COMPUTE
    /// completeness into a [`FallthroughComputeOutcome`].
    ///
    /// This is the centralised fallthrough-compute scoping for the DIRECT
    /// extract paths (the singleflight path scopes per-attempt inside the
    /// request executor). It replaces the former ad-hoc `enter()` +
    /// `current_cold_compute_completeness()` caller dance: a child fallthrough
    /// resolved during the compute bubbles its partiality into THIS scope (on
    /// its executor drop / its own follower fold), so the captured
    /// completeness covers the whole surface — and a stale partial from a
    /// DISCARDED completion-fence retry cannot taint a later complete attempt,
    /// because the completeness travels with the resolution it describes.
    pub(crate) fn compute_fallthrough_outcome_from_resolved_state(
        &self,
        canonical_id: &str,
        resolved: &crate::meta_resolve::ResolvedComponentMetaState,
        prop_type_overrides: Option<&crate::resolver_core::FallthroughPropOverrideSet>,
        visiting: &mut rustc_hash::FxHashSet<String>,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    ) -> FallthroughComputeOutcome {
        let _completeness_scope = crate::request_context::ColdComputeCompletenessScope::enter();
        let resolution = self.compute_fallthrough_surface_from_resolved_state(
            canonical_id,
            resolved,
            prop_type_overrides,
            visiting,
            ctx,
        );
        let completeness = crate::request_context::current_cold_compute_completeness();
        FallthroughComputeOutcome {
            resolution,
            completeness,
        }
    }

    pub(crate) fn compute_fallthrough_surface_from_resolved_state(
        &self,
        canonical_id: &str,
        resolved: &crate::meta_resolve::ResolvedComponentMetaState,
        prop_type_overrides: Option<&crate::resolver_core::FallthroughPropOverrideSet>,
        visiting: &mut rustc_hash::FxHashSet<String>,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    ) -> Option<crate::types::FallthroughResolution> {
        // INTERNAL backstop (install-if-none, NEVER install-always): a direct
        // internal caller of this fallthrough choke with no active request
        // context still gets the projection-budget fuse. A public surface that
        // already armed an outer context keeps it and this no-ops — so the
        // budget is never double-installed (the choke only scopes the
        // fallthrough sub-compute; the outer install spans the whole request).
        let _choke_budget_ctx_guard = self.install_request_budget_context_if_none(
            self.next_request_id(),
            canonical_id,
            false,
        );
        let fallthrough_fact_versions = resolved.fact_versions.clone();

        // Macro-DTO surface read runs under the request-bound `ctx` (not
        // `self`, the bare host) — `vue_macro_dtos_with_ctx` ->
        // `ctx.store_view()` panics on the bare-host rail in a release
        // build. See `tests/cases/g_session/session_meta_store_view_regression.rs`.
        let resolved_macros = resolver_component_meta_resolved_macros(
            ctx,
            canonical_id,
            resolved.snapshot.macros.as_ref(),
            &resolved.resolved_macros,
        );
        let resolved_type_registry =
            resolver_component_meta_type_registry(&resolved.resolved_type_registry);
        let input = verter_semantic::analysis::component_meta::ComponentMetaInput {
            macros: &resolved.snapshot.macros,
            bindings: &resolved.snapshot.bindings,
            imports: &resolved.snapshot.imports,
            template: resolved.snapshot.template.as_deref(),
            options_api: resolved.snapshot.options_api.as_ref(),
            analysis_flags: verter_semantic::analysis::types::AnalysisFlags::from_bits_truncate(
                resolved.snapshot.script_flags,
            ),
            styles: &resolved.snapshot.styles,
            vue_api_calls: &resolved.snapshot.vue_api_calls,
            store_usages: &resolved.snapshot.store_usages,
            resolved_macros: &resolved_macros,
            resolved_type_registry: &resolved_type_registry,
            evaluated_types: resolved.evaluated_types.as_ref(),
            file_path: canonical_id,
        };
        let base_meta = verter_semantic::analysis::component_meta::extract_component_meta(input);
        let fallthrough_resolver = HostFallthroughResolver {
            host: self,
            parent_canonical_id: canonical_id,
            parent_snapshot: &resolved.snapshot,
            // Carry the request-bound ctx through to the engine
            // constructions in `build_generic_child_prop_overrides`,
            // `resolve_root_consumption`, `resolve_dynamic_root_candidates`,
            // and `intrinsic_members_for_tag` so they bind to the
            // overlay-aware view rather than rebuild a workspace
            // snapshot inside the cold-compute `with_fact_tracer` scope.
            // `ctx.store_view()` is ALSO the per-element / per-child /
            // per-root fallthrough-node cache validation view — it is the
            // currentness-gated `RequestStoreView`, built once, so a
            // non-current cold-seed makes those node-cache validations fail
            // closed (the prior separate raw `live_view` field dropped the
            // currentness flag — the leak this closes).
            ctx,
        };
        // Build a lightweight fallthrough eval env: base owner env + runtime
        // values + prop overrides.
        let eval_env = self.build_fallthrough_eval_env_lightweight(
            canonical_id,
            &resolved.snapshot,
            Some(&base_meta.root_reachability),
        );

        let resolved_surface = resolver_resolve_fallthrough_surface(
            &fallthrough_resolver,
            canonical_id,
            &resolved.snapshot,
            &base_meta,
            prop_type_overrides,
            eval_env,
            fallthrough_fact_versions,
            visiting,
        );

        // R28 producer completeness — observe the fallthrough
        // resolver's CROSS-FILE dependency facts into the active fact
        // tracer. `resolver_resolve_fallthrough_surface` walks
        // recursive child-component surfaces and accumulates a
        // `fact_versions` set covering every child it followed (via
        // `current_dependency_fact_versions` + each child resolution's
        // own facts), but those reads go through the curated path and
        // never reach the tracer on their own. A fallthrough node
        // CACHE HIT for a child likewise returns without bubbling the
        // child's facts. Fanning the cross-file facts into every
        // active tracer scope here closes the gap: the component-meta
        // cold compute's `with_fact_tracer` scope (this method runs
        // inside `extract_component_meta_from_resolved`) captures the
        // recursive child deps, so the published
        // `ComponentMetaResultEntry` signature invalidates correctly
        // when a child component's root changes.
        //
        // The owner's OWN facts are excluded from the fan-out: the
        // owner's content is already observed by the cold compute's
        // dispatch reads and gated by the result cache's legacy
        // whole-hash rail. Excluding them also keeps the owner's
        // `DerivedFactHash{Route}` — which can shift as the owner's
        // own `IndexedReady` lazily (re-)materialises mid-request —
        // out of the tracer-owned signature, so a fallthrough query
        // does not reintroduce a non-round-tripping owner Route fact.
        // Child / dep `Route` facts DO round-trip, so they are kept.
        // Empty signatures and an absent tracer stack are both a
        // no-op.
        let cross_file_fallthrough_facts: Vec<crate::resolver_core::FactVersionRef> =
            resolved_surface
                .fact_versions
                .iter()
                .filter(|fact| fact.canonical_id() != Some(canonical_id))
                .cloned()
                .collect();
        crate::fact_signature_helpers::observe_fact_signature(&cross_file_fallthrough_facts);

        Some(crate::types::FallthroughResolution {
            accepted_props: resolved_surface.accepted_props,
            accepted_events: resolved_surface.accepted_events,
            accepted_surface_completeness: resolved_surface.accepted_surface_completeness,
            fallthrough_surface: resolved_surface.fallthrough_surface,
            fact_versions: resolved_surface.fact_versions,
        })
    }

    /// Lightweight fallthrough eval env: base owner env + runtime values.
    ///
    /// Child prop-type overrides are NOT injected here: they ride the
    /// fallthrough recursion as a node-backed
    /// [`crate::resolver_core::FallthroughPropOverrideSet`] and are consumed in
    /// node domain by the value evaluators (`evaluate_fallthrough_value_node`'s
    /// override forwarding), never re-injected into this `EvalEnv` as a
    /// `TypeExpr`.
    pub(super) fn build_fallthrough_eval_env_lightweight(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        root_reachability: Option<&verter_semantic::analysis::component_meta::RootReachability>,
    ) -> Option<verter_semantic::analysis::type_eval::EvalEnv> {
        component_meta_trace_custom!(
            "build_fallthrough_eval_env_lightweight",
            format!(
                "owner={} imports={} store_view={}",
                canonical_id,
                snapshot.imports.len(),
                false,
            ),
        );
        let mut env = self
            .base_eval_env_arc(canonical_id)
            .map(|env| (*env).clone())?;

        // Hydrate required runtime values from imports.
        let required_runtime_value_names = match root_reachability {
            Some(root_reachability) => {
                collect_required_root_fallthrough_runtime_value_names(snapshot, root_reachability)
            }
            None => collect_required_template_runtime_value_names(snapshot),
        };
        if !required_runtime_value_names.is_empty() {
            let local_value_names: rustc_hash::FxHashSet<String> =
                env.value_symbols.keys().cloned().collect();

            // The graph-native dep extractor
            // (`fallthrough_runtime_value_deps_graph_native`) enumerates the
            // cross-file runtime-value sources the materializer hydrates,
            // WITHOUT a whole-env clone of any dependency, so a future
            // whole-env-free builder can drive hydration off the per-name
            // value readers. Its equivalence with the materializer is proved
            // OFFLINE on full `(source_canonical, source_name)` pairs by
            // `c3_fallthrough_runtime_value_deps_graph_native_equals_\
            // materializer_touched_full_pairs`. No in-production cross-check
            // runs here: the only faithful in-production touched-pair recompute
            // would route through the legacy `resolve_value_export_target`
            // whole-env peel (materialising every dependency's whole env) —
            // the exact cost the readiness work removes — and a name-count
            // proxy is unsound (legal double-alias-onto-one-source hydrates two
            // bindings from one dep pair). The offline pair-equality test is
            // the authoritative equivalence rail.

            self.materialize_imported_runtime_values_into_env(
                snapshot,
                &local_value_names,
                Some(&required_runtime_value_names),
                &mut env,
            );
        }

        Some(env)
    }

    /// Graph-native dep-extraction reader for the lightweight fallthrough
    /// env consumer: enumerates the cross-file runtime-value DEP SET the
    /// hydration touches WITHOUT a whole-env clone of any dependency, so a
    /// whole-env-free builder can drive hydration off the per-name value
    /// readers instead of a dependency `whole_env()`.
    ///
    /// Returns the deterministic, deduplicated set of
    /// `(source_canonical_id, source_name)` pairs the materializer
    /// resolves for the owner's required runtime-value bindings — the
    /// EXACT selection `materialize_imported_runtime_values_into_env`
    /// makes (skip type-only imports/bindings + namespace bindings, skip
    /// owner-shadowed locals, filter to the required names), routed
    /// graph-natively (no dependency whole-env). The
    /// `owner_local_value_names` shadow set is read from the per-symbol
    /// value-header index (PRESENCE, no whole-env), matching the
    /// materializer's `env.value_symbols.keys()` shadow set for
    /// file-scope value symbols.
    ///
    /// Its equivalence with the materializer is proved on full
    /// `(source_canonical, source_name)` pairs by the C3 dep-equivalence
    /// tests; its presence is pinned by
    /// `whole_env_consumer_graph_native_inventory.rs`.
    #[allow(dead_code)]
    pub(super) fn fallthrough_runtime_value_deps_graph_native(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        root_reachability: Option<&verter_semantic::analysis::component_meta::RootReachability>,
    ) -> std::collections::BTreeSet<(String, String)> {
        use verter_semantic::analysis::types::ImportBindingKind;

        let required_runtime_value_names = match root_reachability {
            Some(root_reachability) => {
                collect_required_root_fallthrough_runtime_value_names(snapshot, root_reachability)
            }
            None => collect_required_template_runtime_value_names(snapshot),
        };

        let mut deps = std::collections::BTreeSet::new();
        if required_runtime_value_names.is_empty() {
            return deps;
        }

        // Owner-local value-symbol shadow set via the per-symbol header
        // index — NO whole-env clone. A binding whose name is an owner
        // file-scope value symbol is shadowed and never hydrated, exactly
        // as the materializer's `local_value_names` filter requires.
        let owner_local_value_names: rustc_hash::FxHashSet<String> = self
            .routed_shallow_state(canonical_id)
            .map(|state| {
                state
                    .decl_bodies()
                    .header_index()
                    .value_headers
                    .keys()
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        for import in &snapshot.imports {
            if import.is_type_only {
                continue;
            }
            let Some(dep_canonical_id) = import.resolved_canonical_id.as_deref() else {
                continue;
            };
            for binding in &import.bindings {
                if binding.is_type_only
                    || matches!(binding.kind, ImportBindingKind::Namespace)
                    || owner_local_value_names.contains(&binding.name)
                    || !required_runtime_value_names.contains(&binding.name)
                {
                    continue;
                }
                let imported_name = binding
                    .imported_name
                    .as_deref()
                    .unwrap_or(binding.name.as_str());
                // Graph-native export-target + alias peel: NEVER reaches
                // `base_eval_env_arc`/`whole_env()` on the dependency, so
                // this reader does not materialise the dependency's whole
                // env. The materializer's selection identity is preserved
                // — same `resolve_named_export` walk, same single-segment
                // alias chain, peeled per-symbol instead of via the env.
                let (source_canonical_id, source_name) = self
                    .resolve_value_export_target_graph_native(dep_canonical_id, imported_name)
                    .map(|target| (target.canonical_id, target.name))
                    .unwrap_or_else(|| (dep_canonical_id.to_string(), imported_name.to_string()));
                deps.insert((source_canonical_id, source_name));
            }
        }

        deps
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(super) fn materialize_imported_runtime_values_into_env(
        &self,
        snapshot: &FileAnalysisSnapshot,
        owner_local_value_names: &rustc_hash::FxHashSet<String>,
        required_runtime_value_names: Option<&rustc_hash::FxHashSet<String>>,
        env: &mut verter_semantic::analysis::type_eval::EvalEnv,
    ) {
        component_meta_trace_custom!(
            "materialize_runtime_values",
            format!(
                "imports={} owner_local_values={} existing_value_symbols={} store_view={}",
                snapshot.imports.len(),
                owner_local_value_names.len(),
                env.value_symbols.len(),
                false,
            ),
        );
        let started = component_meta_debug_enabled().then(Instant::now);
        let resolver = HostRuntimeValueResolver { host: self };
        materialize_imported_runtime_values_into_env(
            snapshot.imports.as_slice(),
            owner_local_value_names,
            required_runtime_value_names,
            env,
            &resolver,
        );
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "materialize_runtime_values imports={} value_symbols={} took {:?}",
                snapshot.imports.len(),
                env.value_symbols.len(),
                started.map(|start| start.elapsed()).unwrap_or_default(),
            ));
        }
        component_meta_trace_custom!(
            "materialize_runtime_values_result",
            format!(
                "imports={} owner_local_values={} value_symbols={}",
                snapshot.imports.len(),
                owner_local_value_names.len(),
                env.value_symbols.len(),
            ),
        );
    }

    pub(super) fn build_generic_child_prop_overrides(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        usage_index: u32,
        eval_env: &mut Option<verter_semantic::analysis::type_eval::EvalEnv>,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        overrides_in: Option<&crate::resolver_core::FallthroughPropOverrideSet>,
    ) -> Option<crate::resolver_core::FallthroughPropOverrideSet> {
        if !self.config.generic_root_propagation {
            return None;
        }

        let template = snapshot.template.as_deref()?;
        let usage = template.components.get(usage_index as usize)?;
        // Bind the engine to the supplied request-bound `ctx` so cache
        // validators inside the engine inherit the overlay-aware view.
        let mut engine = crate::resolver_core::ComponentMetaQueryEngine::new(ctx);
        let env_ref = eval_env.as_ref();

        // Each child prop override is carried as its resolved value NODE
        // (env- + parent-override-aware, via `value_expression_override_node`).
        // The child consumes these nodes in node domain; nothing is
        // materialised to a `TypeExpr` here.
        let mut entries: Vec<crate::resolver_core::FallthroughPropOverride> = Vec::new();
        for prop in &usage.props {
            if prop.from_spread {
                continue;
            }
            if usage.is_dynamic && prop.name == "is" {
                continue;
            }

            let Some(node) =
                engine.value_expression_override_node(canonical_id, prop, env_ref, overrides_in)
            else {
                continue;
            };
            entries.push(crate::resolver_core::FallthroughPropOverride {
                name: prop.name.clone(),
                node,
            });
        }

        if entries.is_empty() {
            None
        } else {
            Some(crate::resolver_core::FallthroughPropOverrideSet { entries })
        }
    }

    pub(super) fn resolve_root_consumption(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        element_index: u32,
        base: &verter_semantic::analysis::component_meta::ConsumedRootBindings,
        has_unknown_spread: bool,
        eval_env: &mut Option<verter_semantic::analysis::type_eval::EvalEnv>,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        overrides: Option<&crate::resolver_core::FallthroughPropOverrideSet>,
    ) -> ResolvedConsumedBindings {
        use verter_semantic::analysis::component_meta::PartialBranchReason;

        let mut resolved = ResolvedConsumedBindings {
            bindings: verter_semantic::analysis::component_meta::ConsumedRootBindings {
                attrs: base.attrs.clone(),
                listeners: base.listeners.clone(),
                has_dynamic_attr_name: base.has_dynamic_attr_name,
                has_dynamic_listener_name: base.has_dynamic_listener_name,
            },
            partial_reasons: Vec::new(),
        };

        if base.has_dynamic_attr_name {
            push_partial_reason(
                &mut resolved.partial_reasons,
                PartialBranchReason::DynamicAttrName,
            );
        }
        if base.has_dynamic_listener_name {
            push_partial_reason(
                &mut resolved.partial_reasons,
                PartialBranchReason::DynamicListenerName,
            );
        }

        if has_unknown_spread {
            let Some(template) = snapshot.template.as_deref() else {
                push_partial_reason(
                    &mut resolved.partial_reasons,
                    PartialBranchReason::UnknownSpread,
                );
                return resolved;
            };

            let Some(element) = template.elements.get(element_index as usize) else {
                push_partial_reason(
                    &mut resolved.partial_reasons,
                    PartialBranchReason::UnknownSpread,
                );
                return resolved;
            };

            let spread_directives: Vec<_> = element
                .directives
                .iter()
                .filter(|directive| directive.name == "bind" && directive.argument.is_none())
                .collect();

            if spread_directives.is_empty() {
                push_partial_reason(
                    &mut resolved.partial_reasons,
                    PartialBranchReason::UnknownSpread,
                );
            }

            // Bind the engine to the supplied request-bound `ctx` so
            // cache validators inside the engine inherit the overlay-aware
            // view.
            let mut engine = crate::resolver_core::ComponentMetaQueryEngine::new(ctx);
            let env_ref = eval_env.as_ref();
            for directive in spread_directives {
                let Some(expression) = directive.expression.as_deref() else {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                    continue;
                };

                let Some(summary) = engine.known_spread_keys_for_value_expression(
                    canonical_id,
                    expression,
                    env_ref,
                    overrides,
                ) else {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                    continue;
                };

                resolved.bindings.attrs.extend(summary.attrs);
                resolved.bindings.listeners.extend(summary.listeners);
                if !summary.exact {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                }
            }
        }

        resolved.bindings.attrs.sort();
        resolved.bindings.attrs.dedup();
        resolved.bindings.listeners.sort();
        resolved.bindings.listeners.dedup();
        resolved.partial_reasons.sort();
        resolved.partial_reasons.dedup();
        resolved
    }

    pub(super) fn resolve_dynamic_root_candidates(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        usage_index: u32,
        eval_env: &mut Option<verter_semantic::analysis::type_eval::EvalEnv>,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        overrides: Option<&crate::resolver_core::FallthroughPropOverrideSet>,
    ) -> Vec<DynamicRootCandidate> {
        let Some(template) = snapshot.template.as_deref() else {
            return Vec::new();
        };
        let Some(usage) = template.components.get(usage_index as usize) else {
            return Vec::new();
        };
        let Some(is_prop) = usage.props.iter().find(|prop| prop.name == "is") else {
            return Vec::new();
        };

        let expression = is_prop
            .expression
            .clone()
            .or_else(|| is_prop.is_shorthand.then(|| is_prop.name.clone()));
        let Some(expression) = expression else {
            return Vec::new();
        };

        let mut candidates = Vec::new();
        if let Some(lowered) =
            verter_semantic::analysis::type_eval_build::parse_value_expression_type(&expression)
        {
            candidates.extend(collect_dynamic_root_candidates_from_type(
                &lowered,
                snapshot.imports.as_slice(),
            ));
        }
        // Bind the engine to the supplied request-bound `ctx` so
        // cache validators inside the engine inherit the overlay-aware
        // view. The evaluated `is=` value resolves to a NODE and its
        // dynamic-root candidates are read in node domain (the raw-parse
        // step above stays syntactic).
        let mut engine = crate::resolver_core::ComponentMetaQueryEngine::new(ctx);
        candidates.extend(engine.dynamic_root_candidates_for_value_expression(
            canonical_id,
            &expression,
            eval_env.as_ref(),
            overrides,
            snapshot.imports.as_slice(),
        ));

        candidates.sort_by(|left, right| left.ordering(right));
        candidates.dedup();
        candidates
    }

    /// Store fallthrough resolution in the compile cache.
    ///
    /// `probe` is the cacheability probe of the scope opened around the whole
    /// request in [`Self::resolve_fallthrough_surface_internal_with_overrides`]
    /// — it therefore encloses the cold compute that produced `result`. Every
    /// admission below (both node stores AND the legacy mirror) is gated on it.
    pub(super) fn cache_fallthrough_result(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<&crate::resolver_core::FallthroughPropOverrideSet>,
        result: &crate::types::FallthroughResolution,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
    ) {
        // No-poison: a PARTIAL fallthrough (a budget/fuse trip folded into the
        // active cold-compute completeness scope) must NOT warm any fallthrough
        // cache. Both `store_node` and `mirror_cached_fallthrough_arc` self-gate
        // on the same typed completeness signal — the single no-poison rail
        // shared with the materialiser — so this early return is a short-circuit,
        // not the only rail.
        if crate::cache_runtime::refuse_result_cache_admission_if_partial(
            crate::request_context::current_cold_compute_completeness().is_partial(),
        ) {
            return;
        }

        let cache_key = fallthrough_cache_key(
            canonical_id,
            self.config.generic_root_propagation,
            prop_type_overrides,
        );
        let resolution = Arc::new(result.clone());
        self.resolver_runtime().fallthrough.store_node(
            crate::resolver_core::fallthrough_resolver::root_follow_key(
                canonical_id,
                crate::resolver_core::FallthroughOverrideIdentity::for_overrides(
                    prop_type_overrides,
                ),
                self.config.generic_root_propagation,
            ),
            self.build_runtime_root_follow_node(result),
            probe,
        );
        self.resolver_runtime().fallthrough.store_node(
            cache_key,
            self.build_runtime_fallthrough_node(result),
            probe,
        );
        if prop_type_overrides.is_none() {
            self.mirror_cached_fallthrough_arc(canonical_id, resolution, probe);
        }
    }

    pub(super) fn extract_runtime_branch_results(
        result: &crate::types::FallthroughResolution,
    ) -> Vec<crate::resolver_core::fallthrough_resolver::FallthroughBranchResult> {
        match &result.fallthrough_surface {
            verter_semantic::analysis::component_meta::FallthroughSurface::Branches { branches } => branches
                .iter()
                .map(
                    |branch| crate::resolver_core::fallthrough_resolver::FallthroughBranchResult {
                        branch_key: branch.branch_key.clone(),
                        inherited_prop_names: branch
                            .props
                            .iter()
                            .map(|prop| prop.name.clone())
                            .collect(),
                        inherited_event_names: branch
                            .events
                            .iter()
                            .map(|event| event.name.clone())
                            .collect(),
                        resolved: !matches!(
                            branch.status,
                            verter_semantic::analysis::component_meta::BranchStatus::Unresolved { .. }
                        ),
                    },
                )
                .collect(),
            verter_semantic::analysis::component_meta::FallthroughSurface::None { .. } => Vec::new(),
        }
    }

    pub(super) fn build_runtime_fallthrough_node(
        &self,
        result: &crate::types::FallthroughResolution,
    ) -> crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
        let branches = Self::extract_runtime_branch_results(result);
        crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
            value: crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::BranchUnion(
                crate::resolver_core::fallthrough_resolver::BranchUnionResult {
                    accepted_props: result.accepted_props.clone(),
                    accepted_events: result.accepted_events.clone(),
                    accepted_surface_completeness: result.accepted_surface_completeness,
                    fallthrough_surface: result.fallthrough_surface.clone(),
                    all_resolved: matches!(
                        result.accepted_surface_completeness,
                        verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::Exact,
                    ),
                    branches,
                },
            ),
            facts: result.fact_versions.clone(),
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn build_runtime_root_follow_node(
        &self,
        result: &crate::types::FallthroughResolution,
    ) -> crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
        let branches = Self::extract_runtime_branch_results(result);
        crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
            value: crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::RootFollow(
                crate::resolver_core::fallthrough_resolver::RootFollowResult {
                    accepted_props: result.accepted_props.clone(),
                    accepted_events: result.accepted_events.clone(),
                    accepted_surface_completeness: result.accepted_surface_completeness,
                    fallthrough_surface: result.fallthrough_surface.clone(),
                    has_single_root: matches!(
                        result.fallthrough_surface,
                        verter_semantic::analysis::component_meta::FallthroughSurface::Branches { ref branches } if branches.len() == 1,
                    ),
                    branches,
                },
            ),
            facts: result.fact_versions.clone(),
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn build_runtime_child_surface_node(
        &self,
        result: &crate::types::FallthroughResolution,
    ) -> crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
        crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
            value: crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::ChildSurfaceFollow(
                crate::resolver_core::fallthrough_resolver::ChildSurfaceResult {
                    accepted_props: result.accepted_props.clone(),
                    accepted_events: result.accepted_events.clone(),
                    accepted_surface_completeness: result.accepted_surface_completeness,
                    fallthrough_surface: result.fallthrough_surface.clone(),
                    inherited_prop_names: result
                        .accepted_props
                        .iter()
                        .map(|prop| prop.name.clone())
                        .collect(),
                    inherited_event_names: result
                        .accepted_events
                        .iter()
                        .map(|event| event.name.clone())
                        .collect(),
                    resolved: matches!(
                        result.accepted_surface_completeness,
                        verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::Exact,
                    ),
                },
            ),
            facts: result.fact_versions.clone(),
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn build_runtime_intrinsic_surface_node(
        &self,
        members: &[crate::resolver_core::IntrinsicSurfaceMember],
    ) -> crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
        let mut attr_names = Vec::new();
        let mut event_names = Vec::new();
        for member in members {
            match member.kind {
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Attr => {
                    attr_names.push(member.name.clone());
                }
                verter_semantic::analysis::html_intrinsics::IntrinsicMemberKind::Listener => {
                    event_names.push(member.name.clone());
                }
            }
        }

        crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
            value:
                crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::IntrinsicSurface(
                    crate::resolver_core::fallthrough_resolver::IntrinsicSurfaceResult {
                        members: members.to_vec(),
                        attr_names,
                        event_names,
                    },
                ),
            facts: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn build_runtime_consumed_bindings_node(
        &self,
        resolved: &ResolvedConsumedBindings,
    ) -> crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
        let mut consumed_names = resolved.bindings.attrs.clone();
        consumed_names.extend(resolved.bindings.listeners.iter().cloned());
        consumed_names.sort();
        consumed_names.dedup();

        crate::resolver_core::fallthrough_resolver::FallthroughNodeResult {
            value:
                crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::ConsumedBindings(
                    crate::resolver_core::fallthrough_resolver::ConsumedBindingsResult {
                        attrs: resolved.bindings.attrs.clone(),
                        listeners: resolved.bindings.listeners.clone(),
                        has_dynamic_attr_name: resolved.bindings.has_dynamic_attr_name,
                        has_dynamic_listener_name: resolved.bindings.has_dynamic_listener_name,
                        partial_reasons: resolved.partial_reasons.clone(),
                        consumed_names,
                    },
                ),
            facts: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn runtime_child_node_to_resolution(
        &self,
        node: crate::resolver_core::fallthrough_resolver::FallthroughNodeResult,
    ) -> Option<crate::types::FallthroughResolution> {
        match node.value {
            crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::ChildSurfaceFollow(
                child,
            ) => Some(crate::types::FallthroughResolution {
                accepted_props: child.accepted_props,
                accepted_events: child.accepted_events,
                accepted_surface_completeness: child.accepted_surface_completeness,
                fallthrough_surface: child.fallthrough_surface,
                fact_versions: node.facts,
            }),
            _ => None,
        }
    }

    pub(super) fn runtime_branch_union_node_to_resolution(
        &self,
        node: crate::resolver_core::fallthrough_resolver::FallthroughNodeResult,
    ) -> Option<crate::types::FallthroughResolution> {
        match node.value {
            crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::BranchUnion(
                branch_union,
            ) => Some(crate::types::FallthroughResolution {
                accepted_props: branch_union.accepted_props,
                accepted_events: branch_union.accepted_events,
                accepted_surface_completeness: branch_union.accepted_surface_completeness,
                fallthrough_surface: branch_union.fallthrough_surface,
                fact_versions: node.facts,
            }),
            _ => None,
        }
    }

    pub(super) fn runtime_root_follow_node_to_resolution(
        &self,
        node: crate::resolver_core::fallthrough_resolver::FallthroughNodeResult,
    ) -> Option<crate::types::FallthroughResolution> {
        match node.value {
            crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::RootFollow(
                root_follow,
            ) => Some(crate::types::FallthroughResolution {
                accepted_props: root_follow.accepted_props,
                accepted_events: root_follow.accepted_events,
                accepted_surface_completeness: root_follow.accepted_surface_completeness,
                fallthrough_surface: root_follow.fallthrough_surface,
                fact_versions: node.facts,
            }),
            _ => None,
        }
    }

    pub(super) fn runtime_intrinsic_node_to_members(
        &self,
        node: crate::resolver_core::fallthrough_resolver::FallthroughNodeResult,
    ) -> Option<Vec<crate::resolver_core::IntrinsicSurfaceMember>> {
        match node.value {
            crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::IntrinsicSurface(
                intrinsic,
            ) => Some(intrinsic.members),
            _ => None,
        }
    }

    pub(super) fn runtime_consumed_bindings_to_resolution(
        &self,
        node: crate::resolver_core::fallthrough_resolver::FallthroughNodeResult,
    ) -> Option<ResolvedConsumedBindings> {
        match node.value {
            crate::resolver_core::fallthrough_resolver::FallthroughNodeValue::ConsumedBindings(
                consumed,
            ) => Some(ResolvedConsumedBindings {
                bindings: verter_semantic::analysis::component_meta::ConsumedRootBindings {
                    attrs: consumed.attrs,
                    listeners: consumed.listeners,
                    has_dynamic_attr_name: consumed.has_dynamic_attr_name,
                    has_dynamic_listener_name: consumed.has_dynamic_listener_name,
                },
                partial_reasons: consumed.partial_reasons,
            }),
            _ => None,
        }
    }

    pub(super) fn mirror_cached_fallthrough_arc(
        &self,
        canonical_id: &str,
        resolution: Arc<crate::types::FallthroughResolution>,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
    ) {
        // No-poison SELF-GATE: the mirror is a promotion site (it warms the
        // legacy `cached_fallthrough` entry on `DerivedRawState`), so it carries
        // the SAME two rails as `store_node` — never relying on caller
        // discipline.
        //
        // NON-CACHEABLE: the enclosing compute consumed a fenced serve / a
        // broken decl-body lease / an unrootable route / an unobservable
        // contributor source env, or overflowed the fact-signature cap. The
        // mirror's own `fact_versions` root on the LIVE view, so a poisoned
        // entry would revalidate on every warm read forever. The resolution is
        // still returned to the caller — mirror non-admission only.
        if probe.non_cacheable() {
            return;
        }
        // PARTIAL: a budget / fuse / fatal read folded into the active
        // cold-compute completeness scope means this surface is incomplete;
        // mirroring it would warm a partial as complete.
        if crate::cache_runtime::refuse_result_cache_admission_if_partial(
            crate::request_context::current_cold_compute_completeness().is_partial(),
        ) {
            return;
        }
        // cached_fallthrough lives on DerivedRawState (D48 split).
        {
            if self.effective_file_state(canonical_id, None).is_some() {
                // R3/R26/R28: lift the resolution's observed fact set
                // into an `Arc<[FactVersionRef]>` so warm-hit reads
                // clone a cheap handle.
                let fact_versions: Arc<[crate::resolver_core::FactVersionRef]> =
                    Arc::from(resolution.fact_versions.clone().into_boxed_slice());
                // Fan-out to outer active tracers so the mirrored
                // fallthrough entry participates in transitive
                // CROSS-FILE fact bubbling. The owner's own facts are
                // excluded: they are not transitive dependencies of
                // the owner's surface, and the curated
                // `DerivedFactHash{owner, Route}` carried in
                // `resolution.fact_versions` is dual-sourced on
                // `HostStoreView::derived_hashes` and does not
                // round-trip on warm validation (see
                // `mirror_cached_resolved_meta_arc`). The stored
                // `CachedFallthroughEntry` keeps the FULL set for its
                // own warm validation. Empty signatures are a no-op.
                let cross_file_facts: Vec<crate::resolver_core::FactVersionRef> = fact_versions
                    .iter()
                    .filter(|fact| fact.canonical_id() != Some(canonical_id))
                    .cloned()
                    .collect();
                crate::fact_signature_helpers::observe_fact_signature(&cross_file_facts);
                let mut derived_ref = self
                    .derived_raw_cache()
                    .entry(canonical_id.to_string())
                    .or_default();
                derived_ref.value_mut().cached_fallthrough =
                    Some(crate::types::CachedFallthroughEntry {
                        fact_versions,
                        generic_root_propagation: self.config.generic_root_propagation,
                        resolution,
                    });
            }
        }
    }

    pub(super) fn parse_dependency_set_for_file(
        &self,
        canonical_id: &str,
    ) -> std::collections::BTreeSet<String> {
        {
            use crate::host_executor::HostSourceData;

            let Some(source) = self.scheduler.try_get_source(canonical_id) else {
                return std::collections::BTreeSet::new();
            };
            let Some(hd) = source.downcast_data::<HostSourceData>() else {
                return std::collections::BTreeSet::new();
            };

            hd.parse
                .external_requests
                .iter()
                .map(|r| r.resolved_canonical_id.clone())
                .chain(
                    hd.parse
                        .script_analysis
                        .imports
                        .iter()
                        .filter(|imp| imp.source.starts_with('.'))
                        .map(|imp| crate::id::resolve_external(canonical_id, &imp.source)),
                )
                .collect()
        }
    }

    pub(super) fn resolved_dependency_targets(
        import_routes: &rustc_hash::FxHashMap<String, DependencyResolution>,
    ) -> std::collections::BTreeSet<String> {
        import_routes
            .values()
            .filter_map(|res| res.effective_target().map(|s| s.to_string()))
            .collect()
    }

    /// Sync transitive macro/type dependencies for a file. The
    /// workspace's `replace_semantic_transitive` is called
    /// UNCONDITIONALLY: even when `cc.dependencies` union is
    /// unchanged, the semantic-class slice may have changed (e.g., a
    /// dep moves from semantic-only to direct-import).
    pub(crate) fn sync_transitive_macro_type_dependencies(
        &self,
        canonical_id: &str,
        transitive_deps: &std::collections::BTreeSet<String>,
    ) {
        let mut new_deps = self.parse_dependency_set_for_file(canonical_id);
        // import_routes lives on DerivedRawState; dependencies on
        // DependencyState (D48 split).
        {
            let derived_routes = self
                .derived_raw_cache()
                .get(canonical_id)
                .map(|d| d.import_routes.clone())
                .unwrap_or_default();
            new_deps.extend(Self::resolved_dependency_targets(&derived_routes));
            new_deps.extend(transitive_deps.iter().cloned());
            let mut dep_ref = self
                .dependency_cache()
                .entry(canonical_id.to_string())
                .or_default();
            dep_ref.value_mut().dependencies = new_deps;
        }
        // ALWAYS fires — even when cc.dependencies union is unchanged, the
        // semantic-class slice may have changed (closes F15).
        self.ws()
            .replace_semantic_transitive(canonical_id, transitive_deps.clone());
    }
}

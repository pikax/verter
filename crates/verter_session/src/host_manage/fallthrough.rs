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

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use crate::resolver_core::{
    collect_dynamic_root_candidates_from_type,
    component_meta_resolved_macros as resolver_component_meta_resolved_macros,
    component_meta_type_registry as resolver_component_meta_type_registry, fallthrough_cache_key,
    known_spread_keys_from_type_expr, materialize_imported_runtime_values_into_env,
    push_partial_reason, resolve_fallthrough_surface as resolver_resolve_fallthrough_surface,
    resolve_usage_prop_type, DynamicRootCandidate, RequestSource, ResolvedConsumedBindings,
    SingleflightRole,
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

impl VerterHost {
    pub fn resolve_fallthrough_surface(
        &self,
        canonical_id: &str,
    ) -> Option<crate::types::FallthroughResolution> {
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
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
        >,
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
                    .map(|overrides| overrides.len())
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

        let result = crate::resolver_core::run_fallthrough_request(
            self,
            &self.resolver_runtime().top_level_fallthrough_singleflight,
            canonical_id,
            prop_type_overrides,
            visiting,
            None,
            STORE_VIEW_STABILITY_MAX_ATTEMPTS,
        );

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
        result.value
    }

    pub(super) fn compute_fallthrough_surface_uncached(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
        >,
        visiting: &mut rustc_hash::FxHashSet<String>,
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
            self.compute_component_meta_state_for_fallthrough(canonical_id, whole_hash)?
        };
        self.compute_fallthrough_surface_from_resolved_state(
            canonical_id,
            &resolved,
            prop_type_overrides,
            visiting,
        )
    }

    pub(crate) fn compute_fallthrough_surface_from_resolved_state(
        &self,
        canonical_id: &str,
        resolved: &crate::meta_resolve::ResolvedComponentMetaState,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
        >,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Option<crate::types::FallthroughResolution> {
        let fallthrough_fact_versions = resolved.fact_versions.clone();

        let resolved_macros = resolver_component_meta_resolved_macros(
            resolved.snapshot.macros.as_ref(),
            &resolved.resolved_macros,
        );
        let resolved_type_registry =
            resolver_component_meta_type_registry(&resolved.resolved_type_registry);
        let canonical_source = self.read_analysis_source(canonical_id);
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
            canonical_source: canonical_source.as_deref(),
        };
        let base_meta = verter_semantic::analysis::component_meta::extract_component_meta(input);
        let fallthrough_resolver = HostFallthroughResolver {
            host: self,
            parent_canonical_id: canonical_id,
            parent_snapshot: &resolved.snapshot,
        };
        // Build a lightweight fallthrough eval env: base owner env + runtime
        // values + prop overrides.
        let eval_env = self.build_fallthrough_eval_env_lightweight(
            canonical_id,
            &resolved.snapshot,
            Some(&base_meta.root_reachability),
            prop_type_overrides,
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

        Some(crate::types::FallthroughResolution {
            accepted_props: resolved_surface.accepted_props,
            accepted_events: resolved_surface.accepted_events,
            accepted_surface_completeness: resolved_surface.accepted_surface_completeness,
            fallthrough_surface: resolved_surface.fallthrough_surface,
            fact_versions: resolved_surface.fact_versions,
        })
    }

    /// Lightweight fallthrough eval env: base owner env + runtime values + overrides.
    pub(super) fn build_fallthrough_eval_env_lightweight(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        root_reachability: Option<&verter_semantic::analysis::component_meta::RootReachability>,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
        >,
    ) -> Option<verter_semantic::analysis::type_eval::EvalEnv> {
        component_meta_trace_custom!(
            "build_fallthrough_eval_env_lightweight",
            format!(
                "owner={} imports={} overrides={} store_view={}",
                canonical_id,
                snapshot.imports.len(),
                prop_type_overrides
                    .map(|overrides| overrides.len())
                    .unwrap_or_default(),
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
            self.materialize_imported_runtime_values_into_env(
                snapshot,
                &local_value_names,
                Some(&required_runtime_value_names),
                &mut env,
            );
        }

        // Apply prop type overrides for generic root propagation.
        if let Some(overrides) = prop_type_overrides {
            crate::resolver_core::inject_prop_type_overrides(&mut env, overrides);
        }

        Some(env)
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
    ) -> Option<rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>> {
        if !self.config.generic_root_propagation {
            return None;
        }

        let template = snapshot.template.as_deref()?;
        let usage = template.components.get(usage_index as usize)?;
        let mut overrides = rustc_hash::FxHashMap::default();
        let mut engine = crate::resolver_core::ComponentMetaQueryEngine::new(self);
        let env_ref = eval_env.as_ref();

        for prop in &usage.props {
            if prop.from_spread {
                continue;
            }
            if usage.is_dynamic && prop.name == "is" {
                continue;
            }

            let Some(prop_type) = resolve_usage_prop_type(prop, |expr| {
                crate::resolver_core::evaluate_value_expression_via_env_or_dispatch(
                    expr,
                    canonical_id,
                    env_ref,
                    &mut engine,
                )
            }) else {
                continue;
            };
            overrides.insert(prop.name.clone(), prop_type);
        }

        if overrides.is_empty() {
            None
        } else {
            Some(overrides)
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

            let mut engine = crate::resolver_core::ComponentMetaQueryEngine::new(self);
            let env_ref = eval_env.as_ref();
            for directive in spread_directives {
                let Some(expression) = directive.expression.as_deref() else {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                    continue;
                };

                let Some(ty) = crate::resolver_core::evaluate_value_expression_via_env_or_dispatch(
                    expression,
                    canonical_id,
                    env_ref,
                    &mut engine,
                ) else {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                    continue;
                };

                let Some(summary) = known_spread_keys_from_type_expr(&ty) else {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                    continue;
                };

                resolved.bindings.attrs.extend(summary.attrs.into_iter());
                resolved
                    .bindings
                    .listeners
                    .extend(summary.listeners.into_iter());
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
        let mut engine = crate::resolver_core::ComponentMetaQueryEngine::new(self);
        if let Some(evaluated) = crate::resolver_core::evaluate_value_expression_via_env_or_dispatch(
            &expression,
            canonical_id,
            eval_env.as_ref(),
            &mut engine,
        ) {
            candidates.extend(collect_dynamic_root_candidates_from_type(
                &evaluated,
                snapshot.imports.as_slice(),
            ));
        }

        candidates.sort_by(|left, right| match (left, right) {
            (
                DynamicRootCandidate::NativeTag { tag: left_tag },
                DynamicRootCandidate::NativeTag { tag: right_tag },
            ) => left_tag.cmp(right_tag),
            (
                DynamicRootCandidate::NativeTag { .. },
                DynamicRootCandidate::ComponentImport { .. },
            ) => std::cmp::Ordering::Less,
            (
                DynamicRootCandidate::ComponentImport { .. },
                DynamicRootCandidate::NativeTag { .. },
            ) => std::cmp::Ordering::Greater,
            (
                DynamicRootCandidate::ComponentImport {
                    component_name: left_name,
                    import_source: left_source,
                    binding_kind: _,
                    imported_name: _,
                },
                DynamicRootCandidate::ComponentImport {
                    component_name: right_name,
                    import_source: right_source,
                    binding_kind: _,
                    imported_name: _,
                },
            ) => (left_name, left_source).cmp(&(right_name, right_source)),
        });
        candidates.dedup();
        candidates
    }

    /// Store fallthrough resolution in the compile cache.
    pub(super) fn cache_fallthrough_result(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_semantic::analysis::type_expr::TypeExpr>,
        >,
        result: &crate::types::FallthroughResolution,
    ) {
        let cache_key = fallthrough_cache_key(
            canonical_id,
            self.config.generic_root_propagation,
            prop_type_overrides,
        );
        let resolution = Arc::new(result.clone());
        self.resolver_runtime().fallthrough.store_node(
            crate::resolver_core::fallthrough_resolver::root_follow_key(
                canonical_id,
                prop_type_overrides
                    .map(crate::resolver_core::hash_prop_type_overrides)
                    .unwrap_or_default(),
                self.config.generic_root_propagation,
            ),
            self.build_runtime_root_follow_node(result),
        );
        self.resolver_runtime()
            .fallthrough
            .store_node(cache_key, self.build_runtime_fallthrough_node(result));
        if prop_type_overrides.is_none() {
            self.mirror_cached_fallthrough_arc(canonical_id, resolution);
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
        members: &[verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember],
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
    ) -> Option<Vec<verter_semantic::analysis::html_intrinsics::OwnedIntrinsicMember>> {
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
    ) {
        // cached_fallthrough lives on DerivedRawState (D48 split).
        {
            if self.effective_file_state(canonical_id, None).is_some() {
                let mut derived_ref = self
                    .derived_raw_cache()
                    .entry(canonical_id.to_string())
                    .or_default();
                derived_ref.value_mut().cached_fallthrough =
                    Some(crate::types::CachedFallthroughEntry {
                        fact_versions: resolution.fact_versions.clone(),
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

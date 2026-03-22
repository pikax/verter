//! `impl VerterHost` — file management, analysis, and diagnostics methods.
//!
//! Contains [`VerterHost::remove`], [`VerterHost::get_analysis`],
//! [`VerterHost::get_diagnostics`], and [`VerterHost::set_import_dependencies`].

use std::sync::Arc;
use std::time::Instant;

use crate::hash::compile_profile_hash;
use crate::id::canonicalize_id;
use crate::shared::{read_lock, write_lock};
use crate::types::*;
use crate::VerterHost;

fn component_meta_debug_enabled() -> bool {
    std::env::var_os("VERTER_COMPONENT_META_DEBUG").is_some()
        || std::env::var_os("VERTER_META_DEBUG").is_some()
}

fn component_meta_debug(message: impl AsRef<str>) {
    if component_meta_debug_enabled() {
        eprintln!("[verter-meta] {}", message.as_ref());
    }
}

fn macro_debug_summary(snapshot: &FileAnalysisSnapshot) -> String {
    snapshot
        .macros
        .iter()
        .map(|mac| {
            format!(
                "{:?}(refs=[{}], props={}, emits={}, slots={})",
                mac.kind,
                mac.type_references.join(","),
                mac.prop_fields.len(),
                mac.emit_fields.len(),
                mac.slot_fields.len(),
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn log_snapshot_debug(
    stage: &str,
    canonical: &str,
    started: Instant,
    snapshot: &FileAnalysisSnapshot,
) {
    component_meta_debug(format!(
        "{stage} {canonical} took {:?} imports={} macro_type_deps={} macros=[{}]",
        started.elapsed(),
        snapshot.imports.len(),
        snapshot.macro_type_deps.len(),
        macro_debug_summary(snapshot),
    ));
}

pub(crate) struct ImportedEvalInputs {
    pub(crate) sources: Vec<String>,
    pub(crate) resolved_types: Vec<verter_analysis::ResolvedLocalType>,
    pub(crate) canonical_dependencies: std::collections::BTreeSet<String>,
}

impl VerterHost {
    fn build_eval_script_source(
        source: &str,
        cached_parse: Option<&verter_core::parser::types::ParsedSfc>,
    ) -> String {
        crate::host_resolve::extract_vue_script_content(source, cached_parse)
            .unwrap_or_else(|| source.to_string())
    }

    fn is_evaluated_types_empty(
        result: &verter_analysis::type_eval_build::EvaluatedComponentTypes,
    ) -> bool {
        result.props.is_empty()
            && result.define_props.is_empty()
            && result.emits.is_empty()
            && result.slot_bindings.is_empty()
            && result.bindings.is_empty()
    }

    fn current_eval_state(
        &self,
        canonical_id: &str,
    ) -> Option<(
        Arc<str>,
        Option<Arc<verter_core::parser::types::ParsedSfc>>,
        Hash16,
    )> {
        #[cfg(feature = "scheduler")]
        {
            let state = self.effective_file_state(canonical_id, None)?;
            Some((state.source, state.cached_parse, state.whole_hash))
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let entry = files.get(canonical_id)?;
            Some((
                Arc::clone(&entry.source),
                entry.cached_parse.clone(),
                entry.whole_hash,
            ))
        }
    }

    pub(crate) fn dependency_resolutions_for_eval(
        &self,
        canonical_id: &str,
    ) -> rustc_hash::FxHashMap<String, DependencyResolution> {
        #[cfg(feature = "scheduler")]
        {
            self.compile_cache
                .get(canonical_id)
                .map(|entry| entry.dependency_resolutions.clone())
                .unwrap_or_default()
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files
                .get(canonical_id)
                .map(|entry| entry.dependency_resolutions.clone())
                .unwrap_or_default()
        }
    }

    fn read_eval_dependency_source_with_fallback(&self, dep_canonical: &str) -> Option<String> {
        let read_candidate = |candidate: &str| -> Option<String> {
            if let Some((source, cached_parse, _)) = self.current_eval_state(candidate) {
                return Some(Self::build_eval_script_source(
                    &source,
                    cached_parse.as_deref(),
                ));
            }

            self.read_dep_source_for_type_resolution(candidate, None)
                .map(|source| Self::build_eval_script_source(&source, None))
        };

        if let Some(source) = read_candidate(dep_canonical) {
            return Some(source);
        }

        let mut candidates = Vec::new();
        if let Some(stem) = dep_canonical.strip_suffix(".js") {
            candidates.push(format!("{stem}.d.ts"));
        }
        if let Some(stem) = dep_canonical.strip_suffix(".jsx") {
            candidates.push(format!("{stem}.d.ts"));
        }
        if let Some(stem) = dep_canonical.strip_suffix(".mjs") {
            candidates.push(format!("{stem}.d.mts"));
        }
        if let Some(stem) = dep_canonical.strip_suffix(".cjs") {
            candidates.push(format!("{stem}.d.cts"));
        }
        candidates.extend([
            format!("{dep_canonical}.d.ts"),
            format!("{dep_canonical}.ts"),
            format!("{dep_canonical}.tsx"),
            format!("{dep_canonical}/index.d.ts"),
            format!("{dep_canonical}/index.ts"),
            format!("{dep_canonical}/index.tsx"),
        ]);

        for candidate in candidates {
            if let Some(source) = read_candidate(&candidate) {
                return Some(source);
            }
        }

        None
    }

    fn collect_eval_dependency_sources_recursive(
        &self,
        canonical_id: &str,
        seen: &mut rustc_hash::FxHashSet<String>,
        out: &mut Vec<String>,
        canonical_dependencies: &mut std::collections::BTreeSet<String>,
    ) {
        for dep in
            Self::resolved_dependency_targets(&self.dependency_resolutions_for_eval(canonical_id))
        {
            if !seen.insert(dep.clone()) {
                continue;
            }

            canonical_dependencies.insert(dep.clone());

            if let Some(source) = self.read_eval_dependency_source_with_fallback(&dep) {
                out.push(source);
            }

            self.collect_eval_dependency_sources_recursive(
                dep.as_str(),
                seen,
                out,
                canonical_dependencies,
            );
        }
    }

    pub(crate) fn imported_eval_inputs(
        &self,
        owner_canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
    ) -> ImportedEvalInputs {
        let mut seen = rustc_hash::FxHashSet::default();
        let mut inputs = Vec::new();
        let mut resolved_type_names = rustc_hash::FxHashSet::default();
        let mut resolved_types = Vec::new();
        let mut canonical_dependencies = std::collections::BTreeSet::new();
        let mut cache = rustc_hash::FxHashMap::default();
        let mut visiting = rustc_hash::FxHashSet::default();

        for dep in snapshot.macro_type_deps.iter() {
            let mut tracked_deps = std::collections::BTreeSet::new();
            let resolved = self.resolve_external_type_from_loaded_files(
                owner_canonical_id,
                &dep.import_source,
                &dep.type_name,
                &mut tracked_deps,
                &mut cache,
                &mut visiting,
                false,
                verter_vfs::ResolveRequestKind::TypeImport,
                false,
                None,
            );
            let mut pushed_tracked_source = false;

            if let Ok(Some(resolved)) = &resolved {
                if resolved_type_names.insert(dep.type_name.clone()) {
                    resolved_types.push(verter_analysis::ResolvedLocalType {
                        name: dep.type_name.clone(),
                        expanded: resolved_elements_to_expanded_text_via_type_text(resolved),
                        span: verter_span::Span::default(),
                    });
                }
            }

            if matches!(resolved, Ok(Some(_)) | Ok(None)) {
                for tracked_dep in tracked_deps {
                    if !seen.insert(tracked_dep.clone()) {
                        continue;
                    }
                    canonical_dependencies.insert(tracked_dep.clone());
                    let Some(source) = self.read_eval_dependency_source_with_fallback(&tracked_dep)
                    else {
                        continue;
                    };
                    pushed_tracked_source = true;
                    inputs.push(source);
                }
            }

            if pushed_tracked_source {
                continue;
            }

            let dep_canonical = if dep.import_source.starts_with('.') {
                self.resolve_loaded_dependency_canonical(
                    owner_canonical_id,
                    &dep.import_source,
                    verter_vfs::ResolveRequestKind::TypeImport,
                )
                .or_else(|| {
                    Some(crate::id::resolve_external(
                        owner_canonical_id,
                        &dep.import_source,
                    ))
                })
            } else if let Some(import) = snapshot
                .imports
                .iter()
                .find(|import| import.source == dep.import_source)
            {
                import.resolved_canonical_id.clone()
            } else if let Some(resolved) = self.resolve_loaded_dependency_canonical(
                owner_canonical_id,
                &dep.import_source,
                verter_vfs::ResolveRequestKind::TypeImport,
            ) {
                Some(resolved)
            } else {
                dep_resolutions
                    .get(&dep.import_source)
                    .and_then(|resolution| resolution.resolved_canonical_id.clone())
            };
            let Some(dep_canonical) = dep_canonical else {
                continue;
            };
            if !seen.insert(dep_canonical.clone()) {
                continue;
            }
            canonical_dependencies.insert(dep_canonical.clone());

            let Some(source) = self.read_eval_dependency_source_with_fallback(&dep_canonical)
            else {
                continue;
            };
            inputs.push(source);
        }

        self.collect_eval_dependency_sources_recursive(
            owner_canonical_id,
            &mut seen,
            &mut inputs,
            &mut canonical_dependencies,
        );

        ImportedEvalInputs {
            sources: inputs,
            resolved_types,
            canonical_dependencies,
        }
    }

    /// Compute evaluated types using pre-computed imported eval inputs.
    /// Avoids redundant `imported_eval_inputs()` calls when the caller
    /// already has them (e.g., `resolve_component_meta`).
    pub(crate) fn compute_evaluated_types_with_inputs(
        &self,
        canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        imported_inputs: &ImportedEvalInputs,
    ) -> Option<verter_analysis::type_eval_build::EvaluatedComponentTypes> {
        let (source, cached_parse, _) = self.current_eval_state(canonical)?;

        let eval_source = Self::build_eval_script_source(&source, cached_parse.as_deref());
        let mut env = verter_analysis::type_eval_build::parse_and_build_env(&eval_source);
        let local_binding_names: rustc_hash::FxHashSet<String> =
            env.value_symbols.keys().cloned().collect();
        for dep_source in &imported_inputs.sources {
            env.extend_missing(verter_analysis::type_eval_build::parse_and_build_env(
                dep_source,
            ));
        }
        for resolved in &imported_inputs.resolved_types {
            if env.type_symbols.contains_key(&resolved.name) {
                continue;
            }
            let body = verter_analysis::type_expr_lower::parse_type_annotation(&resolved.expanded);
            if body.is_unknown() {
                continue;
            }
            env.type_symbols.insert(
                resolved.name.clone(),
                verter_analysis::type_eval::TypeDeclInfo {
                    name: resolved.name.clone(),
                    kind: verter_analysis::type_eval::TypeDeclKind::Alias,
                    type_parameters: Vec::new(),
                    body,
                },
            );
        }

        let result = verter_analysis::type_eval_build::evaluate_macro_types_with_env_and_source_and_local_bindings(
            snapshot.macros.as_ref(),
            &eval_source,
            &mut env,
            &local_binding_names,
        );
        if Self::is_evaluated_types_empty(&result) {
            None
        } else {
            Some(result)
        }
    }

    pub fn evaluate_types(
        &self,
        canonical_or_alias: &str,
    ) -> Option<verter_analysis::type_eval_build::EvaluatedComponentTypes> {
        self.provenance
            .evaluate_types_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let resolved =
            self.resolve_component_meta(canonical_or_alias, crate::types::ResolverMode::Expanded)?;
        resolved.evaluated_types
    }

    /// Single native component-meta query.
    ///
    /// Uses `resolve_component_meta(Expanded)` as the single enrichment owner,
    /// then projects the result through the analysis-owned `extract_component_meta`.
    ///
    /// Returns `None` if the file doesn't exist.
    pub fn get_component_meta(
        &self,
        canonical_or_alias: &str,
    ) -> Option<verter_analysis::component_meta::ComponentMetaAnalysis> {
        self.provenance
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let resolved =
            self.resolve_component_meta(canonical_or_alias, crate::types::ResolverMode::Expanded)?;
        Some(extract_component_meta_from_resolved(
            self,
            canonical_or_alias,
            &resolved,
        ))
    }

    /// Combined query: resolves component-meta once and returns both the
    /// analysis projection and the resolved-meta sidecar. Avoids the
    /// double `resolve_component_meta(Expanded)` that happens if callers
    /// invoke `get_component_meta()` + `resolve_component_meta()` separately.
    pub fn get_component_meta_with_resolution(
        &self,
        canonical_or_alias: &str,
    ) -> Option<(
        verter_analysis::component_meta::ComponentMetaAnalysis,
        crate::meta_resolve::ResolvedComponentMetaState,
    )> {
        self.provenance
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let resolved =
            self.resolve_component_meta(canonical_or_alias, crate::types::ResolverMode::Expanded)?;
        let analysis = extract_component_meta_from_resolved(self, canonical_or_alias, &resolved);
        Some((analysis, resolved))
    }

    /// Resolve the accepted surface for a component's fallthrough inheritance.
    ///
    /// This is an internal method — the host owns all inheritance semantics.
    /// Returns `None` if the file doesn't exist or has no analysis.
    pub fn resolve_fallthrough_surface(
        &self,
        canonical_id: &str,
    ) -> Option<crate::types::FallthroughResolution> {
        let mut visiting = rustc_hash::FxHashSet::default();
        self.resolve_fallthrough_surface_internal(canonical_id, &mut visiting)
    }

    /// Internal recursive method with cycle detection.
    fn resolve_fallthrough_surface_internal(
        &self,
        canonical_id: &str,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Option<crate::types::FallthroughResolution> {
        self.resolve_fallthrough_surface_internal_with_overrides(canonical_id, None, visiting)
    }

    fn resolve_fallthrough_surface_internal_with_overrides(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Option<crate::types::FallthroughResolution> {
        use verter_analysis::component_meta::*;

        // Cycle detection
        if !visiting.insert(canonical_id.to_string()) {
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
            });
        }

        // Check cache first
        if prop_type_overrides.is_none() {
            #[cfg(feature = "scheduler")]
            {
                if let Some(cc) = self.compile_cache.get(canonical_id) {
                    if let Some((cached_hash, cached_generic_mode, ref cached)) =
                        cc.cached_fallthrough
                    {
                        if let Some(state) = self.effective_file_state(canonical_id, None) {
                            if state.whole_hash == cached_hash
                                && cached_generic_mode == self.config.generic_root_propagation
                            {
                                visiting.remove(canonical_id);
                                return Some((**cached).clone());
                            }
                        }
                    }
                }
            }
        }

        // Get the analysis-only meta (without fallthrough populated)
        let resolved =
            self.resolve_component_meta(canonical_id, crate::types::ResolverMode::Expanded)?;

        let resolved_macros = component_meta_resolved_macros(&resolved.resolved_macros);
        let resolved_type_registry = component_meta_type_registry(&resolved.resolved_type_registry);
        let input = ComponentMetaInput {
            macros: &resolved.snapshot.macros,
            bindings: &resolved.snapshot.bindings,
            imports: &resolved.snapshot.imports,
            template: resolved.snapshot.template.as_deref(),
            options_api: resolved.snapshot.options_api.as_ref(),
            analysis_flags: verter_analysis::types::AnalysisFlags::from_bits_truncate(
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
        let base_meta = extract_component_meta(input);

        // Build declared prop/event name sets for subtraction
        let declared_prop_names: rustc_hash::FxHashSet<String> =
            base_meta.props.iter().map(|p| p.name.clone()).collect();
        let declared_event_names: rustc_hash::FxHashSet<String> =
            base_meta.events.iter().map(|e| e.name.clone()).collect();
        let declared_listener_aliases: rustc_hash::FxHashSet<String> = base_meta
            .props
            .iter()
            .filter_map(|p| verter_analysis::html_intrinsics::on_prop_to_event_name(&p.name))
            .collect();

        // Build the accepted props from declared members
        let mut accepted_props: Vec<AcceptedPropAnalysis> = base_meta
            .props
            .iter()
            .map(|p| AcceptedPropAnalysis {
                name: p.name.clone(),
                type_expr: p.type_expr.clone(),
                raw_type: p.raw_type.clone(),
                required: p.required,
                provenance: MemberProvenance::Declared,
                availability: MemberAvailability::Always,
                kind: AcceptedPropKind::DeclaredProp,
            })
            .collect();

        let mut accepted_events: Vec<AcceptedEventAnalysis> = base_meta
            .events
            .iter()
            .map(|e| AcceptedEventAnalysis {
                name: e.name.clone(),
                payload: e.payload.clone(),
                raw_signature: e.raw_signature.clone(),
                provenance: MemberProvenance::Declared,
                availability: MemberAvailability::Always,
                kind: AcceptedEventKind::DeclaredEmit,
            })
            .collect();

        let root_reachability = &base_meta.root_reachability;

        match root_reachability {
            RootReachability::NoFallthrough { reason } => {
                let result = crate::types::FallthroughResolution {
                    accepted_props,
                    accepted_events,
                    accepted_surface_completeness: AcceptedSurfaceCompleteness::Exact,
                    fallthrough_surface: FallthroughSurface::None {
                        reason: reason.clone(),
                    },
                };
                if prop_type_overrides.is_none() {
                    self.cache_fallthrough_result(canonical_id, &result);
                }
                visiting.remove(canonical_id);
                Some(result)
            }
            RootReachability::Branches { branches } => {
                let mut fallthrough_branches = Vec::new();
                let mut any_partial = false;
                let mut any_unresolved = false;
                let mut eval_env = self.build_fallthrough_eval_env(
                    canonical_id,
                    &resolved.snapshot,
                    prop_type_overrides,
                );

                for branch in branches {
                    let branch_key = branch.branch_index.to_string();
                    let element_index = match &branch.target {
                        RootTargetRef::NativeElement { element_index, .. }
                        | RootTargetRef::DynamicComponentUsage { element_index, .. }
                        | RootTargetRef::ComponentUsage { element_index, .. }
                        | RootTargetRef::UnresolvedTarget { element_index, .. } => *element_index,
                    };
                    let resolved_consumed = self.resolve_root_consumption(
                        &resolved.snapshot,
                        element_index,
                        &branch.consumed,
                        branch.has_unknown_spread,
                        &mut eval_env,
                    );
                    let consumed = &resolved_consumed.bindings;
                    let parent_partial_reasons = resolved_consumed.partial_reasons.clone();

                    match &branch.target {
                        RootTargetRef::NativeElement { tag, .. } => {
                            push_native_candidate_branch(
                                self,
                                tag,
                                branch_key,
                                branch.condition_text.clone(),
                                consumed,
                                &parent_partial_reasons,
                                &declared_prop_names,
                                &declared_event_names,
                                &declared_listener_aliases,
                                &mut fallthrough_branches,
                                &mut any_partial,
                            );
                        }

                        RootTargetRef::DynamicComponentUsage { usage_index, .. } => {
                            let child_prop_overrides = self.build_generic_child_prop_overrides(
                                &resolved.snapshot,
                                *usage_index,
                                &mut eval_env,
                            );
                            let candidates = self.resolve_dynamic_root_candidates(
                                &resolved.snapshot,
                                *usage_index,
                                &mut eval_env,
                            );

                            if candidates.is_empty() {
                                any_unresolved = true;
                                fallthrough_branches.push(FallthroughBranch {
                                    branch_key,
                                    condition_text: branch.condition_text.clone(),
                                    props: Vec::new(),
                                    events: Vec::new(),
                                    root_chain: vec![ResolvedRootStep::Unresolved {
                                        tag: "component".to_string(),
                                        reason: UnresolvedBranchReason::DynamicComponentIs,
                                    }],
                                    status: BranchStatus::Unresolved {
                                        reason: UnresolvedBranchReason::DynamicComponentIs,
                                    },
                                });
                                continue;
                            }

                            let multiple_candidates = candidates.len() > 1;
                            for (candidate_index, candidate) in candidates.into_iter().enumerate() {
                                let candidate_key = if multiple_candidates {
                                    format!("{}.{}", branch_key, candidate_index)
                                } else {
                                    branch_key.clone()
                                };
                                match candidate {
                                    DynamicRootCandidate::NativeTag { tag } => {
                                        push_native_candidate_branch(
                                            self,
                                            &tag,
                                            candidate_key,
                                            branch.condition_text.clone(),
                                            consumed,
                                            &parent_partial_reasons,
                                            &declared_prop_names,
                                            &declared_event_names,
                                            &declared_listener_aliases,
                                            &mut fallthrough_branches,
                                            &mut any_partial,
                                        );
                                    }
                                    DynamicRootCandidate::ComponentImport {
                                        component_name,
                                        import_source,
                                    } => {
                                        append_component_candidate_branches(
                                            self,
                                            canonical_id,
                                            &component_name,
                                            &import_source,
                                            candidate_key,
                                            branch.condition_text.clone(),
                                            consumed,
                                            &parent_partial_reasons,
                                            child_prop_overrides.as_ref(),
                                            &declared_prop_names,
                                            &declared_event_names,
                                            &declared_listener_aliases,
                                            &mut fallthrough_branches,
                                            &mut any_partial,
                                            &mut any_unresolved,
                                            visiting,
                                        );
                                    }
                                }
                            }
                        }

                        RootTargetRef::ComponentUsage {
                            usage_index,
                            name,
                            import_source,
                            ..
                        } => {
                            let child_prop_overrides = self.build_generic_child_prop_overrides(
                                &resolved.snapshot,
                                *usage_index,
                                &mut eval_env,
                            );

                            match import_source.as_deref() {
                                Some(import_source) => {
                                    append_component_candidate_branches(
                                        self,
                                        canonical_id,
                                        name,
                                        import_source,
                                        branch_key,
                                        branch.condition_text.clone(),
                                        consumed,
                                        &parent_partial_reasons,
                                        child_prop_overrides.as_ref(),
                                        &declared_prop_names,
                                        &declared_event_names,
                                        &declared_listener_aliases,
                                        &mut fallthrough_branches,
                                        &mut any_partial,
                                        &mut any_unresolved,
                                        visiting,
                                    );
                                }
                                None => {
                                    any_unresolved = true;
                                    fallthrough_branches.push(FallthroughBranch {
                                        branch_key,
                                        condition_text: branch.condition_text.clone(),
                                        props: Vec::new(),
                                        events: Vec::new(),
                                        root_chain: vec![ResolvedRootStep::Unresolved {
                                            tag: name.clone(),
                                            reason: UnresolvedBranchReason::UnresolvedChildImport {
                                                import_source: None,
                                            },
                                        }],
                                        status: BranchStatus::Unresolved {
                                            reason: UnresolvedBranchReason::UnresolvedChildImport {
                                                import_source: None,
                                            },
                                        },
                                    });
                                }
                            }
                        }

                        RootTargetRef::UnresolvedTarget { tag, reason, .. } => {
                            any_unresolved = true;
                            fallthrough_branches.push(FallthroughBranch {
                                branch_key,
                                condition_text: branch.condition_text.clone(),
                                props: Vec::new(),
                                events: Vec::new(),
                                root_chain: vec![ResolvedRootStep::Unresolved {
                                    tag: tag.clone(),
                                    reason: UnresolvedBranchReason::RootTarget {
                                        reason: reason.clone(),
                                    },
                                }],
                                status: BranchStatus::Unresolved {
                                    reason: UnresolvedBranchReason::RootTarget {
                                        reason: reason.clone(),
                                    },
                                },
                            });
                        }
                    }
                }

                fallthrough_branches.sort_by(|a, b| a.branch_key.cmp(&b.branch_key));

                // Build flat accepted projection from branches
                let total_branches = fallthrough_branches.len();
                let force_conditional = any_partial || any_unresolved;

                // Collect inherited members from resolved + partial branches
                let mut inherited_prop_map: rustc_hash::FxHashMap<
                    String,
                    (AcceptedPropAnalysis, Vec<String>),
                > = rustc_hash::FxHashMap::default();
                let mut inherited_event_map: rustc_hash::FxHashMap<
                    String,
                    (AcceptedEventAnalysis, Vec<String>),
                > = rustc_hash::FxHashMap::default();

                for fb in &fallthrough_branches {
                    if matches!(fb.status, BranchStatus::Unresolved { .. }) {
                        continue; // Unresolved branches contribute no inherited members
                    }

                    for fp in &fb.props {
                        let entry =
                            inherited_prop_map
                                .entry(fp.name.clone())
                                .or_insert_with(|| {
                                    (
                                        AcceptedPropAnalysis {
                                            name: fp.name.clone(),
                                            type_expr: fp.type_expr.clone(),
                                            raw_type: fp.raw_type.clone(),
                                            required: false,
                                            provenance: MemberProvenance::Inherited {
                                                sources: fp.sources.clone(),
                                            },
                                            availability: MemberAvailability::Always,
                                            kind: AcceptedPropKind::Attr,
                                        },
                                        Vec::new(),
                                    )
                                });
                        merge_type_expr(&mut entry.0.type_expr, &fp.type_expr);
                        if entry.0.raw_type != fp.raw_type {
                            entry.0.raw_type = None;
                        }
                        if let MemberProvenance::Inherited { sources } = &mut entry.0.provenance {
                            merge_inherited_sources(sources, &fp.sources);
                        }
                        entry.1.push(fb.branch_key.clone());
                    }

                    for fe in &fb.events {
                        let entry =
                            inherited_event_map
                                .entry(fe.name.clone())
                                .or_insert_with(|| {
                                    (
                                        AcceptedEventAnalysis {
                                            name: fe.name.clone(),
                                            payload: fe.payload.clone(),
                                            raw_signature: fe.raw_signature.clone(),
                                            provenance: MemberProvenance::Inherited {
                                                sources: fe.sources.clone(),
                                            },
                                            availability: MemberAvailability::Always,
                                            kind: AcceptedEventKind::Listener,
                                        },
                                        Vec::new(),
                                    )
                                });
                        merge_type_expr(&mut entry.0.payload, &fe.payload);
                        if entry.0.raw_signature != fe.raw_signature {
                            entry.0.raw_signature = None;
                        }
                        if let MemberProvenance::Inherited { sources } = &mut entry.0.provenance {
                            merge_inherited_sources(sources, &fe.sources);
                        }
                        entry.1.push(fb.branch_key.clone());
                    }
                }

                // Compute availability for inherited members
                for (_, (prop, branch_keys)) in inherited_prop_map.iter_mut() {
                    branch_keys.sort();
                    branch_keys.dedup();
                    if force_conditional || branch_keys.len() < total_branches {
                        prop.availability = MemberAvailability::Conditional {
                            branch_keys: branch_keys.clone(),
                        };
                    }
                }
                for (_, (event, branch_keys)) in inherited_event_map.iter_mut() {
                    branch_keys.sort();
                    branch_keys.dedup();
                    if force_conditional || branch_keys.len() < total_branches {
                        event.availability = MemberAvailability::Conditional {
                            branch_keys: branch_keys.clone(),
                        };
                    }
                }

                // Sort inherited members and append after declared
                let mut inherited_props: Vec<AcceptedPropAnalysis> =
                    inherited_prop_map.into_values().map(|(p, _)| p).collect();
                inherited_props.sort_by(|a, b| a.name.cmp(&b.name));
                accepted_props.extend(inherited_props);

                let mut inherited_events: Vec<AcceptedEventAnalysis> =
                    inherited_event_map.into_values().map(|(e, _)| e).collect();
                inherited_events.sort_by(|a, b| a.name.cmp(&b.name));
                accepted_events.extend(inherited_events);

                let completeness = if any_partial || any_unresolved {
                    AcceptedSurfaceCompleteness::LowerBound
                } else {
                    AcceptedSurfaceCompleteness::Exact
                };

                let result = crate::types::FallthroughResolution {
                    accepted_props,
                    accepted_events,
                    accepted_surface_completeness: completeness,
                    fallthrough_surface: FallthroughSurface::Branches {
                        branches: fallthrough_branches,
                    },
                };
                if prop_type_overrides.is_none() {
                    self.cache_fallthrough_result(canonical_id, &result);
                }
                visiting.remove(canonical_id);
                Some(result)
            }
        }
    }

    fn build_fallthrough_eval_env(
        &self,
        canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        prop_type_overrides: Option<
            &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
        >,
    ) -> Option<verter_analysis::type_eval::EvalEnv> {
        let (source, cached_parse, _) = self.current_eval_state(canonical_id)?;
        let eval_source = Self::build_eval_script_source(&source, cached_parse.as_deref());
        let mut env = verter_analysis::type_eval_build::parse_and_build_env(&eval_source);
        let dep_resolutions = self.dependency_resolutions_for_eval(canonical_id);
        let imported_inputs = self.imported_eval_inputs(canonical_id, snapshot, &dep_resolutions);
        for dep_source in &imported_inputs.sources {
            env.extend_missing(verter_analysis::type_eval_build::parse_and_build_env(
                dep_source,
            ));
        }
        if let Some(overrides) = prop_type_overrides {
            inject_prop_type_overrides(&mut env, overrides);
        }
        Some(env)
    }

    fn build_generic_child_prop_overrides(
        &self,
        snapshot: &FileAnalysisSnapshot,
        usage_index: u32,
        eval_env: &mut Option<verter_analysis::type_eval::EvalEnv>,
    ) -> Option<rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>> {
        if !self.config.generic_root_propagation {
            return None;
        }

        let template = snapshot.template.as_deref()?;
        let usage = template.components.get(usage_index as usize)?;
        let mut overrides = rustc_hash::FxHashMap::default();

        for prop in &usage.props {
            if prop.from_spread {
                continue;
            }
            if usage.is_dynamic && prop.name == "is" {
                continue;
            }

            let Some(prop_type) = resolve_usage_prop_type(prop, eval_env) else {
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

    fn resolve_root_consumption(
        &self,
        snapshot: &FileAnalysisSnapshot,
        element_index: u32,
        base: &verter_analysis::component_meta::ConsumedRootBindings,
        has_unknown_spread: bool,
        eval_env: &mut Option<verter_analysis::type_eval::EvalEnv>,
    ) -> ResolvedConsumedBindings {
        use verter_analysis::component_meta::PartialBranchReason;

        let mut resolved = ResolvedConsumedBindings {
            bindings: verter_analysis::component_meta::ConsumedRootBindings {
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

            for directive in spread_directives {
                let Some(expression) = directive.expression.as_deref() else {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                    continue;
                };

                let Some(env) = eval_env.as_mut() else {
                    push_partial_reason(
                        &mut resolved.partial_reasons,
                        PartialBranchReason::UnknownSpread,
                    );
                    continue;
                };

                let Some(ty) =
                    verter_analysis::type_eval_build::evaluate_value_expression(expression, env)
                else {
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

    fn resolve_dynamic_root_candidates(
        &self,
        snapshot: &FileAnalysisSnapshot,
        usage_index: u32,
        eval_env: &mut Option<verter_analysis::type_eval::EvalEnv>,
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
            verter_analysis::type_eval_build::parse_value_expression_type(&expression)
        {
            candidates.extend(collect_dynamic_root_candidates_from_type(
                &lowered, snapshot,
            ));
        }
        if let Some(env) = eval_env.as_mut() {
            if let Some(evaluated) =
                verter_analysis::type_eval_build::evaluate_value_expression(&expression, env)
            {
                candidates.extend(collect_dynamic_root_candidates_from_type(
                    &evaluated, snapshot,
                ));
            }
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
                },
                DynamicRootCandidate::ComponentImport {
                    component_name: right_name,
                    import_source: right_source,
                },
            ) => (left_name, left_source).cmp(&(right_name, right_source)),
        });
        candidates.dedup();
        candidates
    }

    /// Store fallthrough resolution in the compile cache.
    fn cache_fallthrough_result(
        &self,
        canonical_id: &str,
        result: &crate::types::FallthroughResolution,
    ) {
        #[cfg(feature = "scheduler")]
        {
            if let Some(state) = self.effective_file_state(canonical_id, None) {
                let mut cc = self
                    .compile_cache
                    .entry(canonical_id.to_string())
                    .or_default();
                cc.cached_fallthrough = Some((
                    state.whole_hash,
                    self.config.generic_root_propagation,
                    Arc::new(result.clone()),
                ));
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let _ = (canonical_id, result);
        }
    }

    fn parse_dependency_set_for_file(
        &self,
        canonical_id: &str,
    ) -> std::collections::BTreeSet<String> {
        #[cfg(feature = "scheduler")]
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

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let Some(entry) = files.get(canonical_id) else {
                return std::collections::BTreeSet::new();
            };

            entry
                .external_requests
                .iter()
                .map(|r| r.resolved_canonical_id.clone())
                .chain(
                    entry
                        .script_analysis
                        .imports
                        .iter()
                        .filter(|imp| imp.source.starts_with('.'))
                        .map(|imp| crate::id::resolve_external(canonical_id, &imp.source)),
                )
                .collect()
        }
    }

    fn resolved_dependency_targets(
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
    ) -> std::collections::BTreeSet<String> {
        dep_resolutions
            .values()
            .flat_map(|res| {
                res.resolved_canonical_id
                    .iter()
                    .cloned()
                    .chain(res.possible_canonical_ids.iter().cloned())
            })
            .collect()
    }

    pub(crate) fn sync_transitive_macro_type_dependencies(
        &self,
        canonical_id: &str,
        transitive_deps: &std::collections::BTreeSet<String>,
    ) {
        let mut new_deps = self.parse_dependency_set_for_file(canonical_id);

        #[cfg(feature = "scheduler")]
        let old_deps = {
            let mut cc_ref = self
                .compile_cache
                .entry(canonical_id.to_string())
                .or_default();
            let cc = cc_ref.value_mut();
            new_deps.extend(Self::resolved_dependency_targets(
                &cc.dependency_resolutions,
            ));
            new_deps.extend(transitive_deps.iter().cloned());
            let old_deps = cc.dependencies.clone();
            cc.dependencies = new_deps.clone();
            old_deps
        };

        #[cfg(not(feature = "scheduler"))]
        let old_deps = {
            let mut files = write_lock(&self.files);
            let Some(entry) = files.get_mut(canonical_id) else {
                return;
            };
            new_deps.extend(Self::resolved_dependency_targets(
                &entry.dependency_resolutions,
            ));
            new_deps.extend(transitive_deps.iter().cloned());
            let old_deps = entry.dependencies.clone();
            entry.dependencies = new_deps.clone();
            old_deps
        };

        if old_deps != new_deps {
            self.update_reverse_deps(canonical_id, &old_deps, &new_deps);
        }
    }

    /// Returns the original source for a file by canonical ID or alias.
    /// Returns `None` when the file does not exist in the host.
    pub fn get_source(&self, canonical_or_alias: &str) -> Option<std::sync::Arc<str>> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return None;
                }
            }
            self.scheduler
                .try_get_source(&canonical)
                .map(|s| s.source.clone())
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files.get(&canonical).map(|entry| entry.source.clone())
        }
    }

    #[cfg(feature = "scheduler")]
    fn build_template_analysis(
        &self,
        canonical: &str,
        source: &Arc<str>,
        cached_parse: Option<Arc<verter_core::parser::types::ParsedSfc>>,
        src_blocks: &[crate::SrcBlockInfo],
        external_requests: &[crate::ExternalSourceRequest],
        imports: &[verter_analysis::AnalyzedImport],
        macros: &[verter_analysis::AnalyzedMacro],
        bindings: &[verter_analysis::AnalyzedBinding],
    ) -> Option<Arc<verter_analysis::template::TemplateAnalysisSnapshot>> {
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
                    std::borrow::Cow::Owned(verter_core::compile::parse_sfc(
                        &merged_source,
                        None,
                        None,
                    ))
                })
        } else {
            std::borrow::Cow::Owned(verter_core::compile::parse_sfc(&merged_source, None, None))
        };

        let alloc = oxc_allocator::Allocator::new();
        let options = verter_core::compile::CodegenOptions {
            target: verter_core::compile::CompileTarget::META,
            filename: Some(canonical.to_string()),
            ..verter_core::compile::CodegenOptions::default()
        };
        let verter_opts = verter_core::compile::VerterCompileOptions {
            extract_template_data: true,
            ..verter_core::compile::VerterCompileOptions::default()
        };
        let compiled = verter_core::compile::compile_from_parsed(
            &merged_source,
            &parsed,
            &options,
            &verter_opts,
            &alloc,
        );

        let has_structural_errors = compiled.errors.iter().any(|d| {
            matches!(
                d.severity,
                verter_core::compile::CompileDiagnosticSeverity::Error
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
    /// `compile_from_parsed()` — bypassing the host `compile_entry()` which fails
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

        #[cfg(feature = "scheduler")]
        let (source, cached_parse, src_blocks, external_requests) = {
            use crate::host_executor::HostSourceData;
            let Some(snap) = self.scheduler.try_get_source(canonical) else {
                return;
            };
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
        };

        #[cfg(not(feature = "scheduler"))]
        let (source, cached_parse, src_blocks, external_requests) = {
            let files = read_lock(&self.files);
            let Some(entry) = files.get(canonical) else {
                return;
            };
            if entry.file_kind != FileKind::VueSfc {
                return;
            }
            (
                entry.source.clone(),
                entry.cached_parse.clone(),
                entry.src_blocks.clone(),
                entry.external_requests.clone(),
            )
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
                    std::borrow::Cow::Owned(verter_core::compile::parse_sfc(
                        &merged_source,
                        None,
                        None,
                    ))
                })
        } else {
            std::borrow::Cow::Owned(verter_core::compile::parse_sfc(&merged_source, None, None))
        };

        // Compile with META target — script codegen + template data, no JS/TSX output
        let alloc = oxc_allocator::Allocator::new();
        let options = verter_core::compile::CodegenOptions {
            target: verter_core::compile::CompileTarget::META,
            filename: Some(canonical.to_string()),
            ..verter_core::compile::CodegenOptions::default()
        };
        let verter_opts = verter_core::compile::VerterCompileOptions {
            extract_template_data: true,
            ..verter_core::compile::VerterCompileOptions::default()
        };
        let compiled = verter_core::compile::compile_from_parsed(
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
                verter_core::compile::CompileDiagnosticSeverity::Error
            ) && !d.code.starts_with("XInvalidMacroType")
                && !d.code.starts_with("XMissingMacroType")
        });
        if has_structural_errors {
            return;
        }

        // Convert RawTemplateData → TemplateAnalysisSnapshot using existing converter
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
                .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps);
            if let Some(dp) = define_props {
                for field in &dp.prop_fields {
                    if let Some(type_ann) = &field.type_annotation {
                        let classes = verter_analysis::parse_string_literal_union(type_ann);
                        if !classes.is_empty() {
                            unions.push((field.name.clone(), classes));
                        }
                    }
                }
            }
            for binding in &snapshot.bindings {
                if let Some(type_ann) = &binding.type_annotation {
                    let effective =
                        verter_analysis::unwrap_reactive_type(type_ann).unwrap_or(type_ann);
                    let classes = verter_analysis::parse_string_literal_union(effective);
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
                #[cfg(feature = "scheduler")]
                if let Some(mut cc) = self.compile_cache.get_mut(canonical) {
                    cc.raw_template_analysis = Some(tpl_arc);
                }

                #[cfg(not(feature = "scheduler"))]
                {
                    let mut files = write_lock(&self.files);
                    if let Some(entry) = files.get_mut(canonical) {
                        entry.template_analysis = Some(tpl_arc);
                    }
                }
            }
        }
    }

    /// Returns a serializable snapshot of the file's static analysis data.
    /// Returns `None` if the file doesn't exist.
    /// When `eager_analysis` is false, computes analysis on demand from stored source.
    ///
    /// Template analysis is lazily computed via `CompileTarget::META` when the scope
    /// includes template analysis and no prior compilation has populated it.
    ///
    /// Import `resolved_canonical_id` fields are populated lazily using the host's
    /// file map, alias map, and parent dependency set.
    pub fn get_analysis(&self, canonical_or_alias: &str) -> Option<FileAnalysisSnapshot> {
        self.provenance
            .get_analysis_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let analysis_started = component_meta_debug_enabled().then(Instant::now);
        self.get_analysis_snapshot_internal(&canonical, analysis_started)
    }

    fn get_analysis_snapshot_internal(
        &self,
        canonical: &str,
        analysis_started: Option<Instant>,
    ) -> Option<FileAnalysisSnapshot> {
        // Eviction gate (scheduler path)
        #[cfg(feature = "scheduler")]
        {
            if let Some(cc) = self.compile_cache.get(canonical) {
                if cc.evicted {
                    return None;
                }
            }
        }

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;

            let source_snap = self.scheduler.try_get_source(canonical)?;
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
                    .compile_cache
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
                let mut snapshot = FileAnalysisSnapshot {
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
                self.resolve_snapshot_imports(canonical, &mut snapshot);
                self.enrich_destructured_bindings(&mut snapshot);
                if scope.needs_template_analysis() {
                    self.compute_template_analysis_if_missing(canonical, &mut snapshot);
                }
                if let Some(started) = analysis_started {
                    log_snapshot_debug("get_analysis", canonical, started, &snapshot);
                }
                return Some(snapshot);
            }
            drop(source_snap);

            let mut snapshot = self.build_snapshot_from_scheduler(canonical)?;
            self.resolve_snapshot_imports(canonical, &mut snapshot);
            self.enrich_destructured_bindings(&mut snapshot);
            if self.config.effective_scope().needs_template_analysis() {
                self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            }
            if let Some(started) = analysis_started {
                log_snapshot_debug("get_analysis", canonical, started, &snapshot);
            }
            Some(snapshot)
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let entry = files.get(canonical)?;

            let scope = self.config.effective_scope();
            if entry.file_kind == FileKind::VueSfc
                && (!scope.needs_script_analysis() || !scope.needs_style_analysis())
            {
                let source = entry.source.clone();
                let stored_script = entry.script_analysis.clone();
                let stored_styles = Arc::clone(&entry.style_analyses);
                let template = entry.template_analysis.clone();
                let cached_parse = entry.cached_parse.clone();
                let export_sigs = entry.export_signatures.clone();
                drop(files);

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
                            parsed, &source, &canonical,
                        ))
                    } else {
                        Arc::new(crate::parse::build_style_analyses_from_source(
                            &source, &canonical,
                        ))
                    }
                } else {
                    stored_styles
                };
                if !style_analyses.is_empty() && !script_analysis.bindings.is_empty() {
                    script_analysis.mark_bindings_used_in_style(&style_analyses);
                }
                let mut snapshot = FileAnalysisSnapshot {
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
                self.resolve_snapshot_imports(canonical, &mut snapshot);
                self.enrich_destructured_bindings(&mut snapshot);
                if scope.needs_template_analysis() {
                    self.compute_template_analysis_if_missing(canonical, &mut snapshot);
                }
                if let Some(started) = analysis_started {
                    log_snapshot_debug("get_analysis", canonical, started, &snapshot);
                }
                return Some(snapshot);
            }

            let mut snapshot = Self::build_snapshot_from_entry(entry);
            drop(files);
            self.resolve_snapshot_imports(canonical, &mut snapshot);
            self.enrich_destructured_bindings(&mut snapshot);
            if self.config.effective_scope().needs_template_analysis() {
                self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            }
            if let Some(started) = analysis_started {
                log_snapshot_debug("get_analysis", canonical, started, &snapshot);
            }
            Some(snapshot)
        }
    }

    /// Get the current whole_hash for a file.
    pub(crate) fn get_whole_hash(&self, canonical: &str) -> Option<Hash16> {
        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;
            let snap = self.scheduler.try_get_source(canonical)?;
            let hd = snap.downcast_data::<HostSourceData>()?;
            Some(hd.parse.whole_hash)
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files.get(canonical).map(|entry| entry.whole_hash)
        }
    }

    /// Returns the semantic hash for a file by canonical ID or alias.
    ///
    /// The semantic hash changes when the file's semantically significant content
    /// changes (script, template, scoped styles). Returns `None` for missing files.
    pub fn get_semantic_hash(&self, canonical_or_alias: &str) -> Option<Hash16> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;
            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return None;
                }
            }
            let snap = self.scheduler.try_get_source(&canonical)?;
            let hd = snap.downcast_data::<HostSourceData>()?;
            Some(hd.parse.semantic_hash)
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files.get(&canonical).map(|entry| entry.semantic_hash)
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

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::{HostAnalysisData, HostSourceData};
            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return None;
                }
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

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let entry = files.get(&canonical)?;
            if entry.file_kind != FileKind::VueSfc {
                return None;
            }
            Some(CompileBlockersSnapshot {
                external_source_requests: entry.external_requests.clone(),
                macro_type_deps: Arc::clone(&entry.arc_script_cache.macro_type_deps),
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

        #[cfg(feature = "scheduler")]
        {
            for &id in canonical_ids {
                let canonical = self.resolve_alias_or_canonical(id);
                if let Some(cc) = self.compile_cache.get(&canonical) {
                    if cc.evicted {
                        continue;
                    }
                }
                if let Some(snapshot) = self.build_snapshot_from_scheduler(&canonical) {
                    results.push((canonical, snapshot));
                }
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            for &id in canonical_ids {
                let canonical = self.resolve_alias_or_canonical(id);
                if let Some(entry) = files.get(&canonical) {
                    let snapshot = Self::build_snapshot_from_entry(entry);
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
        #[cfg(feature = "scheduler")]
        let mut results = {
            let ids = self.scheduler.node_ids();
            let mut results = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(cc) = self.compile_cache.get(&id) {
                    if cc.evicted {
                        continue;
                    }
                }
                if let Some(snapshot) = self.build_snapshot_from_scheduler(&id) {
                    results.push((id, snapshot));
                }
            }
            results
        };

        #[cfg(not(feature = "scheduler"))]
        let mut results = {
            let files = read_lock(&self.files);
            let mut results = Vec::with_capacity(files.len());
            for (canonical, entry) in files.iter() {
                let snapshot = Self::build_snapshot_from_entry(entry);
                results.push((canonical.clone(), snapshot));
            }
            results
        };

        for (canonical, snapshot) in &mut results {
            self.resolve_snapshot_imports(canonical, snapshot);
            self.enrich_destructured_bindings(snapshot);
        }
        results
    }

    /// Build a `FileAnalysisSnapshot` from a `FileEntry` using Arc::clone
    /// for immutable fields and deep clone for mutable fields (imports, bindings).
    #[cfg(not(feature = "scheduler"))]
    pub(crate) fn build_snapshot_from_entry(entry: &crate::FileEntry) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            imports: entry.script_analysis.imports.clone(),
            bindings: entry.script_analysis.bindings.clone(),
            // Arc::clone — cheap pointer bump, no deep copy
            module_references: Arc::clone(&entry.arc_script_cache.module_references),
            macros: Arc::clone(&entry.arc_script_cache.macros),
            macro_type_deps: Arc::clone(&entry.arc_script_cache.macro_type_deps),
            script_flags: entry.script_analysis.flags.bits(),
            styles: Arc::clone(&entry.style_analyses),
            template: entry.template_analysis.clone(),
            vue_api_calls: Arc::clone(&entry.arc_script_cache.vue_api_calls),
            dom_query_calls: Arc::clone(&entry.arc_script_cache.dom_query_calls),
            css_var_manipulations: Arc::clone(&entry.arc_script_cache.css_var_manipulations),
            script_binding_occurrences: Arc::clone(
                &entry.arc_script_cache.script_binding_occurrences,
            ),
            export_signatures: Arc::new(entry.export_signatures.clone()),
            options_api: entry.script_analysis.options_api.clone(),
            store_usages: Arc::clone(&entry.arc_script_cache.store_usages),
            store_definitions: Arc::clone(&entry.arc_script_cache.store_definitions),
            is_typescript: entry.script_analysis.is_typescript,
        }
    }

    /// Build a `FileAnalysisSnapshot` from scheduler snapshots and compile_cache.
    ///
    /// Reads `HostAnalysisData` for script analysis, export signatures, styles,
    /// and pre-computed `AnalysisArcs`. Template analysis comes from compile_cache
    /// (raw_template_analysis). Uses Arc::clone for all immutable fields.
    #[cfg(feature = "scheduler")]
    pub(crate) fn build_snapshot_from_scheduler(
        &self,
        canonical: &str,
    ) -> Option<FileAnalysisSnapshot> {
        use crate::host_executor::HostAnalysisData;

        let analysis_snap = self.scheduler.try_get_analysis(canonical)?;
        let ad = analysis_snap.downcast_data::<HostAnalysisData>()?;

        let template = self
            .compile_cache
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
        #[cfg(feature = "scheduler")]
        {
            // Try scheduler first (dep may be loaded)
            if let Some(snap) = self.scheduler.try_get_source(resolved_canonical_id) {
                return Some(snap.source.clone());
            }
            // Try VFS resolution fallback (handles aliases like @/... and bare modules)
            let dep_id = self.resolve_loaded_dependency_canonical(
                owner_canonical,
                specifier,
                verter_vfs::ResolveRequestKind::EsmImport,
            );
            if let Some(ref id) = dep_id {
                // File resolved but not yet in scheduler — try loading from disk
                if self.scheduler.try_get_source(id).is_none() {
                    self.ensure_loaded(id);
                }
            }
            dep_id.and_then(|id| self.scheduler.try_get_source(&id).map(|s| s.source.clone()))
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let dep_id = files
                .contains_key(resolved_canonical_id)
                .then(|| resolved_canonical_id.to_string())
                .or_else(|| {
                    self.resolve_loaded_dependency_canonical(
                        owner_canonical,
                        specifier,
                        verter_vfs::ResolveRequestKind::EsmImport,
                    )
                });
            dep_id.and_then(|id| files.get(&id).map(|e| e.source.clone()))
        }
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
                let ctx = verter_vfs::ResolutionContext {
                    phase: verter_vfs::ResolvePhase::CodegenBlocker,
                    kind: if import.is_type_only {
                        verter_vfs::ResolveRequestKind::TypeImport
                    } else {
                        verter_vfs::ResolveRequestKind::EsmImport
                    },
                };
                import.resolved_canonical_id =
                    self.resolve_via_vfs(parent_canonical_id, &import.source, ctx);
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
        use verter_analysis::types::{BindingInitializer, ComposableReturn, ReactivityKind};

        // Build a map of import source → resolved canonical ID from the snapshot
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
            #[cfg(feature = "scheduler")]
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

            #[cfg(not(feature = "scheduler"))]
            let composable_info = {
                let files = read_lock(&self.files);
                files.get(canonical_id).and_then(|entry| {
                    entry
                        .script_analysis
                        .exported_functions
                        .iter()
                        .find(|f| f.name == *callee)
                        .and_then(|f| f.composable.clone())
                })
            };

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

        #[cfg(feature = "scheduler")]
        {
            let cc = self.compile_cache.get(&canonical)?;
            if cc.evicted {
                return None;
            }
            cc.latest_diagnostics.get(&profile_hash).cloned()
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let entry = files.get(&canonical)?;
            entry.latest_diagnostics.get(&profile_hash).cloned()
        }
    }

    /// Returns the monotonic diagnostics generation counter for a file.
    /// Incremented on every write to `latest_diagnostics`. Used by the LSP
    /// cache to detect host-driven recompiles without a document version change.
    pub fn get_diagnostics_generation(&self, canonical_or_alias: &str) -> Option<u64> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            let cc = self.compile_cache.get(&canonical)?;
            if cc.evicted {
                return None;
            }
            Some(cc.diagnostics_generation)
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files.get(&canonical).map(|e| e.diagnostics_generation)
        }
    }

    /// Bump the diagnostics generation counter for a file without changing
    /// its diagnostics.
    pub fn bump_diagnostics_generation(&self, canonical_or_alias: &str) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        if let Some(mut cc) = self.compile_cache.get_mut(&canonical) {
            cc.diagnostics_generation += 1;
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical) {
                entry.diagnostics_generation += 1;
            }
        }
    }

    /// Clear all compile slots for a specific file.
    pub fn invalidate_compile_slots(&self, canonical_or_alias: &str) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        if let Some(mut cc) = self.compile_cache.get_mut(&canonical) {
            cc.compile_slots.clear();
            cc.cached_resolved_meta.clear();
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical) {
                entry.compile_slots.clear();
                entry.cached_resolved_meta.clear();
            }
        }
    }

    /// Invalidate compile outputs of files that depend on the given path.
    ///
    /// Unlike `remove()`, this works even when the dependency file was never
    /// loaded into the host but reverse-dependency edges were registered.
    pub fn invalidate_dependents_of(&self, canonical_or_alias: &str) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        self.smart_invalidate_dependents(&canonical, &[], &[]);
    }

    /// Remove a file from the host, cleaning up aliases, dependencies,
    /// and invalidating compile slots of any dependents.
    pub fn remove(&self, canonical_or_alias: &str) -> Option<HostRemoveResult> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            // Read aliases and dependencies from compile_cache before removing.
            let (aliases, deps) = {
                let cc = self.compile_cache.get(&canonical)?;
                (cc.aliases.clone(), cc.dependencies.clone())
            };

            {
                let mut alias_map = write_lock(&self.alias_to_canonical);
                for alias in &aliases {
                    alias_map.remove(alias);
                }
            }

            let dependents = {
                let rev = read_lock(&self.reverse_dependencies);
                rev.get(&canonical).cloned().unwrap_or_default()
            };

            {
                let mut rev = write_lock(&self.reverse_dependencies);
                for dep in &deps {
                    if let Some(owners) = rev.get_mut(dep) {
                        owners.remove(&canonical);
                        if owners.is_empty() {
                            rev.remove(dep);
                        }
                    }
                }
                rev.remove(&canonical);
            }

            // Invalidate compile_cache slots for dependents.
            for owner in &dependents {
                if let Some(mut cc) = self.compile_cache.get_mut(owner) {
                    cc.compile_slots.clear();
                    cc.cached_resolved_meta.clear();
                }
            }

            self.ws().notify_delete(&canonical);
            self.compile_cache.remove(&canonical);
            self.scheduler.remove(&canonical);

            Some(HostRemoveResult {
                canonical_id: canonical,
            })
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let removed = {
                let mut files = write_lock(&self.files);
                files.remove(&canonical)
            }?;

            {
                let mut alias_map = write_lock(&self.alias_to_canonical);
                for alias in &removed.aliases {
                    alias_map.remove(alias);
                }
            }

            let dependents = {
                let rev = read_lock(&self.reverse_dependencies);
                rev.get(&canonical).cloned().unwrap_or_default()
            };

            {
                let mut rev = write_lock(&self.reverse_dependencies);
                for dep in &removed.dependencies {
                    if let Some(owners) = rev.get_mut(dep) {
                        owners.remove(&canonical);
                        if owners.is_empty() {
                            rev.remove(dep);
                        }
                    }
                }
                rev.remove(&canonical);
            }

            if !dependents.is_empty() {
                let mut files = write_lock(&self.files);
                for owner in &dependents {
                    if let Some(file) = files.get_mut(owner) {
                        file.compile_slots.clear();
                        file.cached_resolved_meta.clear();
                    }
                }
            }

            self.ws().notify_delete(&canonical);

            Some(HostRemoveResult {
                canonical_id: canonical,
            })
        }
    }

    /// Returns the list of virtual node kinds for a file.
    /// Returns an empty vec if the file doesn't exist.
    pub fn list_virtual_nodes(&self, canonical_or_alias: &str) -> Vec<VirtualNodeKind> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;
            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return Vec::new();
                }
            }
            if let Some(snap) = self.scheduler.try_get_source(&canonical) {
                if let Some(hd) = snap.downcast_data::<HostSourceData>() {
                    return hd.parse.meta.virtual_nodes();
                }
            }
            Vec::new()
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files
                .get(&canonical)
                .map(|e| e.all_virtual_nodes())
                .unwrap_or_default()
        }
    }

    /// Provide caller-resolved import dependency resolution records.
    ///
    /// Called after `upsert()` when the caller resolves import specifiers
    /// (tsconfig paths, vite aliases, etc.) using bundler/LSP resolution.
    /// Each record maps a raw import specifier to its resolved canonical ID
    /// (or a list of candidate canonical IDs).
    ///
    /// Records are merged into the file's `dependency_resolutions` map (keyed by
    /// specifier). The flat `dependencies` set is updated in parallel for
    /// reverse-dependency tracking.
    pub fn set_import_dependencies(
        &self,
        canonical_or_alias: &str,
        resolutions: Vec<DependencyResolution>,
    ) {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let parse_deps = self.parse_dependency_set_for_file(&canonical);

        // Build VFS exact resolutions for ALL relevant (phase, kind) contexts.
        let vfs_resolutions: Vec<verter_vfs::ExactResolution> = resolutions
            .iter()
            .flat_map(|r| {
                let resolved = r.resolved_canonical_id.as_ref().map(|id| {
                    let norm = canonicalize_id(id);
                    norm.into_owned()
                });
                let possible: Vec<String> = r
                    .possible_canonical_ids
                    .iter()
                    .map(|c| {
                        let norm = canonicalize_id(c);
                        norm.into_owned()
                    })
                    .collect();
                use verter_vfs::{ResolvePhase as P, ResolveRequestKind as K};
                [
                    (P::CodegenBlocker, K::EsmImport),
                    (P::CodegenBlocker, K::TypeImport),
                    (P::ProviderGraph, K::EsmImport),
                    (P::ProviderGraph, K::TypeImport),
                ]
                .into_iter()
                .map(move |(phase, kind)| verter_vfs::ExactResolution {
                    specifier: r.specifier.clone(),
                    phase,
                    kind,
                    resolved_canonical_id: resolved.clone(),
                    possible_canonical_ids: possible.clone(),
                })
            })
            .collect();

        // Normalize resolutions and persist direct import resolutions.
        let mut dep_resolutions = rustc_hash::FxHashMap::default();
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
            dep_resolutions.insert(res.specifier.clone(), res);
        }

        // Preserve already-discovered transitive macro-type deps; compilation
        // refreshes them, but direct import updates should not discard them.
        #[cfg(feature = "scheduler")]
        let old_transitive_deps = {
            let mut cc_ref = self.compile_cache.entry(canonical.clone()).or_default();
            let cc = cc_ref.value_mut();
            let old_deps = cc.dependencies.clone();
            let old_direct_deps = {
                let mut deps = parse_deps.clone();
                deps.extend(Self::resolved_dependency_targets(
                    &cc.dependency_resolutions,
                ));
                deps
            };
            cc.dependency_resolutions = dep_resolutions.clone();
            old_deps
                .difference(&old_direct_deps)
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
        };
        #[cfg(not(feature = "scheduler"))]
        let old_transitive_deps = {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical) {
                let old_deps = entry.dependencies.clone();
                let old_direct_deps = {
                    let mut deps = parse_deps.clone();
                    deps.extend(Self::resolved_dependency_targets(
                        &entry.dependency_resolutions,
                    ));
                    deps
                };
                entry.dependency_resolutions = dep_resolutions;
                old_deps
                    .difference(&old_direct_deps)
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>()
            } else {
                std::collections::BTreeSet::new()
            }
        };

        self.sync_transitive_macro_type_dependencies(&canonical, &old_transitive_deps);

        // Sync exact resolutions to workspace.
        self.ws().set_exact_resolutions(&canonical, vfs_resolutions);
    }

    /// Returns all known canonical file IDs and their file kinds.
    pub fn list_files(&self) -> Vec<(String, FileKind)> {
        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;
            self.scheduler
                .node_ids()
                .into_iter()
                .filter_map(|id| {
                    if let Some(cc) = self.compile_cache.get(&id) {
                        if cc.evicted {
                            return None;
                        }
                    }
                    let snap = self.scheduler.try_get_source(&id)?;
                    let hd = snap.downcast_data::<HostSourceData>()?;
                    Some((id, hd.file_kind))
                })
                .collect()
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            files
                .iter()
                .map(|(id, entry)| (id.clone(), entry.file_kind))
                .collect()
        }
    }

    pub(crate) fn raw_template_analysis_for_file(
        &self,
        canonical: &str,
    ) -> Option<Arc<verter_analysis::template::TemplateAnalysisSnapshot>> {
        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;
            if let Some(cc) = self.compile_cache.get(canonical) {
                if cc.evicted {
                    return None;
                }
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

        #[cfg(not(feature = "scheduler"))]
        {
            let mut snapshot = {
                let files = read_lock(&self.files);
                let entry = files.get(canonical)?;
                if entry.file_kind != FileKind::VueSfc {
                    return None;
                }
                Self::build_snapshot_from_entry(entry)
            };
            self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            snapshot.template
        }
    }

    #[cfg(feature = "scheduler")]
    fn compute_override_template_analysis(
        &self,
        canonical: &str,
        profile_hash: u64,
    ) -> Option<Arc<verter_analysis::template::TemplateAnalysisSnapshot>> {
        let override_with_parse = {
            let cc = self.compile_cache.get(canonical)?;
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
    ) -> verter_analysis::CssVarFlow {
        #[cfg(feature = "scheduler")]
        let profile_hash = profile.map(compile_profile_hash);
        #[cfg(not(feature = "scheduler"))]
        let _ = profile;

        #[cfg(feature = "scheduler")]
        let canonical_ids: Vec<String> = self
            .scheduler
            .node_ids()
            .into_iter()
            .filter(|id| self.compile_cache.get(id).is_none_or(|cc| !cc.evicted))
            .collect();

        #[cfg(not(feature = "scheduler"))]
        let canonical_ids: Vec<String> = {
            let files = read_lock(&self.files);
            files.keys().cloned().collect()
        };

        let mut flow = verter_analysis::CssVarFlow {
            name: var_name.to_string(),
            ..Default::default()
        };

        for canonical_id in canonical_ids {
            let path: std::sync::Arc<std::path::Path> =
                std::sync::Arc::from(std::path::Path::new(canonical_id.as_str()));

            #[cfg(feature = "scheduler")]
            let style_analyses = self
                .effective_style_analyses(&canonical_id, profile_hash)
                .unwrap_or_default();
            #[cfg(not(feature = "scheduler"))]
            let style_analyses = {
                let files = read_lock(&self.files);
                files
                    .get(&canonical_id)
                    .map(|entry| entry.style_analyses.as_ref().clone())
                    .unwrap_or_default()
            };

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
            #[cfg(feature = "scheduler")]
            let template_analysis = if let Some(profile_hash) = profile_hash {
                self.compile_cache
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
            #[cfg(not(feature = "scheduler"))]
            let template_analysis = self.raw_template_analysis_for_file(&canonical_id);

            if let Some(ref tmpl) = template_analysis {
                if tmpl.css_var_names.iter().any(|n| n == var_name) {
                    flow.template_definitions.push(std::sync::Arc::clone(&path));
                }
            }

            // Check script for DOM API CSS variable manipulations
            #[cfg(feature = "scheduler")]
            let script_has_manipulation = self
                .effective_file_state(&canonical_id, profile_hash)
                .map(|efs| {
                    efs.script_analysis
                        .css_var_manipulations
                        .iter()
                        .any(|m| m.var_name == var_name)
                })
                .unwrap_or(false);
            #[cfg(not(feature = "scheduler"))]
            let script_has_manipulation = {
                let files = read_lock(&self.files);
                files
                    .get(&canonical_id)
                    .map(|entry| {
                        entry
                            .script_analysis
                            .css_var_manipulations
                            .iter()
                            .any(|m| m.var_name == var_name)
                    })
                    .unwrap_or(false)
            };

            if script_has_manipulation {
                flow.script_manipulations.push(std::sync::Arc::clone(&path));
            }
        }

        flow
    }

    /// Look up the byte span of an exported name in a target file.
    ///
    /// For `.vue` files: searches `ScriptAnalysisSnapshot.bindings` (script-setup
    /// auto-exports) — spans are SFC-absolute.
    /// For `.ts`/`.js` files: searches `FileEntry.export_signatures` — spans are
    /// file-absolute.
    ///
    /// Returns `None` if the file doesn't exist or the name isn't exported.
    pub fn get_export_span(
        &self,
        canonical_or_alias: &str,
        binding_name: &str,
    ) -> Option<(u32, u32)> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::{HostAnalysisData, HostSourceData};

            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return None;
                }
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

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let entry = files.get(&canonical)?;
            Self::find_export_span(
                entry.file_kind,
                &entry.script_analysis,
                &entry.export_signatures,
                binding_name,
            )
        }
    }

    /// Shared logic for finding an export span from analysis data.
    fn find_export_span(
        file_kind: FileKind,
        script_analysis: &verter_analysis::ScriptAnalysisSnapshot,
        export_signatures: &[verter_analysis::ExportSignature],
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
            if sig.span.start > 0 || sig.span.end > 0 {
                return Some((sig.span.start, sig.span.end));
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
        #[cfg(feature = "scheduler")]
        if let Some(cc) = self.compile_cache.get(&canonical) {
            if cc.evicted {
                return None;
            }
        }
        let mut visited = rustc_hash::FxHashSet::default();
        self.follow_reexport_chain(&canonical, binding_name, &mut visited)
    }

    /// Internal recursive helper for following re-export chains.
    /// Uses a visited set keyed on `(canonical_id, binding_name)` to detect cycles.
    fn follow_reexport_chain(
        &self,
        canonical_id: &str,
        binding_name: &str,
        visited: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<(String, u32, u32)> {
        if !visited.insert((canonical_id.to_string(), binding_name.to_string())) {
            return None;
        }

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::{HostAnalysisData, HostSourceData};

            let source_snap = self.scheduler.try_get_source(canonical_id)?;
            let hd = source_snap.downcast_data::<HostSourceData>()?;
            let file_kind = hd.file_kind;
            drop(source_snap);

            let analysis_snap = self.scheduler.try_get_analysis(canonical_id)?;
            let ad = analysis_snap.downcast_data::<HostAnalysisData>()?;

            if file_kind == crate::FileKind::VueSfc {
                return Self::find_export_span(
                    file_kind,
                    &ad.script_analysis,
                    &ad.export_signatures,
                    binding_name,
                )
                .map(|(start, end)| (canonical_id.to_string(), start, end));
            }

            if let Some(sig) = ad.export_signatures.iter().find(|s| s.name == binding_name) {
                if let (Some(ref source), Some(ref local_name)) =
                    (&sig.reexport_source, &sig.reexport_local)
                {
                    let resolved_target = {
                        let ctx = verter_vfs::ResolutionContext {
                            phase: verter_vfs::ResolvePhase::ProviderGraph,
                            kind: verter_vfs::ResolveRequestKind::EsmImport,
                        };
                        self.resolve_via_vfs(canonical_id, source, ctx)
                    };
                    if let Some(target_canonical) = resolved_target {
                        return self.follow_reexport_chain(&target_canonical, local_name, visited);
                    }
                    return None;
                }

                if sig.span.start > 0 || sig.span.end > 0 {
                    return Some((canonical_id.to_string(), sig.span.start, sig.span.end));
                }
            }

            None
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let (file_kind, export_signatures) = {
                let files = read_lock(&self.files);
                let entry = files.get(canonical_id)?;
                (entry.file_kind, entry.export_signatures.clone())
            };

            if file_kind == crate::FileKind::VueSfc {
                let files = read_lock(&self.files);
                let entry = files.get(canonical_id)?;
                return Self::find_export_span(
                    entry.file_kind,
                    &entry.script_analysis,
                    &entry.export_signatures,
                    binding_name,
                )
                .map(|(start, end)| (canonical_id.to_string(), start, end));
            }

            if let Some(sig) = export_signatures.iter().find(|s| s.name == binding_name) {
                if let (Some(ref source), Some(ref local_name)) =
                    (&sig.reexport_source, &sig.reexport_local)
                {
                    let resolved_target = {
                        let ctx = verter_vfs::ResolutionContext {
                            phase: verter_vfs::ResolvePhase::ProviderGraph,
                            kind: verter_vfs::ResolveRequestKind::EsmImport,
                        };
                        self.resolve_via_vfs(canonical_id, source, ctx)
                    };
                    if let Some(target_canonical) = resolved_target {
                        return self.follow_reexport_chain(&target_canonical, local_name, visited);
                    }
                    return None;
                }

                if sig.span.start > 0 || sig.span.end > 0 {
                    return Some((canonical_id.to_string(), sig.span.start, sig.span.end));
                }
            }

            None
        }
    }

    /// Resolve an import specifier to its canonical ID using the host's file map,
    /// alias map, and parent's resolved dependencies.
    ///
    /// Returns `None` if the import cannot be resolved to a known file
    /// (e.g., bare specifiers like `vue` or unregistered files).
    pub fn resolve_import(&self, parent_canonical_id: &str, import_source: &str) -> Option<String> {
        let canonical_parent = self.resolve_alias_or_canonical(parent_canonical_id);
        #[cfg(feature = "scheduler")]
        if let Some(cc) = self.compile_cache.get(&canonical_parent) {
            if cc.evicted {
                return None;
            }
        }
        let ctx = verter_vfs::ResolutionContext {
            phase: verter_vfs::ResolvePhase::CodegenBlocker,
            kind: verter_vfs::ResolveRequestKind::EsmImport,
        };
        self.resolve_via_vfs(&canonical_parent, import_source, ctx)
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
        #[cfg(feature = "scheduler")]
        if let Some(cc) = self.compile_cache.get(&canonical) {
            if cc.evicted {
                return Vec::new();
            }
        }
        let mut visiting = rustc_hash::FxHashSet::default();
        self.collect_resolved_exports(&canonical, &mut visiting)
    }

    /// Recursively collect resolved exports from a file, following re-export chains.
    fn collect_resolved_exports(
        &self,
        canonical_id: &str,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Vec<ResolvedExport> {
        if !visiting.insert(canonical_id.to_string()) {
            return Vec::new();
        }

        #[cfg(feature = "scheduler")]
        let (file_kind, export_signatures) = {
            use crate::host_executor::{HostAnalysisData, HostSourceData};

            let source_snap = match self.scheduler.try_get_source(canonical_id) {
                Some(s) => s,
                None => {
                    visiting.remove(canonical_id);
                    return Vec::new();
                }
            };
            let hd = match source_snap.downcast_data::<HostSourceData>() {
                Some(d) => d,
                None => {
                    visiting.remove(canonical_id);
                    return Vec::new();
                }
            };
            let file_kind = hd.file_kind;
            drop(source_snap);

            let sigs = self
                .scheduler
                .try_get_analysis(canonical_id)
                .and_then(|a| {
                    a.downcast_data::<HostAnalysisData>()
                        .map(|ad| ad.export_signatures.clone())
                })
                .unwrap_or_default();
            (file_kind, sigs)
        };

        #[cfg(not(feature = "scheduler"))]
        let (file_kind, export_signatures) = {
            let files = read_lock(&self.files);
            let Some(entry) = files.get(canonical_id) else {
                visiting.remove(canonical_id);
                return Vec::new();
            };
            (entry.file_kind, entry.export_signatures.clone())
        };

        let mut results = Vec::new();

        if file_kind == crate::FileKind::VueSfc {
            results.push(ResolvedExport {
                name: "default".to_string(),
                is_type: false,
                source_canonical_id: None,
                source_name: "default".to_string(),
            });
            visiting.remove(canonical_id);
            return results;
        }

        for sig in &export_signatures {
            if sig.name == "*" {
                if let Some(ref source) = sig.reexport_source {
                    let resolved_target = {
                        let ctx = verter_vfs::ResolutionContext {
                            phase: verter_vfs::ResolvePhase::ProviderGraph,
                            kind: verter_vfs::ResolveRequestKind::EsmImport,
                        };
                        self.resolve_via_vfs(canonical_id, source, ctx)
                    };
                    if let Some(target) = resolved_target {
                        let nested = self.collect_resolved_exports(&target, visiting);
                        for mut export in nested {
                            if export.source_canonical_id.is_none() {
                                export.source_canonical_id = Some(target.clone());
                            }
                            results.push(export);
                        }
                    }
                }
                continue;
            }

            if let (Some(ref source), Some(ref local_name)) =
                (&sig.reexport_source, &sig.reexport_local)
            {
                let resolved_target = {
                    let ctx = verter_vfs::ResolutionContext {
                        phase: verter_vfs::ResolvePhase::ProviderGraph,
                        kind: verter_vfs::ResolveRequestKind::EsmImport,
                    };
                    self.resolve_via_vfs(canonical_id, source, ctx)
                };
                if let Some(target) = resolved_target {
                    let resolved = self.resolve_single_export(&target, local_name, visiting);
                    let (src_id, src_name) = match resolved {
                        Some((cid, n)) => (Some(cid), n),
                        None => (Some(target.clone()), local_name.clone()),
                    };
                    results.push(ResolvedExport {
                        name: sig.name.clone(),
                        is_type: sig.is_type,
                        source_canonical_id: src_id,
                        source_name: src_name,
                    });
                } else {
                    results.push(ResolvedExport {
                        name: sig.name.clone(),
                        is_type: sig.is_type,
                        source_canonical_id: None,
                        source_name: local_name.clone(),
                    });
                }
            } else {
                results.push(ResolvedExport {
                    name: sig.name.clone(),
                    is_type: sig.is_type,
                    source_canonical_id: None,
                    source_name: sig.name.clone(),
                });
            }
        }

        visiting.remove(canonical_id);
        results
    }

    /// Follow a re-export chain for a single named export.
    /// Returns (ultimate_canonical_id, ultimate_name) or None if unresolvable.
    fn resolve_single_export(
        &self,
        canonical_id: &str,
        name: &str,
        visiting: &mut rustc_hash::FxHashSet<String>,
    ) -> Option<(String, String)> {
        #[cfg(feature = "scheduler")]
        let (file_kind, export_signatures) = {
            use crate::host_executor::{HostAnalysisData, HostSourceData};
            let source_snap = self.scheduler.try_get_source(canonical_id)?;
            let hd = source_snap.downcast_data::<HostSourceData>()?;
            let fk = hd.file_kind;
            drop(source_snap);
            let sigs = self
                .scheduler
                .try_get_analysis(canonical_id)
                .and_then(|a| {
                    a.downcast_data::<HostAnalysisData>()
                        .map(|ad| ad.export_signatures.clone())
                })
                .unwrap_or_default();
            (fk, sigs)
        };

        #[cfg(not(feature = "scheduler"))]
        let (file_kind, export_signatures) = {
            let files = read_lock(&self.files);
            let entry = files.get(canonical_id)?;
            (entry.file_kind, entry.export_signatures.clone())
        };

        if file_kind == crate::FileKind::VueSfc {
            return Some((canonical_id.to_string(), name.to_string()));
        }

        let sig = export_signatures.iter().find(|s| s.name == name)?;

        if let (Some(ref source), Some(ref local)) = (&sig.reexport_source, &sig.reexport_local) {
            if visiting.contains(canonical_id) {
                return Some((canonical_id.to_string(), name.to_string()));
            }
            visiting.insert(canonical_id.to_string());
            let target = {
                let ctx = verter_vfs::ResolutionContext {
                    phase: verter_vfs::ResolvePhase::ProviderGraph,
                    kind: verter_vfs::ResolveRequestKind::EsmImport,
                };
                self.resolve_via_vfs(canonical_id, source, ctx)
            };
            visiting.remove(canonical_id);

            if let Some(target_id) = target {
                self.resolve_single_export(&target_id, local, visiting)
                    .or(Some((target_id, local.clone())))
            } else {
                Some((canonical_id.to_string(), name.to_string()))
            }
        } else {
            Some((canonical_id.to_string(), name.to_string()))
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ResolvedConsumedBindings {
    bindings: verter_analysis::component_meta::ConsumedRootBindings,
    partial_reasons: Vec<verter_analysis::component_meta::PartialBranchReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DynamicRootCandidate {
    NativeTag {
        tag: String,
    },
    ComponentImport {
        component_name: String,
        import_source: String,
    },
}

#[derive(Debug, Clone, Default)]
struct KnownSpreadKeys {
    attrs: std::collections::BTreeSet<String>,
    listeners: std::collections::BTreeSet<String>,
    exact: bool,
}

#[allow(clippy::too_many_arguments)]
fn push_native_candidate_branch(
    host: &VerterHost,
    tag: &str,
    branch_key: String,
    condition_text: Option<String>,
    consumed: &verter_analysis::component_meta::ConsumedRootBindings,
    parent_partial_reasons: &[verter_analysis::component_meta::PartialBranchReason],
    declared_prop_names: &rustc_hash::FxHashSet<String>,
    declared_event_names: &rustc_hash::FxHashSet<String>,
    declared_listener_aliases: &rustc_hash::FxHashSet<String>,
    fallthrough_branches: &mut Vec<verter_analysis::component_meta::FallthroughBranch>,
    any_partial: &mut bool,
) {
    use verter_analysis::component_meta::*;

    let intrinsic_members = host.intrinsic_members_for_tag(tag);

    let mut inherited_props = Vec::new();
    let mut inherited_events = Vec::new();

    for member in &intrinsic_members {
        match member.kind {
            verter_analysis::html_intrinsics::IntrinsicMemberKind::Attr => {
                if declared_prop_names.contains(member.name.as_str()) {
                    continue;
                }
                if consumed.attrs.iter().any(|attr| attr == &member.name) {
                    continue;
                }
                inherited_props.push(FallthroughPropEntry {
                    name: member.name.clone(),
                    type_expr: member.type_expr.clone(),
                    raw_type: None,
                    sources: vec![InheritedSource::NativeTag {
                        tag: tag.to_string(),
                    }],
                });
            }
            verter_analysis::html_intrinsics::IntrinsicMemberKind::Listener => {
                if declared_event_names.contains(member.name.as_str())
                    || declared_listener_aliases.contains(member.name.as_str())
                {
                    continue;
                }
                if consumed
                    .listeners
                    .iter()
                    .any(|listener| listener == &member.name)
                {
                    continue;
                }
                inherited_events.push(FallthroughEventEntry {
                    name: member.name.clone(),
                    payload: member.type_expr.clone(),
                    raw_signature: None,
                    sources: vec![InheritedSource::NativeTag {
                        tag: tag.to_string(),
                    }],
                });
            }
        }
    }

    inherited_props.sort_by(|left, right| left.name.cmp(&right.name));
    inherited_events.sort_by(|left, right| left.name.cmp(&right.name));

    let status = if parent_partial_reasons.is_empty() {
        BranchStatus::Resolved
    } else {
        *any_partial = true;
        BranchStatus::PartiallyUnresolved {
            reasons: parent_partial_reasons.to_vec(),
        }
    };

    fallthrough_branches.push(FallthroughBranch {
        branch_key,
        condition_text,
        props: inherited_props,
        events: inherited_events,
        root_chain: vec![ResolvedRootStep::NativeTag {
            tag: tag.to_string(),
        }],
        status,
    });
}

#[allow(clippy::too_many_arguments)]
fn append_component_candidate_branches(
    host: &VerterHost,
    canonical_id: &str,
    component_name: &str,
    import_source: &str,
    branch_key: String,
    condition_text: Option<String>,
    consumed: &verter_analysis::component_meta::ConsumedRootBindings,
    parent_partial_reasons: &[verter_analysis::component_meta::PartialBranchReason],
    child_prop_overrides: Option<
        &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
    >,
    declared_prop_names: &rustc_hash::FxHashSet<String>,
    declared_event_names: &rustc_hash::FxHashSet<String>,
    declared_listener_aliases: &rustc_hash::FxHashSet<String>,
    fallthrough_branches: &mut Vec<verter_analysis::component_meta::FallthroughBranch>,
    any_partial: &mut bool,
    any_unresolved: &mut bool,
    visiting: &mut rustc_hash::FxHashSet<String>,
) {
    use verter_analysis::component_meta::*;

    let child_canonical = host.resolve_loaded_dependency_canonical(
        canonical_id,
        import_source,
        verter_vfs::ResolveRequestKind::EsmImport,
    );

    let Some(child_id) = child_canonical else {
        *any_unresolved = true;
        fallthrough_branches.push(FallthroughBranch {
            branch_key,
            condition_text,
            props: Vec::new(),
            events: Vec::new(),
            root_chain: vec![ResolvedRootStep::Unresolved {
                tag: component_name.to_string(),
                reason: UnresolvedBranchReason::UnresolvedChildImport {
                    import_source: Some(import_source.to_string()),
                },
            }],
            status: BranchStatus::Unresolved {
                reason: UnresolvedBranchReason::UnresolvedChildImport {
                    import_source: Some(import_source.to_string()),
                },
            },
        });
        return;
    };

    let Some(child_resolution) = host.resolve_fallthrough_surface_internal_with_overrides(
        &child_id,
        child_prop_overrides,
        visiting,
    ) else {
        *any_unresolved = true;
        fallthrough_branches.push(FallthroughBranch {
            branch_key,
            condition_text,
            props: Vec::new(),
            events: Vec::new(),
            root_chain: vec![ResolvedRootStep::Component {
                canonical_id: child_id.clone(),
                component_name: component_name.to_string(),
            }],
            status: BranchStatus::Unresolved {
                reason: UnresolvedBranchReason::ChildResolutionFailed,
            },
        });
        return;
    };

    match &child_resolution.fallthrough_surface {
        FallthroughSurface::None { .. } => {
            let mut inherited_props = Vec::new();
            let mut inherited_events = Vec::new();

            for prop in &child_resolution.accepted_props {
                if declared_prop_names.contains(&prop.name) {
                    continue;
                }
                if consumed.attrs.iter().any(|attr| attr == &prop.name) {
                    continue;
                }
                inherited_props.push(FallthroughPropEntry {
                    name: prop.name.clone(),
                    type_expr: prop.type_expr.clone(),
                    raw_type: prop.raw_type.clone(),
                    sources: vec![InheritedSource::Component {
                        canonical_id: child_id.clone(),
                    }],
                });
            }

            for event in &child_resolution.accepted_events {
                if declared_event_names.contains(&event.name)
                    || declared_listener_aliases.contains(&event.name)
                {
                    continue;
                }
                if consumed
                    .listeners
                    .iter()
                    .any(|listener| listener == &event.name)
                {
                    continue;
                }
                inherited_events.push(FallthroughEventEntry {
                    name: event.name.clone(),
                    payload: event.payload.clone(),
                    raw_signature: event.raw_signature.clone(),
                    sources: vec![InheritedSource::Component {
                        canonical_id: child_id.clone(),
                    }],
                });
            }

            inherited_props.sort_by(|left, right| left.name.cmp(&right.name));
            inherited_events.sort_by(|left, right| left.name.cmp(&right.name));

            let status = if parent_partial_reasons.is_empty() {
                BranchStatus::Resolved
            } else {
                *any_partial = true;
                BranchStatus::PartiallyUnresolved {
                    reasons: parent_partial_reasons.to_vec(),
                }
            };

            fallthrough_branches.push(FallthroughBranch {
                branch_key,
                condition_text,
                props: inherited_props,
                events: inherited_events,
                root_chain: vec![ResolvedRootStep::Component {
                    canonical_id: child_id,
                    component_name: component_name.to_string(),
                }],
                status,
            });
        }
        FallthroughSurface::Branches {
            branches: child_branches,
        } => {
            let child_declared_props: Vec<_> = child_resolution
                .accepted_props
                .iter()
                .filter(|prop| matches!(prop.provenance, MemberProvenance::Declared))
                .collect();
            let child_declared_events: Vec<_> = child_resolution
                .accepted_events
                .iter()
                .filter(|event| matches!(event.provenance, MemberProvenance::Declared))
                .collect();

            for child_branch in child_branches {
                let composed_key = format!("{}.{}", branch_key, child_branch.branch_key);

                let mut inherited_props = Vec::new();
                let mut inherited_events = Vec::new();

                for prop in &child_declared_props {
                    if declared_prop_names.contains(&prop.name) {
                        continue;
                    }
                    if consumed.attrs.iter().any(|attr| attr == &prop.name) {
                        continue;
                    }
                    inherited_props.push(FallthroughPropEntry {
                        name: prop.name.clone(),
                        type_expr: prop.type_expr.clone(),
                        raw_type: prop.raw_type.clone(),
                        sources: vec![InheritedSource::Component {
                            canonical_id: child_id.clone(),
                        }],
                    });
                }

                for prop in &child_branch.props {
                    if declared_prop_names.contains(&prop.name) {
                        continue;
                    }
                    if consumed.attrs.iter().any(|attr| attr == &prop.name) {
                        continue;
                    }
                    inherited_props.push(prop.clone());
                }

                for event in &child_declared_events {
                    if declared_event_names.contains(&event.name)
                        || declared_listener_aliases.contains(&event.name)
                    {
                        continue;
                    }
                    if consumed
                        .listeners
                        .iter()
                        .any(|listener| listener == &event.name)
                    {
                        continue;
                    }
                    inherited_events.push(FallthroughEventEntry {
                        name: event.name.clone(),
                        payload: event.payload.clone(),
                        raw_signature: event.raw_signature.clone(),
                        sources: vec![InheritedSource::Component {
                            canonical_id: child_id.clone(),
                        }],
                    });
                }

                for event in &child_branch.events {
                    if declared_event_names.contains(&event.name)
                        || declared_listener_aliases.contains(&event.name)
                    {
                        continue;
                    }
                    if consumed
                        .listeners
                        .iter()
                        .any(|listener| listener == &event.name)
                    {
                        continue;
                    }
                    inherited_events.push(event.clone());
                }

                inherited_props.sort_by(|left, right| left.name.cmp(&right.name));
                inherited_events.sort_by(|left, right| left.name.cmp(&right.name));

                let mut root_chain = vec![ResolvedRootStep::Component {
                    canonical_id: child_id.clone(),
                    component_name: component_name.to_string(),
                }];
                root_chain.extend(child_branch.root_chain.clone());

                let status = match &child_branch.status {
                    BranchStatus::Resolved => {
                        if parent_partial_reasons.is_empty() {
                            BranchStatus::Resolved
                        } else {
                            *any_partial = true;
                            BranchStatus::PartiallyUnresolved {
                                reasons: parent_partial_reasons.to_vec(),
                            }
                        }
                    }
                    BranchStatus::PartiallyUnresolved { reasons } => {
                        *any_partial = true;
                        let mut combined = reasons.clone();
                        combined.extend(parent_partial_reasons.iter().cloned());
                        combined.sort();
                        combined.dedup();
                        BranchStatus::PartiallyUnresolved { reasons: combined }
                    }
                    BranchStatus::Unresolved { reason } => {
                        if !parent_partial_reasons.is_empty() {
                            *any_partial = true;
                        }
                        *any_unresolved = true;
                        BranchStatus::Unresolved {
                            reason: reason.clone(),
                        }
                    }
                };

                fallthrough_branches.push(FallthroughBranch {
                    branch_key: composed_key,
                    condition_text: condition_text.clone(),
                    props: inherited_props,
                    events: inherited_events,
                    root_chain,
                    status,
                });
            }
        }
    }
}

fn inject_prop_type_overrides(
    env: &mut verter_analysis::type_eval::EvalEnv,
    overrides: &rustc_hash::FxHashMap<String, verter_analysis::type_expr::TypeExpr>,
) {
    for (name, ty) in overrides {
        env.value_symbols.insert(
            name.clone(),
            verter_analysis::type_eval::ValueDeclInfo {
                name: name.clone(),
                kind: verter_analysis::type_eval::ValueDeclKind::Const,
                type_annotation: Some(ty.clone()),
                function_signature: None,
                object_shape: None,
            },
        );
    }
}

fn resolve_usage_prop_type(
    prop: &verter_analysis::template::TemplatePropUsage,
    eval_env: &mut Option<verter_analysis::type_eval::EvalEnv>,
) -> Option<verter_analysis::type_expr::TypeExpr> {
    use verter_analysis::type_expr::TypeExpr;

    if prop.from_spread {
        return None;
    }

    if !prop.is_bound {
        return match &prop.expression {
            Some(expression) => Some(TypeExpr::string_literal(expression.clone())),
            None => Some(TypeExpr::boolean_literal(true)),
        };
    }

    if let Some(expression) = &prop.expression {
        if let Some(env) = eval_env.as_mut() {
            if let Some(ty) =
                verter_analysis::type_eval_build::evaluate_value_expression(expression, env)
            {
                return Some(ty);
            }
        }

        if let Some(ty) = verter_analysis::type_eval_build::parse_value_expression_type(expression)
        {
            return Some(ty);
        }
    }

    if prop.is_shorthand {
        if let Some(env) = eval_env.as_mut() {
            if let Some(ty) =
                verter_analysis::type_eval_build::evaluate_value_expression(&prop.name, env)
            {
                return Some(ty);
            }
        }

        if let Some(ty) = verter_analysis::type_eval_build::parse_value_expression_type(&prop.name)
        {
            return Some(ty);
        }
    }

    None
}

fn merge_type_expr(
    existing: &mut verter_analysis::type_expr::TypeExpr,
    incoming: &verter_analysis::type_expr::TypeExpr,
) {
    use verter_analysis::type_expr::TypeExpr;

    if existing == incoming {
        return;
    }

    match existing {
        TypeExpr::Union(types) => {
            if !types.iter().any(|t| t == incoming) {
                types.push(incoming.clone());
            }
        }
        _ => {
            *existing = TypeExpr::union(vec![existing.clone(), incoming.clone()]);
        }
    }
}

fn merge_inherited_sources(
    existing: &mut Vec<verter_analysis::component_meta::InheritedSource>,
    incoming: &[verter_analysis::component_meta::InheritedSource],
) {
    existing.extend(incoming.iter().cloned());
    existing.sort();
    existing.dedup();
}

fn push_partial_reason(
    reasons: &mut Vec<verter_analysis::component_meta::PartialBranchReason>,
    reason: verter_analysis::component_meta::PartialBranchReason,
) {
    if !reasons.iter().any(|existing| existing == &reason) {
        reasons.push(reason);
    }
}

fn normalize_public_spread_key(
    key: &str,
    attrs: &mut std::collections::BTreeSet<String>,
    listeners: &mut std::collections::BTreeSet<String>,
) {
    if key == "class" || key == "style" {
        return;
    }
    if let Some(event_name) = verter_analysis::html_intrinsics::on_prop_to_event_name(key) {
        listeners.insert(event_name.to_string());
    } else {
        attrs.insert(key.to_string());
    }
}

fn known_spread_keys_from_object(
    object: &verter_analysis::type_expr::ObjectExpr,
) -> KnownSpreadKeys {
    let mut result = KnownSpreadKeys {
        exact: true,
        ..KnownSpreadKeys::default()
    };

    for member in &object.properties {
        match member {
            verter_analysis::type_expr::ObjectMember::Property(prop) => {
                normalize_public_spread_key(&prop.name, &mut result.attrs, &mut result.listeners)
            }
            verter_analysis::type_expr::ObjectMember::Method(method) => {
                normalize_public_spread_key(&method.name, &mut result.attrs, &mut result.listeners)
            }
            verter_analysis::type_expr::ObjectMember::IndexSignature(_)
            | verter_analysis::type_expr::ObjectMember::CallSignature(_)
            | verter_analysis::type_expr::ObjectMember::ConstructSignature(_) => {
                result.exact = false;
            }
        }
    }

    result
}

fn intersect_known_spread_keys(
    mut left: KnownSpreadKeys,
    right: KnownSpreadKeys,
) -> KnownSpreadKeys {
    left.attrs = left.attrs.intersection(&right.attrs).cloned().collect();
    left.listeners = left
        .listeners
        .intersection(&right.listeners)
        .cloned()
        .collect();
    left.exact &= right.exact;
    left
}

fn known_spread_keys_from_type_expr(
    ty: &verter_analysis::type_expr::TypeExpr,
) -> Option<KnownSpreadKeys> {
    use verter_analysis::type_expr::TypeExpr;

    match ty {
        TypeExpr::Object(obj) => Some(known_spread_keys_from_object(obj)),
        TypeExpr::Parenthesized(inner) => known_spread_keys_from_type_expr(inner),
        TypeExpr::Intersection(types) => {
            let mut result = KnownSpreadKeys {
                exact: true,
                ..KnownSpreadKeys::default()
            };
            let mut saw_any = false;
            for part in types {
                let Some(summary) = known_spread_keys_from_type_expr(part) else {
                    result.exact = false;
                    continue;
                };
                saw_any = true;
                result.attrs.extend(summary.attrs);
                result.listeners.extend(summary.listeners);
                result.exact &= summary.exact;
            }
            saw_any.then_some(result)
        }
        TypeExpr::Union(types) => {
            let mut iter = types.iter();
            let first = known_spread_keys_from_type_expr(iter.next()?)?;
            let mut result = first.clone();
            let mut exact_same_keys = first.exact;
            for ty in iter {
                let Some(summary) = known_spread_keys_from_type_expr(ty) else {
                    result.exact = false;
                    return Some(result);
                };
                exact_same_keys &= summary.exact
                    && summary.attrs == result.attrs
                    && summary.listeners == result.listeners;
                result = intersect_known_spread_keys(result, summary);
            }
            result.exact = exact_same_keys;
            Some(result)
        }
        _ => None,
    }
}

fn collect_dynamic_root_candidates_from_type(
    ty: &verter_analysis::type_expr::TypeExpr,
    snapshot: &FileAnalysisSnapshot,
) -> Vec<DynamicRootCandidate> {
    use verter_analysis::type_expr::{LiteralValue, TypeExpr};

    match ty {
        TypeExpr::Literal(LiteralValue::String(tag)) => {
            vec![DynamicRootCandidate::NativeTag { tag: tag.clone() }]
        }
        TypeExpr::Union(types) => types
            .iter()
            .flat_map(|branch| collect_dynamic_root_candidates_from_type(branch, snapshot))
            .collect(),
        TypeExpr::Parenthesized(inner) => {
            collect_dynamic_root_candidates_from_type(inner, snapshot)
        }
        TypeExpr::TypeOf(value_ref) if value_ref.path.len() == 1 => snapshot
            .imports
            .iter()
            .filter(|import| !import.is_type_only)
            .find_map(|import| {
                import
                    .bindings
                    .iter()
                    .find(|binding| !binding.is_type_only && binding.name == value_ref.path[0])
                    .map(|_| DynamicRootCandidate::ComponentImport {
                        component_name: value_ref.path[0].clone(),
                        import_source: import.source.clone(),
                    })
            })
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

/// Extract slot bindings from a type_text that encodes a slot's function signature.
///
/// Handles property signature types like `(props: { row: Item; index: number }) => any`.
/// Extract slot bindings and return type from a type_text encoding a slot function signature.
///
/// Handles both arrow-style (`(props: { row: Item }) => VNode[]`) and
/// method-style (`(props: { row: Item }): VNode[]`) signatures.
/// Returns `(bindings, return_type)`.
fn component_meta_resolved_macros(
    resolved_macros: &[crate::meta_resolve::ResolvedMacroMeta],
) -> Vec<verter_analysis::component_meta::ResolvedMacroInput> {
    resolved_macros
        .iter()
        .map(
            |resolved| verter_analysis::component_meta::ResolvedMacroInput {
                macro_index: resolved.macro_index,
                props: resolved.props.clone(),
                emits: resolved.emits.clone(),
                slots: resolved.slots.clone(),
            },
        )
        .collect()
}

fn component_meta_type_registry(
    resolved_type_registry: &[verter_analysis::component_meta::ResolvedTypeAnalysis],
) -> Vec<verter_analysis::component_meta::ResolvedTypeAnalysis> {
    let mut seen = rustc_hash::FxHashSet::default();
    let mut registry = Vec::new();

    for entry in resolved_type_registry {
        if seen.insert(entry.name.clone()) {
            registry.push(entry.clone());
        }
    }

    registry
}

/// Build a `ComponentMetaAnalysis` from a resolved-meta state.
/// Shared by `get_component_meta` and `get_component_meta_with_resolution`.
fn extract_component_meta_from_resolved(
    host: &VerterHost,
    canonical_or_alias: &str,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
) -> verter_analysis::component_meta::ComponentMetaAnalysis {
    let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
    let resolved_macros = component_meta_resolved_macros(&resolved.resolved_macros);
    let resolved_type_registry = component_meta_type_registry(&resolved.resolved_type_registry);
    let input = verter_analysis::component_meta::ComponentMetaInput {
        macros: &resolved.snapshot.macros,
        bindings: &resolved.snapshot.bindings,
        imports: &resolved.snapshot.imports,
        template: resolved.snapshot.template.as_deref(),
        options_api: resolved.snapshot.options_api.as_ref(),
        analysis_flags: verter_analysis::types::AnalysisFlags::from_bits_truncate(
            resolved.snapshot.script_flags,
        ),
        styles: &resolved.snapshot.styles,
        vue_api_calls: &resolved.snapshot.vue_api_calls,
        store_usages: &resolved.snapshot.store_usages,
        resolved_macros: &resolved_macros,
        resolved_type_registry: &resolved_type_registry,
        evaluated_types: resolved.evaluated_types.as_ref(),
        file_path: &canonical,
    };
    let mut meta = verter_analysis::component_meta::extract_component_meta(input);

    // Populate fallthrough surface from host resolver
    if let Some(resolution) = host.resolve_fallthrough_surface(&canonical) {
        meta.accepted_props = resolution.accepted_props;
        meta.accepted_events = resolution.accepted_events;
        meta.accepted_surface_completeness = resolution.accepted_surface_completeness;
        meta.fallthrough_surface = resolution.fallthrough_surface;
    }

    meta
}

pub(crate) fn extract_slot_info_from_type_text(
    type_text: Option<&str>,
) -> (
    Vec<verter_analysis::AnalyzedSlotFieldBinding>,
    Option<String>,
) {
    let Some(text) = type_text else {
        return (Vec::new(), None);
    };

    // Extract return type: text after `=>` (arrow) or after closing `):`  (method).
    let return_type = if let Some(arrow_pos) = text.find("=>") {
        let ret = text[arrow_pos + 2..].trim();
        if !ret.is_empty() {
            Some(ret.to_string())
        } else {
            None
        }
    } else if let Some(colon_pos) = text.rfind("):") {
        let ret = text[colon_pos + 2..].trim();
        if !ret.is_empty() {
            Some(ret.to_string())
        } else {
            None
        }
    } else {
        None
    };

    // Extract bindings from the parameter object type.
    let Some(obj_start) = text.find('{') else {
        return (Vec::new(), return_type);
    };
    let mut depth = 0;
    let mut obj_end = obj_start;
    for (i, ch) in text[obj_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    obj_end = obj_start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return (Vec::new(), return_type);
    }

    let obj_text = &text[obj_start..obj_end];

    // Parse the object literal as a type using verter_core's resolver.
    let alloc = oxc_allocator::Allocator::new();
    let resolved = verter_core::utils::oxc::vue::resolve_type::resolve_external_type(
        "_Bindings",
        &format!("export interface _Bindings {obj_text}"),
        &alloc,
    );

    let Some(resolved) = resolved else {
        return (Vec::new(), return_type);
    };

    let bindings = resolved
        .props
        .iter()
        .filter_map(|p| {
            let name = p.key_name.as_ref()?.clone();
            Some(verter_analysis::AnalyzedSlotFieldBinding {
                name,
                type_annotation: p.type_text.clone(),
                span: verter_span::Span::default(),
            })
        })
        .collect();

    (bindings, return_type)
}

/// Convert `ResolvedElements` props to an expanded type text string
/// using pre-resolved `type_text` (set by `finalize_external_resolution`).
///
/// Preferred over `resolved_elements_to_expanded_text` for cross-file types
/// where spans may reference different source files.
pub(crate) fn resolved_elements_to_expanded_text_via_type_text(
    resolved: &verter_core::utils::oxc::vue::resolve_type::ResolvedElements,
) -> String {
    let mut parts = Vec::new();
    for prop in &resolved.props {
        let name = prop.key_name.as_deref().unwrap_or("unknown");
        let opt = if prop.optional { "?" } else { "" };
        let ty = prop.type_text.as_deref().unwrap_or("unknown");
        parts.push(format!("{}{}: {}", name, opt, ty));
    }
    format!("{{ {} }}", parts.join("; "))
}

#[cfg(test)]
#[path = "host_manage_tests.rs"]
mod tests;

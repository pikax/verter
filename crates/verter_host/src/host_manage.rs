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

struct AnalysisSnapshotState {
    snapshot: FileAnalysisSnapshot,
    reused_enriched_snapshot: bool,
}

struct ResolvedComponentMetaState {
    snapshot: FileAnalysisSnapshot,
    evaluated_types: Option<verter_analysis::type_eval_build::EvaluatedComponentTypes>,
    reused_enriched_snapshot: bool,
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

    fn dependency_resolutions_for_eval(
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
        if let Some(source) = self.read_dep_source_for_type_resolution(dep_canonical, None) {
            return Some(source);
        }

        for candidate in [
            format!("{dep_canonical}.d.ts"),
            format!("{dep_canonical}.ts"),
            format!("{dep_canonical}.tsx"),
            format!("{dep_canonical}/index.d.ts"),
            format!("{dep_canonical}/index.ts"),
            format!("{dep_canonical}/index.tsx"),
        ] {
            if let Some(source) = self.read_dep_source_for_type_resolution(&candidate, None) {
                return Some(source);
            }
        }

        None
    }

    fn imported_eval_inputs(
        &self,
        owner_canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
        dep_resolutions: &rustc_hash::FxHashMap<String, DependencyResolution>,
    ) -> Vec<String> {
        let mut seen = rustc_hash::FxHashSet::default();
        let mut inputs = Vec::new();

        for dep in snapshot.macro_type_deps.iter() {
            let dep_canonical = if dep.import_source.starts_with('.') {
                Some(crate::id::resolve_external(
                    owner_canonical_id,
                    &dep.import_source,
                ))
            } else if let Some(import) = snapshot
                .imports
                .iter()
                .find(|import| import.source == dep.import_source)
            {
                import.resolved_canonical_id.clone()
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

            let Some(source) = self.read_eval_dependency_source_with_fallback(&dep_canonical)
            else {
                continue;
            };
            inputs.push(source);
        }

        inputs
    }

    fn try_get_cached_evaluated_types(
        &self,
        canonical: &str,
        whole_hash: Hash16,
    ) -> Option<Option<verter_analysis::type_eval_build::EvaluatedComponentTypes>> {
        #[cfg(feature = "scheduler")]
        if let Some(entry) = self.compile_cache.get(canonical) {
            if let Some((cached_hash, cached)) = &entry.cached_evaluated_types {
                if *cached_hash == whole_hash {
                    let result = cached.as_ref().clone();
                    return Some(if Self::is_evaluated_types_empty(&result) {
                        None
                    } else {
                        Some(result)
                    });
                }
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            if let Some(entry) = files.get(canonical) {
                if let Some((cached_hash, cached)) = &entry.cached_evaluated_types {
                    if *cached_hash == whole_hash {
                        let result = cached.as_ref().clone();
                        return Some(if Self::is_evaluated_types_empty(&result) {
                            None
                        } else {
                            Some(result)
                        });
                    }
                }
            }
        }

        None
    }

    fn store_cached_evaluated_types(
        &self,
        canonical: &str,
        whole_hash: Hash16,
        result: verter_analysis::type_eval_build::EvaluatedComponentTypes,
    ) -> Option<verter_analysis::type_eval_build::EvaluatedComponentTypes> {
        let cached_result = Arc::new(result.clone());

        #[cfg(feature = "scheduler")]
        if let Some(mut entry) = self.compile_cache.get_mut(canonical) {
            entry.cached_evaluated_types = Some((whole_hash, cached_result));
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(canonical) {
                entry.cached_evaluated_types = Some((whole_hash, cached_result));
            }
        }

        if Self::is_evaluated_types_empty(&result) {
            None
        } else {
            Some(result)
        }
    }

    fn get_or_compute_evaluated_types(
        &self,
        canonical: &str,
        snapshot: &FileAnalysisSnapshot,
    ) -> Option<verter_analysis::type_eval_build::EvaluatedComponentTypes> {
        let (source, cached_parse, whole_hash) = self.current_eval_state(canonical)?;
        if let Some(cached) = self.try_get_cached_evaluated_types(canonical, whole_hash) {
            return cached;
        }

        let eval_source = Self::build_eval_script_source(&source, cached_parse.as_deref());
        let mut env = verter_analysis::type_eval_build::parse_and_build_env(&eval_source);
        let dep_resolutions = self.dependency_resolutions_for_eval(canonical);
        for dep_source in self.imported_eval_inputs(canonical, snapshot, &dep_resolutions) {
            env.extend_missing(verter_analysis::type_eval_build::parse_and_build_env(
                &dep_source,
            ));
        }

        let result = verter_analysis::type_eval_build::evaluate_macro_types_with_env_and_source(
            snapshot.macros.as_ref(),
            &eval_source,
            &mut env,
        );
        self.store_cached_evaluated_types(canonical, whole_hash, result)
    }

    pub fn evaluate_types(
        &self,
        canonical_or_alias: &str,
    ) -> Option<verter_analysis::type_eval_build::EvaluatedComponentTypes> {
        self.provenance
            .evaluate_types_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let state = self.get_resolved_component_meta_state(&canonical)?;
        if state.reused_enriched_snapshot {
            self.provenance
                .evaluate_types_reused_enriched_snapshot
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        state.evaluated_types
    }

    /// Single native component-meta query.
    ///
    /// Combines enriched analysis + type evaluation into one call, using the
    /// shared resolved-state helpers (cached enriched analysis, cached evaluated
    /// types). Does NOT re-enter `get_analysis()` / `evaluate_types()` public APIs
    /// when the enriched snapshot is cached.
    ///
    /// Returns `None` if the file doesn't exist.
    pub fn get_component_meta(
        &self,
        canonical_or_alias: &str,
    ) -> Option<verter_analysis::component_meta::ComponentMetaAnalysis> {
        self.provenance
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let state = self.get_resolved_component_meta_state(&canonical)?;
        let snapshot = state.snapshot;
        let evaluated_types = state.evaluated_types;

        // Build the input view for the analysis-owned extractor.
        let features = verter_analysis::component_meta::ComponentMetaFeatures {
            expanded_types: self.deep_expansion_enabled(),
        };
        let input = verter_analysis::component_meta::ComponentMetaInput {
            macros: &snapshot.macros,
            bindings: &snapshot.bindings,
            imports: &snapshot.imports,
            template: snapshot.template.as_deref(),
            options_api: snapshot.options_api.as_ref(),
            analysis_flags: verter_analysis::types::AnalysisFlags::from_bits_truncate(
                snapshot.script_flags,
            ),
            features,
            styles: &snapshot.styles,
            vue_api_calls: &snapshot.vue_api_calls,
            store_usages: &snapshot.store_usages,
            evaluated_types: evaluated_types.as_ref(),
            file_path: &canonical,
        };

        // Extract component-meta using the analysis-owned pure function.
        Some(verter_analysis::component_meta::extract_component_meta(
            input,
        ))
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

    /// Resolve imported type definitions for a file's macro type dependencies.
    ///
    /// For each `MacroTypeDep` (e.g., `defineProps<ButtonProps>()` importing from `./types`),
    /// resolves the type through the authoritative workspace-aware type-import resolver.
    /// Returns expanded type text suitable for the type registry.
    pub fn resolve_imported_types(
        &self,
        canonical_or_alias: &str,
    ) -> Vec<verter_analysis::ResolvedLocalType> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        // Load macro type deps.
        #[cfg(feature = "scheduler")]
        let macro_type_deps = {
            use crate::host_executor::HostSourceData;

            let source_snap = match self.scheduler.try_get_source(&canonical) {
                Some(s) => s,
                None => return Vec::new(),
            };
            let hd = match source_snap.downcast_data::<HostSourceData>() {
                Some(d) => d,
                None => return Vec::new(),
            };
            let deps = hd.parse.script_analysis.macro_type_deps.clone();
            if deps.is_empty() {
                return Vec::new();
            }
            drop(source_snap);
            deps
        };

        #[cfg(not(feature = "scheduler"))]
        let macro_type_deps = {
            let files = read_lock(&self.files);
            let Some(entry) = files.get(&canonical) else {
                return Vec::new();
            };
            let deps = entry.script_analysis.macro_type_deps.clone();
            if deps.is_empty() {
                return Vec::new();
            }
            deps
        };

        let mut result = Vec::new();
        let mut tracked_deps = std::collections::BTreeSet::new();
        let mut cache = rustc_hash::FxHashMap::default();
        let mut visiting = rustc_hash::FxHashSet::default();

        for dep in macro_type_deps.iter() {
            if let Ok(Some(resolved)) = self.resolve_external_type_from_loaded_files(
                &canonical,
                &dep.import_source,
                &dep.type_name,
                &mut tracked_deps,
                &mut cache,
                &mut visiting,
                false,
                verter_vfs::ResolveRequestKind::TypeImport,
                None,
            ) {
                let expanded = resolved_elements_to_expanded_text_via_type_text(&resolved);
                result.push(verter_analysis::ResolvedLocalType {
                    name: dep.type_name.clone(),
                    expanded,
                    span: verter_span::Span::default(),
                });
            }
        }
        result
    }

    /// Enrich a snapshot's macros with cross-file type resolution results.
    ///
    /// For each `MacroTypeDep`, resolves the imported type through the workspace
    /// (VFS aliases, re-exports, disk reads) and populates the target macro's
    /// `prop_fields`/`emit_fields`/`slot_fields` and `resolved_local_types`.
    ///
    /// Called from `get_analysis()` when `deep_macro_resolution_type` is enabled.
    fn enrich_imported_types(&self, canonical: &str, snapshot: &mut FileAnalysisSnapshot) {
        if snapshot.macro_type_deps.is_empty() {
            return;
        }
        self.provenance
            .get_analysis_deep_enrich_runs
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let debug_enabled = component_meta_debug_enabled();
        let macro_type_deps: Vec<verter_analysis::MacroTypeDep> =
            snapshot.macro_type_deps.iter().cloned().collect();

        if debug_enabled {
            component_meta_debug(format!(
                "enrich_imported_types start {canonical} deps=[{}]",
                macro_type_deps
                    .iter()
                    .map(|dep| format!(
                        "{:?}:{} from {}",
                        dep.macro_kind, dep.type_name, dep.import_source
                    ))
                    .collect::<Vec<_>>()
                    .join("; "),
            ));
        }

        let mut cache = rustc_hash::FxHashMap::default();
        let mut visiting = rustc_hash::FxHashSet::default();
        let kind = verter_vfs::ResolveRequestKind::TypeImport;

        // Collect enrichments first, then apply (avoid borrow issues with Arc::make_mut).
        struct Enrichment {
            type_name: String,
            macro_kind: verter_analysis::AnalyzedMacroKind,
            prop_fields: Vec<verter_analysis::AnalyzedPropField>,
            emit_fields: Vec<verter_analysis::AnalyzedEmitField>,
            slot_fields: Vec<verter_analysis::AnalyzedSlotField>,
            resolved_local_type: verter_analysis::ResolvedLocalType,
        }

        let mut enrichments = Vec::new();
        let mut tracked_deps = std::collections::BTreeSet::new();

        for dep in &macro_type_deps {
            let dep_start = debug_enabled.then(Instant::now);
            let resolved = match self.resolve_external_type_from_loaded_files(
                canonical,
                &dep.import_source,
                &dep.type_name,
                &mut tracked_deps,
                &mut cache,
                &mut visiting,
                false,
                kind,
                None,
            ) {
                Ok(Some(r)) => r,
                Ok(None) => {
                    if let Some(started) = dep_start {
                        component_meta_debug(format!(
                            "enrich_imported_types miss {canonical} {:?}:{} from {} took {:?}",
                            dep.macro_kind,
                            dep.type_name,
                            dep.import_source,
                            started.elapsed(),
                        ));
                    }
                    continue;
                }
                Err(err) => {
                    if let Some(started) = dep_start {
                        component_meta_debug(format!(
                            "enrich_imported_types error {canonical} {:?}:{} from {} took {:?}: {:?}",
                            dep.macro_kind,
                            dep.type_name,
                            dep.import_source,
                            started.elapsed(),
                            err,
                        ));
                    }
                    continue;
                }
            };

            if let Some(started) = dep_start {
                component_meta_debug(format!(
                    "enrich_imported_types resolved {canonical} {:?}:{} from {} took {:?} props={} emits={}",
                    dep.macro_kind,
                    dep.type_name,
                    dep.import_source,
                    started.elapsed(),
                    resolved.props.len(),
                    resolved.emits.len(),
                ));
            }

            // Build expanded type text from resolved props using type_text.
            let expanded = resolved_elements_to_expanded_text_via_type_text(&resolved);

            // Convert resolved elements to analysis fields based on macro kind.
            let mut prop_fields = Vec::new();
            let mut emit_fields = Vec::new();
            let mut slot_fields = Vec::new();

            match dep.macro_kind {
                verter_analysis::AnalyzedMacroKind::DefineProps
                | verter_analysis::AnalyzedMacroKind::WithDefaults
                | verter_analysis::AnalyzedMacroKind::DefineModel => {
                    prop_fields = resolved
                        .props
                        .iter()
                        .map(|p| verter_analysis::AnalyzedPropField {
                            name: p.key_name.clone().unwrap_or_else(|| "unknown".to_string()),
                            is_optional: p.optional,
                            span: verter_span::Span::default(),
                            type_annotation: p.type_text.clone(),
                            description: None,
                            tags: Vec::new(),
                            resolution_source: verter_analysis::TypeResolutionSource::Rust,
                            resolution_error: None,
                        })
                        .collect();
                }
                verter_analysis::AnalyzedMacroKind::DefineEmits => {
                    emit_fields = resolved
                        .emits
                        .iter()
                        .map(|e| {
                            // Wrap call-signature payloads in brackets to match
                            // the local extractor format: `[id: number]`
                            let payload_type = match &e.signature {
                                verter_core::utils::oxc::vue::resolve_type::ResolvedEmitSignature::Call { params_text } => {
                                    if params_text.is_empty() {
                                        None
                                    } else {
                                        Some(format!("[{}]", params_text))
                                    }
                                }
                                verter_core::utils::oxc::vue::resolve_type::ResolvedEmitSignature::Tuple { tuple_text } => {
                                    Some(tuple_text.clone())
                                }
                            };
                            verter_analysis::AnalyzedEmitField {
                                name: e.name.clone(),
                                span: verter_span::Span::default(),
                                payload_type,
                                description: None,
                                tags: Vec::new(),
                            }
                        })
                        .collect();
                }
                verter_analysis::AnalyzedMacroKind::DefineSlots => {
                    // Slots from resolved elements: each prop is a slot name.
                    // Slot bindings come from the prop's type_text which encodes
                    // the function parameter type (e.g., `(props: { row: Item }) => any`).
                    slot_fields = resolved
                        .props
                        .iter()
                        .map(|p| {
                            let name = p.key_name.clone().unwrap_or_else(|| "unknown".to_string());
                            let (bindings, return_type) =
                                extract_slot_info_from_type_text(p.type_text.as_deref());
                            verter_analysis::AnalyzedSlotField {
                                name,
                                is_required: !p.optional,
                                span: verter_span::Span::default(),
                                bindings,
                                return_type,
                                description: None,
                                tags: Vec::new(),
                            }
                        })
                        .collect();
                }
                _ => {}
            }

            enrichments.push(Enrichment {
                type_name: dep.type_name.clone(),
                macro_kind: dep.macro_kind,
                prop_fields,
                emit_fields,
                slot_fields,
                resolved_local_type: verter_analysis::ResolvedLocalType {
                    name: dep.type_name.clone(),
                    expanded,
                    span: verter_span::Span::default(),
                },
            });
        }

        // Resolve nested type references for schema expansion (P1 #4).
        // When resolved props have type_text values that look like type references
        // (not primitives), try to resolve those from the same import source and
        // add them to resolved_local_types. This ensures nested refs like
        // `status: Status` get expanded in the schema registry.
        let mut extra_resolved_types: Vec<verter_analysis::ResolvedLocalType> = Vec::new();
        let primitive_names: rustc_hash::FxHashSet<&str> = [
            "string",
            "number",
            "boolean",
            "symbol",
            "null",
            "undefined",
            "void",
            "any",
            "unknown",
            "never",
            "object",
            "bigint",
        ]
        .into_iter()
        .collect();
        for enrichment in &enrichments {
            let dep = macro_type_deps
                .iter()
                .find(|d| d.type_name == enrichment.type_name);
            let Some(dep) = dep else { continue };

            // Collect type names from prop type_text that might be resolvable types
            for field in &enrichment.prop_fields {
                if let Some(ref type_ann) = field.type_annotation {
                    let trimmed = type_ann.trim();
                    // Simple heuristic: single identifier that starts with uppercase
                    // and isn't already resolved or a primitive
                    if trimmed
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_uppercase())
                        && trimmed.chars().all(|c| c.is_alphanumeric() || c == '_')
                        && !primitive_names.contains(trimmed)
                        && !enrichments.iter().any(|e| e.type_name == trimmed)
                        && !extra_resolved_types.iter().any(|r| r.name == trimmed)
                    {
                        if let Ok(Some(resolved)) = self.resolve_external_type_from_loaded_files(
                            canonical,
                            &dep.import_source,
                            trimmed,
                            &mut tracked_deps,
                            &mut cache,
                            &mut visiting,
                            false,
                            kind,
                            None,
                        ) {
                            let expanded =
                                resolved_elements_to_expanded_text_via_type_text(&resolved);
                            extra_resolved_types.push(verter_analysis::ResolvedLocalType {
                                name: trimmed.to_string(),
                                expanded,
                                span: verter_span::Span::default(),
                            });
                        }
                    }
                }
            }
        }

        if enrichments.is_empty() && extra_resolved_types.is_empty() {
            if debug_enabled {
                component_meta_debug(format!(
                    "enrich_imported_types done {canonical} no enrichments produced"
                ));
            }
            return;
        }

        // Apply enrichments to snapshot macros.
        let macros = Arc::make_mut(&mut snapshot.macros);

        for enrichment in enrichments {
            // Find target macro matching BOTH kind and type_references.
            let target = macros.iter_mut().find(|m| {
                m.kind == enrichment.macro_kind && m.type_references.contains(&enrichment.type_name)
            });
            let Some(target) = target else { continue };

            // MERGE fields into target (not replace). This handles intersection
            // types like `defineProps<Foo & Bar>()` where each dep contributes
            // different props to the same macro.
            match enrichment.macro_kind {
                verter_analysis::AnalyzedMacroKind::DefineProps
                | verter_analysis::AnalyzedMacroKind::WithDefaults
                | verter_analysis::AnalyzedMacroKind::DefineModel => {
                    for field in enrichment.prop_fields {
                        if !target.prop_fields.iter().any(|f| f.name == field.name) {
                            target.prop_fields.push(field);
                        }
                    }
                }
                verter_analysis::AnalyzedMacroKind::DefineEmits => {
                    for field in enrichment.emit_fields {
                        if !target.emit_fields.iter().any(|f| f.name == field.name) {
                            target.emit_fields.push(field);
                        }
                    }
                }
                verter_analysis::AnalyzedMacroKind::DefineSlots => {
                    for field in enrichment.slot_fields {
                        if !target.slot_fields.iter().any(|f| f.name == field.name) {
                            target.slot_fields.push(field);
                        }
                    }
                }
                _ => {}
            }

            // Add to resolved_local_types (dedup by name).
            if !target
                .resolved_local_types
                .iter()
                .any(|r| r.name == enrichment.resolved_local_type.name)
            {
                target
                    .resolved_local_types
                    .push(enrichment.resolved_local_type);
            }
        }

        // Add sibling (nested) resolved types to all macros that had enrichments.
        if !extra_resolved_types.is_empty() {
            for m in macros.iter_mut() {
                if !m.resolved_local_types.is_empty() {
                    for rlt in &extra_resolved_types {
                        if !m.resolved_local_types.iter().any(|r| r.name == rlt.name) {
                            m.resolved_local_types.push(rlt.clone());
                        }
                    }
                }
            }
        }

        if debug_enabled {
            component_meta_debug(format!(
                "enrich_imported_types done {canonical} enrichments={} nested_types={} macros=[{}]",
                macros
                    .iter()
                    .filter(|mac| {
                        !mac.prop_fields.is_empty()
                            || !mac.emit_fields.is_empty()
                            || !mac.slot_fields.is_empty()
                            || !mac.resolved_local_types.is_empty()
                    })
                    .count(),
                extra_resolved_types.len(),
                macro_debug_summary(snapshot),
            ));
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
    fn compute_template_analysis_if_missing(
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
            .map(|state| state.snapshot)
    }

    fn get_analysis_snapshot_internal(
        &self,
        canonical: &str,
        analysis_started: Option<Instant>,
    ) -> Option<AnalysisSnapshotState> {
        // Eviction gate (scheduler path)
        #[cfg(feature = "scheduler")]
        {
            if let Some(cc) = self.compile_cache.get(canonical) {
                if cc.evicted {
                    return None;
                }
            }
        }

        // Enriched-analysis cache: if deep_macro_resolution_type is on and we
        // have a cached enriched snapshot whose hash matches, return it directly.
        if self.deep_expansion_enabled() {
            if let Some(cached) = self.try_get_cached_enriched_analysis(canonical) {
                self.provenance
                    .get_analysis_enriched_cache_hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if let Some(started) = analysis_started {
                    log_snapshot_debug("get_analysis (cached)", canonical, started, &cached);
                }
                return Some(AnalysisSnapshotState {
                    snapshot: cached,
                    reused_enriched_snapshot: true,
                });
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
                if self.deep_expansion_enabled() {
                    self.enrich_imported_types(canonical, &mut snapshot);
                    self.store_cached_enriched_analysis(canonical, &snapshot);
                }
                if let Some(started) = analysis_started {
                    log_snapshot_debug("get_analysis", canonical, started, &snapshot);
                }
                return Some(AnalysisSnapshotState {
                    snapshot,
                    reused_enriched_snapshot: false,
                });
            }
            drop(source_snap);

            let mut snapshot = self.build_snapshot_from_scheduler(canonical)?;
            self.resolve_snapshot_imports(canonical, &mut snapshot);
            self.enrich_destructured_bindings(&mut snapshot);
            if self.config.effective_scope().needs_template_analysis() {
                self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            }
            if self.deep_expansion_enabled() {
                self.enrich_imported_types(canonical, &mut snapshot);
                self.store_cached_enriched_analysis(canonical, &snapshot);
            }
            if let Some(started) = analysis_started {
                log_snapshot_debug("get_analysis", canonical, started, &snapshot);
            }
            Some(AnalysisSnapshotState {
                snapshot,
                reused_enriched_snapshot: false,
            })
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
                if self.deep_expansion_enabled() {
                    self.enrich_imported_types(canonical, &mut snapshot);
                    self.store_cached_enriched_analysis(canonical, &snapshot);
                }
                if let Some(started) = analysis_started {
                    log_snapshot_debug("get_analysis", canonical, started, &snapshot);
                }
                return Some(AnalysisSnapshotState {
                    snapshot,
                    reused_enriched_snapshot: false,
                });
            }

            let mut snapshot = Self::build_snapshot_from_entry(entry);
            drop(files);
            self.resolve_snapshot_imports(canonical, &mut snapshot);
            self.enrich_destructured_bindings(&mut snapshot);
            if self.config.effective_scope().needs_template_analysis() {
                self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            }
            if self.deep_expansion_enabled() {
                self.enrich_imported_types(canonical, &mut snapshot);
                self.store_cached_enriched_analysis(canonical, &snapshot);
            }
            if let Some(started) = analysis_started {
                log_snapshot_debug("get_analysis", canonical, started, &snapshot);
            }
            Some(AnalysisSnapshotState {
                snapshot,
                reused_enriched_snapshot: false,
            })
        }
    }

    fn get_resolved_component_meta_state(
        &self,
        canonical: &str,
    ) -> Option<ResolvedComponentMetaState> {
        let analysis_state = self.get_analysis_snapshot_internal(canonical, None)?;
        if !analysis_state.reused_enriched_snapshot {
            self.provenance
                .component_meta_resolved_state_recomputes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        let evaluated_types = if self.deep_expansion_enabled() {
            self.get_or_compute_evaluated_types(canonical, &analysis_state.snapshot)
        } else {
            None
        };

        Some(ResolvedComponentMetaState {
            snapshot: analysis_state.snapshot,
            evaluated_types,
            reused_enriched_snapshot: analysis_state.reused_enriched_snapshot,
        })
    }

    // -----------------------------------------------------------------------
    // Enriched-analysis cache helpers
    // -----------------------------------------------------------------------

    /// Try to read a cached enriched analysis for the given canonical ID.
    /// Returns `Some(snapshot)` if the cache exists and its hash matches
    /// the current whole_hash. Returns `None` on miss or hash mismatch.
    fn try_get_cached_enriched_analysis(&self, canonical: &str) -> Option<FileAnalysisSnapshot> {
        let whole_hash = self.get_whole_hash(canonical)?;

        #[cfg(feature = "scheduler")]
        {
            let cc = self.compile_cache.get(canonical)?;
            let (cached_hash, cached_snapshot) = cc.cached_enriched_analysis.as_ref()?;
            if *cached_hash == whole_hash {
                return Some(cached_snapshot.as_ref().clone());
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let entry = files.get(canonical)?;
            let (cached_hash, cached_snapshot) = entry.cached_enriched_analysis.as_ref()?;
            if *cached_hash == whole_hash {
                return Some(cached_snapshot.as_ref().clone());
            }
        }

        None
    }

    /// Store the enriched analysis snapshot in the cache.
    fn store_cached_enriched_analysis(&self, canonical: &str, snapshot: &FileAnalysisSnapshot) {
        let Some(whole_hash) = self.get_whole_hash(canonical) else {
            return;
        };
        let cached = Arc::new(snapshot.clone());

        #[cfg(feature = "scheduler")]
        {
            if let Some(mut cc) = self.compile_cache.get_mut(canonical) {
                cc.cached_enriched_analysis = Some((whole_hash, cached));
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(canonical) {
                entry.cached_enriched_analysis = Some((whole_hash, cached));
            }
        }
    }

    /// Get the current whole_hash for a file.
    fn get_whole_hash(&self, canonical: &str) -> Option<Hash16> {
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
    fn build_snapshot_from_entry(entry: &crate::FileEntry) -> FileAnalysisSnapshot {
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
    fn build_snapshot_from_scheduler(&self, canonical: &str) -> Option<FileAnalysisSnapshot> {
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
    fn resolve_snapshot_imports(
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
    fn enrich_destructured_bindings(&self, snapshot: &mut FileAnalysisSnapshot) {
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
            cc.cached_evaluated_types = None;
            cc.cached_enriched_analysis = None;
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical) {
                entry.compile_slots.clear();
                entry.cached_evaluated_types = None;
                entry.cached_enriched_analysis = None;
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
                    cc.cached_evaluated_types = None;
                    cc.cached_enriched_analysis = None;
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
                        file.cached_evaluated_types = None;
                        file.cached_enriched_analysis = None;
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

/// Extract slot bindings from a type_text that encodes a slot's function signature.
///
/// Handles property signature types like `(props: { row: Item; index: number }) => any`.
/// Extract slot bindings and return type from a type_text encoding a slot function signature.
///
/// Handles both arrow-style (`(props: { row: Item }) => VNode[]`) and
/// method-style (`(props: { row: Item }): VNode[]`) signatures.
/// Returns `(bindings, return_type)`.
fn extract_slot_info_from_type_text(
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
fn resolved_elements_to_expanded_text_via_type_text(
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

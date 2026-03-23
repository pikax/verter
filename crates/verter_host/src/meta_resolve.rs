//! Shared materialization and resolved-meta owner for component-meta.
//!
//! This module owns:
//! - mode selection (`ResolverMode::Type` vs `ResolverMode::Expanded`)
//! - materialized resolved outputs (`ResolvedComponentMetaState`)
//! - mode-aware caching
//! - JSDoc attachment and typed-tag resolution
//!
//! It calls into `host_resolve.rs` for declaration traversal — it does NOT
//! replace or duplicate the shared traversal substrate.
//!
//! # Architecture
//!
//! ```text
//! caller → resolve_component_meta(canonical, mode)
//!            ↓
//!        meta_resolve.rs  (orchestration, materialization, caching)
//!            ↓
//!        host_resolve.rs  (declaration graph traversal, shared cache)
//! ```

use crate::host_manage::{
    component_meta_debug, component_meta_debug_enabled, extract_slot_info_from_type_text,
};
use crate::types::{FileAnalysisSnapshot, Hash16, ResolverMode};
use crate::VerterHost;
use std::sync::Arc;
use std::time::Instant;

/// Native declaration kind for the resolved pre-expansion type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedDeclarationKind {
    Interface,
    TypeAlias,
    Class,
    Unknown,
}

/// Native pre-expansion declaration metadata retained by the shared resolver.
#[derive(Debug, Clone)]
pub struct ResolvedTypeDeclaration {
    /// Name requested at the import site.
    pub requested_name: String,
    /// Final declaration name after alias/re-export traversal.
    pub resolved_name: String,
    /// Canonical path of the declaration owner.
    pub canonical_source: String,
    /// Span of the declaration in the source file.
    pub span: verter_span::Span,
    /// Declaration kind in the resolved source file.
    pub kind: ResolvedDeclarationKind,
    /// Best-effort declaration text prior to expansion.
    pub text: Option<String>,
}

/// Host-owned sidecar result for component-meta / analysis enrichment.
///
/// Raw snapshot remains raw — resolved imported metadata lives in this sidecar.
/// `Expanded` mode carries materialized surfaces; `Type` mode carries
/// identity/location only.
#[derive(Debug, Clone)]
pub struct ResolvedComponentMetaState {
    /// The raw analysis snapshot (never mutated for enrichment).
    pub snapshot: FileAnalysisSnapshot,
    /// Which mode was used to produce this state.
    pub mode: ResolverMode,
    /// Content hash of the owner file at resolution time.
    pub whole_hash: Hash16,
    /// Resolved macro metadata from cross-file traversal.
    pub resolved_macros: Vec<ResolvedMacroMeta>,
    /// Resolved type registry entries (populated in `Expanded` mode).
    pub resolved_type_registry: Vec<verter_analysis::component_meta::ResolvedTypeAnalysis>,
    /// Native declaration metadata for each resolved type-registry entry.
    pub resolved_type_registry_meta: Vec<ResolvedTypeRegistryMeta>,
    /// Expanded types (populated in `Expanded` mode only).
    pub evaluated_types: Option<verter_analysis::type_expand::ExpandedComponentTypes>,
    /// Cached imported eval inputs from `resolve_component_meta(Expanded)`.
    /// Threaded through to `build_fallthrough_eval_env_with_inputs` to avoid
    /// a redundant second `imported_eval_inputs()` call in the fallthrough path.
    pub cached_eval_inputs: Option<Arc<crate::host_manage::ImportedEvalInputs>>,
}

/// Native provenance retained for an expanded type-registry entry.
#[derive(Debug, Clone)]
pub struct ResolvedTypeRegistryMeta {
    /// Registry key used by component-meta / compat.
    pub name: String,
    /// Pre-expansion declaration metadata for the source declaration.
    pub declaration: ResolvedTypeDeclaration,
}

/// Resolved metadata for a single macro's cross-file type.
#[derive(Debug, Clone)]
pub struct ResolvedMacroMeta {
    /// Index of the target macro in the raw snapshot.
    pub macro_index: usize,
    /// Which macro kind this resolved metadata belongs to.
    pub macro_kind: verter_analysis::AnalyzedMacroKind,
    /// The type name that was resolved (e.g., "ButtonProps").
    pub type_name: String,
    /// The import specifier (e.g., "./types").
    pub import_source: String,
    /// Pre-expansion declaration metadata for the resolved symbol.
    pub declaration: ResolvedTypeDeclaration,
    /// Native resolved props prior to compat/public filtering.
    pub native_props: Vec<ResolvedNativeProp>,
    /// Resolved prop fields (populated in `Expanded` mode).
    pub props: Vec<verter_analysis::AnalyzedPropField>,
    /// Resolved emit fields (populated in `Expanded` mode).
    pub emits: Vec<verter_analysis::AnalyzedEmitField>,
    /// Resolved slot fields (populated in `Expanded` mode).
    pub slots: Vec<verter_analysis::AnalyzedSlotField>,
    /// Resolved JSDoc block attached to the declaration.
    pub jsdoc: Option<ResolvedJsdocBlock>,
}

/// Native resolved prop metadata retained before compat/public projection.
#[derive(Debug, Clone)]
pub struct ResolvedNativeProp {
    pub name: String,
    pub is_optional: bool,
    pub type_annotation: Option<String>,
    pub visibility: verter_core::utils::oxc::vue::resolve_type::ResolvedMemberVisibility,
    pub span: verter_span::Span,
}

/// Resolved JSDoc block with parsed tags.
#[derive(Debug, Clone)]
pub struct ResolvedJsdocBlock {
    /// The raw description text.
    pub description: Option<String>,
    /// Parsed and optionally type-resolved tags.
    pub tags: Vec<ResolvedJsdocTag>,
}

/// A JSDoc tag with optional type resolution.
#[derive(Debug, Clone)]
pub struct ResolvedJsdocTag {
    /// Tag name (e.g., "param", "type", "returns").
    pub name: String,
    /// Raw text after the tag name.
    pub text: Option<String>,
    /// Raw type expression from braces (e.g., "Foo | Bar" from `{Foo | Bar}`).
    pub raw_type: Option<String>,
    /// Subject name for param-like tags (e.g., "id" from `@param id`).
    pub subject_name: Option<String>,
    /// Expanded type information for typed JSDoc tags in `Expanded` mode.
    pub resolved_type: Option<verter_analysis::type_expr::TypeExpr>,
}

impl VerterHost {
    /// Single host-backed resolver API for cross-file component-meta enrichment.
    ///
    /// This is the ONLY entry point for cross-file component-meta resolution.
    /// Mode is chosen explicitly by callers — never inferred.
    ///
    /// - `Type`: resolves symbol identity, canonical location, and attached JSDoc
    ///   without materializing expanded shapes.
    /// - `Expanded`: resolves the same way, then materializes props/emits/slots,
    ///   populates the type registry, and computes evaluated types.
    pub fn resolve_component_meta(
        &self,
        canonical_or_alias: &str,
        mode: ResolverMode,
    ) -> Option<ResolvedComponentMetaState> {
        let started = component_meta_debug_enabled().then(Instant::now);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let whole_hash = self.get_whole_hash(&canonical).unwrap_or_default();

        if let Some(cached) = self.try_get_cached_resolved_meta(&canonical, whole_hash, mode) {
            if let Some(started) = started {
                component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} cached took {:?}",
                    canonical,
                    mode,
                    started.elapsed(),
                ));
            }
            return Some(cached);
        }

        self.provenance
            .component_meta_resolved_state_recomputes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Step 1: Get the raw analysis snapshot (without enrichment).
        let snapshot = self.get_raw_analysis_snapshot(&canonical)?;

        let mut resolved_macros = Vec::new();
        let mut resolved_type_registry = Vec::new();
        let mut resolved_type_registry_meta = Vec::new();
        let mut seen_registry_names = rustc_hash::FxHashSet::default();
        let mut cache = rustc_hash::FxHashMap::default();
        let mut visiting = rustc_hash::FxHashSet::default();
        let mut tracked_deps = std::collections::BTreeSet::new();
        let kind = verter_vfs::ResolveRequestKind::TypeImport;

        // Step 2: Compute evaluated types first (Expanded mode only).
        // This lets us skip redundant external macro resolution when the
        // analysis-owned extractor can already produce authoritative output from
        // evaluated fields and/or local macro surfaces.
        let (evaluated_types, cached_eval_inputs) = if mode == ResolverMode::Expanded {
            let eval_started = component_meta_debug_enabled().then(Instant::now);
            if component_meta_debug_enabled() {
                component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} step=evaluated_types:start imports={} macro_type_deps={}",
                    canonical,
                    mode,
                    snapshot.imports.len(),
                    snapshot.macro_type_deps.len(),
                ));
            }
            let dep_resolutions = self.dependency_resolutions_for_eval(&canonical);
            let imported_inputs =
                Arc::new(self.imported_eval_inputs(&canonical, &snapshot, &dep_resolutions));
            if component_meta_debug_enabled() {
                component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} step=evaluated_types:imported_inputs_done sources={} type_aliases={} tracked_deps={}",
                    canonical,
                    mode,
                    imported_inputs.sources.len(),
                    imported_inputs.type_aliases.len(),
                    imported_inputs.canonical_dependencies.len(),
                ));
            }
            tracked_deps.extend(imported_inputs.canonical_dependencies.iter().cloned());
            tracked_deps.extend(self.cache_dependency_candidates_from_snapshot(
                &canonical,
                &snapshot,
                &dep_resolutions,
            ));
            let eval_types =
                self.compute_evaluated_types_with_inputs(&canonical, &snapshot, &imported_inputs);
            if let Some(eval_started) = eval_started {
                component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} evaluated_types took {:?} has_output={}",
                    canonical,
                    mode,
                    eval_started.elapsed(),
                    eval_types.as_ref().is_some_and(|types| !types.is_empty()),
                ));
            }
            (eval_types, Some(imported_inputs))
        } else {
            (None, None)
        };

        // Step 3: Resolve only the external macro surfaces that are still needed.
        let macro_resolution_started = component_meta_debug_enabled().then(Instant::now);
        let macro_type_deps: Vec<verter_analysis::MacroTypeDep> =
            snapshot.macro_type_deps.iter().cloned().collect();
        for dep in &macro_type_deps {
            let macro_index = dep.macro_index;
            // Resolve the canonical path of the dependency file.
            let dep_canonical = self
                .resolve_type_dependency_canonical(&canonical, &dep.import_source)
                .unwrap_or_default();
            let declaration =
                resolve_type_declaration(self, &dep_canonical, dep.type_name.as_str());
            let jsdoc = resolve_jsdoc_block(
                self,
                declaration.canonical_source.as_str(),
                declaration.span,
                mode,
                &mut tracked_deps,
                &mut cache,
                &mut visiting,
                kind,
            );

            // Always track the dependency canonical and declaration source
            // so cache invalidation works for both modes.
            if !dep_canonical.is_empty() {
                tracked_deps.insert(dep_canonical.clone());
            }
            if !declaration.canonical_source.is_empty()
                && declaration.canonical_source != dep_canonical
            {
                tracked_deps.insert(declaration.canonical_source.clone());
            }

            match mode {
                ResolverMode::Type => {
                    // Type mode: identity only — skip the expensive traversal.
                    // The shared resolved_type_cache is warmed naturally when
                    // Expanded mode is later called.
                    resolved_macros.push(ResolvedMacroMeta {
                        macro_index,
                        macro_kind: dep.macro_kind,
                        type_name: dep.type_name.clone(),
                        import_source: dep.import_source.clone(),
                        declaration: declaration.clone(),
                        native_props: Vec::new(),
                        props: Vec::new(),
                        emits: Vec::new(),
                        slots: Vec::new(),
                        jsdoc: jsdoc.clone(),
                    });
                }
                ResolverMode::Expanded => {
                    let skip_external = should_ignore_external_macro_type(dep);
                    if skip_external {
                        resolved_macros.push(ResolvedMacroMeta {
                            macro_index,
                            macro_kind: dep.macro_kind,
                            type_name: dep.type_name.clone(),
                            import_source: dep.import_source.clone(),
                            declaration: declaration.clone(),
                            native_props: Vec::new(),
                            props: Vec::new(),
                            emits: Vec::new(),
                            slots: Vec::new(),
                            jsdoc: jsdoc.clone(),
                        });
                        continue;
                    }

                    let mut resolution_deps = std::collections::BTreeSet::new();
                    let resolved = self.resolve_external_type_from_loaded_files(
                        &canonical,
                        &dep.import_source,
                        &dep.type_name,
                        &mut tracked_deps,
                        &mut resolution_deps,
                        &mut cache,
                        &mut visiting,
                        false,
                        kind,
                        true,
                        None,
                        0,
                    );

                    match resolved {
                        Ok(Some(elements)) => {
                            let native_props = collect_native_props(&elements);
                            let (props, emits, slots) = materialize_surfaces(
                                self,
                                declaration.canonical_source.as_str(),
                                &dep.macro_kind,
                                &elements,
                            );
                            if seen_registry_names.insert(dep.type_name.clone()) {
                                resolved_type_registry.push(
                                    verter_analysis::component_meta::ResolvedTypeAnalysis {
                                        name: dep.type_name.clone(),
                                        type_expr: crate::host_manage::resolved_elements_to_type_expr_via_type_text(&elements),
                                        type_expansion: None,
                                    },
                                );
                                resolved_type_registry_meta.push(ResolvedTypeRegistryMeta {
                                    name: dep.type_name.clone(),
                                    declaration: declaration.clone(),
                                });
                            }

                            resolved_macros.push(ResolvedMacroMeta {
                                macro_index,
                                macro_kind: dep.macro_kind,
                                type_name: dep.type_name.clone(),
                                import_source: dep.import_source.clone(),
                                declaration: declaration.clone(),
                                native_props,
                                props,
                                emits,
                                slots,
                                jsdoc: jsdoc.clone(),
                            });
                        }
                        Ok(None) | Err(_) => {
                            // Best-effort: record identity even on resolution failure.
                            resolved_macros.push(ResolvedMacroMeta {
                                macro_index,
                                macro_kind: dep.macro_kind,
                                type_name: dep.type_name.clone(),
                                import_source: dep.import_source.clone(),
                                declaration: declaration.clone(),
                                native_props: Vec::new(),
                                props: Vec::new(),
                                emits: Vec::new(),
                                slots: Vec::new(),
                                jsdoc,
                            });
                        }
                    }
                }
            }
        }
        if let Some(macro_resolution_started) = macro_resolution_started {
            component_meta_debug(format!(
                "resolve_component_meta owner={} mode={:?} macro_resolution deps={} took {:?}",
                canonical,
                mode,
                macro_type_deps.len(),
                macro_resolution_started.elapsed(),
            ));
        }

        if mode == ResolverMode::Expanded {
            for mac in snapshot.macros.iter() {
                for resolved in &mac.resolved_local_types {
                    if seen_registry_names.insert(resolved.name.clone()) {
                        resolved_type_registry.push(
                            verter_analysis::component_meta::ResolvedTypeAnalysis {
                                name: resolved.name.clone(),
                                type_expr: resolved.type_expr.clone().unwrap_or_else(|| {
                                    verter_analysis::type_expr_lower::parse_type_annotation(
                                        &resolved.expanded,
                                    )
                                }),
                                type_expansion: None,
                            },
                        );
                        resolved_type_registry_meta.push(ResolvedTypeRegistryMeta {
                            name: resolved.name.clone(),
                            declaration: resolve_local_type_declaration(
                                self,
                                canonical.as_str(),
                                resolved,
                            ),
                        });
                    }
                }
            }
        }

        // Sync transitive macro type dependencies for invalidation tracking.
        self.sync_transitive_macro_type_dependencies(&canonical, &tracked_deps);

        let dependency_hashes = self.current_dependency_hashes(&tracked_deps);
        let state = ResolvedComponentMetaState {
            snapshot,
            mode,
            whole_hash,
            resolved_macros,
            resolved_type_registry,
            resolved_type_registry_meta,
            evaluated_types,
            cached_eval_inputs,
        };
        self.store_cached_resolved_meta(&canonical, mode, &state, &dependency_hashes);
        if let Some(started) = started {
            component_meta_debug(format!(
                "resolve_component_meta owner={} mode={:?} total took {:?}",
                canonical,
                mode,
                started.elapsed(),
            ));
        }
        Some(state)
    }

    /// Get a raw analysis snapshot without any enrichment.
    ///
    /// This bypasses any legacy `get_analysis()` enrichment path, returning only the base snapshot
    /// with resolved imports and destructured bindings.
    pub(crate) fn get_raw_analysis_snapshot(
        &self,
        canonical: &str,
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
            let mut snapshot = self.build_snapshot_from_scheduler(canonical)?;
            self.resolve_snapshot_imports(canonical, &mut snapshot);
            self.enrich_destructured_bindings(&mut snapshot);
            if self.config.effective_scope().needs_template_analysis() {
                self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            }
            Some(snapshot)
        }

        #[cfg(not(feature = "scheduler"))]
        {
            use crate::shared::read_lock;

            let files = read_lock(&self.files);
            let entry = files.get(canonical)?;
            // Use build_snapshot_from_entry for Arc::clone pointer bumps
            // instead of allocating new Arcs.
            let mut snapshot = Self::build_snapshot_from_entry(entry);
            drop(files);
            self.resolve_snapshot_imports(canonical, &mut snapshot);
            self.enrich_destructured_bindings(&mut snapshot);
            if self.config.effective_scope().needs_template_analysis() {
                self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            }
            Some(snapshot)
        }
    }

    fn try_get_cached_resolved_meta(
        &self,
        canonical: &str,
        whole_hash: Hash16,
        mode: ResolverMode,
    ) -> Option<ResolvedComponentMetaState> {
        #[cfg(feature = "scheduler")]
        {
            let entry = self.compile_cache.get(canonical)?;
            let cached = entry.cached_resolved_meta.get(&mode)?;
            if cached.owner_whole_hash != whole_hash {
                return None;
            }
            if !self.dependency_hashes_match(&cached.dependency_hashes) {
                return None;
            }
            Some(cached.state.as_ref().clone())
        }

        #[cfg(not(feature = "scheduler"))]
        {
            use crate::shared::read_lock;

            let files = read_lock(&self.files);
            let entry = files.get(canonical)?;
            let cached = entry.cached_resolved_meta.get(&mode)?;
            if cached.owner_whole_hash != whole_hash {
                return None;
            }
            if !self.dependency_hashes_match(&cached.dependency_hashes) {
                return None;
            }
            Some(cached.state.as_ref().clone())
        }
    }

    fn store_cached_resolved_meta(
        &self,
        canonical: &str,
        mode: ResolverMode,
        state: &ResolvedComponentMetaState,
        dependency_hashes: &[(String, Hash16)],
    ) {
        let cached = crate::types::ResolvedComponentMetaCacheEntry {
            owner_whole_hash: state.whole_hash,
            dependency_hashes: dependency_hashes.to_vec(),
            state: Arc::new(state.clone()),
        };

        #[cfg(feature = "scheduler")]
        {
            if let Some(mut entry) = self.compile_cache.get_mut(canonical) {
                entry.cached_resolved_meta.insert(mode, cached);
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            use crate::shared::write_lock;

            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(canonical) {
                entry.cached_resolved_meta.insert(mode, cached);
            }
        }
    }

    fn current_dependency_hashes(
        &self,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) -> Vec<(String, Hash16)> {
        tracked_deps
            .iter()
            .map(|dep| {
                (
                    dep.clone(),
                    self.get_whole_hash(dep.as_str()).unwrap_or_default(),
                )
            })
            .collect()
    }

    fn dependency_hashes_match(&self, dependency_hashes: &[(String, Hash16)]) -> bool {
        dependency_hashes.iter().all(|(canonical, expected_hash)| {
            self.get_whole_hash(canonical.as_str()).unwrap_or_default() == *expected_hash
        })
    }
}

/// Materialize props/emits/slots from resolved elements based on macro kind.
fn collect_native_props(
    elements: &verter_core::utils::oxc::vue::resolve_type::ResolvedElements,
) -> Vec<ResolvedNativeProp> {
    elements
        .props
        .iter()
        .map(|p| ResolvedNativeProp {
            name: p.key_name.clone().unwrap_or_else(|| "unknown".to_string()),
            is_optional: p.optional,
            type_annotation: p.type_text.clone(),
            visibility: p.visibility,
            span: verter_span::Span::new(p.span.start, p.span.end),
        })
        .collect()
}

/// Materialize props/emits/slots from resolved elements based on macro kind.
fn materialize_surfaces(
    host: &VerterHost,
    canonical_source: &str,
    macro_kind: &verter_analysis::AnalyzedMacroKind,
    elements: &verter_core::utils::oxc::vue::resolve_type::ResolvedElements,
) -> (
    Vec<verter_analysis::AnalyzedPropField>,
    Vec<verter_analysis::AnalyzedEmitField>,
    Vec<verter_analysis::AnalyzedSlotField>,
) {
    let source = read_full_source(host, canonical_source);

    match macro_kind {
        verter_analysis::AnalyzedMacroKind::DefineProps
        | verter_analysis::AnalyzedMacroKind::WithDefaults
        | verter_analysis::AnalyzedMacroKind::DefineModel => {
            let props = elements
                .props
                .iter()
                .filter(|p| p.visibility.is_public())
                .map(|p| {
                    let (description, tags) = member_jsdoc(source.as_deref(), p.span);
                    verter_analysis::AnalyzedPropField {
                        name: p.key_name.clone().unwrap_or_else(|| "unknown".to_string()),
                        is_optional: p.optional,
                        span: verter_span::Span::default(),
                        type_annotation: p.type_text.clone(),
                        description,
                        tags,
                        resolution_source: verter_analysis::TypeResolutionSource::Rust,
                        resolution_error: None,
                    }
                })
                .collect();
            (props, Vec::new(), Vec::new())
        }
        verter_analysis::AnalyzedMacroKind::DefineEmits => {
            let emits = elements
                .emits
                .iter()
                .map(|e| {
                    let (description, tags) = member_jsdoc(source.as_deref(), e.span);
                    let payload_type = match &e.signature {
                        verter_core::utils::oxc::vue::resolve_type::ResolvedEmitSignature::Call {
                            params_text,
                        } => {
                            if params_text.is_empty() {
                                None
                            } else {
                                Some(format!("[{}]", params_text))
                            }
                        }
                        verter_core::utils::oxc::vue::resolve_type::ResolvedEmitSignature::Tuple {
                            tuple_text,
                        } => Some(tuple_text.clone()),
                    };
                    verter_analysis::AnalyzedEmitField {
                        name: e.name.clone(),
                        span: verter_span::Span::default(),
                        payload_type,
                        description,
                        tags,
                    }
                })
                .collect();
            (Vec::new(), emits, Vec::new())
        }
        verter_analysis::AnalyzedMacroKind::DefineSlots => {
            let slots = elements
                .props
                .iter()
                .filter(|p| p.visibility.is_public())
                .map(|p| {
                    let name = p.key_name.clone().unwrap_or_else(|| "unknown".to_string());
                    let (description, tags) = member_jsdoc(source.as_deref(), p.span);
                    let (bindings, return_type) =
                        extract_slot_info_from_type_text(p.type_text.as_deref());
                    verter_analysis::AnalyzedSlotField {
                        name,
                        is_required: !p.optional,
                        span: verter_span::Span::default(),
                        bindings,
                        return_type,
                        description,
                        tags,
                    }
                })
                .collect();
            (Vec::new(), Vec::new(), slots)
        }
        _ => (Vec::new(), Vec::new(), Vec::new()),
    }
}

fn should_ignore_external_macro_type(dep: &verter_analysis::MacroTypeDep) -> bool {
    dep.macro_kind == verter_analysis::AnalyzedMacroKind::DefineSlots
        && dep.import_source == "vue"
        && dep.type_name == "Slot"
}

fn member_jsdoc(
    source: Option<&str>,
    span: verter_span::Span,
) -> (Option<String>, Vec<verter_analysis::types::JsdocTag>) {
    let Some(source) = source else {
        return (None, Vec::new());
    };
    verter_analysis::jsdoc::extract_jsdoc_near_offset(source, span.start)
}

pub(crate) fn resolve_type_declaration(
    host: &VerterHost,
    dep_canonical: &str,
    requested_name: &str,
) -> ResolvedTypeDeclaration {
    let resolved_export = host
        .resolve_exports(dep_canonical)
        .into_iter()
        .find(|export| export.name == requested_name);

    let (canonical_source, resolved_name) = if let Some(export) = resolved_export {
        (
            export
                .source_canonical_id
                .unwrap_or_else(|| dep_canonical.to_string()),
            export.source_name,
        )
    } else {
        follow_direct_type_reexport_chain(host, dep_canonical, requested_name)
            .unwrap_or_else(|| (dep_canonical.to_string(), requested_name.to_string()))
    };
    let export_span = host
        .get_export_span_follow_reexports(dep_canonical, requested_name)
        .map(|(_, start, end)| verter_span::Span::new(start, end))
        .unwrap_or_default();
    let (kind, span, text) = read_full_source(host, canonical_source.as_str())
        .map(|source| extract_declaration_details(&source, export_span, resolved_name.as_str()))
        .unwrap_or((ResolvedDeclarationKind::Unknown, export_span, None));

    // Some declaration entrypoints only re-export a type they imported from a
    // sibling declaration file, e.g. `import { Foo } from "./inner.js"; export
    // type { Foo };`. `resolve_exports()` sees the export surface, but not the
    // imported declaration owner. When the selected source does not actually
    // contain a declaration for `resolved_name`, follow the direct type reexport
    // chain and retry against the concrete declaration owner.
    if kind == ResolvedDeclarationKind::Unknown
        && text.is_none()
        && canonical_source == dep_canonical
    {
        if let Some((followed_canonical, followed_name)) =
            follow_direct_type_reexport_chain(host, dep_canonical, requested_name)
        {
            if followed_canonical != canonical_source || followed_name != resolved_name {
                if let Some(source) = read_full_source(host, followed_canonical.as_str()) {
                    let followed_details =
                        extract_declaration_details(&source, export_span, followed_name.as_str());
                    if followed_details.0 != ResolvedDeclarationKind::Unknown
                        || followed_details.2.is_some()
                    {
                        return ResolvedTypeDeclaration {
                            requested_name: requested_name.to_string(),
                            resolved_name: followed_name,
                            canonical_source: followed_canonical,
                            span: followed_details.1,
                            kind: followed_details.0,
                            text: followed_details.2,
                        };
                    }
                }
            }
        }
    }

    ResolvedTypeDeclaration {
        requested_name: requested_name.to_string(),
        resolved_name,
        canonical_source,
        span,
        kind,
        text,
    }
}

fn follow_direct_type_reexport_chain(
    host: &VerterHost,
    dep_canonical: &str,
    requested_name: &str,
) -> Option<(String, String)> {
    let mut current_canonical = dep_canonical.to_string();
    let mut current_name = requested_name.to_string();
    let mut visited = rustc_hash::FxHashSet::default();

    loop {
        if !visited.insert((current_canonical.clone(), current_name.clone())) {
            return Some((current_canonical, current_name));
        }

        let source = read_full_source(host, current_canonical.as_str())?;
        let alloc = oxc_allocator::Allocator::new();
        let extracted = verter_core::utils::oxc::vue::resolve_type::extract_imported_type_bindings(
            &source, &alloc,
        );

        let Some(reexport) = extracted
            .reexport_bindings
            .iter()
            .find(|binding| binding.local_name == current_name)
        else {
            return Some((current_canonical, current_name));
        };

        let Some(next_canonical) =
            host.resolve_type_dependency_canonical(&current_canonical, &reexport.source)
        else {
            return Some((current_canonical, current_name));
        };

        current_canonical = next_canonical;
        current_name = reexport.imported_name.clone();
    }
}

fn resolve_local_type_declaration(
    host: &VerterHost,
    canonical_source: &str,
    resolved: &verter_analysis::ResolvedLocalType,
) -> ResolvedTypeDeclaration {
    let span = resolved.span;
    let (kind, resolved_span, text) = read_full_source(host, canonical_source)
        .map(|source| extract_declaration_details(&source, span, resolved.name.as_str()))
        .unwrap_or((ResolvedDeclarationKind::Unknown, span, None));

    ResolvedTypeDeclaration {
        requested_name: resolved.name.clone(),
        resolved_name: resolved.name.clone(),
        canonical_source: canonical_source.to_string(),
        span: resolved_span,
        kind,
        text,
    }
}

fn read_full_source(host: &VerterHost, canonical_source: &str) -> Option<String> {
    host.get_source(canonical_source)
        .map(|source| source.to_string())
        .or_else(|| {
            host.workspace
                .read()
                .read_file(canonical_source)
                .map(|source| source.to_string())
        })
}

fn extract_declaration_details(
    source: &str,
    span: verter_span::Span,
    resolved_name: &str,
) -> (ResolvedDeclarationKind, verter_span::Span, Option<String>) {
    if let Some((kind, start)) = find_named_declaration_start(source, span, resolved_name) {
        if let Some((declaration_span, text)) = extract_named_declaration_text(source, start, kind)
        {
            return (kind, declaration_span, Some(text));
        }
    }

    if span.end > span.start {
        let start = span.start as usize;
        let end = span.end as usize;
        if start < source.len() && end <= source.len() {
            return (
                ResolvedDeclarationKind::Unknown,
                span,
                source.get(start..end).map(|text| text.trim().to_string()),
            );
        }
    }

    (ResolvedDeclarationKind::Unknown, span, None)
}

fn find_named_declaration_start(
    source: &str,
    span: verter_span::Span,
    resolved_name: &str,
) -> Option<(ResolvedDeclarationKind, usize)> {
    let search_end = if span.start == 0 && span.end == 0 {
        source.len()
    } else {
        (span.end as usize).min(source.len())
    };
    let haystack = source.get(..search_end).unwrap_or(source);
    let patterns = [
        (
            ResolvedDeclarationKind::Interface,
            format!("interface {resolved_name}"),
        ),
        (
            ResolvedDeclarationKind::TypeAlias,
            format!("type {resolved_name}"),
        ),
        (
            ResolvedDeclarationKind::Class,
            format!("class {resolved_name}"),
        ),
    ];

    patterns
        .into_iter()
        .filter_map(|(kind, needle)| {
            haystack.rfind(&needle).and_then(|start| {
                // Verify word boundary: the character after the name must not be
                // alphanumeric or underscore (prevents "interface Foo" matching
                // "interface FooExtended").
                let after = start + needle.len();
                if after < haystack.len() {
                    let next_ch = haystack.as_bytes()[after];
                    if next_ch.is_ascii_alphanumeric() || next_ch == b'_' {
                        return None;
                    }
                }
                Some((kind, start))
            })
        })
        .max_by_key(|(_, start)| *start)
}

fn extract_named_declaration_text(
    source: &str,
    keyword_start: usize,
    kind: ResolvedDeclarationKind,
) -> Option<(verter_span::Span, String)> {
    let line_start = source[..keyword_start]
        .rfind('\n')
        .map(|idx| idx + 1)
        .unwrap_or(0);

    let end = match kind {
        ResolvedDeclarationKind::Interface | ResolvedDeclarationKind::Class => {
            let brace_start = source.get(keyword_start..)?.find('{')? + keyword_start;
            find_matching_brace(source, brace_start).map(|idx| idx + 1)
        }
        ResolvedDeclarationKind::TypeAlias => {
            // Find the terminating semicolon at brace-depth 0, skipping
            // semicolons inside object/mapped types like `{ [K in keyof T]: T[K]; }`.
            find_top_level_semicolon(source, keyword_start)
                .map(|idx| idx + 1)
                .or_else(|| {
                    source[keyword_start..]
                        .find('\n')
                        .map(|idx| keyword_start + idx)
                })
        }
        ResolvedDeclarationKind::Unknown => None,
    }?;

    source.get(line_start..end).map(|text| {
        (
            verter_span::Span::new(line_start as u32, end as u32),
            text.trim().to_string(),
        )
    })
}

/// Find the closing `}` that matches the opening `{` at `brace_start`.
/// Skips braces inside string literals (single/double/backtick) and
/// single-line comments (`//`). This is a best-effort text scanner, not
/// a full parser — it does not handle multi-line comments or escaped quotes
/// inside template literals.
fn find_matching_brace(source: &str, brace_start: usize) -> Option<usize> {
    let bytes = source.get(brace_start..)?.as_bytes();
    let mut depth = 0u32;
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        match ch {
            // Skip string literals
            b'\'' | b'"' | b'`' => {
                i += 1;
                while i < bytes.len() && bytes[i] != ch {
                    if bytes[i] == b'\\' {
                        i += 1; // skip escaped char
                    }
                    i += 1;
                }
                // skip closing quote
            }
            // Skip single-line comments
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            // Skip multi-line comments
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 1; // skip closing */
            }
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(brace_start + i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Find the first `;` at brace-depth 0 starting from `start`.
/// Skips semicolons inside `{ ... }` blocks (mapped types, object types).
fn find_top_level_semicolon(source: &str, start: usize) -> Option<usize> {
    let bytes = source.get(start..)?.as_bytes();
    let mut depth = 0u32;
    for (i, &ch) in bytes.iter().enumerate() {
        match ch {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => return Some(start + i),
            _ => {}
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn resolve_jsdoc_block(
    host: &VerterHost,
    canonical_source: &str,
    span: verter_span::Span,
    mode: ResolverMode,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    cache: &mut rustc_hash::FxHashMap<
        (String, String),
        Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>,
    >,
    visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    kind: verter_vfs::ResolveRequestKind,
) -> Option<ResolvedJsdocBlock> {
    if span.start == 0 && span.end == 0 {
        return None;
    }

    let source = read_full_source(host, canonical_source)?;
    let (description, tags) =
        verter_analysis::jsdoc::extract_jsdoc_near_offset(&source, span.start);
    if description.is_none() && tags.is_empty() {
        return None;
    }

    Some(ResolvedJsdocBlock {
        description,
        tags: tags
            .into_iter()
            .map(|tag| {
                map_jsdoc_tag(
                    host,
                    canonical_source,
                    mode,
                    tracked_deps,
                    cache,
                    visiting,
                    kind,
                    tag,
                )
            })
            .collect(),
    })
}

#[allow(clippy::too_many_arguments)]
fn map_jsdoc_tag(
    host: &VerterHost,
    canonical_source: &str,
    mode: ResolverMode,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    cache: &mut rustc_hash::FxHashMap<
        (String, String),
        Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>,
    >,
    visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    kind: verter_vfs::ResolveRequestKind,
    tag: verter_analysis::types::JsdocTag,
) -> ResolvedJsdocTag {
    let (text, raw_type, subject_name) = parse_jsdoc_tag_payload(tag.name.as_str(), tag.text);
    let resolved_type = if mode == ResolverMode::Expanded {
        raw_type.as_deref().and_then(|raw_type| {
            resolve_jsdoc_tag_type(
                host,
                canonical_source,
                raw_type,
                tracked_deps,
                cache,
                visiting,
                kind,
            )
        })
    } else {
        None
    };
    ResolvedJsdocTag {
        name: tag.name,
        text,
        raw_type,
        subject_name,
        resolved_type,
    }
}

fn parse_jsdoc_tag_payload(
    tag_name: &str,
    text: Option<String>,
) -> (Option<String>, Option<String>, Option<String>) {
    let Some(text) = text else {
        return (None, None, None);
    };
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix('{') else {
        return (Some(text), None, None);
    };
    // Depth-aware brace matching: find the closing `}` that matches the
    // opening `{`, handling nested braces like `{Record<string, {nested: true}>}`.
    let end = {
        let mut depth = 0u32;
        let mut found = None;
        for (i, ch) in rest.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    if depth == 0 {
                        found = Some(i);
                        break;
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
        found
    };
    let Some(end) = end else {
        return (Some(text), None, None);
    };

    let raw_type = Some(rest[..end].trim().to_string());
    let trailing = rest[end + 1..].trim();
    if trailing.is_empty() {
        return (None, raw_type, None);
    }

    if matches!(tag_name, "param" | "arg" | "argument") {
        let mut parts = trailing.splitn(2, char::is_whitespace);
        let subject_name = parts.next().map(str::to_string);
        let text = parts
            .next()
            .map(str::trim)
            .filter(|rest| !rest.is_empty())
            .map(str::to_string);
        (text, raw_type, subject_name)
    } else {
        (Some(trailing.to_string()), raw_type, None)
    }
}

fn resolve_jsdoc_tag_type(
    host: &VerterHost,
    canonical_source: &str,
    raw_type: &str,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    cache: &mut rustc_hash::FxHashMap<
        (String, String),
        Option<verter_core::utils::oxc::vue::resolve_type::ResolvedElements>,
    >,
    visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    kind: verter_vfs::ResolveRequestKind,
) -> Option<verter_analysis::type_expr::TypeExpr> {
    let source = read_full_source(host, canonical_source)?;
    let synthetic_source = format!("{source}\nexport type __VerterJsdocTag = {raw_type};");

    let import_alloc = oxc_allocator::Allocator::new();
    let extracted = verter_core::utils::oxc::vue::resolve_type::extract_imported_type_bindings(
        &synthetic_source,
        &import_alloc,
    );
    let required_import_names =
        verter_core::utils::oxc::vue::resolve_type::collect_required_import_names_for_external_type(
            "__VerterJsdocTag",
            &synthetic_source,
            &import_alloc,
        );

    let mut companion_types = rustc_hash::FxHashMap::default();
    for binding in extracted
        .bindings
        .iter()
        .filter(|binding| required_import_names.contains(&binding.local_name))
    {
        let mut resolution_deps = std::collections::BTreeSet::new();
        if let Ok(Some(resolved)) = host.resolve_external_type_from_loaded_files(
            canonical_source,
            &binding.source,
            &binding.imported_name,
            tracked_deps,
            &mut resolution_deps,
            cache,
            visiting,
            false,
            kind,
            true,
            None,
            0,
        ) {
            companion_types
                .entry(binding.local_name.clone())
                .or_insert(resolved);
        }
    }

    let resolve_alloc = oxc_allocator::Allocator::new();
    let resolved =
        verter_core::utils::oxc::vue::resolve_type::resolve_external_type_with_companion(
            "__VerterJsdocTag",
            &synthetic_source,
            &companion_types,
            &resolve_alloc,
        )?;
    Some(crate::host_manage::resolved_elements_to_type_expr_via_type_text(&resolved))
}

#[cfg(test)]
#[path = "meta_resolve_tests.rs"]
mod meta_resolve_tests;

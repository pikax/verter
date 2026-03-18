//! `impl VerterHost` — file management, analysis, and diagnostics methods.
//!
//! Contains [`VerterHost::remove`], [`VerterHost::get_analysis`],
//! [`VerterHost::get_diagnostics`], and [`VerterHost::set_import_dependencies`].

use std::sync::Arc;

use crate::hash::compile_profile_hash;
use crate::id::canonicalize_id;
use crate::shared::{read_lock, write_lock};
use crate::types::*;
use crate::VerterHost;

impl VerterHost {
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
    /// resolves the type by reading the dependency source from the host cache and running
    /// `resolve_external_type()`. Returns expanded type text suitable for the type registry.
    pub fn resolve_imported_types(
        &self,
        canonical_or_alias: &str,
    ) -> Vec<verter_analysis::ResolvedLocalType> {
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;

            let source_snap = match self.scheduler.try_get_source(&canonical) {
                Some(s) => s,
                None => return Vec::new(),
            };
            let hd = match source_snap.downcast_data::<HostSourceData>() {
                Some(d) => d,
                None => return Vec::new(),
            };
            let macro_type_deps = hd.parse.script_analysis.macro_type_deps.clone();
            if macro_type_deps.is_empty() {
                return Vec::new();
            }
            let dep_resolutions = self
                .compile_cache
                .get(&canonical)
                .map(|cc| cc.dependency_resolutions.clone())
                .unwrap_or_default();
            drop(source_snap);

            let mut result = Vec::new();
            let alloc = oxc_allocator::Allocator::new();

            for dep in macro_type_deps.iter() {
                let dep_canonical = if dep.import_source.starts_with('.') {
                    crate::id::resolve_external(&canonical, &dep.import_source)
                } else {
                    if let Some(res) = dep_resolutions.get(&dep.import_source) {
                        if let Some(ref id) = res.resolved_canonical_id {
                            id.clone()
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                };

                // Try scheduler for source, then extensions, then VFS fallback
                let dep_source = self
                    .scheduler
                    .try_get_source(&dep_canonical)
                    .map(|s| s.source.clone())
                    .or_else(|| {
                        for ext in &[".ts", ".tsx", "/index.ts", ".d.ts"] {
                            let with_ext = format!("{}{}", dep_canonical, ext);
                            if let Some(s) = self.scheduler.try_get_source(&with_ext) {
                                return Some(s.source.clone());
                            }
                        }
                        None
                    });
                let Some(dep_source) = dep_source else {
                    continue;
                };

                if let Some(resolved) =
                    verter_core::utils::oxc::vue::resolve_type::resolve_external_type(
                        &dep.type_name,
                        &dep_source,
                        &alloc,
                    )
                {
                    let expanded = resolved_elements_to_expanded_text(&resolved, &dep_source);
                    result.push(verter_analysis::ResolvedLocalType {
                        name: dep.type_name.clone(),
                        expanded,
                        span: verter_span::Span::default(),
                    });
                }
            }
            result
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let Some(entry) = files.get(&canonical) else {
                return Vec::new();
            };

            let macro_type_deps = entry.script_analysis.macro_type_deps.clone();
            if macro_type_deps.is_empty() {
                return Vec::new();
            }

            let mut result = Vec::new();
            let alloc = oxc_allocator::Allocator::new();

            for dep in macro_type_deps.iter() {
                let dep_canonical = if dep.import_source.starts_with('.') {
                    crate::id::resolve_external(&canonical, &dep.import_source)
                } else {
                    if let Some(res) = entry.dependency_resolutions.get(&dep.import_source) {
                        if let Some(ref id) = res.resolved_canonical_id {
                            id.clone()
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                };

                let Some(dep_entry) = files.get(&dep_canonical) else {
                    let mut found_source = None;
                    for ext in &[".ts", ".tsx", "/index.ts", ".d.ts"] {
                        let with_ext = format!("{}{}", dep_canonical, ext);
                        if let Some(e) = files.get(&with_ext) {
                            found_source = Some(e.source.clone());
                            break;
                        }
                    }
                    let Some(source) = found_source else {
                        continue;
                    };
                    if let Some(resolved) =
                        verter_core::utils::oxc::vue::resolve_type::resolve_external_type(
                            &dep.type_name,
                            &source,
                            &alloc,
                        )
                    {
                        let expanded = resolved_elements_to_expanded_text(&resolved, &source);
                        result.push(verter_analysis::ResolvedLocalType {
                            name: dep.type_name.clone(),
                            expanded,
                            span: verter_span::Span::default(),
                        });
                    }
                    continue;
                };

                let dep_source = dep_entry.source.clone();
                if let Some(resolved) =
                    verter_core::utils::oxc::vue::resolve_type::resolve_external_type(
                        &dep.type_name,
                        &dep_source,
                        &alloc,
                    )
                {
                    let expanded = resolved_elements_to_expanded_text(&resolved, &dep_source);
                    result.push(verter_analysis::ResolvedLocalType {
                        name: dep.type_name.clone(),
                        expanded,
                        span: verter_span::Span::default(),
                    });
                }
            }
            result
        }
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

        let macro_type_deps: Vec<verter_analysis::MacroTypeDep> =
            snapshot.macro_type_deps.iter().cloned().collect();

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

        for dep in &macro_type_deps {
            let resolved = match self.resolve_external_type_from_loaded_files(
                canonical,
                &dep.import_source,
                &dep.type_name,
                &mut cache,
                &mut visiting,
                false,
                kind,
                None,
            ) {
                Ok(Some(r)) => r,
                _ => continue,
            };

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
    }

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
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        // Eviction gate (scheduler path)
        #[cfg(feature = "scheduler")]
        {
            if let Some(cc) = self.compile_cache.get(&canonical) {
                if cc.evicted {
                    return None;
                }
            }
        }

        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;

            let source_snap = self.scheduler.try_get_source(&canonical)?;
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
                    .try_get_analysis(&canonical)
                    .and_then(|a| {
                        a.downcast_data::<crate::host_executor::HostAnalysisData>()
                            .map(|ad| Arc::clone(&ad.style_analyses))
                    })
                    .unwrap_or_else(|| Arc::new(Vec::new()));
                let template = self
                    .compile_cache
                    .get(&canonical)
                    .and_then(|cc| cc.raw_template_analysis.clone());
                let export_sigs = self
                    .scheduler
                    .try_get_analysis(&canonical)
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
                self.resolve_snapshot_imports(&canonical, &mut snapshot);
                self.enrich_destructured_bindings(&mut snapshot);
                if scope.needs_template_analysis() {
                    self.compute_template_analysis_if_missing(&canonical, &mut snapshot);
                }
                if self.config.deep_macro_resolution_type {
                    self.enrich_imported_types(&canonical, &mut snapshot);
                }
                return Some(snapshot);
            }
            drop(source_snap);

            let mut snapshot = self.build_snapshot_from_scheduler(&canonical)?;
            self.resolve_snapshot_imports(&canonical, &mut snapshot);
            self.enrich_destructured_bindings(&mut snapshot);
            if self.config.effective_scope().needs_template_analysis() {
                self.compute_template_analysis_if_missing(&canonical, &mut snapshot);
            }
            if self.config.deep_macro_resolution_type {
                self.enrich_imported_types(&canonical, &mut snapshot);
            }
            Some(snapshot)
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&self.files);
            let entry = files.get(&canonical)?;

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
                self.resolve_snapshot_imports(&canonical, &mut snapshot);
                self.enrich_destructured_bindings(&mut snapshot);
                if scope.needs_template_analysis() {
                    self.compute_template_analysis_if_missing(&canonical, &mut snapshot);
                }
                if self.config.deep_macro_resolution_type {
                    self.enrich_imported_types(&canonical, &mut snapshot);
                }
                return Some(snapshot);
            }

            let mut snapshot = Self::build_snapshot_from_entry(entry);
            drop(files);
            self.resolve_snapshot_imports(&canonical, &mut snapshot);
            self.enrich_destructured_bindings(&mut snapshot);
            if self.config.effective_scope().needs_template_analysis() {
                self.compute_template_analysis_if_missing(&canonical, &mut snapshot);
            }
            if self.config.deep_macro_resolution_type {
                self.enrich_imported_types(&canonical, &mut snapshot);
            }
            Some(snapshot)
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
            // Try VFS resolution fallback
            let dep_id = self.resolve_loaded_dependency_canonical(
                owner_canonical,
                specifier,
                verter_vfs::ResolveRequestKind::EsmImport,
            );
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
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical) {
                entry.compile_slots.clear();
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

        // Normalize resolutions and derive flat dependency set.
        let mut new_deps = std::collections::BTreeSet::new();
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
            if let Some(ref canonical_id) = res.resolved_canonical_id {
                new_deps.insert(canonical_id.clone());
            } else {
                for candidate in &res.possible_canonical_ids {
                    new_deps.insert(candidate.clone());
                }
            }
            dep_resolutions.insert(res.specifier.clone(), res);
        }

        // Read old deps, write new deps. compile_cache is primary on scheduler path.
        #[cfg(feature = "scheduler")]
        let old_deps = {
            let mut cc_ref = self.compile_cache.entry(canonical.clone()).or_default();
            let cc = cc_ref.value_mut();
            let old = cc.dependencies.clone();
            cc.dependencies = new_deps.clone();
            cc.dependency_resolutions = dep_resolutions.clone();
            old
        };
        #[cfg(not(feature = "scheduler"))]
        let old_deps = {
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(&canonical) {
                let old = entry.dependencies.clone();
                entry.dependencies = new_deps.clone();
                entry.dependency_resolutions = dep_resolutions;
                old
            } else {
                std::collections::BTreeSet::new()
            }
        };

        self.update_reverse_deps(&canonical, &old_deps, &new_deps);

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
        let profile_hash = profile.map(compile_profile_hash);

        #[cfg(feature = "scheduler")]
        let canonical_ids: Vec<String> = self
            .scheduler
            .node_ids()
            .into_iter()
            .filter(|id| self.compile_cache.get(id).map_or(true, |cc| !cc.evicted))
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

            return Self::find_export_span(
                file_kind,
                &ad.script_analysis,
                &ad.export_signatures,
                binding_name,
            );
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

/// Convert `ResolvedElements` props to an expanded type text string.
///
/// Produces `"{ name: type; name2?: type2 }"` format matching `build_expanded_type_text`
/// in `verter_analysis::macros`.
fn resolved_elements_to_expanded_text(
    resolved: &verter_core::utils::oxc::vue::resolve_type::ResolvedElements,
    source: &str,
) -> String {
    let source_bytes = source.as_bytes();
    let mut parts = Vec::new();
    for prop in &resolved.props {
        let name = prop.key_name.as_deref().unwrap_or_else(|| {
            let start = prop.key.start as usize;
            let end = prop.key.end as usize;
            if end <= source_bytes.len() {
                std::str::from_utf8(&source_bytes[start..end]).unwrap_or("unknown")
            } else {
                "unknown"
            }
        });
        let opt = if prop.optional { "?" } else { "" };
        let ty = if let Some(type_span) = prop.type_span {
            let start = type_span.start as usize;
            let end = type_span.end as usize;
            if end <= source_bytes.len() {
                std::str::from_utf8(&source_bytes[start..end]).unwrap_or("unknown")
            } else {
                "unknown"
            }
        } else {
            "unknown"
        };
        parts.push(format!("{}{}: {}", name, opt, ty));
    }
    format!("{{ {} }}", parts.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const LAZY_ANALYSIS_SFC: &str = r#"<template><div>{{ msg }}</div></template>
<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>
<style>
.foo { color: red; }
</style>"#;

    fn make_host() -> VerterHost {
        VerterHost::new_standalone(HostConfig::default())
    }

    fn make_lazy_host() -> VerterHost {
        VerterHost::new_standalone(HostConfig {
            analysis_level: AnalysisLevel::None,
            ..HostConfig::default()
        })
    }

    fn upsert_vue(host: &VerterHost, id: &str, src: &str) {
        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .unwrap();
    }

    #[cfg(not(feature = "scheduler"))]
    fn mutate_lazy_analysis_source(host: &VerterHost) {
        let mut files = crate::shared::write_lock(&host.files);
        let entry = files.get_mut("App.vue").expect("App.vue should exist");
        let broken = entry
            .source
            .replace("<script", "<scripx")
            .replace("</script>", "</scripx>")
            .replace("<style", "<styla")
            .replace("</style>", "</styla>");
        entry.source = Arc::from(broken);
    }

    #[cfg(not(feature = "scheduler"))]
    fn clear_cached_parse(host: &VerterHost) {
        let mut files = crate::shared::write_lock(&host.files);
        let entry = files.get_mut("App.vue").expect("App.vue should exist");
        entry.cached_parse = None;
    }

    #[test]
    fn raw_template_analysis_extracts_css_var_names() {
        let host = make_host();
        upsert_vue(
            &host,
            "/src/A.vue",
            "<script setup>\nconst color = 'red'\n</script>\n<template><div :style=\"{ '--theme-color': color }\">A</div></template>",
        );

        let template = host
            .raw_template_analysis_for_file("/src/A.vue")
            .expect("raw template analysis should be computed");
        assert!(
            template
                .css_var_names
                .iter()
                .any(|name| name == "--theme-color"),
            "raw template analysis should include CSS vars from :style bindings"
        );
    }

    #[cfg(feature = "scheduler")]
    #[test]
    fn override_template_analysis_helper_uses_content_override() {
        let host = make_host();
        upsert_vue(
            &host,
            "/src/A.vue",
            "<script setup>\nconst color = 'red'\n</script>\n<template><div>A</div></template>",
        );

        let profile = CompileProfile::default();
        let profile_hash = crate::hash::compile_profile_hash(&profile);
        let _ = host
            .apply_block_overrides(BlockOverrideRequest {
                canonical_id: "/src/A.vue".to_string(),
                compile_profile: profile.clone(),
                overrides: vec![BlockOverrideEntry {
                    block_type: PreprocessorBlockType::Template,
                    index: 0,
                    code: Arc::from("<div :style=\"{ '--theme-color': color }\">A</div>"),
                    source_map: None,
                }],
            })
            .expect("template override should succeed");

        let template = host
            .compute_override_template_analysis("/src/A.vue", profile_hash)
            .expect("override template analysis should be computed");
        assert!(
            template
                .css_var_names
                .iter()
                .any(|name| name == "--theme-color"),
            "override template analysis should reflect the overridden template"
        );
    }

    /// @ai-generated - get_analysis populates resolved_canonical_id for relative imports
    #[test]
    fn get_analysis_resolves_relative_import() {
        let host = make_host();
        upsert_vue(
            &host,
            "/project/Child.vue",
            "<script setup>\ndefineProps({ msg: String })\n</script>\n<template><div>{{ msg }}</div></template>",
        );
        upsert_vue(
            &host,
            "/project/Parent.vue",
            "<script setup>\nimport Child from './Child.vue'\n</script>\n<template><Child msg=\"hello\" /></template>",
        );

        let analysis = host.get_analysis("/project/Parent.vue").unwrap();
        let child_import = analysis
            .imports
            .iter()
            .find(|i| i.source == "./Child.vue")
            .unwrap();
        assert_eq!(
            child_import.resolved_canonical_id.as_deref(),
            Some("/project/Child.vue"),
            "relative import should resolve to canonical ID"
        );
    }

    /// @ai-generated - get_analysis resolves imports via alias map
    #[test]
    fn get_analysis_resolves_alias_import() {
        let host = make_host();
        upsert_vue(
            &host,
            "/project/src/components/Child.vue",
            "<script setup>\ndefineProps({ msg: String })\n</script>\n<template><div/></template>",
        );
        upsert_vue(
            &host,
            "/project/src/App.vue",
            "<script setup>\nimport Child from '@/components/Child.vue'\n</script>\n<template><Child/></template>",
        );
        // Configure workspace resolver with alias
        {
            host.workspace().configure_resolver(vec![
                verter_analysis::project_resolver::IdeProjectConfig {
                    root: "/project".to_string(),
                    workspace_root: "/project".to_string(),
                    tsconfig_path: None,
                    provider_root: "/project".to_string(),
                    workspace_aliases: vec![verter_vfs::WorkspaceAlias {
                        find: "@/".to_string(),
                        replacement: "/project/src/".to_string(),
                    }],
                    compiler_options:
                        verter_analysis::project_resolver::IdeProjectCompilerOptions::default(),
                    references: vec![],
                    membership: verter_analysis::project_resolver::ProjectMembership::MatchAll,
                },
            ]);
        }

        let analysis = host.get_analysis("/project/src/App.vue").unwrap();
        let child_import = analysis
            .imports
            .iter()
            .find(|i| i.source == "@/components/Child.vue")
            .unwrap();
        assert_eq!(
            child_import.resolved_canonical_id.as_deref(),
            Some("/project/src/components/Child.vue"),
            "alias import should resolve via alias map"
        );
    }

    /// @ai-generated - get_analysis resolves imports with extension guessing
    #[test]
    fn get_analysis_resolves_extension_guessing() {
        let host = make_host();
        upsert_vue(
            &host,
            "/project/Child.vue",
            "<script setup>\n</script>\n<template><div/></template>",
        );
        upsert_vue(
            &host,
            "/project/Parent.vue",
            "<script setup>\nimport Child from './Child'\n</script>\n<template><Child/></template>",
        );

        let analysis = host.get_analysis("/project/Parent.vue").unwrap();
        let child_import = analysis
            .imports
            .iter()
            .find(|i| i.source == "./Child")
            .unwrap();
        assert_eq!(
            child_import.resolved_canonical_id.as_deref(),
            Some("/project/Child.vue"),
            "extension-less import should resolve via .vue guessing"
        );
    }

    /// @ai-generated - get_analysis leaves bare specifiers unresolved
    #[test]
    fn get_analysis_bare_specifier_unresolved() {
        let host = make_host();
        upsert_vue(
            &host,
            "App.vue",
            "<script setup>\nimport { ref } from 'vue'\n</script>\n<template><div/></template>",
        );

        let analysis = host.get_analysis("App.vue").unwrap();
        let vue_import = analysis.imports.iter().find(|i| i.source == "vue").unwrap();
        assert!(
            vue_import.resolved_canonical_id.is_none(),
            "bare specifier 'vue' should not resolve (no node_modules resolution)"
        );
    }

    /// @ai-generated - get_analysis leaves unregistered file imports unresolved
    #[test]
    fn get_analysis_missing_file_unresolved() {
        let host = make_host();
        upsert_vue(
            &host,
            "App.vue",
            "<script setup>\nimport Missing from './Missing.vue'\n</script>\n<template><div/></template>",
        );

        let analysis = host.get_analysis("App.vue").unwrap();
        let missing_import = analysis
            .imports
            .iter()
            .find(|i| i.source == "./Missing.vue")
            .unwrap();
        assert!(
            missing_import.resolved_canonical_id.is_none(),
            "import of unregistered file should not resolve"
        );
    }

    #[test]
    fn get_analysis_uses_cached_parse_for_lazy_analysis() {
        let host = make_lazy_host();
        upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);

        // On the scheduler path, source is immutable in the scheduler snapshot,
        // so mutating host.files has no effect. The scheduler path reads from
        // HostSourceData.cached_parse directly. We just verify get_analysis()
        // returns correct lazy-recomputed data with AnalysisLevel::None.
        #[cfg(not(feature = "scheduler"))]
        mutate_lazy_analysis_source(&host);

        let analysis = host.get_analysis("App.vue").unwrap();

        assert!(
            analysis.bindings.iter().any(|b| b.name == "msg"),
            "lazy script analysis should reuse cached parse for bindings"
        );
        assert_eq!(
            analysis.styles.len(),
            1,
            "lazy style analysis should reuse cached parse for style blocks"
        );
        let css = analysis.styles[0]
            .css
            .as_ref()
            .expect("CSS analysis should exist for cached style block");
        assert!(
            css.classes.iter().any(|class| class.name == "foo"),
            "lazy style analysis should preserve CSS classes"
        );
        assert!(
            analysis
                .module_references
                .iter()
                .any(|reference| reference.literal_specifier.as_deref() == Some("vue")),
            "lazy script analysis should preserve module references"
        );
    }

    #[test]
    fn get_analysis_falls_back_when_cached_parse_missing() {
        let host = make_lazy_host();
        upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);

        // On the scheduler path, cached_parse is immutable in HostSourceData
        // and always present for Vue SFCs. The scheduler path handles both
        // cached_parse present and absent cases. We just verify correctness.
        #[cfg(not(feature = "scheduler"))]
        clear_cached_parse(&host);

        let analysis = host.get_analysis("App.vue").unwrap();

        assert!(
            analysis.bindings.iter().any(|b| b.name == "msg"),
            "source fallback should still recover bindings"
        );
        assert_eq!(
            analysis.styles.len(),
            1,
            "source fallback should still recover style blocks"
        );
        let css = analysis.styles[0]
            .css
            .as_ref()
            .expect("CSS analysis should exist for fallback style block");
        assert!(
            css.classes.iter().any(|class| class.name == "foo"),
            "source fallback should preserve CSS classes"
        );
        assert!(
            analysis
                .module_references
                .iter()
                .any(|reference| reference.literal_specifier.as_deref() == Some("vue")),
            "source fallback should preserve module references"
        );
    }

    /// @ai-generated - get_export_span for .vue file returns binding span
    #[test]
    fn get_export_span_vue_binding() {
        let host = make_host();
        upsert_vue(
            &host,
            "Child.vue",
            "<script setup>\nconst msg = 'hello'\n</script>\n<template><div/></template>",
        );

        let span = host.get_export_span("Child.vue", "msg");
        assert!(span.is_some(), "should find 'msg' binding in .vue file");
        let (start, end) = span.unwrap();
        let source = host.get_source("Child.vue").unwrap();
        let spanned = &source[start as usize..end as usize];
        assert_eq!(spanned, "msg", "span should cover the binding identifier");
    }

    /// @ai-generated - get_export_span for .vue file returns None for unknown binding
    #[test]
    fn get_export_span_vue_unknown_binding() {
        let host = make_host();
        upsert_vue(
            &host,
            "Child.vue",
            "<script setup>\nconst msg = 'hello'\n</script>\n<template><div/></template>",
        );

        assert!(
            host.get_export_span("Child.vue", "nonexistent").is_none(),
            "unknown binding should return None"
        );
    }

    /// @ai-generated - get_export_span for .ts file returns export signature span
    #[test]
    fn get_export_span_ts_file() {
        let host = make_host();
        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: "utils.ts".to_string(),
            source: Arc::from("export function helper() { return 1; }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

        let span = host.get_export_span("utils.ts", "helper");
        assert!(span.is_some(), "should find 'helper' export in .ts file");
        let (start, end) = span.unwrap();
        let source = host.get_source("utils.ts").unwrap();
        let spanned = &source[start as usize..end as usize];
        assert_eq!(
            spanned, "helper",
            "span should cover the function identifier"
        );
    }

    /// @ai-generated - get_export_span for .vue default import finds first binding
    #[test]
    fn get_export_span_vue_default() {
        let host = make_host();
        upsert_vue(
            &host,
            "Child.vue",
            "<script setup>\nconst msg = 'hello'\n</script>\n<template><div/></template>",
        );

        let span = host.get_export_span("Child.vue", "default");
        assert!(
            span.is_some(),
            "default export of .vue should resolve to first binding"
        );
    }

    /// @ai-generated - resolve_import public method works
    #[test]
    fn resolve_import_public_method() {
        let host = make_host();
        upsert_vue(&host, "/project/Child.vue", "<template><div/></template>");
        upsert_vue(
            &host,
            "/project/Parent.vue",
            "<script setup>\nimport Child from './Child.vue'\n</script>\n<template><Child/></template>",
        );

        assert_eq!(
            host.resolve_import("/project/Parent.vue", "./Child.vue")
                .as_deref(),
            Some("/project/Child.vue")
        );
        // Bare specifiers that aren't in the file map resolve to None
        assert!(host
            .resolve_import("/project/Parent.vue", "lodash")
            .is_none());
    }

    #[test]
    fn resolve_import_public_method_handles_relative_full_paths() {
        let host = make_host();
        upsert_vue(
            &host,
            "/project/src/components/BarrelComp.vue",
            "<script setup>\nconst emit = defineEmits<{ custom: [] }>()\n</script>\n",
        );
        upsert_ts(
            &host,
            "/project/src/components/index.ts",
            "export { default as BarrelComp } from './BarrelComp.vue'",
        );
        upsert_vue(
            &host,
            "/project/src/App.vue",
            "<script setup>\nimport { BarrelComp } from './components'\n</script>\n<template><BarrelComp /></template>",
        );

        assert_eq!(
            host.resolve_import("/project/src/components/index.ts", "./BarrelComp.vue")
                .as_deref(),
            Some("/project/src/components/BarrelComp.vue"),
            "relative imports from full-path barrel files should resolve to the child SFC"
        );
    }

    fn upsert_ts(host: &VerterHost, id: &str, src: &str) {
        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();
    }

    #[test]
    fn enriches_destructured_composable_bindings() {
        let host = make_host();

        // Composable that returns { x: ref, y: ref, reset: function }
        upsert_ts(
            &host,
            "/project/useMouse.ts",
            r#"
import { ref } from 'vue'
export function useMouse() {
    const x = ref(0)
    const y = ref(0)
    function reset() { x.value = 0; y.value = 0 }
    return { x, y, reset }
}
"#,
        );

        // SFC that destructures the composable return
        upsert_vue(
            &host,
            "/project/App.vue",
            r#"<script setup>
import { useMouse } from './useMouse.ts'
const { x, y, reset } = useMouse()
</script>
<template><div>{{ x }} {{ y }}</div></template>"#,
        );

        let analysis = host.get_analysis("/project/App.vue").unwrap();

        // x and y should be enriched to Ref (from composable return shape)
        let x_binding = analysis.bindings.iter().find(|b| b.name == "x").unwrap();
        assert_eq!(
            x_binding.reactivity_kind,
            verter_analysis::ReactivityKind::Ref,
            "x should be enriched from MaybeRef to Ref via composable return shape"
        );

        let y_binding = analysis.bindings.iter().find(|b| b.name == "y").unwrap();
        assert_eq!(
            y_binding.reactivity_kind,
            verter_analysis::ReactivityKind::Ref,
            "y should be enriched from MaybeRef to Ref via composable return shape"
        );

        // reset should stay as a function (ReactivityKind::None since it's not reactive)
        let reset_binding = analysis
            .bindings
            .iter()
            .find(|b| b.name == "reset")
            .unwrap();
        assert_eq!(
            reset_binding.reactivity_kind,
            verter_analysis::ReactivityKind::None,
            "reset (a function) should be None, not reactive"
        );

        // Negative: non-enriched bindings should not be affected
        assert!(
            !x_binding.is_reactive
                || x_binding.reactivity_kind != verter_analysis::ReactivityKind::MaybeRef,
            "x should NOT remain MaybeRef after enrichment"
        );
    }

    #[test]
    fn get_export_span_follows_reexport_to_vue() {
        let host = make_host();

        // Target: Popup.vue with a binding
        upsert_vue(
            &host,
            "/project/Popup.vue",
            "<script setup>\nconst message = 'hello'\n</script>\n<template><div>{{ message }}</div></template>",
        );

        // Barrel: index.ts re-exports Popup.vue as default
        upsert_ts(
            &host,
            "/project/index.ts",
            "export { default as Popup } from './Popup.vue'",
        );

        // Follow the re-export: "Popup" in index.ts → default in Popup.vue
        let result = host.get_export_span_follow_reexports("/project/index.ts", "Popup");

        assert!(result.is_some(), "should follow re-export to Popup.vue");
        let (canonical_id, start, end) = result.unwrap();
        assert_eq!(
            canonical_id, "/project/Popup.vue",
            "should resolve to Popup.vue canonical ID"
        );
        assert!(
            start < end,
            "should have a valid span in Popup.vue (start={start}, end={end})"
        );
        // Negative: should NOT return index.ts
        assert_ne!(
            canonical_id, "/project/index.ts",
            "must NOT return the barrel file itself"
        );
    }

    #[test]
    fn get_export_span_follows_reexport_to_vue_full_paths() {
        let host = make_host();

        upsert_vue(
            &host,
            "/project/src/components/BarrelComp.vue",
            "<script setup>\nconst emit = defineEmits<{ custom: [] }>()\n</script>\n",
        );
        upsert_ts(
            &host,
            "/project/src/components/index.ts",
            "export { default as BarrelComp } from './BarrelComp.vue'",
        );

        let result =
            host.get_export_span_follow_reexports("/project/src/components/index.ts", "BarrelComp");

        assert!(
            result.is_some(),
            "should follow full-path barrel re-export to BarrelComp.vue"
        );
        let (canonical_id, start, end) = result.unwrap();
        assert_eq!(
            canonical_id, "/project/src/components/BarrelComp.vue",
            "should resolve to the full child Vue canonical ID"
        );
        assert!(start < end, "should return a valid span in BarrelComp.vue");
    }

    #[test]
    fn get_export_span_follows_named_reexport() {
        let host = make_host();

        // Target: utils.ts with an exported function
        upsert_ts(
            &host,
            "/project/utils.ts",
            "export function helper() { return 42 }",
        );

        // Barrel: re-exports helper as myHelper
        upsert_ts(
            &host,
            "/project/index.ts",
            "export { helper as myHelper } from './utils.ts'",
        );

        let result = host.get_export_span_follow_reexports("/project/index.ts", "myHelper");

        assert!(result.is_some(), "should follow named re-export");
        let (canonical_id, start, end) = result.unwrap();
        assert_eq!(
            canonical_id, "/project/utils.ts",
            "should resolve to utils.ts"
        );
        assert!(start < end, "should have a valid span");
        // Negative: should NOT return barrel
        assert_ne!(canonical_id, "/project/index.ts");
    }

    #[test]
    fn get_export_span_follows_multi_hop_chain() {
        let host = make_host();

        upsert_ts(&host, "/project/a.ts", "export { b } from './b.ts'");
        upsert_ts(&host, "/project/b.ts", "export { c as b } from './c.ts'");
        upsert_ts(&host, "/project/c.ts", "export const c = 42");

        // Should follow a→b→c (no depth limit, cycle detection only)
        let result = host.get_export_span_follow_reexports("/project/a.ts", "b");
        assert!(result.is_some(), "should follow the chain");
        let (canonical_id, _, _) = result.unwrap();
        assert_eq!(canonical_id, "/project/c.ts", "should reach c.ts");
    }

    #[test]
    fn get_export_span_local_export_unchanged() {
        let host = make_host();

        upsert_ts(&host, "utils.ts", "export function foo() { return 1 }");

        // Local export — no re-export, returns span in same file
        let result = host.get_export_span_follow_reexports("utils.ts", "foo");

        assert!(result.is_some(), "should find local export");
        let (canonical_id, start, end) = result.unwrap();
        assert_eq!(
            canonical_id, "utils.ts",
            "local export should return same file"
        );
        assert!(start < end, "should have a valid span");
    }

    #[test]
    fn follow_reexport_cycle_same_binding() {
        let host = make_host();

        // A re-exports foo from B, B re-exports foo from A → cycle
        upsert_ts(&host, "a.ts", "export { foo } from './b.ts'");
        upsert_ts(&host, "b.ts", "export { foo } from './a.ts'");

        let result = host.get_export_span_follow_reexports("a.ts", "foo");
        assert!(
            result.is_none(),
            "cycle on same binding should return None, got: {result:?}"
        );
    }

    #[test]
    fn follow_reexport_same_file_different_binding() {
        let host = make_host();

        // A re-exports foo from B (as foo→bar), B re-exports bar from A (as bar→baz),
        // A has a local baz export. Different bindings each hop → not a cycle.
        upsert_ts(
            &host,
            "/project/a.ts",
            "export { bar as foo } from './b.ts'\nexport const baz = 99",
        );
        upsert_ts(
            &host,
            "/project/b.ts",
            "export { baz as bar } from './a.ts'",
        );

        let result = host.get_export_span_follow_reexports("/project/a.ts", "foo");
        assert!(
            result.is_some(),
            "different bindings through same files should resolve, not be treated as cycle"
        );
        let (canonical_id, _, _) = result.unwrap();
        assert_eq!(
            canonical_id, "/project/a.ts",
            "should resolve to a.ts local baz export"
        );
    }

    #[test]
    fn follow_reexport_indirect_cycle() {
        let host = make_host();

        // A→B→C→A with same binding name "x" at each hop
        upsert_ts(&host, "a.ts", "export { x } from './b.ts'");
        upsert_ts(&host, "b.ts", "export { x } from './c.ts'");
        upsert_ts(&host, "c.ts", "export { x } from './a.ts'");

        let result = host.get_export_span_follow_reexports("a.ts", "x");
        assert!(
            result.is_none(),
            "indirect 3-file cycle should return None, got: {result:?}"
        );
    }

    #[test]
    fn follow_reexport_deep_chain_no_limit() {
        let host = make_host();

        // 15-hop chain: f0→f1→f2→...→f14→terminal.ts
        // Each hop renames: val0→val1→...→val14→val
        for i in 0..15 {
            let next = if i < 14 {
                format!("f{}.ts", i + 1)
            } else {
                "terminal.ts".to_string()
            };
            let next_binding = if i < 14 {
                format!("val{}", i + 1)
            } else {
                "val".to_string()
            };
            let src = format!(
                "export {{ {} as val{} }} from './{}'",
                next_binding, i, next
            );
            upsert_ts(&host, &format!("/project/f{}.ts", i), &src);
        }
        upsert_ts(&host, "/project/terminal.ts", "export const val = 'done'");

        let result = host.get_export_span_follow_reexports("/project/f0.ts", "val0");
        assert!(
            result.is_some(),
            "15-hop chain should resolve without depth limit"
        );
        let (canonical_id, start, end) = result.unwrap();
        assert_eq!(
            canonical_id, "/project/terminal.ts",
            "should reach terminal.ts"
        );
        assert!(start < end, "should have a valid span");
    }

    fn compile_template(host: &VerterHost, id: &str) {
        host.get_virtual_file(crate::types::VirtualQuery {
            raw_id: Some(format!("{id}?vue&type=template")),
            canonical_id: None,
            node_kind: None,
            compile_profile: crate::types::CompileProfile::default(),
        })
        .unwrap();
    }

    #[test]
    fn prop_shorthand_detected() {
        let host = make_host();
        upsert_vue(
            &host,
            "MyComp.vue",
            "<script setup>\ndefineProps<{ bar: number }>()\n</script>\n<template><div/></template>",
        );
        // `:bar` with no value → shorthand; `:bar="bar"` → not shorthand
        upsert_vue(
            &host,
            "App.vue",
            r#"<script setup>
import MyComp from './MyComp.vue'
const bar = 1
</script>
<template><MyComp :bar /><MyComp :bar="bar" /></template>"#,
        );
        compile_template(&host, "App.vue");

        let analysis = host.get_analysis("App.vue").unwrap();
        let tmpl = analysis
            .template
            .as_ref()
            .expect("should have template analysis");
        assert!(
            tmpl.components.len() >= 2,
            "should have at least 2 component usages, got {}",
            tmpl.components.len()
        );

        // First usage: `:bar` (shorthand)
        let comp1 = &tmpl.components[0];
        assert_eq!(comp1.props.len(), 1, "first usage has 1 prop");
        assert!(
            comp1.props[0].is_shorthand,
            "`:bar` (no value) should be shorthand"
        );

        // Second usage: `:bar="bar"` (not shorthand)
        let comp2 = &tmpl.components[1];
        assert_eq!(comp2.props.len(), 1, "second usage has 1 prop");
        assert!(
            !comp2.props[0].is_shorthand,
            "`:bar=\"bar\"` should NOT be shorthand"
        );
    }

    #[test]
    fn prop_name_span_covers_name() {
        let host = make_host();
        upsert_vue(
            &host,
            "MyComp.vue",
            "<script setup>\ndefineProps<{ bar: number }>()\n</script>\n<template><div/></template>",
        );
        let sfc = r#"<script setup>
import MyComp from './MyComp.vue'
const bar = 1
</script>
<template><MyComp :bar="bar" foo="static" /></template>"#;
        upsert_vue(&host, "App.vue", sfc);
        compile_template(&host, "App.vue");

        let analysis = host.get_analysis("App.vue").unwrap();
        let tmpl = analysis
            .template
            .as_ref()
            .expect("should have template analysis");
        assert!(!tmpl.components.is_empty());

        let comp = &tmpl.components[0];
        // Find the bound prop `:bar`
        let bound_prop = comp.props.iter().find(|p| p.name == "bar").unwrap();
        let source = host.get_source("App.vue").unwrap();
        let name_text =
            &source[bound_prop.name_span.start as usize..bound_prop.name_span.end as usize];
        assert_eq!(
            name_text, "bar",
            "name_span should cover 'bar' (the arg, not ':')"
        );
        assert!(
            bound_prop.name_span.start >= bound_prop.span.start,
            "name_span should be within the full prop span"
        );

        // Find the static prop `foo`
        let static_prop = comp.props.iter().find(|p| p.name == "foo").unwrap();
        let name_text =
            &source[static_prop.name_span.start as usize..static_prop.name_span.end as usize];
        assert_eq!(name_text, "foo", "static prop name_span should cover 'foo'");
        assert!(
            !static_prop.is_shorthand,
            "static prop should not be shorthand"
        );
    }

    #[test]
    fn arc_shared_fields_are_pointer_equal() {
        let host = make_host();
        upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);

        let a1 = host.get_analysis("App.vue").unwrap();
        let a2 = host.get_analysis("App.vue").unwrap();

        // Arc-shared fields should be pointer-equal between two calls
        // on the same unchanged file.
        assert!(
            Arc::ptr_eq(&a1.module_references, &a2.module_references),
            "module_references should be Arc-shared (pointer equal)"
        );
        assert!(
            Arc::ptr_eq(&a1.macros, &a2.macros),
            "macros should be Arc-shared (pointer equal)"
        );
        assert!(
            Arc::ptr_eq(&a1.styles, &a2.styles),
            "styles should be Arc-shared (pointer equal)"
        );
        assert!(
            Arc::ptr_eq(&a1.vue_api_calls, &a2.vue_api_calls),
            "vue_api_calls should be Arc-shared (pointer equal)"
        );
    }

    #[test]
    fn enriched_imports_do_not_affect_stored_data() {
        let host = make_host();
        upsert_vue(
            &host,
            "/project/Child.vue",
            "<script setup>\nconst x = 1\n</script>\n<template><div/></template>",
        );
        upsert_vue(
            &host,
            "/project/Parent.vue",
            "<script setup>\nimport Child from './Child.vue'\n</script>\n<template><Child/></template>",
        );

        // First call: enriches imports with resolved_canonical_id
        let a1 = host.get_analysis("/project/Parent.vue").unwrap();
        assert!(
            a1.imports[0].resolved_canonical_id.is_some(),
            "enriched import should have resolved_canonical_id"
        );

        // Verify stored data is not mutated by checking that the
        // internal stored imports still have None
        #[cfg(feature = "scheduler")]
        {
            use crate::host_executor::HostSourceData;
            let source_snap = host
                .scheduler
                .try_get_source("/project/Parent.vue")
                .expect("scheduler should have Parent.vue");
            let hd = source_snap
                .downcast_data::<HostSourceData>()
                .expect("source data should be HostSourceData");
            assert!(
                hd.parse.script_analysis.imports[0]
                    .resolved_canonical_id
                    .is_none(),
                "stored import should NOT be mutated by get_analysis enrichment"
            );
        }
        #[cfg(not(feature = "scheduler"))]
        {
            let files = crate::shared::read_lock(&host.files);
            let entry = files.get("/project/Parent.vue").unwrap();
            assert!(
                entry.script_analysis.imports[0]
                    .resolved_canonical_id
                    .is_none(),
                "stored import should NOT be mutated by get_analysis enrichment"
            );
        }
    }

    #[test]
    fn get_analysis_batch_returns_all_existing() {
        let host = make_host();
        upsert_vue(
            &host,
            "A.vue",
            "<script setup>\nconst a = 1\n</script>\n<template><div/></template>",
        );
        upsert_vue(
            &host,
            "B.vue",
            "<script setup>\nconst b = 2\n</script>\n<template><div/></template>",
        );

        let results = host.get_analysis_batch(&["A.vue", "B.vue", "NonExistent.vue"]);
        assert_eq!(results.len(), 2, "should return only existing files");
        assert!(
            results.iter().any(|(id, _)| id == "A.vue"),
            "should contain A.vue"
        );
        assert!(
            results.iter().any(|(id, _)| id == "B.vue"),
            "should contain B.vue"
        );
        // Negative: should NOT contain non-existent
        assert!(
            !results.iter().any(|(id, _)| id == "NonExistent.vue"),
            "should not contain non-existent file"
        );
    }

    #[test]
    fn get_analysis_batch_matches_individual() {
        let host = make_host();
        upsert_vue(
            &host,
            "A.vue",
            "<script setup>\nimport { ref } from 'vue'\nconst x = ref(0)\n</script>\n<template><div/></template>",
        );

        let individual = host.get_analysis("A.vue").unwrap();
        let batch = host.get_analysis_batch(&["A.vue"]);
        assert_eq!(batch.len(), 1);
        let (_, batch_snap) = &batch[0];

        assert_eq!(
            individual.bindings.len(),
            batch_snap.bindings.len(),
            "batch bindings count should match individual"
        );
        assert_eq!(
            individual.imports.len(),
            batch_snap.imports.len(),
            "batch imports count should match individual"
        );
        assert_eq!(
            individual.script_flags, batch_snap.script_flags,
            "batch script_flags should match individual"
        );
    }

    #[test]
    fn get_analysis_batch_empty_returns_empty() {
        let host = make_host();
        let results = host.get_analysis_batch(&[]);
        assert!(results.is_empty(), "empty batch should return empty vec");
    }

    // ── Export signature tests ──────────────────────────────────────

    fn upsert_ts_result(host: &VerterHost, id: &str, src: &str) -> crate::HostUpdateResult {
        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap()
    }

    /// @ai-generated - upsert of .ts file returns export signatures
    #[test]
    fn upsert_returns_export_signatures_for_ts() {
        let host = make_host();
        let result = upsert_ts_result(
            &host,
            "index.ts",
            r#"export const foo = 1;
export type Bar = string;
export { default as Button } from './Button.vue';
"#,
        );

        assert!(
            !result.export_signatures.is_empty(),
            "upsert should return export signatures for .ts files"
        );

        let foo_sig = result
            .export_signatures
            .iter()
            .find(|s| s.name == "foo")
            .expect("should have 'foo' export");
        assert!(!foo_sig.is_type, "foo is a value export");
        assert!(
            foo_sig.reexport_source.is_none(),
            "foo is local, not a re-export"
        );

        let bar_sig = result
            .export_signatures
            .iter()
            .find(|s| s.name == "Bar")
            .expect("should have 'Bar' export");
        assert!(bar_sig.is_type, "Bar is a type export");

        let button_sig = result
            .export_signatures
            .iter()
            .find(|s| s.name == "Button")
            .expect("should have 'Button' re-export");
        assert_eq!(
            button_sig.reexport_source.as_deref(),
            Some("./Button.vue"),
            "Button re-export source should be './Button.vue'"
        );
        assert_eq!(
            button_sig.reexport_local.as_deref(),
            Some("default"),
            "Button re-export local name should be 'default'"
        );
    }

    /// @ai-generated - get_analysis includes export signatures
    #[test]
    fn get_analysis_includes_export_signatures() {
        let host = make_host();
        upsert_ts(
            &host,
            "utils.ts",
            "export function helper() { return 1; }\nexport type Util = number;",
        );

        let analysis = host.get_analysis("utils.ts").unwrap();
        assert!(
            !analysis.export_signatures.is_empty(),
            "analysis should include export signatures"
        );

        let helper_sig = analysis
            .export_signatures
            .iter()
            .find(|s| s.name == "helper")
            .expect("should have 'helper' export");
        assert!(!helper_sig.is_type);

        let util_sig = analysis
            .export_signatures
            .iter()
            .find(|s| s.name == "Util")
            .expect("should have 'Util' export");
        assert!(util_sig.is_type);
    }

    /// @ai-generated - resolve_exports follows re-export chains
    #[test]
    fn resolve_exports_follows_reexport_chains() {
        let host = make_host();

        upsert_vue(
            &host,
            "/project/Button.vue",
            "<script setup>\ndefineProps({ label: String })\n</script>\n<template><button>{{ label }}</button></template>",
        );

        upsert_ts(
            &host,
            "/project/components/index.ts",
            "export { default as Button } from './Button.vue';",
        );

        // Set up dependency so ./Button.vue resolves from components/index.ts
        host.set_import_dependencies(
            "/project/components/index.ts",
            vec![crate::DependencyResolution {
                specifier: "./Button.vue".to_string(),
                resolved_canonical_id: Some("/project/Button.vue".to_string()),
                possible_canonical_ids: vec![],
            }],
        );

        let exports = host.resolve_exports("/project/components/index.ts");
        assert!(
            !exports.is_empty(),
            "barrel file should have resolved exports"
        );

        let button = exports
            .iter()
            .find(|e| e.name == "Button")
            .expect("should have 'Button' resolved export");
        assert_eq!(
            button.source_canonical_id.as_deref(),
            Some("/project/Button.vue"),
            "Button should resolve to Button.vue"
        );
        assert_eq!(
            button.source_name, "default",
            "Button maps to 'default' in the source file"
        );
    }

    /// @ai-generated - resolve_exports handles direct local exports
    #[test]
    fn resolve_exports_local_exports() {
        let host = make_host();
        upsert_ts(
            &host,
            "utils.ts",
            "export const FOO = 1;\nexport type Bar = string;",
        );

        let exports = host.resolve_exports("utils.ts");
        assert_eq!(exports.len(), 2, "should have 2 exports");

        let foo = exports.iter().find(|e| e.name == "FOO").unwrap();
        assert!(
            foo.source_canonical_id.is_none(),
            "local export has no source file"
        );
        assert_eq!(foo.source_name, "FOO");
        assert!(!foo.is_type);

        let bar = exports.iter().find(|e| e.name == "Bar").unwrap();
        assert!(bar.is_type);
    }

    /// @ai-generated - resolve_exports handles wildcard re-exports
    #[test]
    fn resolve_exports_wildcard_reexports() {
        let host = make_host();

        upsert_ts(
            &host,
            "/project/types.ts",
            "export type Foo = string;\nexport type Bar = number;",
        );
        upsert_ts(&host, "/project/index.ts", "export * from './types';");

        host.set_import_dependencies(
            "/project/index.ts",
            vec![crate::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/project/types.ts".to_string()),
                possible_canonical_ids: vec![],
            }],
        );

        let exports = host.resolve_exports("/project/index.ts");
        assert!(
            exports.iter().any(|e| e.name == "Foo"),
            "wildcard re-export should include Foo"
        );
        assert!(
            exports.iter().any(|e| e.name == "Bar"),
            "wildcard re-export should include Bar"
        );

        let foo = exports.iter().find(|e| e.name == "Foo").unwrap();
        assert_eq!(
            foo.source_canonical_id.as_deref(),
            Some("/project/types.ts"),
            "Foo should trace back to types.ts"
        );
    }

    /// @ai-generated - resolve_exports detects circular re-exports
    #[test]
    fn resolve_exports_circular_protection() {
        let host = make_host();

        upsert_ts(&host, "a.ts", "export * from './b';");
        upsert_ts(&host, "b.ts", "export * from './a';");

        host.set_import_dependencies(
            "a.ts",
            vec![crate::DependencyResolution {
                specifier: "./b".to_string(),
                resolved_canonical_id: Some("b.ts".to_string()),
                possible_canonical_ids: vec![],
            }],
        );
        host.set_import_dependencies(
            "b.ts",
            vec![crate::DependencyResolution {
                specifier: "./a".to_string(),
                resolved_canonical_id: Some("a.ts".to_string()),
                possible_canonical_ids: vec![],
            }],
        );

        // Should not infinite loop
        let exports = host.resolve_exports("a.ts");
        // The result is empty because both files only re-export each other with no local exports
        assert!(
            exports.is_empty(),
            "circular re-exports with no local exports should return empty"
        );
    }

    /// @ai-generated - resolve_exports multi-level barrel chain
    #[test]
    fn resolve_exports_multi_level_barrel() {
        let host = make_host();

        upsert_ts(&host, "/project/deep.ts", "export const DEEP = 42;");
        upsert_ts(&host, "/project/mid.ts", "export { DEEP } from './deep';");
        upsert_ts(&host, "/project/top.ts", "export { DEEP } from './mid';");

        host.set_import_dependencies(
            "/project/mid.ts",
            vec![crate::DependencyResolution {
                specifier: "./deep".to_string(),
                resolved_canonical_id: Some("/project/deep.ts".to_string()),
                possible_canonical_ids: vec![],
            }],
        );
        host.set_import_dependencies(
            "/project/top.ts",
            vec![crate::DependencyResolution {
                specifier: "./mid".to_string(),
                resolved_canonical_id: Some("/project/mid.ts".to_string()),
                possible_canonical_ids: vec![],
            }],
        );

        let exports = host.resolve_exports("/project/top.ts");
        let deep = exports
            .iter()
            .find(|e| e.name == "DEEP")
            .expect("should have DEEP");
        assert_eq!(
            deep.source_canonical_id.as_deref(),
            Some("/project/deep.ts"),
            "should trace through two levels to deep.ts"
        );
    }

    #[test]
    fn get_semantic_hash_returns_hash_for_loaded_file() {
        let host = make_host();
        upsert_vue(&host, "App.vue", "<template><div>hi</div></template>");
        let hash = host.get_semantic_hash("App.vue");
        assert!(hash.is_some(), "loaded file should return a semantic hash");
        assert_ne!(hash.unwrap(), [0u8; 16], "hash should not be all zeros");
    }

    #[test]
    fn get_semantic_hash_returns_none_for_missing_file() {
        let host = make_host();
        assert!(
            host.get_semantic_hash("nonexistent.vue").is_none(),
            "missing file should return None"
        );
    }

    #[test]
    fn get_semantic_hash_changes_on_content_change() {
        let host = make_host();
        upsert_vue(&host, "App.vue", "<template><div>a</div></template>");
        let h1 = host.get_semantic_hash("App.vue").unwrap();
        upsert_vue(&host, "App.vue", "<template><div>b</div></template>");
        let h2 = host.get_semantic_hash("App.vue").unwrap();
        assert_ne!(h1, h2, "semantic hash should change when content changes");
    }

    #[test]
    fn resolve_imported_type_from_ts_dep() {
        let host = make_host();
        // Upsert the .ts type file
        upsert_ts(
            &host,
            "/types.ts",
            "export interface ButtonProps { label: string; size?: number }",
        );
        // Upsert the .vue file that imports from ./types
        upsert_vue(
            &host,
            "/Button.vue",
            r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script><template><div /></template>"#,
        );

        let types = host.resolve_imported_types("/Button.vue");
        assert_eq!(types.len(), 1, "should resolve one imported type");
        assert_eq!(types[0].name, "ButtonProps");
        assert!(
            types[0].expanded.contains("label"),
            "expanded should contain 'label', got: {}",
            types[0].expanded
        );
        assert!(
            types[0].expanded.contains("size"),
            "expanded should contain 'size', got: {}",
            types[0].expanded
        );
        // Negative: should not return empty or unresolved
        assert!(
            !types[0].expanded.is_empty(),
            "expanded should not be empty"
        );
    }

    #[test]
    fn resolve_imported_types_returns_empty_for_no_deps() {
        let host = make_host();
        upsert_vue(
            &host,
            "/Simple.vue",
            r#"<script setup lang="ts">
defineProps<{ count: number }>()
</script><template><div /></template>"#,
        );
        let types = host.resolve_imported_types("/Simple.vue");
        assert!(
            types.is_empty(),
            "should return empty when no imported types"
        );
    }

    // ═══════════════════════════════════════════════════════════
    // enrich_imported_types tests
    // ═══════════════════════════════════════════════════════════

    fn make_deep_host() -> VerterHost {
        VerterHost::new_standalone(HostConfig {
            deep_macro_resolution_type: true,
            ..HostConfig::default()
        })
    }

    fn upsert_non_sfc(host: &VerterHost, id: &str, src: &str) {
        host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();
    }

    /// @ai-generated - enrich_imported_types populates prop_fields from imported interface
    #[test]
    fn enrich_basic_imported_interface() {
        let host = make_deep_host();
        upsert_non_sfc(
            &host,
            "/src/types.ts",
            "export interface Props { label: string }",
        );
        upsert_vue(
            &host,
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        );

        let analysis = host.get_analysis("/src/Comp.vue").unwrap();
        let dp = analysis
            .macros
            .iter()
            .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps)
            .expect("should have DefineProps macro");
        assert!(
            !dp.prop_fields.is_empty(),
            "prop_fields should be populated by enrichment"
        );
        assert_eq!(dp.prop_fields[0].name, "label");
        assert!(
            dp.resolved_local_types.iter().any(|r| r.name == "Props"),
            "resolved_local_types should include 'Props'"
        );
    }

    /// @ai-generated - enrichment is skipped when deep_macro_resolution_type is false
    #[test]
    fn enrich_skips_when_disabled() {
        let host = make_host(); // default config, deep_macro_resolution_type = false
        upsert_non_sfc(
            &host,
            "/src/types.ts",
            "export interface Props { label: string }",
        );
        upsert_vue(
            &host,
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        );

        let analysis = host.get_analysis("/src/Comp.vue").unwrap();
        let dp = analysis
            .macros
            .iter()
            .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps)
            .expect("should have DefineProps macro");
        assert!(
            dp.prop_fields.is_empty(),
            "prop_fields should be empty when enrichment is disabled"
        );
    }

    /// @ai-generated - intersection types merge props from all deps
    #[test]
    fn enrich_intersection_merges_all_deps() {
        let host = make_deep_host();
        upsert_non_sfc(&host, "/src/a.ts", "export interface A { x: string }");
        upsert_non_sfc(&host, "/src/b.ts", "export interface B { y: number }");
        upsert_vue(
            &host,
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { A } from './a'
import type { B } from './b'
defineProps<A & B>()
</script>
<template><div /></template>"#,
        );

        let analysis = host.get_analysis("/src/Comp.vue").unwrap();
        let dp = analysis
            .macros
            .iter()
            .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps)
            .expect("should have DefineProps macro");
        let names: Vec<&str> = dp.prop_fields.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"x"), "should have 'x' from A: {:?}", names);
        assert!(names.contains(&"y"), "should have 'y' from B: {:?}", names);
    }

    /// @ai-generated - call-signature emit payloads are wrapped in brackets
    #[test]
    fn enrich_emit_call_signature_wraps_brackets() {
        let host = make_deep_host();
        upsert_non_sfc(
            &host,
            "/src/events.ts",
            "export interface Events { (e: 'change', id: number): void }",
        );
        upsert_vue(
            &host,
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { Events } from './events'
defineEmits<Events>()
</script>
<template><div /></template>"#,
        );

        let analysis = host.get_analysis("/src/Comp.vue").unwrap();
        let de = analysis
            .macros
            .iter()
            .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineEmits)
            .expect("should have DefineEmits macro");
        assert!(!de.emit_fields.is_empty(), "should have emit fields");
        let change = de.emit_fields.iter().find(|e| e.name == "change");
        assert!(change.is_some(), "should have 'change' emit");
        let payload = change.unwrap().payload_type.as_deref().unwrap_or("");
        assert!(
            payload.starts_with('[') && payload.ends_with(']'),
            "call-signature payload should be wrapped in brackets, got: {payload}"
        );
    }

    /// @ai-generated - imported defineSlots extracts bindings from type_text
    #[test]
    fn enrich_slot_bindings_from_imported_type() {
        let host = make_deep_host();
        upsert_non_sfc(
            &host,
            "/src/slots.ts",
            "export interface Slots { default: (props: { row: string; index: number }) => any }",
        );
        upsert_vue(
            &host,
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { Slots } from './slots'
defineSlots<Slots>()
</script>
<template><div /></template>"#,
        );

        let analysis = host.get_analysis("/src/Comp.vue").unwrap();
        let ds = analysis
            .macros
            .iter()
            .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineSlots)
            .expect("should have DefineSlots macro");
        assert!(!ds.slot_fields.is_empty(), "should have slot fields");
        let default_slot = ds.slot_fields.iter().find(|s| s.name == "default");
        assert!(default_slot.is_some(), "should have 'default' slot");
        let bindings = &default_slot.unwrap().bindings;
        assert!(
            !bindings.is_empty(),
            "slot should have bindings from the imported type"
        );
        let binding_names: Vec<&str> = bindings.iter().map(|b| b.name.as_str()).collect();
        assert!(
            binding_names.contains(&"row"),
            "should have 'row' binding: {:?}",
            binding_names
        );
        assert!(
            binding_names.contains(&"index"),
            "should have 'index' binding: {:?}",
            binding_names
        );
    }

    /// @ai-generated - method-style slot signatures (call signatures) are also captured
    #[test]
    fn enrich_slot_method_style() {
        let host = make_deep_host();
        upsert_non_sfc(
            &host,
            "/src/slots.ts",
            "export interface Slots { default(props: { item: string }): any; header(props: { title: string }): any }",
        );
        upsert_vue(
            &host,
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { Slots } from './slots'
defineSlots<Slots>()
</script>
<template><div /></template>"#,
        );

        let analysis = host.get_analysis("/src/Comp.vue").unwrap();
        let ds = analysis
            .macros
            .iter()
            .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineSlots)
            .expect("should have DefineSlots macro");
        let slot_names: Vec<&str> = ds.slot_fields.iter().map(|s| s.name.as_str()).collect();
        assert!(
            slot_names.contains(&"default"),
            "should have 'default' slot from method signature: {:?}",
            slot_names
        );
        assert!(
            slot_names.contains(&"header"),
            "should have 'header' slot from method signature: {:?}",
            slot_names
        );
    }

    /// @ai-generated - nested types from same import source added to resolved_local_types
    #[test]
    fn enrich_nested_type_expansion() {
        let host = make_deep_host();
        upsert_non_sfc(
            &host,
            "/src/types.ts",
            r#"export type Status = 'active' | 'inactive'
export interface Props { name: string; status: Status }"#,
        );
        upsert_vue(
            &host,
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        );

        let analysis = host.get_analysis("/src/Comp.vue").unwrap();
        let dp = analysis
            .macros
            .iter()
            .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps)
            .expect("should have DefineProps macro");
        let rlt_names: Vec<&str> = dp
            .resolved_local_types
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert!(
            rlt_names.contains(&"Props"),
            "should have 'Props' in resolved_local_types: {:?}",
            rlt_names
        );
        assert!(
            rlt_names.contains(&"Status"),
            "should have nested 'Status' in resolved_local_types: {:?}",
            rlt_names
        );
    }

    /// @ai-generated - slot return types are extracted for strict slots support
    #[test]
    fn enrich_slot_return_type_property_style() {
        let host = make_deep_host();
        upsert_non_sfc(
            &host,
            "/src/slots.ts",
            "export interface Slots { default: (props: { row: string }) => VNode[]; header: (props: {}) => any }",
        );
        upsert_vue(
            &host,
            "/src/Comp.vue",
            r#"<script setup lang="ts">
import type { Slots } from './slots'
defineSlots<Slots>()
</script>
<template><div /></template>"#,
        );

        let analysis = host.get_analysis("/src/Comp.vue").unwrap();
        let ds = analysis
            .macros
            .iter()
            .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineSlots)
            .expect("should have DefineSlots macro");

        let default_slot = ds.slot_fields.iter().find(|s| s.name == "default").unwrap();
        assert_eq!(
            default_slot.return_type.as_deref(),
            Some("VNode[]"),
            "default slot should have return type VNode[]"
        );

        let header_slot = ds.slot_fields.iter().find(|s| s.name == "header").unwrap();
        assert_eq!(
            header_slot.return_type.as_deref(),
            Some("any"),
            "header slot should have return type any"
        );
    }

    /// @ai-generated - local defineSlots with return types
    #[test]
    fn local_slot_return_type_property_style() {
        let host = make_host();
        upsert_vue(
            &host,
            "/Comp.vue",
            r#"<script setup lang="ts">
defineSlots<{
  default: (props: { item: string }) => VNode[]
  header: (props: {}) => any
}>()
</script>
<template><div /></template>"#,
        );

        let analysis = host.get_analysis("/Comp.vue").unwrap();
        let ds = analysis
            .macros
            .iter()
            .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineSlots)
            .expect("should have DefineSlots macro");

        let default_slot = ds.slot_fields.iter().find(|s| s.name == "default").unwrap();
        assert_eq!(
            default_slot.return_type.as_deref(),
            Some("VNode[]"),
            "local default slot should have return type"
        );
    }

    /// @ai-generated - local defineSlots with method-style return types
    #[test]
    fn local_slot_return_type_method_style() {
        let host = make_host();
        upsert_vue(
            &host,
            "/Comp.vue",
            r#"<script setup lang="ts">
defineSlots<{
  default(props: { item: string }): VNode[]
}>()
</script>
<template><div /></template>"#,
        );

        let analysis = host.get_analysis("/Comp.vue").unwrap();
        let ds = analysis
            .macros
            .iter()
            .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineSlots)
            .expect("should have DefineSlots macro");

        let default_slot = ds.slot_fields.iter().find(|s| s.name == "default").unwrap();
        assert_eq!(
            default_slot.return_type.as_deref(),
            Some("VNode[]"),
            "method-style slot should have return type"
        );
    }

    // ═══════════════════════════════════════════════════════════
    // Template slots via lazy analysis (compute_template_analysis_if_missing)
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - template slots detected via lazy META compilation
    #[test]
    fn template_slots_via_analysis_only() {
        let host = make_host(); // analysis_level: Full → scope includes template
        upsert_vue(
            &host,
            "/Comp.vue",
            "<script setup>\n</script>\n<template><div><slot /></div></template>",
        );

        let analysis = host.get_analysis("/Comp.vue").unwrap();
        let tpl = analysis
            .template
            .expect("template analysis should be populated");
        assert_eq!(tpl.defined_slots.len(), 1);
        assert_eq!(tpl.defined_slots[0].name, "default");
    }

    /// @ai-generated - named slots detected via lazy META compilation
    #[test]
    fn template_slots_named() {
        let host = make_host();
        upsert_vue(
            &host,
            "/Comp.vue",
            r#"<template><slot name="header" /><slot /></template>"#,
        );

        let analysis = host.get_analysis("/Comp.vue").unwrap();
        let tpl = analysis
            .template
            .expect("template analysis should be populated");
        assert_eq!(tpl.defined_slots.len(), 2);
        assert!(tpl.defined_slots.iter().any(|s| s.name == "header"));
        assert!(tpl.defined_slots.iter().any(|s| s.name == "default"));
    }

    /// @ai-generated - template analysis not computed when scope doesn't include template
    #[test]
    fn template_slots_not_computed_on_lazy_host() {
        let host = make_lazy_host(); // analysis_level: None → scope excludes template
        upsert_vue(
            &host,
            "/Comp.vue",
            "<script setup>\n</script>\n<template><div><slot /></div></template>",
        );

        let analysis = host.get_analysis("/Comp.vue").unwrap();
        assert!(
            analysis.template.is_none(),
            "template should not be computed when scope excludes it"
        );
    }

    /// @ai-generated - persisted template analysis reused on second call
    #[test]
    fn template_slots_persisted_across_calls() {
        let host = make_host();
        upsert_vue(
            &host,
            "/Comp.vue",
            "<script setup>\n</script>\n<template><div><slot /></div></template>",
        );

        let a1 = host.get_analysis("/Comp.vue").unwrap();
        assert!(a1.template.is_some(), "first call should compute template");

        let a2 = host.get_analysis("/Comp.vue").unwrap();
        assert!(
            a2.template.is_some(),
            "second call should reuse persisted template"
        );
        assert_eq!(
            a2.template.unwrap().defined_slots.len(),
            1,
            "persisted template should have the slot"
        );
    }

    /// @ai-generated - template slots computed even when type deps are unresolved
    #[test]
    fn template_slots_with_unresolved_type_deps() {
        let host = make_deep_host();
        // Don't upsert ./types.ts — the dep is unresolved
        upsert_vue(
            &host,
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { Foo } from './types'
defineProps<Foo>()
</script>
<template><slot /></template>"#,
        );

        let analysis = host.get_analysis("/Comp.vue").unwrap();
        let tpl = analysis
            .template
            .expect("template should be computed even with unresolved type deps");
        assert_eq!(
            tpl.defined_slots.len(),
            1,
            "should detect the <slot> despite unresolved type dep"
        );
    }
}

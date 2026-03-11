//! `impl VerterHost` — upsert and style-override methods.
//!
//! Contains [`VerterHost::upsert`] and [`VerterHost::apply_style_overrides`],
//! which handle file ingestion, change detection, cache invalidation, and
//! style override application.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use crate::cache::{invalidate_nodes, sorted_nodes};
use crate::hash::{compile_profile_hash, content_override_hash, style_override_hash};
use crate::id::{canonicalize_id, render_ids};
use crate::parse::{parse_non_sfc_snapshot, parse_vue_snapshot};
use crate::shared::write_lock;
use crate::types::*;
use crate::upsert::{build_upsert_result, compute_upsert_changes, UpsertResultData};
use crate::VerterHost;

impl VerterHost {
    /// Insert or update a file in the host.
    ///
    /// Parses the source, computes content hashes, detects granular slice-level
    /// changes, invalidates affected compile slots, and returns a
    /// [`HostUpdateResult`] describing which virtual nodes changed or were removed.
    pub fn upsert(&self, req: UpsertRequest) -> Result<HostUpdateResult, HostError> {
        #[cfg(feature = "host_metrics")]
        self.metrics
            .upserts
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let canonical_id = req
            .canonical_id
            .clone()
            .unwrap_or_else(|| canonicalize_id(&req.input_id).into_owned());

        let parse_start = Instant::now();
        let (mut snapshot, cached_parse) = match req.file_kind {
            FileKind::VueSfc => {
                let (snap, parsed) =
                    parse_vue_snapshot(&canonical_id, &req.source, self.config.effective_scope());
                (snap, Some(parsed))
            }
            FileKind::NonSfc => (parse_non_sfc_snapshot(&canonical_id, &req.source), None),
        };
        let parse_duration_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
        #[cfg(feature = "host_metrics")]
        self.metrics.slice_hash_time_us_total.fetch_add(
            parse_start.elapsed().as_micros() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );

        let mut alias_set: BTreeSet<String> = req
            .aliases
            .iter()
            .map(|a| canonicalize_id(a).into_owned())
            .collect();
        alias_set.insert(canonicalize_id(&req.input_id).into_owned());
        alias_set.insert(canonical_id.clone());

        let new_deps: BTreeSet<String> = snapshot
            .external_requests
            .iter()
            .map(|r| r.resolved_canonical_id.clone())
            .chain(
                snapshot
                    .script_analysis
                    .imports
                    .iter()
                    .filter(|imp| imp.source.starts_with('.'))
                    .map(|imp| crate::id::resolve_external(&canonical_id, &imp.source)),
            )
            .collect();

        // Single write lock: read old state + compute changes + apply update atomically.
        // This eliminates the TOCTOU race between read_old_snapshot and apply_entry_update.
        let (
            changes,
            prev_nodes,
            old_meta,
            old_aliases,
            old_deps,
            old_export_signatures,
            result_data,
            new_export_signatures,
        ) = {
            let mut files = write_lock(&self.files);

            let old_entry = files.get(&canonical_id);
            let changes = compute_upsert_changes(old_entry, &snapshot);

            // Fast path: nothing changed — skip all cloning, entry update, and result building.
            // The existing entry is already up-to-date (source, meta, analysis are identical).
            if let (false, Some(existing)) = (changes.changed, old_entry) {
                // Aliases may have been updated — update the alias map.
                let old_aliases = existing.aliases.clone();
                drop(files);
                self.update_alias_map(&canonical_id, &old_aliases, &alias_set);
                return Ok(HostUpdateResult {
                    canonical_id,
                    changed: false,
                    slice_changes: SliceChanges::default(),
                    changed_virtual_nodes: Vec::new(),
                    removed_virtual_nodes: Vec::new(),
                    changed_virtual_ids: Vec::new(),
                    removed_virtual_ids: Vec::new(),
                    changed_lsp_ids: Vec::new(),
                    removed_lsp_ids: Vec::new(),
                    diagnostics: DiagnosticsSnapshot::default(),
                    external_source_requests: Vec::new(),
                    import_specifiers: Vec::new(),
                    module_references: Vec::new(),
                    preprocessor_requests: Vec::new(),
                    export_signatures: Vec::new(),
                    parse_duration_ms,
                });
            }

            let prev_nodes = old_entry.map(|e| e.all_virtual_nodes()).unwrap_or_default();
            let old_meta = old_entry.map(|e| e.meta.clone()).unwrap_or_default();
            let old_aliases = old_entry.map(|e| e.aliases.clone()).unwrap_or_default();
            let old_deps = old_entry
                .map(|e| e.dependencies.clone())
                .unwrap_or_default();
            let old_export_signatures = old_entry
                .map(|e| e.export_signatures.clone())
                .unwrap_or_default();

            // Clone export_signatures for smart_invalidate_dependents (needs both old & new).
            // Must happen before std::mem::take below which moves them out of the snapshot.
            let export_sigs_for_result = snapshot.export_signatures.clone();
            let new_export_signatures = std::mem::take(&mut snapshot.export_signatures);

            // Extract data needed for build_upsert_result before moving snapshot fields.
            let result_data = UpsertResultData {
                new_meta: snapshot.meta.clone(),
                parse_diagnostics: snapshot.parse_diagnostics.clone(),
                imports: snapshot.script_analysis.imports.clone(),
                module_references: snapshot.script_analysis.module_references.clone(),
                external_requests: snapshot.external_requests.clone(),
                preprocessor_requests: snapshot.preprocessor_requests.clone(),
                export_signatures: export_sigs_for_result,
            };

            // Apply entry update — move snapshot fields instead of cloning.
            let entry = files
                .entry(canonical_id.to_string())
                .or_insert_with(|| FileEntry {
                    canonical_id: canonical_id.to_string(),
                    file_kind: req.file_kind,
                    source: Arc::<str>::from(""),
                    whole_hash: [0; 16],
                    semantic_hash: [0; 16],
                    slices: SliceHashes::default(),
                    descriptor: DescriptorMin::default(),
                    meta: FileMeta::default(),
                    aliases: BTreeSet::new(),
                    dependencies: BTreeSet::new(),
                    dependency_resolutions: HashMap::new(),
                    external_requests: Vec::new(),
                    src_blocks: Vec::new(),
                    parse_diagnostics: DiagnosticsSnapshot::default(),
                    script_analysis: verter_analysis::ScriptAnalysisSnapshot::default(),
                    export_signatures: Vec::new(),
                    style_analyses: Arc::new(Vec::new()),
                    template_analysis: None,
                    arc_script_cache: ScriptAnalysisArcs::default(),
                    resolved_type_hashes: HashMap::new(),
                    style_overrides: HashMap::new(),
                    content_overrides: HashMap::new(),
                    compile_slots: HashMap::new(),
                    latest_diagnostics: HashMap::new(),
                    generation: 0,
                    cached_parse: None,
                    cached_tsc_extract: None,
                });

            entry.file_kind = req.file_kind;
            entry.source = req.source.clone();
            entry.whole_hash = snapshot.whole_hash;
            entry.semantic_hash = snapshot.semantic_hash;
            entry.slices = snapshot.slices;
            entry.descriptor = snapshot.descriptor;
            entry.meta = snapshot.meta;
            entry.external_requests = snapshot.external_requests;
            entry.src_blocks = snapshot.src_blocks;
            entry.parse_diagnostics = snapshot.parse_diagnostics;
            entry.script_analysis = snapshot.script_analysis;
            entry.arc_script_cache = ScriptAnalysisArcs::from_analysis(&entry.script_analysis);
            entry.export_signatures = new_export_signatures.clone();
            entry.style_analyses = Arc::new(snapshot.style_analyses);
            entry.cached_parse = cached_parse.map(Arc::new);
            // Clear TSC extract cache when script, template, or descriptor changes,
            // since the extracted state includes template-derived (root_element_tag)
            // and descriptor-derived (generic_params, attrs_type) data.
            if changes.changed
                && (changes.slice_changes.script_changed
                    || changes.slice_changes.structure_changed
                    || changes.slice_changes.template_changed
                    || changes.slice_changes.descriptor_changed)
            {
                entry.cached_tsc_extract = None;
            }
            entry.generation = entry.generation.saturating_add(1);
            entry.aliases = alias_set.clone();
            entry.dependencies = new_deps.clone();
            // Clear caller-provided resolution records — they'll be re-set by the
            // next set_import_dependencies call from the unplugin/LSP after this upsert.
            entry.dependency_resolutions.clear();

            if changes.changed && changes.semantic_changed {
                entry.latest_diagnostics.clear();
                if changes.slice_changes.script_changed
                    || changes.slice_changes.structure_changed
                    || changes.slice_changes.descriptor_changed
                {
                    entry.compile_slots.clear();
                } else if changes.slice_changes.template_changed {
                    invalidate_nodes(
                        &mut entry.compile_slots,
                        &[VirtualNodeKind::Main, VirtualNodeKind::Template],
                    );
                } else {
                    let mut nodes = Vec::new();
                    for idx in &changes.slice_changes.style_indices_changed {
                        nodes.push(VirtualNodeKind::Style { index: *idx });
                    }
                    for idx in &changes.slice_changes.custom_indices_changed {
                        nodes.push(VirtualNodeKind::Custom { index: *idx });
                    }
                    invalidate_nodes(&mut entry.compile_slots, &nodes);
                }
            }

            (
                changes,
                prev_nodes,
                old_meta,
                old_aliases,
                old_deps,
                old_export_signatures,
                result_data,
                new_export_signatures,
            )
        };

        self.update_alias_map(&canonical_id, &old_aliases, &alias_set);
        self.update_reverse_deps(&canonical_id, &old_deps, &new_deps);
        self.smart_invalidate_dependents(
            &canonical_id,
            &old_export_signatures,
            &new_export_signatures,
        );

        build_upsert_result(
            canonical_id,
            result_data,
            &changes,
            &prev_nodes,
            &old_meta,
            parse_duration_ms,
        )
    }

    /// Apply preprocessor-compiled style overrides for a file+profile.
    ///
    /// Called by the bundler after an external CSS preprocessor (Sass, Less, etc.)
    /// has compiled each `<style>` block. The overrides replace the raw style
    /// content in the compile slot so that `get_virtual_file` serves the
    /// preprocessed CSS. Returns a [`HostUpdateResult`] listing affected style nodes.
    pub fn apply_style_overrides(
        &self,
        req: StyleOverrideRequest,
    ) -> Result<HostUpdateResult, HostError> {
        #[cfg(feature = "host_metrics")]
        self.metrics
            .style_override_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let canonical = self.resolve_alias_or_canonical(&req.canonical_id);
        let profile_hash = compile_profile_hash(&req.compile_profile);
        let mut files = write_lock(&self.files);
        let entry = files
            .get_mut(&canonical)
            .ok_or_else(|| HostError::MissingSource {
                canonical_id: canonical.clone(),
            })?;

        let mut by_index = HashMap::new();
        for ov in req.overrides {
            by_index.insert(ov.index, ov);
        }

        let override_hash = style_override_hash(&by_index);
        let previous_hash = entry
            .style_overrides
            .get(&profile_hash)
            .map(|o| o.hash)
            .unwrap_or(0);

        // Early return if the override hash is identical — nothing changed.
        if override_hash == previous_hash {
            let mut result = HostUpdateResult::no_change(canonical);
            result.external_source_requests = entry.external_requests.clone();
            return Ok(result);
        }

        let mut changed_nodes: Vec<VirtualNodeKind> = by_index
            .keys()
            .map(|idx| VirtualNodeKind::Style { index: *idx })
            .collect();

        // Re-analyze compiled CSS and remap spans for each override
        let source = entry.source.as_ref();
        // Collect updates to apply to the Arc'd style_analyses
        let mut style_updates: Vec<(usize, verter_analysis::StyleBlockAnalysis)> = Vec::new();
        for (&idx, ov) in &by_index {
            if idx < entry.style_analyses.len() {
                let existing = &entry.style_analyses[idx];
                let content_offset = existing.content_offset;

                // Run CSS analysis on the compiled CSS
                let mut new_analysis = verter_analysis::build_css_style_analysis(
                    &ov.code,
                    verter_analysis::VueStyleInput::default(),
                    existing.scoped,
                    existing.is_module,
                    existing.module_name.as_deref(),
                    content_offset,
                );

                // If we have a source map, remap the CSS spans from compiled offsets
                // back to original preprocessor source offsets
                if let (Some(sm_json), Some(ref mut css)) = (&ov.source_map, &mut new_analysis.css)
                {
                    // Extract original style content from the SFC source
                    let content_start = content_offset as usize;
                    // Find the end of this style block content (next </style> tag)
                    let original_content = if content_start < source.len() {
                        let rest = &source[content_start..];
                        if let Some(end) = rest.find("</style") {
                            &rest[..end]
                        } else {
                            rest
                        }
                    } else {
                        ""
                    };

                    crate::source_map_remap::remap_css_analysis_spans(
                        css,
                        &ov.code,
                        sm_json,
                        original_content,
                        content_offset,
                    );
                }

                // Preserve existing Vue features (v-binds, special pseudos)
                // since they were extracted from the original parse
                new_analysis.v_binds = existing.v_binds.clone();
                new_analysis.special_pseudos = existing.special_pseudos.clone();

                style_updates.push((idx, new_analysis));
            }
        }
        if !style_updates.is_empty() {
            let styles = Arc::make_mut(&mut entry.style_analyses);
            for (idx, analysis) in style_updates {
                styles[idx] = analysis;
            }
        }

        entry.style_overrides.insert(
            profile_hash,
            StyleOverrideLayer {
                hash: override_hash,
                by_index,
            },
        );

        entry.compile_slots.remove(&profile_hash);
        changed_nodes = sorted_nodes(changed_nodes);

        let mut changed_virtual_ids = Vec::new();
        let mut changed_lsp_ids = Vec::new();
        for node in &changed_nodes {
            let (b, l) = render_ids(&canonical, node, &entry.meta);
            changed_virtual_ids.push(b);
            changed_lsp_ids.push(l);
        }

        Ok(HostUpdateResult {
            canonical_id: canonical,
            changed: true,
            slice_changes: SliceChanges::default(),
            changed_virtual_nodes: changed_nodes,
            removed_virtual_nodes: Vec::new(),
            changed_virtual_ids,
            removed_virtual_ids: Vec::new(),
            changed_lsp_ids,
            removed_lsp_ids: Vec::new(),
            diagnostics: DiagnosticsSnapshot::default(),
            external_source_requests: entry.external_requests.clone(),
            import_specifiers: Vec::new(),
            module_references: Vec::new(),
            preprocessor_requests: Vec::new(),
            export_signatures: Vec::new(),
            parse_duration_ms: 0.0,
        })
    }

    /// Apply preprocessed block overrides for template, script, style, and custom blocks.
    ///
    /// Unified API that replaces the single-purpose `apply_style_overrides`.
    /// Template/script overrides build a synthetic SFC source with the `lang`
    /// attribute stripped and block content replaced, then invalidate the compile
    /// slot so the next `get_virtual_file` recompiles from the synthetic source.
    /// Style overrides delegate to the existing style override logic.
    pub fn apply_block_overrides(
        &self,
        req: BlockOverrideRequest,
    ) -> Result<HostUpdateResult, HostError> {
        let canonical = self.resolve_alias_or_canonical(&req.canonical_id);
        let profile_hash = compile_profile_hash(&req.compile_profile);

        // Separate overrides into template/script vs style buckets
        let mut template_override: Option<ContentOverride> = None;
        let mut script_override: Option<ContentOverride> = None;
        let mut style_overrides_vec: Vec<StyleOverrideEntry> = Vec::new();

        for ov in req.overrides {
            match ov.block_type {
                PreprocessorBlockType::Template => {
                    template_override = Some(ContentOverride {
                        code: ov.code,
                        source_map: ov.source_map,
                    });
                }
                PreprocessorBlockType::Script => {
                    script_override = Some(ContentOverride {
                        code: ov.code,
                        source_map: ov.source_map,
                    });
                }
                PreprocessorBlockType::Style | PreprocessorBlockType::Custom => {
                    style_overrides_vec.push(StyleOverrideEntry {
                        index: ov.index,
                        code: ov.code,
                        source_map: ov.source_map,
                    });
                }
            }
        }

        // Handle style overrides via the existing mechanism
        if !style_overrides_vec.is_empty() {
            let style_req = StyleOverrideRequest {
                canonical_id: req.canonical_id.clone(),
                compile_profile: req.compile_profile.clone(),
                overrides: style_overrides_vec,
            };
            // Apply style overrides (this also invalidates the compile slot)
            let _ = self.apply_style_overrides(style_req)?;
        }

        // Handle template/script content overrides
        let has_content_overrides = template_override.is_some() || script_override.is_some();
        if !has_content_overrides {
            // Only style overrides were provided; style overrides already handled above
            let files = crate::shared::read_lock(&self.files);
            let entry = files
                .get(&canonical)
                .ok_or_else(|| HostError::MissingSource {
                    canonical_id: canonical.clone(),
                })?;
            let mut result = HostUpdateResult::no_change(canonical);
            result.external_source_requests = entry.external_requests.clone();
            result.changed = true;
            return Ok(result);
        }

        let override_hash =
            content_override_hash(template_override.as_ref(), script_override.as_ref());

        let mut files = write_lock(&self.files);
        let entry = files
            .get_mut(&canonical)
            .ok_or_else(|| HostError::MissingSource {
                canonical_id: canonical.clone(),
            })?;

        // Check if unchanged
        let previous_hash = entry
            .content_overrides
            .get(&profile_hash)
            .map(|o| o.hash)
            .unwrap_or(0);

        if override_hash == previous_hash {
            let mut result = HostUpdateResult::no_change(canonical);
            result.external_source_requests = entry.external_requests.clone();
            return Ok(result);
        }

        // Build synthetic SFC source with preprocessed content replacing original blocks.
        // This strips the `lang` attribute so the compiler sees native HTML/JS.
        let synthetic_source = build_synthetic_source(
            &entry.source,
            &entry.meta,
            template_override.as_ref(),
            script_override.as_ref(),
        );

        // Re-parse the synthetic source to get updated template AST and metadata.
        // The reparse sees native lang (html/ts) since we stripped the lang attr.
        let (new_snapshot, new_parsed) =
            parse_vue_snapshot(&canonical, &synthetic_source, self.config.effective_scope());

        // Update entry fields that come from parsing
        entry.meta = new_snapshot.meta;
        entry.slices = new_snapshot.slices;
        entry.descriptor = new_snapshot.descriptor;
        entry.semantic_hash = new_snapshot.semantic_hash;
        entry.whole_hash = new_snapshot.whole_hash;
        entry.source = Arc::from(synthetic_source);
        entry.parse_diagnostics = new_snapshot.parse_diagnostics;
        entry.script_analysis = new_snapshot.script_analysis;
        entry.arc_script_cache = ScriptAnalysisArcs::from_analysis(&entry.script_analysis);
        entry.style_analyses = Arc::new(new_snapshot.style_analyses);
        entry.cached_parse = Some(Arc::new(new_parsed));

        // Store content override layer
        entry.content_overrides.insert(
            profile_hash,
            ContentOverrideLayer {
                hash: override_hash,
                template: template_override,
                script: script_override,
            },
        );

        // Invalidate compile slot for this profile
        entry.compile_slots.remove(&profile_hash);

        // Determine changed nodes
        let mut changed_nodes = Vec::new();
        if entry.meta.has_template {
            changed_nodes.push(VirtualNodeKind::Main);
            changed_nodes.push(VirtualNodeKind::Template);
        }
        if entry.meta.has_script {
            changed_nodes.push(VirtualNodeKind::Script);
        }
        changed_nodes = sorted_nodes(changed_nodes);

        let mut changed_virtual_ids = Vec::new();
        let mut changed_lsp_ids = Vec::new();
        for node in &changed_nodes {
            let (b, l) = render_ids(&canonical, node, &entry.meta);
            changed_virtual_ids.push(b);
            changed_lsp_ids.push(l);
        }

        Ok(HostUpdateResult {
            canonical_id: canonical,
            changed: true,
            slice_changes: SliceChanges::default(),
            changed_virtual_nodes: changed_nodes,
            removed_virtual_nodes: Vec::new(),
            changed_virtual_ids,
            removed_virtual_ids: Vec::new(),
            changed_lsp_ids,
            removed_lsp_ids: Vec::new(),
            diagnostics: DiagnosticsSnapshot::default(),
            external_source_requests: entry.external_requests.clone(),
            import_specifiers: Vec::new(),
            module_references: Vec::new(),
            preprocessor_requests: Vec::new(),
            export_signatures: Vec::new(),
            parse_duration_ms: 0.0,
        })
    }
}

/// Build a synthetic SFC source with preprocessed content replacing original
/// block content and `lang` attributes stripped.
///
/// The synthetic source preserves the same byte structure (tags, offsets) where
/// possible, but replaces block content and removes `lang="xxx"` from template
/// and script tags so the compiler treats them as native HTML/JS.
fn build_synthetic_source(
    original: &str,
    meta: &FileMeta,
    template_override: Option<&ContentOverride>,
    script_override: Option<&ContentOverride>,
) -> String {
    // Simple approach: scan and replace content using string markers.
    // We look for the block tags, strip lang attributes, and replace content.
    let mut result = original.to_string();

    // Replace template content (if override provided)
    if let Some(tpl) = template_override {
        result = replace_block_content(&result, "template", &tpl.code, true);
    }

    // Replace script content (if override provided)
    if let Some(scr) = script_override {
        // Determine which script tag to target
        let tag = if meta.script_lang.is_some() {
            "script"
        } else {
            // No non-native script lang; should not happen, but handle gracefully
            "script"
        };
        result = replace_block_content(&result, tag, &scr.code, true);
    }

    result
}

/// Replace the content of an SFC block tag and optionally strip its `lang` attribute.
///
/// Finds `<{tag}...>...content...</{tag}>` and replaces the content between
/// the opening and closing tags. If `strip_lang` is true, removes `lang="xxx"`
/// from the opening tag.
fn replace_block_content(source: &str, tag: &str, new_content: &str, strip_lang: bool) -> String {
    let bytes = source.as_bytes();

    // Find the opening tag
    let open_pattern = format!("<{}", tag);
    let Some(tag_start) = find_tag_start(bytes, &open_pattern) else {
        return source.to_string();
    };

    // Find the end of the opening tag (the `>`)
    let Some(tag_end) = find_char_after(bytes, tag_start, b'>') else {
        return source.to_string();
    };
    let content_start = tag_end + 1;

    // Find the closing tag
    let close_pattern = format!("</{}", tag);
    let Some(close_start) = find_pattern_after(bytes, content_start, close_pattern.as_bytes())
    else {
        return source.to_string();
    };

    // Build the result
    let mut result = String::with_capacity(source.len() + new_content.len());

    // Opening tag (with optional lang stripping)
    let opening_tag = &source[tag_start..content_start];
    if strip_lang {
        result.push_str(&source[..tag_start]);
        result.push_str(&strip_lang_attr(opening_tag));
    } else {
        result.push_str(&source[..content_start]);
    }

    // New content
    result.push_str(new_content);

    // From closing tag to end
    result.push_str(&source[close_start..]);

    result
}

/// Strip `lang="..."` or `lang='...'` from an opening tag string.
fn strip_lang_attr(tag: &str) -> String {
    // Match lang="..." or lang='...' with optional whitespace around =
    let bytes = tag.as_bytes();
    let mut result = String::with_capacity(tag.len());
    let mut i = 0;
    while i < bytes.len() {
        // Check if we're at "lang"
        if i + 4 <= bytes.len()
            && bytes[i..i + 4].eq_ignore_ascii_case(b"lang")
            && (i == 0 || bytes[i - 1].is_ascii_whitespace())
        {
            // Skip past lang="..."
            let mut j = i + 4;
            // Skip whitespace around =
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'=' {
                j += 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && (bytes[j] == b'"' || bytes[j] == b'\'') {
                    let quote = bytes[j];
                    j += 1;
                    while j < bytes.len() && bytes[j] != quote {
                        j += 1;
                    }
                    if j < bytes.len() {
                        j += 1; // skip closing quote
                    }
                }
                // Also consume any trailing whitespace after the value
                // but keep at least one space if we're between attributes
                i = j;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn find_tag_start(bytes: &[u8], pattern: &str) -> Option<usize> {
    let pat = pattern.as_bytes();
    bytes
        .windows(pat.len())
        .position(|w| w.eq_ignore_ascii_case(pat))
}

fn find_char_after(bytes: &[u8], start: usize, ch: u8) -> Option<usize> {
    bytes[start..]
        .iter()
        .position(|&b| b == ch)
        .map(|p| start + p)
}

fn find_pattern_after(bytes: &[u8], start: usize, pattern: &[u8]) -> Option<usize> {
    if start >= bytes.len() {
        return None;
    }
    bytes[start..]
        .windows(pattern.len())
        .position(|w| w.eq_ignore_ascii_case(pattern))
        .map(|p| start + p)
}

//! `impl VerterHost` — upsert and style-override methods.
//!
//! Contains [`VerterHost::upsert`] and [`VerterHost::apply_style_overrides`],
//! which handle file ingestion, change detection, cache invalidation, and
//! style override application.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use crate::cache::{invalidate_nodes, sorted_nodes};
use crate::hash::{compile_profile_hash, style_override_hash};
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

        let parse_start = std::time::Instant::now();
        let mut snapshot = match req.file_kind {
            FileKind::VueSfc => {
                parse_vue_snapshot(&canonical_id, &req.source, self.config.analysis_level)
            }
            FileKind::NonSfc => parse_non_sfc_snapshot(&canonical_id, &req.source),
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

            // Extract data needed for build_upsert_result before moving snapshot fields.
            let result_data = UpsertResultData {
                new_meta: snapshot.meta.clone(),
                parse_diagnostics: snapshot.parse_diagnostics.clone(),
                imports: snapshot.script_analysis.imports.clone(),
                external_requests: snapshot.external_requests.clone(),
            };

            // Clone export_signatures for smart_invalidate_dependents (needs both old & new).
            let new_export_signatures = std::mem::take(&mut snapshot.export_signatures);

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
                    external_requests: Vec::new(),
                    src_blocks: Vec::new(),
                    parse_diagnostics: DiagnosticsSnapshot::default(),
                    script_analysis: verter_analysis::ScriptAnalysisSnapshot::default(),
                    export_signatures: Vec::new(),
                    style_analyses: Vec::new(),
                    resolved_type_hashes: HashMap::new(),
                    style_overrides: HashMap::new(),
                    compile_slots: HashMap::new(),
                    latest_diagnostics: HashMap::new(),
                    generation: 0,
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
            entry.export_signatures = new_export_signatures.clone();
            entry.style_analyses = snapshot.style_analyses;
            entry.generation = entry.generation.saturating_add(1);
            entry.aliases = alias_set.clone();
            entry.dependencies = new_deps.clone();

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
            parse_duration_ms: 0.0,
        })
    }
}

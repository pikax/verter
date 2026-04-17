//! Upsert change detection and result building.
//!
//! Contains the logic for comparing old and new file states during `upsert()`,
//! computing granular slice-level diffs, and assembling the final
//! [`HostUpdateResult`](crate::HostUpdateResult).

use std::collections::BTreeSet;

use crate::cache::{compute_changed_removed_nodes, sorted_nodes};
use crate::hash::diff_indices;
use crate::id::render_ids;
use crate::types::*;

/// Result of computing what changed between old and new file state.
pub(crate) struct UpsertChangeResult {
    pub(crate) slice_changes: SliceChanges,
    pub(crate) changed: bool,
    pub(crate) semantic_changed: bool,
}

/// Compare old file entry state against a new parse snapshot to determine what changed.
/// Consolidates all change detection logic into a single function.
/// Used by the legacy (non-scheduler) upsert path, WASM, and unit tests.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn compute_upsert_changes(
    old_entry: Option<&FileEntry>,
    new: &ParseSnapshot,
) -> UpsertChangeResult {
    let Some(old) = old_entry else {
        return UpsertChangeResult {
            slice_changes: SliceChanges::default(),
            changed: true,
            semantic_changed: true,
        };
    };

    compute_upsert_changes_core(
        old.whole_hash,
        old.semantic_hash,
        &old.slices,
        &old.descriptor,
        new,
    )
}

/// Compare two ParseSnapshots to determine what changed (scheduler-backed path).
///
/// Takes the old parse snapshot directly rather than a FileEntry. Used by
/// the scheduler-backed upsert where old state comes from the scheduler's
/// committed HostSourceData, not from the files map.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn compute_upsert_changes_from_parse(
    old: Option<&ParseSnapshot>,
    new: &ParseSnapshot,
) -> UpsertChangeResult {
    let Some(old) = old else {
        return UpsertChangeResult {
            slice_changes: SliceChanges::default(),
            changed: true,
            semantic_changed: true,
        };
    };

    compute_upsert_changes_core(
        old.whole_hash,
        old.semantic_hash,
        &old.slices,
        &old.descriptor,
        new,
    )
}

/// Core change detection comparing old hashes/slices against new parse snapshot.
fn compute_upsert_changes_core(
    old_whole_hash: Hash16,
    old_semantic_hash: Hash16,
    old_slices: &SliceHashes,
    old_descriptor: &DescriptorMin,
    new: &ParseSnapshot,
) -> UpsertChangeResult {
    // Quick check: if the whole source is byte-identical, nothing changed.
    if old_whole_hash == new.whole_hash {
        return UpsertChangeResult {
            slice_changes: SliceChanges::default(),
            changed: false,
            semantic_changed: false,
        };
    }

    // Whole hash differs. Check semantic hash + descriptor for meaningful change.
    let semantic_hash_same = old_semantic_hash == new.semantic_hash;
    let descriptor_same = *old_descriptor == new.descriptor;

    // If only whitespace changed (semantic hash and descriptor identical), skip invalidation.
    if semantic_hash_same && descriptor_same {
        return UpsertChangeResult {
            slice_changes: SliceChanges::default(),
            changed: false,
            semantic_changed: false,
        };
    }

    // Real change detected. Compute granular slice-level diffs.
    let slice_changes = SliceChanges {
        script_changed: old_slices.script != new.slices.script,
        template_changed: old_slices.template != new.slices.template,
        style_indices_changed: diff_indices(&old_slices.styles, &new.slices.styles),
        custom_indices_changed: diff_indices(&old_slices.custom, &new.slices.custom),
        structure_changed: old_descriptor.script_count != new.descriptor.script_count
            || old_descriptor.template_count != new.descriptor.template_count
            || old_descriptor.style_count != new.descriptor.style_count
            || old_descriptor.custom_count != new.descriptor.custom_count,
        descriptor_changed: !descriptor_same,
    };

    UpsertChangeResult {
        slice_changes,
        changed: true,
        semantic_changed: true,
    }
}

/// Data extracted from ParseSnapshot for building the upsert result.
/// Avoids borrowing the snapshot after its fields are moved into FileEntry.
pub(crate) struct UpsertResultData {
    pub(crate) new_meta: FileMeta,
    pub(crate) parse_diagnostics: DiagnosticsSnapshot,
    pub(crate) imports: Vec<verter_semantic::analysis::AnalyzedImport>,
    pub(crate) module_references: Vec<verter_semantic::analysis::AnalyzedModuleReference>,
    pub(crate) external_requests: Vec<ExternalSourceRequest>,
    pub(crate) preprocessor_requests: Vec<PreprocessorRequest>,
    pub(crate) export_signatures: Vec<verter_semantic::analysis::ExportSignature>,
}

/// Render bundler and LSP IDs for a list of virtual nodes.
pub(crate) fn render_node_ids(
    canonical_id: &str,
    nodes: &[VirtualNodeKind],
    meta: &FileMeta,
) -> (Vec<String>, Vec<String>) {
    let mut bundler_ids = Vec::with_capacity(nodes.len());
    let mut lsp_ids = Vec::with_capacity(nodes.len());
    for node in nodes {
        let (b, l) = render_ids(canonical_id, node, meta);
        bundler_ids.push(b);
        lsp_ids.push(l);
    }
    (bundler_ids, lsp_ids)
}

/// Compute virtual node lists and render the final HostUpdateResult.
pub(crate) fn build_upsert_result(
    canonical_id: String,
    data: UpsertResultData,
    changes: &UpsertChangeResult,
    prev_nodes: &[VirtualNodeKind],
    old_meta: &FileMeta,
    parse_duration_ms: f64,
) -> Result<HostUpdateResult, HostError> {
    let new_nodes = data.new_meta.virtual_nodes();

    let (changed_nodes, removed_nodes) = compute_changed_removed_nodes(
        &changes.slice_changes,
        changes.changed,
        prev_nodes,
        &new_nodes,
    );

    let changed_nodes_sorted = sorted_nodes(changed_nodes);
    let removed_nodes_sorted = sorted_nodes(removed_nodes);

    let (changed_virtual_ids, changed_lsp_ids) =
        render_node_ids(&canonical_id, &changed_nodes_sorted, &data.new_meta);
    let (removed_virtual_ids, removed_lsp_ids) =
        render_node_ids(&canonical_id, &removed_nodes_sorted, old_meta);

    let diagnostics = if data.parse_diagnostics.diagnostics.is_empty() {
        DiagnosticsSnapshot::default()
    } else {
        data.parse_diagnostics
    };

    let import_specifiers = data
        .imports
        .into_iter()
        .map(|imp| ScriptImportInfo {
            is_type_only: imp.is_type_only,
            bindings: imp.bindings.into_iter().map(|b| b.name).collect(),
            source: imp.source,
        })
        .collect();
    let module_references = data
        .module_references
        .into_iter()
        .map(|reference| ScriptModuleReference {
            syntax: reference.syntax,
            semantics: reference.semantics,
            is_type_only: reference.is_type_only,
            raw_text: reference.raw_text,
            literal_specifier: reference.literal_specifier,
            finite_specifiers: reference.finite_specifiers,
            static_prefix: reference.static_prefix,
            analyzability: reference.analyzability,
            span: reference.span,
            expr_span: reference.expr_span,
        })
        .collect();

    Ok(HostUpdateResult {
        canonical_id,
        changed: !changed_nodes_sorted.is_empty() || !removed_nodes_sorted.is_empty(),
        slice_changes: changes.slice_changes.clone(),
        changed_virtual_nodes: changed_nodes_sorted,
        removed_virtual_nodes: removed_nodes_sorted,
        changed_virtual_ids,
        removed_virtual_ids,
        changed_lsp_ids,
        removed_lsp_ids,
        diagnostics,
        external_source_requests: data.external_requests,
        import_specifiers,
        module_references,
        preprocessor_requests: data.preprocessor_requests,
        export_signatures: data.export_signatures,
        parse_duration_ms,
    })
}

/// Compute which export names changed between old and new export signatures.
/// Returns the set of export names whose declaration hashes differ.
pub(crate) fn compute_changed_exports(
    old: &[verter_semantic::analysis::ExportSignature],
    new: &[verter_semantic::analysis::ExportSignature],
) -> BTreeSet<String> {
    if old.is_empty() && new.is_empty() {
        return BTreeSet::new();
    }
    if old.is_empty() {
        return new.iter().map(|s| s.name.clone()).collect();
    }
    if new.is_empty() {
        return old.iter().map(|s| s.name.clone()).collect();
    }

    use rustc_hash::FxHashMap;
    let old_map: FxHashMap<&str, &[u8; 16]> = old
        .iter()
        .map(|s| (s.name.as_str(), &s.declaration_hash))
        .collect();
    let new_map: FxHashMap<&str, &[u8; 16]> = new
        .iter()
        .map(|s| (s.name.as_str(), &s.declaration_hash))
        .collect();

    let mut changed = BTreeSet::new();

    // Check for changed or removed exports
    for (name, old_hash) in &old_map {
        match new_map.get(name) {
            Some(new_hash) if new_hash != old_hash => {
                changed.insert(name.to_string());
            }
            None => {
                changed.insert(name.to_string());
            }
            _ => {}
        }
    }

    // Check for added exports
    for name in new_map.keys() {
        if !old_map.contains_key(name) {
            changed.insert(name.to_string());
        }
    }

    changed
}

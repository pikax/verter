//! Virtual node diffing, compile slot invalidation, and LRU profile eviction.

use std::collections::BTreeSet;

#[cfg(test)]
use crate::types::CompileSlot;
use crate::types::{SliceChanges, VirtualNodeKind};

pub(crate) fn node_sort_key(node: &VirtualNodeKind) -> (u8, usize) {
    match node {
        VirtualNodeKind::Main => (0, 0),
        VirtualNodeKind::Script => (1, 0),
        VirtualNodeKind::Template => (2, 0),
        VirtualNodeKind::Style { index } => (3, *index),
        VirtualNodeKind::Custom { index } => (4, *index),
    }
}

pub(crate) fn sorted_nodes(mut nodes: Vec<VirtualNodeKind>) -> Vec<VirtualNodeKind> {
    nodes.sort_by_key(node_sort_key);
    nodes.dedup();
    nodes
}

pub(crate) fn compute_changed_removed_nodes(
    slice_changes: &SliceChanges,
    changed: bool,
    prev_nodes: &[VirtualNodeKind],
    new_nodes: &[VirtualNodeKind],
) -> (Vec<VirtualNodeKind>, Vec<VirtualNodeKind>) {
    if !changed {
        return (Vec::new(), Vec::new());
    }
    if prev_nodes.is_empty() {
        return (new_nodes.to_vec(), Vec::new());
    }

    let prev_set: BTreeSet<_> = prev_nodes.iter().cloned().collect();
    let new_set: BTreeSet<_> = new_nodes.iter().cloned().collect();

    let removed: Vec<VirtualNodeKind> = prev_set.difference(&new_set).cloned().collect();

    let mut changed_nodes = Vec::new();
    if slice_changes.structure_changed
        || slice_changes.descriptor_changed
        || slice_changes.script_changed
    {
        changed_nodes.extend(new_nodes.iter().cloned());
    } else if slice_changes.template_changed {
        changed_nodes.push(VirtualNodeKind::Main);
        if new_set.contains(&VirtualNodeKind::Template) {
            changed_nodes.push(VirtualNodeKind::Template);
        }
    } else {
        for idx in &slice_changes.style_indices_changed {
            if new_set.contains(&VirtualNodeKind::Style { index: *idx }) {
                changed_nodes.push(VirtualNodeKind::Style { index: *idx });
            }
        }
        for idx in &slice_changes.custom_indices_changed {
            if new_set.contains(&VirtualNodeKind::Custom { index: *idx }) {
                changed_nodes.push(VirtualNodeKind::Custom { index: *idx });
            }
        }
    }

    // Fallback: if changed=true but no specific slice changes were detected
    // (e.g. non-SFC file content changed), report all new nodes as changed.
    // For SFC files this never triggers because a semantic_hash change always
    // implies at least one slice or descriptor change.
    if changed_nodes.is_empty() {
        changed_nodes.extend(new_nodes.iter().cloned());
    }

    (changed_nodes, removed)
}

/// Removes the given node kinds from both `outputs` and `last_good_outputs`
/// in every compile slot. This is intentional: stale fallback data for
/// invalidated nodes is worse than no fallback, because serving outdated
/// template/main code after a template-only edit could silently produce
/// wrong output. Callers relying on `DevServeLastKnownGood` will get an
/// error instead of stale content for these nodes.
#[cfg(test)]
pub(crate) fn invalidate_nodes(
    slots: &mut rustc_hash::FxHashMap<u64, CompileSlot>,
    nodes: &[VirtualNodeKind],
) {
    for slot in slots.values_mut() {
        for node in nodes {
            slot.outputs.remove(node);
            if let Some(last_good) = slot.last_good_outputs.as_mut() {
                last_good.remove(node);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_nodes(
        has_script: bool,
        has_template: bool,
        n_styles: usize,
        n_custom: usize,
    ) -> Vec<VirtualNodeKind> {
        let mut nodes = vec![VirtualNodeKind::Main];
        if has_script {
            nodes.push(VirtualNodeKind::Script);
        }
        if has_template {
            nodes.push(VirtualNodeKind::Template);
        }
        for i in 0..n_styles {
            nodes.push(VirtualNodeKind::Style { index: i });
        }
        for i in 0..n_custom {
            nodes.push(VirtualNodeKind::Custom { index: i });
        }
        nodes
    }

    #[test]
    fn compute_not_changed_returns_empty() {
        let sc = SliceChanges::default();
        let prev = make_nodes(true, true, 1, 0);
        let new = make_nodes(true, true, 1, 0);
        let (changed, removed) = compute_changed_removed_nodes(&sc, false, &prev, &new);
        assert!(changed.is_empty());
        assert!(removed.is_empty());
    }

    #[test]
    fn compute_first_insert_all_new() {
        let sc = SliceChanges::default();
        let new = make_nodes(true, true, 1, 0);
        let (changed, removed) = compute_changed_removed_nodes(&sc, true, &[], &new);
        assert_eq!(changed.len(), new.len());
        assert!(removed.is_empty());
    }

    #[test]
    fn compute_structure_change_returns_all_new() {
        let sc = SliceChanges {
            structure_changed: true,
            ..SliceChanges::default()
        };
        let prev = make_nodes(true, true, 1, 0);
        let new = make_nodes(true, true, 2, 0);
        let (changed, _removed) = compute_changed_removed_nodes(&sc, true, &prev, &new);
        assert_eq!(changed.len(), new.len());
    }

    #[test]
    fn compute_template_only_change() {
        let sc = SliceChanges {
            template_changed: true,
            ..SliceChanges::default()
        };
        let prev = make_nodes(true, true, 1, 0);
        let new = make_nodes(true, true, 1, 0);
        let (changed, removed) = compute_changed_removed_nodes(&sc, true, &prev, &new);
        assert!(changed.contains(&VirtualNodeKind::Main));
        assert!(changed.contains(&VirtualNodeKind::Template));
        assert_eq!(changed.len(), 2);
        assert!(removed.is_empty());
    }

    #[test]
    fn compute_style_only_change() {
        let sc = SliceChanges {
            style_indices_changed: vec![0],
            ..SliceChanges::default()
        };
        let prev = make_nodes(true, true, 2, 0);
        let new = make_nodes(true, true, 2, 0);
        let (changed, removed) = compute_changed_removed_nodes(&sc, true, &prev, &new);
        assert_eq!(changed, vec![VirtualNodeKind::Style { index: 0 }]);
        assert!(removed.is_empty());
    }

    #[test]
    fn compute_node_removed() {
        let sc = SliceChanges {
            structure_changed: true,
            ..SliceChanges::default()
        };
        let prev = make_nodes(true, true, 2, 0);
        let new = make_nodes(true, true, 1, 0);
        let (_changed, removed) = compute_changed_removed_nodes(&sc, true, &prev, &new);
        assert_eq!(removed, vec![VirtualNodeKind::Style { index: 1 }]);
    }

    #[test]
    fn compute_script_change_includes_all() {
        let sc = SliceChanges {
            script_changed: true,
            ..SliceChanges::default()
        };
        let prev = make_nodes(true, true, 1, 0);
        let new = make_nodes(true, true, 1, 0);
        let (changed, removed) = compute_changed_removed_nodes(&sc, true, &prev, &new);
        assert_eq!(changed.len(), new.len());
        assert!(removed.is_empty());
    }

    // ═══════════════════════════════════════════════════════════
    // Additional cache tests
    // ═══════════════════════════════════════════════════════════

    use rustc_hash::FxHashMap;
    use std::sync::Arc;

    use crate::types::{CachedVirtualFile, CompileSlot, DiagnosticsSnapshot, VirtualMeta};

    /// @ai-generated - Fallback: changed=true but no slice changes → all new nodes reported
    /// This covers non-SFC files where semantic_hash changes but no specific slices exist.
    #[test]
    fn compute_changed_fallback_reports_all_when_no_slice_changes() {
        let sc = SliceChanges::default(); // no specific changes
        let prev = vec![VirtualNodeKind::Main];
        let new = vec![VirtualNodeKind::Main];
        let (changed, removed) = compute_changed_removed_nodes(&sc, true, &prev, &new);
        assert_eq!(changed, vec![VirtualNodeKind::Main]);
        assert!(removed.is_empty());
    }

    /// @ai-generated - invalidate_nodes on empty slots doesn't panic
    #[test]
    fn invalidate_nodes_empty_slots_no_panic() {
        let mut slots = FxHashMap::default();
        invalidate_nodes(
            &mut slots,
            &[VirtualNodeKind::Main, VirtualNodeKind::Template],
        );
        assert!(slots.is_empty(), "should not panic on empty slots");
    }

    /// @ai-generated - invalidate_nodes only removes targeted nodes, leaves others
    #[test]
    fn invalidate_nodes_partial() {
        let mut slots = FxHashMap::default();
        let mut outputs = FxHashMap::default();
        outputs.insert(
            VirtualNodeKind::Main,
            CachedVirtualFile {
                code: Arc::from("main"),
                source_map: None,
                lang: None,
                meta: VirtualMeta::default(),
            },
        );
        outputs.insert(
            VirtualNodeKind::Style { index: 0 },
            CachedVirtualFile {
                code: Arc::from("style"),
                source_map: None,
                lang: None,
                meta: VirtualMeta::default(),
            },
        );
        slots.insert(
            1u64,
            CompileSlot {
                semantic_hash: [0; 16],
                style_override_hash: 0,
                content_override_hash: 0,
                css_hash_override: None,
                outputs,
                diagnostics: DiagnosticsSnapshot::default(),
                last_good_outputs: None,
                last_access_tick: 1,
                tsx: None,
                template_analysis: None,
                fact_dep_signature: crate::fact_signature_helpers::ReadSetSignature::empty(),
                runtime_surface_refused: false,
            },
        );

        invalidate_nodes(&mut slots, &[VirtualNodeKind::Main]);

        let slot = slots.get(&1).unwrap();
        assert!(!slot.outputs.contains_key(&VirtualNodeKind::Main));
        assert!(slot
            .outputs
            .contains_key(&VirtualNodeKind::Style { index: 0 }));
    }
}

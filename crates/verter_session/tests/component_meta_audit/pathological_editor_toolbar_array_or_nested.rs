//! Pathological regression — editor_toolbar.vue whose `items` prop
//! is typed `(ToolbarItem | ToolbarItem[])[]`. Exercises union
//! resolution on a cross-file alias `ToolbarItemOrGroup`.

use crate::harness::{
    build_hermetic_host, footprint_of, resolve_under_audit, EDITOR_TOOLBAR_TYPES_TS,
    EDITOR_TOOLBAR_VUE,
};

#[test]
fn pathological_editor_toolbar_array_or_nested_footprint_is_stable() {
    let host = build_hermetic_host(&[
        ("/editor_toolbar.vue", EDITOR_TOOLBAR_VUE),
        ("/editor_toolbar_types.ts", EDITOR_TOOLBAR_TYPES_TS),
    ]);
    let (analysis, _resolution, record) = resolve_under_audit(host, "/editor_toolbar.vue");

    let prop_names: Vec<&str> = analysis.props.iter().map(|p| p.name.as_ref()).collect();
    assert!(
        prop_names.contains(&"items"),
        "editor_toolbar.vue must expose `items` prop, got {prop_names:?}"
    );

    let fp = footprint_of(&record);
    // ToolbarItemOrGroup is referenced via union arm in the
    // props type; at minimum the types file should be indexed.
    let toolbar_types_touched = fp
        .indexed_ready_builds
        .iter()
        .any(|b| b.canonical_id.contains("editor_toolbar_types"))
        || fp
            .vfs_reads
            .iter()
            .any(|r| r.canonical_id.contains("editor_toolbar_types"));
    assert!(
        toolbar_types_touched,
        "editor_toolbar_types.ts must appear in footprint — props resolution requires it"
    );

    // Pathological-shape guard: masked snapshot is stable.
    let masked = fp.mask_incidental_spans();
    assert!(masked.vfs_reads.is_empty());
}

//! Pathological regression — table.vue uses `script setup generic`
//! with `T extends Record<string, any>`. Exercises generic-SFC
//! component-meta plus cross-file generic type resolution.

use crate::harness::{
    build_hermetic_host, footprint_of, resolve_under_audit, TABLE_TYPES_TS, TABLE_VUE,
};

#[test]
fn pathological_table_loading_animation_footprint_is_stable() {
    let host = build_hermetic_host(&[
        ("/table.vue", TABLE_VUE),
        ("/table_types.ts", TABLE_TYPES_TS),
    ]);
    let (analysis, _resolution, record) = resolve_under_audit(host, "/table.vue");

    // Generic SFC must still expose `columns`, `rows`, `loading`.
    let prop_names: Vec<String> = analysis.props.iter().map(|p| p.name.to_string()).collect();
    for required in ["columns", "rows", "loading"] {
        assert!(
            prop_names.iter().any(|p| p == required),
            "table.vue must expose `{required}` prop, got {prop_names:?}"
        );
    }

    let fp = footprint_of(&record);
    // table_types.ts is the cross-file type dep; must be indexed.
    let table_types_touched = fp
        .indexed_ready_builds
        .iter()
        .any(|b| b.canonical_id.contains("table_types"))
        || fp
            .vfs_reads
            .iter()
            .any(|r| r.canonical_id.contains("table_types"));
    assert!(
        table_types_touched,
        "table_types.ts must appear in footprint (indexed+vfs={:?})",
        (
            fp.indexed_ready_builds
                .iter()
                .map(|b| b.canonical_id.as_ref())
                .collect::<Vec<_>>(),
            fp.vfs_reads
                .iter()
                .map(|r| r.canonical_id.as_ref())
                .collect::<Vec<_>>(),
        ),
    );

    let masked = fp.mask_incidental_spans();
    assert!(masked.vfs_reads.is_empty());
}

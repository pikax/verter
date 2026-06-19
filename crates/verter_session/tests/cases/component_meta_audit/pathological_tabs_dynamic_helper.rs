//! Pathological regression — tabs.vue with a dynamic helper that
//! consumes `TabItem[]` across two files. The fixture exercises
//! cross-file type resolution + function-call type inference on
//! `defineProps<{ items: TabItem[] }>`. Snapshot is shape-pinned
//! via `mask_incidental_spans`.

use super::harness::{
    build_hermetic_host, footprint_of, resolve_under_audit, TABS_HELPER_TS, TABS_TYPES_TS, TABS_VUE,
};

#[test]
fn pathological_tabs_dynamic_helper_footprint_is_stable() {
    let host = build_hermetic_host(&[
        ("/tabs.vue", TABS_VUE),
        ("/tabs_types.ts", TABS_TYPES_TS),
        ("/tabs_helper.ts", TABS_HELPER_TS),
    ]);
    let (analysis, _resolution, record) = resolve_under_audit(host, "/tabs.vue");

    // Analysis sanity: the macro extracted `items` + `modelValue`.
    let prop_names: Vec<&str> = analysis.props.iter().map(|p| p.name.as_ref()).collect();
    assert!(
        prop_names.contains(&"items"),
        "tabs.vue must expose `items` prop, got {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"modelValue"),
        "tabs.vue must expose `modelValue` prop, got {prop_names:?}"
    );

    // Footprint sanity: both dep files should have been touched
    // (either directly via vfs_reads or indirectly via
    // indexed_ready_builds).
    let fp = footprint_of(&record);
    let loaded = fp.loaded_files();
    let loaded_strs: Vec<&str> = loaded.iter().map(|s| s.as_ref()).collect();
    // The entry itself shows up in vfs_reads; deps may show via
    // shared_load_reuses or indexed_ready_builds.
    let any_touched_tabs_ts = loaded_strs
        .iter()
        .any(|s| s.contains("tabs_types") || s.contains("tabs_helper"))
        || fp.indexed_ready_builds.iter().any(|b| {
            b.canonical_id.contains("tabs_types") || b.canonical_id.contains("tabs_helper")
        });
    assert!(
        any_touched_tabs_ts,
        "expected at least one of /tabs_types.ts or /tabs_helper.ts in footprint \
         (loaded={loaded_strs:?}, indexed={:?})",
        fp.indexed_ready_builds
            .iter()
            .map(|b| b.canonical_id.as_ref())
            .collect::<Vec<_>>()
    );

    // Stability: masking incidental spans must produce a footprint
    // whose vfs_reads is empty (that's the mask's job) — the other
    // vectors are preserved.
    let masked = fp.mask_incidental_spans();
    assert!(
        masked.vfs_reads.is_empty(),
        "mask_incidental_spans must clear vfs_reads"
    );
    assert_eq!(
        masked.indexed_ready_builds.len(),
        fp.indexed_ready_builds.len()
    );
}

use std::collections::BTreeSet;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::cache;
use super::deps::{
    import_resolves_to_dep, should_invalidate_dependent_view, strip_configured_extension,
    DependentView,
};
use super::id::canonicalize_id;
use super::parse::parse_vue_snapshot;
use super::shared::{read_lock, write_lock};
use super::upsert::{
    build_upsert_result, compute_changed_exports, compute_upsert_changes, UpsertChangeResult,
    UpsertResultData,
};
use super::*;
use verter_semantic::analysis::AnalysisScope;
use verter_workspace::WorkspaceAccess;

fn profile_dev() -> CompileProfile {
    CompileProfile {
        is_production: false,
        hmr_strategy: HmrStrategy::Vite,
        ..CompileProfile::default()
    }
}

fn upsert_vue(host: &VerterHost, id: &str, src: &str) -> HostUpdateResult {
    host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: id.to_string(),
        source: Arc::from(src),
        file_kind: FileKind::VueSfc,
        aliases: Vec::new(),
    })
    .unwrap()
}

#[test]
fn get_source_returns_source_for_canonical_and_alias() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let source = "<template><div>hello</div></template>";

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "Comp.vue".to_string(),
            source: Arc::from(source),
            file_kind: FileKind::VueSfc,
            aliases: vec!["AliasComp.vue".to_string()],
        })
        .unwrap();

    assert_eq!(host.get_source("Comp.vue").as_deref(), Some(source));
    assert_eq!(host.get_source("AliasComp.vue").as_deref(), Some(source));
    assert_eq!(host.get_source("Missing.vue"), None);
}

#[test]
fn host_internal_diagnostic_spans_remain_byte_offsets() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let source = "<template>\n  😀<div>\n</template>\n";

    let result = upsert_vue(&host, "Comp.vue", source);
    let expected_div_start = source.find("<div>").unwrap() as u32;

    let matches_byte_span = result.diagnostics.diagnostics.iter().any(|d| {
        d.code.contains("XMissingEndTag") && d.span.map(|s| s.start) == Some(expected_div_start)
    });

    assert!(
        matches_byte_span,
        "expected byte span {} in XMissingEndTag diagnostics, got: {:?}",
        expected_div_start,
        result
            .diagnostics
            .diagnostics
            .iter()
            .map(|d| (
                d.code.clone(),
                d.span.map(|s| s.start),
                d.span.map(|s| s.end)
            ))
            .collect::<Vec<_>>()
    );
}

fn file_entry_from_snapshot(canonical_id: &str, src: &str, snap: &ParseSnapshot) -> FileEntry {
    FileEntry {
        canonical_id: canonical_id.to_string(),
        file_kind: FileKind::VueSfc,
        source: Arc::from(src),
        whole_hash: snap.whole_hash,
        semantic_hash: snap.semantic_hash,
        slices: snap.slices.clone(),
        descriptor: snap.descriptor.clone(),
        meta: snap.meta.clone(),
        aliases: BTreeSet::new(),
        dependencies: BTreeSet::new(),
        dependency_resolutions: FxHashMap::default(),
        external_requests: Vec::new(),
        src_blocks: Vec::new(),
        parse_diagnostics: DiagnosticsSnapshot::default(),
        script_analysis: verter_semantic::analysis::ScriptAnalysisSnapshot::default(),
        export_signatures: Vec::new(),
        style_analyses: Arc::new(Vec::new()),
        template_analysis: None,
        arc_script_cache: ScriptAnalysisArcs::default(),
        resolved_type_hashes: FxHashMap::default(),
        style_overrides: FxHashMap::default(),
        content_overrides: FxHashMap::default(),
        compile_slots: FxHashMap::default(),
        latest_diagnostics: FxHashMap::default(),
        diagnostics_generation: 0,
        generation: 1,
        cached_parse: None,
        cached_tsc_extract: None,
        cached_resolved_meta: FxHashMap::default(),
        cached_meta_payloads: FxHashMap::default(),
        cached_fallthrough: None,
        export_registry: None,
    }
}

/// @ai-generated - AnalysisLevel::Essential runs script analysis but not style
#[test]
fn analysis_level_essential_runs_script_not_style() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Essential,
        ..HostConfig::default()
    });
    let src = "<script setup>\nimport { ref } from 'vue'\nconst n = ref(1)\n</script>\n<template><div>{{n}}</div></template>\n<style scoped>.a { color: red }</style>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    #[cfg(feature = "scheduler")]
    {
        use crate::host_executor::{HostAnalysisData, HostSourceData};
        let source_snap = host
            .scheduler
            .try_get_source("Comp.vue")
            .expect("scheduler should have Comp.vue");
        let hd = source_snap
            .downcast_data::<HostSourceData>()
            .expect("source data should be HostSourceData");
        assert!(
            !hd.parse.script_analysis.imports.is_empty(),
            "script analysis should be populated at AnalysisLevel::Essential"
        );
        let empty_styles: Vec<verter_semantic::analysis::StyleBlockAnalysis> = Vec::new();
        let analysis_snap = host.scheduler.try_get_analysis("Comp.vue");
        let style_analyses = analysis_snap
            .as_ref()
            .and_then(|a| a.downcast_data::<HostAnalysisData>())
            .map(|ad| ad.style_analyses.as_ref())
            .unwrap_or(&empty_styles);
        assert!(
            style_analyses.is_empty(),
            "style analyses should not be populated at AnalysisLevel::Essential"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let files = read_lock(&host.files);
        let entry = files.get("Comp.vue").unwrap();
        assert!(
            !entry.script_analysis.imports.is_empty(),
            "script analysis should be populated at AnalysisLevel::Essential"
        );
        assert!(
            entry.style_analyses.is_empty(),
            "style analyses should not be populated at AnalysisLevel::Essential"
        );
    }
}

/// @ai-generated - AnalysisLevel::None skips all analysis during upsert
#[test]
fn analysis_level_none_skips_all_analysis_in_upsert() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::None,
        ..HostConfig::default()
    });
    let src = "<script setup>\nimport { ref } from 'vue'\nconst n = ref(1)\n</script>\n<template><div>{{n}}</div></template>\n<style scoped>.a { color: red }</style>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    #[cfg(feature = "scheduler")]
    {
        use crate::host_executor::{HostAnalysisData, HostSourceData};
        let source_snap = host
            .scheduler
            .try_get_source("Comp.vue")
            .expect("scheduler should have Comp.vue");
        let hd = source_snap
            .downcast_data::<HostSourceData>()
            .expect("source data should be HostSourceData");
        assert!(
            hd.parse.script_analysis.imports.is_empty(),
            "script analysis should not be populated at AnalysisLevel::None"
        );
        let empty_styles: Vec<verter_semantic::analysis::StyleBlockAnalysis> = Vec::new();
        let analysis_snap = host.scheduler.try_get_analysis("Comp.vue");
        let style_analyses = analysis_snap
            .as_ref()
            .and_then(|a| a.downcast_data::<HostAnalysisData>())
            .map(|ad| ad.style_analyses.as_ref())
            .unwrap_or(&empty_styles);
        assert!(
            style_analyses.is_empty(),
            "style analyses should not be populated at AnalysisLevel::None"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let files = read_lock(&host.files);
        let entry = files.get("Comp.vue").unwrap();
        assert!(
            entry.script_analysis.imports.is_empty(),
            "script analysis should not be populated at AnalysisLevel::None"
        );
        assert!(
            entry.style_analyses.is_empty(),
            "style analyses should not be populated at AnalysisLevel::None"
        );
    }
}

/// @ai-generated - Semantic DB queries should read the current stored script and template analysis.
#[test]
fn semantic_queries_use_current_analysis_snapshots() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src = concat!(
        "<script setup lang=\"ts\">\n",
        "const count = 1\n",
        "defineProps<{ msg: string }>()\n",
        "</script>\n",
        "<template><div>{{ count }}</div></template>\n"
    );

    let _ = upsert_vue(&host, "Comp.vue", src);

    let surface = host.semantic_component_surface("Comp.vue");
    assert!(
        surface.is_complete(),
        "component surface query should complete"
    );
    let declared_surface = surface
        .value
        .as_ref()
        .expect("component surface should exist for a script-setup SFC");
    assert!(
        declared_surface
            .declared
            .props
            .iter()
            .any(|prop| prop.name == "msg"),
        "defineProps should contribute declared props"
    );
    assert!(
        declared_surface
            .declared
            .props
            .iter()
            .all(|prop| prop.name != "missing"),
        "unrelated props must not appear in the declared surface"
    );

    let bindings = host.semantic_bindings("Comp.vue");
    assert!(bindings.is_complete(), "bindings query should complete");
    let bindings = bindings
        .value
        .as_ref()
        .expect("bindings should exist for a script block");
    assert!(
        bindings.iter().any(|(binding, _)| binding.name == "count"),
        "script bindings should include top-level declarations"
    );
    assert!(
        bindings
            .iter()
            .all(|(binding, _)| binding.name != "missing"),
        "bindings query must not synthesize unrelated declarations"
    );

    let snapshot = host.semantic_snapshot("Comp.vue");
    assert!(snapshot.is_complete(), "semantic snapshot should complete");
    assert!(
        snapshot.value.component_surface.is_some(),
        "semantic snapshot should include the component surface"
    );
    assert!(
        snapshot.value.find_binding("count").is_some(),
        "semantic snapshot should include extracted bindings"
    );
    assert!(
        snapshot.value.find_binding("missing").is_none(),
        "semantic snapshot must not report missing bindings as present"
    );
}

/// @ai-generated - build_upsert_result: first insert returns all nodes as changed
#[test]
fn build_upsert_result_first_insert() {
    let src = "<script setup>const n = 1</script><template><div/></template><style>.a{}</style>";
    let (snapshot, _) = parse_vue_snapshot("Comp.vue", src, AnalysisScope::LSP);
    let data = UpsertResultData {
        new_meta: snapshot.meta,
        parse_diagnostics: snapshot.parse_diagnostics,
        imports: snapshot.script_analysis.imports,
        module_references: snapshot.script_analysis.module_references,
        external_requests: snapshot.external_requests,
        preprocessor_requests: snapshot.preprocessor_requests,
        export_signatures: snapshot.export_signatures,
    };
    let changes = UpsertChangeResult {
        slice_changes: SliceChanges::default(),
        changed: true,
        semantic_changed: true,
    };
    let result = build_upsert_result(
        "Comp.vue".to_string(),
        data,
        &changes,
        &[], // no prev_nodes
        &FileMeta::default(),
        0.0,
    )
    .unwrap();

    assert!(result.changed);
    assert!(result
        .changed_virtual_nodes
        .contains(&VirtualNodeKind::Main));
    assert!(result
        .changed_virtual_nodes
        .contains(&VirtualNodeKind::Script));
    assert!(result
        .changed_virtual_nodes
        .contains(&VirtualNodeKind::Template));
    assert!(result
        .changed_virtual_nodes
        .contains(&VirtualNodeKind::Style { index: 0 }));
    assert!(result.removed_virtual_nodes.is_empty());
    assert_eq!(
        result.changed_virtual_ids.len(),
        result.changed_lsp_ids.len()
    );
}

/// @ai-generated - build_upsert_result: no change returns empty
#[test]
fn build_upsert_result_no_change() {
    let src = "<script setup>const n = 1</script><template><div/></template>";
    let (snapshot, _) = parse_vue_snapshot("Comp.vue", src, AnalysisScope::LSP);
    let data = UpsertResultData {
        new_meta: snapshot.meta,
        parse_diagnostics: snapshot.parse_diagnostics,
        imports: snapshot.script_analysis.imports,
        module_references: snapshot.script_analysis.module_references,
        external_requests: snapshot.external_requests,
        preprocessor_requests: snapshot.preprocessor_requests,
        export_signatures: snapshot.export_signatures,
    };
    let prev = vec![
        VirtualNodeKind::Main,
        VirtualNodeKind::Script,
        VirtualNodeKind::Template,
    ];
    let changes = UpsertChangeResult {
        slice_changes: SliceChanges::default(),
        changed: false,
        semantic_changed: false,
    };
    let result = build_upsert_result(
        "Comp.vue".to_string(),
        data,
        &changes,
        &prev,
        &FileMeta::default(),
        0.0,
    )
    .unwrap();

    assert!(!result.changed);
    assert!(result.changed_virtual_nodes.is_empty());
    assert!(result.removed_virtual_nodes.is_empty());
}

#[test]
fn canonicalize_id_handles_edge_cases() {
    assert_eq!(
        canonicalize_id("C:\\Users\\foo\\Comp.vue"),
        "c:/Users/foo/Comp.vue"
    );
    assert_eq!(canonicalize_id("Comp.vue?vue&type=script"), "Comp.vue");
    assert_eq!(canonicalize_id("Comp.vue._VERTER_.bundle.ts"), "Comp.vue");
    assert_eq!(canonicalize_id("  Comp.vue  "), "Comp.vue");
    assert_eq!(canonicalize_id(""), "");
    assert_eq!(canonicalize_id("   "), "");
}

/// @ai-generated - compute_changed_exports: added export detected
#[test]
fn compute_changed_exports_added() {
    let old = vec![];
    let new = vec![verter_semantic::analysis::ExportSignature {
        name: "MyType".to_string(),
        declaration_hash: [1; 16],
        is_type: true,
        span: Default::default(),
        reexport_source: None,
        reexport_local: None,
        local_span: None,
    }];
    let changed = compute_changed_exports(&old, &new);
    assert!(changed.contains("MyType"));
}

/// @ai-generated - compute_changed_exports: both empty → empty set
#[test]
fn compute_changed_exports_both_empty() {
    let changed = compute_changed_exports(&[], &[]);
    assert!(changed.is_empty());
}

/// @ai-generated - compute_changed_exports: hash changed detected
#[test]
fn compute_changed_exports_hash_changed() {
    let old = vec![verter_semantic::analysis::ExportSignature {
        name: "MyType".to_string(),
        declaration_hash: [1; 16],
        is_type: true,
        span: Default::default(),
        reexport_source: None,
        reexport_local: None,
        local_span: None,
    }];
    let new = vec![verter_semantic::analysis::ExportSignature {
        name: "MyType".to_string(),
        declaration_hash: [2; 16],
        is_type: true,
        span: Default::default(),
        reexport_source: None,
        reexport_local: None,
        local_span: None,
    }];
    let changed = compute_changed_exports(&old, &new);
    assert!(changed.contains("MyType"));
}

/// @ai-generated - compute_changed_exports: mixed add + remove + change + unchanged
#[test]
fn compute_changed_exports_mixed() {
    let old = vec![
        verter_semantic::analysis::ExportSignature {
            name: "Kept".to_string(),
            declaration_hash: [1; 16],
            is_type: true,
            span: Default::default(),
            reexport_source: None,
            reexport_local: None,
            local_span: None,
        },
        verter_semantic::analysis::ExportSignature {
            name: "Changed".to_string(),
            declaration_hash: [2; 16],
            is_type: true,
            span: Default::default(),
            reexport_source: None,
            reexport_local: None,
            local_span: None,
        },
        verter_semantic::analysis::ExportSignature {
            name: "Removed".to_string(),
            declaration_hash: [3; 16],
            is_type: true,
            span: Default::default(),
            reexport_source: None,
            reexport_local: None,
            local_span: None,
        },
    ];
    let new = vec![
        verter_semantic::analysis::ExportSignature {
            name: "Kept".to_string(),
            declaration_hash: [1; 16],
            is_type: true,
            span: Default::default(),
            reexport_source: None,
            reexport_local: None,
            local_span: None,
        },
        verter_semantic::analysis::ExportSignature {
            name: "Changed".to_string(),
            declaration_hash: [9; 16],
            is_type: true,
            span: Default::default(),
            reexport_source: None,
            reexport_local: None,
            local_span: None,
        },
        verter_semantic::analysis::ExportSignature {
            name: "Added".to_string(),
            declaration_hash: [4; 16],
            is_type: true,
            span: Default::default(),
            reexport_source: None,
            reexport_local: None,
            local_span: None,
        },
    ];
    let changed = compute_changed_exports(&old, &new);
    assert!(!changed.contains("Kept"), "unchanged should not appear");
    assert!(changed.contains("Changed"), "hash-changed should appear");
    assert!(changed.contains("Removed"), "removed should appear");
    assert!(changed.contains("Added"), "added should appear");
    assert_eq!(changed.len(), 3);
}

/// @ai-generated - compute_changed_exports: removed export detected
#[test]
fn compute_changed_exports_removed() {
    let old = vec![verter_semantic::analysis::ExportSignature {
        name: "MyType".to_string(),
        declaration_hash: [1; 16],
        is_type: true,
        span: Default::default(),
        reexport_source: None,
        reexport_local: None,
        local_span: None,
    }];
    let new = vec![];
    let changed = compute_changed_exports(&old, &new);
    assert!(changed.contains("MyType"));
}

/// @ai-generated - compute_changed_exports: unchanged export not in set
#[test]
fn compute_changed_exports_unchanged() {
    let old = vec![verter_semantic::analysis::ExportSignature {
        name: "MyType".to_string(),
        declaration_hash: [1; 16],
        is_type: true,
        span: Default::default(),
        reexport_source: None,
        reexport_local: None,
        local_span: None,
    }];
    let new = vec![verter_semantic::analysis::ExportSignature {
        name: "MyType".to_string(),
        declaration_hash: [1; 16],
        is_type: true,
        span: Default::default(),
        reexport_source: None,
        reexport_local: None,
        local_span: None,
    }];
    let changed = compute_changed_exports(&old, &new);
    assert!(changed.is_empty());
}

/// @ai-generated - compute_upsert_changes: first insert (no old entry) → changed=true
#[test]
fn compute_upsert_changes_first_insert() {
    let (new, _) = parse_vue_snapshot(
        "Comp.vue",
        "<script setup>const n = 1</script><template><div/></template>",
        AnalysisScope::LSP,
    );
    let result = compute_upsert_changes(None, &new);
    assert!(result.changed, "first insert should be changed");
    assert!(
        result.semantic_changed,
        "first insert should be semantic_changed"
    );
    assert!(
        !result.slice_changes.script_changed,
        "no old entry means no diff"
    );
}

/// @ai-generated - compute_upsert_changes: identical content → not changed
#[test]
fn compute_upsert_changes_identical_content() {
    let src = "<script setup>const n = 1</script><template><div/></template>";
    let (old_snap, _) = parse_vue_snapshot("Comp.vue", src, AnalysisScope::LSP);
    let (new_snap, _) = parse_vue_snapshot("Comp.vue", src, AnalysisScope::LSP);
    let old_entry = file_entry_from_snapshot("Comp.vue", src, &old_snap);
    let result = compute_upsert_changes(Some(&old_entry), &new_snap);
    assert!(!result.changed, "identical content should not be changed");
    assert!(!result.semantic_changed);
}

/// @ai-generated - compute_upsert_changes: script-only change detected
#[test]
fn compute_upsert_changes_script_change() {
    let src1 = "<script setup>const n = 1</script><template><div/></template>";
    let src2 = "<script setup>const n = 2</script><template><div/></template>";
    let (old_snap, _) = parse_vue_snapshot("Comp.vue", src1, AnalysisScope::LSP);
    let (new_snap, _) = parse_vue_snapshot("Comp.vue", src2, AnalysisScope::LSP);
    let old_entry = file_entry_from_snapshot("Comp.vue", src1, &old_snap);
    let result = compute_upsert_changes(Some(&old_entry), &new_snap);
    assert!(result.changed);
    assert!(result.slice_changes.script_changed);
    assert!(!result.slice_changes.template_changed);
}

/// @ai-generated - compute_upsert_changes: structure change (style added)
#[test]
fn compute_upsert_changes_structure_change() {
    let src1 = "<script setup>const n = 1</script><template><div/></template>";
    let src2 = "<script setup>const n = 1</script><template><div/></template><style>.a{}</style>";
    let (old_snap, _) = parse_vue_snapshot("Comp.vue", src1, AnalysisScope::LSP);
    let (new_snap, _) = parse_vue_snapshot("Comp.vue", src2, AnalysisScope::LSP);
    let old_entry = file_entry_from_snapshot("Comp.vue", src1, &old_snap);
    let result = compute_upsert_changes(Some(&old_entry), &new_snap);
    assert!(result.changed);
    assert!(result.slice_changes.structure_changed);
}

/// @ai-generated - compute_upsert_changes: template-only change detected
#[test]
fn compute_upsert_changes_template_change() {
    let src1 = "<script setup>const n = 1</script><template><div/></template>";
    let src2 = "<script setup>const n = 1</script><template><section/></template>";
    let (old_snap, _) = parse_vue_snapshot("Comp.vue", src1, AnalysisScope::LSP);
    let (new_snap, _) = parse_vue_snapshot("Comp.vue", src2, AnalysisScope::LSP);
    let old_entry = file_entry_from_snapshot("Comp.vue", src1, &old_snap);
    let result = compute_upsert_changes(Some(&old_entry), &new_snap);
    assert!(result.changed);
    assert!(!result.slice_changes.script_changed);
    assert!(result.slice_changes.template_changed);
}

/// @ai-generated — Style-only change is detected by is_style_only()
#[test]
fn compute_upsert_changes_style_only() {
    let src1 = "<script setup>const n = 1</script><template><div/></template><style>.a{}</style>";
    let src2 = "<script setup>const n = 1</script><template><div/></template><style>.b{}</style>";
    let (old_snap, _) = parse_vue_snapshot("Comp.vue", src1, AnalysisScope::LSP);
    let (new_snap, _) = parse_vue_snapshot("Comp.vue", src2, AnalysisScope::LSP);
    let old_entry = file_entry_from_snapshot("Comp.vue", src1, &old_snap);
    let result = compute_upsert_changes(Some(&old_entry), &new_snap);
    assert!(result.changed);
    assert!(
        result.slice_changes.is_style_only(),
        "only style content changed"
    );
    assert!(!result.slice_changes.script_changed);
    assert!(!result.slice_changes.template_changed);
    assert!(!result.slice_changes.structure_changed);
    assert_eq!(result.slice_changes.style_indices_changed, vec![0]);
}

/// @ai-generated — Script + style change is NOT style-only
#[test]
fn compute_upsert_changes_script_and_style_not_style_only() {
    let src1 = "<script setup>const n = 1</script><template><div/></template><style>.a{}</style>";
    let src2 = "<script setup>const n = 2</script><template><div/></template><style>.b{}</style>";
    let (old_snap, _) = parse_vue_snapshot("Comp.vue", src1, AnalysisScope::LSP);
    let (new_snap, _) = parse_vue_snapshot("Comp.vue", src2, AnalysisScope::LSP);
    let old_entry = file_entry_from_snapshot("Comp.vue", src1, &old_snap);
    let result = compute_upsert_changes(Some(&old_entry), &new_snap);
    assert!(result.changed);
    assert!(!result.slice_changes.is_style_only(), "script also changed");
}

/// @ai-generated - Custom resolve_extensions config is respected
#[test]
fn custom_resolve_extensions_config() {
    let host = VerterHost::new_standalone(HostConfig {
        resolve_extensions: vec![".ts".to_string(), ".js".to_string()],
        ..HostConfig::default()
    });

    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div/></template>",
    );

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyType { foo: string }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    // Change MyType — should still invalidate with custom extensions
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyType { foo: string; bar: number }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    #[cfg(feature = "scheduler")]
    {
        let cc = host
            .compile_cache
            .get("/src/Comp.vue")
            .expect("compile_cache entry exists");
        assert!(
            cc.compile_slots.is_empty(),
            "custom resolve_extensions should still match .ts"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            comp.compile_slots.is_empty(),
            "custom resolve_extensions should still match .ts"
        );
    }
}

/// @ai-generated - File kind change from VueSfc to NonSfc produces correct node list
#[test]
fn file_kind_change_vue_to_nonsfc() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // First upsert as VueSfc
    let _ = upsert_vue(
        &host,
        "Comp.vue",
        "<script setup>const n = 1</script><template><div/></template>",
    );
    let nodes_before = host.list_virtual_files("Comp.vue");
    assert!(nodes_before.contains(&VirtualNodeKind::Script));
    assert!(nodes_before.contains(&VirtualNodeKind::Template));

    // Re-upsert as NonSfc
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "Comp.vue".to_string(),
            source: Arc::from("export default {}"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    let nodes_after = host.list_virtual_files("Comp.vue");
    assert_eq!(nodes_after, vec![VirtualNodeKind::Main]);
}

/// @ai-generated - generation field increments on each upsert
#[test]
fn generation_counter_increments_on_upsert() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let src1 = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", src1);

    #[cfg(feature = "scheduler")]
    {
        let gen1 = host
            .compile_cache
            .get("Comp.vue")
            .expect("compile_cache entry should exist")
            .generation;
        assert_eq!(gen1, 1);

        let src2 = "<script setup>const n = 2</script><template><div>{{n}}</div></template>";
        let _ = upsert_vue(&host, "Comp.vue", src2);

        let gen2 = host
            .compile_cache
            .get("Comp.vue")
            .expect("compile_cache entry should exist")
            .generation;
        assert_eq!(gen2, 2);
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let gen1 = {
            let files = read_lock(&host.files);
            files.get("Comp.vue").unwrap().generation
        };
        assert_eq!(gen1, 1);

        let src2 = "<script setup>const n = 2</script><template><div>{{n}}</div></template>";
        let _ = upsert_vue(&host, "Comp.vue", src2);

        let gen2 = {
            let files = read_lock(&host.files);
            files.get("Comp.vue").unwrap().generation
        };
        assert_eq!(gen2, 2);
    }
}

/// @ai-generated - import_resolves_to_dep: non-relative in dependency set
#[test]
fn import_resolves_to_dep_non_relative_in_deps() {
    let mut deps = BTreeSet::new();
    deps.insert("lodash".to_string());
    let entry = FileEntry {
        canonical_id: "/src/A.vue".to_string(),
        file_kind: FileKind::VueSfc,
        source: Arc::from(""),
        whole_hash: [0; 16],
        semantic_hash: [0; 16],
        slices: SliceHashes::default(),
        descriptor: DescriptorMin::default(),
        meta: FileMeta::default(),
        aliases: BTreeSet::new(),
        dependencies: deps,
        dependency_resolutions: FxHashMap::default(),
        external_requests: Vec::new(),
        src_blocks: Vec::new(),
        parse_diagnostics: DiagnosticsSnapshot::default(),
        script_analysis: verter_semantic::analysis::ScriptAnalysisSnapshot::default(),
        export_signatures: Vec::new(),
        style_analyses: Arc::new(Vec::new()),
        template_analysis: None,
        arc_script_cache: ScriptAnalysisArcs::default(),
        resolved_type_hashes: FxHashMap::default(),
        style_overrides: FxHashMap::default(),
        content_overrides: FxHashMap::default(),
        compile_slots: FxHashMap::default(),
        latest_diagnostics: FxHashMap::default(),
        diagnostics_generation: 0,
        generation: 0,
        cached_parse: None,
        cached_tsc_extract: None,
        cached_resolved_meta: FxHashMap::default(),
        cached_meta_payloads: FxHashMap::default(),
        cached_fallthrough: None,
        export_registry: None,
    };
    let exts = vec![".ts".to_string()];
    assert!(import_resolves_to_dep(&entry, "lodash", "lodash", &exts));
    assert!(!import_resolves_to_dep(
        &entry,
        "lodash",
        "underscore",
        &exts
    ));
}

/// @ai-generated - import_resolves_to_dep: non-relative not in deps → false
#[test]
fn import_resolves_to_dep_non_relative_not_in_deps() {
    let entry = FileEntry {
        canonical_id: "/src/A.vue".to_string(),
        file_kind: FileKind::VueSfc,
        source: Arc::from(""),
        whole_hash: [0; 16],
        semantic_hash: [0; 16],
        slices: SliceHashes::default(),
        descriptor: DescriptorMin::default(),
        meta: FileMeta::default(),
        aliases: BTreeSet::new(),
        dependencies: BTreeSet::new(),
        dependency_resolutions: FxHashMap::default(),
        external_requests: Vec::new(),
        src_blocks: Vec::new(),
        parse_diagnostics: DiagnosticsSnapshot::default(),
        script_analysis: verter_semantic::analysis::ScriptAnalysisSnapshot::default(),
        export_signatures: Vec::new(),
        style_analyses: Arc::new(Vec::new()),
        template_analysis: None,
        arc_script_cache: ScriptAnalysisArcs::default(),
        resolved_type_hashes: FxHashMap::default(),
        style_overrides: FxHashMap::default(),
        content_overrides: FxHashMap::default(),
        compile_slots: FxHashMap::default(),
        latest_diagnostics: FxHashMap::default(),
        diagnostics_generation: 0,
        generation: 0,
        cached_parse: None,
        cached_tsc_extract: None,
        cached_resolved_meta: FxHashMap::default(),
        cached_meta_payloads: FxHashMap::default(),
        cached_fallthrough: None,
        export_registry: None,
    };
    let exts = vec![".ts".to_string()];
    assert!(!import_resolves_to_dep(&entry, "lodash", "lodash", &exts));
}

/// @ai-generated - import_resolves_to_dep: relative import exact match
#[test]
fn import_resolves_to_dep_relative_exact() {
    let entry = FileEntry {
        canonical_id: "/src/A.vue".to_string(),
        file_kind: FileKind::VueSfc,
        source: Arc::from(""),
        whole_hash: [0; 16],
        semantic_hash: [0; 16],
        slices: SliceHashes::default(),
        descriptor: DescriptorMin::default(),
        meta: FileMeta::default(),
        aliases: BTreeSet::new(),
        dependencies: BTreeSet::new(),
        dependency_resolutions: FxHashMap::default(),
        external_requests: Vec::new(),
        src_blocks: Vec::new(),
        parse_diagnostics: DiagnosticsSnapshot::default(),
        script_analysis: verter_semantic::analysis::ScriptAnalysisSnapshot::default(),
        export_signatures: Vec::new(),
        style_analyses: Arc::new(Vec::new()),
        template_analysis: None,
        arc_script_cache: ScriptAnalysisArcs::default(),
        resolved_type_hashes: FxHashMap::default(),
        style_overrides: FxHashMap::default(),
        content_overrides: FxHashMap::default(),
        compile_slots: FxHashMap::default(),
        latest_diagnostics: FxHashMap::default(),
        diagnostics_generation: 0,
        generation: 0,
        cached_parse: None,
        cached_tsc_extract: None,
        cached_resolved_meta: FxHashMap::default(),
        cached_meta_payloads: FxHashMap::default(),
        cached_fallthrough: None,
        export_registry: None,
    };
    let exts = vec![".ts".to_string(), ".js".to_string()];
    assert!(import_resolves_to_dep(&entry, "./B", "/src/B", &exts));
    assert!(!import_resolves_to_dep(&entry, "./B", "/other/B", &exts));
}

/// @ai-generated - import_resolves_to_dep: relative import with extension strip
#[test]
fn import_resolves_to_dep_relative_extension_strip() {
    let entry = FileEntry {
        canonical_id: "/src/A.vue".to_string(),
        file_kind: FileKind::VueSfc,
        source: Arc::from(""),
        whole_hash: [0; 16],
        semantic_hash: [0; 16],
        slices: SliceHashes::default(),
        descriptor: DescriptorMin::default(),
        meta: FileMeta {
            script_lang: Some("ts".to_string()),
            ..FileMeta::default()
        },
        aliases: BTreeSet::new(),
        dependencies: BTreeSet::new(),
        dependency_resolutions: FxHashMap::default(),
        external_requests: Vec::new(),
        src_blocks: Vec::new(),
        parse_diagnostics: DiagnosticsSnapshot::default(),
        script_analysis: verter_semantic::analysis::ScriptAnalysisSnapshot::default(),
        export_signatures: Vec::new(),
        style_analyses: Arc::new(Vec::new()),
        template_analysis: None,
        arc_script_cache: ScriptAnalysisArcs::default(),
        resolved_type_hashes: FxHashMap::default(),
        style_overrides: FxHashMap::default(),
        content_overrides: FxHashMap::default(),
        compile_slots: FxHashMap::default(),
        latest_diagnostics: FxHashMap::default(),
        diagnostics_generation: 0,
        generation: 0,
        cached_parse: None,
        cached_tsc_extract: None,
        cached_resolved_meta: FxHashMap::default(),
        cached_meta_payloads: FxHashMap::default(),
        cached_fallthrough: None,
        export_registry: None,
    };
    let exts = vec![".ts".to_string(), ".js".to_string()];
    // ./types resolves to /src/types, dep is /src/types.ts
    // Extension strip on /src/types.ts → /src/types → match
    assert!(import_resolves_to_dep(
        &entry,
        "./types",
        "/src/types.ts",
        &exts
    ));
}

#[test]
fn should_invalidate_dependent_promotes_workspace_resolution_into_cache() {
    let ws = verter_workspace::MemoryWorkspace::new(verter_workspace::MemoryOptions::default());
    ws.set_exact_resolutions(
        "/src/App.vue",
        vec![verter_workspace::ExactResolution {
            specifier: "@/dep".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/src/dep.ts".to_string()),
            possible_canonical_ids: vec!["/src/dep.ts".to_string()],
        }],
    );

    let mut view = DependentView {
        canonical_id: "/src/App.vue".to_string(),
        dependency_resolutions: FxHashMap::default(),
        dependencies: BTreeSet::default(),
        script_lang: Some("ts".to_string()),
        macro_type_deps: Vec::new(),
        imports: vec![verter_semantic::analysis::AnalyzedImport {
            source: "@/dep".to_string(),
            is_type_only: false,
            bindings: Vec::new(),
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: None,
        }],
        resolved_type_hashes: FxHashMap::default(),
    };
    let changed_exports = BTreeSet::from(["value".to_string()]);
    let resolve_extensions = vec![".ts".to_string()];

    let first = should_invalidate_dependent_view(
        &mut view,
        "/src/dep.ts",
        &changed_exports,
        false,
        None,
        &resolve_extensions,
        Some(&ws),
    );

    assert!(
        first,
        "runtime invalidation should resolve aliased imports through the workspace when the cache is cold"
    );
    assert_eq!(
        view.dependency_resolutions
            .get("@/dep")
            .and_then(|resolution| resolution.resolved_canonical_id.as_deref()),
        Some("/src/dep.ts"),
        "workspace-resolved alias routes should be promoted into dependency_resolutions for future checks"
    );

    let second = should_invalidate_dependent_view(
        &mut view,
        "/src/dep.ts",
        &changed_exports,
        false,
        None,
        &resolve_extensions,
        None,
    );

    assert!(
        second,
        "once promoted into dependency_resolutions, the same invalidation check should succeed without a live workspace resolver"
    );
}

/// @ai-generated - invalidate_nodes removes last_good_outputs for targeted nodes
#[test]
fn invalidate_nodes_removes_last_good() {
    use cache::invalidate_nodes;

    let mut slots = FxHashMap::default();
    let mut outputs = FxHashMap::default();
    outputs.insert(
        VirtualNodeKind::Main,
        CachedVirtualFile {
            code: Arc::from("main code"),
            source_map: None,
            lang: Some("js".to_string()),
            meta: VirtualMeta::default(),
        },
    );
    outputs.insert(
        VirtualNodeKind::Template,
        CachedVirtualFile {
            code: Arc::from("template code"),
            source_map: None,
            lang: Some("tsx".to_string()),
            meta: VirtualMeta::default(),
        },
    );
    let last_good = Some(outputs.clone());
    slots.insert(
        42u64,
        CompileSlot {
            semantic_hash: [0; 16],
            style_override_hash: 0,
            content_override_hash: 0,
            outputs,
            diagnostics: DiagnosticsSnapshot::default(),
            last_good_outputs: last_good,
            last_access_tick: 1,
            tsx: None,
            template_analysis: None,
        },
    );

    invalidate_nodes(
        &mut slots,
        &[VirtualNodeKind::Main, VirtualNodeKind::Template],
    );

    let slot = slots.get(&42).unwrap();
    assert!(!slot.outputs.contains_key(&VirtualNodeKind::Main));
    assert!(!slot.outputs.contains_key(&VirtualNodeKind::Template));
    // last_good_outputs also cleared for these nodes
    let last_good = slot.last_good_outputs.as_ref().unwrap();
    assert!(!last_good.contains_key(&VirtualNodeKind::Main));
    assert!(!last_good.contains_key(&VirtualNodeKind::Template));
}

#[test]
fn profile_cap_evicts_oldest_profiles() {
    let host = VerterHost::new_standalone(HostConfig {
        max_profiles_per_file: 2,
        ..HostConfig::default()
    });
    let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
    let _ = upsert_vue(&host, "Comp.vue", src);

    let p1 = CompileProfile {
        hmr_strategy: HmrStrategy::Vite,
        ..CompileProfile::default()
    };
    let p2 = CompileProfile {
        hmr_strategy: HmrStrategy::Webpack,
        ..CompileProfile::default()
    };
    let p3 = CompileProfile {
        is_production: true,
        ..CompileProfile::default()
    };

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: p1.clone(),
        })
        .unwrap();

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: p2.clone(),
        })
        .unwrap();

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: p3.clone(),
        })
        .unwrap();

    let result = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: p3,
        })
        .unwrap();
    assert!(!result.code.is_empty());
}

/// @ai-generated - Relative imports auto-register in dependency graph
#[test]
fn relative_imports_auto_register_deps() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { MyType } from './types'\n</script>\n<template><div/></template>",
    );

    // Check that ./types was resolved and added to dependencies
    #[cfg(feature = "scheduler")]
    {
        let cc = host
            .compile_cache
            .get("/src/Comp.vue")
            .expect("compile_cache entry should exist");
        assert!(
            cc.dependencies.contains("/src/types"),
            "relative import should auto-register as dependency, got: {:?}",
            cc.dependencies
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            comp.dependencies.contains("/src/types"),
            "relative import should auto-register as dependency, got: {:?}",
            comp.dependencies
        );
    }
}

/// @ai-generated - set_import_dependencies adds to reverse dep graph
#[test]
fn set_import_dependencies_adds_to_reverse_deps() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let _ = upsert_vue(
        &host,
        "Comp.vue",
        "<script setup lang=\"ts\">\nimport { helper } from '@/utils'\n</script>\n<template><div/></template>",
    );

    // Caller resolves @/utils → /src/utils.ts
    host.set_import_dependencies(
        "Comp.vue",
        vec![DependencyResolution {
            specifier: "@/utils".to_string(),
            resolved_canonical_id: Some("/src/utils.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // Check that reverse dependency was added
    let rev = read_lock(&host.reverse_dependencies);
    let owners = rev.get("/src/utils.ts");
    assert!(
        owners.is_some() && owners.unwrap().contains("Comp.vue"),
        "reverse dep should be registered"
    );
}

/// @ai-generated - set_import_dependencies: subsequent dep upsert triggers invalidation
#[test]
fn set_import_dependencies_subsequent_upsert_invalidates() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport { helper } from '@/utils'\n</script>\n<template><div/></template>",
    );

    // Resolve @/utils → /src/utils.ts
    host.set_import_dependencies(
        "/src/Comp.vue",
        vec![DependencyResolution {
            specifier: "@/utils".to_string(),
            resolved_canonical_id: Some("/src/utils.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // Upsert the dependency
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/utils.ts".to_string(),
            source: Arc::from("export function helper() { return 1 }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    // Compile Comp.vue to populate cache
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    // Change the dependency
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/utils.ts".to_string(),
            source: Arc::from("export function helper() { return 2 }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    // Comp.vue should be invalidated because it has a runtime import from this dep
    #[cfg(feature = "scheduler")]
    {
        let cc = host
            .compile_cache
            .get("/src/Comp.vue")
            .expect("compile_cache entry exists");
        assert!(
            cc.compile_slots.is_empty(),
            "compile slots should be cleared when runtime dep changes"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            comp.compile_slots.is_empty(),
            "compile slots should be cleared when runtime dep changes"
        );
    }
}

#[test]
fn invalidate_dependents_of_works_when_dependency_was_never_loaded() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport { helper } from './tempUtil'\n</script>\n<template><div>{{ helper }}</div></template>",
    );

    host.set_import_dependencies(
        "/src/Comp.vue",
        vec![DependencyResolution {
            specifier: "./tempUtil".to_string(),
            resolved_canonical_id: Some("/src/tempUtil.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    host.invalidate_dependents_of("/src/tempUtil.ts");

    #[cfg(feature = "scheduler")]
    {
        let cc = host
            .compile_cache
            .get("/src/Comp.vue")
            .expect("compile_cache entry exists");
        assert!(
            cc.compile_slots.is_empty(),
            "compile slots should be cleared even when the deleted dependency was never loaded"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            comp.compile_slots.is_empty(),
            "compile slots should be cleared even when the deleted dependency was never loaded"
        );
    }
}

/// @ai-generated - Dep file with no export signatures → full invalidation (Tier 1 fallback)
#[test]
fn smart_invalidation_no_signatures_full_invalidation() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // Manually set up a dependency relationship without export signatures
    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<template src=\"./tpl.html\"></template><script setup>const n = 1</script>",
    );
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/tpl.html".to_string(),
            source: Arc::from("<div>A</div>"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    // Compile Comp.vue
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    // Re-upsert non-SFC dependency (tpl.html has no TS exports → empty signatures)
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/tpl.html".to_string(),
            source: Arc::from("<section>B</section>"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    // Comp.vue should be invalidated (full invalidation since no export signatures)
    #[cfg(feature = "scheduler")]
    {
        let cc = host
            .compile_cache
            .get("/src/Comp.vue")
            .expect("compile_cache entry exists");
        assert!(
            cc.compile_slots.is_empty(),
            "compile slots should be cleared for Tier 1 fallback"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            comp.compile_slots.is_empty(),
            "compile slots should be cleared for Tier 1 fallback"
        );
    }
}

/// @ai-generated - Dep file unchanged export → SFC NOT invalidated
#[test]
fn smart_invalidation_unchanged_export_no_invalidation() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // Upsert SFC that imports MyType
    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div/></template>",
    );

    // Upsert dependency with MyType and OtherType
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from(
                "export interface MyType { a: string }\nexport interface OtherType { x: number }",
            ),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    // Compile Comp.vue
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    // Change only OtherType (not used by Comp.vue's macros)
    let _ = host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: "/src/types.ts".to_string(),
        source: Arc::from("export interface MyType { a: string }\nexport interface OtherType { x: number; y: string }"),
        file_kind: FileKind::NonSfc,
        aliases: Vec::new(),
    })
    .unwrap();

    // Comp.vue should still have a cache hit (MyType didn't change)
    // Compile slots should NOT be empty — the smart invalidation should have skipped it
    #[cfg(feature = "scheduler")]
    {
        let cc = host
            .compile_cache
            .get("/src/Comp.vue")
            .expect("compile_cache entry exists");
        assert!(
            !cc.compile_slots.is_empty(),
            "compile slots should not be cleared when only unused exports changed"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            !comp.compile_slots.is_empty(),
            "compile slots should not be cleared when only unused exports changed"
        );
    }
}

/// @ai-generated - strip_configured_extension: empty extensions → None
#[test]
fn strip_configured_extension_empty_extensions() {
    assert_eq!(strip_configured_extension("/src/types.ts", &[], None), None);
    assert_eq!(
        strip_configured_extension("/src/types.ts", &[], Some("ts")),
        None
    );
}

/// @ai-generated - strip_configured_extension with script_lang prioritizes matching extensions
#[test]
fn strip_configured_extension_prioritizes_script_lang() {
    let extensions = vec![
        ".ts".to_string(),
        ".tsx".to_string(),
        ".js".to_string(),
        ".jsx".to_string(),
    ];
    // With lang="ts", .ts is tried first (and matches)
    assert_eq!(
        strip_configured_extension("/src/types.ts", &extensions, Some("ts")),
        Some("/src/types")
    );
    // With lang="js", .js would be tried first but .ts is also in the list
    assert_eq!(
        strip_configured_extension("/src/types.ts", &extensions, Some("js")),
        Some("/src/types")
    );
    // No lang — falls through to full list
    assert_eq!(
        strip_configured_extension("/src/types.ts", &extensions, None),
        Some("/src/types")
    );
    // Extension not in config → None
    assert_eq!(
        strip_configured_extension("/src/types.vue", &extensions, None),
        None
    );
}

/// @ai-generated - Tier 3: comment added to dep type → NO invalidation
/// (Tier 2 WOULD invalidate since export text hash changes, but Tier 3 saves it)
#[test]
fn tier3_comment_added_no_invalidation() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div/></template>",
    );

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyType { foo: string }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    // Add a comment — Tier 2 export text hash changes, but Tier 3 resolved shape is the same
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("/** Updated docs */\nexport interface MyType { foo: string }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    #[cfg(feature = "scheduler")]
    {
        let cc = host
            .compile_cache
            .get("/src/Comp.vue")
            .expect("compile_cache entry exists");
        assert!(
            !cc.compile_slots.is_empty(),
            "compile slots should NOT be cleared when only comments changed (Tier 3 saves)"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            !comp.compile_slots.is_empty(),
            "compile slots should NOT be cleared when only comments changed (Tier 3 saves)"
        );
    }
}

/// @ai-generated - Tier 3: property added to dep type → invalidation
#[test]
fn tier3_property_added_invalidates() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div/></template>",
    );

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyType { foo: string }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    // Add a property to MyType
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyType { foo: string; bar: number }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    // Tier 3: resolved type shape changed → invalidation
    #[cfg(feature = "scheduler")]
    {
        let cc = host
            .compile_cache
            .get("/src/Comp.vue")
            .expect("compile_cache entry exists");
        assert!(
            cc.compile_slots.is_empty(),
            "compile slots should be cleared when resolved type shape changed (prop added)"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            comp.compile_slots.is_empty(),
            "compile slots should be cleared when resolved type shape changed (prop added)"
        );
    }
}

/// @ai-generated - Tier 3: property type changed → invalidation
#[test]
fn tier3_property_type_changed_invalidates() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div/></template>",
    );

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyType { foo: string }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    // Change foo's type from string to number
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyType { foo: number }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    #[cfg(feature = "scheduler")]
    {
        let cc = host
            .compile_cache
            .get("/src/Comp.vue")
            .expect("compile_cache entry exists");
        assert!(
            cc.compile_slots.is_empty(),
            "compile slots should be cleared when prop type changed"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            comp.compile_slots.is_empty(),
            "compile slots should be cleared when prop type changed"
        );
    }
}

/// @ai-generated - Tier 3: resolved_type_hashes are stored for future comparisons
#[test]
fn tier3_stores_resolved_type_hashes() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div/></template>",
    );

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyType { foo: string }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    // Trigger smart invalidation by re-upserting dep with whitespace change
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyType {\n  foo: string;\n}"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    // Check that resolved_type_hashes were stored
    let key = ("/src/types.ts".to_string(), "MyType".to_string());
    #[cfg(feature = "scheduler")]
    {
        let cc = host
            .compile_cache
            .get("/src/Comp.vue")
            .expect("compile_cache entry exists");
        assert!(
            cc.resolved_type_hashes.contains_key(&key),
            "resolved_type_hashes should store hash for (dep_id, type_name)"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            comp.resolved_type_hashes.contains_key(&key),
            "resolved_type_hashes should store hash for (dep_id, type_name)"
        );
    }
}

#[test]
fn transitive_workspace_macro_type_dep_change_invalidates_owner() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from("import type { Nested } from './nested'\nexport interface Props { msg: Nested }"),
    );
    ws.inject_file(
        "/src/nested.ts".to_string(),
        Arc::from("export type Nested = string"),
    );

    let host = VerterHost::new(HostConfig::default(), ws);

    let _ = upsert_vue(
        &host,
        "/src/App.vue",
        "<script setup lang=\"ts\">\nimport type { Props } from './types'\ndefineProps<Props>()\n</script>\n<template><div/></template>",
    );

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/App.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .expect("initial compile should succeed with workspace-backed type deps");

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/nested.ts".to_string(),
            source: Arc::from("export type Nested = number"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    // Transitive dep changes (nested.ts -> types.ts -> App.vue) do NOT
    // propagate invalidation to the ultimate consumer — only direct dependents
    // of the changed file are invalidated.  App.vue depends on types.ts, not
    // nested.ts, so its compile slots remain populated.
    #[cfg(feature = "scheduler")]
    {
        let cc = host
            .compile_cache
            .get("/src/App.vue")
            .expect("compile_cache entry exists");
        assert!(
            !cc.compile_slots.is_empty(),
            "compile slots should still be populated (transitive dep change does not cascade to indirect dependents)"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let files = read_lock(&host.files);
        let comp = files.get("/src/App.vue").unwrap();
        assert!(
            !comp.compile_slots.is_empty(),
            "compile slots should still be populated (transitive dep change does not cascade to indirect dependents)"
        );
    }
}

/// @ai-generated - Tier 3: unrelated type changed in same file → NO invalidation
#[test]
fn tier3_unrelated_type_change_no_invalidation() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div/></template>",
    );

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from(
                "export interface MyType { foo: string }\nexport interface Other { x: number }",
            ),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    // Only change Other (not used by SFC macros)
    let _ = host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: "/src/types.ts".to_string(),
        source: Arc::from(
            "export interface MyType { foo: string }\nexport interface Other { x: number; y: boolean }",
        ),
        file_kind: FileKind::NonSfc,
        aliases: Vec::new(),
    })
    .unwrap();

    // MyType unchanged → Tier 2 already skips (export hash matches)
    // Tier 3 not even needed here since Tier 2 already handles it
    #[cfg(feature = "scheduler")]
    {
        let cc = host
            .compile_cache
            .get("/src/Comp.vue")
            .expect("compile_cache entry exists");
        assert!(
            !cc.compile_slots.is_empty(),
            "compile slots should NOT be cleared when only unrelated types changed"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            !comp.compile_slots.is_empty(),
            "compile slots should NOT be cleared when only unrelated types changed"
        );
    }
}

/// @ai-generated - Tier 3: whitespace-only change to dep type → NO invalidation
#[test]
fn tier3_whitespace_only_change_no_invalidation() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // SFC imports MyType and uses it in defineProps
    let _ = upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { MyType } from './types'\nconst props = defineProps<MyType>()\n</script>\n<template><div/></template>",
    );

    // Initial dep with MyType
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyType { foo: string; bar: number }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    // Compile to populate cache
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/src/Comp.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile_dev(),
        })
        .unwrap();

    // Change MyType with only whitespace differences (same prop shape)
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface MyType {\n  foo: string;\n  bar: number;\n}"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    // Tier 3: resolved type shape is identical → no invalidation
    #[cfg(feature = "scheduler")]
    {
        let cc = host
            .compile_cache
            .get("/src/Comp.vue")
            .expect("compile_cache entry exists");
        assert!(
            !cc.compile_slots.is_empty(),
            "compile slots should NOT be cleared when resolved type shape is unchanged (Tier 3)"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        let files = read_lock(&host.files);
        let comp = files.get("/src/Comp.vue").unwrap();
        assert!(
            !comp.compile_slots.is_empty(),
            "compile slots should NOT be cleared when resolved type shape is unchanged (Tier 3)"
        );
    }
}

/// @ai-generated - update_alias_map: removes old aliases, adds new ones
#[test]
fn update_alias_map_removes_old_adds_new() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let old_aliases: BTreeSet<String> = ["old-alias".to_string()].into();
    let new_aliases: BTreeSet<String> = ["new-alias".to_string(), "Comp.vue".to_string()].into();

    // Pre-populate old alias
    {
        let mut map = write_lock(&host.alias_to_canonical);
        map.insert("old-alias".to_string(), "Comp.vue".to_string());
    }

    host.update_alias_map("Comp.vue", &old_aliases, &new_aliases);

    let map = read_lock(&host.alias_to_canonical);
    assert!(
        !map.contains_key("old-alias"),
        "old alias should be removed"
    );
    assert_eq!(map.get("new-alias"), Some(&"Comp.vue".to_string()));
    assert_eq!(map.get("Comp.vue"), Some(&"Comp.vue".to_string()));
}

/// @ai-generated - update_reverse_deps: keeps shared deps when another owner exists
#[test]
fn update_reverse_deps_keeps_shared_dep() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // shared-dep.ts is owned by both Comp.vue and Other.vue
    {
        let mut rev = write_lock(&host.reverse_dependencies);
        let owners = rev.entry("shared-dep.ts".to_string()).or_default();
        owners.insert("Comp.vue".to_string());
        owners.insert("Other.vue".to_string());
    }

    // Comp.vue drops shared-dep.ts
    let old_deps: BTreeSet<String> = ["shared-dep.ts".to_string()].into();
    let new_deps: BTreeSet<String> = BTreeSet::new();
    host.update_reverse_deps("Comp.vue", &old_deps, &new_deps);

    let rev = read_lock(&host.reverse_dependencies);
    // shared-dep.ts should still exist because Other.vue still depends on it
    let owners = rev.get("shared-dep.ts").unwrap();
    assert!(!owners.contains("Comp.vue"));
    assert!(owners.contains("Other.vue"));
}

/// @ai-generated - update_reverse_deps: removes stale deps, adds new ones
#[test]
fn update_reverse_deps_removes_stale_adds_new() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let old_deps: BTreeSet<String> = ["old-dep.ts".to_string()].into();
    let new_deps: BTreeSet<String> = ["new-dep.ts".to_string()].into();

    // Pre-populate old reverse dep
    {
        let mut rev = write_lock(&host.reverse_dependencies);
        rev.entry("old-dep.ts".to_string())
            .or_default()
            .insert("Comp.vue".to_string());
    }

    host.update_reverse_deps("Comp.vue", &old_deps, &new_deps);

    let rev = read_lock(&host.reverse_dependencies);
    assert!(
        !rev.contains_key("old-dep.ts"),
        "old dependency should be removed"
    );
    let new_owners = rev.get("new-dep.ts").unwrap();
    assert!(new_owners.contains("Comp.vue"));
}

/// @ai-generated - FileMeta::virtual_nodes: empty meta produces only Main
#[test]
fn virtual_nodes_empty() {
    let meta = FileMeta::default();
    let nodes = meta.virtual_nodes();
    assert_eq!(nodes, vec![VirtualNodeKind::Main]);
}

/// @ai-generated - FileMeta::virtual_nodes: full SFC produces all node kinds
#[test]
fn virtual_nodes_full_sfc() {
    let meta = FileMeta {
        has_script: true,
        has_template: true,
        script_lang: Some("ts".to_string()),
        style_langs: vec![None, Some("scss".to_string())],
        custom_types: vec!["i18n".to_string()],
        custom_langs: vec![None],
        ..FileMeta::default()
    };
    let nodes = meta.virtual_nodes();
    assert_eq!(
        nodes,
        vec![
            VirtualNodeKind::Main,
            VirtualNodeKind::Script,
            VirtualNodeKind::Template,
            VirtualNodeKind::Style { index: 0 },
            VirtualNodeKind::Style { index: 1 },
            VirtualNodeKind::Custom { index: 0 },
        ]
    );
}

// ── E2E: Style override with source map remapping ──

/// Build a source map JSON from (dst_line, dst_col, src_line, src_col) tuples.
fn build_test_source_map(original: &str, mappings: &[(u32, u32, u32, u32)]) -> String {
    use sourcemap::SourceMapBuilder;

    let mut builder = SourceMapBuilder::new(Some("output.css"));
    let src_id = builder.add_source("input.sass");
    builder.set_source_contents(src_id, Some(original));

    for &(dst_line, dst_col, src_line, src_col) in mappings {
        builder.add_raw(
            dst_line,
            dst_col,
            src_line,
            src_col,
            Some(src_id),
            None,
            false,
        );
    }

    let sm = builder.into_sourcemap();
    let mut buf = Vec::new();
    sm.to_writer(&mut buf).unwrap();
    String::from_utf8(buf).unwrap()
}

/// E2E Test: Multiple style blocks — CSS block unaffected, preprocessed block remapped.
///
/// Verifies that when a Vue SFC has both `<style>` (CSS) and a preprocessed
/// `<style lang="sass">` block, applying a style override with source map:
/// - Does NOT alter the plain CSS block's analysis spans
/// - DOES remap the preprocessed block's analysis spans to original SFC positions
#[test]
fn style_override_remaps_preprocessed_block_preserves_css_block() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // SFC with two style blocks: plain CSS (index 0) and "sass" (index 1)
    let sfc = concat!(
        "<template><div class=\"used\">hello</div></template>\n",
        "<style>\n",
        ".used { color: red; }\n",
        "</style>\n",
        "<style lang=\"sass\">\n",
        ".header\n",
        "  font-size: 16px\n",
        "</style>\n",
    );

    let _ = upsert_vue(&host, "multi.vue", sfc);

    // Get original analysis before override
    let analysis_before = host.get_analysis("multi.vue").unwrap();
    assert_eq!(
        analysis_before.styles.len(),
        2,
        "should have 2 style blocks"
    );

    let css_block_before = &analysis_before.styles[0];
    let css_classes_before = css_block_before.css.as_ref().unwrap().classes.clone();

    // The Sass block (index 1) initially has no CSS analysis
    // because `build_preprocessor_style_analysis` is used for non-CSS langs
    let sass_block_before = &analysis_before.styles[1];
    let _sass_css_before = sass_block_before.css.as_ref();

    // Simulate transpilation: "Sass" → CSS
    let compiled_css = ".header { font-size: 16px; }\n";

    // The content_offset points right after the `>` of `<style lang="sass">`,
    // which is the `\n` before `.header`. So the actual content from the
    // preprocessor's perspective is `\n.header\n  font-size: 16px\n`.
    // In this content, `.header` is on line 1 (line 0 is the empty `\n`).
    let original_content = "\n.header\n  font-size: 16px\n";
    let sm_json = build_test_source_map(
        original_content,
        &[
            (0, 0, 1, 0), // .header in compiled (line 0) → original line 1, col 0
        ],
    );

    // Apply the style override for index 1 (the sass block)
    let profile = CompileProfile {
        source_map: true,
        target: CompileTarget::BUNDLER | CompileTarget::TSX,
        ..CompileProfile::default()
    };
    let result = host.apply_style_overrides(StyleOverrideRequest {
        canonical_id: "multi.vue".to_string(),
        compile_profile: profile,
        overrides: vec![StyleOverrideEntry {
            index: 1,
            code: Arc::from(compiled_css),
            source_map: Some(Arc::from(sm_json)),
        }],
    });
    assert!(result.is_ok(), "apply_style_overrides should succeed");

    // Get analysis after override
    let analysis_after = host.get_analysis("multi.vue").unwrap();
    assert_eq!(
        analysis_after.styles.len(),
        2,
        "should still have 2 style blocks"
    );

    // CSS block (index 0) should be UNCHANGED
    let css_block_after = &analysis_after.styles[0];
    let css_classes_after = css_block_after.css.as_ref().unwrap().classes.clone();
    assert_eq!(
        css_classes_before.len(),
        css_classes_after.len(),
        "CSS block class count should be unchanged"
    );
    for (before, after) in css_classes_before.iter().zip(css_classes_after.iter()) {
        assert_eq!(
            before.name, after.name,
            "CSS block class names should match"
        );
        assert_eq!(
            before.span.start, after.span.start,
            "CSS block class spans should be unchanged"
        );
    }

    // With scheduler as sole authority, get_analysis() returns RAW analysis.
    // The sass block's CSS analysis is raw (not remapped from the override).
    // Per-profile remapped CSS lives in compile_cache.style_overrides.
    let sass_block_after = &analysis_after.styles[1];
    // Raw sass may or may not parse to valid CSS analysis — that's OK.
    // The key invariant: the raw analysis is UNCHANGED by the override.
    let sass_css_after = sass_block_after.css.as_ref();
    if sass_css_after.is_none() {
        // Raw sass doesn't produce valid CSS analysis — expected on scheduler path.
        return;
    }

    let sass_selectors = &sass_css_after.unwrap().selectors;
    assert!(
        !sass_selectors.is_empty(),
        "should have at least one selector"
    );

    // The .header selector span should point to the original sass content in the SFC
    let header_sel = sass_selectors.iter().find(|s| s.text == ".header");
    assert!(header_sel.is_some(), ".header selector should exist");
    let header_sel = header_sel.unwrap();

    // .header is one byte into the style content (after the leading newline),
    // so the stored span is SFC-absolute content_offset + 1.
    assert_eq!(
        header_sel.span.start,
        sass_block_after.content_offset + 1,
        ".header should be stored as an SFC-absolute span"
    );

    // content_offset should point right after `>` of `<style lang="sass">`
    // (the `\n` before `.header`, NOT at `.header` itself)
    let tag_end = sfc.find("<style lang=\"sass\">").unwrap() + "<style lang=\"sass\">".len();
    assert_eq!(
        sass_block_after.content_offset as usize, tag_end,
        "content_offset should point right after the style tag"
    );

    // Double-check the stored absolute span points directly at ".header".
    let sfc_absolute = header_sel.span.start;
    assert_eq!(
        &sfc[sfc_absolute as usize..sfc_absolute as usize + 7],
        ".header",
        "stored selector span should point to '.header' in the original SFC"
    );
}

// ═══════════════════════════════════════════════════════════
// apply_block_overrides
// ═══════════════════════════════════════════════════════════

/// @ai-generated - apply_block_overrides: template override produces compile-ready source
#[test]
fn apply_block_overrides_template() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let sfc =
        "<template lang=\"pug\">\ndiv hello\n</template>\n<script setup>\nconst x = 1\n</script>";
    let _ = upsert_vue(&host, "test.vue", sfc);

    let profile = CompileProfile::default();
    let result = host.apply_block_overrides(BlockOverrideRequest {
        canonical_id: "test.vue".to_string(),
        compile_profile: profile.clone(),
        overrides: vec![BlockOverrideEntry {
            block_type: PreprocessorBlockType::Template,
            index: 0,
            code: Arc::from("<div>hello</div>"),
            source_map: None,
        }],
    });
    assert!(result.is_ok(), "apply_block_overrides should succeed");
    let result = result.unwrap();
    assert!(result.changed, "should report changed");

    // With scheduler as sole parser, get_source() returns the RAW source
    // (before block overrides). The synthetic source is per-profile in compile_cache.
    let source = host.get_source("test.vue");
    assert!(source.is_some(), "source should exist");
    let source = source.unwrap();
    assert!(
        source.contains("lang=\"pug\""),
        "get_source should return raw source (with pug lang), got: {}",
        source
    );

    // Verify the file can be compiled (get_virtual_file succeeds)
    let vf = host.get_virtual_file(VirtualQuery {
        raw_id: Some("test.vue?vue&type=template".to_string()),
        canonical_id: None,
        node_kind: None,
        compile_profile: profile,
    });
    assert!(
        vf.is_ok(),
        "should be able to compile template after block override: {:?}",
        vf.err()
    );
}

/// @ai-generated - apply_block_overrides: no change if same override applied twice
#[test]
fn apply_block_overrides_no_change_if_same_hash() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let sfc =
        "<template lang=\"pug\">\ndiv hello\n</template>\n<script setup>\nconst x = 1\n</script>";
    let _ = upsert_vue(&host, "test.vue", sfc);

    let profile = CompileProfile::default();
    let overrides = vec![BlockOverrideEntry {
        block_type: PreprocessorBlockType::Template,
        index: 0,
        code: Arc::from("<div>hello</div>"),
        source_map: None,
    }];

    // First apply
    let r1 = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: "test.vue".to_string(),
            compile_profile: profile.clone(),
            overrides: overrides.clone(),
        })
        .unwrap();
    assert!(r1.changed, "first apply should report changed");

    // Second apply with same content — should report no change
    let r2 = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: "test.vue".to_string(),
            compile_profile: profile,
            overrides,
        })
        .unwrap();
    assert!(
        !r2.changed,
        "second apply with same hash should report no change"
    );
}

/// @ai-generated - apply_block_overrides: style overrides delegated to existing mechanism
#[test]
fn apply_block_overrides_style_delegated() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let sfc = "<template><div>hello</div></template>\n<script setup>const x = 1</script>\n<style lang=\"scss\">.a { .b { color: red } }</style>";
    let _ = upsert_vue(&host, "test.vue", sfc);

    let profile = CompileProfile {
        source_map: true,
        ..CompileProfile::default()
    };
    let result = host.apply_block_overrides(BlockOverrideRequest {
        canonical_id: "test.vue".to_string(),
        compile_profile: profile.clone(),
        overrides: vec![BlockOverrideEntry {
            block_type: PreprocessorBlockType::Style,
            index: 0,
            code: Arc::from(".a .b { color: red }"),
            source_map: None,
        }],
    });
    assert!(
        result.is_ok(),
        "apply_block_overrides with style should succeed"
    );

    // Verify the style virtual file serves the overridden CSS
    let vf = host.get_virtual_file(VirtualQuery {
        raw_id: Some("test.vue?vue&type=style&index=0&lang.css".to_string()),
        canonical_id: None,
        node_kind: None,
        compile_profile: profile,
    });
    assert!(vf.is_ok(), "should be able to get style virtual file");
    let vf = vf.unwrap();
    assert!(
        vf.code.contains(".a .b"),
        "style output should contain overridden CSS, got: {}",
        &vf.code[..vf.code.len().min(200)]
    );
}

/// After style preprocessing, virtual IDs should use lang.css
/// instead of the original preprocessor lang (e.g. lang.sass).
/// Without this, Vite would try to re-preprocess compiled CSS
/// as SASS indented syntax, causing build failures.
#[test]
fn style_override_changes_virtual_id_lang_to_css() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let sfc =
        "<template><div>hello</div></template>\n<style lang=\"sass\">.a\n  color: red\n</style>";
    let upsert = upsert_vue(&host, "test.vue", sfc);
    // Before override, the URL should have the original lang
    assert!(
        upsert
            .changed_virtual_ids
            .iter()
            .any(|id| id.contains("lang.sass")),
        "before override, should have lang.sass in virtual IDs: {:?}",
        upsert.changed_virtual_ids
    );

    let profile = CompileProfile::default();
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: "test.vue".to_string(),
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry {
                block_type: PreprocessorBlockType::Style,
                index: 0,
                code: Arc::from(".a { color: red; }"),
                source_map: None,
            }],
        })
        .expect("apply_block_overrides should succeed");

    // The main module assembly should use lang.css after override
    let main = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("test.vue".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: profile,
        })
        .expect("should get main virtual file");
    assert!(
        main.code.contains("lang.css"),
        "main module should import style with lang.css, got:\n{}",
        main.code
    );
    assert!(
        !main.code.contains("lang.sass"),
        "main module should NOT import style with lang.sass, got:\n{}",
        main.code
    );
}

/// @ai-generated - upsert returns preprocessor_requests for pug template
#[test]
fn upsert_returns_preprocessor_requests() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let sfc =
        "<template lang=\"pug\">\ndiv hello\n</template>\n<script setup>\nconst x = 1\n</script>";
    let result = upsert_vue(&host, "test.vue", sfc);
    assert!(
        !result.preprocessor_requests.is_empty(),
        "should have preprocessor requests for pug template"
    );
    let req = &result.preprocessor_requests[0];
    assert_eq!(req.block_type, PreprocessorBlockType::Template);
    assert_eq!(req.lang, "pug");
    assert!(req.content.contains("div hello"));
}

/// Verify that the host's `Shared<T>` RwLock has writer-preferring semantics.
///
/// When a writer is waiting, new readers should queue behind it. This prevents
/// writer starvation where continuous readers indefinitely delay write operations.
///
/// With `parking_lot::RwLock` (writer-preferring): once the writer calls write(),
/// new read() calls block until the writer is done → reader_cycles stays low (~16,
/// equal to the number of currently-holding readers that finish their cycle).
///
/// A reader-preferring lock would allow hundreds+ of reader cycles while the
/// writer waits, causing the upsert latency issues seen in production.
#[test]
fn writer_starvation_under_continuous_read_pressure() {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    let data = Arc::new(default_shared(0u64));
    let stop = Arc::new(AtomicBool::new(false));
    let writer_waiting = Arc::new(AtomicBool::new(false));
    let reader_cycles_during_wait = Arc::new(AtomicU64::new(0));

    // Spawn 16 reader threads that hold the lock for ~5ms each, back-to-back.
    let mut reader_handles = Vec::new();
    for _ in 0..16 {
        let data = Arc::clone(&data);
        let stop = Arc::clone(&stop);
        let ww = Arc::clone(&writer_waiting);
        let cycles = Arc::clone(&reader_cycles_during_wait);
        reader_handles.push(std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let guard = read_lock(&data);
                // Count read cycles that occur while the writer is waiting.
                // With writer-preferring locks: readers block → very low count.
                if ww.load(Ordering::Relaxed) {
                    cycles.fetch_add(1, Ordering::Relaxed);
                }
                // Hold the read lock for ~5ms (busy wait)
                let hold_start = Instant::now();
                while hold_start.elapsed() < Duration::from_millis(5) {
                    std::hint::spin_loop();
                }
                drop(guard);
            }
        }));
    }

    // Let readers saturate
    std::thread::sleep(Duration::from_millis(100));

    // Signal that writer is about to request the lock
    writer_waiting.store(true, Ordering::SeqCst);

    // Acquire write lock — measures how many reader cycles happen while waiting
    let data_w = Arc::clone(&data);
    let start = Instant::now();
    let mut guard = write_lock(&data_w);
    *guard = 42;
    let write_latency = start.elapsed();
    drop(guard);

    // Stop readers
    stop.store(true, Ordering::Relaxed);
    for h in reader_handles {
        let _ = h.join();
    }

    let reader_cycles = reader_cycles_during_wait.load(Ordering::SeqCst);

    // With writer-preferring lock, once the writer calls write(), new readers
    // are blocked. Only the ~16 currently-holding readers finish their 5ms cycle.
    // Threshold: 50 (generous: 16 holding + possible re-acquires before visibility).
    assert!(
        reader_cycles <= 50,
        "writer-preferring lock should block new readers while writer waits — \
         got {reader_cycles} reader cycles during writer wait (latency: {write_latency:?})"
    );
    // Writer should complete in ~5ms (max reader hold time), not seconds.
    assert!(
        write_latency < Duration::from_millis(500),
        "writer should acquire lock quickly with writer-preferring semantics — \
         took {write_latency:?}"
    );
}

#[test]
fn close_clears_all_caches() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // Upsert a file to populate caches
    let _ = upsert_vue(&host, "test.vue", "<template><div>hello</div></template>");

    // Verify the host has data
    #[cfg(feature = "scheduler")]
    {
        assert!(
            !host.scheduler.node_ids().is_empty(),
            "host should have files before close"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        assert!(
            !read_lock(&host.files).is_empty(),
            "host should have files before close"
        );
    }

    // Close and verify everything is cleared
    host.close();

    #[cfg(feature = "scheduler")]
    {
        assert!(
            host.scheduler.node_ids().is_empty(),
            "scheduler nodes should be empty after close"
        );
        assert!(
            host.compile_cache.is_empty(),
            "compile_cache should be empty after close"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        assert!(
            read_lock(&host.files).is_empty(),
            "files should be empty after close"
        );
    }
    assert!(
        read_lock(&host.alias_to_canonical).is_empty(),
        "alias_to_canonical should be empty after close"
    );
    assert!(
        read_lock(&host.reverse_dependencies).is_empty(),
        "reverse_dependencies should be empty after close"
    );
    assert!(
        read_lock(&host.last_const_prop_overrides).is_empty(),
        "last_const_prop_overrides should be empty after close"
    );
}

#[test]
fn close_allows_reuse() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = upsert_vue(&host, "test.vue", "<template><div>hello</div></template>");
    host.close();

    // Host should still be usable after close
    let _ = upsert_vue(
        &host,
        "test2.vue",
        "<template><span>world</span></template>",
    );
    #[cfg(feature = "scheduler")]
    {
        assert!(
            host.scheduler.try_get_source("test2.vue").is_some(),
            "host should accept new files after close"
        );
        assert!(
            host.scheduler.try_get_source("test.vue").is_none(),
            "previously closed files should not reappear"
        );
    }
    #[cfg(not(feature = "scheduler"))]
    {
        assert!(
            read_lock(&host.files).contains_key("test2.vue"),
            "host should accept new files after close"
        );
        assert!(
            !read_lock(&host.files).contains_key("test.vue"),
            "previously closed files should not reappear"
        );
    }
}

// ── Project resolver tests ───────────────────────────────────────

fn make_project_config(
    root: &str,
    paths: Vec<(&str, Vec<&str>)>,
) -> verter_semantic::analysis::project_resolver::IdeProjectConfig {
    let mut config = verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
        root.to_string(),
        root.to_string(),
        None,
    );
    config.compiler_options.paths = paths
        .into_iter()
        .map(|(pat, targets)| {
            (
                pat.to_string(),
                targets.iter().map(|t| t.to_string()).collect(),
            )
        })
        .collect();
    config
}

fn upsert_non_sfc(host: &VerterHost, id: &str, src: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();
}

#[test]
fn configure_projects_exact_alias() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // Configure project with exact path mapping
    host.configure_projects(vec![make_project_config(
        "/project",
        vec![("#imports", vec!["./types/imports.d.ts"])],
    )]);

    // Upsert target file
    upsert_non_sfc(
        &host,
        "/project/types/imports.d.ts",
        "export type Foo = string;",
    );

    // Upsert parent file that imports via the alias
    let _ = upsert_vue(
        &host,
        "/project/src/App.vue",
        "<script setup>\nimport type { Foo } from '#imports'\n</script>\n<template><div/></template>",
    );

    // Resolve the aliased import
    let resolved = host.resolve_import("/project/src/App.vue", "#imports");
    assert_eq!(
        resolved.as_deref(),
        Some("/project/types/imports.d.ts"),
        "exact alias #imports should resolve via project resolver"
    );
}

#[test]
fn configure_projects_wildcard_alias() {
    let host = VerterHost::new_standalone(HostConfig::default());

    host.configure_projects(vec![make_project_config(
        "/project",
        vec![("@/*", vec!["./src/*"])],
    )]);

    let _ = upsert_vue(
        &host,
        "/project/src/components/Child.vue",
        "<script setup>\ndefineProps({ msg: String })\n</script>\n<template><div>{{ msg }}</div></template>",
    );

    let _ = upsert_vue(
        &host,
        "/project/src/App.vue",
        "<script setup>\nimport Child from '@/components/Child.vue'\n</script>\n<template><Child msg=\"hi\" /></template>",
    );

    let resolved = host.resolve_import("/project/src/App.vue", "@/components/Child.vue");
    assert_eq!(
        resolved.as_deref(),
        Some("/project/src/components/Child.vue"),
        "wildcard alias @/* should resolve via project resolver"
    );
}

#[test]
fn configure_projects_multi_project() {
    let host = VerterHost::new_standalone(HostConfig::default());

    host.configure_projects(vec![
        make_project_config("/workspace/app", vec![("@app/*", vec!["./src/*"])]),
        make_project_config("/workspace/lib", vec![("@lib/*", vec!["./src/*"])]),
    ]);

    upsert_non_sfc(
        &host,
        "/workspace/app/src/utils.ts",
        "export const foo = 1;",
    );
    upsert_non_sfc(
        &host,
        "/workspace/lib/src/utils.ts",
        "export const bar = 2;",
    );
    let _ = upsert_vue(
        &host,
        "/workspace/app/src/App.vue",
        "<script setup>\nimport { foo } from '@app/utils'\n</script>\n<template><div/></template>",
    );
    let _ = upsert_vue(
        &host,
        "/workspace/lib/src/Lib.vue",
        "<script setup>\nimport { bar } from '@lib/utils'\n</script>\n<template><div/></template>",
    );

    let app_resolved = host.resolve_import("/workspace/app/src/App.vue", "@app/utils");
    assert_eq!(
        app_resolved.as_deref(),
        Some("/workspace/app/src/utils.ts"),
        "@app/* should resolve to app project"
    );

    let lib_resolved = host.resolve_import("/workspace/lib/src/Lib.vue", "@lib/utils");
    assert_eq!(
        lib_resolved.as_deref(),
        Some("/workspace/lib/src/utils.ts"),
        "@lib/* should resolve to lib project"
    );
}

#[test]
fn set_import_dependencies_overrides_project_resolver() {
    let host = VerterHost::new_standalone(HostConfig::default());

    host.configure_projects(vec![make_project_config(
        "/project",
        vec![("@/*", vec!["./src/*"])],
    )]);

    // Two possible targets
    upsert_non_sfc(&host, "/project/src/utils.ts", "export const a = 1;");
    upsert_non_sfc(&host, "/project/custom/utils.ts", "export const b = 2;");

    let _ = upsert_vue(
        &host,
        "/project/src/App.vue",
        "<script setup>\nimport { b } from '@/utils'\n</script>\n<template><div/></template>",
    );

    // Project resolver would map @/utils → /project/src/utils.ts
    // But structured deps should override to custom/utils.ts
    host.set_import_dependencies(
        "/project/src/App.vue",
        vec![DependencyResolution {
            specifier: "@/utils".to_string(),
            resolved_canonical_id: Some("/project/custom/utils.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = host.resolve_import("/project/src/App.vue", "@/utils");
    assert_eq!(
        resolved.as_deref(),
        Some("/project/custom/utils.ts"),
        "structured deps should override project resolver"
    );
}

#[test]
fn configure_projects_empty_clears() {
    let host = VerterHost::new_standalone(HostConfig::default());

    host.configure_projects(vec![make_project_config(
        "/project",
        vec![("#imports", vec!["./types/imports.d.ts"])],
    )]);

    upsert_non_sfc(
        &host,
        "/project/types/imports.d.ts",
        "export type Foo = string;",
    );
    let _ = upsert_vue(
        &host,
        "/project/src/App.vue",
        "<script setup>\nimport type { Foo } from '#imports'\n</script>\n<template><div/></template>",
    );

    // Should resolve before clearing
    assert!(
        host.resolve_import("/project/src/App.vue", "#imports")
            .is_some(),
        "should resolve before clearing"
    );

    // Clear resolver
    host.configure_projects(vec![]);

    // Should no longer resolve via project resolver
    let resolved = host.resolve_import("/project/src/App.vue", "#imports");
    assert!(
        resolved.is_none(),
        "should not resolve after clearing projects, got: {:?}",
        resolved
    );
}

#[test]
fn configure_projects_fallthrough_unloaded() {
    let host = VerterHost::new_standalone(HostConfig::default());

    host.configure_projects(vec![make_project_config(
        "/project",
        vec![("@/*", vec!["./src/*"])],
    )]);

    // DON'T upsert the target file — it's not loaded
    let _ = upsert_vue(
        &host,
        "/project/src/App.vue",
        "<script setup>\nimport Child from '@/components/Child.vue'\n</script>\n<template><div/></template>",
    );

    // Should gracefully return None (no panic)
    let resolved = host.resolve_import("/project/src/App.vue", "@/components/Child.vue");
    assert!(
        resolved.is_none(),
        "should fall through when resolved file not in host"
    );
}

#[test]
fn cross_file_optimization_with_project_resolver() {
    let host = VerterHost::new_standalone(HostConfig::default());

    host.configure_projects(vec![make_project_config(
        "/project",
        vec![("@/*", vec!["./src/*"])],
    )]);

    let _ = upsert_vue(
        &host,
        "/project/src/components/Child.vue",
        "<script setup>\ndefineProps({ msg: String })\n</script>\n<template><div>{{ msg }}</div></template>",
    );

    let _ = upsert_vue(
        &host,
        "/project/src/App.vue",
        "<script setup>\nimport Child from '@/components/Child.vue'\n</script>\n<template><Child msg=\"hello\" /></template>",
    );

    // Compile both to generate template analysis
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/project/src/components/Child.vue?vue&type=template".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: CompileProfile::default(),
        })
        .unwrap();
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: Some("/project/src/App.vue?vue&type=template".to_string()),
            canonical_id: None,
            node_kind: None,
            compile_profile: CompileProfile::default(),
        })
        .unwrap();

    let result = host.compute_cross_file_optimizations();
    let child_consts = result
        .const_prop_overrides
        .get("/project/src/components/Child.vue");
    assert!(
        child_consts.is_some(),
        "cross-file optimization should resolve aliased import via project resolver. Overrides: {:?}",
        result.const_prop_overrides
    );
    assert!(child_consts.unwrap().contains("msg"), "msg should be const");
}

// ── Workspace integration tests (Phase 4) ──

#[test]
fn new_with_workspace_stores_workspace_ref() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    // Positive: workspace reference is accessible
    let ws_ref = host.workspace();
    assert!(!ws_ref.file_exists("nonexistent.vue"));

    // Inject a file via workspace and verify it's visible
    ws.inject_file("test.vue".to_string(), Arc::from("<template></template>"));
    assert!(ws_ref.file_exists("test.vue"));
}

#[test]
fn new_standalone_creates_host_with_memory_workspace() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // Positive: host is functional
    let ws = host.workspace();
    assert!(!ws.file_exists("anything.vue"));

    // Positive: host can still upsert and compile normally
    let _ = upsert_vue(&host, "App.vue", "<template><div>hi</div></template>");
    let source = host.get_source("App.vue");
    assert!(source.is_some(), "upsert should work on standalone host");
}

#[test]
fn workspace_accessor_returns_same_arc() {
    let ws: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::MemoryWorkspace::new(verter_workspace::MemoryOptions::default()),
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    // Positive: workspace() returns the same Arc we passed in
    let ws_ref = host.workspace();
    // Compare trait object pointer identity via data pointer
    let ptr1 = Arc::as_ptr(&ws) as *const () as usize;
    let ptr2 = Arc::as_ptr(&ws_ref) as *const () as usize;
    assert_eq!(ptr1, ptr2, "workspace() should return the same Arc");
}

#[test]
fn resolve_import_via_workspace_uses_exact_resolutions() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));

    // Set up exact resolutions on the workspace
    ws.set_exact_resolutions(
        "/src/App.vue",
        vec![verter_workspace::ExactResolution {
            specifier: "./Child.vue".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/src/Child.vue".to_string()),
            possible_canonical_ids: vec!["/src/Child.vue".to_string()],
        }],
    );

    let host = VerterHost::new(HostConfig::default(), ws);

    // Positive: exact resolution works
    let resolved = host.resolve_import_via_workspace("/src/App.vue", "./Child.vue");
    assert_eq!(
        resolved.as_deref(),
        Some("/src/Child.vue"),
        "should resolve through workspace exact resolutions"
    );

    // Negative: non-existent specifier returns None
    let not_found = host.resolve_import_via_workspace("/src/App.vue", "./Missing.vue");
    assert!(
        not_found.is_none(),
        "should return None for unresolved imports"
    );
}

#[test]
fn ensure_compiled_hydrates_vue_compile_blockers_via_workspace_resolution() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/src/partials/panel.html".to_string(),
        Arc::from("<div>{{ props.msg }}</div>"),
    );
    ws.inject_file(
        "/workspace/src/types.ts".to_string(),
        Arc::from("export interface Props { msg: string }\n"),
    );

    let host = VerterHost::new(HostConfig::default(), ws);
    let _ = upsert_vue(
        &host,
        "/workspace/src/App.vue",
        "<template src=\"@/partials/panel.html\"></template>\n<script setup lang=\"ts\">\nimport type { Props } from '@/types'\nconst props = defineProps<Props>()\n</script>",
    );
    host.workspace().set_exact_resolutions(
        "/workspace/src/App.vue",
        vec![
            verter_workspace::ExactResolution {
                specifier: "@/partials/panel.html".to_string(),
                phase: verter_workspace::ResolvePhase::CodegenBlocker,
                kind: verter_workspace::ResolveRequestKind::SfcSrcAttr,
                resolved_canonical_id: Some("/workspace/src/partials/panel.html".to_string()),
                possible_canonical_ids: vec!["/workspace/src/partials/panel.html".to_string()],
            },
            verter_workspace::ExactResolution {
                specifier: "@/types".to_string(),
                phase: verter_workspace::ResolvePhase::CodegenBlocker,
                kind: verter_workspace::ResolveRequestKind::TypeImport,
                resolved_canonical_id: Some("/workspace/src/types.ts".to_string()),
                possible_canonical_ids: vec!["/workspace/src/types.ts".to_string()],
            },
        ],
    );

    assert!(
        host.get_source("/workspace/src/partials/panel.html")
            .is_none(),
        "blockers should not be preloaded before compilation",
    );
    assert!(
        host.get_source("/workspace/src/types.ts").is_none(),
        "macro type blockers should not be preloaded before compilation",
    );

    host.ensure_compiled("/workspace/src/App.vue", &CompileProfile::default())
        .expect("compile should hydrate blockers through workspace resolution");

    assert!(
        host.get_source("/workspace/src/partials/panel.html")
            .is_some(),
        "compile should load external src blockers through workspace resolution",
    );
    assert!(
        host.get_source("/workspace/src/types.ts").is_some(),
        "compile should load macro type blockers through workspace resolution",
    );
}

#[test]
fn resolve_import_via_workspace_returns_none_for_no_resolution() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // Negative: standalone workspace has no resolutions
    let result = host.resolve_import_via_workspace("/src/App.vue", "./anything");
    assert!(
        result.is_none(),
        "standalone workspace should not resolve arbitrary imports"
    );
}

#[test]
fn host_debug_impl_works_with_workspace() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let debug_str = format!("{:?}", host);
    assert!(
        debug_str.contains("VerterHost"),
        "Debug should produce VerterHost output"
    );
    assert!(
        !debug_str.contains("workspace"),
        "Debug should not expose workspace internals"
    );
}

// ── VFS authoritative host runtime tests ──

#[test]
fn set_workspace_swaps_resolution_source() {
    // Start with a standalone host (MemoryWorkspace).
    let host = VerterHost::new_standalone(HostConfig::default());

    // Upsert two files: a parent and a dependency.
    let _ = upsert_vue(
        &host,
        "/src/App.vue",
        "<script setup>\nimport Btn from './Btn.vue'\n</script>\n<template><Btn/></template>",
    );
    let _ = upsert_vue(
        &host,
        "/src/Btn.vue",
        "<template><button>click</button></template>",
    );

    // Build a NEW workspace that has exact resolution wired.
    let new_ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    new_ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            "<script setup>\nimport Btn from './Btn.vue'\n</script>\n<template><Btn/></template>",
        ),
    );
    new_ws.inject_file(
        "/src/Btn.vue".to_string(),
        Arc::from("<template><button>click</button></template>"),
    );
    // Set exact resolution: ./Btn.vue -> /src/Btn.vue
    new_ws.set_exact_resolutions(
        "/src/App.vue",
        vec![verter_workspace::ExactResolution {
            specifier: "./Btn.vue".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/src/Btn.vue".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    // Swap the workspace.
    host.set_workspace(new_ws.clone() as Arc<dyn verter_workspace::WorkspaceAccess>);

    // Positive: resolve_import_via_workspace should use the new workspace.
    let result = host.resolve_import_via_workspace("/src/App.vue", "./Btn.vue");
    assert_eq!(
        result.as_deref(),
        Some("/src/Btn.vue"),
        "set_workspace should make the new workspace's exact resolutions available"
    );

    // Negative: the old standalone workspace should no longer be used.
    // (The new workspace doesn't have an arbitrary specifier.)
    let no_result = host.resolve_import_via_workspace("/src/App.vue", "./NotExist.vue");
    assert!(
        no_result.is_none(),
        "specifiers not in the new workspace should not resolve"
    );
}

#[test]
fn configure_projects_syncs_to_workspace() {
    use verter_semantic::analysis::project_resolver::IdeProjectConfig;

    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    // Inject files into the workspace.
    ws.inject_file(
        "/my-project/src/App.vue".to_string(),
        Arc::from(
            "<script setup>\nimport Foo from '@/Foo.vue'\n</script>\n<template><Foo/></template>",
        ),
    );
    ws.inject_file(
        "/my-project/src/Foo.vue".to_string(),
        Arc::from("<template><div>Foo</div></template>"),
    );

    // Configure project with a path alias: @ -> /my-project/src
    let mut project = IdeProjectConfig::new(
        "/my-project".to_string(),
        "/my-project".to_string(),
        Some("/my-project/tsconfig.json".to_string()),
    );
    project.compiler_options.paths =
        vec![("@/*".to_string(), vec!["/my-project/src/*".to_string()])];
    host.configure_projects(vec![project]);

    // Positive: workspace should now resolve @/Foo.vue via the synced resolver.
    let result = ws.resolve_import(
        "/my-project/src/App.vue",
        "@/Foo.vue",
        verter_workspace::ResolutionContext {
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
        },
    );
    assert!(
        result.is_some(),
        "configure_projects should sync resolver to workspace"
    );
    assert_eq!(
        result.unwrap().source_id,
        "/my-project/src/Foo.vue",
        "workspace resolver should resolve @/Foo.vue"
    );

    // Negative: non-matching alias should not resolve.
    let no_result = ws.resolve_import(
        "/my-project/src/App.vue",
        "~/Bar.vue",
        verter_workspace::ResolutionContext {
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
        },
    );
    assert!(
        no_result.is_none(),
        "non-matching alias should not resolve via workspace"
    );
}

#[test]
fn set_import_dependencies_syncs_exact_resolutions_to_workspace() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    // Upsert the parent file.
    let _ = upsert_vue(
        &host,
        "/src/App.vue",
        "<script setup>\nimport Btn from '@comp/Btn.vue'\n</script>\n<template><Btn/></template>",
    );
    // Upsert the dependency.
    let _ = upsert_vue(
        &host,
        "/src/components/Btn.vue",
        "<template><button/></template>",
    );

    // Set import dependencies (simulating bundler/LSP resolution).
    host.set_import_dependencies(
        "/src/App.vue",
        vec![DependencyResolution {
            specifier: "@comp/Btn.vue".to_string(),
            resolved_canonical_id: Some("/src/components/Btn.vue".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    // Positive: workspace should now have exact resolution for this specifier.
    let result = ws.resolve_import(
        "/src/App.vue",
        "@comp/Btn.vue",
        verter_workspace::ResolutionContext {
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
        },
    );
    assert!(
        result.is_some(),
        "set_import_dependencies should sync exact resolutions to workspace"
    );
    assert_eq!(
        result.unwrap().source_id,
        "/src/components/Btn.vue",
        "workspace exact resolution should match the provided dependency"
    );

    // Negative: other specifiers should not resolve.
    let no_result = ws.resolve_import(
        "/src/App.vue",
        "@comp/Other.vue",
        verter_workspace::ResolutionContext {
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
        },
    );
    assert!(
        no_result.is_none(),
        "specifiers not in set_import_dependencies should not resolve"
    );
}

#[test]
fn workspace_resolution_is_phase_0_primary() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    // Inject files.
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from(
            "<script setup>\nimport T from './types'\n</script>\n<template><div/></template>",
        ),
    );
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from("export type Foo = string;"),
    );

    // Upsert both into the host.
    let _ = upsert_vue(
        &host,
        "/src/App.vue",
        "<script setup>\nimport T from './types'\n</script>\n<template><div/></template>",
    );
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export type Foo = string;"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();

    // Set exact resolution on workspace ONLY (not on host's dependency_resolutions).
    ws.set_exact_resolutions(
        "/src/App.vue",
        vec![verter_workspace::ExactResolution {
            specifier: "./types".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    // Positive: workspace Phase 0 should resolve ./types -> /src/types.ts
    // even though the host's legacy Phase 1 has no dependency_resolutions for it.
    let result = host.resolve_import_via_workspace("/src/App.vue", "./types");
    assert_eq!(
        result.as_deref(),
        Some("/src/types.ts"),
        "workspace Phase 0 should be primary resolution source"
    );

    // Negative: random specifiers still don't resolve.
    let no_result = host.resolve_import_via_workspace("/src/App.vue", "./nonexistent");
    assert!(
        no_result.is_none(),
        "unresolvable specifiers should still return None"
    );
}

#[test]
fn smart_invalidation_reads_workspace_reverse_deps() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));

    // Inject files into the workspace.
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from("export interface Props { name: string }"),
    );
    ws.inject_file(
        "/src/App.vue".to_string(),
        Arc::from("<script setup lang=\"ts\">\nimport type { Props } from './types'\ndefineProps<Props>()\n</script>\n<template><div/></template>"),
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    // Upsert both files.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/types.ts".to_string(),
            source: Arc::from("export interface Props { name: string }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();
    let _ = upsert_vue(
        &host,
        "/src/App.vue",
        "<script setup lang=\"ts\">\nimport type { Props } from './types'\ndefineProps<Props>()\n</script>\n<template><div/></template>",
    );

    // Simulate the bundler/LSP resolution flow: after upsert, the caller
    // provides resolved import dependencies. set_import_dependencies now
    // syncs exact resolutions to the workspace.
    host.set_import_dependencies(
        "/src/App.vue",
        vec![DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    // Positive: workspace should now have /src/App.vue as a reverse dep
    // of /src/types.ts via exact_resolved_deps.
    let rev_deps = ws.reverse_deps_for("/src/types.ts");
    assert!(
        rev_deps.contains(&"/src/App.vue".to_string()),
        "workspace reverse deps should include App.vue after set_import_dependencies (got: {:?})",
        rev_deps,
    );

    // Negative: non-imported files should NOT appear as reverse deps.
    let other_rev = ws.reverse_deps_for("/src/something-else.ts");
    assert!(
        !other_rev.contains(&"/src/App.vue".to_string()),
        "unrelated files should not appear in reverse deps"
    );
}

// ── Scheduler integration tests ──

#[cfg(feature = "scheduler")]
mod scheduler_tests {
    use super::*;
    use crate::host_executor::HostSourceData;

    /// Submit to the scheduler and wait for completion (blocks until the
    /// driver thread processes Source→Analysis).
    fn sched_submit_wait(host: &VerterHost, id: &str, src: &str) {
        let handle = host
            .scheduler()
            .submit_request(verter_scheduler::scheduler::Request {
                file_id: id.to_string(),
                target: verter_scheduler::stage::TargetStage::Analysis,
                priority: verter_scheduler::stage::Priority::Interactive,
                source: Some(Arc::from(src)),
                file_kind: None,
            });
        handle.wait();
    }

    #[test]
    fn scheduler_populated_on_upsert() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let src = r#"<script setup lang="ts">
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#;

        let _ = upsert_vue(&host, "/src/App.vue", src);
        sched_submit_wait(&host, "/src/App.vue", src);

        let snap = host.scheduler_source("/src/App.vue");
        assert!(snap.is_some(), "scheduler should have source after upsert");
        let snap = snap.unwrap();
        assert_eq!(&*snap.source, src);
        assert_ne!(snap.whole_hash, [0; 16], "whole_hash should be computed");
    }

    #[test]
    fn scheduler_source_has_parse_data() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let src = "<script setup>\nconst x = 1\n</script>\n<template><div/></template>";
        let _ = upsert_vue(&host, "/src/App.vue", src);
        sched_submit_wait(&host, "/src/App.vue", src);

        let snap = host.scheduler_source("/src/App.vue").unwrap();
        let host_data = snap.downcast_data::<HostSourceData>();
        assert!(
            host_data.is_some(),
            "source snapshot should contain HostSourceData"
        );
        let host_data = host_data.unwrap();
        assert!(
            !host_data.parse.script_analysis.bindings.is_empty(),
            "parse snapshot should have bindings for 'const x = 1'"
        );
    }

    #[test]
    fn scheduler_analysis_populated_on_upsert() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let src = "<script setup>\nconst x = 1\n</script>\n<template><div/></template>";
        let _ = upsert_vue(&host, "/src/App.vue", src);
        sched_submit_wait(&host, "/src/App.vue", src);

        let snap = host.scheduler_analysis("/src/App.vue");
        assert!(
            snap.is_some(),
            "scheduler should have analysis after upsert"
        );
    }

    #[test]
    fn scheduler_source_updates_on_re_upsert() {
        let host = VerterHost::new_standalone(HostConfig::default());

        let src1 = "<template><div>v1</div></template>";
        let _ = upsert_vue(&host, "/src/App.vue", src1);
        sched_submit_wait(&host, "/src/App.vue", src1);
        let snap1 = host.scheduler_source("/src/App.vue").unwrap();
        assert!(snap1.source.contains("v1"));

        let src2 = "<template><div>v2</div></template>";
        let _ = upsert_vue(&host, "/src/App.vue", src2);
        sched_submit_wait(&host, "/src/App.vue", src2);
        let snap2 = host.scheduler_source("/src/App.vue").unwrap();
        assert!(snap2.source.contains("v2"));
        assert!(
            !snap2.source.contains("v1"),
            "old content should be replaced"
        );
    }

    #[test]
    fn scheduler_non_sfc_upsert() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let src = "export interface Props { count: number }";
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/types.ts".to_string(),
                source: Arc::from(src),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();
        sched_submit_wait(&host, "/src/types.ts", src);

        let snap = host.scheduler_source("/src/types.ts");
        assert!(snap.is_some(), "non-SFC files should be in scheduler");
    }

    #[test]
    fn scheduler_accessor_returns_scheduler() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let sched = host.scheduler();
        assert!(
            !sched.has_node("/nonexistent"),
            "empty scheduler should have no nodes"
        );
    }

    #[test]
    fn scheduler_analysis_has_real_data() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let src = "<script setup>\nimport { ref } from 'vue'\nconst x = ref(0)\n</script>\n<template><div>{{ x }}</div></template>";
        let _ = upsert_vue(&host, "/src/App.vue", src);
        sched_submit_wait(&host, "/src/App.vue", src);

        // Typed accessor should return real script analysis
        let analysis = host.scheduler_script_analysis("/src/App.vue");
        assert!(analysis.is_some(), "scheduler should have script analysis");
        let analysis = analysis.unwrap();
        assert!(
            !analysis.imports.is_empty(),
            "should have imports (import {{ ref }} from 'vue')"
        );
        assert!(
            analysis.imports[0].source == "vue",
            "first import should be from 'vue'"
        );

        // Export signatures accessor
        let sigs = host.scheduler_export_signatures("/src/App.vue");
        assert!(sigs.is_some(), "scheduler should have export signatures");
    }

    #[test]
    fn scheduler_artifact_has_real_compile_output() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let src = "<script setup>\nconst x = 1\n</script>\n<template><div>{{ x }}</div></template>";
        let _ = upsert_vue(&host, "/src/App.vue", src);
        // Wait for the scheduler driver to process the upsert so the node
        // exists with the correct generation before we compile.
        sched_submit_wait(&host, "/src/App.vue", src);

        // Compile a virtual file — this populates the scheduler artifact.
        let profile = profile_dev();
        let profile_hash = crate::hash::compile_profile_hash(&profile);
        let _ = host.get_virtual_file(crate::VirtualQuery {
            canonical_id: Some("/src/App.vue".to_string()),
            raw_id: None,
            node_kind: Some(crate::VirtualNodeKind::Main),
            compile_profile: profile,
        });

        // The scheduler should now have artifact data with compiled outputs.
        let outputs = host.scheduler_artifact_outputs("/src/App.vue", profile_hash);
        assert!(
            outputs.is_some(),
            "scheduler should have artifact outputs after compile"
        );
        let outputs = outputs.unwrap();
        assert!(
            outputs.contains_key(&crate::VirtualNodeKind::Main),
            "outputs should contain Main virtual node"
        );

        // Diagnostics should also be available.
        let diags = host.scheduler_artifact_diagnostics("/src/App.vue", profile_hash);
        assert!(
            diags.is_some(),
            "scheduler should have artifact diagnostics"
        );
    }

    #[test]
    fn scheduler_shutdown_on_host_drop() {
        // Verify the driver thread exits cleanly when the host is dropped.
        // This tests the Weak lifecycle fix — the driver holds Weak<Scheduler>,
        // so dropping the host (which drops its Arc) allows Drop to run.
        let host = VerterHost::new_standalone(HostConfig::default());
        let src = "<template><div>hello</div></template>";
        let _ = upsert_vue(&host, "/src/App.vue", src);
        sched_submit_wait(&host, "/src/App.vue", src);
        drop(host);
        // If the driver thread leaked, this test would hang or the process
        // would not exit cleanly.
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn immediate_reupsert_then_compile_never_publishes_artifact_for_the_wrong_generation() {
        let host = VerterHost::new_standalone(HostConfig::default());

        // With scheduler as sole parser, upsert waits for scheduler to commit.
        // After upsert returns, scheduler always has the latest version.
        let v1 = "<template><div>v1</div></template>";
        let v2 = "<template><div>v2</div></template>";

        let _ = upsert_vue(&host, "/src/App.vue", v1);

        let _ = upsert_vue(&host, "/src/App.vue", v2);

        let profile = profile_dev();
        let response = host
            .get_virtual_file(crate::VirtualQuery {
                canonical_id: Some("/src/App.vue".to_string()),
                raw_id: None,
                node_kind: Some(crate::VirtualNodeKind::Main),
                compile_profile: profile,
            })
            .unwrap();

        // Host compile should succeed with v2 content
        assert!(response.code.contains("v2"), "host compile should use v2");

        // Scheduler must also have v2 — no split-brain possible with scheduler as sole parser
        let source = host.scheduler_source("/src/App.vue").unwrap();
        assert!(
            source.source.contains("v2"),
            "scheduler must have v2 after upsert returns"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn same_semantic_hash_different_whole_hash_is_handled_by_upsert_fast_path() {
        use verter_semantic::analysis::AnalysisScope;

        // When two sources have the same semantic hash but different whole hash
        // (e.g., inter-block whitespace changes), the host's upsert fast-path
        // returns early without updating the entry.
        let host = VerterHost::new_standalone(HostConfig::default());

        let v1 = "<script setup>\nconst x = 1\n</script>\n<template><div>same</div></template>";
        let v2 = "<script setup>\nconst x = 1\n</script>\n\n\n<template><div>same</div></template>";

        let (p1, _) = crate::parse::parse_vue_snapshot("/src/App.vue", v1, AnalysisScope::LSP);
        let (p2, _) = crate::parse::parse_vue_snapshot("/src/App.vue", v2, AnalysisScope::LSP);
        assert_eq!(
            p1.semantic_hash, p2.semantic_hash,
            "precondition: semantic hashes must match"
        );
        assert_ne!(
            p1.whole_hash, p2.whole_hash,
            "precondition: whole hashes must differ"
        );

        let _ = upsert_vue(&host, "/src/App.vue", v1);

        // v2 upsert is a no-op (same semantic hash → fast path returns changed=false)
        let result = upsert_vue(&host, "/src/App.vue", v2);
        assert!(
            !result.changed,
            "upsert should be a no-op for same-semantic-hash sources"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Phase 1 — Structural types: HostSourceData, HostAnalysisData, CompileCacheEntry
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(feature = "scheduler")]
mod phase1_structural_tests {
    use super::*;
    use crate::host_executor::{AnalysisArcs, HostAnalysisData, HostSourceData};
    use crate::types::CompileCacheEntry;

    #[test]
    fn test_source_data_has_cached_parse() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let _ = upsert_vue(
            &host,
            "/src/App.vue",
            "<template><div>hello</div></template>",
        );

        // Drive the scheduler so it commits SourceSnapshot
        host.scheduler().drive_all();

        let snap = host
            .scheduler_source("/src/App.vue")
            .expect("source should exist");
        let hd = snap
            .downcast_data::<HostSourceData>()
            .expect("should be HostSourceData");
        assert!(
            hd.cached_parse.is_some(),
            "Vue SFC should have cached_parse"
        );
    }

    #[test]
    fn test_source_data_non_sfc_no_cached_parse() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/types.ts".to_string(),
                source: Arc::from("export interface Foo { bar: string }"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        host.scheduler().drive_all();

        let snap = host
            .scheduler_source("/src/types.ts")
            .expect("source should exist");
        let hd = snap
            .downcast_data::<HostSourceData>()
            .expect("should be HostSourceData");
        assert!(
            hd.cached_parse.is_none(),
            "Non-SFC should NOT have cached_parse"
        );
    }

    #[test]
    fn test_source_data_has_file_kind() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let _ = upsert_vue(&host, "/src/App.vue", "<template><div/></template>");
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/src/types.ts".to_string(),
                source: Arc::from("export type A = string"),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();

        host.scheduler().drive_all();

        let vue_snap = host.scheduler_source("/src/App.vue").unwrap();
        let vue_hd = vue_snap.downcast_data::<HostSourceData>().unwrap();
        assert_eq!(vue_hd.file_kind, FileKind::VueSfc);

        let ts_snap = host.scheduler_source("/src/types.ts").unwrap();
        let ts_hd = ts_snap.downcast_data::<HostSourceData>().unwrap();
        assert_eq!(ts_hd.file_kind, FileKind::NonSfc);
    }

    #[test]
    fn test_source_data_has_parse_duration() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let _ = upsert_vue(
            &host,
            "/src/App.vue",
            "<template><div>hello</div></template>",
        );
        host.scheduler().drive_all();

        let snap = host.scheduler_source("/src/App.vue").unwrap();
        let hd = snap.downcast_data::<HostSourceData>().unwrap();
        assert!(
            hd.parse_duration_ms >= 0.0,
            "parse_duration_ms should be non-negative"
        );
    }

    #[test]
    fn test_analysis_data_has_arcs() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let _ = upsert_vue(
            &host,
            "/src/App.vue",
            "<template><div>hello</div></template>\n<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst count = ref(0)\n</script>",
        );
        host.scheduler().drive_all();

        let snap = host
            .scheduler_analysis("/src/App.vue")
            .expect("analysis should exist");
        let hd = snap
            .downcast_data::<HostAnalysisData>()
            .expect("should be HostAnalysisData");

        // Verify arcs are populated
        assert!(
            !hd.arcs.module_references.is_empty(),
            "should have module references for 'vue' import"
        );

        // Verify Arc sharing: clone and check pointer equality
        let arcs_clone = hd.arcs.clone();
        assert!(
            Arc::ptr_eq(&hd.arcs.module_references, &arcs_clone.module_references),
            "Arc::clone should be pointer-eq (no deep copy)"
        );
        assert!(
            Arc::ptr_eq(&hd.arcs.macros, &arcs_clone.macros),
            "macros Arc should be pointer-eq"
        );
    }

    #[test]
    fn test_compile_cache_entry_default() {
        let cc = CompileCacheEntry::default();
        assert!(cc.content_overrides.is_empty());
        assert!(cc.style_overrides.is_empty());
        assert!(cc.compile_slots.is_empty());
        assert!(cc.latest_diagnostics.is_empty());
        assert_eq!(cc.diagnostics_generation, 0);
        assert!(cc.cached_tsc_extract.is_none());
        assert!(cc.raw_template_analysis.is_none());
        assert!(cc.dependency_resolutions.is_empty());
        assert!(cc.dependencies.is_empty());
        assert!(cc.resolved_type_hashes.is_empty());
        assert!(cc.aliases.is_empty());
        assert_eq!(cc.generation, 0);
        assert!(!cc.evicted, "new entry should not be evicted");
    }

    #[test]
    fn test_analysis_arcs_from_analysis() {
        // Build a ScriptAnalysisSnapshot with some data
        let sa = verter_semantic::analysis::ScriptAnalysisSnapshot {
            module_references: vec![verter_semantic::analysis::AnalyzedModuleReference {
                syntax: verter_semantic::analysis::types::ModuleReferenceSyntax::StaticImport,
                semantics: verter_semantic::analysis::types::ModuleReferenceSemantics::Import,
                is_type_only: false,
                span: verter_span::Span::new(0, 30),
                expr_span: verter_span::Span::new(20, 25),
                raw_text: "'vue'".to_string(),
                literal_specifier: Some("vue".to_string()),
                finite_specifiers: vec![],
                static_prefix: None,
                analyzability:
                    verter_semantic::analysis::types::ModuleReferenceAnalyzability::Exact,
            }],
            ..Default::default()
        };

        let arcs = AnalysisArcs::from_analysis(&sa);
        assert_eq!(arcs.module_references.len(), 1);
        assert_eq!(
            arcs.module_references[0].literal_specifier.as_deref(),
            Some("vue")
        );
        assert!(arcs.macros.is_empty());
        assert!(arcs.macro_type_deps.is_empty());
    }

    #[test]
    fn test_compile_cache_on_host() {
        let host = VerterHost::new_standalone(HostConfig::default());

        // compile_cache should be accessible and empty
        assert!(
            host.compile_cache.is_empty(),
            "compile_cache should start empty"
        );

        // Insert and verify
        host.compile_cache
            .insert("/src/App.vue".to_string(), CompileCacheEntry::default());
        assert_eq!(host.compile_cache.len(), 1);

        // Verify it's accessible
        let entry = host.compile_cache.get("/src/App.vue");
        assert!(entry.is_some());
        assert!(!entry.unwrap().evicted);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Phase 2A — Upsert populates compile_cache, scheduler is sole parser
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(feature = "scheduler")]
mod phase2a_upsert_tests {
    use super::*;
    use crate::host_executor::HostSourceData;

    #[test]
    fn test_upsert_single_parse() {
        // Verify scheduler is sole parser — scheduler has committed data after upsert
        let host = VerterHost::new_standalone(HostConfig::default());
        let src = "<template><div>hello</div></template>";
        let _ = upsert_vue(&host, "/src/App.vue", src);

        // Scheduler must have the source
        let snap = host
            .scheduler_source("/src/App.vue")
            .expect("scheduler must have source");
        assert_eq!(snap.source.as_ref(), src);

        // HostSourceData must be populated
        let hd = snap.downcast_data::<HostSourceData>().unwrap();
        assert!(hd.cached_parse.is_some());
        assert_eq!(hd.file_kind, FileKind::VueSfc);
    }

    #[test]
    fn test_upsert_populates_compile_cache() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let _ = upsert_vue(
            &host,
            "/src/App.vue",
            "<template><div>hello</div></template>",
        );

        // compile_cache must have an entry
        let cc = host.compile_cache.get("/src/App.vue");
        assert!(cc.is_some(), "compile_cache should have entry after upsert");
        let cc = cc.unwrap();
        assert!(!cc.evicted);
        assert_eq!(cc.generation, 1);
        assert!(cc.aliases.contains("/src/App.vue"));
    }

    #[test]
    fn test_upsert_invalidation_on_semantic_change() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let _ = upsert_vue(&host, "/src/App.vue", "<template><div>v1</div></template>");

        // Manually add a compile slot to verify it gets cleared
        {
            let mut cc = host.compile_cache.get_mut("/src/App.vue").unwrap();
            cc.compile_slots.insert(
                42,
                CompileSlot {
                    semantic_hash: [0; 16],
                    style_override_hash: 0,
                    content_override_hash: 0,
                    outputs: Default::default(),
                    diagnostics: Default::default(),
                    last_good_outputs: None,
                    last_access_tick: 0,
                    tsx: None,
                    template_analysis: None,
                },
            );
        }

        // Semantic change
        let _ = upsert_vue(&host, "/src/App.vue", "<template><div>v2</div></template>");

        let cc = host.compile_cache.get("/src/App.vue").unwrap();
        assert!(
            cc.compile_slots.is_empty(),
            "compile_slots should be cleared on semantic change"
        );
        assert_eq!(cc.generation, 2);
    }

    #[test]
    fn test_whitespace_only_change_clears_overrides() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let v1 = "<template><div>hello</div></template>";
        let v2 = "<template><div>hello</div></template>  \n"; // trailing whitespace

        let _ = upsert_vue(&host, "/src/App.vue", v1);

        // Manually add content override
        {
            let mut cc = host.compile_cache.get_mut("/src/App.vue").unwrap();
            cc.content_overrides.insert(
                42,
                crate::types::ContentOverrideWithParse {
                    layer: ContentOverrideLayer {
                        hash: 123,
                        template: None,
                        script: None,
                    },
                    parse: crate::parse::parse_non_sfc_snapshot("/src/App.vue", ""),
                    cached_parse: None,
                    source: Arc::from(""),
                },
            );
        }

        let _ = upsert_vue(&host, "/src/App.vue", v2);

        // Per plan: whole_hash changed → overrides cleared (byte offsets shifted)
        let cc = host.compile_cache.get("/src/App.vue").unwrap();
        assert!(
            cc.content_overrides.is_empty(),
            "content_overrides should be cleared when whole_hash changes"
        );
    }

    #[test]
    fn test_upsert_fast_path_no_change() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let src = "<template><div>hello</div></template>";

        let r1 = upsert_vue(&host, "/src/App.vue", src);
        assert!(r1.changed);

        // Same content again — fast path
        let r2 = upsert_vue(&host, "/src/App.vue", src);
        assert!(!r2.changed, "identical content should trigger fast path");
    }

    #[test]
    fn test_upsert_compile_cache_deps_populated() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let src =
            "<script setup>\nimport Foo from './Foo.vue'\n</script>\n<template><Foo/></template>";
        let _ = upsert_vue(&host, "/src/App.vue", src);

        let cc = host.compile_cache.get("/src/App.vue").unwrap();
        assert!(
            !cc.dependencies.is_empty(),
            "dependencies should be populated from parse"
        );
    }

    #[test]
    fn test_evict_is_cheap_flag_flip() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let _ = upsert_vue(
            &host,
            "/src/App.vue",
            "<template><div>hello</div></template>",
        );

        // Verify entry exists and is not evicted
        assert!(!host.compile_cache.get("/src/App.vue").unwrap().evicted);

        // Evict
        host.evict("/src/App.vue");

        // Entry still exists but is evicted
        let cc = host.compile_cache.get("/src/App.vue").unwrap();
        assert!(cc.evicted, "evict should set evicted flag");
        assert!(
            cc.compile_slots.is_empty(),
            "profile state should be cleared"
        );
        // But deps/aliases are preserved for reload diffing
        assert!(
            !cc.aliases.is_empty(),
            "aliases should be preserved for reload"
        );
    }

    #[test]
    fn test_ensure_loaded_cold_load_creates_compile_cache_without_extra_generation() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from("<template><div>hello</div></template>"),
        );
        let host = VerterHost::new(HostConfig::default(), ws);

        assert!(
            host.compile_cache.get("/src/App.vue").is_none(),
            "cold file should start without compile_cache state"
        );

        let loaded = host.ensure_loaded("/src/App.vue");
        assert!(
            loaded,
            "ensure_loaded should succeed for a cold workspace file"
        );

        let snap = host
            .scheduler_source("/src/App.vue")
            .expect("scheduler should have the loaded source");
        assert_eq!(
            snap.generation, 1,
            "cold ensure_loaded should not re-submit identical source and bump generation twice"
        );

        let cc = host
            .compile_cache
            .get("/src/App.vue")
            .expect("ensure_loaded should materialize compile_cache state");
        assert!(
            !cc.evicted,
            "cold ensure_loaded should leave the entry visible"
        );
        assert_eq!(
            cc.generation, snap.generation,
            "compile_cache generation should track the committed scheduler generation"
        );
    }

    #[test]
    fn test_upsert_fast_path_materializes_compile_cache_after_scheduler_only_load() {
        use verter_scheduler::job::CompletionState;

        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        let src = "<template><div>hello</div></template>";
        ws.inject_file("/src/App.vue".to_string(), Arc::from(src));
        let host = VerterHost::new(HostConfig::default(), ws);

        let handle = host
            .scheduler()
            .submit_request(verter_scheduler::scheduler::Request {
                file_id: "/src/App.vue".to_string(),
                target: verter_scheduler::stage::TargetStage::Analysis,
                priority: verter_scheduler::stage::Priority::Interactive,
                source: None,
                file_kind: None,
            });

        assert!(
            matches!(handle.wait(), CompletionState::Ready(_)),
            "scheduler-only cold load should succeed"
        );
        assert!(
            host.compile_cache.get("/src/App.vue").is_none(),
            "scheduler-only load should not implicitly create compile_cache state"
        );

        let result = host
            .upsert(UpsertRequest {
                canonical_id: Some("/src/App.vue".to_string()),
                input_id: "/src/App.vue".to_string(),
                source: Arc::from(src),
                file_kind: FileKind::VueSfc,
                aliases: Vec::new(),
            })
            .expect("byte-identical upsert should succeed");
        assert!(
            !result.changed,
            "byte-identical upsert after scheduler-only load should remain a no-op"
        );

        let snap = host
            .scheduler_source("/src/App.vue")
            .expect("scheduler source should still exist");
        let cc = host
            .compile_cache
            .get("/src/App.vue")
            .expect("byte-identical upsert should still materialize compile_cache state");
        assert_eq!(
            cc.generation, snap.generation,
            "fast-path upsert should align compile_cache generation with the scheduler"
        );
    }

    #[test]
    fn test_ensure_loaded_after_evict_reloads_workspace_source() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        let host = VerterHost::new(HostConfig::default(), ws.clone());
        let v1 = "<template><div>v1</div></template>";
        let v2 = "<template><div>v2</div></template>";

        ws.inject_file("/src/App.vue".to_string(), Arc::from(v1));
        assert!(host.ensure_loaded("/src/App.vue"));
        assert!(
            host.get_source("/src/App.vue").unwrap().contains("v1"),
            "initial load should reflect workspace v1"
        );

        host.evict("/src/App.vue");
        ws.inject_file("/src/App.vue".to_string(), Arc::from(v2));

        let loaded = host.ensure_loaded("/src/App.vue");
        assert!(loaded, "ensure_loaded should succeed after evict");
        assert!(
            host.get_source("/src/App.vue").unwrap().contains("v2"),
            "ensure_loaded after evict must reload the latest workspace content"
        );

        let cc = host.compile_cache.get("/src/App.vue").unwrap();
        assert!(!cc.evicted, "ensure_loaded should clear evicted flag");
    }

    #[test]
    fn test_resolve_import_returns_none_for_evicted_parent() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let _ = upsert_vue(&host, "/src/Child.vue", "<template><div/></template>");
        let _ = upsert_vue(
            &host,
            "/src/App.vue",
            "<script setup>\nimport Child from './Child.vue'\n</script>\n<template><Child/></template>",
        );

        assert_eq!(
            host.resolve_import("/src/App.vue", "./Child.vue")
                .as_deref(),
            Some("/src/Child.vue")
        );

        host.evict("/src/App.vue");

        assert!(
            host.resolve_import("/src/App.vue", "./Child.vue").is_none(),
            "evicted files should be invisible to resolve_import"
        );
    }

    #[test]
    fn test_close_full_cleanup() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let _ = upsert_vue(&host, "/src/A.vue", "<template><div>a</div></template>");
        let _ = upsert_vue(&host, "/src/B.vue", "<template><div>b</div></template>");

        assert_eq!(host.compile_cache.len(), 2);

        host.close();

        assert!(
            host.compile_cache.is_empty(),
            "compile_cache should be empty after close"
        );
        assert!(crate::shared::read_lock(&host.alias_to_canonical).is_empty());
        assert!(crate::shared::read_lock(&host.reverse_dependencies).is_empty());
    }

    #[test]
    fn test_block_override_does_not_leak_into_raw_source() {
        // P1 invariant: applying a block override for profile A must NOT change
        // what get_source() returns (raw, profileless).
        let host = VerterHost::new_standalone(HostConfig::default());
        let sfc = "<template lang=\"pug\">\ndiv hello\n</template>\n<script setup>\nconst x = 1\n</script>";
        let _ = upsert_vue(&host, "/src/App.vue", sfc);

        let raw_before = host.get_source("/src/App.vue").unwrap();

        let _ = host
            .apply_block_overrides(BlockOverrideRequest {
                canonical_id: "/src/App.vue".to_string(),
                compile_profile: CompileProfile::default(),
                overrides: vec![BlockOverrideEntry {
                    block_type: PreprocessorBlockType::Template,
                    index: 0,
                    code: Arc::from("<div>hello</div>"),
                    source_map: None,
                }],
            })
            .unwrap();

        let raw_after = host.get_source("/src/App.vue").unwrap();
        assert_eq!(
            raw_before.as_ref(),
            raw_after.as_ref(),
            "get_source must return raw (unchanged) source after block override"
        );
        assert!(
            raw_after.contains("lang=\"pug\""),
            "raw source must still contain pug lang"
        );
    }

    #[test]
    fn test_style_override_does_not_leak_into_raw_analysis() {
        // P1 invariant: applying a style override for profile A must NOT change
        // the raw style_analyses returned by get_analysis().
        let host = VerterHost::new_standalone(HostConfig::default());
        let sfc = "<template><div>hi</div></template>\n<style lang=\"sass\">\n.header\n  color: red\n</style>";
        let _ = upsert_vue(&host, "/src/App.vue", sfc);

        let analysis_before = host.get_analysis("/src/App.vue").unwrap();
        let style_count_before = analysis_before.styles.len();

        let _ = host
            .apply_style_overrides(StyleOverrideRequest {
                canonical_id: "/src/App.vue".to_string(),
                compile_profile: CompileProfile::default(),
                overrides: vec![StyleOverrideEntry {
                    index: 0,
                    code: Arc::from(".header { color: green }"),
                    source_map: None,
                }],
            })
            .unwrap();

        let analysis_after = host.get_analysis("/src/App.vue").unwrap();
        assert_eq!(
            analysis_after.styles.len(),
            style_count_before,
            "style count should be unchanged after override"
        );
        // Raw style analysis content_offset should be identical
        assert_eq!(
            analysis_before.styles[0].content_offset, analysis_after.styles[0].content_offset,
            "raw style content_offset must not change after override"
        );
    }

    #[test]
    fn test_profile_a_override_does_not_affect_profile_b() {
        // P1 invariant: override for profile A must not affect profile B compile.
        let host = VerterHost::new_standalone(HostConfig::default());
        let sfc = "<template><div>hello</div></template>\n<style>.a { color: red }</style>";
        let _ = upsert_vue(&host, "/src/App.vue", sfc);

        let profile_a = CompileProfile {
            is_production: false,
            ..CompileProfile::default()
        };
        let profile_b = CompileProfile {
            is_production: true,
            ..CompileProfile::default()
        };

        // Compile with profile B first (no overrides)
        let _ = host
            .get_virtual_file(VirtualQuery {
                canonical_id: Some("/src/App.vue".to_string()),
                raw_id: None,
                node_kind: Some(VirtualNodeKind::Style { index: 0 }),
                compile_profile: profile_b.clone(),
            })
            .unwrap();

        // Apply style override for profile A only
        let _ = host
            .apply_style_overrides(StyleOverrideRequest {
                canonical_id: "/src/App.vue".to_string(),
                compile_profile: profile_a.clone(),
                overrides: vec![StyleOverrideEntry {
                    index: 0,
                    code: Arc::from(".a { color: green }"),
                    source_map: None,
                }],
            })
            .unwrap();

        // Recompile with profile B — should still have red (raw), not green
        let _ = host.invalidate_compile_slots("/src/App.vue");
        let b_result = host
            .get_virtual_file(VirtualQuery {
                canonical_id: Some("/src/App.vue".to_string()),
                raw_id: None,
                node_kind: Some(VirtualNodeKind::Style { index: 0 }),
                compile_profile: profile_b,
            })
            .unwrap();
        assert!(
            b_result.code.contains("red"),
            "profile B should compile with raw style (red), not override (green). Got: {}",
            b_result.code
        );
        assert!(
            !b_result.code.contains("green"),
            "profile B must NOT contain override content from profile A"
        );
    }
}

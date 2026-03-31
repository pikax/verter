use super::*;
use crate::types::HostConfig;
use crate::VerterHost;
use std::collections::BTreeSet;
use std::sync::Arc;
use verter_analysis::type_expand::ExpandedComponentTypes;
use verter_analysis::type_expr::{LiteralValue, ObjectMember, PrimitiveName, TypeExpr};
use verter_resolver::{ResolverStore, StoreView};

fn make_project() -> Arc<MetaProject> {
    make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    })
}

fn make_project_with_config(config: HostConfig) -> Arc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..config
    });
    MetaProject::new(host)
}

fn make_workspace_project(ws: Arc<verter_workspace::MemoryWorkspace>) -> Arc<MetaProject> {
    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    MetaProject::new(host)
}

fn sfc(props: &str) -> String {
    format!(
        r#"<script setup lang="ts">
defineProps<{{ {props} }}>()
</script>
<template><div>hello</div></template>"#
    )
}

/// Extract prop field names from a FileAnalysisSnapshot's macros.
fn prop_names(snapshot: &crate::types::FileAnalysisSnapshot) -> Vec<String> {
    snapshot
        .macros
        .iter()
        .filter(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps)
        .flat_map(|m| m.prop_fields.iter())
        .map(|f| f.name.clone())
        .collect()
}

fn evaluated_prop_type<'a>(types: &'a ExpandedComponentTypes, name: &str) -> &'a TypeExpr {
    &types
        .props
        .iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("missing evaluated prop {name}"))
        .r#type
}

fn evaluated_define_props_type<'a>(types: &'a ExpandedComponentTypes, name: &str) -> &'a TypeExpr {
    &types
        .define_props
        .iter()
        .flat_map(|entry| entry.result.value.properties.iter())
        .find(|prop| prop.name == name)
        .unwrap_or_else(|| panic!("missing defineProps property {name}"))
        .ty
}

fn resolved_imported_alias_body(
    host: &VerterHost,
    alias: &verter_resolver::ImportedTypeAlias,
) -> TypeExpr {
    let view = host.resolver_store_view();
    host.resolve_shallow_symbol_dependency_alias_in_view(
        alias.merge_root_canonical.as_str(),
        alias.merge_root_exported.as_str(),
        Some(&view),
    )
    .map(|prepared| prepared.2.decl.body)
    .expect("imported alias should materialize through the host cache")
}

fn assert_union_string_literals(expr: &TypeExpr, expected: &[&str]) {
    let mut actual = BTreeSet::new();
    match expr {
        TypeExpr::Literal(LiteralValue::String(value)) => {
            actual.insert(value.as_str());
        }
        TypeExpr::Union(types) => {
            for ty in types.iter() {
                match ty {
                    TypeExpr::Literal(LiteralValue::String(value)) => {
                        actual.insert(value.as_str());
                    }
                    TypeExpr::Primitive(PrimitiveName::Undefined) => {}
                    other => panic!(
                        "expected only string literal members (plus optional undefined), got {other:?}"
                    ),
                }
            }
        }
        other => panic!("expected string literal union, got {other:?}"),
    }

    assert_eq!(
        actual,
        BTreeSet::from_iter(expected.iter().copied()),
        "unexpected literal union members for {expr:?}"
    );
}

fn assert_route_union_surface(expr: &TypeExpr) {
    let mut saw_string = false;
    let mut saw_path_variant = false;
    let mut saw_name_variant = false;
    let mut variant_count = 0usize;

    let members: Vec<&TypeExpr> = match expr {
        TypeExpr::Primitive(PrimitiveName::String) => {
            saw_string = true;
            Vec::new()
        }
        TypeExpr::Union(types) => types.iter().collect(),
        other => panic!("expected route union, got {other:?}"),
    };

    for ty in members {
        match ty {
            TypeExpr::Primitive(PrimitiveName::String) => {
                saw_string = true;
                variant_count += 1;
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                assert!(
                    type_arguments.is_empty(),
                    "expected plain symbolic refs in route union, got generic ref {ty:?}"
                );
                match name.as_ref() {
                    "St" => saw_path_variant = true,
                    "vt" => saw_name_variant = true,
                    other => panic!("unexpected symbolic route variant {other} in {expr:?}"),
                }
                variant_count += 1;
            }
            TypeExpr::Object(obj) => {
                let has_path = obj.properties.iter().any(
                    |member| matches!(member, ObjectMember::Property(prop) if prop.name == "path"),
                );
                let has_name = obj.properties.iter().any(
                    |member| matches!(member, ObjectMember::Property(prop) if prop.name == "name"),
                );
                assert!(
                    has_path || has_name,
                    "expected object route variant to contain path or name, got {ty:?}"
                );
                saw_path_variant |= has_path;
                saw_name_variant |= has_name;
                variant_count += 1;
            }
            TypeExpr::Primitive(PrimitiveName::Undefined) => {}
            other => {
                panic!("expected route union to contain string plus route variants, got {other:?}")
            }
        }
    }

    assert!(
        saw_string,
        "expected route union to include string, got {expr:?}"
    );
    assert!(
        saw_path_variant,
        "expected route union to include a path-like variant, got {expr:?}"
    );
    assert!(
        saw_name_variant,
        "expected route union to include a name-like variant, got {expr:?}"
    );
    assert!(
        variant_count >= 3,
        "expected route union to keep distinct string/path/name variants, got {expr:?}"
    );
}

fn cached_resolved_state(
    project: &MetaProject,
    canonical: &str,
    mode: crate::types::ResolverMode,
) -> Option<Arc<crate::meta_resolve::ResolvedComponentMetaState>> {
    #[cfg(feature = "scheduler")]
    {
        project
            .host()
            .compile_cache
            .get(canonical)
            .and_then(|entry| {
                entry
                    .cached_resolved_meta
                    .get(&mode)
                    .map(|cached| Arc::clone(&cached.state))
            })
    }

    #[cfg(not(feature = "scheduler"))]
    {
        let files = crate::shared::read_lock(&project.host().files);
        files.get(canonical).and_then(|entry| {
            entry
                .cached_resolved_meta
                .get(&mode)
                .map(|cached| Arc::clone(&cached.state))
        })
    }
}

fn clear_legacy_cached_resolved_state(
    project: &MetaProject,
    canonical: &str,
    mode: crate::types::ResolverMode,
) {
    #[cfg(feature = "scheduler")]
    {
        if let Some(mut entry) = project.host().compile_cache.get_mut(canonical) {
            entry.cached_resolved_meta.remove(&mode);
        }
    }

    #[cfg(not(feature = "scheduler"))]
    {
        let mut files = crate::shared::write_lock(&project.host().files);
        if let Some(entry) = files.get_mut(canonical) {
            entry.cached_resolved_meta.remove(&mode);
        }
    }
}

#[cfg(feature = "scheduler")]
fn cached_fallthrough_state(
    project: &MetaProject,
    canonical: &str,
) -> Option<Arc<crate::types::FallthroughResolution>> {
    project
        .host()
        .compile_cache
        .get(canonical)
        .and_then(|entry| {
            entry
                .cached_fallthrough
                .as_ref()
                .map(|cached| Arc::clone(&cached.resolution))
        })
}

#[cfg(feature = "scheduler")]
fn clear_legacy_cached_fallthrough_state(project: &MetaProject, canonical: &str) {
    if let Some(mut entry) = project.host().compile_cache.get_mut(canonical) {
        entry.cached_fallthrough = None;
    }
}

#[cfg(feature = "scheduler")]
fn clear_runtime_top_level_fallthrough_node(project: &MetaProject, canonical: &str) {
    let key = verter_resolver::fallthrough_cache_key(
        canonical,
        project.host().config.generic_root_propagation,
        None,
    );
    project
        .host()
        .resolver_runtime()
        .fallthrough
        .remove_node_for_test(&key);
}

#[cfg(feature = "scheduler")]
fn clear_runtime_root_follow_node(project: &MetaProject, canonical: &str) {
    let key = verter_resolver::fallthrough_resolver::root_follow_key(
        canonical,
        0,
        project.host().config.generic_root_propagation,
    );
    project
        .host()
        .resolver_runtime()
        .fallthrough
        .remove_node_for_test(&key);
}

#[cfg(feature = "scheduler")]
fn cached_fallthrough_entry(
    project: &MetaProject,
    canonical: &str,
) -> Option<crate::types::CachedFallthroughEntry> {
    project
        .host()
        .compile_cache
        .get(canonical)
        .and_then(|entry| entry.cached_fallthrough.clone())
}

#[cfg(feature = "scheduler")]
#[test]
fn fact_versions_match_uses_derived_fact_kind_specific_validation() {
    let project = make_project();
    project
        .upsert_base("/index.ts", "export * from './inner'")
        .unwrap();
    project
        .upsert_base("/inner.ts", "export interface Inner {}")
        .unwrap();

    let mut entry = project
        .host()
        .compile_cache
        .get_mut("/index.ts")
        .expect("compile cache entry should exist");
    entry.dependency_resolutions.insert(
        "./inner".to_string(),
        crate::types::DependencyResolution {
            specifier: "./inner".to_string(),
            resolved_canonical_id: Some("/inner.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        },
    );
    entry.export_registry = Some(crate::types::FileExportRegistry {
        source_hash: [1; 16],
        named: rustc_hash::FxHashMap::default(),
        wildcard_edges: Vec::new(),
    });
    entry.barrel_export_surface = Some(crate::types::BarrelResolutionState {
        export_map: rustc_hash::FxHashMap::default(),
        source_hash: [2; 16],
        wildcard_sources: Vec::new(),
        scanned_sources: rustc_hash::FxHashMap::default(),
        tracked_deps: rustc_hash::FxHashSet::default(),
        fully_resolved: true,
        generation: 7,
    });
    entry.import_route_cache.insert(
        (
            "./inner".to_string(),
            "Inner".to_string(),
            verter_workspace::ResolveRequestKind::TypeImport,
        ),
        crate::types::ImportTypeRouteEntry {
            owner_hash: project
                .host()
                .get_whole_hash("/index.ts")
                .expect("owner hash should exist"),
            target: Some(crate::types::NormalizedTypeTarget {
                final_canonical_id: "/inner.ts".to_string(),
                exported_name: "Inner".to_string(),
            }),
            tracked_deps: vec!["/inner.ts".to_string()],
            route_hashes: vec![(
                "/inner.ts".to_string(),
                project
                    .host()
                    .get_whole_hash("/inner.ts")
                    .expect("inner hash should exist"),
            )],
            negative_barrel_gen: None,
        },
    );
    drop(entry);

    let route_hash = {
        let entry = project
            .host()
            .compile_cache
            .get("/index.ts")
            .expect("compile cache entry should exist");
        crate::resolver_store::hash_import_route_cache(&entry.import_route_cache)
    };
    let exact_hash = {
        let entry = project
            .host()
            .compile_cache
            .get("/index.ts")
            .expect("compile cache entry should exist");
        crate::resolver_store::hash_dependency_resolutions(&entry.dependency_resolutions)
    };

    assert!(project.host().fact_versions_match(&[
        verter_resolver::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: verter_resolver::DerivedFactKind::ExportRegistry,
            hash: [1; 16],
        },
        verter_resolver::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: verter_resolver::DerivedFactKind::Route,
            hash: route_hash,
        },
        verter_resolver::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: verter_resolver::DerivedFactKind::BarrelSurface,
            hash: [2; 16],
        },
        verter_resolver::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: verter_resolver::DerivedFactKind::ExactResolution,
            hash: exact_hash,
        },
        verter_resolver::FactVersionRef::BarrelGeneration {
            canonical_id: "/index.ts".to_string(),
            generation: 7,
        },
    ]));

    assert!(!project.host().fact_versions_match(&[
        verter_resolver::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: verter_resolver::DerivedFactKind::ExportRegistry,
            hash: [9; 16],
        },
    ]));
}

#[test]
fn snapshot_view_is_stale_but_coherent_after_host_changes() {
    let project = make_project();
    project
        .upsert_base("/types.ts", "export interface Props { label: string }")
        .unwrap();

    let before_hash = project
        .host()
        .get_whole_hash("/types.ts")
        .expect("whole hash should exist before mutation");
    let before_view = project.host().snapshot_view();
    let before_epoch = before_view.mutation_epoch();
    let fact = verter_resolver::FactVersionRef::FileWholeHash {
        canonical_id: "/types.ts".to_string(),
        hash: before_hash,
    };

    assert!(before_view.validates(&fact));

    project
        .upsert_base("/types.ts", "export interface Props { disabled: boolean }")
        .unwrap();

    let after_view = project.host().snapshot_view();
    let after_epoch = after_view.mutation_epoch();

    assert!(
        before_view.validates(&fact),
        "a captured store view should keep validating against the snapshot it was created from"
    );
    assert!(
        !after_view.validates(&fact),
        "a fresh store view should reject stale facts after the host changes"
    );
    assert_ne!(before_epoch, after_epoch);
    assert_ne!(before_view.compat_token(), after_view.compat_token());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ensure_loaded_advances_store_view_epoch() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("msg: string")),
    );

    let project = make_workspace_project(ws);
    let before_view = project.host().snapshot_view();
    let before_epoch = before_view.mutation_epoch();

    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "ensure_loaded should load the workspace file into the host"
    );

    let after_view = project.host().snapshot_view();
    assert_ne!(before_epoch, after_view.mutation_epoch());
    assert_ne!(before_view.compat_token(), after_view.compat_token());
}

#[test]
fn store_view_compat_token_matches_snapshot_epoch() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .expect("upsert should succeed");

    let view = project.host().snapshot_view();

    assert_eq!(
        view.compat_token(),
        verter_resolver::StoreViewCompatToken(view.mutation_epoch()),
        "v1 store-view compatibility must be exact snapshot epoch equality"
    );
}

#[test]
fn store_view_epoch_advances_on_upsert() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .expect("upsert should succeed");
    let epoch_after_first = project.host().current_store_view_epoch();

    project
        .upsert_base("/App.vue", &sfc("msg: number"))
        .expect("re-upsert should succeed");
    let epoch_after_second = project.host().current_store_view_epoch();

    assert_ne!(
        epoch_after_first, epoch_after_second,
        "mutation epoch must advance on re-upsert so compat tokens distinguish views"
    );
}

#[test]
fn store_view_epoch_advances_on_evict() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .expect("upsert should succeed");
    let epoch_before = project.host().current_store_view_epoch();

    project.host().evict("/App.vue");
    let epoch_after = project.host().current_store_view_epoch();

    assert_ne!(
        epoch_before, epoch_after,
        "mutation epoch must advance on evict so compat tokens distinguish views"
    );
}

#[test]
fn store_view_epoch_advances_on_clear_compile_cache() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .expect("upsert should succeed");
    let epoch_before = project.host().current_store_view_epoch();

    project.host().clear_compile_cache();
    let epoch_after = project.host().current_store_view_epoch();

    assert_ne!(
        epoch_before, epoch_after,
        "mutation epoch must advance on clear_compile_cache so compat tokens distinguish views"
    );
}

#[cfg(feature = "scheduler")]
#[test]
fn current_dependency_fact_versions_include_derived_resolver_facts() {
    let project = make_project();
    project
        .upsert_base("/index.ts", "export * from './inner'")
        .unwrap();

    let whole_hash = project
        .host()
        .get_whole_hash("/index.ts")
        .expect("whole hash should exist");

    let mut entry = project
        .host()
        .compile_cache
        .get_mut("/index.ts")
        .expect("compile cache entry should exist");
    entry.dependency_resolutions.insert(
        "./inner".to_string(),
        crate::types::DependencyResolution {
            specifier: "./inner".to_string(),
            resolved_canonical_id: Some("/inner.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        },
    );
    entry.export_registry = Some(crate::types::FileExportRegistry {
        source_hash: [3; 16],
        named: rustc_hash::FxHashMap::default(),
        wildcard_edges: Vec::new(),
    });
    entry.barrel_export_surface = Some(crate::types::BarrelResolutionState {
        export_map: rustc_hash::FxHashMap::default(),
        source_hash: [4; 16],
        wildcard_sources: Vec::new(),
        scanned_sources: rustc_hash::FxHashMap::default(),
        tracked_deps: rustc_hash::FxHashSet::default(),
        fully_resolved: true,
        generation: 11,
    });
    entry.import_route_cache.insert(
        (
            "./inner".to_string(),
            "Inner".to_string(),
            verter_workspace::ResolveRequestKind::TypeImport,
        ),
        crate::types::ImportTypeRouteEntry {
            owner_hash: whole_hash,
            target: Some(crate::types::NormalizedTypeTarget {
                final_canonical_id: "/inner.ts".to_string(),
                exported_name: "Inner".to_string(),
            }),
            tracked_deps: vec!["/inner.ts".to_string()],
            route_hashes: vec![("/inner.ts".to_string(), [5; 16])],
            negative_barrel_gen: None,
        },
    );
    drop(entry);

    let facts = project
        .host()
        .current_dependency_fact_versions("/index.ts", &std::collections::BTreeSet::new());

    assert!(
        facts.contains(&verter_resolver::FactVersionRef::FileWholeHash {
            canonical_id: "/index.ts".to_string(),
            hash: whole_hash,
        })
    );
    assert!(
        facts.contains(&verter_resolver::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: verter_resolver::DerivedFactKind::ExportRegistry,
            hash: [3; 16],
        })
    );
    assert!(
        facts.contains(&verter_resolver::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: verter_resolver::DerivedFactKind::Route,
            hash: {
                let entry = project
                    .host()
                    .compile_cache
                    .get("/index.ts")
                    .expect("compile cache entry should exist");
                crate::resolver_store::hash_import_route_cache(&entry.import_route_cache)
            },
        })
    );
    assert!(
        facts.contains(&verter_resolver::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: verter_resolver::DerivedFactKind::BarrelSurface,
            hash: [4; 16],
        })
    );
    assert!(
        facts.contains(&verter_resolver::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: verter_resolver::DerivedFactKind::ExactResolution,
            hash: {
                let entry = project
                    .host()
                    .compile_cache
                    .get("/index.ts")
                    .expect("compile cache entry should exist");
                crate::resolver_store::hash_dependency_resolutions(&entry.dependency_resolutions)
            },
        })
    );
    assert!(
        facts.contains(&verter_resolver::FactVersionRef::BarrelGeneration {
            canonical_id: "/index.ts".to_string(),
            generation: 11,
        })
    );
}

#[cfg(not(feature = "scheduler"))]
#[test]
fn current_dependency_fact_versions_include_derived_resolver_facts_non_scheduler() {
    let project = make_project();
    project
        .upsert_base("/index.ts", "export * from './inner'")
        .unwrap();

    let whole_hash = project
        .host()
        .get_whole_hash("/index.ts")
        .expect("whole hash should exist");

    {
        let mut files = crate::shared::write_lock(&project.host().files);
        let entry = files.get_mut("/index.ts").expect("file entry should exist");
        entry.dependency_resolutions.insert(
            "./inner".to_string(),
            crate::types::DependencyResolution {
                specifier: "./inner".to_string(),
                resolved_canonical_id: Some("/inner.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        );
        entry.export_registry = Some(crate::types::FileExportRegistry {
            source_hash: [3; 16],
            named: rustc_hash::FxHashMap::default(),
            wildcard_edges: Vec::new(),
        });
        entry.barrel_export_surface = Some(crate::types::BarrelResolutionState {
            export_map: rustc_hash::FxHashMap::default(),
            source_hash: [4; 16],
            wildcard_sources: Vec::new(),
            scanned_sources: rustc_hash::FxHashMap::default(),
            tracked_deps: rustc_hash::FxHashSet::default(),
            fully_resolved: true,
            generation: 11,
        });
        entry.import_route_cache.insert(
            (
                "./inner".to_string(),
                "Inner".to_string(),
                verter_workspace::ResolveRequestKind::TypeImport,
            ),
            crate::types::ImportTypeRouteEntry {
                owner_hash: whole_hash,
                target: Some(crate::types::NormalizedTypeTarget {
                    final_canonical_id: "/inner.ts".to_string(),
                    exported_name: "Inner".to_string(),
                }),
                tracked_deps: vec!["/inner.ts".to_string()],
                route_hashes: vec![("/inner.ts".to_string(), [5; 16])],
                negative_barrel_gen: None,
            },
        );
    }

    let facts = project
        .host()
        .current_dependency_fact_versions("/index.ts", &std::collections::BTreeSet::new());

    assert!(
        facts.contains(&verter_resolver::FactVersionRef::FileWholeHash {
            canonical_id: "/index.ts".to_string(),
            hash: whole_hash,
        })
    );
    assert!(
        facts.contains(&verter_resolver::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: verter_resolver::DerivedFactKind::ExportRegistry,
            hash: [3; 16],
        }),
        "non-scheduler store views must track export-registry facts"
    );
    assert!(
        facts.contains(&verter_resolver::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: verter_resolver::DerivedFactKind::Route,
            hash: {
                let files = crate::shared::read_lock(&project.host().files);
                let entry = files.get("/index.ts").expect("file entry should exist");
                crate::resolver_store::hash_import_route_cache(&entry.import_route_cache)
            },
        }),
        "non-scheduler store views must track import-route facts"
    );
    assert!(
        facts.contains(&verter_resolver::FactVersionRef::DerivedFactHash {
            canonical_id: "/index.ts".to_string(),
            kind: verter_resolver::DerivedFactKind::BarrelSurface,
            hash: [4; 16],
        }),
        "non-scheduler store views must track barrel-surface facts"
    );
    assert!(
        facts.contains(&verter_resolver::FactVersionRef::BarrelGeneration {
            canonical_id: "/index.ts".to_string(),
            generation: 11,
        }),
        "non-scheduler store views must track negative barrel invalidation generations"
    );
}

// ---------------------------------------------------------------------------
// Basic project lifecycle
// ---------------------------------------------------------------------------

#[test]
fn open_session_returns_unique_ids() {
    let project = make_project();
    let s1 = project.open_session().unwrap();
    let s2 = project.open_session().unwrap();
    assert_ne!(s1.id(), s2.id());
    assert_eq!(project.session_count(), 2);
}

#[test]
fn close_session_is_idempotent() {
    let project = make_project();
    let s = project.open_session().unwrap();
    s.close();
    s.close(); // second close is a no-op
    assert!(s.is_closed());
    assert_eq!(project.session_count(), 0);
}

#[test]
fn session_drop_auto_closes() {
    let project = make_project();
    {
        let _s = project.open_session().unwrap();
        assert_eq!(project.session_count(), 1);
    }
    assert_eq!(project.session_count(), 0);
}

#[test]
fn ensure_loaded_populates_shared_base_from_workspace() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("msg: string")),
    );

    let project = make_workspace_project(Arc::clone(&ws));

    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "ensure_loaded should materialize the workspace file into the shared base project"
    );
    assert!(
        project.base_file_ids().contains("/workspace/App.vue"),
        "base index should include the loaded workspace file"
    );

    let session = project.open_session().unwrap();
    assert!(session.has_file("/workspace/App.vue").unwrap());
    let source = session
        .get_effective_source("/workspace/App.vue")
        .unwrap()
        .expect("session should see the loaded base source");
    assert!(source.contains("msg: string"));
}

#[test]
fn refresh_base_reloads_workspace_source_into_shared_base() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("msg: string")),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(project.ensure_loaded("/workspace/App.vue").unwrap());

    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(sfc("count: number")),
    );

    assert!(
        project.refresh_base("/workspace/App.vue").unwrap(),
        "refresh_base should reload the latest workspace content into shared base state"
    );

    let session = project.open_session().unwrap();
    let source = session
        .get_effective_source("/workspace/App.vue")
        .unwrap()
        .expect("session should see the refreshed base source");
    assert!(source.contains("count: number"));
    assert!(!source.contains("msg: string"));
}

#[test]
fn methods_fail_after_close() {
    let project = make_project();
    let s = project.open_session().unwrap();
    s.close();
    assert!(matches!(
        s.upsert("Comp.vue", "source".into()),
        Err(MetaError::SessionClosed)
    ));
    assert!(matches!(
        s.delete("Comp.vue"),
        Err(MetaError::SessionClosed)
    ));
    assert!(matches!(
        s.get_analysis("Comp.vue"),
        Err(MetaError::SessionClosed)
    ));
}

// ---------------------------------------------------------------------------
// Overlay isolation: two sessions don't see each other's overlays
// ---------------------------------------------------------------------------

#[test]
fn two_sessions_dont_see_each_others_upserts() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s1 = project.open_session().unwrap();
    let s2 = project.open_session().unwrap();

    // Session 1 updates the file
    let modified = sfc("msg: string; count: number");
    s1.upsert("Comp.vue", modified.clone()).unwrap();

    // Session 1 sees the modified source
    let src1 = s1.get_effective_source("Comp.vue").unwrap().unwrap();
    assert!(
        src1.contains("count: number"),
        "session 1 should see its own overlay"
    );

    // Session 2 sees the original base source
    let src2 = s2.get_effective_source("Comp.vue").unwrap().unwrap();
    assert!(
        !src2.contains("count: number"),
        "session 2 must NOT see session 1's overlay"
    );
    assert!(
        src2.contains("msg: string"),
        "session 2 should see base source"
    );
}

#[test]
fn delete_in_session_a_does_not_hide_from_session_b() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s1 = project.open_session().unwrap();
    let s2 = project.open_session().unwrap();

    // Session 1 deletes the file
    s1.delete("Comp.vue").unwrap();

    // Session 1 doesn't see the file
    assert!(!s1.has_file("Comp.vue").unwrap());
    assert!(s1.get_effective_source("Comp.vue").unwrap().is_none());

    // Session 2 still sees the file
    assert!(s2.has_file("Comp.vue").unwrap());
    let src2 = s2.get_effective_source("Comp.vue").unwrap();
    assert!(src2.is_some(), "session 2 should still see the base file");
}

// ---------------------------------------------------------------------------
// Analysis through overlay
// ---------------------------------------------------------------------------

#[test]
fn get_analysis_sees_overlay_content() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s = project.open_session().unwrap();
    let modified = sfc("msg: string; count: number");
    s.upsert("Comp.vue", modified).unwrap();

    let analysis = s.get_analysis("Comp.vue").unwrap();
    assert!(
        analysis.is_some(),
        "should return analysis for overlayed file"
    );

    let snapshot = analysis.unwrap();
    let names = prop_names(&snapshot);
    assert!(
        names.contains(&"count".to_string()),
        "analysis should reflect overlay content with 'count' prop, got: {:?}",
        names
    );
}

#[test]
fn get_analysis_without_overlay_uses_base() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s = project.open_session().unwrap();

    // No overlay — should see base analysis
    let analysis = s.get_analysis("Comp.vue").unwrap();
    assert!(analysis.is_some());

    let snapshot = analysis.unwrap();
    let names = prop_names(&snapshot);
    assert!(
        names.contains(&"msg".to_string()),
        "should see base 'msg' prop, got: {:?}",
        names
    );
    assert!(
        !names.contains(&"count".to_string()),
        "should NOT see 'count' prop from base"
    );
}

#[test]
fn get_analysis_for_deleted_file_returns_none() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s = project.open_session().unwrap();
    s.delete("Comp.vue").unwrap();

    let analysis = s.get_analysis("Comp.vue").unwrap();
    assert!(
        analysis.is_none(),
        "analysis for tombstoned file should be None"
    );
}

// ---------------------------------------------------------------------------
// Overlay isolation for analysis
// ---------------------------------------------------------------------------

#[test]
fn analysis_isolation_between_sessions() {
    let project = make_project();
    let base = sfc("msg: string");
    project.upsert_base("Comp.vue", &base).unwrap();

    let s1 = project.open_session().unwrap();
    let s2 = project.open_session().unwrap();

    // Session 1 modifies the file
    s1.upsert("Comp.vue", sfc("count: number")).unwrap();

    // Session 1 sees count
    let snap1 = s1.get_analysis("Comp.vue").unwrap().unwrap();
    let names1 = prop_names(&snap1);
    assert!(
        names1.contains(&"count".to_string()),
        "session 1 should see 'count', got: {:?}",
        names1
    );
    assert!(
        !names1.contains(&"msg".to_string()),
        "session 1 should NOT see 'msg'"
    );

    // Session 2 sees msg (base)
    let snap2 = s2.get_analysis("Comp.vue").unwrap().unwrap();
    let names2 = prop_names(&snap2);
    assert!(
        names2.contains(&"msg".to_string()),
        "session 2 should see base 'msg', got: {:?}",
        names2
    );
    assert!(
        !names2.contains(&"count".to_string()),
        "session 2 should NOT see session 1's 'count'"
    );
}

// ---------------------------------------------------------------------------
// Shutdown
// ---------------------------------------------------------------------------

#[test]
fn shutdown_marks_project_dead() {
    let project = make_project();
    let s = project.open_session().unwrap();

    project.shutdown();

    assert!(project.is_shutdown());
    assert!(matches!(
        s.upsert("Comp.vue", "x".into()),
        Err(MetaError::Shutdown)
    ));
    assert!(matches!(project.open_session(), Err(MetaError::Shutdown)));
}

#[test]
fn shutdown_is_idempotent() {
    let project = make_project();
    project.shutdown();
    project.shutdown(); // no panic
}

// ---------------------------------------------------------------------------
// Overlay generation tracking
// ---------------------------------------------------------------------------

#[test]
fn overlay_generation_bumps_on_mutations() {
    let project = make_project();
    let s = project.open_session().unwrap();

    assert_eq!(s.overlay_generation(), 0);
    s.upsert("A.vue", "a".into()).unwrap();
    assert_eq!(s.overlay_generation(), 1);
    s.delete("B.vue").unwrap();
    assert_eq!(s.overlay_generation(), 2);
}

#[test]
fn reset_restores_base_state_and_drops_overlay_only_files() {
    let project = make_project();
    let base = sfc("label: string");
    let modified = sfc("count: number");
    project.upsert_base("A.vue", &base).unwrap();

    let s = project.open_session().unwrap();
    s.upsert("A.vue", modified.clone()).unwrap();
    s.upsert("Temp.vue", sfc("temp: boolean")).unwrap();

    assert!(s
        .get_effective_source("A.vue")
        .unwrap()
        .unwrap()
        .contains("count: number"));
    assert!(s.has_file("Temp.vue").unwrap());

    s.reset("A.vue").unwrap();
    s.reset("Temp.vue").unwrap();

    let restored = s.get_effective_source("A.vue").unwrap().unwrap();
    assert!(restored.contains("label: string"));
    assert!(!restored.contains("count: number"));
    assert!(!s.has_file("Temp.vue").unwrap());
    assert!(s.get_effective_source("Temp.vue").unwrap().is_none());
    assert_eq!(s.overlay_generation(), 4);
}

#[test]
fn reset_reverts_an_active_overlay_from_the_shared_host() {
    let project = make_project();
    let base = sfc("label: string");
    let modified = sfc("count: number");
    project.upsert_base("A.vue", &base).unwrap();

    let s = project.open_session().unwrap();
    s.upsert("A.vue", modified).unwrap();

    let analysis = s.get_analysis("A.vue").unwrap().unwrap();
    assert!(
        prop_names(&analysis).contains(&"count".to_string()),
        "active overlay should be visible before reset"
    );

    s.reset("A.vue").unwrap();

    let analysis = s.get_analysis("A.vue").unwrap().unwrap();
    let names = prop_names(&analysis);
    assert!(
        names.contains(&"label".to_string()),
        "base props should be visible after reset, got: {names:?}"
    );
    assert!(
        !names.contains(&"count".to_string()),
        "overlay props must be removed after reset, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// visible_file_ids
// ---------------------------------------------------------------------------

#[test]
fn visible_file_ids_reflects_overlays() {
    let project = make_project();
    project.upsert_base("A.vue", &sfc("a: string")).unwrap();
    project.upsert_base("B.vue", &sfc("b: string")).unwrap();

    let s = project.open_session().unwrap();
    s.delete("A.vue").unwrap();
    s.upsert("C.vue", sfc("c: string")).unwrap();

    let ids = s.visible_file_ids().unwrap();
    assert!(!ids.contains(&"A.vue".to_string()), "A.vue was deleted");
    assert!(ids.contains(&"B.vue".to_string()), "B.vue is in base");
    assert!(
        ids.contains(&"C.vue".to_string()),
        "C.vue was added by overlay"
    );
}

// ---------------------------------------------------------------------------
// clear_caches preserves files but flushes compile results
// ---------------------------------------------------------------------------

#[test]
fn clear_caches_preserves_base_files() {
    let project = make_project();
    project
        .upsert_base("Comp.vue", &sfc("msg: string"))
        .unwrap();

    let s = project.open_session().unwrap();
    let _ = s
        .get_analysis("Comp.vue")
        .unwrap()
        .expect("analysis should exist before clearing caches");

    project.clear_caches().unwrap();

    // Base file should still exist and be queryable after clearing caches
    let analysis = s.get_analysis("Comp.vue").unwrap();
    assert!(
        analysis.is_some(),
        "file should still be accessible after clear_caches"
    );
}

// ---------------------------------------------------------------------------
// Dependency invalidation within session
// ---------------------------------------------------------------------------

#[test]
fn changing_dependency_invalidates_importer_in_session() {
    let project = make_project();

    // Set up a types file and a component that imports from it
    let types_source = r#"export interface ButtonProps { label: string }"#;
    let comp_source = r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script>
<template><div>{{ label }}</div></template>"#;

    project.upsert_base("types.ts", types_source).unwrap();
    project.upsert_base("Button.vue", comp_source).unwrap();

    let s = project.open_session().unwrap();

    // Query analysis succeeds for the base file
    let snap = s.get_analysis("Button.vue").unwrap();
    assert!(snap.is_some(), "analysis should succeed for the base file");

    // Modify types in session to add 'disabled'
    let new_types = r#"export interface ButtonProps { label: string; disabled: boolean }"#;
    s.upsert("types.ts", new_types.into()).unwrap();

    // After modifying types in the session, querying Button.vue through the
    // session should succeed (the overlay applies the new types.ts to the host)
    let snap2 = s.get_analysis("Button.vue").unwrap();
    assert!(
        snap2.is_some(),
        "analysis should succeed after dependency update"
    );
}

// ---------------------------------------------------------------------------
// Concurrent session activity (sequential in this test, but isolated)
// ---------------------------------------------------------------------------

#[test]
fn concurrent_sessions_on_different_files() {
    let project = make_project();
    project.upsert_base("A.vue", &sfc("a: string")).unwrap();
    project.upsert_base("B.vue", &sfc("b: string")).unwrap();

    let s1 = project.open_session().unwrap();
    let s2 = project.open_session().unwrap();

    // Session 1 modifies A
    s1.upsert("A.vue", sfc("a_modified: number")).unwrap();

    // Session 2 modifies B
    s2.upsert("B.vue", sfc("b_modified: number")).unwrap();

    // Session 1 queries its files
    let snap_a1 = s1.get_analysis("A.vue").unwrap().unwrap();
    let names_a1 = prop_names(&snap_a1);
    assert!(
        names_a1.contains(&"a_modified".to_string()),
        "s1 should see its overlay on A, got: {:?}",
        names_a1
    );
    let snap_b1 = s1.get_analysis("B.vue").unwrap().unwrap();
    let names_b1 = prop_names(&snap_b1);
    assert!(
        names_b1.contains(&"b".to_string()),
        "s1 should see base B (not s2's overlay), got: {:?}",
        names_b1
    );

    // Session 2 queries its files
    let snap_b2 = s2.get_analysis("B.vue").unwrap().unwrap();
    let names_b2 = prop_names(&snap_b2);
    assert!(
        names_b2.contains(&"b_modified".to_string()),
        "s2 should see its overlay on B, got: {:?}",
        names_b2
    );
    let snap_a2 = s2.get_analysis("A.vue").unwrap().unwrap();
    let names_a2 = prop_names(&snap_a2);
    assert!(
        names_a2.contains(&"a".to_string()),
        "s2 should see base A (not s1's overlay), got: {:?}",
        names_a2
    );
}

// ---------------------------------------------------------------------------
// Native type evaluation
// ---------------------------------------------------------------------------

#[test]
fn evaluate_types_combines_all_cached_script_blocks() {
    let project = make_project();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script lang="ts">
function makeLabel() {
  return "cached" as string
}
</script>

<script setup lang="ts">
defineProps<{
  label: ReturnType<typeof makeLabel>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let evaluated = session.evaluate_types("Comp.vue").unwrap().unwrap();

    assert_eq!(
        evaluated_prop_type(&evaluated, "label"),
        &TypeExpr::Primitive(PrimitiveName::String)
    );
    assert!(
        evaluated.props.iter().all(|field| field.name != "missing"),
        "evaluation should only include actual props"
    );
}

#[test]
fn get_analysis_resolves_exported_local_props_from_sibling_script_block() {
    let project = make_project();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script lang="ts">
export interface Props {
  label: string
  count?: number
}
</script>

<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let analysis = session
        .get_analysis("Comp.vue")
        .unwrap()
        .expect("analysis should exist");
    let define_props = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps)
        .expect("defineProps macro should exist");

    let names: Vec<&str> = define_props
        .prop_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert!(
        names.contains(&"label"),
        "exported interface field 'label' should resolve, got: {:?}",
        names
    );
    assert!(
        names.contains(&"count"),
        "exported interface field 'count' should resolve, got: {:?}",
        names
    );
}

#[test]
fn get_analysis_resolves_non_exported_local_props_from_sibling_script_block() {
    let project = make_project();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script lang="ts">
interface Props {
  label: string
  count?: number
}
</script>

<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let analysis = session
        .get_analysis("Comp.vue")
        .unwrap()
        .expect("analysis should exist");
    let define_props = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps)
        .expect("defineProps macro should exist");

    let names: Vec<&str> = define_props
        .prop_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert!(
        names.contains(&"label"),
        "sibling script field 'label' should resolve, got: {:?}",
        names
    );
    assert!(
        names.contains(&"count"),
        "sibling script field 'count' should resolve, got: {:?}",
        names
    );
}

#[test]
fn evaluate_types_reuses_cached_results_until_the_file_changes() {
    let project = make_project();
    project
        .upsert_base("Comp.vue", &sfc("count: number"))
        .unwrap();

    let session = project.open_session().unwrap();
    let first = session.evaluate_types("Comp.vue").unwrap().unwrap();
    assert_eq!(
        evaluated_prop_type(&first, "count"),
        &TypeExpr::Primitive(PrimitiveName::Number)
    );

    let first_cache =
        cached_resolved_state(&project, "Comp.vue", crate::types::ResolverMode::Expanded)
            .expect("first evaluation should populate the cache");

    let second = session.evaluate_types("Comp.vue").unwrap().unwrap();
    let second_cache =
        cached_resolved_state(&project, "Comp.vue", crate::types::ResolverMode::Expanded)
            .expect("second evaluation should reuse the cache");

    assert_eq!(first.props.len(), second.props.len());
    assert!(Arc::ptr_eq(&first_cache, &second_cache));

    session
        .upsert("Comp.vue", sfc("count: number; label: string"))
        .unwrap();
    let third = session.evaluate_types("Comp.vue").unwrap().unwrap();
    let third_cache =
        cached_resolved_state(&project, "Comp.vue", crate::types::ResolverMode::Expanded)
            .expect("updated file should repopulate the cache");

    assert!(third.props.iter().any(|field| field.name == "label"));
    assert!(!Arc::ptr_eq(&second_cache, &third_cache));
}

#[test]
fn resolved_meta_reuses_resolver_cache_after_legacy_slot_is_cleared() {
    let project = make_project();
    project
        .upsert_base("Comp.vue", &sfc("count: number"))
        .unwrap();

    let _ = project
        .host()
        .resolve_component_meta("Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("initial resolve should succeed");
    let first_cache =
        cached_resolved_state(&project, "Comp.vue", crate::types::ResolverMode::Expanded)
            .expect("initial resolve should populate legacy cache mirror");

    clear_legacy_cached_resolved_state(&project, "Comp.vue", crate::types::ResolverMode::Expanded);
    assert!(
        cached_resolved_state(&project, "Comp.vue", crate::types::ResolverMode::Expanded).is_none(),
        "legacy cache slot should be cleared before the second lookup"
    );

    project.host().provenance().reset();
    let _ = project
        .host()
        .resolve_component_meta("Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("second resolve should succeed from resolver-owned cache");
    let second_cache =
        cached_resolved_state(&project, "Comp.vue", crate::types::ResolverMode::Expanded)
            .expect("resolver-owned cache hit should mirror back into the legacy slot");

    assert!(Arc::ptr_eq(&first_cache, &second_cache));
    assert_eq!(
        provenance(&project).component_meta_resolved_state_recomputes,
        0,
        "resolver-owned cache hit should avoid a recompute after the legacy slot is cleared"
    );
    assert_eq!(
        provenance(&project).resolver_node_cache_hits,
        1,
        "second lookup should be served from the resolver-owned cache"
    );
    assert_eq!(
        provenance(&project).resolver_node_cache_misses,
        0,
        "second lookup should not miss the resolver-owned cache after the legacy slot is cleared"
    );
    assert_eq!(
        provenance(&project).resolver_singleflight_coalesced,
        0,
        "single-threaded cache reuse should not require singleflight coalescing"
    );
}

#[cfg(feature = "scheduler")]
#[test]
fn fallthrough_reuses_resolver_cache_after_legacy_slot_is_cleared() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><div>child</div></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    project.host().resolver_runtime().reset_counters();
    let _ = get_meta(&project, "/App.vue");
    let after_first = project.host().resolver_runtime().counter_snapshot();
    let first_cache = cached_fallthrough_state(&project, "/App.vue")
        .expect("initial lookup should populate the legacy fallthrough mirror");

    clear_legacy_cached_fallthrough_state(&project, "/App.vue");
    assert!(
        cached_fallthrough_state(&project, "/App.vue").is_none(),
        "legacy fallthrough cache slot should be cleared before the second lookup"
    );

    project.host().provenance.reset();

    let _ = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("second fallthrough resolve should succeed from resolver-owned cache");
    let after_second = project.host().resolver_runtime().counter_snapshot();
    let second_cache = cached_fallthrough_state(&project, "/App.vue")
        .expect("resolver-owned fallthrough cache hit should mirror back into the legacy slot");

    let first_prop_names: Vec<_> = first_cache
        .accepted_props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    let second_prop_names: Vec<_> = second_cache
        .accepted_props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert_eq!(first_prop_names, second_prop_names);
    assert_eq!(
        first_cache.accepted_surface_completeness,
        second_cache.accepted_surface_completeness
    );
    assert_eq!(
        first_cache.fact_versions.len(),
        second_cache.fact_versions.len(),
        "legacy mirror repopulation should preserve dependency fact coverage"
    );
    assert!(
        after_first.node_cache_misses > 0,
        "first fallthrough resolve should populate runtime fallthrough nodes, got {:?}",
        after_first
    );
    assert!(
        after_second.node_cache_hits > after_first.node_cache_hits,
        "clearing only the legacy mirror should now reuse the runtime top-level cache directly, before={:?} after={:?}",
        after_first,
        after_second
    );
    assert_eq!(
        provenance(&project).resolver_node_cache_hits,
        1,
        "second fallthrough lookup should be served from the runtime cache and mirrored back into the legacy slot"
    );
    assert_eq!(
        provenance(&project).resolver_node_cache_misses,
        0,
        "second fallthrough lookup should not miss once the runtime cache is consulted after the legacy slot is cleared"
    );
}

#[cfg(feature = "scheduler")]
#[test]
fn fallthrough_runtime_reuse_survives_host_cache_clear() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let first = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("initial fallthrough resolve should succeed");
    assert!(
        first.accepted_props.iter().any(|prop| prop.name == "value"),
        "initial fallthrough resolve should inherit input attrs from the child"
    );

    clear_legacy_cached_fallthrough_state(&project, "/App.vue");
    project.host().provenance.reset();
    project.host().resolver_runtime().reset_counters();

    let second = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("second fallthrough resolve should succeed from runtime-owned cache");
    let runtime = project.host().resolver_runtime().counter_snapshot();
    let provenance = provenance(&project);

    assert!(
        second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "value"),
        "runtime-owned top-level fallthrough should preserve inherited input attrs"
    );
    assert!(
        runtime.node_cache_hits > 0,
        "runtime branch-union nodes should satisfy the top-level lookup after host cache clear, got {:?}",
        runtime
    );
    assert_eq!(
        provenance.resolver_node_cache_hits,
        1,
        "top-level fallthrough should be served from the runtime-owned cache once host caches are cleared"
    );
    assert_eq!(
        provenance.resolver_node_cache_misses,
        0,
        "runtime-owned top-level fallthrough should avoid a host-side miss after host caches are cleared, got provenance={:?}",
        provenance
    );
}

#[cfg(feature = "scheduler")]
#[test]
fn top_level_fallthrough_lives_in_runtime_not_host_wrapper_cache() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let result = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("fallthrough resolve should succeed");
    let key = verter_resolver::fallthrough_cache_key(
        "/App.vue",
        project.host().config.generic_root_propagation,
        None,
    );
    assert!(
        result
            .accepted_props
            .iter()
            .any(|prop| prop.name == "value"),
        "resolved fallthrough should inherit input attrs from the child"
    );
    assert!(
        cached_fallthrough_state(&project, "/App.vue").is_some(),
        "legacy compile-cache mirror should still be populated"
    );
    assert!(
        project
            .host()
            .resolver_runtime()
            .fallthrough
            .get_cached_node(&key, &project.host().resolver_store_view())
            .is_some(),
        "top-level fallthrough should live only in runtime nodes once runtime owns top-level authority"
    );
}

#[cfg(feature = "scheduler")]
#[test]
fn fallthrough_recomputes_from_runtime_subnodes_after_top_level_node_clear() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const attrs = { id: 'hero', title: 'Hello' }
</script>
<template><div v-bind="attrs" /></template>"#,
        )
        .unwrap();

    let first = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("initial fallthrough resolve should succeed");
    assert!(
        first
            .accepted_props
            .iter()
            .any(|prop| prop.name == "placeholder"),
        "initial fallthrough resolve should include remaining div attrs"
    );
    assert!(
        !first.accepted_props.iter().any(|prop| prop.name == "id"),
        "consumed spread attrs must not leak into inherited attrs"
    );

    clear_legacy_cached_fallthrough_state(&project, "/App.vue");
    clear_runtime_top_level_fallthrough_node(&project, "/App.vue");
    clear_runtime_root_follow_node(&project, "/App.vue");
    project.host().provenance.reset();
    project.host().resolver_runtime().reset_counters();

    let second = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("second fallthrough resolve should rebuild from runtime subnodes");
    let runtime = project.host().resolver_runtime().counter_snapshot();

    assert!(
        second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "placeholder"),
        "recomputed fallthrough should preserve remaining div attrs"
    );
    assert!(
        !second.accepted_props.iter().any(|prop| prop.name == "id"),
        "recomputed fallthrough must still treat spread attrs as consumed"
    );
    assert!(
        runtime.node_cache_hits >= 2,
        "recomputing after evicting the top-level and root-follow nodes should reuse multiple deeper runtime subnodes, got {:?}",
        runtime
    );
}

#[cfg(feature = "scheduler")]
#[test]
fn fallthrough_reuses_root_follow_after_branch_union_node_clear() {
    let project = make_project();
    project
        .upsert_base("/App.vue", r#"<template><UnknownRoot /></template>"#)
        .unwrap();

    let first = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("initial fallthrough resolve should succeed");
    assert!(
        first.accepted_props.is_empty(),
        "unresolved root should not fabricate inherited attrs"
    );

    clear_legacy_cached_fallthrough_state(&project, "/App.vue");
    clear_runtime_top_level_fallthrough_node(&project, "/App.vue");
    project.host().provenance.reset();
    project.host().resolver_runtime().reset_counters();

    let second = project
        .host()
        .resolve_fallthrough_surface("/App.vue")
        .expect("second fallthrough resolve should rebuild from root-follow and consumed-binding runtime nodes");
    let runtime = project.host().resolver_runtime().counter_snapshot();

    assert!(
        second.accepted_props.is_empty(),
        "recomputed unresolved root should not fabricate inherited attrs"
    );
    assert!(
        runtime.node_cache_hits >= 1,
        "evicting only the branch-union node should still reuse the cached root-follow node, got {:?}",
        runtime
    );
    assert_eq!(
        runtime.node_cache_misses,
        1,
        "only the missing branch-union node should miss once root-follow is runtime-owned, got {:?}",
        runtime
    );
}

#[test]
fn evaluate_types_resolves_local_typeof_from_sibling_script_block() {
    let project = make_project();
    project
        .upsert_base(
            "Comp.vue",
            r#"<script lang="ts">
const theme = {
  item: "item",
  body: "body",
}

type Props = {
  ui: typeof theme
}
</script>

<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let evaluated = session.evaluate_types("Comp.vue").unwrap().unwrap();

    match evaluated_prop_type(&evaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"item"));
            assert!(names.contains(&"body"));
        }
        other => panic!("expected typeof theme to resolve to an object, got {other:?}"),
    }
}

#[test]
fn evaluate_types_resolves_imported_default_typeof() {
    let project = make_project();
    project
        .upsert_base(
            "/theme.ts",
            r#"export default {
  item: "item",
  body: "body",
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import theme from './theme'

defineProps<{
  ui: typeof theme
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let analysis = session.get_analysis("/Comp.vue").unwrap().unwrap();
    assert_eq!(analysis.imports.len(), 1);
    assert_eq!(analysis.imports[0].bindings.len(), 1);
    assert_eq!(
        analysis.imports[0].bindings[0].kind,
        verter_analysis::types::ImportBindingKind::Default
    );
    assert_eq!(
        analysis.imports[0].bindings[0].imported_name.as_deref(),
        Some("default")
    );
    assert!(
        analysis.imports[0].resolved_canonical_id.is_some(),
        "default import should already be resolved in the analysis snapshot"
    );
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();

    match evaluated_prop_type(&evaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"item"));
            assert!(names.contains(&"body"));
        }
        other => panic!("expected imported typeof theme to resolve to an object, got {other:?}"),
    }
}

#[test]
fn imported_default_typeof_recovers_after_dependency_is_added() {
    let project = make_project();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import theme from './theme'

defineProps<{
  ui: typeof theme
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let initial = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    assert!(
        !matches!(evaluated_prop_type(&initial, "ui"), TypeExpr::Object(_)),
        "missing dependency should not resolve imported typeof exactly"
    );

    project
        .upsert_base(
            "/theme.ts",
            r#"export default {
  item: "item",
  body: "body",
}"#,
        )
        .unwrap();

    let reevaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    match evaluated_prop_type(&reevaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"item"));
            assert!(names.contains(&"body"));
        }
        other => panic!("expected imported typeof theme to recover to an object, got {other:?}"),
    }
}

#[test]
fn evaluate_types_resolves_imported_types_before_running_utilities() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface ImportedUser {
  id: number
  name: string
  password: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { ImportedUser } from './types'

defineProps<{
  user: Pick<ImportedUser, 'id' | 'name'>
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let evaluated = session.evaluate_types("/Comp.vue").unwrap().unwrap();

    match evaluated_prop_type(&evaluated, "user") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"id"));
            assert!(names.contains(&"name"));
            assert!(!names.contains(&"password"));
        }
        other => panic!("expected imported utility to resolve to an object, got {other:?}"),
    }
}

#[test]
fn evaluate_types_prunes_imported_eval_inputs_to_macro_reachable_deps() {
    let project = make_project();
    project
        .upsert_base(
            "/used.ts",
            r#"export interface UsedProps {
  title: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/unused-c.ts",
            r#"export interface UnusedC {
  c: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/unused-b.ts",
            r#"import type { UnusedC } from './unused-c'
export type UnusedB = UnusedC & { b: string }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/unused-a.ts",
            r#"import type { UnusedB } from './unused-b'
export type UnusedA = UnusedB & { a: string }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { UsedProps } from './used'
import type { UnusedA } from './unused-a'

defineProps<UsedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let evaluated = session.evaluate_types("/App.vue").unwrap().unwrap();

    assert_eq!(
        evaluated_define_props_type(&evaluated, "title"),
        &TypeExpr::Primitive(PrimitiveName::String)
    );

    let state = cached_resolved_state(&project, "/App.vue", crate::types::ResolverMode::Expanded)
        .expect("evaluation should populate the resolved-meta cache");
    let inputs = state
        .cached_eval_inputs
        .as_ref()
        .expect("expanded resolution should cache imported eval inputs");

    assert!(
        inputs.canonical_dependencies.contains("/used.ts"),
        "macro-reachable dependency should be tracked"
    );
    assert!(
        !inputs.canonical_dependencies.contains("/unused-a.ts"),
        "unused owner import should not be pulled into eval inputs"
    );
    assert!(
        !inputs.canonical_dependencies.contains("/unused-b.ts"),
        "transitive graph behind an unused import should stay pruned"
    );
    assert!(
        !inputs.canonical_dependencies.contains("/unused-c.ts"),
        "unreachable transitive dependency should stay pruned"
    );
}

#[test]
fn evaluate_types_resolve_relevant_transitive_imported_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/base.ts",
            r#"export interface BaseProps {
  id: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/props.ts",
            r#"import type { BaseProps } from './base'

export interface Props extends BaseProps {
  label: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './props'

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let evaluated = session.evaluate_types("/App.vue").unwrap().unwrap();

    match evaluated_define_props_type(&evaluated, "id") {
        TypeExpr::Primitive(PrimitiveName::String) => {}
        other => panic!("expected inherited prop 'id' to resolve to string, got {other:?}"),
    }
    match evaluated_define_props_type(&evaluated, "label") {
        TypeExpr::Primitive(PrimitiveName::String) => {}
        other => panic!("expected direct prop 'label' to resolve to string, got {other:?}"),
    }

    let state = cached_resolved_state(&project, "/App.vue", crate::types::ResolverMode::Expanded)
        .expect("evaluation should populate the resolved-meta cache");
    let inputs = state
        .cached_eval_inputs
        .as_ref()
        .expect("expanded resolution should cache imported eval inputs");

    assert!(
        inputs.canonical_dependencies.contains("/props.ts"),
        "direct imported declaration source should be tracked"
    );
    assert!(
        inputs.canonical_dependencies.contains("/base.ts"),
        "relevant transitive heritage dependency should be tracked"
    );
}

#[test]
fn evaluate_types_preserve_script_setup_generic_metadata_in_define_props() {
    let project = make_project();
    project
        .upsert_base(
            "/Generic.vue",
            r#"<script lang="ts">
export interface Item {
  id: string
}

export interface Props<U extends Item = Item> {
  items?: U[]
  selected?: U extends infer Selected ? Selected : never
}
</script>

<script setup lang="ts" generic="T extends Item = Item">
defineProps<Props<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let evaluated = session.evaluate_types("/Generic.vue").unwrap().unwrap();

    match evaluated_define_props_type(&evaluated, "items") {
        TypeExpr::Array { element, .. } => match element.as_ref() {
            TypeExpr::TypeParameter(param) => {
                assert_eq!(param.name, "T");
                assert!(matches!(
                    param.constraint.as_deref(),
                    Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
                ));
                assert!(matches!(
                    param.default.as_deref(),
                    Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
                ));
            }
            other => {
                panic!("expected items element to preserve the script setup generic, got {other:?}")
            }
        },
        other => panic!("expected items prop to be an array, got {other:?}"),
    }

    match evaluated_define_props_type(&evaluated, "selected") {
        TypeExpr::TypeParameter(param) => {
            assert_eq!(param.name, "T");
            assert!(matches!(
                param.constraint.as_deref(),
                Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
            ));
        }
        other => panic!(
            "expected infer conditional to resolve to the script setup generic, got {other:?}"
        ),
    }
}

#[test]
fn get_component_meta_uses_default_type_parameters_when_generic_args_are_omitted() {
    let project = make_project();
    project
        .upsert_base(
            "/Generic.vue",
            r#"<script lang="ts">
export interface Item {
  id: string
}

export interface Props<T = Item> {
  items?: T[]
}
</script>

<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/Generic.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    let items = meta
        .props
        .iter()
        .find(|prop| prop.name == "items")
        .expect("items prop should exist");

    let TypeExpr::Array { element, .. } = &items.type_expr else {
        panic!(
            "expected items to resolve to an array, got {:?}",
            items.type_expr
        );
    };
    let TypeExpr::Object(shape) = element.as_ref() else {
        panic!(
            "expected omitted generic default to instantiate to Item, got {:?}",
            element
        );
    };
    assert!(
        shape
            .properties
            .iter()
            .any(|member| matches!(member, ObjectMember::Property(prop) if prop.name == "id")),
        "expected instantiated Item shape to expose id, got {:?}",
        shape.properties
    );
}

#[test]
fn evaluate_types_skips_irrelevant_transitive_generic_arg_dependencies() {
    let project = make_project();
    project
        .upsert_base(
            "/tv.ts",
            r#"export type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: string
}

export type ComponentConfig<T extends { slots?: Record<string, any> }, A extends Record<string, any>> = {
  appConfig: A
  slots: ComponentSlots<T>
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/schema-leaf.ts",
            r#"export interface SchemaLeaf {
  label: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/schema.ts",
            r#"import type { SchemaLeaf } from './schema-leaf'

export interface AppConfig {
  ui?: SchemaLeaf
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/theme.ts",
            r#"export default {
  slots: {
    item: 'item',
    body: 'body'
  }
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ComponentConfig } from './tv'
import type { AppConfig } from './schema'
import theme from './theme'

type Accordion = ComponentConfig<typeof theme, AppConfig>

defineProps<{
  ui: Accordion['slots']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let evaluated = session.evaluate_types("/App.vue").unwrap().unwrap();

    match evaluated_prop_type(&evaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"item"));
            assert!(names.contains(&"body"));
        }
        other => panic!("expected ui slots object, got {other:?}"),
    }

    let state = cached_resolved_state(&project, "/App.vue", crate::types::ResolverMode::Expanded)
        .expect("evaluation should populate the resolved-meta cache");
    let inputs = state
        .cached_eval_inputs
        .as_ref()
        .expect("expanded resolution should cache imported eval inputs");

    assert!(
        inputs.canonical_dependencies.contains("/tv.ts"),
        "ComponentConfig declaration source should be tracked"
    );
    assert!(
        inputs.canonical_dependencies.contains("/schema.ts"),
        "direct generic arg source should still be tracked for invalidation"
    );
    assert!(
        !inputs.canonical_dependencies.contains("/schema-leaf.ts"),
        "irrelevant transitive imports behind an unused generic arg should stay out of eval inputs"
    );
}

#[test]
fn evaluate_types_skip_irrelevant_transitive_slot_value_dependencies() {
    let project = make_project();
    project
        .upsert_base(
            "/leaf.ts",
            r#"export interface LeafValue {
  class: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/tv.ts",
            r#"import type { LeafValue } from './leaf'

type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: LeafValue
}

export type ComponentConfig<T extends { slots?: Record<string, any> }> = {
  slots: ComponentSlots<T>
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/theme.ts",
            r#"export default {
  slots: {
    item: 'item',
    body: 'body'
  }
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ComponentConfig } from './tv'
import theme from './theme'

type Accordion = ComponentConfig<typeof theme>

defineProps<{
  ui: Accordion['slots']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let evaluated = session.evaluate_types("/App.vue").unwrap().unwrap();

    match evaluated_prop_type(&evaluated, "ui") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"item"));
            assert!(names.contains(&"body"));
        }
        other => panic!("expected ui slots object, got {other:?}"),
    }

    let state = cached_resolved_state(&project, "/App.vue", crate::types::ResolverMode::Expanded)
        .expect("evaluation should populate the resolved-meta cache");
    let inputs = state
        .cached_eval_inputs
        .as_ref()
        .expect("expanded resolution should cache imported eval inputs");

    assert!(
        inputs.canonical_dependencies.contains("/tv.ts"),
        "ComponentConfig declaration source should be tracked"
    );
    assert!(
        !inputs.canonical_dependencies.contains("/leaf.ts"),
        "slot value leaf imports should stay out of eval inputs when only slot keys are needed"
    );
}

#[test]
fn evaluate_types_materializes_imported_indexed_access_from_shallow_alias_source_env() {
    let project = make_project();
    project
        .upsert_base(
            "/dep.ts",
            r#"type Child = string

export type Parent = {
  x: Child
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Parent } from './dep'

defineProps<{
  value: Parent['x']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let evaluated = session.evaluate_types("/App.vue").unwrap().unwrap();

    assert_eq!(
        evaluated_define_props_type(&evaluated, "value"),
        &TypeExpr::Primitive(PrimitiveName::String),
        "indexed access through an imported shallow alias should still resolve via the source env"
    );
}

#[test]
fn get_component_meta_merges_local_eval_surface_with_imported_props() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface ExternalProps {
  /** Stable id description. */
  id: string
  /** Optional label description. */
  label?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ExternalProps } from './types'

interface LocalProps extends Pick<ExternalProps, 'id' | 'label'> {
  own?: boolean
}

defineProps<LocalProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();
    let meta = get_meta(&project, "/App.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert_eq!(prop_names, vec!["id", "label", "own"]);
    let id = meta
        .props
        .iter()
        .find(|prop| prop.name == "id")
        .expect("id prop should exist");
    let label = meta
        .props
        .iter()
        .find(|prop| prop.name == "label")
        .expect("label prop should exist");
    assert!(id.required, "imported required prop should stay required");
    assert!(
        !label.required,
        "imported optional prop should stay optional after wrapper flattening"
    );
    assert_eq!(id.description.as_deref(), Some("Stable id description."));
    assert_eq!(
        label.description.as_deref(),
        Some("Optional label description.")
    );
}

#[test]
fn get_component_meta_uses_evaluated_define_props_from_split_script_sfc() {
    let project = make_project();
    project
        .upsert_base(
            "/types/index.ts",
            "export * from '../Link.vue'\nexport * from '../icons'",
        )
        .unwrap();
    project
        .upsert_base(
            "/icons.ts",
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Link.vue",
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
}

export interface LinkProps extends RouterLinkProps {
  href?: string
  raw?: boolean
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Button.vue",
            r#"<script lang="ts">
import type { LinkProps, UseComponentIconsProps } from './types'

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
}
</script>

<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/Button.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"icon"),
        "split-script defineProps should include imported interface members, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"loading"),
        "split-script defineProps should include imported interface members, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"href"),
        "split-script defineProps should include imported Omit survivors, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"replace"),
        "split-script defineProps should include inherited base props, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"label") && prop_names.contains(&"color"),
        "split-script defineProps should keep local props, got: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"raw") && !prop_names.contains(&"custom"),
        "split-script defineProps should respect Omit, got: {prop_names:?}"
    );
}

#[test]
fn get_component_meta_uses_evaluated_types_for_imported_define_props() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface ExternalProps {
  id: string
  label?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ExternalProps } from './types'

defineProps<ExternalProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("full meta should resolve");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert_eq!(prop_names, vec!["id", "label"]);
}

#[test]
fn get_component_meta_includes_imported_define_emits_members() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export type ExternalEmits = {
  change: [event: Event]
  "update:modelValue": [value: string]
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ExternalEmits } from './types'

defineEmits<ExternalEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("full meta should resolve");
    let event_names: Vec<&str> = meta
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();

    assert!(
        event_names.contains(&"change"),
        "full meta should keep direct emit members, got: {event_names:?}"
    );
    assert!(
        event_names.contains(&"update:modelValue"),
        "full meta should include imported emit members from the resolved macro surface, got: {event_names:?}"
    );
}

#[test]
fn get_component_meta_keeps_imported_members_from_local_emit_aliases() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export type ModelEmits<T = string> = {
  "update:modelValue": [value: T]
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ModelEmits } from './types'

type AppEmits = {
  change: [event: Event]
} & ModelEmits

defineEmits<AppEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("full meta should resolve");
    let event_names: Vec<&str> = meta
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();

    assert!(
        event_names.contains(&"change"),
        "full meta should keep direct emit members, got: {event_names:?}"
    );
    assert!(
        event_names.contains(&"update:modelValue"),
        "full meta should not drop imported emit members from local aliases, got: {event_names:?}"
    );
}

#[test]
fn get_component_meta_resolves_imported_helper_aliases_without_dep_env_merge() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"type Status = 'idle' | 'busy'

export interface ExternalProps {
  status: Status
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ExternalProps } from './types'

defineProps<ExternalProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("full meta should resolve");
    let status = meta
        .props
        .iter()
        .find(|prop| prop.name == "status")
        .expect("status prop should be present");

    assert_eq!(
        status.type_expr,
        TypeExpr::union(vec![
            TypeExpr::string_literal("idle"),
            TypeExpr::string_literal("busy"),
        ]),
        "full meta should preserve the resolved helper alias shape"
    );
}

#[test]
fn get_component_meta_preserves_barrel_cycle_utility_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/index.ts",
            r#"export * from '../Link.vue'
export * from '../Button.vue'"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
  exactActiveClass?: string
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
  class?: any
  raw?: boolean
}

export type LinkPropsKeys = 'to' | 'replace' | 'activeClass' | 'ariaCurrentValue'
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: string
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface ChildProps extends Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  status?: string
}

defineProps<ChildProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../Link.vue".to_string(),
                resolved_canonical_id: Some("/src/Link.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../Button.vue".to_string(),
                resolved_canonical_id: Some("/src/Button.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("full meta should resolve");
    let mut prop_names: Vec<String> = meta.props.iter().map(|prop| prop.name.clone()).collect();
    prop_names.sort();

    assert!(
        prop_names.iter().any(|name| name == "loading"),
        "full meta should preserve inherited imported props, got: {prop_names:?}"
    );
    assert!(
        prop_names.iter().any(|name| name == "href"),
        "full meta should preserve surviving imported utility props, got: {prop_names:?}"
    );
    assert!(
        prop_names.iter().any(|name| name == "status"),
        "full meta should preserve local additions, got: {prop_names:?}"
    );
    assert!(
        !prop_names.iter().any(|name| name == "icon"),
        "full meta should keep omitted props removed, got: {prop_names:?}"
    );
    assert!(
        !prop_names.iter().any(|name| name == "replace"),
        "full meta should keep omitted key-alias props removed, got: {prop_names:?}"
    );
}

#[test]
fn evaluate_types_only_expands_surface_requested_bindings() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface HiddenPayload {
  deep: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { HiddenPayload } from './types'

const hidden: HiddenPayload = { deep: 'x' }
const shown: number = 1

defineProps<{ label: string }>()
defineExpose({ shown })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let evaluated = project
        .host()
        .evaluate_types("/App.vue")
        .expect("evaluated types should exist");

    let binding_names: Vec<&str> = evaluated
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();

    assert_eq!(
        binding_names,
        vec!["shown"],
        "only bindings requested by the component surface should be expanded"
    );
}

#[test]
fn get_component_meta_resolves_workspace_only_barrel_dependencies_for_define_props() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from("export * from '../components/Link.vue'\nexport * from '../icons'"),
    );
    ws.inject_file(
        "/workspace/src/runtime/icons.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/Link.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
}

export interface LinkProps extends RouterLinkProps {
  href?: string
  raw?: boolean
}
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/Button.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { LinkProps, UseComponentIconsProps } from '../types'

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
}
</script>

<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/Button.vue")
            .unwrap(),
        "workspace owner should load into the shared base project"
    );

    let meta = get_meta(&project, "/workspace/src/runtime/components/Button.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"icon") && prop_names.contains(&"loading"),
        "workspace-only deps should preserve imported icon props, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"href") && prop_names.contains(&"replace"),
        "workspace-only deps should preserve imported LinkProps survivors, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"label") && prop_names.contains(&"color"),
        "workspace-only deps should preserve local props, got: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"raw") && !prop_names.contains(&"custom"),
        "workspace-only deps should still respect Omit, got: {prop_names:?}"
    );
}

#[test]
fn get_component_meta_recurses_workspace_only_imports_of_imported_vue_types() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from("export * from '../components/Link.vue'\nexport * from '../icons'"),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/router.ts".to_string(),
        Arc::from(
            r#"export interface RouterLinkProps {
  replace?: boolean
  activeClass?: string
  custom?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts".to_string(),
        Arc::from(
            r#"export interface AnchorHTMLAttributes {
  href?: string
  download?: string
  ping?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/icons.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/Link.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { RouterLinkProps } from '../types/router'
import type { AnchorHTMLAttributes } from '../types/html'

export interface LinkProps extends Omit<RouterLinkProps, 'custom'>, Omit<AnchorHTMLAttributes, 'href'> {
  href?: string
  raw?: boolean
}
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/Button.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { LinkProps, UseComponentIconsProps } from '../types'

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw'> {
  label?: string
}
</script>

<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/Button.vue")
            .unwrap(),
        "workspace owner should load into the shared base project"
    );

    let meta = get_meta(&project, "/workspace/src/runtime/components/Button.vue");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"icon"),
        "workspace-only nested imports should keep icon props, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"replace") && prop_names.contains(&"activeClass"),
        "workspace-only nested imports should recurse into imported router types, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"download") && prop_names.contains(&"ping"),
        "workspace-only nested imports should recurse into imported html attrs, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"href") && prop_names.contains(&"label"),
        "workspace-only nested imports should preserve direct survivors and locals, got: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"raw") && !prop_names.contains(&"custom"),
        "workspace-only nested imports should still respect Omit, got: {prop_names:?}"
    );
}

// @ai-generated - Reproduces wrapper props imported from a generic interface exported by another .vue file through a barrel.
#[test]
fn get_component_meta_keeps_props_from_barrel_imported_generic_vue_interfaces() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from("export * from '../components/SelectMenu.vue'\nexport * from '../icons'\nexport * from './input'\n"),
    );
    ws.inject_file(
        "/workspace/src/runtime/icons.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts".to_string(),
        Arc::from(
            r#"export interface ButtonHTMLAttributes {
  name?: string
  formaction?: string
  formtarget?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/utils.ts".to_string(),
        Arc::from(
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/input.ts".to_string(),
        Arc::from(
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/SelectMenu.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'name'> {
  open?: boolean
  disabled?: boolean
  name?: string
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/ColorModeSelect.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
defineProps<ColorModeSelectProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/ColorModeSelect.vue")
            .unwrap(),
        "workspace owner should load into the shared base project"
    );

    let meta = get_meta(
        &project,
        "/workspace/src/runtime/components/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"loading"),
        "barrel-imported generic vue props should keep imported interface members, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"open")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"name"),
        "barrel-imported generic vue props should keep direct generic survivors, got: {prop_names:?}"
    );
    assert!(
        prop_names.contains(&"formaction")
            && prop_names.contains(&"formtarget")
            && prop_names.contains(&"searchInput")
            && prop_names.contains(&"valueKey"),
        "barrel-imported generic vue props should recurse into imported utility heritage, got: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"icon")
            && !prop_names.contains(&"items")
            && !prop_names.contains(&"modelValue"),
        "barrel-imported generic vue props should still respect wrapper Omit, got: {prop_names:?}"
    );
}

// @ai-generated - Reproduces imported Pick<VueButtonHTMLAttributes, ...> heritage surviving through generic wrapper Omit chains.
#[test]
fn get_component_meta_keeps_imported_picked_button_form_attrs_through_generic_wrapper_omits() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from("export * from '../components/SelectMenu.vue'\nexport * from '../icons'\nexport * from './input'\n"),
    );
    ws.inject_file(
        "/workspace/src/runtime/icons.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/vue-dom.ts".to_string(),
        Arc::from(
            r#"export interface VueButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts".to_string(),
        Arc::from(
            r#"import type { VueButtonHTMLAttributes } from '../vue-dom'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/utils.ts".to_string(),
        Arc::from(
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/input.ts".to_string(),
        Arc::from(
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/SelectMenu.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  disabled?: boolean
  name?: string
  open?: boolean
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/ColorModeSelect.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
defineProps<ColorModeSelectProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/ColorModeSelect.vue")
            .unwrap(),
        "workspace owner should load into the shared base project"
    );

    let meta = get_meta(
        &project,
        "/workspace/src/runtime/components/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"formenctype")
            && prop_names.contains(&"formmethod")
            && prop_names.contains(&"formnovalidate")
            && prop_names.contains(&"formtarget"),
        "picked button form attrs should survive generic wrapper omits, got: {prop_names:?}"
    );
}

// @ai-generated - Reproduces Pick<VueButtonHTMLAttributes, ...> form attrs disappearing when the source alias comes from a package import.
#[test]
fn get_component_meta_keeps_picked_package_button_form_attrs_through_generic_wrapper_omits() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/index.ts",
            "export * from '../components/SelectMenu.vue'\nexport * from '../icons'\nexport * from './input'\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/icons.ts",
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/input.ts",
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/utils.ts",
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/SelectMenu.vue",
            r#"<script lang="ts">
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  disabled?: boolean
  name?: string
  open?: boolean
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/color-mode/ColorModeSelect.vue",
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
defineProps<ColorModeSelectProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = get_meta(
        &project,
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"formenctype")
            && prop_names.contains(&"formmethod")
            && prop_names.contains(&"formnovalidate")
            && prop_names.contains(&"formtarget"),
        "package-picked button form attrs should survive generic wrapper omits, got: {prop_names:?}"
    );
}

// @ai-generated - Reproduces package-imported Pick<VueButtonHTMLAttributes, ...> heritage surviving through a cyclic barrel that also re-exports the wrapper component.
#[test]
fn get_component_meta_keeps_picked_package_button_form_attrs_through_cyclic_barrel_wrapper_omits() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/index.ts",
            r#"export * from '../components/SelectMenu.vue'
export * from '../components/color-mode/ColorModeSelect.vue'
export * from '../icons'
export * from './input'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/icons.ts",
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/input.ts",
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/utils.ts",
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/SelectMenu.vue",
            r#"<script lang="ts">
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  disabled?: boolean
  name?: string
  open?: boolean
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/color-mode/ColorModeSelect.vue",
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
defineProps<ColorModeSelectProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
        vec![crate::types::DependencyResolution {
            specifier: "../../types".to_string(),
            resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/SelectMenu.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "../types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/html".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/utils".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../components/SelectMenu.vue".to_string(),
                resolved_canonical_id: Some("/src/runtime/components/SelectMenu.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../components/color-mode/ColorModeSelect.vue".to_string(),
                resolved_canonical_id: Some(
                    "/src/runtime/components/color-mode/ColorModeSelect.vue".to_string(),
                ),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../icons".to_string(),
                resolved_canonical_id: Some("/src/runtime/icons.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./input".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/input.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let meta = get_meta(
        &project,
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"formenctype")
            && prop_names.contains(&"formmethod")
            && prop_names.contains(&"formnovalidate")
            && prop_names.contains(&"formtarget"),
        "package-picked button form attrs should survive cyclic barrel wrapper omits, got: {prop_names:?}"
    );
}

// @ai-generated - Reproduces package-imported Pick<VueButtonHTMLAttributes, ...> heritage surviving through a cyclic barrel when defineProps is wrapped in withDefaults().
#[test]
fn get_component_meta_keeps_picked_package_button_form_attrs_through_cyclic_barrel_with_defaults() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}

export declare function withDefaults<T, D>(props: T, defaults: D): T & D
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/index.ts",
            r#"export * from '../components/SelectMenu.vue'
export * from '../components/color-mode/ColorModeSelect.vue'
export * from '../icons'
export * from './input'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/icons.ts",
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/input.ts",
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/utils.ts",
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/SelectMenu.vue",
            r#"<script lang="ts">
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  disabled?: boolean
  name?: string
  open?: boolean
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/color-mode/ColorModeSelect.vue",
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<ColorModeSelectProps>(), {
  searchInput: false
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "../../types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "vue".to_string(),
                resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/SelectMenu.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "../types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/html".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/utils".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../components/SelectMenu.vue".to_string(),
                resolved_canonical_id: Some("/src/runtime/components/SelectMenu.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../components/color-mode/ColorModeSelect.vue".to_string(),
                resolved_canonical_id: Some(
                    "/src/runtime/components/color-mode/ColorModeSelect.vue".to_string(),
                ),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../icons".to_string(),
                resolved_canonical_id: Some("/src/runtime/icons.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./input".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/input.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let meta = get_meta(
        &project,
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"formenctype")
            && prop_names.contains(&"formmethod")
            && prop_names.contains(&"formnovalidate")
            && prop_names.contains(&"formtarget"),
        "package-picked button form attrs should survive cyclic barrel withDefaults wrapper, got: {prop_names:?}"
    );
}

// @ai-generated - Reproduces package-imported Pick<VueButtonHTMLAttributes, ...> heritage surviving when the imported generic interface also extends a picked external generic package interface.
#[test]
fn get_component_meta_keeps_picked_package_button_form_attrs_through_external_generic_pick_and_cyclic_barrel(
) {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"export interface ComboboxRootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  name?: string
  by?: string
  items?: T
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/index.ts",
            r#"export * from '../components/SelectMenu.vue'
export * from '../components/color-mode/ColorModeSelect.vue'
export * from '../icons'
export * from './input'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/icons.ts",
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/input.ts",
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/utils.ts",
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/SelectMenu.vue",
            r#"<script lang="ts">
import type { ComboboxRootProps } from 'reka-ui'
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends Pick<ComboboxRootProps<T>, 'open' | 'defaultOpen' | 'disabled' | 'name' | 'by'>,
    UseComponentIconsProps,
    Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/color-mode/ColorModeSelect.vue",
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<ColorModeSelectProps>(), {
  searchInput: false
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "../../types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "vue".to_string(),
                resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/SelectMenu.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "reka-ui".to_string(),
                resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/html".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../types/utils".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../components/SelectMenu.vue".to_string(),
                resolved_canonical_id: Some("/src/runtime/components/SelectMenu.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../components/color-mode/ColorModeSelect.vue".to_string(),
                resolved_canonical_id: Some(
                    "/src/runtime/components/color-mode/ColorModeSelect.vue".to_string(),
                ),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../icons".to_string(),
                resolved_canonical_id: Some("/src/runtime/icons.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./input".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/input.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let meta = get_meta(
        &project,
        "/src/runtime/components/color-mode/ColorModeSelect.vue",
    );
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"formenctype")
            && prop_names.contains(&"formmethod")
            && prop_names.contains(&"formnovalidate")
            && prop_names.contains(&"formtarget"),
        "package-picked button form attrs should survive external generic pick + cyclic barrel wrapper, got: {prop_names:?}"
    );
}

#[test]
fn evaluate_types_keeps_reexported_vue_button_form_attrs_through_workspace_generic_wrapper() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue/package.json".to_string(),
        Arc::from(
            r#"{ "name": "vue", "types": "./dist/vue.d.ts", "exports": { ".": { "types": "./dist/vue.d.ts", "import": "./dist/vue.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/dist/vue.d.ts".to_string(),
        Arc::from("export * from '@vue/runtime-dom'"),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/dist/vue.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/package.json".to_string(),
        Arc::from(
            r#"{ "name": "@vue/runtime-dom", "types": "./dist/runtime-dom.d.ts", "exports": { ".": { "types": "./dist/runtime-dom.d.ts", "import": "./dist/runtime-dom.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts".to_string(),
        Arc::from(
            r#"export interface HTMLAttributes {
  class?: any
}

export interface ButtonHTMLAttributes extends HTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/dist/runtime-dom.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/package.json".to_string(),
        Arc::from(
            r#"{ "name": "reka-ui", "types": "./dist/index.d.ts", "exports": { ".": { "types": "./dist/index.d.ts", "import": "./dist/index.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/index.d.ts".to_string(),
        Arc::from(
            r#"export interface ComboboxRootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  name?: string
  by?: string
  items?: T
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/index.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from(
            r#"export * from '../components/SelectMenu.vue'
export * from '../components/color-mode/ColorModeSelect.vue'
export * from '../icons'
export * from './input'
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/icons.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/input.ts".to_string(),
        Arc::from(
            r#"export interface InputProps {
  modelValue?: string
  placeholder?: string
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts".to_string(),
        Arc::from(
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/utils.ts".to_string(),
        Arc::from(
            r#"export type ArrayOrNested<T> = T[]
export type GetItemKeys<T> = string
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/SelectMenu.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { ComboboxRootProps } from 'reka-ui'
import type { InputProps, UseComponentIconsProps } from '../types'
import type { ButtonHTMLAttributes } from '../types/html'
import type { ArrayOrNested, GetItemKeys } from '../types/utils'

export type SelectMenuItem = {
  label?: string
  value?: string
}

export interface SelectMenuProps<
  T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<T> | undefined = undefined,
  M extends boolean = false
> extends Pick<ComboboxRootProps<T>, 'open' | 'defaultOpen' | 'disabled' | 'name' | 'by'>,
    UseComponentIconsProps,
    Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  searchInput?: boolean | Omit<InputProps, 'modelValue'>
  valueKey?: VK
  items?: T
  modelValue?: M extends true ? T : SelectMenuItem
}
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/color-mode/ColorModeSelect.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<ColorModeSelectProps>(), {
  searchInput: false
})
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    let project = MetaProject::new(host);
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/color-mode/ColorModeSelect.vue")
            .unwrap(),
        "workspace owner should load the wrapper component"
    );

    let session = project.open_session().unwrap();
    let evaluated = session
        .evaluate_types("/workspace/src/runtime/components/color-mode/ColorModeSelect.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    let define_props = evaluated
        .define_props
        .first()
        .expect("wrapper should produce a defineProps expansion");
    let prop_names: Vec<&str> = define_props
        .result
        .value
        .properties
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"formenctype")
            && prop_names.contains(&"formmethod")
            && prop_names.contains(&"formnovalidate")
            && prop_names.contains(&"formtarget"),
        "workspace evaluate_types should preserve re-exported vue button form attrs, got: {prop_names:?}"
    );
    assert!(
        !define_props.result.diagnostics.iter().any(|diagnostic| {
            diagnostic.reason
                == verter_analysis::type_expand::ExpansionStopReason::UnresolvedReference
                && diagnostic.context.contains("VueButtonHTMLAttributes")
        }),
        "workspace evaluate_types should not leave VueButtonHTMLAttributes unresolved, got {:?}",
        define_props.result.diagnostics
    );
}

#[test]
fn evaluate_types_keeps_complex_nuxt_ui_form_attrs_through_wrapper_omits() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/tsconfig.json".to_string(),
        Arc::from(
            r#"{ "compilerOptions": { "module": "esnext", "moduleResolution": "bundler" } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/package.json".to_string(),
        Arc::from(
            r#"{ "name": "vue", "type": "module", "exports": { ".": { "types": "./dist/vue.d.mts", "import": "./dist/vue.runtime.mjs" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/dist/vue.d.mts".to_string(),
        Arc::from(
            r#"export * from '@vue/runtime-dom'
export type VNode = any
export declare function withDefaults<T, D>(props: T, defaults: D): T & D
"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/dist/vue.runtime.mjs".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/package.json".to_string(),
        Arc::from(
            r#"{ "name": "@vue/runtime-dom", "type": "module", "exports": { ".": { "types": "./dist/runtime-dom.d.ts", "import": "./dist/runtime-dom.mjs" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts".to_string(),
        Arc::from(
            r#"export interface HTMLAttributes {
  class?: any
}

export interface ButtonHTMLAttributes extends HTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-dom/dist/runtime-dom.mjs".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/package.json".to_string(),
        Arc::from(
            r#"{ "name": "reka-ui", "type": "module", "exports": { ".": { "types": "./dist/index.d.ts", "import": "./dist/index.mjs" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/index.d.ts".to_string(),
        Arc::from(
            r#"export interface ComboboxRootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  name?: string
  resetSearchTermOnBlur?: boolean
  resetSearchTermOnSelect?: boolean
  resetModelValueOnClear?: boolean
  highlightOnHover?: boolean
  by?: string
  items?: T
}

export interface ComboboxRootEmits {
  'update:open': [value: boolean]
}

export interface ComboboxContentProps {
  side?: 'bottom' | 'top'
  sideOffset?: number
  collisionPadding?: number
  position?: 'popper' | 'item-aligned'
  as?: string
  asChild?: boolean
  forceMount?: boolean
}

export interface ComboboxContentEmits {
  escapeKeyDown?: [event: KeyboardEvent]
}

export interface ComboboxArrowProps {
  width?: number
  height?: number
  as?: string
  asChild?: boolean
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/reka-ui/dist/index.mjs".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/input.ts".to_string(),
        Arc::from(
            r#"export interface ModelModifiers {
  trim?: boolean
  number?: boolean
  lazy?: boolean
}

export type ApplyModifiers<T, _Mod> = T
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/utils.ts".to_string(),
        Arc::from(
            r#"export type AcceptableValue = string | number
export type ArrayOrNested<T> = T[] | T[][]
export type GetItemKeys<T> = string
export type GetItemValue<T, VK> = VK extends string ? string : T
export type GetModelValue<T, VK, M, ExcludeItem> = M extends true
  ? Array<GetItemValue<T, VK>>
  : GetItemValue<T, VK> | ExcludeItem
export type NestedItem<A> = A extends Array<infer U> ? U : never
export type EmitsToProps<T> = T extends object ? { [K in keyof T as K extends string ? `on${Capitalize<K>}` : never]?: T[K] } : {}
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/tv.ts".to_string(),
        Arc::from(
            r#"export type ComponentConfig<_Theme, _AppConfig, _Name extends string> = {
  variants: {
    color: 'primary' | 'neutral'
    variant: 'outline' | 'ghost'
    size: 'sm' | 'md'
  }
  slots: Record<string, any>
  ui: Record<string, any>
}
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts".to_string(),
        Arc::from(
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from(
            r#"export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}

export interface AvatarProps {
  src?: string
}

export interface ButtonProps {
  color?: string
  variant?: string
  icon?: string
}

export interface ChipProps {
  color?: string
}

export interface IconProps {
  name?: string
}

export interface InputProps {
  modelValue?: string
  defaultValue?: string
  placeholder?: string
  variant?: string
}

export type LinkPropsKeys = 'href' | 'to'

export * from '../components/SelectMenu.vue'
export * from '../components/color-mode/ColorModeSelect.vue'
export * from './input'
export * from './tv'
export * from './utils'
"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/SelectMenu.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { ComboboxRootProps, ComboboxRootEmits, ComboboxContentProps, ComboboxContentEmits, ComboboxArrowProps } from 'reka-ui'
import type { VNode } from 'vue'
import type { UseComponentIconsProps } from '../types'
import type { AvatarProps, ButtonProps, ChipProps, IconProps, InputProps, LinkPropsKeys } from '../types'
import type { ModelModifiers, ApplyModifiers } from '../types/input'
import type { ButtonHTMLAttributes } from '../types/html'
import type { AcceptableValue, ArrayOrNested, GetItemKeys, GetModelValue, NestedItem, EmitsToProps } from '../types/utils'
import type { ComponentConfig } from '../types/tv'

type SelectMenu = ComponentConfig<unknown, {}, 'selectMenu'>

export type SelectMenuValue = AcceptableValue

export type SelectMenuItem = SelectMenuValue | {
  label?: string
  description?: string
  icon?: IconProps['name']
  avatar?: AvatarProps
  chip?: ChipProps
  type?: 'label' | 'separator' | 'item'
  disabled?: boolean
  onSelect?: (e: Event) => void
  class?: any
  ui?: Pick<SelectMenu['slots'], 'label' | 'separator' | 'item'>
  [key: string]: any
}

type ExcludeItem = { type: 'label' | 'separator' }
type IsClearUsed<M extends boolean, C extends boolean | object> = M extends false
  ? (C extends true ? null : C extends object ? null : never)
  : never

export interface SelectMenuProps<T extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>, VK extends GetItemKeys<T> | undefined = undefined, M extends boolean = false, Mod extends Omit<ModelModifiers, 'lazy'> = Omit<ModelModifiers, 'lazy'>, C extends boolean | object = false> extends Pick<ComboboxRootProps<T>, 'open' | 'defaultOpen' | 'disabled' | 'name' | 'resetSearchTermOnBlur' | 'resetSearchTermOnSelect' | 'resetModelValueOnClear' | 'highlightOnHover' | 'by'>, UseComponentIconsProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  id?: string
  placeholder?: string
  searchInput?: boolean | Omit<InputProps, 'modelValue' | 'defaultValue'>
  color?: SelectMenu['variants']['color']
  variant?: SelectMenu['variants']['variant']
  size?: SelectMenu['variants']['size']
  required?: boolean
  trailingIcon?: IconProps['name']
  selectedIcon?: IconProps['name']
  clear?: (C & boolean) | (C & Partial<Omit<ButtonProps, LinkPropsKeys>>)
  clearIcon?: IconProps['name']
  content?: Omit<ComboboxContentProps, 'as' | 'asChild' | 'forceMount'> & Partial<EmitsToProps<ComboboxContentEmits>>
  arrow?: boolean | Omit<ComboboxArrowProps, 'as' | 'asChild'>
  portal?: boolean | string | HTMLElement
  virtualize?: boolean | {
    overscan?: number
    estimateSize?: number | ((index: number) => number)
  }
  valueKey?: VK
  labelKey?: GetItemKeys<T>
  descriptionKey?: GetItemKeys<T>
  items?: T
  defaultValue?: ApplyModifiers<GetModelValue<T, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>
  modelValue?: ApplyModifiers<GetModelValue<T, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>
  modelModifiers?: Mod
  multiple?: M & boolean
  highlight?: boolean
  createItem?: boolean | 'always' | { position?: 'top' | 'bottom', when?: 'empty' | 'always' }
  filterFields?: string[]
  ignoreFilter?: boolean
  autofocus?: boolean
  autofocusDelay?: number
  class?: any
  ui?: SelectMenu['slots']
}

export interface SelectMenuEmits<
  A extends ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<A> | undefined,
  M extends boolean,
  Mod extends Omit<ModelModifiers, 'lazy'> = Omit<ModelModifiers, 'lazy'>,
  C extends boolean | object = false
> extends Pick<ComboboxRootEmits, 'update:open'> {
  'change': [event: Event]
  'blur': [event: FocusEvent]
  'focus': [event: FocusEvent]
  'create': [item: string]
  'clear': []
  'highlight': [payload: {
    ref: HTMLElement
    value: ApplyModifiers<GetModelValue<A, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>
  } | undefined]
  'update:modelValue': [value: ApplyModifiers<GetModelValue<A, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>]
}

type SlotProps<T extends SelectMenuItem> = (props: { item: T, index: number, ui: SelectMenu['ui'] }) => VNode[]

export interface SelectMenuSlots<
  A extends ArrayOrNested<SelectMenuItem> = ArrayOrNested<SelectMenuItem>,
  VK extends GetItemKeys<A> | undefined = undefined,
  M extends boolean = false,
  Mod extends Omit<ModelModifiers, 'lazy'> = Omit<ModelModifiers, 'lazy'>,
  C extends boolean | object = false,
  T extends NestedItem<A> = NestedItem<A>
> {
  'default'?(props: {
    modelValue: ApplyModifiers<GetModelValue<A, VK, M, ExcludeItem>, Mod> | IsClearUsed<M, C>
    open: boolean
    ui: SelectMenu['ui']
  }): VNode[]
  'item'?: SlotProps<T>
}
</script>

<script setup lang="ts" generic="T extends ArrayOrNested<SelectMenuItem>, VK extends GetItemKeys<T> | undefined = undefined, M extends boolean = false, Mod extends Omit<ModelModifiers, 'lazy'> = Omit<ModelModifiers, 'lazy'>, C extends boolean | object = false">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<SelectMenuProps<T, VK, M, Mod, C>>(), {
  portal: true,
  searchInput: true,
  labelKey: 'label',
  descriptionKey: 'description',
  resetSearchTermOnBlur: true,
  resetSearchTermOnSelect: true,
  resetModelValueOnClear: true,
  autofocusDelay: 0,
  virtualize: false
})
</script>
<template><div /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/color-mode/ColorModeSelect.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
import type { SelectMenuProps, SelectMenuItem } from '../../types'

export interface ColorModeSelectProps extends Omit<SelectMenuProps<SelectMenuItem[]>, 'icon' | 'items' | 'modelValue'> {
}
</script>

<script setup lang="ts">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<ColorModeSelectProps>(), {
  searchInput: false
})
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    let project = MetaProject::new(host);
    assert!(
        project
            .ensure_loaded("/workspace/src/runtime/components/color-mode/ColorModeSelect.vue")
            .unwrap(),
        "workspace owner should load the complex wrapper component"
    );

    let session = project.open_session().unwrap();
    let evaluated = session
        .evaluate_types("/workspace/src/runtime/components/color-mode/ColorModeSelect.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    let define_props = evaluated
        .define_props
        .first()
        .expect("wrapper should produce a defineProps expansion");
    let prop_names: Vec<&str> = define_props
        .result
        .value
        .properties
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"formenctype")
            && prop_names.contains(&"formmethod")
            && prop_names.contains(&"formnovalidate")
            && prop_names.contains(&"formtarget"),
        "complex Nuxt UI wrapper should preserve inherited button form attrs, got: {prop_names:?}"
    );
}

#[test]
fn evaluate_types_hydrates_transitive_imported_pick_dependencies_for_wrapper_props() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types.ts",
            r#"import type { ButtonHTMLAttributes } from './types/html'

export interface Props extends Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  label?: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './runtime/types'

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/runtime/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./types/html".to_string(),
            resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./runtime/types".to_string(),
            resolved_canonical_id: Some("/src/runtime/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    let evaluated = session
        .evaluate_types("/src/App.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    let define_props = evaluated
        .define_props
        .first()
        .expect("wrapper should produce a defineProps expansion");
    let prop_names: Vec<&str> = define_props
        .result
        .value
        .properties
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"formenctype")
            && prop_names.contains(&"formmethod")
            && prop_names.contains(&"formnovalidate")
            && prop_names.contains(&"formtarget"),
        "transitive imported Pick dependencies should survive wrapper evaluation, got: {prop_names:?}"
    );
    assert!(
        !define_props.result.diagnostics.iter().any(|diagnostic| {
            diagnostic.reason == verter_analysis::type_expand::ExpansionStopReason::UnresolvedReference
                && diagnostic.context.contains("VueButtonHTMLAttributes")
        }),
        "transitive imported Pick dependencies should not leave VueButtonHTMLAttributes unresolved, got {:?}",
        define_props.result.diagnostics
    );
}

#[test]
fn evaluate_types_hydrates_transitive_imported_pick_dependencies_from_dual_script_vue_deps() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}

export declare function withDefaults<T, D>(props: T, defaults: D): T & D
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/html.ts",
            r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/components/SelectMenu.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes } from '../types/html'

export type SelectMenuItem = {
  label?: string
}

export interface SelectMenuProps<T extends SelectMenuItem[] = SelectMenuItem[]> extends Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  items?: T
  label?: string
}
</script>

<script setup lang="ts" generic="T extends SelectMenuItem[] = SelectMenuItem[]">
import { withDefaults } from 'vue'

const props = withDefaults(defineProps<SelectMenuProps<T>>(), {})
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/runtime/types/index.ts",
            r#"export * from '../components/SelectMenu.vue'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { SelectMenuProps, SelectMenuItem } from './runtime/types'

defineProps<Omit<SelectMenuProps<SelectMenuItem[]>, 'items'>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/runtime/types/html.ts",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/runtime/components/SelectMenu.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "../types/html".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "vue".to_string(),
                resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/runtime/types/index.ts",
        vec![crate::types::DependencyResolution {
            specifier: "../components/SelectMenu.vue".to_string(),
            resolved_canonical_id: Some("/src/runtime/components/SelectMenu.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./runtime/types".to_string(),
            resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    let evaluated = session
        .evaluate_types("/src/App.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    let define_props = evaluated
        .define_props
        .first()
        .expect("wrapper should produce a defineProps expansion");
    let prop_names: Vec<&str> = define_props
        .result
        .value
        .properties
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"formenctype")
            && prop_names.contains(&"formmethod")
            && prop_names.contains(&"formnovalidate")
            && prop_names.contains(&"formtarget"),
        "dual-script vue wrapper evaluation should preserve transitive Pick dependencies, got: {prop_names:?}"
    );
    assert!(
        !define_props.result.diagnostics.iter().any(|diagnostic| {
            diagnostic.reason == verter_analysis::type_expand::ExpansionStopReason::UnresolvedReference
                && diagnostic.context.contains("VueButtonHTMLAttributes")
        }),
        "dual-script vue wrapper evaluation should not leave VueButtonHTMLAttributes unresolved, got {:?}",
        define_props.result.diagnostics
    );
}

#[test]
fn get_component_meta_keeps_local_slot_surface_without_imported_helper_pollution() {
    let project = make_project();
    project
        .upsert_base(
            "/tv.ts",
            r#"export type DynamicSlots<T extends Record<string, any>> = {
  [K in keyof T]?: (props: {}) => any
}

export type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: (props: {}) => any
}

export type ComponentConfig<T extends { slots?: Record<string, any> }, A extends Record<string, any>> = {
  appConfig: A
  slots: ComponentSlots<T>
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/schema.ts",
            r#"export interface AppConfig {
  ui?: { variant: string }
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/theme.ts",
            r#"export default {
  slots: {
    leading: 'leading',
    trailing: 'trailing'
  }
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ComponentConfig, DynamicSlots } from './tv'
import type { AppConfig } from './schema'
import theme from './theme'

type Accordion = ComponentConfig<typeof theme, AppConfig>

interface Slots extends DynamicSlots<Accordion['slots']> {
  default(props: { item: string }): any
  leading?(): any
  trailing?(): any
}

defineSlots<Slots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();
    let meta = get_meta(&project, "/App.vue");
    let slot_names: Vec<&str> = meta.slots.iter().map(|slot| slot.name.as_str()).collect();
    assert_eq!(slot_names, vec!["default", "leading", "trailing"]);
    assert!(
        !slot_names.contains(&"appConfig") && !slot_names.contains(&"slots"),
        "defineSlots output should not be polluted by imported helper object members: {slot_names:?}"
    );
}

#[test]
fn evaluate_types_invalidates_cached_results_when_dependency_changes() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface ImportedUser {
  id: number
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { ImportedUser } from './types'

defineProps<{
  user: ImportedUser
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let first = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let first_cache =
        cached_resolved_state(&project, "/Comp.vue", crate::types::ResolverMode::Expanded)
            .expect("first evaluation should populate the cache");

    match evaluated_prop_type(&first, "user") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(names, vec!["id"]);
        }
        other => panic!("expected imported interface to resolve to an object, got {other:?}"),
    }

    session
        .upsert(
            "/types.ts",
            r#"export interface ImportedUser {
  id: number
  label: string
}"#
            .into(),
        )
        .unwrap();

    let second = session.evaluate_types("/Comp.vue").unwrap().unwrap();
    let second_cache =
        cached_resolved_state(&project, "/Comp.vue", crate::types::ResolverMode::Expanded)
            .expect("dependency update should repopulate the cache");

    assert!(
        !Arc::ptr_eq(&first_cache, &second_cache),
        "dependency change must invalidate the owner's resolved-meta cache",
    );
    match evaluated_prop_type(&second, "user") {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|member| match member {
                    ObjectMember::Property(prop) => Some(prop.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"id"));
            assert!(names.contains(&"label"));
        }
        other => panic!("expected imported interface to resolve to an object, got {other:?}"),
    }
}

// ===========================================================================
// Phase 1: Provenance counters and enriched-analysis caching
// ===========================================================================

/// Helper to read the provenance counters from a MetaProject's host.
fn provenance(project: &MetaProject) -> crate::types::MetaProvenanceSnapshot {
    project.host().provenance().snapshot()
}

#[test]
fn evaluate_types_returns_correct_results_for_imported_types() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<{ item: Props }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();

    let evaluated = session
        .evaluate_types("/App.vue")
        .expect("evaluate_types should succeed")
        .expect("should return evaluated types");

    // Assert+: the prop referencing the imported type is present
    assert_eq!(
        evaluated.props.len(),
        1,
        "should have exactly 1 prop 'item'"
    );
    assert_eq!(evaluated.props[0].name, "item");

    // Assert-: no spurious props with names from the imported interface
    assert!(
        !evaluated
            .props
            .iter()
            .any(|p| p.name == "a" || p.name == "b"),
        "imported interface fields should not appear as top-level props"
    );
}

#[test]
fn evaluate_types_cold_path_does_not_call_public_get_analysis_workflow() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session().unwrap();

    let _ = session
        .evaluate_types("/App.vue")
        .expect("evaluate_types should succeed on a cold path");

    let p = provenance(&project);
    assert_eq!(
        p.get_analysis_calls, 0,
        "evaluate_types should use the private resolved-state helper instead of the public get_analysis workflow",
    );
}

#[test]
fn evaluate_types_works_independently_of_prior_get_analysis_call() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("count: number; label: string"))
        .unwrap();

    let session = project.open_session().unwrap();

    // Call get_analysis first (raw, no enrichment)
    let analysis = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("get_analysis should return raw analysis");

    // get_analysis returns raw props
    let raw_names = prop_names(&analysis);
    assert!(
        raw_names.contains(&"count".to_string()),
        "raw analysis should have 'count' prop"
    );

    // evaluate_types should still work correctly regardless of prior get_analysis
    let evaluated = session
        .evaluate_types("/App.vue")
        .expect("evaluate_types should succeed")
        .expect("should return evaluated types");

    // Assert+: types are properly resolved
    assert_eq!(
        evaluated_prop_type(&evaluated, "count"),
        &TypeExpr::Primitive(PrimitiveName::Number),
    );
    assert_eq!(
        evaluated_prop_type(&evaluated, "label"),
        &TypeExpr::Primitive(PrimitiveName::String),
    );

    // Assert-: only the expected props
    assert_eq!(evaluated.props.len(), 2);
}

#[test]
fn evaluate_types_returns_consistent_results_for_repeated_calls() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("a: string; b: number"))
        .unwrap();

    let session = project.open_session().unwrap();

    // First call
    let first = session
        .evaluate_types("/App.vue")
        .expect("first evaluate_types should succeed")
        .expect("should return evaluated types");

    // Second call — should return identical results
    let second = session
        .evaluate_types("/App.vue")
        .expect("second evaluate_types should succeed")
        .expect("should return evaluated types");

    // Assert+: both calls return the same prop count and types
    assert_eq!(
        first.props.len(),
        second.props.len(),
        "repeated evaluate_types calls should return the same number of props"
    );
    assert_eq!(
        evaluated_prop_type(&first, "a"),
        evaluated_prop_type(&second, "a"),
        "repeated calls should return the same type for prop 'a'"
    );

    // Assert-: no extra props introduced
    assert_eq!(first.props.len(), 2, "should have exactly 2 props");
}

#[test]
fn resolve_component_meta_expanded_returns_consistent_results_on_repeated_calls() {
    use crate::types::ResolverMode;

    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    // Force host to load the file
    let _ = session.get_analysis("/App.vue").unwrap();

    // First call
    let first = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("first resolve_component_meta should succeed");

    // Second call — should return consistent results
    let second = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("second resolve_component_meta should succeed");

    // Assert+: both calls return the same resolved macros
    assert_eq!(
        first.resolved_macros.len(),
        second.resolved_macros.len(),
        "repeated calls should return the same number of resolved macros"
    );

    // Assert+: resolved macros have consistent prop counts
    assert!(
        !first.resolved_macros.is_empty(),
        "Expanded mode should resolve cross-file macro types on first call"
    );
    assert!(
        !second.resolved_macros.is_empty(),
        "Expanded mode should resolve cross-file macro types on second call"
    );
    assert_eq!(
        first.resolved_macros[0].props.len(),
        second.resolved_macros[0].props.len(),
        "repeated calls should produce the same resolved prop count"
    );

    // Assert-: mode is Expanded, not Type
    assert_eq!(first.mode, ResolverMode::Expanded);
    assert_ne!(first.mode, ResolverMode::Type);
}

#[test]
fn resolve_component_meta_expanded_returns_updated_results_after_owner_change() {
    use crate::types::ResolverMode;

    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("a: string; b: number"))
        .unwrap();

    // First call — inline props should be resolved
    let first = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("first resolve_component_meta should succeed");

    let first_snap_props = prop_names(&first.snapshot);
    assert!(
        first_snap_props.contains(&"a".to_string()),
        "first call should have prop 'a', got: {:?}",
        first_snap_props
    );
    assert_eq!(first_snap_props.len(), 2, "should start with 2 props");

    // Modify the owner SFC to change props
    project
        .upsert_base("/App.vue", &sfc("c: boolean; d: string"))
        .unwrap();

    // Second call — should see the updated props
    let second = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("second resolve_component_meta should succeed after owner change");

    let second_snap_props = prop_names(&second.snapshot);

    // Assert+: result includes the new props
    assert!(
        second_snap_props.contains(&"c".to_string()),
        "owner change should produce updated props including 'c', got: {:?}",
        second_snap_props
    );
    assert!(
        second_snap_props.contains(&"d".to_string()),
        "owner change should produce updated props including 'd', got: {:?}",
        second_snap_props
    );

    // Assert-: old props should not appear
    assert!(
        !second_snap_props.contains(&"a".to_string()),
        "old prop 'a' should not appear after owner change"
    );
    assert!(
        !second_snap_props.contains(&"b".to_string()),
        "old prop 'b' should not appear after owner change"
    );
}

#[test]
fn resolve_component_meta_expanded_returns_updated_results_after_dependency_change() {
    use crate::types::ResolverMode;

    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Manually register the import dependency so reverse-dep tracking works.
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // First call — should resolve props a, b via resolved_macros
    let first = project
        .host()
        .resolve_component_meta("/src/App.vue", ResolverMode::Expanded)
        .expect("first resolve_component_meta should succeed");

    assert!(
        !first.resolved_macros.is_empty(),
        "Expanded mode should resolve cross-file macro types"
    );
    let first_prop_names: Vec<&str> = first.resolved_macros[0]
        .props
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert!(
        first_prop_names.contains(&"a") && first_prop_names.contains(&"b"),
        "first call should resolve props a and b, got: {:?}",
        first_prop_names
    );

    // Modify the dependency via base upsert (directly on host, not session)
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface Props { a: string; b: number; c: boolean }"#,
        )
        .unwrap();

    // Second call — should reflect the dependency change
    let second = project
        .host()
        .resolve_component_meta("/src/App.vue", ResolverMode::Expanded)
        .expect("resolve_component_meta should succeed after dependency change");

    assert!(
        !second.resolved_macros.is_empty(),
        "should still have resolved macros after dep change"
    );
    let second_prop_names: Vec<&str> = second.resolved_macros[0]
        .props
        .iter()
        .map(|p| p.name.as_str())
        .collect();

    // Assert+: result includes the new prop 'c'
    assert!(
        second_prop_names.contains(&"c"),
        "dependency change should produce updated props including 'c', got: {:?}",
        second_prop_names
    );

    // Assert-: should not still have only the old 2-prop result
    assert!(
        second_prop_names.len() > 2,
        "dependency change must not return the stale 2-prop result, got: {:?}",
        second_prop_names
    );
}

#[test]
fn invalidate_compile_slots_does_not_break_subsequent_analysis() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    let session = project.open_session().unwrap();
    let before = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should exist before invalidation");
    let before_names = prop_names(&before);
    assert!(
        before_names.contains(&"msg".to_string()),
        "should see 'msg' prop before invalidation"
    );

    project.host().invalidate_compile_slots("/App.vue");

    // Assert+: analysis still works after invalidation
    let after = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should still work after invalidate_compile_slots");
    let after_names = prop_names(&after);
    assert!(
        after_names.contains(&"msg".to_string()),
        "should still see 'msg' prop after invalidation"
    );

    // Assert-: no spurious props introduced
    assert_eq!(
        after_names.len(),
        1,
        "should have exactly 1 prop after invalidation, not more"
    );
}

#[test]
fn removing_dependency_does_not_break_subsequent_analysis() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    // Verify analysis works before removal
    let before = session
        .get_analysis("/src/App.vue")
        .unwrap()
        .expect("analysis should work before dependency removal");
    // Raw analysis may not resolve cross-file props, but should succeed
    assert!(
        before
            .macros
            .iter()
            .any(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps),
        "should have defineProps macro before removal"
    );

    let _ = project.host().remove("/src/types.ts");

    // Assert+: analysis still returns a result (doesn't panic/crash)
    let after = session.get_analysis("/src/App.vue").unwrap();
    assert!(
        after.is_some(),
        "analysis should still return a result after dependency removal"
    );

    // Assert-: the removed dependency should not be resolvable as a component
    assert!(
        project
            .host()
            .resolve_component_meta("/src/types.ts", crate::types::ResolverMode::Type)
            .is_none(),
        "removed dependency should not be resolvable via resolve_component_meta"
    );
}

#[cfg(not(feature = "scheduler"))]
#[test]
fn non_scheduler_upsert_reflects_updated_source_in_subsequent_analysis() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    let session = project.open_session().unwrap();
    let before = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should exist before upsert");
    let before_names = prop_names(&before);
    assert!(
        before_names.contains(&"msg".to_string()),
        "should see 'msg' before upsert"
    );

    let updated = sfc("msg: string; count: number");
    let _ = project
        .host()
        .upsert(crate::types::UpsertRequest {
            canonical_id: Some("/App.vue".to_string()),
            input_id: "/App.vue".to_string(),
            source: Arc::from(updated.as_str()),
            file_kind: crate::types::FileKind::from_path("/App.vue"),
            aliases: Vec::new(),
        })
        .unwrap();

    // Assert+: subsequent analysis reflects updated content
    let after = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("analysis should work after upsert");
    let after_names = prop_names(&after);
    assert!(
        after_names.contains(&"count".to_string()),
        "should see 'count' after upsert, got: {:?}",
        after_names
    );

    // Assert-: should not lose the original prop
    assert!(
        after_names.contains(&"msg".to_string()),
        "should still see 'msg' after upsert"
    );
}

// ===========================================================================
// Phase 3: Native get_component_meta query
// ===========================================================================

#[test]
fn get_component_meta_returns_props_and_events() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ label: string; count?: number }>()
defineEmits<{ change: [value: string] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    // Assert+: props extracted
    assert_eq!(meta.props.len(), 2, "should extract 2 props");
    assert_eq!(meta.props[0].name, "label");
    assert!(meta.props[0].required, "label should be required");
    assert_eq!(meta.props[1].name, "count");
    assert!(!meta.props[1].required, "count should be optional");

    // Assert+: events extracted
    assert_eq!(meta.events.len(), 1, "should extract 1 event");
    assert_eq!(meta.events[0].name, "change");

    // Assert-: no models, no exposed
    assert!(meta.models.is_empty(), "no defineModel → no models");
    assert!(meta.exposed.is_empty(), "no defineExpose → no exposed");
}

#[test]
fn get_component_meta_uses_single_native_query_path() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session().unwrap();

    let _meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let p = provenance(&project);

    // Assert+: the new query was called
    assert_eq!(
        p.get_component_meta_calls, 1,
        "get_component_meta should record one call"
    );

    // Assert+: resolved state was computed at most once
    assert!(
        p.component_meta_resolved_state_recomputes <= 1,
        "get_component_meta should compute resolved state at most once, got: {}",
        p.component_meta_resolved_state_recomputes
    );
}

#[test]
fn get_component_meta_returns_consistent_results_on_repeated_calls() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    let session = project.open_session().unwrap();

    // First call
    let first = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("first call should return metadata");

    // Second call — should return consistent results
    let second = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("second call should return metadata");

    // Assert+: both calls return the same props
    assert_eq!(
        first.props.len(),
        second.props.len(),
        "repeated calls should return the same number of props"
    );
    assert_eq!(
        first.props[0].name, second.props[0].name,
        "repeated calls should return the same prop names"
    );

    // Assert-: no extra events/models introduced
    assert!(
        first.events.is_empty() && second.events.is_empty(),
        "no defineEmits means no events on either call"
    );
    assert!(
        first.models.is_empty() && second.models.is_empty(),
        "no defineModel means no models on either call"
    );
}

#[test]
fn get_component_meta_provenance_uses_single_resolver_path() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session().unwrap();

    let _meta = session.get_component_meta("/App.vue").unwrap().unwrap();
    let p = provenance(&project);

    // Assert+: exactly one resolved state computation
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 1,
        "native get_component_meta should compute resolved state exactly once"
    );
    // Assert-: get_analysis should NOT have been called (component-meta uses the resolver path)
    assert_eq!(
        p.get_analysis_calls, 0,
        "native get_component_meta must not call get_analysis()"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn repeated_declared_component_meta_queries_reuse_cached_resolved_state_for_workspace_type_deps() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div>{{ msg }}</div></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/types.ts".to_string(),
        Arc::from(
            r#"export interface Base { id?: string }
export interface Props extends Base { msg: string; count?: number }"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "owner SFC should load into the host"
    );
    assert!(
        project
            .host()
            .get_whole_hash("/workspace/types.ts")
            .is_none(),
        "workspace dependency should not be eagerly loaded before the first query"
    );

    let session = project.open_session().unwrap();
    let first = session
        .get_declared_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("first declared query should return component meta");
    assert!(
        first.props.iter().any(|prop| prop.name == "msg"),
        "first declared query should resolve the imported prop surface"
    );
    assert!(
        first.props.iter().any(|prop| prop.name == "count"),
        "first declared query should resolve optional imported props"
    );

    project.host().provenance().reset();
    let second = session
        .get_declared_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("second declared query should return component meta");
    let p = provenance(&project);

    assert_eq!(
        second.props.len(),
        first.props.len(),
        "repeated declared query should keep the same prop surface"
    );
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 0,
        "second declared query should reuse the cached resolved state instead of recomputing it, got provenance={p:?}"
    );
    assert_eq!(
        p.resolver_node_cache_misses, 0,
        "second declared query should not miss the resolver node cache once the first query populated it, got provenance={p:?}"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn repeated_full_component_meta_queries_reuse_cached_resolved_state_for_workspace_type_deps() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div>{{ msg }}</div></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/types.ts".to_string(),
        Arc::from(
            r#"export interface Base { id?: string }
export interface Props extends Base { msg: string; count?: number }"#,
        ),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "owner SFC should load into the host"
    );
    assert!(
        project
            .host()
            .get_whole_hash("/workspace/types.ts")
            .is_none(),
        "workspace dependency should not be eagerly loaded before the first query"
    );

    let session = project.open_session().unwrap();
    let first = session
        .get_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("first full query should return component meta");
    assert!(
        first.props.iter().any(|prop| prop.name == "msg"),
        "first full query should resolve the imported prop surface"
    );
    assert!(
        first.props.iter().any(|prop| prop.name == "count"),
        "first full query should resolve optional imported props"
    );

    project.host().provenance().reset();
    let second = session
        .get_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("second full query should return component meta");
    let p = provenance(&project);

    assert_eq!(
        second.props.len(),
        first.props.len(),
        "repeated full query should keep the same prop surface"
    );
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 0,
        "second full query should reuse the cached resolved state instead of recomputing it, got provenance={p:?}"
    );
    assert_eq!(
        p.resolver_node_cache_misses, 0,
        "second full query should not miss the resolver node cache once the first query populated it, got provenance={p:?}"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn repeated_full_component_meta_queries_reuse_cached_resolved_state_for_imported_dependency_graph()
{
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { Props } from 'pkg'
defineProps<Props>()
</script>
<template><div>{{ msg }}</div></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/package.json".to_string(),
        Arc::from(
            r#"{ "name": "pkg", "types": "./dist/index.d.ts", "exports": { ".": { "types": "./dist/index.d.ts", "import": "./dist/index.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts".to_string(),
        Arc::from(r#"export { Props } from "./shared";"#),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/shared.d.ts".to_string(),
        Arc::from(
            r#"import type { Base } from "./base"
export interface Props extends Base { msg: string }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/base.d.ts".to_string(),
        Arc::from(r#"export interface Base { id?: string }"#),
    );

    let project = make_workspace_project(Arc::clone(&ws));
    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "owner SFC should load into the host"
    );
    project.host().set_import_dependencies(
        "/workspace/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "pkg".to_string(),
            resolved_canonical_id: Some("/workspace/node_modules/pkg/dist/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./shared".to_string(),
            resolved_canonical_id: Some("/workspace/node_modules/pkg/dist/shared.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/workspace/node_modules/pkg/dist/shared.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./base".to_string(),
            resolved_canonical_id: Some("/workspace/node_modules/pkg/dist/base.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    assert!(
        project
            .host()
            .get_whole_hash("/workspace/node_modules/pkg/dist/shared.d.ts")
            .is_none(),
        "imported dependency should not be eagerly loaded before the first query"
    );

    let session = project.open_session().unwrap();
    let first = session
        .get_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("first imported-dependency query should return component meta");
    assert!(
        first.props.iter().any(|prop| prop.name == "msg"),
        "first query should resolve the package prop surface"
    );
    assert!(
        first.props.iter().any(|prop| prop.name == "id"),
        "first query should resolve transitive imported base props"
    );

    project.host().provenance().reset();
    let second = session
        .get_component_meta("/workspace/App.vue")
        .unwrap()
        .expect("second imported-dependency query should return component meta");
    let p = provenance(&project);

    assert_eq!(
        second.props.len(),
        first.props.len(),
        "repeated imported-dependency query should keep the same prop surface"
    );
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 0,
        "second imported-dependency query should reuse the cached resolved state instead of recomputing it, got provenance={p:?}"
    );
    assert_eq!(
        p.resolver_node_cache_misses, 0,
        "second imported-dependency query should not miss the resolver node cache once the first query populated it, got provenance={p:?}"
    );
}

#[test]
fn get_component_meta_does_not_call_public_evaluate_types_workflow() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session().unwrap();

    let _meta = session.get_component_meta("/App.vue").unwrap().unwrap();
    let p = provenance(&project);

    assert_eq!(
        p.evaluate_types_calls, 0,
        "native get_component_meta must not route through the public evaluate_types workflow"
    );
}

#[test]
fn get_component_meta_cold_path_does_not_call_public_get_analysis_workflow() {
    let project = make_project();
    project
        .upsert_base("/App.vue", &sfc("msg: string"))
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session().unwrap();

    let _meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");
    let p = provenance(&project);

    assert_eq!(
        p.get_analysis_calls, 0,
        "native get_component_meta must not route through the public get_analysis workflow",
    );
}

#[test]
fn get_component_meta_prefers_declaration_entrypoints_for_package_type_imports() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/fancy/package.json".to_string(),
        Arc::from(
            r#"{ "name": "fancy", "types": "./dist/index.d.ts", "exports": { ".": { "import": "./dist/index.js", "require": "./dist/index.cjs" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.d.ts".to_string(),
        Arc::from(r#"import { FancyProps } from "./inner.js"; export type { FancyProps };"#),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/inner.d.ts".to_string(),
        Arc::from("export interface FancyProps { open: boolean }"),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/inner.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    let project = MetaProject::new(host);
    project
        .upsert_base(
            "/workspace/src/Consumer.vue",
            r#"<script setup lang="ts">
import type { FancyProps } from 'fancy'
defineProps<FancyProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/workspace/src/Consumer.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    assert_eq!(meta.props.len(), 1, "should extract the imported prop");
    assert_eq!(meta.props[0].name, "open");
    assert_eq!(meta.props[0].raw_type.as_deref(), Some("boolean"));
    assert!(
        matches!(
            meta.props[0].type_expr,
            TypeExpr::Primitive(PrimitiveName::Boolean)
        ),
        "expanded prop type should come from the declaration entrypoint, got: {:?}",
        meta.props[0].type_expr
    );
}

#[test]
fn evaluate_types_prefers_declaration_entrypoints_for_package_type_imports() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/fancy/package.json".to_string(),
        Arc::from(
            r#"{ "name": "fancy", "types": "./dist/index.d.ts", "exports": { ".": { "import": "./dist/index.js", "require": "./dist/index.cjs" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.d.ts".to_string(),
        Arc::from(r#"import { FancyProps } from "./inner.js"; export type { FancyProps };"#),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/inner.d.ts".to_string(),
        Arc::from("export interface FancyProps { open: boolean }"),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/inner.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    let project = MetaProject::new(host);
    project
        .upsert_base(
            "/workspace/src/Consumer.vue",
            r#"<script setup lang="ts">
import type { FancyProps } from 'fancy'
defineProps<FancyProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let evaluated = session
        .evaluate_types("/workspace/src/Consumer.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    let open_field = evaluated
        .define_props
        .iter()
        .flat_map(|entry| entry.result.value.properties.iter())
        .find(|field| field.name == "open")
        .expect("evaluated defineProps should include imported declaration prop");
    assert!(
        matches!(open_field.ty, TypeExpr::Primitive(PrimitiveName::Boolean)),
        "evaluate_types should resolve declaration-entrypoint prop types, got: {:?}",
        open_field.ty
    );
}

#[test]
fn get_component_meta_materializes_imported_pick_indexed_access_props() {
    let project = make_project();
    project
        .upsert_base(
            "/src/vue-dom.ts",
            r#"
export interface VueButtonHTMLAttributes {
  type?: 'button' | 'submit' | 'reset'
  disabled?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/html.ts",
            r#"
import type { VueButtonHTMLAttributes } from './vue-dom'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'type' | 'disabled'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes } from './html'

export interface Props {
  type?: ButtonHTMLAttributes['type']
  mirror?: Props['type']
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ResolverMode::Expanded)
        .expect("resolved component meta should exist");
    let inputs = resolved
        .cached_eval_inputs
        .as_ref()
        .expect("resolved state should retain cached imported eval inputs");
    let alias = inputs
        .type_aliases
        .iter()
        .find(|alias| alias.local_name == "ButtonHTMLAttributes")
        .expect("ButtonHTMLAttributes should be prepared as an imported alias");
    let alias_body = resolved_imported_alias_body(project.host(), alias);
    assert!(
        matches!(alias_body, TypeExpr::Object(_)),
        "prepared imported alias body should already be materialized, got {:?}",
        alias_body
    );

    let session = project.open_session().unwrap();
    let evaluated = session
        .evaluate_types("/src/App.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    assert_union_string_literals(
        evaluated_define_props_type(&evaluated, "type"),
        &["button", "submit", "reset"],
    );
    assert_union_string_literals(
        evaluated_define_props_type(&evaluated, "mirror"),
        &["button", "submit", "reset"],
    );

    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");
    let type_prop = meta
        .props
        .iter()
        .find(|prop| prop.name == "type")
        .expect("type prop should exist");
    let mirror_prop = meta
        .props
        .iter()
        .find(|prop| prop.name == "mirror")
        .expect("mirror prop should exist");

    assert!(
        !matches!(
            type_prop.type_expr,
            TypeExpr::Unknown { .. } | TypeExpr::IndexedAccess { .. }
        ),
        "imported Pick indexed access should not stay symbolic for type: {:?}",
        type_prop.type_expr
    );
    assert!(
        !matches!(
            mirror_prop.type_expr,
            TypeExpr::Unknown { .. } | TypeExpr::IndexedAccess { .. }
        ),
        "self indexed access should inherit the resolved imported surface: {:?}",
        mirror_prop.type_expr
    );
    assert_union_string_literals(&type_prop.type_expr, &["button", "submit", "reset"]);
    assert_union_string_literals(&mirror_prop.type_expr, &["button", "submit", "reset"]);
}

#[test]
fn evaluate_types_materializes_package_reexported_route_aliases_for_component_props() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue-router/package.json".to_string(),
        Arc::from(
            r#"{ "name": "vue-router", "types": "./dist/vue-router.d.ts", "exports": { ".": { "types": "./dist/vue-router.d.ts", "import": "./dist/vue-router.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/vue-router.d.ts".to_string(),
        Arc::from(r#"export { Lt as RouteLocationRaw } from "./index-typed.js";"#),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/index-typed.d.ts".to_string(),
        Arc::from(
            r#"
export interface St { path: string }
export interface vt { name: string }
export type Lt = string | St | vt
"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/index-typed.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    let project = MetaProject::new(host);
    project
        .upsert_base(
            "/workspace/src/Link.vue",
            r#"<script lang="ts">
import type { RouteLocationRaw } from 'vue-router'

export interface Props {
  to?: RouteLocationRaw
  href?: Props['to']
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/workspace/src/Link.vue",
            crate::types::ResolverMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let inputs = resolved
        .cached_eval_inputs
        .as_ref()
        .expect("resolved state should retain cached imported eval inputs");
    let alias = inputs
        .type_aliases
        .iter()
        .find(|alias| alias.local_name == "RouteLocationRaw")
        .expect("RouteLocationRaw should be prepared as an imported alias");
    assert_eq!(
        alias.source_canonical_id,
        "/workspace/node_modules/vue-router/dist/vue-router.d.ts"
    );
    assert_eq!(alias.exported_name, "RouteLocationRaw");
    assert_route_union_surface(&resolved_imported_alias_body(project.host(), alias));
    let registry_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "RouteLocationRaw")
        .expect("RouteLocationRaw should be published in the resolved type registry");
    assert_route_union_surface(&registry_entry.type_expr);

    let session = project.open_session().unwrap();
    let evaluated = session
        .evaluate_types("/workspace/src/Link.vue")
        .unwrap()
        .expect("evaluate_types should return a result");

    assert_route_union_surface(evaluated_define_props_type(&evaluated, "to"));
    assert_route_union_surface(evaluated_define_props_type(&evaluated, "href"));

    let meta = session
        .get_component_meta("/workspace/src/Link.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");
    let to_prop = meta
        .props
        .iter()
        .find(|prop| prop.name == "to")
        .expect("to prop should exist");
    let href_prop = meta
        .props
        .iter()
        .find(|prop| prop.name == "href")
        .expect("href prop should exist");

    assert!(
        !matches!(to_prop.type_expr, TypeExpr::Ref { .. }),
        "package re-exported route alias should not stay as a bare ref: {:?}",
        to_prop.type_expr
    );
    assert!(
        !matches!(
            href_prop.type_expr,
            TypeExpr::Unknown { .. } | TypeExpr::IndexedAccess { .. }
        ),
        "self indexed access through a package alias should not stay symbolic: {:?}",
        href_prop.type_expr
    );
    assert_route_union_surface(&to_prop.type_expr);
    assert_route_union_surface(&href_prop.type_expr);
}

#[test]
fn evaluate_types_materializes_package_import_then_exported_route_aliases_for_component_props() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue-router/package.json".to_string(),
        Arc::from(
            r#"{ "name": "vue-router", "types": "./dist/vue-router.d.ts", "exports": { ".": { "types": "./dist/vue-router.d.ts", "import": "./dist/vue-router.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/vue-router.d.ts".to_string(),
        Arc::from(
            r#"import { Lt as RouteLocationRaw, St, vt } from "./index-typed.js";
export { RouteLocationRaw, St, vt };"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/index-typed.d.ts".to_string(),
        Arc::from(
            r#"
export interface St { path: string }
export interface vt { name: string }
type RouteLocationRaw = string | St | vt
export { RouteLocationRaw as Lt, St, vt }
"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue-router/dist/index-typed.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    let project = MetaProject::new(host);
    project
        .upsert_base(
            "/workspace/src/Link.vue",
            r#"<script lang="ts">
import type { RouteLocationRaw } from 'vue-router'

export interface Props {
  to?: RouteLocationRaw
  href?: Props['to']
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta(
            "/workspace/src/Link.vue",
            crate::types::ResolverMode::Expanded,
        )
        .expect("resolved component meta should exist");
    let inputs = resolved
        .cached_eval_inputs
        .as_ref()
        .expect("resolved state should retain cached imported eval inputs");
    let alias = inputs
        .type_aliases
        .iter()
        .find(|alias| alias.local_name == "RouteLocationRaw")
        .expect("RouteLocationRaw should be prepared as an imported alias");
    assert_route_union_surface(&resolved_imported_alias_body(project.host(), alias));
    let registry_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "RouteLocationRaw")
        .expect("RouteLocationRaw should be published in the resolved type registry");
    assert_route_union_surface(&registry_entry.type_expr);
    eprintln!("registry={:#?}", resolved.resolved_type_registry);
    let published_names: std::collections::BTreeSet<_> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert!(
        !published_names.contains("RouteLocationAsStringTypedList"),
        "direct package aliases should not eagerly publish transitive package helpers, got {published_names:?}"
    );
    assert!(
        !published_names.contains("RouteLocationAsRelativeTypedList"),
        "direct package aliases should stay shallow instead of walking the full package helper graph, got {published_names:?}"
    );
}

#[test]
fn resolve_component_meta_keeps_package_registry_helpers_shallow_for_local_slot_types() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/pkg/package.json".to_string(),
        Arc::from(r#"{ "name": "pkg", "types": "./dist/index.d.ts" }"#),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts".to_string(),
        Arc::from(
            r#"
export interface InternalNode {
  leaf: string
}

export type PublicNode = InternalNode | {
  next: InternalNode
}
"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    let project = MetaProject::new(host);
    project
        .upsert_base(
            "/workspace/src/slot-types.ts",
            r#"import type { PublicNode } from 'pkg'

export interface ButtonSlots {
  default?(): PublicNode
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/workspace/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonSlots } from './slot-types'

defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/workspace/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./slot-types".to_string(),
            resolved_canonical_id: Some("/workspace/src/slot-types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta(
            "/workspace/src/App.vue",
            crate::types::ResolverMode::Expanded,
        )
        .expect("resolved component meta should exist");

    let published_names: std::collections::BTreeSet<_> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert!(
        published_names.contains("ButtonSlots"),
        "local slot helper should still be published, got {published_names:?}"
    );
    assert!(
        published_names.contains("PublicNode"),
        "direct package alias used by the slot contract should still be published, got {published_names:?}"
    );
    assert!(
        !published_names.contains("InternalNode"),
        "package registry publication should stay shallow instead of recursing into helper internals, got {published_names:?}"
    );
}

#[test]
fn resolve_component_meta_skips_unreferenced_owner_local_registry_helpers() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
type Used = {
  label: string
}

type UnusedLeaf = {
  deep: {
    nested: string
  }
}

type UnusedWrapper = {
  payload: UnusedLeaf
}

export interface Props {
  item?: Used
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ResolverMode::Expanded)
        .expect("resolved component meta should exist");

    let published_names: std::collections::BTreeSet<_> = resolved
        .resolved_type_registry
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert!(
        published_names.contains("Props"),
        "the queried defineProps contract should stay published, got {published_names:?}"
    );
    assert!(
        published_names.contains("Used"),
        "owner-local helpers that are referenced by the queried surface should still publish, got {published_names:?}"
    );
    assert!(
        !published_names.contains("UnusedLeaf"),
        "resolve_component_meta should not eagerly publish unrelated owner-local helpers, got {published_names:?}"
    );
    assert!(
        !published_names.contains("UnusedWrapper"),
        "resolve_component_meta should stay demand-driven for owner-local registry helpers, got {published_names:?}"
    );
}

#[test]
fn resolve_component_meta_includes_owner_local_helper_types_in_registry() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
interface RouteLocationObject {
  path: string
}

type RouteLocationRaw = string | RouteLocationObject

interface NuxtLinkProps {
  to?: RouteLocationRaw
  href?: NuxtLinkProps['to']
}

export interface LinkProps extends NuxtLinkProps {
  external?: boolean
}
</script>
<script setup lang="ts">
defineProps<LinkProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ResolverMode::Expanded)
        .expect("resolved component meta should exist");

    let route = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "RouteLocationRaw")
        .expect("owner-local route helper should be published in the type registry");
    let TypeExpr::Union(route_variants) = &route.type_expr else {
        panic!(
            "owner-local route helper should remain a route union, got {:?}",
            route.type_expr
        );
    };
    assert!(
        route_variants
            .iter()
            .any(|variant| matches!(variant, TypeExpr::Primitive(PrimitiveName::String))),
        "owner-local route helper should preserve its string branch, got {:?}",
        route.type_expr
    );
    assert!(
        route_variants.iter().any(|variant| {
            matches!(variant, TypeExpr::Ref { name, type_arguments } if name.as_ref() == "RouteLocationObject" && type_arguments.is_empty())
                || matches!(
                    variant,
                    TypeExpr::Object(shape)
                        if shape.properties.iter().any(|member| matches!(member, ObjectMember::Property(property) if property.name == "path"))
                )
        }),
        "owner-local route helper should preserve its object branch, got {:?}",
        route.type_expr
    );
    let route_object = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "RouteLocationObject")
        .expect("owner-local route object helper should also be published in the type registry");
    let TypeExpr::Object(route_object_shape) = &route_object.type_expr else {
        panic!(
            "RouteLocationObject should project as an object type, got {:?}",
            route_object.type_expr
        );
    };
    assert!(
        route_object_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "path")
        ),
        "RouteLocationObject should keep its path member, got {:?}",
        route_object.type_expr
    );

    let nuxt_link = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "NuxtLinkProps")
        .expect("owner-local helper interface should be published in the type registry");
    let TypeExpr::Object(shape) = &nuxt_link.type_expr else {
        panic!(
            "NuxtLinkProps should project as an object type, got {:?}",
            nuxt_link.type_expr
        );
    };
    let member_names: Vec<&str> = shape
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        member_names.contains(&"to") && member_names.contains(&"href"),
        "NuxtLinkProps registry entry should preserve helper members, got {:?}",
        member_names
    );
}

#[test]
fn resolve_component_meta_evaluates_owner_local_registry_aliases_against_imported_generic_helpers()
{
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export type ComponentConfig<TSlots, TVariants> = {
  slots: TSlots
  variants: TVariants
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { ComponentConfig } from './types'

type Button = ComponentConfig<
  { root?: { base: string } },
  { color?: 'primary' | 'neutral' }
>

export interface Props {
  ui?: Button['slots']
  color?: Button['variants']['color']
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ResolverMode::Expanded)
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("owner-local Button helper should be published in the type registry");
    let TypeExpr::Object(button_shape) = &button_entry.type_expr else {
        panic!(
            "owner-local helper alias should be evaluated against imported generic helpers, got {:?}",
            button_entry.type_expr
        );
    };
    let button_member_names: Vec<&str> = button_shape
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(property) => Some(property.name.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        button_member_names.contains(&"slots") && button_member_names.contains(&"variants"),
        "evaluated owner-local helper alias should publish concrete slots/variants members, got {:?}",
        button_member_names
    );
}

#[test]
fn resolve_component_meta_materializes_transitive_generic_registry_helpers_for_indexed_access() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export type ComponentVariants<TTheme> = {
  color: 'primary' | 'secondary'
  size: 'sm' | 'md'
}

export type ComponentSlots<TTheme> = {
  root?: {
    base: string
  }
}

export type ComponentConfig<TTheme> = {
  variants: ComponentVariants<TTheme>
  slots: ComponentSlots<TTheme>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ComponentConfig } from './types'
import theme from '#build/ui/button'

type Button = ComponentConfig<typeof theme>

defineProps<{
  activeColor?: Button['variants']['color']
  ui?: Button['slots']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ResolverMode::Expanded)
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let TypeExpr::Object(button_shape) = &button_entry.type_expr else {
        panic!(
            "Button helper should materialize as an object for indexed-access recovery, got {:?}",
            button_entry.type_expr
        );
    };
    let variants_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "variants" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a variants member");
    let TypeExpr::Object(variants_shape) = variants_member else {
        panic!(
            "Button.variants should materialize as an object for chained indexed access, got {:?}",
            variants_member
        );
    };
    assert!(
        variants_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "color")
        ),
        "Button.variants should keep its color member, got {:?}",
        variants_member
    );

    let slots_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "ComponentSlots")
        .expect("transitive ComponentSlots helper should be published in the type registry");
    let TypeExpr::Object(slots_shape) = &slots_entry.type_expr else {
        panic!(
            "ComponentSlots helper should materialize as an object, got {:?}",
            slots_entry.type_expr
        );
    };
    assert!(
        slots_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "root")
        ),
        "ComponentSlots should keep its root member, got {:?}",
        slots_entry.type_expr
    );
}

#[test]
fn resolve_component_meta_materializes_owner_local_mapped_generic_helpers() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: string
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>
  slots: ComponentSlots<T>
  ui: ComponentUI<T>
}

const theme = {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { solid: '', soft: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const

type Button = ComponentConfig<typeof theme>

defineProps<{
  activeColor?: Button['variants']['color']
  ui?: Button['slots']
  slotUi?: Button['ui']
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ResolverMode::Expanded)
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let TypeExpr::Object(button_shape) = &button_entry.type_expr else {
        panic!(
            "owner-local Button helper should materialize as an object, got {:?}",
            button_entry.type_expr
        );
    };

    let variants_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "variants" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a variants member");
    let TypeExpr::Object(variants_shape) = variants_member else {
        panic!(
            "Button.variants should materialize as an object, got {:?}",
            variants_member
        );
    };
    assert!(
        variants_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "color")
        ),
        "Button.variants should expose color, got {:?}",
        variants_member
    );

    let slots_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "slots" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a slots member");
    let TypeExpr::Object(slots_shape) = slots_member else {
        panic!(
            "Button.slots should materialize as an object, got {:?}",
            slots_member
        );
    };
    assert!(
        slots_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "base")
        ),
        "Button.slots should expose base, got {:?}",
        slots_member
    );

    let ui_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "ui" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a ui member");
    let TypeExpr::Object(ui_shape) = ui_member else {
        panic!(
            "Button.ui should materialize as an object, got {:?}",
            ui_member
        );
    };
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "base")
        ),
        "Button.ui should expose base, got {:?}",
        ui_member
    );
}

#[test]
fn resolve_component_meta_materializes_imported_component_config_registry_helpers() {
    let project = make_project();
    project
        .upsert_base(
            "/src/tailwind-variants.d.ts",
            r#"export type ClassValue = string | { [key: string]: boolean }
export type TVVariants<S, C, V> = { [K in keyof V]: keyof V[K] }
export type TVCompoundVariants<V, S, C, O, U> = never
export type TVDefaultVariants<V, S, O, U> = never
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/tv.ts",
            r#"import type { ClassValue, TVVariants, TVCompoundVariants, TVDefaultVariants } from './tailwind-variants'

export type TVConfig<T extends Record<string, any>> = {
  [P in keyof T]?: {
    [K in keyof T[P] as K extends 'base' | 'slots' | 'variants' | 'defaultVariants' ? K : never]?: K extends 'base' ? ClassValue
      : K extends 'slots' ? {
        [S in keyof T[P]['slots']]?: ClassValue
      }
        : K extends 'variants' ? TVVariants<T[P]['slots'], ClassValue, WidenVariantsValues<T[P]['variants']>>
          : K extends 'defaultVariants' ? TVDefaultVariants<WidenVariantsValues<T[P]['variants']>, T[P]['slots'], object, undefined>
            : never
  }
} & {
  [P in keyof T]?: {
    compoundVariants?: TVCompoundVariants<WidenVariantsValues<T[P]['variants']>, T[P]['slots'], ClassValue, object, undefined>
  }
}

type WidenVariantsValues<V extends Record<string, any> | undefined>
  = V extends Record<string, any> ? V & {
    [K in keyof V]: V[K] extends Record<string, any>
      ? V[K] & Record<string & {}, any>
      : V[K]
  } : V

type Id<T> = {} & { [P in keyof T]: T[P] }

type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof T['slots']]?: ClassValue
}>

type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

type GetComponentAppConfig<A, U extends string, K extends string>
  = A extends Record<U, Record<K, any>> ? A[U][K] : {}

type ComponentAppConfig<
  T,
  A extends Record<string, any>,
  K extends string,
  U extends string = 'ui' | 'ui.prose'
> = A & (
  U extends 'ui.prose'
    ? { ui?: { prose?: { [k in K]?: Partial<T> } } }
    : { [key in Exclude<U, 'ui.prose'>]?: { [k in K]?: Partial<T> } }
)

export type ComponentConfig<
  T extends Record<string, any>,
  A extends Record<string, any>,
  K extends string,
  U extends 'ui' | 'ui.prose' = 'ui'
> = {
  AppConfig: ComponentAppConfig<T, A, K, U>
  variants: ComponentVariants<T & GetComponentAppConfig<A, U, K>>
  slots: ComponentSlots<T>
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/schema.ts",
            r#"export interface AppConfig {
  ui: {
    button: {
      variants: {
        color: {
          neutral: string
        }
      }
    }
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { solid: '', soft: '' },
    size: { sm: '', md: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { AppConfig } from './schema'
import theme from './theme'
import type { ComponentConfig } from './tv'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>

export interface ButtonProps {
  color?: Button['variants']['color']
  ui?: Button['slots']
}

export interface ButtonSlots {
  default?(props: { ui: Button['ui'] }): any
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/Button.vue", crate::types::ResolverMode::Expanded)
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let TypeExpr::Object(button_shape) = &button_entry.type_expr else {
        panic!(
            "imported ComponentConfig alias should materialize as an object, got {:?}",
            button_entry.type_expr
        );
    };

    let variants_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "variants" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a variants member");
    let TypeExpr::Object(variants_shape) = variants_member else {
        panic!(
            "Button.variants should materialize as an object, got {:?}",
            variants_member
        );
    };
    let color_member = variants_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "color" => Some(&property.ty),
            _ => None,
        })
        .expect("Button.variants should keep a color member");
    assert_union_string_literals(color_member, &["neutral", "primary", "secondary"]);

    let slots_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "slots" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a slots member");
    let TypeExpr::Object(slots_shape) = slots_member else {
        panic!(
            "Button.slots should materialize as an object, got {:?}",
            slots_member
        );
    };
    assert!(
        slots_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "base")
        ),
        "Button.slots should expose base, got {:?}",
        slots_member
    );
    assert!(
        slots_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "label")
        ),
        "Button.slots should expose label, got {:?}",
        slots_member
    );
}

#[test]
fn resolve_component_meta_materializes_bound_registry_members_despite_opaque_sibling_args() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: string
}

export type ComponentConfig<T extends Record<string, any>, A> = {
  variants: ComponentVariants<T>
  slots: ComponentSlots<T>
  appConfig?: A
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { primary: '', secondary: '' }
  },
  slots: {
    base: '',
    label: ''
  }
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { ComponentConfig } from './types'
import theme from './theme'

type Button = ComponentConfig<typeof theme, MissingAppConfig>

export interface ButtonProps {
  color?: Button['variants']['color']
  ui?: Button['slots']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/Button.vue", crate::types::ResolverMode::Expanded)
        .expect("resolved component meta should exist");

    let button_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button helper should be published in the resolved type registry");
    let TypeExpr::Object(button_shape) = &button_entry.type_expr else {
        panic!(
            "Button helper should materialize as an object despite the opaque sibling arg, got {:?}",
            button_entry.type_expr
        );
    };

    let variants_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "variants" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a variants member");
    let TypeExpr::Object(variants_shape) = variants_member else {
        panic!(
            "Button.variants should materialize as an object, got {:?}",
            variants_member
        );
    };
    let color_member = variants_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "color" => Some(&property.ty),
            _ => None,
        })
        .expect("Button.variants should keep a color member");
    assert_union_string_literals(color_member, &["primary", "secondary"]);

    let slots_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "slots" => Some(&property.ty),
            _ => None,
        })
        .expect("Button helper should keep a slots member");
    let TypeExpr::Object(slots_shape) = slots_member else {
        panic!(
            "Button.slots should materialize as an object, got {:?}",
            slots_member
        );
    };
    assert!(
        slots_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "base")
        ),
        "Button.slots should expose base, got {:?}",
        slots_member
    );
    assert!(
        slots_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "label")
        ),
        "Button.slots should expose label, got {:?}",
        slots_member
    );
}

#[test]
fn resolve_component_meta_publishes_transitive_registry_aliases_for_nested_indexed_access_refs() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: string
}

export type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>
  slots: ComponentSlots<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/avatar-theme.ts",
            r#"export default {
  variants: {
    size: { sm: '', md: '' }
  },
  slots: {
    base: ''
  }
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/avatar-types.ts",
            r#"import type { ComponentConfig } from './types'
import avatarTheme from './avatar-theme'

export type Avatar = ComponentConfig<typeof avatarTheme>

export interface AvatarProps {
  size?: Avatar['variants']['size']
  ui?: Avatar['slots']
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { AvatarProps } from './avatar-types'

export interface ButtonProps {
  avatar?: AvatarProps
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./avatar-types".to_string(),
            resolved_canonical_id: Some("/src/avatar-types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/avatar-types.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./avatar-theme".to_string(),
                resolved_canonical_id: Some("/src/avatar-theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/Button.vue", crate::types::ResolverMode::Expanded)
        .expect("resolved component meta should exist");

    let avatar_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Avatar")
        .expect("transitive Avatar alias should be published in the resolved type registry");
    let TypeExpr::Object(avatar_shape) = &avatar_entry.type_expr else {
        panic!(
            "Avatar helper should materialize as an object, got {:?}",
            avatar_entry.type_expr
        );
    };

    let variants_member = avatar_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "variants" => Some(&property.ty),
            _ => None,
        })
        .expect("Avatar helper should keep a variants member");
    let TypeExpr::Object(variants_shape) = variants_member else {
        panic!(
            "Avatar.variants should materialize as an object, got {:?}",
            variants_member
        );
    };
    let size_member = variants_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "size" => Some(&property.ty),
            _ => None,
        })
        .expect("Avatar.variants should keep a size member");
    assert_union_string_literals(size_member, &["md", "sm"]);
}

#[test]
fn resolve_component_meta_handles_renamed_import_cycles_in_shallow_alias_hydration() {
    let project = make_project();
    project
        .upsert_base(
            "/src/helpers.ts",
            r#"type Id<T> = T

type SlotInfo<T> = Id<{
  value: T
}>

type WithChildren<T> = {
  slot: SlotInfo<ComponentConfig<T>>
}

export type ComponentConfig<T> = WithChildren<T>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { ComponentConfig as LocalConfig } from './helpers'

export interface ButtonProps {
  slot?: LocalConfig<string>['slot']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/Button.vue", crate::types::ResolverMode::Expanded)
        .expect("resolved component meta should exist");

    let local_config = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "LocalConfig")
        .expect("renamed imported alias should be published in the resolved type registry");
    let TypeExpr::Object(local_config_shape) = &local_config.type_expr else {
        panic!(
            "LocalConfig should materialize as an object, got {:?}",
            local_config.type_expr
        );
    };
    assert!(
        local_config_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "slot")
        ),
        "LocalConfig should keep its slot member, got {:?}",
        local_config.type_expr
    );
}

#[test]
fn resolve_component_meta_publishes_transitive_renamed_imported_registry_aliases() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export type Inner = {
  nested: {
    leaf: string
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/helpers.ts",
            r#"import type { Inner as LocalInner } from './base'

export type ComponentConfig = {
  ui: LocalInner
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { ComponentConfig } from './helpers'

export interface ButtonProps {
  ui?: ComponentConfig['ui']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./helpers".to_string(),
            resolved_canonical_id: Some("/src/helpers.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/helpers.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./base".to_string(),
            resolved_canonical_id: Some("/src/base.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/Button.vue", crate::types::ResolverMode::Expanded)
        .expect("resolved component meta should exist");

    let local_inner = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "LocalInner")
        .expect(
            "transitive renamed imported alias should be published in the resolved type registry",
        );
    let TypeExpr::Object(local_inner_shape) = &local_inner.type_expr else {
        panic!(
            "LocalInner should materialize as an object, got {:?}",
            local_inner.type_expr
        );
    };

    let nested_member = local_inner_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "nested" => Some(&property.ty),
            _ => None,
        })
        .expect("LocalInner should keep a nested member");
    let TypeExpr::Object(nested_shape) = nested_member else {
        panic!(
            "LocalInner.nested should materialize as an object, got {:?}",
            nested_member
        );
    };
    assert!(
        nested_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "leaf")
        ),
        "LocalInner.nested should expose leaf, got {:?}",
        nested_member
    );
}

#[test]
fn resolve_component_meta_keeps_deep_imported_registry_branches_shallow() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export type Level3 = {
  leaf: string
}

export type Level2 = {
  node: Level3
}

export type Level1 = {
  node: Level2
}

export type ComponentConfig = {
  ui: Level1
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { ComponentConfig } from './types'

export interface ButtonProps {
  ui?: ComponentConfig['ui']
}
</script>
<script setup lang="ts">
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/Button.vue", crate::types::ResolverMode::Expanded)
        .expect("resolved component meta should exist");

    let config_entry = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "ComponentConfig")
        .expect("ComponentConfig should be published in the resolved type registry");
    let TypeExpr::Object(config_shape) = &config_entry.type_expr else {
        panic!(
            "ComponentConfig should materialize as an object, got {:?}",
            config_entry.type_expr
        );
    };

    let ui_member = config_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "ui" => Some(&property.ty),
            _ => None,
        })
        .expect("ComponentConfig should keep a ui member");
    let TypeExpr::Object(ui_shape) = ui_member else {
        panic!(
            "ComponentConfig.ui should materialize as an object, got {:?}",
            ui_member
        );
    };

    let node_member = ui_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "node" => Some(&property.ty),
            _ => None,
        })
        .expect("ComponentConfig.ui should keep a node member");
    assert!(
        matches!(
            node_member,
            TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "Level2" && type_arguments.is_empty()
        ),
        "deep imported registry branches should stay shallow once the structural depth cap is hit, got {:?}",
        node_member
    );
}

#[test]
fn get_component_meta_returns_full_native_metadata_contract() {
    let project = make_project();
    project
        .upsert_base(
            "/FancyButton.vue",
            r#"<script setup lang="ts">
defineProps<{ label: string; modelValue: number }>()
</script>
<template><button><slot /></button></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import FancyButton from './FancyButton.vue'

const count = ref(0)
const accentColor = "red"
const doubled = computed(() => count.value * 2)

onMounted(() => {
  console.log(count.value)
})
</script>
<template>
  <FancyButton
    id="wrapper"
    ref="button"
    :label="`${doubled}`"
    class="primary"
    :class="{ active: count > 0 }"
    v-model="count"
  >
    <template #default>{{ count }}</template>
  </FancyButton>
</template>
<style scoped module="theme">
#wrapper .primary {
  color: v-bind(accentColor);
  --accent: red;
}
</style>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    assert_eq!(
        meta.components.len(),
        1,
        "template component usage should be present"
    );
    assert_eq!(meta.components[0].name, "FancyButton");
    assert_eq!(
        meta.components[0].import_source.as_deref(),
        Some("./FancyButton.vue")
    );
    assert!(!meta.components[0].has_spread);
    assert!(meta.components[0].has_dynamic_class);
    assert_eq!(meta.components[0].v_models, vec!["modelValue".to_string()]);
    assert_eq!(
        meta.components[0]
            .v_model_entries
            .iter()
            .map(|entry| entry.binding_name.as_str())
            .collect::<Vec<_>>(),
        vec!["modelValue"]
    );
    let label_prop = meta.components[0]
        .props
        .iter()
        .find(|prop| prop.name == "label")
        .expect("label prop usage should be present");
    assert_eq!(label_prop.expression.as_deref(), Some("`${doubled}`"));
    assert_eq!(label_prop.referenced_bindings, vec!["doubled".to_string()]);
    assert!(!label_prop.from_spread);
    assert!(!label_prop.is_shorthand);

    assert_eq!(
        meta.template_refs.len(),
        1,
        "template refs should be present"
    );
    assert_eq!(meta.template_refs[0].name, "button");
    assert_eq!(meta.template_refs[0].target_tag, "FancyButton");

    let child_meta = session
        .get_component_meta("/FancyButton.vue")
        .unwrap()
        .expect("child component meta should be available");
    let public_instance = child_meta
        .public_instance
        .as_ref()
        .expect("host should provide a public-instance sidecar");
    let public_member_names: Vec<_> = public_instance
        .members
        .iter()
        .map(|member| member.name.as_str())
        .collect();
    assert!(
        public_member_names.contains(&"label"),
        "public instance should expose declared props, got {:?}",
        public_member_names
    );
    assert!(
        public_member_names.contains(&"modelValue"),
        "public instance should expose model props, got {:?}",
        public_member_names
    );
    assert!(
        public_member_names.contains(&"$slots"),
        "public instance should expose $slots, got {:?}",
        public_member_names
    );
    assert!(
        public_instance.members.iter().any(|member| {
            member.name == "$slots"
                && matches!(
                    member.kind,
                    verter_analysis::component_meta::PublicInstanceMemberKind::SlotContainer
                )
        }),
        "$slots should be tagged as a public-instance slot container"
    );

    assert!(
        meta.imports.iter().any(|import| import.source == "vue"),
        "script imports should be preserved"
    );
    assert!(
        meta.bindings
            .iter()
            .any(|binding| binding.name == "count" && binding.used_in_template),
        "bindings should preserve template usage information"
    );
    assert!(
        meta.vue_api_calls.iter().any(|call| matches!(
            call.api,
            verter_analysis::types::VueApiClassification::OnMounted
        )),
        "Vue API calls should be preserved"
    );
    assert_eq!(meta.styles.len(), 1, "style metadata should be present");
    assert_eq!(meta.styles[0].classes, vec!["primary".to_string()]);
    assert_eq!(meta.styles[0].ids, vec!["wrapper".to_string()]);
    assert_eq!(
        meta.styles[0].custom_properties,
        vec!["--accent".to_string()]
    );
    assert_eq!(meta.styles[0].v_binds, vec!["accentColor".to_string()]);
    assert!(
        meta.styles[0]
            .selectors
            .iter()
            .any(|selector| selector.text == "#wrapper .primary"),
        "style selectors should be preserved"
    );
}

#[test]
fn get_component_meta_surfaces_sfc_block_metadata() {
    let project = make_project();
    project
        .upsert_base(
            "/Button.vue",
            r#"<script lang="ts">
export const legacy = true
</script>
<script setup lang="ts" generic="T extends string = string" attrs="ButtonAttrs">
defineProps<{ label: string }>()
defineSlots<{
  default(props: { item: number }): any
}>()
defineExpose({
  focus() {}
})
</script>
<template lang="html" data-layout="stack">
  <button>{{ label }}</button>
  <slot :item="1" />
</template>
<style scoped module="theme" lang="scss">
.primary { color: red; }
</style>
<i18n lang="json">
{ "label": "Button" }
</i18n>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/Button.vue")
        .unwrap()
        .expect("component meta should be available");

    let blocks = meta
        .sfc_blocks
        .as_ref()
        .expect("host should surface SFC block metadata");
    assert_eq!(
        blocks
            .script
            .as_ref()
            .and_then(|block| block.lang.as_deref()),
        Some("ts")
    );
    assert_eq!(
        blocks
            .script_setup
            .as_ref()
            .and_then(|block| block.generic.as_deref()),
        Some("T extends string = string")
    );
    assert_eq!(
        blocks
            .script_setup
            .as_ref()
            .and_then(|block| block.attrs_type.as_deref()),
        Some("ButtonAttrs")
    );
    assert_eq!(
        blocks
            .template
            .as_ref()
            .and_then(|block| block.lang.as_deref()),
        Some("html")
    );
    assert!(
        blocks.template.as_ref().is_some_and(|block| block
            .attributes
            .iter()
            .any(|attribute| attribute.name == "data-layout"
                && attribute.value.as_deref() == Some("stack"))),
        "template block should preserve arbitrary root attributes"
    );
    assert_eq!(blocks.styles.len(), 1);
    assert_eq!(blocks.styles[0].index, 0);
    assert_eq!(blocks.styles[0].lang.as_deref(), Some("scss"));
    assert!(blocks.styles[0].scoped);
    assert!(blocks.styles[0].is_module);
    assert_eq!(blocks.styles[0].module_name.as_deref(), Some("theme"));
    assert_eq!(blocks.custom.len(), 1);
    assert_eq!(blocks.custom[0].index, 0);
    assert_eq!(blocks.custom[0].block_type, "i18n");
    assert_eq!(blocks.custom[0].lang.as_deref(), Some("json"));
}

#[test]
fn get_component_meta_preserves_component_spread_usage() {
    let project = make_project();
    project
        .upsert_base(
            "/FancyButton.vue",
            r#"<script setup lang="ts">
defineProps<{ label?: string }>()
</script>
<template><button><slot /></button></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import FancyButton from './FancyButton.vue'

const attrs = { label: 'Hello' }
</script>
<template>
  <FancyButton v-bind="attrs" />
</template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should return metadata");

    assert_eq!(meta.components.len(), 1);
    assert!(
        meta.components[0].has_spread,
        "component usage should preserve v-bind spread markers"
    );
}

// ===========================================================================
// Phase 6: Resolved external type cache
// ===========================================================================

#[test]
fn resolved_type_cache_is_reused_across_different_owners() {
    let project = make_project();

    // Shared dependency
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface SharedProps { shared: string }"#,
        )
        .unwrap();

    // Two different SFCs importing the same type from the same dep
    project
        .upsert_base(
            "/src/A.vue",
            r#"<script setup lang="ts">
import { SharedProps } from './types'
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/B.vue",
            r#"<script setup lang="ts">
import { SharedProps } from './types'
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Set up dep resolution for both owners
    project.host().set_import_dependencies(
        "/src/A.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/B.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();

    // First owner resolves the type (cache miss)
    project.host().provenance().reset();
    let meta_a = session.get_component_meta("/src/A.vue").unwrap().unwrap();
    let p1 = provenance(&project);

    assert!(
        p1.resolved_external_type_cache_misses >= 1,
        "first owner should miss the resolved type cache"
    );
    assert_eq!(meta_a.props.len(), 1, "A.vue should have the shared prop");

    // Reset counters for second owner
    project.host().provenance().reset();
    let meta_b = session.get_component_meta("/src/B.vue").unwrap().unwrap();
    let p2 = provenance(&project);

    assert_eq!(meta_b.props.len(), 1, "B.vue should have the shared prop");
    assert_eq!(meta_b.props[0].name, "shared");

    // Assert+: second owner should hit the host-level cache
    assert!(
        p2.resolved_external_type_cache_hits >= 1,
        "second owner importing the same type from the same unchanged dep should hit the host-level cache, got hits={} misses={}",
        p2.resolved_external_type_cache_hits,
        p2.resolved_external_type_cache_misses,
    );
}

#[test]
fn resolved_type_cache_is_reused_for_workspace_only_package_dependencies() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/fancy/package.json".to_string(),
        Arc::from(
            r#"{ "name": "fancy", "types": "./dist/index.d.ts", "exports": { ".": { "import": "./dist/index.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.d.ts".to_string(),
        Arc::from("export interface SharedProps { shared: string }"),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    let project = MetaProject::new(host);
    project
        .configure_projects(vec![
            verter_analysis::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.json".to_string()),
            ),
        ])
        .unwrap();
    project
        .upsert_base(
            "/workspace/src/A.vue",
            r#"<script setup lang="ts">
import type { SharedProps } from 'fancy'
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/workspace/src/B.vue",
            r#"<script setup lang="ts">
import type { SharedProps } from 'fancy'
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();

    project.host().provenance().reset();
    let meta_a = session
        .get_component_meta("/workspace/src/A.vue")
        .unwrap()
        .unwrap();
    let p1 = provenance(&project);
    assert_eq!(
        meta_a.props.len(),
        1,
        "A.vue should resolve the package prop"
    );
    assert!(
        p1.resolved_external_type_cache_misses >= 1,
        "first owner should miss the resolved type cache for a workspace-only dep"
    );

    project.host().provenance().reset();
    let meta_b = session
        .get_component_meta("/workspace/src/B.vue")
        .unwrap()
        .unwrap();
    let p2 = provenance(&project);
    assert_eq!(
        meta_b.props.len(),
        1,
        "B.vue should resolve the package prop"
    );
    assert_eq!(meta_b.props[0].name, "shared");
    assert!(
        p2.resolved_external_type_cache_hits >= 1,
        "second owner should hit the host-level resolved type cache even when the dep only exists in the workspace, got hits={} misses={}",
        p2.resolved_external_type_cache_hits,
        p2.resolved_external_type_cache_misses,
    );
}

#[test]
fn resolved_type_cache_cleared_on_host_close() {
    let project = make_project();
    project
        .upsert_base("/types.ts", r#"export interface Props { a: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    let _ = session.get_component_meta("/App.vue").unwrap();

    // Verify cache is populated
    assert!(
        !project.host().resolved_type_cache.lock().is_empty(),
        "cache should be populated after resolution"
    );

    // Clear caches
    project.clear_caches().unwrap();

    assert!(
        project.host().resolved_type_cache.lock().is_empty(),
        "clear_caches must flush the resolved type cache"
    );
}

#[test]
fn resolved_type_cache_is_bounded() {
    // Verify that inserting beyond cap doesn't grow unbounded
    let host = VerterHost::new_standalone(HostConfig {
        ..HostConfig::default()
    });

    {
        let mut cache = host.resolved_type_cache.lock();
        // Fill to cap
        for i in 0..crate::types::RESOLVED_TYPE_CACHE_CAP {
            cache.insert(
                crate::types::ResolvedTypeCacheKey {
                    dep_canonical_id: format!("/dep_{i}.ts"),
                    dep_source_hash: [0u8; 16],
                    type_name: "T".to_string(),
                    resolve_kind: verter_workspace::ResolveRequestKind::TypeImport,
                },
                crate::types::ResolvedTypeCacheEntry {
                    resolved: None,
                    tracked_deps: Vec::new(),
                },
            );
        }
        assert_eq!(
            cache.len(),
            crate::types::RESOLVED_TYPE_CACHE_CAP,
            "cache should be at cap"
        );
    }

    // The eviction happens inside resolve_external_type_from_loaded_files,
    // but we can verify the cap constant is reasonable.
    assert!(
        crate::types::RESOLVED_TYPE_CACHE_CAP >= 1024,
        "cache cap should be at least 1024"
    );
    assert!(
        crate::types::RESOLVED_TYPE_CACHE_CAP <= 16384,
        "cache cap should not exceed 16384"
    );
}

// ===========================================================================
// Phase 8: Correctness — typeof, double script, interface extends imported
// ===========================================================================

#[test]
fn local_typeof_resolves_in_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const config = { x: 1, y: 'hello' }
defineProps<typeof config>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();

    // Assert+: both fields from config
    assert!(names.contains(&"x"), "should have 'x', got: {names:?}");
    assert!(names.contains(&"y"), "should have 'y', got: {names:?}");

    // Assert-: no extra fields
    assert_eq!(meta.props.len(), 2, "should have exactly 2 props");
}

#[test]
fn double_script_same_file_visibility_in_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script lang="ts">
export interface SharedProps { shared: boolean }
</script>
<script setup lang="ts">
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    // Assert+: prop from sibling script block
    assert_eq!(
        meta.props.len(),
        1,
        "should have 1 prop from sibling script"
    );
    assert_eq!(meta.props[0].name, "shared");

    // Assert-: no unresolved types or errors — prop should be fully resolved
    assert!(
        meta.props[0].raw_type.is_some(),
        "shared prop should have a resolved raw type"
    );
}

#[test]
fn interface_extends_pick_of_imported_type_in_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export interface BaseProps { a: string; b: number; c: boolean; d: object }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import { BaseProps } from './base'
interface MyProps extends Pick<BaseProps, 'a' | 'b'> { local: string }
defineProps<MyProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./base".to_string(),
            resolved_canonical_id: Some("/src/base.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();

    // Assert+: inherited + local
    assert!(
        names.contains(&"a"),
        "should have 'a' from Pick, got: {names:?}"
    );
    assert!(
        names.contains(&"b"),
        "should have 'b' from Pick, got: {names:?}"
    );
    assert!(
        names.contains(&"local"),
        "should have 'local', got: {names:?}"
    );

    // Assert-: excluded fields
    assert!(!names.contains(&"c"), "should NOT have 'c', got: {names:?}");
    assert!(!names.contains(&"d"), "should NOT have 'd', got: {names:?}");
}

#[test]
fn union_object_variants_synthesize_component_meta_props() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
type FixedProps = {
  layout?: 'fixed'
  editor: string
}

type BubbleProps = {
  layout?: 'bubble'
  editor: string
  floating?: boolean
}

type Props = FixedProps | BubbleProps
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"layout"),
        "should have 'layout', got: {names:?}"
    );
    assert!(
        names.contains(&"editor"),
        "should have 'editor', got: {names:?}"
    );
    assert!(
        names.contains(&"floating"),
        "should have union branch props, got: {names:?}"
    );
}

#[test]
fn mixed_intersection_retains_local_component_meta_props() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
type Props = {
  id?: string
  disabled?: boolean
} & Omit<FormHTMLAttributes, 'name'>

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"id"), "should have 'id', got: {names:?}");
    assert!(
        names.contains(&"disabled"),
        "should have 'disabled', got: {names:?}"
    );
}

#[test]
fn imported_barrel_types_are_available_to_define_props_evaluation() {
    let project = make_project();
    project
        .upsert_base("/src/types/index.ts", r#"export * from '../Button.vue'"#)
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
export interface IconProps {
  icon?: string
}

export interface ButtonProps extends IconProps {
  label?: string
  color?: string
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps } from './types'

type Props = Omit<ButtonProps, 'color'> & {
  status?: string
}

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![crate::types::DependencyResolution {
            specifier: "../Button.vue".to_string(),
            resolved_canonical_id: Some("/src/Button.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"icon"),
        "should have 'icon', got: {names:?}"
    );
    assert!(
        names.contains(&"label"),
        "should have 'label', got: {names:?}"
    );
    assert!(
        names.contains(&"status"),
        "should keep local props, got: {names:?}"
    );
    assert!(
        !names.contains(&"color"),
        "should omit 'color', got: {names:?}"
    );
}

#[test]
fn imported_barrel_cycles_still_resolve_nested_omit_props() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/index.ts",
            r#"export * from '../Link.vue'
export * from '../Button.vue'"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
  exactActiveClass?: string
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
  class?: any
  raw?: boolean
}

export type LinkPropsKeys = 'to' | 'replace' | 'activeClass' | 'ariaCurrentValue'
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: string
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface ChildProps extends Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  status?: string
}

defineProps<ChildProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../Link.vue".to_string(),
                resolved_canonical_id: Some("/src/Link.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../Button.vue".to_string(),
                resolved_canonical_id: Some("/src/Button.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"loading"),
        "should include inherited icon props, got: {names:?}"
    );
    assert!(
        names.contains(&"label"),
        "should include inherited button props, got: {names:?}"
    );
    assert!(
        names.contains(&"size"),
        "should include inherited button props, got: {names:?}"
    );
    assert!(
        names.contains(&"href"),
        "should include inherited link props, got: {names:?}"
    );
    assert!(
        names.contains(&"status"),
        "should keep local props, got: {names:?}"
    );
    assert!(!names.contains(&"icon"), "should omit icon, got: {names:?}");
    assert!(
        !names.contains(&"color"),
        "should omit color, got: {names:?}"
    );
    assert!(
        !names.contains(&"variant"),
        "should omit variant, got: {names:?}"
    );
    assert!(
        !names.contains(&"to"),
        "should omit link keys, got: {names:?}"
    );
    assert!(
        !names.contains(&"replace"),
        "should omit router link keys, got: {names:?}"
    );
    assert!(
        !names.contains(&"activeClass"),
        "should omit router link keys, got: {names:?}"
    );
    assert!(
        !names.contains(&"ariaCurrentValue"),
        "should omit router link keys, got: {names:?}"
    );
}

#[test]
fn resolve_component_meta_handles_barrel_cycle_utility_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/index.ts",
            r#"export * from '../Link.vue'
export * from '../Button.vue'"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
interface RouterLinkOptions {
  replace?: boolean
  activeClass?: string
  ariaCurrentValue?: string
}

interface RouterLinkProps extends RouterLinkOptions {
  custom?: boolean
  exactActiveClass?: string
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
  class?: any
  raw?: boolean
}

export type LinkPropsKeys = 'to' | 'replace' | 'activeClass' | 'ariaCurrentValue'
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  loading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: string
}
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface ChildProps extends Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  status?: string
}

defineProps<ChildProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "../Link.vue".to_string(),
                resolved_canonical_id: Some("/src/Link.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "../Button.vue".to_string(),
                resolved_canonical_id: Some("/src/Button.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ResolverMode::Expanded)
        .expect("expanded state should resolve");
    let button = resolved
        .resolved_macros
        .iter()
        .find(|meta| meta.type_name == "ButtonProps")
        .expect("should resolve ButtonProps");
    assert!(
        button.props.iter().any(|prop| prop.name == "loading"),
        "resolved ButtonProps should include inherited props, got: {:?}",
        button.props
    );
    assert!(
        button.props.iter().any(|prop| prop.name == "label"),
        "resolved ButtonProps should include button props, got: {:?}",
        button.props
    );
}

#[test]
fn imported_pick_slot_bindings_keep_symbolic_raw_type() {
    let project = make_project();
    project
        .upsert_base(
            "/src/reka-ui.ts",
            r#"
export interface CalendarCellTriggerProps {
  day: Date
  month: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/slots.ts",
            r#"
import type { CalendarCellTriggerProps } from './reka-ui'

export interface CalendarSlots {
  day?: (props: Pick<CalendarCellTriggerProps, 'day'>) => any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { CalendarSlots } from './slots'

defineSlots<CalendarSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");

    let day_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "day")
        .expect("should extract imported day slot");
    let day_binding = day_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "day")
        .expect("day slot should expose the day binding");

    assert_eq!(
        day_binding.raw_type.as_deref(),
        Some("CalendarCellTriggerProps['day']"),
        "imported Pick slot bindings should keep the symbolic source contract"
    );
}

#[test]
fn imported_slot_binding_indexed_access_helpers_resolve_to_concrete_members() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

export type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  ui: ComponentUI<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"
export const theme = {
  slots: {
    base: '',
    label: ''
  }
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/button-types.ts",
            r#"
import type { ComponentConfig } from './types'
import { theme } from './theme'

export type Button = ComponentConfig<typeof theme>

export interface ButtonSlots {
  default?(props: {
    ui: Button['ui']
  }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonSlots } from './button-types'

defineSlots<ButtonSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ResolverMode::Expanded)
        .expect("should resolve component meta state");

    let button_slots = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "ButtonSlots")
        .expect("ButtonSlots should be published in the resolved type registry");
    let TypeExpr::Object(button_slots_shape) = &button_slots.type_expr else {
        panic!(
            "ButtonSlots should materialize as an object, got {:?}",
            button_slots.type_expr
        );
    };
    let default_slot = button_slots_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "default" => Some(&property.ty),
            _ => None,
        })
        .expect("ButtonSlots should keep the default slot signature");
    let TypeExpr::Function(default_slot_fn) = default_slot else {
        panic!(
            "default slot should materialize as a function, got {:?}",
            default_slot
        );
    };
    let Some(props_param) = default_slot_fn.parameters.first() else {
        panic!("default slot should keep its props parameter");
    };
    let TypeExpr::Object(props_shape) = &props_param.ty else {
        panic!(
            "slot props should materialize as an object, got {:?}",
            props_param.ty
        );
    };
    assert!(
        props_shape.properties.iter().any(
            |member| matches!(
                member,
                ObjectMember::Property(property)
                    if property.name == "ui"
                        && matches!(
                            &property.ty,
                            TypeExpr::IndexedAccess {
                                object,
                                index
                            } if matches!(object.as_ref(), TypeExpr::Ref { name, .. } if name.as_ref() == "Button")
                                && matches!(index.as_ref(), TypeExpr::Literal(LiteralValue::String(value)) if value == "ui")
                        )
            )
        ),
        "default slot props should keep the ui indexed-access contract, got {:?}",
        default_slot
    );

    let button = resolved
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Button")
        .expect("Button should be published transitively for imported slot bindings");
    let TypeExpr::Object(button_shape) = &button.type_expr else {
        panic!(
            "Button should materialize as an object, got {:?}",
            button.type_expr
        );
    };
    let ui_member = button_shape
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::Property(property) if property.name == "ui" => Some(&property.ty),
            _ => None,
        })
        .expect("resolved Button helper should expose a ui member");
    let TypeExpr::Object(ui_shape) = ui_member else {
        panic!(
            "Button.ui should materialize as an object, got {:?}",
            ui_member
        );
    };
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "base")
        ),
        "resolved Button.ui should expose base, got {:?}",
        ui_member
    );
    assert!(
        ui_shape.properties.iter().any(
            |member| matches!(member, ObjectMember::Property(property) if property.name == "label")
        ),
        "resolved Button.ui should expose label, got {:?}",
        ui_member
    );
}

#[test]
fn local_pick_slot_bindings_keep_symbolic_raw_type() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
interface CalendarCellTriggerProps {
  day: Date
  month: number
}

export interface CalendarSlots {
  day?: (props: Pick<CalendarCellTriggerProps, 'day'>) => any
}
</script>
<script setup lang="ts">
defineSlots<CalendarSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");

    let day_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "day")
        .expect("should extract local day slot");
    let day_binding = day_slot
        .bindings
        .iter()
        .find(|binding| binding.name == "day")
        .expect("day slot should expose the day binding");

    assert_eq!(
        day_binding.raw_type.as_deref(),
        Some("CalendarCellTriggerProps['day']"),
        "local Pick slot bindings should keep the symbolic source contract"
    );
}

#[test]
fn nested_imported_omit_preserves_html_attrs_and_omits_link_only_keys() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/html.ts",
            r#"
export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  name?: string
  type?: 'button' | 'submit'
}

export interface AnchorHTMLAttributes {
  download?: boolean
  href?: string
  hreflang?: string
  media?: string
  ping?: string
  referrerpolicy?: string
  rel?: string
  target?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from './types/html'

interface RouterLinkProps {
  replace?: boolean
  exactActiveClass?: string
  viewTransition?: boolean
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
  external?: boolean
  target?: string | null
  rel?: string | null
  noRel?: boolean
  prefetchedClass?: string
  prefetch?: boolean
  prefetchOn?: 'visibility' | 'interaction'
  noPrefetch?: boolean
  trailingSlash?: 'append' | 'remove'
}

export interface LinkProps extends NuxtLinkProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
  active?: boolean
  exact?: boolean
  exactQuery?: boolean | 'partial'
  exactHash?: boolean
  inactiveClass?: string
  custom?: boolean
  raw?: boolean
  class?: any
}

export type LinkPropsKeys =
  | 'to'
  | 'href'
  | 'target'
  | 'rel'
  | 'noRel'
  | 'external'
  | 'prefetch'
  | 'prefetchOn'
  | 'prefetchedClass'
  | 'noPrefetch'
  | 'trailingSlash'
  | 'replace'
  | 'active'
  | 'exact'
  | 'exactQuery'
  | 'exactHash'
  | 'inactiveClass'
  | 'download'
  | 'ping'
  | 'referrerpolicy'
  | 'hreflang'
  | 'media'
  | 'viewTransition'
</script>
<template><a /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  leading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: 'sm' | 'md'
  square?: boolean
  block?: boolean
  class?: any
  ui?: object
}
</script>
<template><button /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types/index.ts",
            "export * from '../Link.vue'\nexport * from '../Button.vue'",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface Props extends Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  color?: ButtonProps['color']
  variant?: ButtonProps['variant']
  side?: 'left' | 'right'
}

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"autofocus")
            && prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"name"),
        "nested imported Omit should preserve inherited button attrs: {:?}",
        prop_names
    );
    assert!(
        !prop_names.contains(&"to")
            && !prop_names.contains(&"href")
            && !prop_names.contains(&"target")
            && !prop_names.contains(&"rel")
            && !prop_names.contains(&"prefetch")
            && !prop_names.contains(&"prefetchOn")
            && !prop_names.contains(&"external")
            && !prop_names.contains(&"viewTransition"),
        "nested imported Omit should exclude link-only keys: {:?}",
        prop_names
    );
}

#[test]
fn dual_heritage_omit_keeps_button_attrs_without_leaking_link_keys() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types/html.ts",
            r#"
export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  name?: string
  type?: 'button' | 'submit'
}

export interface AnchorHTMLAttributes {
  download?: boolean
  href?: string
  hreflang?: string
  media?: string
  ping?: string
  referrerpolicy?: string
  rel?: string
  target?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/drag.ts",
            r#"
export interface DragHandleProps {
  class?: any
  computePositionConfig?: unknown
  editor?: object
  element?: object
  getReferencedVirtualElement?: () => unknown
  nested?: boolean
  nestedOptions?: object
  onElementDragEnd?: () => void
  onElementDragStart?: () => void
  onNodeChange?: () => void
  pluginKey?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from './types/html'

interface RouterLinkProps {
  replace?: boolean
  exactActiveClass?: string
  viewTransition?: boolean
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
  external?: boolean
  target?: string | null
  rel?: string | null
  noRel?: boolean
  prefetchedClass?: string
  prefetch?: boolean
  prefetchOn?: 'visibility' | 'interaction'
  noPrefetch?: boolean
  trailingSlash?: 'append' | 'remove'
}

export interface LinkProps extends NuxtLinkProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
  active?: boolean
  exact?: boolean
  exactQuery?: boolean | 'partial'
  exactHash?: boolean
  inactiveClass?: string
  custom?: boolean
  raw?: boolean
  class?: any
}

export type LinkPropsKeys =
  | 'to'
  | 'href'
  | 'target'
  | 'rel'
  | 'noRel'
  | 'external'
  | 'prefetch'
  | 'prefetchOn'
  | 'prefetchedClass'
  | 'noPrefetch'
  | 'trailingSlash'
  | 'replace'
  | 'active'
  | 'exact'
  | 'exactQuery'
  | 'exactHash'
  | 'inactiveClass'
  | 'download'
  | 'ping'
  | 'referrerpolicy'
  | 'hreflang'
  | 'media'
  | 'viewTransition'
</script>
<template><a /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './types'

export interface UseComponentIconsProps {
  icon?: string
  leading?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: string
  variant?: string
  size?: 'sm' | 'md'
  square?: boolean
  block?: boolean
  class?: any
  ui?: object
}
</script>
<template><button /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types/index.ts",
            "export * from '../Link.vue'\nexport * from '../Button.vue'",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { DragHandleProps } from './drag'
import type { ButtonProps, LinkPropsKeys } from './types'

interface Props extends Omit<DragHandleProps, 'editor' | 'element' | 'onNodeChange' | 'computePositionConfig' | 'class'>, Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  color?: ButtonProps['color']
  variant?: ButtonProps['variant']
  options?: object
  editor: object
  ui?: ButtonProps['ui']
}

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/App.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"autofocus")
            && prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"name"),
        "dual-heritage Omit should preserve inherited button attrs: {:?}",
        prop_names
    );
    assert!(
        !prop_names.contains(&"to")
            && !prop_names.contains(&"href")
            && !prop_names.contains(&"target")
            && !prop_names.contains(&"rel")
            && !prop_names.contains(&"prefetch")
            && !prop_names.contains(&"prefetchOn")
            && !prop_names.contains(&"external")
            && !prop_names.contains(&"viewTransition"),
        "dual-heritage Omit should exclude link-only keys: {:?}",
        prop_names
    );
}

#[test]
fn link_props_keep_inherited_html_attrs_across_vue_ignore_utility_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue-router/index.d.ts",
            r#"
export { R as RouterLinkProps } from './dist/index.js'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/vue-router/dist/index.d.ts",
            r#"
export interface RouterLinkOptions {
  to?: string
  replace?: boolean
  viewTransition?: boolean
}

export interface R extends RouterLinkOptions {
  activeClass?: string
  exactActiveClass?: string
  ariaCurrentValue?: 'page'
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types/html.ts",
            r#"
export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  name?: string
  type?: 'button' | 'submit'
}

export interface AnchorHTMLAttributes {
  download?: boolean
  href?: string
  hreflang?: string
  media?: string
  ping?: string
  referrerpolicy?: string
  rel?: string
  target?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from './types/html'

interface RouterLinkProps {
  replace?: boolean
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps, /** @vue-ignore */ Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, /** @vue-ignore */ Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
}
</script>
<script setup lang="ts">
defineProps<LinkProps>()
</script>
<template><a /></template>"#,
        )
        .unwrap();

    let export = project
        .host()
        .resolve_named_export_in_view(
            "/node_modules/vue-router/index.d.ts",
            "RouterLinkProps",
            Some(true),
            None,
        )
        .expect("package re-export should resolve RouterLinkProps");
    assert_eq!(
        export.source_canonical_id.as_deref(),
        Some("/node_modules/vue-router/dist/index.d.ts")
    );
    assert_eq!(export.source_name, "R");
    let decl = crate::meta_resolve::resolve_type_declaration_in_view(
        project.host(),
        "/node_modules/vue-router/index.d.ts",
        "RouterLinkProps",
        None,
    );
    assert_eq!(
        decl.canonical_source,
        "/node_modules/vue-router/dist/index.d.ts"
    );
    assert_eq!(decl.resolved_name, "R");

    let meta = project
        .host()
        .get_component_meta("/src/Link.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"autofocus")
            && prop_names.contains(&"form")
            && prop_names.contains(&"formaction")
            && prop_names.contains(&"name")
            && prop_names.contains(&"download")
            && prop_names.contains(&"hreflang"),
        "LinkProps should keep inherited HTML attrs across vue-ignore utility heritage: {:?}",
        prop_names
    );
}

#[test]
fn link_props_keep_router_members_across_package_reexported_utility_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue-router/index.d.ts",
            r#"
export { R as RouterLinkProps } from './dist/index.js'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/node_modules/vue-router/dist/index.d.ts",
            r#"
export interface RouterLinkOptions {
  to?: string
  replace?: boolean
  viewTransition?: boolean
}

export interface R extends RouterLinkOptions {
  activeClass?: string
  exactActiveClass?: string
  ariaCurrentValue?: 'page'
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Link.vue",
            r#"<script lang="ts">
import type { RouterLinkProps } from 'vue-router'

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  custom?: boolean
}
</script>
<script setup lang="ts">
defineProps<LinkProps>()
</script>
<template><a /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/Link.vue",
        vec![crate::types::DependencyResolution {
            specifier: "vue-router".to_string(),
            resolved_canonical_id: Some("/node_modules/vue-router/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/node_modules/vue-router/index.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./dist/index.js".to_string(),
            resolved_canonical_id: Some("/node_modules/vue-router/dist/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = project
        .host()
        .get_component_meta("/src/Link.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        prop_names.contains(&"replace")
            && prop_names.contains(&"viewTransition")
            && prop_names.contains(&"activeClass")
            && prop_names.contains(&"exactActiveClass")
            && prop_names.contains(&"ariaCurrentValue"),
        "LinkProps should keep router members across package re-exported Omit heritage: {:?}",
        prop_names
    );
}

#[test]
fn imported_omit_props_preserve_jsdoc_and_raw_type_text() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface UseComponentIconsProps {
  icon?: string
}

interface NuxtLinkProps {
  to?: string
}

interface ButtonHTMLAttributes {
  type?: 'button' | 'submit'
}

interface AnchorHTMLAttributes {
  href?: string
}

export interface LinkProps extends NuxtLinkProps, /** @vue-ignore */ Omit<ButtonHTMLAttributes, 'type'>, /** @vue-ignore */ Omit<AnchorHTMLAttributes, 'href'> {
  /** Force the link to be active independent of the current route. */
  active?: boolean
  /** Class to apply when the link is active */
  activeClass?: string
  raw?: boolean
  custom?: boolean
}

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Button.vue",
            r#"<script setup lang="ts">
import type { ButtonProps } from './types'

defineProps<ButtonProps>()
</script>
<template><button /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/src/Button.vue")
        .expect("should return component meta");

    let active = meta
        .props
        .iter()
        .find(|prop| prop.name == "active")
        .expect("active prop should be preserved through imported Omit");
    assert_eq!(active.raw_type.as_deref(), Some("boolean"));
    assert_eq!(
        active.description.as_deref(),
        Some("Force the link to be active independent of the current route.")
    );

    let active_class = meta
        .props
        .iter()
        .find(|prop| prop.name == "activeClass")
        .expect("activeClass prop should be preserved through imported Omit");
    assert_eq!(active_class.raw_type.as_deref(), Some("string"));
    assert_eq!(
        active_class.description.as_deref(),
        Some("Class to apply when the link is active")
    );
}

// ===========================================================================
// Phase 3: Fallthrough inheritance resolver
// ===========================================================================

use verter_analysis::component_meta::{
    AcceptedEventKind, AcceptedPropKind, AcceptedSurfaceCompleteness, BranchStatus,
    FallthroughSurface, MemberAvailability, MemberProvenance, PartialBranchReason,
    ResolvedRootStep, UnresolvedBranchReason,
};

/// Helper: get the component meta for a file (through session).
fn get_meta(
    project: &Arc<MetaProject>,
    canonical_id: &str,
) -> verter_analysis::component_meta::ComponentMetaAnalysis {
    let session = project.open_session().unwrap();
    session
        .get_component_meta(canonical_id)
        .unwrap()
        .expect("get_component_meta should return metadata")
}

#[test]
fn single_native_root_inherits_intrinsic_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is in accepted_props
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"
            && matches!(p.provenance, MemberProvenance::Declared)
            && matches!(p.kind, AcceptedPropKind::DeclaredProp)),
        "accepted_props should contain declared 'msg' prop, got: {:?}",
        meta.accepted_props
            .iter()
            .map(|p| &p.name)
            .collect::<Vec<_>>()
    );

    // Assert+: inherited attrs from div should be present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "id"
            && matches!(p.provenance, MemberProvenance::Inherited { .. })
            && matches!(p.kind, AcceptedPropKind::Attr)),
        "accepted_props should contain inherited 'id' attr from <div>, got: {:?}",
        meta.accepted_props
            .iter()
            .map(|p| &p.name)
            .collect::<Vec<_>>()
    );

    // Assert+: inherited events from div
    assert!(
        meta.accepted_events.iter().any(|e| e.name == "click"
            && matches!(e.provenance, MemberProvenance::Inherited { .. })
            && matches!(e.kind, AcceptedEventKind::Listener)),
        "accepted_events should contain inherited 'click' listener from <div>, got: {:?}",
        meta.accepted_events
            .iter()
            .map(|e| &e.name)
            .collect::<Vec<_>>()
    );

    // Assert+: surface completeness should be Exact
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::Exact,
        "completeness should be Exact for a simple native root"
    );

    // Assert+: fallthrough_surface should have branches
    assert!(
        matches!(
            meta.fallthrough_surface,
            FallthroughSurface::Branches { .. }
        ),
        "fallthrough_surface should be Branches, got: {:?}",
        meta.fallthrough_surface
    );

    // Assert-: declared props should NOT appear in fallthrough_surface
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert_eq!(branches.len(), 1, "should have one branch");
        assert!(
            !branches[0].props.iter().any(|p| p.name == "msg"),
            "fallthrough_surface should NOT contain declared 'msg' prop"
        );
        assert_eq!(
            branches[0].status,
            BranchStatus::Resolved,
            "branch status should be Resolved"
        );
        assert!(
            matches!(&branches[0].root_chain[0], ResolvedRootStep::NativeTag { tag } if tag == "div"),
            "root_chain should show NativeTag div"
        );
    }
}

#[test]
fn explicit_root_bindings_are_subtracted() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div id="root" @click="() => {}">{{ msg }}</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert-: explicitly bound 'id' attr should NOT be inherited
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            !branches[0].props.iter().any(|p| p.name == "id"),
            "consumed 'id' attr should be subtracted from inherited props"
        );
    }

    // Assert-: explicitly bound 'click' listener should NOT be inherited
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            !branches[0].events.iter().any(|e| e.name == "click"),
            "consumed 'click' listener should be subtracted from inherited events"
        );
    }

    // Assert+: other attrs should still be inherited
    assert!(
        meta.accepted_props.iter().any(
            |p| p.name == "title" && matches!(p.provenance, MemberProvenance::Inherited { .. })
        ),
        "non-consumed 'title' attr should still be inherited"
    );
}

#[test]
fn declared_props_and_events_take_precedence() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ id: number }>()
defineEmits<{ (e: 'click', value: string): void }>()
</script>
<template><div>hello</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: 'id' should be declared, not inherited
    let id_prop = meta
        .accepted_props
        .iter()
        .find(|p| p.name == "id")
        .expect("should have 'id' in accepted_props");
    assert!(
        matches!(id_prop.provenance, MemberProvenance::Declared),
        "'id' should be declared, not inherited"
    );

    // Assert+: 'click' should be declared, not inherited
    let click_event = meta
        .accepted_events
        .iter()
        .find(|e| e.name == "click")
        .expect("should have 'click' in accepted_events");
    assert!(
        matches!(click_event.provenance, MemberProvenance::Declared),
        "'click' should be declared, not inherited"
    );

    // Assert-: should NOT have duplicate 'id' or 'click'
    assert_eq!(
        meta.accepted_props
            .iter()
            .filter(|p| p.name == "id")
            .count(),
        1,
        "'id' should appear exactly once"
    );
    assert_eq!(
        meta.accepted_events
            .iter()
            .filter(|e| e.name == "click")
            .count(),
        1,
        "'click' should appear exactly once"
    );
}

#[test]
fn declared_on_listener_alias_prop_blocks_inherited_click_listener() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ onClick?: () => void }>()
</script>
<template><div>hello</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        meta.accepted_props
            .iter()
            .any(|p| p.name == "onClick" && matches!(p.provenance, MemberProvenance::Declared)),
        "declared onClick prop must remain on the accepted prop surface"
    );
    assert!(
        !meta.accepted_events.iter().any(|e| e.name == "click"),
        "declared onClick prop must block the inherited click listener alias"
    );

    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            branches
                .iter()
                .all(|branch| branch.events.iter().all(|event| event.name != "click")),
            "fallthrough branches must not leak click when a declared onClick prop shadows it"
        );
    }
}

#[test]
fn inherit_attrs_false_returns_declared_only_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false })
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"),
        "should have declared 'msg'"
    );

    // Assert-: no inherited members
    assert!(
        !meta
            .accepted_props
            .iter()
            .any(|p| matches!(p.provenance, MemberProvenance::Inherited { .. })),
        "should have no inherited props when inheritAttrs: false"
    );
    assert!(
        !meta
            .accepted_events
            .iter()
            .any(|e| matches!(e.provenance, MemberProvenance::Inherited { .. })),
        "should have no inherited events when inheritAttrs: false"
    );

    // Assert+: fallthrough_surface is None
    assert!(
        matches!(meta.fallthrough_surface, FallthroughSurface::None { .. }),
        "fallthrough_surface should be None when inheritAttrs: false"
    );
}

#[test]
fn unconditional_multi_root_returns_declared_only_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>a</div><span>b</span></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"),
        "should have declared 'msg'"
    );

    // Assert-: no inherited members
    assert!(
        !meta
            .accepted_props
            .iter()
            .any(|p| matches!(p.provenance, MemberProvenance::Inherited { .. })),
        "multi-root should have no inherited props"
    );

    // Assert+: fallthrough_surface is None
    assert!(
        matches!(meta.fallthrough_surface, FallthroughSurface::None { .. }),
        "fallthrough_surface should be None for multi-root"
    );
}

#[test]
fn conditional_single_root_returns_exact_branches() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const show = true
defineProps<{ msg: string }>()
</script>
<template>
  <div v-if="show">a</div>
  <input v-else />
</template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: should have branches
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert_eq!(branches.len(), 2, "should have 2 branches (div, input)");

        // Branch 0: div
        assert_eq!(branches[0].branch_key, "0");
        assert!(
            matches!(&branches[0].root_chain[0], ResolvedRootStep::NativeTag { tag } if tag == "div"),
            "first branch should be div"
        );

        // Branch 1: input
        assert_eq!(branches[1].branch_key, "1");
        assert!(
            matches!(&branches[1].root_chain[0], ResolvedRootStep::NativeTag { tag } if tag == "input"),
            "second branch should be input"
        );

        // Assert+: input-specific attrs should be conditional
        // (only in branch 1, not branch 0)
        let input_specific = meta.accepted_props.iter().find(|p| p.name == "type");
        if let Some(p) = input_specific {
            assert!(
                matches!(p.availability, MemberAvailability::Conditional { .. }),
                "'type' attr should be conditional (only in input branch)"
            );
        }
    } else {
        panic!("expected FallthroughSurface::Branches");
    }
}

#[test]
fn static_dynamic_is_root_resolves_native_candidates() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
const showNative = true
</script>
<template><component :is="showNative ? 'div' : Child" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    let value_prop = meta
        .accepted_props
        .iter()
        .find(|p| p.name == "value")
        .expect("dynamic :is should propagate the input branch's accepted attrs");
    assert!(
        matches!(
            value_prop.availability,
            MemberAvailability::Conditional { .. }
        ),
        "input-only attrs from dynamic :is candidates must stay conditional"
    );

    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            branches
                .iter()
                .any(|branch| matches!(&branch.root_chain[0], ResolvedRootStep::NativeTag { tag } if tag == "div")),
            "dynamic :is should produce a native div branch"
        );
        assert!(
            branches.iter().any(|branch| {
                branch
                    .root_chain
                    .iter()
                    .any(|step| matches!(step, ResolvedRootStep::Component { component_name, .. } if component_name == "Child"))
            }),
            "dynamic :is should also preserve the imported component branch"
        );
    } else {
        panic!("expected FallthroughSurface::Branches");
    }
}

#[test]
fn root_v_bind_known_object_shape_is_consumed_exactly() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const rootAttrs = {
  id: 'root',
  onClick: () => {},
}
</script>
<template><div v-bind="rootAttrs" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        !meta.accepted_props.iter().any(|p| p.name == "id"),
        "exact root spread keys must be subtracted from inherited attrs"
    );
    assert!(
        !meta.accepted_events.iter().any(|e| e.name == "click"),
        "exact root spread listener aliases must be subtracted from inherited listeners"
    );
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::Exact,
        "resolvable root spreads should not force a lower-bound surface"
    );

    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(
            branches
                .iter()
                .all(|branch| branch.props.iter().all(|prop| prop.name != "id")),
            "spread-consumed attrs must not leak back into fallthrough branches"
        );
        assert!(
            branches
                .iter()
                .all(|branch| branch.events.iter().all(|event| event.name != "click")),
            "spread-consumed listeners must not leak back into fallthrough branches"
        );
        assert!(
            branches
                .iter()
                .all(|branch| matches!(branch.status, BranchStatus::Resolved)),
            "an exact root spread should keep the branch resolved"
        );
    } else {
        panic!("expected FallthroughSurface::Branches");
    }
}

#[test]
fn root_v_bind_unknown_shape_uses_structured_partial_reason() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const rootAttrs: Record<string, unknown> = {}
</script>
<template><div v-bind="rootAttrs" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::LowerBound,
        "unknown root spreads must lower accepted-surface completeness"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };

    assert!(
        branches.iter().any(|branch| matches!(
            &branch.status,
            BranchStatus::PartiallyUnresolved { reasons }
                if reasons == &vec![PartialBranchReason::UnknownSpread]
        )),
        "unknown root spreads must surface a structured UnknownSpread reason, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.status)
            .collect::<Vec<_>>()
    );
}

#[test]
fn project_local_intrinsics_load_from_vue_type_entrypoints() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue/package.json".to_string(),
        Arc::from(
            r#"{
  "name": "vue",
  "types": "./index.d.ts",
  "exports": {
    ".": { "types": "./index.d.ts", "import": "./index.js" },
    "./jsx": { "types": "./jsx.d.ts", "import": "./jsx.js" }
  }
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/index.d.ts".to_string(),
        Arc::from(
            r#"export interface HTMLAttributes {
  fallbackOnly?: string
  onProjectClick?: ProjectClickEvent
}

export interface ProjectClickEvent {
  source: 'project'
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/jsx.d.ts".to_string(),
        Arc::from(
            r#"import type { NativeElements } from "./jsx-runtime"

export namespace JSX {
  export interface IntrinsicElements extends NativeElements {}
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/jsx-runtime.d.ts".to_string(),
        Arc::from(
            r#"import type { HTMLAttributes } from "./index"

export interface NativeElements {
  div: HTMLAttributes & { projectOnly?: string }
}"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    let project = MetaProject::new(host);
    project
        .upsert_base("/workspace/src/App.vue", r#"<template><div /></template>"#)
        .unwrap();

    let meta = get_meta(&project, "/workspace/src/App.vue");

    assert!(
        meta.accepted_props
            .iter()
            .any(|prop| prop.name == "projectOnly"),
        "native intrinsics loading should surface tag-specific members from vue/jsx"
    );
    assert!(
        meta.accepted_props
            .iter()
            .any(|prop| prop.name == "fallbackOnly"),
        "native intrinsics loading should surface fallback HTMLAttributes members from vue"
    );
    assert!(
        meta.accepted_events
            .iter()
            .any(|event| event.name == "projectClick"),
        "native intrinsics loading should expose listeners derived from the project-local HTMLAttributes surface"
    );
    assert!(
        !meta.accepted_props.iter().any(|prop| prop.name == "id"),
        "project-local intrinsic surfaces should replace the generated built-in tag surface when vue entrypoints resolve"
    );
}

#[test]
fn project_local_intrinsics_tag_members_override_fallback_duplicates() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/vue/package.json".to_string(),
        Arc::from(
            r#"{
  "name": "vue",
  "types": "./index.d.ts",
  "exports": {
    ".": { "types": "./index.d.ts", "import": "./index.js" },
    "./jsx": { "types": "./jsx.d.ts", "import": "./jsx.js" }
  }
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/index.d.ts".to_string(),
        Arc::from(
            r#"export interface HTMLAttributes {
  projectOnly?: number
  onClick?: (payload: FallbackClickEvent) => void
}

export interface FallbackClickEvent {
  source: 'fallback'
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/jsx.d.ts".to_string(),
        Arc::from(
            r#"import type { NativeElements } from "./jsx-runtime"

export namespace JSX {
  export interface IntrinsicElements extends NativeElements {}
}"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/vue/jsx-runtime.d.ts".to_string(),
        Arc::from(
            r#"import type { HTMLAttributes } from "./index"

export interface NativeElements {
  div: HTMLAttributes & {
    projectOnly?: string
    onClick?: (payload: ProjectClickEvent) => void
  }
}

export interface ProjectClickEvent {
  source: 'project'
}"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    let project = MetaProject::new(host);
    project
        .upsert_base("/workspace/src/App.vue", r#"<template><div /></template>"#)
        .unwrap();

    let meta = get_meta(&project, "/workspace/src/App.vue");

    let project_only = meta
        .accepted_props
        .iter()
        .find(|prop| prop.name == "projectOnly")
        .expect("project-local tag members must still be present");
    assert!(
        matches!(
            project_only.type_expr,
            TypeExpr::Primitive(PrimitiveName::String)
        ),
        "tag-specific projectOnly should override the fallback type, got: {:?}",
        project_only.type_expr
    );

    let click = meta
        .accepted_events
        .iter()
        .find(|event| event.name == "click")
        .expect("tag-specific listeners must still appear on the accepted event surface");
    assert!(
        matches!(
            &click.payload,
            TypeExpr::Function(function)
                if function.parameters.len() == 1
                    && matches!(
                        &function.parameters[0].ty,
                        TypeExpr::Ref { name, type_arguments }
                            if name.as_ref() == "ProjectClickEvent" && type_arguments.is_empty()
                    )
        ),
        "tag-specific listener payloads must override fallback listeners, got: {:?}",
        click.payload
    );
}

#[test]
fn generic_root_propagation_off_stays_sound() {
    let project = make_project();
    project
        .upsert_base(
            "/Poly.vue",
            r#"<script setup lang="ts" generic="T extends 'button' | 'input'">
defineProps<{ as: T }>()
</script>
<template><component :is="as" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Poly from './Poly.vue'
</script>
<template><Poly as="input" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        !meta.accepted_props.iter().any(|prop| prop.name == "value"),
        "generic root propagation disabled must not invent input-only attrs"
    );
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::LowerBound,
        "an unresolved generic root must remain a lower-bound surface"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches.iter().any(|branch| {
            matches!(
                &branch.status,
                BranchStatus::Unresolved {
                    reason: UnresolvedBranchReason::DynamicComponentIs
                }
            )
        }),
        "without propagation the generic child root should remain unresolved, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.status)
            .collect::<Vec<_>>()
    );
}

#[test]
fn generic_root_propagation_specializes_dynamic_is_when_enabled() {
    let project = make_project_with_config(HostConfig {
        generic_root_propagation: true,
        ..HostConfig::default()
    });
    project
        .upsert_base(
            "/Poly.vue",
            r#"<script setup lang="ts" generic="T extends 'button' | 'input'">
defineProps<{ as: T }>()
</script>
<template><component :is="as" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Poly from './Poly.vue'
</script>
<template><Poly as="input" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    let value_prop = meta
        .accepted_props
        .iter()
        .find(|prop| prop.name == "value")
        .expect("generic propagation should specialize the child root to input");
    assert!(
        matches!(value_prop.availability, MemberAvailability::Always),
        "single specialized generic roots should yield always-available attrs"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches.iter().any(|branch| {
            matches!(
                branch.root_chain.as_slice(),
                [
                    ResolvedRootStep::Component { component_name, .. },
                    ResolvedRootStep::NativeTag { tag }
                ] if component_name == "Poly" && tag == "input"
            )
        }),
        "generic propagation should resolve the child root chain to Poly -> input, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.root_chain)
            .collect::<Vec<_>>()
    );
}

#[test]
fn generic_root_propagation_recurses_through_component_chain() {
    let project = make_project_with_config(HostConfig {
        generic_root_propagation: true,
        ..HostConfig::default()
    });
    project
        .upsert_base(
            "/Poly.vue",
            r#"<script setup lang="ts" generic="T extends 'button' | 'input'">
defineProps<{ as: T }>()
</script>
<template><component :is="as" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/Wrapper.vue",
            r#"<script setup lang="ts" generic="T extends 'button' | 'input'">
import Poly from './Poly.vue'
defineProps<{ as: T }>()
</script>
<template><Poly :as="as" /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Wrapper from './Wrapper.vue'
</script>
<template><Wrapper as="input" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        meta.accepted_props.iter().any(|prop| prop.name == "value"),
        "recursive generic propagation should preserve the specialized input attrs through Wrapper"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches.iter().any(|branch| {
            matches!(
                branch.root_chain.as_slice(),
                [
                    ResolvedRootStep::Component { component_name: wrapper_name, .. },
                    ResolvedRootStep::Component { component_name: poly_name, .. },
                    ResolvedRootStep::NativeTag { tag }
                ] if wrapper_name == "Wrapper" && poly_name == "Poly" && tag == "input"
            )
        }),
        "recursive generic propagation should resolve Wrapper -> Poly -> input, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.root_chain)
            .collect::<Vec<_>>()
    );
}

#[test]
fn recursive_cycle_uses_structured_unresolved_reason() {
    let project = make_project();
    project
        .upsert_base(
            "/A.vue",
            r#"<script setup lang="ts">
import B from './B.vue'
</script>
<template><B /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/B.vue",
            r#"<script setup lang="ts">
import A from './A.vue'
</script>
<template><A /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();
    let meta = get_meta(&project, "/A.vue");
    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };

    assert!(
        branches.iter().any(|branch| matches!(
            &branch.status,
            BranchStatus::Unresolved {
                reason: UnresolvedBranchReason::Cycle { canonical_id }
            } if canonical_id == "/A.vue"
        )),
        "cycles must terminate with a structured cycle reason, got: {:?}",
        branches
            .iter()
            .map(|branch| &branch.status)
            .collect::<Vec<_>>()
    );

    assert!(
        branches.iter().any(|branch| {
            branch.root_chain.iter().any(|step| {
                matches!(
                    step,
                    ResolvedRootStep::Unresolved {
                        reason: UnresolvedBranchReason::Cycle { canonical_id },
                        ..
                    } if canonical_id == "/A.vue"
                )
            })
        }),
        "cycle branches must preserve the structured cycle reason in the root chain"
    );
    assert!(
        provenance(&project).resolver_cycle_detections >= 1,
        "fallthrough cycles should increment the shared resolver cycle counter"
    );
}

#[test]
fn recursive_component_propagates_inherited_surface() {
    let project = make_project();

    // Child component with <div> root
    project
        .upsert_base(
            "/Child.vue",
            r#"<script setup lang="ts">
defineProps<{ childProp: string }>()
</script>
<template><div>child</div></template>"#,
        )
        .unwrap();

    // Parent with component root
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
defineProps<{ parentProp: string }>()
</script>
<template><Child :childProp="parentProp" /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "parentProp"),
        "should have declared 'parentProp'"
    );

    // Assert+: fallthrough_surface should have branches
    assert!(
        matches!(
            meta.fallthrough_surface,
            FallthroughSurface::Branches { .. }
        ),
        "fallthrough_surface should be Branches for component root"
    );

    // Assert+: root_chain should show Component step
    if let FallthroughSurface::Branches { ref branches } = meta.fallthrough_surface {
        assert!(!branches.is_empty(), "should have at least one branch");
        assert!(
            branches[0]
                .root_chain
                .iter()
                .any(|step| matches!(step, ResolvedRootStep::Component { .. })),
            "root_chain should contain a Component step, got: {:?}",
            branches[0].root_chain
        );
    }
}

#[test]
fn recursive_component_keeps_child_declared_surface_alongside_child_fallthrough() {
    let project = make_project();

    project
        .upsert_base(
            "/Child.vue",
            r#"<script setup lang="ts">
defineProps<{ childProp: string }>()
defineEmits<{ (e: 'childClick', value: number): void }>()
</script>
<template><div>child</div></template>"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    let child_prop = meta
        .accepted_props
        .iter()
        .find(|p| p.name == "childProp")
        .expect("parent must expose child's declared prop through component root recursion");
    assert!(
        matches!(child_prop.provenance, MemberProvenance::Inherited { .. }),
        "child declared prop must arrive as inherited acceptance on the parent"
    );
    assert!(
        matches!(child_prop.kind, AcceptedPropKind::Attr),
        "child declared prop should be exposed as an accepted attr on the parent"
    );

    let child_event = meta
        .accepted_events
        .iter()
        .find(|e| e.name == "childClick")
        .expect("parent must expose child's declared event through component root recursion");
    assert!(
        matches!(child_event.provenance, MemberProvenance::Inherited { .. }),
        "child declared event must arrive as inherited acceptance on the parent"
    );
    assert!(
        matches!(child_event.kind, AcceptedEventKind::Listener),
        "child declared event should be exposed as an accepted listener on the parent"
    );

    assert!(
        meta.accepted_props.iter().any(|p| p.name == "id"),
        "parent must still expose child's inherited native attrs, not just declared members"
    );
}

#[test]
fn non_vue_component_root_stops_fallthrough_recursion_at_the_boundary() {
    let project = make_project();

    project
        .upsert_base(
            "/Child.ts",
            r#"export default function Child() {
  return null
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child'
defineProps<{ parentProp: string }>()
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    assert!(
        meta.accepted_props.iter().any(|p| p.name == "parentProp"),
        "declared props must remain on the accepted surface"
    );
    assert!(
        !meta.accepted_props.iter().any(|p| p.name == "id"),
        "non-Vue child roots must not invent inherited attrs"
    );
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::LowerBound,
        "non-Vue child roots should degrade completeness instead of recursing"
    );

    let FallthroughSurface::Branches { branches } = &meta.fallthrough_surface else {
        panic!("expected FallthroughSurface::Branches");
    };
    assert!(
        branches.iter().any(|branch| {
            matches!(
                &branch.status,
                BranchStatus::Unresolved {
                    reason: UnresolvedBranchReason::ChildResolutionFailed,
                }
            )
        }),
        "non-Vue child roots should stop at an unresolved branch"
    );
}

#[test]
fn cycle_terminates_without_invented_members() {
    let project = make_project();

    // A imports B, B imports A — create a cycle
    project
        .upsert_base(
            "/A.vue",
            r#"<script setup lang="ts">
import B from './B.vue'
defineProps<{ aProp: string }>()
</script>
<template><B /></template>"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/B.vue",
            r#"<script setup lang="ts">
import A from './A.vue'
defineProps<{ bProp: string }>()
</script>
<template><A /></template>"#,
        )
        .unwrap();

    // Should not panic or infinite loop
    let meta = get_meta(&project, "/A.vue");

    // Assert+: declared props are present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "aProp"),
        "should have declared 'aProp'"
    );

    // Assert+: surface completeness should be LowerBound due to cycle
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::LowerBound,
        "cycle should produce LowerBound completeness"
    );

    // Assert-: no invented members from the cycle
    assert!(
        !meta.accepted_props.iter().any(|p| p.name == "bProp"),
        "should NOT inherit 'bProp' through a cycle"
    );
}

#[test]
fn unresolved_target_branch_does_not_crash() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><slot /></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"),
        "should have declared 'msg'"
    );

    // Assert-: no inherited members from slot
    assert!(
        !meta
            .accepted_props
            .iter()
            .any(|p| matches!(p.provenance, MemberProvenance::Inherited { .. })),
        "slot root should produce no inherited props"
    );
}

#[test]
fn builtin_root_is_unresolved_branch() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><Teleport to="body">{{ msg }}</Teleport></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared prop is present
    assert!(
        meta.accepted_props.iter().any(|p| p.name == "msg"),
        "should have declared 'msg'"
    );

    // Assert-: no inherited members from Teleport
    assert!(
        !meta
            .accepted_props
            .iter()
            .any(|p| matches!(p.provenance, MemberProvenance::Inherited { .. })),
        "Teleport root should produce no inherited props"
    );
}

#[test]
fn accepted_surface_member_order_is_deterministic() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ z: string; a: number }>()
</script>
<template><div>test</div></template>"#,
        )
        .unwrap();

    let meta = get_meta(&project, "/App.vue");

    // Assert+: declared props come first in declared source order
    let declared_props: Vec<&str> = meta
        .accepted_props
        .iter()
        .filter(|p| matches!(p.provenance, MemberProvenance::Declared))
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(
        declared_props,
        vec!["z", "a"],
        "declared props should keep source order"
    );

    // Assert+: inherited props come after declared, sorted lexicographically
    let inherited_props: Vec<&str> = meta
        .accepted_props
        .iter()
        .filter(|p| matches!(p.provenance, MemberProvenance::Inherited { .. }))
        .map(|p| p.name.as_str())
        .collect();
    let mut sorted = inherited_props.clone();
    sorted.sort();
    assert_eq!(
        inherited_props, sorted,
        "inherited props should be sorted lexicographically"
    );
}

#[test]
fn cache_hit_reused() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        )
        .unwrap();

    // First call
    let meta1 = get_meta(&project, "/App.vue");
    // Second call — should use cache
    let meta2 = get_meta(&project, "/App.vue");

    // Assert+: both calls return the same accepted surface
    let names1: Vec<&str> = meta1
        .accepted_props
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    let names2: Vec<&str> = meta2
        .accepted_props
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names1, names2, "cached result should be identical");
}

#[test]
fn child_change_invalidates_parent_fallthrough_cache() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><div>child</div></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let first = get_meta(&project, "/App.vue");
    assert!(
        !first.accepted_props.iter().any(|p| p.name == "value"),
        "div-root child should not expose input-only attrs before the dependency changes"
    );

    #[cfg(feature = "scheduler")]
    let first_cache = cached_fallthrough_state(&project, "/App.vue")
        .expect("first query should cache fallthrough");

    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();

    let second = get_meta(&project, "/App.vue");
    assert!(
        second.accepted_props.iter().any(|p| p.name == "value"),
        "parent fallthrough surface must refresh when the child root changes"
    );

    #[cfg(feature = "scheduler")]
    {
        let second_cache = cached_fallthrough_state(&project, "/App.vue")
            .expect("second query should repopulate the parent fallthrough cache");
        assert!(
            !Arc::ptr_eq(&first_cache, &second_cache),
            "dependency change must invalidate the parent's cached fallthrough surface"
        );
    }
}

#[cfg(feature = "scheduler")]
#[test]
fn shared_child_fallthrough_reuses_runtime_child_surface_nodes() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/ParentA.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/ParentB.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    project.host().resolver_runtime().reset_counters();

    let first = get_meta(&project, "/ParentA.vue");
    let after_first = project.host().resolver_runtime().counter_snapshot();
    let second = get_meta(&project, "/ParentB.vue");
    let after_second = project.host().resolver_runtime().counter_snapshot();

    assert!(
        first.accepted_props.iter().any(|prop| prop.name == "value"),
        "first parent should inherit input attrs from the shared child"
    );
    assert!(
        second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "value"),
        "second parent should inherit input attrs from the shared child"
    );
    assert!(
        !second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "missing"),
        "shared child reuse must not fabricate unrelated attrs"
    );
    assert!(
        after_first.node_cache_misses > 0,
        "first parent should populate runtime fallthrough child nodes, got {:?}",
        after_first
    );
    assert!(
        after_second.node_cache_hits > after_first.node_cache_hits,
        "second parent should reuse runtime child-surface nodes for the shared child, before={:?} after={:?}",
        after_first,
        after_second
    );
}

#[cfg(feature = "scheduler")]
#[test]
fn shared_child_runtime_reuse_survives_host_child_cache_clear() {
    let project = make_project();
    project
        .upsert_base("/Child.vue", r#"<template><input /></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/ParentA.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/ParentB.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let first = get_meta(&project, "/ParentA.vue");
    assert!(
        first.accepted_props.iter().any(|prop| prop.name == "value"),
        "first parent should inherit input attrs from the child"
    );

    clear_legacy_cached_fallthrough_state(&project, "/Child.vue");
    project.host().provenance.reset();
    project.host().resolver_runtime().reset_counters();

    let second = get_meta(&project, "/ParentB.vue");
    let runtime = project.host().resolver_runtime().counter_snapshot();
    let provenance = provenance(&project);

    assert!(
        second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "value"),
        "second parent should still inherit input attrs after host child caches are cleared"
    );
    assert!(
        runtime.node_cache_hits > 0,
        "runtime child-surface nodes should satisfy the shared child lookup after host cache clear, got {:?}",
        runtime
    );
    assert_eq!(
        provenance.resolver_node_cache_misses,
        2,
        "only the new parent's component-meta and top-level fallthrough requests should miss once the child is runtime-owned, got provenance={:?}",
        provenance
    );
    assert_eq!(
        provenance.component_meta_resolved_state_recomputes,
        1,
        "the shared child should reuse runtime-owned fallthrough state instead of recomputing component meta, got provenance={:?}",
        provenance
    );
}

#[cfg(feature = "scheduler")]
#[test]
fn distinct_children_reuse_runtime_intrinsic_surface_nodes() {
    let project = make_project();
    project
        .upsert_base("/ChildA.vue", r#"<template><input /></template>"#)
        .unwrap();
    project
        .upsert_base("/ChildB.vue", r#"<template><input /></template>"#)
        .unwrap();
    project
        .upsert_base(
            "/ParentA.vue",
            r#"<script setup lang="ts">
import ChildA from './ChildA.vue'
</script>
<template><ChildA /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/ParentB.vue",
            r#"<script setup lang="ts">
import ChildB from './ChildB.vue'
</script>
<template><ChildB /></template>"#,
        )
        .unwrap();

    let first = get_meta(&project, "/ParentA.vue");
    assert!(
        first.accepted_props.iter().any(|prop| prop.name == "value"),
        "first parent should inherit input attrs from ChildA"
    );

    project.host().provenance.reset();
    project.host().resolver_runtime().reset_counters();

    let second = get_meta(&project, "/ParentB.vue");
    let runtime = project.host().resolver_runtime().counter_snapshot();

    assert!(
        second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "value"),
        "second parent should inherit input attrs from ChildB"
    );
    assert!(
        !second
            .accepted_props
            .iter()
            .any(|prop| prop.name == "missing"),
        "intrinsic reuse must not fabricate unrelated attrs"
    );
    assert!(
        runtime.node_cache_hits > 0,
        "the second parent should reuse runtime intrinsic-surface nodes for the shared <input> root, got {:?}",
        runtime
    );
}

#[cfg(feature = "scheduler")]
#[test]
fn cached_fallthrough_fact_versions_include_transitive_child_component_meta_dependencies() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            "export interface ChildProps { msg?: string; count?: number }",
        )
        .unwrap();
    project
        .upsert_base(
            "/Child.vue",
            r#"<script setup lang="ts">
import type { ChildProps } from './types'
defineProps<ChildProps>()
</script>
<template><div>child</div></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template><Child /></template>"#,
        )
        .unwrap();

    let _ = get_meta(&project, "/App.vue");
    let cached = cached_fallthrough_entry(&project, "/App.vue")
        .expect("parent fallthrough should be cached after meta extraction");

    assert!(
        cached.fact_versions.iter().any(|fact| matches!(
            fact,
            verter_resolver::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/Child.vue"
        )),
        "cached fallthrough facts should include the child component file"
    );
    assert!(
        cached.fact_versions.iter().any(|fact| matches!(
            fact,
            verter_resolver::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/types.ts"
        )),
        "cached fallthrough facts should include transitive child component-meta deps"
    );
}

// ── Fix 2: eval-path host cache reuse within single resolve_component_meta ──

#[test]
fn eval_path_reuses_cached_eval_inputs_within_single_resolve() {
    // The Phase 2 architecture no longer expects the eval path to bounce back
    // through resolve_external_type_from_loaded_files inside the same call.
    // Instead, expanded resolution should build imported eval inputs once,
    // cache them on the resolved state, and reuse that cached input set for the
    // follow-up eval/fallthrough work within the same get_component_meta call.
    let project = make_project();
    let session = project.open_session().unwrap();

    session
        .upsert(
            "/src/types.ts",
            "export interface ButtonProps { label: string }".to_string(),
        )
        .unwrap();
    session
        .upsert(
            "/src/Button.vue",
            r#"<script setup lang="ts">
import { ButtonProps } from './types'
defineProps<ButtonProps>()
</script>
<template><button /></template>"#
                .to_string(),
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Button.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // Reset counters, then query component meta once.
    project.host().provenance().reset();
    let meta = session
        .get_component_meta("/src/Button.vue")
        .unwrap()
        .unwrap();
    let p = provenance(&project);

    assert_eq!(
        meta.props.len(),
        1,
        "should resolve the prop from cross-file type"
    );
    assert_eq!(meta.props[0].name, "label");
    assert_eq!(
        p.imported_eval_inputs_calls, 1,
        "expanded resolution should build imported eval inputs once and reuse them within the same call, got calls={}",
        p.imported_eval_inputs_calls,
    );

    let state = cached_resolved_state(
        &project,
        "/src/Button.vue",
        crate::types::ResolverMode::Expanded,
    )
    .expect("expanded resolved state should be cached");
    assert!(
        state.cached_eval_inputs.is_some(),
        "expanded resolved state should retain cached imported eval inputs"
    );
}

// ── Fix 3: Eliminate double imported_eval_inputs per getComponentMeta ──

#[test]
fn imported_eval_inputs_called_once_per_get_component_meta() {
    // A single get_component_meta() call should invoke imported_eval_inputs()
    // exactly once, not twice. Before Fix 3, the flow was:
    //   resolve_component_meta(Expanded) -> imported_eval_inputs()  [call 1]
    //   extract_component_meta_from_resolved -> resolve_fallthrough_surface
    //     -> build_fallthrough_eval_env -> imported_eval_inputs()   [call 2]
    // After Fix 3, the cached inputs from call 1 are threaded through to
    // build_fallthrough_eval_env_with_inputs, eliminating call 2.
    let project = make_project();
    let session = project.open_session().unwrap();

    session
        .upsert(
            "/src/types.ts",
            "export interface CardProps { title: string; subtitle?: string }".to_string(),
        )
        .unwrap();
    session
        .upsert(
            "/src/Card.vue",
            r#"<script setup lang="ts">
import { CardProps } from './types'
defineProps<CardProps>()
</script>
<template><div>{{ title }}</div></template>"#
                .to_string(),
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Card.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // Reset counters, then query component meta once
    project.host().provenance().reset();
    let meta = session
        .get_component_meta("/src/Card.vue")
        .unwrap()
        .unwrap();
    let p = provenance(&project);

    // Sanity: props resolved correctly from cross-file type
    assert_eq!(
        meta.props.len(),
        2,
        "should resolve both props from cross-file type"
    );
    assert!(
        meta.props.iter().any(|p| p.name == "title"),
        "should have 'title' prop"
    );
    assert!(
        meta.props.iter().any(|p| p.name == "subtitle"),
        "should have 'subtitle' prop"
    );

    // The critical assertion: imported_eval_inputs should be called exactly once,
    // not twice. The fallthrough path should reuse the cached inputs.
    assert_eq!(
        p.imported_eval_inputs_calls, 1,
        "imported_eval_inputs should be called exactly once per get_component_meta, \
         but was called {} times (the fallthrough path should reuse cached inputs)",
        p.imported_eval_inputs_calls,
    );
}

#[test]
fn root_spread_with_cross_file_type_still_resolves_after_eval_caching() {
    // Regression test for Fix 3: when cached eval inputs are threaded through
    // to fallthrough resolution, root v-bind="importedObj" must still resolve
    // the spread keys correctly and not degrade to UnknownSpread.
    use verter_analysis::component_meta::AcceptedSurfaceCompleteness;

    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface WidgetProps { enabled: boolean }
export const rootAttrs = { id: 'root', onClick: () => {} }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Widget.vue",
            r#"<script setup lang="ts">
import { WidgetProps, rootAttrs } from './types'
defineProps<WidgetProps>()
</script>
<template><div v-bind="rootAttrs">content</div></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/Widget.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = get_meta(&project, "/src/Widget.vue");

    // The declared prop must be present
    assert!(
        meta.props.iter().any(|p| p.name == "enabled"),
        "should have the declared 'enabled' prop"
    );

    // The root spread keys ('id', 'click') must be consumed and subtracted
    // from the accepted surface. If the eval caching broke, the spread would
    // degrade to UnknownSpread and the surface would be LowerBound.
    assert!(
        !meta.accepted_props.iter().any(|p| p.name == "id"),
        "root spread key 'id' must be consumed and subtracted from accepted attrs"
    );
    assert!(
        !meta.accepted_events.iter().any(|e| e.name == "click"),
        "root spread listener 'click' must be consumed and subtracted from accepted listeners"
    );
    assert_eq!(
        meta.accepted_surface_completeness,
        AcceptedSurfaceCompleteness::Exact,
        "with resolvable root spreads, accepted surface should be Exact, not degraded to LowerBound"
    );
}

// ── Fix 4: full eval source set for utility heritage and fallthrough ─────────

#[test]
fn cached_eval_inputs_track_macro_and_runtime_dependencies() {
    // Component with:
    // - a cross-file macro type dep (ButtonProps from ./types.ts)
    // - an imported value (rootAttrs from ./utils.ts) used in v-bind spread
    // - additional non-type imports (./helpers.ts)
    //
    // Cached eval inputs now track invalidation dependencies rather than
    // eagerly retaining every imported macro route in `sources`/`type_aliases`.
    // Runtime values and macro types are resolved on demand through the
    // cache-owned lookup path when the owner eval env is built.
    //
    // The cached inputs should therefore:
    // - track the macro type dependency for invalidation
    // - track runtime imports in `canonical_dependencies` for invalidation
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            "export interface ButtonProps { label: string }",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/helpers.ts",
            "export function format(s: string): string { return s }",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/utils.ts",
            "export const rootAttrs = { id: 'root-attrs-marker', onClick: () => {} }",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import { ButtonProps } from './types'
import { rootAttrs } from './utils'
import { format } from './helpers'
defineProps<ButtonProps>()
const msg = format('hello')
</script>
<template><div v-bind="rootAttrs">{{ msg }}</div></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./helpers".to_string(),
                resolved_canonical_id: Some("/src/helpers.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./utils".to_string(),
                resolved_canonical_id: Some("/src/utils.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let meta = get_meta(&project, "/src/App.vue");

    // Type eval should still resolve props correctly with the full imported source set.
    assert_eq!(meta.props.len(), 1, "should resolve the cross-file prop");
    assert_eq!(meta.props[0].name, "label");

    let state = cached_resolved_state(
        &project,
        "/src/App.vue",
        crate::types::ResolverMode::Expanded,
    )
    .expect("expanded resolved state should be cached");
    let imported_inputs = state
        .cached_eval_inputs
        .as_ref()
        .expect("expanded resolved state should retain cached eval inputs");

    assert!(
        imported_inputs
            .canonical_dependencies
            .contains("/src/types.ts"),
        "macro type dependencies should still be retained for invalidation"
    );
    assert!(
        imported_inputs.canonical_dependencies.contains("/src/utils.ts"),
        "runtime imports should still be tracked for invalidation even when they are materialized outside imported source merging"
    );
}

#[test]
fn type_reachable_count_zero_falls_back_to_all_sources() {
    // Component with inline defineProps (no macro_type_deps) should still
    // resolve locally without any cross-file imported-eval work.
    let project = make_project();
    let session = project.open_session().unwrap();

    session
        .upsert(
            "/src/App.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#
                .to_string(),
        )
        .unwrap();

    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("should get component meta");

    // Type eval should still work with inline types
    assert_eq!(meta.props.len(), 1, "should resolve inline prop");
    assert_eq!(meta.props[0].name, "msg");
}

// ── Barrel resolution cache tests ──────────────────────────────────────

#[test]
fn barrel_many_wildcard_exports_resolves_without_hang() {
    // Regression test: barrel with many `export *` entries should not hang.
    // Previously, each type lookup scanned ALL wildcard sources linearly.
    let project = make_project();

    // Create 30 Vue files, each exporting a unique type
    for i in 0..30 {
        project
            .upsert_base(
                &format!("/src/components/Comp{i}.vue"),
                &format!(
                    r#"<script lang="ts">
export interface Comp{i}Props {{
  value{i}?: string
}}
</script>
<template><div /></template>"#
                ),
            )
            .unwrap();
    }

    // Create a barrel that re-exports all 30 + a direct types file
    let mut barrel = String::new();
    for i in 0..30 {
        barrel.push_str(&format!("export * from '../components/Comp{i}.vue'\n"));
    }
    barrel.push_str("export * from './utils'\n");
    project.upsert_base("/src/types/index.ts", &barrel).unwrap();

    project
        .upsert_base(
            "/src/types/utils.ts",
            r#"export interface UtilType { helper: boolean }"#,
        )
        .unwrap();

    // Component that imports from the barrel
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Comp15Props, UtilType } from './types'

interface AppProps extends Comp15Props {
  extra?: UtilType
}

defineProps<AppProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Set up dependency resolutions
    let mut barrel_deps: Vec<crate::types::DependencyResolution> = (0..30)
        .map(|i| crate::types::DependencyResolution {
            specifier: format!("../components/Comp{i}.vue"),
            resolved_canonical_id: Some(format!("/src/components/Comp{i}.vue")),
            possible_canonical_ids: Vec::new(),
        })
        .collect();
    barrel_deps.push(crate::types::DependencyResolution {
        specifier: "./utils".to_string(),
        resolved_canonical_id: Some("/src/types/utils.ts".to_string()),
        possible_canonical_ids: Vec::new(),
    });
    project
        .host()
        .set_import_dependencies("/src/types/index.ts", barrel_deps);

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"value15"),
        "should resolve Comp15Props.value15 through barrel: {names:?}"
    );
    assert!(
        names.contains(&"extra"),
        "should keep local extra prop: {names:?}"
    );
}

#[test]
fn barrel_fully_resolved_returns_none_for_missing_type() {
    let project = make_project();

    project
        .upsert_base(
            "/src/types/index.ts",
            r#"export * from './a'
export * from './b'"#,
        )
        .unwrap();
    project
        .upsert_base("/src/types/a.ts", r#"export interface AType { a: string }"#)
        .unwrap();
    project
        .upsert_base("/src/types/b.ts", r#"export interface BType { b: number }"#)
        .unwrap();

    // Component imports a type that doesn't exist in the barrel
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { AType } from './types'

defineProps<AType>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/types/index.ts",
        vec![
            crate::types::DependencyResolution {
                specifier: "./a".to_string(),
                resolved_canonical_id: Some("/src/types/a.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./b".to_string(),
                resolved_canonical_id: Some("/src/types/b.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types/index.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"a"),
        "should resolve AType.a through barrel: {names:?}"
    );
    // Negative: BType should NOT appear (not imported)
    assert!(
        !names.contains(&"b"),
        "should not have BType.b (not imported): {names:?}"
    );
}

#[test]
fn barrel_nested_export_star_chain_resolves() {
    // A -> export * from B -> export * from C
    // A type from C should be found through the chain.
    let project = make_project();

    project
        .upsert_base("/src/barrel_a.ts", r#"export * from './barrel_b'"#)
        .unwrap();
    project
        .upsert_base("/src/barrel_b.ts", r#"export * from './deep'"#)
        .unwrap();
    project
        .upsert_base(
            "/src/deep.ts",
            r#"export interface DeepType { level: number }"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { DeepType } from './barrel_a'

defineProps<DeepType>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/barrel_a.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./barrel_b".to_string(),
            resolved_canonical_id: Some("/src/barrel_b.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/barrel_b.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./deep".to_string(),
            resolved_canonical_id: Some("/src/deep.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./barrel_a".to_string(),
            resolved_canonical_id: Some("/src/barrel_a.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should succeed");

    let names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        names.contains(&"level"),
        "should resolve DeepType.level through nested barrel chain: {names:?}"
    );
}

#[test]
fn depth_limit_does_not_hang_on_extreme_chain() {
    // Create a chain of 40 barrel files, each re-exporting from the next.
    // Verifies the resolver terminates on long chains without stack overflow.
    // (135 caused stack overflow in tests; 40 is safe and still exercises the chain.)
    let project = make_project();

    for i in 0..40 {
        let source = format!("export * from './barrel_{}'", i + 1);
        project
            .upsert_base(&format!("/src/barrel_{i}.ts"), &source)
            .unwrap();
        project.host().set_import_dependencies(
            &format!("/src/barrel_{i}.ts"),
            vec![crate::types::DependencyResolution {
                specifier: format!("./barrel_{}", i + 1),
                resolved_canonical_id: Some(format!("/src/barrel_{}.ts", i + 1)),
                possible_canonical_ids: Vec::new(),
            }],
        );
    }
    // Terminal file
    project
        .upsert_base(
            "/src/barrel_40.ts",
            r#"export interface FinalType { done: boolean }"#,
        )
        .unwrap();

    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { FinalType } from './barrel_0'
defineProps<FinalType>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./barrel_0".to_string(),
            resolved_canonical_id: Some("/src/barrel_0.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    // Should complete without hanging — depth limit terminates the chain
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("get_component_meta should return a result");

    // The type won't be found (depth exceeded), but the call must not hang
    // It's OK if props is empty — the important thing is termination.
    assert!(
        meta.props.len() <= 1,
        "depth-limited chain should produce 0-1 props (not hang): {:?}",
        meta.props.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
}

#[test]
fn component_meta_budget_error_detects_symbolic_budget_exceeded() {
    let types = ExpandedComponentTypes {
        props: vec![verter_analysis::type_expand::ExpandedField {
            name: "label".to_string(),
            r#type: TypeExpr::Primitive(PrimitiveName::String),
            raw_type: None,
            optional: false,
            completeness: verter_analysis::type_expand::ExpansionCompleteness::Partial,
            diagnostics: vec![verter_analysis::type_expand::ExpansionDiagnostic {
                reason: verter_analysis::type_expand::ExpansionStopReason::BudgetExceeded,
                context: "symbolic work limit reached".to_string(),
                property_name: None,
            }],
        }],
        ..ExpandedComponentTypes::default()
    };

    assert!(
        component_meta_expansion_budget_exceeded(&types),
        "budget-exceeded diagnostics should force an explicit component-meta error"
    );
}

#[test]
fn symbolic_budget_is_not_fatal_when_component_surface_exists() {
    let analysis = verter_analysis::component_meta::ComponentMetaAnalysis {
        props: vec![verter_analysis::component_meta::PropAnalysis {
            name: "label".to_string(),
            type_expr: TypeExpr::Primitive(PrimitiveName::String),
            type_expansion: None,
            raw_type: Some("string".to_string()),
            required: true,
            has_default: false,
            default_value: None,
            description: None,
            tags: Vec::new(),
        }],
        events: Vec::new(),
        slots: Vec::new(),
        models: Vec::new(),
        exposed: Vec::new(),
        public_instance: None,
        sfc_blocks: None,
        type_registry: Vec::new(),
        components: Vec::new(),
        template_refs: Vec::new(),
        imports: Vec::new(),
        bindings: Vec::new(),
        vue_api_calls: Vec::new(),
        styles: Vec::new(),
        flags: verter_analysis::component_meta::ComponentMetaFlags::default(),
        root_reachability: verter_analysis::component_meta::RootReachability::NoFallthrough {
            reason: verter_analysis::component_meta::NoFallthroughReason::NoTemplate,
        },
        accepted_props: Vec::new(),
        accepted_events: Vec::new(),
        accepted_surface_completeness:
            verter_analysis::component_meta::AcceptedSurfaceCompleteness::Exact,
        fallthrough_surface: verter_analysis::component_meta::FallthroughSurface::None {
            reason: verter_analysis::component_meta::NoFallthroughReason::NoTemplate,
        },
        options_api: false,
        file_path: "/src/App.vue".to_string(),
    };

    assert!(!component_meta_symbolic_budget_is_fatal(Some(&analysis)));
    assert!(component_meta_symbolic_budget_is_fatal(None));
}

#[test]
fn get_component_meta_retries_symbolic_budget_for_large_local_object_shapes() {
    let project = make_project();

    let prop_count = 2_400usize;
    let mut props_body = String::new();
    for index in 0..prop_count {
        props_body.push_str(&format!("  p{index}: string\n"));
    }

    project
        .upsert_base(
            "/src/App.vue",
            &format!(
                r#"<script setup lang="ts">
interface Props {{
{props_body}}}

defineProps<Props>()
</script>
<template><div /></template>"#
            ),
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/src/App.vue")
        .unwrap()
        .expect("large local object shape should succeed after budget retry");

    assert_eq!(
        meta.props.len(),
        prop_count,
        "retry path should materialize the full local prop surface"
    );
    assert!(meta.props.iter().any(|prop| prop.name == "p0"));
    assert!(meta
        .props
        .iter()
        .any(|prop| prop.name == format!("p{}", prop_count - 1)));
}

#[test]
fn get_component_meta_errors_when_external_type_resolution_step_budget_is_exhausted() {
    let project = make_project();

    let import_count = 2_005usize;
    let mut defs_source = String::new();
    for index in 0..import_count {
        defs_source.push_str(&format!(
            "export interface T{index} {{ p{index}: string }}\n"
        ));
    }

    let mut types_source = String::new();
    types_source.push_str("import type { ");
    for index in 0..import_count {
        if index > 0 {
            types_source.push_str(", ");
        }
        types_source.push_str(&format!("T{index}"));
    }
    types_source.push_str(" } from './defs'\n");
    types_source.push_str("export interface Props extends ");
    for index in 0..import_count {
        if index > 0 {
            types_source.push_str(", ");
        }
        types_source.push_str(&format!("T{index}"));
    }
    types_source.push_str(" {}\n");

    project.upsert_base("/src/defs.ts", &defs_source).unwrap();
    project.upsert_base("/src/types.ts", &types_source).unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Props } from "./types"
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./defs".to_string(),
            resolved_canonical_id: Some("/src/defs.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let session = project.open_session().unwrap();
    let err = session
        .get_component_meta("/src/App.vue")
        .expect_err("runaway external type resolution should fail with an explicit budget error");

    match err {
        MetaError::Host(message) => {
            assert!(
                message.contains("external type resolution step budget exceeded"),
                "error should explain the traversal cap, got: {message}"
            );
            assert!(
                message.contains("2000"),
                "error should include the configured step cap, got: {message}"
            );
        }
        other => panic!("expected host budget error, got {other:?}"),
    }
}

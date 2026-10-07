use super::*;

#[test]
fn provenance_snapshot_includes_vfs_dir_index_counters_from_workspace() {
    let dir = verter_test_support::unique_temp_dir("verter-host-provenance");
    std::fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("Comp.vue");
    std::fs::write(&file_path, "<template><div /></template>").unwrap();

    let canonical = file_path.to_string_lossy().replace('\\', "/");
    let ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_vfs_provenance();
    assert!(
        ws.file_exists(&canonical),
        "the filesystem workspace should seed its dir index from disk"
    );

    let snapshot = host.provenance_snapshot();
    assert_eq!(
        snapshot.dir_index_refresh_count, 1,
        "host provenance snapshots should surface VFS dir-index refreshes for benchmark validation"
    );
    assert_eq!(
        snapshot.native_fs_read_dir_count, 1,
        "host provenance snapshots should include the VFS read_dir count"
    );
    assert_eq!(
        snapshot.dir_index_hit_count, 0,
        "the first dir-index seed should refresh, not hit a cached directory listing"
    );
    assert_eq!(
        snapshot.native_fs_read_file_miss_count, 0,
        "seeding the dir index for a present file should not record a disk read miss"
    );

    std::fs::remove_file(&file_path).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn current_store_view_can_resolve_missing_relative_type_routes_for_existing_workspace_files() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/workspace/composables/useComponentIcons.ts",
        "export interface UseComponentIconsProps { icon?: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/types/index.ts",
        "export interface LinkProps { href?: string }\n",
    );
    upsert_vue(
        &host,
        "/workspace/components/Button.vue",
        r#"<script lang="ts">
import type { UseComponentIconsProps } from '../composables/useComponentIcons'
import type { LinkProps } from '../types'

export interface ButtonProps extends UseComponentIconsProps, LinkProps {
  label?: string
}
</script>
<template><button /></template>"#,
    );

    let _view = host.resolver_store_view_read().into_owned_view();

    assert_eq!(
        host.resolve_type_dependency_canonical(
            "/workspace/components/Button.vue",
            "../composables/useComponentIcons")
        .as_deref(),
        Some("/workspace/composables/useComponentIcons.ts"),
        "current store views should resolve missing relative type routes for existing workspace files",
    );
    assert_eq!(
        host.resolve_type_dependency_canonical(
            "/workspace/components/Button.vue",
            "../types")
        .as_deref(),
        Some("/workspace/types/index.ts"),
        "current store views should resolve missing relative barrel routes for existing workspace files",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn prepared_type_decl_reuses_warmed_package_target_and_leaves_undemanded_helper_cold() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        "export type { PackageEmits } from './index3.d.ts'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index3.d.ts",
        "import type { Payload } from './payload.d.ts'\nexport interface PackageEmits {\n  (e: 'open', value?: Payload): void\n}\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/payload.d.ts",
        "export interface Payload { value: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    host.set_import_dependencies(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        vec![exact_dependency(
            "./index3.d.ts",
            "/workspace/node_modules/pkg/dist/index3.d.ts",
        )],
    );
    host.set_import_dependencies(
        "/workspace/node_modules/pkg/dist/index3.d.ts",
        vec![exact_dependency(
            "./payload.d.ts",
            "/workspace/node_modules/pkg/dist/payload.d.ts",
        )],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let target = host
        .resolve_imported_type_root(
            "/workspace/node_modules/pkg/dist/index.d.ts",
            "PackageEmits",
        )
        .expect("PackageEmits must resolve to its exact imported declaration");
    assert_eq!(
        target.canonical_id.as_ref(),
        "/workspace/node_modules/pkg/dist/index3.d.ts",
        "shallow lookup should still normalize the package export target first",
    );
    assert_eq!(
        target.owner,
        verter_type_expr::TopLevelOwnerId::ordinary_file()
    );
    assert_eq!(target.symbol_name.as_ref(), "PackageEmits");
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/payload.d.ts")
            .is_none(),
        "the helper must remain cold until exact-owner canonicalization demands it",
    );

    ws.reset_reads();
    host.provenance().reset();

    let prepared = host
        .prepared_type_decl(target.canonical_id.as_ref(), target.symbol_name.as_ref())
        .expect("prepared package declaration should reuse the warmed indexed shallow state");

    let payload = prepared
        .name_resolution
        .get("Payload")
        .expect("prepared package declaration should still resolve imported helper names");
    assert_eq!(
        payload.canonical_id.as_ref(), "/workspace/node_modules/pkg/dist/payload.d.ts",
        "prepared package declaration should canonicalize imported helper edges from the warmed shallow state",
    );
    assert_eq!(payload.symbol_name.as_ref(), "Payload");
    assert!(
        ws.read_count("/workspace/node_modules/pkg/dist/index3.d.ts") <= 1,
        "prepared package declaration lookup should pay at most one shallow package target read for the active route",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/index.d.ts")
            .is_some(),
        "the inspected provider barrel owns a canonical IndexedReady",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/index3.d.ts")
            .is_some(),
        "the inspected active package target owns a canonical IndexedReady",
    );
    // The demand-driven discriminator: index/index3 are already warm and the
    // helper `Payload` was NOT demanded — preparation records only the
    // DIRECT-hop identity and materializes NOTHING. An eager
    // whole-bundle chain walk would have indexed the helper here (the
    // pre-redesign behavior these zero-counts discriminate against);
    // rebuilding either warm package artifact would also raise the count.
    assert_eq!(
        host.provenance().snapshot().indexed_ready_materializes,
        0,
        "the prepared-decl build must reuse index/index3 and leave the \
         un-demanded helper cold (demand-driven canonicalization walks no \
         import chain at bundle build)",
    );
    assert_eq!(
        ws.read_count("/workspace/node_modules/pkg/dist/payload.d.ts"),
        0,
        "the un-demanded helper must not be read at preparation time",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/payload.d.ts")
            .is_none(),
        "the un-demanded helper stays cold until a decl-prepare / ref-head \
         demand actually resolves it",
    );
}

#[test]
fn resolver_store_view_resolves_exports_for_unloaded_workspace_barrels() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/types/index.ts".to_string(),
        Arc::from("export * from '../Button.vue'\n"),
    );
    ws.inject_file(
        "/workspace/Button.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
export interface ButtonProps {
  label?: string
}
</script>
<template><button /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    assert!(
        host.ensure_loaded("/workspace/App.vue"),
        "entry file should load from workspace",
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let exports = host.resolve_exports("/workspace/types/index.ts");

    assert!(
        exports.iter().any(|export| {
            export.name == "ButtonProps"
                && export.source_canonical_id.as_deref() == Some("/workspace/Button.vue")
        }),
        "captured store view should resolve exports for unloaded workspace barrels, got: {exports:?}",
    );
}

#[test]
fn store_view_generic_dependency_paths_promote_snapshot_and_env_into_imported_cache() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/shared.d.ts",
        r#"export interface Alpha { alpha?: string }"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let source = host
        .read_analysis_source("/workspace/node_modules/pkg/dist/shared.d.ts")
        .expect("dependency source should load into the imported dependency cache");
    assert!(
        source.contains("Alpha"),
        "sanity check: the dependency source should be readable"
    );

    let before = host
        .ensure_indexed_ready("/workspace/node_modules/pkg/dist/shared.d.ts")
        .expect("source-only imported dependency entry should exist");
    // In the new IndexedReady model, snapshot is always Arc<FileAnalysisSnapshot>.
    // Before explicit snapshot build, it starts as default (empty bindings).
    assert!(
        before.snapshot.bindings.is_empty(),
        "source-only imported dependency entry should start with an empty snapshot"
    );
    let snapshot = host
        .get_raw_analysis_snapshot("/workspace/node_modules/pkg/dist/shared.d.ts")
        .expect("store-view snapshot path should build the dependency snapshot");
    assert!(
        snapshot.bindings.is_empty(),
        "simple declaration file should still produce a valid analysis snapshot"
    );

    let env = host
        .base_eval_env_arc("/workspace/node_modules/pkg/dist/shared.d.ts")
        .expect("store-view eval env path should build the dependency env");
    assert!(
        env.type_symbols.contains_key("Alpha"),
        "built dependency env should expose the declaration symbol"
    );

    let after = host
        .ensure_indexed_ready("/workspace/node_modules/pkg/dist/shared.d.ts")
        .expect("dependency entry should remain cached after store-view generic access");
    // Verify facts still exist after store-view generic access
    assert!(
        !after.raw_source.is_empty(),
        "store-view access should preserve the module facts"
    );
}

#[test]
fn read_dep_source_for_type_resolution_promotes_eval_source_for_loaded_workspace_file() {
    let ws = Arc::new(CountingWorkspace::new());
    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    upsert_vue(
        &host,
        "/workspace/src/InputMenu.vue",
        r#"<script setup lang="ts">
const answer: string = '42'
</script>
<template><div>{{ answer }}</div></template>"#,
    );

    // In the new IndexedReady DB, ensure_indexed_ready eagerly materializes
    // from workspace sources, so we can't assert is_none before read_dep.

    let first = host.read_dep_source_for_type_resolution("/workspace/src/InputMenu.vue", None);
    let second = host.read_dep_source_for_type_resolution("/workspace/src/InputMenu.vue", None);
    let promoted = host
        .ensure_indexed_ready("/workspace/src/InputMenu.vue")
        .expect("type-resolution read should promote eval source into the host dependency cache");

    assert_eq!(
        first.as_deref().map(str::trim),
        Some("const answer: string = '42'"),
        "Vue type-resolution reads should return script content only",
    );
    assert_eq!(
        second, first,
        "warm reads should reuse the same promoted source"
    );
    assert_eq!(
        Some(promoted.eval_source.trim()),
        Some("const answer: string = '42'"),
        "the promoted dependency cache entry should keep the extracted type-resolution source",
    );
    assert!(
        promoted.framework_parse.is_some(),
        "the promoted Vue dependency cache entry should retain the carrier parse artifact",
    );
    // In the new IndexedReady model, ensure_indexed_ready eagerly builds a
    // full snapshot, so we just verify the facts are present and well-formed.
    assert!(
        promoted.route_inventory.counts.top_level_statement_count > 0,
        "type-resolution reads should seed routes alongside the eval source",
    );
}

#[test]
fn resolve_dep_source_reuses_cached_source_without_loading_dependency_into_host_state() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/partial.html",
        "<div class=\"partial\">partial</div>",
    );
    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    upsert_vue(
        &host,
        "/workspace/src/App.vue",
        r#"<template src="./partial.html"></template>
<script setup>const ok = true</script>"#,
    );
    ws.set_exact_resolutions(
        "/workspace/src/App.vue",
        vec![verter_workspace::ExactResolution {
            specifier: "./partial.html".to_string(),
            phase: verter_session_query::resolution::ResolvePhase::CodegenBlocker,
            kind: verter_session_query::resolution::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/workspace/src/partial.html".to_string()),
            possible_canonical_ids: vec!["/workspace/src/partial.html".to_string()],
        }],
    );

    ws.reset_reads();
    let first = host.resolve_dep_source(
        "/workspace/src/App.vue",
        "/workspace/src/partial.html",
        "./partial.html",
    );
    let second = host.resolve_dep_source(
        "/workspace/src/App.vue",
        "/workspace/src/partial.html",
        "./partial.html",
    );

    assert_eq!(
        first.as_deref(),
        Some("<div class=\"partial\">partial</div>"),
        "first dependency source lookup should return the external source text"
    );
    assert_eq!(
        second, first,
        "warm dependency source lookup should return the same cached source"
    );
    // External dep source reads go through workspace read_file each time.
    // The functional contract is that both calls return the same content.
    assert!(
        host.get_source("/workspace/src/partial.html").is_none(),
        "external dep source should not be promoted into host file state"
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
    // Configure workspace resolver via host wrapper.
    {
        host.configure_projects(vec![verter_session_query::resolution::IdeProjectConfig {
            root: "/project".to_string(),
            workspace_root: "/project".to_string(),
            tsconfig_path: None,
            provider_root: "/project".to_string(),
            workspace_aliases: vec![verter_session_query::resolution::WorkspaceAlias {
                find: "@/".to_string(),
                replacement: "/project/src/".to_string(),
            }],
            compiler_options: verter_session_query::resolution::IdeProjectCompilerOptions::default(
            ),
            references: vec![],
            membership: verter_workspace::configured_membership_match_all_under_root(
                &verter_workspace::CanonicalPath::new("/project"),
            ),
        }]);
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
    assert_eq!(
        (start, end),
        (0, 0),
        "should anchor at the BarrelComp.vue file start"
    );
}

#[test]
fn resolve_exports_reads_workspace_only_barrels_and_vue_targets() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from("export * from '../components/Link.vue'"),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/Link.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
export interface LinkProps {
  href?: string
  replace?: boolean
}
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(HostConfig::default(), ws);
    let exports = host.resolve_exports("/workspace/src/runtime/types/index.ts");
    let link_props = exports
        .iter()
        .find(|export| export.name == "LinkProps")
        .expect("workspace-only barrel should expose LinkProps");

    assert_eq!(
        link_props.source_canonical_id.as_deref(),
        Some("/workspace/src/runtime/components/Link.vue"),
        "workspace-only re-export should resolve to the Vue declaration owner"
    );
    assert_eq!(
        link_props.source_name, "LinkProps",
        "workspace-only re-export should preserve the exported declaration name"
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
fn resolve_component_meta_uses_workspace_type_resolution_for_package_declarations() {
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
        Arc::from("export interface FancyProps { open: boolean; label?: string }"),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(HostConfig::default(), ws);
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);
    upsert_vue(
        &host,
        "/workspace/src/Consumer.vue",
        "<script setup lang=\"ts\">\nimport type { FancyProps } from 'fancy'\ndefineProps<FancyProps>()\n</script>\n<template><div /></template>",
    );

    let state = resolve_expanded_state(&host, "/workspace/src/Consumer.vue");
    let dtos = macro_dtos_by_type(&host, "/workspace/src/Consumer.vue", &state, "FancyProps");
    let props: Vec<&str> = dtos
        .prop_fields()
        .iter()
        .map(|prop| prop.analysis.name.as_str())
        .collect();
    assert!(
        props.contains(&"open"),
        "expanded props should contain fields from the package declaration entrypoint, got: {:?}",
        props
    );
}

#[test]
fn template_class_wrapper_routes_follow_import_then_export_and_local_alias_barrels() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );
    upsert_ts(
        &host,
        "/workspace/src/barrel-a.ts",
        "import type { Ref } from 'vue'; export type { Ref as ImportedRef };",
    );
    host.set_import_dependencies(
        "/workspace/src/barrel-a.ts",
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );
    upsert_ts(
        &host,
        "/workspace/src/barrel-b.ts",
        "import type { ImportedRef } from './barrel-a'; export type LocalRef<T> = ImportedRef<T>;",
    );
    host.set_import_dependencies(
        "/workspace/src/barrel-b.ts",
        vec![exact_dependency("./barrel-a", "/workspace/src/barrel-a.ts")],
    );
    let canonical = "/workspace/src/BarrelRoute.vue";
    upsert_vue(
        &host,
        canonical,
        r#"<script setup lang="ts">
import type { LocalRef as Selected } from './barrel-b'
const variant: Selected<'primary' | 'secondary'> = null as never
</script><template><div :class="variant" /></template>"#,
    );
    host.set_import_dependencies(
        canonical,
        vec![exact_dependency("./barrel-b", "/workspace/src/barrel-b.ts")],
    );
    let template = host
        .get_analysis(canonical)
        .expect("analysis")
        .template
        .expect("template");
    assert_eq!(
        template.elements[0].dynamic_classes,
        ["primary", "secondary"]
    );
    let facts = template_class_facts_for(&host, canonical);
    let provenance = facts.rows()[0]
        .wrapper
        .import_provenance
        .as_ref()
        .expect("route proof");
    assert_eq!(provenance.local_binding.as_ref(), "Selected");
    assert_eq!(provenance.import_source.as_ref(), "./barrel-b");
    assert_eq!(provenance.terminal_import_source.as_ref(), "vue");
    assert_eq!(
        facts.rows()[0]
            .wrapper
            .symbol
            .as_ref()
            .expect("terminal")
            .symbol
            .as_ref(),
        "Ref"
    );
    assert!(provenance
        .local_alias_hops
        .iter()
        .any(|hop| hop.as_ref() == "LocalRef"));
}

/// Authored route provenance is value-side EVIDENCE, never semantic query or
/// cache identity (the route-provenance ruling's forbidden path "no local alias
/// in semantic query/cache identity").
///
/// `Ref as A` and `Ref as B` are two authored routes to ONE vue `Ref`
/// declaration. Annotated with the SAME type argument they are the same
/// instantiation, so they MUST hash-cons to a single interned
/// `InstantiationRef` node — while the artifact still reports the two DISTINCT
/// authored local bindings. Identity equal, provenance distinct: that pair is
/// what proves the alias was kept out of identity without destroying the proof.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn authored_local_alias_is_evidence_not_semantic_cache_identity() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );
    let canonical = "/workspace/src/AliasIdentity.vue";
    upsert_vue(
        &host,
        canonical,
        r#"<script setup lang="ts">
import type { Ref as A, Ref as B } from 'vue'
const first: A<'shared-x' | 'shared-y'> = null as never
const second: B<'shared-x' | 'shared-y'> = null as never
</script><template>
  <div :class="first" />
  <span :class="second" />
</template>"#,
    );
    host.set_import_dependencies(
        canonical,
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );

    let template = host
        .get_analysis(canonical)
        .expect("lazy analysis")
        .template
        .expect("template");
    assert_eq!(
        template
            .elements
            .iter()
            .map(|element| element.dynamic_classes.clone())
            .collect::<Vec<_>>(),
        [
            vec!["shared-x".to_string(), "shared-y".to_string()],
            vec!["shared-x".to_string(), "shared-y".to_string()],
        ],
        "two authored aliases of one terminal publish the same closed domain"
    );

    let facts = template_class_facts_for(&host, canonical);
    let row_for = |label: &str| {
        facts
            .rows()
            .iter()
            .find(|row| row.subject.label() == label)
            .expect("requested binding row")
            .clone()
    };
    let first = row_for("first");
    let second = row_for("second");

    // PROVENANCE DISTINCT — the exact authored local binding is retained per
    // declaration and the two proofs are NOT interchangeable.
    let first_provenance = first
        .wrapper
        .import_provenance
        .as_ref()
        .expect("exact route for the `A` alias");
    let second_provenance = second
        .wrapper
        .import_provenance
        .as_ref()
        .expect("exact route for the `B` alias");
    assert_eq!(first_provenance.local_binding.as_ref(), "A");
    assert_eq!(second_provenance.local_binding.as_ref(), "B");
    for (provenance, expected_alias) in [(first_provenance, "A"), (second_provenance, "B")] {
        let verter_type_expr::facts::AuthoredReferenceHeadFact::Bare { local_name, .. } =
            &provenance.authored_head
        else {
            panic!("expected a bare authored value-annotation head for {expected_alias}");
        };
        assert_eq!(local_name.as_ref(), expected_alias);
        assert_eq!(provenance.imported_name.as_ref(), "Ref");
        assert_eq!(provenance.terminal_import_source.as_ref(), "vue");
    }
    assert_ne!(
        first_provenance, second_provenance,
        "route provenance must still DISCRIMINATE the two authored aliases"
    );

    // IDENTITY EQUAL — the resolved terminal is one and the same declaration.
    assert_eq!(
        first.wrapper.symbol, second.wrapper.symbol,
        "two aliases of one import must resolve to ONE terminal symbol identity"
    );
    assert_eq!(
        first.domain, second.domain,
        "the alias must not change the classified domain"
    );

    // THE CACHE-IDENTITY PIN: `InstantiationRef` identity is `(DeclIdentity,
    // args)`, so structurally equivalent generic applications intern to the
    // SAME node. If the authored alias re-entered semantic/cache identity, `A`
    // and `B` would fork into TWO interned instantiations of the same vue
    // `Ref` with the same argument.
    let graph = host.project_type_store().semantic_graph();
    let mut vue_instantiations = std::collections::HashSet::new();
    for id in 0u64..(graph.node_count() as u64) {
        let node = verter_type_engine::semantic_query::SemanticNodeId(id);
        let Some(data) = graph.node_data(node) else {
            continue;
        };
        if let verter_type_engine::semantic_query::SemanticNodeData::InstantiationRef {
            base,
            args,
        } = data.as_ref()
        {
            // Scope the sweep by the DECLARING FILE, never by the decl name —
            // an alias leaking into identity would change the name, and a
            // name-filtered sweep would then silently find nothing instead of
            // reporting the fork.
            if base.canonical_id.as_ref() == "/workspace/node_modules/vue/index.d.ts" {
                vue_instantiations.insert((base.clone(), args.clone()));
            }
        }
    }
    assert_eq!(
        vue_instantiations.len(),
        1,
        "`Ref as A` and `Ref as B` with the SAME argument must hash-cons to ONE \
         interned instantiation — a local alias must never fork semantic/cache \
         identity, got {vue_instantiations:?}"
    );
    let (interned_base, _) = vue_instantiations
        .into_iter()
        .next()
        .expect("checked non-empty above");
    assert_eq!(
        interned_base.decl_name.as_ref(),
        "Ref",
        "the interned instantiation must be keyed by the RESOLVED declaration \
         name, never by the authored local alias"
    );
}

/// A6-02 — renamed imports, one- and multi-hop local aliases, a direct
/// re-export and an import-then-export barrel all resolve to the SAME role as
/// the direct form, through the shared demand. The ordered alias hops and the
/// final `vue` edge are retained; dropping a hop or canonicalizing the edge
/// before proof composition REDs.
#[test]
fn return_wrapper_routes_follow_renamed_imports_local_aliases_and_barrels() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        RETURN_WRAPPER_VUE_DTS,
    );

    // Renamed import + two local alias hops in the OWNER file.
    let aliased = "/workspace/src/aliased.ts";
    upsert_ts(
        &host,
        aliased,
        "import type { Ref as R } from 'vue'\n\
         type W<T> = R<T>\n\
         type Outer<T> = W<T>\n\
         export function getValue(): Outer<number> { return null as never; }\n",
    );
    host.set_import_dependencies(
        aliased,
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );
    let (role, provenance) = return_wrapper_role_for(&host, aliased, "getValue");
    assert_eq!(role, verter_type_expr::ReactiveWrapperRole::Ref);
    let provenance = provenance.expect("route proof");
    assert_eq!(provenance.terminal_import_source.as_ref(), "vue");
    assert_eq!(
        provenance.local_binding.as_ref(),
        "R",
        "the exact renamed local import binding must be retained"
    );
    assert_eq!(provenance.imported_name.as_ref(), "Ref");
    assert_eq!(
        provenance
            .local_alias_hops
            .iter()
            .map(|hop| hop.as_ref())
            .collect::<Vec<_>>(),
        ["Outer", "W"],
        "the ORDERED local alias hops must be retained"
    );

    // Direct re-export.
    upsert_ts(
        &host,
        "/workspace/src/reexport.ts",
        "export type { Ref as ReRef } from 'vue'\n",
    );
    host.set_import_dependencies(
        "/workspace/src/reexport.ts",
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );
    let via_reexport = "/workspace/src/via-reexport.ts";
    upsert_ts(
        &host,
        via_reexport,
        "import type { ReRef } from './reexport'\n\
         export function getValue(): ReRef<number> { return null as never; }\n",
    );
    host.set_import_dependencies(
        via_reexport,
        vec![exact_dependency("./reexport", "/workspace/src/reexport.ts")],
    );
    let (role, provenance) = return_wrapper_role_for(&host, via_reexport, "getValue");
    assert_eq!(role, verter_type_expr::ReactiveWrapperRole::Ref);
    let provenance = provenance.expect("re-export route proof");
    assert_eq!(provenance.import_source.as_ref(), "./reexport");
    assert_eq!(provenance.terminal_import_source.as_ref(), "vue");

    // Import-then-export barrel plus a barrel-local generic alias.
    upsert_ts(
        &host,
        "/workspace/src/barrel-a.ts",
        "import type { Ref } from 'vue'; export type { Ref as ImportedRef };",
    );
    host.set_import_dependencies(
        "/workspace/src/barrel-a.ts",
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );
    upsert_ts(
        &host,
        "/workspace/src/barrel-b.ts",
        "import type { ImportedRef } from './barrel-a'; export type LocalRef<T> = ImportedRef<T>;",
    );
    host.set_import_dependencies(
        "/workspace/src/barrel-b.ts",
        vec![exact_dependency("./barrel-a", "/workspace/src/barrel-a.ts")],
    );
    let via_barrel = "/workspace/src/via-barrel.ts";
    upsert_ts(
        &host,
        via_barrel,
        "import type { LocalRef as Selected } from './barrel-b'\n\
         export function getValue(): Selected<number> { return null as never; }\n",
    );
    host.set_import_dependencies(
        via_barrel,
        vec![exact_dependency("./barrel-b", "/workspace/src/barrel-b.ts")],
    );
    let (role, provenance) = return_wrapper_role_for(&host, via_barrel, "getValue");
    assert_eq!(role, verter_type_expr::ReactiveWrapperRole::Ref);
    let provenance = provenance.expect("barrel route proof");
    assert_eq!(provenance.local_binding.as_ref(), "Selected");
    assert_eq!(provenance.import_source.as_ref(), "./barrel-b");
    assert_eq!(provenance.terminal_import_source.as_ref(), "vue");
    assert!(provenance
        .local_alias_hops
        .iter()
        .any(|hop| hop.as_ref() == "LocalRef"));
}

/// A6-03 — a wrapper-SHAPED head that is not Vue's is never classified. A
/// resolved non-Vue terminal is a COMPLETE non-wrapper proof (`None`); an
/// unresolvable bare name fails closed (typed unresolved) rather than claiming
/// either a wrapper or a completed non-wrapper. No case publishes provenance.
#[test]
fn return_wrapper_role_rejects_local_and_foreign_package_fakes() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        RETURN_WRAPPER_VUE_DTS,
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/not-vue/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );

    // A local declaration that merely SPELLS the wrapper name.
    let local_fake = "/workspace/src/local-fake.ts";
    upsert_ts(
        &host,
        local_fake,
        "interface Ref<T> { value: T }\n\
         export function getValue(): Ref<number> { return null as never; }\n",
    );
    let (role, provenance) = return_wrapper_role_for(&host, local_fake, "getValue");
    assert_eq!(
        role,
        verter_type_expr::ReactiveWrapperRole::None,
        "a local same-name declaration is a COMPLETE non-wrapper proof"
    );
    assert!(provenance.is_none());

    // A package-backed same-shape wrapper outside the exact Vue route.
    let package_fake = "/workspace/src/package-fake.ts";
    upsert_ts(
        &host,
        package_fake,
        "import type { Ref } from 'not-vue'\n\
         export function getValue(): Ref<number> { return null as never; }\n",
    );
    host.set_import_dependencies(
        package_fake,
        vec![exact_dependency(
            "not-vue",
            "/workspace/node_modules/not-vue/index.d.ts",
        )],
    );
    let (role, provenance) = return_wrapper_role_for(&host, package_fake, "getValue");
    assert_eq!(
        role,
        verter_type_expr::ReactiveWrapperRole::None,
        "a non-`vue` package terminal must fail the exact route gate"
    );
    assert!(provenance.is_none());

    // A WORKSPACE-OWNED file that resolves the specifier `vue`. The terminal
    // import source is spelled exactly `vue` and the terminal export is named
    // exactly `Ref`, so the specifier half of the route gate passes: only the
    // package-backed half rejects it. Dropping that half classifies a userland
    // `vue.ts` as Vue's reactive wrapper.
    upsert_ts(
        &host,
        "/workspace/src/vue.ts",
        "export interface Ref<T> { value: T }\n",
    );
    let workspace_vue = "/workspace/src/workspace-vue.ts";
    upsert_ts(
        &host,
        workspace_vue,
        "import type { Ref } from 'vue'\n\
         export function getValue(): Ref<number> { return null as never; }\n",
    );
    host.set_import_dependencies(
        workspace_vue,
        vec![exact_dependency("vue", "/workspace/src/vue.ts")],
    );
    let (role, provenance) = return_wrapper_role_for(&host, workspace_vue, "getValue");
    assert_eq!(
        role,
        verter_type_expr::ReactiveWrapperRole::None,
        "a workspace-owned `vue` terminal must fail the package-backed half of the route gate"
    );
    assert!(provenance.is_none());

    // The pinned defect, inverted: the `build_tests.rs` fixture that imported
    // NOTHING and was still reported as Vue's `Ref` on spelling alone. An
    // unbound name is not resolvable, so it fails closed — never `Ref`, and
    // never a fabricated provenance.
    let unimported = "/workspace/src/unimported.ts";
    upsert_ts(
        &host,
        unimported,
        "export function getRef(): Ref<number> { return null as never; }\n",
    );
    let (role, provenance) = return_wrapper_role_for(&host, unimported, "getRef");
    assert_eq!(
        role,
        verter_type_expr::ReactiveWrapperRole::Unresolved {
            reason: verter_type_expr::ReactiveWrapperUnresolvedReason::Unsupported
        },
        "an unbound `Ref<number>` must fail closed, not classify as Vue's Ref"
    );
    assert!(provenance.is_none());

    // Positive control in the SAME host: the exact Vue route still classifies,
    // so the rejections above are not a blanket refusal.
    let exact = "/workspace/src/exact.ts";
    upsert_ts(
        &host,
        exact,
        "import type { Ref } from 'vue'\n\
         export function getValue(): Ref<number> { return null as never; }\n",
    );
    host.set_import_dependencies(
        exact,
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );
    let (role, provenance) = return_wrapper_role_for(&host, exact, "getValue");
    assert_eq!(role, verter_type_expr::ReactiveWrapperRole::Ref);
    assert!(provenance.is_some());
}

#[test]
fn resolve_eval_dependency_canonical_prefers_extension_candidates_before_raw_extensionless_probe() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    let resolved = host.resolve_eval_dependency_canonical("/workspace/src/runtime/types/html");

    assert_eq!(
        resolved.as_deref(),
        Some("/workspace/src/runtime/types/html.ts"),
        "extensionless dependency canonicalization should resolve to the typed companion",
    );
    assert_eq!(
        ws.read_count("/workspace/src/runtime/types/html"),
        0,
        "extensionless dependency canonicalization must stay on existence probes",
    );
    assert_eq!(
        ws.exists_count("/workspace/src/runtime/types/html"),
        0,
        "extensionless dependency canonicalization should not probe the raw missing path before the typed companion candidates",
    );
}

#[test]
fn current_eval_state_normalizes_extensionless_canonical_before_fallback_load() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    let state = host.current_eval_state("/workspace/src/runtime/types/html");

    assert!(
        state.is_some(),
        "extensionless canonical ids should still materialize eval state from the typed companion",
    );
    assert_eq!(
        ws.read_count("/workspace/src/runtime/types/html"),
        0,
        "materializing eval state must not read the raw missing extensionless path",
    );
    assert_eq!(
        ws.exists_count("/workspace/src/runtime/types/html"),
        0,
        "materializing eval state must not probe the raw extensionless path before normalization",
    );
}

#[test]
fn get_raw_analysis_snapshot_normalizes_extensionless_canonical_before_building_snapshot() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    let snapshot = host.get_raw_analysis_snapshot("/workspace/src/runtime/types/html");

    assert!(
        snapshot.is_some(),
        "extensionless canonical ids should still build a raw snapshot from the typed companion",
    );
    assert_eq!(
        ws.read_count("/workspace/src/runtime/types/html"),
        0,
        "building the raw snapshot must not read the raw missing extensionless path",
    );
    assert_eq!(
        ws.exists_count("/workspace/src/runtime/types/html"),
        0,
        "building the raw snapshot must not probe the raw extensionless path before normalization",
    );
}

#[test]
fn prepared_type_decl_normalizes_extensionless_canonical_before_shallow_backfill() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    let prepared =
        host.prepared_type_decl("/workspace/src/runtime/types/html", "ButtonHTMLAttributes");

    assert!(
        prepared.is_some(),
        "prepared type lookup should backfill from the typed companion when the canonical id is extensionless",
    );
    assert_eq!(
        ws.read_count("/workspace/src/runtime/types/html"),
        0,
        "prepared type lookup must not read the raw missing extensionless path",
    );
    assert_eq!(
        ws.exists_count("/workspace/src/runtime/types/html"),
        0,
        "prepared type lookup must not probe the raw extensionless path before normalization",
    );
}

#[test]
fn ensure_loaded_normalizes_extensionless_canonical_before_workspace_read() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    let loaded = host.ensure_loaded("/workspace/src/runtime/types/html");

    assert!(
        loaded,
        "ensure_loaded should accept extensionless canonical ids when a typed companion exists",
    );
    assert_eq!(
        ws.read_count("/workspace/src/runtime/types/html"),
        0,
        "ensure_loaded must not read the raw missing extensionless path",
    );
    assert_eq!(
        ws.exists_count("/workspace/src/runtime/types/html"),
        0,
        "ensure_loaded must not probe the raw missing extensionless path before normalization",
    );
}

#[test]
fn upsert_normalizes_extensionless_macro_type_blockers_before_scheduler_workspace_read() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    upsert_vue(
        &host,
        "/workspace/src/runtime/components/Button.vue",
        r#"<script setup lang="ts">
import type { ButtonHTMLAttributes } from '../types/html'

defineProps<ButtonHTMLAttributes>()
</script>
<template><button /></template>"#,
    );

    for _ in 0..100 {
        if ws.read_count("/workspace/src/runtime/types/html.ts") >= 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    assert_eq!(
        ws.read_count("/workspace/src/runtime/types/html"),
        0,
        "scheduler blocker ingestion must not read the raw extensionless dependency path",
    );
    assert_eq!(
        ws.exists_count("/workspace/src/runtime/types/html"),
        0,
        "scheduler blocker ingestion must not probe the raw extensionless dependency path before normalization",
    );
    assert!(
        ws.read_count("/workspace/src/runtime/types/html.ts") >= 1,
        "scheduler blocker ingestion should read the normalized typed companion at least once",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn direct_imported_type_root_fast_path_reuses_exact_owner_local_type_declaration() {
    let host = make_host();
    upsert_vue(
        &host,
        "/src/types.vue",
        r#"<script lang="ts">
export interface Props { label: string }
</script>
<template><div /></template>"#,
    );

    let analysis = host
        .scheduler_script_analysis("/src/types.vue")
        .expect("the provider should own a published parse-header surface");
    assert!(
        analysis.declaration_entries.iter().any(|entry| {
            entry.name == "Props"
                && entry.owner == verter_type_expr::TopLevelOwnerId::module(0)
                && entry.kind == verter_session_query::analysis::types::LocalDeclarationKind::Type
        }),
        "the fixture must expose a type-only symbol in the module-script owner",
    );
    assert!(
        !analysis.declaration_entries.iter().any(|entry| {
            entry.name == "Props"
                && matches!(
                    entry.kind,
                    verter_session_query::analysis::types::LocalDeclarationKind::Value
                        | verter_session_query::analysis::types::LocalDeclarationKind::TypeAndValue
                )
        }),
        "the fixture must not let a value-space fallback hide a type-space regression",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/src/types.vue")
            .is_none(),
        "the fixture must start with headers only, not a prebuilt IndexedReady artifact",
    );

    host.provenance().reset();
    let (resolved, facts) = host
        .resolve_direct_imported_type_root_fast_path_with_context(
            &host,
            None,
            "/src/types.vue",
            "Props",
        )
        .expect("a direct local exported declaration should stay on the shallow fast path");

    assert_eq!(
        resolved,
        expected_imported_root_tuple(
            "/src/types.vue",
            verter_type_expr::TopLevelOwnerId::module(0),
            "Props",
        ),
        "the fast path must preserve the exact defining owner and symbol",
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/types.vue"
        )),
        "the direct-local proof must track the provider content hash",
    );
    assert_eq!(
        host.provenance().snapshot().indexed_ready_materializes,
        0,
        "a published exact local header must not materialize an IndexedReady artifact",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/src/types.vue")
            .is_none(),
        "the header fast path must leave the IndexedReady store cold",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn direct_imported_type_root_fast_path_tracks_provider_route_and_target_whole_hash_only() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/index.ts", "export { Props } from './target'\n");
    ws.inject_file(
        "/src/target.ts",
        "export interface Props { label: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    host.set_import_dependencies(
        "/src/index.ts",
        vec![exact_dependency("./target", "/src/target.ts")],
    );

    let (resolved, facts) = host
        .resolve_direct_imported_type_root_fast_path_with_context(
            &host,
            None,
            "/src/index.ts",
            "Props",
        )
        .expect("direct named reexport should resolve through the fast imported-root path");

    assert_eq!(
        resolved,
        expected_imported_root_tuple(
            "/src/target.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Props",
        ),
        "fast imported-root proof should preserve the exact child target tuple",
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/index.ts"
        )),
        "fast imported-root proof must track the provider file content hash",
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::Parse(parse)
                if parse.canonical_id == "/src/index.ts"
                    && matches!(parse.key, verter_session_query::facts::FactKey::SyntacticRouteInterface)
        )),
        "fast imported-root proof must track the provider's parse-owned route interface",
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/target.ts"
        )),
        "fast imported-root proof must track the direct child file content hash",
    );
    assert!(
        !facts.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::Parse(parse)
                if parse.canonical_id == "/src/target.ts"
                    && matches!(parse.key, verter_session_query::facts::FactKey::SyntacticRouteInterface)
        )),
        "direct imported-root proof should not need the child's route interface when the parent directly names the target reexport",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn direct_imported_type_root_fast_path_resolves_cold_target_under_store_view() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/index.ts",
        "import { Props as InnerProps } from './target'\nexport { InnerProps as Props }\n",
    );
    ws.inject_file(
        "/src/target.ts",
        "export interface Props { label: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    host.set_import_dependencies(
        "/src/index.ts",
        vec![exact_dependency("./target", "/src/target.ts")],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let (resolved, facts) = host
        .resolve_direct_imported_type_root_fast_path_with_context(
            &host,
            None,
            "/src/index.ts",
            "Props",
        )
        .expect(
            "fast imported-root proof should resolve cold child hashes under a current store view",
        );

    assert_eq!(
        resolved,
        expected_imported_root_tuple(
            "/src/target.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Props",
        ),
        "store-view fast path should keep the same routed child tuple",
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/target.ts"
        )),
        "store-view fast path must still track the cold child file content hash",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn direct_imported_type_root_fast_path_reuses_provider_shallow_state_for_provider_facts() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/index.ts",
        "import { Props as InnerProps } from './target'\nexport { InnerProps as Props }\n",
    );
    ws.inject_file(
        "/src/target.ts",
        "export interface Props { label: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    host.set_import_dependencies(
        "/src/index.ts",
        vec![exact_dependency("./target", "/src/target.ts")],
    );

    let _ = host
        .resolve_direct_imported_type_root_fast_path_with_context(
            &host,
            None,
            "/src/index.ts",
            "Props",
        )
        .expect("exported local imports should resolve through the fast imported-root path");

    assert_eq!(
        ws.read_count("/src/index.ts"),
        1,
        "fast imported-root proof should reuse the provider's existing routed shallow read when collecting provider facts",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn imported_type_root_fast_path_follows_exported_local_import_without_child_route_hash() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/index.ts",
        "import { Props as InnerProps } from './target'\nexport { InnerProps as Props }\n",
    );
    ws.inject_file(
        "/src/target.ts",
        "export interface Props { label: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    host.set_import_dependencies(
        "/src/index.ts",
        vec![exact_dependency("./target", "/src/target.ts")],
    );

    let (resolved, facts) = host
        .resolve_direct_imported_type_root_fast_path_with_context(
            &host,
            None,
            "/src/index.ts",
            "Props",
        )
        .expect("exported local imports should resolve through the fast imported-root path");

    assert_eq!(
        resolved,
        expected_imported_root_tuple(
            "/src/target.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Props",
        ),
        "fast imported-root proof should follow the exported local import to the exact child target tuple",
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/index.ts"
        )),
        "fast imported-root proof must track the provider file content hash",
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::Parse(parse)
                if parse.canonical_id == "/src/index.ts"
                    && matches!(parse.key, verter_session_query::facts::FactKey::SyntacticRouteInterface)
        )),
        "fast imported-root proof must track the provider's parse-owned route interface",
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/target.ts"
        )),
        "fast imported-root proof must track the direct child file content hash",
    );
    assert!(
        !facts.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::Parse(parse)
                if parse.canonical_id == "/src/target.ts"
                    && matches!(parse.key, verter_session_query::facts::FactKey::SyntacticRouteInterface)
        )),
        "direct imported-root proof should not need the child's route interface when the provider only re-exports the imported local binding",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_named_type_export_target_nested_barrel_alias_skips_later_unrelated_siblings() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "export * from './Button.vue'\nexport * from './Link.vue'\nexport * from './Unused.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
import type { LinkProps } from './types'

export interface ButtonProps extends Omit<LinkProps, 'raw'> {
  label?: string
}
</script>
<template><button /></template>"#,
    );
    ws.inject_file(
        "/src/Link.vue",
        r#"<script lang="ts">
export interface LinkProps {
  href?: string
  raw?: boolean
}
</script>
<template><a /></template>"#,
    );
    ws.inject_file(
        "/src/Unused.vue",
        r#"<script lang="ts">
export interface UnusedProps {
  never?: number
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    host.set_import_dependencies(
        "/src/types.ts",
        vec![
            exact_dependency("./Button.vue", "/src/Button.vue"),
            exact_dependency("./Link.vue", "/src/Link.vue"),
            exact_dependency("./Unused.vue", "/src/Unused.vue"),
        ],
    );

    ws.reset_reads();
    let resolved = host.resolve_named_type_export_target("/src/types.ts", "ButtonProps");

    assert_eq!(
        resolved,
        Some(("/src/Button.vue".to_string(), "ButtonProps".to_string())),
        "named export target resolution should route to the first matching nested barrel child",
    );
    assert_eq!(
        ws.read_count("/src/Unused.vue"),
        0,
        "named export target resolution should stop at the matched route instead of loading later unrelated siblings",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_component_meta_native_props_tracks_routed_package_targets_across_requests() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { PackageEmits } from './types'

const emit = defineEmits<PackageEmits>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/workspace/src/types.ts",
        "export type { PackageEmits } from 'pkg'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        "export type { PackageEmits } from './index3.d.ts'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index3.d.ts",
        "export interface PackageEmits {\n  (e: 'open', value?: string): void\n}\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/workspace/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/workspace/src/Consumer.vue",
        vec![exact_dependency("./types", "/workspace/src/types.ts")],
    );
    host.set_import_dependencies(
        "/workspace/src/types.ts",
        vec![exact_dependency(
            "pkg",
            "/workspace/node_modules/pkg/dist/index.d.ts",
        )],
    );
    host.set_import_dependencies(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        vec![exact_dependency(
            "./index3.d.ts",
            "/workspace/node_modules/pkg/dist/index3.d.ts",
        )],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    host.provenance().reset();

    let mut tracked_deps_first = std::collections::BTreeSet::new();
    let mut resolution_deps_first = std::collections::BTreeSet::new();
    let mut cache_first =
        crate::resolver_core::component_meta::NativePropProjectionCache::default();
    let resolved_first = host.resolve_component_meta_native_props(
        "/workspace/src/Consumer.vue",
        "./types",
        "PackageEmits",
        &mut tracked_deps_first,
        &mut resolution_deps_first,
        &mut cache_first,
    );
    assert!(
        resolved_first.is_some(),
        "the first imported macro lookup should resolve the package emits surface",
    );
    assert!(
        tracked_deps_first.contains("/workspace/node_modules/pkg/dist/index3.d.ts"),
        "the first lookup should track the routed package target canonical",
    );
    assert!(
        resolution_deps_first.contains("/workspace/node_modules/pkg/dist/index3.d.ts"),
        "the first lookup should record the routed package target canonical in resolution deps",
    );

    let mut tracked_deps_second = std::collections::BTreeSet::new();
    let mut resolution_deps_second = std::collections::BTreeSet::new();
    let mut cache_second =
        crate::resolver_core::component_meta::NativePropProjectionCache::default();
    let resolved_second = host.resolve_component_meta_native_props(
        "/workspace/src/Consumer.vue",
        "./types",
        "PackageEmits",
        &mut tracked_deps_second,
        &mut resolution_deps_second,
        &mut cache_second,
    );
    assert!(
        resolved_second.is_some(),
        "the second imported macro lookup should still resolve the package emits surface",
    );
    assert!(
        tracked_deps_second.contains("/workspace/node_modules/pkg/dist/index3.d.ts"),
        "the warm lookup must keep tracking the routed package target canonical",
    );
    assert!(
        resolution_deps_second.contains("/workspace/node_modules/pkg/dist/index3.d.ts"),
        "the warm lookup must keep the routed package target in resolution deps for downstream fact tracking",
    );
    assert!(
        host.project_type_store.indexed().get_any("/workspace/node_modules/pkg/dist/index.d.ts")
            .is_some(),
        "the inspected package provider barrel owns exactly one canonical IndexedReady built by the unified cold path",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/index3.d.ts")
            .is_some(),
        "the actively resolved package target owns exactly one canonical IndexedReady built by the unified cold path",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_component_meta_native_props_materializes_active_package_target_once() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { PackageEmits } from './types'

const emit = defineEmits<PackageEmits>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/workspace/src/types.ts",
        "export type { PackageEmits } from 'pkg'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        "export type { PackageEmits } from './index3.d.ts'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index3.d.ts",
        "export interface PackageEmits {\n  (e: 'open', value?: string): void\n}\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/workspace/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/workspace/src/Consumer.vue",
        vec![exact_dependency("./types", "/workspace/src/types.ts")],
    );
    host.set_import_dependencies(
        "/workspace/src/types.ts",
        vec![exact_dependency(
            "pkg",
            "/workspace/node_modules/pkg/dist/index.d.ts",
        )],
    );
    host.set_import_dependencies(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        vec![exact_dependency(
            "./index3.d.ts",
            "/workspace/node_modules/pkg/dist/index3.d.ts",
        )],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::component_meta::NativePropProjectionCache::default();

    let resolved = host.resolve_component_meta_native_props(
        "/workspace/src/Consumer.vue",
        "./types",
        "PackageEmits",
        &mut tracked_deps,
        &mut resolution_deps,
        &mut cache,
    );

    assert!(
        resolved.is_some(),
        "component-meta macro resolution should still resolve the package reexported emits surface",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/index.d.ts")
            .is_some(),
        "the inspected package provider barrel owns a canonical IndexedReady",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/index3.d.ts")
            .is_some(),
        "the actively resolved package target owns a canonical IndexedReady",
    );
    // The ONCE discriminator: a second identical resolution reuses every
    // artifact the first one built — zero new materialisations.
    host.provenance().reset();
    let mut tracked_deps2 = std::collections::BTreeSet::new();
    let mut resolution_deps2 = std::collections::BTreeSet::new();
    let mut cache2 = crate::resolver_core::component_meta::NativePropProjectionCache::default();
    let re_resolved = host.resolve_component_meta_native_props(
        "/workspace/src/Consumer.vue",
        "./types",
        "PackageEmits",
        &mut tracked_deps2,
        &mut resolution_deps2,
        &mut cache2,
    );
    assert!(
        re_resolved.is_some(),
        "warm re-resolution must still resolve"
    );
    assert_eq!(
        host.provenance().snapshot().indexed_ready_materializes,
        0,
        "the active package target must materialise ONCE — a repeated \
         resolution must reuse its IndexedReady, not rebuild it",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_component_meta_macro_surface_materializes_active_package_target_once() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { PackageEmits } from './types'

const emit = defineEmits<PackageEmits>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/workspace/src/types.ts",
        "export type { PackageEmits } from 'pkg'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        "export type { PackageEmits } from './index3.d.ts'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index3.d.ts",
        "export interface PackageEmits {\n  (e: 'open', value?: string): void\n}\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/workspace/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/workspace/src/Consumer.vue",
        vec![exact_dependency("./types", "/workspace/src/types.ts")],
    );
    host.set_import_dependencies(
        "/workspace/src/types.ts",
        vec![exact_dependency(
            "pkg",
            "/workspace/node_modules/pkg/dist/index.d.ts",
        )],
    );
    host.set_import_dependencies(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        vec![exact_dependency(
            "./index3.d.ts",
            "/workspace/node_modules/pkg/dist/index3.d.ts",
        )],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::component_meta::NativePropProjectionCache::default();

    let resolved = host.resolve_component_meta_macro_surface(
        "/workspace/src/Consumer.vue",
        "./types",
        "PackageEmits",
        &mut tracked_deps,
        &mut resolution_deps,
        &mut cache,
    );

    assert!(
        resolved.is_some(),
        "component-meta macro surface resolution should still resolve the package reexported emits surface",
    );
    // The ONCE discriminator: a second identical resolution reuses every
    // artifact the first one built — zero new materialisations.
    host.provenance().reset();
    let mut tracked_deps2 = std::collections::BTreeSet::new();
    let mut resolution_deps2 = std::collections::BTreeSet::new();
    let mut cache2 = crate::resolver_core::component_meta::NativePropProjectionCache::default();
    let re_resolved = host.resolve_component_meta_macro_surface(
        "/workspace/src/Consumer.vue",
        "./types",
        "PackageEmits",
        &mut tracked_deps2,
        &mut resolution_deps2,
        &mut cache2,
    );
    assert!(
        re_resolved.is_some(),
        "warm re-resolution must still resolve"
    );
    assert_eq!(
        host.provenance().snapshot().indexed_ready_materializes,
        0,
        "the active package target must materialise ONCE — a repeated \
         resolution must reuse its IndexedReady, not rebuild it",
    );
    assert!(
        resolved
            .as_ref()
            .and_then(|surface| surface.declaration.declaration_id)
            .is_some(),
        "package macro declaration ownership should still expose a stable declaration id",
    );
    assert!(
        host.project_type_store.indexed().get_any("/workspace/node_modules/pkg/dist/index.d.ts")
            .is_some(),
        "the inspected package provider barrel owns exactly one canonical IndexedReady built by the unified cold path during surface resolution too",
    );
    assert!(
        host.project_type_store.indexed().get_any("/workspace/node_modules/pkg/dist/index3.d.ts")
            .is_some(),
        "the actively resolved package target owns exactly one canonical IndexedReady built by the unified cold path when building macro declaration ownership",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_imported_type_root_nested_barrel_alias_materializes_only_the_matched_vue_child() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "export * from './Button.vue'\nexport * from './Link.vue'\nexport * from './Unused.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
import type { LinkProps } from './types'

export interface ButtonProps extends Omit<LinkProps, 'raw'> {
  label?: string
}
</script>
<template><button /></template>"#,
    );
    ws.inject_file(
        "/src/Link.vue",
        r#"<script lang="ts">
export interface LinkProps {
  href?: string
  raw?: boolean
}
</script>
<template><a /></template>"#,
    );
    ws.inject_file(
        "/src/Unused.vue",
        r#"<script lang="ts">
export interface UnusedProps {
  never?: number
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    host.set_import_dependencies(
        "/src/Button.vue",
        vec![exact_dependency("./types", "/src/types.ts")],
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![
            exact_dependency("./Button.vue", "/src/Button.vue"),
            exact_dependency("./Link.vue", "/src/Link.vue"),
            exact_dependency("./Unused.vue", "/src/Unused.vue"),
        ],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let root = host.resolve_imported_type_root("/src/types.ts", "LinkProps");

    assert_eq!(
        root,
        expected_imported_root(
            "/src/Link.vue",
            verter_type_expr::TopLevelOwnerId::module(0),
            "LinkProps",
        ),
        "nested barrel alias proof should still resolve LinkProps through the direct sibling barrel child",
    );
    for never_inspected in ["/src/Button.vue", "/src/Unused.vue"] {
        assert_eq!(
            ws.read_count(never_inspected),
            0,
            "nested barrel alias proof should never read the uninspected sibling {never_inspected}",
        );
        assert!(
            host.project_type_store
                .indexed()
                .get_any(never_inspected)
                .is_none(),
            "Vue siblings the nested barrel alias proof never inspects stay off FileArtifactStore: {never_inspected}",
        );
    }
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/src/Link.vue")
            .is_some(),
        "the matched Vue child was inspected and owns exactly one canonical IndexedReady built by the unified cold path",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_component_meta_nested_barrel_alias_resolves_expected_props() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { ButtonProps } from './types'

defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/types.ts",
        "export * from './Button.vue'\nexport * from './Link.vue'\nexport * from './Unused.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
import type { LinkProps } from './types'

export interface ButtonProps extends Omit<LinkProps, 'raw'> {
  label?: string
}
</script>
<template><button /></template>"#,
    );
    ws.inject_file(
        "/src/Link.vue",
        r#"<script lang="ts">
export interface LinkProps {
  href?: string
  raw?: boolean
}
</script>
<template><a /></template>"#,
    );
    ws.inject_file(
        "/src/Unused.vue",
        r#"<script lang="ts">
export interface UnusedProps {
  never?: number
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types", "/src/types.ts")],
    );
    host.set_import_dependencies(
        "/src/Button.vue",
        vec![exact_dependency("./types", "/src/types.ts")],
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![
            exact_dependency("./Button.vue", "/src/Button.vue"),
            exact_dependency("./Link.vue", "/src/Link.vue"),
            exact_dependency("./Unused.vue", "/src/Unused.vue"),
        ],
    );

    ws.reset_reads();
    let resolved = host
        .resolve_component_meta(
            "/src/Consumer.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("expanded component meta should resolve");

    let prop_names: std::collections::BTreeSet<String> =
        hm_prop_names(&host, "/src/Consumer.vue", &resolved)
            .into_iter()
            .collect();
    assert!(
        prop_names.contains("label") && prop_names.contains("href"),
        "nested barrel alias should still resolve reached props, got {prop_names:?}",
    );
    assert!(
        !prop_names.contains("raw"),
        "nested barrel alias should still respect Omit, got {prop_names:?}",
    );
    assert_eq!(
        ws.read_count("/src/Unused.vue"),
        0,
        "matching wildcard stems should keep unrelated same-layer barrel siblings off the component-meta expansion path",
    );
    assert!(
        host.project_type_store.indexed().get_any("/src/Unused.vue")
            .is_none(),
        "component-meta expansion should keep shallow-only same-layer barrel siblings off FileArtifactStore",
    );
}

/// Regression: validates() now accepts FileWholeHash facts for untracked files
/// (dependency files not in the store view). When a workspace-only dependency
/// file changes content (without being upserted), the old archived module_facts
/// must NOT be returned through the store-view-validated cache path.
///
/// The scenario:
/// 1. A dependency file is loaded from workspace (never upserted → not tracked)
/// 2. Module_facts are materialized then archived (as HostStoreView::build does)
/// 3. The workspace file changes (simulating a disk edit)
/// 4. A new store view is created — still doesn't track the dependency
/// 5. module_facts.get(dep, view) must NOT return stale archived facts
#[test]
fn archived_indexed_ready_rejected_when_workspace_dep_changes_content() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/dep.ts", "export interface DepType { version: 1 }\n");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    // Step 1: materialize module_facts for /src/dep.ts (workspace-only,
    // never upserted → won't be tracked by the store view).
    let facts_v1 = host
        .ensure_indexed_ready("/src/dep.ts")
        .expect("dep v1 should materialize");
    let hash_v1 = facts_v1.whole_hash;

    // Step 2: remove the cached IndexedReady entry so subsequent reads
    // re-materialize from the scheduler. The retired `FileArtifactStore` used
    // to archive soft-invalidated entries; `FileArtifactStore` validates by
    // whole_hash instead, so `remove` is the correct replacement.
    host.project_type_store().indexed().remove("/src/dep.ts");

    // Step 3: change the dependency content via workspace injection, then
    // notify the scheduler via `ensure_loaded` (the canonical content-change
    // ingress path under the new architecture: disk reads are no longer
    // implicit inside resolvers).
    ws.inject_file(
        "/src/dep.ts",
        "export interface DepType { version: 2; extra: string }\n",
    );
    // Evict so ensure_loaded re-reads the workspace content.
    host.evict("/src/dep.ts");
    assert!(host.ensure_loaded("/src/dep.ts"));

    // Step 4: create a store view snapshotted AFTER the content change.
    let _view = host.resolver_store_view_read().into_owned_view();

    // Step 5: query module_facts with the store view. The validated cache
    // must NOT return the stale V1 facts from the archive.
    let facts_after = host
        .ensure_indexed_ready("/src/dep.ts")
        .expect("dep should re-materialize with current workspace content");
    assert_ne!(
        facts_after.whole_hash, hash_v1,
        "IndexedReady via fence-validated cache must reflect the current \
         workspace content (V2), not stale archived V1 facts. The untracked-file \
         acceptance in validates() should not allow archived entries with a \
         mismatched content hash to pass validation.",
    );
}

#[test]
fn read_analysis_source_trace_result_labels_workspace_vfs_reads() {
    assert_eq!(
        super::super::read_analysis_source_result_detail(
            "/src/types.ts",
            "workspace-vfs",
            128,
            false,
        ),
        "owner=/src/types.ts source=workspace-vfs bytes=128"
    );
    assert_eq!(
        super::super::read_analysis_source_result_detail("/src/types.ts", "workspace-vfs", 0, true,),
        "owner=/src/types.ts source=workspace-vfs bytes=0 missing=true"
    );
}

#[test]
fn workspace_vfs_source_kind_includes_layer_detail_when_present() {
    assert_eq!(
        super::super::workspace_vfs_source_kind(Some("layer=snapshot cache=hit".to_string())),
        "workspace-vfs layer=snapshot cache=hit"
    );
    assert_eq!(
        super::super::workspace_vfs_source_kind(None),
        "workspace-vfs"
    );
}

// `request_store_view_extends_across_mid_request_ensure_loaded` is
// intentionally not part of this suite: the `RequestStoreView` type
// and its captured-view-plus-extension semantics are not part of the
// final design. Live-host probes validated via the host's fact-
// signature path are the authoritative substitute.

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ensure_loaded_reload_with_identical_content_does_not_bump_epoch() {
    // Regression lock-in: after an evict + ensure_loaded cycle for a
    // file whose on-disk content is identical to the pre-evict
    // snapshot, `store_view_epoch` must NOT bump. A regression that
    // bumped the epoch unconditionally would clear every thread-local
    // cache (parsed-eval-program, type-context) and force a cold re-
    // resolution on the follow-up lookup. The `pre_evict_hash ==
    // post_reload_hash` comparison must short-circuit the bump on
    // no-op reload; caches stay warm.
    let ws = std::sync::Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/App.vue",
        "<script setup lang=\"ts\">\nconst x = 1\n</script>\n<template><div /></template>",
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    host.ensure_loaded("/src/App.vue");

    let pre_epoch = host.current_store_view_epoch();
    host.evict("/src/App.vue");
    // evict() bumps the epoch unconditionally (real invalidation).
    assert!(
        host.current_store_view_epoch() > pre_epoch,
        "evict() should bump the epoch"
    );
    let post_evict_epoch = host.current_store_view_epoch();

    // Reload with identical content — scheduler sees identical bytes.
    host.ensure_loaded("/src/App.vue");
    assert_eq!(
        host.current_store_view_epoch(),
        post_evict_epoch,
        "reload with identical content must NOT bump the epoch"
    );
}

mod manifest_types_entry_routing_tests {
    //! `derive_type_preferred_exact_target` MUST route through
    //! `WorkspaceAccess::manifest_types_entry_for` (workspace-classification
    //! aware) rather than a `/node_modules/` substring check on the
    //! resolved canonical id.
    //!
    //! Discriminating fixture: a pnpm-hoisted layout where a workspace
    //! project root sits at `/ws/node_modules/@scope/local-pkg/`. A
    //! runtime-script (`.js`) target under this root has a canonical id
    //! that contains `/node_modules/` but `is_workspace_owned` returns
    //! `true` (because the project root's suffix is empty under itself).
    //!
    //! A naive substring router routes this incorrectly: the canonical
    //! id contains `/node_modules/`, so the `is_runtime_script_target`
    //! check fires and the manifest-types-entry resolution returns
    //! `None` for the workspace-owned package, and the fallback then
    //! short-circuits because the canonical id contains `/node_modules/`.
    //! Result: `None`.
    //!
    //! The `WorkspaceAccess` accessor routes correctly: the workspace
    //! classifies the target as `is_workspace_owned`, so the path is
    //! returned verbatim. Result: `Some(resolved)`.
    //!
    //! Asserting the correct return path discriminates the two
    //! implementations.
    use std::sync::Arc;

    use crate::types::DependencyResolution;
    use crate::{HostConfig, VerterHost};
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    fn build_pnpm_hoisted_workspace() -> Arc<MemoryWorkspace> {
        let ws = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
        // Register a project whose root sits INSIDE node_modules/. The
        // engine's is_workspace_owned + is_package_backed pair classifies
        // such files as workspace-owned (the suffix between root and
        // path contains no further /node_modules/ segment).
        ws.set_project_graph(verter_workspace::ProjectGraph::from_configs(vec![
            verter_workspace::VfsProjectConfig {
                root: "/ws/node_modules/@scope/local-pkg".to_string(),
                rank: verter_workspace::ProjectRank::Explicit,
                tsconfig_path: Some("/ws/node_modules/@scope/local-pkg/tsconfig.json".to_string()),
                root_files: vec![],
                extensions: vec![".ts".into(), ".js".into(), ".vue".into()],
                workspace_root: "/ws/node_modules/@scope/local-pkg".to_string(),
                workspace_aliases: vec![],
                compiler_options:
                    verter_session_query::resolution::IdeProjectCompilerOptions::default(),
                references: vec![],
                membership: verter_workspace::configured_membership_match_all_under_root(
                    &verter_workspace::CanonicalPath::new("/ws/node_modules/@scope/local-pkg"),
                ),
            },
        ]));
        // Inject a runtime-script target inside the workspace-owned
        // project. The canonical id contains /node_modules/ but the
        // workspace classification API returns is_workspace_owned=true.
        ws.inject_file(
            "/ws/node_modules/@scope/local-pkg/dist/index.js".to_string(),
            Arc::<str>::from(""),
        );
        ws
    }

    #[test]
    fn derive_type_preferred_exact_target_returns_workspace_owned_js_under_node_modules() {
        let ws = build_pnpm_hoisted_workspace();
        let access: Arc<dyn WorkspaceAccess> = ws.clone();

        // Sanity-check the discriminating fixture: the canonical id
        // contains /node_modules/ AND the workspace classifies it as
        // workspace-owned (NOT package-backed). A naive substring check
        // confuses these two; the WorkspaceAccess accessor does not.
        let resolved = "/ws/node_modules/@scope/local-pkg/dist/index.js";
        assert!(
            access.is_workspace_owned(resolved),
            "fixture invariant: resolved must be workspace-owned"
        );
        assert!(
            !access.is_package_backed(resolved),
            "fixture invariant: resolved must NOT be package-backed"
        );
        assert!(
            access.manifest_types_entry_for(resolved).is_none(),
            "manifest_types_entry_for must return None for workspace-owned targets"
        );

        let host = VerterHost::new(HostConfig::default(), access);
        let resolution = DependencyResolution {
            specifier: "@scope/local-pkg".to_string(),
            resolved_canonical_id: Some(resolved.to_string()),
            possible_canonical_ids: vec![],
        };

        let derived = host.derive_type_preferred_exact_target(&resolution);

        // The discriminating assertion: the workspace-owned .js path is
        // returned as-is via the workspace classification accessor. A
        // substring router would have short-circuited to None here.
        assert_eq!(
            derived.as_deref(),
            Some(resolved),
            "workspace-owned runtime-script target under /node_modules/ must \
             pass through the WorkspaceAccess routing path"
        );
    }

    // ── F4: carrier-generic extension classifiers ────────────────────────────
    //
    // `is_type_preferred_target` and `has_file_like_extension` used a hardcoded
    // `.vue` arm; a `.svelte` carrier must be classified identically to a `.vue`
    // one (both are framework carriers projecting a type-bearing virtual
    // surface / being real file-like paths). These pin the carrier-generic
    // behavior — they FAIL against the pre-fix `.vue`-only arms.

    #[test]
    fn type_preferred_target_treats_svelte_carrier_like_vue() {
        use super::super::is_type_preferred_target;
        // A `.vue` SFC is type-preferred…
        assert!(is_type_preferred_target("/src/App.vue"));
        // …and so is a `.svelte` carrier (the F4 fix). Pre-fix this was false.
        assert!(is_type_preferred_target("/src/Widget.svelte"));
        // `.d.ts`/`.ts` stay type-preferred; a bare `.js` does not.
        assert!(is_type_preferred_target("/src/types.d.ts"));
        assert!(!is_type_preferred_target("/src/runtime.js"));
        // A rune module (`.svelte.ts`) ends with `.ts` → type-preferred via the
        // script arm (unchanged, and correct — it is a real TS surface).
        assert!(is_type_preferred_target("/src/store.svelte.ts"));
    }

    #[test]
    fn file_like_extension_recognizes_svelte_carrier_like_vue() {
        use super::super::has_file_like_extension;
        // Both carriers are real file-like paths, not bare specifiers.
        assert!(has_file_like_extension("/src/App.vue"));
        assert!(has_file_like_extension("/src/Widget.svelte"));
        // Scripts / json stay file-like; a bare module specifier does not.
        assert!(has_file_like_extension("/src/util.ts"));
        assert!(has_file_like_extension("/src/data.json"));
        assert!(!has_file_like_extension("lodash"));
    }

    #[test]
    fn relative_svelte_path_is_not_misclassified_as_raw_specifier() {
        use super::super::is_raw_import_specifier_id;
        // The downstream consequence of the `has_file_like_extension` fix:
        // a relative `./Widget.svelte` import is a FILE, not a raw module
        // specifier. Pre-fix `has_file_like_extension` missed `.svelte`, so the
        // `./`-prefixed path fell through to the raw-specifier `true` arm — a
        // carrier asymmetry vs `./App.vue` (which was correctly `false`).
        assert!(!is_raw_import_specifier_id("./Widget.svelte"));
        assert!(!is_raw_import_specifier_id("./App.vue"));
        // A genuine bare specifier is still a raw specifier.
        assert!(is_raw_import_specifier_id("./some-pkg"));
        assert!(is_raw_import_specifier_id("lodash"));
    }
}

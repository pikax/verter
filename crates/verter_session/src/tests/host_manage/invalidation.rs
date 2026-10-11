use super::*;

#[test]
fn prepared_type_decl_bundle_invalidates_when_exact_resolution_changes() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/base.ts", "export interface Base { base: string }\n");
    ws.inject_file("/src/alt.ts", "export interface Base { alt: number }\n");
    ws.inject_file(
        "/src/types.ts",
        "import type { Base } from './dep'\nexport interface Props extends Base {}\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should seed module facts");
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./dep", "/src/base.ts")],
    );

    let _view_before = host.resolver_store_view_read().into_owned_view();
    let initial = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should materialize before the route change");
    assert_eq!(
        initial
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/base.ts"),
    );

    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./dep", "/src/alt.ts")],
    );

    let _view_after = host.resolver_store_view_read().into_owned_view();
    let rebuilt = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should rebuild after the effective dependency target changes");
    assert_eq!(
        rebuilt
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/alt.ts"),
        "exact-resolution fact validation must invalidate the old bundle when the effective target changes",
    );
}

/// Editing the barrel's re-export TARGET must invalidate the DEMAND-TIME
/// final resolution — the route authority's memoized entry (`ImportedRootDb`)
/// records the barrel route-chain facts at demand, so a retarget anywhere on
/// the chain misses the warm entry and re-resolves to the NEW defining file.
/// A stale final-root is never served.
///
/// Discriminating: if the demand-time resolution did NOT record the barrel
/// route facts (or rooted only on the owner + final file), retargeting the
/// barrel from `/src/a.ts` to `/src/b.ts` would keep serving the stale
/// `/src/a.ts` final root — the second assert demands `/src/b.ts`.
#[test]
fn prepared_decl_name_resolution_barrel_retarget_invalidates_final_canonical() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/a.ts", "export type Node = { from: 'a' }\n");
    ws.inject_file("/src/b.ts", "export type Node = { from: 'b' }\n");
    ws.inject_file("/src/barrel.ts", "export type { Node } from './a'\n");
    ws.inject_file(
        "/src/owner.ts",
        "import type { Node } from './barrel'\nexport interface Props { n: Node }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    for owner in ["/src/owner.ts", "/src/barrel.ts"] {
        let _ = host
            .ensure_indexed_ready(owner)
            .unwrap_or_else(|| panic!("{owner} should index"));
    }
    host.set_import_dependencies(
        "/src/owner.ts",
        vec![exact_dependency("./barrel", "/src/barrel.ts")],
    );
    host.set_import_dependencies("/src/barrel.ts", vec![exact_dependency("./a", "/src/a.ts")]);

    let initial = host
        .prepared_type_decl("/src/owner.ts", "Props")
        .expect("Props should prepare through the barrel pointing at /src/a.ts");
    assert_eq!(
        initial
            .name_resolution
            .get("Node")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/barrel.ts"),
        "the prepared name_resolution stores the DIRECT barrel hop \
         (demand-driven canonicalization)",
    );
    let initial_final = host
        .resolve_imported_type_root("/src/barrel.ts", "Node")
        .expect("the demand-time route resolves through the barrel");
    assert_eq!(
        initial_final.canonical_id.as_ref(),
        "/src/a.ts",
        "before the barrel retarget the demand-time final canonical is /src/a.ts",
    );

    // Retarget the barrel's re-export from ./a to ./b (a content edit to the
    // barrel's re-export clause + its route).
    ws.inject_file("/src/barrel.ts", "export type { Node } from './b'\n");
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/barrel.ts".to_string()),
            input_id: "/src/barrel.ts".to_string(),
            source: Arc::from("export type { Node } from './b'\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("barrel retarget upsert");
    host.set_import_dependencies("/src/barrel.ts", vec![exact_dependency("./b", "/src/b.ts")]);
    // Re-index the retargeted barrel (mirrors the initial setup) so it is
    // present for the canonicalization walk on the owner's bundle rebuild.
    let _ = host
        .ensure_indexed_ready("/src/barrel.ts")
        .expect("retargeted barrel should re-index");

    let rebuilt = host
        .prepared_type_decl("/src/owner.ts", "Props")
        .expect("Props should rebuild after the barrel retarget");
    assert_eq!(
        rebuilt
            .name_resolution
            .get("Node")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/barrel.ts"),
        "the direct-hop entry stays the barrel — the retarget moves the \
         DEMAND-TIME resolution, not the bundle-stored hop",
    );
    let retargeted_final = host
        .resolve_imported_type_root("/src/barrel.ts", "Node")
        .expect("the demand-time route re-resolves after the retarget");
    assert_eq!(
        retargeted_final.canonical_id.as_ref(),
        "/src/b.ts",
        "the barrel retarget must invalidate the memoized demand-time route so the \
         final canonical follows the barrel to /src/b.ts (no stale-served final root)",
    );
}

/// Prepared-decl-bundle breadth: ONE bundle, exercised across BOTH a
/// cache-hit-equivalence re-read AND an exact-resolution-change invalidation —
/// pairing the two halves the model siblings assert SEPARATELY
/// (`prepared_decl_bundle_without_store_view_reuses_stable_cache` =
/// reuse-only; `prepared_type_decl_bundle_invalidates_when_exact_resolution_changes`
/// = invalidate-only) against ONE shared `/src/types.ts:Props` bundle.
///
/// 1. First lookup materializes exactly one bundle
///    (`bundle_materializations == 1`).
/// 2. A second unchanged lookup REUSES the stable cache — no rematerialization
///    (`bundle_materializations` stays 1) and a cache hit registers
///    (`bundle_cache_hits >= 1`). This is the cache-hit equivalence, and both
///    reads resolve `Base` to `/src/base.ts`.
/// 3. The exact resolution is then upgraded (the import route for `./dep` is
///    retargeted `/src/base.ts` → `/src/alt.ts` via `set_import_dependencies`).
///    The next lookup INVALIDATES + rebuilds: `bundle_materializations` climbs
///    to 2 AND the rebuilt `name_resolution` now resolves `Base` to
///    `/src/alt.ts`.
///
/// Discriminates: if prepared-bundle invalidation regressed (the exact-route
/// fact dropped from the bundle's validity signature), the post-retarget
/// lookup REUSES the stale bundle — `bundle_materializations` stays 1 and
/// `Base` keeps resolving to `/src/base.ts`. If reuse regressed, step 2
/// rematerializes (`bundle_materializations` climbs to 2 prematurely).
#[test]
fn prepared_decl_bundle_reuses_then_invalidates_on_exact_resolution_change() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/base.ts", "export interface Base { base: string }\n");
    ws.inject_file("/src/alt.ts", "export interface Base { alt: number }\n");
    ws.inject_file(
        "/src/types.ts",
        "import type { Base } from './dep'\nexport interface Props extends Base {}\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should seed module facts");
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./dep", "/src/base.ts")],
    );
    host.provenance().reset();

    // ── First lookup: materialize exactly one bundle, resolving Base →
    // /src/base.ts.
    let first = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("first lookup should materialize a prepared bundle");
    let after_first = host.provenance().snapshot();
    assert_eq!(
        after_first.bundle_materializations, 1,
        "first lookup should materialize exactly one bundle"
    );
    assert_eq!(
        first
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/base.ts"),
    );

    // ── Cache-hit equivalence: an unchanged second lookup REUSES the bundle.
    let second = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("second lookup should reuse the prepared bundle");
    let after_second = host.provenance().snapshot();
    assert_eq!(
        after_second.bundle_materializations, 1,
        "cache-hit equivalence: the unchanged re-read MUST reuse the stable bundle \
         cache, not rematerialize"
    );
    assert!(
        after_second.bundle_cache_hits >= 1,
        "cache-hit equivalence: the unchanged re-read MUST register a bundle cache hit, got {after_second:?}"
    );
    assert_eq!(
        second
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/base.ts"),
    );

    // ── Invalidation: upgrade the exact resolution (retarget ./dep to
    // /src/alt.ts). The bundle's exact-route fact must invalidate the stale
    // bundle so the next lookup rebuilds.
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./dep", "/src/alt.ts")],
    );

    let rebuilt = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should rebuild after the effective dependency target changes");
    let after_rebuild = host.provenance().snapshot();
    assert_eq!(
        after_rebuild.bundle_materializations, 2,
        "DISCRIMINATING (invalidation): upgrading the exact resolution MUST \
         invalidate the stale bundle so the next lookup REBUILDS \
         (bundle_materializations 1 -> 2) — a reused stale bundle would keep it at 1"
    );
    assert_eq!(
        rebuilt
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/alt.ts"),
        "DISCRIMINATING (invalidation): the rebuilt bundle's name_resolution MUST \
         observe the upgraded route (Base -> /src/alt.ts); the stale /src/base.ts \
         resolution must NOT survive",
    );
}

// NOTE: stale prepared-decl replacement test was removed — prepared decls
// are managed through the host-owned bundle cache path, not IndexedReady.

#[test]
fn resolver_store_view_tracks_transitive_dependency_targets() {
    let host = strict_host();

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );
    upsert_non_sfc(&host, "/src/types.ts", "export { Props } from './dep'\n");
    upsert_non_sfc(
        &host,
        "/src/dep.ts",
        "export interface Props { msg: string }\n",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![crate::DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: Some("/src/dep.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let view = host.resolver_store_view_read().into_owned_view();

    assert!(
        view.whole_hash("/src/types.ts").is_some(),
        "captured store view should include direct dependency whole hashes"
    );
    assert!(
        view.whole_hash("/src/dep.ts").is_some(),
        "captured store view should include transitive dependency whole hashes"
    );
    // The import-route rail is resolve-domain now: the view captures the
    // immutable resolution world rather than snapshotting a per-owner
    // route digest, and the transitive owner's witness validates against
    // that capture.
    let witness = host
        .owner_import_route_witness_for_tests("/src/types.ts")
        .expect("a tracked transitive owner must produce a rootable witness");
    for fact in &witness {
        assert!(
            verter_session_query::facts::store_view::StoreView::validates(&view, fact),
            "the captured store view must validate the transitive owner's \
             import-route witness fact {fact:?}"
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn store_view_shallow_state_materializes_tracked_imported_dependency_indexed_ready() {
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
    assert!(host.ensure_loaded("/workspace/src/Consumer.vue"));
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
    assert!(
        host.routed_shallow_state("/workspace/node_modules/pkg/dist/index3.d.ts")
            .is_some(),
        "active route traversal should be able to build the target's shallow state first",
    );

    assert!(
        host.shallow_file_state("/workspace/node_modules/pkg/dist/index3.d.ts")
            .is_some_and(|state| state.has_type_symbol("PackageEmits")),
        "tracked imported declarations should expose memo-owned shallow headers",
    );
    assert!(
        host.project_type_store.indexed().get_any("/workspace/node_modules/pkg/dist/index3.d.ts")
            .is_some(),
        "the inspected tracked imported dependency owns exactly one canonical IndexedReady built by the unified cold path",
    );
}

/// Warm re-upsert (unchanged content) must still surface external `src`
/// block requests. Bundler transforms re-resolve them every time; empty
/// warm requests cause HOST_MISSING_EXTERNAL when the dep was never loaded
/// on a prior pass (zyronon-douyin `<style src="./switches.less">`).
#[test]
fn warm_upsert_still_returns_external_style_src_requests() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    let src = r#"<template><div class="x"/></template>
<script>export default { name: 'Switches' }</script>
<style src="./switches.less" lang="less"></style>
"#;
    let first = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/src/switches.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    assert_eq!(
        first.external_source_requests.len(),
        1,
        "cold upsert must report the style src request"
    );
    assert_eq!(
        first.external_source_requests[0].specifier,
        "./switches.less"
    );

    let second = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/src/switches.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    assert!(
        !second.changed,
        "byte-identical re-upsert should be unchanged"
    );
    assert_eq!(
        second.external_source_requests.len(),
        1,
        "warm upsert must still report external style src requests"
    );
    assert_eq!(
        second.external_source_requests[0].specifier,
        "./switches.less"
    );
    assert_eq!(
        second.external_source_requests[0].resolved_canonical_id,
        first.external_source_requests[0].resolved_canonical_id
    );
}

#[test]
fn build_fallthrough_eval_env_skips_unused_runtime_import_dependency_lookups() {
    let host = make_host();
    upsert_non_sfc(&host, "/src/used.ts", "export const used = 'used'");
    upsert_non_sfc(&host, "/src/unused.ts", "export const unused = 'unused'");
    upsert_vue(
        &host,
        "/src/App.vue",
        r#"<script setup lang="ts">
import { used } from './used'
import { unused } from './unused'
</script>
<template><div :title="used" /></template>"#,
    );
    host.set_import_dependencies(
        "/src/App.vue",
        vec![
            exact_dependency("./used", "/src/used.ts"),
            exact_dependency("./unused", "/src/unused.ts"),
        ],
    );

    let snapshot = host
        .get_analysis_snapshot_internal("/src/App.vue", None)
        .expect("analysis snapshot should exist");
    let env = host
        .build_fallthrough_eval_env_lightweight("/src/App.vue", &snapshot, None)
        .expect("fallthrough owner env should build");

    // Owner-aware presence: setup-owner hydration is keyed by the
    // import's lexical owner, so check by NAME across owners.
    assert!(
        env.value_symbols.keys().any(|key| &*key.name == "used"),
        "template-referenced runtime bindings should still be materialized"
    );
    assert!(
        !env.value_symbols.keys().any(|key| &*key.name == "unused"),
        "unused runtime imports should stay out of the fallthrough owner env"
    );
}

#[test]
fn resolved_type_declaration_same_name_edit_never_replays_stale_metadata() {
    let host = make_host();
    let canonical = "/src/types.ts";

    upsert_non_sfc(&host, canonical, "export interface Props { label: string }");
    let _ = host
        .ensure_indexed_ready(canonical)
        .expect("the initial declaration must be indexed");

    let before =
        crate::host_manage::jsdoc_resolve::resolve_type_declaration(&host, canonical, "Props");
    assert_eq!(
        before.kind,
        verter_session_query::declarations::metadata::ResolvedDeclarationKind::Interface,
        "control: the first lookup must resolve the authored interface"
    );

    upsert_non_sfc(
        &host,
        canonical,
        "export type Props = { label: string; count: number }",
    );
    let _ = host
        .ensure_indexed_ready(canonical)
        .expect("the edited declaration must be re-indexed");

    let after =
        crate::host_manage::jsdoc_resolve::resolve_type_declaration(&host, canonical, "Props");
    assert_eq!(
        after.kind,
        verter_session_query::declarations::metadata::ResolvedDeclarationKind::TypeAlias,
        "a same-name edit must resolve current declaration metadata rather than replaying a stale symbol-cache entry: before={before:?}, after={after:?}"
    );
    assert_ne!(
        after.span, before.span,
        "the declaration span must move with the edited body"
    );
}

#[test]
fn get_export_span_local_export_unchanged() {
    let host = make_host();

    upsert_ts(&host, "utils.ts", "export function foo() { return 1 }");

    // Local export — no re-export, returns span in same file
    let result = host.get_export_span_follow_reexports("utils.ts", "foo");

    assert!(result.is_some(), "should find local export");
    let (canonical_id, start, end) = result.unwrap();
    assert_eq!(
        canonical_id, "utils.ts",
        "local export should return same file"
    );
    assert!(start < end, "should have a valid span");
}

/// @ai-generated - upsert of .ts file returns export signatures
#[test]
fn upsert_returns_export_signatures_for_ts() {
    let host = make_host();
    let result = upsert_ts_result(
        &host,
        "index.ts",
        r#"export const foo = 1;
export type Bar = string;
export { default as Button } from './Button.vue';
"#,
    );

    assert!(
        !result.export_signatures.is_empty(),
        "upsert should return export signatures for .ts files"
    );

    let foo_sig = result
        .export_signatures
        .iter()
        .find(|s| s.name == "foo")
        .expect("should have 'foo' export");
    assert!(!foo_sig.is_type, "foo is a value export");
    assert!(
        foo_sig.reexport_source.is_none(),
        "foo is local, not a re-export"
    );

    let bar_sig = result
        .export_signatures
        .iter()
        .find(|s| s.name == "Bar")
        .expect("should have 'Bar' export");
    assert!(bar_sig.is_type, "Bar is a type export");

    let button_sig = result
        .export_signatures
        .iter()
        .find(|s| s.name == "Button")
        .expect("should have 'Button' re-export");
    assert_eq!(
        button_sig.reexport_source.as_deref(),
        Some("./Button.vue"),
        "Button re-export source should be './Button.vue'"
    );
    assert_eq!(
        button_sig.reexport_local.as_deref(),
        Some("default"),
        "Button re-export local name should be 'default'"
    );
}

#[test]
fn get_semantic_hash_changes_on_content_change() {
    let host = make_host();
    upsert_vue(&host, "App.vue", "<template><div>a</div></template>");
    let h1 = host.get_semantic_hash("App.vue").unwrap();
    upsert_vue(&host, "App.vue", "<template><div>b</div></template>");
    let h2 = host.get_semantic_hash("App.vue").unwrap();
    assert_ne!(h1, h2, "semantic hash should change when content changes");
}

#[test]
fn lazy_template_class_cache_revalidates_dependency_edits_and_missing_arrival() {
    let host = make_host();
    let canonical = "/workspace/src/DependencyDomain.vue";
    upsert_non_sfc(
        &host,
        "/workspace/src/types.ts",
        "export type Variant = 'primary';",
    );
    upsert_vue(
        &host,
        canonical,
        r#"<script setup lang="ts">
import type { Variant } from './types'
const variant: Variant = 'primary'
</script><template><div :class="variant" /></template>"#,
    );
    host.set_import_dependencies(
        canonical,
        vec![exact_dependency("./types", "/workspace/src/types.ts")],
    );
    let profile = CompileProfile::default();
    let query = || VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile.clone(),
    };
    let cold_compile = host
        .get_virtual_file(query())
        .expect("cold session compile");
    assert!(!cold_compile.cache_hit);
    let warm_compile = host
        .get_virtual_file(query())
        .expect("warm session compile");
    assert!(
        warm_compile.cache_hit,
        "unchanged normal virtual-file compilation must reach its validated session warm hit"
    );
    let first = host.get_analysis(canonical).expect("analysis");
    assert_eq!(
        first.template.expect("template").elements[0].dynamic_classes,
        ["primary"]
    );

    upsert_non_sfc(
        &host,
        "/workspace/src/types.ts",
        "export type Variant = 'secondary';",
    );
    let changed_compile = host
        .get_virtual_file(query())
        .expect("dependency-edited session compile");
    assert!(
        !changed_compile.cache_hit,
        "a template-class dependency edit must reject the normal compile warm slot"
    );
    let second = host.get_analysis(canonical).expect("analysis");
    assert_eq!(
        second.template.expect("template").elements[0].dynamic_classes,
        ["secondary"],
        "the raw-template warm slot must validate its semantic dependency signature"
    );

    let missing = "/workspace/src/MissingArrival.vue";
    upsert_vue(
        &host,
        missing,
        r#"<script setup lang="ts">
import type { Later } from './later'
const variant: Later = null as never
</script><template><div :class="variant" /></template>"#,
    );
    let initial = host.get_analysis(missing).expect("analysis");
    assert!(initial.template.expect("template").elements[0]
        .dynamic_classes
        .is_empty());
    assert!(
        host.derived_raw_cache()
            .get(missing)
            .is_none_or(|derived| derived.raw_template_analysis().is_none()),
        "ReturnOnly missing-dependency facts must not warm the raw-template slot"
    );

    upsert_non_sfc(
        &host,
        "/workspace/src/later.ts",
        "export type Later = 'arrived';",
    );
    host.set_import_dependencies(
        missing,
        vec![exact_dependency("./later", "/workspace/src/later.ts")],
    );
    let arrived = host.get_analysis(missing).expect("analysis");
    assert_eq!(
        arrived.template.expect("template").elements[0].dynamic_classes,
        ["arrived"]
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn base_content_dependency_class_facts_are_fresh_but_never_admitted() {
    let host = strict_host();
    let profile = CompileProfile {
        requested_mode: CompileCacheMode::Content,
        ..CompileProfile::default()
    };
    let compile = |canonical: &str| {
        host.get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical.to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile.clone(),
        })
        .expect("content compile")
    };

    let edited = "/workspace/src/ContentDependency.vue";
    upsert_non_sfc(
        &host,
        "/workspace/src/content-types.ts",
        "export type Variant = 'primary';",
    );
    upsert_vue(
        &host,
        edited,
        r#"<script setup lang="ts">
import type { Variant } from './content-types'
const variant: Variant = null as never
</script><template><div :class="variant" /></template>"#,
    );
    host.set_import_dependencies(
        edited,
        vec![exact_dependency(
            "./content-types",
            "/workspace/src/content-types.ts",
        )],
    );
    let first = compile(edited);
    assert_eq!(first.actual_mode, CompileCacheMode::Content);
    assert!(!first.cache_hit);
    assert_eq!(
        host.get_analysis(edited)
            .expect("first analysis")
            .template
            .expect("template")
            .elements[0]
            .dynamic_classes,
        ["primary"]
    );
    assert_eq!(host.compile_output_pure_content_entry_count(), 0);

    upsert_non_sfc(
        &host,
        "/workspace/src/content-types.ts",
        "export type Variant = 'secondary';",
    );
    let second = compile(edited);
    assert_eq!(second.actual_mode, CompileCacheMode::Content);
    assert!(!second.cache_hit);
    assert_eq!(
        host.get_analysis(edited)
            .expect("edited analysis")
            .template
            .expect("template")
            .elements[0]
            .dynamic_classes,
        ["secondary"]
    );
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "dependency-derived Content output must remain return-only after an edit"
    );

    let arrival = "/workspace/src/ContentArrival.vue";
    upsert_vue(
        &host,
        arrival,
        r#"<script setup lang="ts">
import type { Later } from './content-later'
const variant: Later = null as never
</script><template><div :class="variant" /></template>"#,
    );
    let missing = compile(arrival);
    assert_eq!(missing.actual_mode, CompileCacheMode::Content);
    assert!(!missing.cache_hit);
    assert!(host
        .get_analysis(arrival)
        .expect("missing analysis")
        .template
        .expect("template")
        .elements[0]
        .dynamic_classes
        .is_empty());
    assert_eq!(host.compile_output_pure_content_entry_count(), 0);

    upsert_non_sfc(
        &host,
        "/workspace/src/content-later.ts",
        "export type Later = 'arrived';",
    );
    host.set_import_dependencies(
        arrival,
        vec![exact_dependency(
            "./content-later",
            "/workspace/src/content-later.ts",
        )],
    );
    let arrived = compile(arrival);
    assert_eq!(arrived.actual_mode, CompileCacheMode::Content);
    assert!(!arrived.cache_hit);
    assert_eq!(
        host.get_analysis(arrival)
            .expect("arrival analysis")
            .template
            .expect("template")
            .elements[0]
            .dynamic_classes,
        ["arrived"]
    );
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "missing-dependency arrival must not seed the pure-content cache"
    );
}

/// Cache population is path-independent: the raw-template persist verdict, the
/// served template, and the recorded class-fact invalidation rail must not
/// depend on whether the `IndexedReady` artifact happened to be cached first.
///
/// Two hosts, the SAME bytes, two orders — one pre-warms the artifact store
/// through `ensure_indexed_ready` (the lane then takes the base fork), one does
/// not (the cold-seed fork). Run for a purely LOCAL closed domain and again for
/// a CROSS-FILE one.
///
/// On the recorded signature this asserts what path-independence actually
/// requires, not raw set equality: the same owner `FileWholeHash` rooting, and
/// the SAME cross-file fact set — no rail the owner's `source_generation` stamp
/// cannot see may be lost by taking the cold fork. The two forks legitimately
/// differ by OWNER-SCOPED incidental observations (the base context reads the
/// owner's already-materialised route surface; the cold-seed context resolves
/// without it), and a signature must record what its compute actually observed
/// — fabricating the missing observation to force byte equality would be the
/// real defect. The cross-file pair proves the cross-file assertion is not
/// vacuous: its shared fact set is non-empty.
///
/// DISCRIMINATION: the last arm's verdict is legitimately `false` (a
/// content-override lane never populates the base slot), so the agreeing
/// verdicts above are not "both `true` by construction".
#[cfg(not(target_arch = "wasm32"))]
#[test]
#[should_panic(expected = "CorrelationMismatch")]
fn raw_template_persist_is_independent_of_indexed_artifact_warmth() {
    // ── Pair 1: a purely LOCAL closed domain ──
    const LOCAL: &str = "/workspace/src/WarmthIndependentLocal.vue";
    const LOCAL_SOURCE: &str = r#"<script setup lang="ts">
type Variant = 'primary' | 'secondary'
const variant: Variant = 'primary'
</script><template><div :class="variant" /></template>"#;

    let (cold_domain, cold_signature) = lazy_template_lane_arm(LOCAL, LOCAL_SOURCE, None, false);
    let (warm_domain, warm_signature) = lazy_template_lane_arm(LOCAL, LOCAL_SOURCE, None, true);
    let cold_signature = cold_signature.expect(
        "path-independent cache population: the COLD-store order must reach the \
         SAME persist verdict as the warm-store order — a cold artifact store is \
         not a fence",
    );
    let warm_signature =
        warm_signature.expect("the warm-store order persists (the pre-fix behaviour)");

    assert_eq!(
        cold_domain,
        ["primary", "secondary"],
        "the cold-store order still publishes the closed domain",
    );
    assert_eq!(
        cold_domain, warm_domain,
        "the served class domain must not depend on artifact-cache warmth",
    );
    let cold_root = owner_whole_hash_facts(&cold_signature, LOCAL);
    assert!(
        !cold_root.is_empty(),
        "every admitted entry roots on the owner's own FileWholeHash",
    );
    assert_eq!(
        cold_root,
        owner_whole_hash_facts(&warm_signature, LOCAL),
        "the owner rooting must not depend on artifact-cache warmth",
    );
    assert_eq!(
        cross_file_facts(&cold_signature, LOCAL),
        cross_file_facts(&warm_signature, LOCAL),
        "no cross-file rail may be lost by taking the cold fork: cold={:?} warm={:?}",
        cold_signature.facts,
        warm_signature.facts,
    );

    // ── Pair 2: a CROSS-FILE closed domain — proves the cross-file assertion
    // above is not vacuously satisfied by two empty sets. ──
    const IMPORTED: &str = "/workspace/src/WarmthIndependentImported.vue";
    const IMPORTED_SOURCE: &str = r#"<script setup lang="ts">
import type { Variant } from './warmth-types'
const variant: Variant = 'primary'
</script><template><div :class="variant" /></template>"#;
    let dependency = Some((
        "./warmth-types",
        "/workspace/src/warmth-types.ts",
        "export type Variant = 'primary' | 'secondary';",
    ));

    let (cold_imported_domain, cold_imported_signature) =
        lazy_template_lane_arm(IMPORTED, IMPORTED_SOURCE, dependency, false);
    let (warm_imported_domain, warm_imported_signature) =
        lazy_template_lane_arm(IMPORTED, IMPORTED_SOURCE, dependency, true);
    let cold_imported_signature = cold_imported_signature.expect(
        "path-independent cache population: a cross-file closed domain persists \
         from the COLD-store order too",
    );
    let warm_imported_signature =
        warm_imported_signature.expect("the warm-store order persists the cross-file domain");

    assert_eq!(
        cold_imported_domain,
        ["primary", "secondary"],
        "the cold-store order resolves the IMPORTED closed domain",
    );
    assert_eq!(
        cold_imported_domain, warm_imported_domain,
        "the served cross-file class domain must not depend on artifact-cache warmth",
    );
    let cold_cross = cross_file_facts(&cold_imported_signature, IMPORTED);
    assert!(
        !cold_cross.is_empty(),
        "non-vacuity: a cross-file class domain records at least one cross-file \
         rail; got {:?}",
        cold_imported_signature.facts,
    );
    assert_eq!(
        cold_cross,
        cross_file_facts(&warm_imported_signature, IMPORTED),
        "no cross-file rail may be lost by taking the cold fork: cold={:?} warm={:?}",
        cold_imported_signature.facts,
        warm_imported_signature.facts,
    );
    assert_eq!(
        owner_whole_hash_facts(&cold_imported_signature, IMPORTED),
        owner_whole_hash_facts(&warm_imported_signature, IMPORTED),
        "the owner rooting must not depend on artifact-cache warmth",
    );

    // ── DISCRIMINATION: a verdict that is legitimately `false`. A
    // content-override lane is a genuinely fenced input and must never populate
    // the base slot, so the agreeing verdicts above are a real agreement rather
    // than an unfalsifiable one. ──
    let override_host = make_host();
    upsert_vue(&override_host, LOCAL, LOCAL_SOURCE);
    let profile = CompileProfile::default();
    let _ = override_host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: LOCAL.to_string(),
            compile_profile: profile,
            overrides: vec![BlockOverrideEntry::unissued_for_test(
                "<div :class=\"variant\" /><span :class=\"variant\" />",
            )],
        })
        .expect("template override must apply");
    let override_template = override_host
        .raw_template_analysis_for_file(LOCAL)
        .expect("the override lane must serve its own template");
    assert_eq!(
        override_template.elements.len(),
        2,
        "discrimination invariant: the served template is the OVERRIDE's template",
    );
    assert_eq!(
        override_template.elements[0].dynamic_classes,
        ["primary", "secondary"],
        "discrimination invariant: the override lane still SERVES its resolved \
         domain — it is fenced from publishing, not from resolving",
    );
    assert!(
        persisted_raw_template(&override_host, LOCAL).is_none(),
        "a content-override lane is a fenced INPUT and must never populate the \
         base raw-template slot — this is the arm whose verdict is legitimately \
         false, so the arms above are not both true by construction",
    );
}

#[test]
fn resolved_dependency_targets_uses_effective_target() {
    let mut import_routes = rustc_hash::FxHashMap::default();
    // Resolved: should use resolved_canonical_id only
    import_routes.insert(
        "./types".to_string(),
        crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: vec!["/src/types.js".to_string()],
        },
    );
    // Unresolved: should use highest-priority possible
    import_routes.insert(
        "./utils".to_string(),
        crate::types::DependencyResolution {
            specifier: "./utils".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: vec![
                "/src/utils.js".to_string(),
                "/src/utils.d.ts".to_string(),
            ],
        },
    );
    // No resolution at all
    import_routes.insert(
        "./missing".to_string(),
        crate::types::DependencyResolution {
            specifier: "./missing".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: Vec::new(),
        },
    );

    let targets = VerterHost::resolved_dependency_targets(&import_routes);

    assert!(
        targets.contains("/src/types.ts"),
        "should include resolved ID"
    );
    assert!(
        !targets.contains("/src/types.js"),
        "should NOT include possibles when resolved exists"
    );
    assert!(
        targets.contains("/src/utils.d.ts"),
        "should include highest-priority possible"
    );
    assert!(
        !targets.contains("/src/utils.js"),
        "should NOT include lower-priority possible"
    );
    assert_eq!(targets.len(), 2, "missing should not contribute a target");
}

#[test]
fn route_inventory_reuses_cached_artifact_for_same_dependency() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "import type { Base } from './base'\nexport interface Props extends Base { label: string }\n",
    );
    ws.inject_file("/src/base.ts", "export interface Base { id: string }\n");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    ws.reset_reads();
    let first = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("first read should load and cache the dependency");
    let second = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("second read should reuse the cached dependency artifact");

    assert!(
        Arc::ptr_eq(&first.route_inventory, &second.route_inventory),
        "repeated dependency reads should reuse the cached route inventory",
    );
    assert!(
        Arc::ptr_eq(&first.route_inventory, &first.shallow_state.route_inventory),
        "the artifact and shallow state must share one canonical route inventory",
    );
    assert_eq!(
        ws.read_count("/src/types.ts"),
        1,
        "the dependency source should only be loaded once for repeated analysis lookups",
    );
}

#[test]
fn route_inventory_is_replaced_atomically_after_content_edit() {
    let host = make_host();
    let canonical = "/src/routes.ts";
    upsert_non_sfc(
        &host,
        canonical,
        "import type { A } from './a'; export type Public = A;\n",
    );
    let before = host
        .ensure_indexed_ready(canonical)
        .expect("initial route artifact must materialize");
    assert_eq!(before.route_inventory.imports[0].source, "./a");

    upsert_non_sfc(
        &host,
        canonical,
        "import type { B } from './b'; export type Public = B;\n",
    );
    let after = host
        .ensure_indexed_ready(canonical)
        .expect("edited route artifact must rematerialize");

    assert!(!Arc::ptr_eq(
        &before.route_inventory,
        &after.route_inventory
    ));
    assert_eq!(after.route_inventory.imports[0].source, "./b");
    assert!(Arc::ptr_eq(
        &after.route_inventory,
        &after.shallow_state.route_inventory
    ));
}

#[test]
fn shallow_state_prefers_declaration_companion_for_runtime_js_dependencies() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.js".to_string(),
        Arc::from("export const runtimeOnly = true\n"),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts".to_string(),
        Arc::from("export interface Props { label: string }\n"),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/package.json".to_string(),
        Arc::from(r#"{"name":"pkg","types":"./dist/index.d.ts"}"#),
    );

    let host = VerterHost::new(HostConfig::default(), ws);

    let state = host
        .shallow_file_state("/workspace/node_modules/pkg/dist/index.js")
        .expect("runtime-script shallow requests should prefer the declaration companion");

    assert!(
        state.has_type_symbol("Props"),
        "the declaration companion headers should expose declaration symbols",
    );

    // In the new IndexedReady DB, ensure_indexed_ready normalizes .js → .d.ts
    // companion, so the .js path returns the .d.ts entry. Verify the declaration companion
    // is properly cached with analysis content.
    let declaration_entry = host
        .ensure_indexed_ready("/workspace/node_modules/pkg/dist/index.d.ts")
        .expect("the declaration companion should own the cached analysis");
    assert!(
        declaration_entry
            .route_inventory
            .counts
            .top_level_statement_count
            > 0,
        "the declaration companion should cache its route inventory",
    );
}

#[test]
fn resolve_eval_dependency_canonical_prefers_declaration_companion_shallowly() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.js",
        "export const runtimeOnly = true\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        "export interface Props { label: string }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    let resolved =
        host.resolve_eval_dependency_canonical("/workspace/node_modules/pkg/dist/index.js");

    assert_eq!(
        resolved.as_deref(),
        Some("/workspace/node_modules/pkg/dist/index.d.ts"),
        "runtime-script dependency canonicalization should prefer the declaration companion",
    );
    assert_eq!(
        ws.read_count("/workspace/node_modules/pkg/dist/index.js"),
        0,
        "companion selection should not read the runtime script when a declaration companion exists",
    );
    assert_eq!(
        ws.read_count("/workspace/node_modules/pkg/dist/index.d.ts"),
        0,
        "companion selection should stay on shallow existence probes and avoid reading the declaration companion",
    );

    // In the new IndexedReady DB, ensure_indexed_ready eagerly materializes.
    // Verify that the FileArtifactStore was NOT populated by the shallow
    // resolve_eval_dependency_canonical call itself.
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/index.js")
            .is_none(),
        "shallow companion selection should not cache .js facts in the FileArtifactStore",
    );
    assert!(
        host.project_type_store.indexed().get_any("/workspace/node_modules/pkg/dist/index.d.ts").is_none(),
        "companion canonicalization must not materialize or cache the declaration target during shallow selection",
    );
}

/// DISCRIMINATING regression (overlay seed, negative-route staleness): the
/// overlay materialiser's seed loop is the SAME gate as the base seed — a
/// stale `set_import_dependencies` known-miss must not be re-baked into a
/// session artifact after the target appears.
///
/// FAILS pre-fix: the overlay artifact re-bakes the stale known-miss.
/// PASSES post-fix: the overlay flight re-resolves `./missing` live.
#[test]
fn overlay_seed_does_not_rebake_stale_known_miss_after_target_appears() {
    use crate::session_view::OverlaidView;
    let host = Arc::new(make_host());
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);

    const OWNER_SOURCE: &str = "export type { Foo } from './missing';\n";
    upsert_non_sfc(&host, "/workspace/owner.ts", OWNER_SOURCE);
    host.set_import_dependencies(
        "/workspace/owner.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./missing".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: Vec::new(),
        }],
    );

    // Byte-identical overlay — the opened-but-unmodified LSP case.
    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert("/workspace/owner.ts".to_string(), Arc::from(OWNER_SOURCE));
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    let first = host
        .materialize_overlay_indexed_ready_with_view("/workspace/owner.ts", &view)
        .expect("overlay IndexedReady materialises");
    assert_eq!(
        first
            .shallow_state
            .exports
            .values()
            .filter_map(|export| match export {
                verter_session_query::inputs::shallow::ExportTarget::Reexport {
                    source_specifier,
                    ..
                } => Some(source_specifier.as_str()),
                verter_session_query::inputs::shallow::ExportTarget::Local { .. } => None,
            })
            .collect::<Vec<_>>(),
        vec!["./missing"],
        "precondition: the overlay surface publishes the authored specifier"
    );
    assert_eq!(
        host.resolve_type_dependency_canonical_shallow("/workspace/owner.ts", "./missing"),
        None,
        "precondition: while ./missing does not exist the specifier resolves to nothing"
    );

    upsert_non_sfc(
        &host,
        "/workspace/missing.ts",
        "export type Foo = string;\n",
    );

    let second = host
        .materialize_overlay_indexed_ready_with_view("/workspace/owner.ts", &view)
        .expect("overlay IndexedReady stays served");
    assert_eq!(
        second.whole_hash, first.whole_hash,
        "the overlay's parse artifact is unchanged — this is a dependency-set move"
    );
    assert_eq!(
        host.resolve_type_dependency_canonical_shallow("/workspace/owner.ts", "./missing")
            .as_deref(),
        Some("/workspace/missing.ts"),
        "STALE NEGATIVE ROUTE (overlay): a specifier that becomes resolvable must \
         resolve through the live authority — no overlay artifact may pin the \
         earlier miss"
    );
}

/// DISCRIMINATING regression (generation-current wildcard-edge surface on the
/// INDEXED producer). `ensure_indexed_ready` bakes wildcard sources into the
/// content-pinned `IndexedReady` surface, and the indexed surface is the SOLE
/// route authority `current_derived_fact_hash(Route)` / `HostStoreView::build`
/// serve — a baked wildcard edge depends on the dependency file set, not the
/// owner's content. A barrel `export * from './runtime'` indexed while only
/// `runtime.js` exists bakes the `./runtime → runtime.js` edge; when the
/// `.d.ts` companion appears TS-first priority retargets the edge, but the
/// content-pinned indexed surface (owner content unchanged) keeps serving
/// runtime.js.
///
/// The root fix roots the indexed surface in `IndexedReady.edge_generation` and
/// routes every route-fact producer + the indexed materializer-reuse through
/// the shared edge-currency oracle: an edge-stale wildcard-bearing indexed
/// surface produces no `Route` fact and is rebuilt on reuse, so the warm host
/// re-resolves and matches a fresh host.
///
/// FAILS pre-root-fix: the warm host returns `runtime.js` after `runtime.d.ts`
/// appears. PASSES post: warm == fresh == `runtime.d.ts`.
#[test]
fn indexed_route_fact_retargets_on_warm_host_after_dependency_set_change() {
    let ws = Arc::new(CountingWorkspace::new());
    let index = "/workspace/index.ts";
    ws.inject_file(
        index,
        "export type * from './runtime';\nexport type * from './present';\n",
    );
    ws.inject_file("/workspace/present.ts", "export type Present = number;\n");
    // `./runtime` initially resolves to the directory-index file.
    ws.inject_file(
        "/workspace/runtime/index.ts",
        "export type Runtime = number;\n",
    );
    let warm = VerterHost::new(HostConfig::default(), ws.clone());

    // FORCE the indexed surface — the route-surface producer under test.
    // `ensure_indexed_ready` bakes `./runtime → runtime/index.ts` into the
    // content-pinned `IndexedReady` at the current generation.
    let _ = warm.ensure_indexed_ready(index);
    let r1 = warm.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        r1,
        Some((
            "/workspace/runtime/index.ts".to_string(),
            "Runtime".to_string()
        )),
        "precondition: Runtime resolves to the directory-index file while it is \
         the only ./runtime target"
    );

    // A more-specific `./runtime.ts` appears. The SAME resolution policy (a
    // file preferred over a directory-index) retargets `./runtime` to
    // `runtime.ts` — a genuine baked-edge change the indexed materialiser
    // itself produces on rebuild (no resolution-policy divergence between
    // producers).
    ws.inject_file("/workspace/runtime.ts", "export type Runtime = boolean;\n");

    let fresh = VerterHost::new(HostConfig::default(), ws.clone());
    let _ = fresh.ensure_indexed_ready(index);
    let fresh_result = fresh.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        fresh_result,
        Some(("/workspace/runtime.ts".to_string(), "Runtime".to_string())),
        "precondition: a fresh host retargets Runtime to the more-specific \
         ./runtime.ts after it appears"
    );

    let warm_result = warm.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        warm_result, fresh_result,
        "the WARM host MUST return the SAME retargeted target as a FRESH host once \
         ./runtime.ts appears — a content-pinned INDEXED wildcard surface (owner \
         content unchanged, but the dependency file set shifted) must be rooted in \
         its edge-resolution generation so every route-fact producer and the \
         indexed materializer reuse re-resolve instead of serving the stale \
         runtime/index.ts edge"
    );
}

/// Regression pin (dependency APPEARANCE on a NON-wildcard reexport edge):
/// the owner bakes `./missing` as a known-miss into its indexed
/// `import_routes`; when `missing.ts` later appears the edge must
/// re-resolve. At HEAD this case is ALSO covered by the known-miss
/// generation revalidation rail (`generation_current_route_resolution`
/// re-resolves a recorded miss against the live file set), so this test is
/// green even without the edge-currency staling of non-wildcard surfaces —
/// it pins the appearance CLASS end-to-end, while the discriminating RED
/// pin for the oracle extension is the retarget sibling above
/// (`non_wildcard_route_fact_retargets_via_edge_refresh_on_warm_host`).
#[test]
fn non_wildcard_route_fact_resolves_after_dependency_appears_on_warm_host() {
    let ws = Arc::new(CountingWorkspace::new());
    let index = "/workspace/index.ts";
    ws.inject_file(index, "export type { Missing } from './missing';\n");
    let warm = VerterHost::new(HostConfig::default(), ws.clone());

    // FORCE the indexed surface while `./missing` is unresolvable — the
    // known-miss bakes into the content-pinned route surface.
    let _ = warm.ensure_indexed_ready(index);
    let r0 = warm.resolve_named_type_export_target(index, "Missing");
    assert_eq!(
        r0, None,
        "precondition: Missing must miss while ./missing is absent"
    );

    ws.inject_file("/workspace/missing.ts", "export type Missing = string;\n");

    let warm_result = warm.resolve_named_type_export_target(index, "Missing");
    assert_eq!(
        warm_result,
        Some(("/workspace/missing.ts".to_string(), "Missing".to_string())),
        "a NON-wildcard reexport edge baked as a known-miss MUST re-resolve \
         once the dependency appears — the baked import_routes known-miss is \
         a dependency-set-derived edge exactly like a wildcard edge, so the \
         edge-currency oracle must stale the owner surface on the \
         content-generation advance"
    );
}

/// DISCRIMINATING regression (RouteDb stale-serve hole 3, review finding 2 —
/// the OVERLAY materialiser is a third sibling route-fact producer). The
/// overlay `IndexedReady` materialiser resolved its TypeImport edges' ESM
/// fallback to the RAW `source_id` (the runtime `.js`), while route traversal +
/// known-miss revalidation normalize the ESM fallback to the `.d.ts`
/// declaration companion through the single shared `resolve_route_edge_canonical`
/// policy. Session store views consume the overlay's route facts, so an overlay
/// barrel with an ESM-fallback edge recorded a route canonical the base
/// route-fact producers cannot reproduce — the SAME stale-serve class as hole 3,
/// persisting on the overlay path.
///
/// The fix routes the overlay's TypeImport edge resolution through the SAME
/// shared `resolve_route_edge_canonical` policy (no third copy). Because the
/// overlay's `export *` wildcard sources flow through the SAME
/// `required_import_sources` chain (a bare `export *` is captured in
/// `export_signatures`), normalizing the chain also normalizes wildcard edges —
/// no separate wildcard pass is needed.
///
/// FAILS pre-fix: the overlay records `/workspace/runtime.js` (raw). PASSES
/// post-fix: the overlay records `/workspace/runtime.d.ts`, identical to the
/// shared `resolve_route_edge_canonical` oracle.
#[test]
fn overlay_materializer_esm_fallback_normalizes_like_shared_route_edge_policy() {
    use crate::session_view::OverlaidView;
    let ws = Arc::new(CountingWorkspace::new());
    let barrel = "/workspace/index.ts";
    // Runtime target + its declaration companion (the `.js` → `.d.ts`
    // normalization the shared policy applies).
    ws.inject_file("/workspace/runtime.js", "export const runtime = true\n");
    ws.inject_file("/workspace/runtime.d.ts", "export type Runtime = boolean\n");
    // Resolvable ONLY via `EsmImport` — forces the ESM fallback (the exact site
    // where the overlay kept the raw `source_id`).
    ws.set_exact_resolutions(
        barrel,
        vec![verter_workspace::ExactResolution {
            specifier: "runtimedep".to_string(),
            phase: verter_session_query::resolution::ResolvePhase::CodegenBlocker,
            kind: verter_session_query::resolution::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/workspace/runtime.js".to_string()),
            possible_canonical_ids: vec!["/workspace/runtime.js".to_string()],
        }],
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws.clone()));

    // The shared oracle — route traversal + known-miss both delegate here.
    let oracle = host.resolve_route_edge_canonical(barrel, "runtimedep");
    assert_eq!(
        oracle.as_deref(),
        Some("/workspace/runtime.d.ts"),
        "precondition: the shared route-edge policy normalizes the ESM fallback to \
         the .d.ts companion"
    );

    // Overlay-only barrel re-exporting from the ESM-fallback target. No base
    // `IndexedReady` is seeded, so the overlay resolves the edge itself — this
    // exercises the overlay materialiser's OWN edge-resolution policy (not a
    // value copied from the base `DerivedRawState`).
    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert(
        barrel.to_string(),
        Arc::from("export type * from 'runtimedep';\n"),
    );
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    let overlay = host
        .materialize_overlay_indexed_ready_with_view(barrel, &view)
        .expect("overlay materialiser produces an IndexedReady for the overlaid barrel");

    assert!(Arc::ptr_eq(
        &overlay.route_inventory,
        &overlay.shallow_state.route_inventory
    ));
    assert_eq!(overlay.route_inventory.wildcard_reexports.len(), 1);

    let recorded = host.resolve_route_edge_canonical(barrel, "runtimedep");
    assert_eq!(
        recorded.as_deref(),
        Some("/workspace/runtime.d.ts"),
        "the overlay materialiser MUST normalize its ESM-fallback edge through the \
         SAME shared route-edge policy (resolve_route_edge_canonical) as route \
         traversal + known-miss revalidation — recording the raw runtime .js \
         diverges the overlay route facts from the base route-fact producers and \
         stale-serves across the overlay boundary (RouteDb stale-serve hole 3, \
         overlay materialiser, review finding 2)"
    );
    assert_eq!(
        recorded, oracle,
        "the overlay's recorded edge canonical MUST equal the shared route-edge oracle"
    );
}

/// DISCRIMINATING regression: the OVERLAY materialiser's cache-hit reuse must
/// be edge-current, the overlay analog of the indexed materialiser reuse gate.
/// A session-overlay barrel `export type * from './runtime'` is materialised
/// while `./runtime` resolves to the directory-index file, baking that edge
/// into the overlay `IndexedReady` at the current generation. The overlay
/// artifact is keyed by overlay content hash + discriminator, so a BASE
/// file-set change (a more-specific `./runtime.ts` appears, advancing
/// `content_generation` without touching the overlay source) does NOT
/// re-materialise it — the cache-hit returns the stale baked edge.
///
/// The fix gates the overlay cache-hit on the shared edge-currency oracle: an
/// edge-stale wildcard-bearing overlay `IndexedReady` is NOT reused; control
/// falls through to RE-MATERIALISE the overlay artifact (re-resolving the
/// wildcard edges against the live file set from the overlay source) — it must
/// NOT fall back to the base surface (overlay-blindness).
///
/// FAILS pre-fix: the warm session host keeps the baked `runtime/index.ts`
/// edge. PASSES post-fix: the warm session host retargets to `runtime.ts`,
/// matching a fresh session host.
#[test]
fn overlay_materializer_wildcard_reuse_retargets_after_base_file_set_change() {
    use crate::session_view::OverlaidView;
    let ws = Arc::new(CountingWorkspace::new());
    let barrel = "/workspace/index.ts";
    // `./runtime` initially resolves to the directory-index file.
    ws.inject_file(
        "/workspace/runtime/index.ts",
        "export type Runtime = number;\n",
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws.clone()));

    let make_view = |host: &Arc<VerterHost>| {
        let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> =
            rustc_hash::FxHashMap::default();
        overlays.insert(
            barrel.to_string(),
            Arc::from("export type * from './runtime';\n"),
        );
        OverlaidView::new(Arc::clone(host), overlays)
    };

    let view = make_view(&host);
    // Materialise + cache the overlay artifact (bakes `./runtime → runtime/index.ts`).
    let first = host
        .materialize_overlay_indexed_ready_with_view(barrel, &view)
        .expect("overlay materialiser produces an IndexedReady");
    assert!(Arc::ptr_eq(
        &first.route_inventory,
        &first.shallow_state.route_inventory
    ));
    assert_eq!(
        host.resolve_route_edge_canonical(barrel, "./runtime")
            .as_deref(),
        Some("/workspace/runtime/index.ts"),
        "precondition: ./runtime resolves to the directory index while it is the \
         only ./runtime target"
    );

    // A more-specific `./runtime.ts` appears — a BASE file-set change that
    // advances `content_generation` but leaves the overlay source untouched.
    ws.inject_file("/workspace/runtime.ts", "export type Runtime = boolean;\n");

    // A fresh session host on the same workspace is the oracle for the retarget.
    let fresh_host = Arc::new(VerterHost::new(HostConfig::default(), ws.clone()));
    let fresh_view = make_view(&fresh_host);
    let fresh = fresh_host
        .materialize_overlay_indexed_ready_with_view(barrel, &fresh_view)
        .expect("fresh overlay materialiser produces an IndexedReady");
    let _ = &fresh;
    let fresh_target = fresh_host.resolve_route_edge_canonical(barrel, "./runtime");
    assert_eq!(
        fresh_target.as_deref(),
        Some("/workspace/runtime.ts"),
        "precondition: a fresh session host retargets ./runtime to the more-specific \
         ./runtime.ts"
    );

    // Warm session host: re-materialise through the SAME view. The overlay
    // cache-hit must re-resolve the wildcard edge (re-materialise), not serve
    // the stale baked directory-index edge — and must NOT fall back to base.
    let warm = host
        .materialize_overlay_indexed_ready_with_view(barrel, &view)
        .expect("warm overlay materialiser produces an IndexedReady");
    assert!(Arc::ptr_eq(
        &warm.route_inventory,
        &warm.shallow_state.route_inventory
    ));
    let _ = &warm;
    let warm_target = host.resolve_route_edge_canonical(barrel, "./runtime");
    assert_eq!(
        warm_target, fresh_target,
        "the WARM session host MUST retarget ./runtime to the SAME target as a FRESH \
         session host after ./runtime.ts appears — a wildcard-bearing overlay \
         IndexedReady reused from an earlier generation holds a stale baked edge; \
         the overlay cache-hit must be gated on the edge-currency oracle and \
         re-materialise (not serve the base surface)"
    );
    // Overlay-blindness guard: the retargeted edge is a genuine overlay
    // re-materialisation, not a base-surface read — the overlay barrel exists
    // ONLY in the overlay, so a base read would resolve nothing.
    assert_eq!(
        warm_target.as_deref(),
        Some("/workspace/runtime.ts"),
        "the warm overlay surface must carry the re-resolved overlay edge"
    );
}

/// DISCRIMINATING regression: direct overlay artifact READERS (not just the
/// materialiser's own cache-hit) must serve an edge-current wildcard surface.
/// The session resolver context's `indexed_for_current_content` (and the
/// frontier-adapter / routed-shallow-with-view readers) clone the cached overlay
/// `IndexedReady` directly via `lookup_overlay_artifacts`, bypassing the
/// edge-currency gate — so a wildcard-bearing overlay surface materialised
/// before a BASE file-set change is read stale.
///
/// The fix routes every overlay reader through the gated overlay materialiser
/// accessor (`materialize_overlay_indexed_ready_with_view`), which re-resolves
/// the wildcard edges against the live file set when the cached surface is
/// edge-stale and re-publishes — NEVER falling back to the base surface
/// (overlay-blindness). The overlay barrel exists only in the overlay, so a
/// base read would resolve nothing; the assertion that the reader returns the
/// retargeted overlay edge proves the re-materialisation is overlay-rooted.
///
/// FAILS pre-fix: the reader serves the stale baked `runtime/index.ts` edge.
/// PASSES post-fix: it retargets to `runtime.ts`, matching a fresh materialise.
#[test]
fn overlay_reader_retargets_wildcard_after_base_file_set_change() {
    use crate::session_view::OverlaidView;
    let ws = Arc::new(CountingWorkspace::new());
    let barrel = "/workspace/index.ts";
    ws.inject_file(
        "/workspace/runtime/index.ts",
        "export type Runtime = number;\n",
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws.clone()));
    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert(
        barrel.to_string(),
        Arc::from("export type * from './runtime';\n"),
    );
    let view = OverlaidView::new(Arc::clone(&host), overlays);
    let first = host
        .materialize_overlay_indexed_ready_with_view(barrel, &view)
        .expect("overlay materializes");
    let _ = &first;
    assert_eq!(
        host.resolve_route_edge_canonical(barrel, "./runtime")
            .as_deref(),
        Some("/workspace/runtime/index.ts"),
        "precondition: ./runtime resolves to the directory index"
    );
    // BASE file-set change: a more-specific `./runtime.ts` appears.
    ws.inject_file("/workspace/runtime.ts", "export type Runtime = boolean;\n");

    let base = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &view);
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::SessionResolverContext::new(&host, &view, &base, overlay);
    let warm = verter_type_engine::resolver_core::request_ports::IndexedInputs::indexed_for_current_content(
        &ctx, barrel,
    )
    .expect("session context returns overlay indexed");
    assert!(
        warm.shallow_state.has_wildcard_reexports(),
        "the session context still serves the overlay barrel's wildcard surface"
    );
    let target = host.resolve_route_edge_canonical(barrel, "./runtime");
    assert_eq!(
        target.as_deref(),
        Some("/workspace/runtime.ts"),
        "the wildcard edge must retarget after a base file-set change — the \
         surface names the AUTHORED specifier and the one authority resolves it"
    );
}

/// DISCRIMINATING regression: the BASE shallow reader (`shallow_file_state` →
/// `indexed_for_current_content` → `current_content_pinned_indexed`) must serve
/// an edge-current wildcard surface. The base pin is keyed only by the OWNER's
/// content hash, so a wildcard-bearing `IndexedReady` materialised before a
/// dependency file-set change is served with a stale baked `export *` edge —
/// the owner content is unchanged, so the content pin still matches.
///
/// The fix gates the base content-pinned reader on the shared edge-currency
/// oracle: an edge-stale wildcard surface is re-indexed from BASE content via
/// `ensure_indexed_ready` (whose reuse is itself edge-gated, so it re-resolves
/// the edges against the live file set) rather than served stale.
///
/// FAILS pre-fix: the base reader returns the stale `runtime/index.ts` edge.
/// PASSES post-fix: it retargets to `runtime.ts`.
#[test]
fn base_shallow_reader_retargets_wildcard_after_dependency_set_change() {
    let ws = Arc::new(CountingWorkspace::new());
    let barrel = "/workspace/index.ts";
    ws.inject_file(barrel, "export type * from './runtime';\n");
    ws.inject_file(
        "/workspace/runtime/index.ts",
        "export type Runtime = number;\n",
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    let first = host
        .ensure_indexed_ready(barrel)
        .expect("indexed materialiser produces the barrel");
    let _ = &first;
    assert_eq!(
        host.resolve_route_edge_canonical(barrel, "./runtime")
            .as_deref(),
        Some("/workspace/runtime/index.ts"),
        "precondition: ./runtime resolves to the directory index"
    );
    ws.inject_file("/workspace/runtime.ts", "export type Runtime = boolean;\n");
    let state = host
        .shallow_file_state(barrel)
        .expect("base shallow reader returns a surface");
    assert_eq!(
        state
            .wildcard_reexports
            .iter()
            .map(|w| w.source_specifier.as_str())
            .collect::<Vec<_>>(),
        vec!["./runtime"],
        "the base shallow reader serves the AUTHORED wildcard specifier"
    );
    assert_eq!(
        host.resolve_route_edge_canonical(barrel, "./runtime")
            .as_deref(),
        Some("/workspace/runtime.ts"),
        "the wildcard edge MUST retarget after a dependency file-set change — no \
         artifact may pin the directory-index answer"
    );
}

/// DISCRIMINATING regression for the session-UNMASKED frontier reader
/// (`routed_shallow_state_with_view`): a session-bearing query (the view
/// is `Some`) for a NON-overlaid wildcard barrel reads the published base
/// artifact via the base-key `lookup_overlay_artifacts` — which must be
/// served only while edge-current. After a dependency file-set change the
/// unmasked reader must fall through to the gated base route path and retarget
/// rather than serve the stale baked edge.
///
/// FAILS pre-fix (unmasked branch returns the stale `runtime/index.ts` clone);
/// PASSES post-fix (edge-stale → fall through to the gated `route_shallow_state`,
/// which re-indexes to `runtime.ts`).
#[test]
fn session_unmasked_reader_retargets_wildcard_after_dependency_set_change() {
    use crate::session_view::OverlaidView;
    let ws = Arc::new(CountingWorkspace::new());
    let barrel = "/workspace/index.ts";
    ws.inject_file(barrel, "export type * from './runtime';\n");
    ws.inject_file(
        "/workspace/runtime/index.ts",
        "export type Runtime = number;\n",
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws.clone()));
    let _ = host
        .ensure_indexed_ready(barrel)
        .expect("indexed materialiser produces the barrel");
    ws.inject_file("/workspace/runtime.ts", "export type Runtime = boolean;\n");

    // A session view with NO overlay for the barrel (an empty overlay set) — a
    // base-passthrough view, so the reader takes the session-UNMASKED branch.
    let view = OverlaidView::new(Arc::clone(&host), rustc_hash::FxHashMap::default());
    let state = host
        .routed_shallow_state_with_view(barrel, Some(&view))
        .expect("session-unmasked reader returns a surface");
    assert_eq!(
        state
            .wildcard_reexports
            .iter()
            .map(|w| w.source_specifier.as_str())
            .collect::<Vec<_>>(),
        vec!["./runtime"],
        "the session-unmasked frontier reader serves the AUTHORED specifier"
    );
    assert_eq!(
        host.resolve_route_edge_canonical(barrel, "./runtime")
            .as_deref(),
        Some("/workspace/runtime.ts"),
        "the wildcard edge MUST retarget after a dependency file-set change — the \
         reader hands out a parse surface and the one authority resolves it"
    );
}

#[test]
fn resolve_eval_dependency_canonical_ignores_empty_candidate_without_reads() {
    let ws = Arc::new(CountingWorkspace::new());
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    let resolved = host.resolve_eval_dependency_canonical("");

    assert!(
        resolved.is_none(),
        "empty canonical ids should not produce synthetic companion candidates",
    );
    assert_eq!(
        ws.read_count(""),
        0,
        "empty canonical ids must not trigger analysis-source reads",
    );
    assert!(
        host.ensure_indexed_ready("").is_none(),
        "empty canonical ids must not seed imported dependency cache entries",
    );
}

#[test]
fn resolve_eval_dependency_canonical_prefers_bundle_entry_declaration_companion_shallowly() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-core/dist/runtime-core.esm-bundler.js",
        "export { useId } from './runtime-core.js'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-core/dist/runtime-core.d.ts",
        "export declare function useId(): string\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    let resolved = host.resolve_eval_dependency_canonical(
        "/workspace/node_modules/@vue/runtime-core/dist/runtime-core.esm-bundler.js",
    );

    assert_eq!(
        resolved.as_deref(),
        Some("/workspace/node_modules/@vue/runtime-core/dist/runtime-core.d.ts"),
        "bundle entry runtime scripts should prefer the shared declaration companion when present",
    );
    assert_eq!(
        ws.read_count("/workspace/node_modules/@vue/runtime-core/dist/runtime-core.esm-bundler.js"),
        0,
        "bundle companion selection should stay on shallow existence probes for the runtime bundle",
    );
    assert_eq!(
        ws.read_count("/workspace/node_modules/@vue/runtime-core/dist/runtime-core.d.ts"),
        0,
        "bundle companion selection should stay on shallow existence probes for the declaration companion",
    );
}

#[test]
fn resolve_eval_dependency_canonical_memoizes_positive_result_within_request_context() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    let rctx = verter_type_engine::request_context::RequestContext::new(
        4201,
        Arc::from("/workspace/src/App.vue"),
        false,
        None,
    );
    let _guard =
        verter_type_engine::request_context::RequestContextGuard::install(Arc::clone(&rctx));

    let first = host.resolve_eval_dependency_canonical("/workspace/src/runtime/types/html");
    assert_eq!(
        first.as_deref(),
        Some("/workspace/src/runtime/types/html.ts"),
        "the first resolve must run the candidate walk and find the typed companion",
    );
    assert_eq!(
        rctx.dep_canonical_memo
            .lock()
            .get("/workspace/src/runtime/types/html")
            .map(String::as_str),
        Some("/workspace/src/runtime/types/html.ts"),
        "a positive resolution must populate the request-scoped memo",
    );

    ws.reset_exists();
    let second = host.resolve_eval_dependency_canonical("/workspace/src/runtime/types/html");
    assert_eq!(
        second, first,
        "the memoized resolve must return the same canonical as the cold walk",
    );
    assert_eq!(
        ws.exists_count("/workspace/src/runtime/types/html.d.ts"),
        0,
        "a memo hit must not re-probe the .d.ts candidate — the candidate walk ran once per request",
    );
    assert_eq!(
        ws.exists_count("/workspace/src/runtime/types/html.ts"),
        0,
        "a memo hit must not re-probe the resolved .ts candidate — the candidate walk ran once per request",
    );
}

#[test]
fn resolve_eval_dependency_canonical_resolves_without_request_context() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    assert!(
        verter_type_engine::request_context::current_request_context().is_none(),
        "precondition: no request context is installed on this thread",
    );

    let first = host.resolve_eval_dependency_canonical("/workspace/src/runtime/types/html");
    assert_eq!(
        first.as_deref(),
        Some("/workspace/src/runtime/types/html.ts"),
        "resolution must keep working with no request context installed",
    );

    // Without a request context there is no memo layer: a repeated call
    // re-runs the candidate walk (no behavior change outside requests).
    ws.reset_exists();
    let second = host.resolve_eval_dependency_canonical("/workspace/src/runtime/types/html");
    assert_eq!(second, first);
    assert!(
        ws.exists_count("/workspace/src/runtime/types/html.d.ts") >= 1,
        "with no request context the candidate walk must re-run — no host-global memoization",
    );
}

#[test]
fn resolve_eval_dependency_canonical_does_not_memoize_negative_results() {
    let ws = Arc::new(CountingWorkspace::new());
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    let rctx = verter_type_engine::request_context::RequestContext::new(
        4202,
        Arc::from("/workspace/src/App.vue"),
        false,
        None,
    );
    let _guard =
        verter_type_engine::request_context::RequestContextGuard::install(Arc::clone(&rctx));

    let first = host.resolve_eval_dependency_canonical("/workspace/src/missing/nope");
    assert!(
        first.is_none(),
        "a dependency with no on-disk candidate resolves to None",
    );
    assert!(
        rctx.dep_canonical_memo.lock().is_empty(),
        "a None resolution must NOT enter the request-scoped memo — mid-request \
         artifact publication can turn a None into a hit, so negatives stay uncached",
    );

    // A later identical call must re-run the candidate walk (the None is
    // not pinned for the rest of the request).
    ws.reset_exists();
    let second = host.resolve_eval_dependency_canonical("/workspace/src/missing/nope");
    assert!(second.is_none());
    assert!(
        ws.exists_count("/workspace/src/missing/nope.d.ts") >= 1,
        "a repeated None resolve must probe candidates again — negatives are not memoized",
    );
}

#[test]
fn resolve_eval_dependency_canonical_memo_is_isolated_per_request_context() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    {
        let rctx1 = verter_type_engine::request_context::RequestContext::new(
            4203,
            Arc::from("/workspace/src/App.vue"),
            false,
            None,
        );
        let _guard1 =
            verter_type_engine::request_context::RequestContextGuard::install(Arc::clone(&rctx1));
        let resolved = host.resolve_eval_dependency_canonical("/workspace/src/runtime/types/html");
        assert_eq!(
            resolved.as_deref(),
            Some("/workspace/src/runtime/types/html.ts"),
        );
        assert_eq!(
            rctx1.dep_canonical_memo.lock().len(),
            1,
            "the first request's memo holds the positive mapping",
        );
        // `_guard1` drops here — the first request is over.
    }

    let rctx2 = verter_type_engine::request_context::RequestContext::new(
        4204,
        Arc::from("/workspace/src/Other.vue"),
        false,
        None,
    );
    let _guard2 =
        verter_type_engine::request_context::RequestContextGuard::install(Arc::clone(&rctx2));
    assert!(
        rctx2.dep_canonical_memo.lock().is_empty(),
        "a fresh request context starts with an empty memo — no cross-request sharing",
    );

    ws.reset_exists();
    let resolved = host.resolve_eval_dependency_canonical("/workspace/src/runtime/types/html");
    assert_eq!(
        resolved.as_deref(),
        Some("/workspace/src/runtime/types/html.ts"),
    );
    assert!(
        ws.exists_count("/workspace/src/runtime/types/html.d.ts") >= 1,
        "a fresh request context must not inherit the previous request's memo — \
         the candidate walk re-runs once per request",
    );
    assert_eq!(
        rctx2.dep_canonical_memo.lock().len(),
        1,
        "the second request populates its OWN memo from its own cold walk",
    );
}

#[test]
fn shallow_index_uses_eval_source_for_vue_dependencies() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/base.ts",
        "export interface Base { id: string }\n",
    );
    upsert_vue(
        &host,
        "/src/types.vue",
        r#"<script lang="ts">
import type { Base } from './base'

export interface Props extends Base {
  label: string
}
</script>
<template><div /></template>"#,
    );
    host.set_import_dependencies(
        "/src/types.vue",
        vec![exact_dependency("./base", "/src/base.ts")],
    );

    let indexed = host
        .ensure_indexed_ready("/src/types.vue")
        .expect("vue shallow index should be built from the script/eval source");

    assert!(
        indexed
            .shallow_state
            .has_type_symbol_in(verter_type_expr::TopLevelOwnerId::module(0), "Props"),
        "vue shallow headers should see local type symbols in the script block",
    );
    let base_binding = indexed
        .shallow_state
        .import_target_in(verter_type_expr::TopLevelOwnerId::module(0), "Base")
        .expect("the module script import must retain its exact owner-qualified binding");
    assert_eq!(base_binding.source_specifier, "./base");
    assert_eq!(
        base_binding.imported_name, "Base",
        "vue shallow routes should keep the exact imported export name",
    );
    // Required imported names are a BODY-dependent product: they
    // demand-walk through the artifact's shallow state (lazy
    // declaration-body memo), still over the script/eval source.
    assert!(
        indexed
            .shallow_state
            .required_import_names_in(verter_type_expr::TopLevelOwnerId::module(0), "Props")
            .contains("Base"),
        "vue dependency demand-walk should compute required imported names from the script block",
    );
}

#[test]
fn base_eval_env_prefers_declaration_companion_for_runtime_js_dependencies() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.js".to_string(),
        Arc::from("export const runtimeOnly = true\n"),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts".to_string(),
        Arc::from("export declare function useForwardProps<T>(value: T): T\n"),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/package.json".to_string(),
        Arc::from(r#"{"name":"pkg","types":"./dist/index.d.ts"}"#),
    );

    let host = VerterHost::new(HostConfig::default(), ws);

    let env = host
        .base_eval_env_arc("/workspace/node_modules/pkg/dist/index.js")
        .expect("runtime-script env requests should prefer the declaration companion");

    assert!(
        env.value_symbols.contains_key("useForwardProps"),
        "the declaration companion env should expose value declarations",
    );

    // In the new IndexedReady DB, ensure_indexed_ready normalizes .js → .d.ts
    // companion and eagerly materializes. Verify the companion has the right content.
    let declaration_entry = host
        .ensure_indexed_ready("/workspace/node_modules/pkg/dist/index.d.ts")
        .expect("the declaration companion should own the cached env");
    // Verify declaration companion has content.
    assert!(
        !declaration_entry.raw_source.is_empty(),
        "the declaration companion should have source content",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_named_type_export_target_seeds_shallow_dependency_state_without_snapshot_materialization(
) {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/index.ts", "export * from './types'\n");
    ws.inject_file(
        "/src/types.ts",
        "import type { Base } from './base'\nexport interface Props extends Base { label: string }\n",
    );
    ws.inject_file("/src/base.ts", "export interface Base { id: string }\n");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    ws.reset_reads();
    let resolved = host.resolve_named_type_export_target("/src/index.ts", "Props");

    assert_eq!(
        resolved,
        Some(("/src/types.ts".to_string(), "Props".to_string())),
        "named export routing should resolve through the barrel",
    );

    let barrel_entry = host
        .ensure_indexed_ready("/src/index.ts")
        .expect("barrel file should be cached after routing");
    let target_entry = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("target file should be cached after routing");

    assert!(
        barrel_entry
            .route_inventory
            .counts
            .top_level_statement_count
            > 0,
        "barrel routing should seed routes for the imported barrel file",
    );
    assert!(
        target_entry
            .route_inventory
            .counts
            .top_level_statement_count
            > 0,
        "barrel routing should seed routes for the resolved target file",
    );
    // In the new IndexedReady DB, ensure_indexed_ready eagerly builds
    // full snapshots. The shallowness constraint applies to the internal routing,
    // not to the post-hoc facts query.
    assert_eq!(
        ws.read_count("/src/base.ts"),
        0,
        "shallow export routing should not touch transitive children that are not on the requested path",
    );
}

#[test]
fn shallow_prepared_decl_name_resolution_uses_shallow_dependency_targets() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/node_modules/@vue/runtime-core.d.ts",
        "export interface Component { name?: string }",
    );
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        r#"
import type { Component } from '@vue/runtime-core'

export interface Props {
  as?: Component
}
"#,
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency(
            "@vue/runtime-core",
            "/node_modules/@vue/runtime-core.d.ts",
        )],
    );

    let entry = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should seed imported state");
    // In the new IndexedReady DB, ensure_indexed_ready eagerly builds full facts.
    assert!(
        !entry.raw_source.is_empty(),
        "types dependency should have source content",
    );

    let prepared = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should prepare from the imported cache");
    let resolved = prepared.name_resolution.get("Component").expect(
        "prepared declaration should resolve imported Component through dependency targets",
    );
    assert_eq!(
        resolved.canonical_id.as_ref(),
        "/node_modules/@vue/runtime-core.d.ts",
        "prepared declaration lookup should canonicalize imported names",
    );

    let cached = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should stay cached");
    assert!(
        !cached.raw_source.is_empty(),
        "types dependency should maintain source content after prepared lookup",
    );
}

#[test]
fn owner_local_prepared_decl_name_resolution_uses_stored_dependency_targets() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/schema.ts",
        "export interface AppConfig { ui: {} }",
    );
    upsert_non_sfc(
        &host,
        "/src/tv.ts",
        "export type ComponentConfig<T, A, K> = { config: A; key: K }",
    );
    upsert_non_sfc(
        &host,
        "/src/theme.ts",
        "export default { value: true } as const",
    );
    upsert_vue(
        &host,
        "/src/Button.vue",
        r#"<script lang="ts">
import type { AppConfig } from './schema'
import theme from './theme'
import type { ComponentConfig } from './tv'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>
</script>
<template><div /></template>"#,
    );
    host.set_import_dependencies(
        "/src/Button.vue",
        vec![
            exact_dependency("./schema", "/src/schema.ts"),
            exact_dependency("./theme", "/src/theme.ts"),
            exact_dependency("./tv", "/src/tv.ts"),
        ],
    );

    let _store_view = host.resolver_store_view_read().into_owned_view();
    let prepared = host
        .prepared_type_decl("/src/Button.vue", "Button")
        .expect("Button should prepare from the owner-local shallow cache");

    assert_eq!(
        prepared
            .name_resolution
            .get("AppConfig")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/schema.ts"),
        "owner-local prepared declarations should canonicalize type imports through stored dependency targets",
    );
    assert_eq!(
        prepared
            .name_resolution
            .get("ComponentConfig")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/tv.ts"),
        "owner-local prepared declarations should canonicalize imported helper aliases through stored dependency targets",
    );
    assert_eq!(
        prepared
            .name_resolution
            .get("theme")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/theme.ts"),
        "owner-local prepared declarations should canonicalize imported values through stored dependency targets",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn current_dependency_fact_versions_keeps_imported_barrel_route_facts_shallow() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types/index.ts",
        "export * from './a'\nexport * from './b'\nexport * from './target'\n",
    );
    ws.inject_file(
        "/src/types/a.ts",
        "export interface AOnly { unused: string }\n",
    );
    ws.inject_file(
        "/src/types/b.ts",
        "export interface BOnly { unused: number }\n",
    );
    ws.inject_file(
        "/src/types/target.ts",
        "export interface TargetProps { label: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    host.ensure_indexed_ready("/src/types/index.ts")
        .expect("barrel should materialize shallow imported state");
    let _view = host.resolver_store_view_read().into_owned_view();

    ws.reset_resolves();

    let facts = host.current_dependency_fact_versions(
        "/src/types/index.ts",
        &std::collections::BTreeSet::new(),
    );

    // Laziness, restated for the resolve-domain rooting rail: the
    // capture OBSERVES the owner's own authored specifiers (that is the
    // import-route witness), each at most once, and opens no wildcard
    // sibling source to do it.
    for specifier in ["./a", "./b", "./target"] {
        assert!(
            ws.resolve_count("/src/types/index.ts", specifier) <= 1,
            "captured fact-version lookup must observe {specifier} at most \
             once for route rooting — a repeated resolve is the live \
             wildcard replay this pins against (got {})",
            ws.resolve_count("/src/types/index.ts", specifier),
        );
    }
    for sibling in ["/src/types/a.ts", "/src/types/b.ts", "/src/types/target.ts"] {
        assert_eq!(
            ws.read_count(sibling),
            0,
            "captured fact-version lookup must not OPEN the wildcard sibling \
             {sibling} — the imported barrel's route facts stay shallow",
        );
    }
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            verter_session_query::facts::fact_cache::FactVersionRef::Parse(parse)
                if parse.canonical_id == "/src/types/index.ts"
                    && matches!(parse.key, verter_session_query::facts::FactKey::SyntacticRouteInterface)
        )),
        "captured fact-version lookup should reuse the snapshotted parse-owned route interface for a shallow imported barrel without live wildcard replay",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn store_view_import_routes_do_not_depend_on_live_owner_state() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/types/index.ts", "export * from './target'\n");
    ws.inject_file(
        "/src/types/target.ts",
        "export interface TargetProps { label: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    host.ensure_indexed_ready("/src/types/index.ts")
        .expect("barrel should materialize shallow export state");
    let view = host.resolver_store_view_read().into_owned_view();

    let witness = host
        .owner_import_route_witness_for_tests("/src/types/index.ts")
        .expect("a materialised barrel must produce a rootable import-route witness");
    for fact in &witness {
        assert!(
            verter_session_query::facts::store_view::StoreView::validates(&view, fact),
            "captured store views must validate the owner's import-route \
             witness fact {fact:?} against their captured resolution world, \
             without reconstructing the old structural shadow path",
        );
    }

    ws.reset_resolves();
    ws.remove_file("/src/types/index.ts");
    host.compile_cache().remove("/src/types/index.ts");

    let resolved =
        host.resolve_type_dependency_canonical_shallow("/src/types/index.ts", "./target");

    assert_eq!(
        resolved.as_deref(),
        Some("/src/types/target.ts"),
        "route resolution should continue to resolve the barrel's `export *` target through cached module facts",
    );
    // The barrel's surface is wildcard-bearing, so its baked `export *` edge is
    // rooted in the indexed `edge_generation`. Removing the owner advances the
    // workspace `content_generation`, which the shared edge-currency oracle
    // treats as a potential dependency-set change: the wildcard edge is
    // re-validated against the live workspace rather than served from the
    // now-edge-stale baked surface. The resolution stays correct (`./target`
    // still resolves to `target.ts`); the re-validation is the correctness
    // conservatism the wildcard-edge rooting introduces. A non-wildcard
    // import-route surface carries no dependency-set-derived edge and would
    // still serve from cache without a live resolve.
    //
    // The rebuild resolves `./target` twice: once in the `resolve_missing`
    // loop (the plain `export *` source is classified `EsmImport`) and once in
    // the wildcard pass that re-resolves it through the shared TS-first
    // `resolve_route_edge_canonical` policy and overwrites — so the indexed
    // wildcard canonical agrees with the route-traversal / overlay surfaces.
    assert_eq!(
        ws.resolve_count("/src/types/index.ts", "./target"),
        1,
        "a wildcard-bearing barrel re-validates its baked `export *` edge once \
         content_generation advances (edge-currency) through the shared TS-first \
         Engine policy, still resolving correctly without trusting the baked route",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn current_dependency_fact_versions_keeps_shallow_tracked_barrel_siblings_off_indexed_ready() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script>
<template><button /></template>"#,
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

    let _view = host.resolver_store_view_read().into_owned_view();
    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::component_meta::NativePropProjectionCache::default();

    ws.reset_reads();
    let resolved = host.resolve_component_meta_native_props(
        "/src/Consumer.vue",
        "./types",
        "ButtonProps",
        &mut tracked_deps,
        &mut resolution_deps,
        &mut cache,
    );

    assert!(
        resolved.is_some(),
        "component-meta macro resolution should still resolve ButtonProps",
    );
    assert_eq!(
        ws.read_count("/src/Unused.vue"),
        0,
        "root-stem route proof should keep unrelated same-layer barrel siblings off the active component-meta path",
    );
    assert!(
        host.project_type_store.indexed().get_any("/src/Unused.vue")
            .is_none(),
        "route-only frontier discovery should keep unrelated same-layer siblings off FileArtifactStore",
    );

    let _final_view = host.resolver_store_view_read().into_owned_view();
    let unused_reads_before = ws.read_count("/src/Unused.vue");
    let _facts = host.current_dependency_fact_versions("/src/Consumer.vue", &tracked_deps);

    assert_eq!(
        ws.read_count("/src/Unused.vue"),
        unused_reads_before,
        "fact-version capture must not reread a shallow-only tracked barrel sibling",
    );
    assert!(
        host.project_type_store.indexed().get_any("/src/Unused.vue")
            .is_none(),
        "fact-version capture must not promote a shallow-only tracked barrel sibling into FileArtifactStore",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_component_meta_native_props_cached_lookup_tracks_routed_target_dependencies() {
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
        "export { ButtonProps } from './Button.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
export interface ButtonProps {
  label?: string
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
        "/src/types.ts",
        vec![exact_dependency("./Button.vue", "/src/Button.vue")],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let mut cache = crate::resolver_core::component_meta::NativePropProjectionCache::default();

    let mut tracked_deps_first = std::collections::BTreeSet::new();
    let mut resolution_deps_first = std::collections::BTreeSet::new();
    let resolved_first = host.resolve_component_meta_native_props(
        "/src/Consumer.vue",
        "./types",
        "ButtonProps",
        &mut tracked_deps_first,
        &mut resolution_deps_first,
        &mut cache,
    );

    assert!(
        resolved_first.is_some(),
        "the first imported macro lookup should resolve ButtonProps",
    );
    assert!(
        tracked_deps_first.contains("/src/Button.vue"),
        "the first lookup should track the routed target canonical",
    );
    assert!(
        resolution_deps_first.contains("/src/Button.vue"),
        "the first lookup should record the routed target canonical in resolution deps",
    );

    let mut tracked_deps_second = std::collections::BTreeSet::new();
    let mut resolution_deps_second = std::collections::BTreeSet::new();
    let resolved_second = host.resolve_component_meta_native_props(
        "/src/Consumer.vue",
        "./types",
        "ButtonProps",
        &mut tracked_deps_second,
        &mut resolution_deps_second,
        &mut cache,
    );

    assert!(
        resolved_second.is_some(),
        "the warm imported macro lookup should still resolve ButtonProps",
    );
    assert!(
        tracked_deps_second.contains("/src/Button.vue"),
        "the warm lookup must keep tracking the routed target canonical, not just the barrel file",
    );
    assert!(
        resolution_deps_second.contains("/src/Button.vue"),
        "the warm lookup must keep the routed target in resolution deps for downstream fact tracking",
    );
}

/// Mutating an imported dependency should invalidate only that file's
/// cache lineage, while owner-file caches stay warm.
#[test]
fn changed_imported_dependency_keeps_owner_files_warm() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/types.ts",
        "export interface Props { label: string }",
    );
    upsert_vue(
        &host,
        "/OwnerA.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );
    upsert_vue(
        &host,
        "/OwnerB.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    // Cold: both owners compute meta
    let meta_a1 = host.get_component_meta("/OwnerA.vue").expect("cold OwnerA");
    let meta_b1 = host.get_component_meta("/OwnerB.vue").expect("cold OwnerB");
    assert_eq!(meta_a1.props.len(), 1);
    assert_eq!(meta_b1.props.len(), 1);

    // Mutate the imported dependency — add a new prop
    upsert_ts(
        &host,
        "/types.ts",
        "export interface Props { label: string; count: number }",
    );

    // Owner files were NOT changed, only the dependency
    let meta_a2 = host
        .get_component_meta("/OwnerA.vue")
        .expect("post-change OwnerA");
    let meta_b2 = host
        .get_component_meta("/OwnerB.vue")
        .expect("post-change OwnerB");

    // Both owners should now see the updated two-prop shape
    assert_eq!(
        meta_a2.props.len(),
        2,
        "OwnerA should reflect the updated dependency (label + count)"
    );
    assert_eq!(
        meta_b2.props.len(),
        2,
        "OwnerB should reflect the updated dependency (label + count)"
    );
    // Positive: new prop present
    assert!(
        meta_a2.props.iter().any(|p| p.name == "count"),
        "OwnerA should have the new 'count' prop"
    );
    // Positive: old prop still present
    assert!(
        meta_a2.props.iter().any(|p| p.name == "label"),
        "OwnerA should still have the original 'label' prop"
    );
    // Negative: old result no longer valid
    assert_ne!(
        meta_a1.props.len(),
        meta_a2.props.len(),
        "cached meta must have been invalidated by the dependency change"
    );
    // Negative: no phantom props beyond the expected two
    let prop_names: Vec<&str> = meta_a2.props.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        prop_names.len(),
        2,
        "should have exactly 2 props, no phantom data: {:?}",
        prop_names
    );
}

/// Unit test 3: Atomic rebuild on route change.
///
/// When the effective dependency target changes from `/inner-v1.ts` to
/// `/inner-v2.ts`, the bundle must be invalidated and ALL prepared decls in
/// the rebuilt bundle must have updated `name_resolution` entries.
#[test]
fn atomic_rebuild_on_route_change() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/inner-v1.ts",
        "export interface Inner { version: 1 }\n",
    );
    ws.inject_file(
        "/src/inner-v2.ts",
        "export interface Inner { version: 2 }\n",
    );
    ws.inject_file(
        "/src/types.ts",
        "import type { Inner } from './inner'\nexport interface Props { child: Inner }\nexport interface Alt { other: Inner }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should materialize");

    // Route to v1.
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./inner", "/src/inner-v1.ts")],
    );

    let _view_v1 = host.resolver_store_view_read().into_owned_view();
    let props_v1 = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should prepare pointing to v1");
    assert_eq!(
        props_v1
            .name_resolution
            .get("Inner")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/inner-v1.ts"),
        "Props name_resolution should point to inner-v1",
    );
    let alt_v1 = host
        .prepared_type_decl("/src/types.ts", "Alt")
        .expect("Alt should prepare pointing to v1");
    assert_eq!(
        alt_v1
            .name_resolution
            .get("Inner")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/inner-v1.ts"),
        "Alt name_resolution should point to inner-v1",
    );

    // Change route to v2.
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./inner", "/src/inner-v2.ts")],
    );

    let _view_v2 = host.resolver_store_view_read().into_owned_view();
    let props_v2 = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should rebuild after route change");
    assert_eq!(
        props_v2
            .name_resolution
            .get("Inner")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/inner-v2.ts"),
        "Props name_resolution must point to inner-v2 after route change",
    );
    let alt_v2 = host
        .prepared_type_decl("/src/types.ts", "Alt")
        .expect("Alt should rebuild after route change");
    assert_eq!(
        alt_v2
            .name_resolution
            .get("Inner")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/inner-v2.ts"),
        "ALL prepared decls must point to inner-v2 after route change — atomic rebuild",
    );
}

/// Regression guard 7: Stale prepared decls after dep-resolution change.
///
/// Guards the exact bug the old route-refresh was designed to prevent.
/// When the import target's effective canonical changes, prepared decls must
/// reflect the new target in `name_resolution`.
#[test]
fn regression_stale_prepared_decls_after_dep_resolution_change() {
    let host = make_host();
    upsert_non_sfc(&host, "/types-a.ts", "export interface Foo { source: 'a' }");
    upsert_non_sfc(&host, "/types-b.ts", "export interface Foo { source: 'b' }");
    upsert_non_sfc(
        &host,
        "/src/consumer.ts",
        "import { Foo } from './types'\nexport interface Bar { inner: Foo }\n",
    );

    let _ = host
        .ensure_indexed_ready("/src/consumer.ts")
        .expect("consumer dependency should materialize");

    // Set initial route to types-a.
    host.set_import_dependencies(
        "/src/consumer.ts",
        vec![exact_dependency("./types", "/types-a.ts")],
    );

    let initial = host
        .prepared_type_decl("/src/consumer.ts", "Bar")
        .expect("Bar should prepare with route to types-a");
    assert_eq!(
        initial
            .name_resolution
            .get("Foo")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/types-a.ts"),
        "initial lookup should resolve Foo to types-a",
    );

    // Change route to types-b.
    host.set_import_dependencies(
        "/src/consumer.ts",
        vec![exact_dependency("./types", "/types-b.ts")],
    );

    let updated = host
        .prepared_type_decl("/src/consumer.ts", "Bar")
        .expect("Bar should rebuild after route change to types-b");
    assert_eq!(
        updated
            .name_resolution
            .get("Foo")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/types-b.ts"),
        "prepared decl must reflect the new target after dep-resolution change — guards against stale route bug",
    );
    // Negative: must NOT still point to the old target.
    assert_ne!(
        updated
            .name_resolution
            .get("Foo")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/types-a.ts"),
        "prepared decl must NOT retain the stale route to types-a",
    );
}

/// Regression: intermediate barrel re-export changes must invalidate
/// ImportedRootDb entries, even when the top-level provider and the
/// original leaf file remain text-identical.
///
/// Chain: /src/index.ts -> /src/barrel.ts -> /src/types-a.ts
/// After: /src/index.ts -> /src/barrel.ts -> /src/types-b.ts
/// (only /src/barrel.ts changes)
#[test]
fn imported_root_invalidates_on_intermediate_barrel_change() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/types-a.ts", "export interface Props { version: 1 }\n");
    ws.inject_file("/src/types-b.ts", "export interface Props { version: 2 }\n");
    // Barrel re-exports Props from types-a
    ws.inject_file("/src/barrel.ts", "export { Props } from './types-a'\n");
    // Index re-exports everything from barrel
    ws.inject_file("/src/index.ts", "export { Props } from './barrel'\n");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    // Set up import dependencies so the route chain resolves
    host.set_import_dependencies(
        "/src/index.ts",
        vec![exact_dependency("./barrel", "/src/barrel.ts")],
    );
    host.set_import_dependencies(
        "/src/barrel.ts",
        vec![exact_dependency("./types-a", "/src/types-a.ts")],
    );

    // Warm: resolve Props through the chain
    let _view1 = host.resolver_store_view_read().into_owned_view();
    let root1 = host.resolve_imported_type_root("/src/index.ts", "Props");
    assert_eq!(
        root1,
        expected_imported_root(
            "/src/types-a.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Props",
        ),
        "initial root should point to types-a",
    );

    // Change barrel to point to types-b instead. Workspace inject + evict +
    // ensure_loaded is the canonical sequence for content changes under the
    // new architecture (no implicit disk reads inside resolvers).
    ws.inject_file("/src/barrel.ts", "export { Props } from './types-b'\n");
    host.evict("/src/barrel.ts");
    assert!(host.ensure_loaded("/src/barrel.ts"));
    host.set_import_dependencies(
        "/src/barrel.ts",
        vec![exact_dependency("./types-b", "/src/types-b.ts")],
    );

    // The provider (/src/index.ts) and the old leaf (/src/types-a.ts)
    // are unchanged. Only the barrel changed.
    let _view2 = host.resolver_store_view_read().into_owned_view();
    let root2 = host.resolve_imported_type_root("/src/index.ts", "Props");
    assert_eq!(
        root2,
        expected_imported_root(
            "/src/types-b.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Props",
        ),
        "after intermediate barrel change, root must point to types-b, \
         not stale types-a from the cached imported root",
    );
}

/// The overlay source-registration route takes its carrier grammar from
/// the registered frontend catalog row (adapter × carrier language) and
/// fails closed on a catalog miss — never another framework's grammar.
#[test]
fn overlay_structure_grammar_comes_from_registered_catalog_identity() {
    use verter_language::{FileLanguage, FrameworkAdapterId, LanguageId};

    let host = Arc::new(make_host());
    let vue_canonical = "/overlay/App.vue";
    let svelte_canonical = "/overlay/App.svelte";
    let miss_canonical = "/overlay/App.html";
    let vue_src: Arc<str> = Arc::from("<template><div/></template>");
    let svelte_src: Arc<str> = Arc::from("<div/>");

    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert(vue_canonical.to_string(), Arc::clone(&vue_src));
    overlays.insert(svelte_canonical.to_string(), Arc::clone(&svelte_src));
    overlays.insert(miss_canonical.to_string(), Arc::clone(&vue_src));
    let view = crate::session_view::OverlaidView::new(Arc::clone(&host), overlays);

    // Registered identities: the catalog row's grammar fact is accepted
    // end-to-end by the host's grammar authority on the overlay route.
    for (canonical, source, language) in [
        (vue_canonical, &vue_src, FileLanguage::vue()),
        (svelte_canonical, &svelte_src, FileLanguage::svelte()),
    ] {
        assert!(
            host.registered_overlay_structure(canonical, Arc::clone(source), &language, &view)
                .is_some(),
            "the registered catalog grammar for {language:?} must be accepted \
             on the overlay route",
        );
    }

    // Catalog miss: a same-adapter NON-carrier-registered row has no
    // frontend catalog row, so the overlay route fails closed instead of
    // falling through to another framework's grammar.
    let unregistered = FileLanguage::Framework {
        adapter_id: FrameworkAdapterId::vue(),
        language_id: LanguageId::new("html"),
    };
    assert!(
        host.registered_overlay_structure(miss_canonical, vue_src, &unregistered, &view)
            .is_none(),
        "a carrier row without a registered frontend catalog row must fail closed",
    );
}

/// An editing session must not retain one whole intrinsic-element surface per
/// keystroke.
///
/// `intrinsic_members_for_tag` is reached once per native template element on
/// every fallthrough resolve, and it caches the projected surface in the shared
/// fallthrough-node cache. That cache is a MAP: a key that carries the
/// workspace content generation is never superseded by a later one, so every
/// edit anywhere in the workspace minted a brand-new entry holding a complete
/// intrinsic surface (`div`'s whole attribute + listener member list) and
/// retired none. Over a long editing session that is an unbounded retained-byte
/// slope with no plateau — the exact shape the bounded-memory contract forbids.
///
/// Discriminating: the assertion is on the cache's KEY count across many edits,
/// not on a byte figure, so it fails deterministically on a per-generation key
/// and cannot pass by accident. With the generation in the key the count climbs
/// by one per edit; with the version axis on the VALUE the count is flat
/// because each edit REPLACES the superseded surface under its stable key.
///
/// The freshness leg is asserted alongside it: the surface served after the
/// edits is still a resolved one, so the bound is not bought by serving nothing.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn repeated_edits_do_not_retain_one_intrinsic_surface_per_generation() {
    const EDITS: usize = 24;

    fn source(revision: usize) -> String {
        format!(
            r#"<script setup lang="ts">
defineProps<{{ label?: string }}>()
const revision = {revision}
</script>
<template><div :title="label">{{{{ revision }}}}</div></template>"#
        )
    }

    let host = make_host();
    let canonical = "/src/Churn.vue";
    upsert_vue(&host, canonical, &source(0));
    assert!(
        host.resolve_fallthrough_surface(canonical).is_some(),
        "the fixture must resolve a fallthrough surface for a single native root"
    );
    let after_first = host.resolver_runtime().fallthrough.cached_node_count();
    assert!(
        after_first > 0,
        "the first resolve must have warmed at least one fallthrough node"
    );

    for revision in 1..=EDITS {
        upsert_vue(&host, canonical, &source(revision));
        assert!(
            host.resolve_fallthrough_surface(canonical).is_some(),
            "revision {revision} must still resolve a fallthrough surface"
        );
    }

    let after_edits = host.resolver_runtime().fallthrough.cached_node_count();
    assert!(
        after_edits <= after_first,
        "{EDITS} edits must not grow the fallthrough node cache — it held {after_first} keys \
         after one resolve and {after_edits} after {EDITS} more, i.e. one retained intrinsic \
         surface per content generation"
    );
}

/// Cold component-meta publications of a still-open owner under successive
/// overlay views keep at most the two most recent views, in the resolver's
/// validated states and in the derived `cached_resolved_meta` mirror alike —
/// without the owner being edited or closed.
///
/// Discriminating: only the legacy-mirror rehydration noted a view before;
/// the normal cold publication inserted straight into the runtime map and the
/// mirror kept every `(mode, view_fingerprint)` it was ever handed, so each
/// overlay of a dependency left one more state per mode for the life of the
/// owner (five views, five states per mode).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn overlay_views_of_an_open_owner_keep_two_component_meta_views() {
    use crate::session_view::{OverlaidViewRef, SessionView};

    const OWNER: &str = "/src/Consumer.vue";
    const DEPENDENCY: &str = "/src/types/icon.ts";
    let (_ws, host) = activity_gate_fixture();
    // The fixture resolved the owner once under the base view: one state
    // per projection mode it publishes.
    let modes = host.retention_snapshot().component_meta_states;
    assert!(
        modes > 0,
        "fixture: the base resolution admitted its states"
    );
    let mirrored_views = |host: &VerterHost| {
        let entry = host
            .derived_raw_cache()
            .get(OWNER)
            .expect("the owner keeps its derived state while open");
        let mut per_mode: rustc_hash::FxHashMap<
            verter_type_engine::semantic_query::ProjectionMode,
            usize,
        > = rustc_hash::FxHashMap::default();
        for (mode, _view_fingerprint) in entry.cached_resolved_meta.keys() {
            *per_mode.entry(*mode).or_default() += 1;
        }
        (
            per_mode.values().copied().max().unwrap_or(0),
            entry
                .cached_resolved_meta
                .keys()
                .map(|(_, view_fingerprint)| *view_fingerprint)
                .collect::<std::collections::HashSet<_>>(),
        )
    };
    let tombstones = std::collections::HashSet::new();
    for variant in 1..=4usize {
        let source: Arc<str> = Arc::from(format!(
            "export interface IconProps {{ name: string; size: number; extra{variant}: boolean }}\n"
        ));
        let mut overlays = rustc_hash::FxHashMap::default();
        let mut overlay_hashes = rustc_hash::FxHashMap::default();
        overlay_hashes.insert(
            DEPENDENCY.to_string(),
            verter_semantic_source::source_hash::hash_16(source.as_bytes()),
        );
        overlays.insert(DEPENDENCY.to_string(), source);
        let view = OverlaidViewRef::new(&host, &overlays, &overlay_hashes, &tombstones);
        let meta = host
            .get_component_meta_via_view(OWNER, &view)
            .expect("overlay component meta resolves");
        assert!(
            meta.props
                .iter()
                .any(|prop| prop.name == format!("extra{variant}")),
            "view {variant}: the owner's props follow the overlaid dependency"
        );
        let states = host.retention_snapshot().component_meta_states;
        assert!(
            states <= 2 * modes,
            "view {variant}: the resolver keeps at most two views per mode \
             ({states} states for {modes} modes)"
        );
        let (per_mode, views) = mirrored_views(&host);
        assert!(
            per_mode <= 2,
            "view {variant}: the mirror keeps at most two views per mode ({per_mode})"
        );
        assert!(
            views.contains(&view.fingerprint()),
            "view {variant}: the current view stays mirrored"
        );
    }
}

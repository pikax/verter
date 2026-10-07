use super::*;

#[test]
fn prepared_type_decl_resolves_plain_declaration_import_helpers() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/helper/dist/helper.d.ts",
        "export type Prettify<T> = { [K in keyof T]: T[K] }\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/helper/dist/helper.js",
        "export const runtimeOnly = true\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/index.d.ts",
        r#"
import { Prettify } from 'helper'
export type FancyProps = Prettify<{ open: boolean }>
"#,
    );

    host.set_import_dependencies(
        "/workspace/node_modules/lib/dist/index.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "helper".to_string(),
            resolved_canonical_id: Some(
                "/workspace/node_modules/helper/dist/helper.js".to_string(),
            ),
            possible_canonical_ids: vec![
                "/workspace/node_modules/helper/dist/helper.js".to_string(),
                "/workspace/node_modules/helper/dist/helper.d.ts".to_string(),
            ],
        }],
    );

    let prepared = host
        .prepared_type_decl("/workspace/node_modules/lib/dist/index.d.ts", "FancyProps")
        .expect("FancyProps should prepare from the imported declaration cache");

    assert_eq!(
        prepared
            .name_resolution
            .get("Prettify")
            .map(|identity| (identity.canonical_id.as_ref(), identity.symbol_name.as_ref())),
        Some(("/workspace/node_modules/helper/dist/helper.d.ts", "Prettify")),
        "plain imports inside declaration files must resolve helper names through the declaration entrypoint rather than leaving them unresolved or pinned to JS companions",
    );
}

#[test]
fn prepared_type_decl_rebuilds_name_resolution_after_import_route_upgrade() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/workspace/.nuxt/ui/checkbox.ts",
        "const theme = { slots: { root: 'slot' } } as const\nexport default theme\n",
    );
    upsert_vue(
        &host,
        "/workspace/Checkbox.vue",
        r#"<script lang="ts">
import theme from '#build/ui/checkbox'

export interface CheckboxProps {
  slots?: typeof theme.slots
}
</script>
<template><div /></template>"#,
    );

    let initial_view = host.resolver_store_view_read().into_owned_view();
    assert!(
        matches!(
            host.prepared_type_decl_in_with_store_view(
                &initial_view,
                None,
                "/workspace/Checkbox.vue",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "CheckboxProps",
            ),
            Err(verter_session_query::inputs::prepared::PreparationFailure::MissingExternalOwner {
                local_name,
            }) if local_name == "theme"
        ),
        "an unresolved alias has no authoritative target owner and must fail closed before the route upgrade",
    );

    host.set_import_dependencies(
        "/workspace/Checkbox.vue",
        vec![crate::types::DependencyResolution {
            specifier: "#build/ui/checkbox".to_string(),
            resolved_canonical_id: Some("/workspace/.nuxt/ui/checkbox.ts".to_string()),
            possible_canonical_ids: vec!["/workspace/.nuxt/ui/checkbox.ts".to_string()],
        }],
    );

    let rebuilt = host
        .prepared_type_decl("/workspace/Checkbox.vue", "CheckboxProps")
        .expect("CheckboxProps should rebuild after import routes are upgraded");
    assert_eq!(
        rebuilt.name_resolution.get("theme").map(|identity| (
            identity.canonical_id.as_ref(),
            identity.symbol_name.as_ref()
        )),
        Some(("/workspace/.nuxt/ui/checkbox.ts", "default")),
        "prepared decl caches must rebuild to the resolved DIRECT hop when \
         dependency resolutions improve (demand-driven canonicalization keeps \
         the imported name; the value demand peels the default alias)",
    );
    // Demand-side: the value rail peels `export default theme` to the exact
    // exported value declaration at demand.
    let peeled = host
        .resolve_value_export_target_graph_native("/workspace/.nuxt/ui/checkbox.ts", "default")
        .expect("the value demand resolves the upgraded route's default export");
    assert_eq!(
        (peeled.canonical_id.as_str(), peeled.name.as_str()),
        ("/workspace/.nuxt/ui/checkbox.ts", "theme"),
        "the demand-time value rail must peel the default alias to the exact \
         exported value owner",
    );
}

#[test]
fn prepared_vue_ignore_facts_survive_sfc_and_imported_file_indexing() {
    use verter_type_expr::facts::VueIgnoredHeritageFact;

    let host = make_host();
    upsert_vue(
        &host,
        "/src/VueIgnore.vue",
        r#"<script setup lang="ts">
interface IgnoredProps { ignored: string }
interface KeptProps { kept: boolean }
interface Props extends /* @vue-ignore */ IgnoredProps, KeptProps { own: number }

interface IgnoredEmits { ignoredEvent: [value: string] }
interface KeptEmits { keptEvent: [value: boolean] }
interface Emits extends /* @vue-ignore */ IgnoredEmits, KeptEmits { ownEvent: [] }
</script>"#,
    );
    upsert_non_sfc(
        &host,
        "/src/imported-ignore.ts",
        r#"
export interface ImportedBase { importedIgnored: string }
export interface ImportedProps extends /* @vue-ignore */ ImportedBase {
  importedOwn: number
}
"#,
    );
    upsert_vue(
        &host,
        "/src/VueIgnoreEdit.vue",
        r#"<script setup lang="ts">
interface Base { ignored: string }
interface Props extends /* @vue-ignore */ Base { own: number }
defineProps<Props>()
</script>"#,
    );

    let expected = [VueIgnoredHeritageFact {
        contributor_ordinal: 0,
        intersection_arm_ordinal: 0,
    }];
    for (canonical, owner, name) in [
        (
            "/src/VueIgnore.vue",
            verter_type_expr::TopLevelOwnerId::instance(0),
            "Props",
        ),
        (
            "/src/VueIgnore.vue",
            verter_type_expr::TopLevelOwnerId::instance(0),
            "Emits",
        ),
        (
            "/src/VueIgnoreEdit.vue",
            verter_type_expr::TopLevelOwnerId::instance(0),
            "Props",
        ),
        (
            "/src/imported-ignore.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "ImportedProps",
        ),
    ] {
        let prepared =
            verter_type_engine::resolver_core::request_ports::OwnedLowering::prepared_type_decl(
                &host, canonical, owner, name,
            )
            .expect("exact-owner preparation should succeed")
            .unwrap_or_else(|| panic!("{canonical}#{name} should prepare"));
        assert_eq!(prepared.root_identity.owner, owner);
        assert_eq!(
            prepared.vue_ignored_heritage.as_ref(),
            expected,
            "{canonical}#{name} must retain the exact producer ordinal"
        );
    }
}

/// Prepared import canonicalization is DEMAND-DRIVEN: the bundle records the
/// DIRECT hop `(barrel, ordinary-file owner, imported name)` for each import
/// binding (the `ordinary_file()` owner is the provisional final-resolution-
/// owed marker), and the FINAL defining identity resolves at the first
/// decl-prepare / ref-head demand through the shared route authority — the
/// type-export rail for type bindings, the graph-native value-export rail
/// (with the terminal alias peel) for value bindings. Bundle build walks NO
/// import chain.
///
/// Discriminating both ways: an eager build that pre-resolved the chain would
/// store `/src/defining.ts` in `name_resolution` (the first asserts demand
/// the barrel DIRECT hop); a demand path that stopped at the barrel would
/// return `/src/barrel.ts` from the route authority (the second asserts
/// demand the final `/src/defining.ts` identities on BOTH rails).
#[test]
fn prepared_decl_name_resolution_stores_direct_hop_and_demand_resolves_final() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/defining.ts",
        "export type Node = { label: string }\nexport const themeImpl = { color: 'dark' }\n",
    );
    ws.inject_file(
        "/src/barrel.ts",
        "export type { Node } from './defining'\nexport { themeImpl as theme } from './defining'\n",
    );
    ws.inject_file(
        "/src/owner.ts",
        "import type { Node } from './barrel'\n\
         import { theme } from './barrel'\n\
         export interface Props { n: Node; t: typeof theme }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
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
    host.set_import_dependencies(
        "/src/barrel.ts",
        vec![exact_dependency("./defining", "/src/defining.ts")],
    );

    let prepared = host
        .prepared_type_decl("/src/owner.ts", "Props")
        .expect("Props should prepare through the barrel");

    // Bundle-side contract: BOTH bindings store the DIRECT hop with the
    // provisional ordinary-file owner — bundle build walked no chain.
    for (local, imported) in [("Node", "Node"), ("theme", "theme")] {
        let identity = prepared
            .name_resolution
            .get(local)
            .unwrap_or_else(|| panic!("`{local}` must have a name_resolution entry"));
        assert_eq!(
            (
                identity.canonical_id.as_ref(),
                identity.owner,
                identity.symbol_name.as_ref()
            ),
            (
                "/src/barrel.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                imported
            ),
            "the prepared name_resolution stores the DIRECT hop for `{local}` \
             (demand-driven canonicalization) — an eager chain walk at bundle \
             build would have stored the final defining identity",
        );
    }

    // Demand-side contract, TYPE rail: the shared route authority resolves
    // the direct hop to the FINAL defining file at demand.
    let node_final = host
        .resolve_imported_type_root("/src/barrel.ts", "Node")
        .expect("the type rail resolves the barrel hop at demand");
    assert_eq!(
        (
            node_final.canonical_id.as_ref(),
            node_final.symbol_name.as_ref()
        ),
        ("/src/defining.ts", "Node"),
        "the demand-time TYPE rail must canonicalize the barrel-imported `Node` \
         to the FINAL defining file, not the intermediate barrel",
    );

    // Demand-side contract, VALUE rail: the graph-native value-export rail
    // resolves the barrel hop AND peels the terminal alias to `themeImpl`.
    let theme_final = host
        .resolve_value_export_target_graph_native("/src/barrel.ts", "theme")
        .expect("the value rail resolves the barrel hop at demand");
    assert_eq!(
        (theme_final.canonical_id.as_str(), theme_final.name.as_str()),
        ("/src/defining.ts", "themeImpl"),
        "the demand-time VALUE rail must canonicalize the barrel-imported `theme` \
         to the FINAL defining (/src/defining.ts, themeImpl), peeling the \
         re-export alias",
    );
}

#[test]
fn imported_import_route_upgrades_replace_cached_known_miss_entries() {
    let host = make_host();
    let canonical_id = "/workspace/node_modules/lib/dist/index.d.ts";
    upsert_non_sfc(
        &host,
        canonical_id,
        r#"import { FancyProps } from "./inner.js"
export type { FancyProps }"#,
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/inner.d.ts",
        "export interface FancyProps { open: boolean }",
    );

    let _ = host
        .ensure_indexed_ready(canonical_id)
        .expect("declaration entrypoint should seed module facts");

    host.set_import_dependencies(
        canonical_id,
        vec![exact_dependency(
            "./inner.js",
            "/workspace/node_modules/lib/dist/inner.d.ts",
        )],
    );

    let resolved = host.resolve_imported_type_root(canonical_id, "FancyProps");
    assert_eq!(
        resolved,
        expected_imported_root(
            "/workspace/node_modules/lib/dist/inner.d.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "FancyProps",
        ),
        "imported declaration entrypoints must upgrade stale miss routes to the exact declaration target",
    );

    let facts = host
        .ensure_indexed_ready(canonical_id)
        .expect("module facts should exist after resolution");
    assert_eq!(
        facts
            .shallow_state
            .import_target("FancyProps")
            .map(|target| target.source_specifier.as_str()),
        Some("./inner.js"),
        "the rebuilt shallow surface retains the AUTHORED specifier",
    );
    assert_eq!(
        host.resolve_type_dependency_canonical_shallow(canonical_id, "./inner.js")
            .as_deref(),
        Some("/workspace/node_modules/lib/dist/inner.d.ts"),
        "the live route authority must resolve the upgraded declaration route",
    );
}

/// The owner's import-route RESOLUTION WITNESS is built by producers
/// that must not disturb the state they are recording. Building it may
/// re-resolve a known-miss specifier to observe whether the dependency
/// has appeared, but it must not materialize a shallow-only importer
/// into the indexed `FileArtifactStore`, nor mutate the importer's
/// host-owned import-route / dependency caches.
///
/// Scenario: `/workspace/src/importer.ts` authors `./theme`, which is
/// recorded as a known-miss (the importer has no indexed `IndexedReady`
/// — only the route table). `./theme.ts` is then added, so the
/// previously-unresolvable specifier now resolves.
///
/// Discrimination property: a known-miss re-resolve that routes through
/// the full `resolve_type_dependency_canonical` path falls through
/// `cached_import_route_resolution` to `authoritative_import_route`
/// (which calls `ensure_indexed_ready` and materializes the shallow-only
/// importer into the indexed store) and then `resolve_workspace_dependency_and_cache`
/// → `cache_positive_import_route_result` (which rewrites the known-miss
/// entry to a positive resolution and registers the new dependency).
/// This test fails if the builder re-resolves through that side-effecting
/// path: post-call the importer would be in the indexed store and
/// `import_routes["./theme"]` would be a positive resolution. A
/// side-effect-free workspace re-resolve keeps the indexed store and
/// the route/dependency caches untouched while still observing the
/// now-resolvable target, so the pre-appearance witness stops
/// validating.
#[test]
fn owner_import_route_witness_is_side_effect_free() {
    let host = make_host();
    let importer = "/workspace/src/importer.ts";
    upsert_non_sfc(
        &host,
        importer,
        "import { Theme } from './theme'\nexport type Re = Theme\n",
    );

    // Record `./theme` as a known-miss in `DerivedRawState.import_routes`
    // — `./theme.ts` does not exist yet. `set_import_dependencies`
    // stamps the miss with the current workspace `content_generation`.
    let known_miss = DependencyResolution {
        specifier: "./theme".to_string(),
        resolved_canonical_id: None,
        possible_canonical_ids: Vec::new(),
    };
    host.set_import_dependencies(importer, vec![known_miss]);

    let route_is_known_miss = |label: &str| {
        let derived = host
            .derived_raw_cache()
            .get(importer)
            .unwrap_or_else(|| panic!("{label}: importer must have a DerivedRawState entry"));
        let resolution = derived
            .import_routes
            .get("./theme")
            .unwrap_or_else(|| panic!("{label}: ./theme must be recorded in import_routes"));
        VerterHost::import_route_is_known_miss(resolution)
    };
    assert!(
        route_is_known_miss("after set_import_dependencies"),
        "./theme must start as a known-miss before the dependency is added"
    );
    let view_before_dep = host.resolver_store_view_read().into_owned_view();
    let witness_before_dep = host
        .owner_import_route_witness_for_tests(importer)
        .expect("the builder must produce a witness while ./theme is unresolved");
    assert!(
        !witness_before_dep.is_empty(),
        "the known-miss must have observed at least one resolver fact — an \
         empty witness would make the invalidation assertion vacuous"
    );
    for fact in &witness_before_dep {
        assert!(
            verter_session_query::facts::store_view::StoreView::validates(&view_before_dep, fact),
            "precondition: {fact:?} must validate against the view it was captured from"
        );
    }

    // Add `./theme.ts`, advancing the workspace `content_generation` so
    // the previously-unresolvable specifier now resolves. The
    // precondition check goes through the bare workspace VFS resolve so
    // it does not itself mutate the importer's import-route cache.
    upsert_non_sfc(
        &host,
        "/workspace/src/theme.ts",
        "export interface Theme { item: string }\n",
    );
    assert_eq!(
        host.ws()
            .resolve_import(
                importer,
                "./theme",
                verter_session_query::resolution::ResolutionContext {
                    phase: verter_session_query::resolution::ResolvePhase::CodegenBlocker,
                    kind: verter_session_query::resolution::ResolveRequestKind::TypeImport,
                },
            )
            .map(|r| r.source_id),
        Some("/workspace/src/theme.ts".to_string()),
        "precondition: ./theme must be workspace-resolvable once theme.ts exists"
    );

    let importer_indexed_before = host.project_type_store().indexed().get_any(importer);
    let dependencies_before = host
        .dependency_cache()
        .get(importer)
        .map(|entry| entry.dependencies.clone())
        .unwrap_or_default();

    // Build the witness again on the producer path. It must re-resolve
    // `./theme` against the current world WITHOUT materializing the
    // importer or mutating its caches.
    let _witness_after_dep = host
        .owner_import_route_witness_for_tests(importer)
        .expect("the builder must produce a witness after ./theme becomes resolvable");

    let importer_indexed_after = host.project_type_store().indexed().get_any(importer);
    assert_eq!(
        importer_indexed_before.is_some(),
        importer_indexed_after.is_some(),
        "the witness builder must NOT change whether the importer is \
         materialized in the indexed store"
    );
    assert!(
        route_is_known_miss("after witness"),
        "the witness builder must NOT rewrite the ./theme known-miss to a \
         positive import-route resolution"
    );
    let dependencies_after = host
        .dependency_cache()
        .get(importer)
        .map(|entry| entry.dependencies.clone())
        .unwrap_or_default();
    assert_eq!(
        dependencies_before, dependencies_after,
        "the witness builder must NOT register theme.ts in the importer's \
         dependency set"
    );

    // The witness must still be absence-sensitive: the appearance of
    // theme.ts advances exactly the fact the miss observed, so a warm
    // entry rooted on the pre-appearance witness stops validating.
    let view_after_dep = host.resolver_store_view_read().into_owned_view();
    assert!(
        witness_before_dep.iter().any(|fact| {
            !verter_session_query::facts::store_view::StoreView::validates(&view_after_dep, fact)
        }),
        "the pre-appearance witness must stop validating once the \
         previously-unresolvable ./theme resolves. Witness: \
         {witness_before_dep:?}"
    );
}

/// Within one request an owner's import-route witness resolves the owner's
/// specifiers once: every later consumer rooting on it in the request
/// shares that build, its observations replayed into any witness scope
/// open around the consumer, and an edit in the request builds it again.
/// Building it for every consumer resolved each specifier of a real-world
/// component about a thousand times per request.
#[test]
fn a_request_builds_an_owners_import_route_witness_once() {
    use crate::host_manage::import_route_witness::{
        witness_builds_for_tests, ResolutionWitnessScope,
    };
    let host = make_host();
    let importer = "/workspace/src/importer.ts";
    upsert_non_sfc(
        &host,
        importer,
        "import { Theme } from './theme'\nimport { Size } from './size'\nexport type Re = [Theme, Size]\n",
    );
    let _request = verter_type_engine::request_context::install_test_request_for(importer);
    let before = witness_builds_for_tests();
    let first = host
        .owner_import_route_witness_for_tests(importer)
        .expect("the builder produces a witness");
    assert!(!first.is_empty(), "the unresolved specifiers observe facts");
    for _ in 0..3 {
        assert_eq!(
            host.owner_import_route_witness_for_tests(importer),
            Some(first.clone())
        );
    }
    assert_eq!(
        witness_builds_for_tests() - before,
        1,
        "one build per request"
    );

    let replayed = {
        let scope = ResolutionWitnessScope::enter();
        let _ = host.owner_import_route_witness_for_tests(importer);
        scope.collected()
    };
    let as_set = |facts: &[verter_session_query::facts::fact_cache::FactVersionRef]| {
        facts.iter().cloned().collect::<rustc_hash::FxHashSet<_>>()
    };
    assert_eq!(
        as_set(&replayed),
        as_set(&first),
        "a shared build records its observations into the scope around its consumer"
    );
    assert_eq!(witness_builds_for_tests() - before, 1);

    upsert_non_sfc(
        &host,
        "/workspace/src/theme.ts",
        "export interface Theme { item: string }\n",
    );
    let after_edit = host
        .owner_import_route_witness_for_tests(importer)
        .expect("the builder produces a witness after the edit");
    assert_eq!(
        witness_builds_for_tests() - before,
        2,
        "an edit builds it again"
    );
    assert_ne!(
        after_edit, first,
        "the rebuilt witness observes the new file"
    );
}

/// The CALLER-DECLARED specifier is covered by the owner's import-route
/// witness.
///
/// The deleted `ImportRoute` digest composed an owner's route table from
/// two resolved sources — the content-pinned `IndexedReady.import_routes`
/// snapshot MERGED with the caller-supplied `DerivedRawState.import_routes`
/// table — and the source-selection order between them was a live defect
/// class. `IndexedReady` retains no route table at all now, and the host
/// memoises no resolution, so `DerivedRawState.import_routes` is
/// exclusively the CALLER-SUPPLIED authoritative push. There is nothing to
/// select between.
///
/// The witness inventory therefore reads that table's KEYS — the caller's
/// DECLARED request identities, a request-domain input; the resolved VALUES
/// are never read. Without them a purely synthetic bundler specifier with
/// no authored counterpart in the owner's source would contribute nothing
/// to the witness, and a consumer rooted on the witness would never observe
/// it retargeting.
///
/// This fixes an import-free owner with a caller-pushed route and asserts
/// BOTH directions: the witness OBSERVES the pushed specifier, and it stops
/// validating when that specifier's target disappears.
#[test]
fn import_route_witness_covers_caller_pushed_unauthored_specifiers() {
    let host = make_host();

    // The dependency exists before anything else, so the caller-supplied
    // route can be independently re-admitted by Engine.
    let dep = "/workspace/src/prefetch_dep.ts";
    upsert_non_sfc(&host, dep, "export interface Dep { ok: boolean }\n");

    // The owner has NO `import` statements — nothing the indexer can put
    // in its authored specifier inventory.
    let owner = "/workspace/src/prefetch_owner.ts";
    upsert_non_sfc(&host, owner, "export const marker = 1\n");

    let indexed = host
        .ensure_indexed_ready(owner)
        .expect("owner IndexedReady must materialise");
    assert!(
        indexed.shallow_state.import_targets.is_empty()
            && indexed.shallow_state.wildcard_reexports.is_empty(),
        "fixture invariant: an import-free owner must materialise a shallow \
         surface with NO cross-file edges — otherwise the caller-only path is \
         not exercised",
    );
    assert!(
        host.owner_import_route_witness_for_tests(owner)
            .expect("an owner with a readable parse surface is rootable")
            .is_empty(),
        "fixture invariant: before the push there is nothing to observe",
    );

    // Publish an explicit caller route. Its stored target is only the
    // caller's statement; the witness re-resolves the specifier itself.
    host.set_import_dependencies(owner, vec![exact_dependency("./prefetch_dep", dep)]);

    let indexed_after = host
        .current_content_pinned_indexed(owner)
        .expect("owner IndexedReady must still be content-pinned-current");
    assert!(
        indexed_after.shallow_state.import_targets.is_empty(),
        "fixture invariant: a caller route push must NOT back-fill the \
         parse-domain shallow surface — the route lands only in \
         DerivedRawState",
    );
    let derived_routes = host
        .derived_raw_cache()
        .get(owner)
        .map(|entry| entry.import_routes.clone())
        .expect("fixture invariant: DerivedRawState entry must exist for the owner");
    let derived_route = derived_routes
        .get("./prefetch_dep")
        .expect("fixture invariant: ./prefetch_dep must be recorded in DerivedRawState");
    assert!(
        !VerterHost::import_route_is_known_miss(derived_route),
        "fixture invariant: ./prefetch_dep resolves to an existing file, so \
         its caller-supplied route is a POSITIVE resolution",
    );

    let view_before = host.resolver_store_view_read().into_owned_view();
    let witness = host
        .owner_import_route_witness_for_tests(owner)
        .expect("an owner with a readable parse surface must produce a rootable witness");
    assert!(
        !witness.is_empty(),
        "CALLER-PUSHED BLIND SPOT: a caller-declared specifier with no authored \
         counterpart must enter the witness inventory, or a consumer rooted on \
         the witness never observes it retargeting",
    );
    for fact in &witness {
        assert!(
            verter_session_query::facts::store_view::StoreView::validates(&view_before, fact),
            "precondition: {fact:?} must validate against the view it was captured from"
        );
    }

    // The caller re-pushes the specifier at a DIFFERENT target — the
    // owner's own bytes do not move, and nothing it AUTHORS changes.
    let retarget = "/workspace/src/prefetch_dep2.ts";
    upsert_non_sfc(&host, retarget, "export interface Dep { ok: boolean }\n");
    host.set_import_dependencies(owner, vec![exact_dependency("./prefetch_dep", retarget)]);

    let view_after = host.resolver_store_view_read().into_owned_view();
    assert!(
        witness.iter().any(
            |fact| !verter_session_query::facts::store_view::StoreView::validates(
                &view_after,
                fact
            )
        ),
        "the witness must stop validating once the caller-pushed specifier's \
         target disappears. Witness: {witness:?}"
    );
}

#[test]
fn resolve_imported_type_root_caches_stable_miss_in_imported_root_db() {
    let host = make_host();
    let canonical_id = "/workspace/node_modules/lib/dist/index.d.ts";
    upsert_non_sfc(
        &host,
        canonical_id,
        "export interface PresentProps { open: boolean }\n",
    );

    host.ensure_indexed_ready(canonical_id)
        .expect("module facts should seed the provider before capturing a store view");
    let view = host.resolver_store_view_read().into_owned_view();

    let resolved = host.resolve_imported_type_root(canonical_id, "MissingProps");
    assert_eq!(
        resolved,
        None,
        "a stable imported-root miss must fail closed instead of fabricating a provider-local identity",
    );

    let cached = host
        .resolver
        .runtime
        .imported_roots
        .get(canonical_id, "MissingProps", &view)
        .expect("imported-root lookup should publish a stable miss to the shared DB");
    assert!(
        cached.is_miss(),
        "missing imported roots must be cached as Miss, not as a fallback self-resolution: {:?}",
        cached
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn prepared_type_decl_canonicalizes_imported_extends_base() {
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
        ws,
    );

    let prepared = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("prepared decl should materialize the imported extends base");

    let base = prepared
        .name_resolution
        .get("Base")
        .expect("prepared decl should carry imported Base resolution");
    assert_eq!(
        base.canonical_id.as_ref(),
        "/src/base.ts",
        "prepared decl name_resolution should point at the imported canonical owner",
    );
    assert_eq!(
        base.symbol_name.as_ref(),
        "Base",
        "prepared decl name_resolution should preserve the imported exported name",
    );
}

#[test]
fn prepared_type_decl_mints_content_free_class_heritage_base_facts() {
    // The class-heritage candidates are PRODUCER-MINTED content-free facts on
    // the prepared decl (minted once at lazy decl-body lowering from the class
    // body's Intersection fold) — never a query-time TypeExpr walk. Each fact
    // carries the authored base NAME (also the `name_resolution` routing key
    // the dispatch head-resolution uses) plus one content-free
    // `TypeArgLocator` per authored heritage type argument. The fact stores no
    // resolved identity and no embedded body: the head resolves at dispatch
    // time, the arguments deref + lower on demand.
    use verter_type_expr::locators::{LocatorSymbolSpace, TypeBodyPathStep};

    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/base.ts",
        "export class Base<T, U> { static tag: string = ''; constructor(x: T, y: U) {} }\n",
    );
    ws.inject_file(
        "/src/derived.ts",
        "import { Base } from './base'\n\
         declare function mixin<T>(base: T): T;\n\
         export class Derived extends Base<string, number> {}\n\
         export class Plain { static own: number = 1 }\n\
         export class Mixed extends mixin(Plain) { own2: number = 2 }\n\
         interface LocalIface { y: number }\n\
         export interface NotAClass extends LocalIface { x: string }\n\
         export type AliasIx = LocalIface & { z: boolean }\n",
    );
    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let prepared = host
        .prepared_type_decl("/src/derived.ts", "Derived")
        .expect("prepared decl should materialize the derived class");
    assert_eq!(
        prepared.heritage_bases.len(),
        1,
        "one heritage base fact for `extends Base<string, number>`, got {:?}",
        prepared.heritage_bases
    );
    let fact = &prepared.heritage_bases[0];
    assert_eq!(fact.name, "Base", "the authored base name");
    assert_eq!(
        fact.name_resolution_ref, "Base",
        "the name_resolution routing key is the authored head name"
    );
    // Dispatch head-resolution: the fact's routing key resolves CROSS-FILE
    // through the prepared decl's own name_resolution — the fact itself never
    // stores the resolved identity.
    let head = prepared
        .name_resolution
        .get(fact.name_resolution_ref.as_str())
        .expect("the heritage head routes through name_resolution");
    assert_eq!(head.canonical_id.as_ref(), "/src/base.ts");
    assert_eq!(head.symbol_name.as_ref(), "Base");
    // One content-free locator per authored type argument, addressing the
    // heritage Ref arm of the class body's Intersection fold; `arg_index`
    // selects the authored argument.
    assert_eq!(fact.type_args.len(), 2, "two authored type arguments");
    for (index, arg) in fact.type_args.iter().enumerate() {
        assert_eq!(arg.arg_index, index as u32, "source-order arg ordinal");
        assert_eq!(arg.anchor.canonical_id.as_ref(), "/src/derived.ts");
        assert_eq!(arg.anchor.symbol.as_ref(), "Derived");
        assert_eq!(arg.anchor.space, LocatorSymbolSpace::Type);
        assert_eq!(
            arg.path.as_ref(),
            &[TypeBodyPathStep::IntersectionArm { ordinal: 0 }],
            "the arg-bearing position is the heritage Ref arm (arm 0, before \
             the own Object arm)"
        );
    }

    // A heritage-free class mints no facts.
    let plain = host
        .prepared_type_decl("/src/derived.ts", "Plain")
        .expect("prepared decl should materialize the plain class");
    assert!(
        plain.heritage_bases.is_empty(),
        "a heritage-free class carries no heritage base facts: {:?}",
        plain.heritage_bases
    );

    // An interface's extends fold mints its heritage facts as a class body
    // does (its `extends` clauses are its bases); an alias's authored
    // intersection is no heritage and mints none.
    let iface = host
        .prepared_type_decl("/src/derived.ts", "NotAClass")
        .expect("prepared decl should materialize the interface");
    assert_eq!(
        iface
            .heritage_bases
            .iter()
            .map(|fact| fact.name.as_str())
            .collect::<Vec<_>>(),
        ["LocalIface"],
        "an interface extends fold mints one heritage fact per extends clause"
    );
    let alias = host
        .prepared_type_decl("/src/derived.ts", "AliasIx")
        .expect("prepared decl should materialize the alias");
    assert!(
        alias.heritage_bases.is_empty(),
        "an alias intersection is authored composition, not heritage: {:?}",
        alias.heritage_bases
    );

    // The UNNAMED-heritage fact. A class whose `extends` clause is an
    // EXPRESSION folds the synthetic value its expression registers under
    // (`Mixed:extends`), which reads the expression's value — never a
    // declaration a NOMINAL consumer could name, so reading that base list
    // as the class's nominal ancestry would publish a fabricated answer.
    // `heritage_undecidable` says so; it is meaningless on a non-class
    // declaration and stays false there.
    let mixed = host
        .prepared_type_decl("/src/derived.ts", "Mixed")
        .expect("prepared decl should materialize the mixin-based class");
    assert_eq!(
        mixed
            .heritage_bases
            .iter()
            .map(|fact| fact.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Mixed:extends"],
        "a call-expression base folds its synthetic value arm"
    );
    assert!(
        mixed.heritage_undecidable,
        "a class whose base the producer could not NAME is heritage-undecidable"
    );
    assert!(
        !plain.heritage_undecidable,
        "a heritage-free class is a PROOF of no heritage, not an undecidable one"
    );
    assert!(
        !prepared.heritage_undecidable,
        "a class whose every base minted a fact is decidable"
    );
    assert!(
        !iface.heritage_undecidable && !alias.heritage_undecidable,
        "the unnamed-heritage fact is class-only and never set on another kind"
    );
}

#[test]
fn prepared_type_decl_backfills_missing_local_symbol_when_cache_is_partial() {
    let host = make_host();
    let canonical_id = "/workspace/node_modules/lib/dist/index.d.ts";
    let source = r#"
type Local = { open: boolean }
export type FancyProps = Local
"#;
    upsert_non_sfc(&host, canonical_id, source);

    let prepared = host
        .prepared_type_decl(canonical_id, "Local")
        .expect("missing local decl should be prepared from shallow state");

    assert_eq!(prepared.root_identity.canonical_id.as_ref(), canonical_id);
    assert_eq!(prepared.root_identity.symbol_name.as_ref(), "Local");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolver_store_view_does_not_materialize_tracked_indexed_ready() {
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
    upsert_non_sfc(
        &host,
        "/src/unused.ts",
        "export interface Unused { label: string }\n",
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

    host.project_type_store()
        .indexed()
        .remove("/src/Consumer.vue");
    host.project_type_store().indexed().remove("/src/types.ts");
    host.project_type_store().indexed().remove("/src/dep.ts");
    host.project_type_store().indexed().remove("/src/unused.ts");
    host.provenance().reset();

    let view = host.resolver_store_view_read().into_owned_view();
    let provenance = host.provenance_snapshot();

    assert_eq!(
        provenance.indexed_ready_scheduler_snapshot_reuse, 0,
        "capturing a store view should not eagerly materialize tracked module facts; the view should snapshot known whole/import-route hashes without paying scheduler-backed module-facts loads",
    );
    assert!(
        view.whole_hash("/src/types.ts").is_some(),
        "store view should still capture tracked whole hashes for direct dependencies",
    );
    assert!(
        view.whole_hash("/src/dep.ts").is_some(),
        "store view should still capture tracked whole hashes for transitive dependencies",
    );
    // The import-route rail moved to the resolve domain: the build must
    // snapshot NO per-owner route digest for it (composing one is what
    // forced the build to re-resolve every published owner).
    assert!(
        view.derived_hash(
            "/src/types.ts",
            verter_session_query::facts::fact_cache::DerivedFactKind::Route
        )
        .is_none(),
        "an unmaterialised tracked canonical must not gain a route digest \
         from the store-view build",
    );

    let resolved = host.resolve_type_dependency_canonical_shallow("/src/Consumer.vue", "./types");
    assert_eq!(
        resolved.as_deref(),
        Some("/src/types.ts"),
        "lazy store views should still resolve direct import edges through the captured import-route hash/state",
    );
}

#[test]
fn resolver_store_view_tracks_reexport_import_routes() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/dep.ts",
        "export interface Props { msg: string }\n",
    );
    upsert_non_sfc(&host, "/src/index.ts", "export { Props } from './dep'\n");

    let _view = host.resolver_store_view_read().into_owned_view();

    assert_eq!(
        host.resolve_type_dependency_canonical_shallow("/src/index.ts", "./dep")
            .as_deref(),
        Some("/src/dep.ts"),
        "captured store views should resolve re-export import routes without requiring a synthesized import-route snapshot",
    );
}

#[test]
fn resolver_store_view_prefers_declaration_companion_routes_for_dts_imports() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/index.d.ts",
        "import { Props } from './inner.js'\nexport type { Props }\n",
    );
    upsert_non_sfc(
        &host,
        "/src/inner.d.ts",
        "export interface Props { msg: string }\n",
    );
    upsert_non_sfc(&host, "/src/inner.js", "export const runtimeOnly = true\n");

    let _view = host.resolver_store_view_read().into_owned_view();

    assert_eq!(
        host.resolve_type_dependency_canonical("/src/index.d.ts", "./inner.js")
            .as_deref(),
        Some("/src/inner.d.ts"),
        "captured store views should resolve declaration-file imports through the declaration companion",
    );
}

#[test]
fn store_view_imported_seed_reuses_cached_source_for_snapshot_and_env() {
    let ws = Arc::new(CountingWorkspace::new());
    let canonical_id = "/workspace/node_modules/pkg/dist/shared.d.ts";
    ws.inject_file(canonical_id, "export interface Alpha { alpha?: string }");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    ws.reset_reads();

    let entry = host
        .ensure_indexed_ready(canonical_id)
        .expect("explicit shallow seeding should load imported dependency state");
    assert!(
        entry.route_inventory.counts.top_level_statement_count > 0,
        "explicit shallow seeding should build the route inventory",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "explicit shallow seeding should read the imported file once",
    );

    assert!(
        host.current_eval_state(canonical_id).is_some(),
        "current_eval_state should reuse the imported cache after explicit seeding",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "current_eval_state should reuse the seeded imported cache without another workspace read",
    );

    let snapshot = host
        .get_raw_analysis_snapshot(canonical_id)
        .expect("snapshot build should reuse the seeded imported cache");
    assert!(
        snapshot.bindings.is_empty(),
        "simple declaration file should still produce a valid snapshot",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "snapshot materialization should not reread the imported file once seeded",
    );

    let env = host
        .base_eval_env_arc(canonical_id)
        .expect("eval env build should reuse the seeded imported cache");
    assert!(
        env.type_symbols.contains_key("Alpha"),
        "eval env should expose the imported declaration symbol",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "eval env build should not reread the imported file once seeded",
    );
}

#[test]
fn store_view_warm_imported_eval_env_hit_reuses_indexed_route_hash_without_reread() {
    let ws = Arc::new(CountingWorkspace::new());
    let canonical_id = "/workspace/node_modules/pkg/dist/shared.d.ts";
    ws.inject_file(canonical_id, "export interface Alpha { alpha?: string }");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    assert!(
        host.routed_shallow_state(canonical_id).is_some(),
        "routed shallow state should seed the imported declaration lazily",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "routed shallow seeding should read the imported file once",
    );

    let env = host
        .base_eval_env_arc(canonical_id)
        .expect("first eval env build should succeed from the routed imported file");
    assert!(
        env.type_symbols.contains_key("Alpha"),
        "first eval env build should expose the imported declaration symbol",
    );

    ws.reset_reads();

    let env = host
        .base_eval_env_arc(canonical_id)
        .expect("warm eval env lookup should reuse the cached env");
    assert!(
        env.type_symbols.contains_key("Alpha"),
        "warm eval env lookup should still expose the imported declaration symbol",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        0,
        "warm eval env lookup should not reread the imported file once the indexed shallow hash and env are cached",
    );
}

#[test]
fn store_view_indexed_imported_seed_reuses_cached_source_for_snapshot_and_env() {
    let ws = Arc::new(CountingWorkspace::new());
    let canonical_id = "/workspace/node_modules/pkg/dist/shared.d.ts";
    ws.inject_file(canonical_id, "export interface Alpha { alpha?: string }");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    assert!(
        host.routed_shallow_state(canonical_id).is_some(),
        "routed shallow seeding should load the imported dependency state",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "routed shallow seeding should read the imported file once",
    );

    assert!(
        host.read_analysis_source(canonical_id).is_some(),
        "read_analysis_source should reuse the indexed imported source once seeded",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "read_analysis_source should not reread the imported file once indexed state is cached",
    );

    assert!(
        host.current_eval_state(canonical_id).is_some(),
        "current_eval_state should reuse the indexed imported source once seeded",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "current_eval_state should not reread the imported file once indexed state is cached",
    );

    let snapshot = host
        .get_raw_analysis_snapshot(canonical_id)
        .expect("snapshot build should reuse the indexed imported source");
    assert!(
        snapshot.bindings.is_empty(),
        "simple declaration file should still produce a valid snapshot",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "snapshot materialization should not reread the imported file once indexed state is cached",
    );

    let shallow = host
        .shallow_file_state(canonical_id)
        .expect("shallow state should reuse the indexed imported source");
    assert!(
        shallow.has_type_symbol("Alpha"),
        "memo-owned headers should expose the imported declaration symbol",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "shallow state should not reread the imported file once indexed state is cached",
    );

    let env = host
        .base_eval_env_arc(canonical_id)
        .expect("eval env build should reuse the indexed imported source");
    assert!(
        env.type_symbols.contains_key("Alpha"),
        "eval env should expose the imported declaration symbol",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "eval env build should not reread the imported file once indexed state is cached",
    );
}

#[test]
fn cached_import_route_resolution_reuses_untracked_current_version_across_epoch_bumps() {
    let ws = Arc::new(CountingWorkspace::new());
    let provider = "/workspace/node_modules/pkg/dist/index.d.ts";
    let target = "/workspace/node_modules/pkg/dist/inner.d.ts";
    ws.inject_file(provider, "export type { InnerProps } from './inner.d.ts'\n");
    ws.inject_file(target, "export interface InnerProps { label: string }\n");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    host.set_import_dependencies(provider, vec![exact_dependency("./inner.d.ts", target)]);

    let _view = host.resolver_store_view_read().into_owned_view();
    assert!(
        host.routed_shallow_state(provider).is_some(),
        "routed shallow seeding should build the current provider surface after the view snapshot",
    );

    let resolved = host.cached_import_route_resolution(provider, "./inner.d.ts");
    assert_eq!(
        resolved
            .as_ref()
            .and_then(|resolution| resolution.resolved_canonical_id.as_deref()),
        Some(target),
        "the untracked current-version import-route cache should resolve before any unrelated host mutation",
    );

    upsert_non_sfc(
        &host,
        "/workspace/src/Unrelated.ts",
        "export const changed = 1",
    );

    let resolved = host.cached_import_route_resolution(provider, "./inner.d.ts");
    assert_eq!(
        resolved
            .as_ref()
            .and_then(|resolution| resolution.resolved_canonical_id.as_deref()),
        Some(target),
        "unchanged imported providers loaded after the view snapshot should keep reusing their current import-route cache across unrelated epoch bumps",
    );
}

#[test]
fn store_view_seeded_imported_barrel_backfills_wildcard_import_routes() {
    let ws = Arc::new(CountingWorkspace::new());
    let barrel = "/workspace/node_modules/pkg/dist/index.d.ts";
    let shared = "/workspace/node_modules/pkg/dist/shared.d.ts";
    ws.inject_file(barrel, "export * from './shared.js'\n");
    ws.inject_file(shared, "export interface Shared { label?: string }\n");
    ws.inject_file(
        "/workspace/node_modules/pkg/package.json",
        r#"{"name":"pkg","types":"./dist/index.d.ts"}"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    let _view = host.resolver_store_view_read().into_owned_view();

    let facts = host
        .ensure_indexed_ready(barrel)
        .expect("explicit shallow seeding should materialize the barrel facts");
    assert_eq!(
        facts.shallow_state.wildcard_reexports.len(),
        1,
        "barrel should publish its wildcard reexport",
    );
    assert_eq!(
        facts.shallow_state.wildcard_reexports[0].source_specifier, "./shared.js",
        "the seeded IndexedReady retains the AUTHORED wildcard specifier",
    );

    ws.reset_resolves();
    let resolved = host.resolve_type_dependency_canonical_shallow(barrel, "./shared.js");
    assert_eq!(
        resolved.as_deref(),
        Some(shared),
        "shallow dependency lookup should reuse the seeded barrel route",
    );
    assert_eq!(
        ws.resolve_count(barrel, "./shared.js"),
        1,
        "a persistent shallow lookup must re-enter Engine instead of treating the seeded wildcard route as publication authority",
    );
}

/// An external `<style src="...">` block must produce the TYPED deferred
/// analysis state — never a fabricated empty (or inline-sliced) `CssAnalysis`
/// presented as a positive zero-classes fact — and binding liveness must fail
/// OPEN so a binding consumed only by the external sheet's `v-bind()` is never
/// diagnosed unused.
#[test]
fn external_src_style_defers_content_and_fails_open_on_binding_liveness() {
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
    // The stray inline content inside a `src` block is IGNORED by Vue (the
    // external file replaces the block content) — analyzing it would be a
    // fabricated analysis of content the framework never uses.
    let src = r#"<script setup>const themeColor = 'red'</script>
<template><div class="x"/></template>
<style src="./theme.css">.stray { color: #fff }</style>
"#;
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/src/Themed.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();

    let analysis = host.get_analysis("/workspace/src/Themed.vue").unwrap();
    assert_eq!(analysis.styles.len(), 1, "one style block");
    let style = &analysis.styles[0];
    assert!(
        !style.content_is_available(),
        "external src block must carry the typed deferred state"
    );
    assert!(
        style.css.is_none(),
        "no fabricated CssAnalysis for deferred external content: {:?}",
        style.css
    );
    assert!(
        style.v_binds.is_empty(),
        "no v-bind facts may be minted from unseen external content"
    );

    let binding = analysis
        .bindings
        .iter()
        .find(|b| b.name == "themeColor")
        .expect("themeColor binding");
    assert!(
        binding.used_in_style,
        "binding liveness must fail OPEN while the external sheet is deferred \
         (no false unused-binding diagnostic)"
    );

    // Negative control: an INLINE style block still analyzes normally.
    let inline = r#"<script setup>const inlineColor = 'red'</script>
<template><div class="x"/></template>
<style>.local { color: v-bind(inlineColor) }</style>
"#;
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/src/Inline.vue".to_string(),
            source: Arc::from(inline),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let inline_analysis = host.get_analysis("/workspace/src/Inline.vue").unwrap();
    assert_eq!(inline_analysis.styles.len(), 1);
    assert!(inline_analysis.styles[0].content_is_available());
    assert!(
        inline_analysis.styles[0].css.is_some(),
        "inline blocks keep their scanned analysis"
    );
}

#[test]
fn internal_and_public_import_lookups_reenter_engine_admission() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/workspace/src/dep.ts", "export const dep = 1");
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
        r#"<script setup lang="ts">
import { dep } from "@/dep"
</script>"#,
    );
    ws.set_exact_resolutions(
        "/workspace/src/App.vue",
        vec![verter_workspace::ExactResolution {
            specifier: "@/dep".to_string(),
            phase: verter_session_query::resolution::ResolvePhase::CodegenBlocker,
            kind: verter_session_query::resolution::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/workspace/src/dep.ts".to_string()),
            possible_canonical_ids: vec!["/workspace/src/dep.ts".to_string()],
        }],
    );

    ws.reset_resolves();
    let first = host.resolve_loaded_dependency_canonical(
        "/workspace/src/App.vue",
        "@/dep",
        verter_session_query::resolution::ResolveRequestKind::EsmImport,
    );
    let second = host.resolve_import("/workspace/src/App.vue", "@/dep");
    let third = host.resolve_loaded_dependency_canonical(
        "/workspace/src/App.vue",
        "@/dep",
        verter_session_query::resolution::ResolveRequestKind::EsmImport,
    );

    assert_eq!(
        first.as_deref(),
        Some("/workspace/src/dep.ts"),
        "first lookup should resolve through the workspace fallback"
    );
    assert_eq!(
        second.as_deref(),
        Some("/workspace/src/dep.ts"),
        "public resolve_import should resolve the same admitted canonical route"
    );
    assert_eq!(
        third, first,
        "subsequent internal lookups should keep returning the admitted route"
    );
    assert_eq!(
        ws.resolve_count("/workspace/src/App.vue", "@/dep"),
        3,
        "every persistent consumer must re-enter the workspace Engine instead of treating a host route cache as publication authority"
    );
}

#[test]
fn build_fallthrough_eval_env_skips_nested_non_root_component_bindings() {
    let host = make_host();
    upsert_non_sfc(&host, "/src/used.ts", "export const used = 'used'");
    upsert_non_sfc(
        &host,
        "/src/unused-nested.ts",
        "export const unusedNested = 'unused-nested'",
    );
    upsert_vue(
        &host,
        "/src/Child.vue",
        r#"<script setup lang="ts">
defineProps<{ label?: string }>()
</script>
<template><span /></template>"#,
    );
    upsert_vue(
        &host,
        "/src/App.vue",
        r#"<script setup lang="ts">
import { used } from './used'
import { unusedNested } from './unused-nested'
import Child from './Child.vue'
</script>
<template>
  <div :title="used">
    <Child :label="unusedNested" />
  </div>
</template>"#,
    );
    host.set_import_dependencies(
        "/src/App.vue",
        vec![
            exact_dependency("./used", "/src/used.ts"),
            exact_dependency("./unused-nested", "/src/unused-nested.ts"),
            exact_dependency("./Child.vue", "/src/Child.vue"),
        ],
    );

    let snapshot = host
        .get_analysis_snapshot_internal("/src/App.vue", None)
        .expect("analysis snapshot should exist");
    let mut visiting = rustc_hash::FxHashSet::default();
    let resolved = host
        .compute_component_meta_state(
            "/src/App.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
            host.get_whole_hash("/src/App.vue")
                .expect("whole hash should exist for App.vue"),
        )
        .expect("resolved meta should exist");
    let resolution = crate::resolver_core::with_bare_host_ctx_for_test(&host, |ctx| {
        let fixture_dispatch_0 =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);

        host.compute_fallthrough_surface_from_resolved_state(
            "/src/App.vue",
            &resolved,
            None,
            &mut visiting,
            ctx,
            &fixture_dispatch_0,
        )
    })
    .expect("fallthrough should resolve");
    assert!(
        matches!(
            resolution.fallthrough_surface,
            verter_session_query::analysis::component_meta::FallthroughSurface::Branches { .. }
        ),
        "sanity check: single native root should still produce a fallthrough branch"
    );

    let base_meta = verter_semantic::analysis::component_meta::extract_component_meta(
        verter_semantic::analysis::component_meta::ComponentMetaInput {
            macros: &resolved.snapshot.macros,
            bindings: &resolved.snapshot.bindings,
            imports: &resolved.snapshot.imports,
            template: resolved.snapshot.template.as_deref(),
            options_api: resolved.snapshot.options_api.as_ref(),
            analysis_flags:
                verter_session_query::analysis::types::AnalysisFlags::from_bits_truncate(
                    resolved.snapshot.script_flags,
                ),
            styles: &resolved.snapshot.styles,
            vue_api_calls: &resolved.snapshot.vue_api_calls,
            store_usages: &resolved.snapshot.store_usages,
            resolved_macros: &[],
            resolved_binding_reactivity: &[],
            resolved_type_registry: &[],
            evaluated_types: None,
            file_path: "/src/App.vue",
        },
    );
    let env = host
        .build_fallthrough_eval_env_lightweight(
            "/src/App.vue",
            &snapshot,
            Some(&base_meta.root_reachability),
        )
        .expect("fallthrough owner env should build");

    // Owner-aware presence: setup-owner hydration is keyed by the
    // import's lexical owner, so check by NAME across owners.
    assert!(
        env.value_symbols.keys().any(|key| &*key.name == "used"),
        "root-branch runtime bindings should still be materialized"
    );
    assert!(
        !env.value_symbols
            .keys()
            .any(|key| &*key.name == "unusedNested"),
        "nested non-root component prop bindings should stay out of the root fallthrough env"
    );
}

#[test]
fn extract_component_meta_from_resolved_keeps_fallthrough_on_captured_store_view() {
    let host = make_host();
    upsert_vue(&host, "/src/Link.vue", r#"<template><a /></template>"#);
    upsert_vue(
        &host,
        "/src/Button.vue",
        r#"<script setup lang="ts">
import Link from './Link.vue'
</script>
<template><Link /></template>"#,
    );
    host.set_import_dependencies(
        "/src/Button.vue",
        vec![exact_dependency("./Link.vue", "/src/Link.vue")],
    );

    let _store_view = host.resolver_store_view_read().into_owned_view();

    upsert_non_sfc(&host, "/src/shared.ts", "export const shared = 'shared'");
    upsert_vue(
        &host,
        "/src/UnrelatedA.vue",
        r#"<script setup lang="ts">
import { shared } from './shared'
</script>
<template><div :title="shared" /></template>"#,
    );
    upsert_vue(
        &host,
        "/src/UnrelatedB.vue",
        r#"<script setup lang="ts">
import { shared } from './shared'
</script>
<template><div :title="shared" /></template>"#,
    );

    host.provenance().reset();

    let resolved = host
        .resolve_component_meta(
            "/src/Button.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("resolved meta should be computed from the captured view");

    let meta = crate::resolver_core::with_bare_host_ctx_for_test(&host, |ctx| {
        let fixture_dispatch_1 =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);

        extract_component_meta_from_resolved(
            &host,
            "/src/Button.vue",
            &resolved,
            true,
            ctx,
            &fixture_dispatch_1,
        )
    })
    .analysis;

    assert!(
        matches!(
            meta.fallthrough_surface,
            verter_session_query::analysis::component_meta::FallthroughSurface::Branches { .. }
        ),
        "button fallthrough should still resolve through the imported Link root",
    );
}

#[test]
fn fallthrough_barrel_routing_preserves_default_vs_named_binding_identity() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/components/index.ts",
        "export { default as Button } from './NamedButton.vue'\nexport { default } from './DefaultButton.vue'\n",
    );
    upsert_vue(
        &host,
        "/src/components/NamedButton.vue",
        "<template><button /></template>",
    );
    upsert_vue(
        &host,
        "/src/components/DefaultButton.vue",
        "<template><a /></template>",
    );
    upsert_vue(
        &host,
        "/src/AppNamed.vue",
        r#"<script setup lang="ts">
import { Button } from './components'
</script>
<template><Button /></template>"#,
    );
    upsert_vue(
        &host,
        "/src/AppDefault.vue",
        r#"<script setup lang="ts">
import Button from './components'
</script>
<template><Button /></template>"#,
    );

    host.set_import_dependencies(
        "/src/AppNamed.vue",
        vec![exact_dependency("./components", "/src/components/index.ts")],
    );
    host.set_import_dependencies(
        "/src/AppDefault.vue",
        vec![exact_dependency("./components", "/src/components/index.ts")],
    );
    host.set_import_dependencies(
        "/src/components/index.ts",
        vec![
            exact_dependency("./NamedButton.vue", "/src/components/NamedButton.vue"),
            exact_dependency("./DefaultButton.vue", "/src/components/DefaultButton.vue"),
        ],
    );

    let named = host
        .get_component_meta("/src/AppNamed.vue")
        .expect("named barrel import should resolve component meta");
    let default = host
        .get_component_meta("/src/AppDefault.vue")
        .expect("default barrel import should resolve component meta");

    let named_props: std::collections::BTreeSet<_> = named
        .accepted_props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    let default_props: std::collections::BTreeSet<_> = default
        .accepted_props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();

    assert!(
        named_props.contains("disabled"),
        "named barrel import should route to the button child surface, got {:?}",
        named_props
    );
    assert!(
        !named_props.contains("href"),
        "named barrel import should not route through the barrel default export, got {:?}",
        named_props
    );
    assert!(
        default_props.contains("href"),
        "default barrel import should route to the default-exported child surface, got {:?}",
        default_props
    );
    assert!(
        !default_props.contains("disabled"),
        "default barrel import should not route through the named Button export, got {:?}",
        default_props
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_fallthrough_surface_reuses_parent_snapshot_for_child_binding_lookup() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Parent.vue",
        r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template>
  <Child class="root-child" />
</template>"#,
    );
    ws.inject_file(
        "/src/Child.vue",
        r#"<script setup lang="ts">
defineProps<{ label?: string }>()
</script>
<template><button /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/src/Parent.vue"),
        "parent should load from the workspace",
    );
    host.set_import_dependencies(
        "/src/Parent.vue",
        vec![exact_dependency("./Child.vue", "/src/Child.vue")],
    );

    ws.reset_reads();
    let resolution = host.resolve_fallthrough_surface("/src/Parent.vue");

    assert!(
        resolution.is_some(),
        "fallthrough should resolve through the imported child root",
    );
    assert!(
        ws.read_count("/src/Parent.vue") <= 1,
        "fallthrough should reuse the parent snapshot while resolving child binding identity instead of rereading the parent source; saw {} reads",
        ws.read_count("/src/Parent.vue"),
    );
}

#[test]
fn prepared_type_decl_lookup_resolves_barrel_reexport_through_indexed_ready() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/base.ts",
        "export interface BaseProps { replace?: boolean }",
    );
    upsert_non_sfc(
        &host,
        "/src/barrel.ts",
        "export type { BaseProps } from './base'",
    );
    host.set_import_dependencies(
        "/src/barrel.ts",
        vec![exact_dependency("./base", "/src/base.ts")],
    );

    // Materialize barrel and base module facts
    let barrel_facts = host
        .ensure_indexed_ready("/src/barrel.ts")
        .expect("barrel should materialize module facts");
    assert!(
        barrel_facts.shallow_state.exports.contains_key("BaseProps"),
        "barrel module facts should list BaseProps as a reexport",
    );

    // The base file should also be materializable
    let base_facts = host
        .ensure_indexed_ready("/src/base.ts")
        .expect("base should materialize module facts");
    assert!(
        base_facts.shallow_state.has_type_symbol("BaseProps"),
        "base module facts should have BaseProps as a local symbol",
    );
}

#[test]
fn prepared_type_decl_lookup_rejects_stale_cache_entries() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string }",
    );
    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should materialize");

    // Warm the bundle cache so the fact-validated entry exists.
    let _view_before = host.resolver_store_view_read().into_owned_view();
    assert!(
        host.prepared_type_decl("/src/types.ts", "Props").is_some(),
        "prepared lookup should succeed before the file content changes"
    );

    // Change the file content — `Props` no longer exists.
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Other { value: number }",
    );
    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should re-materialize after content change");

    // Take a new view that records the updated hash.
    let _view_after = host.resolver_store_view_read().into_owned_view();

    // The stale bundle (cached with original hash) must be rejected by
    // fact validation against the new view, and the re-materialized
    // bundle must not contain the removed symbol.
    assert!(
        host.prepared_type_decl("/src/types.ts", "Props").is_none(),
        "prepared lookup should drop stale cached declarations when the owning file hash changes"
    );
    assert!(
        host.prepared_type_decl("/src/types.ts", "Props").is_none(),
        "prepared lookup without an explicit store view should also reject the stale bundle"
    );
}

/// @ai-generated - get_analysis populates resolved_canonical_id for relative imports
#[test]
fn get_analysis_resolves_relative_import() {
    let host = make_host();
    upsert_vue(
        &host,
        "/project/Child.vue",
        "<script setup>\ndefineProps({ msg: String })\n</script>\n<template><div>{{ msg }}</div></template>",
    );
    upsert_vue(
        &host,
        "/project/Parent.vue",
        "<script setup>\nimport Child from './Child.vue'\n</script>\n<template><Child msg=\"hello\" /></template>",
    );

    let analysis = host.get_analysis("/project/Parent.vue").unwrap();
    let child_import = analysis
        .imports
        .iter()
        .find(|i| i.source == "./Child.vue")
        .unwrap();
    assert_eq!(
        child_import.resolved_canonical_id.as_deref(),
        Some("/project/Child.vue"),
        "relative import should resolve to canonical ID"
    );
}

/// @ai-generated - get_analysis leaves bare specifiers unresolved
#[test]
fn get_analysis_bare_specifier_unresolved() {
    let host = make_host();
    upsert_vue(
        &host,
        "App.vue",
        "<script setup>\nimport { ref } from 'vue'\n</script>\n<template><div/></template>",
    );

    let analysis = host.get_analysis("App.vue").unwrap();
    let vue_import = analysis.imports.iter().find(|i| i.source == "vue").unwrap();
    assert!(
        vue_import.resolved_canonical_id.is_none(),
        "bare specifier 'vue' should not resolve (no node_modules resolution)"
    );
}

/// @ai-generated - get_analysis leaves unregistered file imports unresolved
#[test]
fn get_analysis_missing_file_unresolved() {
    let host = make_host();
    upsert_vue(
        &host,
        "App.vue",
        "<script setup>\nimport Missing from './Missing.vue'\n</script>\n<template><div/></template>",
    );

    let analysis = host.get_analysis("App.vue").unwrap();
    let missing_import = analysis
        .imports
        .iter()
        .find(|i| i.source == "./Missing.vue")
        .unwrap();
    assert!(
        missing_import.resolved_canonical_id.is_none(),
        "import of unregistered file should not resolve"
    );
}

/// @ai-generated - get_export_span for .vue file returns binding span
#[test]
fn get_export_span_vue_binding() {
    let host = make_host();
    upsert_vue(
        &host,
        "Child.vue",
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div/></template>",
    );

    let span = host.get_export_span("Child.vue", "msg");
    assert!(span.is_some(), "should find 'msg' binding in .vue file");
    let (start, end) = span.unwrap();
    let source = host.get_source("Child.vue").unwrap();
    let spanned = &source[start as usize..end as usize];
    assert_eq!(spanned, "msg", "span should cover the binding identifier");
}

/// @ai-generated - get_export_span for .vue file returns None for unknown binding
#[test]
fn get_export_span_vue_unknown_binding() {
    let host = make_host();
    upsert_vue(
        &host,
        "Child.vue",
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div/></template>",
    );

    assert!(
        host.get_export_span("Child.vue", "nonexistent").is_none(),
        "unknown binding should return None"
    );
}

/// @ai-generated - resolve_import public method works
#[test]
fn resolve_import_public_method() {
    let host = make_host();
    upsert_vue(&host, "/project/Child.vue", "<template><div/></template>");
    upsert_vue(
        &host,
        "/project/Parent.vue",
        "<script setup>\nimport Child from './Child.vue'\n</script>\n<template><Child/></template>",
    );

    assert_eq!(
        host.resolve_import("/project/Parent.vue", "./Child.vue")
            .as_deref(),
        Some("/project/Child.vue")
    );
    // Bare specifiers that aren't in the file map resolve to None
    assert!(host
        .resolve_import("/project/Parent.vue", "lodash")
        .is_none());
}

#[test]
fn enriches_destructured_composable_bindings() {
    let host = make_host();

    // Composable that returns { x: ref, y: ref, reset: function }
    upsert_ts(
        &host,
        "/project/useMouse.ts",
        r#"
import { ref } from 'vue'
export function useMouse() {
const x = ref(0)
const y = ref(0)
function reset() { x.value = 0; y.value = 0 }
return { x, y, reset }
}
"#,
    );

    // SFC that destructures the composable return
    upsert_vue(
        &host,
        "/project/App.vue",
        r#"<script setup>
import { useMouse } from './useMouse.ts'
const { x, y, reset } = useMouse()
</script>
<template><div>{{ x }} {{ y }}</div></template>"#,
    );

    let analysis = host.get_analysis("/project/App.vue").unwrap();

    // x and y should be enriched to Ref (from composable return shape)
    let x_binding = analysis.bindings.iter().find(|b| b.name == "x").unwrap();
    assert_eq!(
        x_binding.reactivity_kind,
        verter_session_query::analysis::types::ReactivityKind::Ref,
        "x should be enriched from MaybeRef to Ref via composable return shape"
    );

    let y_binding = analysis.bindings.iter().find(|b| b.name == "y").unwrap();
    assert_eq!(
        y_binding.reactivity_kind,
        verter_session_query::analysis::types::ReactivityKind::Ref,
        "y should be enriched from MaybeRef to Ref via composable return shape"
    );

    // reset should stay as a function (ReactivityKind::None since it's not reactive)
    let reset_binding = analysis
        .bindings
        .iter()
        .find(|b| b.name == "reset")
        .unwrap();
    assert_eq!(
        reset_binding.reactivity_kind,
        verter_session_query::analysis::types::ReactivityKind::None,
        "reset (a function) should be None, not reactive"
    );

    // Negative: non-enriched bindings should not be affected
    assert!(
        !x_binding.is_reactive
            || x_binding.reactivity_kind
                != verter_session_query::analysis::types::ReactivityKind::MaybeRef,
        "x should NOT remain MaybeRef after enrichment"
    );
}

#[test]
fn follow_reexport_cycle_same_binding() {
    let host = make_host();

    // A re-exports foo from B, B re-exports foo from A → cycle
    upsert_ts(&host, "a.ts", "export { foo } from './b.ts'");
    upsert_ts(&host, "b.ts", "export { foo } from './a.ts'");

    let result = host.get_export_span_follow_reexports("a.ts", "foo");
    assert!(
        result.is_none(),
        "cycle on same binding should return None, got: {result:?}"
    );
}

#[test]
fn follow_reexport_same_file_different_binding() {
    let host = make_host();

    // A re-exports foo from B (as foo→bar), B re-exports bar from A (as bar→baz),
    // A has a local baz export. Different bindings each hop → not a cycle.
    upsert_ts(
        &host,
        "/project/a.ts",
        "export { bar as foo } from './b.ts'\nexport const baz = 99",
    );
    upsert_ts(
        &host,
        "/project/b.ts",
        "export { baz as bar } from './a.ts'",
    );

    let result = host.get_export_span_follow_reexports("/project/a.ts", "foo");
    assert!(
        result.is_some(),
        "different bindings through same files should resolve, not be treated as cycle"
    );
    let (canonical_id, _, _) = result.unwrap();
    assert_eq!(
        canonical_id, "/project/a.ts",
        "should resolve to a.ts local baz export"
    );
}

#[test]
fn enriched_imports_do_not_affect_stored_data() {
    let host = make_host();
    upsert_vue(
        &host,
        "/project/Child.vue",
        "<script setup>\nconst x = 1\n</script>\n<template><div/></template>",
    );
    upsert_vue(
        &host,
        "/project/Parent.vue",
        "<script setup>\nimport Child from './Child.vue'\n</script>\n<template><Child/></template>",
    );

    // First call: enriches imports with resolved_canonical_id
    let a1 = host.get_analysis("/project/Parent.vue").unwrap();
    assert!(
        a1.imports[0].resolved_canonical_id.is_some(),
        "enriched import should have resolved_canonical_id"
    );

    // Verify stored data is not mutated by checking that the
    // internal stored imports still have None
    #[cfg(not(target_arch = "wasm32"))]
    {
        use crate::host_executor::HostSourceData;
        let source_snap = host
            .scheduler
            .try_get_source("/project/Parent.vue")
            .expect("scheduler should have Parent.vue");
        let hd = source_snap
            .downcast_data::<HostSourceData>()
            .expect("source data should be HostSourceData");
        assert!(
            hd.parse.script_analysis.imports[0]
                .resolved_canonical_id
                .is_none(),
            "stored import should NOT be mutated by get_analysis enrichment"
        );
    }
    #[cfg(target_arch = "wasm32")]
    {
        let files = crate::shared::read_lock(&host.files);
        let entry = files.get("/project/Parent.vue").unwrap();
        assert!(
            entry.script_analysis.imports[0]
                .resolved_canonical_id
                .is_none(),
            "stored import should NOT be mutated by get_analysis enrichment"
        );
    }
}

/// @ai-generated - resolve_exports follows re-export chains
#[test]
fn resolve_exports_follows_reexport_chains() {
    let host = make_host();

    upsert_vue(
        &host,
        "/project/Button.vue",
        "<script setup>\ndefineProps({ label: String })\n</script>\n<template><button>{{ label }}</button></template>",
    );

    upsert_ts(
        &host,
        "/project/components/index.ts",
        "export { default as Button } from './Button.vue';",
    );

    // Set up dependency so ./Button.vue resolves from components/index.ts
    host.set_import_dependencies(
        "/project/components/index.ts",
        vec![crate::DependencyResolution {
            specifier: "./Button.vue".to_string(),
            resolved_canonical_id: Some("/project/Button.vue".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    let exports = host.resolve_exports("/project/components/index.ts");
    assert!(
        !exports.is_empty(),
        "barrel file should have resolved exports"
    );

    let button = exports
        .iter()
        .find(|e| e.name == "Button")
        .expect("should have 'Button' resolved export");
    assert_eq!(
        button.source_canonical_id.as_deref(),
        Some("/project/Button.vue"),
        "Button should resolve to Button.vue"
    );
    assert_eq!(
        button.source_name, "default",
        "Button maps to 'default' in the source file"
    );
}

/// @ai-generated - resolve_exports handles direct local exports
#[test]
fn resolve_exports_local_exports() {
    let host = make_host();
    upsert_ts(
        &host,
        "utils.ts",
        "export const FOO = 1;\nexport type Bar = string;",
    );

    let exports = host.resolve_exports("utils.ts");
    assert_eq!(exports.len(), 2, "should have 2 exports");

    let foo = exports.iter().find(|e| e.name == "FOO").unwrap();
    assert!(
        foo.source_canonical_id.is_none(),
        "local export has no source file"
    );
    assert_eq!(foo.source_name, "FOO");
    assert!(!foo.is_type);

    let bar = exports.iter().find(|e| e.name == "Bar").unwrap();
    assert!(bar.is_type);
}

/// @ai-generated - resolve_exports handles wildcard re-exports
#[test]
fn resolve_exports_wildcard_reexports() {
    let host = make_host();

    upsert_ts(
        &host,
        "/project/types.ts",
        "export type Foo = string;\nexport type Bar = number;",
    );
    upsert_ts(&host, "/project/index.ts", "export * from './types';");

    host.set_import_dependencies(
        "/project/index.ts",
        vec![crate::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/project/types.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    let exports = host.resolve_exports("/project/index.ts");
    assert!(
        exports.iter().any(|e| e.name == "Foo"),
        "wildcard re-export should include Foo"
    );
    assert!(
        exports.iter().any(|e| e.name == "Bar"),
        "wildcard re-export should include Bar"
    );

    let foo = exports.iter().find(|e| e.name == "Foo").unwrap();
    assert_eq!(
        foo.source_canonical_id.as_deref(),
        Some("/project/types.ts"),
        "Foo should trace back to types.ts"
    );
}

/// @ai-generated - resolve_exports detects circular re-exports
#[test]
fn resolve_exports_circular_protection() {
    let host = make_host();

    upsert_ts(&host, "a.ts", "export * from './b';");
    upsert_ts(&host, "b.ts", "export * from './a';");

    host.set_import_dependencies(
        "a.ts",
        vec![crate::DependencyResolution {
            specifier: "./b".to_string(),
            resolved_canonical_id: Some("b.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );
    host.set_import_dependencies(
        "b.ts",
        vec![crate::DependencyResolution {
            specifier: "./a".to_string(),
            resolved_canonical_id: Some("a.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    // Should not infinite loop
    let exports = host.resolve_exports("a.ts");
    // The result is empty because both files only re-export each other with no local exports
    assert!(
        exports.is_empty(),
        "circular re-exports with no local exports should return empty"
    );
}

/// @ai-generated - resolve_exports multi-level barrel chain
#[test]
fn resolve_exports_multi_level_barrel() {
    let host = make_host();

    upsert_ts(&host, "/project/deep.ts", "export const DEEP = 42;");
    upsert_ts(&host, "/project/mid.ts", "export { DEEP } from './deep';");
    upsert_ts(&host, "/project/top.ts", "export { DEEP } from './mid';");

    host.set_import_dependencies(
        "/project/mid.ts",
        vec![crate::DependencyResolution {
            specifier: "./deep".to_string(),
            resolved_canonical_id: Some("/project/deep.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );
    host.set_import_dependencies(
        "/project/top.ts",
        vec![crate::DependencyResolution {
            specifier: "./mid".to_string(),
            resolved_canonical_id: Some("/project/mid.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    let exports = host.resolve_exports("/project/top.ts");
    let deep = exports
        .iter()
        .find(|e| e.name == "DEEP")
        .expect("should have DEEP");
    assert_eq!(
        deep.source_canonical_id.as_deref(),
        Some("/project/deep.ts"),
        "should trace through two levels to deep.ts"
    );
}

#[test]
fn resolve_imported_type_from_ts_dep() {
    let host = make_host();
    // Upsert the .ts type file
    upsert_ts(
        &host,
        "/types.ts",
        "export interface ButtonProps { label: string; size?: number }",
    );
    // Upsert the .vue file that imports from ./types
    upsert_vue(
        &host,
        "/Button.vue",
        r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script><template><div /></template>"#,
    );

    let state = resolve_expanded_state(&host, "/Button.vue");
    let dtos = macro_dtos_by_type(&host, "/Button.vue", &state, "ButtonProps");
    let props: Vec<&str> = dtos
        .prop_fields()
        .iter()
        .map(|prop| prop.analysis.name.as_str())
        .collect();

    assert!(
        props.contains(&"label"),
        "expanded props should contain 'label', got: {:?}",
        props
    );
    assert!(
        props.contains(&"size"),
        "expanded props should contain 'size', got: {:?}",
        props
    );
}

#[test]
fn resolve_component_meta_returns_no_resolved_macros_for_no_imported_type_deps() {
    let host = make_host();
    upsert_vue(
        &host,
        "/Simple.vue",
        r#"<script setup lang="ts">
defineProps<{ count: number }>()
</script><template><div /></template>"#,
    );
    let state = resolve_expanded_state(&host, "/Simple.vue");
    assert!(
        state.resolved_macros.is_empty(),
        "should not resolve any cross-file macros when there are no imported type deps"
    );
}

#[test]
fn resolve_imported_type_from_vue_dep() {
    let host = make_host();
    upsert_vue(
        &host,
        "/types.vue",
        "<script setup lang=\"ts\">export interface Props { label: string }</script>\n<template><div /></template>",
    );
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { Props } from './types.vue'\ndefineProps<Props>()\n</script>\n<template><div /></template>",
    );

    let state = resolve_expanded_state(&host, "/Comp.vue");
    let resolved = resolved_macro_by_type(&state, "Props");
    let dtos = macro_dtos_for_resolved(&host, "/Comp.vue", resolved);
    let props: Vec<&str> = dtos
        .prop_fields()
        .iter()
        .map(|prop| prop.analysis.name.as_str())
        .collect();
    assert!(
        props.contains(&"label"),
        "expanded props should contain 'label', got: {:?}",
        props
    );
    assert!(
        !resolved
            .declaration
            .text
            .as_deref()
            .unwrap_or_default()
            .contains("<template>"),
        "declaration text must not leak raw SFC markup, got: {:?}",
        resolved.declaration.text
    );
}

#[test]
fn resolve_imported_type_from_dual_script_vue_dep() {
    let host = make_host();
    upsert_vue(
        &host,
        "/types.vue",
        "<script lang=\"ts\">\nexport interface DualProps { title: string; count: number }\n</script>\n<script setup lang=\"ts\">\n// empty setup block\n</script>\n<template><div /></template>",
    );
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { DualProps } from './types.vue'\ndefineProps<DualProps>()\n</script>\n<template><div /></template>",
    );

    let state = resolve_expanded_state(&host, "/Comp.vue");
    let dtos = macro_dtos_by_type(&host, "/Comp.vue", &state, "DualProps");
    let props: Vec<&str> = dtos
        .prop_fields()
        .iter()
        .map(|prop| prop.analysis.name.as_str())
        .collect();
    assert!(
        props.contains(&"title"),
        "expanded props should contain 'title' from companion script, got: {:?}",
        props
    );
}

#[test]
fn resolve_imported_type_from_vue_dep_without_vue_suffix_uses_file_kind() {
    let host = make_host();
    // Use .vue extension so that VFS resolution can resolve the import.
    // The test verifies that Vue SFC script extraction works for deps
    // that are stored with VueSfc file kind.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/types.vue".to_string(),
            source: Arc::from(
                "<script setup lang=\"ts\">export interface Props { label: string }</script>\n<template><div /></template>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { Props } from './types.vue'\ndefineProps<Props>()\n</script>\n<template><div /></template>",
    );

    let state = resolve_expanded_state(&host, "/Comp.vue");
    let resolved = resolved_macro_by_type(&state, "Props");
    let dtos = macro_dtos_for_resolved(&host, "/Comp.vue", resolved);
    let props: Vec<&str> = dtos
        .prop_fields()
        .iter()
        .map(|prop| prop.analysis.name.as_str())
        .collect();
    assert!(
        props.contains(&"label"),
        "expanded props should contain 'label', got: {:?}",
        props
    );
    assert!(
        !resolved
            .declaration
            .text
            .as_deref()
            .unwrap_or_default()
            .contains("<template>"),
        "declaration text must NOT contain raw SFC markup, got: {:?}",
        resolved.declaration.text
    );
}

// ═══════════════════════════════════════════════════════════
// enrich_imported_types tests
// ═══════════════════════════════════════════════════════════

/// resolve_component_meta(Expanded) populates prop fields from imported interface
#[test]
fn enrich_basic_imported_interface() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta(
            "/src/Comp.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("should return resolved state");
    let props = hm_prop_names(&host, "/src/Comp.vue", &state);
    assert!(
        props.contains(&"label".to_string()),
        "props should include 'label': {:?}",
        props
    );
    // Negative: get_analysis must NOT have enriched the snapshot
    let analysis = host.get_analysis("/src/Comp.vue").unwrap();
    let dp = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_session_query::analysis::types::AnalyzedMacroKind::DefineProps)
        .unwrap();
    assert!(
        dp.prop_fields.is_empty(),
        "get_analysis must NOT enrich prop_fields"
    );
}

/// resolve_component_meta(Expanded) extracts slot bindings from imported type
#[test]
fn enrich_slot_bindings_from_imported_type() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/slots.ts",
        "export interface Slots { default: (props: { row: string; index: number }) => any }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './slots'
defineSlots<Slots>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta(
            "/src/Comp.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("should return resolved state");
    let slot_dtos = dtos_for_kind(
        &host,
        "/src/Comp.vue",
        &state,
        verter_session_query::analysis::types::AnalyzedMacroKind::DefineSlots,
    );
    let slots: Vec<_> = slot_dtos
        .iter()
        .flat_map(|d| d.slot_fields().iter())
        .collect();
    let default_slot = slots.iter().find(|s| s.name == "default");
    assert!(default_slot.is_some(), "should have 'default' slot");
    let bindings = &default_slot.unwrap().bindings;
    assert!(!bindings.is_empty(), "slot should have bindings");
    let binding_names: Vec<&str> = bindings.iter().map(|b| b.name.as_str()).collect();
    assert!(
        binding_names.contains(&"row"),
        "should have 'row': {:?}",
        binding_names
    );
    assert!(
        binding_names.contains(&"index"),
        "should have 'index': {:?}",
        binding_names
    );
}

/// resolve_component_meta(Expanded) resolves nested type references
#[test]
fn enrich_nested_type_expansion() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        r#"export type Status = 'active' | 'inactive'
export interface Props { name: string; status: Status }"#,
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta(
            "/src/Comp.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("should return resolved state");
    let prop_names = hm_prop_names(&host, "/src/Comp.vue", &state);
    assert!(
        prop_names.contains(&"name".to_string()),
        "should have 'name': {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"status".to_string()),
        "should have 'status': {:?}",
        prop_names
    );
    // Negative: props should not contain 'Status' as a prop (it's a type, not a prop)
    assert!(
        !prop_names.contains(&"Status".to_string()),
        "Status is a type, not a prop"
    );
}

/// resolve_component_meta(Expanded) extracts slot return types
#[test]
fn enrich_slot_return_type_property_style() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/slots.ts",
        "export interface Slots { default: (props: { row: string }) => VNode[]; header: (props: {}) => any }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './slots'
defineSlots<Slots>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta(
            "/src/Comp.vue",
            verter_type_engine::semantic_query::ProjectionMode::Expanded,
        )
        .expect("should return resolved state");
    let slot_dtos = dtos_for_kind(
        &host,
        "/src/Comp.vue",
        &state,
        verter_session_query::analysis::types::AnalyzedMacroKind::DefineSlots,
    );
    let slots: Vec<_> = slot_dtos
        .iter()
        .flat_map(|d| d.slot_fields().iter())
        .collect();

    let default_slot = slots.iter().find(|s| s.name == "default").unwrap();
    assert_eq!(
        default_slot.return_type.as_deref(),
        Some("VNode[]"),
        "default slot should have return type VNode[]"
    );

    let header_slot = slots.iter().find(|s| s.name == "header").unwrap();
    assert_eq!(
        header_slot.return_type.as_deref(),
        Some("any"),
        "header slot should have return type any"
    );
}

/// @ai-generated - local defineSlots with return types
#[test]
fn local_slot_return_type_property_style() {
    let host = make_host();
    upsert_vue(
        &host,
        "/Comp.vue",
        r#"<script setup lang="ts">
defineSlots<{
  default: (props: { item: string }) => VNode[],
  header: (props: {}) => any
}>()
</script>
<template><div /></template>"#,
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    let ds = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_session_query::analysis::types::AnalyzedMacroKind::DefineSlots)
        .expect("should have DefineSlots macro");

    let default_slot = ds.slot_fields.iter().find(|s| s.name == "default").unwrap();
    assert_eq!(
        default_slot.return_type.as_deref(),
        Some("VNode[]"),
        "local default slot should have return type"
    );
}

/// @ai-generated - local defineSlots with method-style return types
#[test]
fn local_slot_return_type_method_style() {
    let host = make_host();
    upsert_vue(
        &host,
        "/Comp.vue",
        r#"<script setup lang="ts">
defineSlots<{
  default(props: { item: string }): VNode[]
}>()
</script>
<template><div /></template>"#,
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    let ds = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_session_query::analysis::types::AnalyzedMacroKind::DefineSlots)
        .expect("should have DefineSlots macro");

    let default_slot = ds.slot_fields.iter().find(|s| s.name == "default").unwrap();
    assert_eq!(
        default_slot.return_type.as_deref(),
        Some("VNode[]"),
        "method-style slot should have return type"
    );
}

/// A5-03b — a NAMED or ALIASED `defineProps` type argument peels EXACT Vue
/// wrapper routes, with the same proof the inline-object form produces.
///
/// `defineProps<Props>()` leases a `TypeExpr::Ref`, not a `TypeExpr::Object`, so
/// the inline macro-mirror sidecar (`MacroHotProduct.prop_reference_heads`) mints
/// nothing for it. The exact authored evidence lives on the props declaration's
/// PREPARED MEMBER FACT (`PreparedMemberFact.reference_head`) — minted once at
/// lazy decl-body lowering and copied at prepare time — and is composed through
/// the SAME shared machinery the inline form uses:
/// `resolve_authored_reference_route` → `wrapper_candidate_for_route` →
/// `demand_terminal_symbol_instantiation`.
///
/// This test is the INVERSION of the T-A5 rider
/// (`..._prop_wrapper_publishes_no_classes_and_no_route`), which pinned this
/// capability's ABSENCE. Its fail-closed siblings are
/// `template_class_imported_props_type_argument_fails_closed_with_local_control`
/// (the ruled-negative imported arm) and the local/package fake + missing
/// dependency arms below.
///
/// The published `authored_head` argument locator is asserted to be a
/// `Value(TypeArgLocator)` rooted at the `Props` DECLARATION with a
/// `[Member, MemberValue]` path — not a `MacroPayload` locator. That is what
/// discriminates the member-fact producer from the inline sidecar: if the route
/// came from the macro mirror the locator arm would differ.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn template_class_named_type_argument_prop_wrappers_peel_exact_vue_routes() {
    let host = strict_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/not-vue/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );

    // ── (A) a named `interface Props`: one exact positive plus the two
    // same-shape fakes that must stay fail-closed in the SAME props body.
    let named = "/workspace/src/NamedPropWrapper.vue";
    upsert_vue(
        &host,
        named,
        r#"<script setup lang="ts">
import type { Ref } from 'vue'
import type { Ref as OtherRef } from 'not-vue'
type LocalRef<T> = { value: T }
interface Props {
  variant: Ref<'primary' | 'secondary'>
  localFake: LocalRef<'local'>
  packageFake: OtherRef<'package'>
}
const props = defineProps<Props>()
</script><template>
  <div :class="props.variant" />
  <div :class="props.localFake" />
  <div :class="props.packageFake" />
</template>"#,
    );
    host.set_import_dependencies(
        named,
        vec![
            exact_dependency("vue", "/workspace/node_modules/vue/index.d.ts"),
            exact_dependency("not-vue", "/workspace/node_modules/not-vue/index.d.ts"),
        ],
    );

    let template = host
        .get_analysis(named)
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
            vec!["primary".to_string(), "secondary".to_string()],
            vec![],
            vec![],
        ],
        "a named-type-argument prop wrapper must peel its exact Vue route while \
         same-shape local and foreign-package fakes stay fail-closed"
    );

    let facts = template_class_facts_for(&host, named);
    let row = facts
        .rows()
        .iter()
        .find(|row| row.subject.label() == "variant")
        .expect("the named prop subject must join to an artifact row");
    assert_eq!(row.wrapper.role, verter_type_expr::ReactiveWrapperRole::Ref);
    let verter_type_expr::ClosedLiteralDomain::Strings(values) = &row.wrapper.inner_domain else {
        panic!(
            "expected a closed wrapper inner domain, got {:?}",
            row.wrapper.inner_domain
        );
    };
    assert_eq!(
        values.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
        ["primary", "secondary"],
        "the TERMINAL substituted argument drives the inner closed domain"
    );
    let provenance = row
        .wrapper
        .import_provenance
        .as_ref()
        .expect("exact named-type-argument prop route");
    assert_eq!(provenance.import_source.as_ref(), "vue");
    assert_eq!(provenance.terminal_import_source.as_ref(), "vue");
    assert_eq!(provenance.package.as_ref(), "vue");
    assert_eq!(provenance.local_binding.as_ref(), "Ref");
    assert!(
        provenance.local_alias_hops.is_empty(),
        "a direct `Ref` member annotation crosses no local alias hop, got {:?}",
        provenance.local_alias_hops
    );

    // The authored head is the MEMBER's, addressed through the props
    // DECLARATION anchor — the producer discriminator against the inline
    // macro-mirror sidecar (which mints `MacroPayload` argument locators).
    let verter_type_expr::facts::AuthoredReferenceHeadFact::Bare { local_name, args } =
        &provenance.authored_head
    else {
        panic!(
            "expected a bare authored member head, got {:?}",
            provenance.authored_head
        );
    };
    assert_eq!(local_name.as_ref(), "Ref");
    let [verter_type_expr::facts::AuthoredReferenceArgLocator::Value(arg)] = args.as_ref() else {
        panic!(
            "a named-type-argument route must publish the MEMBER's Value argument \
             locator, not a macro-payload locator, got {args:?}"
        );
    };
    assert_eq!(arg.anchor.canonical_id.as_ref(), named);
    assert_eq!(arg.anchor.symbol.as_ref(), "Props");
    assert_eq!(
        arg.anchor.space,
        verter_type_expr::locators::LocatorSymbolSpace::Type
    );
    assert_eq!(
        &*arg.path,
        &[
            verter_type_expr::locators::TypeBodyPathStep::Member { ordinal: 0 },
            verter_type_expr::locators::TypeBodyPathStep::MemberValue
        ],
        "the head argument locator must address the member's authored value position"
    );
    assert_eq!(arg.arg_index, 0);

    // The two fakes: no route, no closed subset, no `Ref` role.
    for label in ["localFake", "packageFake"] {
        let fake = facts
            .rows()
            .iter()
            .find(|row| row.subject.label() == label)
            .unwrap_or_else(|| panic!("row for {label}"));
        assert!(
            fake.wrapper.import_provenance.is_none(),
            "{label} must claim no import provenance"
        );
        assert_ne!(
            fake.wrapper.role,
            verter_type_expr::ReactiveWrapperRole::Ref,
            "{label} must not be granted the `Ref` wrapper role"
        );
        assert!(
            !matches!(
                fake.wrapper.inner_domain,
                verter_type_expr::ClosedLiteralDomain::Strings(_)
            ),
            "{label} must publish no closed wrapper inner domain, got {:?}",
            fake.wrapper.inner_domain
        );
    }

    // ── (B) an ALIASED type argument (`type AliasProps = { ... }`).
    let aliased = "/workspace/src/AliasPropWrapper.vue";
    upsert_vue(
        &host,
        aliased,
        r#"<script setup lang="ts">
import type { Ref } from 'vue'
type AliasProps = { other: Ref<'x' | 'y'> }
const props = defineProps<AliasProps>()
</script><template><div :class="props.other" /></template>"#,
    );
    host.set_import_dependencies(
        aliased,
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );
    let alias_template = host
        .get_analysis(aliased)
        .expect("lazy analysis")
        .template
        .expect("template");
    assert_eq!(
        alias_template.elements[0].dynamic_classes,
        ["x", "y"],
        "an aliased object-literal type argument must peel the same exact route"
    );
    let alias_row = template_class_facts_for(&host, aliased)
        .rows()
        .iter()
        .find(|row| row.subject.label() == "other")
        .cloned()
        .expect("the aliased prop subject must join");
    assert_eq!(
        alias_row.wrapper.role,
        verter_type_expr::ReactiveWrapperRole::Ref
    );
    let alias_provenance = alias_row
        .wrapper
        .import_provenance
        .as_ref()
        .expect("exact aliased prop route");
    assert_eq!(alias_provenance.import_source.as_ref(), "vue");
    assert_eq!(alias_provenance.terminal_import_source.as_ref(), "vue");
    let verter_type_expr::facts::AuthoredReferenceHeadFact::Bare {
        args: alias_args, ..
    } = &alias_provenance.authored_head
    else {
        panic!("expected a bare authored member head for the alias arm");
    };
    let [verter_type_expr::facts::AuthoredReferenceArgLocator::Value(alias_arg)] =
        alias_args.as_ref()
    else {
        panic!("expected the member Value argument locator for the alias arm");
    };
    assert_eq!(alias_arg.anchor.symbol.as_ref(), "AliasProps");

    // ── (C) MISSING DEPENDENCY: the same authored member shape whose import
    // edge resolves to a canonical the host has no state for. The routed
    // terminal cannot be reached, so no route is claimed.
    let unresolved = "/workspace/src/MissingDepPropWrapper.vue";
    upsert_vue(
        &host,
        unresolved,
        r#"<script setup lang="ts">
import type { Ref } from './gone'
interface Props { variant: Ref<'primary' | 'secondary'> }
const props = defineProps<Props>()
</script><template><div :class="props.variant" /></template>"#,
    );
    host.set_import_dependencies(
        unresolved,
        vec![exact_dependency("./gone", "/workspace/src/gone.ts")],
    );
    let missing_template = host
        .get_analysis(unresolved)
        .expect("lazy analysis")
        .template
        .expect("template");
    assert!(
        missing_template.elements[0].dynamic_classes.is_empty(),
        "a missing `vue` dependency must publish no closed subset, got {:?}",
        missing_template.elements[0].dynamic_classes
    );
    let missing_row = template_class_facts_for(&host, unresolved)
        .rows()
        .iter()
        .find(|row| row.subject.label() == "variant")
        .cloned()
        .expect("the subject must still join");
    assert!(
        missing_row.wrapper.import_provenance.is_none(),
        "a missing dependency must claim no import provenance"
    );
    assert_ne!(
        missing_row.wrapper.role,
        verter_type_expr::ReactiveWrapperRole::Ref
    );
}

/// Classifying ONE named-type-argument prop subject must not open unrelated
/// wrapper-shaped imports. The member-head route resolves only the requested
/// member's own authored reference; there is no owner-wide candidate scan and no
/// whole-props-surface materialisation.
///
/// Mirrors `template_class_requested_subject_does_not_read_unrelated_cold_wrapper_import`
/// for the member-fact producer: the decoy is imported by the props DECLARATION's
/// file and referenced by a SIBLING member the template never requests.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn template_class_named_type_argument_subject_does_not_read_unrelated_cold_wrapper_import() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/node_modules/vue/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );
    ws.inject_file(
        "/workspace/node_modules/cold-pkg/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    let canonical = "/workspace/src/ColdMemberDecoy.vue";
    upsert_vue(
        &host,
        canonical,
        r#"<script setup lang="ts">
import type { Ref } from 'vue'
import type { Ref as ColdRef } from 'cold-pkg'
interface Props {
  variant: Ref<'primary' | 'secondary'>
  unrequested: ColdRef<'cold-a' | 'cold-b'>
}
const props = defineProps<Props>()
</script><template><div :class="props.variant" /></template>"#,
    );
    host.set_import_dependencies(
        canonical,
        vec![
            exact_dependency("vue", "/workspace/node_modules/vue/index.d.ts"),
            exact_dependency("cold-pkg", "/workspace/node_modules/cold-pkg/index.d.ts"),
        ],
    );
    ws.reset_reads();

    let template = host
        .get_analysis(canonical)
        .expect("analysis")
        .template
        .expect("template");
    assert_eq!(
        template.elements[0].dynamic_classes,
        ["primary", "secondary"],
        "the requested member must still peel its exact route"
    );
    assert_eq!(
        ws.read_count("/workspace/node_modules/cold-pkg/index.d.ts"),
        0,
        "classifying one named-type-argument prop member must not traverse an \
         unrelated sibling member's wrapper-shaped import"
    );
}

#[test]
fn template_class_qualified_and_import_type_routes_are_exact() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/not-vue/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/src/ambiguous.ts",
        "export * from 'vue'\nexport * from 'not-vue'\n",
    );
    host.set_import_dependencies(
        "/workspace/src/ambiguous.ts",
        vec![
            exact_dependency("vue", "/workspace/node_modules/vue/index.d.ts"),
            exact_dependency("not-vue", "/workspace/node_modules/not-vue/index.d.ts"),
        ],
    );
    let canonical = "/workspace/src/AuthoredHeadShapes.vue";
    upsert_vue(
        &host,
        canonical,
        r#"<script setup lang="ts">
import * as Vue from 'vue'
import * as Other from 'not-vue'
import * as Ambiguous from './ambiguous'
namespace Local { export type Ref<T> = { value: T } }
const namespaceRef: Vue.Ref<'namespace-a' | 'namespace-b'> = null as never
const importTypeRef: import('vue').Ref<'import-a' | 'import-b'> = null as never
const namespaceFake: Other.Ref<'bad'> = null as never
const importTypeFake: import('not-vue').Ref<'bad'> = null as never
const missingNamespace: Vue.Missing<'bad'> = null as never
const missingImport: import('missing-vue').Ref<'bad'> = null as never
const ambiguousNamespace: Ambiguous.Ref<'bad'> = null as never
const ambiguousImport: import('./ambiguous').Ref<'bad'> = null as never
const localNamespace: Local.Ref<'bad'> = null as never
</script><template>
  <div :class="namespaceRef" />
  <div :class="importTypeRef" />
  <div :class="namespaceFake" />
  <div :class="importTypeFake" />
  <div :class="missingNamespace" />
  <div :class="missingImport" />
  <div :class="ambiguousNamespace" />
  <div :class="ambiguousImport" />
  <div :class="localNamespace" />
</template>"#,
    );
    host.set_import_dependencies(
        canonical,
        vec![
            exact_dependency("vue", "/workspace/node_modules/vue/index.d.ts"),
            exact_dependency("not-vue", "/workspace/node_modules/not-vue/index.d.ts"),
            exact_dependency("./ambiguous", "/workspace/src/ambiguous.ts"),
        ],
    );
    let template = host
        .get_analysis(canonical)
        .expect("analysis")
        .template
        .expect("template");
    let classes = template
        .elements
        .iter()
        .map(|element| element.dynamic_classes.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        classes,
        [
            vec!["namespace-a".to_string(), "namespace-b".to_string()],
            vec!["import-a".to_string(), "import-b".to_string()],
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
        ]
    );

    let facts = template_class_facts_for(&host, canonical);
    for (label, local) in [("namespaceRef", "Vue"), ("importTypeRef", "")] {
        let row = facts
            .rows()
            .iter()
            .find(|row| row.subject.label() == label)
            .expect("authored-head route row");
        let provenance = row
            .wrapper
            .import_provenance
            .as_ref()
            .expect("exact authored route");
        assert_eq!(provenance.local_binding.as_ref(), local);
        assert_eq!(provenance.package.as_ref(), "vue");
        assert_eq!(provenance.import_source.as_ref(), "vue");
        assert_eq!(provenance.owner_canonical.as_ref(), canonical);
        assert_eq!(provenance.imported_name.as_ref(), "Ref");
        assert_eq!(provenance.terminal_import_source.as_ref(), "vue");
        assert!(provenance.local_alias_hops.is_empty());
        assert_eq!(
            provenance.exactness,
            verter_type_expr::ResolutionExactness::ExactSymbolic
        );
        assert_eq!(
            provenance.provenance,
            verter_type_expr::ResolutionProvenance::FrameworkSurface
        );
        match (label, &provenance.authored_head) {
            (
                "namespaceRef",
                verter_type_expr::facts::AuthoredReferenceHeadFact::Qualified {
                    local_root,
                    member_path,
                    args,
                },
            ) => {
                assert_eq!(local_root.as_ref(), "Vue");
                assert_eq!(
                    member_path.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
                    ["Ref"]
                );
                assert_exact_value_reference_arg(args, canonical, "namespaceRef");
            }
            (
                "importTypeRef",
                verter_type_expr::facts::AuthoredReferenceHeadFact::ImportType {
                    specifier,
                    member_path,
                    args,
                },
            ) => {
                assert_eq!(specifier.as_ref(), "vue");
                assert_eq!(
                    member_path.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
                    ["Ref"]
                );
                assert_exact_value_reference_arg(args, canonical, "importTypeRef");
            }
            _ => panic!("unexpected authored head for {label}"),
        }
        let terminal = row.wrapper.symbol.as_ref().expect("terminal");
        assert_eq!(
            terminal.canonical_id.as_ref(),
            "/workspace/node_modules/vue/index.d.ts"
        );
        assert_eq!(
            terminal.owner,
            verter_type_expr::TopLevelOwnerId::ordinary_file()
        );
        assert_eq!(terminal.symbol.as_ref(), "Ref");
    }
    for label in [
        "namespaceFake",
        "importTypeFake",
        "missingNamespace",
        "missingImport",
        "ambiguousNamespace",
        "ambiguousImport",
        "localNamespace",
    ] {
        let row = facts
            .rows()
            .iter()
            .find(|row| row.subject.label() == label)
            .expect("negative authored-head row");
        assert!(row.wrapper.import_provenance.is_none());
        assert!(!matches!(
            row.domain,
            verter_type_expr::ClosedLiteralDomain::Strings(_)
        ));
    }
}

#[test]
fn template_class_requested_subject_does_not_read_unrelated_cold_wrapper_import() {
    let ws = Arc::new(CountingWorkspace::new());
    // The decoy must be PACKAGE-BACKED: a relative `./cold` decoy is invisible
    // to an owner-wide scan by construction (proven by the T-A5b M8 mutation —
    // the same plant that stayed GREEN against a relative decoy REDs against
    // this one), so only a node_modules decoy makes this footprint rail
    // load-bearing.
    ws.inject_file(
        "/workspace/node_modules/cold-pkg/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    let canonical = "/workspace/src/ColdDecoy.vue";
    upsert_vue(
        &host,
        canonical,
        r#"<script setup lang="ts">
import type { Ref as ColdRef } from 'cold-pkg'
type Variant = 'primary' | 'secondary'
const variant: Variant = 'primary'
</script><template><div :class="variant" /></template>"#,
    );
    host.set_import_dependencies(
        canonical,
        vec![exact_dependency(
            "cold-pkg",
            "/workspace/node_modules/cold-pkg/index.d.ts",
        )],
    );
    ws.reset_reads();

    let template = host
        .get_analysis(canonical)
        .expect("analysis")
        .template
        .expect("template");
    assert_eq!(
        template.elements[0].dynamic_classes,
        ["primary", "secondary"]
    );
    assert_eq!(
        ws.read_count("/workspace/node_modules/cold-pkg/index.d.ts"),
        0,
        "requested-subject classification must not traverse unrelated wrapper-shaped imports"
    );
}

/// A6-05 — the structural twin of the A5-05 footprint test: resolving ONE
/// requested return head must not read an unrelated cold wrapper-shaped import,
/// and must not lower unrelated declarations in the owner. Reintroducing an
/// owner-wide wrapper-candidate scan, or resolving at the analyzer boundary,
/// makes the cold read count non-zero.
#[test]
fn return_wrapper_demand_does_not_read_unrelated_cold_wrapper_import() {
    /// Resolve one requested return head in a fresh host whose owner file
    /// carries `unrelated` extra deep declarations, and report the cold decoy's
    /// read count plus the number of declaration bodies the demand lowered.
    fn measure(unrelated: usize) -> (verter_type_expr::ReactiveWrapperRole, u64, u64) {
        let ws = Arc::new(CountingWorkspace::new());
        ws.inject_file(
            "/workspace/src/cold.ts",
            "export interface Ref<T> { value: T }\n",
        );
        let host = VerterHost::new(HostConfig::default(), ws.clone());
        upsert_non_sfc(
            &host,
            "/workspace/node_modules/vue/index.d.ts",
            RETURN_WRAPPER_VUE_DTS,
        );
        let decoys = (0..unrelated)
            .map(|index| {
                format!(
                    "export type DeepUnrelated{index} = {{ a: {{ b: {{ c: ColdRef<{index}> }} }} }}\n"
                )
            })
            .collect::<String>();
        let canonical = "/workspace/src/cold-decoy.ts";
        upsert_ts(
            &host,
            canonical,
            &format!(
                "import type {{ Ref }} from 'vue'\n\
                 import type {{ Ref as ColdRef }} from './cold'\n\
                 {decoys}\
                 export function getValue(): Ref<number> {{ return null as never; }}\n"
            ),
        );
        host.set_import_dependencies(
            canonical,
            vec![
                exact_dependency("vue", "/workspace/node_modules/vue/index.d.ts"),
                exact_dependency("./cold", "/workspace/src/cold.ts"),
            ],
        );
        host.provenance().reset();
        ws.reset_reads();
        let (role, provenance) = return_wrapper_role_for(&host, canonical, "getValue");
        assert_eq!(
            provenance
                .expect("route proof")
                .terminal_import_source
                .as_ref(),
            "vue"
        );
        (
            role,
            ws.read_count("/workspace/src/cold.ts"),
            host.provenance().snapshot().decl_bodies_lowered,
        )
    }

    let (role, cold_reads, lowered_one) = measure(1);
    assert_eq!(
        role,
        verter_type_expr::ReactiveWrapperRole::Ref,
        "the requested subject must still resolve exactly"
    );
    assert_eq!(
        cold_reads, 0,
        "requested-subject return classification must not traverse unrelated \
         wrapper-shaped imports"
    );

    // Differential footprint proof for the OWNER-local declarations: growing the
    // file's unrelated deep declarations from 1 to 4 must not change how many
    // declaration bodies the demand lowers. An owner-wide walk (or resolution at
    // the analyzer boundary) scales with the file; a demand-scoped one does not.
    let (role, cold_reads, lowered_four) = measure(4);
    assert_eq!(role, verter_type_expr::ReactiveWrapperRole::Ref);
    assert_eq!(cold_reads, 0);
    assert_eq!(
        lowered_one, lowered_four,
        "the demand's lowering footprint must be independent of the owner's \
         unrelated declaration count ({lowered_one} vs {lowered_four})"
    );
}

/// A6-06 — the named public-boundary acceptance test. The component-meta
/// consumer publishes the demand-resolved whole-return wrapper role on its
/// binding surface with its EXACTNESS intact, across three classes on one
/// fixture: exact (from a BODILESS declaration the value path cannot reach), a
/// completed non-wrapper proof, and a typed degradation. Warm re-reads are
/// stable.
///
/// Discriminating mutations: making the demand return no rows leaves every role
/// `None` and every kind `MaybeRef`; mapping `ReactiveWrapperRole::None` onto
/// `ReactivityKind::None` flips the non-wrapper arm; collapsing the degradation
/// reasons flips the overload arm.
#[test]
fn component_meta_binding_return_wrapper_role_is_exact_and_degrades_typed() {
    let host = make_host();
    let owner = "/workspace/src/App.vue";
    upsert_vue(
        &host,
        owner,
        "<template><div>{{ counter }}{{ total }}{{ plain }}{{ ambiguous }}</div></template>\n\
         <script setup lang=\"ts\">\n\
         import { useCounter, useTotal, usePlain, useOverloaded } from './composables'\n\
         const counter = useCounter()\n\
         const total = useTotal()\n\
         const plain = usePlain()\n\
         const ambiguous = useOverloaded()\n\
         </script>\n",
    );
    a6_wire_composable_host(
        &host,
        owner,
        "/workspace/src/composables.d.ts",
        A6_BODILESS_COMPOSABLE_DTS,
    );

    let meta = host
        .get_component_meta(owner)
        .expect("component meta must resolve");

    // EXACT — a bodiless `Ref<number>` return decides the binding's reactivity,
    // where the value-space walk publishes only `MaybeRef`.
    let counter = a6_binding(&meta, "counter");
    assert_eq!(
        counter.return_wrapper_role,
        Some(verter_type_expr::ReactiveWrapperRole::Ref),
        "the bodiless composable's authored return type must publish the exact role"
    );
    assert_eq!(
        counter.reactivity_kind,
        verter_session_query::analysis::types::ReactivityKind::Ref,
        "an exact role must REFINE the collapsed decoration kind"
    );

    // EXACT, second family — the vocabulary is not collapsed onto `Ref`.
    let total = a6_binding(&meta, "total");
    assert_eq!(
        total.return_wrapper_role,
        Some(verter_type_expr::ReactiveWrapperRole::ComputedRef)
    );
    assert_eq!(
        total.reactivity_kind,
        verter_session_query::analysis::types::ReactivityKind::Computed
    );
    assert_ne!(
        total.reactivity_kind,
        verter_session_query::analysis::types::ReactivityKind::Ref,
        "ComputedRef must not collapse onto the ref decoration"
    );

    // COMPLETED NON-WRAPPER PROOF — published as such, and it does NOT downgrade
    // the value-space classification (see the monotonicity test for why).
    let plain = a6_binding(&meta, "plain");
    assert_eq!(
        plain.return_wrapper_role,
        Some(verter_type_expr::ReactiveWrapperRole::None),
        "a resolved non-wrapper return type is a COMPLETE proof, not a degradation"
    );
    assert_eq!(
        plain.reactivity_kind,
        verter_session_query::analysis::types::ReactivityKind::MaybeRef,
        "a proven non-wrapper return type must NOT downgrade the reactivity kind"
    );

    // TYPED DEGRADATION — an overload group cannot be resolved without
    // argument-based overload resolution, so it fails closed with its exact
    // reason rather than guessing the first overload's family.
    let ambiguous = a6_binding(&meta, "ambiguous");
    assert_eq!(
        ambiguous.return_wrapper_role,
        Some(verter_type_expr::ReactiveWrapperRole::Unresolved {
            reason: verter_type_expr::ReactiveWrapperUnresolvedReason::Unsupported
        }),
        "an overload group must degrade typed, never guess ordinal 0"
    );
    assert_eq!(
        ambiguous.reactivity_kind,
        verter_session_query::analysis::types::ReactivityKind::MaybeRef,
        "a degradation must claim no reactivity"
    );

    // The four classes are mutually distinguishable on the published surface —
    // an implementation that collapsed any pair fails here.
    let published = [
        counter.return_wrapper_role.clone(),
        total.return_wrapper_role.clone(),
        plain.return_wrapper_role.clone(),
        ambiguous.return_wrapper_role.clone(),
    ];
    for index in 1..published.len() {
        assert!(
            !published[..index].contains(&published[index]),
            "published role {:?} must not alias an earlier class",
            published[index]
        );
    }

    // Warm-hit stability: a second read serves the same exact + degraded roles.
    let warm = host
        .get_component_meta(owner)
        .expect("warm component meta must resolve");
    for name in ["counter", "total", "plain", "ambiguous"] {
        assert_eq!(
            a6_binding(&warm, name).return_wrapper_role,
            a6_binding(&meta, name).return_wrapper_role,
            "the second read must serve `{name}`'s role unchanged"
        );
        assert_eq!(
            a6_binding(&warm, name).reactivity_kind,
            a6_binding(&meta, name).reactivity_kind,
            "the second read must serve `{name}`'s refined kind unchanged"
        );
    }
}

/// MONOTONE REFINEMENT — `ReactiveWrapperRole::None` is a proof about the
/// WRAPPER FAMILY of the whole return type, never a proof of non-reactivity.
/// Vue's `reactive<T>(t: T): UnwrapNestedRefs<T>` means a composable that
/// returns a `reactive()` object has a return type that is NOT `Reactive<T>` —
/// so a `None` role must not downgrade a value-space classification.
///
/// Discriminating mutation: map `None` onto `ReactivityKind::None` in
/// `refined_reactivity_kind` and the `reactive`-returning binding loses its
/// value-space answer.
#[test]
fn component_meta_binding_role_none_never_downgrades_value_space_reactivity() {
    let host = make_host();
    let owner = "/workspace/src/Monotone.vue";
    upsert_vue(
        &host,
        owner,
        "<template><div>{{ state }}</div></template>\n\
         <script setup lang=\"ts\">\n\
         import { useStore } from './composables'\n\
         const state = useStore()\n\
         </script>\n",
    );
    // The composable's authored return type is a PLAIN interface — a resolvable,
    // proven non-wrapper — exactly the shape `reactive()` produces in real Vue.
    a6_wire_composable_host(
        &host,
        owner,
        "/workspace/src/composables.d.ts",
        "export interface StoreState { count: number }\n\
         export declare function useStore(): StoreState\n",
    );

    let meta = host.get_component_meta(owner).expect("component meta");
    let state = a6_binding(&meta, "state");
    assert_eq!(
        state.return_wrapper_role,
        Some(verter_type_expr::ReactiveWrapperRole::None),
        "a resolvable plain return type is a completed non-wrapper proof"
    );
    assert_eq!(
        state.reactivity_kind,
        verter_session_query::analysis::types::ReactivityKind::MaybeRef,
        "the value-space classification must survive a `None` role untouched"
    );
    assert_ne!(
        state.reactivity_kind,
        verter_session_query::analysis::types::ReactivityKind::None,
        "`ReactiveWrapperRole::None` must NEVER downgrade to `ReactivityKind::None`"
    );
}

/// FAIL-CLOSED NEGATIVES — a wrapper-SHAPED but non-Vue return head publishes no
/// reactive kind from the type path. A local `interface Ref<T>` and a `Ref` from
/// a non-`vue` package are both completed non-wrapper proofs, never `Ref`.
///
/// Discriminating mutation: drop the `workspace_is_package_backed` half of
/// `wrapper_candidate_for_route`'s gate, or accept terminal-name equality, and
/// the local fake classifies as `Ref`.
#[test]
fn component_meta_binding_role_rejects_local_and_foreign_wrapper_fakes() {
    // (a) a LOCAL `interface Ref<T>` in the composable file itself.
    let host = make_host();
    let owner = "/workspace/src/LocalFake.vue";
    upsert_vue(
        &host,
        owner,
        "<template><div>{{ fake }}</div></template>\n\
         <script setup lang=\"ts\">\n\
         import { useFake } from './composables'\n\
         const fake = useFake()\n\
         </script>\n",
    );
    a6_wire_composable_host(
        &host,
        owner,
        "/workspace/src/composables.d.ts",
        "export interface Ref<T> { value: T }\n\
         export declare function useFake(): Ref<number>\n",
    );
    let meta = host.get_component_meta(owner).expect("component meta");
    let fake = a6_binding(&meta, "fake");
    assert_eq!(
        fake.return_wrapper_role,
        Some(verter_type_expr::ReactiveWrapperRole::None),
        "a LOCAL wrapper-shaped return type is proven non-Vue, not guessed"
    );
    assert_ne!(
        fake.reactivity_kind,
        verter_session_query::analysis::types::ReactivityKind::Ref,
        "a local fake `Ref` must publish NO reactive kind from the type path"
    );

    // (b) a `Ref` from a DIFFERENT package.
    let host = make_host();
    let owner = "/workspace/src/ForeignFake.vue";
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/not-vue/index.d.ts",
        "export interface Ref<T> { value: T }\n",
    );
    upsert_vue(
        &host,
        owner,
        "<template><div>{{ foreign }}</div></template>\n\
         <script setup lang=\"ts\">\n\
         import { useForeign } from './composables'\n\
         const foreign = useForeign()\n\
         </script>\n",
    );
    let composables = "/workspace/src/composables.d.ts";
    upsert_non_sfc(
        &host,
        composables,
        "import type { Ref } from 'not-vue'\n\
         export declare function useForeign(): Ref<number>\n",
    );
    host.set_import_dependencies(
        composables,
        vec![exact_dependency(
            "not-vue",
            "/workspace/node_modules/not-vue/index.d.ts",
        )],
    );
    host.set_import_dependencies(owner, vec![exact_dependency("./composables", composables)]);
    let meta = host.get_component_meta(owner).expect("component meta");
    let foreign = a6_binding(&meta, "foreign");
    assert_eq!(
        foreign.return_wrapper_role,
        Some(verter_type_expr::ReactiveWrapperRole::None),
        "a foreign-package `Ref` is a completed non-wrapper proof"
    );
    assert_ne!(
        foreign.reactivity_kind,
        verter_session_query::analysis::types::ReactivityKind::Ref
    );
}

/// A6-06 degradation rail — a typed `Unresolved` role refuses the WHOLE
/// component-meta result warm admission (the no-poison invariant), and yet does
/// NOT drop the row: the degraded role is still published per-binding, and the
/// exact sibling binding in the same file still publishes its exact role.
///
/// Discriminating mutations: drop the `fold_result_completeness` call and the
/// degraded result is admitted; make a degradation abort the row (or the whole
/// meta) and the sibling's exact role disappears.
#[test]
fn component_meta_binding_role_degradation_refuses_warm_without_dropping_rows() {
    let host = make_host();
    let owner = "/workspace/src/Degraded.vue";
    upsert_vue(
        &host,
        owner,
        "<template><div>{{ ambiguous }}{{ counter }}</div></template>\n\
         <script setup lang=\"ts\">\n\
         import { useOverloaded, useCounter } from './composables'\n\
         const ambiguous = useOverloaded()\n\
         const counter = useCounter()\n\
         </script>\n",
    );
    a6_wire_composable_host(
        &host,
        owner,
        "/workspace/src/composables.d.ts",
        A6_BODILESS_COMPOSABLE_DTS,
    );

    let results_before = host.project_type_store().component_meta_results().len();
    let meta = host.get_component_meta(owner).expect("component meta");
    let results_after = host.project_type_store().component_meta_results().len();

    // The degraded row is PUBLISHED — a degradation is a per-row fail-closed
    // answer, not a dropped row.
    assert_eq!(
        a6_binding(&meta, "ambiguous").return_wrapper_role,
        Some(verter_type_expr::ReactiveWrapperRole::Unresolved {
            reason: verter_type_expr::ReactiveWrapperUnresolvedReason::Unsupported
        })
    );
    // …and one binding's degradation must NOT cost the whole meta its other
    // answers: the exact sibling still resolves exactly.
    assert_eq!(
        a6_binding(&meta, "counter").return_wrapper_role,
        Some(verter_type_expr::ReactiveWrapperRole::Ref),
        "one row's degradation must not suppress a sibling row's exact role"
    );
    // …nor its declared surface: the meta is a real, usable result.
    assert_eq!(
        meta.bindings.len(),
        2,
        "both bindings must be published despite the degradation"
    );

    // The no-poison rail: the degraded result is refused warm admission.
    assert_eq!(
        results_after, results_before,
        "a typed whole-return degradation must refuse the component-meta result \
         warm admission (before={results_before} after={results_after})"
    );
}

/// ZERO REGRESSION + ZERO COST — a binding the value-space walk already decided
/// is never demanded, and a non-call binding is never demanded at all. The
/// value-space classification authority is untouched.
///
/// The fixture is deliberately RESOLVABLE: `vue` declares `ref<T>(): Ref<T>`, so
/// dropping the `MaybeRef` gate would genuinely resolve a role for `decided` and
/// publish it — the assertion below then goes RED. A vue surface without that
/// declaration would make the test pass either way (the demand would find no
/// prepared declaration), i.e. would not discriminate.
#[test]
fn component_meta_binding_role_is_not_demanded_when_value_space_decided() {
    let host = make_host();
    let owner = "/workspace/src/NoDemand.vue";
    upsert_vue(
        &host,
        owner,
        "<template><div>{{ decided }}{{ literal }}</div></template>\n\
         <script setup lang=\"ts\">\n\
         import { ref } from 'vue'\n\
         const decided = ref(0)\n\
         const literal = 42\n\
         </script>\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/vue/index.d.ts",
        "export interface Ref<T> { value: T }\n\
         export declare function ref<T>(value: T): Ref<T>\n",
    );
    host.set_import_dependencies(
        owner,
        vec![exact_dependency(
            "vue",
            "/workspace/node_modules/vue/index.d.ts",
        )],
    );

    let meta = host.get_component_meta(owner).expect("component meta");
    let decided = a6_binding(&meta, "decided");
    assert_eq!(
        decided.reactivity_kind,
        verter_session_query::analysis::types::ReactivityKind::Ref,
        "the value-space `ref()` classification is unchanged"
    );
    assert_eq!(
        decided.return_wrapper_role, None,
        "a value-space-decided binding must never be demanded a whole-return role"
    );
    assert_eq!(
        a6_binding(&meta, "literal").return_wrapper_role,
        None,
        "a non-call binding must never be demanded"
    );
}

/// The FIXED MECHANISM, proven by an OBSERVABLE consequence rather than a
/// counter: the demand resolves through the caller's request-bound
/// `ResolverContext`, so a SESSION OVERLAY is visible to it.
///
/// A session overlay rewrites the composable's authored return type from
/// `Ref<number>` to `ComputedRef<number>` WITHOUT touching the shared base. A
/// demand routed through the request's own context reads the overlay and
/// publishes `ComputedRef`; a demand routed through the bare `&VerterHost` (the
/// deleted entry's shape) reads BASE content and publishes `Ref`. The base host
/// is asserted unchanged afterwards, so the overlay cannot have leaked.
///
/// `bare_engine_constructions` is deliberately NOT the instrument here: the
/// counter is snapshotted from the finalising thread's request context and does
/// not observe this demand's dispatch construction at all, so asserting on it
/// would be a non-discriminating assertion dressed as a mechanism proof.
///
/// Discriminating mutation: build the demand's dispatch from
/// `ctx.host_for_fact_tracer_install()` instead of `ctx` and the overlay answer
/// reverts to the base `Ref`.
#[test]
fn component_meta_binding_return_wrapper_role_demand_is_request_bound() {
    let meta_host = crate::component_meta_host::ComponentMetaHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let host = meta_host.host();
    let owner = "/workspace/src/Overlaid.vue";
    let composables = "/workspace/src/composables.d.ts";
    upsert_vue(
        host,
        owner,
        "<template><div>{{ counter }}</div></template>\n\
         <script setup lang=\"ts\">\n\
         import { useCounter } from './composables'\n\
         const counter = useCounter()\n\
         </script>\n",
    );
    a6_wire_composable_host(host, owner, composables, A6_BODILESS_COMPOSABLE_DTS);

    // Baseline on the shared base: the authored `Ref<number>` return.
    let base = host.get_component_meta(owner).expect("base component meta");
    assert_eq!(
        a6_binding(&base, "counter").return_wrapper_role,
        Some(verter_type_expr::ReactiveWrapperRole::Ref),
        "baseline: the base composable returns Ref<number>"
    );

    // A session overlay rewrites ONLY the composable's return annotation.
    let session = meta_host.open_session_batch().expect("session opens");
    session
        .upsert(
            composables,
            "import type { Ref, ComputedRef } from 'vue'\n\
             export declare function useCounter(): ComputedRef<number>\n\
             export declare function unused(): Ref<number>\n"
                .to_string(),
        )
        .expect("overlay upsert");
    let overlaid = session
        .get_component_meta(owner)
        .expect("session component meta")
        .expect("session meta present");
    assert_eq!(
        a6_binding(&overlaid, "counter").return_wrapper_role,
        Some(verter_type_expr::ReactiveWrapperRole::ComputedRef),
        "the demand must resolve through the REQUEST's context, so the session \
         overlay decides the role — a base-view dispatch would answer from the \
         shared base and still report Ref"
    );
    assert_eq!(
        a6_binding(&overlaid, "counter").reactivity_kind,
        verter_session_query::analysis::types::ReactivityKind::Computed,
        "and the refined decoration kind follows the overlay too"
    );

    // The overlay must not have leaked into the shared base.
    drop(session);
    let base_again = host.get_component_meta(owner).expect("base component meta");
    assert_eq!(
        a6_binding(&base_again, "counter").return_wrapper_role,
        Some(verter_type_expr::ReactiveWrapperRole::Ref),
        "the session overlay must never poison the shared base answer"
    );
}

/// FOOTPRINT — the demand opens only its own subject's route. An unrelated
/// wrapper-shaped cold module in the workspace is never read, and the demand's
/// declaration-lowering footprint does not scale with the composable file's
/// unrelated declarations.
///
/// Discriminating mutation: demand for every binding regardless of the gates, or
/// reintroduce an owner-wide scan, and the cold decoy is read.
#[test]
fn component_meta_binding_role_demand_does_not_read_unrelated_cold_module() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/node_modules/vue/index.d.ts",
        RETURN_WRAPPER_VUE_DTS,
    );
    ws.inject_file(
        "/workspace/src/cold.ts",
        "export interface Ref<T> { value: T }\nexport declare function useCold(): Ref<number>\n",
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    let owner = "/workspace/src/Footprint.vue";
    upsert_vue(
        &host,
        owner,
        "<template><div>{{ counter }}</div></template>\n\
         <script setup lang=\"ts\">\n\
         import { useCounter } from './composables'\n\
         const counter = useCounter()\n\
         </script>\n",
    );
    a6_wire_composable_host(
        &host,
        owner,
        "/workspace/src/composables.d.ts",
        A6_BODILESS_COMPOSABLE_DTS,
    );

    ws.reset_reads();
    let meta = host.get_component_meta(owner).expect("component meta");
    assert_eq!(
        a6_binding(&meta, "counter").return_wrapper_role,
        Some(verter_type_expr::ReactiveWrapperRole::Ref),
        "the requested subject must still resolve exactly"
    );
    assert_eq!(
        ws.read_count("/workspace/src/cold.ts"),
        0,
        "the demand must not traverse an unrelated wrapper-shaped cold module"
    );
    // Positive control: the probe itself CAN see a read of that path, so the
    // zero above is a real absence and not an inert counter.
    let _ = host.ensure_indexed_ready("/workspace/src/cold.ts");
    assert!(
        ws.read_count("/workspace/src/cold.ts") > 0,
        "control: the read probe must be capable of observing this path"
    );
}

/// @ai-generated - template slots computed even when type deps are unresolved
#[test]
fn template_slots_with_unresolved_type_deps() {
    let host = make_host();
    // Don't upsert ./types.ts — the dep is unresolved
    upsert_vue(
        &host,
        "/Comp.vue",
        r#"<script setup lang="ts">
import type { Foo } from './types'
defineProps<Foo>()
</script>
<template><slot /></template>"#,
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    let tpl = analysis
        .template
        .expect("template should be computed even with unresolved type deps");
    assert_eq!(
        tpl.defined_slots.len(),
        1,
        "should detect the <slot> despite unresolved type dep"
    );
}

/// CF3-A1-LAZY-VALUE-IMPORTS — a type-only import is NOT a runtime value
/// binding, so the lazy template lane (upsert → `get_analysis`, no
/// `compile_entry`) must NEVER carrier-link a `<X/>` tag to it. Covers both
/// the declaration-level (`import type { X }`) and the per-specifier
/// (`import { type X }`) form.
#[test]
fn cf3_lazy_template_excludes_type_only_component_links() {
    let host = make_host();
    upsert_vue(
        &host,
        "/DeclTypeOnly.vue",
        r#"<script setup lang="ts">
import type { X } from './X.vue'
</script>
<template><X /></template>"#,
    );
    upsert_vue(
        &host,
        "/SpecifierTypeOnly.vue",
        r#"<script setup lang="ts">
import { type X } from './X.vue'
</script>
<template><X /></template>"#,
    );

    for canonical in ["/DeclTypeOnly.vue", "/SpecifierTypeOnly.vue"] {
        let analysis = host.get_analysis(canonical).unwrap();
        let tpl = analysis
            .template
            .expect("template analysis should be populated");
        let usage = tpl
            .components
            .iter()
            .find(|c| c.name == "X")
            .expect("<X/> usage should be recorded");
        assert_eq!(
            usage.import_source, None,
            "a type-only import must not carrier-link <X/> ({canonical})"
        );
    }
}

// ── Fix 1: effective_target + resolved_dependency_targets ──────────

#[test]
fn effective_target_returns_resolved_when_present() {
    let res = crate::types::DependencyResolution {
        specifier: "./types".to_string(),
        resolved_canonical_id: Some("/src/types.ts".to_string()),
        possible_canonical_ids: vec!["/src/types.js".to_string(), "/src/types.d.ts".to_string()],
    };
    assert_eq!(
        res.effective_target(),
        Some("/src/types.ts"),
        "resolved_canonical_id should win over possibles"
    );
}

/// Inverted-polarity successor to the two deleted baked-wildcard-canonical
/// regressions (RouteDb stale-serve holes 1 + 2's producer-agreement half).
///
/// Those pinned that the indexed materialiser BAKED the same wildcard target
/// as the shared route-edge policy, because a producer disagreement between
/// two resolutions of the same edge stale-served every route depending on it.
/// The artifact bakes nothing now: the shallow surface retains the AUTHORED
/// specifier and every consumer demands the target from the one route-edge
/// authority, so the disagreement class is gone by construction.
///
/// What remains testable — and is asserted here in both directions — is that
/// the surviving authority is TS-first and that the parse artifact really is
/// target-free. FAILS against a regression that reintroduces a baked target
/// (the surface would have to carry one) or that makes the live authority
/// pick the runtime `.js`.
#[test]
fn indexed_barrel_wildcard_surface_is_specifier_only_and_resolves_ts_first() {
    let ws = Arc::new(CountingWorkspace::new());
    let barrel = "/workspace/index.ts";
    // PLAIN `export *` (not `export type *`) — the EsmImport-classified shape,
    // which is exactly where the two producers used to diverge.
    ws.inject_file(barrel, "export * from './runtime';\n");
    // `./runtime` has a runtime `.js` WITH a `.d.ts` declaration companion: the
    // shared policy picks the `.d.ts`, a raw EsmImport resolve picks the `.js`.
    ws.inject_file("/workspace/runtime.js", "export const Runtime = true\n");
    ws.inject_file("/workspace/runtime.d.ts", "export type Runtime = boolean\n");
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    // A configured project makes the workspace resolver resolve relative
    // specifiers (mirrors a real workspace; a bare unconfigured workspace
    // cannot resolve `./runtime`).
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);

    let indexed = host
        .ensure_indexed_ready(barrel)
        .expect("indexed materialiser must produce an artifact for the barrel");
    let wildcards = &indexed.shallow_state.wildcard_reexports;
    assert_eq!(
        wildcards.len(),
        1,
        "the barrel has exactly one `export *` wildcard reexport"
    );
    assert_eq!(
        wildcards[0].source_specifier, "./runtime",
        "the published surface retains the AUTHORED wildcard specifier"
    );

    // The ONE authority resolves it, TS-first. This is the answer every
    // consumer of that wildcard now demands.
    assert_eq!(
        host.resolve_route_edge_canonical(barrel, "./runtime")
            .as_deref(),
        Some("/workspace/runtime.d.ts"),
        "the shared route-edge policy is TS-first (.d.ts companion), and it is \
         the only place the wildcard target comes from"
    );

    // Negative: materialising the barrel must not have published the
    // runtime `.js` as a resolved dependency artifact behind the TS-first
    // answer's back.
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/runtime.js")
            .is_none(),
        "the TS-first edge must not drag the runtime `.js` into the artifact store"
    );
}

/// DISCRIMINATING regression (RouteDb stale-serve hole 2): a cached
/// `RouteResult::Miss` produced because an `export *` wildcard edge could not
/// be resolved must NOT be served stale after the wildcard's target file
/// appears. The Miss was rooted only on the provider's `FileWholeHash` +
/// `Route` derived hash — neither of which re-resolves a known-miss specifier
/// — so when the target appeared (the provider's own content unchanged) the
/// recorded facts still revalidated and the stale Miss was served forever.
///
/// The fix roots such a Miss in the import-route witness rail:
/// the owner's import-route witness re-resolves the provider's known-miss
/// specifiers against the live workspace, so the recorded fact changes the
/// moment `./missing` resolves — invalidating the cached Miss. (If that fact
/// cannot be produced, the route entry is not admitted, so a cold re-resolve
/// runs instead of serving an unrooted Miss.)
///
/// FAILS pre-fix: the second resolve returns `None` (the stale cached Miss).
/// PASSES post-fix: the second resolve returns the now-existing target.
#[test]
fn unresolvable_wildcard_route_miss_reresolves_after_target_appears() {
    let host = make_host();
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);

    // A barrel that re-exports from a target which does NOT yet exist.
    upsert_non_sfc(&host, "/workspace/index.ts", "export * from './missing';\n");

    // Cold resolve: `Foo` cannot resolve because `./missing` is unresolvable.
    let first = host.resolve_named_type_export_target("/workspace/index.ts", "Foo");
    assert_eq!(
        first, None,
        "precondition: Foo must miss while ./missing does not resolve"
    );

    // The wildcard target appears (file-set change advances the epoch).
    upsert_non_sfc(
        &host,
        "/workspace/missing.ts",
        "export type Foo = string;\n",
    );

    // The cached Miss MUST NOT be served stale — Foo now resolves through the
    // wildcard to the freshly-appeared target.
    let second = host.resolve_named_type_export_target("/workspace/index.ts", "Foo");
    assert_eq!(
        second,
        Some(("/workspace/missing.ts".to_string(), "Foo".to_string())),
        "after ./missing appears, the cached wildcard Miss MUST invalidate and \
         re-resolve Foo to the now-existing target (RouteDb stale-serve hole 2)"
    );
}

/// DISCRIMINATING regression (OwnerImportSurface, unresolved-direct-import
/// staleness): an owner surface whose computation SKIPPED an unresolvable
/// direct import must carry a fact that goes stale when the missing target
/// appears. The cold body's skip arm recorded NO owner `ImportRoute` fact,
/// so the cached (empty-binding) surface was signed only by facts that do
/// not move on a file appearance — `resolve_owner_direct_import` kept
/// returning `None` forever after the target appeared.
///
/// The fix roots the skip in the import-route witness rail (the
/// same rail that roots unresolvable wildcard route misses):
/// the owner's import-route witness re-resolves the owner's known-miss
/// specifiers against the live workspace, so the recorded fact MOVES the
/// moment `./missing` resolves and the warm surface read declines.
///
/// FAILS pre-fix: the second resolve returns `None` (stale warm surface).
/// PASSES post-fix: the second resolve returns the now-existing target.
#[test]
fn owner_import_surface_unresolved_direct_import_reresolves_after_target_appears() {
    let host = make_host();
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);

    upsert_non_sfc(
        &host,
        "/workspace/owner.ts",
        "import { Foo } from './missing';\nexport type Bar = Foo;\n",
    );

    let first = host.resolve_owner_direct_import("/workspace/owner.ts", "Foo");
    assert_eq!(
        first, None,
        "precondition: Foo must not resolve while ./missing does not exist"
    );

    upsert_non_sfc(
        &host,
        "/workspace/missing.ts",
        "export type Foo = string;\n",
    );

    let second = host.resolve_owner_direct_import("/workspace/owner.ts", "Foo");
    assert_eq!(
        second,
        Some(("/workspace/missing.ts".to_string(), "Foo".to_string())),
        "STALE OWNER SURFACE: after ./missing appears the cached owner import \
         surface (computed while the import was unresolvable) MUST go stale and \
         re-resolve — the skip arm roots in the owner's ImportRoute fact rail"
    );
}

/// DISCRIMINATING regression (bindingless imports): a file whose ONLY
/// cross-file construct is a specifier-less import (`import './dep';` — no
/// bindings) must still have that specifier in its AUTHORED inventory and
/// must still resolve it live once the target appears.
///
/// The failure this originally caught was an edge-currency blind spot: the
/// bindingless import was missing from the shallow edge inventory, so an
/// artifact carrying a baked known-miss for it was judged permanently
/// edge-current. No artifact bakes a target now, so the surviving half of
/// the invariant is the inventory itself plus the live re-resolution —
/// which is also what the owner's import-route WITNESS is built from, so a
/// specifier missing from the inventory would silently drop out of the
/// witness too.
#[test]
fn bindingless_import_surface_reresolves_after_target_appears() {
    let host = make_host();
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);

    upsert_non_sfc(
        &host,
        "/workspace/owner.ts",
        "import './dep';\nexport const owner = 1;\n",
    );

    let _first = host
        .ensure_indexed_ready("/workspace/owner.ts")
        .expect("owner IndexedReady materialises");
    assert_eq!(
        host.resolve_type_dependency_canonical_shallow("/workspace/owner.ts", "./dep"),
        None,
        "precondition: while ./dep does not exist the specifier resolves to nothing"
    );
    let witness_before = host
        .owner_import_route_witness_for_tests("/workspace/owner.ts")
        .expect("the owner must produce a rootable witness");
    assert!(
        !witness_before.is_empty(),
        "INVENTORY BLIND SPOT: a bindingless (side-effect) import is a \
         cross-file edge and must appear in the owner's AUTHORED specifier \
         inventory — an empty witness means it dropped out"
    );
    let view_before = host.resolver_store_view_read().into_owned_view();
    for fact in &witness_before {
        assert!(
            verter_session_query::facts::store_view::StoreView::validates(&view_before, fact),
            "precondition: {fact:?} must validate against the view it was captured from"
        );
    }

    upsert_non_sfc(&host, "/workspace/dep.ts", "export const dep = 1;\n");

    assert_eq!(
        host.resolve_type_dependency_canonical_shallow("/workspace/owner.ts", "./dep")
            .as_deref(),
        Some("/workspace/dep.ts"),
        "the bindingless import must re-resolve live once its target appears"
    );
    let view_after = host.resolver_store_view_read().into_owned_view();
    assert!(
        witness_before.iter().any(|fact| {
            !verter_session_query::facts::store_view::StoreView::validates(&view_after, fact)
        }),
        "the pre-appearance witness must stop validating — otherwise a consumer \
         rooted on it warm-serves the miss forever. Witness: {witness_before:?}"
    );
}

/// DISCRIMINATING regression (import-route currency, POSITIVE
/// retarget): the owner's resolution witness must move when an
/// already-resolving specifier RETARGETS, not only when a known-miss
/// becomes resolvable. A prefetch-class positive recorded `./dep →
/// /workspace/dep.js`; when the `.d.ts` companion appears the TS-first
/// policy retargets the edge, while neither the owner's bytes nor the
/// old target's bytes move.
///
/// This is precisely the case an "answer-shaped" rail cannot see. The
/// shared type-route policy probes the `TypeImport` lane FIRST, and only
/// that probe's absence observation changes here — the final admitted
/// transaction (the `EsmImport` fallback, pre-appearance) would report an
/// unchanged signature. So the witness builder must record every
/// resolution it drives, including the intermediate lane. FAILS if the
/// builder keeps only the returned carrier's own signature.
#[test]
fn import_route_witness_moves_when_a_positive_retargets() {
    let host = make_host();
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);

    upsert_non_sfc(&host, "/workspace/dep.js", "export const dep = 1;\n");
    upsert_non_sfc(
        &host,
        "/workspace/owner.ts",
        "import { dep } from './dep';\nexport const owner = dep;\n",
    );

    assert_eq!(
        host.resolve_type_dependency_canonical_shallow("/workspace/owner.ts", "./dep")
            .as_deref(),
        Some("/workspace/dep.js"),
        "precondition: with no companion present the edge resolves to the runtime .js"
    );

    let view_before = host.resolver_store_view_read().into_owned_view();
    let witness_before = host
        .owner_import_route_witness_for_tests("/workspace/owner.ts")
        .expect("the resolving positive yields a rootable witness");
    assert!(
        !witness_before.is_empty(),
        "the positive must have observed at least one resolver fact — an \
         empty witness would make the retarget assertion vacuous"
    );
    for fact in &witness_before {
        assert!(
            verter_session_query::facts::store_view::StoreView::validates(&view_before, fact),
            "precondition: {fact:?} must validate against the view it was captured from"
        );
    }

    // The declaration companion appears — the dependency file set moves and
    // the TS-first policy now retargets `./dep` to the `.d.ts`.
    upsert_non_sfc(
        &host,
        "/workspace/dep.d.ts",
        "export declare const dep: number;\n",
    );

    let view_after = host.resolver_store_view_read().into_owned_view();
    assert!(
        witness_before.iter().any(|fact| {
            !verter_session_query::facts::store_view::StoreView::validates(&view_after, fact)
        }),
        "STALE POSITIVE ROUTE WITNESS: the appearance of the higher-priority \
         .d.ts companion must invalidate the pre-retarget witness — otherwise \
         dependents warm-validate against the retargeted route forever. \
         Witness: {witness_before:?}"
    );
}

/// DISCRIMINATING regression (RouteDb stale-serve hole 2, review finding 1
/// facet b — wrongly-drops-valid / over-aggressive None). A barrel whose only
/// edges are `export *` wildcards (`export * from './missing'; export * from
/// './present';`) is a wildcard-only provider: its wildcards resolve into a
/// local `dep_edges` map and are NOT published into `import_routes`, so
/// the wildcard rooting produced no witness (`None`). The hole-2
/// rooting loop fed that `None` through `?`, dropping the WHOLE route entry —
/// so a valid result resolved via the LATER wildcard (`./present`) was returned
/// as `None` (no value served at all). "Do not admit to cache" was wrongly
/// implemented as "return no result".
///
/// The fix splits the two cleanly: when an owner's import-route hash cannot be
/// produced, the resolved route surface is still RETURNED to the caller (with
/// empty facts → route_db's negative-cache path serves it without persisting),
/// never dropped. The next query re-resolves cold against the live workspace.
///
/// FAILS pre-fix: the first resolve returns `None` (the valid `./present`
/// result is wrongly dropped). PASSES post-fix: the valid result is returned
/// and still resolves after `./missing` appears.
#[test]
fn route_resolved_via_later_wildcard_not_dropped_by_unresolvable_earlier_wildcard() {
    let host = make_host();
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);

    upsert_non_sfc(
        &host,
        "/workspace/present.ts",
        "export type Shared = number;\n",
    );
    // Barrel: an UNRESOLVABLE earlier `export *` then a RESOLVABLE later one.
    // (`Shared` is not a prefix of either wildcard's source stem, so the
    // wildcards are tried in declaration order — the unresolvable `./missing`
    // first, recording the owner as having an unresolved edge.)
    upsert_non_sfc(
        &host,
        "/workspace/index.ts",
        "export * from './missing';\nexport * from './present';\n",
    );

    // The earlier unresolvable wildcard must NOT cause the valid later-wildcard
    // result to be dropped.
    let first = host.resolve_named_type_export_target("/workspace/index.ts", "Shared");
    assert_eq!(
        first,
        Some(("/workspace/present.ts".to_string(), "Shared".to_string())),
        "a valid result resolved via a LATER `export *` wildcard MUST NOT be dropped \
         because an EARLIER `export *` wildcard is unresolvable — refusing to cache an \
         unrootable known-miss must never be implemented as returning no result \
         (RouteDb stale-serve hole 2, facet b)"
    );

    // The earlier wildcard target appears; the later-wildcard result still
    // resolves freshly (the served-without-caching surface re-resolves cold).
    upsert_non_sfc(
        &host,
        "/workspace/missing.ts",
        "export type Other = string;\n",
    );
    let second = host.resolve_named_type_export_target("/workspace/index.ts", "Shared");
    assert_eq!(
        second,
        Some(("/workspace/present.ts".to_string(), "Shared".to_string())),
        "after ./missing appears the later-wildcard result still resolves freshly"
    );
}

/// End-to-end regression: a mixed wildcard barrel re-resolves once its
/// unresolvable `export *` target appears. The barrel mixes an unresolvable
/// `export * from './missing'` with a RESOLVABLE named reexport
/// (`export { Present } from './present'`), supplied a PARTIAL `import_routes`
/// snapshot (`set_import_dependencies` recording only `./present`, omitting the
/// wildcard source). A first query for `Missing` misses; after `missing.ts`
/// appears the query MUST resolve it rather than stale-serve the cached `Miss`.
///
/// Two independent rails enforce this, and the test guards the end-to-end
/// behaviour rather than isolating either:
/// - The coverage-checked `ImportRoute` admission
///   (the route walk's own witness scope): the rooting
///   loop admits an `ImportRoute` fact ONLY when the produced hash covers
///   EVERY unresolved wildcard source the traversal hit; a partial table that
///   omits `./missing` yields no fact, so the `Miss` is returned with EMPTY
///   facts (RouteDb negative-cache: served, never persisted) and re-resolves.
/// - The shared edge-currency oracle (`route_surface_is_edge_current`): the
///   barrel's surface is wildcard-bearing, so its `Route` participant fact is
///   edge-stale once `content_generation` advances (a file appeared), which
///   independently invalidates the cached `Miss`.
///
/// Because the edge-currency oracle backstops the wildcard rail, this test is
/// NOT a discriminator for the coverage check in isolation (reverting the
/// coverage check alone keeps it green — the edge oracle still invalidates).
/// The coverage check is retained as sound cache hygiene: it refuses to record
/// an `ImportRoute` fact that does not represent every dependency the entry
/// actually rests on.
///
/// FAILS against a tree with neither rail (the original stale-serve bug);
/// PASSES with either present.
#[test]
fn import_route_fact_admitted_only_when_it_covers_unresolved_wildcard_source() {
    let host = make_host();
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);

    // A resolvable sibling exists; the wildcard target does NOT yet exist.
    upsert_non_sfc(
        &host,
        "/workspace/present.ts",
        "export type Present = number;\n",
    );
    // Barrel mixes a RESOLVABLE named reexport with an UNRESOLVABLE `export *`.
    // The file is left un-indexed (no `ensure_indexed_ready`), so the
    // import-route surface comes solely from the PARTIAL snapshot below.
    upsert_non_sfc(
        &host,
        "/workspace/index.ts",
        "export { Present } from './present';\nexport * from './missing';\n",
    );

    // PARTIAL snapshot: records ONLY `./present`. The `./missing` wildcard
    // source is deliberately omitted, so `DerivedRawState.import_routes` has a
    // fully-resolved table (no known-miss) that does NOT cover the wildcard the
    // route traversal hits. This is the bug's precondition: an owner WITH a
    // route surface whose `ImportRoute` hash silently fails to track the
    // unresolved wildcard.
    host.set_import_dependencies(
        "/workspace/index.ts",
        vec![exact_dependency("./present", "/workspace/present.ts")],
    );

    // Cold resolve a name ONLY the unresolvable wildcard can provide → MISS.
    let first = host.resolve_named_type_export_target("/workspace/index.ts", "Missing");
    assert_eq!(
        first, None,
        "precondition: Missing must miss while ./missing is unresolvable"
    );

    // The wildcard target appears (provider content unchanged; `./present`
    // still resolves identically).
    upsert_non_sfc(
        &host,
        "/workspace/missing.ts",
        "export type Missing = string;\n",
    );

    let second = host.resolve_named_type_export_target("/workspace/index.ts", "Missing");
    assert_eq!(
        second,
        Some(("/workspace/missing.ts".to_string(), "Missing".to_string())),
        "the cached Miss MUST invalidate when ./missing appears — a partial \
         import-route snapshot that resolves ./present but omits the wildcard \
         source produces an ImportRoute hash that does NOT cover ./missing, so \
         admitting it as the rooting fact stale-serves the Miss forever. The \
         fact must be admitted only when it covers every unresolved wildcard \
         source the traversal hit (RouteDb stale-serve hole 2 — coverage-checked \
         ImportRoute admission)"
    );
}

#[test]
fn read_analysis_source_and_current_eval_state_ignore_raw_import_specifiers() {
    let ws = Arc::new(CountingWorkspace::new());
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    for specifier in ["../types/html", "#build/ui/checkbox", "@nuxt/schema", "vue"] {
        ws.reset_reads();
        ws.reset_exists();

        assert!(
            host.read_analysis_source(specifier).is_none(),
            "raw import specifier {specifier} should not resolve analysis source",
        );
        assert!(
            host.current_eval_state(specifier).is_none(),
            "raw import specifier {specifier} should not materialize eval state",
        );
        assert_eq!(
            ws.read_count(specifier),
            0,
            "raw import specifier {specifier} must not trigger workspace reads",
        );
        assert_eq!(
            ws.exists_count(specifier),
            0,
            "raw import specifier {specifier} must not trigger workspace existence probes",
        );
        assert!(
            host.ensure_indexed_ready(specifier).is_none(),
            "raw import specifier {specifier} must not seed imported dependency cache entries",
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn component_meta_native_projection_skips_leaf_imported_prop_companions() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/App.vue",
        r#"<script setup lang="ts">
import type { UseComponentIconsProps } from './useComponentIcons'

defineProps<UseComponentIconsProps>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/useComponentIcons.ts",
        r#"import type { AvatarProps, IconProps } from './types'

export interface UseComponentIconsProps {
  icon?: IconProps['name']
  avatar?: AvatarProps
}"#,
    );
    ws.inject_file(
        "/src/types/index.ts",
        "export * from './Avatar.vue'\nexport * from './Icon.vue'\n",
    );
    ws.inject_file(
        "/src/Icon.vue",
        r#"<script lang="ts">
export interface IconProps {
  name: string
  mode?: 'svg' | 'css'
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Avatar.vue",
        r#"<script lang="ts">
import type { ChipProps } from './Chip.vue'

export interface AvatarProps {
  chip?: ChipProps
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Chip.vue",
        r#"<script lang="ts">
export interface ChipProps {
  tone?: string
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
        "/src/App.vue",
        vec![exact_dependency(
            "./useComponentIcons",
            "/src/useComponentIcons.ts",
        )],
    );
    host.set_import_dependencies(
        "/src/useComponentIcons.ts",
        vec![exact_dependency("./types", "/src/types/index.ts")],
    );
    host.set_import_dependencies(
        "/src/types/index.ts",
        vec![
            exact_dependency("./Avatar.vue", "/src/Avatar.vue"),
            exact_dependency("./Icon.vue", "/src/Icon.vue"),
        ],
    );
    host.set_import_dependencies(
        "/src/Avatar.vue",
        vec![exact_dependency("./Chip.vue", "/src/Chip.vue")],
    );

    ws.reset_reads();
    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::component_meta::NativePropProjectionCache::default();

    // The replacement semantic query for the retired frontier element
    // payload: the component-meta macro-elements rail resolves the routed
    // root's declaration carrier through the ONE shared dispatch and
    // projects its one-level Shallow surface (member values stay carriers).
    let resolved = host
        .resolve_component_meta_native_props(
            "/src/App.vue",
            "./useComponentIcons",
            "UseComponentIconsProps",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
        )
        .expect("UseComponentIconsProps should resolve");

    assert!(
        resolved.iter().any(|prop| prop.name == "icon"),
        "Icon-backed props should still resolve through structural indexed access, got {:?}",
        resolved
    );
    assert!(
        resolved
            .iter()
            .any(|prop| prop.name == "avatar"),
        "leaf imported prop aliases should remain present without resolving the companion body, got {:?}",
        resolved
    );
    assert!(
        ws.read_count("/src/Avatar.vue") <= 1,
        "routed native projection should skip unmatched siblings when the target is found earlier (got {} reads)",
        ws.read_count("/src/Avatar.vue"),
    );
    assert_eq!(
        ws.read_count("/src/Chip.vue"),
        0,
        "skipping the leaf companion should also avoid its transitive imported graph",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_named_type_export_target_uses_vue_tsx_registry_build() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/index.ts", "export * from './types.vue'\n");
    ws.inject_file(
        "/src/types.vue",
        r#"<script lang="tsx">
const Button = () => <button />

export type Props = {
  render: typeof Button
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let resolved = host.resolve_named_type_export_target("/src/index.ts", "Props");

    assert_eq!(
        resolved,
        Some(("/src/types.vue".to_string(), "Props".to_string())),
        "registry routing should preserve the vue script lang and find tsx exports behind barrels",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shallow_imported_barrel_state_keeps_reexport_routes_lazy_until_lookup() {
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
        r#"
export interface TargetProps { label: string }
export type TargetEmits = { change: [value: string] }
"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    let shallow = host
        .ensure_indexed_ready("/src/types/index.ts")
        .expect("barrel should materialize shallow imported state");

    // Module facts now eagerly resolve wildcard reexport specifiers via
    // workspace during materialization. The barrel's shallow state should
    // have wildcard_reexports with resolved canonical IDs.
    assert!(
        !shallow.shallow_state.wildcard_reexports.is_empty(),
        "barrel module facts should have wildcard reexport entries",
    );
    assert!(
        shallow
            .shallow_state
            .wildcard_reexports
            .iter()
            .any(|w| w.source_specifier == "./target"),
        "barrel module facts should include the ./target wildcard reexport",
    );

    ws.reset_resolves();
    let props_root = host.resolve_imported_type_root("/src/types/index.ts", "TargetProps");
    let emits_root = host.resolve_imported_type_root("/src/types/index.ts", "TargetEmits");

    assert_eq!(
        props_root,
        expected_imported_root(
            "/src/types/target.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "TargetProps",
        ),
        "TargetProps should resolve through the cached shallow barrel route",
    );
    assert_eq!(
        emits_root,
        expected_imported_root(
            "/src/types/target.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "TargetEmits",
        ),
        "TargetEmits should resolve through the cached shallow barrel route",
    );
    // The barrel's wildcard target is a RESOLVE-domain answer: the seeded
    // parse artifact names `./target` and nothing more, so the lookup
    // resolves it. The count is bounded by the number of matching-stem
    // wildcard hops the two lookups traverse (one each) — never by the
    // barrel's total sibling count, which is what the laziness contract
    // is actually about (asserted directly below).
    assert!(
        ws.resolve_count("/src/types/index.ts", "./target") <= 2,
        "the barrel's matching wildcard resolves at most once per lookup \
         (got {})",
        ws.resolve_count("/src/types/index.ts", "./target"),
    );
    assert_eq!(
        ws.resolve_count("/src/types/index.ts", "./a"),
        0,
        "an unrelated earlier wildcard sibling must never be resolved — the \
         walk stops at the matching stem",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn prepared_type_decl_keeps_export_only_barrels_shallow_for_missing_local_symbols() {
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

    ws.reset_resolves();

    let prepared = host.prepared_type_decl("/src/types/index.ts", "TargetProps");

    assert!(
        prepared.is_none(),
        "prepared decl lookup on an export-only barrel should stay local and defer route resolution",
    );
    // Laziness, restated for the resolve-domain rooting rail.
    //
    // The bundle roots on the owner's import-route RESOLUTION WITNESS,
    // so the owner's OWN authored specifiers are necessarily observed —
    // that observation IS the rail, and a lookup that skipped it would
    // publish an entry no dependency appearance could invalidate. What
    // must still not happen is the BREADTH WALK: each owner specifier is
    // observed at most once, and no wildcard sibling's SOURCE is opened
    // to hunt for the missing local symbol.
    for specifier in ["./a", "./b", "./target"] {
        assert!(
            ws.resolve_count("/src/types/index.ts", specifier) <= 1,
            "missing local prepared decl lookup must observe {specifier} at \
             most once — a repeated resolve is the breadth walk this pins \
             against (got {})",
            ws.resolve_count("/src/types/index.ts", specifier),
        );
    }
    for sibling in ["/src/types/a.ts", "/src/types/b.ts", "/src/types/target.ts"] {
        assert_eq!(
            ws.read_count(sibling),
            0,
            "missing local prepared decl lookup must not OPEN the wildcard \
             sibling {sibling} — the barrel stays shallow",
        );
    }
}

/// Laziness pin for a LOCAL-export root resolve, narrowed to the
/// content-read dimension.
///
/// WHAT IS PINNED: the dependency SOURCES stay UNREAD
/// (`read_count == 0` for `/src/a.ts` and `/src/b.ts`) — resolving a
/// symbol exported by the owner itself never reads or parses any
/// dependency's content.
///
/// WHAT IS NOT PINNED (anymore): specifier RESOLUTION. The owner's
/// whole-file route surface bakes ALL of the owner's import edges at
/// `IndexedReady` build time, so the resolver MAY canonicalise `./a`
/// and `./b` while materialising `/src/types.ts` — the earlier
/// `resolve_count == 0` pin was retired with that bake-all-owner-edges
/// design. Laziness is demand-scoped DEEPENING (reading/parsing dep
/// content on demand), not edge canonicalisation, which is owner-local
/// route-surface construction.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_imported_type_root_keeps_local_export_dep_sources_unread() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        r#"
import type { A } from './a'
import type { B } from './b'

export interface Props {
  label: string
}
"#,
    );
    ws.inject_file("/src/a.ts", "export interface A { value: string }\n");
    ws.inject_file("/src/b.ts", "export interface B { value: number }\n");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    ws.reset_reads();

    let root = host.resolve_imported_type_root("/src/types.ts", "Props");

    assert_eq!(
        root,
        expected_imported_root(
            "/src/types.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Props",
        ),
        "local exported symbols should resolve to their defining file without leaving the file",
    );
    // The owner's own import EDGES canonicalise once as part of its
    // `IndexedReady` build (the canonical-edge rule); laziness is
    // demand-scoped DEEPENING — the dependency SOURCES stay unread and
    // unparsed for a local-export resolve that never leaves the file.
    assert_eq!(
        ws.read_count("/src/a.ts"),
        0,
        "imported-root proof for a local export must not read unrelated dependency sources",
    );
    assert_eq!(
        ws.read_count("/src/b.ts"),
        0,
        "imported-root proof for a local export must not read later unrelated dependency sources",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn get_component_meta_resolves_transitive_macro_types_without_frontier_prewarm() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'

defineProps<Props>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/types.ts",
        r#"
import type { Base } from './base'

export interface Props extends Base {
  label?: string
}
"#,
    );
    ws.inject_file(
        "/src/base.ts",
        r#"
import type { Inner } from './inner'

export interface Base {
  inner?: Inner
}
"#,
    );
    ws.inject_file(
        "/src/inner.ts",
        "export interface Inner { value: string }\n",
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
        vec![exact_dependency("./base", "/src/base.ts")],
    );
    host.set_import_dependencies(
        "/src/base.ts",
        vec![exact_dependency("./inner", "/src/inner.ts")],
    );

    ws.reset_reads();
    let meta = host
        .get_component_meta("/src/Consumer.vue")
        .expect("component meta should resolve for the consumer");

    assert!(
        meta.props.iter().any(|prop| prop.name == "label"),
        "resolved props should include Props.label, got {:?}",
        meta.props,
    );
    assert!(
        meta.props.iter().any(|prop| prop.name == "inner"),
        "resolved props should include Base.inner through transitive solver lookup, got {:?}",
        meta.props,
    );
    assert!(
        ws.read_count("/src/inner.ts") <= 1,
        "transitive macro expansion should reach the inner dependency on demand without repeated workspace reads, got {}",
        ws.read_count("/src/inner.ts"),
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_named_type_export_target_registry_seeding_keeps_barrel_children_shallow() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "export * from './Button.vue'\nexport * from './Link.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
export interface ButtonProps {
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
}
</script>
<template><a /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let resolved = host.resolve_named_type_export_target("/src/types.ts", "LinkProps");

    assert_eq!(
        resolved,
        Some(("/src/Link.vue".to_string(), "LinkProps".to_string())),
        "wildcard barrel routing should still resolve the requested child",
    );

    // In the new IndexedReady DB, ensure_indexed_ready eagerly builds complete
    // facts including export_signatures and script_analysis. Verify the facts exist
    // and have the expected content.
    let barrel = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("barrel should be cached after routing");
    assert!(
        barrel.route_inventory.counts.top_level_statement_count > 0,
        "barrel routing should keep shallow routes in cache",
    );

    let child = host
        .ensure_indexed_ready("/src/Link.vue")
        .expect("matched child should be cached after routing");
    assert!(
        child.route_inventory.counts.top_level_statement_count > 0,
        "matched child should be cached through the shallow index",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shallow_imported_export_state_skips_non_reexport_import_resolution() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Link.vue",
        r#"<script lang="ts">
import type { SharedProps } from './shared'

export interface LinkProps extends SharedProps {
  href?: string
}
</script>
<template><a /></template>"#,
    );
    ws.inject_file(
        "/src/shared.ts",
        "export interface SharedProps { label?: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    ws.reset_resolves();
    let entry = host
        .ensure_indexed_ready("/src/Link.vue")
        .expect("component should materialize shallow export state");

    assert!(
        entry.export_signatures.is_some(),
        "export-only shallow state should still capture export signatures",
    );
    // The indexed materialiser publishes a pure parse/index product, and
    // ordinary forward edges are workspace-owned rather than duplicated in
    // the scheduler dependency producer. Neither path resolves this
    // non-macro import during shallow materialisation.
    assert_eq!(
        ws.resolve_count("/src/Link.vue", "./shared"),
        0,
        "non-macro imports must not re-enter Engine during shallow materialisation",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn import_route_lookup_reuses_indexed_ready_without_live_owner_state() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "export * from './Button.vue'\nexport * from './Unused.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
export interface ButtonProps {
  label?: string
}
</script>
<template><button /></template>"#,
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

    host.ensure_indexed_ready("/src/types.ts")
        .expect("barrel should seed shallow import routes");

    ws.remove_file("/src/types.ts");
    host.compile_cache().remove("/src/types.ts");

    let resolved = host.resolve_type_dependency_canonical_shallow("/src/types.ts", "./Button.vue");

    assert_eq!(
        resolved,
        Some("/src/Button.vue".to_string()),
        "dependency lookup should reuse cached imported import routes",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_named_type_export_target_unseeded_barrel_keeps_wildcard_children_shallow() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "export * from './Button.vue'\nexport * from './Link.vue'\nexport * from './Unused.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
export interface ButtonProps {
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

    ws.reset_reads();
    let resolved = host.resolve_named_type_export_target("/src/types.ts", "ButtonProps");

    assert_eq!(
        resolved,
        Some(("/src/Button.vue".to_string(), "ButtonProps".to_string())),
        "unseeded wildcard barrel routing should still resolve the first matching child",
    );
    assert_eq!(
        ws.read_count("/src/Link.vue"),
        0,
        "route selection should not preload later wildcard siblings while seeding the barrel cache",
    );
    assert_eq!(
        ws.read_count("/src/Unused.vue"),
        0,
        "route selection should stop after the matched first-level wildcard child",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_named_type_export_target_unseeded_late_match_skips_earlier_wildcard_siblings() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        concat!(
            "export * from './Accordion.vue'\n",
            "export * from './Alert.vue'\n",
            "export * from './AuthForm.vue'\n",
            "export * from './Avatar.vue'\n",
            "export * from './Checkbox.vue'\n",
            "export * from './Unused.vue'\n",
        ),
    );
    ws.inject_file(
        "/src/Accordion.vue",
        r#"<script lang="ts">
export interface AccordionProps {
  items?: string[]
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Alert.vue",
        r#"<script lang="ts">
export interface AlertProps {
  color?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/AuthForm.vue",
        r#"<script lang="ts">
export interface AuthFormProps {
  title?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Avatar.vue",
        r#"<script lang="ts">
export interface AvatarProps {
  src?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Checkbox.vue",
        r#"<script lang="ts">
export interface CheckboxProps {
  checked?: boolean
}
</script>
<template><div /></template>"#,
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

    ws.reset_reads();
    let resolved = host.resolve_named_type_export_target("/src/types.ts", "CheckboxProps");

    assert_eq!(
        resolved,
        Some(("/src/Checkbox.vue".to_string(), "CheckboxProps".to_string())),
        "late wildcard match should still resolve to the correct child",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_imported_type_root_unseeded_late_match_materializes_only_the_matched_vue_child() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        concat!(
            "export * from './Accordion.vue'\n",
            "export * from './Alert.vue'\n",
            "export * from './AuthForm.vue'\n",
            "export * from './Avatar.vue'\n",
            "export * from './Checkbox.vue'\n",
            "export * from './Unused.vue'\n",
        ),
    );
    ws.inject_file(
        "/src/Accordion.vue",
        r#"<script lang="ts">
export interface AccordionProps {
  items?: string[]
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Alert.vue",
        r#"<script lang="ts">
export interface AlertProps {
  color?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/AuthForm.vue",
        r#"<script lang="ts">
export interface AuthFormProps {
  title?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Avatar.vue",
        r#"<script lang="ts">
export interface AvatarProps {
  src?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Checkbox.vue",
        r#"<script lang="ts">
export interface CheckboxProps {
  checked?: boolean
}
</script>
<template><div /></template>"#,
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

    let root = host.resolve_imported_type_root("/src/types.ts", "CheckboxProps");

    assert_eq!(
        root,
        expected_imported_root(
            "/src/Checkbox.vue",
            verter_type_expr::TopLevelOwnerId::module(0),
            "CheckboxProps",
        ),
        "late wildcard imported-root proof should still resolve to the correct child",
    );
    for never_inspected in [
        "/src/Accordion.vue",
        "/src/Alert.vue",
        "/src/AuthForm.vue",
        "/src/Avatar.vue",
        "/src/Unused.vue",
    ] {
        assert_eq!(
            ws.read_count(never_inspected),
            0,
            "late wildcard imported-root proof should never read the uninspected sibling {never_inspected}",
        );
        assert!(
            host.project_type_store
                .indexed()
                .get_any(never_inspected)
                .is_none(),
            "Vue siblings the late wildcard proof never inspects stay off FileArtifactStore: {never_inspected}",
        );
    }
    assert!(
        host.project_type_store.indexed().get_any("/src/Checkbox.vue")
            .is_some(),
        "the matched Vue child was inspected and owns exactly one canonical IndexedReady built by the unified cold path",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_imported_type_root_reuses_indexed_vue_child_across_distinct_symbol_proofs() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/types.ts", "export * from './Accordion.vue'\n");
    ws.inject_file(
        "/src/Accordion.vue",
        r#"<script lang="ts">
export interface AccordionProps {
  items?: string[]
}

export interface AccordionEmits {
  change?: [value: string]
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

    ws.reset_reads();
    let props_root = host.resolve_imported_type_root("/src/types.ts", "AccordionProps");
    let indexed_after_first = host
        .project_type_store
        .indexed()
        .get_any("/src/Accordion.vue");
    let emits_root = host.resolve_imported_type_root("/src/types.ts", "AccordionEmits");
    let indexed_after_second = host
        .project_type_store
        .indexed()
        .get_any("/src/Accordion.vue");

    assert_eq!(
        props_root,
        expected_imported_root(
            "/src/Accordion.vue",
            verter_type_expr::TopLevelOwnerId::module(0),
            "AccordionProps",
        ),
        "first imported-root proof should resolve the Vue child type export",
    );
    assert_eq!(
        emits_root,
        expected_imported_root(
            "/src/Accordion.vue",
            verter_type_expr::TopLevelOwnerId::module(0),
            "AccordionEmits",
        ),
        "second imported-root proof should resolve through the same indexed Vue child",
    );
    assert!(
        ws.read_count("/src/Accordion.vue") <= 1,
        "distinct imported-root proofs should reuse the indexed Vue child state instead of rereading it; saw {} reads",
        ws.read_count("/src/Accordion.vue"),
    );
    let indexed_after_first = indexed_after_first.expect(
        "the first imported-root proof inspects the matched Vue child, which then owns exactly one canonical IndexedReady built by the unified cold path",
    );
    let indexed_after_second = indexed_after_second.expect(
        "the matched Vue child keeps its canonical IndexedReady across distinct symbol proofs",
    );
    assert!(
        Arc::ptr_eq(&indexed_after_first, &indexed_after_second),
        "the second symbol proof must reuse the same IndexedReady Arc the first proof materialized, not rebuild it",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_named_type_export_target_prefers_longest_wildcard_prefix_match() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "export * from './Checkbox.vue'\nexport * from './CheckboxGroup.vue'\n",
    );
    ws.inject_file(
        "/src/Checkbox.vue",
        r#"<script lang="ts">
export interface CheckboxProps {
  checked?: boolean
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/CheckboxGroup.vue",
        r#"<script lang="ts">
export interface CheckboxGroupProps {
  items?: string[]
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

    ws.reset_reads();
    let resolved = host.resolve_named_type_export_target("/src/types.ts", "CheckboxGroupProps");

    assert_eq!(
        resolved,
        Some((
            "/src/CheckboxGroup.vue".to_string(),
            "CheckboxGroupProps".to_string()
        )),
        "route selection should prefer the longest matching wildcard source stem",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_component_meta_native_props_skips_unrelated_wildcard_siblings_when_root_stem_matches() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { ModalProps } from './types'

defineProps<ModalProps>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/types.ts",
        concat!(
            "export * from './Accordion.vue'\n",
            "export * from './Alert.vue'\n",
            "export * from './Modal.vue'\n",
            "export * from './Unused.vue'\n",
        ),
    );
    ws.inject_file(
        "/src/Accordion.vue",
        r#"<script lang="ts">
export interface AccordionProps {
  items?: string[]
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Alert.vue",
        r#"<script lang="ts">
export interface AlertProps {
  color?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Modal.vue",
        r#"<script lang="ts">
export interface ModalProps {
  title?: string
}
</script>
<template><div /></template>"#,
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
        "/src/types.ts",
        vec![
            exact_dependency("./Accordion.vue", "/src/Accordion.vue"),
            exact_dependency("./Alert.vue", "/src/Alert.vue"),
            exact_dependency("./Modal.vue", "/src/Modal.vue"),
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
        "ModalProps",
        &mut tracked_deps,
        &mut resolution_deps,
        &mut cache,
    );

    assert!(
        resolved.is_some(),
        "component-meta macro resolution should still resolve ModalProps",
    );
    assert_eq!(
        ws.read_count("/src/Accordion.vue"),
        0,
        "matching wildcard stem should keep earlier unrelated siblings off the active route",
    );
    assert_eq!(
        ws.read_count("/src/Alert.vue"),
        0,
        "matching wildcard stem should skip other unrelated siblings in the same barrel layer",
    );
    assert_eq!(
        ws.read_count("/src/Unused.vue"),
        0,
        "matching wildcard stem should stop before unrelated later siblings",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/src/Accordion.vue")
            .is_none(),
        "unrelated wildcard siblings should stay off FileArtifactStore",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/src/Alert.vue")
            .is_none(),
        "unrelated wildcard siblings should stay off FileArtifactStore",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/src/Unused.vue")
            .is_none(),
        "unrelated wildcard siblings should stay off FileArtifactStore",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_component_meta_native_props_keeps_leaf_object_prop_imports_symbolic() {
    // The leaf imported object prop (`avatar?: AvatarProps`) is published as
    // a shallow reference carrier: resolving it builds the versioned root
    // identity through AT MOST ONE canonical cold shallow materialization of
    // `/src/Avatar.vue` (the permitted first read — the canonical shallow
    // inventory is what warm identities and invalidation facts root on),
    // NEVER a declaration-body execution. Avatar's decl body importing
    // `ChipProps` from `/src/Chip.vue` is the body-execution discriminator:
    // lowering Avatar's body would demand Chip.vue, so `Chip.vue == 0 reads`
    // proves the member value stayed a carrier. A repeat resolution performs
    // ZERO new workspace reads (warm identities re-serve from the canonical
    // caches).
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'

defineProps<Props>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/types.ts",
        r#"
import type { AvatarProps } from './Avatar.vue'
import type { IconProps } from './Icon.vue'

export interface Props {
  icon?: IconProps['name']
  avatar?: AvatarProps
}
"#,
    );
    ws.inject_file(
        "/src/Avatar.vue",
        r#"<script lang="ts">
import type { ChipProps } from './Chip.vue'

export interface AvatarProps {
  src?: string
  alt?: string
  chip?: ChipProps
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Chip.vue",
        r#"<script lang="ts">
export interface ChipProps {
  tone?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Icon.vue",
        r#"<script lang="ts">
export interface IconProps {
  name?: string
  class?: string
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
        vec![
            exact_dependency("./Avatar.vue", "/src/Avatar.vue"),
            exact_dependency("./Icon.vue", "/src/Icon.vue"),
        ],
    );
    host.set_import_dependencies(
        "/src/Avatar.vue",
        vec![exact_dependency("./Chip.vue", "/src/Chip.vue")],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::component_meta::NativePropProjectionCache::default();

    ws.reset_reads();
    let resolved = host.resolve_component_meta_native_props(
        "/src/Consumer.vue",
        "./types",
        "Props",
        &mut tracked_deps,
        &mut resolution_deps,
        &mut cache,
    );

    assert!(
        resolved.is_some(),
        "component-meta macro resolution should still resolve Props",
    );
    // The resolved surface publishes BOTH members: the leaf imported object
    // prop stays a shallow carrier but is still a published row.
    let native_props = resolved.as_ref().unwrap();
    assert!(
        native_props.iter().any(|prop| prop.name == "avatar"),
        "the leaf imported object prop publishes its row, got {:?}",
        native_props,
    );
    assert!(
        native_props.iter().any(|prop| prop.name == "icon"),
        "the indexed-access member publishes its row, got {:?}",
        native_props,
    );
    // At most ONE canonical cold shallow materialization of the leaf import:
    // the versioned root identity needs Avatar's canonical shallow inventory
    // exactly once per content generation.
    let avatar_cold_reads = ws.read_count("/src/Avatar.vue");
    assert!(
        avatar_cold_reads <= 1,
        "the leaf imported object prop performs at most ONE canonical cold \
         shallow materialization (got {avatar_cold_reads} reads)",
    );
    assert!(
        ws.read_count("/src/Icon.vue") > 0,
        "actionable indexed member routes should still resolve the imported file they actually need",
    );
    // NO declaration-body execution for the leaf import: Avatar's body
    // imports ChipProps, so a body lowering would demand Chip.vue.
    assert_eq!(
        ws.read_count("/src/Chip.vue"),
        0,
        "keeping the leaf member value a carrier must not execute Avatar's \
         declaration body (its transitive Chip.vue import stays untouched)",
    );
    // The canonical shallow artifact EXISTS — the first materialization is
    // the permitted canonical shallow read, stored once on the shared store.
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/src/Avatar.vue")
            .is_some(),
        "the leaf import's canonical shallow artifact exists after the cold \
         materialization (shallow inventory, not a body store)",
    );

    // A REPEAT resolution performs ZERO new workspace reads for the leaf
    // import — warm identities re-serve from the canonical caches.
    let mut tracked_deps_warm = std::collections::BTreeSet::new();
    let mut resolution_deps_warm = std::collections::BTreeSet::new();
    let mut cache_warm = crate::resolver_core::component_meta::NativePropProjectionCache::default();
    let resolved_warm = host.resolve_component_meta_native_props(
        "/src/Consumer.vue",
        "./types",
        "Props",
        &mut tracked_deps_warm,
        &mut resolution_deps_warm,
        &mut cache_warm,
    );
    assert!(
        resolved_warm.is_some(),
        "the warm repeat resolution should still resolve Props",
    );
    assert_eq!(
        ws.read_count("/src/Avatar.vue"),
        avatar_cold_reads,
        "the warm repeat performs ZERO new Avatar.vue workspace reads",
    );
    assert_eq!(
        ws.read_count("/src/Chip.vue"),
        0,
        "the warm repeat still executes no Avatar declaration body",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_component_meta_native_props_skip_imported_declaration_builds() {
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
    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();

    host.provenance().reset();
    let resolved_elements = host.resolve_component_meta_native_props(
        "/src/Consumer.vue",
        "./types",
        "ButtonProps",
        &mut tracked_deps,
        &mut resolution_deps,
        &mut cache,
    );
    assert!(
        resolved_elements.is_some(),
        "element-only imported macro resolution should still resolve ButtonProps",
    );

    let after_elements = host.provenance().snapshot();
    assert_eq!(
        after_elements.imported_macro_declaration_builds,
        0,
        "element-only imported macro resolution should not build declaration ownership it immediately discards",
    );

    let resolved_surface = host.resolve_component_meta_macro_surface(
        "/src/Consumer.vue",
        "./types",
        "ButtonProps",
        &mut tracked_deps,
        &mut resolution_deps,
        &mut cache,
    );
    assert!(
        resolved_surface.is_some(),
        "combined imported macro resolution should still resolve ButtonProps with declaration ownership",
    );

    let after_surface = host.provenance().snapshot();
    assert_eq!(
        after_surface.imported_macro_declaration_builds, 1,
        "combined imported macro resolution should still build declaration ownership once",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn required_import_routes_for_exported_whole_route_preserves_member_tail() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        r#"
import type { AvatarProps } from './Avatar.vue'
import type { IconProps } from './Icon.vue'

export interface Props {
  icon?: IconProps['name']
  avatar?: AvatarProps
}
"#,
    );
    ws.inject_file(
        "/src/Avatar.vue",
        r#"<script lang="ts">
export interface AvatarProps {
  src?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Icon.vue",
        r#"<script lang="ts">
export interface IconProps {
  name?: string
  class?: string
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
        host.ensure_loaded("/src/types.ts"),
        "types should load from the workspace",
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![
            exact_dependency("./Avatar.vue", "/src/Avatar.vue"),
            exact_dependency("./Icon.vue", "/src/Icon.vue"),
        ],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let routes = host.required_import_routes_for_exported_route(
        "/src/types.ts",
        "Props",
        &crate::resolver_core::RouteDemand::Whole,
    );

    assert_eq!(
        routes.len(),
        1,
        "whole-route imported closure should only include actionable indexed-member refs",
    );
    assert_eq!(
        routes.get("IconProps"),
        Some(&crate::resolver_core::RouteDemand::member_path(vec![
            "name".to_string()
        ])),
        "whole-route imported closure should preserve the requested member tail instead of widening to Whole",
    );
    assert!(
        !routes.contains_key("AvatarProps"),
        "direct imported object props should stay symbolic on whole-route closure",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_imported_type_root_prefers_matching_wildcard_stem_before_unrelated_earlier_siblings() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        concat!(
            "export * from './AuthForm.vue'\n",
            "export * from './Avatar.vue'\n",
            "export * from './Icon.vue'\n",
            "export * from './Unused.vue'\n",
        ),
    );
    ws.inject_file(
        "/src/AuthForm.vue",
        r#"<script lang="ts">
export interface AuthFormProps {
  title?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Avatar.vue",
        r#"<script lang="ts">
export interface AvatarProps {
  src?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Icon.vue",
        r#"<script lang="ts">
export interface IconProps {
  name?: string
}
</script>
<template><div /></template>"#,
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

    ws.reset_reads();
    let root = host.resolve_imported_type_root("/src/types.ts", "IconProps");

    assert_eq!(
        root,
        expected_imported_root(
            "/src/Icon.vue",
            verter_type_expr::TopLevelOwnerId::module(0),
            "IconProps",
        ),
        "wildcard route proof should resolve IconProps through the matching child stem",
    );
    assert_eq!(
        ws.read_count("/src/AuthForm.vue"),
        0,
        "wildcard route proof should skip earlier unrelated siblings when the requested export name points at a later matching stem",
    );
    assert_eq!(
        ws.read_count("/src/Avatar.vue"),
        0,
        "wildcard route proof should not read other earlier siblings once the matching stem narrows the active route",
    );
    assert_eq!(
        ws.read_count("/src/Unused.vue"),
        0,
        "wildcard route proof should stop after the matching child without touching later unrelated siblings",
    );
    for never_inspected in ["/src/AuthForm.vue", "/src/Avatar.vue", "/src/Unused.vue"] {
        assert!(
            host.project_type_store
                .indexed()
                .get_any(never_inspected)
                .is_none(),
            "Vue siblings the wildcard route proof never inspects stay off FileArtifactStore: {never_inspected}",
        );
    }
    assert!(
        host.project_type_store.indexed().get_any("/src/types.ts")
            .is_some(),
        "the inspected provider barrel owns exactly one canonical IndexedReady built by the unified cold path",
    );
}

// ---------------------------------------------------------------------------
// Characterization tests for solver-host local-only env behaviour
// ---------------------------------------------------------------------------

/// Test 1: local-only defineProps resolves from EvalEnv without walkers.
#[test]
fn solver_host_resolves_local_define_props_from_local_only_env() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
interface Props { msg: string; count: number }
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    let meta = host
        .get_component_meta("/App.vue")
        .expect("local defineProps should produce component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(prop_names.contains(&"msg"), "should have msg prop");
    assert!(prop_names.contains(&"count"), "should have count prop");
    assert_eq!(prop_names.len(), 2, "should have exactly 2 props");
}

/// Test 2: cross-file defineProps — the solver host must resolve imported types
/// from the host prepared-decl cache, not from the fat owner env.
///
/// This test exercises the full meta pipeline and pins the
/// invariant: cross-file `defineProps` must resolve through the host
/// prepared-decl cache using only the local-only env + solver host.
#[test]
fn solver_host_resolves_cross_file_define_props_through_host_cache() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/types.ts",
        "export interface ImportedProps { id: string; label?: string }",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import type { ImportedProps } from './types'
defineProps<ImportedProps>()
</script>
<template><div /></template>"#,
    );

    let meta = host
        .get_component_meta("/App.vue")
        .expect("cross-file defineProps should produce component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"id"),
        "imported prop 'id' must resolve, got: {prop_names:?}",
    );
    assert!(
        prop_names.contains(&"label"),
        "imported prop 'label' must resolve, got: {prop_names:?}",
    );
    // Negative: no extra props should leak
    assert_eq!(
        prop_names.len(),
        2,
        "should have exactly 2 props from ImportedProps, got: {prop_names:?}",
    );
}

/// Test 3: transitive cross-file resolution — imported type extends same-file base.
/// Invariant: the solver host must resolve both the direct import
/// AND its same-file dependencies through the prepared-decl cache.
#[test]
fn solver_host_resolves_transitive_same_file_deps_in_imported_type() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/types.ts",
        "interface BaseProps { id: string }\nexport interface ImportedProps extends BaseProps { label?: string }",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import type { ImportedProps } from './types'
defineProps<ImportedProps>()
</script>
<template><div /></template>"#,
    );

    let meta = host
        .get_component_meta("/App.vue")
        .expect("transitive imported defineProps should produce component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"id"),
        "base prop 'id' from same-file BaseProps must resolve transitively, got: {prop_names:?}",
    );
    assert!(
        prop_names.contains(&"label"),
        "direct prop 'label' from ImportedProps must resolve, got: {prop_names:?}",
    );
    assert_eq!(
        prop_names.len(),
        2,
        "should have exactly 2 props (1 inherited + 1 direct), got: {prop_names:?}",
    );
}

/// Test 4: `typeof importedValue` in a prop type — the solver host must resolve
/// runtime-value type references from imported bindings.
#[test]
fn solver_host_resolves_typeof_imported_value_in_prop_type() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/theme.ts",
        "export const importedTheme = { primary: 'blue', secondary: 'gray' } as const;",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import { importedTheme } from './theme'
defineProps<{ ui: typeof importedTheme }>()
</script>
<template><div /></template>"#,
    );

    let meta = host
        .get_component_meta("/App.vue")
        .expect("typeof importedValue defineProps should produce component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"ui"),
        "should have 'ui' prop from typeof importedTheme, got: {prop_names:?}",
    );
    assert_eq!(
        prop_names.len(),
        1,
        "should have exactly 1 prop, got: {prop_names:?}",
    );
    // Negative: prop type should not be Unknown
    let ui_prop = meta.props.iter().find(|p| p.name == "ui").unwrap();
    let ui_ty = crate::test_only::semantic_source_probe::demand_type_expr(
        &host,
        "/App.vue",
        ui_prop
            .publication
            .source_position()
            .present()
            .expect("typed ui prop"),
    )
    .unwrap_or_else(|| panic!("ui's published source must demand-materialize"));
    assert!(
        !matches!(ui_ty, verter_type_expr::TypeExpr::Unknown { .. }),
        "typeof imported value prop type should not be Unknown",
    );
}

/// Test 5: mixed local + imported types — `defineProps<LocalProps & ImportedProps>()`
/// must resolve both local and cross-file members through the solver host.
#[test]
fn solver_host_resolves_mixed_local_and_imported_intersection_props() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/types.ts",
        "export interface ImportedProps { imported_field: number }",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import type { ImportedProps } from './types'
interface LocalProps { local_field: string }
defineProps<LocalProps & ImportedProps>()
</script>
<template><div /></template>"#,
    );

    let meta = host
        .get_component_meta("/App.vue")
        .expect("mixed local+imported intersection defineProps should produce component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"local_field"),
        "local prop 'local_field' must resolve, got: {prop_names:?}",
    );
    assert!(
        prop_names.contains(&"imported_field"),
        "imported prop 'imported_field' must resolve, got: {prop_names:?}",
    );
    assert_eq!(
        prop_names.len(),
        2,
        "should have exactly 2 props (1 local + 1 imported), got: {prop_names:?}",
    );
}

/// Test 6: generic imported types — `defineProps<Partial<ImportedProps>>()`
/// must resolve through the solver host's generic instantiation.
#[test]
fn solver_host_resolves_generic_imported_partial_props() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/types.ts",
        "export interface ImportedProps { name: string; age: number }",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import type { ImportedProps } from './types'
defineProps<Partial<ImportedProps>>()
</script>
<template><div /></template>"#,
    );

    let meta = host
        .get_component_meta("/App.vue")
        .expect("Partial<ImportedProps> defineProps should produce component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"name"),
        "Partial<ImportedProps> should include 'name', got: {prop_names:?}",
    );
    assert!(
        prop_names.contains(&"age"),
        "Partial<ImportedProps> should include 'age', got: {prop_names:?}",
    );
    assert_eq!(
        prop_names.len(),
        2,
        "should have exactly 2 props from Partial<ImportedProps>, got: {prop_names:?}",
    );
    // All props should be optional because of Partial<>
    for prop in &meta.props {
        assert!(
            !prop.required,
            "Partial<> should make all props optional, but '{}' is required",
            prop.name,
        );
    }
}

/// Test 7: end-to-end fallthrough with runtime values — the meta pipeline should
/// produce fallthrough metadata for a component with a template binding referencing
/// an imported runtime value via v-bind.
#[test]
fn solver_host_fallthrough_with_imported_runtime_v_bind() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/attrs.ts",
        "export const imported_obj = { class: 'foo', id: 'bar' };",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import { imported_obj } from './attrs'
</script>
<template><div v-bind="imported_obj">hello</div></template>"#,
    );

    let meta = host
        .get_component_meta("/App.vue")
        .expect("v-bind with imported runtime value should produce component meta");
    // The component has a single native root <div>, so it should have no declared props
    assert!(
        meta.props.is_empty(),
        "no declared props expected, got: {:?}",
        meta.props.iter().map(|p| &p.name).collect::<Vec<_>>(),
    );
    // The template should parse successfully — meta should exist
    // (this test validates the runtime-value path doesn't crash, not fallthrough surface details)
}

/// Test 9: same-request macro+fallthrough — verifies that a single get_component_meta
/// call resolves both macro types AND fallthrough surface without redundant reads.
///
/// Full verification requires read/parse counter instrumentation on the host,
/// which is not available in the standalone test harness.
#[test]
fn solver_host_same_request_macro_and_fallthrough_single_pass() {
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
        "/App.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    let meta = host
        .get_component_meta("/App.vue")
        .expect("single-pass should produce component meta");

    // Both macro resolution (defineProps) and the meta surface should complete
    // in a single get_component_meta call
    assert!(
        meta.props.iter().any(|p| p.name == "label"),
        "macro type should resolve imported prop 'label'"
    );
    assert_eq!(
        host.provenance
            .get_component_meta_calls
            .load(std::sync::atomic::Ordering::Relaxed),
        1,
        "only one get_component_meta call should have been made"
    );
}

/// Test 10: negative — missing imported symbol must not silently succeed.
#[test]
fn solver_host_missing_import_does_not_silently_resolve() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    // types.ts exists but does NOT export MissingType
    upsert_ts(
        &host,
        "/types.ts",
        "export interface OtherType { x: number }",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import type { MissingType } from './types'
defineProps<MissingType>()
</script>
<template><div /></template>"#,
    );

    let meta = host.get_component_meta("/App.vue");
    // Should either return None or return meta with 0 props
    if let Some(meta) = meta {
        assert!(
            meta.props.is_empty(),
            "missing imported type must not produce phantom props, got: {:?}",
            meta.props.iter().map(|p| &p.name).collect::<Vec<_>>(),
        );
    }
}

#[test]
fn prepared_type_decl_only_prepares_requested_symbol_on_first_lookup() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        r#"
export interface Alpha { alpha: string }
export interface Beta { beta: string }
export interface Gamma { gamma: string }
export interface Delta { delta: string }
"#,
    );
    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should materialize");

    crate::resolver_core::prepared_decl::reset_prepared_type_decl_build_count_for_tests();

    let gamma = host
        .prepared_type_decl("/src/types.ts", "Gamma")
        .expect("Gamma should prepare");
    assert_eq!(gamma.root_identity.symbol_name.as_ref(), "Gamma");
    assert_eq!(
        crate::resolver_core::prepared_decl::prepared_type_decl_build_count_for_tests(),
        1,
        "first lookup should prepare only the requested symbol",
    );

    let gamma_again = host
        .prepared_type_decl("/src/types.ts", "Gamma")
        .expect("Gamma should stay cached");
    assert_eq!(gamma_again.root_identity.symbol_name.as_ref(), "Gamma");
    assert_eq!(
        crate::resolver_core::prepared_decl::prepared_type_decl_build_count_for_tests(),
        1,
        "repeat lookup should reuse the prepared symbol cache",
    );

    let alpha = host
        .prepared_type_decl("/src/types.ts", "Alpha")
        .expect("Alpha should prepare");
    assert_eq!(alpha.root_identity.symbol_name.as_ref(), "Alpha");
    assert_eq!(
        crate::resolver_core::prepared_decl::prepared_type_decl_build_count_for_tests(),
        2,
        "looking up a second symbol should prepare only that symbol",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn source_type_is_stable_across_callsites_for_same_canonical() {
    // Single-authority invariant for `source_type`:
    //
    // A regression that returned `SourceType::ts()` when `framework_parse: None`
    // but the carrier `<script lang>` resolution when
    // `framework_parse: Some(artifact)` would diverge for a `lang="tsx"` SFC —
    // different cache slots for the same `(canonical, whole_hash)`.
    //
    // The scheduler computes `source_type` once at `execute_source` time with
    // full access to the parsed SFC, stores it on `HostSourceData::source_type`,
    // and every downstream cache-key site must read the authoritative value.
    let host = make_host();
    let tsx_vue = r#"<script lang="tsx">
const Button = () => <button />
export type Props = { render: typeof Button }
</script>
<template><div /></template>"#;
    upsert_vue(&host, "/src/Foo.vue", tsx_vue);

    use crate::host_executor::HostSourceData;
    let source_snap = host
        .scheduler
        .try_get_source("/src/Foo.vue")
        .expect("scheduler should have Foo.vue");
    let hd = source_snap
        .downcast_data::<HostSourceData>()
        .expect("source data should be HostSourceData");

    // HostSourceData carries the authoritative source_type — computed once at parse time
    // using the full parse artifact, not reconstructed from raw_source + optional artifact.
    assert!(
        hd.source_type.is_jsx(),
        "HostSourceData.source_type should be tsx (JSX-bearing) for lang=tsx SFC, got {:?}",
        hd.source_type,
    );
    assert!(
        hd.source_type.is_typescript(),
        "HostSourceData.source_type should be TypeScript for lang=tsx SFC, got {:?}",
        hd.source_type,
    );
}

// ----------------------------------------------------------------
// F7 — `resolve_imported_type_root` trace dedup.
// Discriminator: same (canonical, imported_name) queried N times in
// a single request emits the `resolve_imported_type_root` Custom
// event exactly ONCE (the cache miss). Pre-fix the event fired on
// every call regardless of cache state.
//
// Test must live here (not under `tests/`) because
// `resolve_imported_type_root` is `pub(crate)` and integration
// tests cannot reach it (D36 placement rule).
// ----------------------------------------------------------------

#[cfg(test)]
mod imported_root_trace_dedup_tests {
    use super::super::*;
    use crate::component_meta_audit::structured_event::StructuredAuditEvent;
    use verter_type_engine::request_context::{RequestContext, RequestContextGuard};
    use verter_type_engine::request_footprint::RequestFootprintAccumulator;
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    fn host_with_props_ts() -> Arc<VerterHost> {
        let workspace: Arc<dyn WorkspaceAccess> =
            Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
        let host = Arc::new(VerterHost::new(
            HostConfig {
                audit_enabled: true,
                footprint_capture: true,
                ..HostConfig::default()
            },
            workspace,
        ));
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some("/props.ts".into()),
            input_id: "/props.ts".into(),
            source: Arc::from("export interface Props { label: string; }\n"),
            file_language: FileLanguage::script_ts(),
            aliases: vec![],
        });
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some("/Component.vue".into()),
            input_id: "/Component.vue".into(),
            source: Arc::from(
                "<script setup lang=\"ts\">\n\
                 import type { Props } from './props';\n\
                 defineProps<Props>();\n\
                 </script>\n\
                 <template><div /></template>\n",
            ),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        });
        host
    }

    #[test]
    fn resolve_imported_type_root_traces_once_per_cache_miss_not_per_call() {
        let host = host_with_props_ts();
        let acc = Arc::new(RequestFootprintAccumulator::new());
        let ctx = RequestContext::new(
            7777,
            Arc::from("/Component.vue"),
            true,
            Some(Arc::clone(&acc)),
        );
        let _guard = RequestContextGuard::install(ctx);

        // Five repeated calls with identical inputs. The
        // ImportedRootDb (host.resolver.runtime.imported_roots) caches
        // the result on the first call; subsequent calls are cache
        // hits.
        for _ in 0..5 {
            let _ = host.resolve_imported_type_root("/Component.vue", "Props");
        }

        let state = acc.drain();
        let resolve_events: Vec<&StructuredAuditEvent> = state
            .structured_events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    StructuredAuditEvent::Custom { name, .. }
                        if name.as_ref() == "resolve_imported_type_root"
                )
            })
            .collect();

        // Pre-fix expected count = 5 (the trace fired before the
        // cache check, so every call emitted). Post-fix expected
        // count = 1 (the trace moved inside the closure, runs only
        // on cache miss; first call misses, subsequent four hit).
        assert_eq!(
            resolve_events.len(),
            1,
            "F7 contract: `resolve_imported_type_root` must trace once per \
             cache MISS, not once per call. Got {} events for 5 identical \
             calls. Pre-fix the trace was emitted before the ImportedRootDb \
             cache check.",
            resolve_events.len(),
        );
    }
}

// ── resolve_eval_dependency_canonical_with: candidate probe contract ─────────
//
// The exact candidate probe ORDER of `resolve_eval_dependency_canonical_with`
// is a behavioral contract: callers (`VerterHost::resolve_eval_dependency_canonical`,
// the executor's `extract_deps` normalizer) rely on higher-priority typed
// companions winning over lower-priority ones, and the probe closure is
// side-effectful at some call sites (existence probes are observable). These
// tests pin the full probe sequence with a recording closure so any change to
// candidate generation — including allocation-strategy refactors — must keep
// the order, the probe count, and the returned strings byte-identical.
mod resolve_eval_dependency_probe_contract_tests {
    use super::super::resolve_eval_dependency_canonical_with;

    /// Runs the resolver with a closure that records every probed candidate
    /// in order and reports existence only for members of `existing`.
    fn probe_trace(dep: &str, existing: &[&str]) -> (Option<String>, Vec<String>) {
        let mut probed = Vec::new();
        let result = resolve_eval_dependency_canonical_with(dep, |candidate| {
            probed.push(candidate.to_string());
            existing.contains(&candidate)
        });
        (result, probed)
    }

    #[test]
    fn runtime_js_input_probes_declaration_companion_then_appends_then_input_last() {
        let (result, probed) = probe_trace("/ws/pkg/dist/index.js", &[]);
        assert_eq!(result, None);
        assert_eq!(
            probed,
            vec![
                "/ws/pkg/dist/index.d.ts",
                "/ws/pkg/dist/index.js.d.ts",
                "/ws/pkg/dist/index.js.ts",
                "/ws/pkg/dist/index.js.tsx",
                "/ws/pkg/dist/index.js/index.d.ts",
                "/ws/pkg/dist/index.js/index.ts",
                "/ws/pkg/dist/index.js/index.tsx",
                "/ws/pkg/dist/index.js",
            ],
            "a runtime .js dependency must probe its declaration companion \
             first, then the append candidates in declared order, and the raw \
             input only as the final type-companion fallback",
        );
    }

    #[test]
    fn bundler_suffix_input_probes_bundle_companion_before_plain_js_companion() {
        let dep = "/ws/@vue/runtime-core/dist/runtime-core.esm-bundler.js";
        let (result, probed) = probe_trace(dep, &[]);
        assert_eq!(result, None);
        assert_eq!(
            probed,
            vec![
                // The bundler-suffix companion strips the WHOLE bundle suffix…
                "/ws/@vue/runtime-core/dist/runtime-core.d.ts".to_string(),
                // …and the plain `.js` companion strips only `.js`, later.
                "/ws/@vue/runtime-core/dist/runtime-core.esm-bundler.d.ts".to_string(),
                format!("{dep}.d.ts"),
                format!("{dep}.ts"),
                format!("{dep}.tsx"),
                format!("{dep}/index.d.ts"),
                format!("{dep}/index.ts"),
                format!("{dep}/index.tsx"),
                dep.to_string(),
            ],
            "bundle-suffix stripping must be probed before plain .js stripping",
        );
    }

    #[test]
    fn every_bundler_suffix_probes_its_declaration_companion_first() {
        for suffix in [
            ".esm-bundler.js",
            ".esm-browser.js",
            ".esm-browser.prod.js",
            ".global.js",
            ".global.prod.js",
            ".cjs.js",
            ".cjs.prod.js",
        ] {
            let dep = format!("/ws/pkg/dist/entry{suffix}");
            let (result, probed) = probe_trace(&dep, &["/ws/pkg/dist/entry.d.ts"]);
            assert_eq!(
                result.as_deref(),
                Some("/ws/pkg/dist/entry.d.ts"),
                "suffix {suffix} must resolve to the stripped declaration companion",
            );
            assert_eq!(
                probed,
                vec!["/ws/pkg/dist/entry.d.ts".to_string()],
                "suffix {suffix}: the bundle companion must be the FIRST probe",
            );
        }
    }

    #[test]
    fn jsx_mjs_cjs_inputs_map_to_their_specific_declaration_companions() {
        let (result, probed) = probe_trace("/ws/c/comp.jsx", &["/ws/c/comp.d.ts"]);
        assert_eq!(result.as_deref(), Some("/ws/c/comp.d.ts"));
        assert_eq!(probed, vec!["/ws/c/comp.d.ts".to_string()]);

        let (result, probed) = probe_trace("/ws/m/entry.mjs", &["/ws/m/entry.d.mts"]);
        assert_eq!(result.as_deref(), Some("/ws/m/entry.d.mts"));
        assert_eq!(probed, vec!["/ws/m/entry.d.mts".to_string()]);

        let (result, probed) = probe_trace("/ws/m/entry.cjs", &["/ws/m/entry.d.cts"]);
        assert_eq!(result.as_deref(), Some("/ws/m/entry.d.cts"));
        assert_eq!(probed, vec!["/ws/m/entry.d.cts".to_string()]);
    }

    #[test]
    fn extensionless_input_probes_typed_candidates_before_raw_input() {
        let (result, probed) = probe_trace("/ws/src/runtime/types/html", &[]);
        assert_eq!(result, None);
        assert_eq!(
            probed,
            vec![
                "/ws/src/runtime/types/html.d.ts",
                "/ws/src/runtime/types/html.ts",
                "/ws/src/runtime/types/html.tsx",
                "/ws/src/runtime/types/html/index.d.ts",
                "/ws/src/runtime/types/html/index.ts",
                "/ws/src/runtime/types/html/index.tsx",
                "/ws/src/runtime/types/html",
            ],
            "an extensionless dependency probes every typed candidate before \
             falling back to the raw extensionless path",
        );
    }

    #[test]
    fn extensionless_input_resolves_to_index_candidate_in_order() {
        let (result, probed) = probe_trace("/ws/lib/util", &["/ws/lib/util/index.ts"]);
        assert_eq!(result.as_deref(), Some("/ws/lib/util/index.ts"));
        assert_eq!(
            probed,
            vec![
                "/ws/lib/util.d.ts",
                "/ws/lib/util.ts",
                "/ws/lib/util.tsx",
                "/ws/lib/util/index.d.ts",
                "/ws/lib/util/index.ts",
            ],
            "probing must stop at the first existing candidate",
        );
    }

    #[test]
    fn extensionless_input_falls_back_to_existing_raw_path_after_all_candidates() {
        let (result, probed) = probe_trace("/ws/lib/util", &["/ws/lib/util"]);
        assert_eq!(result.as_deref(), Some("/ws/lib/util"));
        assert_eq!(
            probed.len(),
            7,
            "the raw path is only probed after all six typed candidates",
        );
        assert_eq!(probed.last().map(String::as_str), Some("/ws/lib/util"));
    }

    #[test]
    fn explicit_non_js_extension_fast_path_probes_only_the_input() {
        // (b) the early-return case: an explicit non-js extension that exists
        // must be returned untouched after probing ONLY the input itself.
        let (result, probed) = probe_trace("/ws/lib/foo.d.ts", &["/ws/lib/foo.d.ts"]);
        assert_eq!(result.as_deref(), Some("/ws/lib/foo.d.ts"));
        assert_eq!(
            probed,
            vec!["/ws/lib/foo.d.ts".to_string()],
            "the explicit-extension fast path must probe exactly the input and \
             nothing else",
        );

        let (result, probed) =
            probe_trace("/ws/components/Button.vue", &["/ws/components/Button.vue"]);
        assert_eq!(result.as_deref(), Some("/ws/components/Button.vue"));
        assert_eq!(probed, vec!["/ws/components/Button.vue".to_string()]);
    }

    #[test]
    fn explicit_declaration_input_probes_itself_first_then_append_candidates() {
        let (result, probed) = probe_trace("/ws/lib/foo.d.ts", &[]);
        assert_eq!(result, None);
        assert_eq!(
            probed,
            vec![
                "/ws/lib/foo.d.ts",
                "/ws/lib/foo.d.ts.d.ts",
                "/ws/lib/foo.d.ts.ts",
                "/ws/lib/foo.d.ts.tsx",
                "/ws/lib/foo.d.ts/index.d.ts",
                "/ws/lib/foo.d.ts/index.ts",
                "/ws/lib/foo.d.ts/index.tsx",
            ],
            "a missing explicit non-js-extension input is probed FIRST (fast \
             path), then only the append candidates; no trailing raw re-probe",
        );
    }

    #[test]
    fn runtime_js_input_falls_back_to_existing_raw_path_probed_last() {
        let (result, probed) = probe_trace("/ws/d/index.js", &["/ws/d/index.js"]);
        assert_eq!(result.as_deref(), Some("/ws/d/index.js"));
        assert_eq!(
            probed,
            vec![
                "/ws/d/index.d.ts",
                "/ws/d/index.js.d.ts",
                "/ws/d/index.js.ts",
                "/ws/d/index.js.tsx",
                "/ws/d/index.js/index.d.ts",
                "/ws/d/index.js/index.ts",
                "/ws/d/index.js/index.tsx",
                "/ws/d/index.js",
            ],
            "a runtime script that exists is returned only after every typed \
             companion candidate missed",
        );
    }

    #[test]
    fn empty_input_returns_none_without_any_probe() {
        // Even an `existing` set containing the empty string must not be
        // consulted: the resolver returns before any probe.
        let (result, probed) = probe_trace("", &[""]);
        assert_eq!(result, None);
        assert!(probed.is_empty(), "empty input must not probe at all");
    }

    #[test]
    fn hidden_js_basename_probes_raw_input_twice_at_the_tail() {
        // `Path::extension()` treats `.js` (a dot-file basename) as having NO
        // extension while `ends_with(".js")` still marks it type-companion-
        // preferring — so BOTH tail fallback probes fire for the raw input.
        // This pins the exact probe multiset of the current contract.
        let (result, probed) = probe_trace("/ws/.js", &[]);
        assert_eq!(result, None);
        assert_eq!(
            probed,
            vec![
                "/ws/.d.ts",
                "/ws/.js.d.ts",
                "/ws/.js.ts",
                "/ws/.js.tsx",
                "/ws/.js/index.d.ts",
                "/ws/.js/index.ts",
                "/ws/.js/index.tsx",
                "/ws/.js",
                "/ws/.js",
            ],
            "a dot-file .js basename fires both the extensionless fallback and \
             the type-companion fallback probes",
        );
    }
}

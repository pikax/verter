//! Contract tests for the project-global cache.
//!
//! These tests lock in the observable contract the architecture must
//! satisfy. They are written against the `VerterHost` surface plus
//! the project-global store (`project_type_store` module). Hot-path
//! integrations consume the same `IndexedReady` publication and
//! extend these tests rather than duplicate them.
//!
//! Test matrix:
//!
//! - A. `IndexedReady` is published once per `(canonical_id, whole_hash)`
//!   and is shared across consumers.
//! - B. `IndexedReady` matches itself across observers — same
//!   `whole_hash`, same `shallow_state`, same `import_routes`
//!   (identity, not deep equality).
//! - C. Unrelated files stay warm across an edit to one file.
//! - D. Edits replace the live entry under the new `whole_hash` but
//!   do not mutate the previous entry in place.
//! - E. `ProjectTypeStore::bump_project_generation` is monotonic and
//!   observable through the accessor.
//! - F. `FileArtifactStore` lookups reject stale whole-hashes at the
//!   key level.
//!
//! Semantic-query-graph dedup tests
//! (`semantic_query::SemanticQueryApi::execute`) live in the same file
//! and exercise the static invariants of the key surface
//! (scope-awareness, generic-argument distinctness, projection-mode
//! distinctness) that the `semantic_query::tests` module also covers.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::{CompileErrorPolicy, FileLanguage, HostConfig, UpsertRequest, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
}

fn upsert_vue(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
}

/// Force IndexedReady materialization for a canonical. Consumers normally
/// trigger this implicitly through a query; tests need an explicit hook
/// because the upsert path evicts on content change and `FileArtifactStore`
/// is lazily re-materialized on first demand.
fn ensure_facts(host: &VerterHost, canonical_id: &str) -> [u8; 16] {
    let indexed = host
        .ensure_indexed_ready(canonical_id)
        .unwrap_or_else(|| panic!("ensure_indexed_ready returned None for {canonical_id}"));
    indexed.whole_hash
}

fn indexed_whole_hash(host: &VerterHost, canonical_id: &str) -> Option<[u8; 16]> {
    // `IndexedReady` is the only post-parse cache; warm it via
    // `ensure_indexed_ready` then verify the project store reflects
    // the same whole_hash.
    let whole_hash = host
        .ensure_indexed_ready(canonical_id)
        .map(|indexed| indexed.whole_hash)?;
    host.project_type_store()
        .indexed()
        .get(canonical_id, whole_hash)
        .map(|ir| ir.whole_hash)
}

/// A. Upserting a file publishes a matching IndexedReady entry.
#[test]
fn upsert_publishes_indexed_ready() {
    let host = host();
    upsert_ts(&host, "/w/types.ts", "export type Foo = { x: number }");

    let hash =
        indexed_whole_hash(&host, "/w/types.ts").expect("IndexedReady must be published on upsert");
    assert_ne!(hash, [0u8; 16]);
}

/// C. Unrelated files stay warm across an edit to one file — the
/// project-global store must isolate invalidation to the affected canonical.
#[test]
fn unrelated_files_stay_warm_across_edit() {
    let host = host();
    upsert_ts(&host, "/w/a.ts", "export type A = { a: number }");
    upsert_ts(&host, "/w/b.ts", "export type B = { b: string }");

    let a_hash_v1 = indexed_whole_hash(&host, "/w/a.ts").expect("IndexedReady for a.ts must exist");
    let b_hash_v1 = indexed_whole_hash(&host, "/w/b.ts").expect("IndexedReady for b.ts must exist");

    // Edit a.ts only. b.ts must remain at the same whole_hash.
    upsert_ts(&host, "/w/a.ts", "export type A = { a: string }");
    let a_hash_v2 = indexed_whole_hash(&host, "/w/a.ts").expect("IndexedReady for a.ts must exist");
    let b_hash_v2 = indexed_whole_hash(&host, "/w/b.ts").expect("IndexedReady for b.ts must exist");

    assert_ne!(a_hash_v1, a_hash_v2, "a.ts whole_hash must advance");
    assert_eq!(
        b_hash_v1, b_hash_v2,
        "unrelated b.ts must remain warm across a.ts edit"
    );
}

/// D. Edits replace the live entry under the new `whole_hash` but never
/// mutate a prior entry in place. After an edit, the old hash's entry is
/// unreachable; the new hash's entry is the live authority.
#[test]
fn edit_replaces_entry_without_in_place_mutation() {
    let host = host();
    upsert_ts(&host, "/w/t.ts", "export type T = { x: number }");
    let hash_v1 = indexed_whole_hash(&host, "/w/t.ts").expect("v1 IndexedReady must exist");

    upsert_ts(&host, "/w/t.ts", "export type T = { x: string }");
    let hash_v2 = indexed_whole_hash(&host, "/w/t.ts").expect("v2 IndexedReady must exist");
    assert_ne!(hash_v1, hash_v2);

    // Looking up under v1 now misses — the live entry is under v2.
    let miss = host.project_type_store().indexed().get("/w/t.ts", hash_v1);
    assert!(
        miss.is_none(),
        "after an edit, IndexedReady must reject stale whole_hash lookups"
    );

    let live = host.project_type_store().indexed().get("/w/t.ts", hash_v2);
    assert!(live.is_some(), "live entry under new hash must be present");
}

/// A content edit publishes a fresh `IndexedReady` under the new
/// `whole_hash`. `FileArtifactStore` is content-addressed: a lookup
/// under the new hash hits the fresh entry. The upsert performs no
/// eager eviction of the prior content-addressed artifact — a stale
/// entry may physically linger, and current reads miss it by
/// content-hash identity rather than by an eager drain.
#[test]
fn content_change_publishes_new_indexed_entry_under_new_hash() {
    let host = host();
    upsert_ts(&host, "/w/t.ts", "export type T = { x: number }");
    let hash_v1 = indexed_whole_hash(&host, "/w/t.ts").expect("v1 IndexedReady must exist");

    // After the content edit, materialize the fresh entry.
    upsert_ts(&host, "/w/t.ts", "export type T = { x: string }");
    let hash_v2 = indexed_whole_hash(&host, "/w/t.ts").expect("v2 IndexedReady must exist");

    // The edit advanced the whole_hash.
    assert_ne!(hash_v1, hash_v2);

    // The fresh entry is content-addressed under the new hash. The
    // prior v1 artifact is neither asserted present nor absent — the
    // store is content-addressed and may keep it as an inert candidate;
    // current reads key on the new hash.
    assert!(host
        .project_type_store()
        .indexed()
        .get("/w/t.ts", hash_v2)
        .is_some());
}

/// E. `ProjectTypeStore::bump_project_generation` is monotonic — the host
/// and the project-global store agree on a single generation counter.
#[test]
fn project_generation_bump_is_monotonic_from_host() {
    let host = host();
    let store = host.project_type_store();
    let g0 = store.project_generation();
    let g1 = store.bump_project_generation();
    let g2 = store.bump_project_generation();
    assert_eq!(g1, g0 + 1);
    assert_eq!(g2, g0 + 2);
    assert_eq!(store.project_generation(), g2);
}

/// F. `FileArtifactStore` lookups reject stale whole-hashes at the key level
/// without requiring the caller to know about request-view identity.
#[test]
fn indexed_lookup_rejects_stale_hash_without_a_request_view() {
    let host = host();
    upsert_ts(&host, "/w/t.ts", "export type T = {}");

    let hash = indexed_whole_hash(&host, "/w/t.ts").expect("IndexedReady must exist");
    let wrong_hash = {
        let mut h = hash;
        h[0] = h[0].wrapping_add(1);
        h
    };

    assert!(host
        .project_type_store()
        .indexed()
        .get("/w/t.ts", wrong_hash)
        .is_none());
    assert!(host
        .project_type_store()
        .indexed()
        .get("/w/t.ts", hash)
        .is_some());
}

/// Vue SFC files also publish IndexedReady — component-meta consumers must
/// read from the same cache entry whether the file is a `.ts` module or a
/// `.vue` SFC.
#[test]
fn vue_sfc_upsert_publishes_indexed_ready() {
    let host = host();
    let sfc =
        "<script setup lang=\"ts\">const x: number = 1</script>\n<template><div /></template>\n";
    upsert_vue(&host, "/w/App.vue", sfc);

    let hash = indexed_whole_hash(&host, "/w/App.vue").expect("Vue SFC must publish IndexedReady");
    assert_ne!(hash, [0u8; 16]);
}

/// The project-global `FileArtifactStore` accessor returns the same `Arc`
/// instance across repeated lookups — so concurrent warm readers never
/// clone the payload.
#[test]
fn repeated_indexed_lookups_return_same_arc() {
    let host = host();
    upsert_ts(&host, "/w/t.ts", "export type T = {}");
    let hash = indexed_whole_hash(&host, "/w/t.ts").expect("IndexedReady must exist");
    let store = host.project_type_store();
    let a = store.indexed().get("/w/t.ts", hash).unwrap();
    let b = store.indexed().get("/w/t.ts", hash).unwrap();
    assert!(Arc::ptr_eq(&a, &b), "warm lookups must share one Arc");
}

/// Dedup contract: repeated `SemanticQueryApi::execute(ResolveDecl {..})`
/// for the same scope+name key memoizes to one semantic node id and
/// the shared memo's warm entry count stays at one. Distinct
/// `ResolveDecl` keys populate distinct entries without aliasing.
#[test]
fn semantic_subqueries_dedup_across_request_boundaries() {
    use crate::project_semantic_dispatch::{resolve_decl_key, ProjectSemanticDispatch};
    use crate::semantic_query::{
        QueryResult, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    };

    let host = host();
    upsert_ts(
        &host,
        "/w/types.ts",
        "export type C = { foo: number; bar: string }",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);

    let before = host
        .project_type_store()
        .semantic_graph()
        .memo_entry_count();
    let key = resolve_decl_key("/w/types.ts", "C");
    let first = dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(key.clone()));
    let second = dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(key));

    let (a, b) = match (first, second) {
        (
            QueryResult::Value(SemanticQueryOutput { value: a, .. }),
            QueryResult::Value(SemanticQueryOutput { value: b, .. }),
        ) => (a, b),
        other => panic!("expected two values from ResolveDecl, got {other:?}"),
    };
    assert_eq!(a, b, "same key must memoize to one node id");
    let after = host
        .project_type_store()
        .semantic_graph()
        .memo_entry_count();
    assert_eq!(
        after - before,
        1,
        "two identical queries must share one warm memo entry"
    );
}

/// The canonical `execute` entry returns the domain-agnostic
/// `SemanticQueryValue`: a live `ResolveDecl` resolves to a
/// `TypeNode` carried in a `SemanticQueryOutput` with `clean` boundary
/// provenance. Negative: the value is NOT any non-`TypeNode` domain.
#[test]
fn execute_returns_typed_type_node_with_clean_provenance() {
    use crate::project_semantic_dispatch::{resolve_decl_key, ProjectSemanticDispatch};
    use crate::semantic_query::{
        QueryResult, ResultProvenance, ResultTaint, SemanticQueryApi, SemanticQueryKey,
        SemanticQueryValue, SemanticQueryValueTag,
    };

    let host = host();
    upsert_ts(&host, "/w/types.ts", "export type C = { foo: number }");
    let dispatch = ProjectSemanticDispatch::new(&host);

    let key = resolve_decl_key("/w/types.ts", "C");
    match dispatch.execute(SemanticQueryKey::ResolveDecl(key)) {
        QueryResult::Value(out) => {
            assert_eq!(
                out.provenance,
                ResultProvenance {
                    taint: ResultTaint::Clean
                },
                "boundary provenance must be clean"
            );
            assert!(
                matches!(&out.value, SemanticQueryValue::TypeNode(_)),
                "a live ResolveDecl must resolve to the TypeNode domain, got {:?}",
                out.value.tag()
            );
            // Negative: it is exactly TypeNode, not any other domain tag.
            assert_eq!(out.value.tag(), SemanticQueryValueTag::TypeNode);
            assert_ne!(out.value.tag(), SemanticQueryValueTag::OverloadSet);
            assert_ne!(out.value.tag(), SemanticQueryValueTag::Relation);
            assert_ne!(out.value.tag(), SemanticQueryValueTag::DeclarationAnalysis);
        }
        other => panic!("expected typed Value(TypeNode), got {other:?}"),
    }
}

/// Wrapper parity: `resolve_decl` (the provenance-stripped convenience
/// wrapper) returns the same node id that `execute_type_node` carries on
/// its `Value` arm for the same key.
#[test]
fn resolve_decl_wrapper_matches_execute_type_node_node() {
    use crate::project_semantic_dispatch::{resolve_decl_key, ProjectSemanticDispatch};
    use crate::semantic_query::{QueryResult, SemanticQueryApi, SemanticQueryKey};

    let host = host();
    upsert_ts(&host, "/w/types.ts", "export type C = { foo: number }");
    let dispatch = ProjectSemanticDispatch::new(&host);

    let key = resolve_decl_key("/w/types.ts", "C");
    let wrapper = match dispatch.resolve_decl(key.clone()) {
        QueryResult::Value(node) => node,
        other => panic!("expected wrapper Value, got {other:?}"),
    };
    let typed = match dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(key)) {
        QueryResult::Value(out) => out.value,
        other => panic!("expected typed Value, got {other:?}"),
    };
    assert_eq!(
        wrapper, typed,
        "resolve_decl wrapper node must equal execute_type_node's node"
    );
}

/// Phase 3: a second `get_component_meta` call against the same owner
/// version hits `ComponentMetaResultDb` — the cache's live-entry counter
/// stays at one and the payload is returned without re-running the
/// resolver pipeline.
#[test]
fn component_meta_warm_rerun_hits_final_result_cache_phase3() {
    let host = host();
    upsert_vue(
        &host,
        "/w/App.vue",
        "<script setup lang=\"ts\">defineProps<{ title: string }>()</script>\n<template><div /></template>\n",
    );

    let before = host
        .project_type_store()
        .counters
        .snapshot()
        .component_meta_live;

    let first = host
        .get_component_meta("/w/App.vue")
        .expect("first query must return component meta");
    let after_first = host
        .project_type_store()
        .counters
        .snapshot()
        .component_meta_live;
    assert_eq!(
        after_first - before,
        1,
        "cold build must publish exactly one cache entry"
    );

    // Second call: warm hit — no new cache entries.
    let second = host
        .get_component_meta("/w/App.vue")
        .expect("second query must return the cached meta");
    let after_second = host
        .project_type_store()
        .counters
        .snapshot()
        .component_meta_live;
    assert_eq!(
        after_second, after_first,
        "warm rerun must not publish a new cache entry"
    );
    assert_eq!(
        first.props.len(),
        second.props.len(),
        "warm result payload must match cold build"
    );
}

/// Transitive-dependency invalidation contract: a warm
/// `get_component_meta` cache hit must invalidate when a *transitive*
/// dependency file changes, not just the owner file. A dep-signature
/// that omits transitive whole-hashes would leave the cached entry
/// warm under an unchanged owner hash and return a stale payload.
#[test]
fn component_meta_cache_invalidates_on_transitive_dep_edit() {
    let host = host();
    upsert_ts(
        &host,
        "/w/props.ts",
        "export interface Props { label: string }",
    );
    upsert_vue(
        &host,
        "/w/App.vue",
        "<script setup lang=\"ts\">\nimport type { Props } from './props'\ndefineProps<Props>()\n</script>\n<template><div /></template>\n",
    );

    let first = host
        .get_component_meta("/w/App.vue")
        .expect("initial cold build");
    assert_eq!(
        first.props.len(),
        1,
        "initial build should see one prop from the dep file"
    );

    // Warm-entry live counter is 1 — the cache published the owner's result.
    assert_eq!(
        host.project_type_store()
            .counters
            .snapshot()
            .component_meta_live,
        1
    );

    // Edit the dep file, not the owner. Owner's whole-hash is unchanged, but
    // the dep's whole-hash advanced. Without transitive dep tracking the
    // warm entry would return the pre-edit single-prop payload.
    upsert_ts(
        &host,
        "/w/props.ts",
        "export interface Props { label: string; disabled: boolean }",
    );

    let refreshed = host
        .get_component_meta("/w/App.vue")
        .expect("post-transitive-edit lookup");
    assert_eq!(
        refreshed.props.len(),
        2,
        "transitive dep edit must invalidate the owner's cache entry and pick up the new prop"
    );
}

// The post-owner-edit component-meta recompute contract is owned by
// the `block_2_canary_owner_self_edit` suite — `owner_self_edit_to_
// local_prop_type_recomputes_component_meta` asserts the recomputed
// props reflect the edit, a stronger observable than a physical
// entry-count check. Same-canonical invalidation is lazy: a warm
// `ComponentMetaResultDb` entry is rejected on read by its
// self-version root, not by an eager upsert-time drain, so a
// physical-entry-count assertion no longer characterizes the contract.

/// Phase 2: direct owner imports are resolved once per owner version and
/// reused across stages. The first lookup on an owner builds the surface;
/// every subsequent lookup against the same `(owner, whole_hash)` must hit
/// the cached surface without rebuilding.
#[test]
fn owner_direct_imports_resolve_once_per_owner_version_phase2() {
    let host = host();
    upsert_ts(
        &host,
        "/w/types.ts",
        "export type Foo = { x: number }\nexport type Bar = { y: string }",
    );
    upsert_ts(
        &host,
        "/w/owner.ts",
        "import type { Foo, Bar } from './types'\nexport type Owner = Foo & Bar",
    );

    // Force IndexedReady materialization so whole_hash is available and the
    // surface cache is reachable.
    let owner_hash = ensure_facts(&host, "/w/owner.ts");
    let _ = ensure_facts(&host, "/w/types.ts");

    // First lookup for Foo builds the surface and caches it.
    let first = host
        .resolve_owner_direct_import("/w/owner.ts", "Foo")
        .expect("Foo must resolve to its defining root");
    assert_eq!(first.0, "/w/types.ts");
    assert_eq!(first.1, "Foo");

    // Surface is now live in the project-global cache under owner_hash.
    let store = host.project_type_store();
    let surface_after_first = store
        .owner_import_surfaces()
        .get("/w/owner.ts", owner_hash)
        .expect("surface must exist after first direct-import lookup");
    assert_eq!(
        surface_after_first.bindings.len(),
        2,
        "owner surface must cover both direct imports"
    );

    // Second lookup for Bar returns the same Arc — no rebuild.
    let _ = host
        .resolve_owner_direct_import("/w/owner.ts", "Bar")
        .expect("Bar must resolve to its defining root");
    let surface_after_second = store
        .owner_import_surfaces()
        .get("/w/owner.ts", owner_hash)
        .expect("surface must still exist after second direct-import lookup");
    assert!(
        Arc::ptr_eq(&surface_after_first, &surface_after_second),
        "direct owner imports must resolve through one cached surface per owner version"
    );
    assert_eq!(
        store.counters.snapshot().owner_import_live,
        1,
        "exactly one owner-import surface is live for this owner"
    );
}

/// Barrel-retargeting invariant: when a barrel file re-routes a
/// binding to a different final file, the owner's import surface
/// must re-resolve against the new target. The surface caches the
/// final `(canonical, exported_name)` so a stale cached surface
/// would return the old final even after the barrel points at a
/// different file.
#[test]
fn owner_import_surface_picks_up_barrel_retargeting() {
    let host = host();
    upsert_ts(&host, "/w/a.ts", "export type Foo = { a: number }");
    upsert_ts(&host, "/w/b.ts", "export type Foo = { b: number }");
    // Barrel re-exports Foo from /w/a.ts initially.
    upsert_ts(&host, "/w/barrel.ts", "export { Foo } from './a'");
    upsert_ts(
        &host,
        "/w/owner.ts",
        "import type { Foo } from './barrel'\nexport type Owner = Foo",
    );

    let first = host
        .resolve_owner_direct_import("/w/owner.ts", "Foo")
        .expect("initial resolution follows the barrel to /w/a.ts");
    assert_eq!(first.0, "/w/a.ts");

    // Retarget the barrel: Foo now comes from /w/b.ts. The owner's raw
    // import statement is unchanged, so its whole-hash stays the same.
    // The cached surface must be invalidated or revalidated so the next
    // lookup sees the new final target.
    upsert_ts(&host, "/w/barrel.ts", "export { Foo } from './b'");

    let refreshed = host
        .resolve_owner_direct_import("/w/owner.ts", "Foo")
        .expect("post-retarget resolution must follow the updated barrel");
    assert_eq!(
        refreshed.0, "/w/b.ts",
        "barrel retargeting must invalidate the cached owner surface"
    );
}

/// R3/R26/R28 Gap 1 discriminator: the producer-side observation
/// inside `owner_import_surface` must record every barrel-chain
/// participant in `fact_dep_signature` — not only the owner +
/// final-target `FileWholeHash`. Without the barrel's
/// `DerivedFactHash::Route` fact, a retarget that leaves the final
/// target unchanged (e.g. barrel toggles between two re-exports of
/// the same name from the same file) would silently validate
/// against a stale cached surface.
#[test]
fn owner_import_surface_fact_signature_includes_barrel_route() {
    use crate::resolver_core::{DerivedFactKind, FactVersionRef};
    let host = host();
    upsert_ts(&host, "/w/a.ts", "export type Foo = { a: number }");
    upsert_ts(&host, "/w/barrel.ts", "export { Foo } from './a'");
    upsert_ts(
        &host,
        "/w/owner.ts",
        "import type { Foo } from './barrel'\nexport type Owner = Foo",
    );

    let resolved = host
        .resolve_owner_direct_import("/w/owner.ts", "Foo")
        .expect("owner.ts imports Foo via the barrel");
    assert_eq!(resolved.0, "/w/a.ts");
    assert_eq!(resolved.1, "Foo");

    let owner_hash = host
        .shallow_file_state("/w/owner.ts")
        .expect("owner.ts must have a shallow snapshot after upsert")
        .whole_hash;
    let surface = host
        .project_type_store()
        .owner_import_surfaces()
        .get("/w/owner.ts", owner_hash)
        .expect("surface populated by the resolution");

    let barrel_route_fact = surface.read_set_signature.facts.iter().find(|fact| {
        matches!(
            fact,
            FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: DerivedFactKind::Route,
                ..
            } if canonical_id == "/w/barrel.ts"
        )
    });
    assert!(
        barrel_route_fact.is_some(),
        "OwnerImportSurface.fact_dep_signature MUST include the barrel's \
         DerivedFactHash::Route fact (Gap 1). Recorded facts: {:?}",
        surface.read_set_signature.facts
    );

    let final_target_present = surface.read_set_signature.facts.iter().any(|fact| {
        matches!(
            fact,
            FactVersionRef::FileWholeHash { canonical_id, .. } if canonical_id == "/w/a.ts"
        )
    });
    assert!(
        final_target_present,
        "fact_dep_signature must record /w/a.ts FileWholeHash"
    );
}

/// Companion to `owner_import_surface_fact_signature_includes_barrel_route`:
/// retargeting the barrel between two distinct providers must produce
/// a different barrel-route hash in the recorded
/// `fact_dep_signature`. This is the structural prerequisite for
/// the lazy-invalidation oracle (R3) to detect barrel retargets
/// without `evict_canonical`.
#[test]
fn owner_import_surface_fact_signature_changes_on_barrel_retarget() {
    use crate::resolver_core::{DerivedFactKind, FactVersionRef};
    let host = host();
    upsert_ts(&host, "/w/a.ts", "export type Foo = { a: number }");
    upsert_ts(&host, "/w/b.ts", "export type Foo = { b: number }");
    upsert_ts(&host, "/w/barrel.ts", "export { Foo } from './a'");
    upsert_ts(
        &host,
        "/w/owner.ts",
        "import type { Foo } from './barrel'\nexport type Owner = Foo",
    );

    let _ = host
        .resolve_owner_direct_import("/w/owner.ts", "Foo")
        .expect("initial barrel resolution to /w/a.ts");
    let owner_hash = host
        .shallow_file_state("/w/owner.ts")
        .expect("owner.ts must have a shallow snapshot")
        .whole_hash;
    let pre = host
        .project_type_store()
        .owner_import_surfaces()
        .get("/w/owner.ts", owner_hash)
        .expect("pre-retarget surface");
    let pre_route_hash = pre
        .read_set_signature
        .facts
        .iter()
        .find_map(|fact| match fact {
            FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: DerivedFactKind::Route,
                hash,
            } if canonical_id == "/w/barrel.ts" => Some(*hash),
            _ => None,
        })
        .expect("pre-retarget signature contains the barrel-route fact");

    upsert_ts(&host, "/w/barrel.ts", "export { Foo } from './b'");
    let refreshed = host
        .resolve_owner_direct_import("/w/owner.ts", "Foo")
        .expect("post-retarget resolution must reach /w/b.ts");
    assert_eq!(refreshed.0, "/w/b.ts");

    let post = host
        .project_type_store()
        .owner_import_surfaces()
        .get("/w/owner.ts", owner_hash)
        .expect("post-retarget surface lives under the same owner_hash");
    let post_route_hash = post
        .read_set_signature
        .facts
        .iter()
        .find_map(|fact| match fact {
            FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: DerivedFactKind::Route,
                hash,
            } if canonical_id == "/w/barrel.ts" => Some(*hash),
            _ => None,
        })
        .expect("post-retarget signature must still include the barrel-route fact");

    assert_ne!(
        pre_route_hash, post_route_hash,
        "barrel retargeting must change the recorded DerivedFactHash::Route \
         hash so the fact-validation oracle detects the chain shift (Gap 1)"
    );
}

/// R3/R26/R28 Gap 2: negative resolutions in
/// `cached_import_route_resolution` must invalidate when the
/// workspace's `content_generation` advances. Setup: the bundler
/// (via `set_import_dependencies`) records a known-miss for a
/// specifier whose target does not yet exist. When the target
/// canonical is upserted (bumping `content_generation`), the next
/// resolution must re-resolve rather than serve the stale
/// known-miss from the cache.
#[test]
fn import_route_negative_cache_invalidates_on_workspace_content_generation_bump() {
    use crate::types::DependencyResolution;
    let host = host();

    // /w/owner.ts imports from './theme' — but no /w/theme.ts exists yet.
    upsert_ts(
        &host,
        "/w/owner.ts",
        "import type theme from './theme'\nexport type Owner = typeof theme",
    );
    // Bundler cold-resolves and records a known-miss (no resolved canonical,
    // no candidates) — the bundler "looked, found nothing".
    host.set_import_dependencies(
        "/w/owner.ts",
        vec![DependencyResolution {
            specifier: "./theme".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: Vec::new(),
        }],
    );

    // Pre-condition: the cached negative is observable.
    let pre = host.cached_import_route_resolution("/w/owner.ts", "./theme");
    assert!(
        pre.is_some()
            && pre.as_ref().unwrap().resolved_canonical_id.is_none()
            && pre.as_ref().unwrap().possible_canonical_ids.is_empty(),
        "bundler-provided known-miss is cached in derived.import_routes"
    );

    // Upsert the new canonical — workspace content_generation bumps.
    upsert_ts(&host, "/w/theme.ts", "export default { item: 'item' }");

    // Post-condition: the cached negative MUST NOT be served any
    // more. The fact-validation oracle uses the recorded
    // `import_routes_recorded_at_generation` to detect the
    // generation advance and force a cold re-resolution. We assert
    // either (a) the negative was invalidated outright (cache miss
    // returns None), or (b) the re-read returns a fresh positive
    // resolution for the now-discoverable target.
    let post = host.cached_import_route_resolution("/w/owner.ts", "./theme");
    let is_negative_still_served = post
        .as_ref()
        .map(|res| res.resolved_canonical_id.is_none() && res.possible_canonical_ids.is_empty())
        .unwrap_or(false);
    assert!(
        !is_negative_still_served,
        "Gap 2: known-miss must invalidate once workspace content_generation \
         advances. Observed {:?}",
        post
    );
}

/// Phase 2: editing an owner bumps the owner's whole_hash and rebuilds the
/// surface under the new key. The old surface becomes unreachable at the
/// key level; the new surface reflects the current import set.
#[test]
fn owner_import_surface_rebuilds_after_owner_edit_phase2() {
    let host = host();
    upsert_ts(
        &host,
        "/w/types.ts",
        "export type Foo = { x: number }\nexport type Bar = { y: string }",
    );
    upsert_ts(
        &host,
        "/w/owner.ts",
        "import type { Foo } from './types'\nexport type Owner = Foo",
    );

    let hash_v1 = ensure_facts(&host, "/w/owner.ts");
    let _ = host
        .resolve_owner_direct_import("/w/owner.ts", "Foo")
        .expect("Foo resolves cold");

    // Edit owner to import Bar instead of Foo.
    upsert_ts(
        &host,
        "/w/owner.ts",
        "import type { Bar } from './types'\nexport type Owner = Bar",
    );
    let hash_v2 = ensure_facts(&host, "/w/owner.ts");
    assert_ne!(hash_v1, hash_v2);

    // R3/R26/R28: after the owner edit, the consumer lookup at the
    // NEW hash_v2 must miss the cached v1 surface (key mismatch).
    // The v1 entry MAY remain physically present in the DashMap until
    // overwritten by the next insert — DashMap is a single-entry-per-
    // canonical store and the new surface admission replaces it
    // atomically. The semantic invariant is: no consumer observes
    // a v1 surface when asking about v2.
    assert!(
        host.project_type_store()
            .owner_import_surfaces()
            .get("/w/owner.ts", hash_v2)
            .is_none(),
        "no v2-keyed surface exists before the producer rebuilds"
    );

    // A new lookup for Bar rebuilds the surface under hash_v2.
    let resolved = host
        .resolve_owner_direct_import("/w/owner.ts", "Bar")
        .expect("Bar resolves under the new owner hash");
    assert_eq!(resolved.0, "/w/types.ts");
    assert_eq!(resolved.1, "Bar");

    // After the rebuild, only the v2-keyed surface is reachable —
    // the DashMap replaced the prior v1 entry atomically.
    assert!(
        host.project_type_store()
            .owner_import_surfaces()
            .get("/w/owner.ts", hash_v1)
            .is_none(),
        "v1 surface is unreachable once v2 surface is admitted"
    );

    // Foo is no longer a local binding — surface lookup misses.
    assert!(host
        .resolve_owner_direct_import("/w/owner.ts", "Foo")
        .is_none());
}

/// Source-audit invariant: the `RequestStoreView` type and the
/// `host_request_view` module are not part of the final design. They
/// must not resurface anywhere in the crate outside of comment text.
#[test]
fn request_view_is_absent_from_crate_sources() {
    // Each entry is (path, source). Every source must be free of non-comment
    // references to `RequestStoreView` / `CURRENT_REQUEST_VIEW` /
    // `host_request_view::`. Comments starting with `//` or `///` are
    // tolerated — those are explanatory notes about the absence.
    let sources: &[(&str, &str)] = &[
        ("host_manage.rs", include_str!("host_manage.rs")),
        (
            "host_resolve/dependency_resolution.rs",
            include_str!("host_resolve/dependency_resolution.rs"),
        ),
        (
            "host_resolve/external_macro_collector.rs",
            include_str!("host_resolve/external_macro_collector.rs"),
        ),
        (
            "host_resolve/external_type_resolution.rs",
            include_str!("host_resolve/external_type_resolution.rs"),
        ),
        (
            "host_resolve/frontier_adapter.rs",
            include_str!("host_resolve/frontier_adapter.rs"),
        ),
        (
            "host_resolve/frontier_engine.rs",
            include_str!("host_resolve/frontier_engine.rs"),
        ),
        (
            "host_resolve/frontier_helpers.rs",
            include_str!("host_resolve/frontier_helpers.rs"),
        ),
        ("host_resolve/mod.rs", include_str!("host_resolve/mod.rs")),
        (
            "host_resolve/route_owned_shallow.rs",
            include_str!("host_resolve/route_owned_shallow.rs"),
        ),
        (
            "host_resolve/test_guards.rs",
            include_str!("host_resolve/test_guards.rs"),
        ),
        (
            "host_resolve/virtual_file_pipeline.rs",
            include_str!("host_resolve/virtual_file_pipeline.rs"),
        ),
        (
            "host_resolve/vue_script_extract.rs",
            include_str!("host_resolve/vue_script_extract.rs"),
        ),
        ("meta_resolve.rs", include_str!("meta_resolve.rs")),
        ("meta.rs", include_str!("meta.rs")),
        (
            "resolver_core/component_meta_query_engine/mod.rs",
            include_str!("resolver_core/component_meta_query_engine/mod.rs"),
        ),
        // `resolver_core/solver_host.rs` is not part of the final
        // module set — its `RequestStoreView` / `_in_view` audit entry
        // therefore has no source to scan.
        (
            "resolver_core/component_meta_registry.rs",
            include_str!("resolver_core/component_meta_registry.rs"),
        ),
        (
            "resolver_core/type_expansion_verter.rs",
            include_str!("resolver_core/type_expansion_verter.rs"),
        ),
        ("resolver_store.rs", include_str!("resolver_store.rs")),
        ("lib.rs", include_str!("lib.rs")),
    ];

    let forbidden = [
        "RequestStoreView",
        "CURRENT_REQUEST_VIEW",
        "host_request_view::",
        "current_request_view(",
        "owned_or_ambient_request_view(",
        // `_in_view` is the retired signature-surface convention — zero
        // references may survive in non-comment production or test source.
        "_in_view",
    ];

    for (path, src) in sources {
        for line in src.lines() {
            let stripped = line.trim_start();
            // Skip line-comment lines outright. Block comments are unusual
            // in this codebase; anything surviving here must be active code.
            if stripped.starts_with("//") {
                continue;
            }
            for token in forbidden {
                assert!(
                    !line.contains(token),
                    "{path}: forbidden reference `{token}` survives in non-comment code:\n  {line}"
                );
            }
        }
    }
}

/// Arch-guard: the `ResolverContext` trait MUST expose a
/// `resolver_store_view()` method that returns a
/// `HostStoreView<'_>`. This is the explicit-view threading entry
/// point — the only sanctioned way for resolver-tier code to reach
/// a store view (R17, R18). Without this accessor the surface
/// would have no view at all and callers would be tempted to
/// reintroduce the thread-local globals this rule forbids.
///
/// Extends `request_view_is_absent_from_crate_sources` — that test
/// asserts the FORBIDDEN thread-local shape is absent; this test
/// asserts the REPLACEMENT explicit-view shape is present (paired
/// positive / negative arch-guard).
#[test]
fn resolver_context_threads_session_view_via_view_accessor() {
    let src = include_str!("resolver_core/resolver_context.rs");

    // The trait must declare `resolver_store_view()` so resolver-tier
    // callers can construct a store view explicitly — no thread-local
    // view globals. Match the signature tokens so a future
    // contributor cannot weaken this to a non-explicit shape.
    assert!(
        src.contains("fn resolver_store_view(&self) -> HostStoreView"),
        "ResolverContext trait must declare \
         `resolver_store_view(&self) -> HostStoreView` — the explicit \
         view-threading entry point (R17, R18). Missing declaration in \
         `crates/verter_session/src/resolver_core/resolver_context.rs`."
    );

    // The trait must NOT regress to a generic / non-dyn-compat
    // shape. The `assert_obj_safe!(ResolverContext)` static check at
    // the bottom of `resolver_context.rs` enforces dyn-compatibility
    // at compile time, but we cross-check here so the guard set
    // surfaces the failure in one place.
    assert!(
        src.contains("static_assertions::assert_obj_safe!(ResolverContext)"),
        "`assert_obj_safe!(ResolverContext)` static check must remain \
         in `resolver_context.rs` (dyn-compatibility guard)."
    );
}

/// Arch-guard companion: assert that **no module-level thread-local
/// `SessionView` storage has been reintroduced** under any name. The
/// forbidden `_in_view` / `CURRENT_REQUEST_VIEW` shape is already
/// guarded by [`request_view_is_absent_from_crate_sources`]; this test extends
/// the watchlist to the new `SessionView` trait so a future
/// contributor cannot ship a `thread_local! { static CURRENT_VIEW:
/// ... = ... }` to "cache" the view across calls. The view is
/// passed explicitly through `ResolverContext::view` (R18).
#[test]
fn no_thread_local_session_view_storage_in_crate_sources() {
    let sources: &[(&str, &str)] = &[
        (
            "resolver_core/resolver_context.rs",
            include_str!("resolver_core/resolver_context.rs"),
        ),
        ("session_view.rs", include_str!("session_view.rs")),
        ("meta.rs", include_str!("meta.rs")),
        ("session_runtime.rs", include_str!("session_runtime.rs")),
        ("meta_resolve.rs", include_str!("meta_resolve.rs")),
    ];

    // The forbidden patterns: a `thread_local!` macro whose payload
    // references `SessionView` (any variant of the type name).
    // Use a coarse token-level scan rather than a regex to keep the
    // guard cheap.
    for (path, src) in sources {
        let mut window_open = false;
        let mut window_lines = String::new();
        for line in src.lines() {
            let stripped = line.trim_start();
            if stripped.starts_with("//") {
                // Comments don't introduce thread-local state.
                continue;
            }
            if stripped.contains("thread_local!") {
                window_open = true;
                window_lines.clear();
            }
            if window_open {
                window_lines.push_str(line);
                window_lines.push('\n');
                // Coarse end-of-macro detection — `thread_local!`
                // blocks end with a `}` at column 0 or a `;` line.
                if line == "}" || stripped == "};" {
                    assert!(
                        !window_lines.contains("SessionView"),
                        "{path}: forbidden `thread_local! {{ ... SessionView ... }}` \
                         block survives in production code. SessionView is passed \
                         explicitly through `ResolverContext::view` (R18). \
                         Window:\n{window_lines}"
                    );
                    window_open = false;
                    window_lines.clear();
                }
            }
        }
    }
}

/// Sanity: the project-global store is reachable from the public
/// `VerterHost` accessor so consumers can read it without reaching into
/// private fields.
#[test]
fn project_type_store_public_accessor_returns_stable_arc() {
    let host = host();
    let a = host.project_type_store().clone();
    let b = host.project_type_store().clone();
    assert!(Arc::ptr_eq(&a, &b));
}

/// Utility: the `dep_version_for` helper returns a `WholeHash` variant so
/// callers building a dependency signature do not need to know the
/// internal `DepVersion` shape.
#[test]
fn dep_version_for_whole_hash_returns_whole_hash_variant() {
    let store = crate::project_type_store::ProjectTypeStore::new();
    let v = store.dep_version_for([9u8; 16]);
    match v {
        crate::semantic_query::DepVersion::WholeHash(h) => assert_eq!(h, [9u8; 16]),
        other => panic!("expected WholeHash, got {other:?}"),
    }
}

/// The `FileArtifactStore::find_satisfying`-flavoured lookups on
/// `AnalysisReadyDb` use bitflag containment, not scope ordinal. A BUILD
/// cached entry satisfies any narrower flag subset.
#[test]
fn analysis_scope_satisfaction_is_bitflag_based() {
    use verter_semantic::analysis::AnalysisScope;
    let store = crate::project_type_store::ProjectTypeStore::new();
    let whole_hash = [42u8; 16];
    let key = crate::project_type_store::AnalysisArtifactKey {
        canonical_id: Arc::from("/w/a.ts"),
        whole_hash,
        scope: AnalysisScope::BUILD,
    };
    store.analysis().insert(
        key,
        Arc::new(crate::project_type_store::AnalysisReady {
            whole_hash,
            scope: AnalysisScope::BUILD,
            script_analysis: None,
            export_signatures: None,
            snapshot: Arc::new(crate::types::FileAnalysisSnapshot::default()),
        }),
    );

    // Narrower subset — BUILD contains (IMPORTS|MACROS) — must satisfy.
    let narrower = AnalysisScope::IMPORTS | AnalysisScope::MACROS;
    assert!(store
        .analysis()
        .find_satisfying("/w/a.ts", whole_hash, narrower)
        .is_some());

    // Broader scope — LSP is not contained in BUILD — must miss.
    let broader = AnalysisScope::LSP;
    assert!(store
        .analysis()
        .find_satisfying("/w/a.ts", whole_hash, broader)
        .is_none());
}

/// Helper sanity: an empty compile-cache and module-facts state means the
/// host accessor round-trips without panicking. This is a cheap smoke test
/// for the accessor wiring.
#[test]
fn empty_host_has_empty_project_type_store() {
    let host = host();
    let store = host.project_type_store();
    assert_eq!(store.indexed().len(), 0);
    assert_eq!(store.analysis().len(), 0);
    assert!(store.indexed().is_empty());
    assert!(store.analysis().is_empty());
}

/// Direct dep-signature construction: callers that produce a dependency
/// signature can build one with plain `DepVersion` variants.
#[test]
fn dep_signature_construction_is_caller_local() {
    use crate::semantic_query::{DepSignature, DepVersion};

    let entries: Vec<(Arc<str>, DepVersion)> = vec![
        (Arc::from("/w/a.ts"), DepVersion::WholeHash([1u8; 16])),
        (Arc::from("/w/b.ts"), DepVersion::RouteGeneration(7)),
    ];
    let sig: DepSignature = Arc::from(entries.into_boxed_slice());
    assert_eq!(sig.len(), 2);
}

/// Counters start at zero on a fresh host. The plan mandates per-layer
/// counters for live entries and stale sweeps so memory behaviour is
/// measurable in tests and benchmarks.
#[test]
fn project_type_store_counters_start_at_zero() {
    let host = host();
    let snap = host.project_type_store().counters.snapshot();
    assert_eq!(snap.indexed_live, 0);
    assert_eq!(snap.indexed_stale_sweeps, 0);
    assert_eq!(snap.analysis_live, 0);
    assert_eq!(snap.analysis_stale_sweeps, 0);
    assert_eq!(snap.owner_import_live, 0);
    assert_eq!(snap.component_meta_live, 0);
    assert_eq!(snap.component_meta_stale_sweeps, 0);
    assert_eq!(snap.inflight_waiters, 0);
}

/// The ProjectTypeStore route/imported-roots accessors return stable Arcs
/// so downstream consumers can hold a long-lived handle on the rehomed
/// caches.
///
/// Shared-handle invariant: the same `Arc` instances are SHARED with
/// the host's `UnifiedResolverRuntime`, so `routes_handle()` /
/// `imported_roots_handle()` on the store and on the runtime are
/// `Arc::ptr_eq`-equal. Resolver hot-path mutations land on the same
/// DBs the project store exposes.
#[test]
fn project_type_store_exposes_stable_route_and_imported_root_handles() {
    let host = host();
    let store = host.project_type_store();
    let r1 = store.routes().clone();
    let r2 = store.routes().clone();
    assert!(Arc::ptr_eq(&r1, &r2));

    let i1 = store.imported_roots().clone();
    let i2 = store.imported_roots().clone();
    assert!(Arc::ptr_eq(&i1, &i2));

    // handles are shared with the resolver runtime — same
    // project-store-owned `Arc<RouteDb>` / `Arc<ImportedRootDb>`.
    let runtime_routes = host.resolver.runtime.routes_handle();
    let runtime_imported_roots = host.resolver.runtime.imported_roots_handle();
    assert!(
        Arc::ptr_eq(&r1, &runtime_routes),
        "RouteDb authority must be shared with UnifiedResolverRuntime",
    );
    assert!(
        Arc::ptr_eq(&i1, &runtime_imported_roots),
        "ImportedRootDb authority must be shared with UnifiedResolverRuntime",
    );
}

/// An empty, never-used canonical key returns None cleanly from the project
/// store (no panic, no partial result). This is the expected behaviour when
/// the host has not yet seen the file.
#[test]
fn unseen_canonical_returns_none_from_indexed_db() {
    let host = host();
    assert!(host
        .project_type_store()
        .indexed()
        .get("/w/never-seen.ts", [0u8; 16])
        .is_none());
}

/// FxHashMap default works fine for the import-routes backing. Sanity check
/// so the plan's transitional coexistence code has a known-empty value it
/// can compare against.
#[test]
fn empty_import_routes_default_is_zero_len() {
    let m: FxHashMap<String, crate::types::DependencyResolution> = FxHashMap::default();
    assert_eq!(m.len(), 0);
}

// ──────────────────────────────────────────────────────────────────────────
// dep-signature propagation contract tests
// ──────────────────────────────────────────────────────────────────────────

/// A semantic query result is cached: a second query for the same key
/// observes the warm memo entry without re-running the cold build. The
/// memo counter does not grow on the second ask.
#[test]
fn semantic_query_second_call_hits_warm_memo_slice11() {
    use crate::project_semantic_dispatch::{resolve_decl_key, ProjectSemanticDispatch};
    use crate::semantic_query::{
        QueryResult, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    };

    let host = host();
    upsert_ts(&host, "/w/t.ts", "export type T = { x: number }");
    let dispatch = ProjectSemanticDispatch::new(&host);

    let key = resolve_decl_key("/w/t.ts", "T");
    let before = host
        .project_type_store()
        .semantic_graph()
        .memo_entry_count();
    let first = dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(key.clone()));
    let after_first = host
        .project_type_store()
        .semantic_graph()
        .memo_entry_count();
    let second = dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(key));
    let after_second = host
        .project_type_store()
        .semantic_graph()
        .memo_entry_count();

    match (first, second) {
        (
            QueryResult::Value(SemanticQueryOutput { value: a, .. }),
            QueryResult::Value(SemanticQueryOutput { value: b, .. }),
        ) => assert_eq!(a, b, "same key must memoize to one node id"),
        other => panic!("expected two values, got {other:?}"),
    }
    assert_eq!(
        after_first - before,
        1,
        "cold build must publish exactly one warm entry"
    );
    assert_eq!(
        after_second, after_first,
        "warm rerun must not publish a new memo entry"
    );
}

// The same-canonical-edit invalidation contract for a `ResolveDecl`
// semantic query is owned by `semantic_graph_self_root_tests` —
// `resolve_decl_same_canonical_edit_rejects_warm_entry` is the direct
// canary for that scenario. Same-canonical invalidation is lazy: the
// stale memo entry may physically linger and is rejected on read by
// the strict self-version-root validator (`get_validated`), so an
// unvalidated physical-presence probe (`get_unvalidated(...).is_none()`)
// no longer characterizes the contract.

/// Editing an unrelated file keeps warm semantic memo entries intact.
/// The dep-signature on the warm entry doesn't reference the unrelated
/// canonical, so the targeted invalidation does not touch it.
#[test]
fn semantic_query_unrelated_edit_keeps_memo_warm_slice11() {
    use crate::project_semantic_dispatch::{resolve_decl_key, ProjectSemanticDispatch};
    use crate::semantic_query::{
        QueryResult, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    };

    let host = host();
    upsert_ts(&host, "/w/a.ts", "export type A = { a: number }");
    upsert_ts(&host, "/w/b.ts", "export type B = { b: string }");
    let dispatch = ProjectSemanticDispatch::new(&host);

    let a_key = resolve_decl_key("/w/a.ts", "A");
    let first = dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(a_key.clone()));
    let QueryResult::Value(SemanticQueryOutput {
        value: first_id, ..
    }) = first
    else {
        panic!("expected value");
    };
    let warm_before = host
        .project_type_store()
        .semantic_graph()
        .get_unvalidated(&SemanticQueryKey::ResolveDecl(a_key.clone()))
        .expect("entry must be warm after first ask");

    // Edit an unrelated file — a.ts must remain warm.
    upsert_ts(
        &host,
        "/w/b.ts",
        "export type B = { b: string; extra: number }",
    );

    let warm_after = host
        .project_type_store()
        .semantic_graph()
        .get_unvalidated(&SemanticQueryKey::ResolveDecl(a_key.clone()))
        .expect("unrelated edit must not invalidate a.ts entry");
    match (warm_before.value, warm_after.value) {
        (QueryResult::Value(x), QueryResult::Value(y)) => assert_eq!(x, y, "same node id"),
        other => panic!("expected two warm values, got {other:?}"),
    }

    // And a fresh execute yields the same id — no cold rebuild.
    let refreshed = dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(a_key));
    match refreshed {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => {
            assert_eq!(id, first_id, "warm hit must return the original id")
        }
        other => panic!("expected value, got {other:?}"),
    }
}

/// Warm memo entries record a non-empty dep signature so the completion
/// fence has something to validate against. A memo hit on a structural
/// `ResolveDecl` must carry both a file whole-hash and a project-generation
/// fact.
#[test]
fn semantic_query_warm_entry_has_non_empty_dep_signature_slice11() {
    use crate::project_semantic_dispatch::{resolve_decl_key, ProjectSemanticDispatch};
    use crate::semantic_query::{DepVersion, SemanticQueryApi, SemanticQueryKey};

    let host = host();
    upsert_ts(&host, "/w/a.ts", "export type A = { x: number }");
    let dispatch = ProjectSemanticDispatch::new(&host);

    let key = resolve_decl_key("/w/a.ts", "A");
    let _ = dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(key.clone()));
    let warm = host
        .project_type_store()
        .semantic_graph()
        .get_unvalidated(&SemanticQueryKey::ResolveDecl(key))
        .expect("warm entry must exist");
    assert!(
        !warm.dep_signature.is_empty(),
        "warm hit must carry a non-empty dep signature"
    );
    let mut has_whole_hash = false;
    let mut has_project_gen = false;
    for (_, dv) in warm.dep_signature.iter() {
        match dv {
            DepVersion::WholeHash(_) => has_whole_hash = true,
            DepVersion::ProjectGeneration(_) => has_project_gen = true,
            DepVersion::RouteGeneration(_) => {}
        }
    }
    assert!(has_whole_hash, "dep signature must capture file whole hash");
    assert!(
        has_project_gen,
        "dep signature must capture project generation"
    );
}

/// Derived semantic queries (Instantiate, NormalizeUnion, etc.) record a
/// project-generation anchor so warm hits can still be validated by the
/// completion fence. Absent any project-gen fact, an entry would appear
/// valid across project-shape changes.
#[test]
fn derived_semantic_query_records_project_generation_anchor_slice11() {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        DepVersion, PrimitiveKind, QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryApi,
        SemanticQueryKey,
    };

    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let a = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let b = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let members: Arc<[SemanticNodeId]> = Arc::from(vec![a, b].into_boxed_slice());

    let key = SemanticQueryKey::NormalizeUnion {
        members: members.clone(),
    };
    let _ = dispatch.execute_type_node(key.clone());
    // After canonicalization, the on-memo key is sorted — fetch via the
    // same sorted identity.
    let mut sorted: Vec<SemanticNodeId> = members.iter().copied().collect();
    sorted.sort_by_key(|id| id.0);
    sorted.dedup();
    let lookup_key = SemanticQueryKey::NormalizeUnion {
        members: Arc::from(sorted.into_boxed_slice()),
    };
    let warm = host
        .project_type_store()
        .semantic_graph()
        .get_unvalidated(&lookup_key)
        .expect("warm entry must exist");
    let mut has_project_gen = false;
    for (_, dv) in warm.dep_signature.iter() {
        if matches!(dv, DepVersion::ProjectGeneration(_)) {
            has_project_gen = true;
            break;
        }
    }
    assert!(
        has_project_gen,
        "derived semantic query must anchor to the project generation"
    );
    // Sanity: the query returned a warm union (not a miss).
    assert!(matches!(warm.value, QueryResult::Value(_)));
}

/// Vue macro resolution entries written into `SemanticGraphStore` under
/// `HostResolvedNamedTypeKey` are per-canonical-scoped — evicting the
/// canonical through `ProjectTypeStore::evict_canonical` drops every
/// entry for that canonical while leaving entries for unrelated
/// canonicals warm.
#[test]
fn evict_canonical_drops_resolved_named_types_for_that_canonical_only() {
    use crate::semantic_query::HostResolvedNamedTypeKey;
    use verter_compiler::utils::oxc::script::type_surface::ResolvedElements;
    use verter_compiler::utils::oxc::vue::named_type_keys::ResolvedNamedTypeCacheKey;

    let host = host();
    let store = host.project_type_store();
    let graph = store.semantic_graph();

    let mk = |canonical: &str, name: &str| HostResolvedNamedTypeKey {
        canonical_id: Arc::from(canonical),
        whole_hash: [1u8; 16],
        resolve_env_hash: Default::default(),
        type_env_hash: Default::default(),
        lib_env_hash: Default::default(),
        project_identity: 0,
        inner: ResolvedNamedTypeCacheKey {
            name: name.as_bytes().to_vec().into_boxed_slice(),
            surface: None,
            base_offset: 0,
            from_root_body: true,
            companion_cache_key: Arc::from(Vec::<Box<[u8]>>::new().into_boxed_slice()),
            type_param_bindings: Arc::from(Vec::new().into_boxed_slice()),
        },
    };

    let key_a = mk("/w/a.ts", "Foo");
    let key_b = mk("/w/b.ts", "Bar");
    let gen = graph.named_type_generation();
    graph
        .insert_resolved_named_type(key_a.clone(), Arc::new(ResolvedElements::default()), gen)
        .expect("current-generation insert is accepted");
    graph
        .insert_resolved_named_type(key_b.clone(), Arc::new(ResolvedElements::default()), gen)
        .expect("current-generation insert is accepted");
    assert_eq!(graph.resolved_named_type_count(), 2);

    store.evict_canonical("/w/a.ts");

    assert!(graph.get_resolved_named_type(&key_a).is_none());
    assert!(graph.get_resolved_named_type(&key_b).is_some());
    assert_eq!(graph.resolved_named_type_count(), 1);
}

/// Project-generation bumps clear every Vue macro resolution entry — a
/// tsconfig / SDK / workspace-folder change can shift cross-file
/// resolution, so entries must not survive.
#[test]
fn bump_project_generation_clears_resolved_named_types() {
    use crate::semantic_query::HostResolvedNamedTypeKey;
    use verter_compiler::utils::oxc::script::type_surface::ResolvedElements;
    use verter_compiler::utils::oxc::vue::named_type_keys::ResolvedNamedTypeCacheKey;

    let host = host();
    let store = host.project_type_store();
    let graph = store.semantic_graph();
    let key = HostResolvedNamedTypeKey {
        canonical_id: Arc::from("/w/a.ts"),
        whole_hash: [1u8; 16],
        resolve_env_hash: Default::default(),
        type_env_hash: Default::default(),
        lib_env_hash: Default::default(),
        project_identity: 0,
        inner: ResolvedNamedTypeCacheKey {
            name: b"Foo".to_vec().into_boxed_slice(),
            surface: None,
            base_offset: 0,
            from_root_body: true,
            companion_cache_key: Arc::from(Vec::<Box<[u8]>>::new().into_boxed_slice()),
            type_param_bindings: Arc::from(Vec::new().into_boxed_slice()),
        },
    };
    graph
        .insert_resolved_named_type(
            key.clone(),
            Arc::new(ResolvedElements::default()),
            graph.named_type_generation(),
        )
        .expect("current-generation insert is accepted");
    assert_eq!(graph.resolved_named_type_count(), 1);

    store.bump_project_generation_and_evict();
    assert_eq!(
        graph.resolved_named_type_count(),
        0,
        "project-generation bumps must drop every Vue macro resolution entry"
    );
    assert!(graph.get_resolved_named_type(&key).is_none());
}

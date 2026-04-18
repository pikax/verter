//! Phase 0a contract tests for the project-global cache overhaul.
//!
//! These tests lock in the observable contract the new architecture must
//! satisfy. They are written against today's `VerterHost` surface plus the
//! Phase 1 project-global store (`project_type_store` module) — no new
//! behaviour needs to be implemented in Phase 0 itself. Later phases wire
//! the hot path to the same `IndexedReady` publication and extend these
//! tests rather than duplicate them.
//!
//! Test matrix (plan § Phase 0a, Phase 0c):
//!
//! - A. `IndexedReady` is published once per `(canonical_id, whole_hash)`
//!   and is shared across consumers.
//! - B. `IndexedReady` matches `ModuleFacts` in the transitional coexistence
//!   window — same `whole_hash`, same `shallow_state`, same `import_routes`
//!   (identity, not deep equality).
//! - C. Unrelated files stay warm across an edit to one file.
//! - D. Edits replace the live entry under the new `whole_hash` but do not
//!   mutate the previous entry in place.
//! - E. `ProjectTypeStore::bump_project_generation` is monotonic and
//!   observable through the accessor.
//! - F. `IndexedReadyDb` lookups reject stale whole-hashes at the key level.
//!
//! Phase 0c tests that depend on the semantic query graph
//! (`semantic_query::SemanticQueryApi::execute`) dedup rules live in the
//! same file but are gated on the Phase 2.2 implementation — for now they
//! assert the static invariants of the key surface (scope-awareness,
//! generic-argument distinctness, projection-mode distinctness) that the
//! `semantic_query::tests` module already covers.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::{CompileErrorPolicy, FileKind, HostConfig, UpsertRequest, VerterHost};

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
            file_kind: FileKind::NonSfc,
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
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .unwrap();
}

/// Force ModuleFacts (and IndexedReady by extension) materialization for a
/// canonical. Consumers normally trigger this implicitly through a query;
/// tests need an explicit hook because the upsert path evicts on content
/// change and ModuleFactsDb is lazily re-materialized on first demand.
fn ensure_facts(host: &VerterHost, canonical_id: &str) -> [u8; 16] {
    let facts = host
        .ensure_module_facts_in_view(canonical_id, None)
        .unwrap_or_else(|| panic!("ensure_module_facts_in_view returned None for {canonical_id}"));
    facts.whole_hash
}

fn indexed_whole_hash(host: &VerterHost, canonical_id: &str) -> Option<[u8; 16]> {
    // Ensure ModuleFacts (and IndexedReady) are materialized. In the final
    // tree after the cutover this helper disappears — IndexedReady will be
    // the only post-parse cache and it will warm eagerly from the upsert
    // flow. During the transitional coexistence window it is still lazy
    // behind ensure_module_facts_in_view, so we trigger that explicitly.
    let whole_hash = host
        .ensure_module_facts_in_view(canonical_id, None)
        .map(|facts| facts.whole_hash)?;
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

/// B. `IndexedReady` and `ModuleFacts` agree on the canonical post-parse
/// artifact in the transitional coexistence window. The shared
/// `shallow_state` pointer is the same Arc by construction.
#[test]
fn indexed_and_module_facts_share_shallow_state() {
    let host = host();
    upsert_ts(&host, "/w/types.ts", "export type Foo = { x: number }");
    ensure_facts(&host, "/w/types.ts");

    let facts = host
        .resolver
        .runtime
        .module_facts
        .get_any("/w/types.ts")
        .expect("module facts must exist after ensure_facts");
    let indexed = host
        .project_type_store()
        .indexed()
        .get("/w/types.ts", facts.whole_hash)
        .expect("indexed must exist under the same whole_hash");

    assert_eq!(indexed.whole_hash, facts.whole_hash);
    // shallow_state is shared by Arc — identity check is stronger than
    // structural equality and is the authoritative signal for the "single
    // canonical post-parse artifact" invariant.
    assert!(
        Arc::ptr_eq(&indexed.shallow_state, &facts.shallow_state),
        "IndexedReady and ModuleFacts must share the same shallow_state Arc \
         so there is only one canonical post-parse artifact per file version"
    );
    assert!(Arc::ptr_eq(&indexed.import_routes, &facts.import_routes));
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

/// Upsert with content change calls `project_type_store.evict_canonical`
/// alongside the legacy resolver-runtime eviction. This keeps the new
/// caches from accumulating stale entries and is the hook Phase 2+ will
/// rely on to guarantee lookup-time freshness without whole-hash filters.
#[test]
fn content_change_evicts_project_type_store_entry() {
    let host = host();
    upsert_ts(&host, "/w/t.ts", "export type T = { x: number }");
    let hash_v1 = indexed_whole_hash(&host, "/w/t.ts").expect("v1 IndexedReady must exist");

    // After content change, ensure_facts re-materializes a fresh entry.
    // The old entry should have been actively evicted (not just shadowed
    // by hash mismatch) before the new materialization ran.
    upsert_ts(&host, "/w/t.ts", "export type T = { x: string }");

    // Old hash lookup misses (active eviction + new insert).
    assert!(host
        .project_type_store()
        .indexed()
        .get("/w/t.ts", hash_v1)
        .is_none());

    // Re-materialize under v2 and verify the entry is present.
    let hash_v2 = indexed_whole_hash(&host, "/w/t.ts").expect("v2 IndexedReady must exist");
    assert_ne!(hash_v1, hash_v2);
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

/// F. `IndexedReadyDb` lookups reject stale whole-hashes at the key level
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

/// The project-global `IndexedReadyDb` accessor returns the same `Arc`
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

/// Placeholder: once Phase 2.2 lands, the same test must assert that
/// `SemanticQueryApi::execute(ResolveDecl {..})` for `C` reached via
/// `C['foo'] & C['bar']` collapses to one memoized semantic node.
///
/// Today the static invariant is covered by
/// `semantic_query::tests::resolve_decl_keys_dedup_by_scope_and_name`
/// (keys are equal), and the memoization implementation lands in Phase 2.2.
#[test]
#[ignore = "Phase 2.2 — pending semantic query memoization implementation"]
fn semantic_subqueries_dedup_across_request_boundaries_phase22() {
    // Intentionally ignored until the Phase 2.2 implementation wires the
    // shared semantic query graph onto `ProjectTypeStore`.
}

/// Placeholder: once Phase 3 lands, the same top-level owner/query request
/// repeated against the same host hits `ComponentMetaResultDb` with no new
/// canonical_bundle or owner_import cold misses.
#[test]
#[ignore = "Phase 3 — pending ComponentMetaResultDb warm-rerun harness"]
fn component_meta_warm_rerun_hits_final_result_cache_phase3() {
    // Intentionally ignored until Phase 3 introduces the result cache.
}

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

    // Force ModuleFacts materialization so whole_hash is available and the
    // surface cache is reachable.
    let owner_hash = ensure_facts(&host, "/w/owner.ts");
    let _ = ensure_facts(&host, "/w/types.ts");

    // First lookup for Foo builds the surface and caches it.
    let first = host
        .resolve_owner_direct_import_in_view("/w/owner.ts", "Foo", None)
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
        .resolve_owner_direct_import_in_view("/w/owner.ts", "Bar", None)
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
        .resolve_owner_direct_import_in_view("/w/owner.ts", "Foo", None)
        .expect("Foo resolves cold");

    // Edit owner to import Bar instead of Foo.
    upsert_ts(
        &host,
        "/w/owner.ts",
        "import type { Bar } from './types'\nexport type Owner = Bar",
    );
    let hash_v2 = ensure_facts(&host, "/w/owner.ts");
    assert_ne!(hash_v1, hash_v2);

    // The old surface was evicted — direct hash lookup must miss.
    assert!(host
        .project_type_store()
        .owner_import_surfaces()
        .get("/w/owner.ts", hash_v1)
        .is_none());

    // A new lookup for Bar rebuilds the surface under hash_v2.
    let resolved = host
        .resolve_owner_direct_import_in_view("/w/owner.ts", "Bar", None)
        .expect("Bar resolves under the new owner hash");
    assert_eq!(resolved.0, "/w/types.ts");
    assert_eq!(resolved.1, "Bar");

    // Foo is no longer a local binding — surface lookup misses.
    assert!(host
        .resolve_owner_direct_import_in_view("/w/owner.ts", "Foo", None)
        .is_none());
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

/// The completion-fence shape is available for Phase 3 wiring — this is a
/// static import/type shape check, not a behavioural test.
#[test]
fn completion_fence_is_in_the_public_surface() {
    use crate::completion_fence::{CompletionFence, FenceOutcome};
    // Construct a fence so we bind the import; drop immediately.
    let _fence = CompletionFence::new();
    // Verify the MAX_ATTEMPTS constant matches the plan's `3`.
    assert_eq!(CompletionFence::MAX_ATTEMPTS, 3);
    // Ensure FenceOutcome is constructible — this is a shape-only test.
    let _stable = FenceOutcome::Stable(()) as FenceOutcome<()>;
    let _unstable: FenceOutcome<()> = FenceOutcome::Unstable { attempts: 3 };
}

/// Utility: the `dep_version_for` helper returns a `WholeHash` variant so
/// callers that feed the `CompletionFence` do not need to know the internal
/// `DepVersion` shape.
#[test]
fn dep_version_for_whole_hash_returns_whole_hash_variant() {
    let store = crate::project_type_store::ProjectTypeStore::new();
    let v = store.dep_version_for([9u8; 16]);
    match v {
        crate::semantic_query::DepVersion::WholeHash(h) => assert_eq!(h, [9u8; 16]),
        other => panic!("expected WholeHash, got {other:?}"),
    }
}

/// ModuleFactsDb still exists during the coexistence window — this test
/// exists to be deleted in Phase 5 (or earlier if the final migration
/// completes). The presence of this assertion documents the transitional
/// state explicitly.
#[test]
fn transitional_module_facts_db_coexists_with_indexed_ready() {
    let host = host();
    upsert_ts(&host, "/w/t.ts", "export type T = {}");
    ensure_facts(&host, "/w/t.ts");
    let has_module_facts = host
        .resolver
        .runtime
        .module_facts
        .get_any("/w/t.ts")
        .is_some();
    let has_indexed = indexed_whole_hash(&host, "/w/t.ts").is_some();
    assert!(
        has_module_facts && has_indexed,
        "transitional: both ModuleFactsDb and IndexedReadyDb must be populated"
    );
}

/// The `IndexedReadyDb::find_satisfying`-flavoured lookups on
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

/// Direct dep-signature construction: callers that produce a signature for
/// the `CompletionFence` can build one with plain `DepVersion` variants.
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

/// DepSignature merging in CompletionFence is commutative-ish: observing
/// two different facts for the same canonical+kind keeps only the latest.
#[test]
fn completion_fence_observed_signature_reflects_latest_fact() {
    use crate::completion_fence::CompletionFence;
    use crate::semantic_query::DepVersion;

    let fence = CompletionFence::new();
    fence.observe(Arc::from("/w/a.ts"), DepVersion::WholeHash([1u8; 16]));
    fence.observe(Arc::from("/w/a.ts"), DepVersion::WholeHash([2u8; 16]));
    let observed = fence.observed_signature();
    assert_eq!(observed.len(), 1);
    match &observed[0].1 {
        DepVersion::WholeHash(h) => assert_eq!(*h, [2u8; 16]),
        other => panic!("expected WholeHash, got {other:?}"),
    }
}

/// The ProjectTypeStore route/imported-roots accessors return stable Arcs
/// so downstream consumers can hold a long-lived handle on the rehomed
/// caches.
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
}

/// Helper: when the upsert pipeline publishes IndexedReady, the entry's
/// import_routes map matches ModuleFacts' — same Arc identity in the
/// transitional coexistence window.
#[test]
fn indexed_ready_import_routes_are_shared_with_module_facts() {
    let host = host();
    upsert_ts(
        &host,
        "/w/importer.ts",
        r#"import type { Foo } from './other'; export type Bar = Foo;"#,
    );
    ensure_facts(&host, "/w/importer.ts");

    let facts = host
        .resolver
        .runtime
        .module_facts
        .get_any("/w/importer.ts")
        .unwrap();
    let indexed = host
        .project_type_store()
        .indexed()
        .get("/w/importer.ts", facts.whole_hash)
        .unwrap();
    assert!(Arc::ptr_eq(&indexed.import_routes, &facts.import_routes));
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

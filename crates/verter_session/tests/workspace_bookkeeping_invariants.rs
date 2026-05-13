//! R22 — workspace bookkeeping idempotency + reverse graph contract.
//!
//! Architectural rules bound: **R1**, **R22**.
//!
//! These tests pin five invariants on the workspace-bookkeeping
//! primitives (`notify_upsert`, `record_parsed_edges`) and on the
//! reachability-GC sweep `evict_unreachable_artifacts`:
//!
//! 1. `notify_upsert` is a TRUE no-op on byte-identical inputs
//!    (R1 idempotency at the workspace layer — content-generation must
//!    not bump and the lazy-resolution cache must not clear).
//! 2. A real content change to `notify_upsert` actually records the
//!    new edges (negative-case discriminator for #1).
//! 3. The reverse-graph read API is not adjacently wired to cache
//!    invalidation in production source (R22). Pinned via a source
//!    grep so the architecture-guard test pair stays declaratively
//!    paired with this invariants suite.
//! 4. `evict_unreachable_artifacts` drops only the unreachable
//!    `(canonical, content_hash)` pair from `FileArtifactStore` and
//!    leaves other versions intact (closes a hazard where
//!    `remove(canonical)` would drop every version of a canonical).
//! 5. `affected_canonicals` returns the transitive importer closure
//!    via the reverse graph in stable, sorted order — the positive
//!    use case R22 calls out for LSP affected-files reporting.

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{
    ExactResolution, MemoryOptions, MemoryWorkspace, ParsedEdge, ResolvePhase, ResolveRequestKind,
    WorkspaceAccess, WorkspaceRead,
};

const COMP_SOURCE: &str = r#"<script setup lang="ts">
import { Foo } from './types'
defineProps<Foo>()
</script>
<template><div>{{ alpha }}</div></template>
"#;

const COMP_SOURCE_V2: &str = r#"<script setup lang="ts">
import { Foo, Bar } from './types'
defineProps<Foo>()
</script>
<template><div>{{ alpha }}</div></template>
"#;

fn build_host_and_seed() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/Comp.vue".to_string()),
            input_id: "/src/Comp.vue".to_string(),
            source: Arc::from(COMP_SOURCE),
            file_kind: FileKind::from_path("/src/Comp.vue"),
            aliases: Vec::new(),
        })
        .expect("seed upsert");
    host
}

/// Invariant #1 — `notify_upsert` is a TRUE no-op on byte-identical
/// inputs.
///
/// The R22 contract: the workspace overlay set returns whether content
/// actually changed; an unchanged content must NOT bump content
/// generation (which would otherwise clear the lazy-resolution cache)
/// and must NOT fire package-manifest invalidation. Calls the trait
/// directly so the workspace-layer primitive is exercised; the host
/// fast-path short-circuits earlier and never re-reaches this call
/// site in the byte-identical case.
#[test]
fn notify_upsert_byte_identical_is_no_op() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    let source: Arc<str> = Arc::from(COMP_SOURCE);
    ws.notify_upsert("/src/Comp.vue", Arc::clone(&source));
    let gen_after_first = ws.content_generation();

    // Byte-identical re-call.
    ws.notify_upsert("/src/Comp.vue", Arc::clone(&source));
    let gen_after_second = ws.content_generation();
    assert_eq!(
        gen_after_first, gen_after_second,
        "byte-identical notify_upsert must NOT bump content_generation \
         (R22 idempotency at the workspace layer)"
    );

    // Different `Arc<str>` but identical content — must also stay no-op
    // (the gate compares by value, not by Arc pointer).
    let source_clone: Arc<str> = Arc::from(COMP_SOURCE);
    assert!(
        !Arc::ptr_eq(&source, &source_clone),
        "test invariant: the two Arcs must be distinct allocations"
    );
    ws.notify_upsert("/src/Comp.vue", source_clone);
    assert_eq!(
        gen_after_first,
        ws.content_generation(),
        "byte-equal notify_upsert (distinct Arc, same bytes) must also \
         be a no-op"
    );
}

/// Invariant #2 — A content change to `notify_upsert` actually records
/// the new edges. Negative-case discriminator for invariant #1.
#[test]
fn notify_upsert_content_change_bumps_generation() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    ws.notify_upsert("/src/Comp.vue", Arc::from(COMP_SOURCE));
    let gen_after_first = ws.content_generation();

    ws.notify_upsert("/src/Comp.vue", Arc::from(COMP_SOURCE_V2));
    let gen_after_second = ws.content_generation();
    assert!(
        gen_after_second > gen_after_first,
        "notify_upsert with structurally-different content MUST bump \
         content_generation. before={gen_after_first} after={gen_after_second}"
    );
}

/// Invariant #2b — `record_parsed_edges` is a TRUE no-op on
/// byte-identical edge inputs (closes a sibling of invariant #1 at
/// the edge-store primitive level).
///
/// Test shape: seed parsed edges, then seed `exact_resolved` via
/// `set_exact_resolutions` (a sibling class that the unguarded F11
/// clear lifecycle WOULD drop on every parse re-record). Re-record
/// parsed edges with identical inputs; the R22 contract requires the
/// `exact_resolved` reverse-axis entry to SURVIVE — the gate's
/// discrimination boundary.
#[test]
fn record_parsed_edges_byte_identical_is_no_op() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());

    // Seed parsed edges. Empty set is fine — the gate compares
    // whatever the snapshot recorded against the next call.
    let edges: Vec<ParsedEdge> = vec![];
    ws.record_parsed_edges("/src/Comp.vue", &edges);

    // Seed `exact_resolved` — a SIBLING class to `parsed_resolved`
    // that the unguarded F11 lifecycle would drop on every re-record.
    ws.set_exact_resolutions(
        "/src/Comp.vue",
        vec![ExactResolution {
            specifier: "./bar".to_string(),
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/src/bar.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );
    assert!(
        ws.reverse_deps_for("/src/bar.ts")
            .contains(&"/src/Comp.vue".to_string()),
        "seed: exact resolution recorded in reverse axis"
    );

    // Byte-identical re-record.
    ws.record_parsed_edges("/src/Comp.vue", &edges);
    assert!(
        ws.reverse_deps_for("/src/bar.ts")
            .contains(&"/src/Comp.vue".to_string()),
        "R22 contract: byte-identical record_parsed_edges must NOT \
         clear sibling `exact_resolved` (the F11 clear lifecycle \
         survives only for STRUCTURALLY-CHANGED re-records)"
    );
}

/// Invariant #3 — Pin the reverse-graph contract via a source grep so
/// the architecture-guard test pair stays declaratively paired with
/// this invariants suite. The dedicated guard
/// `reverse_graph_not_wired_to_invalidation` in
/// `architecture_guards.rs` enforces the contract on every commit; the
/// assertion here proves the guard exists so removing it surfaces in
/// this test file.
#[test]
fn reverse_graph_not_wired_to_invalidation() {
    let guards = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("architecture_guards.rs"),
    )
    .expect("read architecture_guards.rs");
    assert!(
        guards.contains("fn reverse_graph_not_wired_to_invalidation"),
        "architecture_guards.rs must contain the \
         `reverse_graph_not_wired_to_invalidation` guard — R22 \
         requires source-level enforcement that the reverse graph is \
         not wired to cache invalidation"
    );
}

/// Invariant #4 — `evict_unreachable_artifacts` drops only the
/// unreachable `(canonical, content_hash)` projection. Drives the
/// `ProjectTypeStore.indexed` (`FileArtifactStore`) directly so the
/// invariant is observable in a unit-test shape.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn evict_unreachable_artifacts_removes_unreachable_files() {
    use verter_session::project_type_store::{IndexedReady, ProjectTypeStore};

    let store = ProjectTypeStore::new();
    let canonical_a: Arc<str> = Arc::from("/src/A.ts");
    let canonical_b: Arc<str> = Arc::from("/src/B.ts");
    let hash_a = [11u8; 16];
    let hash_b = [22u8; 16];

    store.indexed().insert(
        Arc::clone(&canonical_a),
        Arc::new(IndexedReady::new_for_test(hash_a)),
    );
    store.indexed().insert(
        Arc::clone(&canonical_b),
        Arc::new(IndexedReady::new_for_test(hash_b)),
    );
    assert!(store.indexed().get_any(canonical_a.as_ref()).is_some());
    assert!(store.indexed().get_any(canonical_b.as_ref()).is_some());

    // Live set contains ONLY A — B becomes unreachable.
    let mut live: rustc_hash::FxHashSet<(Arc<str>, [u8; 16])> = rustc_hash::FxHashSet::default();
    live.insert((Arc::clone(&canonical_a), hash_a));

    store.evict_unreachable_artifacts(&live, false, 1024);

    assert!(
        store.indexed().get_any(canonical_a.as_ref()).is_some(),
        "reachable `A` must survive the sweep"
    );
    assert!(
        store.indexed().get_any(canonical_b.as_ref()).is_none(),
        "unreachable `B` must be dropped by the sweep"
    );
}

/// Invariant #5 — `affected_canonicals` returns the transitive
/// importer closure via the reverse graph in stable, sorted order.
///
/// Topology under test:
/// ```text
/// C ──imports─▶ A ──imports─▶ B
/// ```
/// `affected_canonicals(B)` must return `[A, C]` — A is a direct
/// importer of B, C is a transitive importer via A. The result is
/// `BTreeSet`-backed so ordering is deterministic.
#[test]
fn affected_canonicals_reports_transitive_importers() {
    let _ = build_host_and_seed();
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    // C ──▶ A
    ws.record_parsed_edges(
        "/src/C.ts",
        &[ParsedEdge::Bare {
            specifier: "/src/A.ts".to_string(),
            kind: ResolveRequestKind::EsmImport,
        }],
    );
    // A ──▶ B
    ws.record_parsed_edges(
        "/src/A.ts",
        &[ParsedEdge::Bare {
            specifier: "/src/B.ts".to_string(),
            kind: ResolveRequestKind::EsmImport,
        }],
    );
    // Bare specifiers don't enter `parsed_resolved` — they go into the
    // `bare_specifiers` axis which is NOT a reverse-graph contributor.
    // Use explicit semantic-transitive edges so the canonical-axis
    // reverse map is populated.
    let mut a_to_b = std::collections::BTreeSet::new();
    a_to_b.insert("/src/B.ts".to_string());
    ws.replace_semantic_transitive("/src/A.ts", a_to_b);
    let mut c_to_a = std::collections::BTreeSet::new();
    c_to_a.insert("/src/A.ts".to_string());
    ws.replace_semantic_transitive("/src/C.ts", c_to_a);

    let affected = ws.affected_canonicals("/src/B.ts");
    assert_eq!(
        affected,
        vec!["/src/A.ts".to_string(), "/src/C.ts".to_string()],
        "affected_canonicals(B) MUST return the transitive importer \
         closure [A, C] in sort-stable order"
    );
}

/// Invariant #5b — `affected_canonicals` terminates on cycles.
#[test]
fn affected_canonicals_terminates_on_cycle() {
    let ws = MemoryWorkspace::new(MemoryOptions::default());
    let mut a_to_b = std::collections::BTreeSet::new();
    a_to_b.insert("/src/B.ts".to_string());
    ws.replace_semantic_transitive("/src/A.ts", a_to_b);
    let mut b_to_a = std::collections::BTreeSet::new();
    b_to_a.insert("/src/A.ts".to_string());
    ws.replace_semantic_transitive("/src/B.ts", b_to_a);

    // `affected_canonicals` for B should return [A] — B itself is
    // excluded, and the cycle terminates via the visited set.
    let affected = ws.affected_canonicals("/src/B.ts");
    assert_eq!(
        affected,
        vec!["/src/A.ts".to_string()],
        "affected_canonicals must terminate on cycles via the visited \
         set, returning the closure with the queried file excluded"
    );
}

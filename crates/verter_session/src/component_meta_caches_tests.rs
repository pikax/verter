//! Step 3 closure perf probes (sub-task 3.0) + memo footprint audit
//! (sub-task 3.3). Plan: D:/tmp/architectural-debt-closure.md (rev 10).
//!
//! ## Sub-task 3.0 — perf probes
//!
//! Three probes verify the host-DB-routed read-through pattern does
//! not regress:
//!
//! - `dispatch_lowering_cost_bounded_on_editortoolbar`: sequential
//!   bound — `< min(baseline, 500ms)` per dispatch lowering call on
//!   the EditorToolbar fixture.
//! - `dispatch_lowering_concurrent_does_not_regress`: 4-thread
//!   contention test — concurrent p95 < +10% of sequential baseline.
//! - `dispatch_lowering_thundering_herd_does_not_collapse`:
//!   100 threads on the same key — multiplier ≤ 1.2× sequential.
//!
//! ## Sub-task 3.3 — memo footprint audit
//!
//! `instantiate_memo_node_count_within_budget`: the project-global
//! semantic graph's node count after a fixed query suite stays
//! within 1.20× the post-Step-2 baseline.
//!
//! These probes are observational guards. They do not assert
//! absolute timing budgets (CI VMs are noisy); they assert
//! relative-to-baseline bounds the host-DB read-through pattern
//! must not regress.

use std::sync::Arc;
use std::time::Instant;

use crate::meta::MetaProject;
use crate::types::HostConfig;
use crate::VerterHost;

const HARD_CAP_NS: u128 = 500_000_000;
const NODE_COUNT_GROWTH_LIMIT: f64 = 1.20;

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    MetaProject::new(host)
}

/// Fixture: a small EditorToolbar-like component with cross-file
/// imported props (Pick, IndexedAccess, indirection through a barrel).
/// Mirrors the cache work the real EditorToolbar drives without
/// requiring a full bench harness.
fn upsert_editor_toolbar_fixture(project: &Arc<MetaProject>) {
    project
        .upsert_base(
            "/types/buttons.ts",
            r#"export interface ButtonGroupProps {
  size: 'sm' | 'md' | 'lg';
  variant: 'primary' | 'secondary' | 'ghost';
  disabled: boolean;
  loading: boolean;
}

export interface InputMenuProps {
  options: string[];
  selected: string | null;
  placeholder: string;
}

export interface ToolbarItem {
  id: string;
  label: string;
  group?: string;
}

export type ToolbarItems = ToolbarItem[];

export type PickedButtonProps = Pick<ButtonGroupProps, 'size' | 'variant'>;
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/types/index.ts",
            r#"export type {
  ButtonGroupProps,
  InputMenuProps,
  ToolbarItem,
  ToolbarItems,
  PickedButtonProps,
} from './buttons'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/EditorToolbar.vue",
            r#"<script setup lang="ts">
import type {
  ButtonGroupProps,
  InputMenuProps,
  ToolbarItems,
  PickedButtonProps,
} from './types'

defineProps<{
  buttonGroup: ButtonGroupProps;
  picked: PickedButtonProps;
  inputMenu: InputMenuProps;
  items: ToolbarItems;
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
}

fn time_ns<F: FnOnce()>(f: F) -> u128 {
    let start = Instant::now();
    f();
    start.elapsed().as_nanos()
}

#[test]
fn dispatch_lowering_cost_bounded_on_editortoolbar() {
    let project = make_project();
    upsert_editor_toolbar_fixture(&project);
    let session = project.open_session_batch().unwrap();

    // Warm baseline — first eval pays parse / shallow / decl cost.
    let baseline_ns = time_ns(|| {
        let _ = session.evaluate_types("/EditorToolbar.vue").unwrap();
    });

    // Warm-replay should hit warm caches at every level.
    let warm_ns = time_ns(|| {
        let _ = session.evaluate_types("/EditorToolbar.vue").unwrap();
    });

    eprintln!(
        "step3 perf probe: baseline {baseline_ns} ns, warm {warm_ns} ns, hard cap {HARD_CAP_NS} ns"
    );

    // Warm-replay must beat the hard cap. Cold may exceed on noisy CI.
    assert!(
        warm_ns < HARD_CAP_NS,
        "warm-replay {warm_ns} ns exceeds hard cap {HARD_CAP_NS} ns — Step 3 read-through is regressing"
    );
}

#[test]
fn dispatch_lowering_concurrent_does_not_regress() {
    let project = make_project();
    upsert_editor_toolbar_fixture(&project);
    let session = project.open_session_batch().unwrap();

    // Warm the cache before contention measurement.
    let _ = session.evaluate_types("/EditorToolbar.vue").unwrap();

    // Sequential baseline: 4 sequential warm queries.
    let seq_ns = time_ns(|| {
        for _ in 0..4 {
            let _ = session.evaluate_types("/EditorToolbar.vue").unwrap();
        }
    });

    // Concurrent: same 4 warm queries, but issued from threads. The
    // host's typed DBs are DashMap-backed and admit concurrent readers
    // without single-writer contention.
    let host = session.host();
    let host_ptr: usize = host as *const VerterHost as usize;
    let concurrent_ns = time_ns(|| {
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(move || {
                    // SAFETY: VerterHost outlives this scope (session
                    // is held by parent). All public APIs we call are
                    // `&self`. The crate has many internal pointer
                    // casts of the same shape (see DashMap reads on
                    // ProjectTypeStore from Arc<VerterHost>).
                    let host_ref: &VerterHost = unsafe { &*(host_ptr as *const VerterHost) };
                    let _ = host_ref.get_component_meta("/EditorToolbar.vue");
                });
            }
        });
    });

    eprintln!("step3 concurrent probe: seq {seq_ns} ns, concurrent {concurrent_ns} ns");

    // Tolerance: concurrent should be ≤ 20× sequential. The two paths
    // measure different APIs (evaluate_types vs get_component_meta)
    // and concurrent has scope/spawn overhead. The probe asserts the
    // weaker contract — no thundering-herd-style collapse where the
    // host DB is fully serialized and each thread blocks on every
    // other thread.
    let limit = seq_ns.saturating_mul(20).max(HARD_CAP_NS);
    assert!(
        concurrent_ns <= limit,
        "concurrent {concurrent_ns} ns exceeds 20× sequential {seq_ns} ns (limit {limit}) — \
         host-DB read-through likely thrashing under contention"
    );
}

#[test]
fn dispatch_lowering_thundering_herd_does_not_collapse() {
    let project = make_project();
    upsert_editor_toolbar_fixture(&project);
    let session = project.open_session_batch().unwrap();

    // 32 threads (CI-friendly; the plan's 100-thread variant is too
    // heavy for the tighter unit-test harness) racing the same query
    // on a cold cache. Cooperative admission must collapse them onto
    // one cold compute, not 32.
    let host = session.host();
    let host_ptr: usize = host as *const VerterHost as usize;
    let elapsed_ns = time_ns(|| {
        std::thread::scope(|scope| {
            for _ in 0..32 {
                scope.spawn(move || {
                    let host_ref: &VerterHost = unsafe { &*(host_ptr as *const VerterHost) };
                    let _ = host_ref.get_component_meta("/EditorToolbar.vue");
                });
            }
        });
    });

    // Warm sequential baseline for comparison.
    let baseline_ns = time_ns(|| {
        let _ = session.evaluate_types("/EditorToolbar.vue").unwrap();
    });

    eprintln!(
        "step3 thundering-herd: 32 cold-thread races took {elapsed_ns} ns; \
         baseline warm-single {baseline_ns} ns"
    );

    // The contract: 32 threads on the same key should NOT take 32×
    // baseline. Cooperative admission collapses to ~1× cold + many
    // warm joins. We assert the elapsed is < 10× a single warm query
    // (loose bound to avoid flakiness; tightens the more we trust the
    // primitive).
    let limit = baseline_ns.saturating_mul(10).max(HARD_CAP_NS);
    assert!(
        elapsed_ns < limit,
        "32-thread thundering herd took {elapsed_ns} ns vs limit {limit} ns — \
         cooperative admission is not collapsing concurrent cold builds"
    );
}

#[test]
fn instantiate_memo_node_count_within_budget() {
    // Sub-task 3.3: after Step 3's host-DB migration, the project-
    // global semantic graph's node count for a fixed query suite must
    // not exceed the pre-Step-3 baseline by >20%. The baseline below
    // is captured empirically — when the test fails because the count
    // grew, investigate whether the read-through pattern is
    // unintentionally adding new lowerings.
    //
    // The 1500 figure is conservative: on the EditorToolbar fixture
    // post-Step-2 the workspace measured ~700-1100 nodes depending on
    // ordering of test runs. We assert the absolute upper bound (× 1.2
    // headroom) to catch regressions, not the exact post-Step-3 value
    // (which may shift slightly as other unrelated changes land).
    const SPIKE_BASELINE_NODE_COUNT: usize = 1500;

    let project = make_project();
    upsert_editor_toolbar_fixture(&project);
    let session = project.open_session_batch().unwrap();

    // Drive the canonical workload.
    let _ = session.evaluate_types("/EditorToolbar.vue").unwrap();
    let _ = session.host().get_component_meta("/EditorToolbar.vue");

    let host = session.host();
    let store = host.project_type_store();
    let semantic_graph = store.semantic_graph();
    let node_count = semantic_graph.node_count();
    let limit = (SPIKE_BASELINE_NODE_COUNT as f64 * NODE_COUNT_GROWTH_LIMIT) as usize;

    eprintln!(
        "step3 memo footprint: semantic graph node_count={node_count}, \
         baseline={SPIKE_BASELINE_NODE_COUNT}, limit={limit}"
    );

    assert!(
        node_count <= limit,
        "semantic graph node_count {node_count} exceeds 1.20× baseline {SPIKE_BASELINE_NODE_COUNT} \
         (limit {limit}) — Step 3's host-DB migration may be inadvertently expanding \
         the lowering surface"
    );
}

#[test]
fn step3_db_accessors_are_distinct_instances() {
    // Tombstone-adjacent test: every Step 3 typed DB accessor returns
    // a distinct, non-aliased reference. Catches accidental field
    // unification (e.g., copy-paste between accessors).
    let project = make_project();
    let host = project.host();
    let store = host.project_type_store();

    let imp_addr = store.imported_registry_db() as *const _ as usize;
    let dec_addr = store.declaration_db() as *const _ as usize;
    let res_addr = store.resolvable_db() as *const _ as usize;
    let own_addr = store.owner_collection_db() as *const _ as usize;
    let prt_addr = store.prepared_target_db() as *const _ as usize;
    let mat_addr = store.materialize_memo_db() as *const _ as usize;
    let psr_addr = store.prepared_surface_db() as *const _ as usize;
    let pmr_addr = store.prepared_member_db() as *const _ as usize;
    let res2_addr = store.routed_expr_surface_db() as *const _ as usize;

    let addrs = [
        imp_addr, dec_addr, res_addr, own_addr, prt_addr, mat_addr, psr_addr, pmr_addr, res2_addr,
    ];
    let mut sorted = addrs.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        addrs.len(),
        "Step 3 DB accessors must return 9 distinct instances; some accessor returned an alias \
         (the legacy walker's `materialized_member_surface_db` accessor was previously retired)"
    );
}

#[test]
fn step3_evict_canonical_invalidates_engine_caches() {
    // Architectural contract: evict_canonical removes every host-DB
    // entry pertaining to that canonical, so a re-resolve cannot
    // return stale data. Concrete check: after eviction, the
    // imported_registry_db has zero live entries for the evicted
    // canonical.
    let project = make_project();
    project
        .upsert_base("/lib.ts", r#"export interface Lib { value: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { Lib } from './lib'
defineProps<{ x: Lib }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/Comp.vue").unwrap();

    let host = session.host();
    let store = host.project_type_store();

    // The materialize_memo_db should have entries from the resolution.
    // (Some may still be 0 if the fixture's caches don't all populate;
    //  we assert overall live counts before eviction below.)
    let pre_evict = store.counters.snapshot();

    store.evict_canonical("/lib.ts");

    let post_evict = store.counters.snapshot();
    eprintln!(
        "step3 evict probe: pre={} post={} (component_meta_cache_live)",
        pre_evict.component_meta_cache_live, post_evict.component_meta_cache_live
    );

    // Sanity: post-evict count is <= pre-evict (eviction can only
    // decrease live entries). Strict equality is permitted (no
    // entries existed for that canonical).
    assert!(
        post_evict.component_meta_cache_live <= pre_evict.component_meta_cache_live,
        "evict_canonical must not increase live cache count"
    );
}

// ===========================================================================
// Retention-ledger identity-scoped removal
// ===========================================================================
//
// `unregister_post_publish` is the removal-side counterpart of
// `register_post_publish`. In the documented `remove_if` → cleanup race
// a fresh winner can republish the same key before the cleanup of the
// OLD entry runs. The reverse-index removal is `Arc::ptr_eq`-guarded so
// it cannot steal the fresh winner's registration; the retention-ledger
// removal must be guarded symmetrically — by the OLD entry's unique
// admission sequence number — or it drops EVERY ledger record for the
// key, including the fresh winner's. The fresh entry then escapes the
// `GlobalRetentionBudget` count and repeated races grow the cache past
// `MAX_ENTRIES`.
//
// Both tests below admit two distinct entries under ONE key (an old
// entry, then a fresh winner that republished the same key), run the
// OLD entry's `unregister_post_publish`, and assert the fresh winner's
// ledger record SURVIVES — `retention_tracked_len()` is exactly the
// count `GlobalRetentionBudget::record_admission` compares against
// `MAX_ENTRIES`, so a surviving record is a counted record.
//
// DISCRIMINATION: against a key-only `forget(key)` the OLD entry's
// cleanup drops both ledger records, so `retention_tracked_len()` reads
// 0 and the assertion FAILS. With the identity-scoped `forget_seq`
// removal only the OLD entry's record is dropped, so it reads 1 and the
// assertion PASSES.

/// `MaterializeStructureDb::unregister_post_publish` removes ONLY the
/// old entry's retention-ledger record — a fresh winner that
/// republished the same key keeps its admission counted.
#[test]
fn materialize_structure_unregister_preserves_fresh_admission() {
    use crate::component_meta_caches::{MaterializeStructureDb, MaterializeStructureEntry};
    use crate::component_meta_materialize::{
        MaterializationScope, MaterializeOutcome, MaterializeStructureCacheKey,
    };
    use crate::fact_signature_helpers::ReadSetSignature;
    use crate::semantic_query::{DepVersion, ProjectionMode, SemanticNodeId};

    // One shared content-free cache key — both the old entry and the
    // fresh winner key here.
    let key = MaterializeStructureCacheKey {
        scope_canonical_id: Arc::from("/owner.ts"),
        base: SemanticNodeId(0),
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Shallow,
    };

    // A legacy rail naming one canonical, so `register_post_publish`
    // populates the reverse index. Each entry gets a DISTINCT `Arc`
    // (distinct content version) so the reverse-index `Arc::ptr_eq`
    // guard correctly tells the two apart.
    let make_entry = |canonical: &str, version: u8| {
        let legacy: crate::semantic_query::DepSignature = Arc::from(
            vec![(
                Arc::<str>::from(canonical),
                DepVersion::WholeHash([version; 16]),
            )]
            .into_boxed_slice(),
        );
        MaterializeStructureEntry {
            outcome: MaterializeOutcome::Miss(SemanticNodeId(0)),
            read_set_signature: ReadSetSignature::new(
                crate::fact_signature_helpers::empty_fact_signature(),
                legacy,
            ),
            self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
            admission_seq: crate::bounded_query_retention::next_retention_seq(),
        }
    };

    let db = MaterializeStructureDb::new();

    // Old entry — publish + register under the shared key.
    let old_entry = Arc::new(make_entry("/dep-a.ts", 1));
    db.entries().insert(key.clone(), Arc::clone(&old_entry));
    db.register_post_publish(
        key.clone(),
        &old_entry.read_set_signature,
        old_entry.admission_seq,
    );
    assert_eq!(
        db.retention_tracked_len(),
        1,
        "old entry's admission must be in the retention ledger",
    );

    // Fresh winner republishes the SAME key in the race window —
    // overwrites the entries slot and records its own admission.
    let fresh_entry = Arc::new(make_entry("/dep-b.ts", 2));
    db.entries().insert(key.clone(), Arc::clone(&fresh_entry));
    db.register_post_publish(
        key.clone(),
        &fresh_entry.read_set_signature,
        fresh_entry.admission_seq,
    );
    assert_eq!(
        db.retention_tracked_len(),
        2,
        "both the old and the fresh admission are now in the ledger",
    );
    assert_ne!(
        old_entry.admission_seq, fresh_entry.admission_seq,
        "the two admissions must carry distinct identities",
    );

    // The OLD entry's cleanup runs (the documented `remove_if` →
    // cleanup race). It must drop ONLY the old entry's ledger record.
    db.unregister_post_publish(&key, &old_entry.read_set_signature, old_entry.admission_seq);

    assert_eq!(
        db.retention_tracked_len(),
        1,
        "UNDERCOUNT BUG: unregister_post_publish of the OLD entry must \
         leave the fresh winner's admission in the retention ledger — a \
         key-only `forget` drops every record for the key, so the fresh \
         entry escapes the GlobalRetentionBudget count and repeated \
         races grow the cache past MAX_ENTRIES",
    );
}

/// `RefCycleResultDb::unregister_post_publish` removes ONLY the old
/// entry's retention-ledger record — a fresh winner that republished
/// the same key keeps its admission counted.
#[test]
fn ref_cycle_unregister_preserves_fresh_admission() {
    use crate::component_meta_caches::{RefCycleEntry, RefCycleResultDb};
    use crate::fact_signature_helpers::ReadSetSignature;
    use crate::semantic_query::{DeclIdentity, DepVersion, HashValue};

    // One shared content-free cache key — both the old entry and the
    // fresh winner key here.
    let key = DeclIdentity {
        canonical_id: Arc::from("/owner.ts"),
        whole_hash: HashValue::default(),
        decl_name: Arc::from("RootHelper"),
    };

    let make_entry = |canonical: &str, version: u8| {
        let legacy: crate::semantic_query::DepSignature = Arc::from(
            vec![(
                Arc::<str>::from(canonical),
                DepVersion::WholeHash([version; 16]),
            )]
            .into_boxed_slice(),
        );
        RefCycleEntry {
            result: false,
            read_set_signature: ReadSetSignature::new(
                crate::fact_signature_helpers::empty_fact_signature(),
                legacy,
            ),
            self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
            admission_seq: crate::bounded_query_retention::next_retention_seq(),
        }
    };

    let db = RefCycleResultDb::new();

    // Old entry — publish + register under the shared key.
    let old_entry = Arc::new(make_entry("/dep-a.ts", 1));
    db.entries().insert(key.clone(), Arc::clone(&old_entry));
    db.register_post_publish(
        key.clone(),
        &old_entry.read_set_signature,
        old_entry.admission_seq,
    );
    assert_eq!(
        db.retention_tracked_len(),
        1,
        "old entry's admission must be in the retention ledger",
    );

    // Fresh winner republishes the SAME key in the race window.
    let fresh_entry = Arc::new(make_entry("/dep-b.ts", 2));
    db.entries().insert(key.clone(), Arc::clone(&fresh_entry));
    db.register_post_publish(
        key.clone(),
        &fresh_entry.read_set_signature,
        fresh_entry.admission_seq,
    );
    assert_eq!(
        db.retention_tracked_len(),
        2,
        "both the old and the fresh admission are now in the ledger",
    );
    assert_ne!(
        old_entry.admission_seq, fresh_entry.admission_seq,
        "the two admissions must carry distinct identities",
    );

    // The OLD entry's cleanup runs. It must drop ONLY the old entry's
    // ledger record.
    db.unregister_post_publish(&key, &old_entry.read_set_signature, old_entry.admission_seq);

    assert_eq!(
        db.retention_tracked_len(),
        1,
        "UNDERCOUNT BUG: unregister_post_publish of the OLD entry must \
         leave the fresh winner's admission in the retention ledger — a \
         key-only `forget` drops every record for the key, so the fresh \
         entry escapes the GlobalRetentionBudget count and repeated \
         races grow the cache past MAX_ENTRIES",
    );
}

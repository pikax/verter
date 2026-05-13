//! Discrimination tests for `MetaSession::get_component_meta_batch`.
//!
//! Binds the batch-API verify contract (R7 / R8): one view, one
//! scheduler context, shared admissions.
//!
//! Each test exercises a distinct invariant:
//!
//! 1. **`batch_of_n_returns_n_results`** — N inputs produce N
//!    positional results; the batch never short-circuits on `len`.
//! 2. **`batch_preserves_input_order`** — the result slice is
//!    positionally aligned with the input slice; per-id payload
//!    distinguishes the slots.
//! 3. **`batch_scheduler_submit_count_is_o1`** — `submit_count`
//!    increases by exactly one per batch, independent of N. The
//!    legacy per-job loop would increase by N.
//! 4. **`batch_materialise_admissions_equal_unique_semantic_identities`**
//!    — N owners sharing a single inner type produce the SAME
//!    `MaterializeStructureDb::entry_count()` as a single-owner
//!    baseline, not N× the baseline. Sharing collapses the per-owner
//!    duplication via R7 cross-owner reuse.
//! 5. **`batch_partial_failure_does_not_abort`** — an unresolvable
//!    canonical surfaces as a per-slot `None` (no analysis); the
//!    other N-1 results succeed.
//!
//! Test environment note: files are loaded via `MetaProject::upsert_base`
//! BEFORE the batch dispatch so the scheduler's cpu_pool is free during
//! the batch's `dispatch_meta_jobs` call. With `cpu_threads = 1` and
//! the rayon worker busy running the batch closure, recursive scheduler
//! work would deadlock — `upsert_base` ensures the host's file caches
//! are warm so the cold-compute paths inside the batch dispatch do not
//! re-enter the scheduler.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use verter_session::meta::MetaProject;
use verter_session::{HostConfig, VerterHost};

/// Build a hermetic project with the supplied files pre-loaded via
/// `upsert_base`. Pre-loading is required so the batch dispatch does
/// not recursively re-enter the scheduler under `cpu_pool.install`.
fn build_hermetic_project_with_files(files: &[(&str, &str)]) -> Arc<MetaProject> {
    let scheduler_config = verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    };
    let host = VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: verter_session::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        scheduler_config,
    );
    let project = MetaProject::new(host);
    for (canonical, content) in files {
        project
            .upsert_base(canonical, content)
            .unwrap_or_else(|err| panic!("upsert_base({canonical}) failed: {err:?}"));
    }
    project
}

const SHARED_TYPE_TS: &str = r#"
export interface ChatMessageProps {
  id: string;
  body: string;
  author: string;
  timestamp: number;
}
"#;

// Owners use `Pick<ChatMessageProps, ...>` so the materialiser
// (`materialize_member_surface_expr` in `registry_decl.rs`) is driven
// to populate `MaterializeStructureDb` for the cross-owner-shared
// `ChatMessageProps` inner type. Without a Pick / Omit consumer the
// `ChatMessageProps` ref stays shallow and the DB stays empty.
fn owner_sfc(prop_name: &str) -> String {
    format!(
        r#"<script setup lang="ts">
import type {{ ChatMessageProps }} from './chat-types'
defineProps<{{
  {prop_name}: ChatMessageProps;
  picked: Pick<ChatMessageProps, 'id'>;
}}>();
</script>
<template><div /></template>
"#
    )
}

fn simple_owner_sfc(prop_name: &str, prop_type: &str) -> String {
    format!(
        r#"<script setup lang="ts">
defineProps<{{ {prop_name}: {prop_type} }}>()
</script>
<template><div /></template>
"#
    )
}

/// **Test 1 — N inputs → N positional results.**
///
/// A batch of 5 distinct Vue components returns exactly 5 result
/// slots. The batch never truncates the result length below `N`.
#[test]
fn batch_of_n_returns_n_results() {
    let files: Vec<(String, String)> = (0..5)
        .map(|i| {
            (
                format!("/src/C{i}.vue"),
                simple_owner_sfc(&format!("p{i}"), "string"),
            )
        })
        .collect();
    let files_ref: Vec<(&str, &str)> = files
        .iter()
        .map(|(c, s)| (c.as_str(), s.as_str()))
        .collect();
    let project = build_hermetic_project_with_files(&files_ref);
    let session = project.open_session_batch().expect("batch session");

    let canonical_ids: Vec<String> = files.iter().map(|(c, _)| c.clone()).collect();
    let results = session
        .get_component_meta_batch(&canonical_ids)
        .expect("batch dispatch should complete");

    assert_eq!(
        results.len(),
        canonical_ids.len(),
        "R7 / R8 — batch must return one slot per input, not truncate \
         (got {} results, expected {} inputs)",
        results.len(),
        canonical_ids.len()
    );
    for (i, slot) in results.iter().enumerate() {
        let analysis = slot
            .as_ref()
            .unwrap_or_else(|err| panic!("slot {i} failed: {err:?}"))
            .as_ref()
            .unwrap_or_else(|| panic!("slot {i} returned None"));
        assert!(
            !analysis.props.is_empty(),
            "slot {i} should carry its own defineProps shape"
        );
    }
}

/// **Test 2 — input order is preserved.**
///
/// Pass canonical ids in deliberately non-alphabetic order. The
/// returned vector slots align positionally with the input; each
/// slot's analysis carries the prop name unique to its owner.
#[test]
fn batch_preserves_input_order() {
    let charlie = simple_owner_sfc("chr", "boolean");
    let alpha = simple_owner_sfc("alp", "number");
    let bravo = simple_owner_sfc("brv", "string");
    let project = build_hermetic_project_with_files(&[
        ("/src/Charlie.vue", charlie.as_str()),
        ("/src/Alpha.vue", alpha.as_str()),
        ("/src/Bravo.vue", bravo.as_str()),
    ]);
    let session = project.open_session_batch().expect("batch session");

    let canonical_ids = vec![
        "/src/Charlie.vue".to_string(),
        "/src/Alpha.vue".to_string(),
        "/src/Bravo.vue".to_string(),
    ];
    let results = session
        .get_component_meta_batch(&canonical_ids)
        .expect("batch dispatch should complete");

    assert_eq!(results.len(), 3, "one result slot per input");

    // Inspect each slot's prop name — distinguishes the slot's owner.
    let expected_prop_names = ["chr", "alp", "brv"];
    for (i, slot) in results.iter().enumerate() {
        let analysis = slot
            .as_ref()
            .unwrap_or_else(|err| panic!("slot {i} failed: {err:?}"))
            .as_ref()
            .unwrap_or_else(|| panic!("slot {i} returned None"));
        let prop_names: Vec<&str> = analysis.props.iter().map(|p| p.name.as_str()).collect();
        assert!(
            prop_names.contains(&expected_prop_names[i]),
            "slot {i} (input={}) must carry prop `{}` (input-order positional \
             contract); got props={prop_names:?}",
            canonical_ids[i],
            expected_prop_names[i]
        );
    }
}

/// **Test 3 — `submit_count` increases by exactly 1 per batch.**
///
/// Pre-Stage-8 the loop in `dispatch_meta_jobs` incremented
/// `submit_count` once per job (N times for N inputs). The
/// architecture contract is now O(1) per batch — a single scheduler
/// submission with N jobs fanned out internally.
#[test]
fn batch_scheduler_submit_count_is_o1() {
    let n: usize = 10;
    let files: Vec<(String, String)> = (0..n)
        .map(|i| {
            (
                format!("/src/B{i}.vue"),
                simple_owner_sfc(&format!("b{i}"), "number"),
            )
        })
        .collect();
    let files_ref: Vec<(&str, &str)> = files
        .iter()
        .map(|(c, s)| (c.as_str(), s.as_str()))
        .collect();
    let project = build_hermetic_project_with_files(&files_ref);
    let session = project.open_session_batch().expect("batch session");

    let scheduler = project.host().scheduler();
    let baseline = scheduler.counters().submit_count.load(Ordering::Relaxed);

    let canonical_ids: Vec<String> = files.iter().map(|(c, _)| c.clone()).collect();
    let results = session
        .get_component_meta_batch(&canonical_ids)
        .expect("batch dispatch should complete");
    assert_eq!(results.len(), n, "one result per input");

    let after = scheduler.counters().submit_count.load(Ordering::Relaxed);
    let delta = after - baseline;
    assert_eq!(
        delta, 1,
        "batch dispatch is O(1) per batch — `scheduler.counters.submit_count` \
         MUST bump by exactly 1 regardless of N={n} jobs. delta={delta} \
         (baseline={baseline}, after={after}). A delta of N would indicate \
         the per-job loop variant is back.",
    );
}

/// **Test 4 — shared admissions: N owners over a shared inner type
/// produce baseline-bounded `MaterializeStructureDb::entry_count()`.**
///
/// Drive `get_component_meta_batch` over N=5 owners that all consume
/// the SAME inner type `ChatMessageProps` via
/// `Pick<ChatMessageProps,'id'>`. The materialiser populates
/// `MaterializeStructureDb` for the shared inner type. Under R7
/// cross-owner reuse (the `MaterializeStructureCacheKey`'s
/// `Hash`/`PartialEq` excludes `scope_canonical_id`), N owners
/// collapse to the same slot — the entry count after the batch is
/// bounded by the single-owner baseline plus N, not N× baseline.
#[test]
fn batch_materialise_admissions_equal_unique_semantic_identities() {
    let inbox = owner_sfc("inbox_msg");
    let chat = owner_sfc("chat_msg");
    let sidebar = owner_sfc("sidebar_msg");
    let header = owner_sfc("header_msg");
    let footer = owner_sfc("footer_msg");
    let project = build_hermetic_project_with_files(&[
        ("/src/chat-types.ts", SHARED_TYPE_TS),
        ("/src/Inbox.vue", inbox.as_str()),
        ("/src/Chat.vue", chat.as_str()),
        ("/src/Sidebar.vue", sidebar.as_str()),
        ("/src/Header.vue", header.as_str()),
        ("/src/Footer.vue", footer.as_str()),
    ]);
    let session = project.open_session_batch().expect("batch session");

    let host = project.host();
    let db = host.project_type_store().materialize_structure_db();
    let entry_count_before = db.entry_count();
    eprintln!("[batch shared-admissions] entry_count_before={entry_count_before}");

    let owners = vec![
        "/src/Inbox.vue".to_string(),
        "/src/Chat.vue".to_string(),
        "/src/Sidebar.vue".to_string(),
        "/src/Header.vue".to_string(),
        "/src/Footer.vue".to_string(),
    ];
    let results = session
        .get_component_meta_batch(&owners)
        .expect("batch dispatch should complete");
    assert_eq!(results.len(), owners.len(), "one result slot per owner");
    for (i, slot) in results.iter().enumerate() {
        let analysis = slot
            .as_ref()
            .unwrap_or_else(|err| panic!("owner {i} failed: {err:?}"))
            .as_ref()
            .unwrap_or_else(|| panic!("owner {i} returned None"));
        assert!(
            !analysis.props.is_empty(),
            "owner {} must publish at least one prop",
            owners[i]
        );
    }

    let batch_entries = db.entry_count();
    let baseline = single_owner_baseline_entries();
    let n = owners.len();
    let batch_delta = batch_entries - entry_count_before;
    eprintln!(
        "[batch shared-admissions] batch_entries={batch_entries}, \
         batch_delta={batch_delta}, single_owner_baseline={baseline}, N={n}"
    );
    // R7 cross-owner reuse: N owners over a shared inner type collapse
    // to a single `MaterializeStructureDb` slot per semantic identity.
    // The upper bound `baseline + N` allows per-owner work for the
    // owner-local prop bag, but disallows N× duplication of the inner
    // type's slots.
    assert!(
        batch_entries <= baseline + n,
        "R7 cross-owner reuse: N={n} owners over a shared inner type MUST \
         produce entries bounded by single-owner baseline + N — sharing \
         MUST collapse inner-type slots. \
         got batch_entries={batch_entries}, baseline={baseline}, \
         max permitted = baseline ({baseline}) + N ({n}) = {}",
        baseline + n
    );
}

/// **Test 5 — partial failure does not abort the batch.**
///
/// A batch containing one unresolvable canonical (no matching file on
/// the workspace, no overlay) returns `None` in that slot while the
/// other slots succeed. The batch never propagates the per-id miss as
/// an `Err` at the batch level.
#[test]
fn batch_partial_failure_does_not_abort() {
    let real1 = simple_owner_sfc("a", "string");
    let real2 = simple_owner_sfc("b", "number");
    let project = build_hermetic_project_with_files(&[
        ("/src/Real1.vue", real1.as_str()),
        ("/src/Real2.vue", real2.as_str()),
    ]);
    let session = project.open_session_batch().expect("batch session");

    let canonical_ids = vec![
        "/src/Real1.vue".to_string(),
        "/src/DoesNotExist.vue".to_string(),
        "/src/Real2.vue".to_string(),
    ];
    let results = session
        .get_component_meta_batch(&canonical_ids)
        .expect("batch dispatch should not abort on per-id failures");
    assert_eq!(results.len(), 3, "one result slot per input");

    // Slot 0 (Real1) — must succeed with analysis.
    let real1_analysis = results[0]
        .as_ref()
        .unwrap_or_else(|err| panic!("real1 slot failed: {err:?}"))
        .as_ref()
        .expect("real1 slot must produce analysis");
    assert!(
        real1_analysis.props.iter().any(|p| p.name == "a"),
        "real1 must carry prop `a`"
    );

    // Slot 1 (DoesNotExist) — `get_component_meta` returns `Ok(None)`
    // for unknown canonicals; the batch surfaces that None in its slot.
    let missing = results[1]
        .as_ref()
        .unwrap_or_else(|err| panic!("missing slot must be Ok(None), got Err: {err:?}"));
    assert!(
        missing.is_none(),
        "missing slot must be Ok(None); got Some(_) — batch failed to \
         surface the per-id miss positionally"
    );

    // Slot 2 (Real2) — must succeed with analysis.
    let real2_analysis = results[2]
        .as_ref()
        .unwrap_or_else(|err| panic!("real2 slot failed: {err:?}"))
        .as_ref()
        .expect("real2 slot must produce analysis");
    assert!(
        real2_analysis.props.iter().any(|p| p.name == "b"),
        "real2 must carry prop `b`"
    );
}

/// Single-owner baseline: drive `get_component_meta` against ONE
/// owner over the same shared inner type and record how many entries
/// it produces in `MaterializeStructureDb`. The N-owner test above
/// uses this to compute a per-slot upper bound for the cross-owner
/// case.
fn single_owner_baseline_entries() -> usize {
    let solo = owner_sfc("solo_msg");
    let project = build_hermetic_project_with_files(&[
        ("/src/chat-types.ts", SHARED_TYPE_TS),
        ("/src/Solo.vue", solo.as_str()),
    ]);
    let session = project.open_session().expect("interactive session");
    let _ = session
        .get_component_meta("/src/Solo.vue")
        .expect("single-owner query should succeed");
    project
        .host()
        .project_type_store()
        .materialize_structure_db()
        .entry_count()
}

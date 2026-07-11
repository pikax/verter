//! §9.6 concurrency tests — additional rows beyond
//! `policy_active_refs_no_self_await_deadlock` and
//! `recursive_indexed_access_terminates_deterministically` already
//! authored in `component_meta_resolution_policy_cycle_tests.rs`.
//!
//! Each test exercises the cooperative-admission cache contract
//! under concurrent invocation. The tests are deterministic — no
//! wall-clock budgets, no platform-specific deadlock detection.
//! Cancellation / panic-poisoning rows are surfaced as §17.7
//! deviations because the cancellation primitive used by
//! `cancellation_does_not_poison_cache` is not exposed at a
//! useful level on integration HEAD `c4c26c1f`; see module
//! docstring for full deviation rationale.

use std::sync::Arc;
use std::thread;

use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use crate::types::HostConfig;
use crate::VerterHost;

#[allow(deprecated)]
fn make_project_config(root: &str) -> verter_workspace::VfsProjectConfig {
    verter_workspace::VfsProjectConfig {
        root: root.to_string(),
        rank: verter_workspace::ProjectRank::Explicit,
        tsconfig_path: Some(format!("{root}/tsconfig.json")),
        root_files: vec![],
        extensions: vec![],
        workspace_root: root.to_string(),
        workspace_aliases: vec![],
        compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: verter_workspace::ConfiguredMembership::match_all_under_root(
            &verter_workspace::CanonicalPath::new(root),
        ),
    }
}

fn build_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
    #[allow(deprecated)]
    let project_graph =
        verter_workspace::ProjectGraph::from_configs(vec![make_project_config("/workspace")]);
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.set_project_graph(project_graph);
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    let host = VerterHost::new(HostConfig::default(), ws_access);
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    Arc::new(host)
}

const SHARED_TYPES_TS: &str = r#"export interface CompProps {
  initial: string
  count: number
}
"#;

const SHARED_COMP_VUE: &str = r#"<script setup lang="ts">
import type { CompProps } from '/workspace/src/types'
defineProps<CompProps>()
</script>
<template><div /></template>
"#;

/// §9.6 row: two threads request `getComponentMeta('/CompA.vue')`
/// simultaneously on a cold cache; both threads MUST observe the
/// same `Some(ComponentMetaAnalysis)` result with the same prop
/// names. The cooperative-admission contract guarantees this:
/// concurrent requests for the same canonical id collapse onto
/// one materialization path; the second arrival blocks on
/// completion fence rather than re-running the materializer.
///
/// Discriminating predicate: both threads' returned analyses must
/// be `Some(...)` AND must agree on the prop list. A regression
/// where the cache promoted a torn intermediate state would
/// surface here as one thread observing a partial prop list. A
/// regression where two threads each ran the materializer
/// independently would be acceptable for correctness (still
/// converges) but observable through dispatch counters; that
/// behaviour assertion is reserved for the per-bundle counter
/// gates (e.g., the projector pipeline's per-request audit
/// counters) when the host exposes per-thread captures
/// (CaptureToken is per-request, not aggregable across threads).
#[test]
fn concurrent_cold_query_returns_same_metadata_to_both_threads() {
    let host = build_host(&[
        ("/workspace/src/types.ts", SHARED_TYPES_TS),
        ("/workspace/src/CompA.vue", SHARED_COMP_VUE),
    ]);

    let host_t1 = Arc::clone(&host);
    let host_t2 = Arc::clone(&host);

    let t1 = thread::Builder::new()
        .name("concurrent_cold_query_t1".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || host_t1.get_component_meta("/workspace/src/CompA.vue"))
        .expect("spawn t1");
    let t2 = thread::Builder::new()
        .name("concurrent_cold_query_t2".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || host_t2.get_component_meta("/workspace/src/CompA.vue"))
        .expect("spawn t2");

    let r1 = t1.join().expect("t1 join");
    let r2 = t2.join().expect("t2 join");

    let m1 = r1.expect("t1 must observe Some(ComponentMetaAnalysis)");
    let m2 = r2.expect("t2 must observe Some(ComponentMetaAnalysis)");

    let names1: Vec<String> = m1.props.iter().map(|p| p.name.clone()).collect();
    let names2: Vec<String> = m2.props.iter().map(|p| p.name.clone()).collect();

    assert_eq!(
        names1, names2,
        "concurrent t1 / t2 prop lists must match — torn-cache regression \
         would surface as the threads observing different prop sets. \
         t1 = {names1:?}, t2 = {names2:?}",
    );
    // Floor: both must include the canonical fields from
    // `CompProps`. A regression that promoted an empty intermediate
    // state would surface as the prop list being empty.
    assert!(
        names1.iter().any(|n| n == "initial") && names1.iter().any(|n| n == "count"),
        "concurrent t1 prop list must include the canonical fields from CompProps; got {names1:?}",
    );
}

/// §9.6 row: a stress variant — eight threads request the same
/// component on a cold cache. The cooperative-admission floor is
/// the same as the two-thread case; the stress run pins the
/// invariant under broader concurrency where transient TLS state
/// or per-request caches would surface as flake.
///
/// Discriminating predicate: every thread observes a non-None
/// result AND every thread observes the same prop list as the
/// first one. A regression that exposed torn intermediate state
/// to one thread would surface as a prop-list mismatch.
#[test]
fn concurrent_cold_query_eight_threads_consistent_metadata() {
    let host = build_host(&[
        ("/workspace/src/types.ts", SHARED_TYPES_TS),
        ("/workspace/src/CompA.vue", SHARED_COMP_VUE),
    ]);

    let mut handles = Vec::new();
    for i in 0..8 {
        let h = Arc::clone(&host);
        let label = format!("concurrent_cold_query_t{i}");
        let handle = thread::Builder::new()
            .name(label)
            .stack_size(8 * 1024 * 1024)
            .spawn(move || h.get_component_meta("/workspace/src/CompA.vue"))
            .expect("spawn worker");
        handles.push(handle);
    }

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    let mut prop_lists: Vec<Vec<String>> = Vec::new();
    for (i, r) in results.into_iter().enumerate() {
        let m = r.unwrap_or_else(|| panic!("thread {i} must observe Some(meta)"));
        let names: Vec<String> = m.props.iter().map(|p| p.name.clone()).collect();
        prop_lists.push(names);
    }
    let first = &prop_lists[0];
    for (i, names) in prop_lists.iter().enumerate().skip(1) {
        assert_eq!(
            names, first,
            "thread {i} prop list MUST match thread 0; got {names:?}, expected {first:?}",
        );
    }
    assert!(
        first.iter().any(|n| n == "initial") && first.iter().any(|n| n == "count"),
        "thread 0 prop list must include canonical fields from CompProps; got {first:?}",
    );
}

/// §9.6 row (variant): two threads request DIFFERENT components
/// concurrently. The cooperative-admission contract MUST allow
/// independent materializations to proceed in parallel — a
/// regression that linearised all materializations through a
/// single global lock would surface as one thread blocking the
/// other indefinitely. The test simply asserts both complete
/// successfully without timing out under the cargo test budget.
///
/// Discriminating predicate: both calls return `Some(...)` with
/// the expected prop set. Termination is a hard gate; a global-
/// lock regression would either deadlock both threads or
/// serialize them so heavily that the test exceeds the cargo test
/// wall-clock budget.
#[test]
fn concurrent_independent_components_resolve_in_parallel() {
    let comp_b_vue = r#"<script setup lang="ts">
defineProps<{ greeting: string }>()
</script>
<template><div /></template>
"#;

    let host = build_host(&[
        ("/workspace/src/types.ts", SHARED_TYPES_TS),
        ("/workspace/src/CompA.vue", SHARED_COMP_VUE),
        ("/workspace/src/CompB.vue", comp_b_vue),
    ]);

    let host_a = Arc::clone(&host);
    let host_b = Arc::clone(&host);

    let ta = thread::Builder::new()
        .name("concurrent_indep_a".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || host_a.get_component_meta("/workspace/src/CompA.vue"))
        .expect("spawn ta");
    let tb = thread::Builder::new()
        .name("concurrent_indep_b".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || host_b.get_component_meta("/workspace/src/CompB.vue"))
        .expect("spawn tb");

    let ra = ta.join().expect("ta join");
    let rb = tb.join().expect("tb join");

    let ma = ra.expect("CompA must resolve");
    let mb = rb.expect("CompB must resolve");

    let names_a: Vec<String> = ma.props.iter().map(|p| p.name.clone()).collect();
    let names_b: Vec<String> = mb.props.iter().map(|p| p.name.clone()).collect();
    assert!(names_a.iter().any(|n| n == "initial"));
    assert!(names_b.iter().any(|n| n == "greeting"));
}

// ── §9.6 deviation rows ──
//
// Three §9.6 rows are surfaced as §17.7 deviations rather than
// authored as discriminating tests, because the production
// surface required to discriminate is not exposed on integration
// HEAD `c4c26c1f`:
//
// - `cancellation_does_not_poison_cache`: the scheduler's
//   cancellation primitive is not exposed at a useful level via
//   the host API. Authoring a discriminating test would require
//   either touching `crates/verter_session/src/capture_token.rs`
//   (B-A0 territory, forbidden by the sidecar's "DO NOT" list)
//   or adding a public cancellation API (forbidden by §17.7
//   deviation triggers).
//
// - `panic_during_materialize_does_not_poison_cache`: injecting a
//   panic inside the materializer requires either a feature-gated
//   panic-injection hook (forbidden by §17.7 "no runtime feature
//   flag or conditional compilation to gate tests") or
//   instrumenting production code. The fence rule in CLAUDE.md is
//   architecturally guaranteed by `HostFenceValidator`'s
//   completion fence; verifying the architectural invariant is
//   covered by the existing `audited_request_e2e.rs::no_torn_*`
//   class of tests.
//
// - `recursive_indexed_access_terminates_deterministically` is
//   already authored in
//   `component_meta_resolution_policy_cycle_tests.rs::recursive_indexed_access_terminates_deterministically`
//   and is on integration HEAD.

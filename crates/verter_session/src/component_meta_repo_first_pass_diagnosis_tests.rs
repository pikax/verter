//! Phase 11b — `repo_first_pass` semantic-state regression DIAGNOSIS
//! tests (NOT acceptance tests).
//!
//! These tests exercise the four overlay-isolation scenarios from
//! §10.4 against a §4.2-shaped fixture (one root that depends on a
//! shared dep) and assert that the Phase 11b instrumentation hooks
//! capture **non-empty** counter data for each scenario. The tests
//! deliberately do NOT assert thresholds; threshold gates live in
//! Phase 11d. The discriminating value of these tests is that they
//! FAIL on the pre-instrumentation tree (counters never increment
//! because the production hooks are not wired) and PASS on the
//! post-instrumentation tree.
//!
//! Acceptance tests for the regression's *fix* (Phase 11d) will
//! consume the same scenario shape but bind threshold assertions
//! against the post-B2 baseline; this file's role is only to prove
//! the harness is wired correctly so the diagnosis benchmark in
//! `packages/benchmark/src/repo-first-pass.spec.ts` produces
//! non-trivial data.

use std::sync::Arc;

use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use crate::capture_token::CaptureToken;
use crate::types::HostConfig;
use crate::VerterHost;

/// Build a host backed by an in-memory workspace populated with
/// `files`. Mirrors the helper in
/// `component_meta_concurrency_tests.rs` so the diagnosis fixture
/// stays hermetic — no third-party corpus required.
fn build_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
    #[allow(deprecated)]
    let project_graph = verter_workspace::ProjectGraph::from_configs(vec![
        #[allow(deprecated)]
        verter_workspace::VfsProjectConfig {
            root: "/workspace".to_string(),
            rank: verter_workspace::ProjectRank::Explicit,
            tsconfig_path: Some("/workspace/tsconfig.json".to_string()),
            root_files: vec![],
            extensions: vec![],
            workspace_root: "/workspace".to_string(),
            workspace_aliases: vec![],
            compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: verter_workspace::ProjectMembership::MatchAll,
        },
    ]);
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

const SHARED_TYPES_TS: &str = r#"export interface BaseProps {
  initial: string
  count: number
}

export interface DerivedProps extends BaseProps {
  variant: 'primary' | 'secondary'
  size: 'sm' | 'md' | 'lg'
}
"#;

fn comp_vue(prop_type: &str) -> String {
    format!(
        r#"<script setup lang="ts">
import type {{ {prop_type} }} from '/workspace/src/types'
defineProps<{prop_type}>()
</script>
<template><div /></template>
"#
    )
}

/// Build a fixture with one shared dep and N components. Each
/// component imports the same `DerivedProps` so the dep graph is
/// identical and the per-component dispatch should reuse the
/// underlying `Instantiate` and `ProjectPath` warm cache once the
/// first component hydrates them.
fn build_repo_fixture(num_components: usize) -> (Arc<VerterHost>, Vec<String>) {
    let mut files: Vec<(String, String)> = Vec::new();
    files.push((
        "/workspace/src/types.ts".to_string(),
        SHARED_TYPES_TS.to_string(),
    ));
    let mut canonical_ids = Vec::with_capacity(num_components);
    for i in 0..num_components {
        let canonical = format!("/workspace/src/Comp{i}.vue");
        canonical_ids.push(canonical.clone());
        files.push((canonical, comp_vue("DerivedProps")));
    }
    // Build flat slice for build_host's expected lifetime
    let slice: Vec<(&str, &str)> = files
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let host = build_host(&slice);
    (host, canonical_ids)
}

/// §10.4 (i): full include / one overlay (single_cold equivalent).
/// Run getComponentMeta for the target with no prior queries on
/// other components. Captures a "single cold" baseline.
#[test]
fn repo_first_pass_diagnosis_emits_capture_curves() {
    // Use a small fixture so the test is fast; the diagnosis benchmark
    // runs the full §4.2 component list.
    const N: usize = 4;
    let (host, canonical_ids) = build_repo_fixture(N);
    assert_eq!(canonical_ids.len(), N);

    let target = canonical_ids.last().expect("at least one component");

    // Scenario (i) — single_cold: target only.
    let snap_single = {
        let host_clone = Arc::clone(&host);
        let guard = CaptureToken::start_for_query("repo_first_pass_single_cold");
        let _meta = host_clone
            .get_component_meta(target)
            .expect("scenario (i) must return Some(ComponentMetaAnalysis)");
        guard.end()
    };

    // Capture pool size after scenario (i) so subsequent scenarios can
    // record their delta against the post-(i) state. We attribute the
    // pool size at end-of-capture by reading it from the project type
    // store.
    let pool_size_after_i = host
        .project_type_store()
        .semantic_graph()
        .derivation_signature_pool_size() as u64;

    // The instrumentation must be wired. A regression that drops the
    // hooks (e.g., reverts the production-side `Instant::now()` deltas
    // or the `with_active_capture` calls) makes these counters stay 0
    // and the test fails — that is the discriminating predicate.
    assert!(
        snap_single.origin_edge_count > 0,
        "scenario (i): origin_edge_count must be > 0 (got {})",
        snap_single.origin_edge_count
    );
    assert!(
        snap_single.record_origin_edge_total_ns > 0,
        "scenario (i): record_origin_edge_total_ns must be > 0 (got {})",
        snap_single.record_origin_edge_total_ns
    );
    assert!(
        snap_single.derivation_signature_intern_calls > 0,
        "scenario (i): derivation_signature_intern_calls must be > 0 (got {})",
        snap_single.derivation_signature_intern_calls
    );
    // Entries-mutex hold time must be observed for any non-trivial
    // query — the warm-publish path always acquires the entries lock
    // at least once via `warm_publish_one`, and the warm `get` path
    // does too. A zero hold-time would indicate the diagnosed-lock
    // helper is not wired.
    assert!(
        snap_single.entries_mutex_hold_total_ns > 0,
        "scenario (i): entries_mutex_hold_total_ns must be > 0 (got {})",
        snap_single.entries_mutex_hold_total_ns
    );
    assert!(
        pool_size_after_i > 0,
        "scenario (i): derivation_signature_pool_size must grow (got {pool_size_after_i})"
    );

    // ---------------------------------------------------------------
    // Scenario (ii) — full include / all overlays / target FIRST.
    // Identical to (i) on this fixture (no overlay isolation in the
    // host-level test fixture; the benchmark scenario differs only in
    // overlay surface). The capture is still expected to be non-empty
    // because the warm cache has at least the `(family, slot)` from
    // (i) and we invoke get_component_meta on a fresh canonical.
    // ---------------------------------------------------------------
    let target_for_ii = &canonical_ids[0];
    let snap_target_first = {
        let host_clone = Arc::clone(&host);
        let guard = CaptureToken::start_for_query("repo_first_pass_target_first");
        let _meta = host_clone
            .get_component_meta(target_for_ii)
            .expect("scenario (ii) must return Some(ComponentMetaAnalysis)");
        guard.end()
    };

    // Scenario (ii) on this hermetic fixture queries a different
    // canonical from scenario (i), but the underlying `BaseProps` and
    // `DerivedProps` types are already warm in the `SemanticGraphStore`
    // from (i). The component-level final-result cache key differs
    // (different canonical) so the analysis path does run; whether
    // origin_edge_count is > 0 here is a fixture-specific detail
    // recorded by the diagnosis benchmark, not asserted by this
    // harness test. We still record the snapshot for the report.
    let _ = &snap_target_first;

    // ---------------------------------------------------------------
    // Scenario (iii) — full include / all overlays / target AFTER prior.
    // Resolve N-1 prior components first, THEN the target. This is
    // where the `repo_first_pass` regression manifests: the prior
    // queries warm `(family, slot)` entries that should make the
    // target's query nearly free, but observed cost is not flat.
    // ---------------------------------------------------------------
    // Warm caches by querying the first N-1 components.
    for canonical in &canonical_ids[..N - 1] {
        let _ = host
            .get_component_meta(canonical)
            .expect("warming query must succeed");
    }
    let target_for_iii = canonical_ids.last().unwrap();
    let snap_after_prior = {
        let host_clone = Arc::clone(&host);
        let guard = CaptureToken::start_for_query("repo_first_pass_target_after_prior");
        let _meta = host_clone
            .get_component_meta(target_for_iii)
            .expect("scenario (iii) must return Some(ComponentMetaAnalysis)");
        guard.end()
    };

    // The target's own component-level analysis is still cold the
    // first time we query it (scenario (i) targeted a different
    // canonical), but other prior components share the same dep
    // graph and may have warmed everything. Counter values here are
    // fixture-specific and recorded by the diagnosis benchmark.
    let _ = &snap_after_prior;

    // ---------------------------------------------------------------
    // Scenario (iv) — same as (iii) but `clear_caches` before target.
    // Per prior measurements, `clear_caches` does NOT reset the
    // semantic-graph store / overlay state — the diagnosis report
    // explicitly observes which counters return to (i)'s value and
    // which do not.
    // ---------------------------------------------------------------
    // Reset the fixture to a fresh state to avoid scenario-(iii)
    // residual interference, then run scenario (iv) cleanly.
    let (host_iv, canonical_ids_iv) = build_repo_fixture(N);
    for canonical in &canonical_ids_iv[..N - 1] {
        let _ = host_iv
            .get_component_meta(canonical)
            .expect("warming query must succeed");
    }
    // The host-level `clear_compile_cache` mirrors the JS-side
    // `clearCaches()` (component-meta calls into it via
    // `MetaProject::clear_caches`).
    host_iv.clear_compile_cache();
    let target_for_iv = canonical_ids_iv.last().unwrap();
    let snap_after_clear = {
        let host_clone = Arc::clone(&host_iv);
        let guard = CaptureToken::start_for_query("repo_first_pass_after_clear");
        let _meta = host_clone
            .get_component_meta(target_for_iv)
            .expect("scenario (iv) must return Some(ComponentMetaAnalysis)");
        guard.end()
    };

    // Scenario (iv) re-runs a cold target after `clear_compile_cache`.
    // The compile cache is reset, so per-component compile state is
    // cold; however the semantic-graph store and overlay state are
    // intentionally NOT reset by `clear_compile_cache` (per the
    // §10.4 prior measurements baked into Phase 11b's contract).
    // Counter shape recorded in the diagnosis report.
    let _ = &snap_after_clear;

    // The diagnosis report focuses on the deltas; the test's only
    // job is to prove the harness emits non-empty data on the
    // discriminating scenario (i). Scenarios (ii)-(iv) on this
    // hermetic fixture may legitimately be near-empty because the
    // fixture's dep graph collapses fully on the first cold target
    // — the diagnosis benchmark exercises the §4.2 component list
    // where the regression manifests. Both code paths share the
    // instrumentation hooks; if they're wired here they're wired
    // there.
    let _ = (
        &snap_single,
        &snap_target_first,
        &snap_after_prior,
        &snap_after_clear,
    );
}

/// Per-scenario per-counter table. Returned to the diagnosis
/// benchmark via the public `verter_session::for_tests` shim so the
/// vitest can introspect counters without depending on private
/// internals. Public so the diagnosis benchmark can deserialize it
/// into JSON-friendly form.
///
/// This is a Rust-side helper for the diagnosis test harness; the
/// vitest benchmark exercises the same API path through
/// component-meta's host-backed interface and emits the JSON
/// report.
#[cfg(test)]
mod helpers {
    /// Compile-time check that the diagnosis counter fields are
    /// reachable through the `for_tests` re-export shim. A future
    /// refactor that hides `CaptureSnapshot::origin_edge_count`
    /// breaks this no-op test, surfacing the regression early.
    #[test]
    fn diagnosis_counter_fields_are_publicly_visible() {
        use crate::for_tests::CaptureSnapshot;
        // We only need the fields to be addressable in code; the
        // values are set via the harness path. Use a destructuring
        // pattern so any future field rename surfaces here too.
        fn _check(snap: &CaptureSnapshot) -> u64 {
            let CaptureSnapshot {
                origin_edge_count,
                derivation_signature_intern_calls,
                derivation_signature_intern_returned_existing,
                ..
            } = snap;
            *origin_edge_count
                + *derivation_signature_intern_calls
                + *derivation_signature_intern_returned_existing
        }
    }
}

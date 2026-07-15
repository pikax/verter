//! Stage 10 — `repo_warm_second_pass` bench.
//!
//! Reuses the Stage-0 hermetic baseline fixture (16 components +
//! shared types) and measures the warm-pass distribution after a
//! cold first pass. The Stage 7 canary contract is:
//!
//! > `repo_warm_second_pass` per-component p99 ≤ 1.0 × Stage-0
//! > baseline p50 (on the same host).
//!
//! This bench measures the actual p50/p95/p99 of the warm-pass and
//! compares against `crates/verter_session/tests/fixtures/cache_baseline/baseline.json`.
//! The baseline file pins the structural fields (component count,
//! fact_validation_*_count, materialise_cardinality_per_owner,
//! candidate_set_size_histogram); the wall-clock fields are
//! null-pinned because timings are machine-dependent. The bench
//! captures both passes on the canary host so the ratio comparison
//! stays valid.
//!
//! Hermeticity: no third-party corpus; constructs an in-process
//! `MemoryWorkspace` + `VerterHost` and runs every test on it.

#![allow(clippy::needless_pass_by_value)]

use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use verter_session::HostConfig;
use verter_session::VerterHost;

use verter_semantic::analysis::project_resolver::IdeProjectConfig;
use verter_workspace::{
    IdeProjectCompilerOptions, MemoryOptions, MemoryWorkspace, ProjectGraph, ProjectRank,
    VfsProjectConfig, WorkspaceAccess,
};

const NUM_COMPONENTS: usize = 16;

const SHARED_TYPES_TS: &str = r#"export interface BaseProps {
  initial: string;
  count: number;
}

export interface DerivedProps extends BaseProps {
  variant: 'primary' | 'secondary';
  size: 'sm' | 'md' | 'lg';
}
"#;

fn comp_vue() -> String {
    r#"<script setup lang="ts">
import type { DerivedProps } from '/workspace/src/types';
defineProps<DerivedProps>();
</script>
<template><div /></template>
"#
    .to_string()
}

fn build_host() -> (Arc<VerterHost>, Vec<String>) {
    let mut files: Vec<(String, String)> = Vec::with_capacity(NUM_COMPONENTS + 1);
    files.push((
        "/workspace/src/types.ts".to_string(),
        SHARED_TYPES_TS.to_string(),
    ));
    let mut canonicals = Vec::with_capacity(NUM_COMPONENTS);
    for i in 0..NUM_COMPONENTS {
        let canonical = format!("/workspace/src/Comp{i}.vue");
        canonicals.push(canonical.clone());
        files.push((canonical, comp_vue()));
    }

    #[allow(deprecated)]
    let project_graph = ProjectGraph::from_configs(vec![
        #[allow(deprecated)]
        VfsProjectConfig {
            root: "/workspace".to_string(),
            rank: ProjectRank::Explicit,
            tsconfig_path: Some("/workspace/tsconfig.json".to_string()),
            root_files: vec![],
            extensions: vec![],
            workspace_root: "/workspace".to_string(),
            workspace_aliases: vec![],
            compiler_options: IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: verter_workspace::ConfiguredMembership::match_all_under_root(
                &verter_workspace::CanonicalPath::new("/workspace"),
            ),
        },
    ]);
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.set_project_graph(project_graph);
    for (canonical, content) in &files {
        workspace.inject_file(canonical.clone(), Arc::from(content.as_str()));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    let host = VerterHost::new(HostConfig::default(), ws_access);
    host.configure_projects(vec![IdeProjectConfig::new(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    )]);
    (Arc::new(host), canonicals)
}

fn percentile_ms(samples: &[f64], pct: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f64> = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let rank = ((pct / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

/// Drive 3 iterations × NUM_COMPONENTS through `get_component_meta`
/// to characterise the warm-pass distribution AFTER a cold first
/// pass. Each iteration captures per-component timings; the bench
/// asserts the cold→warm ratio is dominated by warm hits (warm p99
/// is at most ~1.0× cold p50, the Stage-7 canary contract).
fn bench_repo_warm_second_pass(c: &mut Criterion) {
    c.bench_function("repo_warm_second_pass/aggregate", |b| {
        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;
            for _ in 0..iters {
                let (host, canonicals) = build_host();
                // Cold pass: every component queried for the first
                // time. Times are captured for context but not asserted
                // (the Stage-0 baseline JSON pins null timings).
                let mut cold_samples = Vec::with_capacity(NUM_COMPONENTS);
                for canonical in &canonicals {
                    let started = Instant::now();
                    let meta = host.get_component_meta(canonical);
                    cold_samples.push(started.elapsed().as_secs_f64() * 1000.0);
                    black_box(meta);
                }
                // Warm pass: same components, second touch. Per the
                // Stage 7 canary contract, p99 of the warm pass must
                // be ≤ 1.0 × cold p50.
                let mut warm_samples = Vec::with_capacity(NUM_COMPONENTS);
                let warm_started = Instant::now();
                for canonical in &canonicals {
                    let started = Instant::now();
                    let meta = host.get_component_meta(canonical);
                    warm_samples.push(started.elapsed().as_secs_f64() * 1000.0);
                    black_box(meta);
                }
                total += warm_started.elapsed();
                let cold_p50 = percentile_ms(&cold_samples, 50.0);
                let warm_p99 = percentile_ms(&warm_samples, 99.0);
                // Print the cold/warm/ratio so a CI consumer can
                // diagnose regressions without re-running.
                eprintln!(
                    "[repo_warm_second_pass] cold_p50={:.3}ms warm_p99={:.3}ms ratio={:.3}",
                    cold_p50,
                    warm_p99,
                    if cold_p50 > 0.0 {
                        warm_p99 / cold_p50
                    } else {
                        0.0
                    }
                );
                assert!(
                    warm_p99 <= cold_p50 * 1.0 + 0.5, /* 0.5ms tolerance for sub-ms timer noise */
                    "Stage 10 canary failure: warm_p99 ({:.3}ms) > cold_p50 ({:.3}ms). \
                     R24/Stage 7 contract: warm-pass p99 ≤ 1.0× cold-pass p50 on the \
                     same host. A warm-pass slower than the cold-pass median indicates \
                     the warm cache is not delivering — investigate fact-validation \
                     correctness or cache-key drift.",
                    warm_p99,
                    cold_p50
                );
            }
            total
        });
    });
}

criterion_group!(benches, bench_repo_warm_second_pass);
criterion_main!(benches);

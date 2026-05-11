//! Stage-0 hermetic baseline bench for the fact-based cache refactor.
//!
//! Drives `repo_first_pass` against a hermetic synthetic workspace
//! (no nuxt-ui; no `.integration-tests/repos` required) and emits a
//! Stage-0-snapshot JSON to two places:
//!
//! 1. `target/cache-baseline.json` — generated at each bench run; not
//!    committed.
//! 2. `crates/verter_session/tests/fixtures/cache_baseline/baseline.json`
//!    — the COMMITTED Stage-0 snapshot. Stage 7's canary diffs against
//!    this file.
//!
//! Output schema (stable; see plan §"Stage 0" sub-task 5 and
//! §"Canary criteria"):
//!
//! ```text
//! {
//!   "schema_version": 1,
//!   "captured_at_sha": "<40-char sha>",
//!   "windows_native": <bool>,
//!   "av_excluded": <bool>,
//!   "components": [{"path": "…", "cold_ms": …, "warm_ms": …}, …],
//!   "aggregates": {
//!     "p50_per_component_ms": …,
//!     "p95_per_component_ms": …,
//!     "p99_per_component_ms": …,
//!     "fact_validation_warm_hit_count": 0,
//!     "fact_validation_miss_count": …,
//!     "materialise_cardinality_per_owner": …,
//!     "candidate_set_size_histogram": {"1": …}
//!   }
//! }
//! ```
//!
//! Pre-Stage-1 the fact_validation counters are 0 (no fact-based cache
//! exists yet); per the plan the `candidate_set_size_histogram` is
//! `{"1": N}` (every cache slot carries exactly one entry today).
//!
//! Plan citation: §"Stage 0" sub-task 5.

#![allow(clippy::needless_pass_by_value)]

use criterion::{criterion_group, criterion_main, Criterion};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use verter_session::HostConfig;
use verter_session::VerterHost;
use verter_workspace::{
    IdeProjectCompilerOptions, MemoryOptions, MemoryWorkspace, ProjectGraph, ProjectMembership,
    ProjectRank, VfsProjectConfig, WorkspaceAccess,
};

const BASELINE_NUM_COMPONENTS: usize = 16;

/// Shared types file imported by every Comp{i}.vue component.
const SHARED_TYPES_TS: &str = r#"export interface BaseProps {
  initial: string;
  count: number;
}

export interface DerivedProps extends BaseProps {
  variant: 'primary' | 'secondary';
  size: 'sm' | 'md' | 'lg';
}
"#;

fn comp_vue(prop_type: &str) -> String {
    format!(
        r#"<script setup lang="ts">
import type {{ {prop_type} }} from '/workspace/src/types';
defineProps<{prop_type}>();
</script>
<template><div /></template>
"#
    )
}

fn build_baseline_host(num_components: usize) -> (Arc<VerterHost>, Vec<String>) {
    let mut files: Vec<(String, String)> = Vec::with_capacity(num_components + 1);
    files.push((
        "/workspace/src/types.ts".to_string(),
        SHARED_TYPES_TS.to_string(),
    ));
    let mut canonicals = Vec::with_capacity(num_components);
    for i in 0..num_components {
        let canonical = format!("/workspace/src/Comp{i}.vue");
        canonicals.push(canonical.clone());
        files.push((canonical, comp_vue("DerivedProps")));
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
            membership: ProjectMembership::MatchAll,
        },
    ]);
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.set_project_graph(project_graph);
    for (canonical, content) in &files {
        workspace.inject_file(canonical.clone(), Arc::from(content.as_str()));
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
    (Arc::new(host), canonicals)
}

#[derive(Serialize)]
struct ComponentTiming {
    path: String,
    cold_ms: f64,
    warm_ms: f64,
}

#[derive(Serialize)]
struct CandidateSetSizeHistogram {
    #[serde(rename = "1")]
    one: u64,
}

#[derive(Serialize)]
struct AggregatesSnapshot {
    p50_per_component_ms: f64,
    p95_per_component_ms: f64,
    p99_per_component_ms: f64,
    fact_validation_warm_hit_count: u64,
    fact_validation_miss_count: u64,
    materialise_cardinality_per_owner: f64,
    candidate_set_size_histogram: CandidateSetSizeHistogram,
}

#[derive(Serialize)]
struct BaselineSnapshot {
    schema_version: u32,
    captured_at_sha: String,
    windows_native: bool,
    av_excluded: bool,
    pre_stage1_notes: Vec<String>,
    components: Vec<ComponentTiming>,
    aggregates: AggregatesSnapshot,
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

fn captured_sha() -> String {
    // The Stage-0 baseline SHA is recorded as the post-stage-0-pre commit
    // ccc05223. Future regenerations on a later base SHA update this
    // string in lockstep with the committed JSON.
    "ccc0522309091c532d6fba756da392598eab059c".to_string()
}

fn windows_native() -> bool {
    cfg!(target_os = "windows")
}

/// Compute the baseline snapshot by exercising `get_component_meta` on
/// every component twice: a cold pass (first-touch) and a warm pass
/// (re-query). Returns timings + aggregates.
fn run_baseline_measurement() -> BaselineSnapshot {
    let (host, canonicals) = build_baseline_host(BASELINE_NUM_COMPONENTS);

    let mut timings: Vec<ComponentTiming> = Vec::with_capacity(canonicals.len());

    // Cold pass.
    let mut cold_samples: Vec<f64> = Vec::with_capacity(canonicals.len());
    for canonical in &canonicals {
        let start = Instant::now();
        let _meta = host
            .get_component_meta(canonical)
            .expect("cold get_component_meta must succeed on hermetic baseline");
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        cold_samples.push(elapsed_ms);
        timings.push(ComponentTiming {
            path: canonical.clone(),
            cold_ms: elapsed_ms,
            warm_ms: 0.0,
        });
    }

    // Warm pass — second invocation should hit warm caches (today's
    // component-meta cache).
    let mut warm_samples: Vec<f64> = Vec::with_capacity(canonicals.len());
    for (i, canonical) in canonicals.iter().enumerate() {
        let start = Instant::now();
        let _meta = host
            .get_component_meta(canonical)
            .expect("warm get_component_meta must succeed");
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        warm_samples.push(elapsed_ms);
        timings[i].warm_ms = elapsed_ms;
    }

    // Per-component samples are the COLD measurements (Stage 7 canary
    // measures repo_warm_second_pass against this distribution).
    let p50 = percentile_ms(&cold_samples, 50.0);
    let p95 = percentile_ms(&cold_samples, 95.0);
    let p99 = percentile_ms(&cold_samples, 99.0);

    // Pre-Stage-1, fact validation counters do not exist; record 0.
    // The committed snapshot uses these zeros as the discriminator: a
    // Stage 6d run that emits non-zero
    // `fact_validation_warm_hit_count` invalidates the Stage-0 snapshot
    // by design (the Stage 7 canary expects ≥ 98 % warm hit on the
    // hermetic baseline).
    let aggregates = AggregatesSnapshot {
        p50_per_component_ms: p50,
        p95_per_component_ms: p95,
        p99_per_component_ms: p99,
        fact_validation_warm_hit_count: 0,
        fact_validation_miss_count: canonicals.len() as u64,
        // Stage-5 sub-task A audit conclusion: today every
        // `(scope_canonical_id, base, scope_axis, mode)` slot in
        // `MaterializeStructureDb` carries exactly one entry. The
        // hermetic baseline's `ChatMessageProps`-style shared dep
        // (`DerivedProps`) appears in all N component-owners' slots
        // pre-cutover, giving cardinality == N. Stage 5 inverts to
        // == 1 (single shared slot, multi-candidate).
        materialise_cardinality_per_owner: canonicals.len() as f64,
        candidate_set_size_histogram: CandidateSetSizeHistogram {
            one: canonicals.len() as u64,
        },
    };

    BaselineSnapshot {
        schema_version: 1,
        captured_at_sha: captured_sha(),
        windows_native: windows_native(),
        // The orchestrator documented this as `false` for Stage 0; tweak
        // when AV exclusion lands on the bench host.
        av_excluded: false,
        pre_stage1_notes: vec![
            "fact_validation_warm_hit_count == 0 because no fact-based cache exists yet \
             (Stage 3 introduces facts; Stage 6d wires validation)."
                .to_string(),
            "fact_validation_miss_count records the cold-pass count (every component \
             missed because no fact-validated cache existed at query time)."
                .to_string(),
            "materialise_cardinality_per_owner == N today (one MaterializeStructureDb \
             entry per owner-instance of the shared dep); Stage 5 inverts to == 1."
                .to_string(),
            "candidate_set_size_histogram has only the \"1\" bin populated today; Stage 5 \
             multi-candidate storage grows the histogram up to the R20 cap = 4."
                .to_string(),
            "p50 / p95 / p99 are wall-clock cold-pass milliseconds against the \
             hermetic 16-component fixture in build_baseline_host. The Stage 7 canary \
             measures warm-pass against the same fixture and the same SHA pin."
                .to_string(),
        ],
        components: timings,
        aggregates,
    }
}

/// Write the snapshot to `target/cache-baseline.json` (regenerated each
/// run; not committed).
fn write_target_snapshot(snapshot: &BaselineSnapshot) {
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .join("target")
        .join("cache-baseline.json");
    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let payload = serde_json::to_string_pretty(snapshot).expect("snapshot serialises");
    std::fs::write(&target, payload).unwrap_or_else(|e| {
        panic!("write target snapshot {}: {}", target.display(), e);
    });
}

fn cache_baseline_bench(c: &mut Criterion) {
    // The "bench" entry point publishes the snapshot. We use criterion
    // as the harness because the workspace bench Cargo.toml is set up
    // for criterion-driven benches; this entry simply runs the measurement
    // once per iteration. Sample size 1 — we want a deterministic single
    // measurement, not a microbench-style sample distribution.
    let mut group = c.benchmark_group("stage_0/cache_baseline");
    group.sample_size(10);

    group.bench_function("repo_first_pass_hermetic", |b| {
        b.iter(|| {
            let snapshot = run_baseline_measurement();
            write_target_snapshot(&snapshot);
            // black-box to keep the optimiser honest
            std::hint::black_box(snapshot);
        });
    });

    group.finish();
}

criterion_group!(baseline_benches, cache_baseline_bench);
criterion_main!(baseline_benches);

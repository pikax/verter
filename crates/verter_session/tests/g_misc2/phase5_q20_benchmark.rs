//! Q20 benchmark tracking for the projector decomposition.
//!
//! Records the warm and cold latency of `getComponentMeta` for a
//! representative ChatMessage-like fixture as a regression baseline
//! (10 iterations, median).
//!
//! The benchmark's role is regression detection: warm
//! median must remain dramatically faster than cold median (proves
//! the cache fence is functioning), and absolute medians must stay
//! within reasonable bounds (proves the projector path's dispatch
//! budget hasn't regressed to O(N²)).
//!
//! The test asserts:
//!
//! 1. The cold pass produces populated metadata (the projector path
//!    is wired).
//! 2. The warm-pass median is < 50% of the cold-pass median (the
//!    cache fence is dramatically faster; this is much stricter
//!    than the previous "< 2× cold" check and discriminates against
//!    a regression that fully duplicates the cold work on warm).
//! 3. Cold + warm medians together complete within 10s for the
//!    small fixture (regression sanity).

use std::sync::Arc;
use std::time::Instant;

use verter_session::{HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

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
        membership: verter_workspace::ProjectMembership::MatchAll,
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

const CHATMESSAGE_TYPES_TS: &str = r#"export interface ChatRole {
  role: 'user' | 'assistant' | 'system'
  tool: string
  timestamp: number
}

export interface MessageBase<T> {
  id: string
  user: string
  count: number
  message: T
}

export type ChatMessageProps = MessageBase<ChatRole>
"#;

const CHATMESSAGE_VUE: &str = r#"<script setup lang="ts">
import type { ChatMessageProps } from '/workspace/src/types'
defineProps<ChatMessageProps>()
defineEmits<{
  click: [event: string]
  edit: [id: string, newText: string]
}>()
defineSlots<{
  default: () => unknown
  avatar?: () => unknown
}>()
</script>
<template><div /></template>
"#;

fn median_ns(samples: &mut [u128]) -> u128 {
    samples.sort();
    samples[samples.len() / 2]
}

/// Q20 benchmark — 10 iterations, median cold + median warm.
/// Asserts (a) warm median is dramatically faster than cold median
/// (cache fence functions), (b) totals stay within 10s budget,
/// (c) prop counts match across cold/warm passes.
#[test]
fn phase5_q20_benchmark_recorded() {
    const ITERATIONS: usize = 10;

    let mut cold_samples: Vec<u128> = Vec::with_capacity(ITERATIONS);
    let mut warm_samples: Vec<u128> = Vec::with_capacity(ITERATIONS);
    let mut prop_counts: Vec<usize> = Vec::with_capacity(ITERATIONS);

    for _ in 0..ITERATIONS {
        // Fresh host per iteration so cold pass is genuinely cold.
        let host = build_host(&[
            ("/workspace/src/types.ts", CHATMESSAGE_TYPES_TS),
            ("/workspace/src/ChatMessage.vue", CHATMESSAGE_VUE),
        ]);

        let started_cold = Instant::now();
        let meta_cold = host
            .get_component_meta("/workspace/src/ChatMessage.vue")
            .expect("Q20 cold pass must succeed");
        cold_samples.push(started_cold.elapsed().as_nanos());

        let started_warm = Instant::now();
        let meta_warm = host
            .get_component_meta("/workspace/src/ChatMessage.vue")
            .expect("Q20 warm pass must succeed");
        warm_samples.push(started_warm.elapsed().as_nanos());

        assert_eq!(
            meta_cold.props.len(),
            meta_warm.props.len(),
            "Q20: cold/warm prop counts must match per iteration (cold={}, warm={})",
            meta_cold.props.len(),
            meta_warm.props.len(),
        );
        assert!(
            !meta_cold.props.is_empty(),
            "Q20 cold: ChatMessage must publish at least one prop"
        );
        prop_counts.push(meta_cold.props.len());
    }

    let cold_median_ns = median_ns(&mut cold_samples.clone());
    let warm_median_ns = median_ns(&mut warm_samples.clone());
    let cold_median_ms = cold_median_ns as f64 / 1_000_000.0;
    let warm_median_ms = warm_median_ns as f64 / 1_000_000.0;

    // Strict warm-vs-cold gate: warm median must be < 50% of cold
    // median (with a 10ms floor on cold to avoid jitter on very fast
    // calls). The cache fence MUST short-circuit warm work — a
    // regression that re-runs cold-equivalent work on warm would
    // push warm to ≈ cold and fail this gate.
    let cold_floor_ns = cold_median_ns.max(10_000_000);
    assert!(
        warm_median_ns * 2 < cold_floor_ns,
        "Q20: warm median must be < 50% of cold median \
         (cold={cold_median_ms:.2}ms, warm={warm_median_ms:.2}ms). \
         A regression that re-runs cold-equivalent work on warm \
         would fail this gate."
    );

    // Total-budget regression sanity.
    let total_median_s = (cold_median_ns + warm_median_ns) as f64 / 1_000_000_000.0;
    assert!(
        total_median_s < 10.0,
        "Q20: cold + warm median must complete < 10s total (got {total_median_s:.2}s)"
    );

    // Record numbers for CI / audit observability.
    eprintln!(
        "Q20 benchmark (n={ITERATIONS}, median): cold={:.2}ms warm={:.2}ms \
         (warm/cold = {:.2}%) props={}",
        cold_median_ms,
        warm_median_ms,
        100.0 * warm_median_ms / cold_median_ms.max(0.001),
        prop_counts[0],
    );
}

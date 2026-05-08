//! Q20 benchmark tracking for the projector decomposition.
//!
//! Records the warm and cold latency of `getComponentMeta` for a
//! representative ChatMessage-like fixture as a regression baseline.
//! The benchmark numbers go into the commit body so future agents
//! have a reference point for the Q20 tie-breaker.
//!
//! The test does NOT assert specific numeric thresholds — those live
//! in commit-body documentation. The test asserts:
//!
//! 1. The cold pass produces populated metadata (the projector path
//!    is wired).
//! 2. The warm pass is no slower than 2× the cold pass (the warm
//!    cache fence is functioning; an unbounded warm pass would
//!    indicate cooperative-admission breakage).
//! 3. The fixture completes within an aggressive 10s budget — the
//!    same kind of bound as the 100-prop check, but applied to a
//!    smaller payload.

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

/// Q20 benchmark — cold + warm pass timing recorded for reference.
/// Asserts the fundamental invariants (warm < 2× cold, total < 10s)
/// without coupling to absolute numbers.
#[test]
fn phase5_q20_benchmark_recorded() {
    let host = build_host(&[
        ("/workspace/src/types.ts", CHATMESSAGE_TYPES_TS),
        ("/workspace/src/ChatMessage.vue", CHATMESSAGE_VUE),
    ]);

    // Cold pass.
    let started_cold = Instant::now();
    let meta_cold = host
        .get_component_meta("/workspace/src/ChatMessage.vue")
        .expect("Q20 cold pass must succeed");
    let cold_elapsed = started_cold.elapsed();

    assert!(
        !meta_cold.props.is_empty(),
        "Q20 cold: ChatMessage must publish at least one prop \
         (got {})",
        meta_cold.props.len()
    );

    // Warm pass.
    let started_warm = Instant::now();
    let meta_warm = host
        .get_component_meta("/workspace/src/ChatMessage.vue")
        .expect("Q20 warm pass must succeed");
    let warm_elapsed = started_warm.elapsed();

    assert_eq!(
        meta_cold.props.len(),
        meta_warm.props.len(),
        "Q20: cold/warm prop counts must match (cold={}, warm={})",
        meta_cold.props.len(),
        meta_warm.props.len(),
    );

    // Warm pass must be no slower than 2× cold (with a 10ms floor to
    // avoid jitter on very fast calls). A warm pass that's slower
    // would indicate the fence/admission gate isn't returning early.
    let cold_ns = cold_elapsed.as_nanos().max(10_000_000);
    let warm_ns = warm_elapsed.as_nanos();
    assert!(
        warm_ns < cold_ns.saturating_mul(2),
        "Q20: warm pass must be < 2× cold (cold={:.1}ms, warm={:.1}ms)",
        cold_elapsed.as_secs_f64() * 1000.0,
        warm_elapsed.as_secs_f64() * 1000.0,
    );

    // Total budget: both passes complete well within 10s. (Generous
    // upper bound for hermetic CI; a regression to O(N^2) dispatch
    // would push this over.)
    assert!(
        cold_elapsed.as_secs_f64() + warm_elapsed.as_secs_f64() < 10.0,
        "Q20: cold + warm must complete < 10s total (cold={:.2}s, warm={:.2}s)",
        cold_elapsed.as_secs_f64(),
        warm_elapsed.as_secs_f64(),
    );

    // Side-effect: print the numbers so they show in test output and
    // get captured in CI logs / commit body. Not an assertion.
    eprintln!(
        "Q20 benchmark recorded: cold={:.2}ms, warm={:.2}ms, props={}",
        cold_elapsed.as_secs_f64() * 1000.0,
        warm_elapsed.as_secs_f64() * 1000.0,
        meta_cold.props.len(),
    );
}

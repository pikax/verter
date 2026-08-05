//! Degraded flow-return interior is never laundered into a complete,
//! warm-admitted component-meta result (C1/SCC-2 distinctness).
//!
//! A `FlowReturn` DEGRADED SUCCESS (a usable value whose evaluation
//! substituted a modeled-`any`) is `ReturnOnly` at the flow layer. When an
//! enclosing composition (here: `defineProps<ReturnType<typeof helper>>()`
//! whose helper's body-derived return degrades with `NonCallableBinding`)
//! consumes that value, the degradation MUST fold into the enclosing
//! channels — the request partial sticky plus build-local `cache_suppress`
//! — so the component-meta result:
//!
//! - still PUBLISHES the prop (a degraded success is a usable value — the
//!   opposite collapse, interning a miss, is equally forbidden);
//! - reports `synthesis_should_suppress == true`;
//! - is REFUSED `ComponentMetaResultDb` warm admission (a second request
//!   cold-recomputes; it never warm-replays the degraded-derived value as
//!   complete).
//!
//! Before the fix the four `FunctionReturnNode::Flow(result)` consumers
//! dropped `result.degradation`, so the degraded-derived prop published as
//! complete + warm-admissible and the second meta call performed ZERO
//! dispatches.

#![cfg(test)]

use std::sync::Arc;

use verter_session::audited_request::AuditedRequest;
use verter_session::{AnalysisLevel, HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

/// The helper's body-derived return degrades: calling the `number`-typed
/// parameter is the `NonCallableBinding` degraded SUCCESS (`p: any` — a
/// usable value with a typed degradation reason).
const FLOW_HELPER_TS: &str = r#"
export function makeProps(notFn: number) {
  return { p: notFn() };
}
"#;

const COMPONENT_SFC: &str = r#"<script setup lang="ts">
import { makeProps } from './flowHelper'
defineProps<ReturnType<typeof makeProps>>();
</script>
<template><div /></template>
"#;

fn build_host() -> Arc<VerterHost> {
    let scheduler_config = verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    };
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file("/workspace/flowHelper.ts".into(), Arc::from(FLOW_HELPER_TS));
    workspace.inject_file("/workspace/Comp.vue".into(), Arc::from(COMPONENT_SFC));
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    Arc::new(VerterHost::new_with_scheduler_config(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        ws_access,
        scheduler_config,
    ))
}

fn component_meta_cache_len(host: &Arc<VerterHost>) -> usize {
    host.project_type_store().component_meta_results().len()
}

#[test]
fn degraded_flow_interior_suppresses_meta_warm_but_still_publishes_prop() {
    let host = build_host();
    assert_eq!(component_meta_cache_len(&host), 0);

    // Request 1 — cold. The degraded flow interior must mark the result
    // partial: prop published, suppress raised, final cache refused.
    let (_a1, resolution1, record1) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/workspace/Comp.vue")
        .expect("request 1 must succeed");
    assert!(!record1.from_cache, "request 1 must be cold");

    let meta1 = host
        .get_component_meta("/workspace/Comp.vue")
        .expect("component meta resolves");
    assert!(
        meta1.props.iter().any(|prop| prop.name == "p"),
        "the degraded success is a USABLE value: the prop is still \
         published (got props {:?})",
        meta1
            .props
            .iter()
            .map(|prop| prop.name.clone())
            .collect::<Vec<_>>(),
    );
    assert!(
        resolution1.synthesis_should_suppress,
        "a degraded flow interior MUST mark the enclosing component-meta \
         result partial (synthesis_should_suppress) — degraded-vs-complete \
         distinctness (C1/SCC-2)",
    );
    assert_eq!(
        component_meta_cache_len(&host),
        0,
        "the degraded-derived result must NOT be admitted to \
         ComponentMetaResultDb — warm admission requires an explicit \
         degradation admission row, and none exists",
    );

    // Request 2 — must NOT warm-replay the degraded-derived value as
    // complete. The final cache stays empty and the request is served
    // cold again.
    let (_a2, resolution2, record2) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/workspace/Comp.vue")
        .expect("request 2 must succeed");
    assert!(
        !record2.from_cache,
        "request 2 must NOT warm-hit a laundered complete entry",
    );
    assert!(
        resolution2.synthesis_should_suppress,
        "request 2 reproduces the degraded partial, never a laundered \
         complete result",
    );
    assert_eq!(
        component_meta_cache_len(&host),
        0,
        "ComponentMetaResultDb stays empty after request 2",
    );
}

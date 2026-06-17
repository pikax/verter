//! Sampler thread MUST cleanly stop when the host is dropped.
//!
//! `HostAuditRuntime` owns the sampler thread; the thread holds a
//! `Weak<HostAuditRuntime>`. When the last `Arc<HostAuditRuntime>`
//! drops, the next `weak.upgrade()` in the loop returns `None` and
//! the thread terminates. The runtime's `Drop` impl explicitly joins
//! the thread handle so we never leak threads across host drops.
//!
//! Discrimination contract:
//! - The runtime exposes process-static
//!   `sampler_thread_spawn_count()` /
//!   `sampler_thread_join_count()` test-only accessors. After the
//!   host is constructed AND a request runs, spawn-count >= 1.
//!   After the host drops, join-count must equal spawn-count for the
//!   delta we owned in this test. Pre-change tree (no join in Drop)
//!   leaves join-count behind by 1 — assertion FAILS.
//! - Drop must also return promptly (`< 5s`). A leaked thread that
//!   blocks on join would hang the test harness; a leaked thread
//!   that's never joined would still mutate the (now freed) runtime
//!   through `weak.upgrade()` returning the stale Arc — but Weak
//!   semantics prevent this UB by returning `None` once the strong
//!   count reaches zero.
//!
//! Skipped on WASM (sampler does not exist there).

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;
use std::time::{Duration, Instant};

use verter_session::host_audit_runtime::{sampler_thread_join_count, sampler_thread_spawn_count};
use verter_session::{HostConfig, UpsertRequest, VerterHost};

const SFC: &str = r#"<script setup lang="ts">
defineProps<{ a: string }>()
</script>
<template><div>{{ a }}</div></template>
"#;

#[test]
fn sampler_thread_joins_cleanly_on_host_drop() {
    // Snapshot the global spawn/join counts BEFORE constructing the
    // host. Other tests in the suite may have spawned and joined
    // their own samplers; we only assert on the delta we created.
    let spawn_before = sampler_thread_spawn_count();
    let join_before = sampler_thread_join_count();

    {
        let host = Arc::new(VerterHost::new_standalone(HostConfig {
            audit_enabled: true,
            audit_timing_capture: true,
            footprint_capture: true,
            ..HostConfig::default()
        }));
        let canonical = "/sampler_join.vue";
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(SFC),
            file_language: verter_session::LanguageRegistry::global()
                .classify_static(canonical)
                .static_resolution(),
            aliases: Vec::new(),
        });

        // Drive a request so the sampler is guaranteed to have
        // spawned (the runtime spawns lazily on first audit-enabled
        // request when timing flag is on).
        let (_analysis, resolution) = host
            .get_component_meta_with_resolution(canonical)
            .expect("component-meta resolution must succeed");
        let _record = host
            .host_audit_runtime()
            .take_record(resolution.request_id)
            .expect("record must be present after the request finalises");

        // The sampler should have spawned exactly once for this
        // host. Pre-change tree (sampler never spawned) would leave
        // the spawn delta at 0; post-change wires the spawn at first
        // active registration.
        let spawn_during = sampler_thread_spawn_count();
        assert!(
            spawn_during > spawn_before,
            "sampler must have spawned at least one thread for the audit-enabled \
             host with audit_timing_capture=true; got spawn_before={spawn_before}, \
             spawn_during={spawn_during}"
        );

        // Drop the host below — measured timing wraps the join.
        let drop_started = Instant::now();
        drop(host);
        let drop_elapsed = drop_started.elapsed();

        // Drop must return within a generous bound. The sampler
        // ticks every 50 ms; worst-case shutdown latency is one
        // tick + join overhead. 5 s is comfortable slack on loaded
        // CI workers; a non-joining Drop impl would either hang
        // (timeout) OR appear instant (leaking the thread).
        assert!(
            drop_elapsed < Duration::from_secs(5),
            "host drop must complete promptly — sampler thread join \
             exceeded 5 seconds (got {drop_elapsed:?}). A non-joining \
             Drop impl on a stuck thread would deadlock here.",
        );
    }

    // Post-drop: spawn delta must equal join delta. A leaked thread
    // (spawned but never joined) shows spawn > join here.
    let spawn_after = sampler_thread_spawn_count();
    let join_after = sampler_thread_join_count();
    let spawn_delta = spawn_after - spawn_before;
    let join_delta = join_after - join_before;
    assert!(
        spawn_delta >= 1,
        "expected at least one sampler spawn during this test; \
         spawn_delta={spawn_delta} (spawn_before={spawn_before}, \
         spawn_after={spawn_after})"
    );
    assert_eq!(
        spawn_delta, join_delta,
        "thread leak suspected: spawn_delta={spawn_delta}, join_delta={join_delta}. \
         Pre-change tree (no Drop join) would show spawn_delta > join_delta because \
         the spawned thread is never explicitly joined when the runtime drops."
    );
}

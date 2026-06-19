//! Sampler thread MUST cleanly stop when the host is dropped.
//!
//! `HostAuditRuntime` owns the sampler thread; the thread holds a
//! `Weak<HostAuditRuntime>`. When the last `Arc<HostAuditRuntime>`
//! drops, the next `weak.upgrade()` in the loop returns `None` and
//! the thread terminates. The runtime's `Drop` impl explicitly joins
//! the thread handle so we never leak threads across host drops.
//!
//! Discrimination contract (host-owned, NOT process-global):
//! - The runtime exposes a per-host `sampler_spawned()` accessor. After
//!   the host is constructed AND a request runs, it is `true`.
//!   Pre-change tree (sampler never spawned) leaves it `false` —
//!   assertion FAILS.
//! - The runtime exposes a per-host `sampler_join_observer()` — an
//!   `Arc<AtomicBool>` the test clones BEFORE dropping the host. The
//!   runtime's `Drop` flips it to `true` ONLY after `JoinHandle::join()`
//!   returns. After the host drops, the observable must read `true`.
//!   Pre-change tree (no join in Drop) never reaches the flip — the
//!   observable stays `false` and the assertion FAILS.
//! - Drop must also return promptly (`< 5s`). A leaked thread that
//!   blocks on join would hang the test harness.
//!
//! This per-host probe replaces the retired process-global
//! spawn/join counters: it observes ONLY this host's sampler, so a
//! concurrent in-binary test that spawns-but-has-not-yet-joined its
//! own sampler cannot perturb the measurement.
//!
//! Skipped on WASM (sampler does not exist there).

#![cfg(not(target_arch = "wasm32"))]

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use verter_session::{HostConfig, UpsertRequest, VerterHost};

const SFC: &str = r#"<script setup lang="ts">
defineProps<{ a: string }>()
</script>
<template><div>{{ a }}</div></template>
"#;

#[test]
fn sampler_thread_joins_cleanly_on_host_drop() {
    // Hold the host-owned join observable across the host drop so we
    // can read it AFTER the runtime is gone.
    let join_observer: Arc<std::sync::atomic::AtomicBool>;

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
        // request when the timing flag is on).
        let (_analysis, resolution) = host
            .get_component_meta_with_resolution(canonical)
            .expect("component-meta resolution must succeed");
        let _record = host
            .host_audit_runtime()
            .take_record(resolution.request_id)
            .expect("record must be present after the request finalises");

        // The sampler must have spawned for THIS host. Pre-change tree
        // (sampler never spawned) leaves the per-host latch `false`;
        // post-change wires the spawn at first active registration.
        assert!(
            host.host_audit_runtime().sampler_spawned(),
            "sampler must have spawned for the audit-enabled host with \
             audit_timing_capture=true (per-host `sampler_spawned()` was false)"
        );

        // Clone the host-owned join observable BEFORE dropping the
        // host. It is `false` now; the runtime's `Drop` flips it to
        // `true` only after joining the sampler thread.
        join_observer = host.host_audit_runtime().sampler_join_observer();
        assert!(
            !join_observer.load(Ordering::Acquire),
            "join observable must be false before the host drops"
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

    // Post-drop: THIS host's join observable must have flipped to
    // `true`. A leaked thread (spawned but never joined) never reaches
    // the `Drop` flip, so the observable stays `false`. This is the
    // host-scoped discriminator against a non-joining `Drop` impl —
    // immune to concurrent samplers in sibling tests.
    assert!(
        join_observer.load(Ordering::Acquire),
        "thread leak suspected: this host's sampler join observable is \
         still false after the host dropped. Pre-change tree (no Drop \
         join) never reaches the post-join flip because the spawned \
         thread is never explicitly joined when the runtime drops."
    );
}

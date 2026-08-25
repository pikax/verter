//! Sampler thread MUST cleanly stop when the host is dropped.
//!
//! `HostAuditRuntime` owns the sampler thread; the thread holds
//! `Arc<SamplerState>` and never `Arc<HostAuditRuntime>`. Owner drop
//! stores an exact stop flag, unparks the sampler, and joins on the
//! owner thread so shutdown does not wait for the next periodic tick
//! and cannot self-join if drop races an in-flight sample.
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
//! - A handshake test forces the sampler inside a sample, drops the
//!   owner on a side thread, then releases the sampler and proves
//!   causal join — no elapsed-time bound participates in correctness.
//!
//! Skipped on WASM (sampler does not exist there).

#![cfg(not(target_arch = "wasm32"))]

use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use verter_session::{HostConfig, UpsertRequest, VerterHost};

const SFC: &str = r#"<script setup lang="ts">
defineProps<{ a: string }>()
</script>
<template><div>{{ a }}</div></template>
"#;

fn audit_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        audit_timing_capture: true,
        footprint_capture: true,
        ..HostConfig::default()
    }))
}

fn spawn_sampler_via_request(host: &VerterHost, canonical: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(SFC),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
    let (_analysis, resolution) = host
        .get_component_meta_with_resolution(canonical)
        .expect("component-meta resolution must succeed");
    let _record = host
        .host_audit_runtime()
        .take_record(resolution.request_id)
        .expect("record must be present after the request finalises");
}

#[test]
fn sampler_thread_joins_cleanly_on_host_drop() {
    let join_observer: Arc<std::sync::atomic::AtomicBool>;

    {
        let host = audit_host();
        spawn_sampler_via_request(&host, "/sampler_join.vue");

        assert!(
            host.host_audit_runtime().sampler_spawned(),
            "sampler must have spawned for the audit-enabled host with \
             audit_timing_capture=true (per-host `sampler_spawned()` was false)"
        );

        join_observer = host.host_audit_runtime().sampler_join_observer();
        assert!(
            !join_observer.load(Ordering::Acquire),
            "join observable must be false before the host drops"
        );

        drop(host);
    }

    assert!(
        join_observer.load(Ordering::Acquire),
        "thread leak suspected: this host's sampler join observable is \
         still false after the host dropped. Pre-change tree (no Drop \
         join) never reaches the post-join flip because the spawned \
         thread is never explicitly joined when the runtime drops."
    );
}

/// Force the sampler inside a sample, drop the last runtime owner on
/// a side thread, release the sampler, and prove causal join. A
/// sampler that upgrades `Weak<HostAuditRuntime>` during the sample
/// would run `Drop` on its own thread and deadlock on `join`.
#[test]
fn sampler_shuts_down_during_an_active_sample() {
    let (entered_tx, entered_rx) = mpsc::sync_channel(0);
    let (release_tx, release_rx) = mpsc::sync_channel(0);

    let host = audit_host();
    // Run the request FIRST. The sampler pauses inside its sample while
    // holding the active-request registry's read guard, so arming the
    // handshake before the request would block that request's own
    // finalize on the registry write lock.
    spawn_sampler_via_request(&host, "/sampler_handshake.vue");
    assert!(
        host.host_audit_runtime().sampler_spawned(),
        "sampler must have spawned before the in-sample handshake"
    );
    host.host_audit_runtime()
        .arm_sample_handshake(entered_tx, release_rx);

    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("sampler must enter the armed sample handshake (outer watchdog)");

    let join_observer = host.host_audit_runtime().sampler_join_observer();
    let (drop_entered_tx, drop_entered_rx) = mpsc::sync_channel(0);
    host.host_audit_runtime().arm_drop_entered(drop_entered_tx);
    let (dropped_tx, dropped_rx) = mpsc::sync_channel(0);
    let dropper = thread::spawn(move || {
        drop(host);
        dropped_tx
            .send(())
            .expect("test still waits for owner drop");
    });

    // The witness is the release token: releasing the sampler costs one, and
    // only an entered owner release mints one, so this pair cannot be
    // reordered into "release first, hope the dropper got there".
    let drop_entered = drop_entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("owner Drop must enter while the sampler is still in the sample");
    release_tx
        .send(drop_entered)
        .expect("sampler must still be blocked in the handshake");
    dropped_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("owner drop must join the sampler without waiting for a tick");
    dropper.join().expect("dropper thread must not panic");

    assert!(
        join_observer.load(Ordering::Acquire),
        "owner drop during an active sample must join the sampler on the \
         owner thread; a self-join deadlock never flips this observable"
    );
}

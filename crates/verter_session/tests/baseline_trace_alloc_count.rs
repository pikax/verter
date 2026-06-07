//! Baseline allocation count for `getComponentMeta` with
//! `audit_enabled: false`.
//!
//! Wraps the global allocator with a counting allocator, runs a
//! `getComponentMeta` request on a small fixture, and records the
//! allocation count. With the lazy trace macro, the trace sites do
//! not allocate when no accumulator is installed — a naive trace
//! macro would instead run its `format!(...)` argument on every call
//! site even when no accumulator was installed.
//!
//! Test binaries each install their own `#[global_allocator]`, so this
//! test owns the entire process's allocator. Other tests in the same
//! binary would see the counting allocator too — keeping this test in
//! its own integration-test file ensures its allocator does not
//! interfere with sibling tests.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

/// Counting allocator that delegates to the system allocator and
/// tracks total allocation count. Reset via `COUNTER.store(0, ...)`.
struct CountingAllocator;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNTER.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

const FIXTURE_VUE: &str = "<script setup lang=\"ts\">\n\
defineProps<{ label: string; count: number }>();\n\
</script>\n\
<template><div>{{ label }}: {{ count }}</div></template>\n";

#[test]
fn record_baseline_allocation_count_for_audit_off_get_component_meta() {
    // Build host + fixture. These allocate too, but we only measure
    // the resolution-phase allocations after the reset.
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: false,
            footprint_capture: false,
            ..HostConfig::default()
        },
        workspace,
    ));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/Small.vue".into()),
        input_id: "/Small.vue".into(),
        source: Arc::from(FIXTURE_VUE),
        file_kind: FileKind::VueSfc,
        aliases: vec![],
    });

    // First call: warm bootstrap (parsing, indexing, initial resolution).
    // Allocations during this phase are not what the lazy trace macro targets.
    let primed = host.get_component_meta_with_resolution("/Small.vue");
    assert!(
        primed.is_some(),
        "baseline precondition: host must resolve `/Small.vue` before measurement",
    );

    // Reset, then measure a second resolution. Audit is off → no
    // `RequestFootprintAccumulator` is installed in TLS for either call,
    // so the trace macros short-circuit before evaluating their detail
    // expressions; a naive trace site would unconditionally run
    // `format!(...)`.
    COUNTER.store(0, Ordering::Relaxed);
    let _ = host.get_component_meta_with_resolution("/Small.vue");
    let allocations = COUNTER.load(Ordering::Relaxed);

    eprintln!("F8_BASELINE_AUDIT_OFF_ALLOCATIONS = {allocations}");

    // Sanity invariant: the audit-off resolution must allocate
    // SOMETHING (we still build the resolved state, hash maps, etc.)
    // but should not blow into the millions for this trivial
    // fixture. Adjust upper bound only if the resolution architecture
    // legitimately changes its allocation profile.
    assert!(
        allocations > 0,
        "baseline: counting allocator must have observed allocations \
         from a non-trivial getComponentMeta resolution",
    );
    assert!(
        allocations < 200_000,
        "baseline: audit-off resolution allocated {allocations} times — \
         a naive trace macro would fire format!() on every call regardless \
         of accumulator presence. If this number is large, the lazy trace \
         macro may have regressed.",
    );
}

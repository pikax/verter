//! §5.D.2 read-once / shallow-first / lazy-expansion tests for the
//! §5.B variants and seed closures (Phase 5g-supplement backfill for
//! 5b/5e/5f).
//!
//! Each test asserts:
//! 1. Cold path: the OWNER and TRANSITIVELY-NEEDED deps are loaded;
//!    UNRELATED files are NOT loaded (lazy expansion).
//! 2. Warm path: the SECOND identical query triggers ZERO additional
//!    reads, shallow processes, or lowerings (read-once contract).
//! 3. Result equality: q1 == q2 byte-for-byte (the warm answer is
//!    not a wrongly-cached one).
//!
//! Uses the §5.D.0 r17 instrumentation surface (`audit().*`) which
//! lives behind bare `#[cfg(test)]` per r17/N12.
//!
//! Plan: §5.D.2 (Phase 5g-supplement.1.B for 5b/5e/5f backfill).

use std::sync::Arc;

use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use crate::types::HostConfig;
use crate::VerterHost;

/// Build a hermetic [`VerterHost`] backed by a [`MemoryWorkspace`]
/// and the given files. The host opts into audit + footprint capture
/// so the existing component-meta audit surface stays exercised
/// alongside the §5.D.0 r17 host-level test counters.
fn build_hermetic_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        ws_access,
    ))
}

const A_VUE: &str = r#"<script setup lang="ts">
import type { BProps } from './B'
defineProps<BProps>()
</script>
<template><div /></template>
"#;

const B_TS: &str = r#"export interface BProps {
  foo: string;
  bar: number;
}
"#;

const C_TS: &str = r#"export interface UnrelatedC {
  qux: boolean;
}
"#;

/// 5b §5.D.2 — `ResolveMacroPayload` read-once / shallow-first /
/// lazy-expansion. The owner /A.vue uses `defineProps<BProps>()` so
/// /B.ts is transitively needed; /C.ts is unrelated and MUST NOT be
/// loaded.
#[test]
fn read_once_shallow_first_lazy_for_resolve_macro_payload() {
    let host = build_hermetic_host(&[("/A.vue", A_VUE), ("/B.ts", B_TS), ("/C.ts", C_TS)]);

    // First query — cold path: /A.vue + /B.ts loaded; /C.ts NOT loaded.
    let q1 = host.get_component_meta("/A.vue");
    assert!(
        q1.is_some(),
        "first get_component_meta on /A.vue must produce a result"
    );
    let after_first = host.audit().loaded_files();
    let after_first_set: std::collections::HashSet<&str> =
        after_first.iter().map(|s| s.as_ref()).collect();
    assert!(
        after_first_set.contains("/A.vue"),
        "owner /A.vue must be loaded after first query (got {after_first:?})"
    );
    assert!(
        after_first_set.contains("/B.ts"),
        "transitively-needed /B.ts must be loaded after first query (got {after_first:?})"
    );
    assert!(
        !after_first_set.contains("/C.ts"),
        "unrelated /C.ts MUST NOT be loaded after first query (lazy expansion contract; got {after_first:?})"
    );

    // Second query — warm path: NO additional reads / shallows /
    // lowerings; q1 == q2 byte-for-byte.
    let baseline_reads = host.audit().total_reads();
    let baseline_shallow = host.audit().total_shallow_processes();
    let baseline_lowerings = host.audit().total_lowerings();
    let q2 = host.get_component_meta("/A.vue");
    let read_delta = host.audit().total_reads() - baseline_reads;
    let shallow_delta = host.audit().total_shallow_processes() - baseline_shallow;
    let lowering_delta = host.audit().total_lowerings() - baseline_lowerings;
    assert_eq!(
        read_delta, 0,
        "second query must NOT trigger additional read (got delta={read_delta})"
    );
    assert_eq!(
        shallow_delta, 0,
        "second query must NOT trigger additional shallow process (got delta={shallow_delta})"
    );
    assert_eq!(
        lowering_delta, 0,
        "second query must NOT trigger additional lowering (got delta={lowering_delta})"
    );
    // ComponentMetaAnalysis does not derive PartialEq; compare debug
    // strings as a structural-equality proxy. Both queries must
    // produce IDENTICAL output — that proves the warm answer is the
    // same answer, not a wrongly-cached one.
    assert_eq!(
        format!("{:?}", q1),
        format!("{:?}", q2),
        "first and second component-meta results must be debug-equal"
    );
}

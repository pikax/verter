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

const FALLTHROUGH_VUE: &str = r#"<script setup lang="ts">
import type { Inh } from './inh'
defineProps<Inh>()
defineEmits<{ click: [evt: MouseEvent] }>()
</script>
<template><div /></template>
"#;

const FALLTHROUGH_INH_TS: &str = r#"export interface Inh {
  label: string;
}
"#;

const FALLTHROUGH_UNRELATED_TS: &str = r#"export interface UnusedZ {
  qux: number;
}
"#;

/// 5f §5.D.2 — `fallthrough_inheritance` read-once / shallow-first /
/// lazy-expansion. Owner uses defineProps + defineEmits so the
/// inherited-emits + indexed-paths dispatch (which §5f closes via
/// the resolver-internals migration) is exercised. /unrelated.ts
/// stays untouched.
#[test]
fn read_once_shallow_first_lazy_for_fallthrough_inheritance() {
    let host = build_hermetic_host(&[
        ("/A.vue", FALLTHROUGH_VUE),
        ("/inh.ts", FALLTHROUGH_INH_TS),
        ("/unrelated.ts", FALLTHROUGH_UNRELATED_TS),
    ]);

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
        "owner /A.vue must be loaded (got {after_first:?})"
    );
    assert!(
        after_first_set.contains("/inh.ts"),
        "transitively-needed /inh.ts must be loaded (got {after_first:?})"
    );
    assert!(
        !after_first_set.contains("/unrelated.ts"),
        "unrelated /unrelated.ts MUST NOT be loaded (got {after_first:?})"
    );

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
    assert_eq!(
        format!("{:?}", q1),
        format!("{:?}", q2),
        "first and second component-meta results must be debug-equal"
    );
}

const ROUTE_PICK_VUE: &str = r#"<script setup lang="ts">
import type { BaseProps } from './props'
defineProps<Pick<BaseProps, 'foo' | 'bar'>>()
</script>
<template><div /></template>
"#;

const ROUTE_PROPS_TS: &str = r#"export interface BaseProps {
  foo: string;
  bar: number;
  baz: boolean;
}
"#;

const ROUTE_UNRELATED_TS: &str = r#"export interface UnrelatedC {
  qux: boolean;
}
"#;

/// 5e §5.D.2 — `route_target_pick_omit` read-once / shallow-first /
/// lazy-expansion. The owner uses `defineProps<Pick<BaseProps, 'foo' |
/// 'bar'>>()` so /props.ts is transitively needed; /unrelated.ts is
/// unrelated and MUST NOT be loaded.
#[test]
fn read_once_shallow_first_lazy_for_route_target_pick_omit() {
    let host = build_hermetic_host(&[
        ("/A.vue", ROUTE_PICK_VUE),
        ("/props.ts", ROUTE_PROPS_TS),
        ("/unrelated.ts", ROUTE_UNRELATED_TS),
    ]);

    // First query — cold path.
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
        after_first_set.contains("/props.ts"),
        "transitively-needed /props.ts must be loaded after first query (got {after_first:?})"
    );
    assert!(
        !after_first_set.contains("/unrelated.ts"),
        "unrelated /unrelated.ts MUST NOT be loaded after first query (got {after_first:?})"
    );

    // Second query — warm path.
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
    assert_eq!(
        format!("{:?}", q1),
        format!("{:?}", q2),
        "first and second component-meta results must be debug-equal"
    );
}

const USERLAND_SHADOWING_PICK_VUE: &str = r#"<script setup lang="ts">
import type { CfgUserland } from './shadow_cfg'
type Pick<T, _K> = T;
defineProps<Pick<CfgUserland, 'alpha'>>()
</script>
<template><div /></template>
"#;

const USERLAND_SHADOWING_CFG_TS: &str = r#"export interface CfgUserland {
  alpha: string;
  beta: number;
  gamma: boolean;
}
"#;

const USERLAND_SHADOWING_UNRELATED_TS: &str = r#"export interface UnrelatedHelper {
  qux: boolean;
}
"#;

/// 5h §5.D.2 — `userland_shadowing_pick` read-once / shallow-first /
/// lazy-expansion. The owner /A.vue declares a userland
/// `type Pick<T, _K> = T` shadowing the ambient lib's `Pick`, and
/// imports the source interface `CfgUserland` from /shadow_cfg.ts —
/// so the `defineProps<Pick<CfgUserland, 'alpha'>>()` call walks
/// the userland Pick's body and expands `CfgUserland`. The
/// transitively-needed file /shadow_cfg.ts MUST be loaded; the
/// unrelated /unused.ts MUST NOT be loaded (the resolver-context
/// shadow gate must NOT cause spurious cross-file walks). The
/// second identical query MUST trigger ZERO additional reads /
/// shallow processes / lowerings (the read-once contract holds for
/// the shadow-gate path).
#[test]
fn read_once_shallow_first_lazy_for_userland_shadowing_pick() {
    let host = build_hermetic_host(&[
        ("/A.vue", USERLAND_SHADOWING_PICK_VUE),
        ("/shadow_cfg.ts", USERLAND_SHADOWING_CFG_TS),
        ("/unused.ts", USERLAND_SHADOWING_UNRELATED_TS),
    ]);

    let q1 = host.get_component_meta("/A.vue");
    assert!(
        q1.is_some(),
        "first get_component_meta on /A.vue must produce a result for the userland shadowing case"
    );
    let after_first = host.audit().loaded_files();
    let after_first_set: std::collections::HashSet<&str> =
        after_first.iter().map(|s| s.as_ref()).collect();
    assert!(
        after_first_set.contains("/A.vue"),
        "owner /A.vue must be loaded after first query (got {after_first:?})"
    );
    assert!(
        after_first_set.contains("/shadow_cfg.ts"),
        "transitively-needed /shadow_cfg.ts must be loaded after first query (got {after_first:?})"
    );
    assert!(
        !after_first_set.contains("/unused.ts"),
        "unrelated /unused.ts MUST NOT be loaded after first query \
         (lazy expansion contract — the userland-shadow gate must \
         not cause spurious cross-file walks; got {after_first:?})"
    );

    let baseline_reads = host.audit().total_reads();
    let baseline_shallow = host.audit().total_shallow_processes();
    let baseline_lowerings = host.audit().total_lowerings();
    let q2 = host.get_component_meta("/A.vue");
    let read_delta = host.audit().total_reads() - baseline_reads;
    let shallow_delta = host.audit().total_shallow_processes() - baseline_shallow;
    let lowering_delta = host.audit().total_lowerings() - baseline_lowerings;
    assert_eq!(
        read_delta, 0,
        "second userland-shadowing query must NOT trigger additional read (got delta={read_delta})"
    );
    assert_eq!(
        shallow_delta, 0,
        "second userland-shadowing query must NOT trigger additional shallow process (got delta={shallow_delta})"
    );
    assert_eq!(
        lowering_delta, 0,
        "second userland-shadowing query must NOT trigger additional lowering (got delta={lowering_delta})"
    );
    assert_eq!(
        format!("{:?}", q1),
        format!("{:?}", q2),
        "first and second userland-shadowing component-meta results must be debug-equal"
    );
}

const EXCLUDE_EXTRACT_VUE: &str = r#"<script setup lang="ts">
import type { Source } from './source_types'
defineProps<{ kept: Exclude<Source, 'b'> }>()
</script>
<template><div /></template>
"#;

const EXCLUDE_EXTRACT_SOURCE_TS: &str = r#"export type Source = 'a' | 'b' | 'c';
"#;

const EXCLUDE_EXTRACT_UNRELATED_TS: &str = r#"export interface UnrelatedZ {
  qux: number;
}
"#;

/// 5i §5.D.2 — `Exclude<>` / `Extract<>` reduction read-once /
/// shallow-first / lazy-expansion. The owner /A.vue uses
/// `defineProps<{ kept: Exclude<Source, 'b'> }>()` where `Source`
/// is imported from /source_types.ts. The transitively-needed
/// /source_types.ts MUST be loaded; the unrelated /unused.ts MUST
/// NOT be loaded (the `Exclude` arm must NOT cause spurious
/// cross-file walks). The second identical query MUST trigger
/// ZERO additional reads / shallow processes / lowerings (the
/// read-once contract holds for the per-member `relate_nodes`
/// dispatch path).
#[test]
fn read_once_shallow_first_lazy_for_exclude_extract_reduction() {
    let host = build_hermetic_host(&[
        ("/A.vue", EXCLUDE_EXTRACT_VUE),
        ("/source_types.ts", EXCLUDE_EXTRACT_SOURCE_TS),
        ("/unused.ts", EXCLUDE_EXTRACT_UNRELATED_TS),
    ]);

    // First query — cold path.
    let q1 = host.get_component_meta("/A.vue");
    assert!(
        q1.is_some(),
        "first get_component_meta on /A.vue must produce a result for the Exclude reduction"
    );
    let after_first = host.audit().loaded_files();
    let after_first_set: std::collections::HashSet<&str> =
        after_first.iter().map(|s| s.as_ref()).collect();
    assert!(
        after_first_set.contains("/A.vue"),
        "owner /A.vue must be loaded after first query (got {after_first:?})"
    );
    assert!(
        after_first_set.contains("/source_types.ts"),
        "transitively-needed /source_types.ts must be loaded after first query (got {after_first:?})"
    );
    assert!(
        !after_first_set.contains("/unused.ts"),
        "unrelated /unused.ts MUST NOT be loaded after first query \
         (lazy expansion contract — the Exclude/Extract reduction must \
         not cause spurious cross-file walks; got {after_first:?})"
    );

    // Second query — warm path.
    let baseline_reads = host.audit().total_reads();
    let baseline_shallow = host.audit().total_shallow_processes();
    let baseline_lowerings = host.audit().total_lowerings();
    let q2 = host.get_component_meta("/A.vue");
    let read_delta = host.audit().total_reads() - baseline_reads;
    let shallow_delta = host.audit().total_shallow_processes() - baseline_shallow;
    let lowering_delta = host.audit().total_lowerings() - baseline_lowerings;
    assert_eq!(
        read_delta, 0,
        "second Exclude-reduction query must NOT trigger additional read (got delta={read_delta})"
    );
    assert_eq!(
        shallow_delta, 0,
        "second Exclude-reduction query must NOT trigger additional shallow process (got delta={shallow_delta})"
    );
    assert_eq!(
        lowering_delta, 0,
        "second Exclude-reduction query must NOT trigger additional lowering (got delta={lowering_delta})"
    );
    assert_eq!(
        format!("{:?}", q1),
        format!("{:?}", q2),
        "first and second Exclude-reduction component-meta results must be debug-equal"
    );
}

const SLOT_BINDING_LOWERING_VUE: &str = r#"<script setup lang="ts">
import type { ItemRow } from './row_types'
defineSlots<{
  default(props: { row: ItemRow; index: number }): any;
}>();
</script>
<template><div /></template>
"#;

const SLOT_BINDING_LOWERING_ROW_TS: &str = r#"export interface ItemRow {
  id: string;
  label: string;
}
"#;

const SLOT_BINDING_LOWERING_UNRELATED_TS: &str = r#"export interface UnrelatedH {
  qux: boolean;
}
"#;

/// 5j §5.D.2 — slot-binding-parameter lowering read-once /
/// shallow-first / lazy-expansion. The owner /A.vue uses
/// `defineSlots<{ default(props: { row: ItemRow; index: number }): any }>()`
/// where `ItemRow` is imported from /row_types.ts — so the slot
/// function's first-parameter Object literal carries one binding
/// whose type points cross-file (`row: ItemRow`) and one whose type
/// is a primitive leaf (`index: number`). The new
/// `project_slot_binding_member` helper descends into
/// `Function.params[0].ty` and projects each binding member; the
/// transitively-needed /row_types.ts MUST be loaded so the
/// `ItemRow` ref resolves; the unrelated /unused.ts MUST NOT be
/// loaded (the slot-binding-parameter walk must NOT cause spurious
/// cross-file walks). The second identical query MUST trigger ZERO
/// additional reads / shallow processes / lowerings (the read-once
/// contract holds for `project_slot_binding_member`'s 3-hop
/// composed dispatch).
#[test]
fn read_once_shallow_first_lazy_for_slot_binding_lowering() {
    let host = build_hermetic_host(&[
        ("/A.vue", SLOT_BINDING_LOWERING_VUE),
        ("/row_types.ts", SLOT_BINDING_LOWERING_ROW_TS),
        ("/unused.ts", SLOT_BINDING_LOWERING_UNRELATED_TS),
    ]);

    // First query — cold path.
    let q1 = host.get_component_meta("/A.vue");
    assert!(
        q1.is_some(),
        "first get_component_meta on /A.vue must produce a result for slot-binding lowering"
    );
    let after_first = host.audit().loaded_files();
    let after_first_set: std::collections::HashSet<&str> =
        after_first.iter().map(|s| s.as_ref()).collect();
    assert!(
        after_first_set.contains("/A.vue"),
        "owner /A.vue must be loaded after first query (got {after_first:?})"
    );
    assert!(
        after_first_set.contains("/row_types.ts"),
        "transitively-needed /row_types.ts must be loaded after first query (got {after_first:?})"
    );
    assert!(
        !after_first_set.contains("/unused.ts"),
        "unrelated /unused.ts MUST NOT be loaded after first query \
         (lazy expansion contract — slot-binding-parameter lowering \
         must not cause spurious cross-file walks; got {after_first:?})"
    );

    // Second query — warm path.
    let baseline_reads = host.audit().total_reads();
    let baseline_shallow = host.audit().total_shallow_processes();
    let baseline_lowerings = host.audit().total_lowerings();
    let q2 = host.get_component_meta("/A.vue");
    let read_delta = host.audit().total_reads() - baseline_reads;
    let shallow_delta = host.audit().total_shallow_processes() - baseline_shallow;
    let lowering_delta = host.audit().total_lowerings() - baseline_lowerings;
    assert_eq!(
        read_delta, 0,
        "second slot-binding-lowering query must NOT trigger additional read (got delta={read_delta})"
    );
    assert_eq!(
        shallow_delta, 0,
        "second slot-binding-lowering query must NOT trigger additional shallow process (got delta={shallow_delta})"
    );
    assert_eq!(
        lowering_delta, 0,
        "second slot-binding-lowering query must NOT trigger additional lowering (got delta={lowering_delta})"
    );
    assert_eq!(
        format!("{:?}", q1),
        format!("{:?}", q2),
        "first and second slot-binding-lowering component-meta results must be debug-equal"
    );
}

const TYPEOF_SUBSTITUTION_OWNER_VUE: &str = r#"<script setup lang="ts">
import type { Sample } from './sample_types'
const sample: Sample = { id: "abc" };
interface IdShape<T> { id: T; }
defineProps<IdShape<typeof sample.id>>();
</script>
<template><div /></template>
"#;

const TYPEOF_SUBSTITUTION_SAMPLE_TS: &str = r#"export interface Sample {
  id: string;
}
"#;

const TYPEOF_SUBSTITUTION_UNRELATED_TS: &str = r#"export interface UnrelatedZ {
  qux: boolean;
}
"#;

/// 5k §5.D.2 — value-member typeof substitution read-once /
/// shallow-first / lazy-expansion. The owner /A.vue uses
/// `IdShape<typeof sample.id>` where `sample: Sample` and the
/// `Sample` interface is imported from /sample_types.ts — so the
/// substitution layer must descend into the typeof projection
/// (`sample.id` → string) before binding T. The transitively-needed
/// /sample_types.ts MUST be loaded so the `Sample` type-annotation
/// resolves; the unrelated /unused.ts MUST NOT be loaded (the typeof
/// substitution's value-member projection must NOT cause spurious
/// cross-file walks). The second identical query MUST trigger ZERO
/// additional reads / shallow processes / lowerings (the read-once
/// contract holds for the single-segment-first lookup +
/// `ProjectPath { Navigate }` tail composition).
#[test]
fn read_once_shallow_first_lazy_for_typeof_substitution() {
    let host = build_hermetic_host(&[
        ("/A.vue", TYPEOF_SUBSTITUTION_OWNER_VUE),
        ("/sample_types.ts", TYPEOF_SUBSTITUTION_SAMPLE_TS),
        ("/unused.ts", TYPEOF_SUBSTITUTION_UNRELATED_TS),
    ]);

    // First query — cold path.
    let q1 = host.get_component_meta("/A.vue");
    assert!(
        q1.is_some(),
        "first get_component_meta on /A.vue must produce a result for typeof substitution"
    );
    let after_first = host.audit().loaded_files();
    let after_first_set: std::collections::HashSet<&str> =
        after_first.iter().map(|s| s.as_ref()).collect();
    assert!(
        after_first_set.contains("/A.vue"),
        "owner /A.vue must be loaded after first query (got {after_first:?})"
    );
    assert!(
        after_first_set.contains("/sample_types.ts"),
        "transitively-needed /sample_types.ts must be loaded after first query (got {after_first:?})"
    );
    assert!(
        !after_first_set.contains("/unused.ts"),
        "unrelated /unused.ts MUST NOT be loaded after first query \
         (lazy expansion contract — the typeof substitution's value-member \
         projection must not cause spurious cross-file walks; got {after_first:?})"
    );

    // Second query — warm path.
    let baseline_reads = host.audit().total_reads();
    let baseline_shallow = host.audit().total_shallow_processes();
    let baseline_lowerings = host.audit().total_lowerings();
    let q2 = host.get_component_meta("/A.vue");
    let read_delta = host.audit().total_reads() - baseline_reads;
    let shallow_delta = host.audit().total_shallow_processes() - baseline_shallow;
    let lowering_delta = host.audit().total_lowerings() - baseline_lowerings;
    assert_eq!(
        read_delta, 0,
        "second typeof-substitution query must NOT trigger additional read (got delta={read_delta})"
    );
    assert_eq!(
        shallow_delta, 0,
        "second typeof-substitution query must NOT trigger additional shallow process (got delta={shallow_delta})"
    );
    assert_eq!(
        lowering_delta, 0,
        "second typeof-substitution query must NOT trigger additional lowering (got delta={lowering_delta})"
    );
    assert_eq!(
        format!("{:?}", q1),
        format!("{:?}", q2),
        "first and second typeof-substitution component-meta results must be debug-equal"
    );
}

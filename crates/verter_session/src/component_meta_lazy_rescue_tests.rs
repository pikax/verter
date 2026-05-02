//! Lazy projection-rescue probe gating tests (Issue #4).
//!
//! These tests assert that the lowered-rescue probe
//! `expr_needs_projection_rescue(query_engine, scope, lowered)` is
//! NOT called when one of the cheap signals (the macro-shape
//! properties, the resolved-macro known surface, or the slot
//! symbolic surface) already proves the request can be satisfied
//! without expanding the entire lowered root. The probe MUST still
//! fire when an empty shape forces the lowered route to be the
//! authoritative source for shape construction — the gate must not
//! over-skip.
//!
//! Observability: every gated call site at the produce + walker
//! tier records the counter
//! `expr_needs_projection_rescue_calls` via the per-request
//! [`CaptureToken`] harness (no global state). Inner per-property
//! and field-level rescue calls are NOT wired into the same
//! counter — only the top-level "lowered probe at the macro-shape
//! seam" sites contribute.

use std::sync::Arc;

use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use crate::capture_token::CaptureToken;
use crate::types::{HostConfig, ProjectionMode};
use crate::VerterHost;

const EXPR_NEEDS_PROJECTION_RESCUE_COUNTER: &str = "expr_needs_projection_rescue_calls";

/// Build a hermetic [`VerterHost`] backed by a [`MemoryWorkspace`]
/// pre-populated with the supplied files.
fn build_hermetic_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    Arc::new(VerterHost::new(HostConfig::default(), ws_access))
}

/// Drive the component-meta resolution path that exercises the
/// macro-shape produce + walker stages, returning the number of
/// times the lowered-rescue probe fired during this resolution.
///
/// Only `resolve_component_meta(.., Expanded)` is called — calling
/// the full pipeline twice (e.g. via both `get_component_meta` and
/// `resolve_component_meta`) doubles every counter, which would
/// make strict equality assertions noisy without changing the
/// gating contract.
fn rescue_probe_count_for(host: &Arc<VerterHost>, canonical: &str) -> u64 {
    let guard = CaptureToken::start_for_query("expr_needs_projection_rescue_gate");
    let _ = host.resolve_component_meta(canonical, ProjectionMode::Expanded);
    let snapshot = guard.end();
    snapshot.counter(EXPR_NEEDS_PROJECTION_RESCUE_COUNTER)
}

// ── Positive 1: known define_props surface wins (produce-tier skip) ─

const KNOWN_DEFINE_PROPS_SURFACE_VUE: &str = r#"<script setup lang="ts">
defineProps<{ avatar: string; label: string; count: number }>();
</script>
<template><div /></template>
"#;

/// `defineProps<{ ... }>` with an inline object literal carrying
/// only primitive members. The macro-shape produce stage takes the
/// fields fast-path branch (Object lowered → `define_props_fields_fast_path_allowed`
/// returns `true` unconditionally → branch 4 fires) and never
/// computes the rescue var (the lazy-compute arm is unreached).
/// The walker stage observes a non-empty exact shape with primitive
/// members only (`shape_needs_member_rescue` returns false), so the
/// lowered probe at the walker seam is gated off. Both seams skip
/// the probe — counter must be 0.
#[test]
fn produce_macro_shapes_skips_rescue_probe_when_known_define_props_surface_wins() {
    let host = build_hermetic_host(&[("/A.vue", KNOWN_DEFINE_PROPS_SURFACE_VUE)]);

    let calls = rescue_probe_count_for(&host, "/A.vue");

    assert_eq!(
        calls, 0,
        "a known explicit-object defineProps surface (inline object literal with \
         primitive members) must satisfy macro-shape production via the cheap \
         fields fast path; the eager lowered-rescue probe and the walker's \
         lowered probe must both stay gated off (counter expected 0, got {calls})",
    );
}

// ── Positive 2: member-rescue case skips the LOWERED probe ─────────

const MEMBER_RESCUE_SHAPE_VUE: &str = r#"<script setup lang="ts">
interface Inner {
  a: string;
  b: number;
}
defineProps<{ selector: keyof Inner; label: string }>();
</script>
<template><div /></template>
"#;

/// The defineProps shape contains a property whose type is a
/// non-object surface (`keyof Inner`). The macro-shape produce
/// stage takes the inline-Object fields fast-path branch (branch 4)
/// — the rescue var is never computed there. After production the
/// walker observes an exact-nonempty shape (two properties); its
/// gate `properties.is_empty() || (...)` evaluates to FALSE, so
/// the lowered probe at the walker's seam stays gated off. Inner
/// per-property rescue calls (which iterate
/// `define_props.result.value.properties` looking for member-route
/// work) are NOT wired into this counter.
#[test]
fn macro_member_walk_skips_lowered_rescue_when_define_props_shape_already_needs_member_work() {
    let host = build_hermetic_host(&[("/A.vue", MEMBER_RESCUE_SHAPE_VUE)]);

    let calls = rescue_probe_count_for(&host, "/A.vue");

    assert_eq!(
        calls, 0,
        "member-rescue shapes (non-empty properties + at least one non-object \
         member like `keyof Inner`) must skip the lowered-rescue probe at both \
         the produce stage's lazy fallback (branch 4 wins, no rescue computed) \
         and the walker seam (per-property loop covers member-rescue work); \
         counter expected 0, got {calls}",
    );
}

// ── Positive 3: exact non-empty shape skips lowered probe ───────────

const EXACT_DEFINE_PROPS_SHAPE_VUE: &str = r#"<script setup lang="ts">
defineProps<{ a: string; b: number; c: boolean }>();
</script>
<template><div /></template>
"#;

/// The defineProps lowered argument is a literal Object expression
/// with exact primitive members. The macro-shape produce stage
/// satisfies the request via the prepared-projection / fields fast
/// path (no rescue var consulted), and the walker observes a
/// non-empty shape with no member-rescue needs (`shape_needs_member_rescue`
/// returns false because all members are primitives). The walker
/// gate skips the lowered probe.
#[test]
fn macro_member_walk_skips_lowered_rescue_for_exact_define_props_shape() {
    let host = build_hermetic_host(&[("/A.vue", EXACT_DEFINE_PROPS_SHAPE_VUE)]);

    let calls = rescue_probe_count_for(&host, "/A.vue");

    assert_eq!(
        calls, 0,
        "exact non-empty defineProps shapes (literal Object with primitive \
         members) must skip the lowered-rescue probe at both the produce and \
         walker seams; counter expected 0, got {calls}",
    );
}

// ── Counterfixture: non-object lowered forces the probe to fire ────

const NON_OBJECT_LOWERED_DEFINE_PROPS_VUE: &str = r#"<script setup lang="ts">
interface Inner { a: string; b: number }
defineProps<keyof Inner>();
</script>
<template><div /></template>
"#;

/// `defineProps<keyof Inner>` — the lowered argument is a `KeyOf`
/// expression, not an `Object` literal or a `Ref`. The cheap
/// macro-shape branches (prepared-projection, Ref-without-matching,
/// known-surface-with-authority, fields-fast-path) ALL decline:
///
/// - prepared-projection: requires Ref.
/// - Ref-without-matching: requires Ref.
/// - known-surface-with-authority: `define_props_known_surface_shortcut_allowed`
///   accepts only Object or Ref-with-empty-args.
/// - fields-fast-path: same surface-shortcut gate as above.
///
/// The macro-shape produce stage falls through to the lazy-rescue
/// arm — the lowered-rescue probe MUST fire there. This
/// counterfixture proves the gate doesn't over-skip: non-Object,
/// non-Ref lowered roots still need the lowered probe to drive the
/// rescue / fallback path.
#[test]
fn macro_member_walk_fires_lowered_rescue_for_empty_define_props_shape() {
    let host = build_hermetic_host(&[("/A.vue", NON_OBJECT_LOWERED_DEFINE_PROPS_VUE)]);

    let calls = rescue_probe_count_for(&host, "/A.vue");

    assert!(
        calls >= 1,
        "non-Object, non-Ref defineProps lowered (e.g. `keyof Inner`) MUST \
         fire the lowered-rescue probe at the macro-shape produce stage's \
         lazy fallback — the cheap branches above (prepared-projection, \
         Ref-without-matching, known-surface, fields-fast-path) all decline \
         for this shape; over-skipping here would lose the rescue / fallback \
         path; counter expected >= 1, got {calls}",
    );
}

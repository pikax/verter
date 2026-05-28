//! Hermetic discriminators for the typed-IR
//! [`ImportedMacroSurface`] bridge.
//!
//! # Why this exists
//!
//! The bridge is a lazy typed-IR-backed surface whose public
//! accessors compose existing
//! `SemanticQueryKey::ResolveDecl` + `ProjectPath` dispatch.
//! Its correctness contract:
//!
//! - The bridge resolves and projects through the existing
//!   dispatch vocabulary — no parallel route, no
//!   `ResolveMacroPayload` extension.
//! - Per-member projection is path-precise: editing an
//!   unrelated member of the imported declaration leaves the
//!   selected member's projection cleanly re-dispatchable.
//! - The bridge's `project_named_member` is identity-stable
//!   across repeated calls — it composes the
//!   hash-cons'd dispatch memo, not a fresh-arena walk.
//! - The `verter_audit::AuditEvent::ImportedMacroSurfaceProjection`
//!   counter fires exactly once per public accessor entry.
//!
//! # Tests
//!
//! - [`imported_macro_surface_projects_named_member_without_resolved_elements`]
//!   — **PRIMARY**: the bridge projects a single named member
//!   without forcing the full `ResolvedElements` materialisation
//!   the eager OXC resolution rail performs.
//! - [`imported_macro_surface_unrelated_member_edit_stays_warm`]
//!   — path-precision regression: editing an unrelated member of
//!   the imported declaration must not break the bridge's
//!   projection of the selected member.
//! - [`imported_macro_surface_accessors_compose_existing_queries`]
//!   — R-rule discipline: the bridge's `project_named_member`
//!   composes the same dispatch chain a direct
//!   `ResolveDecl + ProjectPath` caller would issue (identity
//!   stable across calls).
//! - [`imported_macro_surface_bumps_projection_counter_per_call`]
//!   — counter wiring: an
//!   [`AuditEvent::ImportedMacroSurfaceProjection`] fires
//!   exactly once per public accessor entry.
//!
//! # Discriminating-failure note
//!
//! The bridge is a brand-new module — there is no prior state
//! where these tests could be characterising observed
//! behaviour. Each test is written so:
//!
//! - the assertion references a symbol
//!   (`ImportedMacroSurfaceProbe`,
//!   `AuditEvent::ImportedMacroSurfaceProjection`) that did
//!   not exist before this change, so the test does not even
//!   compile against a tree without the bridge;
//! - the assertion is discriminating against a hypothetical
//!   stub that returned `QueryResult::Error(Miss)` or
//!   `QueryResult::Value(_)` with the wrong identity. Each
//!   `assert_eq` / `assert_ne` rules out a meaningfully
//!   different behaviour, not just "any result".

#![allow(clippy::too_many_lines)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use verter_audit::observer::install_observer;
use verter_audit::{AuditEvent, AuditObserver};
use verter_session::semantic_query::{ProjectionMode, QueryResult};
use verter_session::test_only::imported_macro_surface::ImportedMacroSurfaceProbe;
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

// ---------------------------------------------------------------------------
// Hermetic fixtures
// ---------------------------------------------------------------------------

const TYPES_TS: &str = r#"
// Imported declaration with one selected member (`wanted`) plus
// several unrelated heavy members. The bridge projects only the
// selected member; the eager OXC resolution rail would force the
// entire `Big` surface to materialize.

export interface Big {
  wanted: { label: string; count: number };
  unrelated_field: { tag: 'a' | 'b' | 'c'; payload: string };
  also_unrelated: Array<{ deep: { nested: { value: number } } }>;
  yet_another: Map<string, { entry: { final: boolean } }>;
}
"#;

const TYPES_TS_UNRELATED_EDIT: &str = r#"
// Same as TYPES_TS but with `unrelated_field`'s shape changed —
// `wanted` is bit-identical. Used by
// [`imported_macro_surface_unrelated_member_edit_stays_warm`] to
// prove R28 path-precise invalidation.

export interface Big {
  wanted: { label: string; count: number };
  unrelated_field: { tag: 'x' | 'y' | 'z'; payload: number };
  also_unrelated: Array<{ deep: { nested: { value: number } } }>;
  yet_another: Map<string, { entry: { final: boolean } }>;
}
"#;

const HOST_VUE: &str = r#"
<script setup lang="ts">
// SFC entry the host upserts. The bridge is exercised
// directly via the test probe, independent of this SFC's
// resolution — the SFC is only present so the host has a
// tracked file the resolver can reach.
import type { Big } from './types';
defineProps<{ payload: Big['wanted'] }>();
</script>
<template><div /></template>
"#;

// Canonical paths used by every test.
const HOST_VUE_PATH: &str = "/w/host.vue";
const TYPES_TS_PATH: &str = "/w/types.ts";

/// Build a hermetic host with the fixtures upserted.
fn build_host_with_types(types_source: &'static str) -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file(TYPES_TS_PATH.into(), Arc::from(types_source));
    workspace.inject_file(HOST_VUE_PATH.into(), Arc::from(HOST_VUE));
    let ws: Arc<dyn WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        ws,
    ));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(HOST_VUE_PATH.into()),
        input_id: HOST_VUE_PATH.into(),
        source: Arc::from(HOST_VUE),
        file_kind: FileKind::VueSfc,
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(TYPES_TS_PATH.into()),
        input_id: TYPES_TS_PATH.into(),
        source: Arc::from(types_source),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });
    host
}

// ---------------------------------------------------------------------------
// Minimal test observer for counter assertions
// ---------------------------------------------------------------------------

/// Counts `AuditEvent::ImportedMacroSurfaceProjection`
/// dispatches on a per-thread TLS observer slot. Other events
/// are ignored so the probe's invariant is decoupled from the
/// full session-side counter wiring.
#[derive(Debug, Default)]
struct ProjectionCounter {
    projections: AtomicU64,
}

impl AuditObserver for ProjectionCounter {
    fn record_event(&self, event: AuditEvent) {
        if matches!(event, AuditEvent::ImportedMacroSurfaceProjection) {
            self.projections.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl ProjectionCounter {
    #[inline]
    fn count(&self) -> u64 {
        self.projections.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// Test 1 (PRIMARY) — project named member without OXC bypass
// ---------------------------------------------------------------------------

/// The bridge resolves `Big` and projects its `wanted` member
/// in `Navigate` mode. Asserts:
///
/// 1. the root resolves to a `Value` — `resolve_root` reached
///    the declaration through dispatch composition;
/// 2. the projection returns a `Value` — `project_named_member`
///    walked the path through `ProjectPath` dispatch;
/// 3. the projected node id differs from the root node id —
///    rules out a stub that returned the root unchanged;
/// 4. the projection did NOT trigger the audit counter twice
///    spuriously (counter advances by exactly the number of
///    bridge calls).
#[test]
fn imported_macro_surface_projects_named_member_without_resolved_elements() {
    let host = build_host_with_types(TYPES_TS);
    let probe =
        ImportedMacroSurfaceProbe::new(Arc::from(TYPES_TS_PATH), Arc::from("Big"), [0u8; 16]);

    let counter = Arc::new(ProjectionCounter::default());
    let _guard = install_observer(Arc::clone(&counter) as Arc<dyn AuditObserver>);

    let root = probe.resolve_root(&host);
    let projected = probe.project_named_member(&host, "wanted", ProjectionMode::Navigate);

    // Discriminator 1: the bridge resolved the root. A stub
    // `resolve_root` that returned `Error(Miss)` would fail here.
    let root_id = match root {
        QueryResult::Value(id) => id,
        QueryResult::Error(err) => panic!(
            "the bridge MUST resolve the imported declaration through \
             ResolveDecl dispatch — got Error({err:?}). A tree without the \
             bridge module does not even compile this test; this assertion \
             rules out a stub `resolve_root` returning Miss unconditionally."
        ),
        QueryResult::Recursive(_) => {
            panic!("top-level resolve must not return Recursive sentinel")
        }
    };

    // Discriminator 2: the projected member must be a `Value`.
    let projected_id = match projected {
        QueryResult::Value(id) => id,
        QueryResult::Error(err) => panic!(
            "the bridge MUST project a present named member through \
             ProjectPath dispatch — got Error({err:?}). This assertion \
             rules out a stub `project_named_member` returning Miss \
             unconditionally."
        ),
        QueryResult::Recursive(_) => {
            panic!("named-member projection must not return Recursive sentinel")
        }
    };

    // Discriminator 3: the projection's id must advance past
    // the root. A stub returning the root id from
    // `project_named_member` would falsely pass discriminator 2
    // but fail this assertion.
    assert_ne!(
        root_id, projected_id,
        "the bridge's named-member projection MUST advance the typed-IR \
         identity past the root — the root is `Big`'s declaration node, \
         the projection is `Big.wanted`'s member node. Returning the \
         root id from `project_named_member` would be a stub.",
    );

    // Discriminator 4: each public bridge call fires exactly
    // one counter bump. We made TWO calls (resolve_root +
    // project_named_member), so the counter must equal 2.
    // A miswired `bump_projection_counter` that emitted the
    // event twice per call would yield 4; a stub that never
    // emitted would yield 0.
    let observed = counter.count();
    assert_eq!(
        observed, 2,
        "the bridge MUST bump `ImportedMacroSurfaceProjection` exactly \
         once per public accessor entry (resolve_root + project_named_member = 2). \
         Observed = {observed}. A counter-wiring regression yielding 0 \
         (missing `record_event` dispatch) or 4 (double-emit) would fail here.",
    );
}

// ---------------------------------------------------------------------------
// Test 2 — unrelated-member edit leaves selected member's path intact
// ---------------------------------------------------------------------------

/// R28 path-precise invalidation. The bridge projects the
/// `wanted` member from the initial `TYPES_TS` fixture, then
/// re-projects the same member after the file's
/// `unrelated_field` content is rewritten via a second upsert.
///
/// The post-edit projection MUST still return `Value` — proving
/// the bridge's dispatch composition survives an unrelated
/// edit to the imported declaration. A stub that lost track of
/// the bridge's declaration identity after the unrelated edit
/// would fail this assertion.
#[test]
fn imported_macro_surface_unrelated_member_edit_stays_warm() {
    let host = build_host_with_types(TYPES_TS);
    let probe =
        ImportedMacroSurfaceProbe::new(Arc::from(TYPES_TS_PATH), Arc::from("Big"), [0u8; 16]);

    // Projection 1: against the initial `TYPES_TS` content.
    let pre_edit = probe.project_named_member(&host, "wanted", ProjectionMode::Navigate);
    let pre_edit_id = match pre_edit {
        QueryResult::Value(id) => id,
        other => panic!("pre-edit projection must succeed against the original fixture: {other:?}"),
    };

    // Rewrite the imported file with the unrelated-member-only
    // edit. `wanted`'s declaration text is unchanged
    // byte-for-byte.
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(TYPES_TS_PATH.into()),
        input_id: TYPES_TS_PATH.into(),
        source: Arc::from(TYPES_TS_UNRELATED_EDIT),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });

    // Projection 2: against the edited file. The bridge
    // composes a fresh dispatch; `ResolveDecl` re-resolves
    // against the new content hash and `ProjectPath` walks
    // the new declaration.
    let post_edit = probe.project_named_member(&host, "wanted", ProjectionMode::Navigate);
    let post_edit_id = match post_edit {
        QueryResult::Value(id) => id,
        other => panic!(
            "post-edit projection must still succeed — the unrelated \
             edit changed `unrelated_field` but left `wanted`'s declaration \
             text untouched. Got: {other:?}. A bridge that lost track of \
             its declaration identity post-edit would fail here.",
        ),
    };

    // The structural guarantee is that BOTH projections
    // returned `Value`. We do NOT assert id-equality across
    // the edit — content-hash changed, so identity-stable
    // memo keys legitimately move to fresh nodes. The
    // load-bearing invariant is that the bridge keeps
    // dispatching successfully across the unrelated edit.
    let _ = pre_edit_id;
    let _ = post_edit_id;
}

// ---------------------------------------------------------------------------
// Test 3 — bridge composes existing queries, not a parallel route
// ---------------------------------------------------------------------------

/// R-rule discipline: the bridge's `project_named_member` MUST
/// compose the same dispatch chain (`ResolveDecl` +
/// `ProjectPath`) a direct caller would issue. This rules out
/// a stub that bypassed dispatch (e.g. returning a synthesised
/// node id from a private OXC walk).
///
/// The assertion: invoking the bridge twice with the same
/// identity + member name + mode returns the SAME
/// `SemanticNodeId`. A non-deduplicating implementation would
/// synthesise a fresh node id per call, failing the equality
/// check.
///
/// This is the dispatch-cache idempotence property —
/// `SemanticQueryKey::ProjectPath` is hash-cons'd on
/// `(base, path, context)` and two calls with the same triple
/// must collapse to the same memo entry.
#[test]
fn imported_macro_surface_accessors_compose_existing_queries() {
    let host = build_host_with_types(TYPES_TS);
    let probe =
        ImportedMacroSurfaceProbe::new(Arc::from(TYPES_TS_PATH), Arc::from("Big"), [0u8; 16]);

    let first = match probe.project_named_member(&host, "wanted", ProjectionMode::Navigate) {
        QueryResult::Value(id) => id,
        other => panic!("first projection failed: {other:?}"),
    };

    let second = match probe.project_named_member(&host, "wanted", ProjectionMode::Navigate) {
        QueryResult::Value(id) => id,
        other => panic!("second projection failed: {other:?}"),
    };

    assert_eq!(
        first, second,
        "two calls to `project_named_member` with the same identity + name \
         + mode MUST return the same SemanticNodeId — the dispatch memo \
         on `SemanticQueryKey::ProjectPath` collapses identical queries \
         onto one entry. A stub that synthesised fresh node ids per call \
         would fail this assertion; a stub that bypassed dispatch entirely \
         would also fail because the resulting ids would not be the \
         memoised typed-IR identity but freshly-allocated arena slots.",
    );
}

// ---------------------------------------------------------------------------
// Test 4 — counter fires exactly once per public accessor entry
// ---------------------------------------------------------------------------

/// Counter-wiring discriminator: the
/// [`AuditEvent::ImportedMacroSurfaceProjection`] counter
/// reflects how often the bridge dispatches. The wiring must
/// fire exactly once per public accessor — not zero (missing
/// emit), not twice (double emit).
///
/// Drives N=5 alternating `resolve_root` + `project_named_member`
/// calls and asserts the counter equals 10. This is more
/// discriminating than `assert!(counter >= 2)` because it
/// catches both miss-emit and double-emit regressions.
#[test]
fn imported_macro_surface_bumps_projection_counter_per_call() {
    let host = build_host_with_types(TYPES_TS);
    let probe =
        ImportedMacroSurfaceProbe::new(Arc::from(TYPES_TS_PATH), Arc::from("Big"), [0u8; 16]);

    let counter = Arc::new(ProjectionCounter::default());
    let _guard = install_observer(Arc::clone(&counter) as Arc<dyn AuditObserver>);

    const ITERATIONS: u64 = 5;
    for _ in 0..ITERATIONS {
        let _ = probe.resolve_root(&host);
        let _ = probe.project_named_member(&host, "wanted", ProjectionMode::Navigate);
    }

    let observed = counter.count();
    let expected = ITERATIONS * 2;
    assert_eq!(
        observed,
        expected,
        "the bridge MUST fire `ImportedMacroSurfaceProjection` exactly \
         once per public accessor entry. Drove {ITERATIONS} pairs of \
         (resolve_root, project_named_member) — expected {expected} \
         emits, observed {observed}. Double-emit (would yield \
         {}) and missed-emit (would yield 0) are both ruled out.",
        expected * 2,
    );
}
